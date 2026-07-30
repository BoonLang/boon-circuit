use crate::report_v2::{
    CheckOutcome, ReportStatus, SourceIdentity, ToolResult, current_identity, sha256_bytes,
    sha256_file, unix_time_ms,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const FORMAT_VERSION: u16 = 1;
const PROTOCOL: &str = "boon-phase0-evidence-v1";
const DEFAULT_REPORT: &str = "target/reports/phase0-v1/evidence.json";
const VERSIONS_MANIFEST: &str = "docs/architecture/phase0/versions.toml";
const DELETION_MANIFEST: &str = "docs/architecture/phase0/deletion_ledger.toml";
const DELETION_MANIFEST_SHA256: &str =
    "c8a5997c5678e8dfbe17010d42408225920ee03b0aea9337b25df3ddcabab89d";
const CONTAINER_MANIFEST: &str = "docs/architecture/phase0/container_inventory.toml";
#[cfg(test)]
const CONTAINER_OCCURRENCE_LEDGER: &str = "docs/architecture/phase0/container_occurrences.tsv";
const FIXTURE_MANIFEST: &str = "docs/architecture/phase0/fixtures.toml";
const DATASET_MANIFEST: &str = "docs/architecture/phase0/dataset_fixtures.toml";
const DOCUMENT_MANIFEST: &str = "docs/architecture/phase0/documents.toml";
const BASELINE_MANIFEST: &str = "docs/architecture/phase0/baselines.toml";
const PACKED_BUDGET_MANIFEST: &str = "budgets/packed-data.toml";
const CONTAINER_OCCURRENCE_LEDGER_PROTOCOL: &str = "boon-phase0-container-occurrences-v1";
const CLEAN_BASELINE_HEAD: &str = "4a820727d339038826a9d589c207ef5f973dad83";
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_OCCURRENCE_LEDGER_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_PROBE_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_MAX_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_CHECK_DETAIL_BYTES: usize = 900;
const MAX_OPEN_ITEMS: usize = 32;
const CONTAINER_CONSTRUCT_IDS: &[&str] = &["BTreeMap", "BTreeSet", "HashMap", "HashSet"];
const CONTAINER_CATEGORY_IDS: &[&str] = &[
    "dense-id-table",
    "membership",
    "ordered-index",
    "name-lookup",
    "canonical-boundary",
    "test-oracle",
    "unrelated-host-code",
];
const CONTAINER_REASON_IDS: &[&str] = &[
    "test-only-scope",
    "non-code-reference",
    "typed-list-order-kernel",
    "canonical-boundary-import",
    "membership-container-import",
    "engine-container-import",
    "host-container-import",
    "canonical-boundary-structure",
    "set-membership-structure",
    "ordered-engine-index",
    "canonicalization-context",
    "name-or-path-key-context",
    "engine-id-table-context",
    "parser-name-table-context",
    "host-or-tooling-container",
];

pub fn run(workspace: &Path, output_override: Option<PathBuf>) -> ToolResult<ReportStatus> {
    let start_source = current_identity(workspace)?.source;
    let output = output_override.unwrap_or_else(|| workspace.join(DEFAULT_REPORT));
    let mut checks = Vec::new();
    let mut manifests = Vec::new();
    let mut artifact_summary = ArtifactSummary::default();
    let mut version_summary = VersionSummary::default();
    let mut legacy_summary = LegacySummary::default();
    let mut container_summary = ContainerSummary::default();
    let mut packed_site_summary = PackedSiteSummary::default();
    let mut fixture_summary = FixtureSummary::default();
    let mut dataset_summary = DatasetSummary::default();
    let mut document_summary = DocumentSummary::default();
    let mut baseline_summary = BaselineSummary::default();
    let mut packed_summary = PackedSummary::default();
    let mut report_inventory = ReportInventory::default();
    let mut report_policy = None;

    let workspace_files = match workspace_files(workspace) {
        Ok(files) => {
            push_check(
                &mut checks,
                "workspace-file-inventory",
                Ok(format!(
                    "{} tracked or untracked workspace files",
                    files.len()
                )),
            );
            files
        }
        Err(error) => {
            push_check(&mut checks, "workspace-file-inventory", Err(error));
            Vec::new()
        }
    };

    match load_toml::<VersionsManifest>(workspace, VERSIONS_MANIFEST) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_versions(workspace, &manifest) {
                Ok((artifacts, versions)) => {
                    artifact_summary = artifacts;
                    version_summary = versions;
                    report_policy = Some(manifest.report_identity.clone());
                    push_check(
                        &mut checks,
                        "versions-manifest",
                        Ok(format!(
                            "{} artifact states and {} version axes validated",
                            manifest.artifacts.len(),
                            manifest.version_axes.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "versions-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "versions-manifest", Err(error)),
    }

    match load_toml::<DeletionManifest>(workspace, DELETION_MANIFEST) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_deletions(workspace, &workspace_files, &manifest) {
                Ok(summary) => {
                    legacy_summary = summary;
                    push_check(
                        &mut checks,
                        "deletion-ledger",
                        Ok(format!(
                            "{} owned families, {} scans, {} observed occurrences",
                            legacy_summary.entries,
                            legacy_summary.scans,
                            legacy_summary.observed_occurrences
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "deletion-ledger", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "deletion-ledger", Err(error)),
    }

    match load_toml::<ContainerInventoryManifest>(workspace, CONTAINER_MANIFEST) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_container_inventory(workspace, &workspace_files, &manifest) {
                Ok(summary) => {
                    manifests.push(ManifestEvidence {
                        path: summary.occurrence_ledger_path.clone(),
                        sha256: summary.occurrence_ledger_sha256.clone(),
                        bytes: summary.occurrence_ledger_bytes,
                    });
                    container_summary = summary;
                    push_check(
                        &mut checks,
                        "container-inventory",
                        Ok(format!(
                            "{} exact occurrences across {} Rust files are classified one-to-one; {} files contain multiple categories",
                            container_summary.occurrence_rows,
                            container_summary.rust_files.len(),
                            container_summary.mixed_category_files
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "container-inventory", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "container-inventory", Err(error)),
    }

    match crate::packed_site_inventory::validate(workspace) {
        Ok(summary) => {
            let evidence = [
                file_evidence(
                    workspace,
                    crate::packed_site_inventory::MANIFEST_PATH,
                    MAX_MANIFEST_BYTES,
                ),
                file_evidence(
                    workspace,
                    crate::packed_site_inventory::LEDGER_PATH,
                    32 * 1024 * 1024,
                ),
            ]
            .into_iter()
            .collect::<Result<Vec<_>, _>>();
            match evidence {
                Ok(evidence) => {
                    manifests.extend(evidence);
                    packed_site_summary = PackedSiteSummary::from(summary);
                    push_check(
                        &mut checks,
                        "packed-site-inventory",
                        Ok(format!(
                            "{} exact conservative candidate sites across {} source files and {} categories validated",
                            packed_site_summary.rows,
                            packed_site_summary.occurrence_files,
                            packed_site_summary.categories.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "packed-site-inventory", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "packed-site-inventory", Err(error)),
    }

    match load_toml::<boon_phase0_baseline::evidence::FixtureEvidenceManifestV2>(
        workspace,
        FIXTURE_MANIFEST,
    ) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_fixtures(workspace, &manifest) {
                Ok(summary) => {
                    fixture_summary = summary;
                    push_check(
                        &mut checks,
                        "fixture-manifest",
                        Ok(format!(
                            "{} executable fixture requirements; {} future targets remain honestly unimplemented",
                            fixture_summary.existing.len(),
                            fixture_summary.not_yet_implemented.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "fixture-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "fixture-manifest", Err(error)),
    }

    match load_toml::<boon_phase0_baseline::dataset::DatasetFixtureManifestV1>(
        workspace,
        DATASET_MANIFEST,
    ) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_datasets(workspace, &manifest, &fixture_summary) {
                Ok(summary) => {
                    dataset_summary = summary;
                    push_check(
                        &mut checks,
                        "dataset-fixture-manifest",
                        Ok(format!(
                            "{} canonically identified datasets across {} categories and {} source units",
                            dataset_summary.fixtures.len(),
                            dataset_summary.categories.len(),
                            dataset_summary.total_units
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "dataset-fixture-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "dataset-fixture-manifest", Err(error)),
    }

    match load_toml::<DocumentManifest>(workspace, DOCUMENT_MANIFEST) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_documents(workspace, &workspace_files, &manifest) {
                Ok(summary) => {
                    document_summary = summary;
                    push_check(
                        &mut checks,
                        "document-manifest",
                        Ok(format!(
                            "{} active plan/architecture documents classified",
                            manifest.documents.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "document-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "document-manifest", Err(error)),
    }

    match load_toml::<boon_phase0_baseline::evidence::BaselineEvidenceManifestV2>(
        workspace,
        BASELINE_MANIFEST,
    ) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_baselines(workspace, &manifest, &dataset_summary) {
                Ok(summary) => {
                    baseline_summary = summary;
                    push_check(
                        &mut checks,
                        "baseline-manifest",
                        Ok(format!(
                            "clean compiler baseline has {} passing tests; all {} required areas have structured evidence",
                            baseline_summary.compiler_tests,
                            baseline_summary.areas.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "baseline-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "baseline-manifest", Err(error)),
    }

    match load_toml::<PackedBudgetManifest>(workspace, PACKED_BUDGET_MANIFEST) {
        Ok((manifest, evidence)) => {
            manifests.push(evidence);
            match validate_packed_budget(workspace, &manifest) {
                Ok(summary) => {
                    packed_summary = summary;
                    push_check(
                        &mut checks,
                        "packed-budget-manifest",
                        Ok(format!(
                            "{} current-runtime fixtures, {} measured metrics, and {} explicitly unavailable metrics validated",
                            packed_summary.current_runtime_baselines.len(),
                            packed_summary.measured_metrics.len(),
                            packed_summary.unavailable_metrics.len()
                        )),
                    );
                }
                Err(error) => push_check(&mut checks, "packed-budget-manifest", Err(error)),
            }
        }
        Err(error) => push_check(&mut checks, "packed-budget-manifest", Err(error)),
    }

    if let Some(policy) = report_policy.as_ref() {
        match inspect_reports(workspace, &start_source, policy) {
            Ok(inventory) => {
                report_inventory = inventory;
                push_check(
                    &mut checks,
                    "stale-report-policy",
                    Ok(format!(
                        "{} current clean, {} stale, {} dirty, {} unidentifiable reports",
                        report_inventory.current_clean,
                        report_inventory.stale,
                        report_inventory.dirty_rejected,
                        report_inventory.unidentifiable
                    )),
                );
            }
            Err(error) => push_check(&mut checks, "stale-report-policy", Err(error)),
        }
    } else {
        push_check(
            &mut checks,
            "stale-report-policy",
            Err("versions manifest did not yield a valid report identity policy".to_owned()),
        );
    }

    let end_source = current_identity(workspace)?.source;
    if start_source == end_source {
        push_check(
            &mut checks,
            "stable-source-during-verification",
            Ok("source identity remained stable during collection".to_owned()),
        );
    } else {
        push_check(
            &mut checks,
            "stable-source-during-verification",
            Err("source identity changed while Phase 0 evidence was collected".to_owned()),
        );
    }

    let validation_status = if checks
        .iter()
        .all(|check| check.outcome == CheckOutcome::Pass)
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    let mut open_items = completion_open_items(
        &end_source,
        &container_summary,
        &fixture_summary,
        &dataset_summary,
        &baseline_summary,
    );
    if validation_status == ReportStatus::Fail {
        open_items.insert(
            0,
            "Phase 0 evidence manifests or worktree baselines failed validation".to_owned(),
        );
    }
    open_items.truncate(MAX_OPEN_ITEMS);
    let completion = if validation_status == ReportStatus::Pass && open_items.is_empty() {
        CompletionState::Complete
    } else {
        CompletionState::Incomplete
    };
    let status =
        if validation_status == ReportStatus::Pass && completion == CompletionState::Complete {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        };
    let report = Phase0Report {
        format: FORMAT_VERSION,
        kind: "phase0-evidence",
        protocol: PROTOCOL,
        generated_unix_ms: unix_time_ms(),
        source: end_source,
        status,
        validation_status,
        phase0_completion: completion,
        manifests,
        checks,
        artifacts: artifact_summary,
        versions: version_summary,
        legacy: legacy_summary,
        containers: container_summary,
        packed_sites: packed_site_summary,
        fixtures: fixture_summary,
        datasets: dataset_summary,
        documents: document_summary,
        baselines: baseline_summary,
        packed_budget: packed_summary,
        prior_reports: report_inventory,
        completion_open_items: open_items,
    };
    let max_output_bytes = report_policy
        .as_ref()
        .map(|policy| policy.max_output_bytes)
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
    write_report(&output, &report, max_output_bytes)?;
    println!(
        "wrote {} ({}, Phase 0 {})",
        output.display(),
        status_name(status),
        completion_name(completion)
    );
    Ok(status)
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Phase0Report {
    format: u16,
    kind: &'static str,
    protocol: &'static str,
    generated_unix_ms: u64,
    source: SourceIdentity,
    status: ReportStatus,
    validation_status: ReportStatus,
    phase0_completion: CompletionState,
    manifests: Vec<ManifestEvidence>,
    checks: Vec<Phase0Check>,
    artifacts: ArtifactSummary,
    versions: VersionSummary,
    legacy: LegacySummary,
    containers: ContainerSummary,
    packed_sites: PackedSiteSummary,
    fixtures: FixtureSummary,
    datasets: DatasetSummary,
    documents: DocumentSummary,
    baselines: BaselineSummary,
    packed_budget: PackedSummary,
    prior_reports: ReportInventory,
    completion_open_items: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CompletionState {
    Complete,
    Incomplete,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestEvidence {
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Phase0Check {
    id: String,
    outcome: CheckOutcome,
    detail: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactSummary {
    present: Vec<String>,
    legacy_present: Vec<String>,
    absent: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct VersionSummary {
    current: Vec<String>,
    absent: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacySummary {
    entries: usize,
    scans: usize,
    observed_occurrences: usize,
    present_families: Vec<String>,
    rejection_only_families: Vec<String>,
    policy_families: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct ContainerSummary {
    file_set_sha256: String,
    rust_files: Vec<String>,
    constructs: Vec<ContainerConstructSummary>,
    categories: Vec<ContainerCategorySummary>,
    occurrence_ledger_path: String,
    occurrence_ledger_sha256: String,
    occurrence_ledger_bytes: u64,
    occurrence_rows: usize,
    mixed_category_files: usize,
    within_file_classification: String,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ContainerConstructSummary {
    id: String,
    occurrences: usize,
    files: usize,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ContainerCategorySummary {
    order: u16,
    id: String,
    files: usize,
    btree_map: usize,
    btree_set: usize,
    hash_map: usize,
    hash_set: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct PackedSiteSummary {
    rows: usize,
    occurrence_files: usize,
    scanned_files: usize,
    occurrence_ledger_sha256: String,
    file_set_sha256: String,
    probe_set_sha256: String,
    categories: BTreeMap<String, PackedSiteCategorySummary>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PackedSiteCategorySummary {
    hot: usize,
    cold: usize,
    boundary: usize,
}

impl From<crate::packed_site_inventory::InventorySummary> for PackedSiteSummary {
    fn from(summary: crate::packed_site_inventory::InventorySummary) -> Self {
        Self {
            rows: summary.rows,
            occurrence_files: summary.occurrence_files,
            scanned_files: summary.scanned_files,
            occurrence_ledger_sha256: summary.ledger_sha256,
            file_set_sha256: summary.file_set_sha256,
            probe_set_sha256: summary.probe_set_sha256,
            categories: summary
                .category_counts
                .into_iter()
                .map(|(id, [hot, cold, boundary])| {
                    (
                        id,
                        PackedSiteCategorySummary {
                            hot,
                            cold,
                            boundary,
                        },
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct FixtureSummary {
    existing: Vec<String>,
    current_supported: Vec<String>,
    current_analogues: Vec<String>,
    not_yet_implemented: Vec<String>,
    compile_execute_current: Vec<String>,
    compile_reject_future: Vec<String>,
    current_execution_plus_future_rejection: Vec<String>,
    headless_current_analogues: Vec<String>,
    current_execution_plus_measured_baseline: Vec<String>,
    source_paths: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct DatasetSummary {
    schema: String,
    identity: String,
    categories: Vec<String>,
    fixtures: Vec<DatasetFixtureSummary>,
    total_units: usize,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct DatasetFixtureSummary {
    id: String,
    category: String,
    entrypoint: String,
    source_bundle_digest_v1: String,
    unit_count: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct DocumentSummary {
    current: Vec<String>,
    target_contracts: Vec<String>,
    rewrite_required: Vec<String>,
    delete_after_replacement: Vec<String>,
    historical: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct BaselineSummary {
    compiler_tests: u64,
    compiler_crates: Vec<String>,
    historical_stale_reports: Vec<String>,
    areas: Vec<String>,
    measured_areas: Vec<String>,
    partial_areas: Vec<String>,
    measured_evidence: Vec<String>,
    unavailable_evidence: Vec<String>,
    stale_evidence: Vec<String>,
    not_applicable_evidence: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct PackedSummary {
    schema_status: String,
    report_path: String,
    current_runtime_baselines: Vec<String>,
    measured_metrics: Vec<String>,
    unavailable_metrics: Vec<String>,
    not_applicable_metrics: Vec<String>,
    budgeted_metrics: Vec<String>,
    action_ratchets: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct ReportInventory {
    inspected: usize,
    current_clean: usize,
    stale: usize,
    dirty_rejected: usize,
    unidentifiable: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionsManifest {
    format_version: u16,
    baseline_source_head: String,
    baseline_scope: String,
    claim: String,
    report_identity: ReportIdentityPolicy,
    artifacts: Vec<ArtifactEntry>,
    version_axes: Vec<VersionAxis>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportIdentityPolicy {
    protocol: String,
    source_head: String,
    workspace_digest: String,
    dirty_final_evidence: String,
    stale_result: String,
    inspect_directory: String,
    max_report_files: usize,
    max_report_bytes: u64,
    max_output_bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactEntry {
    id: String,
    owner_phase: String,
    owner_plan: String,
    status: ArtifactStatus,
    probes: Vec<ArtifactProbe>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ArtifactStatus {
    Present,
    LegacyPresent,
    Absent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactProbe {
    kind: ProbeKind,
    path: String,
    #[serde(default)]
    needle: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ProbeKind {
    PathExists,
    PathAbsent,
    TextContains,
    TextAbsent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionAxis {
    id: String,
    #[serde(default)]
    current_version: Option<String>,
    #[serde(default)]
    absent: Option<bool>,
    owner_phase: String,
    owner_plan: String,
    target: String,
    replacement: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionManifest {
    format_version: u16,
    baseline_source_head: String,
    baseline_scope: String,
    entries: Vec<DeletionEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionEntry {
    id: String,
    owner_phase: String,
    owner_plan: String,
    status: LegacyStatus,
    target: LegacyTarget,
    measurement: LegacyMeasurement,
    #[serde(default)]
    scans: Vec<OccurrenceScan>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LegacyStatus {
    Present,
    RejectionOnly,
    Policy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LegacyTarget {
    Zero,
    ClassifiedRetainedOnly,
    NegativeFixturesOnly,
    ExactIdentityOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LegacyMeasurement {
    Occurrence,
    ReportIdentity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum OccurrenceScanMode {
    #[default]
    Text,
    BoonCallArgument,
    JsonSourceBundleText,
    JsonSourceBundleBoonCallArgument,
    RustToken,
    RustCodeToken,
}

#[derive(Clone, Copy)]
struct DeletionFamilyContract {
    id: &'static str,
    owner_phase: &'static str,
    owner_plan: &'static str,
    status: LegacyStatus,
    target: LegacyTarget,
    measurement: LegacyMeasurement,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OccurrenceScan {
    roots: Vec<String>,
    extensions: Vec<String>,
    #[serde(default)]
    mode: OccurrenceScanMode,
    needle: String,
    #[serde(default)]
    call_functions: Vec<String>,
    expected_occurrences: usize,
    expected_files: usize,
    #[serde(default)]
    expected_path_occurrences: HashMap<String, usize>,
    #[serde(default)]
    line_contains_any: Vec<String>,
    #[serde(default)]
    excluded_paths: Vec<String>,
    #[serde(default)]
    allowed_paths: Vec<String>,
}

struct ScanObservation {
    occurrences: usize,
    matched_paths: Vec<String>,
    path_occurrences: HashMap<String, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerInventoryManifest {
    format_version: u16,
    baseline_source_head: String,
    baseline_scope: String,
    scan_root: String,
    extension: String,
    word_boundary: String,
    lexical_scope: String,
    test_scope_policy: String,
    non_code_policy: String,
    expected_file_count: usize,
    expected_file_set_sha256: String,
    occurrence_ledger_path: String,
    occurrence_ledger_protocol: String,
    occurrence_ledger_generator: String,
    expected_occurrence_ledger_sha256: String,
    expected_occurrence_rows: usize,
    within_file_classification: WithinFileClassification,
    within_file_owner_phase: String,
    within_file_owner_action: String,
    constructs: Vec<ContainerConstruct>,
    categories: Vec<ContainerCategory>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerConstruct {
    id: String,
    expected_occurrences: usize,
    expected_files: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerCategory {
    order: u16,
    id: String,
    owner_phase: String,
    owner_plan: String,
    expected_files: usize,
    expected_btree_map: usize,
    expected_btree_set: usize,
    expected_hash_map: usize,
    expected_hash_set: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum WithinFileClassification {
    Missing,
    Complete,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ContainerOccurrenceIdentity {
    path: String,
    line: usize,
    byte_column: usize,
    ordinal: usize,
    construct: String,
    context_sha256: String,
}

#[derive(Clone, Debug)]
struct ClassifiedContainerOccurrence {
    identity: ContainerOccurrenceIdentity,
    category: String,
    reason: String,
}

struct ContainerOccurrenceSemanticSource {
    lines: Vec<String>,
    line_starts: Vec<usize>,
    test_only_intervals: Vec<(usize, usize)>,
    structural: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentManifest {
    format_version: u16,
    documents: Vec<DocumentEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentEntry {
    id: String,
    path: String,
    classification: DocumentClassification,
    owner_phase: String,
    #[serde(default)]
    status_notice: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum DocumentClassification {
    SequenceContract,
    ExecutionContract,
    TargetContract,
    CurrentBehavior,
    ActiveImplementationContract,
    RewriteRequired,
    DeleteAfterReplacement,
    HistoricalDecision,
    HistoricalContext,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedBudgetManifest {
    format_version: u16,
    schema_status: MeasurementStatus,
    owner_phase: String,
    owner_plan: String,
    baseline_source_head: String,
    baseline_scope: String,
    current_runtime_report: String,
    fixture_manifest: String,
    protocol: PackedProtocol,
    baselines: Vec<PackedBaseline>,
    metrics: Vec<PackedMetric>,
    action_ratchets: Vec<PackedActionRatchet>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum MeasurementStatus {
    Unmeasured,
    Measured,
    CurrentRuntimeMeasured,
    Unavailable,
    NotApplicable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedProtocol {
    name: String,
    target_profile: String,
    build_profile: String,
    allocator_scope: String,
    allocator_instrumentation_status: MeasurementStatus,
    warmup_status: MeasurementStatus,
    measured_turn_interval_status: MeasurementStatus,
    #[serde(default)]
    warmup_turns: Option<u64>,
    #[serde(default)]
    measured_turns: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedBaseline {
    id: String,
    status: MeasurementStatus,
    owner: String,
    owner_action: String,
    #[serde(default)]
    measured_report: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedMetric {
    id: String,
    unit: String,
    status: MeasurementStatus,
    owner: String,
    owner_action: String,
    direction: PackedBudgetDirection,
    #[serde(default)]
    baseline_u64: Option<u64>,
    #[serde(default)]
    current_limit_u64: Option<u64>,
    target_limit_u64: u64,
    #[serde(default)]
    allowed_regression_bps: Option<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PackedBudgetDirection {
    Max,
    Min,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedActionRatchet {
    fixture: String,
    action: String,
    throughput_metric: String,
    allocation_count: PackedNumericRatchet,
    allocated_bytes: PackedNumericRatchet,
    latency_p95: PackedNumericRatchet,
    charged_work: PackedNumericRatchet,
    index_work: PackedNumericRatchet,
    throughput: PackedNumericRatchet,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedNumericRatchet {
    unit: String,
    direction: PackedBudgetDirection,
    baseline_u64: u64,
    current_limit_u64: u64,
    target_limit_u64: u64,
    allowed_regression_bps: u16,
}

fn validate_versions(
    workspace: &Path,
    manifest: &VersionsManifest,
) -> Result<(ArtifactSummary, VersionSummary), String> {
    require_format(manifest.format_version, VERSIONS_MANIFEST)?;
    require_source_head(&manifest.baseline_source_head, "versions baseline")?;
    require_exact(
        &manifest.baseline_scope,
        "current-worktree",
        "versions baseline_scope",
    )?;
    require_exact(&manifest.claim, "inventory-only", "versions claim")?;
    validate_report_policy(&manifest.report_identity)?;

    const ARTIFACTS: &[(&str, ArtifactStatus)] = &[
        ("parsed-program", ArtifactStatus::Present),
        ("checked-program", ArtifactStatus::Present),
        ("source-bundle-digest-v1", ArtifactStatus::Present),
        (
            "embedded-program-redacted-artifact",
            ArtifactStatus::Present,
        ),
        (
            "embedded-program-request-currentness",
            ArtifactStatus::Present,
        ),
        (
            "caller-controlled-embedded-program-digests",
            ArtifactStatus::Absent,
        ),
        ("semantic-program", ArtifactStatus::Present),
        ("contract-verified-program", ArtifactStatus::Present),
        ("erased-program", ArtifactStatus::Present),
        ("machine-plan", ArtifactStatus::Present),
        ("program-artifact-v3", ArtifactStatus::Present),
        ("native-playground-protocol-v14", ArtifactStatus::Present),
        ("physical-plan", ArtifactStatus::Absent),
        ("kernel-ir", ArtifactStatus::Absent),
        ("hardware-process", ArtifactStatus::Absent),
        ("portfolio-completion-evidence", ArtifactStatus::Absent),
    ];
    validate_exact_ids(
        manifest.artifacts.iter().map(|entry| entry.id.as_str()),
        &ARTIFACTS.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "artifact",
    )?;
    let mut artifacts = ArtifactSummary::default();
    for entry in &manifest.artifacts {
        let expected = ARTIFACTS
            .iter()
            .find_map(|(id, status)| (*id == entry.id).then_some(*status))
            .ok_or_else(|| format!("unexpected artifact {}", entry.id))?;
        if entry.status != expected {
            return Err(format!(
                "artifact {} status is {:?}; expected {:?}",
                entry.id, entry.status, expected
            ));
        }
        validate_owner(workspace, &entry.owner_phase, &entry.owner_plan)?;
        if entry.probes.is_empty() {
            return Err(format!("artifact {} has no worktree probe", entry.id));
        }
        let mut positive = false;
        let mut negative = false;
        for probe in &entry.probes {
            match probe.kind {
                ProbeKind::PathExists | ProbeKind::TextContains => positive = true,
                ProbeKind::PathAbsent | ProbeKind::TextAbsent => negative = true,
            }
            validate_probe(workspace, probe)
                .map_err(|error| format!("artifact {}: {error}", entry.id))?;
        }
        match entry.status {
            ArtifactStatus::Absent if positive || !negative => {
                return Err(format!(
                    "absent artifact {} must use only negative probes",
                    entry.id
                ));
            }
            ArtifactStatus::Present | ArtifactStatus::LegacyPresent if negative || !positive => {
                return Err(format!(
                    "present artifact {} must use only positive probes",
                    entry.id
                ));
            }
            _ => {}
        }
        match entry.status {
            ArtifactStatus::Present => artifacts.present.push(entry.id.clone()),
            ArtifactStatus::LegacyPresent => artifacts.legacy_present.push(entry.id.clone()),
            ArtifactStatus::Absent => artifacts.absent.push(entry.id.clone()),
        }
    }

    const VERSION_AXES: &[&str] = &[
        "semantic-value-profile",
        "semantic-program-artifact",
        "erased-program-artifact",
        "verification-policy",
        "verification-manifest",
        "proof-report",
        "physical-layout-profile",
        "target-profile-registry",
        "novywave-product-contract",
        "fjordpulse-product-contract",
        "dataset-fixture-identity",
        "source-bundle-artifact",
        "embedded-program-redacted-artifact",
        "embedded-program-request-currentness",
        "machine-plan-artifact",
        "program-artifact",
        "wire-format",
        "client-session-protocol",
        "session-control-protocol",
        "persistence-format",
        "typed-list-key-codec",
        "typed-list-cursor-token",
        "native-report-tool",
        "native-playground-protocol",
        "phase0-evidence-report",
    ];
    validate_exact_ids(
        manifest.version_axes.iter().map(|axis| axis.id.as_str()),
        VERSION_AXES,
        "version axis",
    )?;
    let mut versions = VersionSummary::default();
    for axis in &manifest.version_axes {
        validate_owner(workspace, &axis.owner_phase, &axis.owner_plan)?;
        require_nonempty(&axis.target, &format!("version axis {} target", axis.id))?;
        require_nonempty(
            &axis.replacement,
            &format!("version axis {} replacement", axis.id),
        )?;
        let absent = axis.absent == Some(true);
        if axis.absent == Some(false) {
            return Err(format!(
                "version axis {} may omit absent or set it to true; false is ambiguous",
                axis.id
            ));
        }
        if axis.current_version.is_some() == absent {
            return Err(format!(
                "version axis {} must have exactly one of current_version or absent=true",
                axis.id
            ));
        }
        if let Some(version) = axis.current_version.as_ref() {
            require_nonempty(
                version,
                &format!("version axis {} current_version", axis.id),
            )?;
            if axis.id == "dataset-fixture-identity" {
                require_exact(
                    version,
                    "boon.dataset-fixture-manifest.v1+SourceBundleDigestV1",
                    "dataset fixture identity version",
                )?;
                require_exact(
                    &axis.owner_phase,
                    "goal-phase-0",
                    "dataset fixture identity owner_phase",
                )?;
            }
            versions.current.push(axis.id.clone());
        } else {
            if axis.id == "dataset-fixture-identity" {
                return Err("dataset-fixture-identity must be frozen in Phase 0".to_owned());
            }
            versions.absent.push(axis.id.clone());
        }
    }
    sort_all(&mut [
        &mut artifacts.present,
        &mut artifacts.legacy_present,
        &mut artifacts.absent,
        &mut versions.current,
        &mut versions.absent,
    ]);
    Ok((artifacts, versions))
}

fn validate_report_policy(policy: &ReportIdentityPolicy) -> Result<(), String> {
    require_exact(&policy.protocol, PROTOCOL, "report protocol")?;
    require_exact(&policy.source_head, "exact", "report source_head policy")?;
    require_exact(
        &policy.workspace_digest,
        "exact",
        "report workspace_digest policy",
    )?;
    require_exact(
        &policy.dirty_final_evidence,
        "forbidden",
        "dirty final evidence policy",
    )?;
    require_exact(&policy.stale_result, "fail", "stale report result")?;
    safe_relative(&policy.inspect_directory)?;
    if policy.max_report_files == 0 || policy.max_report_files > 256 {
        return Err("max_report_files must be in 1..=256".to_owned());
    }
    if policy.max_report_bytes == 0 || policy.max_report_bytes > 4 * 1024 * 1024 {
        return Err("max_report_bytes must be in 1..=4194304".to_owned());
    }
    if policy.max_output_bytes == 0 || policy.max_output_bytes > DEFAULT_MAX_OUTPUT_BYTES {
        return Err(format!(
            "max_output_bytes must be in 1..={DEFAULT_MAX_OUTPUT_BYTES}"
        ));
    }
    Ok(())
}

fn validate_probe(workspace: &Path, probe: &ArtifactProbe) -> Result<(), String> {
    let relative = safe_relative(&probe.path)?;
    let path = workspace.join(relative);
    match probe.kind {
        ProbeKind::PathExists => {
            if probe.needle.is_some() {
                return Err("path-exists probe must not have a needle".to_owned());
            }
            if !path.exists() {
                return Err(format!("{} does not exist", probe.path));
            }
        }
        ProbeKind::PathAbsent => {
            if probe.needle.is_some() {
                return Err("path-absent probe must not have a needle".to_owned());
            }
            if path.exists() {
                return Err(format!("{} unexpectedly exists", probe.path));
            }
        }
        ProbeKind::TextContains | ProbeKind::TextAbsent => {
            let needle = probe
                .needle
                .as_deref()
                .ok_or_else(|| "text probe requires a needle".to_owned())?;
            require_nonempty(needle, "text probe needle")?;
            let text = read_bounded_text(&path, MAX_TEXT_PROBE_BYTES)?;
            let contains = text.contains(needle);
            if probe.kind == ProbeKind::TextContains && !contains {
                return Err(format!(
                    "{} does not contain its required marker",
                    probe.path
                ));
            }
            if probe.kind == ProbeKind::TextAbsent && contains {
                return Err(format!("{} contains a forbidden marker", probe.path));
            }
        }
    }
    Ok(())
}

fn validate_deletions(
    workspace: &Path,
    workspace_files: &[String],
    manifest: &DeletionManifest,
) -> Result<LegacySummary, String> {
    require_format(manifest.format_version, DELETION_MANIFEST)?;
    let manifest_digest = sha256_file(&workspace.join(DELETION_MANIFEST))
        .map_err(|error| format!("{DELETION_MANIFEST}: {error}"))?;
    require_exact(
        manifest_digest.as_str(),
        DELETION_MANIFEST_SHA256,
        "deletion manifest SHA-256",
    )?;
    require_exact(
        &manifest.baseline_source_head,
        CLEAN_BASELINE_HEAD,
        "deletion baseline_source_head",
    )?;
    require_exact(
        &manifest.baseline_scope,
        "current-worktree",
        "deletion baseline_scope",
    )?;
    const ENTRIES: &[DeletionFamilyContract] = &[
        DeletionFamilyContract {
            id: "legacy-runtime-bool-null-error",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "legacy-enum-record-ast-types",
            owner_phase: "goal-phase-1-verified-spine",
            owner_plan: "docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "finite-real-binary64",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "zero-based-apis",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "zero-based-api-negative-fixtures",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::RejectionOnly,
            target: LegacyTarget::NegativeFixturesOnly,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "legacy-match-patterns",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "recursive-executable-values",
            owner_phase: "goal-phase-5-packed",
            owner_plan: "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "runtime-string-field-lookup",
            owner_phase: "goal-phase-5-packed",
            owner_plan: "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "tree-containers",
            owner_phase: "goal-phase-5-packed",
            owner_plan: "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::ClassifiedRetainedOnly,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "id-to-tree-expansion",
            owner_phase: "goal-phase-5-packed",
            owner_plan: "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "query-world-artifacts",
            owner_phase: "goal-phase-3-typed-list",
            owner_plan: "docs/plans/TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md",
            status: LegacyStatus::RejectionOnly,
            target: LegacyTarget::NegativeFixturesOnly,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "unverified-compiler-entrypoints",
            owner_phase: "goal-phase-1-verified-spine",
            owner_plan: "docs/plans/BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "physical-layout-leaks",
            owner_phase: "goal-phase-5-packed",
            owner_plan: "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "forgeable-erased-program-boundary",
            owner_phase: "goal-phase-1-verified-spine",
            owner_plan: "docs/plans/BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "recursive-persistence-values",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "plan-value-algebra-sentinels",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "effect-schema-bool-defaults",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "wire-bool-null-error-tags",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "privileged-error-builtins",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "executor-runtime-sentinel-values",
            owner_phase: "goal-phase-2-foundations",
            owner_plan: "docs/plans/BOON_LANGUAGE_FOUNDATIONS_PLAN.md",
            status: LegacyStatus::Present,
            target: LegacyTarget::Zero,
            measurement: LegacyMeasurement::Occurrence,
        },
        DeletionFamilyContract {
            id: "stale-reports",
            owner_phase: "goal-phase-0",
            owner_plan: "docs/plans/GOAL_PROMPT.md",
            status: LegacyStatus::Policy,
            target: LegacyTarget::ExactIdentityOnly,
            measurement: LegacyMeasurement::ReportIdentity,
        },
    ];
    validate_exact_ids(
        manifest.entries.iter().map(|entry| entry.id.as_str()),
        &ENTRIES.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        "deletion family",
    )?;

    let mut summary = LegacySummary {
        entries: manifest.entries.len(),
        ..LegacySummary::default()
    };
    let mut cache = HashMap::<String, String>::new();
    for entry in &manifest.entries {
        let contract = ENTRIES
            .iter()
            .find(|contract| contract.id == entry.id)
            .ok_or_else(|| format!("unexpected deletion family {}", entry.id))?;
        require_exact(
            &entry.owner_phase,
            contract.owner_phase,
            &format!("{} owner_phase", entry.id),
        )?;
        require_exact(
            &entry.owner_plan,
            contract.owner_plan,
            &format!("{} owner_plan", entry.id),
        )?;
        if entry.status != contract.status
            || entry.target != contract.target
            || entry.measurement != contract.measurement
        {
            return Err(format!(
                "{} policy tuple drifted from its verifier-owned contract",
                entry.id
            ));
        }
        validate_owner(workspace, &entry.owner_phase, &entry.owner_plan)?;
        match entry.measurement {
            LegacyMeasurement::Occurrence if entry.scans.is_empty() => {
                return Err(format!("{} has no occurrence scans", entry.id));
            }
            LegacyMeasurement::ReportIdentity if !entry.scans.is_empty() => {
                return Err(format!(
                    "{} report-identity entry must not have occurrence scans",
                    entry.id
                ));
            }
            LegacyMeasurement::ReportIdentity if entry.id != "stale-reports" => {
                return Err(format!(
                    "{} is not the reserved stale report identity entry",
                    entry.id
                ));
            }
            _ => {}
        }
        if entry.status == LegacyStatus::Policy
            && (entry.target != LegacyTarget::ExactIdentityOnly
                || entry.measurement != LegacyMeasurement::ReportIdentity)
        {
            return Err("stale report policy must target exact-identity-only".to_owned());
        }
        for scan in &entry.scans {
            if scan.expected_files > 1
                && scan.expected_path_occurrences.is_empty()
                && entry.id != "tree-containers"
            {
                return Err(format!(
                    "{} multi-file scan {:?} requires exact per-path occurrences",
                    entry.id, scan.needle
                ));
            }
            let observed = execute_scan(workspace, workspace_files, &mut cache, scan)
                .map_err(|error| format!("{}: {error}", entry.id))?;
            summary.scans += 1;
            summary.observed_occurrences += observed;
        }
        match entry.status {
            LegacyStatus::Present => summary.present_families.push(entry.id.clone()),
            LegacyStatus::RejectionOnly => summary.rejection_only_families.push(entry.id.clone()),
            LegacyStatus::Policy => summary.policy_families.push(entry.id.clone()),
        }
    }
    sort_all(&mut [
        &mut summary.present_families,
        &mut summary.rejection_only_families,
        &mut summary.policy_families,
    ]);
    Ok(summary)
}

fn execute_scan(
    workspace: &Path,
    workspace_files: &[String],
    cache: &mut HashMap<String, String>,
    scan: &OccurrenceScan,
) -> Result<usize, String> {
    if scan.roots.is_empty() || scan.extensions.is_empty() {
        return Err("scan roots and extensions must not be empty".to_owned());
    }
    require_nonempty(&scan.needle, "scan needle")?;
    for root in &scan.roots {
        safe_relative(root)?;
    }
    for extension in &scan.extensions {
        if extension.is_empty()
            || extension.starts_with('.')
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(format!("invalid extension {extension}"));
        }
    }
    match scan.mode {
        OccurrenceScanMode::Text => {
            if !scan.call_functions.is_empty() {
                return Err("text occurrence scans cannot declare call_functions".to_owned());
            }
            if !scan.line_contains_any.is_empty() && scan.needle.contains('\n') {
                return Err(
                    "line-filtered occurrence scans require a single-line needle".to_owned(),
                );
            }
            for marker in &scan.line_contains_any {
                require_nonempty(marker, "line_contains_any marker")?;
                if marker.contains('\n') {
                    return Err("line_contains_any markers must be single-line text".to_owned());
                }
            }
        }
        OccurrenceScanMode::BoonCallArgument => {
            if !scan.line_contains_any.is_empty() {
                return Err("boon-call-argument scans cannot declare line_contains_any".to_owned());
            }
            if scan.call_functions.is_empty() {
                return Err("boon-call-argument scans require call_functions".to_owned());
            }
            if !is_boon_argument_name(&scan.needle) {
                return Err(format!("invalid Boon call argument name {:?}", scan.needle));
            }
            for function in &scan.call_functions {
                require_nonempty(function, "call function")?;
                if !function.bytes().all(is_boon_function_name_byte)
                    || !function.as_bytes().contains(&b'/')
                {
                    return Err(format!("invalid Boon call function {function:?}"));
                }
            }
        }
        OccurrenceScanMode::JsonSourceBundleText => {
            if !scan.call_functions.is_empty() {
                return Err(
                    "json-source-bundle-text scans cannot declare call_functions".to_owned(),
                );
            }
            if !scan.line_contains_any.is_empty() {
                return Err(
                    "json-source-bundle-text scans cannot declare line_contains_any".to_owned(),
                );
            }
            if scan.extensions.iter().any(|extension| extension != "json") {
                return Err("json-source-bundle-text scans may inspect only JSON files".to_owned());
            }
            require_exact_source_bundle_scan_root(workspace_files, scan)?;
        }
        OccurrenceScanMode::JsonSourceBundleBoonCallArgument => {
            if !scan.line_contains_any.is_empty() {
                return Err(
                    "json-source-bundle-boon-call-argument scans cannot declare line_contains_any"
                        .to_owned(),
                );
            }
            if scan.extensions.iter().any(|extension| extension != "json") {
                return Err(
                    "json-source-bundle-boon-call-argument scans may inspect only JSON files"
                        .to_owned(),
                );
            }
            require_exact_source_bundle_scan_root(workspace_files, scan)?;
            if scan.call_functions.is_empty() {
                return Err(
                    "json-source-bundle-boon-call-argument scans require call_functions".to_owned(),
                );
            }
            if !is_boon_argument_name(&scan.needle) {
                return Err(format!("invalid Boon call argument name {:?}", scan.needle));
            }
            for function in &scan.call_functions {
                require_nonempty(function, "call function")?;
                if !function.bytes().all(is_boon_function_name_byte)
                    || !function.as_bytes().contains(&b'/')
                {
                    return Err(format!("invalid Boon call function {function:?}"));
                }
            }
        }
        OccurrenceScanMode::RustToken | OccurrenceScanMode::RustCodeToken => {
            if !scan.call_functions.is_empty() {
                return Err("Rust token occurrence scans cannot declare call_functions".to_owned());
            }
            if !scan.line_contains_any.is_empty() {
                return Err(
                    "Rust token occurrence scans cannot declare line_contains_any".to_owned(),
                );
            }
            if scan.needle.contains(char::is_whitespace)
                || !scan
                    .needle
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| is_rust_identifier_byte(*byte))
                || !scan
                    .needle
                    .as_bytes()
                    .last()
                    .is_some_and(|byte| is_rust_identifier_byte(*byte))
            {
                return Err(format!(
                    "rust-token occurrence scan needle must be a single token path with identifier boundaries: {:?}",
                    scan.needle
                ));
            }
        }
    }
    for allowed in &scan.allowed_paths {
        safe_relative(allowed)?;
    }
    for excluded in &scan.excluded_paths {
        safe_relative(excluded)?;
    }
    for expected_path in scan.expected_path_occurrences.keys() {
        safe_relative(expected_path)?;
    }
    if scan
        .allowed_paths
        .iter()
        .any(|allowed| scan.excluded_paths.contains(allowed))
    {
        return Err("a scan path cannot be both allowed and excluded".to_owned());
    }
    if !scan.expected_path_occurrences.is_empty() {
        if !scan.allowed_paths.is_empty() {
            return Err(
                "exact expected_path_occurrences scans cannot also declare allowed_paths"
                    .to_owned(),
            );
        }
        let expected_occurrences = scan
            .expected_path_occurrences
            .values()
            .copied()
            .sum::<usize>();
        if expected_occurrences != scan.expected_occurrences
            || scan.expected_path_occurrences.len() != scan.expected_files
        {
            return Err(format!(
                "expected_path_occurrences totals {expected_occurrences} occurrences in {} files, but aggregate requires {} in {} files",
                scan.expected_path_occurrences.len(),
                scan.expected_occurrences,
                scan.expected_files
            ));
        }
    }
    let ScanObservation {
        occurrences,
        matched_paths,
        path_occurrences: observed_path_occurrences,
    } = observe_scan(workspace, workspace_files, cache, scan)?;
    if !scan.expected_path_occurrences.is_empty()
        && observed_path_occurrences != scan.expected_path_occurrences
    {
        let format_counts = |counts: &HashMap<String, usize>| {
            let mut rows = counts
                .iter()
                .map(|(path, count)| format!("{path}={count}"))
                .collect::<Vec<_>>();
            rows.sort();
            bounded_list(&rows)
        };
        return Err(format!(
            "needle {:?} exact per-path baseline differs; observed {}; expected {}",
            scan.needle,
            format_counts(&observed_path_occurrences),
            format_counts(&scan.expected_path_occurrences)
        ));
    }
    if !scan.allowed_paths.is_empty() {
        let allowed = scan.allowed_paths.iter().collect::<HashSet<_>>();
        let unexpected = matched_paths
            .iter()
            .filter(|path| !allowed.contains(path))
            .cloned()
            .collect::<Vec<_>>();
        if !unexpected.is_empty() {
            return Err(format!(
                "needle has matches outside allowed paths: {}",
                bounded_list(&unexpected)
            ));
        }
    }
    if occurrences != scan.expected_occurrences || matched_paths.len() != scan.expected_files {
        return Err(format!(
            "needle {:?} observed {} occurrences in {} files; baseline requires {} in {} files; matched {}",
            scan.needle,
            occurrences,
            matched_paths.len(),
            scan.expected_occurrences,
            scan.expected_files,
            bounded_list(&matched_paths)
        ));
    }
    Ok(occurrences)
}

fn observe_scan(
    workspace: &Path,
    workspace_files: &[String],
    cache: &mut HashMap<String, String>,
    scan: &OccurrenceScan,
) -> Result<ScanObservation, String> {
    let mut occurrences = 0;
    let mut matched_paths = Vec::new();
    let mut path_occurrences = HashMap::new();
    for relative in workspace_files {
        if scan.excluded_paths.contains(relative) {
            continue;
        }
        if !scan
            .roots
            .iter()
            .any(|root| path_matches_root(relative, root))
            || !scan
                .extensions
                .iter()
                .any(|extension| path_has_extension(relative, extension))
        {
            continue;
        }
        if !cache.contains_key(relative) {
            let text = read_bounded_text(&workspace.join(relative), MAX_TEXT_PROBE_BYTES)
                .map_err(|error| format!("{relative}: {error}"))?;
            cache.insert(relative.clone(), text);
        }
        let count = count_scan_occurrences(
            cache
                .get(relative)
                .expect("scan cache contains the just-inserted path"),
            scan,
        )
        .map_err(|error| format!("{relative}: {error}"))?;
        if count > 0 {
            occurrences += count;
            matched_paths.push(relative.clone());
            path_occurrences.insert(relative.clone(), count);
        }
    }
    Ok(ScanObservation {
        occurrences,
        matched_paths,
        path_occurrences,
    })
}

fn require_exact_source_bundle_scan_root(
    workspace_files: &[String],
    scan: &OccurrenceScan,
) -> Result<(), String> {
    if scan.roots.len() != 1 {
        return Err("JSON source-bundle scans require exactly one file root".to_owned());
    }
    let root = &scan.roots[0];
    if !workspace_files.iter().any(|relative| relative == root) {
        return Err(format!(
            "JSON source-bundle scan root {root} is not a tracked or untracked workspace file"
        ));
    }
    Ok(())
}

fn validate_container_inventory(
    workspace: &Path,
    workspace_files: &[String],
    manifest: &ContainerInventoryManifest,
) -> Result<ContainerSummary, String> {
    require_format(manifest.format_version, CONTAINER_MANIFEST)?;
    require_source_head(&manifest.baseline_source_head, "container baseline")?;
    require_exact(
        &manifest.baseline_scope,
        "current-worktree",
        "container baseline_scope",
    )?;
    require_exact(&manifest.scan_root, "crates", "container scan_root")?;
    require_exact(&manifest.extension, "rs", "container extension")?;
    require_exact(
        &manifest.word_boundary,
        "rust-identifier",
        "container word_boundary",
    )?;
    require_exact(
        &manifest.lexical_scope,
        "all-source-text",
        "container lexical_scope",
    )?;
    require_exact(
        &manifest.test_scope_policy,
        "test-oracle",
        "container test_scope_policy",
    )?;
    require_exact(
        &manifest.non_code_policy,
        "unrelated-host-code",
        "container non_code_policy",
    )?;
    require_digest(
        &manifest.expected_file_set_sha256,
        "container expected_file_set_sha256",
    )?;
    require_exact(
        &manifest.occurrence_ledger_protocol,
        CONTAINER_OCCURRENCE_LEDGER_PROTOCOL,
        "container occurrence_ledger_protocol",
    )?;
    require_exact(
        &manifest.occurrence_ledger_generator,
        "cargo test -p xtask verify_phase0::tests::regenerate_container_occurrence_ledger -- --ignored --exact",
        "container occurrence_ledger_generator",
    )?;
    safe_relative(&manifest.occurrence_ledger_path)?;
    require_digest(
        &manifest.expected_occurrence_ledger_sha256,
        "container expected_occurrence_ledger_sha256",
    )?;
    if manifest.expected_occurrence_rows == 0 {
        return Err("container expected_occurrence_rows must not be zero".to_owned());
    }
    require_exact(
        &manifest.within_file_owner_phase,
        "goal-phase-5-packed",
        "within-file owner_phase",
    )?;
    require_nonempty(
        &manifest.within_file_owner_action,
        "within-file owner_action",
    )?;

    let observed_construct_order = manifest
        .constructs
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    if observed_construct_order != CONTAINER_CONSTRUCT_IDS {
        return Err(format!(
            "container constructs must be ordered as {}",
            CONTAINER_CONSTRUCT_IDS.join(", ")
        ));
    }
    let observed_categories = manifest
        .categories
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    if observed_categories != CONTAINER_CATEGORY_IDS {
        return Err(format!(
            "container categories must be ordered as {}",
            CONTAINER_CATEGORY_IDS.join(", ")
        ));
    }
    for (index, category) in manifest.categories.iter().enumerate() {
        if usize::from(category.order) != index {
            return Err(format!(
                "container category {} order is {}; expected {}",
                category.id, category.order, index
            ));
        }
        validate_owner(workspace, &category.owner_phase, &category.owner_plan)?;
    }

    if manifest.within_file_classification != WithinFileClassification::Complete {
        return Err(
            "within-file classification must be complete and backed by the exact occurrence ledger"
                .to_owned(),
        );
    }
    let observed_occurrences = scan_container_occurrences(
        workspace,
        workspace_files,
        &manifest.scan_root,
        &manifest.extension,
    )?;
    let mut rust_files = observed_occurrences
        .iter()
        .map(|occurrence| occurrence.path.clone())
        .collect::<Vec<_>>();
    rust_files.sort();
    rust_files.dedup();
    if rust_files.len() != manifest.expected_file_count {
        return Err(format!(
            "container worktree has {} matching Rust files; expected {}",
            rust_files.len(),
            manifest.expected_file_count
        ));
    }
    let mut file_set_bytes = rust_files.join("\n").into_bytes();
    file_set_bytes.push(b'\n');
    let file_set_sha256 = sha256_bytes(&file_set_bytes).as_str().to_owned();
    if file_set_sha256 != manifest.expected_file_set_sha256 {
        return Err(format!(
            "container Rust file set digest is {file_set_sha256}; expected {}",
            manifest.expected_file_set_sha256
        ));
    }

    let classified_occurrences = load_container_occurrence_ledger(workspace, manifest)?;
    let classified_identities = classified_occurrences
        .iter()
        .map(|occurrence| occurrence.identity.clone())
        .collect::<Vec<_>>();
    if classified_identities != observed_occurrences {
        let observed = observed_occurrences.iter().collect::<HashSet<_>>();
        let classified = classified_identities.iter().collect::<HashSet<_>>();
        let missing = observed_occurrences
            .iter()
            .filter(|identity| !classified.contains(identity))
            .map(format_container_occurrence)
            .collect::<Vec<_>>();
        let unexpected = classified_identities
            .iter()
            .filter(|identity| !observed.contains(identity))
            .map(format_container_occurrence)
            .collect::<Vec<_>>();
        return Err(format!(
            "container occurrence ledger is not one-to-one with the worktree; missing {}; stale or unexpected {}",
            bounded_list(&missing),
            bounded_list(&unexpected)
        ));
    }
    validate_container_occurrence_semantics(workspace, &classified_occurrences)?;

    let mut construct_occurrences = vec![0_usize; CONTAINER_CONSTRUCT_IDS.len()];
    let mut construct_files = vec![HashSet::<String>::new(); CONTAINER_CONSTRUCT_IDS.len()];
    let mut category_occurrences = vec![[0_usize; 4]; CONTAINER_CATEGORY_IDS.len()];
    let mut category_files = vec![HashSet::<String>::new(); CONTAINER_CATEGORY_IDS.len()];
    let mut file_categories = HashMap::<String, HashSet<String>>::new();
    for occurrence in &classified_occurrences {
        let construct_index = CONTAINER_CONSTRUCT_IDS
            .iter()
            .position(|construct| *construct == occurrence.identity.construct)
            .expect("ledger parser accepts only known constructs");
        construct_occurrences[construct_index] += 1;
        construct_files[construct_index].insert(occurrence.identity.path.clone());
        let category_index = CONTAINER_CATEGORY_IDS
            .iter()
            .position(|category| *category == occurrence.category)
            .expect("ledger parser accepts only known categories");
        category_occurrences[category_index][construct_index] += 1;
        category_files[category_index].insert(occurrence.identity.path.clone());
        file_categories
            .entry(occurrence.identity.path.clone())
            .or_default()
            .insert(occurrence.category.clone());
    }

    let mut construct_summary = Vec::new();
    for (index, expected) in manifest.constructs.iter().enumerate() {
        if construct_occurrences[index] != expected.expected_occurrences
            || construct_files[index].len() != expected.expected_files
        {
            return Err(format!(
                "{} word-boundary count is {} in {} files; expected {} in {} files",
                expected.id,
                construct_occurrences[index],
                construct_files[index].len(),
                expected.expected_occurrences,
                expected.expected_files
            ));
        }
        construct_summary.push(ContainerConstructSummary {
            id: expected.id.clone(),
            occurrences: construct_occurrences[index],
            files: construct_files[index].len(),
        });
    }
    let mut category_summary = Vec::new();
    for (index, expected) in manifest.categories.iter().enumerate() {
        let observed = [
            category_files[index].len(),
            category_occurrences[index][0],
            category_occurrences[index][1],
            category_occurrences[index][2],
            category_occurrences[index][3],
        ];
        let required = [
            expected.expected_files,
            expected.expected_btree_map,
            expected.expected_btree_set,
            expected.expected_hash_map,
            expected.expected_hash_set,
        ];
        if observed != required {
            return Err(format!(
                "container category {} observed files/map/set/hash-map/hash-set {:?}; expected {:?}",
                expected.id, observed, required
            ));
        }
        category_summary.push(ContainerCategorySummary {
            order: expected.order,
            id: expected.id.clone(),
            files: observed[0],
            btree_map: observed[1],
            btree_set: observed[2],
            hash_map: observed[3],
            hash_set: observed[4],
        });
    }
    for index in 0..CONTAINER_CONSTRUCT_IDS.len() {
        let category_total = category_occurrences
            .iter()
            .map(|counts| counts[index])
            .sum::<usize>();
        if category_total != construct_occurrences[index] {
            return Err(format!(
                "{} category total {category_total} differs from aggregate {}",
                CONTAINER_CONSTRUCT_IDS[index], construct_occurrences[index]
            ));
        }
    }
    let mixed_category_files = file_categories
        .values()
        .filter(|categories| categories.len() > 1)
        .count();
    let occurrence_ledger_bytes =
        fs::metadata(workspace.join(safe_relative(&manifest.occurrence_ledger_path)?))
            .map_err(|error| format!("{}: {error}", manifest.occurrence_ledger_path))?
            .len();
    Ok(ContainerSummary {
        file_set_sha256,
        rust_files,
        constructs: construct_summary,
        categories: category_summary,
        occurrence_ledger_path: manifest.occurrence_ledger_path.clone(),
        occurrence_ledger_sha256: manifest.expected_occurrence_ledger_sha256.clone(),
        occurrence_ledger_bytes,
        occurrence_rows: classified_occurrences.len(),
        mixed_category_files,
        within_file_classification: "complete".to_owned(),
    })
}

fn scan_container_occurrences(
    workspace: &Path,
    workspace_files: &[String],
    scan_root: &str,
    extension: &str,
) -> Result<Vec<ContainerOccurrenceIdentity>, String> {
    let mut occurrences = Vec::new();
    for relative in workspace_files {
        if !path_matches_root(relative, scan_root) || !path_has_extension(relative, extension) {
            continue;
        }
        let text = read_bounded_text(&workspace.join(relative), MAX_TEXT_PROBE_BYTES)?;
        let mut ordinals = [0_usize; 4];
        for (line_index, raw_line) in text.split_inclusive('\n').enumerate() {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let mut line_occurrences = Vec::new();
            for (construct_index, construct) in CONTAINER_CONSTRUCT_IDS.iter().enumerate() {
                for byte_column in identifier_word_offsets(line, construct) {
                    line_occurrences.push((byte_column, construct_index));
                }
            }
            line_occurrences.sort();
            let context_sha256 = sha256_bytes(line.as_bytes()).as_str().to_owned();
            for (byte_column, construct_index) in line_occurrences {
                ordinals[construct_index] += 1;
                occurrences.push(ContainerOccurrenceIdentity {
                    path: relative.clone(),
                    line: line_index + 1,
                    byte_column: byte_column + 1,
                    ordinal: ordinals[construct_index],
                    construct: CONTAINER_CONSTRUCT_IDS[construct_index].to_owned(),
                    context_sha256: context_sha256.clone(),
                });
            }
        }
    }
    occurrences.sort();
    Ok(occurrences)
}

fn load_container_occurrence_ledger(
    workspace: &Path,
    manifest: &ContainerInventoryManifest,
) -> Result<Vec<ClassifiedContainerOccurrence>, String> {
    let relative = safe_relative(&manifest.occurrence_ledger_path)?;
    let path = workspace.join(relative);
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("{}: {error}", manifest.occurrence_ledger_path))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OCCURRENCE_LEDGER_BYTES {
        return Err(format!(
            "{} must be a regular file in 1..={MAX_OCCURRENCE_LEDGER_BYTES} bytes",
            manifest.occurrence_ledger_path
        ));
    }
    let digest = sha256_file(&path)
        .map_err(|error| format!("{}: {error}", manifest.occurrence_ledger_path))?
        .as_str()
        .to_owned();
    if digest != manifest.expected_occurrence_ledger_sha256 {
        return Err(format!(
            "container occurrence ledger digest is {digest}; expected {}",
            manifest.expected_occurrence_ledger_sha256
        ));
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", manifest.occurrence_ledger_path))?;
    let mut lines = text.lines();
    let expected_protocol = format!("# {CONTAINER_OCCURRENCE_LEDGER_PROTOCOL}");
    if lines.next() != Some(expected_protocol.as_str()) {
        return Err("container occurrence ledger has the wrong protocol header".to_owned());
    }
    if lines.next()
        != Some("# path\tline\tbyte_column\tordinal\tconstruct\tcategory\treason\tcontext_sha256")
    {
        return Err("container occurrence ledger has the wrong column header".to_owned());
    }
    let mut classified = Vec::new();
    let mut identities = HashSet::new();
    for (index, line) in lines.enumerate() {
        if line.is_empty() {
            return Err(format!(
                "container occurrence ledger row {} is blank",
                index + 3
            ));
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.len() != 8 {
            return Err(format!(
                "container occurrence ledger row {} has {} columns; expected 8",
                index + 3,
                columns.len()
            ));
        }
        safe_relative(columns[0])?;
        let source_line = parse_positive_usize(columns[1], "occurrence line")?;
        let byte_column = parse_positive_usize(columns[2], "occurrence byte_column")?;
        let ordinal = parse_positive_usize(columns[3], "occurrence ordinal")?;
        if !CONTAINER_CONSTRUCT_IDS.contains(&columns[4]) {
            return Err(format!(
                "container occurrence ledger row {} has unknown construct {}",
                index + 3,
                columns[4]
            ));
        }
        if !CONTAINER_CATEGORY_IDS.contains(&columns[5]) {
            return Err(format!(
                "container occurrence ledger row {} has unknown category {}",
                index + 3,
                columns[5]
            ));
        }
        if !CONTAINER_REASON_IDS.contains(&columns[6]) {
            return Err(format!(
                "container occurrence ledger row {} has unknown reason {}",
                index + 3,
                columns[6]
            ));
        }
        require_digest(columns[7], "container occurrence context_sha256")?;
        let identity = ContainerOccurrenceIdentity {
            path: columns[0].to_owned(),
            line: source_line,
            byte_column,
            ordinal,
            construct: columns[4].to_owned(),
            context_sha256: columns[7].to_owned(),
        };
        if !identities.insert(identity.clone()) {
            return Err(format!(
                "container occurrence ledger duplicates {}",
                format_container_occurrence(&identity)
            ));
        }
        classified.push(ClassifiedContainerOccurrence {
            identity,
            category: columns[5].to_owned(),
            reason: columns[6].to_owned(),
        });
    }
    if classified.len() != manifest.expected_occurrence_rows {
        return Err(format!(
            "container occurrence ledger has {} rows; expected {}",
            classified.len(),
            manifest.expected_occurrence_rows
        ));
    }
    classified.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(classified)
}

fn parse_positive_usize(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("{label} {value:?}: {error}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

fn validate_container_occurrence_semantics(
    workspace: &Path,
    occurrences: &[ClassifiedContainerOccurrence],
) -> Result<(), String> {
    let mut sources = HashMap::<String, ContainerOccurrenceSemanticSource>::new();
    for occurrence in occurrences {
        if !sources.contains_key(&occurrence.identity.path) {
            let text = read_bounded_text(
                &workspace.join(&occurrence.identity.path),
                MAX_TEXT_PROBE_BYTES,
            )?;
            sources.insert(
                occurrence.identity.path.clone(),
                container_occurrence_semantic_source(&text),
            );
        }
        let source = sources
            .get(&occurrence.identity.path)
            .expect("semantic source was just inserted");
        validate_container_occurrence_classification(occurrence, source)?;
    }
    Ok(())
}

fn validate_container_occurrence_classification(
    occurrence: &ClassifiedContainerOccurrence,
    source: &ContainerOccurrenceSemanticSource,
) -> Result<(), String> {
    let (expected_category, expected_reason) =
        classify_generated_container_occurrence(&occurrence.identity, source);
    if occurrence.category != expected_category || occurrence.reason != expected_reason {
        return Err(format!(
            "{} is classified as {}/{}; live source requires {expected_category}/{expected_reason}",
            format_container_occurrence(&occurrence.identity),
            occurrence.category,
            occurrence.reason,
        ));
    }
    Ok(())
}

fn container_occurrence_semantic_source(text: &str) -> ContainerOccurrenceSemanticSource {
    ContainerOccurrenceSemanticSource {
        lines: text.lines().map(str::to_owned).collect(),
        line_starts: source_line_starts(text),
        test_only_intervals: test_only_intervals(text),
        structural: rust_structural_bytes(text),
    }
}

fn format_container_occurrence(identity: &ContainerOccurrenceIdentity) -> String {
    format!(
        "{}:{}:{} {}#{}",
        identity.path, identity.line, identity.byte_column, identity.construct, identity.ordinal
    )
}

#[cfg(test)]
fn generate_container_occurrence_ledger(workspace: &Path) -> Result<usize, String> {
    let workspace_files = workspace_files(workspace)?;
    let occurrences = scan_container_occurrences(workspace, &workspace_files, "crates", "rs")?;
    let mut sources = HashMap::<String, ContainerOccurrenceSemanticSource>::new();
    let mut output = format!(
        "# {CONTAINER_OCCURRENCE_LEDGER_PROTOCOL}\n# path\tline\tbyte_column\tordinal\tconstruct\tcategory\treason\tcontext_sha256\n"
    );
    for occurrence in &occurrences {
        if !sources.contains_key(&occurrence.path) {
            let text = read_bounded_text(&workspace.join(&occurrence.path), MAX_TEXT_PROBE_BYTES)?;
            sources.insert(
                occurrence.path.clone(),
                container_occurrence_semantic_source(&text),
            );
        }
        let source = sources
            .get(&occurrence.path)
            .expect("generator source was just inserted");
        let (category, reason) = classify_generated_container_occurrence(occurrence, source);
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            occurrence.path,
            occurrence.line,
            occurrence.byte_column,
            occurrence.ordinal,
            occurrence.construct,
            category,
            reason,
            occurrence.context_sha256
        ));
    }
    let output_path = workspace.join(CONTAINER_OCCURRENCE_LEDGER);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp_path =
        output_path.with_extension(format!("tsv.phase0-generator-{}", std::process::id()));
    fs::write(&temp_path, output).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, &output_path).map_err(|error| error.to_string())?;
    Ok(occurrences.len())
}

#[cfg(test)]
fn regenerate_deletion_scan_expectations(workspace: &Path) -> Result<usize, String> {
    let manifest_path = workspace.join(DELETION_MANIFEST);
    let text = read_bounded_text(&manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest = toml::from_str::<DeletionManifest>(&text)
        .map_err(|error| format!("{DELETION_MANIFEST}: {error}"))?;
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("{DELETION_MANIFEST}: {error}"))?;
    let entry_tables = document
        .get_mut("entries")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
        .ok_or_else(|| format!("{DELETION_MANIFEST}: entries is not an array of tables"))?;
    if entry_tables.len() != manifest.entries.len() {
        return Err(format!(
            "{DELETION_MANIFEST}: parsed {} entry tables but deserialized {} entries",
            entry_tables.len(),
            manifest.entries.len()
        ));
    }

    let workspace_files = workspace_files(workspace)?;
    let mut cache = HashMap::<String, String>::new();
    let mut updated_scans = 0;
    for (entry_table, entry) in entry_tables.iter_mut().zip(&manifest.entries) {
        if entry.scans.is_empty() {
            if entry_table.get("scans").is_some() {
                return Err(format!(
                    "{DELETION_MANIFEST}: entry {} unexpectedly declares scan tables",
                    entry.id
                ));
            }
            continue;
        }
        let scan_tables = entry_table
            .get_mut("scans")
            .and_then(toml_edit::Item::as_array_of_tables_mut)
            .ok_or_else(|| {
                format!(
                    "{DELETION_MANIFEST}: entry {} scans is not an array of tables",
                    entry.id
                )
            })?;
        if scan_tables.len() != entry.scans.len() {
            return Err(format!(
                "{DELETION_MANIFEST}: entry {} has {} scan tables but {} deserialized scans",
                entry.id,
                scan_tables.len(),
                entry.scans.len()
            ));
        }
        for (scan_table, scan) in scan_tables.iter_mut().zip(&entry.scans) {
            let observation = observe_scan(workspace, &workspace_files, &mut cache, scan)
                .map_err(|error| format!("{} {:?}: {error}", entry.id, scan.needle))?;
            let occurrences = i64::try_from(observation.occurrences)
                .map_err(|_| "deletion occurrence count exceeds i64".to_owned())?;
            let files = i64::try_from(observation.matched_paths.len())
                .map_err(|_| "deletion file count exceeds i64".to_owned())?;
            scan_table.insert("expected_occurrences", toml_edit::value(occurrences));
            scan_table.insert("expected_files", toml_edit::value(files));

            let exact_paths = scan.allowed_paths.is_empty()
                && (observation.matched_paths.len() > 1
                    || (observation.matched_paths.len() == 1
                        && !scan.expected_path_occurrences.is_empty()));
            if exact_paths {
                let mut paths = observation.path_occurrences.into_iter().collect::<Vec<_>>();
                paths.sort_by(|left, right| left.0.cmp(&right.0));
                let mut exact = toml_edit::Table::new();
                for (path, count) in paths {
                    let count = i64::try_from(count)
                        .map_err(|_| "deletion path count exceeds i64".to_owned())?;
                    exact.insert(&path, toml_edit::value(count));
                }
                scan_table.insert("expected_path_occurrences", toml_edit::Item::Table(exact));
            } else {
                scan_table.remove("expected_path_occurrences");
            }
            updated_scans += 1;
        }
    }

    let temp_path =
        manifest_path.with_extension(format!("toml.phase0-generator-{}", std::process::id()));
    fs::write(&temp_path, document.to_string()).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, &manifest_path).map_err(|error| error.to_string())?;
    Ok(updated_scans)
}

fn source_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn classify_generated_container_occurrence(
    occurrence: &ContainerOccurrenceIdentity,
    source: &ContainerOccurrenceSemanticSource,
) -> (&'static str, &'static str) {
    let source_offset = source.line_starts[occurrence.line - 1] + occurrence.byte_column - 1;
    if occurrence.path.contains("/tests/")
        || occurrence.path.ends_with("/tests.rs")
        || source
            .test_only_intervals
            .iter()
            .any(|(start, end)| source_offset >= *start && source_offset < *end)
    {
        return ("test-oracle", "test-only-scope");
    }
    let construct_bytes = occurrence.construct.as_bytes();
    if source
        .structural
        .get(source_offset..source_offset + construct_bytes.len())
        != Some(construct_bytes)
    {
        return ("unrelated-host-code", "non-code-reference");
    }

    let context_start = occurrence.line.saturating_sub(3);
    let context_end = (occurrence.line + 2).min(source.lines.len());
    let context = source.lines[context_start..context_end]
        .join(" ")
        .to_ascii_lowercase();
    let current_line = source.lines[occurrence.line - 1].to_ascii_lowercase();
    let path = occurrence.path.as_str();
    let construct = occurrence.construct.as_str();
    let is_set = construct.ends_with("Set");
    let canonical_crate = [
        "crates/boon_contract/",
        "crates/boon_data/",
        "crates/boon_document_model/",
        "crates/boon_effect_schema/",
        "crates/boon_persistence/",
        "crates/boon_wire/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix));
    let engine_crate = [
        "crates/boon_compiler/",
        "crates/boon_ir/",
        "crates/boon_plan/",
        "crates/boon_plan_executor/",
        "crates/boon_runtime/",
        "crates/boon_typecheck/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix));
    let canonical_context = contains_any(
        &context,
        &[
            "artifact",
            "canonical",
            "cbor",
            "codec",
            "decode",
            "deserialize",
            "digest",
            "durable",
            "encode",
            "json",
            "manifest",
            "migration",
            "persist",
            "serialize",
            "snapshot",
            "toml",
            "wire",
        ],
    );
    let ordered_context = contains_any(
        &current_line,
        &[
            "cursor",
            "index_range",
            "order_token",
            "ordered",
            "range(",
            "sort",
        ],
    );
    let name_context = contains_any(
        &current_line,
        &[
            "&str",
            "binding",
            "field_name",
            "module",
            "name",
            "namespace",
            "path",
            "route",
            "source_name",
            "string",
            "symbol",
        ],
    );

    if path.starts_with("crates/boon_list_access/") {
        return ("ordered-index", "typed-list-order-kernel");
    }
    if current_line.contains("std::collections") {
        if canonical_crate {
            return ("canonical-boundary", "canonical-boundary-import");
        }
        if is_set {
            return ("membership", "membership-container-import");
        }
        if engine_crate {
            return ("dense-id-table", "engine-container-import");
        }
        return ("unrelated-host-code", "host-container-import");
    }
    if canonical_crate && (canonical_context || !is_set) {
        return ("canonical-boundary", "canonical-boundary-structure");
    }
    if is_set {
        return ("membership", "set-membership-structure");
    }
    if ordered_context && engine_crate {
        return ("ordered-index", "ordered-engine-index");
    }
    if canonical_context {
        return ("canonical-boundary", "canonicalization-context");
    }
    if name_context {
        return ("name-lookup", "name-or-path-key-context");
    }
    if engine_crate {
        return ("dense-id-table", "engine-id-table-context");
    }
    if path.starts_with("crates/boon_parser/") {
        return ("name-lookup", "parser-name-table-context");
    }
    ("unrelated-host-code", "host-or-tooling-container")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn test_only_intervals(text: &str) -> Vec<(usize, usize)> {
    const ATTRIBUTES: &[&[u8]] = &[
        b"#[cfg(test)]",
        b"#[test]",
        b"#[tokio::test]",
        b"#[wasm_bindgen_test]",
    ];
    let structural = rust_structural_bytes(text);
    let mut intervals = Vec::new();
    for attribute in ATTRIBUTES {
        let mut offset = 0;
        while offset + attribute.len() <= structural.len() {
            let Some(relative) = structural[offset..]
                .windows(attribute.len())
                .position(|window| window == *attribute)
            else {
                break;
            };
            let start = offset + relative;
            let end = test_only_item_end(&structural, start + attribute.len());
            intervals.push((start, end));
            offset = start + attribute.len();
        }
    }
    intervals.sort();
    intervals
}

fn test_only_item_end(structural: &[u8], start: usize) -> usize {
    let mut parentheses = 0_usize;
    let mut brackets = 0_usize;
    let mut angles = 0_usize;
    let mut index = start;
    while index < structural.len() {
        match structural[index] {
            b'(' => parentheses += 1,
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'<' if parentheses == 0 && brackets == 0 => angles += 1,
            b'>' if parentheses == 0 && brackets == 0 => angles = angles.saturating_sub(1),
            b'{' if parentheses == 0 && brackets == 0 && angles == 0 => {
                return matching_structural_brace(structural, index)
                    .map_or(structural.len(), |end| end + 1);
            }
            b';' | b',' if parentheses == 0 && brackets == 0 && angles == 0 => {
                return index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    structural.len()
}

fn matching_structural_brace(structural: &[u8], opening: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in structural[opening..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(opening + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn rust_structural_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut structural = bytes.to_vec();
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |relative| index + relative);
            blank_non_newlines(&mut structural[index..end]);
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            let start = index;
            index += 2;
            let mut depth = 1_usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            blank_non_newlines(&mut structural[start..index]);
            continue;
        }
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = quote - index - 1;
                let start = index;
                index = quote + 1;
                while index < bytes.len() {
                    if bytes[index] == b'"'
                        && index + 1 + hashes <= bytes.len()
                        && bytes[index + 1..index + 1 + hashes]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        index += 1 + hashes;
                        break;
                    }
                    index += 1;
                }
                blank_non_newlines(&mut structural[start..index]);
                continue;
            }
        }
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            blank_non_newlines(&mut structural[start..index]);
            continue;
        }
        if bytes[index] == b'\'' {
            let simple_ascii = index + 2 < bytes.len() && bytes[index + 2] == b'\'';
            let escaped_or_unicode = index + 1 < bytes.len()
                && (bytes[index + 1] == b'\\' || !bytes[index + 1].is_ascii());
            let end = (simple_ascii || escaped_or_unicode)
                .then(|| {
                    bytes[index + 1..]
                        .iter()
                        .position(|byte| *byte == b'\'')
                        .map(|relative| index + relative + 2)
                })
                .flatten();
            if let Some(end) = end.filter(|end| *end - index <= 16) {
                blank_non_newlines(&mut structural[index..end]);
                index = end;
                continue;
            }
        }
        index += 1;
    }
    structural
}

fn blank_non_newlines(bytes: &mut [u8]) {
    for byte in bytes {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn validate_fixtures(
    workspace: &Path,
    manifest: &boon_phase0_baseline::evidence::FixtureEvidenceManifestV2,
) -> Result<FixtureSummary, String> {
    use boon_phase0_baseline::evidence::{
        FixtureEvidenceStatus, FixtureStatus, FixtureTargetStatus,
    };

    manifest.validate_workspace(workspace)?;
    require_exact(
        &manifest.dataset_manifest,
        DATASET_MANIFEST,
        "fixture dataset_manifest",
    )?;
    let mut summary = FixtureSummary::default();
    for fixture in &manifest.fixtures {
        validate_owner(workspace, &fixture.owner_phase, &fixture.owner_plan)?;
        if fixture.status != FixtureStatus::Existing {
            return Err(format!("fixture {} is not executable evidence", fixture.id));
        }
        summary.existing.push(fixture.id.clone());
        summary.source_paths.push(fixture.path.clone());
        match fixture.target_status {
            FixtureTargetStatus::CurrentSupported => {
                summary.current_supported.push(fixture.id.clone())
            }
            FixtureTargetStatus::CurrentAnalogue => {
                summary.current_analogues.push(fixture.id.clone())
            }
            FixtureTargetStatus::NotYetImplemented => {
                summary.not_yet_implemented.push(fixture.id.clone())
            }
        }
        match fixture.evidence_status {
            FixtureEvidenceStatus::CompileExecuteCurrent => {
                summary.compile_execute_current.push(fixture.id.clone())
            }
            FixtureEvidenceStatus::CompileRejectFuture => {
                summary.compile_reject_future.push(fixture.id.clone())
            }
            FixtureEvidenceStatus::CurrentExecutionPlusFutureRejection => summary
                .current_execution_plus_future_rejection
                .push(fixture.id.clone()),
            FixtureEvidenceStatus::HeadlessCurrentAnalogue => {
                summary.headless_current_analogues.push(fixture.id.clone())
            }
            FixtureEvidenceStatus::CurrentExecutionPlusMeasuredBaseline => summary
                .current_execution_plus_measured_baseline
                .push(fixture.id.clone()),
        }
    }
    sort_all(&mut [
        &mut summary.existing,
        &mut summary.current_supported,
        &mut summary.current_analogues,
        &mut summary.not_yet_implemented,
        &mut summary.compile_execute_current,
        &mut summary.compile_reject_future,
        &mut summary.current_execution_plus_future_rejection,
        &mut summary.headless_current_analogues,
        &mut summary.current_execution_plus_measured_baseline,
        &mut summary.source_paths,
    ]);
    Ok(summary)
}

fn validate_datasets(
    workspace: &Path,
    manifest: &boon_phase0_baseline::dataset::DatasetFixtureManifestV1,
    fixture_summary: &FixtureSummary,
) -> Result<DatasetSummary, String> {
    use boon_phase0_baseline::dataset::{
        DATASET_FIXTURE_SCHEMA, DATASET_IDENTITY, DEFAULT_DATASET_MANIFEST,
    };

    require_exact(
        DEFAULT_DATASET_MANIFEST,
        DATASET_MANIFEST,
        "dataset manifest path",
    )?;
    let verified = manifest.verify_workspace(workspace)?;
    let mut entrypoints = HashMap::new();
    for fixture in &manifest.fixtures {
        if let Some(previous) = entrypoints.insert(fixture.entrypoint.as_str(), fixture) {
            return Err(format!(
                "dataset entrypoint {} is assigned to both {} and {}",
                fixture.entrypoint, previous.id, fixture.id
            ));
        }
    }
    for path in &fixture_summary.source_paths {
        if !entrypoints.contains_key(path.as_str()) {
            return Err(format!(
                "executable fixture source {path} is not a canonical dataset entrypoint"
            ));
        }
    }
    let phase0_sources = workspace_files(workspace)?
        .into_iter()
        .filter(|path| path.starts_with("testdata/phase0/fixtures/") && path.ends_with(".bn"))
        .collect::<Vec<_>>();
    let uncovered = phase0_sources
        .iter()
        .filter(|path| !entrypoints.contains_key(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !uncovered.is_empty() {
        return Err(format!(
            "Phase 0 executable source files lack dataset identity: {}",
            bounded_list(&uncovered)
        ));
    }
    let (packed_fixtures, _) = boon_phase0_baseline::manifest::FixtureManifest::load(
        &workspace.join(boon_phase0_baseline::report::DEFAULT_MANIFEST),
    )?;
    let missing_packed = packed_fixtures
        .fixtures
        .iter()
        .filter(|fixture| !entrypoints.contains_key(fixture.source.as_str()))
        .map(|fixture| fixture.source.clone())
        .collect::<Vec<_>>();
    if !missing_packed.is_empty() {
        return Err(format!(
            "packed baseline source files lack dataset entrypoint identity: {}",
            bounded_list(&missing_packed)
        ));
    }

    let mut categories = verified
        .iter()
        .map(|fixture| fixture.category.as_str().to_owned())
        .collect::<Vec<_>>();
    categories.sort();
    categories.dedup();
    let total_units = verified.iter().map(|fixture| fixture.unit_count).sum();
    let fixtures = manifest
        .fixtures
        .iter()
        .zip(verified)
        .map(|(declared, fixture)| DatasetFixtureSummary {
            id: fixture.id,
            category: fixture.category.as_str().to_owned(),
            entrypoint: declared.entrypoint.clone(),
            source_bundle_digest_v1: fixture.source_bundle_digest_v1.to_string(),
            unit_count: fixture.unit_count,
        })
        .collect();
    Ok(DatasetSummary {
        schema: DATASET_FIXTURE_SCHEMA.to_owned(),
        identity: DATASET_IDENTITY.to_owned(),
        categories,
        fixtures,
        total_units,
    })
}

fn validate_documents(
    workspace: &Path,
    workspace_files: &[String],
    manifest: &DocumentManifest,
) -> Result<DocumentSummary, String> {
    require_format(manifest.format_version, DOCUMENT_MANIFEST)?;
    const DOCUMENTS: &[&str] = &[
        "implementation-order",
        "unified-goal",
        "out-parameters",
        "type-inference",
        "foundations",
        "typed-list",
        "formal-verification",
        "persistence",
        "packed-data",
        "first-riscv",
        "portfolio",
        "novywave",
        "fjordpulse",
        "language-semantics",
        "bytes-semantics",
        "runtime-model",
        "list-model",
        "delta-protocol",
        "native-gpu-pipeline",
        "native-gpu-handoff-manifest",
        "repository-agent-contract",
        "boon-type-notation-and-inspector",
        "native-editor-type-hints",
        "pass-passed-and-todomvc-ui-model",
        "number-specialization-experiment",
        "fpga-todomvc-lowering",
        "previous-attempts",
        "simplification-native-recovery",
        "manual-testing-runbook",
        "persons-pro-local-first",
    ];
    const REPLACEMENT_DOCUMENTS: &[(&str, DocumentClassification, &str)] = &[
        (
            "boon-type-notation-and-inspector",
            DocumentClassification::RewriteRequired,
            "goal-phase-2-foundations",
        ),
        (
            "native-editor-type-hints",
            DocumentClassification::RewriteRequired,
            "goal-phase-2-foundations",
        ),
        (
            "pass-passed-and-todomvc-ui-model",
            DocumentClassification::RewriteRequired,
            "goal-phase-1-verified-spine",
        ),
        (
            "persons-pro-local-first",
            DocumentClassification::RewriteRequired,
            "goal-phase-6-web-persistence",
        ),
        (
            "fpga-todomvc-lowering",
            DocumentClassification::DeleteAfterReplacement,
            "goal-phase-8-riscv",
        ),
        (
            "number-specialization-experiment",
            DocumentClassification::DeleteAfterReplacement,
            "goal-phase-2-foundations",
        ),
    ];
    validate_exact_ids(
        manifest.documents.iter().map(|entry| entry.id.as_str()),
        DOCUMENTS,
        "document",
    )?;
    validate_exact_ids(
        manifest
            .documents
            .iter()
            .filter(|entry| {
                matches!(
                    entry.classification,
                    DocumentClassification::RewriteRequired
                        | DocumentClassification::DeleteAfterReplacement
                )
            })
            .map(|entry| entry.id.as_str()),
        &REPLACEMENT_DOCUMENTS
            .iter()
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        "replacement document",
    )?;
    let mut paths = HashSet::new();
    let mut summary = DocumentSummary::default();
    for entry in &manifest.documents {
        if let Some((_, expected_classification, expected_phase)) = REPLACEMENT_DOCUMENTS
            .iter()
            .find(|(id, _, _)| *id == entry.id)
        {
            if entry.classification != *expected_classification {
                return Err(format!(
                    "document {} classification is {:?}; expected {:?}",
                    entry.id, entry.classification, expected_classification
                ));
            }
            require_exact(
                &entry.owner_phase,
                expected_phase,
                &format!("document {} owner_phase", entry.id),
            )?;
        }
        require_nonempty(
            &entry.owner_phase,
            &format!("document {} owner_phase", entry.id),
        )?;
        let relative = safe_relative(&entry.path)?;
        if !paths.insert(entry.path.as_str()) {
            return Err(format!("document path {} is listed twice", entry.path));
        }
        let path = workspace.join(relative);
        if !path.is_file() {
            return Err(format!(
                "document {} path {} is missing",
                entry.id, entry.path
            ));
        }
        let text = read_bounded_text(&path, MAX_TEXT_PROBE_BYTES)?;
        validate_document_status_notice(entry, &text)?;
        match entry.classification {
            DocumentClassification::CurrentBehavior
            | DocumentClassification::ActiveImplementationContract => {
                summary.current.push(entry.id.clone())
            }
            DocumentClassification::SequenceContract
            | DocumentClassification::ExecutionContract
            | DocumentClassification::TargetContract => {
                summary.target_contracts.push(entry.id.clone())
            }
            DocumentClassification::RewriteRequired => {
                summary.rewrite_required.push(entry.id.clone())
            }
            DocumentClassification::DeleteAfterReplacement => {
                summary.delete_after_replacement.push(entry.id.clone())
            }
            DocumentClassification::HistoricalDecision
            | DocumentClassification::HistoricalContext => {
                summary.historical.push(entry.id.clone())
            }
        }
    }
    let contract_files = workspace_files
        .iter()
        .filter(|path| {
            let direct_architecture = path.strip_prefix("docs/architecture/").is_some_and(|name| {
                !name.contains('/') && (name.ends_with(".md") || name.ends_with(".json"))
            });
            let direct_plan = path
                .strip_prefix("docs/plans/")
                .is_some_and(|name| !name.contains('/') && name.ends_with(".md"));
            direct_architecture || direct_plan || path.as_str() == "AGENTS.md"
        })
        .cloned()
        .collect::<Vec<_>>();
    let missing_classification = contract_files
        .iter()
        .filter(|path| !paths.contains(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_classification.is_empty() {
        return Err(format!(
            "direct plan/architecture/AGENTS contracts lack classification: {}",
            bounded_list(&missing_classification)
        ));
    }
    sort_all(&mut [
        &mut summary.current,
        &mut summary.target_contracts,
        &mut summary.rewrite_required,
        &mut summary.delete_after_replacement,
        &mut summary.historical,
    ]);
    Ok(summary)
}

fn validate_document_status_notice(entry: &DocumentEntry, text: &str) -> Result<(), String> {
    let expected = match entry.classification {
        DocumentClassification::RewriteRequired => Some("Phase 0 status: rewrite required."),
        DocumentClassification::DeleteAfterReplacement => {
            Some("Phase 0 status: delete after replacement.")
        }
        _ => None,
    };
    let Some(expected) = expected else {
        if entry.status_notice.is_some() {
            return Err(format!(
                "document {} may declare status_notice only for rewrite-required or delete-after-replacement",
                entry.id
            ));
        }
        return Ok(());
    };
    let declared = entry.status_notice.as_deref().ok_or_else(|| {
        format!(
            "document {} requires a machine-checked top-level status_notice",
            entry.id
        )
    })?;
    require_exact(
        declared,
        expected,
        &format!("document {} status_notice", entry.id),
    )?;
    if count_occurrences(text, declared) != 1 {
        return Err(format!(
            "document {} must contain its exact status_notice once",
            entry.id
        ));
    }
    let first_section_end = text.find("\n## ").unwrap_or(text.len());
    let preamble = &text[..first_section_end];
    if !preamble.contains(declared) {
        return Err(format!(
            "document {} status_notice must appear before the first section",
            entry.id
        ));
    }
    for label in [
        "Current executable behavior:",
        "Historical/stale content:",
        "Target-only content:",
        "Flag-day owner:",
    ] {
        if !preamble.contains(label) {
            return Err(format!(
                "document {} top-level status notice is missing {label}",
                entry.id
            ));
        }
    }
    Ok(())
}

fn validate_baselines(
    workspace: &Path,
    manifest: &boon_phase0_baseline::evidence::BaselineEvidenceManifestV2,
    datasets: &DatasetSummary,
) -> Result<BaselineSummary, String> {
    use boon_phase0_baseline::evidence::{
        BaselineAreaStatus, BaselineEvidenceStatus, HistoricalStatus,
    };

    manifest.validate_workspace(workspace)?;
    require_exact(
        &manifest.baseline_source_head,
        CLEAN_BASELINE_HEAD,
        "baseline source head",
    )?;
    require_exact(
        &manifest.baseline_scope,
        "current-runtime-before-goal-flag-days",
        "baseline scope",
    )?;
    require_exact(
        &manifest.compiler.source_head,
        CLEAN_BASELINE_HEAD,
        "compiler baseline source_head",
    )?;
    require_exact(
        &manifest.compiler.workspace_state,
        "clean",
        "compiler baseline workspace_state",
    )?;
    require_exact(
        &manifest.compiler.command,
        "cargo test -p boon_parser -p boon_typecheck -p boon_ir -p boon_compiler --lib",
        "compiler baseline command",
    )?;
    if manifest.compiler.status != HistoricalStatus::Pass {
        return Err("compiler baseline status must be pass".to_owned());
    }
    if manifest.compiler.total_passed != 405 {
        return Err("compiler baseline total_passed must be 405".to_owned());
    }
    const COMPILER_COUNTS: &[(&str, u64)] = &[
        ("boon_parser", 24),
        ("boon_typecheck", 131),
        ("boon_ir", 103),
        ("boon_compiler", 147),
    ];
    validate_exact_ids(
        manifest
            .compiler
            .crates
            .iter()
            .map(|entry| entry.name.as_str()),
        &COMPILER_COUNTS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        "compiler baseline crate",
    )?;
    let mut sum = 0_u64;
    for entry in &manifest.compiler.crates {
        let expected = COMPILER_COUNTS
            .iter()
            .find_map(|(name, count)| (*name == entry.name).then_some(*count))
            .ok_or_else(|| format!("unexpected compiler baseline crate {}", entry.name))?;
        if entry.passed != expected {
            return Err(format!(
                "{} compiler baseline has {} passes; expected {}",
                entry.name, entry.passed, expected
            ));
        }
        sum = sum
            .checked_add(entry.passed)
            .ok_or_else(|| "compiler pass count overflow".to_owned())?;
    }
    if sum != manifest.compiler.total_passed {
        return Err(format!(
            "compiler crate pass sum {sum} differs from total {}",
            manifest.compiler.total_passed
        ));
    }

    const HISTORICAL_REPORTS: &[&str] = &[
        "architecture",
        "cells",
        "counter-dev",
        "negative",
        "novywave",
        "persons-pro",
        "todomvc-physical",
        "verify-all",
    ];
    validate_exact_ids(
        manifest
            .historical_reports
            .iter()
            .map(|entry| entry.id.as_str()),
        HISTORICAL_REPORTS,
        "historical report",
    )?;
    for report in &manifest.historical_reports {
        if report.status != HistoricalStatus::Stale {
            return Err(format!("historical report {} must be stale", report.id));
        }
        let report_path = workspace.join(safe_relative(&report.path)?);
        if !report_path.is_file() {
            return Err(format!(
                "historical report {} path {} is not a file",
                report.id,
                report_path.display()
            ));
        }
        require_source_head(
            &report.source_head,
            &format!("historical report {} source_head", report.id),
        )?;
        require_digest(
            &report.workspace_digest,
            &format!("historical report {} workspace_digest", report.id),
        )?;
        if report.source_head == CLEAN_BASELINE_HEAD {
            return Err(format!(
                "historical report {} must not be attributed to the clean compiler baseline",
                report.id
            ));
        }
        let _ = report.dirty;
    }

    let packed_report_path = workspace.join(safe_relative(&manifest.packed_report)?);
    let packed_bytes = fs::read(&packed_report_path)
        .map_err(|error| format!("{}: {error}", packed_report_path.display()))?;
    if packed_bytes.is_empty()
        || packed_bytes.len() > boon_phase0_baseline::report::MAX_REPORT_BYTES
    {
        return Err(format!(
            "{} is outside the packed report size bound",
            packed_report_path.display()
        ));
    }
    let packed_report: boon_phase0_baseline::BaselineReport = serde_json::from_slice(&packed_bytes)
        .map_err(|error| format!("{}: {error}", packed_report_path.display()))?;
    packed_report
        .validate()
        .map_err(|error| format!("{}: {error}", packed_report_path.display()))?;
    require_exact(
        &packed_report.protocol,
        &manifest.packed_protocol,
        "baseline packed protocol",
    )?;
    let dataset_by_entrypoint = datasets
        .fixtures
        .iter()
        .map(|fixture| (fixture.entrypoint.as_str(), fixture))
        .collect::<HashMap<_, _>>();
    for fixture in &packed_report.fixtures {
        let dataset = dataset_by_entrypoint
            .get(fixture.source_path.as_str())
            .ok_or_else(|| {
                format!(
                    "packed fixture {} source {} has no dataset identity",
                    fixture.id, fixture.source_path
                )
            })?;
        require_exact(
            &fixture.source_bundle_digest_v1,
            &dataset.source_bundle_digest_v1,
            &format!("packed fixture {} dataset digest", fixture.id),
        )?;
    }
    crosscheck_packed_baseline_values(manifest, &packed_report)?;

    let mut summary = BaselineSummary {
        compiler_tests: manifest.compiler.total_passed,
        compiler_crates: manifest
            .compiler
            .crates
            .iter()
            .map(|entry| entry.name.clone())
            .collect(),
        historical_stale_reports: manifest
            .historical_reports
            .iter()
            .map(|entry| entry.id.clone())
            .collect(),
        ..BaselineSummary::default()
    };
    for area in &manifest.areas {
        validate_owner(workspace, &area.owner_phase, &area.owner_plan)?;
        summary.areas.push(area.id.clone());
        match area.status {
            BaselineAreaStatus::Measured => summary.measured_areas.push(area.id.clone()),
            BaselineAreaStatus::Partial => summary.partial_areas.push(area.id.clone()),
        }
        for evidence in &area.evidence {
            let id = format!("{}:{}", area.id, evidence.id);
            match evidence.status {
                BaselineEvidenceStatus::Measured => summary.measured_evidence.push(id),
                BaselineEvidenceStatus::Unavailable => summary.unavailable_evidence.push(id),
                BaselineEvidenceStatus::Stale => summary.stale_evidence.push(id),
                BaselineEvidenceStatus::NotApplicable => summary.not_applicable_evidence.push(id),
            }
        }
    }
    sort_all(&mut [
        &mut summary.compiler_crates,
        &mut summary.historical_stale_reports,
        &mut summary.areas,
        &mut summary.measured_areas,
        &mut summary.partial_areas,
        &mut summary.measured_evidence,
        &mut summary.unavailable_evidence,
        &mut summary.stale_evidence,
        &mut summary.not_applicable_evidence,
    ]);
    Ok(summary)
}

fn crosscheck_packed_baseline_values(
    manifest: &boon_phase0_baseline::evidence::BaselineEvidenceManifestV2,
    report: &boon_phase0_baseline::BaselineReport,
) -> Result<(), String> {
    let fixture = |id: &str| {
        report
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("packed report has no fixture {id}"))
    };
    let action = |fixture_id: &str, action_id: &str| {
        let fixture = fixture(fixture_id)?;
        fixture
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| format!("packed fixture {fixture_id} has no action {action_id}"))
    };
    let measured_metric = |evidence: &boon_phase0_baseline::MetricEvidence, label: &str| {
        if let boon_phase0_baseline::MetricEvidence::Measured { value, .. } = evidence {
            Ok(*value)
        } else {
            Err(format!("{label} is not measured packed evidence"))
        }
    };
    let report_metric = |id: &str| {
        let evidence = report
            .metrics
            .get(id)
            .ok_or_else(|| format!("packed report has no aggregate metric {id}"))?;
        measured_metric(evidence, &format!("packed aggregate metric {id}"))
    };
    let fixture_metric = |fixture_id: &str, metric_id: &str| {
        let fixture = fixture(fixture_id)?;
        let evidence = fixture
            .metrics
            .get(metric_id)
            .ok_or_else(|| format!("packed fixture {fixture_id} has no metric {metric_id}"))?;
        measured_metric(
            evidence,
            &format!("packed fixture {fixture_id} metric {metric_id}"),
        )
    };
    let action_metric = |fixture_id: &str, action_id: &str, metric_id: &str| {
        let action = action(fixture_id, action_id)?;
        let evidence = action.metrics.get(metric_id).ok_or_else(|| {
            format!("packed fixture {fixture_id} action {action_id} has no metric {metric_id}")
        })?;
        measured_metric(
            evidence,
            &format!("packed fixture {fixture_id} action {action_id} metric {metric_id}"),
        )
    };

    let checks = [
        (
            "allocations",
            "declared-action-allocation-count",
            report_metric("allocation-count")?,
        ),
        (
            "allocations",
            "declared-action-allocated-bytes",
            report_metric("allocated-bytes")?,
        ),
        (
            "memory",
            "counter-retained-requested-bytes",
            fixture_metric("counter", "retained-runtime-requested-bytes")?,
        ),
        (
            "memory",
            "todomvc-retained-requested-bytes",
            fixture_metric("todomvc", "retained-runtime-requested-bytes")?,
        ),
        (
            "memory",
            "cells-retained-requested-bytes",
            fixture_metric("cells", "retained-runtime-requested-bytes")?,
        ),
        (
            "memory",
            "fjordpulse-shaped-retained-requested-bytes",
            fixture_metric("fjordpulse-shaped", "retained-runtime-requested-bytes")?,
        ),
        (
            "memory",
            "million-row-retained-requested-bytes",
            fixture_metric("million-row", "retained-runtime-requested-bytes")?,
        ),
        (
            "memory",
            "maximum-current-bytes-per-row",
            report_metric("bytes-per-row")?,
        ),
        (
            "memory",
            "legacy-store-retained-upper-bound",
            report_metric("bytes-per-store")?,
        ),
        (
            "lookup-currentness-work",
            "aggregate-index-work",
            report_metric("index-work")?,
        ),
        (
            "lookup-currentness-work",
            "fjordpulse-shaped-full-scans",
            action("fjordpulse-shaped", "indexed-page")?
                .work_total
                .access_full_scan_count,
        ),
        (
            "lookup-currentness-work",
            "million-row-full-scans",
            action("million-row", "indexed-tail")?
                .work_total
                .access_full_scan_count,
        ),
        (
            "lookup-currentness-work",
            "cells-sparse-charged-work",
            action_metric("cells", "edit-a0", "charged-work")?,
        ),
        (
            "native-wasm",
            "native-headless-actions",
            report
                .fixtures
                .iter()
                .filter(|fixture| {
                    fixture
                        .target_facts
                        .get("native-headless")
                        .is_some_and(|fact| {
                            fact.status == boon_phase0_baseline::FactStatus::Measured
                        })
                })
                .count() as u64,
        ),
        (
            "product-latency",
            "cells-sparse-p95",
            action("cells", "edit-a0")?.latency.p95_ns,
        ),
        (
            "product-latency",
            "fjordpulse-shaped-page-p95",
            action("fjordpulse-shaped", "indexed-page")?.latency.p95_ns,
        ),
        (
            "product-latency",
            "todomvc-dense-p95",
            action("todomvc", "toggle-all")?.latency.p95_ns,
        ),
    ];
    for (area_id, evidence_id, _observed) in checks {
        let evidence = manifest
            .areas
            .iter()
            .find(|area| area.id == area_id)
            .and_then(|area| {
                area.evidence
                    .iter()
                    .find(|evidence| evidence.id == evidence_id)
            })
            .ok_or_else(|| format!("baseline evidence {area_id}:{evidence_id} is missing"))?;
        let expected_source = match (area_id, evidence_id) {
            ("allocations", "declared-action-allocation-count") => {
                "target/reports/phase0-v1/packed-baseline.json#metrics.allocation-count"
            }
            ("allocations", "declared-action-allocated-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#metrics.allocated-bytes"
            }
            ("memory", "counter-retained-requested-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.counter.metrics.retained-runtime-requested-bytes"
            }
            ("memory", "todomvc-retained-requested-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.todomvc.metrics.retained-runtime-requested-bytes"
            }
            ("memory", "cells-retained-requested-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.cells.metrics.retained-runtime-requested-bytes"
            }
            ("memory", "fjordpulse-shaped-retained-requested-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.fjordpulse-shaped.metrics.retained-runtime-requested-bytes"
            }
            ("memory", "million-row-retained-requested-bytes") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.million-row.metrics.retained-runtime-requested-bytes"
            }
            ("memory", "maximum-current-bytes-per-row") => {
                "target/reports/phase0-v1/packed-baseline.json#metrics.bytes-per-row"
            }
            ("memory", "legacy-store-retained-upper-bound") => {
                "target/reports/phase0-v1/packed-baseline.json#metrics.bytes-per-store"
            }
            ("lookup-currentness-work", "aggregate-index-work") => {
                "target/reports/phase0-v1/packed-baseline.json#metrics.index-work"
            }
            ("lookup-currentness-work", "fjordpulse-shaped-full-scans") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.fjordpulse-shaped.actions.indexed-page.work-total.access-full-scan-count"
            }
            ("lookup-currentness-work", "million-row-full-scans") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.million-row.actions.indexed-tail.work-total.access-full-scan-count"
            }
            ("lookup-currentness-work", "cells-sparse-charged-work") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.cells.actions.edit-a0.metrics.charged-work"
            }
            ("native-wasm", "native-headless-actions") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.*.target-facts.native-headless"
            }
            ("product-latency", "cells-sparse-p95") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.cells.actions.edit-a0.latency.p95-ns"
            }
            ("product-latency", "fjordpulse-shaped-page-p95") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.fjordpulse-shaped.actions.indexed-page.latency.p95-ns"
            }
            ("product-latency", "todomvc-dense-p95") => {
                "target/reports/phase0-v1/packed-baseline.json#fixtures.todomvc.actions.toggle-all.latency.p95-ns"
            }
            _ => {
                return Err(format!(
                    "unexpected packed baseline link {area_id}:{evidence_id}"
                ));
            }
        };
        require_exact(
            &evidence.source,
            expected_source,
            &format!("baseline evidence {area_id}:{evidence_id} source"),
        )?;
        if evidence.value_u64.is_some() || evidence.value_text.is_some() {
            return Err(format!(
                "baseline evidence {area_id}:{evidence_id} duplicates a generated report value"
            ));
        }
    }
    Ok(())
}

fn validate_packed_budget(
    workspace: &Path,
    manifest: &PackedBudgetManifest,
) -> Result<PackedSummary, String> {
    if manifest.format_version != 3 {
        return Err(format!(
            "{PACKED_BUDGET_MANIFEST} format_version is {}; expected 3",
            manifest.format_version
        ));
    }
    require_exact(
        &manifest.baseline_source_head,
        CLEAN_BASELINE_HEAD,
        "packed budget baseline",
    )?;
    require_exact(
        &manifest.baseline_scope,
        "current-worktree",
        "packed budget baseline_scope",
    )?;
    require_exact(
        &manifest.owner_phase,
        "goal-phase-5-packed",
        "packed budget owner_phase",
    )?;
    validate_owner(workspace, &manifest.owner_phase, &manifest.owner_plan)?;
    if manifest.schema_status != MeasurementStatus::CurrentRuntimeMeasured {
        return Err("packed budget schema_status must be current-runtime-measured".to_owned());
    }
    let report_relative = safe_relative(&manifest.current_runtime_report)?;
    let fixture_relative = safe_relative(&manifest.fixture_manifest)?;
    require_exact(
        &manifest.current_runtime_report,
        boon_phase0_baseline::report::DEFAULT_REPORT,
        "packed current_runtime_report",
    )?;
    require_exact(
        &manifest.fixture_manifest,
        boon_phase0_baseline::report::DEFAULT_MANIFEST,
        "packed fixture_manifest",
    )?;
    let report =
        crate::packed_baseline::validate_existing(workspace, &workspace.join(report_relative))
            .map_err(|error| error.to_string())?;
    if report.fixture_manifest_path != fixture_relative.display().to_string() {
        return Err("packed report is not bound to the declared fixture manifest".to_owned());
    }
    require_exact(
        &manifest.protocol.name,
        &report.protocol,
        "packed protocol name",
    )?;
    require_exact(
        &manifest.protocol.target_profile,
        &report.target_profile,
        "packed target_profile",
    )?;
    require_exact(
        &manifest.protocol.build_profile,
        &report.build_profile,
        "packed build_profile",
    )?;
    require_exact(
        &manifest.protocol.allocator_scope,
        &report.allocator_scope,
        "packed allocator_scope",
    )?;
    if manifest.protocol.allocator_instrumentation_status
        != MeasurementStatus::CurrentRuntimeMeasured
        || manifest.protocol.warmup_status != MeasurementStatus::CurrentRuntimeMeasured
        || manifest.protocol.measured_turn_interval_status
            != MeasurementStatus::CurrentRuntimeMeasured
    {
        return Err(
            "packed allocator, warmup, and measured intervals must be current-runtime-measured"
                .to_owned(),
        );
    }
    if manifest.protocol.warmup_turns.is_some() || manifest.protocol.measured_turns.is_some() {
        return Err(
            "packed protocol must not invent one global sample count; exact counts are per action in the report"
                .to_owned(),
        );
    }

    const BASELINES: &[&str] = &[
        "counter",
        "todomvc",
        "cells",
        "fjordpulse-shaped",
        "million-row",
    ];
    validate_exact_ids(
        manifest.baselines.iter().map(|entry| entry.id.as_str()),
        BASELINES,
        "packed baseline fixture",
    )?;
    for baseline in &manifest.baselines {
        if baseline.status != MeasurementStatus::CurrentRuntimeMeasured {
            return Err(format!(
                "packed baseline {} must be current-runtime-measured",
                baseline.id
            ));
        }
        require_nonempty(
            &baseline.owner,
            &format!("packed baseline {} owner", baseline.id),
        )?;
        require_nonempty(
            &baseline.owner_action,
            &format!("packed baseline {} owner_action", baseline.id),
        )?;
        if baseline.measured_report.as_deref() != Some(manifest.current_runtime_report.as_str()) {
            return Err(format!(
                "packed baseline {} is not attributed to the current-runtime report",
                baseline.id
            ));
        }
        if !report
            .fixtures
            .iter()
            .any(|fixture| fixture.id == baseline.id)
        {
            return Err(format!(
                "packed report does not contain fixture {}",
                baseline.id
            ));
        }
    }

    const METRICS: &[&str] = &[
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
    validate_exact_ids(
        manifest.metrics.iter().map(|entry| entry.id.as_str()),
        METRICS,
        "packed metric",
    )?;
    let mut measured_metrics = Vec::new();
    let mut unavailable_metrics = Vec::new();
    let mut not_applicable_metrics = Vec::new();
    let mut budgeted_metrics = Vec::new();
    for metric in &manifest.metrics {
        let evidence = report
            .metrics
            .get(&metric.id)
            .ok_or_else(|| format!("packed report omits metric {}", metric.id))?;
        let expected_status = match evidence {
            boon_phase0_baseline::MetricEvidence::Measured { .. } => {
                measured_metrics.push(metric.id.clone());
                MeasurementStatus::CurrentRuntimeMeasured
            }
            boon_phase0_baseline::MetricEvidence::Unavailable { .. } => {
                unavailable_metrics.push(metric.id.clone());
                MeasurementStatus::Unavailable
            }
            boon_phase0_baseline::MetricEvidence::NotApplicable { .. } => {
                not_applicable_metrics.push(metric.id.clone());
                MeasurementStatus::NotApplicable
            }
        };
        if metric.status != expected_status {
            return Err(format!(
                "packed metric {} status {:?} differs from report evidence {:?}",
                metric.id, metric.status, expected_status
            ));
        }
        require_exact(
            &metric.unit,
            packed_metric_unit_name(evidence),
            &format!("packed metric {} unit", metric.id),
        )?;
        require_nonempty(&metric.owner, &format!("packed metric {} owner", metric.id))?;
        require_nonempty(
            &metric.owner_action,
            &format!("packed metric {} owner_action", metric.id),
        )?;
        validate_metric_budget(metric, evidence)?;
        budgeted_metrics.push(metric.id.clone());
    }

    let expected_action_ids = report
        .fixtures
        .iter()
        .flat_map(|fixture| {
            fixture
                .actions
                .iter()
                .map(|action| format!("{}/{}", fixture.id, action.id))
        })
        .collect::<HashSet<_>>();
    let action_ids = manifest
        .action_ratchets
        .iter()
        .map(|ratchet| format!("{}/{}", ratchet.fixture, ratchet.action))
        .collect::<HashSet<_>>();
    if action_ids != expected_action_ids || action_ids.len() != manifest.action_ratchets.len() {
        return Err(format!(
            "packed action ratchet IDs differ: actual={action_ids:?}, expected={expected_action_ids:?}"
        ));
    }
    for ratchet in &manifest.action_ratchets {
        let fixture = report
            .fixtures
            .iter()
            .find(|fixture| fixture.id == ratchet.fixture)
            .ok_or_else(|| {
                format!(
                    "packed action ratchet fixture {} is absent",
                    ratchet.fixture
                )
            })?;
        let action = fixture
            .actions
            .iter()
            .find(|action| action.id == ratchet.action)
            .ok_or_else(|| {
                format!(
                    "packed action ratchet {}/{} is absent",
                    ratchet.fixture, ratchet.action
                )
            })?;
        validate_action_ratchet(&ratchet.fixture, action, ratchet)?;
    }

    let mut summary = PackedSummary {
        schema_status: "current-runtime-measured".to_owned(),
        report_path: manifest.current_runtime_report.clone(),
        current_runtime_baselines: manifest
            .baselines
            .iter()
            .map(|entry| entry.id.clone())
            .collect(),
        measured_metrics,
        unavailable_metrics,
        not_applicable_metrics,
        budgeted_metrics,
        action_ratchets: action_ids.into_iter().collect(),
    };
    sort_all(&mut [
        &mut summary.current_runtime_baselines,
        &mut summary.measured_metrics,
        &mut summary.unavailable_metrics,
        &mut summary.not_applicable_metrics,
        &mut summary.budgeted_metrics,
        &mut summary.action_ratchets,
    ]);
    Ok(summary)
}

fn validate_metric_budget(
    metric: &PackedMetric,
    evidence: &boon_phase0_baseline::MetricEvidence,
) -> Result<(), String> {
    use boon_phase0_baseline::MetricEvidence;

    let label = format!("packed metric {}", metric.id);
    match evidence {
        MetricEvidence::Measured { value, .. } => {
            let baseline = metric
                .baseline_u64
                .ok_or_else(|| format!("{label} has no integer ratchet baseline"))?;
            let current_limit = metric
                .current_limit_u64
                .ok_or_else(|| format!("{label} has no current-runtime limit"))?;
            let allowed_regression_bps = metric
                .allowed_regression_bps
                .ok_or_else(|| format!("{label} has no allowed regression ratchet"))?;
            validate_numeric_ratchet(
                &label,
                metric.direction,
                baseline,
                current_limit,
                metric.target_limit_u64,
                allowed_regression_bps,
                *value,
            )
        }
        MetricEvidence::Unavailable { .. } | MetricEvidence::NotApplicable { .. } => {
            if metric.baseline_u64.is_some()
                || metric.current_limit_u64.is_some()
                || metric.allowed_regression_bps.is_some()
            {
                return Err(format!(
                    "{label} has no current measurement but declares a current ratchet"
                ));
            }
            Ok(())
        }
    }
}

fn validate_numeric_ratchet(
    label: &str,
    direction: PackedBudgetDirection,
    baseline: u64,
    current_limit: u64,
    target_limit: u64,
    allowed_regression_bps: u16,
    observed: u64,
) -> Result<(), String> {
    if allowed_regression_bps > 5_000 {
        return Err(format!(
            "{label} allows {allowed_regression_bps} basis points of regression; maximum is 5000"
        ));
    }
    let baseline = u128::from(baseline);
    let current_limit = u128::from(current_limit);
    let target_limit = u128::from(target_limit);
    let observed = u128::from(observed);
    let regression_budget = baseline.saturating_mul(u128::from(allowed_regression_bps));
    match direction {
        PackedBudgetDirection::Max => {
            if current_limit < baseline
                || current_limit
                    .saturating_sub(baseline)
                    .saturating_mul(10_000)
                    > regression_budget
            {
                return Err(format!(
                    "{label} current maximum is not within its baseline regression ratchet"
                ));
            }
            if observed > current_limit {
                return Err(format!(
                    "{label} observed {observed} exceeds current maximum {current_limit}"
                ));
            }
            if target_limit > current_limit {
                return Err(format!(
                    "{label} final maximum {target_limit} is weaker than current maximum {current_limit}"
                ));
            }
        }
        PackedBudgetDirection::Min => {
            if current_limit > baseline
                || baseline
                    .saturating_sub(current_limit)
                    .saturating_mul(10_000)
                    > regression_budget
            {
                return Err(format!(
                    "{label} current minimum is not within its baseline regression ratchet"
                ));
            }
            if observed < current_limit {
                return Err(format!(
                    "{label} observed {observed} is below current minimum {current_limit}"
                ));
            }
            if target_limit < current_limit {
                return Err(format!(
                    "{label} final minimum {target_limit} is weaker than current minimum {current_limit}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_action_ratchet(
    fixture: &str,
    action: &boon_phase0_baseline::ActionReport,
    ratchet: &PackedActionRatchet,
) -> Result<(), String> {
    let metric = |id: &str| {
        action
            .metrics
            .get(id)
            .ok_or_else(|| format!("packed action {fixture}/{} omits metric {id}", action.id))
    };
    validate_action_metric(
        fixture,
        &action.id,
        "allocation-count",
        PackedBudgetDirection::Max,
        metric("allocation-count")?,
        &ratchet.allocation_count,
    )?;
    validate_action_metric(
        fixture,
        &action.id,
        "allocated-bytes",
        PackedBudgetDirection::Max,
        metric("allocated-bytes")?,
        &ratchet.allocated_bytes,
    )?;
    validate_action_metric(
        fixture,
        &action.id,
        "latency-p95",
        PackedBudgetDirection::Max,
        metric("latency-p95")?,
        &ratchet.latency_p95,
    )?;
    validate_action_metric(
        fixture,
        &action.id,
        "charged-work",
        PackedBudgetDirection::Max,
        metric("charged-work")?,
        &ratchet.charged_work,
    )?;
    validate_action_metric(
        fixture,
        &action.id,
        "index-work",
        PackedBudgetDirection::Max,
        metric("index-work")?,
        &ratchet.index_work,
    )?;
    if !matches!(
        ratchet.throughput_metric.as_str(),
        "rows-per-second" | "turns-per-second"
    ) {
        return Err(format!(
            "packed action {fixture}/{} has unsupported throughput metric {}",
            action.id, ratchet.throughput_metric
        ));
    }
    validate_action_metric(
        fixture,
        &action.id,
        &ratchet.throughput_metric,
        PackedBudgetDirection::Min,
        metric(&ratchet.throughput_metric)?,
        &ratchet.throughput,
    )
}

fn validate_action_metric(
    fixture: &str,
    action: &str,
    metric_id: &str,
    direction: PackedBudgetDirection,
    evidence: &boon_phase0_baseline::MetricEvidence,
    ratchet: &PackedNumericRatchet,
) -> Result<(), String> {
    let label = format!("packed action {fixture}/{action} metric {metric_id}");
    if ratchet.direction != direction {
        return Err(format!("{label} has the wrong budget direction"));
    }
    require_exact(
        &ratchet.unit,
        packed_metric_unit_name(evidence),
        &format!("{label} unit"),
    )?;
    let boon_phase0_baseline::MetricEvidence::Measured { value, .. } = evidence else {
        return Err(format!("{label} must be measured"));
    };
    validate_numeric_ratchet(
        &label,
        ratchet.direction,
        ratchet.baseline_u64,
        ratchet.current_limit_u64,
        ratchet.target_limit_u64,
        ratchet.allowed_regression_bps,
        *value,
    )
}

fn packed_metric_unit_name(evidence: &boon_phase0_baseline::MetricEvidence) -> &'static str {
    use boon_phase0_baseline::{MetricEvidence, MetricUnit};

    let unit = match evidence {
        MetricEvidence::Measured { unit, .. }
        | MetricEvidence::Unavailable { unit, .. }
        | MetricEvidence::NotApplicable { unit, .. } => *unit,
    };
    match unit {
        MetricUnit::Allocations => "allocations",
        MetricUnit::Bytes => "bytes",
        MetricUnit::BytesPerRow => "bytes-per-row",
        MetricUnit::Items => "items",
        MetricUnit::Nanoseconds => "nanoseconds",
        MetricUnit::RowsPerSecondMilli => "rows-per-second-milli",
        MetricUnit::TurnsPerSecondMilli => "turns-per-second-milli",
        MetricUnit::WorkUnits => "work-units",
    }
}

fn inspect_reports(
    workspace: &Path,
    source: &SourceIdentity,
    policy: &ReportIdentityPolicy,
) -> Result<ReportInventory, String> {
    validate_report_policy(policy)?;
    let directory = workspace.join(safe_relative(&policy.inspect_directory)?);
    if !directory.exists() {
        return Ok(ReportInventory::default());
    }
    if !directory.is_dir() {
        return Err(format!(
            "report inspection path {} is not a directory",
            directory.display()
        ));
    }
    let mut paths = fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    if paths.len() > policy.max_report_files {
        return Err(format!(
            "{} report files exceed policy maximum {}",
            paths.len(),
            policy.max_report_files
        ));
    }
    let mut inventory = ReportInventory {
        inspected: paths.len(),
        ..ReportInventory::default()
    };
    for path in paths {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                inventory.unidentifiable += 1;
                continue;
            }
        };
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > policy.max_report_bytes {
            inventory.unidentifiable += 1;
            continue;
        }
        let value = match fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<JsonValue>(&bytes).ok())
        {
            Some(value) => value,
            None => {
                inventory.unidentifiable += 1;
                continue;
            }
        };
        let head = value
            .pointer("/identity/source/head")
            .and_then(JsonValue::as_str);
        let digest = value
            .pointer("/identity/source/workspace_digest")
            .and_then(JsonValue::as_str);
        let dirty = value
            .pointer("/identity/source/dirty")
            .and_then(JsonValue::as_bool);
        let (Some(head), Some(digest), Some(dirty)) = (head, digest, dirty) else {
            inventory.unidentifiable += 1;
            continue;
        };
        if head == source.head.as_str() && digest == source.workspace_digest.as_str() {
            if dirty {
                inventory.dirty_rejected += 1;
            } else {
                inventory.current_clean += 1;
            }
        } else {
            inventory.stale += 1;
        }
    }
    Ok(inventory)
}

#[allow(clippy::too_many_arguments)]
fn completion_open_items(
    source: &SourceIdentity,
    containers: &ContainerSummary,
    fixtures: &FixtureSummary,
    datasets: &DatasetSummary,
    baselines: &BaselineSummary,
) -> Vec<String> {
    let mut items = Vec::new();
    if source.dirty {
        items.push(
            "current worktree is dirty; final Phase 0 evidence requires a clean committed revision"
                .to_owned(),
        );
    }
    if fixtures.existing.len() != boon_phase0_baseline::evidence::REQUIRED_FIXTURE_IDS.len() {
        items.push(format!(
            "only {} of {} required executable fixture baselines validated",
            fixtures.existing.len(),
            boon_phase0_baseline::evidence::REQUIRED_FIXTURE_IDS.len()
        ));
    }
    if containers.within_file_classification != "complete" {
        items.push(
            "container occurrence classification is not complete with fresh one-to-one per-use coverage"
                .to_owned(),
        );
    }
    if datasets.fixtures.is_empty()
        || datasets.categories.len()
            != boon_phase0_baseline::dataset::REQUIRED_DATASET_CATEGORIES.len()
    {
        items.push(format!(
            "canonical dataset identity coverage is incomplete: {} fixtures across {} categories",
            datasets.fixtures.len(),
            datasets.categories.len()
        ));
    }
    if baselines.areas.len() != boon_phase0_baseline::evidence::REQUIRED_BASELINE_AREA_IDS.len() {
        items.push(format!(
            "only {} of {} required baseline areas have structured evidence",
            baselines.areas.len(),
            boon_phase0_baseline::evidence::REQUIRED_BASELINE_AREA_IDS.len()
        ));
    }
    items
        .into_iter()
        .map(|item| bounded_text(&item, 700))
        .collect()
}

fn load_toml<T: DeserializeOwned>(
    workspace: &Path,
    relative: &str,
) -> Result<(T, ManifestEvidence), String> {
    let relative_path = safe_relative(relative)?;
    let path = workspace.join(relative_path);
    let metadata = fs::metadata(&path).map_err(|error| format!("{relative}: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "{relative} is {} bytes; expected a regular file in 1..={MAX_MANIFEST_BYTES}",
            metadata.len()
        ));
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{relative}: {error}"))?;
    let manifest = toml::from_str(&text).map_err(|error| format!("{relative}: {error}"))?;
    let digest = sha256_file(&path)
        .map_err(|error| format!("{relative}: {error}"))?
        .as_str()
        .to_owned();
    Ok((
        manifest,
        ManifestEvidence {
            path: relative.to_owned(),
            sha256: digest,
            bytes: metadata.len(),
        },
    ))
}

fn file_evidence(
    workspace: &Path,
    relative: &str,
    maximum_bytes: u64,
) -> Result<ManifestEvidence, String> {
    let relative_path = safe_relative(relative)?;
    let path = workspace.join(relative_path);
    let metadata = fs::metadata(&path).map_err(|error| format!("{relative}: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum_bytes {
        return Err(format!(
            "{relative} is {} bytes; expected a regular file in 1..={maximum_bytes}",
            metadata.len()
        ));
    }
    let digest = sha256_file(&path)
        .map_err(|error| format!("{relative}: {error}"))?
        .as_str()
        .to_owned();
    Ok(ManifestEvidence {
        path: relative.to_owned(),
        sha256: digest,
        bytes: metadata.len(),
    })
}

fn workspace_files(workspace: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])
        .output()
        .map_err(|error| format!("run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn write_report(path: &Path, report: &Phase0Report, byte_limit: u64) -> ToolResult<()> {
    if byte_limit == 0 || byte_limit > DEFAULT_MAX_OUTPUT_BYTES {
        return Err(format!("invalid Phase 0 report byte limit {byte_limit}").into());
    }
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > byte_limit {
        return Err(format!(
            "Phase 0 report is {} bytes; limit is {byte_limit}",
            bytes.len()
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("phase0-tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn push_check(checks: &mut Vec<Phase0Check>, id: &str, result: Result<String, String>) {
    let (outcome, detail) = match result {
        Ok(detail) => (CheckOutcome::Pass, detail),
        Err(detail) => (CheckOutcome::Fail, detail),
    };
    checks.push(Phase0Check {
        id: id.to_owned(),
        outcome,
        detail: bounded_text(&detail, MAX_CHECK_DETAIL_BYTES),
    });
}

fn validate_owner(workspace: &Path, phase: &str, plan: &str) -> Result<(), String> {
    require_nonempty(phase, "owner_phase")?;
    let relative = safe_relative(plan)?;
    if !workspace.join(relative).is_file() {
        return Err(format!("owner plan {plan} does not exist"));
    }
    Ok(())
}

fn validate_exact_ids<'a>(
    actual: impl Iterator<Item = &'a str>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let mut actual = actual.map(str::to_owned).collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual != expected {
        let actual_set = actual.iter().collect::<HashSet<_>>();
        let expected_set = expected.iter().collect::<HashSet<_>>();
        let missing = expected
            .iter()
            .filter(|value| !actual_set.contains(value))
            .cloned()
            .collect::<Vec<_>>();
        let unexpected = actual
            .iter()
            .filter(|value| !expected_set.contains(value))
            .cloned()
            .collect::<Vec<_>>();
        return Err(format!(
            "{label} inventory mismatch; missing {}; unexpected {}; duplicate count {}",
            bounded_list(&missing),
            bounded_list(&unexpected),
            actual.len().saturating_sub(actual_set.len())
        ));
    }
    Ok(())
}

fn require_format(observed: u16, path: &str) -> Result<(), String> {
    if observed != FORMAT_VERSION {
        return Err(format!(
            "{path} format_version is {observed}; expected {FORMAT_VERSION}"
        ));
    }
    Ok(())
}

fn require_source_head(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be 40 lowercase hexadecimal bytes"));
    }
    Ok(())
}

fn require_digest(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be 64 lowercase hexadecimal bytes"));
    }
    Ok(())
}

fn require_exact(value: &str, expected: &str, label: &str) -> Result<(), String> {
    if value != expected {
        return Err(format!("{label} is {value:?}; expected {expected:?}"));
    }
    Ok(())
}

fn require_nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.len() > 1024 {
        return Err(format!("{label} exceeds 1024 bytes"));
    }
    Ok(())
}

fn safe_relative(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "path {} must be nonempty and workspace-relative",
            path.display()
        ));
    }
    Ok(path)
}

fn read_bounded_text(path: &Path, limit: u64) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!(
            "{} must be a regular text file no larger than {limit} bytes",
            path.display()
        ));
    }
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn path_matches_root(path: &str, root: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn path_has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|value| value == extension)
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.match_indices(needle).count()
}

fn count_rust_token_occurrences(haystack: &str, needle: &str) -> usize {
    identifier_word_offsets(haystack, needle).len()
}

fn count_rust_code_token_occurrences(haystack: &str, needle: &str) -> usize {
    let structural = rust_structural_bytes(haystack);
    identifier_word_offsets(haystack, needle)
        .into_iter()
        .filter(|offset| structural[*offset..].starts_with(needle.as_bytes()))
        .count()
}

fn count_scan_occurrences(text: &str, scan: &OccurrenceScan) -> Result<usize, String> {
    match scan.mode {
        OccurrenceScanMode::Text if scan.line_contains_any.is_empty() => {
            Ok(count_occurrences(text, &scan.needle))
        }
        OccurrenceScanMode::Text => Ok(text
            .lines()
            .filter(|line| {
                scan.line_contains_any
                    .iter()
                    .any(|marker| line.contains(marker))
            })
            .map(|line| count_occurrences(line, &scan.needle))
            .sum()),
        OccurrenceScanMode::BoonCallArgument => Ok(count_boon_call_argument_occurrences(
            text,
            &scan.call_functions,
            &scan.needle,
        )),
        OccurrenceScanMode::JsonSourceBundleText => {
            let sources = json_source_bundle_sources(text)?;
            Ok(sources
                .into_iter()
                .map(|source| count_occurrences(&source, &scan.needle))
                .sum())
        }
        OccurrenceScanMode::JsonSourceBundleBoonCallArgument => {
            let sources = json_source_bundle_sources(text)?;
            Ok(sources
                .into_iter()
                .map(|source| {
                    count_boon_call_argument_occurrences(
                        &source,
                        &scan.call_functions,
                        &scan.needle,
                    )
                })
                .sum())
        }
        OccurrenceScanMode::RustToken => Ok(count_rust_token_occurrences(text, &scan.needle)),
        OccurrenceScanMode::RustCodeToken => {
            Ok(count_rust_code_token_occurrences(text, &scan.needle))
        }
    }
}

fn json_source_bundle_sources(text: &str) -> Result<Vec<String>, String> {
    let value = serde_json::from_str::<JsonValue>(text)
        .map_err(|error| format!("invalid source-bundle JSON: {error}"))?;
    let units = value
        .as_object()
        .and_then(|object| object.get("units"))
        .and_then(JsonValue::as_array)
        .ok_or_else(|| "source-bundle JSON must contain a units array".to_owned())?;
    if units.is_empty() {
        return Err("source-bundle JSON units array must not be empty".to_owned());
    }
    units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            unit.as_object()
                .and_then(|object| object.get("source"))
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("source-bundle JSON units[{index}].source must be a string"))
        })
        .collect()
}

fn is_boon_argument_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_boon_function_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'/')
}

fn is_boon_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn count_boon_call_argument_occurrences(text: &str, functions: &[String], argument: &str) -> usize {
    functions
        .iter()
        .map(|function| count_boon_call_argument_for_function(text, function, argument))
        .sum()
}

fn count_boon_call_argument_for_function(text: &str, function: &str, argument: &str) -> usize {
    let bytes = text.as_bytes();
    let mut search_from = 0;
    let mut occurrences = 0;
    while search_from < text.len() {
        let Some(relative) = text[search_from..].find(function) else {
            break;
        };
        let start = search_from + relative;
        let end = start + function.len();
        search_from = end;
        if start > 0 && is_boon_function_name_byte(bytes[start - 1]) {
            continue;
        }
        let mut open = end;
        while open < bytes.len() && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if bytes.get(open) != Some(&b'(') {
            continue;
        }
        occurrences += count_named_argument_in_balanced_call(bytes, open, argument.as_bytes());
    }
    occurrences
}

fn count_named_argument_in_balanced_call(bytes: &[u8], open: usize, argument: &[u8]) -> usize {
    let mut parentheses = 1_usize;
    let mut brackets = 0_usize;
    let mut braces = 0_usize;
    let mut offset = open + 1;
    let mut occurrences = 0;
    while offset < bytes.len() {
        if brackets == 0
            && braces == 0
            && (bytes[offset..].starts_with(b"--") || bytes[offset..].starts_with(b"//"))
        {
            offset = bytes[offset..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |newline| offset + newline + 1);
            continue;
        }
        if parentheses == 1
            && brackets == 0
            && braces == 0
            && bytes[offset..].starts_with(argument)
            && (offset == open + 1 || !is_boon_identifier_byte(bytes[offset - 1]))
        {
            let mut colon = offset + argument.len();
            while colon < bytes.len() && bytes[colon].is_ascii_whitespace() {
                colon += 1;
            }
            if bytes.get(colon) == Some(&b':') {
                occurrences += 1;
                offset = colon + 1;
                continue;
            }
        }
        match bytes[offset] {
            b'{' => braces += 1,
            b'}' if braces > 0 => braces -= 1,
            _ if braces > 0 => {}
            b'[' => brackets += 1,
            b']' if brackets > 0 => brackets -= 1,
            _ if brackets > 0 => {}
            b'(' => parentheses += 1,
            b')' => {
                parentheses -= 1;
                if parentheses == 0 {
                    break;
                }
            }
            _ => {}
        }
        offset += 1;
    }
    occurrences
}

#[cfg(test)]
fn count_identifier_word(haystack: &str, needle: &str) -> usize {
    identifier_word_offsets(haystack, needle).len()
}

fn identifier_word_offsets(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut offsets = Vec::new();
    let mut offset = 0;
    while offset + needle_bytes.len() <= bytes.len() {
        let Some(relative) = haystack[offset..].find(needle) else {
            break;
        };
        let start = offset + relative;
        let end = start + needle_bytes.len();
        let before_is_identifier = start > 0 && is_rust_identifier_byte(bytes[start - 1]);
        let after_is_identifier = end < bytes.len() && is_rust_identifier_byte(bytes[end]);
        if !before_is_identifier && !after_is_identifier {
            offsets.push(start);
        }
        offset = end;
    }
    offsets
}

fn is_rust_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn bounded_list(values: &[String]) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    let mut result = values
        .iter()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > 12 {
        result.push_str(&format!(", and {} more", values.len() - 12));
    }
    bounded_text(&result, 700)
}

fn bounded_text(value: &str, maximum: usize) -> String {
    let value = value.replace('\n', "\\n");
    if value.len() <= maximum {
        return value;
    }
    let mut result = String::new();
    for character in value.chars() {
        if result.len() + character.len_utf8() + 3 > maximum {
            break;
        }
        result.push(character);
    }
    result.push_str("...");
    result
}

fn sort_all(values: &mut [&mut Vec<String>]) {
    for value in values {
        value.sort();
    }
}

fn status_name(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Pass => "pass",
        ReportStatus::Fail => "fail",
    }
}

fn completion_name(state: CompletionState) -> &'static str {
    match state {
        CompletionState::Complete => "complete",
        CompletionState::Incomplete => "incomplete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_classification_rejects_category_and_reason_tampering() {
        let construct = concat!("BTree", "Map");
        let source_text = format!("use std::collections::{construct};\n");
        let source_line = source_text.trim_end();
        let identity = ContainerOccurrenceIdentity {
            path: "crates/boon_plan_executor/src/classification_probe.rs".to_owned(),
            line: 1,
            byte_column: source_line.find(construct).unwrap() + 1,
            ordinal: 1,
            construct: construct.to_owned(),
            context_sha256: sha256_bytes(source_line.as_bytes()).as_str().to_owned(),
        };
        let source = container_occurrence_semantic_source(&source_text);
        let mut occurrence = ClassifiedContainerOccurrence {
            identity,
            category: "name-lookup".to_owned(),
            reason: "name-or-path-key-context".to_owned(),
        };
        assert!(validate_container_occurrence_classification(&occurrence, &source).is_err());

        occurrence.category = "dense-id-table".to_owned();
        occurrence.reason = "engine-id-table-context".to_owned();
        assert!(validate_container_occurrence_classification(&occurrence, &source).is_err());

        occurrence.reason = "engine-container-import".to_owned();
        assert!(validate_container_occurrence_classification(&occurrence, &source).is_ok());
    }

    #[test]
    #[ignore = "writes the checked-in exact container occurrence ledger"]
    fn regenerate_container_occurrence_ledger() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let rows = generate_container_occurrence_ledger(&workspace).unwrap();
        assert!(rows > 0);
    }

    #[test]
    #[ignore = "updates exact deletion occurrence and per-path expectations"]
    fn regenerate_deletion_scan_expectations_from_current_worktree() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let scans = regenerate_deletion_scan_expectations(&workspace).unwrap();
        assert!(scans > 0);
    }

    #[test]
    #[ignore = "prints exact per-path Boon call-argument baselines"]
    fn print_boon_call_argument_path_baselines() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (manifest, _) = load_toml::<DeletionManifest>(&workspace, DELETION_MANIFEST).unwrap();
        let workspace_files = workspace_files(&workspace).unwrap();
        for entry in &manifest.entries {
            for scan in &entry.scans {
                if scan.mode != OccurrenceScanMode::BoonCallArgument {
                    continue;
                }
                let mut counts = Vec::new();
                for relative in &workspace_files {
                    if scan.excluded_paths.contains(relative)
                        || !scan
                            .roots
                            .iter()
                            .any(|root| path_matches_root(relative, root))
                        || !scan
                            .extensions
                            .iter()
                            .any(|extension| path_has_extension(relative, extension))
                    {
                        continue;
                    }
                    let text =
                        read_bounded_text(&workspace.join(relative), MAX_TEXT_PROBE_BYTES).unwrap();
                    let count = count_scan_occurrences(&text, scan).unwrap();
                    if count > 0 {
                        counts.push((relative, count));
                    }
                }
                println!("{} {:?} {:?}", entry.id, scan.needle, scan.call_functions);
                for (path, count) in counts {
                    println!("{path} = {count}");
                }
            }
        }
    }

    #[test]
    fn occurrence_count_is_non_overlapping_and_exact() {
        assert_eq!(count_occurrences("aaaa", "aa"), 2);
        assert_eq!(count_occurrences("alpha beta alpha", "alpha"), 2);
        assert_eq!(count_occurrences("alpha", ""), 0);
    }

    #[test]
    fn rust_token_occurrences_require_identifier_boundaries() {
        let needle = concat!("Value", "::Bool");
        let source = concat!(
            "Value",
            "::Bool EvalValue::Bool Value::Boolean (",
            "Value",
            "::Bool)"
        );
        assert_eq!(count_rust_token_occurrences(source, needle), 2);
    }

    #[test]
    fn rust_code_token_occurrences_ignore_comments_and_literals() {
        let source = "let _: Type::Variant; // Type::Variant\nlet _ = \"Type::Variant\";\n";
        assert_eq!(
            count_rust_code_token_occurrences(source, "Type::Variant"),
            1
        );
    }

    #[test]
    fn line_filtered_occurrences_exclude_unrelated_protocol_offsets() {
        let scan = OccurrenceScan {
            roots: vec!["examples".to_owned()],
            extensions: vec!["bn".to_owned()],
            mode: OccurrenceScanMode::Text,
            needle: "offset: 0".to_owned(),
            call_functions: Vec::new(),
            expected_occurrences: 1,
            expected_files: 1,
            expected_path_occurrences: HashMap::new(),
            line_contains_any: vec![
                "List/".to_owned(),
                "Text/".to_owned(),
                "Bytes/".to_owned(),
                "Bits/".to_owned(),
            ],
            excluded_paths: Vec::new(),
            allowed_paths: Vec::new(),
        };
        let source = "\
Wellen/hierarchy_page(artifact: artifact, offset: 0, limit: 32)
bytes |> Bytes/read_unsigned(offset: 0, byte_count: 4, endian: Little)
";
        assert_eq!(count_scan_occurrences(source, &scan).unwrap(), 1);
    }

    #[test]
    fn boon_call_argument_occurrences_follow_balanced_multiline_calls() {
        let scan = OccurrenceScan {
            roots: vec!["examples".to_owned()],
            extensions: vec!["bn".to_owned()],
            mode: OccurrenceScanMode::BoonCallArgument,
            needle: "index".to_owned(),
            call_functions: vec![
                "List/get".to_owned(),
                "Bytes/get".to_owned(),
                "Bytes/set".to_owned(),
            ],
            expected_occurrences: 3,
            expected_files: 1,
            expected_path_occurrences: HashMap::new(),
            line_contains_any: Vec::new(),
            excluded_paths: Vec::new(),
            allowed_paths: Vec::new(),
        };
        let source = r#"
fake_index: items |> List/map(FUNCTION item { item })
first: List/get(
    list: items
    index: 0
)
patched: Bytes/set(
    input: BYTES[1] { 16u01 }
    index: 0
    value: BYTES[1] { 16u02 }
)
nested: List/get(
    list: LIST { Bytes/get(input: BYTES[1] { 16u01 }, index: 0) }
    position: 1
)
label: Text/concat(input: TEXT { x }, with: TEXT { "|index:" })
"#;
        assert_eq!(count_scan_occurrences(source, &scan).unwrap(), 3);
    }

    #[test]
    fn json_source_bundle_scans_decode_embedded_boon_source() {
        let source_bundle = r#"{
  "schema": "boon.source-bundle-golden.v1",
  "units": [
    {
      "path": "RUN.bn",
      "source": "first: List/get(\n    list: rows\n    index: 0\n)\nlegacy: Null\n"
    }
  ]
}"#;
        let text_scan = OccurrenceScan {
            roots: vec!["fixtures/contracts/source_bundle_digest_v1.json".to_owned()],
            extensions: vec!["json".to_owned()],
            mode: OccurrenceScanMode::JsonSourceBundleText,
            needle: "Null".to_owned(),
            call_functions: Vec::new(),
            expected_occurrences: 1,
            expected_files: 1,
            expected_path_occurrences: HashMap::new(),
            line_contains_any: Vec::new(),
            excluded_paths: Vec::new(),
            allowed_paths: Vec::new(),
        };
        let call_scan = OccurrenceScan {
            roots: text_scan.roots.clone(),
            extensions: text_scan.extensions.clone(),
            mode: OccurrenceScanMode::JsonSourceBundleBoonCallArgument,
            needle: "index".to_owned(),
            call_functions: vec!["List/get".to_owned()],
            expected_occurrences: 1,
            expected_files: 1,
            expected_path_occurrences: HashMap::new(),
            line_contains_any: Vec::new(),
            excluded_paths: Vec::new(),
            allowed_paths: Vec::new(),
        };
        assert_eq!(
            count_scan_occurrences(source_bundle, &text_scan).unwrap(),
            1
        );
        assert_eq!(
            count_scan_occurrences(source_bundle, &call_scan).unwrap(),
            1
        );
    }

    #[test]
    fn replacement_documents_require_a_labeled_top_level_notice() {
        let entry = DocumentEntry {
            id: "rewrite".to_owned(),
            path: "rewrite.md".to_owned(),
            classification: DocumentClassification::RewriteRequired,
            owner_phase: "goal-phase-2-foundations".to_owned(),
            status_notice: Some("Phase 0 status: rewrite required.".to_owned()),
        };
        let valid = "\
# Rewrite

> **Phase 0 status: rewrite required.**
> **Current executable behavior:** current.
> **Historical/stale content:** stale.
> **Target-only content:** target.
> **Flag-day owner:** phase.

## Body
";
        assert!(validate_document_status_notice(&entry, valid).is_ok());
        assert!(validate_document_status_notice(&entry, "# Rewrite\n\n## Body\n").is_err());
    }

    #[test]
    fn phase0_deletion_scans_match_current_source() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (manifest, _) = load_toml::<DeletionManifest>(&workspace, DELETION_MANIFEST).unwrap();
        let workspace_files = workspace_files(&workspace).unwrap();
        let mut cache = HashMap::new();
        for entry in &manifest.entries {
            if entry.measurement != LegacyMeasurement::Occurrence {
                continue;
            }
            assert!(!entry.scans.is_empty(), "{} has no exact scans", entry.id);
            for scan in &entry.scans {
                execute_scan(&workspace, &workspace_files, &mut cache, scan)
                    .unwrap_or_else(|error| panic!("{}: {error}", entry.id));
            }
        }
    }

    #[test]
    fn phase0_container_occurrences_match_current_source() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (manifest, _) =
            load_toml::<ContainerInventoryManifest>(&workspace, CONTAINER_MANIFEST).unwrap();
        let workspace_files = workspace_files(&workspace).unwrap();
        validate_container_inventory(&workspace, &workspace_files, &manifest).unwrap();
    }

    #[test]
    fn phase0_replacement_document_notices_match_current_preambles() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (manifest, _) = load_toml::<DocumentManifest>(&workspace, DOCUMENT_MANIFEST).unwrap();
        let workspace_files = workspace_files(&workspace).unwrap();
        let summary = validate_documents(&workspace, &workspace_files, &manifest).unwrap();
        assert_eq!(summary.rewrite_required.len(), 4);
        assert_eq!(summary.delete_after_replacement.len(), 2);
    }

    #[test]
    fn identifier_count_excludes_prefixed_and_suffixed_names() {
        assert_eq!(
            count_identifier_word(
                "BTreeMap BTreeMap2 MyBTreeMap BTreeMap_alias ::BTreeMap::<u8, u8>",
                "BTreeMap",
            ),
            2
        );
        assert_eq!(count_identifier_word("HashSet HashSet", ""), 0);
    }

    #[test]
    fn test_only_ranges_cover_items_without_consuming_following_code() {
        let source = r#"
#[cfg(test)]
fn oracle() {
    let braces = "{ still a string }";
    let _ = BTreeMap::new();
}

fn production() {
    let _ = BTreeMap::new();
}
"#;
        let intervals = test_only_intervals(source);
        let test_offset = source.find("BTreeMap").unwrap();
        let production_offset = source.rfind("BTreeMap").unwrap();
        assert!(
            intervals
                .iter()
                .any(|(start, end)| test_offset >= *start && test_offset < *end)
        );
        assert!(
            intervals
                .iter()
                .all(|(start, end)| production_offset < *start || production_offset >= *end)
        );
    }

    #[test]
    fn structural_scan_distinguishes_code_from_comments_and_strings() {
        let source = "let _: BTreeMap<u8, u8>; // BTreeMap\nlet _ = \"BTreeMap\";\n";
        let structural = rust_structural_bytes(source);
        let offsets = identifier_word_offsets(source, "BTreeMap");
        assert_eq!(offsets.len(), 3);
        assert_eq!(
            offsets
                .iter()
                .filter(|offset| structural[**offset..].starts_with(b"BTreeMap"))
                .count(),
            1
        );
    }

    #[test]
    fn relative_paths_fail_closed() {
        assert!(safe_relative("docs/plans/GOAL_PROMPT.md").is_ok());
        assert!(safe_relative(".").is_ok());
        assert!(safe_relative("../outside").is_err());
        assert!(safe_relative("/absolute").is_err());
        assert!(safe_relative("").is_err());
        assert!(path_matches_root("testdata/phase0/fixture.bn", "."));
        assert!(path_matches_root("crates/local/test.scn", "."));
    }

    #[test]
    fn executable_fixture_manifest_is_bound_to_canonical_datasets() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (manifest, _) = load_toml::<boon_phase0_baseline::evidence::FixtureEvidenceManifestV2>(
            &workspace,
            FIXTURE_MANIFEST,
        )
        .unwrap();
        let summary = validate_fixtures(&workspace, &manifest).unwrap();
        assert_eq!(
            summary.existing.len(),
            boon_phase0_baseline::evidence::REQUIRED_FIXTURE_IDS.len()
        );
        assert!(!summary.not_yet_implemented.is_empty());

        let (datasets, _) = load_toml::<boon_phase0_baseline::dataset::DatasetFixtureManifestV1>(
            &workspace,
            DATASET_MANIFEST,
        )
        .unwrap();
        let datasets = validate_datasets(&workspace, &datasets, &summary).unwrap();
        assert_eq!(datasets.fixtures.len(), 24);
        assert_eq!(datasets.categories.len(), 4);
    }

    #[test]
    fn packed_baseline_values_reject_manifest_tampering() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let (mut manifest, _) = load_toml::<
            boon_phase0_baseline::evidence::BaselineEvidenceManifestV2,
        >(&workspace, BASELINE_MANIFEST)
        .unwrap();
        let report: boon_phase0_baseline::BaselineReport =
            serde_json::from_slice(&fs::read(workspace.join(&manifest.packed_report)).unwrap())
                .unwrap();
        let evidence = manifest
            .areas
            .iter_mut()
            .find(|area| area.id == "allocations")
            .unwrap()
            .evidence
            .iter_mut()
            .find(|evidence| evidence.id == "declared-action-allocation-count")
            .unwrap();
        evidence.value_u64 = Some(1);
        assert!(crosscheck_packed_baseline_values(&manifest, &report).is_err());
    }

    #[test]
    fn packed_numeric_ratchets_enforce_direction_and_regression_budget() {
        assert!(
            validate_numeric_ratchet(
                "max",
                PackedBudgetDirection::Max,
                1_000,
                1_250,
                900,
                2_500,
                1_250,
            )
            .is_ok()
        );
        assert!(
            validate_numeric_ratchet(
                "max-observed-regression",
                PackedBudgetDirection::Max,
                1_000,
                1_250,
                900,
                2_500,
                1_251,
            )
            .is_err()
        );
        assert!(
            validate_numeric_ratchet(
                "max-budget-regression",
                PackedBudgetDirection::Max,
                1_000,
                1_251,
                900,
                2_500,
                1_000,
            )
            .is_err()
        );
        assert!(
            validate_numeric_ratchet(
                "min",
                PackedBudgetDirection::Min,
                1_000,
                750,
                1_100,
                2_500,
                750,
            )
            .is_ok()
        );
        assert!(
            validate_numeric_ratchet(
                "min-observed-regression",
                PackedBudgetDirection::Min,
                1_000,
                750,
                1_100,
                2_500,
                749,
            )
            .is_err()
        );
        assert!(
            validate_numeric_ratchet(
                "min-budget-regression",
                PackedBudgetDirection::Min,
                1_000,
                749,
                1_100,
                2_500,
                1_000,
            )
            .is_err()
        );
    }

    #[test]
    fn packed_numeric_ratchets_reject_weaker_final_limits() {
        assert!(
            validate_numeric_ratchet(
                "max-final",
                PackedBudgetDirection::Max,
                1_000,
                1_100,
                1_101,
                1_000,
                1_000,
            )
            .is_err()
        );
        assert!(
            validate_numeric_ratchet(
                "min-final",
                PackedBudgetDirection::Min,
                1_000,
                900,
                899,
                1_000,
                1_000,
            )
            .is_err()
        );
    }

    #[test]
    fn bounded_text_preserves_utf8_boundaries() {
        let bounded = bounded_text("ééééé", 8);
        assert!(bounded.is_char_boundary(bounded.len()));
        assert!(bounded.len() <= 8);
        assert_eq!(bounded, "éé...");
    }
}
