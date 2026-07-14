use super::effects::{
    HostEffectDriver, HostEffectError, HostEffectReconciliation, HostEffectRequest,
    HostEffectWorker, HostEffectWorkerOperation, HostEffectWorkerOutcome,
};
use super::{DocumentPatch, LiveRuntime, RuntimeTurn};
use boon_persistence::{
    ActivationAck, ActivationBatch, AuthorityTurn, BarrierAck, CommitAck, CompactAck, DecodeLimits,
    DurableOutboxItem, DurableOutboxState, OutboxItemId, PersistenceControlError,
    PersistenceCoordinator, PersistenceDriver, PersistenceInspectorSnapshot, PersistenceStartup,
    PersistenceWorkerConfig, PersistenceWorkerStartError, PersistenceWorkerStatus,
    ResetApplicationAck, ResetApplicationBatch, StoredValue, TurnEnqueueError,
    TurnReservationError, decode_restore_image, encode_restore_image, stage_migration,
};
use boon_plan::{MachinePlan, MemoryKind};
use boon_plan_executor::{SessionOptions, SourceEvent, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug)]
pub enum PersistentRuntimeStartError {
    Runtime(String),
    Persistence(PersistenceWorkerStartError),
    Migration(boon_persistence::MigrationError),
    Activation(PersistenceControlError),
}

impl fmt::Display for PersistentRuntimeStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(detail) => write!(formatter, "runtime startup failed: {detail}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::Migration(error) => write!(formatter, "persistence migration failed: {error}"),
            Self::Activation(error) => {
                write!(
                    formatter,
                    "persistence migration activation failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for PersistentRuntimeStartError {}

#[derive(Debug)]
pub enum PersistentDispatchError {
    Backpressure(TurnReservationError),
    Runtime(String),
    PersistenceAdmissionFailed {
        turn: Box<RuntimeTurn>,
        error: TurnEnqueueError,
        rollback_error: Option<String>,
    },
    ImmediateCommitFailed {
        turn: Box<RuntimeTurn>,
        error: PersistenceControlError,
        rollback_error: Option<String>,
    },
}

impl fmt::Display for PersistentDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure(error) => write!(formatter, "{error}"),
            Self::Runtime(detail) => write!(formatter, "runtime turn failed: {detail}"),
            Self::PersistenceAdmissionFailed {
                error,
                rollback_error,
                ..
            } => match rollback_error {
                Some(rollback) => write!(
                    formatter,
                    "persistence admission failed with `{error}` and runtime rollback failed with `{rollback}`"
                ),
                None => write!(
                    formatter,
                    "persistence admission failed and the prepared runtime turn was rolled back: {error}"
                ),
            },
            Self::ImmediateCommitFailed {
                error,
                rollback_error,
                ..
            } => match rollback_error {
                Some(rollback) => write!(
                    formatter,
                    "immediate persistence commit failed with `{error}` and runtime rollback failed with `{rollback}`"
                ),
                None => write!(
                    formatter,
                    "immediate persistence commit failed and the prepared runtime turn was rolled back: {error}"
                ),
            },
        }
    }
}

impl std::error::Error for PersistentDispatchError {}

#[derive(Debug)]
pub enum PersistentActivationError {
    Persistence(PersistenceControlError),
    Runtime(String),
    Migration(boon_persistence::MigrationError),
    MissingDurableState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectWorkKind {
    Dispatch,
    Reconcile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectWorkItem {
    pub kind: EffectWorkKind,
    pub item: DurableOutboxItem,
}

#[derive(Debug)]
pub enum PersistentEffectError {
    Persistence(PersistenceControlError),
    Runtime(String),
    MissingItem(OutboxItemId),
    CommitFailed {
        error: PersistenceControlError,
        rollback_error: Option<String>,
    },
}

impl fmt::Display for PersistentEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(error) => write!(formatter, "effect persistence failed: {error}"),
            Self::Runtime(detail) => write!(formatter, "effect runtime failed: {detail}"),
            Self::MissingItem(item) => write!(formatter, "outbox item {item} does not exist"),
            Self::CommitFailed {
                error,
                rollback_error,
            } => match rollback_error {
                Some(rollback) => write!(
                    formatter,
                    "effect commit failed with `{error}` and rollback failed with `{rollback}`"
                ),
                None => write!(
                    formatter,
                    "effect commit failed and the runtime transition was rolled back: {error}"
                ),
            },
        }
    }
}

impl std::error::Error for PersistentEffectError {}

#[derive(Debug)]
pub enum PersistentEffectDriveError {
    Runtime(PersistentEffectError),
    Host(HostEffectError),
    HostAndRecovery {
        host: HostEffectError,
        recovery: PersistentEffectError,
    },
}

impl fmt::Display for PersistentEffectDriveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::Host(error) => write!(formatter, "host effect failed: {error}"),
            Self::HostAndRecovery { host, recovery } => write!(
                formatter,
                "host effect failed with `{host}` and durable reconciliation marking failed with `{recovery}`"
            ),
        }
    }
}

impl std::error::Error for PersistentEffectDriveError {}

impl fmt::Display for PersistentActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::Runtime(detail) => write!(formatter, "candidate activation failed: {detail}"),
            Self::Migration(error) => write!(formatter, "candidate migration failed: {error}"),
            Self::MissingDurableState => {
                formatter.write_str("persistence worker has no durable application state")
            }
        }
    }
}

impl std::error::Error for PersistentActivationError {}

/// Owns the only mutable runtime together with its bounded durability lane.
///
/// Queue capacity is reserved before dispatch so ordinary saturation rejects a
/// turn without mutating application authority. The accepted turn contains
/// stable `MemoryId` changes and is moved to the worker without database work.
pub struct PersistentRuntime {
    runtime: LiveRuntime,
    persistence: PersistenceCoordinator,
    last_rebuild_derived_us: u64,
}

pub struct PersistentPlanActivation {
    pub mount: RuntimeTurn,
    pub acknowledgement: Option<ActivationAck>,
    pub migration: Option<boon_persistence::MigrationPreview>,
}

pub struct PersistentPlanPreview {
    pub migration: Option<boon_persistence::MigrationPreview>,
    pub target_schema_version: u64,
    pub target_schema_hash: [u8; 32],
    pub document_node_count: usize,
}

pub struct PersistentPlanReset {
    pub mount: RuntimeTurn,
    pub acknowledgement: ResetApplicationAck,
}

pub struct PersistentStateArtifactPreview {
    pub source_schema_version: u64,
    pub target_schema_version: u64,
    pub scalar_count: usize,
    pub list_count: usize,
    pub row_count: usize,
    pub migration: Option<boon_persistence::MigrationPreview>,
    pub document_node_count: usize,
}

pub struct PersistentStateArtifactActivation {
    pub mount: RuntimeTurn,
    pub acknowledgement: ActivationAck,
    pub preview: PersistentStateArtifactPreview,
}

pub struct DurablyAcknowledgedTurn {
    pub turn: RuntimeTurn,
    pub acknowledgement: CommitAck,
}

impl PersistentRuntime {
    pub fn from_machine_plan<D>(
        plan: MachinePlan,
        options: SessionOptions,
        driver: D,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, PersistenceStartup), PersistentRuntimeStartError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        Self::from_shared_machine_plan(Arc::new(plan), options, driver, config)
    }

    pub fn from_shared_machine_plan<D>(
        plan: Arc<MachinePlan>,
        options: SessionOptions,
        driver: D,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, PersistenceStartup), PersistentRuntimeStartError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        let rebuild_started = Instant::now();
        let default_runtime =
            LiveRuntime::from_shared_machine_plan(Arc::clone(&plan), options.clone())
                .map_err(|error| PersistentRuntimeStartError::Runtime(error.to_string()))?;
        let mut last_rebuild_derived_us = duration_us(rebuild_started.elapsed());
        let initial_image = default_runtime
            .durable_restore_image(0, BTreeSet::new())
            .map_err(|error| PersistentRuntimeStartError::Runtime(error.to_string()))?;
        let (persistence, mut startup) =
            PersistenceCoordinator::start(driver, initial_image, config)
                .map_err(PersistentRuntimeStartError::Persistence)?;

        let runtime = if startup.initialized {
            default_runtime
        } else if startup.restore_image.schema_version == plan.persistence.schema_version
            && startup.restore_image.schema_hash == plan.persistence.schema_hash
        {
            let rebuild_started = Instant::now();
            let runtime = LiveRuntime::from_shared_machine_plan_with_restore(
                plan,
                options,
                Some(startup.restore_image.clone()),
            )
            .map_err(|error| PersistentRuntimeStartError::Runtime(error.to_string()))?;
            last_rebuild_derived_us = duration_us(rebuild_started.elapsed());
            runtime
        } else {
            let staged = match stage_migration(&startup.restore_image, &plan) {
                Ok(staged) => staged,
                Err(error) => {
                    let _ = persistence.shutdown();
                    return Err(PersistentRuntimeStartError::Migration(error));
                }
            };
            let rebuild_started = Instant::now();
            let candidate = match LiveRuntime::from_shared_machine_plan_with_restore(
                Arc::clone(&plan),
                options,
                Some(staged.candidate.clone()),
            ) {
                Ok(candidate) => candidate,
                Err(error) => {
                    let _ = persistence.shutdown();
                    return Err(PersistentRuntimeStartError::Runtime(error.to_string()));
                }
            };
            last_rebuild_derived_us = duration_us(rebuild_started.elapsed());
            let acknowledgement = match persistence.activate(staged.activation) {
                Ok(acknowledgement) => acknowledgement,
                Err(error) => {
                    let _ = persistence.shutdown();
                    return Err(PersistentRuntimeStartError::Activation(error));
                }
            };
            startup.restore_image = staged.candidate;
            startup.restore_image.epoch = acknowledgement.epoch;
            startup.initialized = false;
            candidate
        };

        Ok((
            Self {
                runtime,
                persistence,
                last_rebuild_derived_us,
            },
            startup,
        ))
    }

    pub fn runtime(&self) -> &LiveRuntime {
        &self.runtime
    }

    pub fn inspect_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, PersistentDispatchError> {
        self.runtime
            .inspect_value_current(name, max_rows)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn output_value_current(&mut self, name: &str) -> Result<Value, PersistentDispatchError> {
        self.runtime
            .output_value_current(name)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn demand_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> Result<Vec<DocumentPatch>, PersistentDispatchError> {
        self.runtime
            .demand_document_window_by_id(materialization, visible, overscan)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn dispatch(&mut self, event: SourceEvent) -> Result<RuntimeTurn, PersistentDispatchError> {
        let reservation = self
            .persistence
            .try_reserve_turn()
            .map_err(PersistentDispatchError::Backpressure)?;
        let mut turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        let persistence_started = Instant::now();
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_outbox_changes(turn.outbox_changes.clone());
        if !turn.outbox_changes.is_empty() {
            drop(reservation);
            if let Err(error) = self.persistence.commit_immediate(authority) {
                turn.phase_timings.persistence_enqueue_us =
                    duration_us(persistence_started.elapsed());
                let rollback_error = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| error.to_string());
                return Err(PersistentDispatchError::ImmediateCommitFailed {
                    turn: Box::new(turn),
                    error,
                    rollback_error,
                });
            }
            turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
            self.runtime.settle_turn();
            return Ok(turn);
        }
        if let Err(error) = reservation.enqueue(authority) {
            turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            return Err(PersistentDispatchError::PersistenceAdmissionFailed {
                turn: Box::new(turn),
                error,
                rollback_error,
            });
        }
        turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
        self.runtime.settle_turn();
        Ok(turn)
    }

    /// Executes a source turn and returns only after that exact authority turn
    /// is durable. Server/request hosts use this boundary before sending an
    /// acknowledgement that promises the mutation survived a process crash.
    /// Interactive UI hosts should use `dispatch`, whose ordinary turns remain
    /// buffered off the product thread.
    pub fn dispatch_durably(
        &mut self,
        event: SourceEvent,
    ) -> Result<DurablyAcknowledgedTurn, PersistentDispatchError> {
        let mut turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        let persistence_started = Instant::now();
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_outbox_changes(turn.outbox_changes.clone());
        let acknowledgement = match self.persistence.commit_immediate(authority) {
            Ok(acknowledgement) => acknowledgement,
            Err(error) => {
                turn.phase_timings.persistence_enqueue_us =
                    duration_us(persistence_started.elapsed());
                let rollback_error = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| error.to_string());
                return Err(PersistentDispatchError::ImmediateCommitFailed {
                    turn: Box::new(turn),
                    error,
                    rollback_error,
                });
            }
        };
        turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
        self.runtime.settle_turn();
        Ok(DurablyAcknowledgedTurn {
            turn,
            acknowledgement,
        })
    }

    pub fn status(&self) -> PersistenceWorkerStatus {
        self.persistence.status()
    }

    pub fn last_rebuild_derived_us(&self) -> u64 {
        self.last_rebuild_derived_us
    }

    pub fn barrier(&self) -> Result<BarrierAck, PersistenceControlError> {
        self.persistence.barrier()
    }

    pub fn flush(&self) -> Result<BarrierAck, PersistenceControlError> {
        self.persistence.barrier()
    }

    pub fn compact(&self) -> Result<CompactAck, PersistenceControlError> {
        self.persistence.compact()
    }

    pub fn inspect(&self) -> Result<Option<PersistenceInspectorSnapshot>, PersistenceControlError> {
        self.persistence.inspect()
    }

    pub fn export_state_artifact(&self) -> Result<Vec<u8>, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let image = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        encode_restore_image(&image)
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))
    }

    pub fn preview_state_artifact(
        &self,
        artifact: &[u8],
        options: SessionOptions,
    ) -> Result<PersistentStateArtifactPreview, PersistentActivationError> {
        let (_, preview, _, _) = self.prepare_state_artifact(artifact, options)?;
        Ok(preview)
    }

    pub fn activate_state_artifact(
        &mut self,
        artifact: &[u8],
        options: SessionOptions,
    ) -> Result<PersistentStateArtifactActivation, PersistentActivationError> {
        let (candidate, preview, completed_migration_edges, rebuild_derived_us) =
            self.prepare_state_artifact(artifact, options)?;
        self.last_rebuild_derived_us = rebuild_derived_us;
        let mount = candidate.mount();
        let acknowledgement =
            self.activate_settled_candidate(candidate, completed_migration_edges)?;
        Ok(PersistentStateArtifactActivation {
            mount,
            acknowledgement,
            preview,
        })
    }

    pub fn clear_authority_path(
        &mut self,
        semantic_path: &str,
        options: SessionOptions,
    ) -> Result<PersistentPlanActivation, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        reject_unfinished_outbox(&current, "clear authoritative state")?;
        let plan = self.runtime.shared_machine_plan();
        let mut candidate_image = current.clone();
        let mut changed = false;

        if let Some(memory) = plan.persistence.memory.iter().find(|memory| {
            memory.kind == MemoryKind::Scalar && memory.semantic_path == semantic_path
        }) {
            changed = candidate_image.scalars.remove(&memory.memory_id).is_some();
        } else if let Some(list) = plan
            .persistence
            .lists
            .iter()
            .find(|list| list.semantic_path == semantic_path)
        {
            changed = candidate_image.lists.remove(&list.memory_id).is_some();
        } else if let Some((list_memory_id, leaf_id)) =
            plan.persistence.lists.iter().find_map(|list| {
                list.row_fields
                    .iter()
                    .find(|field| field.semantic_path == semantic_path)
                    .map(|field| (list.memory_id, field.leaf_id))
            })
            && let Some(list) = candidate_image.lists.get_mut(&list_memory_id)
        {
            for row in &mut list.rows {
                changed |= row.fields.remove(&leaf_id).is_some();
                changed |= row.touched_fields.remove(&leaf_id);
            }
            if !list.touched {
                list.rows
                    .retain(|row| !row.fields.is_empty() || !row.touched_fields.is_empty());
                if list.rows.is_empty() {
                    candidate_image.lists.remove(&list_memory_id);
                }
            }
        } else {
            return Err(PersistentActivationError::Runtime(format!(
                "`{semantic_path}` is not an authoritative memory or row field"
            )));
        }
        if !changed {
            return Err(PersistentActivationError::Runtime(format!(
                "`{semantic_path}` has no stored override to clear"
            )));
        }

        let completed_migration_edges = candidate_image.completed_migration_edges.clone();
        let rebuild_started = Instant::now();
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            plan,
            options,
            Some(candidate_image),
        )
        .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        self.last_rebuild_derived_us = duration_us(rebuild_started.elapsed());
        let mount = candidate.mount();
        let acknowledgement =
            self.activate_settled_candidate(candidate, completed_migration_edges)?;
        Ok(PersistentPlanActivation {
            mount,
            acknowledgement: Some(acknowledgement),
            migration: None,
        })
    }

    fn prepare_state_artifact(
        &self,
        artifact: &[u8],
        options: SessionOptions,
    ) -> Result<
        (
            LiveRuntime,
            PersistentStateArtifactPreview,
            BTreeSet<boon_plan::MigrationEdgeId>,
            u64,
        ),
        PersistentActivationError,
    > {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        reject_unfinished_outbox(&current, "import state")?;
        let imported = decode_restore_image(artifact, DecodeLimits::default())
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        if imported.application != current.application {
            return Err(PersistentActivationError::Runtime(
                "state artifact belongs to a different application namespace".to_owned(),
            ));
        }
        reject_unfinished_outbox(&imported, "import state artifact")?;
        let source_schema_version = imported.schema_version;
        let plan = self.runtime.shared_machine_plan();
        let (mut candidate_image, migration) = if imported.schema_version
            == plan.persistence.schema_version
            && imported.schema_hash == plan.persistence.schema_hash
        {
            (imported, None)
        } else {
            let staged =
                stage_migration(&imported, &plan).map_err(PersistentActivationError::Migration)?;
            (staged.candidate, Some(staged.preview))
        };
        candidate_image.epoch = current.epoch;
        candidate_image.through_turn_sequence = current.through_turn_sequence;
        candidate_image.outbox = current.outbox.clone();
        let scalar_count = candidate_image.scalars.len();
        let list_count = candidate_image.lists.len();
        let row_count = candidate_image
            .lists
            .values()
            .map(|list| list.rows.len())
            .sum();
        let completed_migration_edges = candidate_image.completed_migration_edges.clone();
        let rebuild_started = Instant::now();
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            Arc::clone(&plan),
            options,
            Some(candidate_image),
        )
        .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let rebuild_derived_us = duration_us(rebuild_started.elapsed());
        let document_node_count = candidate
            .primary_retained_output_frame()
            .map_or(0, |frame| frame.nodes.len());
        let preview = PersistentStateArtifactPreview {
            source_schema_version,
            target_schema_version: plan.persistence.schema_version,
            scalar_count,
            list_count,
            row_count,
            migration,
            document_node_count,
        };
        Ok((
            candidate,
            preview,
            completed_migration_edges,
            rebuild_derived_us,
        ))
    }

    pub fn load_durable_image(
        &self,
    ) -> Result<Option<boon_persistence::RestoreImage>, PersistenceControlError> {
        self.persistence.load()
    }

    pub fn effect_work_items(&self) -> Result<Vec<EffectWorkItem>, PersistentEffectError> {
        let image = self
            .persistence
            .load()
            .map_err(PersistentEffectError::Persistence)?
            .ok_or_else(|| {
                PersistentEffectError::Runtime(
                    "persistence worker has no durable application state".to_owned(),
                )
            })?;
        Ok(image
            .outbox
            .into_values()
            .filter_map(|item| {
                let kind = match item.state {
                    DurableOutboxState::Pending => EffectWorkKind::Dispatch,
                    DurableOutboxState::Dispatching { .. }
                    | DurableOutboxState::ReconciliationRequired { .. } => {
                        EffectWorkKind::Reconcile
                    }
                    DurableOutboxState::Completed { .. } => return None,
                };
                Some(EffectWorkItem { kind, item })
            })
            .collect())
    }

    pub fn drive_effect_work_once(
        &mut self,
        driver: &mut impl HostEffectDriver,
    ) -> Result<Option<RuntimeTurn>, PersistentEffectDriveError> {
        let Some(work) = self
            .effect_work_items()
            .map_err(PersistentEffectDriveError::Runtime)?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        match work.kind {
            EffectWorkKind::Dispatch => {
                let claimed = self
                    .claim_effect_for_dispatch(work.item.item_id)
                    .map_err(PersistentEffectDriveError::Runtime)?;
                self.dispatch_claimed_effect(driver, claimed).map(Some)
            }
            EffectWorkKind::Reconcile => {
                let item = if matches!(work.item.state, DurableOutboxState::Dispatching { .. }) {
                    self.mark_effect_reconciliation(work.item.item_id)
                        .map_err(PersistentEffectDriveError::Runtime)?
                } else {
                    work.item
                };
                let request = HostEffectRequest::from(&item);
                match driver
                    .reconcile(&request)
                    .map_err(PersistentEffectDriveError::Host)?
                {
                    HostEffectReconciliation::Applied(outcome) => self
                        .complete_effect(item.item_id, outcome)
                        .map(Some)
                        .map_err(PersistentEffectDriveError::Runtime),
                    HostEffectReconciliation::NotApplied => {
                        let claimed = self
                            .claim_effect_for_dispatch(item.item_id)
                            .map_err(PersistentEffectDriveError::Runtime)?;
                        self.dispatch_claimed_effect(driver, claimed).map(Some)
                    }
                }
            }
        }
    }

    /// Advances the bounded background host-effect lane without performing
    /// host I/O on the Session owner thread. Durable claim/reconciliation and
    /// completion remain serialized through this runtime.
    pub fn poll_effect_worker(
        &mut self,
        worker: &mut HostEffectWorker,
    ) -> Result<Option<RuntimeTurn>, PersistentEffectDriveError> {
        if let Some(result) = worker.try_result().map_err(|error| {
            PersistentEffectDriveError::Runtime(PersistentEffectError::Runtime(error.to_string()))
        })? {
            return match result.outcome {
                HostEffectWorkerOutcome::Dispatched(Ok(outcome)) => self
                    .complete_effect(result.request.item_id, outcome)
                    .map(Some)
                    .map_err(PersistentEffectDriveError::Runtime),
                HostEffectWorkerOutcome::Dispatched(Err(host)) => {
                    match self.mark_effect_reconciliation(result.request.item_id) {
                        Ok(_) => Err(PersistentEffectDriveError::Host(host)),
                        Err(recovery) => {
                            Err(PersistentEffectDriveError::HostAndRecovery { host, recovery })
                        }
                    }
                }
                HostEffectWorkerOutcome::Reconciled(Ok(HostEffectReconciliation::Applied(
                    outcome,
                ))) => self
                    .complete_effect(result.request.item_id, outcome)
                    .map(Some)
                    .map_err(PersistentEffectDriveError::Runtime),
                HostEffectWorkerOutcome::Reconciled(Ok(HostEffectReconciliation::NotApplied)) => {
                    let claimed = self
                        .claim_effect_for_dispatch(result.request.item_id)
                        .map_err(PersistentEffectDriveError::Runtime)?;
                    submit_background_effect(
                        worker,
                        HostEffectWorkerOperation::Dispatch,
                        &claimed,
                    )?;
                    Ok(None)
                }
                HostEffectWorkerOutcome::Reconciled(Err(host)) => {
                    Err(PersistentEffectDriveError::Host(host))
                }
            };
        }
        if worker.is_busy() {
            return Ok(None);
        }
        let Some(work) = self
            .effect_work_items()
            .map_err(PersistentEffectDriveError::Runtime)?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        match work.kind {
            EffectWorkKind::Dispatch => {
                let claimed = self
                    .claim_effect_for_dispatch(work.item.item_id)
                    .map_err(PersistentEffectDriveError::Runtime)?;
                submit_background_effect(worker, HostEffectWorkerOperation::Dispatch, &claimed)?;
            }
            EffectWorkKind::Reconcile => {
                let item = if matches!(work.item.state, DurableOutboxState::Dispatching { .. }) {
                    self.mark_effect_reconciliation(work.item.item_id)
                        .map_err(PersistentEffectDriveError::Runtime)?
                } else {
                    work.item
                };
                submit_background_effect(worker, HostEffectWorkerOperation::Reconcile, &item)?;
            }
        }
        Ok(None)
    }

    fn dispatch_claimed_effect(
        &mut self,
        driver: &mut impl HostEffectDriver,
        item: DurableOutboxItem,
    ) -> Result<RuntimeTurn, PersistentEffectDriveError> {
        let request = HostEffectRequest::from(&item);
        match driver.dispatch(&request) {
            Ok(outcome) => self
                .complete_effect(item.item_id, outcome)
                .map_err(PersistentEffectDriveError::Runtime),
            Err(host) => match self.mark_effect_reconciliation(item.item_id) {
                Ok(_) => Err(PersistentEffectDriveError::Host(host)),
                Err(recovery) => {
                    Err(PersistentEffectDriveError::HostAndRecovery { host, recovery })
                }
            },
        }
    }

    pub fn claim_effect_for_dispatch(
        &mut self,
        item_id: OutboxItemId,
    ) -> Result<DurableOutboxItem, PersistentEffectError> {
        let item = self.load_effect_item(item_id)?;
        let turn = self
            .runtime
            .begin_effect_dispatch_unsettled(&item)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        self.commit_effect_turn(&turn)?;
        self.runtime.settle_turn();
        let mut claimed = item;
        let attempt = match claimed.state {
            DurableOutboxState::Pending => 1,
            DurableOutboxState::ReconciliationRequired { attempt } => {
                attempt.checked_add(1).ok_or_else(|| {
                    PersistentEffectError::Runtime("effect attempt overflow".to_owned())
                })?
            }
            _ => {
                return Err(PersistentEffectError::Runtime(
                    "effect item changed state while claiming dispatch".to_owned(),
                ));
            }
        };
        claimed.revision = claimed
            .revision
            .checked_add(1)
            .ok_or_else(|| PersistentEffectError::Runtime("outbox revision overflow".to_owned()))?;
        claimed.updated_turn_sequence = turn.sequence;
        claimed.state = DurableOutboxState::Dispatching { attempt };
        Ok(claimed)
    }

    pub fn mark_effect_reconciliation(
        &mut self,
        item_id: OutboxItemId,
    ) -> Result<DurableOutboxItem, PersistentEffectError> {
        let item = self.load_effect_item(item_id)?;
        let attempt = item.state.attempt();
        let turn = self
            .runtime
            .require_effect_reconciliation_unsettled(&item)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        self.commit_effect_turn(&turn)?;
        self.runtime.settle_turn();
        let mut next = item;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| PersistentEffectError::Runtime("outbox revision overflow".to_owned()))?;
        next.updated_turn_sequence = turn.sequence;
        next.state = DurableOutboxState::ReconciliationRequired { attempt };
        Ok(next)
    }

    pub fn complete_effect(
        &mut self,
        item_id: OutboxItemId,
        outcome: StoredValue,
    ) -> Result<RuntimeTurn, PersistentEffectError> {
        let item = self.load_effect_item(item_id)?;
        let turn = self
            .runtime
            .complete_effect_unsettled(&item, outcome)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        self.commit_effect_turn(&turn)?;
        self.runtime.settle_turn();
        Ok(turn)
    }

    fn load_effect_item(
        &self,
        item_id: OutboxItemId,
    ) -> Result<DurableOutboxItem, PersistentEffectError> {
        self.persistence
            .load()
            .map_err(PersistentEffectError::Persistence)?
            .and_then(|image| image.outbox.get(&item_id).cloned())
            .ok_or(PersistentEffectError::MissingItem(item_id))
    }

    fn commit_effect_turn(&mut self, turn: &RuntimeTurn) -> Result<(), PersistentEffectError> {
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_outbox_changes(turn.outbox_changes.clone());
        if let Err(error) = self.persistence.commit_immediate(authority) {
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            return Err(PersistentEffectError::CommitFailed {
                error,
                rollback_error,
            });
        }
        Ok(())
    }

    /// Commits a fully built and settled candidate before swapping runtime
    /// ownership. A failed backend activation leaves the old runtime active.
    pub fn activate_settled_candidate(
        &mut self,
        candidate: LiveRuntime,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
    ) -> Result<ActivationAck, PersistentActivationError> {
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        let mut candidate_image = candidate
            .durable_restore_image(current.epoch, completed_migration_edges)
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        candidate_image.outbox = current.outbox.clone();
        let batch = ActivationBatch::between(&current, &candidate_image)
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let acknowledgement = self
            .persistence
            .activate(batch)
            .map_err(PersistentActivationError::Persistence)?;
        self.runtime = candidate;
        Ok(acknowledgement)
    }

    /// Builds and settles a replacement plan against the acknowledged image,
    /// commits any required migration, then swaps the only live runtime.
    pub fn activate_machine_plan(
        &mut self,
        plan: Arc<MachinePlan>,
        options: SessionOptions,
    ) -> Result<PersistentPlanActivation, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;

        let (restore, activation, migration) = if current.schema_version
            == plan.persistence.schema_version
            && current.schema_hash == plan.persistence.schema_hash
        {
            (current, None, None)
        } else {
            let staged =
                stage_migration(&current, &plan).map_err(PersistentActivationError::Migration)?;
            (
                staged.candidate,
                Some(staged.activation),
                Some(staged.preview),
            )
        };
        let rebuild_started = Instant::now();
        let candidate =
            LiveRuntime::from_shared_machine_plan_with_restore(plan, options, Some(restore))
                .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let rebuild_derived_us = duration_us(rebuild_started.elapsed());
        let mount = candidate.mount();
        let acknowledgement = activation
            .map(|batch| self.persistence.activate(batch))
            .transpose()
            .map_err(PersistentActivationError::Persistence)?;
        self.runtime = candidate;
        self.last_rebuild_derived_us = rebuild_derived_us;
        Ok(PersistentPlanActivation {
            mount,
            acknowledgement,
            migration,
        })
    }

    /// Builds and settles a candidate against acknowledged durable authority
    /// without committing storage or replacing the active runtime.
    pub fn preview_machine_plan(
        &self,
        plan: Arc<MachinePlan>,
        options: SessionOptions,
    ) -> Result<PersistentPlanPreview, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        let (restore, migration) = if current.schema_version == plan.persistence.schema_version
            && current.schema_hash == plan.persistence.schema_hash
        {
            (current, None)
        } else {
            let staged =
                stage_migration(&current, &plan).map_err(PersistentActivationError::Migration)?;
            (staged.candidate, Some(staged.preview))
        };
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            Arc::clone(&plan),
            options,
            Some(restore),
        )
        .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        Ok(PersistentPlanPreview {
            migration,
            target_schema_version: plan.persistence.schema_version,
            target_schema_hash: plan.persistence.schema_hash,
            document_node_count: candidate
                .primary_retained_output_frame()
                .map_or(0, |frame| frame.nodes.len()),
        })
    }

    /// Rebuilds the target plan from its current defaults, commits one backend
    /// reset transaction, and only then replaces the active runtime. This is
    /// the product Start Over boundary; it preserves the application namespace
    /// and monotonic durable epoch/turn sequence while clearing all authority,
    /// migration history, blobs, and outbox state represented by the default
    /// image.
    pub fn start_over_machine_plan(
        &mut self,
        plan: Arc<MachinePlan>,
        options: SessionOptions,
    ) -> Result<PersistentPlanReset, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        let defaults = LiveRuntime::from_shared_machine_plan(Arc::clone(&plan), options.clone())
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let default_image = defaults
            .durable_restore_image(0, BTreeSet::new())
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let next_epoch = current.epoch.checked_add(1).ok_or_else(|| {
            PersistentActivationError::Runtime(
                "durable epoch overflow while starting over".to_owned(),
            )
        })?;
        let mut candidate_image = default_image.clone();
        candidate_image.epoch = next_epoch;
        candidate_image.through_turn_sequence = current.through_turn_sequence;
        let rebuild_started = Instant::now();
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            plan,
            options,
            Some(candidate_image),
        )
        .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let rebuild_derived_us = duration_us(rebuild_started.elapsed());
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
        let acknowledgement = self
            .persistence
            .reset_application(batch)
            .map_err(PersistentActivationError::Persistence)?;
        self.runtime = candidate;
        self.last_rebuild_derived_us = rebuild_derived_us;
        Ok(PersistentPlanReset {
            mount,
            acknowledgement,
        })
    }

    pub fn shutdown(&self) -> Result<boon_persistence::ShutdownAck, PersistenceControlError> {
        self.persistence.shutdown()
    }
}

fn duration_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn submit_background_effect(
    worker: &mut HostEffectWorker,
    operation: HostEffectWorkerOperation,
    item: &DurableOutboxItem,
) -> Result<(), PersistentEffectDriveError> {
    worker
        .try_submit(operation, HostEffectRequest::from(item))
        .map_err(|error| {
            PersistentEffectDriveError::Runtime(PersistentEffectError::Runtime(error.to_string()))
        })
}

fn reject_unfinished_outbox(
    image: &boon_persistence::RestoreImage,
    operation: &str,
) -> Result<(), PersistentActivationError> {
    if image
        .outbox
        .values()
        .any(|item| !matches!(item.state, DurableOutboxState::Completed { .. }))
    {
        return Err(PersistentActivationError::Runtime(format!(
            "cannot {operation} while consequential effects are unfinished"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileEffectDriver;
    use boon_persistence::{
        InMemoryDriver, PersistenceCommand, PersistenceResult, ShutdownAck, StoreError,
    };
    use boon_plan_executor::SourcePayload;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FailingActivationDriver {
        inner: InMemoryDriver,
        fail_activation: Arc<AtomicBool>,
    }

    impl PersistenceDriver for FailingActivationDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(command, PersistenceCommand::Activate(_))
                && self.fail_activation.swap(false, Ordering::AcqRel)
            {
                return PersistenceResult::Activated(Err(StoreError::Backend(
                    "injected activation failure".to_owned(),
                )));
            }
            self.inner.execute(command)
        }
    }

    #[derive(Clone, Default)]
    struct SharedPersistenceDriver {
        inner: Arc<Mutex<InMemoryDriver>>,
        fail_next_commit: Arc<AtomicBool>,
    }

    impl PersistenceDriver for SharedPersistenceDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(command, PersistenceCommand::Shutdown(_)) {
                return PersistenceResult::ShutdownComplete(Ok(ShutdownAck));
            }
            if matches!(command, PersistenceCommand::Commit(_))
                && self.fail_next_commit.swap(false, Ordering::AcqRel)
            {
                return PersistenceResult::Committed(Err(StoreError::Backend(
                    "injected checkpoint failure".to_owned(),
                )));
            }
            self.inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .execute(command)
        }
    }

    #[derive(Default)]
    struct RecordingEffectDriver {
        dispatch_count: usize,
        reconcile_count: usize,
        applied: BTreeMap<OutboxItemId, StoredValue>,
        fail_next_persistence_commit_after_dispatch: Option<Arc<AtomicBool>>,
    }

    impl HostEffectDriver for RecordingEffectDriver {
        fn dispatch(
            &mut self,
            request: &HostEffectRequest,
        ) -> Result<StoredValue, HostEffectError> {
            self.dispatch_count += 1;
            let StoredValue::Record(intent) = &request.intent else {
                return Err(HostEffectError::rejected("effect intent is not a record"));
            };
            let Some(StoredValue::Text(path)) = intent.get("path") else {
                return Err(HostEffectError::rejected("effect path is not Text"));
            };
            let outcome = StoredValue::Text(path.clone());
            self.applied.insert(request.item_id, outcome.clone());
            if let Some(fail) = &self.fail_next_persistence_commit_after_dispatch {
                fail.store(true, Ordering::Release);
            }
            Ok(outcome)
        }

        fn reconcile(
            &mut self,
            request: &HostEffectRequest,
        ) -> Result<HostEffectReconciliation, HostEffectError> {
            self.reconcile_count += 1;
            Ok(self
                .applied
                .get(&request.item_id)
                .cloned()
                .map(HostEffectReconciliation::Applied)
                .unwrap_or(HostEffectReconciliation::NotApplied))
        }
    }

    fn effect_plan(identity: boon_plan::ApplicationIdentity) -> Arc<MachinePlan> {
        Arc::new(
            boon_compiler::compile_runtime_source_text_to_machine_plan_with_persistence_identity(
                "bytes-file-write-effect.bn",
                include_str!("../../../examples/bytes_file_write_effect.bn"),
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                1,
            )
            .unwrap()
            .plan,
        )
    }

    fn effect_source_event(runtime: &PersistentRuntime, sequence: u64) -> SourceEvent {
        runtime
            .runtime()
            .source_event(sequence, "store.save.press", None, SourcePayload::default())
            .unwrap()
    }

    #[test]
    fn persistent_runtime_reserves_then_checkpoints_stable_authority() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-counter.bn",
            source,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-runtime", "test", "local"),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig {
                coalesce_delay: std::time::Duration::ZERO,
                ..PersistenceWorkerConfig::default()
            },
        )
        .unwrap();
        assert!(startup.initialized);

        let event = runtime
            .runtime()
            .source_event(
                1,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        let turn = runtime.dispatch(event).unwrap();
        assert_eq!(turn.sequence, 1);
        assert!(!turn.durable_changes.is_empty());
        let barrier = runtime.barrier().unwrap();
        assert!(barrier.epoch >= 1);
        let status = runtime.status();
        assert_eq!(status.durable_through_turn_sequence, 1);
        assert!(status.pending.is_none());
        let inspection = runtime.inspect().unwrap().unwrap();
        assert_eq!(inspection.through_turn_sequence, 1);
        assert_eq!(inspection.scalar_count, 1);
        runtime.shutdown().unwrap();
    }

    #[test]
    fn durable_dispatch_acknowledges_the_exact_server_mutation() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-server-ack.bn",
            source,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-server-ack", "test", "local"),
        )
        .unwrap();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            compiled.shared_machine_plan(),
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let event = runtime
            .runtime()
            .source_event(
                1,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();

        let acknowledged = runtime.dispatch_durably(event).unwrap();

        assert_eq!(acknowledged.turn.source_sequence, Some(1));
        assert_eq!(
            acknowledged.acknowledgement.through_turn_sequence,
            acknowledged.turn.sequence
        );
        assert_eq!(
            runtime.status().durable_through_turn_sequence,
            acknowledged.turn.sequence
        );
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            Value::Number(1)
        );
        runtime.shutdown().unwrap();
    }

    #[test]
    fn cells_restart_restores_one_sparse_edit_without_materializing_the_grid() {
        let identity =
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-cells", "test", "local");
        let units = boon_compiler::compiler_source_units_for_path(std::path::Path::new(
            "../../examples/cells.bn",
        ))
        .unwrap();
        let plan = Arc::new(
            boon_compiler::compile_runtime_source_units_to_machine_plan_with_persistence_identity(
                "persistent-cells.bn",
                &units,
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                1,
            )
            .unwrap()
            .plan,
        );
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let event = |runtime: &PersistentRuntime, sequence, path: &str, text: Option<&str>| {
            runtime
                .runtime()
                .source_event(
                    sequence,
                    path,
                    None,
                    SourcePayload {
                        address: Some("A3".to_owned()),
                        text: text.map(str::to_owned),
                        ..SourcePayload::default()
                    },
                )
                .unwrap()
        };
        runtime
            .dispatch(event(&runtime, 1, "cell.sources.editor.select", None))
            .unwrap();
        runtime
            .dispatch(event(&runtime, 2, "cell.sources.editor.change", Some("20")))
            .unwrap();
        runtime
            .dispatch(event(&runtime, 3, "cell.sources.editor.commit", Some("20")))
            .unwrap();
        runtime.barrier().unwrap();
        let durable = runtime.load_durable_image().unwrap().unwrap();
        let stored_rows = durable
            .lists
            .values()
            .map(|list| {
                assert!(!list.touched, "Cells edit stored full list structure");
                list.rows.len()
            })
            .sum::<usize>();
        assert_eq!(stored_rows, 1, "{durable:#?}");
        runtime.shutdown().unwrap();

        let (mut restored, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(!startup.initialized);
        assert!(
            restored
                .runtime
                .document_materialization_stats()
                .logical_rows
                >= 2_600
        );
        assert!(
            restored
                .runtime
                .document_frame()
                .unwrap()
                .nodes
                .values()
                .any(|node| {
                    node.kind == boon_document_model::DocumentNodeKind::TextInput
                        && node.text.as_ref().is_some_and(|text| text.text == "20")
                })
        );
        let Value::List(sample) = restored.inspect_value_current("cell.value", 1).unwrap() else {
            panic!("cell.value inspection must remain demand-current");
        };
        assert_eq!(sample.len(), 1);
        restored.shutdown().unwrap();
    }

    #[test]
    fn novywave_restart_restores_authority_and_rebuilds_signal_views() {
        let identity =
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-novywave", "test", "local");
        let units = boon_compiler::compiler_source_units_for_path(std::path::Path::new(
            "../../examples/novywave/RUN.bn",
        ))
        .unwrap();
        let plan = Arc::new(
            boon_compiler::compile_runtime_source_units_to_machine_plan_with_persistence_identity(
                "persistent-novywave.bn",
                &units,
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                1,
            )
            .unwrap()
            .plan,
        );
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let event = runtime
            .runtime()
            .source_event(
                1,
                "store.elements.panels_toggle_arrangement",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        runtime.dispatch(event).unwrap();
        runtime.barrier().unwrap();
        assert_eq!(
            runtime
                .runtime
                .root_value_current("store.panel_arrangement")
                .unwrap(),
            Value::Text("Docked".to_owned())
        );
        let durable = runtime.load_durable_image().unwrap().unwrap();
        assert_eq!(durable.scalars.len(), 1, "{durable:#?}");
        assert!(durable.lists.is_empty(), "{durable:#?}");
        runtime.shutdown().unwrap();

        let (mut restored, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(!startup.initialized);
        assert_eq!(
            restored
                .runtime
                .root_value_current("store.panel_arrangement")
                .unwrap(),
            Value::Text("Docked".to_owned())
        );
        let Value::List(formatters) = restored
            .inspect_value_current("selected_signal.formatter", 32)
            .unwrap()
        else {
            panic!("selected signal formatters must rebuild after restore");
        };
        assert_eq!(formatters.len(), 14);
        restored.shutdown().unwrap();
    }

    #[test]
    fn todomvc_restart_preserves_dynamic_row_order_and_sparse_row_authority() {
        let identity =
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-todomvc", "test", "local");
        let plan = Arc::new(
            boon_compiler::compile_runtime_source_text_to_machine_plan_with_persistence_identity(
                "persistent-todomvc.bn",
                include_str!("../../../examples/todomvc.bn"),
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                1,
            )
            .unwrap()
            .plan,
        );
        let scenario =
            crate::parse_scenario(std::path::Path::new("../../examples/todomvc.scn")).unwrap();
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        for (sequence, source) in (1_u64..).zip(
            scenario
                .steps
                .iter()
                .filter_map(|step| step.source_event.as_ref())
                .take(2),
        ) {
            let target = runtime.runtime().scenario_target(source).unwrap();
            let event = runtime
                .runtime()
                .source_event(sequence, &source.source, target, source.payload.clone())
                .unwrap();
            runtime.dispatch(event).unwrap();
        }
        runtime.barrier().unwrap();
        let durable = runtime.load_durable_image().unwrap().unwrap();
        let list = durable
            .lists
            .values()
            .find(|list| list.touched)
            .expect("dynamic TodoMVC list structure");
        assert_eq!(list.rows.len(), 5);
        assert!(list.next_key > list.rows.iter().map(|row| row.key).max().unwrap());
        runtime.shutdown().unwrap();

        let (mut restored, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(!startup.initialized);
        let Value::List(rows) = restored.inspect_value_current("todo.title", 8).unwrap() else {
            panic!("todo.title inspection must return row values");
        };
        let titles = rows
            .iter()
            .filter_map(|row| match row {
                Value::Record(fields) => fields.get("value"),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(titles.last(), Some(&&Value::Text("Test todo".to_owned())));
        restored.shutdown().unwrap();
    }

    #[test]
    fn connected_app_restart_keeps_descriptors_but_not_live_sources() {
        let identity =
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-fjordpulse", "test", "local");
        let plan = Arc::new(
            boon_compiler::compile_runtime_source_text_to_machine_plan_with_persistence_identity(
                "persistence-fjordpulse-fixture.bn",
                include_str!("../../../examples/persistence_fjordpulse_fixture.bn"),
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                1,
            )
            .unwrap()
            .plan,
        );
        assert!(
            plan.persistence
                .memory
                .iter()
                .all(|memory| !memory.semantic_path.contains("sources"))
        );
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let mut dispatch = |sequence, source: &str, text: Option<&str>| {
            let event = runtime
                .runtime()
                .source_event(
                    sequence,
                    source,
                    None,
                    SourcePayload {
                        text: text.map(str::to_owned),
                        ..SourcePayload::default()
                    },
                )
                .unwrap();
            runtime.dispatch(event).unwrap();
        };
        dispatch(1, "store.sources.station_input.change", Some("station-42"));
        dispatch(
            2,
            "store.sources.snapshot.receive",
            Some("temperature=12.4"),
        );
        dispatch(3, "store.sources.connected.receive", None);
        dispatch(4, "store.sources.locale_button.press", None);
        dispatch(5, "store.sources.basemap_button.press", None);
        runtime.barrier().unwrap();
        runtime.shutdown().unwrap();

        let (mut restored, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(!startup.initialized);
        for (path, expected) in [
            ("store.locale", Value::Text("nb-NO".to_owned())),
            ("store.basemap", Value::Text("Satellite".to_owned())),
            (
                "store.selected_station",
                Value::Text("station-42".to_owned()),
            ),
            (
                "store.watch_descriptor",
                Value::Text("station-42".to_owned()),
            ),
            (
                "store.last_known_snapshot",
                Value::Text("temperature=12.4".to_owned()),
            ),
            ("store.connection_status", Value::Text("Online".to_owned())),
            ("store.snapshot_stale", Value::Bool(false)),
        ] {
            assert_eq!(restored.runtime.root_value_current(path).unwrap(), expected);
        }
        restored.shutdown().unwrap();
    }

    #[test]
    fn effect_remote_success_is_reconciled_after_restart_without_redispatch() {
        let identity = boon_plan::ApplicationIdentity::new(
            "dev.boon.persistent-effect-restart",
            "test",
            "local",
        );
        let plan = effect_plan(identity);
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(startup.initialized);
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("not written".to_owned())
        );

        let turn = runtime.dispatch(effect_source_event(&runtime, 1)).unwrap();
        assert_eq!(turn.source_sequence, Some(1));
        assert_eq!(turn.outbox_changes.len(), 1);
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("not written".to_owned())
        );
        let pending = runtime.effect_work_items().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, EffectWorkKind::Dispatch);
        assert!(matches!(
            runtime.clear_authority_path("store.result", SessionOptions::default()),
            Err(PersistentActivationError::Runtime(detail))
                if detail.contains("effects are unfinished")
        ));

        let claimed = runtime
            .claim_effect_for_dispatch(pending[0].item.item_id)
            .unwrap();
        assert!(matches!(
            claimed.state,
            DurableOutboxState::Dispatching { attempt: 1 }
        ));
        let mut host = RecordingEffectDriver::default();
        host.dispatch(&HostEffectRequest::from(&claimed)).unwrap();
        runtime.shutdown().unwrap();

        let (mut restarted, startup) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert!(!startup.initialized);
        assert_eq!(
            restarted
                .runtime
                .root_value_current("store.result")
                .unwrap(),
            Value::Text("not written".to_owned())
        );
        assert_eq!(
            restarted.effect_work_items().unwrap()[0].kind,
            EffectWorkKind::Reconcile
        );

        let completion = restarted
            .drive_effect_work_once(&mut host)
            .unwrap()
            .unwrap();
        assert_eq!(completion.source_sequence, None);
        assert_eq!(host.dispatch_count, 1);
        assert_eq!(host.reconcile_count, 1);
        assert_eq!(
            restarted
                .runtime
                .root_value_current("store.result")
                .unwrap(),
            Value::Text("output.bin".to_owned())
        );
        assert!(restarted.effect_work_items().unwrap().is_empty());
        let durable = restarted.load_durable_image().unwrap().unwrap();
        assert!(matches!(
            durable.outbox.values().next().map(|item| &item.state),
            Some(DurableOutboxState::Completed { attempt: 1, .. })
        ));
        restarted.shutdown().unwrap();

        let (mut restored, _) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert_eq!(
            restored.runtime.root_value_current("store.result").unwrap(),
            Value::Text("output.bin".to_owned())
        );
        assert!(restored.effect_work_items().unwrap().is_empty());
        restored.shutdown().unwrap();
    }

    #[test]
    fn failed_effect_outcome_commit_rolls_back_visibility_then_reconciles() {
        let identity = boon_plan::ApplicationIdentity::new(
            "dev.boon.persistent-effect-failure",
            "test",
            "local",
        );
        let plan = effect_plan(identity);
        let storage = SharedPersistenceDriver::default();
        let fail_commit = Arc::clone(&storage.fail_next_commit);
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        runtime.dispatch(effect_source_event(&runtime, 1)).unwrap();
        let mut host = RecordingEffectDriver {
            fail_next_persistence_commit_after_dispatch: Some(fail_commit),
            ..RecordingEffectDriver::default()
        };

        assert!(matches!(
            runtime.drive_effect_work_once(&mut host),
            Err(PersistentEffectDriveError::Runtime(
                PersistentEffectError::CommitFailed { .. }
            ))
        ));
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("not written".to_owned())
        );
        assert_eq!(
            runtime.effect_work_items().unwrap()[0].kind,
            EffectWorkKind::Reconcile
        );

        host.fail_next_persistence_commit_after_dispatch = None;
        runtime.drive_effect_work_once(&mut host).unwrap().unwrap();
        assert_eq!(host.dispatch_count, 1);
        assert_eq!(host.reconcile_count, 1);
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("output.bin".to_owned())
        );
        runtime.shutdown().unwrap();
    }

    #[test]
    fn background_effect_worker_keeps_host_io_outside_the_runtime_owner() {
        let identity = boon_plan::ApplicationIdentity::new(
            "dev.boon.persistent-effect-worker",
            "test",
            "local",
        );
        let root = std::env::temp_dir().join(format!(
            "boon-persistent-effect-worker-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let plan = effect_plan(identity);
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        runtime.dispatch(effect_source_event(&runtime, 1)).unwrap();
        let mut worker = HostEffectWorker::start(FileEffectDriver::new(&root).unwrap()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);

        let completion = loop {
            if let Some(turn) = runtime.poll_effect_worker(&mut worker).unwrap() {
                break turn;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background effect did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };

        assert_eq!(completion.source_sequence, None);
        assert_eq!(
            std::fs::read(root.join("output.bin")).unwrap(),
            [1, 2, 3, 4]
        );
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("output.bin".to_owned())
        );
        worker.shutdown().unwrap();
        runtime.shutdown().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_backend_activation_keeps_the_old_runtime_and_store() {
        let source = include_str!("../../../examples/counter.bn");
        let identity =
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-activation", "test", "local");
        let v1 =
            boon_compiler::compile_runtime_source_text_to_machine_plan_with_persistence_identity(
                "persistent-counter-v1.bn",
                source,
                boon_plan::TargetProfile::SoftwareDefault,
                identity.clone(),
                1,
            )
            .unwrap()
            .plan;
        let fail_activation = Arc::new(AtomicBool::new(false));
        let driver = FailingActivationDriver {
            inner: InMemoryDriver::default(),
            fail_activation: Arc::clone(&fail_activation),
        };
        let (mut runtime, _) = PersistentRuntime::from_machine_plan(
            v1,
            SessionOptions::default(),
            driver,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let event = runtime
            .runtime()
            .source_event(
                1,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        runtime.dispatch(event).unwrap();
        runtime.barrier().unwrap();
        let current = runtime.load_durable_image().unwrap().unwrap();

        let v2 =
            boon_compiler::compile_runtime_source_text_to_machine_plan_with_persistence_identity(
                "persistent-counter-v2.bn",
                source,
                boon_plan::TargetProfile::SoftwareDefault,
                identity,
                2,
            )
            .unwrap()
            .plan;
        let mut candidate_image = current.clone();
        candidate_image.schema_version = v2.persistence.schema_version;
        candidate_image.schema_hash = v2.persistence.schema_hash;
        let candidate = LiveRuntime::from_machine_plan_with_restore(
            v2,
            SessionOptions::default(),
            Some(candidate_image),
        )
        .unwrap();
        fail_activation.store(true, Ordering::Release);

        assert!(matches!(
            runtime.activate_settled_candidate(candidate, BTreeSet::new()),
            Err(PersistentActivationError::Persistence(_))
        ));
        assert_eq!(
            runtime
                .load_durable_image()
                .unwrap()
                .unwrap()
                .schema_version,
            1
        );
        assert!(
            runtime
                .runtime()
                .snapshot()
                .unwrap()
                .states
                .values()
                .any(|value| value == &boon_plan_executor::Value::Number(1))
        );
        runtime.shutdown().unwrap();
    }

    #[test]
    fn plan_preview_settles_without_mutating_active_or_durable_state() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-preview.bn",
            source,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-preview", "test", "local"),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        let (runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let before = runtime.load_durable_image().unwrap().unwrap();
        let active_frame = runtime.runtime().document_frame().cloned();

        let preview = runtime
            .preview_machine_plan(plan, SessionOptions::default())
            .unwrap();

        assert!(preview.migration.is_none());
        assert_eq!(preview.target_schema_version, before.schema_version);
        assert_eq!(runtime.load_durable_image().unwrap(), Some(before));
        assert_eq!(runtime.runtime().document_frame(), active_frame.as_ref());
        runtime.shutdown().unwrap();
    }

    #[test]
    fn start_over_uses_the_same_store_and_preserves_monotonic_progress() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-start-over.bn",
            source,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-start-over", "test", "local"),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig {
                coalesce_delay: std::time::Duration::ZERO,
                ..PersistenceWorkerConfig::default()
            },
        )
        .unwrap();
        let increment = runtime
            .runtime()
            .source_event(
                1,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        runtime.dispatch(increment).unwrap();
        runtime.barrier().unwrap();
        let before = runtime.load_durable_image().unwrap().unwrap();
        assert_eq!(before.through_turn_sequence, 1);
        assert!(!before.scalars.is_empty());

        let reset = runtime
            .start_over_machine_plan(Arc::clone(&plan), SessionOptions::default())
            .unwrap();
        assert_eq!(reset.acknowledgement.epoch, before.epoch + 1);
        assert_eq!(reset.acknowledgement.through_turn_sequence, 1);
        let after = runtime.load_durable_image().unwrap().unwrap();
        assert_eq!(after.epoch, reset.acknowledgement.epoch);
        assert_eq!(after.through_turn_sequence, 1);
        assert!(after.scalars.is_empty());
        assert!(after.lists.is_empty());
        assert!(after.outbox.is_empty());
        assert!(after.completed_migration_edges.is_empty());
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            boon_plan_executor::Value::Number(0)
        );

        let increment = runtime
            .runtime()
            .source_event(
                2,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        runtime.dispatch(increment).unwrap();
        runtime.barrier().unwrap();
        assert_eq!(
            runtime
                .load_durable_image()
                .unwrap()
                .unwrap()
                .through_turn_sequence,
            2
        );
        runtime.shutdown().unwrap();
    }

    #[test]
    fn state_artifact_preview_activation_and_selected_clear_are_atomic() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-state-artifact.bn",
            source,
            boon_plan::ApplicationIdentity::new(
                "dev.boon.persistent-state-artifact",
                "test",
                "local",
            ),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let increment = |runtime: &PersistentRuntime, sequence| {
            runtime
                .runtime()
                .source_event(
                    sequence,
                    "store.sources.increment_button.press",
                    None,
                    SourcePayload::default(),
                )
                .unwrap()
        };
        runtime.dispatch(increment(&runtime, 1)).unwrap();
        runtime.barrier().unwrap();
        let artifact = runtime.export_state_artifact().unwrap();
        runtime.dispatch(increment(&runtime, 2)).unwrap();
        runtime.barrier().unwrap();
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            Value::Number(2)
        );

        let preview = runtime
            .preview_state_artifact(&artifact, SessionOptions::default())
            .unwrap();
        assert_eq!(preview.scalar_count, 1);
        assert!(preview.migration.is_none());
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            Value::Number(2),
            "preview must not mutate active authority"
        );

        let activation = runtime
            .activate_state_artifact(&artifact, SessionOptions::default())
            .unwrap();
        assert!(activation.acknowledgement.epoch > 0);
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            Value::Number(1)
        );
        runtime
            .clear_authority_path("store.count", SessionOptions::default())
            .unwrap();
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            Value::Number(0)
        );
        assert!(
            runtime
                .load_durable_image()
                .unwrap()
                .unwrap()
                .scalars
                .is_empty()
        );
        runtime.shutdown().unwrap();
    }
}
