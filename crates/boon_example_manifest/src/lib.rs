use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub type ManifestResult<T> = Result<T, ManifestError>;

#[derive(Debug)]
pub enum ManifestError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read manifest `{}`: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "failed to parse manifest `{}`: {source}",
                    path.display()
                )
            }
            Self::Invalid { path, message } => {
                write!(
                    formatter,
                    "invalid manifest `{}`: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleManifest {
    #[serde(default)]
    pub example: Vec<ExampleEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleEntry {
    pub id: String,
    pub label: String,
    pub source: String,
    #[serde(default)]
    pub source_files: Vec<String>,
    #[serde(default)]
    pub build_files: Vec<String>,
    #[serde(default)]
    pub asset_files: Vec<String>,
    #[serde(default)]
    pub asset_directories: Vec<String>,
    pub scenario: String,
    pub budget: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_tab_order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_by_default: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_evidence_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_testing_needed: Option<bool>,
    #[serde(default)]
    pub initial_visible_assertions: Vec<String>,
    #[serde(default)]
    pub input_scenarios: Vec<String>,
    #[serde(default)]
    pub native_preview_scenarios: Vec<String>,
    #[serde(default)]
    pub scroll_focus_scenarios: Vec<String>,
    #[serde(default)]
    pub scenario_ref_provenance: Vec<ScenarioRefProvenance>,
    #[serde(default)]
    pub visual_artifacts: Vec<String>,
    #[serde(default)]
    pub performance_thresholds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_sequence: Option<String>,
}

impl ExampleEntry {
    pub fn source_project(&self) -> SourceProject {
        SourceProject {
            source: self.source.clone(),
            source_files: self.source_files.clone(),
        }
    }

    pub fn application_identity(&self) -> Option<ApplicationIdentity> {
        self.application.as_ref().map(ApplicationManifest::identity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRefProvenance {
    pub id: String,
    #[serde(default)]
    pub phases: Vec<String>,
    #[serde(default)]
    pub generated_probe: bool,
    pub provenance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProject {
    pub source: String,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIdentity {
    pub package_id: String,
    pub deployment_domain: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationManifest {
    pub package_id: String,
    pub deployment_domain: String,
}

impl ApplicationManifest {
    pub fn identity(&self) -> ApplicationIdentity {
        ApplicationIdentity {
            package_id: self.package_id.clone(),
            deployment_domain: self.deployment_domain.clone(),
        }
    }
}

impl From<&ApplicationManifest> for ApplicationIdentity {
    fn from(application: &ApplicationManifest) -> Self {
        application.identity()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSequence {
    pub initial_stage: String,
    pub scenario: String,
    #[serde(default, rename = "stage")]
    pub stages: Vec<MigrationStage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationStage {
    pub id: String,
    pub label: String,
    pub schema_version: u64,
    pub source: String,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationScenario {
    pub name: String,
    #[serde(default, rename = "step")]
    pub steps: Vec<MigrationScenarioStep>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationScenarioStep {
    pub id: String,
    pub action: MigrationLifecycleAction,
    #[serde(default, rename = "assert")]
    pub assertions: Vec<MigrationAssertion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_failure: Option<MigrationExpectedFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum MigrationLifecycleAction {
    Start {
        stage: String,
        namespace: String,
    },
    Dispatch {
        public_source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<MigrationRowTarget>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        payload: BTreeMap<String, MigrationScenarioValue>,
    },
    Checkpoint,
    Restart,
    #[serde(rename = "preview")]
    PreviewStage {
        stage: String,
    },
    #[serde(rename = "activate")]
    ActivateStage {
        stage: String,
        mode: MigrationActivationMode,
    },
    StartOver {
        stage: String,
    },
    InjectFault {
        point: MigrationFaultPoint,
        #[serde(default = "default_fault_occurrence")]
        occurrence: u64,
    },
    ClearFault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationActivationMode {
    Incremental,
    Skipped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStateScope {
    Current,
    Durable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationFaultPoint {
    BeforeCheckpoint,
    DuringCheckpoint,
    AfterCheckpointAcknowledgement,
    CandidateSettle,
    BeforeActivationCommit,
    DuringActivationCommit,
    AfterActivationCommit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRowTarget {
    pub list: String,
    pub key: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MigrationScenarioValue {
    Bool(bool),
    Integer(i64),
    Text(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationExpectedFailure {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_contains: Option<String>,
    pub current_unchanged: bool,
    pub durable_unchanged: bool,
    #[serde(default = "default_true")]
    pub schema_unchanged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum MigrationAssertion {
    CurrentValue {
        path: String,
        value: MigrationScenarioValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        touched: Option<bool>,
    },
    DurableValue {
        path: String,
        value: MigrationScenarioValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        touched: Option<bool>,
    },
    CurrentAbsent {
        path: String,
    },
    DurableAbsent {
        path: String,
    },
    Schema {
        current_stage: String,
        durable_stage: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview_stage: Option<String>,
    },
    Edges {
        mode: MigrationActivationMode,
        from_stage: String,
        to_stage: String,
        applied: Vec<MigrationEdgeAssertion>,
    },
    List {
        scope: MigrationStateScope,
        path: String,
        rows: Vec<MigrationListRowAssertion>,
        next_key: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        touched: Option<bool>,
    },
    NamespaceIsolation {
        namespace: String,
        unchanged_since: String,
    },
    NamespaceEquivalent {
        namespace: String,
        other_namespace: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationEdgeAssertion {
    pub from_stage: String,
    pub to_stage: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationListRowAssertion {
    pub key: u64,
    pub generation: u64,
    #[serde(default)]
    pub values: BTreeMap<String, MigrationScenarioValue>,
    #[serde(default)]
    pub touched_fields: Vec<String>,
}

const fn default_fault_occurrence() -> u64 {
    1
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug)]
struct ActiveMigrationFault {
    point: MigrationFaultPoint,
    occurrence: u64,
    compatible_actions_seen: u64,
    observed: bool,
}

impl MigrationStage {
    pub fn source_project(&self) -> SourceProject {
        SourceProject {
            source: self.source.clone(),
            source_files: self.source_files.clone(),
        }
    }
}

impl ExampleManifest {
    pub fn from_path(path: impl AsRef<Path>) -> ManifestResult<Self> {
        let path = path.as_ref();
        let examples_root = path.parent().ok_or_else(|| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: "example manifest path has no examples directory".to_owned(),
        })?;
        Self::from_path_with_examples_root(path, examples_root)
    }

    pub fn from_path_with_examples_root(
        path: impl AsRef<Path>,
        examples_root: impl AsRef<Path>,
    ) -> ManifestResult<Self> {
        let path = path.as_ref();
        let manifest: Self = read_manifest(path)?;
        manifest.validate_at(path, examples_root.as_ref())?;
        Ok(manifest)
    }

    pub fn validate(&self, examples_root: impl AsRef<Path>) -> ManifestResult<()> {
        let examples_root = examples_root.as_ref();
        self.validate_at(&examples_root.join("manifest.toml"), examples_root)
    }

    fn validate_at(&self, manifest_path: &Path, examples_root: &Path) -> ManifestResult<()> {
        let context = ValidationContext::new(manifest_path, examples_root)?;
        if self.example.is_empty() {
            return Err(context.invalid("example manifest has no entries"));
        }

        let mut ids = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for entry in &self.example {
            validate_required_text(&context, "example id", &entry.id)?;
            if !ids.insert(entry.id.as_str()) {
                return Err(context.invalid(format!("duplicate example id `{}`", entry.id)));
            }
            validate_required_text(
                &context,
                &format!("example `{}` label", entry.id),
                &entry.label,
            )?;
            validate_source_project(
                &context,
                &format!("example `{}`", entry.id),
                &entry.source_project(),
            )?;
            context.required_file(&entry.scenario, &format!("example `{}` scenario", entry.id))?;
            context.required_file(&entry.budget, &format!("example `{}` budget", entry.id))?;
            validate_file_paths(
                &context,
                &entry.build_files,
                &format!("example `{}` build file", entry.id),
            )?;
            validate_file_paths(
                &context,
                &entry.asset_files,
                &format!("example `{}` asset file", entry.id),
            )?;
            for directory in &entry.asset_directories {
                context.required_directory(
                    directory,
                    &format!("example `{}` asset directory", entry.id),
                )?;
            }

            if let Some(application) = &entry.application {
                validate_application(&context, &entry.id, application)?;
                let identity = application.identity();
                if !identities.insert(identity.clone()) {
                    return Err(context.invalid(format!(
                        "duplicate application identity `{}` in deployment domain `{}`",
                        identity.package_id, identity.deployment_domain
                    )));
                }
            }

            if let Some(sequence_path) = &entry.migration_sequence {
                if entry.application.is_none() {
                    return Err(context.invalid(format!(
                        "example `{}` has a migration sequence but no stable application identity",
                        entry.id
                    )));
                }
                let sequence_path = context.required_file(
                    sequence_path,
                    &format!("example `{}` migration sequence", entry.id),
                )?;
                MigrationSequence::from_path_with_examples_root(
                    sequence_path,
                    &context.examples_root,
                )?;
            }
        }
        Ok(())
    }
}

impl MigrationSequence {
    pub fn from_path(
        path: impl AsRef<Path>,
        examples_root: impl AsRef<Path>,
    ) -> ManifestResult<Self> {
        Self::from_path_with_examples_root(path, examples_root)
    }

    pub fn from_path_with_examples_root(
        path: impl AsRef<Path>,
        examples_root: impl AsRef<Path>,
    ) -> ManifestResult<Self> {
        let path = path.as_ref();
        let sequence: Self = read_manifest(path)?;
        sequence.validate_at(path, examples_root.as_ref())?;
        Ok(sequence)
    }

    pub fn validate(&self, examples_root: impl AsRef<Path>) -> ManifestResult<()> {
        let examples_root = examples_root.as_ref();
        self.validate_at(
            &examples_root.join("migration-sequence.toml"),
            examples_root,
        )
    }

    fn validate_at(&self, manifest_path: &Path, examples_root: &Path) -> ManifestResult<()> {
        let context = ValidationContext::new(manifest_path, examples_root)?;
        validate_required_text(&context, "initial migration stage", &self.initial_stage)?;
        let scenario_path = context.required_file(&self.scenario, "migration scenario")?;
        if self.stages.is_empty() {
            return Err(context.invalid("migration sequence has no stages"));
        }

        let mut ids = BTreeSet::new();
        let mut versions = BTreeSet::new();
        let mut previous_version = None;
        for stage in &self.stages {
            validate_required_text(&context, "migration stage id", &stage.id)?;
            if !ids.insert(stage.id.as_str()) {
                return Err(context.invalid(format!("duplicate migration stage id `{}`", stage.id)));
            }
            validate_required_text(
                &context,
                &format!("migration stage `{}` label", stage.id),
                &stage.label,
            )?;
            if stage.schema_version == 0 {
                return Err(context.invalid(format!(
                    "migration stage `{}` has invalid schema version 0",
                    stage.id
                )));
            }
            if !versions.insert(stage.schema_version) {
                return Err(context.invalid(format!(
                    "duplicate migration schema version {}",
                    stage.schema_version
                )));
            }
            if previous_version.is_some_and(|version| stage.schema_version <= version) {
                return Err(context.invalid(format!(
                    "migration stage `{}` schema version {} is not forward from {}",
                    stage.id,
                    stage.schema_version,
                    previous_version.unwrap_or_default()
                )));
            }
            previous_version = Some(stage.schema_version);
            validate_source_project(
                &context,
                &format!("migration stage `{}`", stage.id),
                &stage.source_project(),
            )?;
        }

        match self
            .stages
            .iter()
            .position(|stage| stage.id == self.initial_stage)
        {
            None => {
                return Err(context.invalid(format!(
                    "initial migration stage `{}` does not exist",
                    self.initial_stage
                )));
            }
            Some(0) => {}
            Some(_) => {
                return Err(context.invalid(format!(
                    "initial migration stage `{}` is not the first ordered stage",
                    self.initial_stage
                )));
            }
        }

        MigrationScenario::from_path(scenario_path, self)?;
        Ok(())
    }
}

impl MigrationScenario {
    pub fn from_path(path: impl AsRef<Path>, sequence: &MigrationSequence) -> ManifestResult<Self> {
        let path = path.as_ref();
        let scenario: Self = read_manifest(path)?;
        scenario.validate_at(path, sequence)?;
        Ok(scenario)
    }

    pub fn validate(&self, sequence: &MigrationSequence) -> ManifestResult<()> {
        self.validate_at(Path::new(&sequence.scenario), sequence)
    }

    fn validate_at(&self, path: &Path, sequence: &MigrationSequence) -> ManifestResult<()> {
        validate_scenario_text(path, "migration scenario name", &self.name)?;
        if self.steps.is_empty() {
            return Err(invalid_scenario(path, "migration scenario has no steps"));
        }

        let stage_indices = sequence
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| (stage.id.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        let mut step_ids = BTreeSet::new();
        let mut completed_step_namespaces = BTreeMap::new();
        let mut namespace_stages = BTreeMap::<String, usize>::new();
        let mut active_namespace = None::<String>;
        let mut injected_fault = None::<ActiveMigrationFault>;

        for step in &self.steps {
            validate_scenario_text(path, "migration scenario step id", &step.id)?;
            if !step_ids.insert(step.id.as_str()) {
                return Err(invalid_scenario(
                    path,
                    format!("duplicate migration scenario step id `{}`", step.id),
                ));
            }
            validate_expected_failure(path, step)?;

            let expects_failure = step.expect_failure.is_some();
            let mut successful_preview = None;
            let mut successful_activation = None;

            match &step.action {
                MigrationLifecycleAction::Start { stage, namespace } => {
                    let stage_index = scenario_stage_index(
                        path,
                        &stage_indices,
                        stage,
                        &format!("step `{}` start stage", step.id),
                    )?;
                    validate_scenario_identity(
                        path,
                        &format!("step `{}` namespace", step.id),
                        namespace,
                    )?;
                    if !expects_failure {
                        if let Some(previous_stage) = namespace_stages.get(namespace)
                            && *previous_stage != stage_index
                        {
                            return Err(invalid_scenario(
                                path,
                                format!(
                                    "step `{}` starts existing namespace `{namespace}` at stage `{stage}`, but it is at stage `{}`",
                                    step.id, sequence.stages[*previous_stage].id
                                ),
                            ));
                        }
                        namespace_stages.insert(namespace.clone(), stage_index);
                        active_namespace = Some(namespace.clone());
                    }
                }
                MigrationLifecycleAction::Dispatch {
                    public_source,
                    target,
                    payload,
                } => {
                    require_active_namespace(path, &step.id, &active_namespace)?;
                    validate_scenario_identity(
                        path,
                        &format!("step `{}` public source", step.id),
                        public_source,
                    )?;
                    if let Some(target) = target {
                        validate_scenario_identity(
                            path,
                            &format!("step `{}` target list", step.id),
                            &target.list,
                        )?;
                        if target.generation == 0 {
                            return Err(invalid_scenario(
                                path,
                                format!("step `{}` target generation must be positive", step.id),
                            ));
                        }
                    }
                    for field in payload.keys() {
                        validate_scenario_text(
                            path,
                            &format!("step `{}` payload field", step.id),
                            field,
                        )?;
                    }
                }
                MigrationLifecycleAction::Checkpoint | MigrationLifecycleAction::Restart => {
                    require_active_namespace(path, &step.id, &active_namespace)?;
                }
                MigrationLifecycleAction::PreviewStage { stage } => {
                    let (_, current_stage) = active_scenario_stage(
                        path,
                        &step.id,
                        &active_namespace,
                        &namespace_stages,
                    )?;
                    let target_stage = scenario_stage_index(
                        path,
                        &stage_indices,
                        stage,
                        &format!("step `{}` preview stage", step.id),
                    )?;
                    validate_forward_stage(path, step, current_stage, target_stage, sequence)?;
                    if !expects_failure {
                        successful_preview = Some(target_stage);
                    }
                }
                MigrationLifecycleAction::ActivateStage { stage, mode } => {
                    let (namespace, current_stage) = active_scenario_stage(
                        path,
                        &step.id,
                        &active_namespace,
                        &namespace_stages,
                    )?;
                    let target_stage = scenario_stage_index(
                        path,
                        &stage_indices,
                        stage,
                        &format!("step `{}` activation stage", step.id),
                    )?;
                    validate_forward_stage(path, step, current_stage, target_stage, sequence)?;
                    validate_activation_mode(path, step, *mode, current_stage, target_stage)?;
                    if !expects_failure
                        || step
                            .expect_failure
                            .as_ref()
                            .is_some_and(|failure| !failure.schema_unchanged)
                    {
                        successful_activation = Some((*mode, current_stage, target_stage));
                        namespace_stages.insert(namespace, target_stage);
                    }
                }
                MigrationLifecycleAction::StartOver { stage } => {
                    let namespace = require_active_namespace(path, &step.id, &active_namespace)?;
                    let target_stage = scenario_stage_index(
                        path,
                        &stage_indices,
                        stage,
                        &format!("step `{}` start-over stage", step.id),
                    )?;
                    if !expects_failure {
                        namespace_stages.insert(namespace, target_stage);
                    }
                }
                MigrationLifecycleAction::InjectFault { point, occurrence } => {
                    if expects_failure {
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` cannot expect the fault-injection control itself to fail",
                                step.id
                            ),
                        ));
                    }
                    if *occurrence == 0 {
                        return Err(invalid_scenario(
                            path,
                            format!("step `{}` fault occurrence must be positive", step.id),
                        ));
                    }
                    require_active_namespace(path, &step.id, &active_namespace)?;
                    if injected_fault.is_some() {
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` injects a fault while one is already active",
                                step.id
                            ),
                        ));
                    }
                    injected_fault = Some(ActiveMigrationFault {
                        point: *point,
                        occurrence: *occurrence,
                        compatible_actions_seen: 0,
                        observed: false,
                    });
                }
                MigrationLifecycleAction::ClearFault => {
                    require_active_namespace(path, &step.id, &active_namespace)?;
                    if expects_failure {
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` cannot expect the fault-clear control itself to fail",
                                step.id
                            ),
                        ));
                    }
                    let Some(fault) = injected_fault.take() else {
                        return Err(invalid_scenario(
                            path,
                            format!("step `{}` clears a fault when none is active", step.id),
                        ));
                    };
                    if !fault.observed {
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` clears fault `{}` before its expected failure",
                                step.id,
                                migration_fault_point_name(fault.point)
                            ),
                        ));
                    }
                }
            }

            validate_fault_outcome(path, step, &mut injected_fault)?;

            validate_migration_assertions(
                path,
                sequence,
                &stage_indices,
                step,
                &active_namespace,
                &namespace_stages,
                &completed_step_namespaces,
                successful_preview,
                successful_activation,
            )?;

            if let Some(namespace) = &active_namespace {
                completed_step_namespaces.insert(step.id.clone(), namespace.clone());
            }
        }

        if injected_fault.is_some() {
            return Err(invalid_scenario(
                path,
                "migration scenario ends with an injected fault still active",
            ));
        }
        Ok(())
    }
}

fn validate_expected_failure(path: &Path, step: &MigrationScenarioStep) -> ManifestResult<()> {
    let Some(failure) = &step.expect_failure else {
        return Ok(());
    };
    validate_scenario_identity(
        path,
        &format!("step `{}` expected failure code", step.id),
        &failure.code,
    )?;
    if let Some(message) = &failure.message_contains {
        validate_scenario_text(
            path,
            &format!("step `{}` expected failure message", step.id),
            message,
        )?;
    }
    Ok(())
}

fn validate_fault_outcome(
    path: &Path,
    step: &MigrationScenarioStep,
    injected_fault: &mut Option<ActiveMigrationFault>,
) -> ManifestResult<()> {
    let Some(fault) = injected_fault.as_mut() else {
        if step.expect_failure.is_some() {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` expects failure without an injected fault",
                    step.id
                ),
            ));
        }
        return Ok(());
    };

    if matches!(
        step.action,
        MigrationLifecycleAction::InjectFault { .. } | MigrationLifecycleAction::ClearFault
    ) {
        return Ok(());
    }
    if !fault_applies_to_action(fault.point, &step.action) {
        if step.expect_failure.is_some() {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` expects fault `{}` on an incompatible action",
                    step.id,
                    migration_fault_point_name(fault.point)
                ),
            ));
        }
        return Ok(());
    }

    fault.compatible_actions_seen += 1;
    let should_fail = fault.compatible_actions_seen == fault.occurrence;
    if should_fail != step.expect_failure.is_some() {
        let expected = if should_fail { "must" } else { "must not" };
        return Err(invalid_scenario(
            path,
            format!(
                "step `{}` {expected} expect injected fault `{}` occurrence {}",
                step.id,
                migration_fault_point_name(fault.point),
                fault.occurrence
            ),
        ));
    }
    if should_fail {
        let failure = step
            .expect_failure
            .as_ref()
            .expect("triggered fault requires an expected failure");
        let expected_unchanged = match fault.point {
            MigrationFaultPoint::AfterCheckpointAcknowledgement => (true, false, true),
            MigrationFaultPoint::AfterActivationCommit => (false, false, false),
            MigrationFaultPoint::BeforeCheckpoint
            | MigrationFaultPoint::DuringCheckpoint
            | MigrationFaultPoint::CandidateSettle
            | MigrationFaultPoint::BeforeActivationCommit
            | MigrationFaultPoint::DuringActivationCommit => (true, true, true),
        };
        let actual_unchanged = (
            failure.current_unchanged,
            failure.durable_unchanged,
            failure.schema_unchanged,
        );
        if actual_unchanged != expected_unchanged {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` fault `{}` requires current_unchanged={}, durable_unchanged={}, schema_unchanged={}",
                    step.id,
                    migration_fault_point_name(fault.point),
                    expected_unchanged.0,
                    expected_unchanged.1,
                    expected_unchanged.2,
                ),
            ));
        }
        fault.observed = true;
    }
    Ok(())
}

fn fault_applies_to_action(point: MigrationFaultPoint, action: &MigrationLifecycleAction) -> bool {
    match point {
        MigrationFaultPoint::BeforeCheckpoint
        | MigrationFaultPoint::DuringCheckpoint
        | MigrationFaultPoint::AfterCheckpointAcknowledgement => {
            matches!(action, MigrationLifecycleAction::Checkpoint)
        }
        MigrationFaultPoint::CandidateSettle => matches!(
            action,
            MigrationLifecycleAction::PreviewStage { .. }
                | MigrationLifecycleAction::ActivateStage { .. }
        ),
        MigrationFaultPoint::BeforeActivationCommit
        | MigrationFaultPoint::DuringActivationCommit
        | MigrationFaultPoint::AfterActivationCommit => {
            matches!(action, MigrationLifecycleAction::ActivateStage { .. })
        }
    }
}

fn migration_fault_point_name(point: MigrationFaultPoint) -> &'static str {
    match point {
        MigrationFaultPoint::BeforeCheckpoint => "before_checkpoint",
        MigrationFaultPoint::DuringCheckpoint => "during_checkpoint",
        MigrationFaultPoint::AfterCheckpointAcknowledgement => "after_checkpoint_acknowledgement",
        MigrationFaultPoint::CandidateSettle => "candidate_settle",
        MigrationFaultPoint::BeforeActivationCommit => "before_activation_commit",
        MigrationFaultPoint::DuringActivationCommit => "during_activation_commit",
        MigrationFaultPoint::AfterActivationCommit => "after_activation_commit",
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_migration_assertions(
    path: &Path,
    sequence: &MigrationSequence,
    stage_indices: &BTreeMap<&str, usize>,
    step: &MigrationScenarioStep,
    active_namespace: &Option<String>,
    namespace_stages: &BTreeMap<String, usize>,
    completed_step_namespaces: &BTreeMap<String, String>,
    successful_preview: Option<usize>,
    successful_activation: Option<(MigrationActivationMode, usize, usize)>,
) -> ManifestResult<()> {
    for assertion in &step.assertions {
        match assertion {
            MigrationAssertion::CurrentValue {
                path: value_path, ..
            }
            | MigrationAssertion::DurableValue {
                path: value_path, ..
            }
            | MigrationAssertion::CurrentAbsent { path: value_path }
            | MigrationAssertion::DurableAbsent { path: value_path } => {
                require_active_namespace(path, &step.id, active_namespace)?;
                validate_scenario_identity(
                    path,
                    &format!("step `{}` asserted value path", step.id),
                    value_path,
                )?;
            }
            MigrationAssertion::Schema {
                current_stage,
                durable_stage,
                preview_stage,
            } => {
                let (_, modeled_stage) =
                    active_scenario_stage(path, &step.id, active_namespace, namespace_stages)?;
                let current_index = scenario_stage_index(
                    path,
                    stage_indices,
                    current_stage,
                    &format!("step `{}` asserted current stage", step.id),
                )?;
                let durable_index = scenario_stage_index(
                    path,
                    stage_indices,
                    durable_stage,
                    &format!("step `{}` asserted durable stage", step.id),
                )?;
                if current_index != modeled_stage || durable_index != modeled_stage {
                    return Err(invalid_scenario(
                        path,
                        format!(
                            "step `{}` asserts current/durable stages `{current_stage}`/`{durable_stage}`, but the lifecycle is at `{}`",
                            step.id, sequence.stages[modeled_stage].id
                        ),
                    ));
                }
                match (preview_stage, successful_preview) {
                    (Some(stage), Some(preview_index)) => {
                        let asserted_index = scenario_stage_index(
                            path,
                            stage_indices,
                            stage,
                            &format!("step `{}` asserted preview stage", step.id),
                        )?;
                        if asserted_index != preview_index {
                            return Err(invalid_scenario(
                                path,
                                format!(
                                    "step `{}` asserts preview stage `{stage}`, but previews `{}`",
                                    step.id, sequence.stages[preview_index].id
                                ),
                            ));
                        }
                    }
                    (Some(stage), None) => {
                        scenario_stage_index(
                            path,
                            stage_indices,
                            stage,
                            &format!("step `{}` asserted preview stage", step.id),
                        )?;
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` asserts a successful preview without a successful preview action",
                                step.id
                            ),
                        ));
                    }
                    (None, _) => {}
                }
            }
            MigrationAssertion::Edges {
                mode,
                from_stage,
                to_stage,
                applied,
            } => {
                let from_index = scenario_stage_index(
                    path,
                    stage_indices,
                    from_stage,
                    &format!("step `{}` asserted edge source", step.id),
                )?;
                let to_index = scenario_stage_index(
                    path,
                    stage_indices,
                    to_stage,
                    &format!("step `{}` asserted edge target", step.id),
                )?;
                validate_activation_mode(path, step, *mode, from_index, to_index)?;
                let expected = expected_migration_edges(sequence, from_index, to_index);
                if applied != &expected {
                    return Err(invalid_scenario(
                        path,
                        format!(
                            "step `{}` asserted migration edges do not match the ordered sequence from `{from_stage}` to `{to_stage}`",
                            step.id
                        ),
                    ));
                }
                if successful_activation != Some((*mode, from_index, to_index)) {
                    return Err(invalid_scenario(
                        path,
                        format!(
                            "step `{}` edge assertion does not match a successful activation action",
                            step.id
                        ),
                    ));
                }
            }
            MigrationAssertion::List {
                path: list_path,
                rows,
                next_key,
                ..
            } => {
                require_active_namespace(path, &step.id, active_namespace)?;
                validate_scenario_identity(
                    path,
                    &format!("step `{}` asserted list path", step.id),
                    list_path,
                )?;
                validate_list_assertion(path, step, rows, *next_key)?;
            }
            MigrationAssertion::NamespaceIsolation {
                namespace,
                unchanged_since,
            } => {
                validate_scenario_identity(
                    path,
                    &format!("step `{}` isolated namespace", step.id),
                    namespace,
                )?;
                if !namespace_stages.contains_key(namespace) {
                    return Err(invalid_scenario(
                        path,
                        format!("step `{}` asserts unknown namespace `{namespace}`", step.id),
                    ));
                }
                let baseline_namespace = completed_step_namespaces
                    .get(unchanged_since)
                    .ok_or_else(|| {
                        invalid_scenario(
                            path,
                            format!(
                                "step `{}` namespace baseline `{unchanged_since}` is not an earlier active step",
                                step.id
                            ),
                        )
                    })?;
                if baseline_namespace != namespace {
                    return Err(invalid_scenario(
                        path,
                        format!(
                            "step `{}` namespace baseline `{unchanged_since}` belongs to `{baseline_namespace}`, not `{namespace}`",
                            step.id
                        ),
                    ));
                }
                if active_namespace.as_deref() == Some(namespace) {
                    return Err(invalid_scenario(
                        path,
                        format!(
                            "step `{}` must exercise a different active namespace before asserting isolation for `{namespace}`",
                            step.id
                        ),
                    ));
                }
            }
            MigrationAssertion::NamespaceEquivalent {
                namespace,
                other_namespace,
            } => {
                for asserted_namespace in [namespace, other_namespace] {
                    validate_scenario_identity(
                        path,
                        &format!("step `{}` equivalent namespace", step.id),
                        asserted_namespace,
                    )?;
                    if !namespace_stages.contains_key(asserted_namespace) {
                        return Err(invalid_scenario(
                            path,
                            format!(
                                "step `{}` compares unknown namespace `{asserted_namespace}`",
                                step.id
                            ),
                        ));
                    }
                }
                if namespace == other_namespace {
                    return Err(invalid_scenario(
                        path,
                        format!("step `{}` compares a namespace with itself", step.id),
                    ));
                }
                if ![namespace.as_str(), other_namespace.as_str()]
                    .contains(&active_namespace.as_deref().unwrap_or_default())
                {
                    return Err(invalid_scenario(
                        path,
                        format!("step `{}` must have one compared namespace active", step.id),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_list_assertion(
    path: &Path,
    step: &MigrationScenarioStep,
    rows: &[MigrationListRowAssertion],
    next_key: u64,
) -> ManifestResult<()> {
    let mut keys = BTreeSet::new();
    let mut previous_key = None;
    for row in rows {
        if previous_key.is_some_and(|key| row.key <= key) {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` list rows must be in strictly increasing key order",
                    step.id
                ),
            ));
        }
        previous_key = Some(row.key);
        if !keys.insert(row.key) {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` list assertion repeats row key {}",
                    step.id, row.key
                ),
            ));
        }
        if row.generation == 0 {
            return Err(invalid_scenario(
                path,
                format!(
                    "step `{}` list row {} generation must be positive",
                    step.id, row.key
                ),
            ));
        }
        let mut touched_fields = BTreeSet::new();
        for field in &row.touched_fields {
            validate_scenario_text(
                path,
                &format!("step `{}` touched row field", step.id),
                field,
            )?;
            if !touched_fields.insert(field.as_str()) {
                return Err(invalid_scenario(
                    path,
                    format!(
                        "step `{}` list row {} repeats touched field `{field}`",
                        step.id, row.key
                    ),
                ));
            }
            if !row.values.contains_key(field) {
                return Err(invalid_scenario(
                    path,
                    format!(
                        "step `{}` list row {} touches `{field}` without asserting its value",
                        step.id, row.key
                    ),
                ));
            }
        }
        for field in row.values.keys() {
            validate_scenario_text(
                path,
                &format!("step `{}` asserted row field", step.id),
                field,
            )?;
        }
    }
    if next_key == 0 {
        return Err(invalid_scenario(
            path,
            format!("step `{}` list next_key must be positive", step.id),
        ));
    }
    if let Some(maximum_key) = keys.last()
        && next_key <= *maximum_key
    {
        return Err(invalid_scenario(
            path,
            format!(
                "step `{}` list next_key {next_key} does not advance past row key {maximum_key}",
                step.id
            ),
        ));
    }
    Ok(())
}

fn expected_migration_edges(
    sequence: &MigrationSequence,
    from_index: usize,
    to_index: usize,
) -> Vec<MigrationEdgeAssertion> {
    sequence.stages[from_index..=to_index]
        .windows(2)
        .map(|stages| MigrationEdgeAssertion {
            from_stage: stages[0].id.clone(),
            to_stage: stages[1].id.clone(),
        })
        .collect()
}

fn validate_activation_mode(
    path: &Path,
    step: &MigrationScenarioStep,
    mode: MigrationActivationMode,
    from_index: usize,
    to_index: usize,
) -> ManifestResult<()> {
    if to_index <= from_index {
        return Err(invalid_scenario(
            path,
            format!("step `{}` activation is not forward", step.id),
        ));
    }
    let edge_count = to_index - from_index;
    let valid = match mode {
        MigrationActivationMode::Incremental => edge_count == 1,
        MigrationActivationMode::Skipped => edge_count > 1,
    };
    if !valid {
        return Err(invalid_scenario(
            path,
            format!(
                "step `{}` uses {mode:?} activation mode for {edge_count} migration edge(s)",
                step.id
            ),
        ));
    }
    Ok(())
}

fn validate_forward_stage(
    path: &Path,
    step: &MigrationScenarioStep,
    current_stage: usize,
    target_stage: usize,
    sequence: &MigrationSequence,
) -> ManifestResult<()> {
    if target_stage <= current_stage {
        return Err(invalid_scenario(
            path,
            format!(
                "step `{}` targets stage `{}` from `{}`; migration lifecycle targets must be forward",
                step.id, sequence.stages[target_stage].id, sequence.stages[current_stage].id
            ),
        ));
    }
    Ok(())
}

fn active_scenario_stage(
    path: &Path,
    step_id: &str,
    active_namespace: &Option<String>,
    namespace_stages: &BTreeMap<String, usize>,
) -> ManifestResult<(String, usize)> {
    let namespace = require_active_namespace(path, step_id, active_namespace)?;
    let stage = namespace_stages.get(&namespace).copied().ok_or_else(|| {
        invalid_scenario(
            path,
            format!("step `{step_id}` active namespace `{namespace}` has no stage"),
        )
    })?;
    Ok((namespace, stage))
}

fn require_active_namespace(
    path: &Path,
    step_id: &str,
    active_namespace: &Option<String>,
) -> ManifestResult<String> {
    active_namespace.clone().ok_or_else(|| {
        invalid_scenario(
            path,
            format!("step `{step_id}` requires an active namespace"),
        )
    })
}

fn scenario_stage_index(
    path: &Path,
    stage_indices: &BTreeMap<&str, usize>,
    stage: &str,
    field: &str,
) -> ManifestResult<usize> {
    validate_scenario_identity(path, field, stage)?;
    stage_indices
        .get(stage)
        .copied()
        .ok_or_else(|| invalid_scenario(path, format!("{field} `{stage}` does not exist")))
}

fn validate_scenario_identity(path: &Path, field: &str, value: &str) -> ManifestResult<()> {
    validate_scenario_text(path, field, value)?;
    if value != value.trim() || value.chars().any(char::is_whitespace) {
        return Err(invalid_scenario(
            path,
            format!("{field} must be a stable whitespace-free value"),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid_scenario(
            path,
            format!("{field} contains a control character"),
        ));
    }
    Ok(())
}

fn validate_scenario_text(path: &Path, field: &str, value: &str) -> ManifestResult<()> {
    if value.trim().is_empty() {
        Err(invalid_scenario(path, format!("{field} is empty")))
    } else {
        Ok(())
    }
}

fn invalid_scenario(path: &Path, message: impl Into<String>) -> ManifestError {
    ManifestError::Invalid {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

fn read_manifest<T>(path: &Path) -> ManifestResult<T>
where
    T: DeserializeOwned,
{
    let text = fs::read_to_string(path).map_err(|source| ManifestError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ManifestError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_application(
    context: &ValidationContext,
    example_id: &str,
    application: &ApplicationManifest,
) -> ManifestResult<()> {
    validate_identity_text(
        context,
        &format!("example `{example_id}` application package_id"),
        &application.package_id,
    )?;
    validate_identity_text(
        context,
        &format!("example `{example_id}` application deployment_domain"),
        &application.deployment_domain,
    )
}

fn validate_identity_text(
    context: &ValidationContext,
    field: &str,
    value: &str,
) -> ManifestResult<()> {
    validate_required_text(context, field, value)?;
    if value != value.trim() || value.chars().any(char::is_whitespace) {
        return Err(context.invalid(format!("{field} must be a stable whitespace-free value")));
    }
    if value.chars().any(char::is_control) {
        return Err(context.invalid(format!("{field} contains a control character")));
    }
    Ok(())
}

fn validate_required_text(
    context: &ValidationContext,
    field: &str,
    value: &str,
) -> ManifestResult<()> {
    if value.trim().is_empty() {
        Err(context.invalid(format!("{field} is empty")))
    } else {
        Ok(())
    }
}

fn validate_source_project(
    context: &ValidationContext,
    owner: &str,
    project: &SourceProject,
) -> ManifestResult<()> {
    context.required_file(&project.source, &format!("{owner} source"))?;
    validate_file_paths(
        context,
        &project.source_files,
        &format!("{owner} source file"),
    )
}

fn validate_file_paths(
    context: &ValidationContext,
    paths: &[String],
    field: &str,
) -> ManifestResult<()> {
    for path in paths {
        context.required_file(path, field)?;
    }
    Ok(())
}

struct ValidationContext {
    manifest_path: PathBuf,
    examples_root: PathBuf,
    canonical_examples_root: PathBuf,
}

impl ValidationContext {
    fn new(manifest_path: &Path, examples_root: &Path) -> ManifestResult<Self> {
        let manifest_path =
            absolute_lexical_path(manifest_path).map_err(|source| ManifestError::Read {
                path: manifest_path.to_path_buf(),
                source,
            })?;
        let examples_root =
            absolute_lexical_path(examples_root).map_err(|source| ManifestError::Read {
                path: examples_root.to_path_buf(),
                source,
            })?;
        let canonical_examples_root =
            fs::canonicalize(&examples_root).map_err(|source| ManifestError::Read {
                path: examples_root.clone(),
                source,
            })?;
        if !canonical_examples_root.is_dir() {
            return Err(ManifestError::Invalid {
                path: manifest_path,
                message: format!(
                    "examples root `{}` is not a directory",
                    examples_root.display()
                ),
            });
        }
        Ok(Self {
            manifest_path,
            examples_root,
            canonical_examples_root,
        })
    }

    fn invalid(&self, message: impl Into<String>) -> ManifestError {
        ManifestError::Invalid {
            path: self.manifest_path.clone(),
            message: message.into(),
        }
    }

    fn required_file(&self, value: &str, field: &str) -> ManifestResult<PathBuf> {
        self.required_path(value, field, RequiredPathKind::File)
    }

    fn required_directory(&self, value: &str, field: &str) -> ManifestResult<PathBuf> {
        self.required_path(value, field, RequiredPathKind::Directory)
    }

    fn required_path(
        &self,
        value: &str,
        field: &str,
        kind: RequiredPathKind,
    ) -> ManifestResult<PathBuf> {
        validate_required_text(self, field, value)?;
        let path = Path::new(value);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else if path.components().next().is_some_and(|component| {
            matches!(component, Component::Normal(name) if Some(name) == self.examples_root.file_name())
        }) {
            self.examples_root
                .parent()
                .unwrap_or(&self.examples_root)
                .join(path)
        } else {
            self.examples_root.join(path)
        };
        let candidate = normalize_lexically(&candidate);
        if !candidate.starts_with(&self.examples_root) {
            return Err(self.invalid(format!(
                "{field} path `{value}` escapes examples root `{}`",
                self.examples_root.display()
            )));
        }

        let metadata = fs::metadata(&candidate).map_err(|source| {
            self.invalid(format!(
                "{field} path `{value}` is missing or inaccessible: {source}"
            ))
        })?;
        let has_expected_kind = match kind {
            RequiredPathKind::File => metadata.is_file(),
            RequiredPathKind::Directory => metadata.is_dir(),
        };
        if !has_expected_kind {
            return Err(self.invalid(format!("{field} path `{value}` is not a {}", kind.label())));
        }

        let canonical = fs::canonicalize(&candidate).map_err(|source| {
            self.invalid(format!(
                "failed to resolve {field} path `{value}`: {source}"
            ))
        })?;
        if !canonical.starts_with(&self.canonical_examples_root) {
            return Err(self.invalid(format!(
                "{field} path `{value}` resolves outside examples root `{}`",
                self.examples_root.display()
            )));
        }
        Ok(canonical)
    }
}

#[derive(Clone, Copy)]
enum RequiredPathKind {
    File,
    Directory,
}

impl RequiredPathKind {
    fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

fn absolute_lexical_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(normalize_lexically(path))
    } else {
        Ok(normalize_lexically(&std::env::current_dir()?.join(path)))
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestTree {
        root: PathBuf,
        examples: PathBuf,
    }

    impl TestTree {
        fn new() -> Self {
            let ordinal = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "boon-example-manifest-{}-{ordinal}",
                std::process::id()
            ));
            let examples = root.join("examples");
            fs::create_dir_all(&examples).expect("create test examples directory");
            Self { root, examples }
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create test file parent");
            }
            fs::write(&path, contents).expect("write test file");
            path
        }

        fn write_common_example_files(&self) {
            self.write("examples/app.bn", "value: 1\n");
            self.write("examples/app.scn", "name = \"App\"\nsource = \"input\"\n");
            self.write("examples/app.budget.toml", "name = \"app\"\n");
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn basic_manifest(extra: &str) -> String {
        format!(
            concat!(
                "[[example]]\n",
                "id = \"app\"\n",
                "label = \"App\"\n",
                "source = \"examples/app.bn\"\n",
                "scenario = \"examples/app.scn\"\n",
                "budget = \"examples/app.budget.toml\"\n",
                "{extra}\n",
            ),
            extra = extra
        )
    }

    fn valid_sequence_text() -> &'static str {
        concat!(
            "initial_stage = \"v1\"\n",
            "scenario = \"examples/migration/migration.scn\"\n",
            "\n",
            "[[stage]]\n",
            "id = \"v1\"\n",
            "label = \"Version 1\"\n",
            "schema_version = 1\n",
            "source = \"examples/migration/v1.bn\"\n",
            "\n",
            "[[stage]]\n",
            "id = \"v2\"\n",
            "label = \"Version 2\"\n",
            "schema_version = 2\n",
            "source = \"examples/migration/v2.bn\"\n",
            "source_files = [\"examples/migration/shared.bn\"]\n",
            "\n",
            "[[stage]]\n",
            "id = \"v4\"\n",
            "label = \"Version 4\"\n",
            "schema_version = 4\n",
            "source = \"examples/migration/v4.bn\"\n",
        )
    }

    fn valid_migration_scenario_text() -> &'static str {
        concat!(
            "name = \"Migration\"\n",
            "\n",
            "[[step]]\n",
            "id = \"start-v1\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "\n",
            "[[step]]\n",
            "id = \"preview-v4\"\n",
            "action = { kind = \"preview\", stage = \"v4\" }\n",
            "  [[step.assert]]\n",
            "  kind = \"schema\"\n",
            "  current_stage = \"v1\"\n",
            "  durable_stage = \"v1\"\n",
            "  preview_stage = \"v4\"\n",
            "\n",
            "[[step]]\n",
            "id = \"activate-v4\"\n",
            "action = { kind = \"activate\", stage = \"v4\", mode = \"skipped\" }\n",
            "  [[step.assert]]\n",
            "  kind = \"edges\"\n",
            "  mode = \"skipped\"\n",
            "  from_stage = \"v1\"\n",
            "  to_stage = \"v4\"\n",
            "  applied = [\n",
            "    { from_stage = \"v1\", to_stage = \"v2\" },\n",
            "    { from_stage = \"v2\", to_stage = \"v4\" },\n",
            "  ]\n",
        )
    }

    fn write_migration_files(tree: &TestTree) {
        tree.write("examples/migration/v1.bn", "value: 1\n");
        tree.write("examples/migration/v2.bn", "value: 2\n");
        tree.write("examples/migration/v4.bn", "value: 4\n");
        tree.write("examples/migration/shared.bn", "shared: 1\n");
        tree.write(
            "examples/migration/migration.scn",
            valid_migration_scenario_text(),
        );
    }

    #[test]
    fn repository_manifest_loads_with_strict_models() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/manifest.toml");
        let manifest = ExampleManifest::from_path(manifest_path).expect("load repository manifest");
        assert!(manifest.example.iter().any(|entry| entry.id == "cells"));
        assert!(manifest.example.iter().any(|entry| entry.id == "counter"));
        let encoded = toml::to_string(&manifest).expect("serialize repository manifest");
        let decoded: ExampleManifest =
            toml::from_str(&encoded).expect("deserialize repository manifest");
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn strict_models_reject_unknown_fields_and_runtime_namespace() {
        let unknown_entry = basic_manifest("unexpected = true");
        assert!(toml::from_str::<ExampleManifest>(&unknown_entry).is_err());

        let runtime_namespace = basic_manifest(concat!(
            "application = { package_id = \"dev.boon.app\", ",
            "deployment_domain = \"builtin\", runtime_namespace = \"manual\" }"
        ));
        assert!(toml::from_str::<ExampleManifest>(&runtime_namespace).is_err());
    }

    #[test]
    fn duplicate_example_ids_are_rejected() {
        let tree = TestTree::new();
        tree.write_common_example_files();
        let text = format!("{}\n{}", basic_manifest(""), basic_manifest(""));
        let path = tree.write("examples/manifest.toml", &text);
        let error = ExampleManifest::from_path(path).expect_err("reject duplicate example id");
        assert!(error.to_string().contains("duplicate example id `app`"));
    }

    #[test]
    fn stable_application_identity_is_required_for_migrations() {
        let tree = TestTree::new();
        tree.write_common_example_files();
        write_migration_files(&tree);
        tree.write("examples/migration/sequence.toml", valid_sequence_text());
        let path = tree.write(
            "examples/manifest.toml",
            &basic_manifest("migration_sequence = \"examples/migration/sequence.toml\""),
        );
        let error = ExampleManifest::from_path(path).expect_err("require application identity");
        assert!(error.to_string().contains("no stable application identity"));

        let invalid_identity = basic_manifest(concat!(
            "application = { package_id = \"dev.boon.app\", ",
            "deployment_domain = \"two words\" }"
        ));
        let path = tree.write("examples/manifest.toml", &invalid_identity);
        let error = ExampleManifest::from_path(path).expect_err("reject unstable identity");
        assert!(error.to_string().contains("whitespace-free"));
    }

    #[test]
    fn paths_cannot_escape_the_examples_root() {
        let tree = TestTree::new();
        tree.write_common_example_files();
        tree.write("outside.bn", "value: 1\n");
        let manifest = basic_manifest("")
            .replace("source = \"examples/app.bn\"", "source = \"../outside.bn\"");
        let path = tree.write("examples/manifest.toml", &manifest);
        let error = ExampleManifest::from_path(path).expect_err("reject path escape");
        assert!(error.to_string().contains("escapes examples root"));
    }

    #[test]
    fn missing_source_and_scenario_files_are_rejected() {
        let tree = TestTree::new();
        tree.write("examples/app.budget.toml", "name = \"app\"\n");
        let path = tree.write("examples/manifest.toml", &basic_manifest(""));
        let source_error = ExampleManifest::from_path(&path).expect_err("reject missing source");
        assert!(source_error.to_string().contains("example `app` source"));

        tree.write("examples/app.bn", "value: 1\n");
        let scenario_error = ExampleManifest::from_path(path).expect_err("reject missing scenario");
        assert!(
            scenario_error
                .to_string()
                .contains("example `app` scenario")
        );
    }

    #[test]
    fn valid_multi_stage_sequence_loads_in_declared_order() {
        let tree = TestTree::new();
        tree.write_common_example_files();
        write_migration_files(&tree);
        tree.write("examples/migration/sequence.toml", valid_sequence_text());
        let manifest = basic_manifest(concat!(
            "application = { package_id = \"dev.boon.app\", ",
            "deployment_domain = \"builtin\" }\n",
            "migration_sequence = \"examples/migration/sequence.toml\""
        ));
        let path = tree.write("examples/manifest.toml", &manifest);

        let loaded = ExampleManifest::from_path(path).expect("load valid migration manifest");
        let sequence = MigrationSequence::from_path(
            tree.examples.join("migration/sequence.toml"),
            &tree.examples,
        )
        .expect("load valid migration sequence");
        assert_eq!(
            loaded.example[0].application_identity().unwrap().package_id,
            "dev.boon.app"
        );
        assert_eq!(
            sequence
                .stages
                .iter()
                .map(|stage| (stage.id.as_str(), stage.schema_version))
                .collect::<Vec<_>>(),
            vec![("v1", 1), ("v2", 2), ("v4", 4)]
        );
        assert_eq!(sequence.stages[1].source_project().source_files.len(), 1);
    }

    #[test]
    fn malformed_stage_order_ids_and_versions_are_rejected() {
        let tree = TestTree::new();
        write_migration_files(&tree);

        let duplicate_id = valid_sequence_text().replace("id = \"v2\"", "id = \"v1\"");
        let sequence: MigrationSequence =
            toml::from_str(&duplicate_id).expect("parse duplicate id");
        let error = sequence
            .validate(&tree.examples)
            .expect_err("reject duplicate stage id");
        assert!(error.to_string().contains("duplicate migration stage id"));

        let duplicate_version =
            valid_sequence_text().replace("schema_version = 2", "schema_version = 1");
        let sequence: MigrationSequence =
            toml::from_str(&duplicate_version).expect("parse duplicate version");
        let error = sequence
            .validate(&tree.examples)
            .expect_err("reject duplicate version");
        assert!(
            error
                .to_string()
                .contains("duplicate migration schema version")
        );

        let non_forward =
            valid_sequence_text().replacen("schema_version = 1", "schema_version = 3", 1);
        let sequence: MigrationSequence =
            toml::from_str(&non_forward).expect("parse non-forward version");
        let error = sequence
            .validate(&tree.examples)
            .expect_err("reject non-forward version");
        assert!(error.to_string().contains("is not forward"));

        let invalid_initial =
            valid_sequence_text().replace("initial_stage = \"v1\"", "initial_stage = \"v2\"");
        let sequence: MigrationSequence =
            toml::from_str(&invalid_initial).expect("parse initial stage");
        let error = sequence
            .validate(&tree.examples)
            .expect_err("reject non-first initial stage");
        assert!(error.to_string().contains("is not the first ordered stage"));

        let missing_initial =
            valid_sequence_text().replace("initial_stage = \"v1\"", "initial_stage = \"missing\"");
        let sequence: MigrationSequence =
            toml::from_str(&missing_initial).expect("parse missing initial stage");
        let error = sequence
            .validate(&tree.examples)
            .expect_err("reject missing initial stage");
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn missing_migration_sequence_stage_and_scenario_files_are_rejected() {
        let tree = TestTree::new();
        tree.write_common_example_files();
        let manifest = basic_manifest(concat!(
            "application = { package_id = \"dev.boon.app\", ",
            "deployment_domain = \"builtin\" }\n",
            "migration_sequence = \"examples/migration/sequence.toml\""
        ));
        let path = tree.write("examples/manifest.toml", &manifest);
        let error = ExampleManifest::from_path(&path).expect_err("reject missing sequence");
        assert!(error.to_string().contains("migration sequence"));

        write_migration_files(&tree);
        tree.write("examples/migration/sequence.toml", valid_sequence_text());
        fs::remove_file(tree.examples.join("migration/v2.bn")).expect("remove stage source");
        let error = ExampleManifest::from_path(&path).expect_err("reject missing stage source");
        assert!(error.to_string().contains("migration stage `v2` source"));

        tree.write("examples/migration/v2.bn", "value: 2\n");
        fs::remove_file(tree.examples.join("migration/migration.scn"))
            .expect("remove migration scenario");
        let error =
            ExampleManifest::from_path(path).expect_err("reject missing migration scenario");
        assert!(error.to_string().contains("migration scenario"));
    }

    #[test]
    fn migration_scenario_serde_models_are_strict_at_every_level() {
        let unknown_top_level = valid_migration_scenario_text().replacen(
            "name = \"Migration\"\n",
            "name = \"Migration\"\nunexpected = true\n",
            1,
        );
        assert!(toml::from_str::<MigrationScenario>(&unknown_top_level).is_err());

        let unknown_action = valid_migration_scenario_text().replacen(
            "namespace = \"test\" }",
            "namespace = \"test\", unexpected = true }",
            1,
        );
        assert!(toml::from_str::<MigrationScenario>(&unknown_action).is_err());

        let unknown_assertion = valid_migration_scenario_text().replacen(
            "preview_stage = \"v4\"",
            "preview_stage = \"v4\"\nunexpected = true",
            1,
        );
        assert!(toml::from_str::<MigrationScenario>(&unknown_assertion).is_err());

        let unknown_row = concat!(
            "name = \"strict-row\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step.assert]]\n",
            "kind = \"list\"\n",
            "scope = \"current\"\n",
            "path = \"store.rows\"\n",
            "next_key = 2\n",
            "rows = [{ key = 1, generation = 1, unexpected = true }]\n",
        );
        assert!(toml::from_str::<MigrationScenario>(unknown_row).is_err());

        let legacy_dispatch = concat!(
            "name = \"legacy-dispatch\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step]]\n",
            "id = \"dispatch\"\n",
            "action = { kind = \"dispatch\", source = \"store.sources.press\" }\n",
        );
        assert!(toml::from_str::<MigrationScenario>(legacy_dispatch).is_err());
    }

    #[test]
    fn migration_scenario_rejects_unknown_stages_and_incorrect_edge_paths() {
        let sequence: MigrationSequence =
            toml::from_str(valid_sequence_text()).expect("parse sequence");

        let unknown_stage = valid_migration_scenario_text().replacen(
            "action = { kind = \"preview\", stage = \"v4\" }",
            "action = { kind = \"preview\", stage = \"missing\" }",
            1,
        );
        let scenario: MigrationScenario = toml::from_str(&unknown_stage).expect("parse scenario");
        let error = scenario
            .validate(&sequence)
            .expect_err("reject unknown preview stage");
        assert!(error.to_string().contains("stage `missing` does not exist"));

        let incorrect_edges = valid_migration_scenario_text().replace(
            "{ from_stage = \"v2\", to_stage = \"v4\" }",
            "{ from_stage = \"v1\", to_stage = \"v4\" }",
        );
        let scenario: MigrationScenario =
            toml::from_str(&incorrect_edges).expect("parse incorrect edges");
        let error = scenario
            .validate(&sequence)
            .expect_err("reject non-sequential edge list");
        assert!(
            error
                .to_string()
                .contains("do not match the ordered sequence")
        );

        let incorrect_mode = valid_migration_scenario_text().replace("skipped", "incremental");
        let scenario: MigrationScenario =
            toml::from_str(&incorrect_mode).expect("parse incorrect activation mode");
        let error = scenario
            .validate(&sequence)
            .expect_err("reject multi-edge incremental activation");
        assert!(error.to_string().contains("Incremental activation mode"));
    }

    #[test]
    fn migration_scenario_validates_lifecycle_namespace_and_list_invariants() {
        let sequence: MigrationSequence =
            toml::from_str(valid_sequence_text()).expect("parse sequence");
        let action_before_start: MigrationScenario = toml::from_str(concat!(
            "name = \"no-start\"\n",
            "[[step]]\n",
            "id = \"checkpoint\"\n",
            "action = { kind = \"checkpoint\" }\n",
        ))
        .expect("parse action-before-start scenario");
        let error = action_before_start
            .validate(&sequence)
            .expect_err("require a started namespace");
        assert!(error.to_string().contains("requires an active namespace"));

        let invalid_list: MigrationScenario = toml::from_str(concat!(
            "name = \"invalid-list\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step.assert]]\n",
            "kind = \"list\"\n",
            "scope = \"durable\"\n",
            "path = \"store.rows\"\n",
            "next_key = 1\n",
            "rows = [{ key = 1, generation = 0, values = { text = \"value\" }, touched_fields = [\"text\"] }]\n",
        ))
        .expect("parse invalid-list scenario");
        let error = invalid_list
            .validate(&sequence)
            .expect_err("reject zero row generation");
        assert!(error.to_string().contains("generation must be positive"));

        let invalid_namespace_baseline: MigrationScenario = toml::from_str(concat!(
            "name = \"invalid-isolation\"\n",
            "[[step]]\n",
            "id = \"start-a\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"a\" }\n",
            "[[step]]\n",
            "id = \"start-b\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"b\" }\n",
            "[[step]]\n",
            "id = \"start-c\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"c\" }\n",
            "[[step.assert]]\n",
            "kind = \"namespace_isolation\"\n",
            "namespace = \"a\"\n",
            "unchanged_since = \"start-b\"\n",
        ))
        .expect("parse invalid-isolation scenario");
        let error = invalid_namespace_baseline
            .validate(&sequence)
            .expect_err("reject baseline from another namespace");
        assert!(error.to_string().contains("belongs to `b`, not `a`"));
    }

    #[test]
    fn migration_scenario_validates_fault_controls_and_round_trips() {
        let sequence: MigrationSequence =
            toml::from_str(valid_sequence_text()).expect("parse sequence");
        let invalid_fault: MigrationScenario = toml::from_str(concat!(
            "name = \"invalid-fault\"\n",
            "[[step]]\n",
            "id = \"fault\"\n",
            "action = { kind = \"inject_fault\", point = \"before_checkpoint\", occurrence = 0 }\n",
        ))
        .expect("parse invalid-fault scenario");
        let error = invalid_fault
            .validate(&sequence)
            .expect_err("reject zero fault occurrence");
        assert!(
            error
                .to_string()
                .contains("fault occurrence must be positive")
        );

        let scenario: MigrationScenario =
            toml::from_str(valid_migration_scenario_text()).expect("parse valid scenario");
        scenario.validate(&sequence).expect("validate scenario");
        let encoded = toml::to_string(&scenario).expect("serialize scenario");
        let decoded: MigrationScenario = toml::from_str(&encoded).expect("deserialize scenario");
        decoded.validate(&sequence).expect("validate round trip");
        assert_eq!(decoded, scenario);
    }

    #[test]
    fn migration_scenario_requires_a_compatible_observed_fault_and_ordered_rows() {
        let sequence: MigrationSequence =
            toml::from_str(valid_sequence_text()).expect("parse sequence");

        let unobserved_fault: MigrationScenario = toml::from_str(concat!(
            "name = \"unobserved-fault\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step]]\n",
            "id = \"inject\"\n",
            "action = { kind = \"inject_fault\", point = \"candidate_settle\" }\n",
            "[[step]]\n",
            "id = \"clear\"\n",
            "action = { kind = \"clear_fault\" }\n",
        ))
        .expect("parse unobserved fault scenario");
        let error = unobserved_fault
            .validate(&sequence)
            .expect_err("reject an unobserved injected fault");
        assert!(error.to_string().contains("before its expected failure"));

        let incompatible_fault: MigrationScenario = toml::from_str(concat!(
            "name = \"incompatible-fault\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step]]\n",
            "id = \"inject\"\n",
            "action = { kind = \"inject_fault\", point = \"candidate_settle\" }\n",
            "[[step]]\n",
            "id = \"checkpoint\"\n",
            "action = { kind = \"checkpoint\" }\n",
            "expect_failure = { code = \"candidate_settle_failed\", current_unchanged = true, durable_unchanged = true }\n",
        ))
        .expect("parse incompatible fault scenario");
        let error = incompatible_fault
            .validate(&sequence)
            .expect_err("reject an incompatible fault action");
        assert!(error.to_string().contains("incompatible action"));

        let weak_failure: MigrationScenario = toml::from_str(concat!(
            "name = \"weak-failure\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step]]\n",
            "id = \"inject\"\n",
            "action = { kind = \"inject_fault\", point = \"candidate_settle\" }\n",
            "[[step]]\n",
            "id = \"preview\"\n",
            "action = { kind = \"preview\", stage = \"v4\" }\n",
            "expect_failure = { code = \"candidate_settle_failed\", current_unchanged = true, durable_unchanged = false }\n",
        ))
        .expect("parse weak failure scenario");
        let error = weak_failure
            .validate(&sequence)
            .expect_err("reject a failure without durable rollback");
        assert!(
            error
                .to_string()
                .contains("requires current_unchanged=true, durable_unchanged=true")
        );

        let unordered_rows: MigrationScenario = toml::from_str(concat!(
            "name = \"unordered-rows\"\n",
            "[[step]]\n",
            "id = \"start\"\n",
            "action = { kind = \"start\", stage = \"v1\", namespace = \"test\" }\n",
            "[[step.assert]]\n",
            "kind = \"list\"\n",
            "scope = \"current\"\n",
            "path = \"store.rows\"\n",
            "next_key = 3\n",
            "rows = [\n",
            "  { key = 2, generation = 1 },\n",
            "  { key = 1, generation = 1 },\n",
            "]\n",
        ))
        .expect("parse unordered rows scenario");
        let error = unordered_rows
            .validate(&sequence)
            .expect_err("reject unordered list rows");
        assert!(error.to_string().contains("strictly increasing key order"));
    }

    #[test]
    fn repository_migration_scenarios_cover_required_lifecycle_paths() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let examples_root = repository_root.join("examples");

        let counter_sequence = MigrationSequence::from_path(
            examples_root.join("migrations/counter/sequence.toml"),
            &examples_root,
        )
        .expect("load Counter migration sequence");
        let counter = MigrationScenario::from_path(
            repository_root.join(&counter_sequence.scenario),
            &counter_sequence,
        )
        .expect("load Counter migration scenario");
        assert!(
            counter
                .steps
                .iter()
                .any(|step| { matches!(step.action, MigrationLifecycleAction::StartOver { .. }) })
        );
        assert!(
            counter
                .steps
                .iter()
                .any(|step| step.expect_failure.is_some())
        );
        assert!(
            counter
                .steps
                .iter()
                .any(|step| matches!(step.action, MigrationLifecycleAction::Dispatch { .. }))
        );
        assert!(
            counter
                .steps
                .iter()
                .any(|step| matches!(step.action, MigrationLifecycleAction::Checkpoint))
        );
        assert!(
            counter
                .steps
                .iter()
                .any(|step| matches!(step.action, MigrationLifecycleAction::Restart))
        );
        assert!(
            counter
                .steps
                .iter()
                .flat_map(|step| &step.assertions)
                .any(|assertion| matches!(
                    assertion,
                    MigrationAssertion::DurableValue {
                        path,
                        value: MigrationScenarioValue::Integer(0),
                        touched: Some(true),
                    } if path == "store.click_count"
                ))
        );

        let todo_sequence = MigrationSequence::from_path(
            examples_root.join("migrations/todo/sequence.toml"),
            &examples_root,
        )
        .expect("load Todo migration sequence");
        let todo = MigrationScenario::from_path(
            repository_root.join(&todo_sequence.scenario),
            &todo_sequence,
        )
        .expect("load Todo migration scenario");
        let incremental_activations = todo
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    step.action,
                    MigrationLifecycleAction::ActivateStage {
                        mode: MigrationActivationMode::Incremental,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(incremental_activations, 6);
        assert!(todo.steps.iter().any(|step| {
            matches!(
                &step.action,
                MigrationLifecycleAction::ActivateStage {
                    stage,
                    mode: MigrationActivationMode::Skipped,
                } if stage == "v7"
            )
        }));
        assert!(
            todo.steps
                .iter()
                .flat_map(|step| &step.assertions)
                .any(|assertion| matches!(
                    assertion,
                    MigrationAssertion::Edges {
                        mode: MigrationActivationMode::Skipped,
                        applied,
                        ..
                    } if applied.len() == 6
                ))
        );
        assert!(
            todo.steps
                .iter()
                .any(|step| matches!(step.action, MigrationLifecycleAction::Dispatch { .. }))
        );
        assert!(todo.steps.iter().flat_map(|step| &step.assertions).any(
            |assertion| matches!(assertion, MigrationAssertion::List { rows, next_key, .. } if !rows.is_empty() && *next_key > rows.last().expect("non-empty rows").key)
        ));
        assert!(
            todo.steps
                .iter()
                .flat_map(|step| &step.assertions)
                .any(|assertion| matches!(
                    assertion,
                    MigrationAssertion::NamespaceIsolation { .. }
                ))
        );
    }
}
