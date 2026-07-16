use crate::report_v2::{Sha256Digest, ToolResult, sha256_bytes};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PINNED_COMMIT: &str = "dd6e750c2ca9dec3041f66ceda31d30379d4027a";
pub const PARITY_MANIFEST_PATH: &str = "examples/fjordpulse/traceability/parity_manifest.json";
pub const COMPATIBILITY_LEDGER_PATH: &str =
    "examples/fjordpulse/traceability/compatibility_delta_ledger.json";

const STORY_MANIFEST_PATH: &str = "docs/user-stories/00_manifest.json";
const HTTP_OPENAPI_PATH: &str = "contracts/http/openapi.yaml";
const CONTRACT_TRACEABILITY_PATH: &str = "contracts/traceability.json";
const DESIGN_MANIFEST_PATH: &str = "docs/design/00_manifest.json";
const EXPECTED_STORY_COUNT: usize = 108;
const EXPECTED_SCENARIO_COUNT: usize = 340;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceabilityAction {
    Import,
    Verify,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    path: String,
    bytes: u64,
    sha256: Sha256Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReferenceIdentity {
    repository: String,
    commit: String,
    tree: String,
    read_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceInventory {
    source_file_count: u32,
    source_inventory_sha256: Sha256Digest,
    story_manifest: SourceFile,
    black_box_story_files: Vec<SourceFile>,
    story_support_files: Vec<SourceFile>,
    contract_traceability: SourceFile,
    http_openapi: SourceFile,
    http_fixtures: Vec<SourceFile>,
    realtime_schemas: Vec<SourceFile>,
    realtime_fixtures: Vec<SourceFile>,
    design_manifest: SourceFile,
    design_assets: Vec<SourceFile>,
    visual_inventory: Vec<SourceFile>,
    compatibility_source_documents: Vec<SourceFile>,
}

impl SourceInventory {
    fn unique_files(&self) -> Result<Vec<SourceFile>, String> {
        let mut files = BTreeMap::<String, SourceFile>::new();
        let iter = std::iter::once(&self.story_manifest)
            .chain(self.black_box_story_files.iter())
            .chain(self.story_support_files.iter())
            .chain(std::iter::once(&self.contract_traceability))
            .chain(std::iter::once(&self.http_openapi))
            .chain(self.http_fixtures.iter())
            .chain(self.realtime_schemas.iter())
            .chain(self.realtime_fixtures.iter())
            .chain(std::iter::once(&self.design_manifest))
            .chain(self.design_assets.iter())
            .chain(self.visual_inventory.iter())
            .chain(self.compatibility_source_documents.iter());
        for file in iter {
            if let Some(previous) = files.insert(file.path.clone(), file.clone())
                && previous != *file
            {
                return Err(format!(
                    "source file {} has inconsistent digest metadata",
                    file.path
                ));
            }
        }
        Ok(files.into_values().collect())
    }

    fn find(&self, path: &str) -> Result<SourceFile, String> {
        self.unique_files()?
            .into_iter()
            .find(|file| file.path == path)
            .ok_or_else(|| format!("source inventory is missing {path}"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DesignState {
    id: String,
    title: String,
    category: String,
    png_path: String,
    note_path: String,
    dimensions: [u32; 2],
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioClassification {
    AutomatedSemantic,
    BrowserVisualInput,
    ServerIntegration,
    LiveExternal,
    Deployment,
    ExplicitlyDeferred,
    HumanFollowUp,
}

impl ScenarioClassification {
    fn as_str(self) -> &'static str {
        match self {
            Self::AutomatedSemantic => "automated_semantic",
            Self::BrowserVisualInput => "browser_visual_input",
            Self::ServerIntegration => "server_integration",
            Self::LiveExternal => "live_external",
            Self::Deployment => "deployment",
            Self::ExplicitlyDeferred => "explicitly_deferred",
            Self::HumanFollowUp => "human_follow_up",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ClassificationDefinition {
    classification: ScenarioClassification,
    meaning: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceStatus {
    NotImplemented,
    DeferredByPlan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceState {
    status: EvidenceStatus,
    report_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioTrace {
    id: String,
    ordinal: u16,
    source_path: String,
    source_line: u32,
    text: String,
    classification: ScenarioClassification,
    deferred_scopes: Vec<String>,
    evidence: EvidenceState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UnmanifestedNumberedItem {
    provisional_id: String,
    story_id: String,
    source_path: String,
    source_line: u32,
    markdown_ordinal: u16,
    text: String,
    classification: ScenarioClassification,
    reason: String,
    evidence: EvidenceState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoryTrace {
    id: String,
    epic: String,
    title: String,
    source_path: String,
    source_sha256: Sha256Digest,
    acceptance_count: u16,
    scenario_count: u16,
    scenarios: Vec<ScenarioTrace>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ParitySummary {
    story_count: u16,
    scenario_count: u16,
    numbered_black_box_item_count: u16,
    unmanifested_numbered_item_count: u16,
    design_state_count: u16,
    visual_snapshot_count: u16,
    classification_counts: BTreeMap<String, u16>,
    fully_deferred_scenario_count: u16,
    partially_deferred_scenario_count: u16,
    claimed_complete_scenario_count: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ParityManifest {
    schema_version: u16,
    artifact: String,
    generated_by: String,
    reference: ReferenceIdentity,
    completion_semantics: String,
    classification_definitions: Vec<ClassificationDefinition>,
    summary: ParitySummary,
    source_inventory: SourceInventory,
    design_inventory: Vec<DesignState>,
    stories: Vec<StoryTrace>,
    unmanifested_numbered_items: Vec<UnmanifestedNumberedItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ContractAssetDisposition {
    source: SourceFile,
    required_disposition: String,
    approved_delta_ids: Vec<String>,
    target_path: Option<String>,
    implementation_status: String,
    evidence_report_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FieldMapping {
    reference_field_or_scope: String,
    target_field: Option<String>,
    required_target_semantics: String,
    rationale: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TargetContractSkeleton {
    schema_paths: Vec<String>,
    valid_fixture_paths: Vec<String>,
    invalid_fixture_paths: Vec<String>,
    compatible_client_versions: Vec<String>,
    compatible_server_versions: Vec<String>,
    status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DeltaEvidenceSkeleton {
    required_story_ids: Vec<String>,
    report_ids: Vec<String>,
    status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ApprovedDelta {
    id: String,
    class: String,
    rationale: String,
    reference_assets: Vec<SourceFile>,
    field_mappings: Vec<FieldMapping>,
    target_contract: TargetContractSkeleton,
    black_box_evidence: DeltaEvidenceSkeleton,
    implementation_status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DeferredEvidenceScope {
    scenario_id: String,
    coverage: String,
    deferred_scope: String,
    still_required_scope: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityLedger {
    schema_version: u16,
    artifact: String,
    generated_by: String,
    reference: ReferenceIdentity,
    ledger_status: String,
    default_policy: String,
    unchanged_surface_rule: String,
    contract_assets: Vec<ContractAssetDisposition>,
    approved_deltas: Vec<ApprovedDelta>,
    deferred_evidence_scopes: Vec<DeferredEvidenceScope>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryManifest {
    package: String,
    stories: Vec<StoryManifestEntry>,
    files: Vec<StoryManifestFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryManifestEntry {
    id: String,
    epic: String,
    title: String,
    file: String,
    acceptance_count: u16,
    test_scenario_count: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryManifestFile {
    epic: String,
    title: String,
    file: String,
}

#[derive(Clone, Debug)]
struct ParsedScenario {
    ordinal: u16,
    line: u32,
    text: String,
}

#[derive(Clone, Debug)]
struct ParsedStory {
    id: String,
    title: String,
    scenarios: Vec<ParsedScenario>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignManifestEntry {
    id: String,
    title: String,
    category: String,
    png: String,
    md: String,
    dimensions: [u32; 2],
}

struct LoadedSource {
    metadata: SourceFile,
    bytes: Vec<u8>,
}

struct GitReference {
    path: PathBuf,
    identity: ReferenceIdentity,
}

impl GitReference {
    fn open(path: &Path) -> ToolResult<Self> {
        if !path.is_dir() {
            return Err(format!("reference repository does not exist: {}", path.display()).into());
        }
        git_bytes(
            path,
            &["cat-file", "-e", &format!("{PINNED_COMMIT}^{{commit}}")],
        )?;
        let commit = git_text(
            path,
            &[
                "rev-parse",
                "--verify",
                &format!("{PINNED_COMMIT}^{{commit}}"),
            ],
        )?;
        if commit != PINNED_COMMIT {
            return Err(format!(
                "reference resolved pinned commit to {commit}, expected {PINNED_COMMIT}"
            )
            .into());
        }
        let tree = git_text(
            path,
            &[
                "rev-parse",
                "--verify",
                &format!("{PINNED_COMMIT}^{{tree}}"),
            ],
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            identity: ReferenceIdentity {
                repository: "FjordPulse".to_owned(),
                commit,
                tree,
                read_mode: "git_objects_at_pinned_commit".to_owned(),
            },
        })
    }

    fn load(&self, path: &str) -> ToolResult<LoadedSource> {
        validate_git_path(path)?;
        let bytes = git_bytes(
            &self.path,
            &["cat-file", "blob", &format!("{PINNED_COMMIT}:{path}")],
        )?;
        Ok(LoadedSource {
            metadata: SourceFile {
                path: path.to_owned(),
                bytes: u64::try_from(bytes.len())?,
                sha256: sha256_bytes(&bytes),
            },
            bytes,
        })
    }

    fn list(&self, prefix: &str) -> ToolResult<Vec<String>> {
        validate_git_path(prefix.trim_end_matches('/'))?;
        let output = git_text(
            &self.path,
            &["ls-tree", "-r", "--name-only", PINNED_COMMIT, "--", prefix],
        )?;
        let mut paths = output
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

pub fn run(workspace: &Path, action: TraceabilityAction, reference_path: &Path) -> ToolResult<()> {
    let reference = GitReference::open(reference_path)?;
    let (parity, ledger) = generate_artifacts(&reference)?;
    validate_parity(&parity)?;
    validate_ledger(&ledger, &parity)?;
    let parity_bytes = pretty_json(&parity)?;
    let ledger_bytes = pretty_json(&ledger)?;

    let parity_path = workspace.join(PARITY_MANIFEST_PATH);
    let ledger_path = workspace.join(COMPATIBILITY_LEDGER_PATH);
    match action {
        TraceabilityAction::Import => {
            write_atomic(&parity_path, &parity_bytes)?;
            write_atomic(&ledger_path, &ledger_bytes)?;
            println!(
                "imported FjordPulse {}: {} stories, {} scenarios",
                PINNED_COMMIT, parity.summary.story_count, parity.summary.scenario_count
            );
            println!("wrote {}", parity_path.display());
            println!("wrote {}", ledger_path.display());
        }
        TraceabilityAction::Verify => {
            verify_file(&parity_path, &parity_bytes)?;
            verify_file(&ledger_path, &ledger_bytes)?;
            verify_local_contract_oracles(workspace, &parity)?;
            println!(
                "verified FjordPulse {}: {} stories, {} scenarios, {} source files",
                PINNED_COMMIT,
                parity.summary.story_count,
                parity.summary.scenario_count,
                parity.source_inventory.source_file_count
            );
            println!("compatibility ledger remains an unimplemented skeleton");
            println!("repository-local reference contracts match the pinned source digests");
        }
    }
    Ok(())
}

fn verify_local_contract_oracles(workspace: &Path, parity: &ParityManifest) -> ToolResult<()> {
    let contracts = parity
        .source_inventory
        .unique_files()?
        .into_iter()
        .filter(|file| file.path.starts_with("contracts/"))
        .collect::<Vec<_>>();
    if contracts.is_empty() {
        return Err("pinned source inventory contains no contract files".into());
    }
    for contract in &contracts {
        let relative = contract
            .path
            .strip_prefix("contracts/")
            .ok_or_else(|| format!("invalid contract source path {}", contract.path))?;
        let path = workspace
            .join("examples/fjordpulse/contracts/reference")
            .join(relative);
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "cannot read repository-local FjordPulse contract {}: {error}",
                path.display()
            )
        })?;
        let byte_len = u64::try_from(bytes.len())?;
        if byte_len != contract.bytes || sha256_bytes(&bytes) != contract.sha256 {
            return Err(format!(
                "repository-local FjordPulse contract {} differs from pinned {}",
                path.display(),
                contract.path
            )
            .into());
        }
    }
    Ok(())
}

fn generate_artifacts(
    reference: &GitReference,
) -> ToolResult<(ParityManifest, CompatibilityLedger)> {
    let story_manifest_source = reference.load(STORY_MANIFEST_PATH)?;
    let story_manifest: StoryManifest = serde_json::from_slice(&story_manifest_source.bytes)?;
    if story_manifest.package != "fjordpulse_user_stories_blackbox_tests" {
        return Err(format!(
            "unexpected story package {}, expected fjordpulse_user_stories_blackbox_tests",
            story_manifest.package
        )
        .into());
    }
    if story_manifest.stories.len() != EXPECTED_STORY_COUNT {
        return Err(format!(
            "story manifest has {} stories, expected {EXPECTED_STORY_COUNT}",
            story_manifest.stories.len()
        )
        .into());
    }

    let declared_story_files = story_manifest
        .files
        .iter()
        .map(|file| {
            if file.epic.is_empty() || file.title.is_empty() || file.file.is_empty() {
                return Err("story manifest contains an empty epic file entry".to_owned());
            }
            Ok(format!("docs/user-stories/{}", file.file))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let story_paths = story_manifest
        .stories
        .iter()
        .map(|story| format!("docs/user-stories/{}", story.file))
        .collect::<BTreeSet<_>>();
    if story_paths != declared_story_files {
        return Err("story entries and the declared epic file inventory differ".into());
    }
    let mut parsed_story_files = BTreeMap::<String, Vec<ParsedStory>>::new();
    let mut black_box_story_files = Vec::new();
    for path in story_paths {
        let source = reference.load(&path)?;
        let text = std::str::from_utf8(&source.bytes)
            .map_err(|error| format!("{path} is not UTF-8: {error}"))?;
        let parsed = parse_story_markdown(text)?;
        if parsed.is_empty() {
            return Err(format!("{path} contains no FP story headings").into());
        }
        parsed_story_files.insert(path, parsed);
        black_box_story_files.push(source.metadata);
    }

    let mut stories = Vec::with_capacity(EXPECTED_STORY_COUNT);
    let mut unmanifested_numbered_items = Vec::new();
    for (index, source_story) in story_manifest.stories.iter().enumerate() {
        let expected_id = format!("FP-{:03}", index + 1);
        if source_story.id != expected_id {
            return Err(format!(
                "story {} is {}, expected {expected_id}",
                index + 1,
                source_story.id
            )
            .into());
        }
        let source_path = format!("docs/user-stories/{}", source_story.file);
        let parsed_story = parsed_story_files
            .get(&source_path)
            .and_then(|entries| entries.iter().find(|story| story.id == source_story.id))
            .ok_or_else(|| format!("{} is missing from {source_path}", source_story.id))?;
        if parsed_story.title != source_story.title {
            return Err(format!(
                "{} title mismatch: manifest {:?}, markdown {:?}",
                source_story.id, source_story.title, parsed_story.title
            )
            .into());
        }
        if parsed_story.scenarios.len() < usize::from(source_story.test_scenario_count) {
            return Err(format!(
                "{} has only {} parsed scenarios, manifest declares {}",
                source_story.id,
                parsed_story.scenarios.len(),
                source_story.test_scenario_count
            )
            .into());
        }
        let source_sha256 = black_box_story_files
            .iter()
            .find(|file| file.path == source_path)
            .expect("story source was loaded")
            .sha256
            .clone();
        let story_number = u16::try_from(index + 1)?;
        let scenarios = parsed_story
            .scenarios
            .iter()
            .take(usize::from(source_story.test_scenario_count))
            .map(|scenario| {
                let classification = classify_scenario(story_number, scenario.ordinal)?;
                let deferred_scopes = deferred_scopes(story_number, scenario.ordinal);
                Ok(ScenarioTrace {
                    id: format!("{}-S{:02}", source_story.id, scenario.ordinal),
                    ordinal: scenario.ordinal,
                    source_path: source_path.clone(),
                    source_line: scenario.line,
                    text: scenario.text.clone(),
                    classification,
                    deferred_scopes,
                    evidence: EvidenceState {
                        status: if classification == ScenarioClassification::ExplicitlyDeferred {
                            EvidenceStatus::DeferredByPlan
                        } else {
                            EvidenceStatus::NotImplemented
                        },
                        report_ids: Vec::new(),
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (extra_index, scenario) in parsed_story
            .scenarios
            .iter()
            .skip(usize::from(source_story.test_scenario_count))
            .enumerate()
        {
            unmanifested_numbered_items.push(UnmanifestedNumberedItem {
                provisional_id: format!("{}-X{:02}", source_story.id, extra_index + 1),
                story_id: source_story.id.clone(),
                source_path: source_path.clone(),
                source_line: scenario.line,
                markdown_ordinal: scenario.ordinal,
                text: scenario.text.clone(),
                classification: classify_scenario(story_number, scenario.ordinal)?,
                reason: format!(
                    "The pinned Markdown has a numbered item beyond the story manifest's declared test_scenario_count of {}. It is preserved for reconciliation but is not assigned one of the 340 canonical scenario IDs.",
                    source_story.test_scenario_count
                ),
                evidence: EvidenceState {
                    status: EvidenceStatus::NotImplemented,
                    report_ids: Vec::new(),
                },
            });
        }
        stories.push(StoryTrace {
            id: source_story.id.clone(),
            epic: source_story.epic.clone(),
            title: source_story.title.clone(),
            source_path,
            source_sha256,
            acceptance_count: source_story.acceptance_count,
            scenario_count: source_story.test_scenario_count,
            scenarios,
        });
    }

    let story_support_files = load_fixed_sources(
        reference,
        &[
            "docs/user-stories/00_black_box_testing_guide.md",
            "docs/user-stories/traceability_matrix.csv",
            "docs/user-stories/fjordpulse_all_user_stories_blackbox_tests.md",
        ],
    )?;
    let contract_traceability = load_json_source(reference, CONTRACT_TRACEABILITY_PATH)?.metadata;
    let http_openapi_source = reference.load(HTTP_OPENAPI_PATH)?;
    if !http_openapi_source.bytes.starts_with(b"openapi:") {
        return Err(
            format!("{HTTP_OPENAPI_PATH} does not start with an OpenAPI declaration").into(),
        );
    }
    let http_fixtures = load_json_prefix(reference, "contracts/fixtures/http/")?;
    validate_http_fixture_index(reference, &http_fixtures)?;
    let realtime_schemas = load_json_prefix(reference, "contracts/realtime/")?;
    let realtime_fixtures = load_json_prefix(reference, "contracts/fixtures/realtime/")?;

    let design_manifest_source = load_json_source(reference, DESIGN_MANIFEST_PATH)?;
    let design_manifest: Vec<DesignManifestEntry> =
        serde_json::from_slice(&design_manifest_source.bytes)?;
    let (design_inventory, design_assets) = load_design_inventory(reference, &design_manifest)?;
    let visual_inventory = load_visual_inventory(reference)?;
    let compatibility_source_documents = load_fixed_sources(
        reference,
        &[
            "docs/03_api_contract.md",
            "docs/04_realtime_protocol.md",
            "docs/user-stories/11_epic_K_deployment_and_operations.md",
        ],
    )?;

    let mut source_inventory = SourceInventory {
        source_file_count: 0,
        source_inventory_sha256: sha256_bytes(&[]),
        story_manifest: story_manifest_source.metadata,
        black_box_story_files,
        story_support_files,
        contract_traceability,
        http_openapi: http_openapi_source.metadata,
        http_fixtures,
        realtime_schemas,
        realtime_fixtures,
        design_manifest: design_manifest_source.metadata,
        design_assets,
        visual_inventory,
        compatibility_source_documents,
    };
    let unique_files = source_inventory.unique_files()?;
    source_inventory.source_file_count = u32::try_from(unique_files.len())?;
    source_inventory.source_inventory_sha256 = sha256_bytes(&serde_json::to_vec(&unique_files)?);

    let mut classification_counts = BTreeMap::<String, u16>::new();
    for scenario in stories.iter().flat_map(|story| &story.scenarios) {
        *classification_counts
            .entry(scenario.classification.as_str().to_owned())
            .or_default() += 1;
    }
    let scenario_count = stories
        .iter()
        .map(|story| story.scenarios.len())
        .sum::<usize>();
    let visual_snapshot_count = source_inventory
        .visual_inventory
        .iter()
        .filter(|file| file.path.ends_with(".png"))
        .count();
    let parity = ParityManifest {
        schema_version: 1,
        artifact: "fjordpulse_phase_0_parity_manifest".to_owned(),
        generated_by: "cargo xtask fjordpulse-traceability import".to_owned(),
        reference: reference.identity.clone(),
        completion_semantics: "Classification selects a future evidence lane only. Every non-deferred scenario and every unmanifested numbered item is not_implemented and has no report IDs; no implementation or parity completion is claimed. The story manifest's per-story counts define the 340 canonical IDs, while beyond-count Markdown items are retained separately instead of being silently dropped.".to_owned(),
        classification_definitions: classification_definitions(),
        summary: ParitySummary {
            story_count: u16::try_from(stories.len())?,
            scenario_count: u16::try_from(scenario_count)?,
            numbered_black_box_item_count: u16::try_from(
                scenario_count + unmanifested_numbered_items.len(),
            )?,
            unmanifested_numbered_item_count: u16::try_from(
                unmanifested_numbered_items.len(),
            )?,
            design_state_count: u16::try_from(design_inventory.len())?,
            visual_snapshot_count: u16::try_from(visual_snapshot_count)?,
            classification_counts,
            fully_deferred_scenario_count: u16::try_from(
                stories
                    .iter()
                    .flat_map(|story| &story.scenarios)
                    .filter(|scenario| {
                        scenario.classification == ScenarioClassification::ExplicitlyDeferred
                    })
                    .count(),
            )?,
            partially_deferred_scenario_count: u16::try_from(
                stories
                    .iter()
                    .flat_map(|story| &story.scenarios)
                    .filter(|scenario| {
                        scenario.classification != ScenarioClassification::ExplicitlyDeferred
                            && !scenario.deferred_scopes.is_empty()
                    })
                    .count(),
            )?,
            claimed_complete_scenario_count: 0,
        },
        source_inventory,
        design_inventory,
        stories,
        unmanifested_numbered_items,
    };
    let ledger = build_compatibility_ledger(&parity)?;
    Ok((parity, ledger))
}

fn load_fixed_sources(reference: &GitReference, paths: &[&str]) -> ToolResult<Vec<SourceFile>> {
    paths
        .iter()
        .map(|path| reference.load(path).map(|source| source.metadata))
        .collect()
}

fn load_json_source(reference: &GitReference, path: &str) -> ToolResult<LoadedSource> {
    let source = reference.load(path)?;
    serde_json::from_slice::<serde_json::Value>(&source.bytes)
        .map_err(|error| format!("invalid JSON in {path}: {error}"))?;
    Ok(source)
}

fn load_json_prefix(reference: &GitReference, prefix: &str) -> ToolResult<Vec<SourceFile>> {
    let paths = reference
        .list(prefix)?
        .into_iter()
        .filter(|path| path.ends_with(".json"))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(format!("no JSON files found under {prefix}").into());
    }
    paths
        .iter()
        .map(|path| load_json_source(reference, path).map(|source| source.metadata))
        .collect()
}

fn validate_http_fixture_index(
    reference: &GitReference,
    fixtures: &[SourceFile],
) -> ToolResult<()> {
    let index_path = "contracts/fixtures/http/index.json";
    let index = reference.load(index_path)?;
    let entries: BTreeMap<String, String> = serde_json::from_slice(&index.bytes)?;
    let fixture_paths = fixtures
        .iter()
        .map(|fixture| fixture.path.as_str())
        .collect::<BTreeSet<_>>();
    for (operation, file) in entries {
        let path = format!("contracts/fixtures/http/{file}");
        if !fixture_paths.contains(path.as_str()) {
            return Err(
                format!("HTTP fixture index entry {operation} points to missing {path}").into(),
            );
        }
    }
    Ok(())
}

fn load_design_inventory(
    reference: &GitReference,
    entries: &[DesignManifestEntry],
) -> ToolResult<(Vec<DesignState>, Vec<SourceFile>)> {
    if entries.is_empty() {
        return Err("design manifest is empty".into());
    }
    let mut ids = BTreeSet::new();
    let mut assets = Vec::with_capacity(entries.len() * 2);
    let mut inventory = Vec::with_capacity(entries.len());
    for entry in entries {
        if !ids.insert(entry.id.clone()) {
            return Err(format!("duplicate design state {}", entry.id).into());
        }
        let png_path = format!("docs/design/{}", entry.png);
        let note_path = format!("docs/design/{}", entry.md);
        let png = reference.load(&png_path)?;
        let actual_dimensions = png_dimensions(&png.bytes)
            .map_err(|error| format!("invalid design PNG {png_path}: {error}"))?;
        if actual_dimensions != entry.dimensions {
            return Err(format!(
                "design PNG {png_path} is {}x{}, manifest declares {}x{}",
                actual_dimensions[0],
                actual_dimensions[1],
                entry.dimensions[0],
                entry.dimensions[1]
            )
            .into());
        }
        let note = reference.load(&note_path)?;
        if note.bytes.is_empty() {
            return Err(format!("design note {note_path} is empty").into());
        }
        assets.push(png.metadata);
        assets.push(note.metadata);
        inventory.push(DesignState {
            id: entry.id.clone(),
            title: entry.title.clone(),
            category: entry.category.clone(),
            png_path,
            note_path,
            dimensions: entry.dimensions,
        });
    }
    Ok((inventory, assets))
}

fn load_visual_inventory(reference: &GitReference) -> ToolResult<Vec<SourceFile>> {
    let paths = reference.list("tests/visual/")?;
    if paths.is_empty() {
        return Err("visual inventory is empty".into());
    }
    let mut saw_driver = false;
    let mut saw_snapshot = false;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let source = reference.load(&path)?;
        if path == "tests/visual/scenarios.spec.ts" {
            saw_driver = true;
        }
        if path.ends_with(".png") {
            png_dimensions(&source.bytes)
                .map_err(|error| format!("invalid visual snapshot {path}: {error}"))?;
            saw_snapshot = true;
        }
        files.push(source.metadata);
    }
    if !saw_driver || !saw_snapshot {
        return Err("visual inventory must contain scenarios.spec.ts and PNG snapshots".into());
    }
    Ok(files)
}

fn png_dimensions(bytes: &[u8]) -> Result<[u32; 2], String> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("missing PNG signature or IHDR".to_owned());
    }
    Ok([
        u32::from_be_bytes(bytes[16..20].try_into().expect("four width bytes")),
        u32::from_be_bytes(bytes[20..24].try_into().expect("four height bytes")),
    ])
}

fn parse_story_markdown(markdown: &str) -> Result<Vec<ParsedStory>, String> {
    let mut stories = Vec::<ParsedStory>::new();
    let mut current: Option<ParsedStory> = None;
    let mut collecting_scenarios = false;
    for (line_index, line) in markdown.lines().enumerate() {
        if let Some((id, title)) = parse_story_heading(line) {
            if let Some(story) = current.take() {
                stories.push(story);
            }
            current = Some(ParsedStory {
                id,
                title,
                scenarios: Vec::new(),
            });
            collecting_scenarios = false;
            continue;
        }
        if line.trim() == "### Black-box test scenarios" {
            if current.is_none() {
                return Err(format!(
                    "black-box scenario section at line {} has no story",
                    line_index + 1
                ));
            }
            collecting_scenarios = true;
            continue;
        }
        if collecting_scenarios && line.starts_with("### ") {
            collecting_scenarios = false;
            continue;
        }
        if !collecting_scenarios {
            continue;
        }
        if let Some((ordinal, text)) = parse_numbered_item(line) {
            let story = current.as_mut().expect("scenario section has a story");
            let expected =
                u16::try_from(story.scenarios.len() + 1).map_err(|error| error.to_string())?;
            if ordinal != expected {
                return Err(format!(
                    "{} scenario at line {} is numbered {ordinal}, expected {expected}",
                    story.id,
                    line_index + 1
                ));
            }
            story.scenarios.push(ParsedScenario {
                ordinal,
                line: u32::try_from(line_index + 1).map_err(|error| error.to_string())?,
                text: text.to_owned(),
            });
        } else if let Some(scenario) = current
            .as_mut()
            .and_then(|story| story.scenarios.last_mut())
        {
            let continuation = line.trim();
            if !continuation.is_empty() {
                scenario.text.push(' ');
                scenario.text.push_str(continuation);
            }
        }
    }
    if let Some(story) = current {
        stories.push(story);
    }
    for story in &stories {
        if story.scenarios.is_empty() {
            return Err(format!("{} has no parsed black-box scenarios", story.id));
        }
    }
    Ok(stories)
}

fn parse_story_heading(line: &str) -> Option<(String, String)> {
    let heading = line.strip_prefix("## ")?;
    let id = heading.get(..6)?;
    if !is_story_id(id) {
        return None;
    }
    let title = heading[6..]
        .trim_start_matches(|character: char| {
            character == ' ' || character == '-' || character == '\u{2014}'
        })
        .trim();
    if title.is_empty() {
        return None;
    }
    Some((id.to_owned(), title.to_owned()))
}

fn parse_numbered_item(line: &str) -> Option<(u16, &str)> {
    let (number, text) = line.split_once(". ")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) || text.is_empty() {
        return None;
    }
    Some((number.parse().ok()?, text))
}

fn is_story_id(value: &str) -> bool {
    value.len() == 6
        && value.starts_with("FP-")
        && value[3..].bytes().all(|byte| byte.is_ascii_digit())
}

fn classify_scenario(story: u16, scenario: u16) -> Result<ScenarioClassification, String> {
    use ScenarioClassification as Class;
    let classification = match (story, scenario) {
        (89, 2 | 3) => Class::ExplicitlyDeferred,
        (1..=39, _) | (71..=77, _) | (99..=101, _) => Class::BrowserVisualInput,
        (40..=48, _) | (57..=70, _) | (78..=84, _) | (97, _) => Class::ServerIntegration,
        (49..=56, _) => Class::LiveExternal,
        (85..=92, _) | (102, _) | (104..=105, _) | (108, _) => Class::Deployment,
        (93, _) | (106, _) => Class::AutomatedSemantic,
        (94, 2) | (96, 2 | 3) | (107, 2 | 3) => Class::BrowserVisualInput,
        (94, 1 | 3) | (95, 1) | (103, 1 | 3) | (107, 1) => Class::AutomatedSemantic,
        (95, 2 | 3) | (96, 1) => Class::ServerIntegration,
        (98, 3) | (103, 2) => Class::HumanFollowUp,
        (98, 1 | 2) => Class::BrowserVisualInput,
        _ => {
            return Err(format!(
                "classification policy is missing FP-{story:03}-S{scenario:02}"
            ));
        }
    };
    Ok(classification)
}

fn deferred_scopes(story: u16, scenario: u16) -> Vec<String> {
    match (story, scenario) {
        (89, 2) => vec![
            "Backup automation, artifact production, retention, and off-host storage are deferred."
                .to_owned(),
        ],
        (89, 3) => vec![
            "Restore automation and backup-based disaster-recovery drills are deferred."
                .to_owned(),
        ],
        (92, 1) => vec![
            "Only scheduling of backup automation is deferred; cleanup and import scheduling remain required."
                .to_owned(),
        ],
        _ => Vec::new(),
    }
}

fn classification_definitions() -> Vec<ClassificationDefinition> {
    use ScenarioClassification as Class;
    [
        (Class::AutomatedSemantic, "Deterministic semantic, static, unit, contract-document, or repository checks that do not require a running browser product."),
        (Class::BrowserVisualInput, "Automated browser visual, accessibility, input, responsive-layout, or performance behavior."),
        (Class::ServerIntegration, "Controlled HTTP, WebSocket, persistence, security, restart, or process-boundary integration behavior."),
        (Class::LiveExternal, "Behavior whose final evidence requires a Live external Entur or production-data boundary; deterministic fault coverage is still required separately."),
        (Class::Deployment, "Deployment, domain, configuration, maintenance, rollback, smoke, or environment reproducibility behavior."),
        (Class::ExplicitlyDeferred, "A whole scenario limited to backup or restore automation that the rewrite plan explicitly defers."),
        (Class::HumanFollowUp, "A genuinely reviewer-dependent comparison or explanation retained as separate follow-up, never substituted for automated evidence."),
    ]
    .into_iter()
    .map(|(classification, meaning)| ClassificationDefinition {
        classification,
        meaning: meaning.to_owned(),
    })
    .collect()
}

fn build_compatibility_ledger(parity: &ParityManifest) -> Result<CompatibilityLedger, String> {
    let inventory = &parity.source_inventory;
    let openapi = inventory.find(HTTP_OPENAPI_PATH)?;
    let map_fixture = inventory.find("contracts/fixtures/http/map-config-response.json")?;
    let admin_status_fixture =
        inventory.find("contracts/fixtures/http/admin-status-response.json")?;
    let api_doc = inventory.find("docs/03_api_contract.md")?;
    let realtime_doc = inventory.find("docs/04_realtime_protocol.md")?;
    let deployment_stories =
        inventory.find("docs/user-stories/11_epic_K_deployment_and_operations.md")?;

    let mut contract_assets = Vec::new();
    contract_assets.push(asset_disposition(
        openapi.clone(),
        "mixed: listed fields may change; every unlisted OpenAPI operation, field, validation failure, status, and envelope must remain exact",
        &["FPCD-001", "FPCD-002", "FPCD-003"],
    ));
    for source in inventory
        .http_fixtures
        .iter()
        .chain(inventory.realtime_schemas.iter())
        .chain(inventory.realtime_fixtures.iter())
    {
        let delta_ids: &[&str] = match source.path.as_str() {
            "contracts/fixtures/http/map-config-response.json" => &["FPCD-001"],
            "contracts/fixtures/http/admin-status-response.json" => &["FPCD-002", "FPCD-003"],
            _ => &[],
        };
        let disposition = if delta_ids.is_empty() {
            "byte-exact compatibility required"
        } else {
            "paired old/new schema and valid/invalid fixtures required before implementation can claim compatibility"
        };
        contract_assets.push(asset_disposition(source.clone(), disposition, delta_ids));
    }

    let approved_deltas = vec![
        ApprovedDelta {
            id: "FPCD-001".to_owned(),
            class: "map_config_raster_sources".to_owned(),
            rationale: "The Boon client owns retained raster rendering and must receive truthful allowlisted raster-source capabilities rather than MapLibre style documents or browser-exposed provider keys.".to_owned(),
            reference_assets: vec![openapi.clone(), map_fixture],
            field_mappings: vec![
                field_mapping("MapConfigData.provider", "A versioned identity for the allowlisted raster-source configuration; the exact target field is intentionally unset until its schema exists.", "Preserve provider provenance without claiming a MapLibre style-document contract."),
                field_mapping("MapConfigData.basemaps[].styleUrl", "Allowlisted raster source descriptors with attribution, zoom/tile bounds, and opaque capability references; the exact target fields are intentionally unset.", "A style URL and browser key describe the removed MapLibre integration and cannot remain truthful."),
                field_mapping("MapConfigData.defaultBasemap", "A default selection that references one declared raster source.", "Preserve the user's default-map intent while changing only technology-bound source description."),
            ],
            target_contract: empty_target_contract(),
            black_box_evidence: empty_delta_evidence(&["FP-001", "FP-002", "FP-005", "FP-008"]),
            implementation_status: "skeleton_not_implemented".to_owned(),
        },
        ApprovedDelta {
            id: "FPCD-002".to_owned(),
            class: "admin_boon_redb_database_diagnostics".to_owned(),
            rationale: "Admin diagnostics must report Boon semantic collections, indexes, migrations, and redb health instead of exposing SurrealDB INFO or SurrealQL-shaped objects.".to_owned(),
            reference_assets: vec![openapi.clone(), admin_status_fixture.clone(), api_doc.clone()],
            field_mappings: vec![
                field_mapping("AdminStatusData.database.{engine,endpointOrigin,namespace,name,warning}", "Credential-free redb namespace, storage health, and compatibility identity; exact target fields remain unset.", "SurrealDB endpoint and namespace fields would falsely describe the rewritten authority."),
                field_mapping("AdminDatabaseSchemaData.tables[] and DatabaseSchema*", "Read-only allowlisted Boon collection, field, index, bound, and migration metadata; exact target fields remain unset.", "SurrealDB table permissions, events, and schema modes do not model Boon/redb semantics truthfully."),
                field_mapping("AdminDatabaseMigrationsData and DatabaseMigration*", "Read-only source-controlled Boon migration graph, checksums, attempts, and compatibility state; exact target fields remain unset.", "Preserve operator intent and failure states while removing SurrealQL source/object terminology."),
            ],
            target_contract: empty_target_contract(),
            black_box_evidence: empty_delta_evidence(&["FP-057", "FP-058", "FP-059", "FP-065", "FP-089"]),
            implementation_status: "skeleton_not_implemented".to_owned(),
        },
        ApprovedDelta {
            id: "FPCD-003".to_owned(),
            class: "admin_single_boon_server_topology".to_owned(),
            rationale: "The rewritten deployment has one Boon Server process and one durable publication path, so Admin and health responses must not report separate FrankenPHP, AMPHP, SurrealDB, or live-query-bridge services.".to_owned(),
            reference_assets: vec![openapi.clone(), admin_status_fixture, api_doc],
            field_mappings: vec![
                field_mapping("AdminStatusData.services.{backend,realtime,surrealdb,liveQueryBridge}", "One Boon Server lifecycle plus listener, durable publication, redb, Entur, and map capability health; exact target fields remain unset.", "Retain diagnosability while describing the real single-process ownership graph."),
                field_mapping("HealthData.services.{http,realtime,surrealdb,liveQueryBridge}", "Truthful generic host and Boon Server readiness/degradation states; exact target fields remain unset.", "Removed services cannot be reported as independently healthy."),
            ],
            target_contract: empty_target_contract(),
            black_box_evidence: empty_delta_evidence(&["FP-065", "FP-069", "FP-087", "FP-088"]),
            implementation_status: "skeleton_not_implemented".to_owned(),
        },
        ApprovedDelta {
            id: "FPCD-004".to_owned(),
            class: "deployment_identity_and_single_container".to_owned(),
            rationale: "The rewrite deploys at fjordpulse-boon.kavik.cz as one Boon server container instead of the pinned multi-service fjordpulse.kavik.cz topology.".to_owned(),
            reference_assets: vec![realtime_doc, deployment_stories.clone()],
            field_mappings: vec![
                field_mapping("public origin fjordpulse.kavik.cz and wss://fjordpulse.kavik.cz/live", "Public HTTP/WSS identity fjordpulse-boon.kavik.cz with routes preserved unless another approved delta says otherwise.", "Keep externally observable product intent while avoiding a false deployment identity."),
                field_mapping("Coolify frontend/app/realtime/SurrealDB service topology", "One Coolify-managed Boon Server container and mounted redb volume; exact deployment manifest paths remain unset.", "The old service list cannot be used as rewrite readiness evidence."),
            ],
            target_contract: empty_target_contract(),
            black_box_evidence: empty_delta_evidence(&["FP-085", "FP-086", "FP-105"]),
            implementation_status: "skeleton_not_implemented".to_owned(),
        },
        ApprovedDelta {
            id: "FPCD-005".to_owned(),
            class: "backup_restore_automation_deferment".to_owned(),
            rationale: "Only backup/restore automation, retention, off-host storage, and backup-based disaster-recovery drills are outside this milestone; persistence across process, container, host-service, and normal Coolify redeploy restarts remains mandatory.".to_owned(),
            reference_assets: vec![deployment_stories],
            field_mappings: vec![
                field_mapping("FP-089 scenario 2 backup task/artifact and scenario 3 staging restore", "No target implementation in this milestone.", "The rewrite plan explicitly defers these whole backup/restore automation scenarios without waiving persistence."),
                field_mapping("FP-092 scenario 1 scheduled backup portion", "No backup scheduler target in this milestone; cleanup and import scheduling remain required.", "This is a partial-scenario deferment and must not broaden into a waiver of maintenance orchestration."),
            ],
            target_contract: TargetContractSkeleton {
                status: "not_applicable_explicitly_deferred".to_owned(),
                ..empty_target_contract()
            },
            black_box_evidence: DeltaEvidenceSkeleton {
                required_story_ids: vec!["FP-089".to_owned(), "FP-092".to_owned()],
                report_ids: Vec::new(),
                status: "explicitly_deferred_without_completion_evidence".to_owned(),
            },
            implementation_status: "explicitly_deferred".to_owned(),
        },
    ];

    Ok(CompatibilityLedger {
        schema_version: 1,
        artifact: "fjordpulse_phase_0_compatibility_delta_ledger".to_owned(),
        generated_by: "cargo xtask fjordpulse-traceability import".to_owned(),
        reference: parity.reference.clone(),
        ledger_status: "skeleton_not_implemented".to_owned(),
        default_policy: "Public DTOs, routes, envelopes, commands, events, validation failures, status codes, schemas, and fixtures are unchanged unless an approved delta below names the exact affected surface.".to_owned(),
        unchanged_surface_rule: "Pinned SHA-256 values freeze reference bytes only. target_path, target schema/fixture lists, compatible versions, and report IDs must be populated and verified before any implementation compatibility claim.".to_owned(),
        contract_assets,
        approved_deltas,
        deferred_evidence_scopes: vec![
            DeferredEvidenceScope {
                scenario_id: "FP-089-S02".to_owned(),
                coverage: "whole_scenario".to_owned(),
                deferred_scope: "Backup automation, backup artifact production, retention, and off-host storage.".to_owned(),
                still_required_scope: None,
            },
            DeferredEvidenceScope {
                scenario_id: "FP-089-S03".to_owned(),
                coverage: "whole_scenario".to_owned(),
                deferred_scope: "Restore automation and backup-based disaster-recovery drills.".to_owned(),
                still_required_scope: None,
            },
            DeferredEvidenceScope {
                scenario_id: "FP-092-S01".to_owned(),
                coverage: "partial_scenario".to_owned(),
                deferred_scope: "Scheduling the backup task only.".to_owned(),
                still_required_scope: Some(
                    "Cleanup and import tasks must still be scheduled and visible to operators."
                        .to_owned(),
                ),
            },
        ],
    })
}

fn asset_disposition(
    source: SourceFile,
    required_disposition: &str,
    delta_ids: &[&str],
) -> ContractAssetDisposition {
    ContractAssetDisposition {
        source,
        required_disposition: required_disposition.to_owned(),
        approved_delta_ids: delta_ids.iter().map(|id| (*id).to_owned()).collect(),
        target_path: None,
        implementation_status: "not_implemented".to_owned(),
        evidence_report_ids: Vec::new(),
    }
}

fn field_mapping(reference: &str, target_semantics: &str, rationale: &str) -> FieldMapping {
    FieldMapping {
        reference_field_or_scope: reference.to_owned(),
        target_field: None,
        required_target_semantics: target_semantics.to_owned(),
        rationale: rationale.to_owned(),
    }
}

fn empty_target_contract() -> TargetContractSkeleton {
    TargetContractSkeleton {
        schema_paths: Vec::new(),
        valid_fixture_paths: Vec::new(),
        invalid_fixture_paths: Vec::new(),
        compatible_client_versions: Vec::new(),
        compatible_server_versions: Vec::new(),
        status: "not_implemented".to_owned(),
    }
}

fn empty_delta_evidence(story_ids: &[&str]) -> DeltaEvidenceSkeleton {
    DeltaEvidenceSkeleton {
        required_story_ids: story_ids.iter().map(|id| (*id).to_owned()).collect(),
        report_ids: Vec::new(),
        status: "not_implemented".to_owned(),
    }
}

fn validate_parity(parity: &ParityManifest) -> Result<(), String> {
    if parity.schema_version != 1 || parity.reference.commit != PINNED_COMMIT {
        return Err("parity manifest schema or pinned commit is invalid".to_owned());
    }
    if usize::from(parity.summary.story_count) != EXPECTED_STORY_COUNT
        || parity.stories.len() != EXPECTED_STORY_COUNT
    {
        return Err(format!(
            "parity manifest must account for exactly {EXPECTED_STORY_COUNT} stories"
        ));
    }
    let mut scenario_ids = BTreeSet::new();
    let mut actual_classification_counts = BTreeMap::<String, u16>::new();
    let mut fully_deferred = 0_u16;
    let mut partially_deferred = 0_u16;
    for (story_index, story) in parity.stories.iter().enumerate() {
        let expected_story_id = format!("FP-{:03}", story_index + 1);
        if story.id != expected_story_id {
            return Err(format!(
                "parity story {} is {}, expected {expected_story_id}",
                story_index + 1,
                story.id
            ));
        }
        if usize::from(story.scenario_count) != story.scenarios.len() {
            return Err(format!(
                "{} scenario count does not match entries",
                story.id
            ));
        }
        for (scenario_index, scenario) in story.scenarios.iter().enumerate() {
            let expected_scenario_id = format!("{}-S{:02}", story.id, scenario_index + 1);
            if scenario.id != expected_scenario_id
                || usize::from(scenario.ordinal) != scenario_index + 1
                || !scenario_ids.insert(scenario.id.clone())
            {
                return Err(format!("invalid or duplicate scenario {}", scenario.id));
            }
            if !scenario.evidence.report_ids.is_empty() {
                return Err(format!(
                    "{} invents implementation evidence in Phase 0",
                    scenario.id
                ));
            }
            match scenario.classification {
                ScenarioClassification::ExplicitlyDeferred => {
                    fully_deferred += 1;
                    if scenario.evidence.status != EvidenceStatus::DeferredByPlan
                        || !matches!(scenario.id.as_str(), "FP-089-S02" | "FP-089-S03")
                    {
                        return Err(format!("{} is not an approved full deferment", scenario.id));
                    }
                }
                _ => {
                    if scenario.evidence.status != EvidenceStatus::NotImplemented {
                        return Err(format!("{} must remain not_implemented", scenario.id));
                    }
                    if !scenario.deferred_scopes.is_empty() {
                        partially_deferred += 1;
                        if scenario.id != "FP-092-S01" {
                            return Err(format!(
                                "{} has an unapproved partial deferment",
                                scenario.id
                            ));
                        }
                    }
                }
            }
            *actual_classification_counts
                .entry(scenario.classification.as_str().to_owned())
                .or_default() += 1;
        }
    }
    if scenario_ids.len() != EXPECTED_SCENARIO_COUNT
        || usize::from(parity.summary.scenario_count) != EXPECTED_SCENARIO_COUNT
    {
        return Err(format!(
            "parity manifest accounts for {} scenarios, expected {EXPECTED_SCENARIO_COUNT}",
            scenario_ids.len()
        ));
    }
    let mut provisional_ids = BTreeSet::new();
    for item in &parity.unmanifested_numbered_items {
        if !provisional_ids.insert(item.provisional_id.clone())
            || !is_story_id(&item.story_id)
            || item.evidence.status != EvidenceStatus::NotImplemented
            || !item.evidence.report_ids.is_empty()
        {
            return Err(format!(
                "unmanifested numbered item {} has invalid identity or evidence",
                item.provisional_id
            ));
        }
    }
    if usize::from(parity.summary.unmanifested_numbered_item_count)
        != parity.unmanifested_numbered_items.len()
        || usize::from(parity.summary.numbered_black_box_item_count)
            != scenario_ids.len() + parity.unmanifested_numbered_items.len()
    {
        return Err("numbered Markdown item summary does not match preserved entries".to_owned());
    }
    if parity.summary.classification_counts != actual_classification_counts
        || parity.summary.fully_deferred_scenario_count != fully_deferred
        || parity.summary.partially_deferred_scenario_count != partially_deferred
        || parity.summary.claimed_complete_scenario_count != 0
    {
        return Err("parity summary does not match scenario entries".to_owned());
    }
    if fully_deferred != 2 || partially_deferred != 1 {
        return Err(
            "deferment inventory must be exactly two full and one partial scenario".to_owned(),
        );
    }
    let unique_files = parity.source_inventory.unique_files()?;
    if unique_files.len()
        != usize::try_from(parity.source_inventory.source_file_count)
            .map_err(|error| error.to_string())?
        || sha256_bytes(&serde_json::to_vec(&unique_files).map_err(|error| error.to_string())?)
            != parity.source_inventory.source_inventory_sha256
    {
        return Err("source inventory count or digest is stale".to_owned());
    }
    Ok(())
}

fn validate_ledger(ledger: &CompatibilityLedger, parity: &ParityManifest) -> Result<(), String> {
    if ledger.schema_version != 1
        || ledger.reference != parity.reference
        || ledger.ledger_status != "skeleton_not_implemented"
    {
        return Err("compatibility ledger identity or status is invalid".to_owned());
    }
    let expected_ids = ["FPCD-001", "FPCD-002", "FPCD-003", "FPCD-004", "FPCD-005"];
    let actual_ids = ledger
        .approved_deltas
        .iter()
        .map(|delta| delta.id.as_str())
        .collect::<Vec<_>>();
    if actual_ids != expected_ids {
        return Err(
            "compatibility ledger must contain exactly the five approved deltas".to_owned(),
        );
    }
    for asset in &ledger.contract_assets {
        if asset.target_path.is_some()
            || asset.implementation_status != "not_implemented"
            || !asset.evidence_report_ids.is_empty()
        {
            return Err(format!(
                "contract asset {} invents target implementation evidence",
                asset.source.path
            ));
        }
    }
    for delta in &ledger.approved_deltas {
        if delta.field_mappings.is_empty()
            || delta
                .field_mappings
                .iter()
                .any(|mapping| mapping.target_field.is_some())
            || !delta.target_contract.schema_paths.is_empty()
            || !delta.target_contract.valid_fixture_paths.is_empty()
            || !delta.target_contract.invalid_fixture_paths.is_empty()
            || !delta.target_contract.compatible_client_versions.is_empty()
            || !delta.target_contract.compatible_server_versions.is_empty()
            || !delta.black_box_evidence.report_ids.is_empty()
        {
            return Err(format!(
                "{} must remain an evidence-free compatibility skeleton",
                delta.id
            ));
        }
    }
    let deferred = ledger
        .deferred_evidence_scopes
        .iter()
        .map(|scope| (scope.scenario_id.as_str(), scope.coverage.as_str()))
        .collect::<Vec<_>>();
    if deferred
        != [
            ("FP-089-S02", "whole_scenario"),
            ("FP-089-S03", "whole_scenario"),
            ("FP-092-S01", "partial_scenario"),
        ]
    {
        return Err("compatibility ledger deferments exceed the approved backup scopes".to_owned());
    }
    Ok(())
}

fn pretty_json<T: Serialize>(value: &T) -> ToolResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn verify_file(path: &Path, expected: &[u8]) -> ToolResult<()> {
    let actual = fs::read(path)
        .map_err(|error| format!("cannot read generated artifact {}: {error}", path.display()))?;
    if actual != expected {
        return Err(format!(
            "generated artifact drift in {}: expected sha256 {}, found {}; run `cargo xtask fjordpulse-traceability import --reference <FjordPulse-repo>`",
            path.display(),
            sha256_bytes(expected),
            sha256_bytes(&actual)
        )
        .into());
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> ToolResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("generated path {} has no parent", path.display()))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn git_bytes(reference: &Path, args: &[&str]) -> ToolResult<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(reference)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            reference.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

fn git_text(reference: &Path, args: &[&str]) -> ToolResult<String> {
    let bytes = git_bytes(reference, args)?;
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}

fn validate_git_path(path: &str) -> ToolResult<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!("invalid reference Git path {}", path.display()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_parser_preserves_order_lines_and_wrapped_text() {
        let markdown = "# Epic\n\n## FP-001 \u{2014} First story\n\n### Black-box test scenarios\n\n1. First line\n   continued detail\n2. Second line\n\n### Pass evidence\n\n- Evidence\n";
        let stories = parse_story_markdown(markdown).unwrap();
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].id, "FP-001");
        assert_eq!(stories[0].title, "First story");
        assert_eq!(stories[0].scenarios.len(), 2);
        assert_eq!(stories[0].scenarios[0].line, 7);
        assert_eq!(stories[0].scenarios[0].text, "First line continued detail");
    }

    #[test]
    fn classification_policy_defers_only_approved_whole_scenarios() {
        assert_eq!(
            classify_scenario(89, 2).unwrap(),
            ScenarioClassification::ExplicitlyDeferred
        );
        assert_eq!(
            classify_scenario(89, 3).unwrap(),
            ScenarioClassification::ExplicitlyDeferred
        );
        assert_eq!(
            classify_scenario(89, 1).unwrap(),
            ScenarioClassification::Deployment
        );
        assert_eq!(
            classify_scenario(92, 1).unwrap(),
            ScenarioClassification::Deployment
        );
        assert_eq!(deferred_scopes(92, 1).len(), 1);
        assert!(deferred_scopes(92, 2).is_empty());
    }

    #[test]
    fn verifier_reports_byte_drift() {
        let root = std::env::temp_dir().join(format!(
            "boon-fjordpulse-traceability-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("artifact.json");
        fs::write(&path, b"stale\n").unwrap();
        let error = verify_file(&path, b"fresh\n").unwrap_err().to_string();
        assert!(error.contains("generated artifact drift"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn committed_phase_zero_artifacts_have_strict_accounting_without_evidence() {
        let workspace = super::super::workspace_root();
        let parity: ParityManifest =
            serde_json::from_slice(&fs::read(workspace.join(PARITY_MANIFEST_PATH)).unwrap())
                .unwrap();
        let ledger: CompatibilityLedger =
            serde_json::from_slice(&fs::read(workspace.join(COMPATIBILITY_LEDGER_PATH)).unwrap())
                .unwrap();
        validate_parity(&parity).unwrap();
        validate_ledger(&ledger, &parity).unwrap();
        assert_eq!(parity.summary.story_count, 108);
        assert_eq!(parity.summary.scenario_count, 340);
        assert_eq!(parity.summary.numbered_black_box_item_count, 349);
        assert_eq!(parity.summary.unmanifested_numbered_item_count, 9);
        assert_eq!(parity.summary.claimed_complete_scenario_count, 0);
    }
}
