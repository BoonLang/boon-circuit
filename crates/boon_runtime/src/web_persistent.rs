use super::{DocumentPatch, LiveRuntime, RuntimeTurn};
use boon_persistence::{
    ActivationAck, BarrierAck, BarrierRequest, BrowserStorageStatus, CheckpointBatch, CommitAck,
    CompactAck, CompactRequest, InspectRequest, MigrationError, MigrationPreview,
    PersistenceCommand, PersistenceInspectorSnapshot, PersistenceResult, ResetApplicationAck,
    ResetApplicationBatch, RestoreImage, RestoreRequest, RexieDriver, ShutdownAck, ShutdownRequest,
    StoreError, stage_migration,
};
use boon_plan::MachinePlan;
use boon_plan_executor::{SessionOptions, SourceEvent, SourcePayload, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug)]
pub enum WebPersistenceError {
    PendingTurn { sequence: u64 },
    NoPreparedTurn,
    Runtime(String),
    Store(StoreError),
    Protocol(String),
    Migration(MigrationError),
    MissingDurableState,
}

impl fmt::Display for WebPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PendingTurn { sequence } => {
                write!(
                    formatter,
                    "authority turn {sequence} is awaiting persistence"
                )
            }
            Self::NoPreparedTurn => formatter.write_str("no authority turn is prepared"),
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
    Preparation(WebPersistenceError),
    AdoptionFailed {
        turn: Box<RuntimeTurn>,
        error: WebPersistenceError,
        rollback_error: Option<String>,
    },
}

impl fmt::Display for WebPersistentDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation(error) => write!(formatter, "{error}"),
            Self::AdoptionFailed {
                error,
                rollback_error,
                ..
            } => match rollback_error {
                Some(rollback) => write!(
                    formatter,
                    "authority adoption failed with `{error}` and runtime rollback failed with `{rollback}`"
                ),
                None => write!(
                    formatter,
                    "authority adoption failed and the prepared runtime turn was rolled back: {error}"
                ),
            },
        }
    }
}

impl std::error::Error for WebPersistentDispatchError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebPreparedTurn {
    pub sequence: u64,
    pub source_sequence: Option<u64>,
    pub durable_change_count: usize,
    pub outbox_change_count: usize,
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
}

pub struct WebPlanActivation {
    pub mount: RuntimeTurn,
    pub acknowledgement: Option<ActivationAck>,
    pub migration: Option<MigrationPreview>,
}

pub struct WebPlanReset {
    pub mount: RuntimeTurn,
    pub acknowledgement: ResetApplicationAck,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebPersistentRuntimeStatus {
    pub application: boon_plan::ApplicationIdentity,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub durable_epoch: u64,
    pub durable_through_turn_sequence: u64,
    pub prepared_turn_sequence: Option<u64>,
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

/// Browser runtime whose IndexedDB boundary is an explicit asynchronous adoption step.
///
/// `prepare_dispatch` performs only in-memory runtime work and retains one unsettled turn.
/// Hosts must schedule `adopt_prepared_turn` outside synchronous input/render callbacks and may
/// publish the returned `RuntimeTurn` only after adoption succeeds. A rejected command or
/// transaction rolls the unsettled runtime authority back before returning the complete turn.
pub struct WebPersistentRuntime {
    runtime: LiveRuntime,
    persistence: RexieDriver,
    plan: Arc<MachinePlan>,
    options: SessionOptions,
    durable: DurableCursor,
    pending_turn: Option<RuntimeTurn>,
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
        let default_runtime =
            LiveRuntime::from_shared_machine_plan(Arc::clone(&plan), options.clone())
                .map_err(runtime_error)?;
        let initial_image = default_runtime
            .durable_restore_image(0, BTreeSet::new())
            .map_err(runtime_error)?;
        let loaded = load_image(&mut persistence, &initial_image.application).await?;

        let (runtime, restore_image, initialized) = match loaded {
            None => {
                let acknowledgement =
                    initialize_image(&mut persistence, initial_image.clone()).await?;
                ensure_commit_ack(
                    &acknowledgement,
                    initial_image.epoch,
                    initial_image.through_turn_sequence,
                    "Initialize",
                )?;
                (default_runtime, initial_image, true)
            }
            Some(stored) => {
                if stored.application != initial_image.application {
                    return Err(WebPersistenceError::Store(StoreError::IdentityMismatch));
                }
                if stored.schema_version == plan.persistence.schema_version
                    && stored.schema_hash == plan.persistence.schema_hash
                {
                    let runtime = LiveRuntime::from_shared_machine_plan_with_restore(
                        Arc::clone(&plan),
                        options.clone(),
                        Some(stored.clone()),
                    )
                    .map_err(runtime_error)?;
                    (runtime, stored, false)
                } else {
                    let staged =
                        stage_migration(&stored, &plan).map_err(WebPersistenceError::Migration)?;
                    let runtime = LiveRuntime::from_shared_machine_plan_with_restore(
                        Arc::clone(&plan),
                        options.clone(),
                        Some(staged.candidate.clone()),
                    )
                    .map_err(runtime_error)?;
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
                    (runtime, restored, false)
                }
            }
        };
        let mount = runtime.mount();
        let durable = DurableCursor::from_image(&restore_image);
        Ok((
            Self {
                runtime,
                persistence,
                plan,
                options,
                durable,
                pending_turn: None,
            },
            WebPersistenceStartup {
                restore_image,
                initialized,
                mount,
            },
        ))
    }

    pub fn runtime(&self) -> Option<&LiveRuntime> {
        self.pending_turn.is_none().then_some(&self.runtime)
    }

    pub fn status(&self) -> WebPersistentRuntimeStatus {
        WebPersistentRuntimeStatus {
            application: self.durable.application.clone(),
            schema_version: self.durable.schema_version,
            schema_hash: self.durable.schema_hash,
            durable_epoch: self.durable.epoch,
            durable_through_turn_sequence: self.durable.through_turn_sequence,
            prepared_turn_sequence: self.pending_turn.as_ref().map(|turn| turn.sequence),
            command_queue_capacity: self.persistence.command_queue_capacity(),
            storage: self.persistence.storage_status().clone(),
        }
    }

    pub async fn refresh_storage_status(&mut self) -> &BrowserStorageStatus {
        self.persistence.refresh_storage_status().await
    }

    pub fn source_event(
        &self,
        sequence: u64,
        path: &str,
        target: Option<boon_plan_executor::RowId>,
        payload: SourcePayload,
    ) -> Result<SourceEvent, WebPersistenceError> {
        self.ensure_idle()?;
        self.runtime
            .source_event(sequence, path, target, payload)
            .map_err(runtime_error)
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, WebPersistenceError> {
        self.ensure_idle()?;
        self.runtime.root_value_current(name).map_err(runtime_error)
    }

    pub fn output_value_current(&mut self, name: &str) -> Result<Value, WebPersistenceError> {
        self.ensure_idle()?;
        self.runtime
            .output_value_current(name)
            .map_err(runtime_error)
    }

    pub fn inspect_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, WebPersistenceError> {
        self.ensure_idle()?;
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
        self.ensure_idle()?;
        self.runtime
            .demand_document_window_by_id(materialization, visible, overscan)
            .map_err(runtime_error)
    }

    /// Performs the synchronous in-memory half of a turn without publishing its deltas.
    pub fn prepare_dispatch(
        &mut self,
        event: SourceEvent,
    ) -> Result<WebPreparedTurn, WebPersistentDispatchError> {
        if let Some(turn) = &self.pending_turn {
            return Err(WebPersistentDispatchError::Preparation(
                WebPersistenceError::PendingTurn {
                    sequence: turn.sequence,
                },
            ));
        }
        let turn = self.runtime.dispatch_unsettled(event).map_err(|error| {
            WebPersistentDispatchError::Preparation(WebPersistenceError::Runtime(error.to_string()))
        })?;
        let prepared = WebPreparedTurn {
            sequence: turn.sequence,
            source_sequence: turn.source_sequence,
            durable_change_count: turn.durable_changes.len(),
            outbox_change_count: turn.outbox_changes.len(),
        };
        self.pending_turn = Some(turn);
        Ok(prepared)
    }

    /// Asynchronously adopts the complete prepared authority turn and only then exposes deltas.
    pub async fn adopt_prepared_turn(
        &mut self,
    ) -> Result<WebDurablyAcknowledgedTurn, WebPersistentDispatchError> {
        let Some(turn) = self.pending_turn.as_ref() else {
            return Err(WebPersistentDispatchError::Preparation(
                WebPersistenceError::NoPreparedTurn,
            ));
        };
        let batch = match self.checkpoint_for_turn(turn) {
            Ok(batch) => batch,
            Err(error) => return Err(self.rollback_adoption(WebPersistenceError::Store(error))),
        };
        let expected_epoch = batch.next_epoch;
        let expected_turn_sequence = batch.last_turn_sequence;
        let result = self
            .persistence
            .execute(PersistenceCommand::Commit(batch))
            .await;
        let acknowledgement = match result {
            PersistenceResult::Committed(Ok(acknowledgement)) => acknowledgement,
            PersistenceResult::Committed(Err(error)) => {
                return Err(self.rollback_adoption(WebPersistenceError::Store(error)));
            }
            other => {
                return Err(self.rollback_adoption(unexpected_result("Commit", &other)));
            }
        };
        if let Err(error) = ensure_commit_ack(
            &acknowledgement,
            expected_epoch,
            expected_turn_sequence,
            "Commit",
        ) {
            return Err(self.rollback_adoption(error));
        }

        let turn = self
            .pending_turn
            .take()
            .expect("prepared turn exists through adoption");
        self.runtime.settle_turn();
        self.durable.epoch = acknowledgement.epoch;
        self.durable.through_turn_sequence = acknowledgement.through_turn_sequence;
        Ok(WebDurablyAcknowledgedTurn {
            turn,
            acknowledgement,
        })
    }

    pub async fn dispatch(
        &mut self,
        event: SourceEvent,
    ) -> Result<WebDurablyAcknowledgedTurn, WebPersistentDispatchError> {
        self.prepare_dispatch(event)?;
        self.adopt_prepared_turn().await
    }

    pub fn rollback_prepared_turn(&mut self) -> Result<(), WebPersistenceError> {
        let Some(_) = self.pending_turn.take() else {
            return Err(WebPersistenceError::NoPreparedTurn);
        };
        self.runtime
            .rollback_unsettled_turn()
            .map_err(runtime_error)
    }

    pub async fn flush(&mut self) -> Result<BarrierAck, WebPersistenceError> {
        self.ensure_idle()?;
        let result = self
            .persistence
            .execute(PersistenceCommand::Barrier(BarrierRequest {
                application: self.durable.application.clone(),
                through_epoch: self.durable.epoch,
            }))
            .await;
        match result {
            PersistenceResult::BarrierComplete(Ok(acknowledgement)) => Ok(acknowledgement),
            PersistenceResult::BarrierComplete(Err(error)) => {
                Err(WebPersistenceError::Store(error))
            }
            other => Err(unexpected_result("Barrier", &other)),
        }
    }

    pub async fn inspect(
        &mut self,
    ) -> Result<Option<PersistenceInspectorSnapshot>, WebPersistenceError> {
        self.ensure_idle()?;
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
        self.ensure_idle()?;
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
        self.ensure_idle()?;
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
        self.ensure_idle()?;
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
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            Arc::clone(&plan),
            options.clone(),
            Some(restore.clone()),
        )
        .map_err(runtime_error)?;
        let mount = candidate.mount();
        let acknowledgement = match activation {
            Some(batch) => {
                let acknowledgement = activate_batch(&mut self.persistence, batch).await?;
                ensure_activation_ack(
                    &acknowledgement,
                    restore.schema_version,
                    restore.schema_hash,
                    restore.through_turn_sequence,
                )?;
                Some(acknowledgement)
            }
            None => None,
        };
        self.runtime = candidate;
        self.plan = plan;
        self.options = options;
        self.durable = DurableCursor::from_image(&restore);
        if let Some(acknowledgement) = &acknowledgement {
            self.durable.epoch = acknowledgement.epoch;
        }
        Ok(WebPlanActivation {
            mount,
            acknowledgement,
            migration,
        })
    }

    /// Commits current-plan defaults before replacing the published runtime.
    pub async fn start_over(&mut self) -> Result<WebPlanReset, WebPersistenceError> {
        self.ensure_idle()?;
        let current = self.load_durable_image().await?;
        let defaults =
            LiveRuntime::from_shared_machine_plan(Arc::clone(&self.plan), self.options.clone())
                .map_err(runtime_error)?;
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
        let candidate = LiveRuntime::from_shared_machine_plan_with_restore(
            Arc::clone(&self.plan),
            self.options.clone(),
            Some(candidate_image),
        )
        .map_err(runtime_error)?;
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
        let result = self
            .persistence
            .execute(PersistenceCommand::ResetApplication(batch))
            .await;
        let acknowledgement = match result {
            PersistenceResult::ApplicationReset(Ok(acknowledgement)) => acknowledgement,
            PersistenceResult::ApplicationReset(Err(error)) => {
                return Err(WebPersistenceError::Store(error));
            }
            other => return Err(unexpected_result("ResetApplication", &other)),
        };
        if acknowledgement.epoch != next_epoch
            || acknowledgement.schema_version != self.plan.persistence.schema_version
            || acknowledgement.schema_hash != self.plan.persistence.schema_hash
            || acknowledgement.through_turn_sequence != current.through_turn_sequence
        {
            return Err(WebPersistenceError::Protocol(
                "ResetApplication returned an inconsistent acknowledgement".to_owned(),
            ));
        }
        self.runtime = candidate;
        self.durable = DurableCursor {
            application: current.application,
            schema_version: acknowledgement.schema_version,
            schema_hash: acknowledgement.schema_hash,
            epoch: acknowledgement.epoch,
            through_turn_sequence: acknowledgement.through_turn_sequence,
        };
        Ok(WebPlanReset {
            mount,
            acknowledgement,
        })
    }

    pub async fn shutdown(&mut self) -> Result<ShutdownAck, WebPersistenceError> {
        self.ensure_idle()?;
        let result = self
            .persistence
            .execute(PersistenceCommand::Shutdown(ShutdownRequest))
            .await;
        match result {
            PersistenceResult::ShutdownComplete(Ok(acknowledgement)) => Ok(acknowledgement),
            PersistenceResult::ShutdownComplete(Err(error)) => {
                Err(WebPersistenceError::Store(error))
            }
            other => Err(unexpected_result("Shutdown", &other)),
        }
    }

    fn ensure_idle(&self) -> Result<(), WebPersistenceError> {
        match &self.pending_turn {
            Some(turn) => Err(WebPersistenceError::PendingTurn {
                sequence: turn.sequence,
            }),
            None => Ok(()),
        }
    }

    fn checkpoint_for_turn(&self, turn: &RuntimeTurn) -> Result<CheckpointBatch, StoreError> {
        let expected_turn_sequence = self
            .durable
            .through_turn_sequence
            .checked_add(1)
            .ok_or_else(|| StoreError::Backend("persistence turn sequence overflow".to_owned()))?;
        if turn.sequence != expected_turn_sequence {
            return Err(StoreError::NonContiguousTurn);
        }
        let next_epoch = self
            .durable
            .epoch
            .checked_add(1)
            .ok_or_else(|| StoreError::Backend("persistence epoch overflow".to_owned()))?;
        Ok(CheckpointBatch {
            application: self.durable.application.clone(),
            schema_hash: self.durable.schema_hash,
            base_epoch: self.durable.epoch,
            next_epoch,
            first_turn_sequence: turn.sequence,
            last_turn_sequence: turn.sequence,
            changes: turn.durable_changes.clone(),
            outbox_changes: turn.outbox_changes.clone(),
            checksum: [0; 32],
        }
        .seal())
    }

    fn rollback_adoption(&mut self, error: WebPersistenceError) -> WebPersistentDispatchError {
        let turn = self
            .pending_turn
            .take()
            .expect("adoption failure requires a prepared turn");
        let rollback_error = self
            .runtime
            .rollback_unsettled_turn()
            .err()
            .map(|rollback| rollback.to_string());
        WebPersistentDispatchError::AdoptionFailed {
            turn: Box::new(turn),
            error,
            rollback_error,
        }
    }
}

fn runtime_error(error: impl fmt::Display) -> WebPersistenceError {
    WebPersistenceError::Runtime(error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::ApplicationIdentity;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::task::Poll;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    static NEXT_DATABASE_ID: AtomicU32 = AtomicU32::new(0);

    fn counter_plan(identity: ApplicationIdentity) -> Arc<MachinePlan> {
        let runtime = LiveRuntime::from_source_with_identity(
            "web-persistent-counter.bn",
            include_str!("../../../examples/counter.bn"),
            identity,
        )
        .unwrap();
        runtime.shared_machine_plan()
    }

    fn increment_event(runtime: &WebPersistentRuntime, sequence: u64) -> SourceEvent {
        runtime
            .source_event(
                sequence,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap()
    }

    #[wasm_bindgen_test(async)]
    async fn compiled_counter_restores_rolls_back_and_starts_over_across_reopen() {
        let database_name = format!(
            "boon-web-runtime-counter-{}-{}",
            js_sys::Date::now() as u64,
            NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let identity = ApplicationIdentity::new(
            "dev.boon.web-persistent-counter",
            "browser-test",
            "indexeddb",
        );
        let plan = counter_plan(identity.clone());
        let mut opening = Box::pin(WebPersistentRuntime::open_shared(
            Arc::clone(&plan),
            SessionOptions::default(),
            database_name.clone(),
        ));
        assert!(matches!(futures::poll!(opening.as_mut()), Poll::Pending));
        let (mut runtime, startup) = opening.await.unwrap();
        assert!(startup.initialized);
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(0)
        );

        let event = increment_event(&runtime, 1);
        let prepared = runtime.prepare_dispatch(event).unwrap();
        assert_eq!(prepared.sequence, 1);
        assert!(runtime.runtime().is_none());
        let mut adoption = Box::pin(runtime.adopt_prepared_turn());
        assert!(matches!(futures::poll!(adoption.as_mut()), Poll::Pending));
        let acknowledged = adoption.await.unwrap();
        assert_eq!(acknowledged.turn.sequence, 1);
        assert_eq!(acknowledged.acknowledgement.through_turn_sequence, 1);
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(1)
        );
        assert_eq!(runtime.flush().await.unwrap().epoch, 1);
        let inspection = runtime.inspect().await.unwrap().unwrap();
        assert_eq!(inspection.through_turn_sequence, 1);
        assert_eq!(inspection.scalar_count, 1);

        let mut concurrent = RexieDriver::open(database_name.clone()).await.unwrap();
        let external = CheckpointBatch {
            application: identity.clone(),
            schema_hash: plan.persistence.schema_hash,
            base_epoch: 1,
            next_epoch: 2,
            first_turn_sequence: 2,
            last_turn_sequence: 2,
            changes: Vec::new(),
            outbox_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            concurrent
                .execute(PersistenceCommand::Commit(external))
                .await,
            PersistenceResult::Committed(Ok(_))
        ));

        let event = increment_event(&runtime, 2);
        let failed = runtime.dispatch(event).await.unwrap_err();
        assert!(matches!(
            failed,
            WebPersistentDispatchError::AdoptionFailed {
                error: WebPersistenceError::Store(StoreError::StaleEpoch),
                rollback_error: None,
                ..
            }
        ));
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(1)
        );
        assert!(matches!(
            concurrent
                .execute(PersistenceCommand::Shutdown(ShutdownRequest))
                .await,
            PersistenceResult::ShutdownComplete(Ok(_))
        ));
        runtime.shutdown().await.unwrap();

        let (mut runtime, startup) = WebPersistentRuntime::open_shared(
            Arc::clone(&plan),
            SessionOptions::default(),
            database_name.clone(),
        )
        .await
        .unwrap();
        assert!(!startup.initialized);
        assert_eq!(startup.restore_image.through_turn_sequence, 2);
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(1)
        );
        let event = increment_event(&runtime, 3);
        runtime.dispatch(event).await.unwrap();
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(2)
        );

        let activation = runtime
            .activate_machine_plan(Arc::clone(&plan), SessionOptions::default())
            .await
            .unwrap();
        assert!(activation.acknowledgement.is_none());
        let reset = runtime.start_over().await.unwrap();
        assert_eq!(reset.acknowledgement.through_turn_sequence, 3);
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(0)
        );
        runtime.shutdown().await.unwrap();

        let (mut runtime, startup) = WebPersistentRuntime::open_shared(
            plan,
            SessionOptions::default(),
            database_name.clone(),
        )
        .await
        .unwrap();
        assert!(!startup.initialized);
        assert_eq!(
            runtime.root_value_current("store.count").unwrap(),
            Value::Number(0)
        );
        runtime.shutdown().await.unwrap();
        RexieDriver::delete_database(&database_name).await.unwrap();
    }
}
