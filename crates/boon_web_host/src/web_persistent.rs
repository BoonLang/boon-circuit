use boon_persistence::{
    ActivationAck, BarrierAck, BarrierRequest, BrowserPersistenceEnqueueError,
    BrowserPersistenceOperation, BrowserStorageStatus, CheckpointBatch, CommitAck, CompactAck,
    CompactRequest, InspectRequest, MigrationError, MigrationPreview, PersistenceCommand,
    PersistenceInspectorSnapshot, PersistenceResult, ResetApplicationAck, ResetApplicationBatch,
    RestoreImage, RestoreRequest, RexieDriver, ShutdownAck, ShutdownRequest, StoreError,
    stage_migration,
};
use boon_plan::{MachinePlan, SourceRouteToken};
use boon_runtime::{
    DocumentPatch, LiveRuntime, LiveRuntimeBuildPoll, MachineTemplate, RuntimeTurn, SessionOptions,
    SourceEvent, SourcePayload, TurnMetrics, Value,
};
use gloo_timers::future::TimeoutFuture;
use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

const WEB_PERSISTENT_BUILD_STEPS_PER_YIELD: usize = 256;

#[derive(Debug)]
pub enum WebPersistenceError {
    DurabilityBackpressure {
        capacity: usize,
        pending_turns: usize,
    },
    DurabilityFailed(String),
    DurabilityAdmission(BrowserPersistenceEnqueueError),
    OutcomeUnknownReopenRequired {
        operation: &'static str,
    },
    Runtime(String),
    Store(StoreError),
    Protocol(String),
    Migration(MigrationError),
    MissingDurableState,
}

impl fmt::Display for WebPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DurabilityBackpressure {
                capacity,
                pending_turns,
            } => write!(
                formatter,
                "browser durability queue has {pending_turns} pending turns at capacity {capacity}"
            ),
            Self::DurabilityFailed(detail) => {
                write!(formatter, "browser durability lane failed: {detail}")
            }
            Self::DurabilityAdmission(error) => {
                write!(formatter, "browser durability admission failed: {error}")
            }
            Self::OutcomeUnknownReopenRequired { operation } => write!(
                formatter,
                "browser persistence {operation} was admitted but its outcome is unknown; reopen the runtime before further mutation"
            ),
            Self::Runtime(detail) => write!(formatter, "runtime operation failed: {detail}"),
            Self::Store(error) => write!(formatter, "browser persistence failed: {error}"),
            Self::Protocol(detail) => {
                write!(formatter, "browser persistence protocol failed: {detail}")
            }
            Self::Migration(error) => write!(formatter, "browser migration failed: {error}"),
            Self::MissingDurableState => {
                formatter.write_str("browser persistence has no durable application state")
            }
        }
    }
}

impl std::error::Error for WebPersistenceError {}

#[derive(Debug)]
pub enum WebPersistentDispatchError {
    Runtime(WebPersistenceError),
    AdmissionFailed {
        turn: Box<RuntimeTurn>,
        error: WebPersistenceError,
        rollback_error: Option<String>,
    },
}

impl fmt::Display for WebPersistentDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::AdmissionFailed {
                error,
                rollback_error,
                ..
            } => match rollback_error {
                Some(rollback) => write!(
                    formatter,
                    "authority admission failed with `{error}` and runtime rollback failed with `{rollback}`"
                ),
                None => write!(
                    formatter,
                    "authority admission failed and the runtime turn was rolled back: {error}"
                ),
            },
        }
    }
}

impl std::error::Error for WebPersistentDispatchError {}

impl WebRuntimeBuildStats {
    fn merge(self, other: Self) -> Self {
        Self {
            work_slices: self.work_slices.saturating_add(other.work_slices),
            event_loop_yields: self
                .event_loop_yields
                .saturating_add(other.event_loop_yields),
        }
    }
}

#[derive(Debug)]
pub struct WebDurablyAcknowledgedTurn {
    pub turn: RuntimeTurn,
    pub acknowledgement: CommitAck,
}

pub struct WebPersistenceStartup {
    pub restore_image: RestoreImage,
    pub initialized: bool,
    pub mount: RuntimeTurn,
    pub build: WebRuntimeBuildStats,
}

pub struct WebPlanActivation {
    pub mount: RuntimeTurn,
    pub acknowledgement: Option<ActivationAck>,
    pub migration: Option<MigrationPreview>,
    pub build: WebRuntimeBuildStats,
}

pub struct WebPlanReset {
    pub mount: RuntimeTurn,
    pub acknowledgement: ResetApplicationAck,
    pub build: WebRuntimeBuildStats,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WebRuntimeBuildStats {
    pub work_slices: u64,
    pub event_loop_yields: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebPersistentRuntimeStatus {
    pub application: boon_plan::ApplicationIdentity,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub durable_epoch: u64,
    pub durable_through_turn_sequence: u64,
    pub admitted_epoch: u64,
    pub admitted_through_turn_sequence: u64,
    pub pending_turn_count: usize,
    pub outstanding_operation_count: usize,
    pub outstanding_payload_bytes: usize,
    pub outstanding_change_count: usize,
    pub max_outstanding_payload_bytes: usize,
    pub max_outstanding_change_count: usize,
    pub pending_control: Option<&'static str>,
    pub outcome_unknown_reopen_required: bool,
    pub durability_failure: Option<String>,
    pub command_queue_capacity: usize,
    pub storage: BrowserStorageStatus,
}

#[derive(Clone, Debug)]
struct DurableCursor {
    application: boon_plan::ApplicationIdentity,
    schema_version: u64,
    schema_hash: [u8; 32],
    epoch: u64,
    through_turn_sequence: u64,
}

struct PendingWebCommit {
    operation: BrowserPersistenceOperation,
    expected_epoch: u64,
    expected_turn_sequence: u64,
}

struct PendingWebControl {
    operation: BrowserPersistenceOperation,
    label: &'static str,
}

impl DurableCursor {
    fn from_image(image: &RestoreImage) -> Self {
        Self {
            application: image.application.clone(),
            schema_version: image.schema_version,
            schema_hash: image.schema_hash,
            epoch: image.epoch,
            through_turn_sequence: image.through_turn_sequence,
        }
    }
}

/// Browser runtime with a bounded asynchronous IndexedDB durability lane.
///
/// A source turn becomes visible after the complete persistence command is admitted, never after
/// IndexedDB completion. Admission failure rolls the still-unsettled runtime turn back. Later
/// durability failure closes further admission and remains visible through status/flush.
pub struct WebPersistentRuntime {
    runtime: LiveRuntime,
    persistence: RexieDriver,
    plan: Arc<MachinePlan>,
    template: MachineTemplate,
    options: SessionOptions,
    durable: DurableCursor,
    admitted: DurableCursor,
    pending_commits: VecDeque<PendingWebCommit>,
    pending_fence: Option<BrowserPersistenceOperation>,
    pending_control: Option<PendingWebControl>,
    reopen_required_operation: Option<&'static str>,
    durability_failure: Option<String>,
}

impl WebPersistentRuntime {
    pub async fn open(
        plan: MachinePlan,
        options: SessionOptions,
        database_name: impl Into<String>,
    ) -> Result<(Self, WebPersistenceStartup), WebPersistenceError> {
        Self::open_shared(Arc::new(plan), options, database_name).await
    }

    pub async fn open_shared(
        plan: Arc<MachinePlan>,
        options: SessionOptions,
        database_name: impl Into<String>,
    ) -> Result<(Self, WebPersistenceStartup), WebPersistenceError> {
        let persistence = RexieDriver::open(database_name)
            .await
            .map_err(WebPersistenceError::Store)?;
        Self::from_shared_machine_plan(plan, options, persistence).await
    }

    pub async fn from_shared_machine_plan(
        plan: Arc<MachinePlan>,
        options: SessionOptions,
        mut persistence: RexieDriver,
    ) -> Result<(Self, WebPersistenceStartup), WebPersistenceError> {
        let template = MachineTemplate::new_shared(Arc::clone(&plan)).map_err(runtime_error)?;
        let loaded = load_image(&mut persistence, &plan.application.identity).await?;

        let (runtime, restore_image, initialized, build) = match loaded {
            None => {
                let (default_runtime, build) =
                    build_live_runtime(&template, options.clone(), None).await?;
                let initial_image = default_runtime
                    .durable_restore_image(0, BTreeSet::new())
                    .map_err(runtime_error)?;
                let acknowledgement =
                    initialize_image(&mut persistence, initial_image.clone()).await?;
                ensure_commit_ack(
                    &acknowledgement,
                    initial_image.epoch,
                    initial_image.through_turn_sequence,
                    "Initialize",
                )?;
                (default_runtime, initial_image, true, build)
            }
            Some(stored) => {
                if stored.application != plan.application.identity {
                    return Err(WebPersistenceError::Store(StoreError::IdentityMismatch));
                }
                if stored.schema_version == plan.persistence.schema_version
                    && stored.schema_hash == plan.persistence.schema_hash
                {
                    let (runtime, build) =
                        build_live_runtime(&template, options.clone(), Some(stored.clone()))
                            .await?;
                    (runtime, stored, false, build)
                } else {
                    let staged =
                        stage_migration(&stored, &plan).map_err(WebPersistenceError::Migration)?;
                    let (runtime, build) = build_live_runtime(
                        &template,
                        options.clone(),
                        Some(staged.candidate.clone()),
                    )
                    .await?;
                    let acknowledgement =
                        activate_batch(&mut persistence, staged.activation).await?;
                    ensure_activation_ack(
                        &acknowledgement,
                        staged.candidate.schema_version,
                        staged.candidate.schema_hash,
                        staged.candidate.through_turn_sequence,
                    )?;
                    let mut restored = staged.candidate;
                    restored.epoch = acknowledgement.epoch;
                    (runtime, restored, false, build)
                }
            }
        };
        let mount = runtime.mount();
        let durable = DurableCursor::from_image(&restore_image);
        let admitted = durable.clone();
        Ok((
            Self {
                runtime,
                persistence,
                plan,
                template,
                options,
                durable,
                admitted,
                pending_commits: VecDeque::new(),
                pending_fence: None,
                pending_control: None,
                reopen_required_operation: None,
                durability_failure: None,
            },
            WebPersistenceStartup {
                restore_image,
                initialized,
                mount,
                build,
            },
        ))
    }

    pub fn runtime(&self) -> &LiveRuntime {
        &self.runtime
    }

    pub fn status(&self) -> WebPersistentRuntimeStatus {
        WebPersistentRuntimeStatus {
            application: self.durable.application.clone(),
            schema_version: self.durable.schema_version,
            schema_hash: self.durable.schema_hash,
            durable_epoch: self.durable.epoch,
            durable_through_turn_sequence: self.durable.through_turn_sequence,
            admitted_epoch: self.admitted.epoch,
            admitted_through_turn_sequence: self.admitted.through_turn_sequence,
            pending_turn_count: self.pending_commits.len(),
            outstanding_operation_count: self.persistence.outstanding_operation_count(),
            outstanding_payload_bytes: self.persistence.outstanding_payload_bytes(),
            outstanding_change_count: self.persistence.outstanding_change_count(),
            max_outstanding_payload_bytes: self.persistence.max_outstanding_payload_bytes(),
            max_outstanding_change_count: self.persistence.max_outstanding_change_count(),
            pending_control: self
                .pending_control
                .as_ref()
                .map(|pending| pending.label)
                .or(self.reopen_required_operation),
            outcome_unknown_reopen_required: self.pending_control.is_some()
                || self.reopen_required_operation.is_some(),
            durability_failure: self.durability_failure.clone(),
            command_queue_capacity: self.persistence.command_queue_capacity(),
            storage: self.persistence.storage_status().clone(),
        }
    }

    pub fn set_durability_admission_limits(
        &mut self,
        max_outstanding_payload_bytes: usize,
        max_outstanding_change_count: usize,
    ) -> Result<(), WebPersistenceError> {
        self.ensure_operational()?;
        self.persistence
            .set_admission_limits(max_outstanding_payload_bytes, max_outstanding_change_count)
            .map_err(WebPersistenceError::Store)
    }

    pub async fn refresh_storage_status(&mut self) -> &BrowserStorageStatus {
        let _ = self.poll_durability();
        self.persistence.refresh_storage_status().await
    }

    pub fn source_event(
        &self,
        sequence: u64,
        route: SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<SourceEvent, WebPersistenceError> {
        self.runtime
            .source_event(sequence, route, payload)
            .map_err(runtime_error)
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, WebPersistenceError> {
        self.runtime.root_value_current(name).map_err(runtime_error)
    }

    pub fn root_value_current_with_metrics(
        &mut self,
        name: &str,
    ) -> Result<(Value, TurnMetrics), WebPersistenceError> {
        self.runtime
            .root_value_current_with_metrics(name)
            .map_err(runtime_error)
    }

    pub fn startup_metrics(&self) -> &TurnMetrics {
        self.runtime.startup_metrics()
    }

    pub fn output_value_current(&mut self, name: &str) -> Result<Value, WebPersistenceError> {
        self.runtime
            .output_value_current(name)
            .map_err(runtime_error)
    }

    pub fn inspect_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, WebPersistenceError> {
        self.runtime
            .inspect_value_current(name, max_rows)
            .map_err(runtime_error)
    }

    pub fn demand_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> Result<Vec<DocumentPatch>, WebPersistenceError> {
        self.runtime
            .demand_document_window_by_id(materialization, visible, overscan)
            .map_err(runtime_error)
    }

    /// Executes and publishes one turn after bounded persistence admission. No
    /// IndexedDB future is polled on this path.
    pub fn dispatch(
        &mut self,
        event: SourceEvent,
    ) -> Result<RuntimeTurn, WebPersistentDispatchError> {
        self.poll_durability()
            .map_err(WebPersistentDispatchError::Runtime)?;
        self.ensure_operational()
            .map_err(WebPersistentDispatchError::Runtime)?;
        let capacity = self.persistence.command_queue_capacity();
        if self.pending_commits.len() >= capacity {
            return Err(WebPersistentDispatchError::Runtime(
                WebPersistenceError::DurabilityBackpressure {
                    capacity,
                    pending_turns: self.pending_commits.len(),
                },
            ));
        }

        let turn = self.runtime.dispatch_unsettled(event).map_err(|error| {
            WebPersistentDispatchError::Runtime(WebPersistenceError::Runtime(error.to_string()))
        })?;
        if let Err(error) = self.persistence.validate_checkpoint_admission(
            &turn.durable_changes,
            &turn.outbox_changes,
            &[],
        ) {
            return Err(self.rollback_admission(turn, admission_error(error)));
        }
        let batch = match self.checkpoint_for_turn(&turn) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(self.rollback_admission(turn, WebPersistenceError::Store(error)));
            }
        };
        let expected_epoch = batch.next_epoch;
        let expected_turn_sequence = batch.last_turn_sequence;
        let operation = match self
            .persistence
            .try_enqueue(PersistenceCommand::Commit(batch))
        {
            Ok(operation) => operation,
            Err(error) => return Err(self.rollback_admission(turn, admission_error(error))),
        };
        self.pending_commits.push_back(PendingWebCommit {
            operation,
            expected_epoch,
            expected_turn_sequence,
        });
        self.admitted.epoch = expected_epoch;
        self.admitted.through_turn_sequence = expected_turn_sequence;
        self.runtime.settle_turn();
        Ok(turn)
    }

    pub async fn dispatch_durably(
        &mut self,
        event: SourceEvent,
    ) -> Result<WebDurablyAcknowledgedTurn, WebPersistentDispatchError> {
        let turn = self.dispatch(event)?;
        let acknowledgement = self
            .flush_pending_through(turn.sequence)
            .await
            .map_err(WebPersistentDispatchError::Runtime)?
            .ok_or_else(|| {
                WebPersistentDispatchError::Runtime(WebPersistenceError::Protocol(
                    "durable dispatch lost its exact commit acknowledgement".to_owned(),
                ))
            })?;
        Ok(WebDurablyAcknowledgedTurn {
            turn,
            acknowledgement,
        })
    }

    pub fn poll_durability(&mut self) -> Result<usize, WebPersistenceError> {
        let mut completed = 0;
        let already_failed = self.durability_failure.is_some();
        loop {
            let Some(pending) = self.pending_commits.front() else {
                break;
            };
            let result = match self.persistence.try_complete(&pending.operation) {
                Ok(Some(result)) => result,
                Ok(None) => break,
                Err(error) => {
                    let error = admission_error(error);
                    self.record_durability_failure(&error);
                    return Err(error);
                }
            };
            let pending = self
                .pending_commits
                .pop_front()
                .expect("completed browser commit remains queued");
            if already_failed {
                completed += 1;
                continue;
            }
            if let Err(error) = self.accept_commit_result(
                pending.expected_epoch,
                pending.expected_turn_sequence,
                result,
            ) {
                self.record_durability_failure(&error);
                return Err(error);
            }
            completed += 1;
        }
        match &self.durability_failure {
            Some(detail) if already_failed => {
                Err(WebPersistenceError::DurabilityFailed(detail.clone()))
            }
            _ => Ok(completed),
        }
    }

    pub async fn flush(&mut self) -> Result<BarrierAck, WebPersistenceError> {
        self.flush_pending_through(self.admitted.through_turn_sequence)
            .await?;
        self.ensure_operational()?;
        let operation = match self.pending_fence {
            Some(operation) => operation,
            None => {
                let operation = self
                    .persistence
                    .try_enqueue(PersistenceCommand::Barrier(BarrierRequest {
                        application: self.durable.application.clone(),
                        through_epoch: self.durable.epoch,
                    }))
                    .map_err(admission_error)?;
                self.pending_fence = Some(operation);
                operation
            }
        };
        let result = match self.persistence.complete(&operation).await {
            Ok(result) => {
                self.pending_fence = None;
                result
            }
            Err(error) => {
                let error = admission_error(error);
                self.record_durability_failure(&error);
                return Err(error);
            }
        };
        let outcome = match result {
            PersistenceResult::BarrierComplete(Ok(acknowledgement)) => Ok(acknowledgement),
            PersistenceResult::BarrierComplete(Err(error)) => {
                Err(WebPersistenceError::Store(error))
            }
            other => Err(unexpected_result("Barrier", &other)),
        };
        if let Err(error) = &outcome {
            self.record_durability_failure(error);
        }
        outcome
    }

    pub async fn inspect(
        &mut self,
    ) -> Result<Option<PersistenceInspectorSnapshot>, WebPersistenceError> {
        self.flush().await?;
        let result = self
            .persistence
            .execute(PersistenceCommand::Inspect(InspectRequest {
                application: self.durable.application.clone(),
            }))
            .await;
        match result {
            PersistenceResult::Inspected(Ok(snapshot)) => Ok(snapshot),
            PersistenceResult::Inspected(Err(error)) => Err(WebPersistenceError::Store(error)),
            other => Err(unexpected_result("Inspect", &other)),
        }
    }

    pub async fn compact(&mut self) -> Result<CompactAck, WebPersistenceError> {
        self.flush().await?;
        let result = self
            .persistence
            .execute(PersistenceCommand::Compact(CompactRequest {
                application: self.durable.application.clone(),
            }))
            .await;
        match result {
            PersistenceResult::Compacted(Ok(acknowledgement)) => Ok(acknowledgement),
            PersistenceResult::Compacted(Err(error)) => Err(WebPersistenceError::Store(error)),
            other => Err(unexpected_result("Compact", &other)),
        }
    }

    pub async fn load_durable_image(&mut self) -> Result<RestoreImage, WebPersistenceError> {
        self.flush().await?;
        load_image(&mut self.persistence, &self.durable.application)
            .await?
            .ok_or(WebPersistenceError::MissingDurableState)
    }

    /// Builds a private candidate, activates storage, then atomically publishes the candidate.
    pub async fn activate_machine_plan(
        &mut self,
        plan: Arc<MachinePlan>,
        options: SessionOptions,
    ) -> Result<WebPlanActivation, WebPersistenceError> {
        let current = self.load_durable_image().await?;
        let (restore, activation, migration) = if current.schema_version
            == plan.persistence.schema_version
            && current.schema_hash == plan.persistence.schema_hash
        {
            (current.clone(), None, None)
        } else {
            let staged =
                stage_migration(&current, &plan).map_err(WebPersistenceError::Migration)?;
            (
                staged.candidate,
                Some(staged.activation),
                Some(staged.preview),
            )
        };
        let template = MachineTemplate::new_shared(Arc::clone(&plan)).map_err(runtime_error)?;
        let (candidate, build) =
            build_live_runtime(&template, options.clone(), Some(restore.clone())).await?;
        let mount = candidate.mount();
        let acknowledgement = match activation {
            Some(batch) => {
                let operation =
                    self.admit_control("Activate", PersistenceCommand::Activate(batch))?;
                let result = self.complete_control(&operation).await?;
                let acknowledgement = match result {
                    PersistenceResult::Activated(Ok(acknowledgement)) => acknowledgement,
                    PersistenceResult::Activated(Err(error)) => {
                        let error = WebPersistenceError::Store(error);
                        self.record_durability_failure(&error);
                        return Err(error);
                    }
                    other => {
                        let error = unexpected_result("Activate", &other);
                        self.record_durability_failure(&error);
                        return Err(error);
                    }
                };
                if let Err(error) = ensure_activation_ack(
                    &acknowledgement,
                    restore.schema_version,
                    restore.schema_hash,
                    restore.through_turn_sequence,
                ) {
                    self.require_reopen("Activate", &error);
                    return Err(error);
                }
                Some(acknowledgement)
            }
            None => None,
        };
        self.runtime = candidate;
        self.plan = plan;
        self.template = template;
        self.options = options;
        self.durable = DurableCursor::from_image(&restore);
        if let Some(acknowledgement) = &acknowledgement {
            self.durable.epoch = acknowledgement.epoch;
        }
        self.admitted = self.durable.clone();
        Ok(WebPlanActivation {
            mount,
            acknowledgement,
            migration,
            build,
        })
    }

    /// Commits current-plan defaults before replacing the published runtime.
    pub async fn start_over(&mut self) -> Result<WebPlanReset, WebPersistenceError> {
        let current = self.load_durable_image().await?;
        let (defaults, default_build) =
            build_live_runtime(&self.template, self.options.clone(), None).await?;
        let default_image = defaults
            .durable_restore_image(0, BTreeSet::new())
            .map_err(runtime_error)?;
        let next_epoch = current
            .epoch
            .checked_add(1)
            .ok_or_else(|| WebPersistenceError::Runtime("durable epoch overflow".to_owned()))?;
        let mut candidate_image = default_image.clone();
        candidate_image.epoch = next_epoch;
        candidate_image.through_turn_sequence = current.through_turn_sequence;
        let (candidate, candidate_build) =
            build_live_runtime(&self.template, self.options.clone(), Some(candidate_image)).await?;
        let mount = candidate.mount();
        let batch = ResetApplicationBatch {
            application: current.application.clone(),
            expected_base_epoch: current.epoch,
            next_epoch,
            source_schema_hash: current.schema_hash,
            default_image,
            checksum: [0; 32],
        }
        .seal();
        let operation = self.admit_control(
            "ResetApplication",
            PersistenceCommand::ResetApplication(batch),
        )?;
        let result = self.complete_control(&operation).await?;
        let acknowledgement = match result {
            PersistenceResult::ApplicationReset(Ok(acknowledgement)) => acknowledgement,
            PersistenceResult::ApplicationReset(Err(error)) => {
                let error = WebPersistenceError::Store(error);
                self.record_durability_failure(&error);
                return Err(error);
            }
            other => {
                let error = unexpected_result("ResetApplication", &other);
                self.record_durability_failure(&error);
                return Err(error);
            }
        };
        if acknowledgement.epoch != next_epoch
            || acknowledgement.schema_version != self.plan.persistence.schema_version
            || acknowledgement.schema_hash != self.plan.persistence.schema_hash
            || acknowledgement.through_turn_sequence != current.through_turn_sequence
        {
            let error = WebPersistenceError::Protocol(
                "ResetApplication returned an inconsistent acknowledgement".to_owned(),
            );
            self.require_reopen("ResetApplication", &error);
            return Err(error);
        }
        self.runtime = candidate;
        self.durable = DurableCursor {
            application: current.application,
            schema_version: acknowledgement.schema_version,
            schema_hash: acknowledgement.schema_hash,
            epoch: acknowledgement.epoch,
            through_turn_sequence: acknowledgement.through_turn_sequence,
        };
        self.admitted = self.durable.clone();
        Ok(WebPlanReset {
            mount,
            acknowledgement,
            build: default_build.merge(candidate_build),
        })
    }

    pub async fn shutdown(&mut self) -> Result<ShutdownAck, WebPersistenceError> {
        self.flush_pending_through(self.admitted.through_turn_sequence)
            .await?;
        self.ensure_operational()?;
        let operation =
            self.admit_control("Shutdown", PersistenceCommand::Shutdown(ShutdownRequest))?;
        let result = self.complete_control(&operation).await?;
        match result {
            PersistenceResult::ShutdownComplete(Ok(acknowledgement)) => Ok(acknowledgement),
            PersistenceResult::ShutdownComplete(Err(error)) => {
                let error = WebPersistenceError::Store(error);
                self.record_durability_failure(&error);
                Err(error)
            }
            other => {
                let error = unexpected_result("Shutdown", &other);
                self.record_durability_failure(&error);
                Err(error)
            }
        }
    }

    fn ensure_operational(&self) -> Result<(), WebPersistenceError> {
        if let Some(operation) = self
            .pending_control
            .as_ref()
            .map(|pending| pending.label)
            .or(self.reopen_required_operation)
        {
            return Err(WebPersistenceError::OutcomeUnknownReopenRequired { operation });
        }
        match &self.durability_failure {
            Some(detail) => Err(WebPersistenceError::DurabilityFailed(detail.clone())),
            None => Ok(()),
        }
    }

    fn checkpoint_for_turn(&self, turn: &RuntimeTurn) -> Result<CheckpointBatch, StoreError> {
        let expected_turn_sequence = self
            .admitted
            .through_turn_sequence
            .checked_add(1)
            .ok_or_else(|| StoreError::Backend("persistence turn sequence overflow".to_owned()))?;
        if turn.sequence != expected_turn_sequence {
            return Err(StoreError::NonContiguousTurn);
        }
        let next_epoch = self
            .admitted
            .epoch
            .checked_add(1)
            .ok_or_else(|| StoreError::Backend("persistence epoch overflow".to_owned()))?;
        Ok(CheckpointBatch {
            application: self.admitted.application.clone(),
            schema_hash: self.admitted.schema_hash,
            base_epoch: self.admitted.epoch,
            next_epoch,
            first_turn_sequence: turn.sequence,
            last_turn_sequence: turn.sequence,
            changes: turn.durable_changes.clone(),
            outbox_changes: turn.outbox_changes.clone(),
            content_artifact_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal())
    }

    fn rollback_admission(
        &mut self,
        turn: RuntimeTurn,
        error: WebPersistenceError,
    ) -> WebPersistentDispatchError {
        let rollback_error = self
            .runtime
            .rollback_unsettled_turn()
            .err()
            .map(|rollback| rollback.to_string());
        WebPersistentDispatchError::AdmissionFailed {
            turn: Box::new(turn),
            error,
            rollback_error,
        }
    }

    fn accept_commit_result(
        &mut self,
        expected_epoch: u64,
        expected_turn_sequence: u64,
        result: PersistenceResult,
    ) -> Result<CommitAck, WebPersistenceError> {
        let acknowledgement = match result {
            PersistenceResult::Committed(Ok(acknowledgement)) => acknowledgement,
            PersistenceResult::Committed(Err(error)) => {
                return Err(WebPersistenceError::Store(error));
            }
            other => return Err(unexpected_result("Commit", &other)),
        };
        ensure_commit_ack(
            &acknowledgement,
            expected_epoch,
            expected_turn_sequence,
            "Commit",
        )?;
        self.durable.epoch = acknowledgement.epoch;
        self.durable.through_turn_sequence = acknowledgement.through_turn_sequence;
        Ok(acknowledgement)
    }

    fn record_durability_failure(&mut self, error: &WebPersistenceError) {
        if self.durability_failure.is_none() {
            self.durability_failure = Some(error.to_string());
        }
    }

    fn require_reopen(&mut self, operation: &'static str, error: &WebPersistenceError) {
        self.reopen_required_operation = Some(operation);
        self.record_durability_failure(error);
    }

    fn admit_control(
        &mut self,
        label: &'static str,
        command: PersistenceCommand,
    ) -> Result<BrowserPersistenceOperation, WebPersistenceError> {
        self.ensure_operational()?;
        let operation = self
            .persistence
            .try_enqueue(command)
            .map_err(admission_error)?;
        self.pending_control = Some(PendingWebControl { operation, label });
        Ok(operation)
    }

    async fn complete_control(
        &mut self,
        operation: &BrowserPersistenceOperation,
    ) -> Result<PersistenceResult, WebPersistenceError> {
        let result = match self.persistence.complete(operation).await {
            Ok(result) => result,
            Err(error) => {
                let error = admission_error(error);
                self.record_durability_failure(&error);
                return Err(error);
            }
        };
        let pending = self.pending_control.take().ok_or_else(|| {
            WebPersistenceError::Protocol(
                "browser persistence control acknowledgement lost its admitted state".to_owned(),
            )
        })?;
        if pending.operation != *operation {
            let error = WebPersistenceError::Protocol(
                "browser persistence control acknowledgement does not match the admitted operation"
                    .to_owned(),
            );
            self.record_durability_failure(&error);
            return Err(error);
        }
        Ok(result)
    }

    async fn flush_pending_through(
        &mut self,
        through_turn_sequence: u64,
    ) -> Result<Option<CommitAck>, WebPersistenceError> {
        self.ensure_operational()?;
        let mut target_acknowledgement = None;
        while self.durable.through_turn_sequence < through_turn_sequence {
            let pending = self.pending_commits.front().ok_or_else(|| {
                WebPersistenceError::Protocol(format!(
                    "durability queue ended at turn {} before requested turn {through_turn_sequence}",
                    self.durable.through_turn_sequence
                ))
            })?;
            let expected_epoch = pending.expected_epoch;
            let expected_turn_sequence = pending.expected_turn_sequence;
            let operation = pending.operation;
            let result = match self.persistence.complete(&operation).await {
                Ok(result) => result,
                Err(error) => {
                    let error = admission_error(error);
                    self.record_durability_failure(&error);
                    return Err(error);
                }
            };
            let pending = self
                .pending_commits
                .pop_front()
                .expect("acknowledged browser commit remains queued");
            debug_assert_eq!(pending.operation, operation);
            match self.accept_commit_result(expected_epoch, expected_turn_sequence, result) {
                Ok(acknowledgement) => {
                    if expected_turn_sequence == through_turn_sequence {
                        target_acknowledgement = Some(acknowledgement);
                    }
                }
                Err(error) => {
                    self.record_durability_failure(&error);
                    return Err(error);
                }
            }
        }
        if self.durable.through_turn_sequence != through_turn_sequence {
            return Err(WebPersistenceError::Protocol(format!(
                "durability advanced through turn {}, requested exact turn {through_turn_sequence}",
                self.durable.through_turn_sequence
            )));
        }
        Ok(target_acknowledgement)
    }
}

fn runtime_error(error: impl fmt::Display) -> WebPersistenceError {
    WebPersistenceError::Runtime(error.to_string())
}

fn admission_error(error: BrowserPersistenceEnqueueError) -> WebPersistenceError {
    match error {
        BrowserPersistenceEnqueueError::Closed => WebPersistenceError::Store(StoreError::Closed),
        other => WebPersistenceError::DurabilityAdmission(other),
    }
}

fn unexpected_result(operation: &str, result: &PersistenceResult) -> WebPersistenceError {
    WebPersistenceError::Protocol(format!(
        "driver returned the wrong result for {operation}: {result:?}"
    ))
}

fn ensure_commit_ack(
    acknowledgement: &CommitAck,
    expected_epoch: u64,
    expected_turn_sequence: u64,
    operation: &str,
) -> Result<(), WebPersistenceError> {
    if acknowledgement.epoch == expected_epoch
        && acknowledgement.through_turn_sequence == expected_turn_sequence
    {
        Ok(())
    } else {
        Err(WebPersistenceError::Protocol(format!(
            "{operation} returned epoch {} through turn {}, expected epoch {expected_epoch} through turn {expected_turn_sequence}",
            acknowledgement.epoch, acknowledgement.through_turn_sequence
        )))
    }
}

fn ensure_activation_ack(
    acknowledgement: &ActivationAck,
    schema_version: u64,
    schema_hash: [u8; 32],
    through_turn_sequence: u64,
) -> Result<(), WebPersistenceError> {
    if acknowledgement.schema_version == schema_version
        && acknowledgement.schema_hash == schema_hash
        && acknowledgement.through_turn_sequence == through_turn_sequence
    {
        Ok(())
    } else {
        Err(WebPersistenceError::Protocol(
            "Activate returned an inconsistent acknowledgement".to_owned(),
        ))
    }
}

async fn build_live_runtime(
    template: &MachineTemplate,
    options: SessionOptions,
    restore: Option<RestoreImage>,
) -> Result<(LiveRuntime, WebRuntimeBuildStats), WebPersistenceError> {
    let mut task =
        LiveRuntime::begin_machine_template_build_with_restore(template, options, restore)
            .map_err(runtime_error)?;
    let mut build = WebRuntimeBuildStats::default();
    loop {
        build.work_slices = build.work_slices.saturating_add(1);
        match task
            .poll(WEB_PERSISTENT_BUILD_STEPS_PER_YIELD)
            .map_err(runtime_error)?
        {
            LiveRuntimeBuildPoll::Pending(_) => {
                build.event_loop_yields = build.event_loop_yields.saturating_add(1);
                TimeoutFuture::new(0).await;
            }
            LiveRuntimeBuildPoll::Ready(runtime) => return Ok((runtime, build)),
        }
    }
}

async fn load_image(
    persistence: &mut RexieDriver,
    application: &boon_plan::ApplicationIdentity,
) -> Result<Option<RestoreImage>, WebPersistenceError> {
    let result = persistence
        .execute(PersistenceCommand::Load(RestoreRequest {
            application: application.clone(),
            expected_schema_hash: None,
        }))
        .await;
    match result {
        PersistenceResult::Loaded(Ok(image)) => Ok(image),
        PersistenceResult::Loaded(Err(error)) => Err(WebPersistenceError::Store(error)),
        other => Err(unexpected_result("Load", &other)),
    }
}

async fn initialize_image(
    persistence: &mut RexieDriver,
    image: RestoreImage,
) -> Result<CommitAck, WebPersistenceError> {
    let result = persistence
        .execute(PersistenceCommand::Initialize(image))
        .await;
    match result {
        PersistenceResult::Initialized(Ok(acknowledgement)) => Ok(acknowledgement),
        PersistenceResult::Initialized(Err(error)) => Err(WebPersistenceError::Store(error)),
        other => Err(unexpected_result("Initialize", &other)),
    }
}

async fn activate_batch(
    persistence: &mut RexieDriver,
    batch: boon_persistence::ActivationBatch,
) -> Result<ActivationAck, WebPersistenceError> {
    let result = persistence
        .execute(PersistenceCommand::Activate(batch))
        .await;
    match result {
        PersistenceResult::Activated(Ok(acknowledgement)) => Ok(acknowledgement),
        PersistenceResult::Activated(Err(error)) => Err(WebPersistenceError::Store(error)),
        other => Err(unexpected_result("Activate", &other)),
    }
}
