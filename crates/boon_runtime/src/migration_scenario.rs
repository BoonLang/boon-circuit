use super::{LiveRuntime, PersistentRuntime};
use boon_compiler::{
    CompilerSourceUnit, compile_runtime_source_units_to_machine_plan_with_persistence_catalog,
    compiler_source_units_for_manifest_source,
};
use boon_example_manifest::{
    MigrationActivationMode, MigrationAssertion, MigrationFaultPoint, MigrationLifecycleAction,
    MigrationListRowAssertion, MigrationScenario, MigrationScenarioStep, MigrationScenarioValue,
    MigrationSequence, MigrationStateScope,
};
use boon_persistence::{
    InMemoryDriver, PersistenceCommand, PersistenceDriver, PersistenceResult,
    PersistenceWorkerConfig, RestoreImage, ShutdownAck, StagedMigration, StoredValue,
    stage_migration,
};
use boon_plan::{
    ApplicationIdentity, FiniteReal, MachinePlan, MemoryId, MigrationPredecessorBinding,
    TargetProfile,
};
use boon_plan_executor::{RowId, SessionOptions, SourcePayload, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Deterministic lifecycle boundaries exercised by the generic migration
/// runner and the same persistence protocol used by the product runtime.
pub const SUPPORTED_MIGRATION_SCENARIO_FAULTS: &[MigrationFaultPoint] = &[
    MigrationFaultPoint::BeforeCheckpoint,
    MigrationFaultPoint::DuringCheckpoint,
    MigrationFaultPoint::AfterCheckpointAcknowledgement,
    MigrationFaultPoint::CandidateSettle,
    MigrationFaultPoint::BeforeActivationCommit,
    MigrationFaultPoint::DuringActivationCommit,
    MigrationFaultPoint::AfterActivationCommit,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationScenarioReport {
    pub name: String,
    pub steps: Vec<MigrationScenarioStepReport>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationScenarioStepReport {
    pub id: String,
    pub namespace: Option<String>,
    pub current_stage: Option<String>,
    pub durable_stage: Option<String>,
    pub preview_stage: Option<String>,
    pub expected_failure_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationScenarioError {
    pub step_id: Option<String>,
    pub code: String,
    pub message: String,
}

impl MigrationScenarioError {
    fn setup(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            step_id: None,
            code: code.into(),
            message: message.into(),
        }
    }

    fn at_step(
        step_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            step_id: Some(step_id.into()),
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for MigrationScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(step_id) = &self.step_id {
            write!(formatter, "migration scenario step `{step_id}` failed")?;
        } else {
            formatter.write_str("migration scenario setup failed")?;
        }
        write!(formatter, " [{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for MigrationScenarioError {}

#[derive(Clone, Debug)]
struct PreparedStage {
    id: String,
    schema_version: u64,
    source_label: String,
    units: Vec<CompilerSourceUnit>,
}

#[derive(Clone)]
struct CompiledStages {
    ordered: Vec<Arc<MachinePlan>>,
    by_id: BTreeMap<String, Arc<MachinePlan>>,
    id_by_schema: BTreeMap<(u64, [u8; 32]), String>,
}

impl CompiledStages {
    fn plan(&self, stage: &str) -> Result<Arc<MachinePlan>, ActionError> {
        self.by_id
            .get(stage)
            .cloned()
            .ok_or_else(|| ActionError::new("unknown_stage", format!("stage `{stage}` is absent")))
    }

    fn stage_for_image(&self, image: &RestoreImage) -> Option<&str> {
        self.id_by_schema
            .get(&(image.schema_version, image.schema_hash))
            .map(String::as_str)
    }

    fn stage_for_schema_version(&self, schema_version: u64) -> Option<&str> {
        self.ordered
            .iter()
            .find(|plan| plan.persistence.schema_version == schema_version)
            .and_then(|plan| {
                self.id_by_schema.get(&(
                    plan.persistence.schema_version,
                    plan.persistence.schema_hash,
                ))
            })
            .map(String::as_str)
    }
}

#[derive(Default)]
struct ScenarioDriverState {
    driver: InMemoryDriver,
    fail_checkpoints: bool,
    fail_activations: bool,
}

#[derive(Clone, Default)]
struct SharedInMemoryDriver {
    inner: Arc<Mutex<ScenarioDriverState>>,
}

impl SharedInMemoryDriver {
    fn image(&self, application: &ApplicationIdentity) -> Option<RestoreImage> {
        lock(&self.inner).driver.image(application).cloned()
    }

    fn fail_checkpoints(&self, fail: bool) {
        lock(&self.inner).fail_checkpoints = fail;
    }

    fn fail_activations(&self, fail: bool) {
        lock(&self.inner).fail_activations = fail;
    }
}

impl PersistenceDriver for SharedInMemoryDriver {
    fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
        // The facade, rather than the shared backing store, belongs to one
        // worker. Closing a facade must not make a deterministic restart lose
        // the in-memory database shared with the next worker.
        if matches!(command, PersistenceCommand::Shutdown(_)) {
            return PersistenceResult::ShutdownComplete(Ok(ShutdownAck));
        }
        let mut state = lock(&self.inner);
        match command {
            PersistenceCommand::Commit(_) if state.fail_checkpoints => {
                PersistenceResult::Committed(Err(boon_persistence::StoreError::Backend(
                    "injected checkpoint transaction failure".to_owned(),
                )))
            }
            PersistenceCommand::Activate(_) if state.fail_activations => {
                state.fail_activations = false;
                PersistenceResult::Activated(Err(boon_persistence::StoreError::Backend(
                    "injected activation transaction failure".to_owned(),
                )))
            }
            command => state.driver.execute(command),
        }
    }
}

struct PreparedPreview {
    stage: String,
    candidate: LiveRuntime,
    staged: StagedMigration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivationRecord {
    mode: MigrationActivationMode,
    from_stage: String,
    to_stage: String,
    applied: Vec<(String, String)>,
}

struct NamespaceRuntime {
    compiled: CompiledStages,
    driver: SharedInMemoryDriver,
    runtime: Option<PersistentRuntime>,
    current_stage: String,
    preview: Option<PreparedPreview>,
    last_activation: Option<ActivationRecord>,
    next_sequence: u64,
}

#[derive(Clone, Copy, Debug)]
struct ActiveFault {
    point: MigrationFaultPoint,
    occurrence: u64,
    compatible_actions_seen: u64,
    observed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CanonicalRow {
    key: u64,
    generation: u64,
    values: BTreeMap<String, StoredValue>,
    touched_fields: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CanonicalList {
    touched: bool,
    next_key: u64,
    rows: Vec<CanonicalRow>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CanonicalImage {
    scalars: BTreeMap<String, (bool, StoredValue)>,
    lists: BTreeMap<String, CanonicalList>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CanonicalNamespaceState {
    current_stage: String,
    durable_stage: String,
    preview_stage: Option<String>,
    current: CanonicalImage,
    durable: CanonicalImage,
}

#[derive(Clone, Debug)]
struct ActionError {
    code: String,
    message: String,
}

impl ActionError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Compiles and executes one validated migration lifecycle scenario.
///
/// Each scenario namespace gets a distinct application identity, complete
/// inherited stage catalog, `PersistentRuntime`, and in-memory database. All
/// mutations therefore cross the same public runtime/persistence boundary as
/// product code; the runner only reads backend images to assert durable state.
pub struct MigrationScenarioRunner {
    scenario: MigrationScenario,
    stages: Vec<PreparedStage>,
    application_template: ApplicationIdentity,
    namespaces: BTreeMap<String, NamespaceRuntime>,
    active_namespace: Option<String>,
    fault: Option<ActiveFault>,
    completed_steps: BTreeMap<String, (String, CanonicalNamespaceState)>,
}

impl MigrationScenarioRunner {
    pub fn new(
        sequence: MigrationSequence,
        scenario: MigrationScenario,
        application_template: ApplicationIdentity,
    ) -> Result<Self, MigrationScenarioError> {
        scenario.validate(&sequence).map_err(|error| {
            MigrationScenarioError::setup("invalid_scenario", error.to_string())
        })?;
        if !application_template.is_valid() {
            return Err(MigrationScenarioError::setup(
                "invalid_application_identity",
                "application identity components must be non-empty",
            ));
        }

        let stages = sequence
            .stages
            .iter()
            .map(|stage| {
                let units =
                    compiler_source_units_for_manifest_source(&stage.source, &stage.source_files)
                        .map_err(|error| {
                        MigrationScenarioError::setup(
                            "stage_source_load_failed",
                            format!("stage `{}` source load failed: {error}", stage.id),
                        )
                    })?;
                Ok(PreparedStage {
                    id: stage.id.clone(),
                    schema_version: stage.schema_version,
                    source_label: stage.source.clone(),
                    units,
                })
            })
            .collect::<Result<Vec<_>, MigrationScenarioError>>()?;

        Ok(Self {
            scenario,
            stages,
            application_template,
            namespaces: BTreeMap::new(),
            active_namespace: None,
            fault: None,
            completed_steps: BTreeMap::new(),
        })
    }

    pub fn run(mut self) -> Result<MigrationScenarioReport, MigrationScenarioError> {
        let mut reports = Vec::with_capacity(self.scenario.steps.len());
        for step in self.scenario.steps.clone() {
            reports.push(self.run_step(&step)?);
        }
        if let Some(fault) = self.fault {
            return Err(MigrationScenarioError::setup(
                "uncleared_fault",
                format!("scenario ended with {:?} fault still active", fault.point),
            ));
        }
        for namespace in self.namespaces.values_mut() {
            if let Some(runtime) = namespace.runtime.take() {
                runtime.shutdown().map_err(|error| {
                    MigrationScenarioError::setup("shutdown_failed", error.to_string())
                })?;
            }
        }
        Ok(MigrationScenarioReport {
            name: self.scenario.name,
            steps: reports,
        })
    }

    fn run_step(
        &mut self,
        step: &MigrationScenarioStep,
    ) -> Result<MigrationScenarioStepReport, MigrationScenarioError> {
        let before = self
            .active_namespace
            .as_deref()
            .map(|namespace| self.capture_namespace(namespace))
            .transpose()
            .map_err(|error| self.step_error(step, error))?;
        let action_result = self.execute_action(&step.action);

        match (&step.expect_failure, action_result) {
            (None, Ok(())) => {}
            (None, Err(error)) => return Err(self.step_error(step, error)),
            (Some(expected), Ok(())) => {
                return Err(MigrationScenarioError::at_step(
                    &step.id,
                    "expected_failure_missing",
                    format!("action succeeded but `{}` was expected", expected.code),
                ));
            }
            (Some(expected), Err(error)) => {
                if error.code != expected.code {
                    return Err(MigrationScenarioError::at_step(
                        &step.id,
                        "wrong_failure_code",
                        format!(
                            "expected `{}`, received `{}`: {}",
                            expected.code, error.code, error.message
                        ),
                    ));
                }
                if let Some(required) = &expected.message_contains
                    && !error.message.contains(required)
                {
                    return Err(MigrationScenarioError::at_step(
                        &step.id,
                        "wrong_failure_message",
                        format!(
                            "failure message does not contain `{required}`: {}",
                            error.message
                        ),
                    ));
                }
                let after = self
                    .active_namespace
                    .as_deref()
                    .map(|namespace| self.capture_namespace(namespace))
                    .transpose()
                    .map_err(|error| self.step_error(step, error))?;
                if expected.current_unchanged
                    && before.as_ref().map(|state| &state.current)
                        != after.as_ref().map(|state| &state.current)
                {
                    return Err(MigrationScenarioError::at_step(
                        &step.id,
                        "current_changed_after_failure",
                        "failed action changed current authority",
                    ));
                }
                if expected.durable_unchanged
                    && before.as_ref().map(|state| &state.durable)
                        != after.as_ref().map(|state| &state.durable)
                {
                    return Err(MigrationScenarioError::at_step(
                        &step.id,
                        "durable_changed_after_failure",
                        "failed action changed durable authority",
                    ));
                }
                if expected.schema_unchanged
                    && (before.as_ref().map(|state| &state.current_stage)
                        != after.as_ref().map(|state| &state.current_stage)
                        || before.as_ref().map(|state| &state.durable_stage)
                            != after.as_ref().map(|state| &state.durable_stage))
                {
                    return Err(MigrationScenarioError::at_step(
                        &step.id,
                        "schema_changed_after_failure",
                        "failed action changed current or durable schema",
                    ));
                }
            }
        }

        for assertion in &step.assertions {
            self.assert(step, assertion)?;
        }
        if let Some(namespace) = self.active_namespace.clone() {
            let snapshot = self
                .capture_namespace(&namespace)
                .map_err(|error| self.step_error(step, error))?;
            self.completed_steps
                .insert(step.id.clone(), (namespace, snapshot));
        }

        let (current_stage, durable_stage, preview_stage) = self
            .active_namespace
            .as_deref()
            .map(|namespace| self.schema_state(namespace))
            .transpose()
            .map_err(|error| self.step_error(step, error))?
            .map_or((None, None, None), |(current, durable, preview)| {
                (Some(current), Some(durable), preview)
            });
        Ok(MigrationScenarioStepReport {
            id: step.id.clone(),
            namespace: self.active_namespace.clone(),
            current_stage,
            durable_stage,
            preview_stage,
            expected_failure_code: step
                .expect_failure
                .as_ref()
                .map(|failure| failure.code.clone()),
        })
    }

    fn execute_action(&mut self, action: &MigrationLifecycleAction) -> Result<(), ActionError> {
        match action {
            MigrationLifecycleAction::Start { stage, namespace } => self.start(stage, namespace),
            MigrationLifecycleAction::Dispatch {
                public_source,
                target,
                payload,
            } => self.dispatch(public_source, target.as_ref(), payload),
            MigrationLifecycleAction::Checkpoint => self.checkpoint(action),
            MigrationLifecycleAction::Restart => self.restart(),
            MigrationLifecycleAction::PreviewStage { stage } => self.preview(stage, action),
            MigrationLifecycleAction::ActivateStage { stage, mode } => {
                self.activate(stage, *mode, action)
            }
            MigrationLifecycleAction::StartOver { stage } => self.start_over(stage),
            MigrationLifecycleAction::InjectFault { point, occurrence } => {
                if self.fault.is_some() {
                    return Err(ActionError::new(
                        "fault_already_active",
                        "another lifecycle fault is already active",
                    ));
                }
                self.active()?;
                self.fault = Some(ActiveFault {
                    point: *point,
                    occurrence: *occurrence,
                    compatible_actions_seen: 0,
                    observed: false,
                });
                Ok(())
            }
            MigrationLifecycleAction::ClearFault => {
                let fault = self.fault.take().ok_or_else(|| {
                    ActionError::new("no_active_fault", "there is no lifecycle fault to clear")
                })?;
                if !fault.observed {
                    self.fault = Some(fault);
                    return Err(ActionError::new(
                        "fault_not_observed",
                        "lifecycle fault was cleared before its configured occurrence",
                    ));
                }
                Ok(())
            }
        }
    }

    fn start(&mut self, stage: &str, namespace: &str) -> Result<(), ActionError> {
        if let Some(existing) = self.namespaces.get(namespace) {
            if existing.current_stage != stage {
                return Err(ActionError::new(
                    "namespace_stage_mismatch",
                    format!(
                        "namespace `{namespace}` is at `{}`, not `{stage}`",
                        existing.current_stage
                    ),
                ));
            }
            self.active_namespace = Some(namespace.to_owned());
            return Ok(());
        }

        let identity = ApplicationIdentity::new(
            self.application_template.package_id.clone(),
            namespace,
            self.application_template.deployment_domain.clone(),
        );
        let compiled = self.compile_stages(identity)?;
        let plan = compiled.plan(stage)?;
        let driver = SharedInMemoryDriver::default();
        let (runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            driver.clone(),
            deterministic_worker_config(),
        )
        .map_err(|error| ActionError::new("start_failed", error.to_string()))?;
        let next_sequence = startup
            .restore_image
            .through_turn_sequence
            .saturating_add(1);
        self.namespaces.insert(
            namespace.to_owned(),
            NamespaceRuntime {
                compiled,
                driver,
                runtime: Some(runtime),
                current_stage: stage.to_owned(),
                preview: None,
                last_activation: None,
                next_sequence,
            },
        );
        self.active_namespace = Some(namespace.to_owned());
        Ok(())
    }

    fn compile_stages(&self, identity: ApplicationIdentity) -> Result<CompiledStages, ActionError> {
        let mut ordered = Vec::with_capacity(self.stages.len());
        let mut by_id = BTreeMap::new();
        let mut id_by_schema = BTreeMap::new();
        let mut predecessor = None::<MigrationPredecessorBinding>;
        for stage in &self.stages {
            let predecessors = predecessor.as_slice();
            let compiled = compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
                &stage.source_label,
                &stage.units,
                TargetProfile::SoftwareDefault,
                identity.clone(),
                stage.schema_version,
                predecessors,
            )
            .map_err(|error| {
                ActionError::new(
                    "stage_compile_failed",
                    format!("stage `{}` failed to compile: {error}", stage.id),
                )
            })?;
            let plan = Arc::new(compiled.plan);
            predecessor = Some(MigrationPredecessorBinding::from_machine_plan(&plan));
            id_by_schema.insert(
                (
                    plan.persistence.schema_version,
                    plan.persistence.schema_hash,
                ),
                stage.id.clone(),
            );
            by_id.insert(stage.id.clone(), Arc::clone(&plan));
            ordered.push(plan);
        }
        Ok(CompiledStages {
            ordered,
            by_id,
            id_by_schema,
        })
    }

    fn dispatch(
        &mut self,
        public_source: &str,
        target: Option<&boon_example_manifest::MigrationRowTarget>,
        payload: &BTreeMap<String, MigrationScenarioValue>,
    ) -> Result<(), ActionError> {
        let namespace = self.active_mut()?;
        let runtime = namespace.runtime.as_mut().ok_or_else(|| {
            ActionError::new("runtime_missing", "active namespace has no runtime")
        })?;
        let target = target
            .map(|target| {
                let plan = runtime.runtime().machine_plan();
                let list = plan
                    .persistence
                    .lists
                    .iter()
                    .find(|list| list.semantic_path == target.list)
                    .ok_or_else(|| {
                        ActionError::new(
                            "unknown_target_list",
                            format!("target list `{}` is absent", target.list),
                        )
                    })?;
                let slot = plan
                    .storage_layout
                    .list_slots
                    .iter()
                    .find(|slot| slot.id == list.runtime_slot)
                    .ok_or_else(|| {
                        ActionError::new(
                            "target_list_slot_missing",
                            format!("target list `{}` has no runtime slot", target.list),
                        )
                    })?;
                Ok(RowId {
                    list: slot.list_id,
                    key: target.key,
                    generation: target.generation,
                })
            })
            .transpose()?;
        let event = runtime
            .runtime()
            .source_event(
                namespace.next_sequence,
                public_source,
                target,
                source_payload(payload)?,
            )
            .map_err(|error| ActionError::new("source_event_failed", error.to_string()))?;
        runtime
            .dispatch(event)
            .map_err(|error| ActionError::new("dispatch_failed", error.to_string()))?;
        namespace.next_sequence = namespace.next_sequence.saturating_add(1);
        namespace.preview = None;
        Ok(())
    }

    fn checkpoint(&mut self, action: &MigrationLifecycleAction) -> Result<(), ActionError> {
        let fault = self.triggered_fault(action);
        if fault == Some(MigrationFaultPoint::BeforeCheckpoint) {
            return Err(fault_error(MigrationFaultPoint::BeforeCheckpoint));
        }
        let namespace = self.active_mut()?;
        if fault == Some(MigrationFaultPoint::DuringCheckpoint) {
            namespace.driver.fail_checkpoints(true);
        }
        let result = namespace
            .runtime
            .as_ref()
            .ok_or_else(|| ActionError::new("runtime_missing", "active namespace has no runtime"))?
            .barrier();
        if fault == Some(MigrationFaultPoint::DuringCheckpoint) {
            namespace.driver.fail_checkpoints(false);
            return match result {
                Err(_) => Err(fault_error(MigrationFaultPoint::DuringCheckpoint)),
                Ok(_) => Err(ActionError::new(
                    "during_checkpoint_not_injected",
                    "checkpoint transaction unexpectedly succeeded",
                )),
            };
        }
        result.map_err(|error| ActionError::new("checkpoint_failed", error.to_string()))?;
        if fault == Some(MigrationFaultPoint::AfterCheckpointAcknowledgement) {
            return Err(fault_error(
                MigrationFaultPoint::AfterCheckpointAcknowledgement,
            ));
        }
        Ok(())
    }

    fn restart(&mut self) -> Result<(), ActionError> {
        let namespace = self.active_mut()?;
        let plan = namespace.compiled.plan(&namespace.current_stage)?;
        let runtime = namespace.runtime.take().ok_or_else(|| {
            ActionError::new("runtime_missing", "active namespace has no runtime")
        })?;
        runtime
            .barrier()
            .map_err(|error| ActionError::new("restart_barrier_failed", error.to_string()))?;
        runtime
            .shutdown()
            .map_err(|error| ActionError::new("restart_shutdown_failed", error.to_string()))?;
        let (runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            namespace.driver.clone(),
            deterministic_worker_config(),
        )
        .map_err(|error| ActionError::new("restart_failed", error.to_string()))?;
        namespace.next_sequence = startup
            .restore_image
            .through_turn_sequence
            .saturating_add(1);
        namespace.runtime = Some(runtime);
        namespace.preview = None;
        Ok(())
    }

    fn preview(
        &mut self,
        stage: &str,
        action: &MigrationLifecycleAction,
    ) -> Result<(), ActionError> {
        let fault = self.triggered_fault(action);
        let namespace = self.active_mut()?;
        let runtime = namespace.runtime.as_ref().ok_or_else(|| {
            ActionError::new("runtime_missing", "active namespace has no runtime")
        })?;
        runtime
            .barrier()
            .map_err(|error| ActionError::new("preview_barrier_failed", error.to_string()))?;
        let current = runtime
            .load_durable_image()
            .map_err(|error| ActionError::new("preview_load_failed", error.to_string()))?
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let plan = namespace.compiled.plan(stage)?;
        let staged = stage_migration(&current, &plan)
            .map_err(|error| ActionError::new("migration_preview_failed", error.to_string()))?;
        if fault == Some(MigrationFaultPoint::CandidateSettle) {
            return Err(fault_error(MigrationFaultPoint::CandidateSettle));
        }
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            plan,
            SessionOptions::default(),
            Some(staged.candidate.clone()),
        )
        .map_err(|error| ActionError::new("candidate_settle_failed", error.to_string()))?;
        let _ = candidate.mount();
        namespace.preview = Some(PreparedPreview {
            stage: stage.to_owned(),
            candidate,
            staged,
        });
        Ok(())
    }

    fn activate(
        &mut self,
        stage: &str,
        mode: MigrationActivationMode,
        action: &MigrationLifecycleAction,
    ) -> Result<(), ActionError> {
        let fault = self.triggered_fault(action);
        if fault == Some(MigrationFaultPoint::CandidateSettle) {
            return Err(fault_error(MigrationFaultPoint::CandidateSettle));
        }
        if fault == Some(MigrationFaultPoint::BeforeActivationCommit) {
            return Err(fault_error(MigrationFaultPoint::BeforeActivationCommit));
        }
        let namespace = self.active_mut()?;
        let from_stage = namespace.current_stage.clone();
        let plan = namespace.compiled.plan(stage)?;
        let preview = namespace
            .preview
            .take()
            .filter(|preview| preview.stage == stage);
        if fault == Some(MigrationFaultPoint::DuringActivationCommit) {
            namespace.driver.fail_activations(true);
        }
        let migration_preview = if let Some(preview) = preview {
            let completed_edges = preview.staged.candidate.completed_migration_edges.clone();
            namespace
                .runtime
                .as_mut()
                .ok_or_else(|| {
                    ActionError::new("runtime_missing", "active namespace has no runtime")
                })?
                .activate_settled_candidate(preview.candidate, completed_edges)
                .map_err(|error| {
                    let code = if fault == Some(MigrationFaultPoint::DuringActivationCommit) {
                        "during_activation_commit_failed"
                    } else {
                        "activation_failed"
                    };
                    ActionError::new(code, error.to_string())
                })?;
            Some(preview.staged.preview)
        } else {
            namespace
                .runtime
                .as_mut()
                .ok_or_else(|| {
                    ActionError::new("runtime_missing", "active namespace has no runtime")
                })?
                .activate_machine_plan(plan, SessionOptions::default())
                .map_err(|error| {
                    let code = if fault == Some(MigrationFaultPoint::DuringActivationCommit) {
                        "during_activation_commit_failed"
                    } else {
                        "activation_failed"
                    };
                    ActionError::new(code, error.to_string())
                })?
                .migration
        };

        let applied = migration_preview
            .as_ref()
            .map(|preview| {
                preview
                    .steps
                    .iter()
                    .map(|step| {
                        let from = namespace
                            .compiled
                            .stage_for_schema_version(step.source_schema_version)
                            .ok_or_else(|| {
                                ActionError::new(
                                    "migration_edge_stage_missing",
                                    format!(
                                        "schema version {} has no stage",
                                        step.source_schema_version
                                    ),
                                )
                            })?;
                        let to = namespace
                            .compiled
                            .stage_for_schema_version(step.target_schema_version)
                            .ok_or_else(|| {
                                ActionError::new(
                                    "migration_edge_stage_missing",
                                    format!(
                                        "schema version {} has no stage",
                                        step.target_schema_version
                                    ),
                                )
                            })?;
                        Ok((from.to_owned(), to.to_owned()))
                    })
                    .collect::<Result<Vec<_>, ActionError>>()
            })
            .transpose()?
            .unwrap_or_default();
        namespace.current_stage = stage.to_owned();
        namespace.last_activation = Some(ActivationRecord {
            mode,
            from_stage,
            to_stage: stage.to_owned(),
            applied,
        });
        namespace.next_sequence = namespace
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.driver_image_turn(&namespace.driver))
            .unwrap_or(namespace.next_sequence.saturating_sub(1))
            .saturating_add(1);
        if fault == Some(MigrationFaultPoint::AfterActivationCommit) {
            return Err(fault_error(MigrationFaultPoint::AfterActivationCommit));
        }
        Ok(())
    }

    fn start_over(&mut self, stage: &str) -> Result<(), ActionError> {
        let namespace = self.active_mut()?;
        let plan = namespace.compiled.plan(stage)?;
        let reset = namespace
            .runtime
            .as_mut()
            .ok_or_else(|| ActionError::new("runtime_missing", "active namespace has no runtime"))?
            .start_over_machine_plan(plan, SessionOptions::default())
            .map_err(|error| ActionError::new("start_over_failed", error.to_string()))?;
        namespace.current_stage = stage.to_owned();
        namespace.preview = None;
        namespace.last_activation = None;
        namespace.next_sequence = reset
            .acknowledgement
            .through_turn_sequence
            .saturating_add(1);
        Ok(())
    }

    fn active(&self) -> Result<&NamespaceRuntime, ActionError> {
        let namespace = self.active_namespace.as_deref().ok_or_else(|| {
            ActionError::new(
                "no_active_namespace",
                "scenario has not started a namespace",
            )
        })?;
        self.namespaces.get(namespace).ok_or_else(|| {
            ActionError::new(
                "active_namespace_missing",
                format!("active namespace `{namespace}` is absent"),
            )
        })
    }

    fn active_mut(&mut self) -> Result<&mut NamespaceRuntime, ActionError> {
        let namespace = self.active_namespace.clone().ok_or_else(|| {
            ActionError::new(
                "no_active_namespace",
                "scenario has not started a namespace",
            )
        })?;
        self.namespaces.get_mut(&namespace).ok_or_else(|| {
            ActionError::new(
                "active_namespace_missing",
                format!("active namespace `{namespace}` is absent"),
            )
        })
    }

    fn triggered_fault(
        &mut self,
        action: &MigrationLifecycleAction,
    ) -> Option<MigrationFaultPoint> {
        let fault = self.fault.as_mut()?;
        if !fault_applies(fault.point, action) {
            return None;
        }
        fault.compatible_actions_seen = fault.compatible_actions_seen.saturating_add(1);
        if fault.compatible_actions_seen == fault.occurrence {
            fault.observed = true;
            Some(fault.point)
        } else {
            None
        }
    }

    fn step_error(
        &self,
        step: &MigrationScenarioStep,
        error: ActionError,
    ) -> MigrationScenarioError {
        MigrationScenarioError::at_step(&step.id, error.code, error.message)
    }

    fn assert(
        &mut self,
        step: &MigrationScenarioStep,
        assertion: &MigrationAssertion,
    ) -> Result<(), MigrationScenarioError> {
        let result = match assertion {
            MigrationAssertion::CurrentValue {
                path,
                value,
                touched,
            } => self.assert_scalar(MigrationStateScope::Current, path, value, *touched),
            MigrationAssertion::DurableValue {
                path,
                value,
                touched,
            } => self.assert_scalar(MigrationStateScope::Durable, path, value, *touched),
            MigrationAssertion::CurrentAbsent { path } => {
                self.assert_absent(MigrationStateScope::Current, path)
            }
            MigrationAssertion::DurableAbsent { path } => {
                self.assert_absent(MigrationStateScope::Durable, path)
            }
            MigrationAssertion::Schema {
                current_stage,
                durable_stage,
                preview_stage,
            } => self.assert_schema(current_stage, durable_stage, preview_stage.as_deref()),
            MigrationAssertion::Edges {
                mode,
                from_stage,
                to_stage,
                applied,
            } => {
                let namespace = self
                    .active()
                    .map_err(|error| self.step_error(step, error))?;
                let expected = ActivationRecord {
                    mode: *mode,
                    from_stage: from_stage.clone(),
                    to_stage: to_stage.clone(),
                    applied: applied
                        .iter()
                        .map(|edge| (edge.from_stage.clone(), edge.to_stage.clone()))
                        .collect(),
                };
                if namespace.last_activation.as_ref() == Some(&expected) {
                    Ok(())
                } else {
                    Err(ActionError::new(
                        "edge_assertion_failed",
                        format!(
                            "expected activation {expected:?}, received {:?}",
                            namespace.last_activation
                        ),
                    ))
                }
            }
            MigrationAssertion::List {
                scope,
                path,
                rows,
                next_key,
                touched,
            } => self.assert_list(*scope, path, rows, *next_key, *touched),
            MigrationAssertion::NamespaceIsolation {
                namespace,
                unchanged_since,
            } => self.assert_namespace_isolation(namespace, unchanged_since),
            MigrationAssertion::NamespaceEquivalent {
                namespace,
                other_namespace,
            } => self.assert_namespace_equivalent(namespace, other_namespace),
        };
        result.map_err(|error| self.step_error(step, error))
    }

    fn assert_scalar(
        &mut self,
        scope: MigrationStateScope,
        path: &str,
        expected: &MigrationScenarioValue,
        expected_touched: Option<bool>,
    ) -> Result<(), ActionError> {
        let namespace_name = self.active_namespace.clone().ok_or_else(|| {
            ActionError::new(
                "no_active_namespace",
                "scenario has not started a namespace",
            )
        })?;
        let namespace = self.namespaces.get_mut(&namespace_name).ok_or_else(|| {
            ActionError::new(
                "active_namespace_missing",
                format!("active namespace `{namespace_name}` is absent"),
            )
        })?;
        let durable = namespace
            .driver
            .image(&namespace_application(namespace)?)
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let (plan, value, image) = match scope {
            MigrationStateScope::Current => {
                let runtime = namespace.runtime.as_mut().ok_or_else(|| {
                    ActionError::new("runtime_missing", "active namespace has no runtime")
                })?;
                let plan = namespace.compiled.plan(&namespace.current_stage)?;
                let value = runtime
                    .inspect_value_current(path, 100_000)
                    .map_err(|error| {
                        ActionError::new("current_value_read_failed", error.to_string())
                    })?;
                let image = runtime
                    .runtime()
                    .durable_restore_image(durable.epoch, durable.completed_migration_edges.clone())
                    .map_err(|error| {
                        ActionError::new("current_authority_read_failed", error.to_string())
                    })?;
                (plan, value, image)
            }
            MigrationStateScope::Durable => {
                let stage = namespace
                    .compiled
                    .stage_for_image(&durable)
                    .ok_or_else(|| {
                        ActionError::new(
                            "durable_schema_unknown",
                            format!(
                                "schema version {} and hash are not a compiled stage",
                                durable.schema_version
                            ),
                        )
                    })?;
                let plan = namespace.compiled.plan(stage)?;
                let mut runtime = LiveRuntime::from_shared_machine_plan_with_restore(
                    Arc::clone(&plan),
                    SessionOptions::default(),
                    Some(durable.clone()),
                )
                .map_err(|error| {
                    ActionError::new("durable_value_restore_failed", error.to_string())
                })?;
                let value = runtime
                    .inspect_value_current(path, 100_000)
                    .map_err(|error| {
                        ActionError::new("durable_value_read_failed", error.to_string())
                    })?;
                (plan, value, durable)
            }
        };
        if value != scenario_value(expected) {
            return Err(ActionError::new(
                "value_assertion_failed",
                format!(
                    "{scope:?} `{path}` expected {:?}, received {value:?}",
                    scenario_value(expected)
                ),
            ));
        }
        if let Some(expected_touched) = expected_touched {
            let memory = scalar_memory(&plan, path).ok_or_else(|| {
                ActionError::new(
                    "scalar_memory_missing",
                    format!("`{path}` is not scalar semantic memory"),
                )
            })?;
            let touched = image
                .scalars
                .get(&memory.memory_id)
                .is_some_and(|scalar| scalar.touched);
            if touched != expected_touched {
                return Err(ActionError::new(
                    "touched_assertion_failed",
                    format!(
                        "{scope:?} `{path}` touched expected {expected_touched}, received {touched}"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn assert_absent(&self, scope: MigrationStateScope, path: &str) -> Result<(), ActionError> {
        let namespace = self.active()?;
        let durable = namespace
            .driver
            .image(&namespace_application(namespace)?)
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let (stage, image) = match scope {
            MigrationStateScope::Current => {
                let runtime = namespace.runtime.as_ref().ok_or_else(|| {
                    ActionError::new("runtime_missing", "active namespace has no runtime")
                })?;
                let image = runtime
                    .runtime()
                    .durable_restore_image(durable.epoch, durable.completed_migration_edges.clone())
                    .map_err(|error| {
                        ActionError::new("current_authority_read_failed", error.to_string())
                    })?;
                (namespace.current_stage.as_str(), image)
            }
            MigrationStateScope::Durable => (
                namespace
                    .compiled
                    .stage_for_image(&durable)
                    .ok_or_else(|| {
                        ActionError::new(
                            "durable_schema_unknown",
                            "durable image does not match a compiled stage",
                        )
                    })?,
                durable,
            ),
        };
        let plan = namespace.compiled.plan(stage)?;
        if semantic_memory_present(&plan, path) {
            return Err(ActionError::new(
                "absent_assertion_failed",
                format!("{scope:?} schema still contains `{path}`"),
            ));
        }
        let leaked = namespace.compiled.ordered.iter().any(|historical| {
            memory_ids_for_path(historical, path)
                .into_iter()
                .any(|memory| {
                    image.scalars.contains_key(&memory) || image.lists.contains_key(&memory)
                })
        });
        if leaked {
            return Err(ActionError::new(
                "deleted_authority_still_stored",
                format!("{scope:?} authority still stores deleted path `{path}`"),
            ));
        }
        Ok(())
    }

    fn assert_schema(
        &self,
        expected_current: &str,
        expected_durable: &str,
        expected_preview: Option<&str>,
    ) -> Result<(), ActionError> {
        let namespace = self.active_namespace.as_deref().ok_or_else(|| {
            ActionError::new(
                "no_active_namespace",
                "scenario has not started a namespace",
            )
        })?;
        let (current, durable, preview) = self.schema_state(namespace)?;
        if current != expected_current
            || durable != expected_durable
            || preview.as_deref() != expected_preview
        {
            return Err(ActionError::new(
                "schema_assertion_failed",
                format!(
                    "expected current={expected_current}, durable={expected_durable}, preview={expected_preview:?}; received current={current}, durable={durable}, preview={preview:?}"
                ),
            ));
        }
        Ok(())
    }

    fn assert_list(
        &self,
        scope: MigrationStateScope,
        path: &str,
        expected_rows: &[MigrationListRowAssertion],
        expected_next_key: u64,
        expected_touched: Option<bool>,
    ) -> Result<(), ActionError> {
        let namespace = self.active()?;
        let durable = namespace
            .driver
            .image(&namespace_application(namespace)?)
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let (plan, snapshot, raw) = match scope {
            MigrationStateScope::Current => {
                let plan = namespace.compiled.plan(&namespace.current_stage)?;
                let runtime = namespace.runtime.as_ref().ok_or_else(|| {
                    ActionError::new("runtime_missing", "active namespace has no runtime")
                })?;
                let snapshot = runtime.runtime().snapshot().map_err(|error| {
                    ActionError::new("current_list_read_failed", error.to_string())
                })?;
                let raw = runtime
                    .runtime()
                    .durable_restore_image(durable.epoch, durable.completed_migration_edges.clone())
                    .map_err(|error| {
                        ActionError::new("current_authority_read_failed", error.to_string())
                    })?;
                (plan, snapshot, raw)
            }
            MigrationStateScope::Durable => {
                let stage = namespace
                    .compiled
                    .stage_for_image(&durable)
                    .ok_or_else(|| {
                        ActionError::new(
                            "durable_schema_unknown",
                            "durable image does not match a compiled stage",
                        )
                    })?;
                let plan = namespace.compiled.plan(stage)?;
                let runtime = LiveRuntime::from_shared_machine_plan_with_restore(
                    Arc::clone(&plan),
                    SessionOptions::default(),
                    Some(durable.clone()),
                )
                .map_err(|error| {
                    ActionError::new("durable_list_restore_failed", error.to_string())
                })?;
                let snapshot = runtime.snapshot().map_err(|error| {
                    ActionError::new("durable_list_read_failed", error.to_string())
                })?;
                (plan, snapshot, durable)
            }
        };
        assert_list_snapshot(
            scope,
            &plan,
            &snapshot,
            &raw,
            path,
            expected_rows,
            expected_next_key,
            expected_touched,
        )
    }

    fn assert_namespace_isolation(
        &self,
        namespace: &str,
        unchanged_since: &str,
    ) -> Result<(), ActionError> {
        let (baseline_namespace, baseline) =
            self.completed_steps.get(unchanged_since).ok_or_else(|| {
                ActionError::new(
                    "isolation_baseline_missing",
                    format!("step `{unchanged_since}` has no recorded namespace state"),
                )
            })?;
        if baseline_namespace != namespace {
            return Err(ActionError::new(
                "isolation_baseline_namespace_mismatch",
                format!(
                    "step `{unchanged_since}` recorded `{baseline_namespace}`, not `{namespace}`"
                ),
            ));
        }
        let current = self.capture_namespace(namespace)?;
        if &current != baseline {
            return Err(ActionError::new(
                "namespace_isolation_failed",
                format!("namespace `{namespace}` changed since `{unchanged_since}`"),
            ));
        }
        Ok(())
    }

    fn assert_namespace_equivalent(
        &self,
        namespace: &str,
        other_namespace: &str,
    ) -> Result<(), ActionError> {
        let left = self.capture_namespace(namespace)?;
        let right = self.capture_namespace(other_namespace)?;
        if left != right {
            return Err(ActionError::new(
                "namespace_equivalence_failed",
                format!("namespaces `{namespace}` and `{other_namespace}` differ"),
            ));
        }
        Ok(())
    }

    fn capture_namespace(
        &self,
        namespace_name: &str,
    ) -> Result<CanonicalNamespaceState, ActionError> {
        let namespace = self.namespaces.get(namespace_name).ok_or_else(|| {
            ActionError::new(
                "namespace_missing",
                format!("namespace `{namespace_name}` is absent"),
            )
        })?;
        let runtime = namespace
            .runtime
            .as_ref()
            .ok_or_else(|| ActionError::new("runtime_missing", "namespace has no runtime"))?;
        let application = runtime
            .runtime()
            .machine_plan()
            .application
            .identity
            .clone();
        let durable = namespace
            .driver
            .image(&application)
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let current = runtime
            .runtime()
            .durable_restore_image(durable.epoch, durable.completed_migration_edges.clone())
            .map_err(|error| {
                ActionError::new("current_authority_read_failed", error.to_string())
            })?;
        let current_plan = namespace.compiled.plan(&namespace.current_stage)?;
        let durable_stage = namespace
            .compiled
            .stage_for_image(&durable)
            .ok_or_else(|| {
                ActionError::new(
                    "durable_schema_unknown",
                    format!(
                        "durable schema version {} does not match a compiled stage",
                        durable.schema_version
                    ),
                )
            })?;
        let durable_plan = namespace.compiled.plan(durable_stage)?;
        Ok(CanonicalNamespaceState {
            current_stage: namespace.current_stage.clone(),
            durable_stage: durable_stage.to_owned(),
            preview_stage: namespace
                .preview
                .as_ref()
                .map(|preview| preview.stage.clone()),
            current: canonical_image(&current_plan, &current),
            durable: canonical_image(&durable_plan, &durable),
        })
    }

    fn schema_state(
        &self,
        namespace_name: &str,
    ) -> Result<(String, String, Option<String>), ActionError> {
        let namespace = self.namespaces.get(namespace_name).ok_or_else(|| {
            ActionError::new(
                "namespace_missing",
                format!("namespace `{namespace_name}` is absent"),
            )
        })?;
        let durable = namespace
            .driver
            .image(&namespace_application(namespace)?)
            .ok_or_else(|| ActionError::new("durable_state_missing", "durable state is absent"))?;
        let durable_stage = namespace
            .compiled
            .stage_for_image(&durable)
            .ok_or_else(|| {
                ActionError::new(
                    "durable_schema_unknown",
                    "durable image does not match a compiled stage",
                )
            })?;
        Ok((
            namespace.current_stage.clone(),
            durable_stage.to_owned(),
            namespace
                .preview
                .as_ref()
                .map(|preview| preview.stage.clone()),
        ))
    }
}

fn namespace_application(namespace: &NamespaceRuntime) -> Result<ApplicationIdentity, ActionError> {
    namespace
        .runtime
        .as_ref()
        .map(|runtime| {
            runtime
                .runtime()
                .machine_plan()
                .application
                .identity
                .clone()
        })
        .ok_or_else(|| ActionError::new("runtime_missing", "namespace has no runtime"))
}

fn scalar_memory<'a>(plan: &'a MachinePlan, path: &str) -> Option<&'a boon_plan::MemoryPlan> {
    plan.persistence
        .memory
        .iter()
        .find(|memory| memory.kind == boon_plan::MemoryKind::Scalar && memory.semantic_path == path)
}

fn semantic_memory_present(plan: &MachinePlan, path: &str) -> bool {
    plan.persistence
        .memory
        .iter()
        .any(|memory| memory.semantic_path == path)
        || plan
            .persistence
            .lists
            .iter()
            .any(|list| list.semantic_path == path)
}

fn memory_ids_for_path(plan: &MachinePlan, path: &str) -> Vec<MemoryId> {
    plan.persistence
        .memory
        .iter()
        .filter(|memory| memory.semantic_path == path)
        .map(|memory| memory.memory_id)
        .chain(
            plan.persistence
                .lists
                .iter()
                .filter(|list| list.semantic_path == path)
                .map(|list| list.memory_id),
        )
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn assert_list_snapshot(
    scope: MigrationStateScope,
    plan: &MachinePlan,
    snapshot: &boon_plan_executor::Snapshot,
    raw: &RestoreImage,
    path: &str,
    expected_rows: &[MigrationListRowAssertion],
    expected_next_key: u64,
    expected_touched: Option<bool>,
) -> Result<(), ActionError> {
    let list = plan
        .persistence
        .lists
        .iter()
        .find(|list| list.semantic_path == path)
        .ok_or_else(|| {
            ActionError::new(
                "list_memory_missing",
                format!("`{path}` is not list semantic memory"),
            )
        })?;
    let slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.id == list.runtime_slot)
        .ok_or_else(|| {
            ActionError::new(
                "list_slot_missing",
                format!("list `{path}` has no runtime slot"),
            )
        })?;
    let rows = snapshot.lists.get(&slot.list_id).ok_or_else(|| {
        ActionError::new(
            "list_snapshot_missing",
            format!("list `{path}` has no runtime snapshot"),
        )
    })?;
    if rows.len() != expected_rows.len() {
        return Err(ActionError::new(
            "list_row_count_mismatch",
            format!(
                "{scope:?} `{path}` expected {} rows, received {}",
                expected_rows.len(),
                rows.len()
            ),
        ));
    }

    let stored = raw.lists.get(&list.memory_id);
    if let Some(expected_touched) = expected_touched {
        let touched = stored.is_some_and(|stored| stored.touched);
        if touched != expected_touched {
            return Err(ActionError::new(
                "list_touched_mismatch",
                format!(
                    "{scope:?} `{path}` touched expected {expected_touched}, received {touched}"
                ),
            ));
        }
    }
    let next_key = stored
        .filter(|stored| stored.next_key != 0)
        .map(|stored| stored.next_key)
        .unwrap_or_else(|| {
            rows.iter()
                .map(|row| row.id.key)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
        });
    if next_key != expected_next_key {
        return Err(ActionError::new(
            "list_next_key_mismatch",
            format!(
                "{scope:?} `{path}` next_key expected {expected_next_key}, received {next_key}"
            ),
        ));
    }

    for (actual, expected) in rows.iter().zip(expected_rows) {
        if actual.id.key != expected.key || actual.id.generation != expected.generation {
            return Err(ActionError::new(
                "list_row_identity_mismatch",
                format!(
                    "{scope:?} `{path}` expected row {}:{}, received {}:{}",
                    expected.key, expected.generation, actual.id.key, actual.id.generation
                ),
            ));
        }
        for (field_name, expected_value) in &expected.values {
            let leaf = list
                .row_fields
                .iter()
                .find(|leaf| {
                    leaf.semantic_path == *field_name
                        || local_name(&leaf.semantic_path) == field_name
                })
                .ok_or_else(|| {
                    ActionError::new(
                        "list_field_missing",
                        format!("list `{path}` has no persistent field `{field_name}`"),
                    )
                })?;
            let field_id = leaf.runtime_field_id.ok_or_else(|| {
                ActionError::new(
                    "list_field_runtime_id_missing",
                    format!("list `{path}` field `{field_name}` has no runtime field ID"),
                )
            })?;
            let actual_value = actual.fields.get(&field_id).ok_or_else(|| {
                ActionError::new(
                    "list_field_value_missing",
                    format!(
                        "{scope:?} `{path}` row {}:{} has no `{field_name}` value",
                        actual.id.key, actual.id.generation
                    ),
                )
            })?;
            if actual_value != &scenario_value(expected_value) {
                return Err(ActionError::new(
                    "list_field_value_mismatch",
                    format!(
                        "{scope:?} `{path}` row {}:{} field `{field_name}` expected {:?}, received {actual_value:?}",
                        actual.id.key,
                        actual.id.generation,
                        scenario_value(expected_value)
                    ),
                ));
            }
        }
        let actual_touched = stored
            .and_then(|stored| {
                stored
                    .rows
                    .iter()
                    .find(|row| row.key == actual.id.key && row.generation == actual.id.generation)
            })
            .map(|row| {
                row.touched_fields
                    .iter()
                    .filter_map(|field| {
                        list.row_fields
                            .iter()
                            .find(|leaf| leaf.leaf_id == *field)
                            .map(|leaf| local_name(&leaf.semantic_path).to_owned())
                    })
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let expected_touched = expected
            .touched_fields
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual_touched != expected_touched {
            return Err(ActionError::new(
                "list_row_touched_fields_mismatch",
                format!(
                    "{scope:?} `{path}` row {}:{} touched fields expected {expected_touched:?}, received {actual_touched:?}",
                    actual.id.key, actual.id.generation
                ),
            ));
        }
    }
    Ok(())
}

fn canonical_image(plan: &MachinePlan, image: &RestoreImage) -> CanonicalImage {
    let scalars = plan
        .persistence
        .memory
        .iter()
        .filter_map(|memory| {
            image.scalars.get(&memory.memory_id).map(|scalar| {
                (
                    memory.semantic_path.clone(),
                    (scalar.touched, scalar.value.clone()),
                )
            })
        })
        .collect();
    let lists = plan
        .persistence
        .lists
        .iter()
        .filter_map(|list| {
            image.lists.get(&list.memory_id).map(|stored| {
                let rows = stored
                    .rows
                    .iter()
                    .map(|row| {
                        let values = row
                            .fields
                            .iter()
                            .filter_map(|(field, value)| {
                                list.row_fields
                                    .iter()
                                    .find(|leaf| leaf.leaf_id == *field)
                                    .map(|leaf| {
                                        (local_name(&leaf.semantic_path).to_owned(), value.clone())
                                    })
                            })
                            .collect();
                        let touched_fields = row
                            .touched_fields
                            .iter()
                            .filter_map(|field| {
                                list.row_fields
                                    .iter()
                                    .find(|leaf| leaf.leaf_id == *field)
                                    .map(|leaf| local_name(&leaf.semantic_path).to_owned())
                            })
                            .collect();
                        CanonicalRow {
                            key: row.key,
                            generation: row.generation,
                            values,
                            touched_fields,
                        }
                    })
                    .collect();
                (
                    list.semantic_path.clone(),
                    CanonicalList {
                        touched: stored.touched,
                        next_key: stored.next_key,
                        rows,
                    },
                )
            })
        })
        .collect();
    CanonicalImage { scalars, lists }
}

fn local_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

pub fn run_migration_scenario(
    sequence: MigrationSequence,
    scenario: MigrationScenario,
    application_template: ApplicationIdentity,
) -> Result<MigrationScenarioReport, MigrationScenarioError> {
    MigrationScenarioRunner::new(sequence, scenario, application_template)?.run()
}

fn deterministic_worker_config() -> PersistenceWorkerConfig {
    PersistenceWorkerConfig {
        // Controls force an immediate flush, while ordinary turns remain
        // pending long enough for deterministic transaction fault injection.
        coalesce_delay: Duration::from_secs(60 * 60),
        ..PersistenceWorkerConfig::default()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn source_payload(
    values: &BTreeMap<String, MigrationScenarioValue>,
) -> Result<SourcePayload, ActionError> {
    let mut payload = SourcePayload::default();
    for (name, value) in values {
        match (name.as_str(), value) {
            ("text", MigrationScenarioValue::Text(value)) => payload.text = Some(value.clone()),
            ("key", MigrationScenarioValue::Text(value)) => payload.key = Some(value.clone()),
            ("address", MigrationScenarioValue::Text(value)) => {
                payload.address = Some(value.clone())
            }
            ("text" | "key" | "address", _) => {
                return Err(ActionError::new(
                    "invalid_payload",
                    format!("reserved payload field `{name}` must be text"),
                ));
            }
            _ => {
                payload.fields.insert(name.clone(), scenario_value(value));
            }
        }
    }
    Ok(payload)
}

fn scenario_value(value: &MigrationScenarioValue) -> Value {
    match value {
        MigrationScenarioValue::Bool(value) => Value::Bool(*value),
        MigrationScenarioValue::Integer(value) => Value::Number(
            FiniteReal::from_i64_exact(*value)
                .expect("validated migration scenario integer must be exactly representable"),
        ),
        MigrationScenarioValue::Text(value) => Value::Text(value.clone()),
    }
}

fn fault_applies(point: MigrationFaultPoint, action: &MigrationLifecycleAction) -> bool {
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

fn fault_name(point: MigrationFaultPoint) -> &'static str {
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

fn fault_error(point: MigrationFaultPoint) -> ActionError {
    ActionError::new(
        format!("{}_failed", fault_name(point)),
        format!("injected {} failure", fault_name(point)),
    )
}

trait PersistentRuntimeImageTurn {
    fn driver_image_turn(&self, driver: &SharedInMemoryDriver) -> Option<u64>;
}

impl PersistentRuntimeImageTurn for PersistentRuntime {
    fn driver_image_turn(&self, driver: &SharedInMemoryDriver) -> Option<u64> {
        driver
            .image(&self.runtime().machine_plan().application.identity)
            .map(|image| image.through_turn_sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn fixture(name: &str) -> (MigrationSequence, MigrationScenario) {
        let root = repository_root();
        let examples = root.join("examples");
        let sequence = MigrationSequence::from_path(
            examples.join(format!("migrations/{name}/sequence.toml")),
            &examples,
        )
        .unwrap_or_else(|error| panic!("load {name} migration sequence: {error}"));
        let scenario = MigrationScenario::from_path(root.join(&sequence.scenario), &sequence)
            .unwrap_or_else(|error| panic!("load {name} migration scenario: {error}"));
        (sequence, scenario)
    }

    fn identity(name: &str) -> ApplicationIdentity {
        ApplicationIdentity::new(
            format!("dev.boon.migration-scenario.{name}"),
            "scenario-template",
            "test",
        )
    }

    #[test]
    fn counter_migration_scenario_runs_deterministically() {
        let (sequence, scenario) = fixture("counter");
        let expected_step_count = scenario.steps.len();
        let first = run_migration_scenario(sequence.clone(), scenario.clone(), identity("counter"))
            .expect("first Counter migration scenario run");
        let second = run_migration_scenario(sequence, scenario, identity("counter"))
            .expect("second Counter migration scenario run");

        assert_eq!(first, second);
        assert_eq!(first.steps.len(), expected_step_count);
        assert_eq!(
            first.steps.last().map(|step| {
                (
                    step.current_stage.as_deref(),
                    step.durable_stage.as_deref(),
                    step.preview_stage.as_deref(),
                )
            }),
            Some((Some("v3"), Some("v3"), None))
        );
        assert!(first.steps.iter().any(|step| {
            step.expected_failure_code.as_deref() == Some("candidate_settle_failed")
        }));
    }

    #[test]
    fn todo_incremental_and_skipped_migrations_share_one_generic_runner() {
        let (sequence, scenario) = fixture("todo");
        let expected_step_count = scenario.steps.len();
        let report = run_migration_scenario(sequence, scenario, identity("todo"))
            .expect("Todo migration scenario run");

        assert_eq!(report.steps.len(), expected_step_count);
        assert!(report.steps.iter().any(|step| {
            step.id == "activate-incremental-v7"
                && step.current_stage.as_deref() == Some("v7")
                && step.durable_stage.as_deref() == Some("v7")
        }));
        assert!(report.steps.iter().any(|step| {
            step.id == "activate-skipped-v7"
                && step.current_stage.as_deref() == Some("v7")
                && step.durable_stage.as_deref() == Some("v7")
        }));
    }

    #[test]
    fn persons_source_controlled_migration_preserves_authority_across_paths() {
        let (sequence, scenario) = fixture("persons_pro");
        let expected_step_count = scenario.steps.len();
        let prepared = MigrationScenarioRunner::new(
            sequence.clone(),
            scenario.clone(),
            identity("persons-plan-identity"),
        )
        .expect("prepare Persons.pro migration stages");
        let compiled = prepared
            .compile_stages(identity("persons-plan-identity"))
            .expect("compile Persons.pro migration stages");
        let source_draft_memory = ["v1", "v2", "v3"].map(|stage| {
            scalar_memory(&compiled.plan(stage).unwrap(), "store.source_draft")
                .unwrap()
                .clone()
        });
        assert_eq!(
            source_draft_memory[0].memory_id,
            source_draft_memory[1].memory_id
        );
        assert_eq!(
            source_draft_memory[1].memory_id, source_draft_memory[2].memory_id,
            "v2 owner {:?}; v3 owner {:?}",
            source_draft_memory[1].owner, source_draft_memory[2].owner,
        );
        let first =
            run_migration_scenario(sequence.clone(), scenario.clone(), identity("persons-pro"))
                .expect("first Persons.pro migration scenario run");
        let second = run_migration_scenario(sequence, scenario, identity("persons-pro"))
            .expect("second Persons.pro migration scenario run");

        assert_eq!(first, second);
        assert_eq!(first.steps.len(), expected_step_count);
        assert_eq!(
            first.steps.last().map(|step| {
                (
                    step.current_stage.as_deref(),
                    step.durable_stage.as_deref(),
                    step.preview_stage.as_deref(),
                )
            }),
            Some((Some("v3"), Some("v3"), None))
        );
    }
}
