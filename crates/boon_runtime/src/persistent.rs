use super::effects::{
    HostEffectDriver, HostEffectError, HostEffectReconciliation, HostEffectRequest,
    HostEffectWorker, HostEffectWorkerOperation, HostEffectWorkerOutcome,
};
use super::{
    DocumentPatch, LiveRuntime, ProgramArtifact, ProgramArtifactOwnership, ProgramCompletion,
    ProgramDiagnostic, ProgramDocumentHost, ProgramHostCompletion, ProgramHostRequest,
    ProgramHostUpdate, ProgramRejection, ProgramRequestId, ProgramSessionId, RuntimeTurn,
    SourcePayload, TransientEffectCallId,
};
use boon_persistence::{
    ActivationAck, ActivationBatch, AuthorityTurn, AuthorityTurnReservation, BarrierAck, CommitAck,
    CompactAck, ContentArtifact, ContentArtifactId, ContentArtifactLoadCompletion,
    ContentArtifactLoadEnqueueError, ContentArtifactLoadTicket, ContentArtifactManifest,
    ContentArtifactRetention, ContentArtifactStoreCompletion, ContentArtifactStoreEnqueueError,
    ContentArtifactStoreTicket, DecodeLimits, DurableContentArtifactChange, DurableOutboxChange,
    DurableOutboxItem, DurableOutboxState, OutboxItemId, PersistenceControlError,
    PersistenceCoordinator, PersistenceDriver, PersistenceInspectorSnapshot,
    PersistenceWorkerConfig, PersistenceWorkerStartError, PersistenceWorkerStatus,
    PutContentArtifactAck, ResetApplicationAck, ResetApplicationBatch, RestoreImage, StoredValue,
    TurnEnqueueError, TurnReservationError, apply_durable_outbox_changes,
    decode_application_transfer, encode_application_transfer, stage_migration,
};
use boon_plan::{MachinePlan, MemoryKind};
use boon_plan_executor::{SessionOptions, SourceEvent, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        error: Box<TurnEnqueueError>,
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
    /// Latest successfully admitted effect state, including turns not yet durable.
    effect_work: EffectWorkIndex,
    /// Effect state covered by the persistence worker's durable watermark.
    durable_effect_work: EffectWorkIndex,
    pending_effect_durability: VecDeque<PendingEffectDurability>,
    ready_effect_actions: VecDeque<DeferredEffectAction>,
    last_rebuild_derived_us: u64,
    generation: u64,
    program_artifacts: ProgramArtifactLanes,
}

const MAX_PENDING_EFFECT_DURABILITY: usize = 8;

#[derive(Clone, Debug)]
struct PendingEffectDurability {
    turn_sequence: u64,
    durable_effect_work: EffectWorkIndex,
    after_acknowledgement: Option<DeferredEffectAction>,
}

#[derive(Clone, Debug)]
enum DeferredEffectAction {
    Dispatch(DurableOutboxItem),
    Reconcile(DurableOutboxItem),
}

#[derive(Clone, Debug, Default)]
struct EffectWorkIndex {
    items: BTreeMap<OutboxItemId, DurableOutboxItem>,
    unfinished_count: usize,
}

impl EffectWorkIndex {
    fn from_items(items: BTreeMap<OutboxItemId, DurableOutboxItem>) -> Self {
        let unfinished_count = items
            .values()
            .filter(|item| !matches!(item.state, DurableOutboxState::Completed { .. }))
            .count();
        Self {
            items,
            unfinished_count,
        }
    }

    fn applying(&self, changes: &[DurableOutboxChange]) -> Result<Self, String> {
        if changes.is_empty() {
            return Ok(self.clone());
        }
        let mut items = self.items.clone();
        apply_durable_outbox_changes(&mut items, changes).map_err(|error| error.to_string())?;
        Ok(Self::from_items(items))
    }

    fn has_work(&self) -> bool {
        self.unfinished_count != 0
    }

    fn work_items(&self) -> Vec<EffectWorkItem> {
        self.items.values().filter_map(effect_work_item).collect()
    }

    fn next_work(&self) -> Option<EffectWorkItem> {
        self.items.values().find_map(effect_work_item)
    }

    fn item(&self, item_id: OutboxItemId) -> Option<&DurableOutboxItem> {
        self.items.get(&item_id)
    }
}

fn effect_work_item(item: &DurableOutboxItem) -> Option<EffectWorkItem> {
    let kind = match item.state {
        DurableOutboxState::Pending => EffectWorkKind::Dispatch,
        DurableOutboxState::Dispatching { .. }
        | DurableOutboxState::ReconciliationRequired { .. } => EffectWorkKind::Reconcile,
        DurableOutboxState::Completed { .. } => return None,
    };
    Some(EffectWorkItem {
        kind,
        item: item.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistentRuntimeStartupDisposition {
    Fresh,
    Restored,
    Migrated(boon_persistence::MigrationPreview),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistentRuntimeStartup {
    pub restore_image: RestoreImage,
    pub disposition: PersistentRuntimeStartupDisposition,
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

type PreparedStateArtifact = (
    LiveRuntime,
    PersistentStateArtifactPreview,
    BTreeSet<boon_plan::MigrationEdgeId>,
    ContentArtifactManifest,
    BTreeMap<ContentArtifactId, ContentArtifact>,
    u64,
);

pub struct DurablyAcknowledgedTurn {
    pub turn: RuntimeTurn,
    pub acknowledgement: CommitAck,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DurabilityTicket {
    pub generation: u64,
    pub turn_sequence: u64,
}

const MAX_PROGRAM_ARTIFACT_STORE_SESSIONS: usize = 8;
const MAX_PROGRAM_ARTIFACT_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS: usize = 8;
const REPLACEABLE_ARTIFACT_QUIET_PERIOD: Duration = Duration::from_millis(34);
const PROGRAM_ARTIFACT_STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramArtifactLaneKind {
    Store,
    Load,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramArtifactLaneOutcome {
    Applied,
    StaleRejected,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ProgramArtifactLaneObservation {
    pub lane: ProgramArtifactLaneKind,
    pub request_id: String,
    pub revision: u64,
    pub queue_depth: u32,
    pub queue_wait_us: u64,
    pub worker_us: u64,
    pub apply_us: u64,
    pub end_to_end_us: u64,
    pub outcome: ProgramArtifactLaneOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramArtifactTurnKind {
    Parent,
    Child,
}

#[derive(Clone, Debug)]
pub struct ProgramArtifactTurn {
    pub kind: ProgramArtifactTurnKind,
    pub source_path: String,
    pub turn: RuntimeTurn,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProgramCompletionObservation {
    Host(ProgramHostCompletion),
    ArtifactStorePending {
        session: ProgramSessionId,
        request_id: ProgramRequestId,
        artifact_id: ContentArtifactId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedProgramCompletion {
    pub changed: bool,
    pub completion: ProgramCompletionObservation,
}

#[derive(Debug, Default)]
pub struct ProgramArtifactDrive {
    pub changed: bool,
    pub patches: Vec<DocumentPatch>,
    pub turns: Vec<ProgramArtifactTurn>,
    pub observations: Vec<ProgramArtifactLaneObservation>,
    pub completion: Option<ProgramCompletionObservation>,
    pub poll_required: bool,
}

impl ProgramArtifactDrive {
    fn merge(&mut self, mut other: Self) {
        self.changed |= other.changed;
        self.patches.append(&mut other.patches);
        self.turns.append(&mut other.turns);
        self.observations.append(&mut other.observations);
        if other.completion.is_some() {
            self.completion = other.completion;
        }
        self.poll_required |= other.poll_required;
    }
}

#[derive(Clone, Debug)]
struct PendingProgramArtifactStore {
    candidate_sequence: u64,
    mount_epoch: u64,
    session: ProgramSessionId,
    request_id: ProgramRequestId,
    artifact: ProgramArtifact,
    ownership: ProgramArtifactOwnership,
    activated_before_store: bool,
    queued_at: Instant,
    store_after: Instant,
    queue_depth: u32,
    queue_wait_us: u64,
    worker_us: u64,
}

#[derive(Debug)]
struct ProgramArtifactStoreFlight {
    ticket: ContentArtifactStoreTicket,
    mount_epoch: u64,
    artifact_id: ContentArtifactId,
    waiters: BTreeMap<ProgramSessionId, PendingProgramArtifactStore>,
    started_at: Instant,
}

#[derive(Debug)]
struct PendingProgramArtifactActivation {
    ticket: DurabilityTicket,
    pending: PendingProgramArtifactStore,
}

#[derive(Debug, Default)]
struct ProgramArtifactStoreLane {
    mount_epoch: u64,
    next_candidate_sequence: u64,
    in_flight: Option<ProgramArtifactStoreFlight>,
    pending_by_session: BTreeMap<ProgramSessionId, PendingProgramArtifactStore>,
    ready_for_authority: BTreeMap<u64, PendingProgramArtifactStore>,
    awaiting_durability: BTreeMap<u64, PendingProgramArtifactActivation>,
    queued_bytes: usize,
}

impl ProgramArtifactStoreLane {
    fn has_pending(&self) -> bool {
        self.in_flight.is_some()
            || !self.pending_by_session.is_empty()
            || !self.ready_for_authority.is_empty()
            || !self.awaiting_durability.is_empty()
    }

    fn session_count(&self) -> usize {
        let mut sessions = self
            .pending_by_session
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(in_flight) = &self.in_flight {
            sessions.extend(in_flight.waiters.keys().cloned());
        }
        sessions.extend(
            self.ready_for_authority
                .values()
                .map(|pending| pending.session.clone()),
        );
        sessions.extend(
            self.awaiting_durability
                .values()
                .map(|activation| activation.pending.session.clone()),
        );
        sessions.len()
    }

    fn next_sequence(&mut self) -> u64 {
        self.next_candidate_sequence = self.next_candidate_sequence.saturating_add(1);
        self.next_candidate_sequence
    }

    fn remove_waiter_bytes(&mut self, waiter: &PendingProgramArtifactStore) {
        self.queued_bytes = self
            .queued_bytes
            .saturating_sub(waiter.artifact.content_bytes_len());
    }
}

#[derive(Debug)]
struct PendingProgramArtifactLoad {
    candidate_sequence: u64,
    mount_epoch: u64,
    request: ProgramHostRequest,
    queued_at: Instant,
    queue_depth: u32,
}

#[derive(Debug)]
struct ProgramArtifactLoadFlight {
    ticket: ContentArtifactLoadTicket,
    mount_epoch: u64,
    artifact_id: ContentArtifactId,
    waiters: BTreeMap<ProgramSessionId, PendingProgramArtifactLoad>,
    started_at: Instant,
}

#[derive(Debug, Default)]
struct ProgramArtifactLoadLane {
    mount_epoch: u64,
    next_candidate_sequence: u64,
    in_flight: Option<ProgramArtifactLoadFlight>,
    pending_by_session: BTreeMap<ProgramSessionId, PendingProgramArtifactLoad>,
}

impl ProgramArtifactLoadLane {
    fn has_pending(&self) -> bool {
        self.in_flight.is_some() || !self.pending_by_session.is_empty()
    }

    fn session_count(&self) -> usize {
        let mut sessions = self
            .pending_by_session
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(in_flight) = &self.in_flight {
            sessions.extend(in_flight.waiters.keys().cloned());
        }
        sessions.len()
    }

    fn next_sequence(&mut self) -> u64 {
        self.next_candidate_sequence = self.next_candidate_sequence.saturating_add(1);
        self.next_candidate_sequence
    }
}

#[derive(Debug, Default)]
struct ProgramArtifactLanes {
    store: ProgramArtifactStoreLane,
    load: ProgramArtifactLoadLane,
    cache: BTreeMap<ContentArtifactId, ContentArtifact>,
    requests: Vec<ProgramHostRequest>,
}

impl PersistentRuntime {
    pub fn from_machine_plan<D>(
        plan: MachinePlan,
        options: SessionOptions,
        driver: D,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, PersistentRuntimeStartup), PersistentRuntimeStartError>
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
    ) -> Result<(Self, PersistentRuntimeStartup), PersistentRuntimeStartError>
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

        let (runtime, disposition) = if startup.initialized {
            (default_runtime, PersistentRuntimeStartupDisposition::Fresh)
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
            (runtime, PersistentRuntimeStartupDisposition::Restored)
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
            let preview = staged.preview;
            startup.restore_image = staged.candidate;
            startup.restore_image.epoch = acknowledgement.epoch;
            (
                candidate,
                PersistentRuntimeStartupDisposition::Migrated(preview),
            )
        };

        let startup = PersistentRuntimeStartup {
            restore_image: startup.restore_image,
            disposition,
        };
        let effect_work = EffectWorkIndex::from_items(startup.restore_image.outbox.clone());

        Ok((
            Self {
                runtime,
                persistence,
                durable_effect_work: effect_work.clone(),
                effect_work,
                pending_effect_durability: VecDeque::new(),
                ready_effect_actions: VecDeque::new(),
                last_rebuild_derived_us,
                generation: 1,
                program_artifacts: ProgramArtifactLanes::default(),
            },
            startup,
        ))
    }

    pub fn runtime(&self) -> &LiveRuntime {
        &self.runtime
    }

    pub fn reset_program_artifacts(&mut self) {
        self.program_artifacts = ProgramArtifactLanes::default();
        let _ = self.take_content_artifact_store_completions();
        let _ = self.take_content_artifact_load_completions();
    }

    pub fn queue_program_requests(&mut self, requests: Vec<ProgramHostRequest>) {
        self.program_artifacts.requests.extend(requests);
    }

    pub fn take_program_requests(&mut self) -> Vec<ProgramHostRequest> {
        std::mem::take(&mut self.program_artifacts.requests)
    }

    pub fn program_artifact_lane_counts(&self) -> (usize, usize) {
        (
            self.program_artifacts.store.session_count(),
            self.program_artifacts.load.session_count(),
        )
    }

    pub fn program_artifacts_pending(&self) -> bool {
        self.program_artifacts.store.has_pending() || self.program_artifacts.load.has_pending()
    }

    pub fn resolve_program_artifact_requests_blocking(
        &mut self,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let started = Instant::now();
        let mut drive = self.resolve_program_artifact_requests(host, source_sequence)?;
        loop {
            drive.merge(self.poll_program_artifacts(host, source_sequence)?);
            let queued_artifact_request = self
                .program_artifacts
                .requests
                .iter()
                .any(ProgramHostRequest::is_artifact_load);
            if !queued_artifact_request && !self.program_artifacts.load.has_pending() {
                return Ok(drive);
            }
            if started.elapsed() >= PROGRAM_ARTIFACT_STARTUP_TIMEOUT {
                let persistence = self.status();
                return Err(PersistentDispatchError::Runtime(format!(
                    "program artifact startup currentness barrier exceeded {} ms; queued_requests={}, load_sessions={}, persistence_loads={}, worker_alive={}, last_error={:?}",
                    PROGRAM_ARTIFACT_STARTUP_TIMEOUT.as_millis(),
                    usize::from(queued_artifact_request),
                    self.program_artifacts.load.session_count(),
                    persistence.pending_content_artifact_loads,
                    persistence.worker_alive,
                    persistence.last_error,
                )));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn resolve_program_artifact_requests(
        &mut self,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut lanes = std::mem::take(&mut self.program_artifacts);
        let result = lanes.resolve_requests(self, host, source_sequence);
        self.program_artifacts = lanes;
        result
    }

    pub fn complete_program_observed(
        &mut self,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut lanes = std::mem::take(&mut self.program_artifacts);
        let completed =
            lanes.complete_program(self, host, source_sequence, session, request_id, result);
        self.program_artifacts = lanes;
        completed
    }

    pub fn poll_program_artifacts(
        &mut self,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut lanes = std::mem::take(&mut self.program_artifacts);
        let result = lanes.poll(self, host, source_sequence);
        self.program_artifacts = lanes;
        result
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

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, PersistentDispatchError> {
        self.runtime
            .root_value_current(name)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn output_value_current(&mut self, name: &str) -> Result<Value, PersistentDispatchError> {
        self.runtime
            .output_value_current(name)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn assert_scenario_step(
        &mut self,
        step: &crate::ScenarioStep,
        turn: Option<&RuntimeTurn>,
    ) -> Result<(), PersistentDispatchError> {
        self.runtime
            .assert_scenario_step(step, turn)
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
        let turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.admit_buffered_turn(reservation, turn)
    }

    /// Completes one transient host effect as a normal persistent authority
    /// turn. The completion can update durable state and emit chained effects;
    /// neither is exposed unless the same admission/rollback contract as a
    /// source turn succeeds.
    pub fn complete_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<RuntimeTurn, PersistentDispatchError> {
        let reservation = self
            .persistence
            .try_reserve_turn()
            .map_err(PersistentDispatchError::Backpressure)?;
        let turn = self
            .runtime
            .complete_transient_effect_unsettled(call_id, outcome)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.admit_buffered_turn(reservation, turn)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<RuntimeTurn, PersistentDispatchError> {
        let reservation = self
            .persistence
            .try_reserve_turn()
            .map_err(PersistentDispatchError::Backpressure)?;
        let turn = self
            .runtime
            .deliver_transient_effect_result_unsettled(call_id, result_sequence, outcome)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.admit_buffered_turn(reservation, turn)
    }

    fn admit_buffered_turn(
        &mut self,
        reservation: AuthorityTurnReservation,
        mut turn: RuntimeTurn,
    ) -> Result<RuntimeTurn, PersistentDispatchError> {
        let next_effect_work = self.stage_effect_work_for_unsettled_turn(&turn)?;
        if next_effect_work.is_some()
            && self.pending_effect_durability.len() >= MAX_PENDING_EFFECT_DURABILITY
        {
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            let rollback = rollback_error.map_or_else(String::new, |rollback| {
                format!("; runtime rollback also failed: {rollback}")
            });
            return Err(PersistentDispatchError::Runtime(format!(
                "durable effect lane has {} pending turns, limit is {MAX_PENDING_EFFECT_DURABILITY}{rollback}",
                self.pending_effect_durability.len()
            )));
        }
        let persistence_started = Instant::now();
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_outbox_changes(turn.outbox_changes.clone());
        if let Err(error) = reservation.enqueue(authority) {
            turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            return Err(PersistentDispatchError::PersistenceAdmissionFailed {
                turn: Box::new(turn),
                error: Box::new(error),
                rollback_error,
            });
        }
        turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
        if let Some(next_effect_work) = next_effect_work {
            self.effect_work = next_effect_work.clone();
            self.pending_effect_durability
                .push_back(PendingEffectDurability {
                    turn_sequence: turn.sequence,
                    durable_effect_work: next_effect_work,
                    after_acknowledgement: None,
                });
        }
        self.runtime.settle_turn();
        Ok(turn)
    }

    /// Admits a program-artifact ownership transition to the existing bounded
    /// authority queue without waiting for storage. The caller may expose
    /// compile progress immediately, but must not activate the corresponding
    /// child program until `durability_ticket_is_acknowledged` returns true.
    pub fn dispatch_with_content_artifact_changes(
        &mut self,
        event: SourceEvent,
        content_artifact_changes: Vec<DurableContentArtifactChange>,
    ) -> Result<(RuntimeTurn, DurabilityTicket), PersistentDispatchError> {
        let reservation = self
            .persistence
            .try_reserve_turn()
            .map_err(PersistentDispatchError::Backpressure)?;
        let mut turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        if !turn.outbox_changes.is_empty() {
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            let rollback = rollback_error.map_or_else(String::new, |rollback| {
                format!("; runtime rollback also failed: {rollback}")
            });
            return Err(PersistentDispatchError::Runtime(format!(
                "program artifact lifecycle turn cannot also start a host effect{rollback}"
            )));
        }
        let persistence_started = Instant::now();
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_content_artifact_changes(content_artifact_changes);
        if let Err(error) = reservation.enqueue(authority) {
            turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
            let rollback_error = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| error.to_string());
            return Err(PersistentDispatchError::PersistenceAdmissionFailed {
                turn: Box::new(turn),
                error: Box::new(error),
                rollback_error,
            });
        }
        turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
        let ticket = DurabilityTicket {
            generation: self.generation,
            turn_sequence: turn.sequence,
        };
        self.runtime.settle_turn();
        Ok((turn, ticket))
    }

    pub fn durability_ticket_is_acknowledged(&self, ticket: DurabilityTicket) -> bool {
        ticket.generation == self.generation
            && self.persistence.status().durable_through_turn_sequence >= ticket.turn_sequence
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
        let turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.commit_unsettled_immediate(turn)
    }

    /// Completes a transient effect and waits until that exact completion turn
    /// is durable. Request hosts use this before exposing a response derived
    /// from an asynchronous effect result.
    pub fn complete_transient_effect_durably(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<DurablyAcknowledgedTurn, PersistentDispatchError> {
        let turn = self
            .runtime
            .complete_transient_effect_unsettled(call_id, outcome)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.commit_unsettled_immediate(turn)
    }

    pub fn deliver_transient_effect_result_durably(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<DurablyAcknowledgedTurn, PersistentDispatchError> {
        let turn = self
            .runtime
            .deliver_transient_effect_result_unsettled(call_id, result_sequence, outcome)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        self.commit_unsettled_immediate(turn)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<bool, PersistentDispatchError> {
        self.runtime
            .cancel_transient_effect(call_id)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn pending_transient_effect_credits(&self, call_id: TransientEffectCallId) -> Option<u32> {
        self.runtime.pending_transient_effect_credits(call_id)
    }

    fn commit_unsettled_immediate(
        &mut self,
        mut turn: RuntimeTurn,
    ) -> Result<DurablyAcknowledgedTurn, PersistentDispatchError> {
        let next_effect_work = self.stage_effect_work_for_unsettled_turn(&turn)?;
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
        self.promote_effect_durability();
        if let Some(next_effect_work) = next_effect_work {
            self.effect_work = next_effect_work.clone();
            self.durable_effect_work = next_effect_work;
        }
        self.runtime.settle_turn();
        Ok(DurablyAcknowledgedTurn {
            turn,
            acknowledgement,
        })
    }

    fn stage_effect_work_for_unsettled_turn(
        &mut self,
        turn: &RuntimeTurn,
    ) -> Result<Option<EffectWorkIndex>, PersistentDispatchError> {
        if turn.outbox_changes.is_empty() {
            return Ok(None);
        }
        match self.effect_work.applying(&turn.outbox_changes) {
            Ok(next) => Ok(Some(next)),
            Err(error) => {
                let rollback_error = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| error.to_string());
                let rollback = rollback_error.map_or_else(String::new, |rollback| {
                    format!("; runtime rollback also failed: {rollback}")
                });
                Err(PersistentDispatchError::Runtime(format!(
                    "runtime produced invalid durable effect transitions: {error}{rollback}"
                )))
            }
        }
    }

    pub fn status(&self) -> PersistenceWorkerStatus {
        self.persistence.status()
    }

    pub fn semantic_value_image(&self) -> Result<RestoreImage, String> {
        self.runtime
            .semantic_value_image()
            .map_err(|error| error.to_string())
    }

    pub fn last_rebuild_derived_us(&self) -> u64 {
        self.last_rebuild_derived_us
    }

    pub fn generation(&self) -> u64 {
        self.generation
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

    pub fn put_content_artifact(
        &self,
        artifact: ContentArtifact,
    ) -> Result<PutContentArtifactAck, PersistenceControlError> {
        self.persistence.put_content_artifact(artifact)
    }

    pub fn try_put_content_artifact(
        &self,
        artifact: ContentArtifact,
    ) -> Result<ContentArtifactStoreTicket, ContentArtifactStoreEnqueueError> {
        self.persistence.try_put_content_artifact(artifact)
    }

    pub fn take_content_artifact_store_completions(&self) -> Vec<ContentArtifactStoreCompletion> {
        self.persistence.take_content_artifact_store_completions()
    }

    pub fn load_content_artifact(
        &self,
        id: ContentArtifactId,
    ) -> Result<Option<ContentArtifact>, PersistenceControlError> {
        self.persistence.load_content_artifact(id)
    }

    pub fn try_load_content_artifact(
        &self,
        id: ContentArtifactId,
    ) -> Result<ContentArtifactLoadTicket, ContentArtifactLoadEnqueueError> {
        self.persistence.try_load_content_artifact(id)
    }

    pub fn take_content_artifact_load_completions(&self) -> Vec<ContentArtifactLoadCompletion> {
        self.persistence.take_content_artifact_load_completions()
    }

    pub fn inspect(&self) -> Result<Option<PersistenceInspectorSnapshot>, PersistenceControlError> {
        self.persistence.inspect()
    }

    pub fn export_state_artifact(&self) -> Result<Vec<u8>, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let transfer = self
            .persistence
            .export_application()
            .map_err(PersistentActivationError::Persistence)?;
        encode_application_transfer(&transfer)
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))
    }

    pub fn preview_state_artifact(
        &self,
        artifact: &[u8],
        options: SessionOptions,
    ) -> Result<PersistentStateArtifactPreview, PersistentActivationError> {
        let (_, preview, _, _, _, _) = self.prepare_state_artifact(artifact, options)?;
        Ok(preview)
    }

    pub fn activate_state_artifact(
        &mut self,
        artifact: &[u8],
        options: SessionOptions,
    ) -> Result<PersistentStateArtifactActivation, PersistentActivationError> {
        let (
            candidate,
            preview,
            completed_migration_edges,
            content_artifact_manifest,
            content_artifacts,
            rebuild_derived_us,
        ) = self.prepare_state_artifact(artifact, options)?;
        self.last_rebuild_derived_us = rebuild_derived_us;
        let mount = candidate.mount();
        let acknowledgement = self.activate_settled_candidate_with_artifacts(
            candidate,
            completed_migration_edges,
            Some(content_artifact_manifest),
            content_artifacts,
        )?;
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
    ) -> Result<PreparedStateArtifact, PersistentActivationError> {
        self.persistence
            .barrier()
            .map_err(PersistentActivationError::Persistence)?;
        let current = self
            .persistence
            .load()
            .map_err(PersistentActivationError::Persistence)?
            .ok_or(PersistentActivationError::MissingDurableState)?;
        reject_unfinished_outbox(&current, "import state")?;
        let transfer = decode_application_transfer(artifact, DecodeLimits::default())
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        let imported = transfer.restore_image;
        if imported.application != current.application {
            return Err(PersistentActivationError::Runtime(
                "state artifact belongs to a different application namespace".to_owned(),
            ));
        }
        reject_unfinished_outbox(&imported, "import state artifact")?;
        let source_schema_version = imported.schema_version;
        let content_artifact_manifest = imported.content_artifact_manifest.clone();
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
            content_artifact_manifest,
            transfer.content_artifacts,
            rebuild_derived_us,
        ))
    }

    pub fn load_durable_image(
        &self,
    ) -> Result<Option<boon_persistence::RestoreImage>, PersistenceControlError> {
        self.persistence.load()
    }

    /// Returns whether acknowledged durable effect work is ready locally.
    ///
    /// This is the scheduler hot-path query. It never talks to the persistence
    /// worker; restart recovery initialized the index from the durable image,
    /// and successful outbox commits advance it in lockstep.
    pub fn has_effect_work(&self) -> bool {
        self.durable_effect_work.has_work()
            || !self.pending_effect_durability.is_empty()
            || !self.ready_effect_actions.is_empty()
    }

    pub fn effect_work_items(&self) -> Result<Vec<EffectWorkItem>, PersistentEffectError> {
        Ok(self.effect_work.work_items())
    }

    fn promote_effect_durability(&mut self) {
        let durable_turn = self.persistence.status().durable_through_turn_sequence;
        while self
            .pending_effect_durability
            .front()
            .is_some_and(|pending| pending.turn_sequence <= durable_turn)
        {
            let pending = self
                .pending_effect_durability
                .pop_front()
                .expect("durable effect transition exists");
            self.durable_effect_work = pending.durable_effect_work;
            if let Some(action) = pending.after_acknowledgement {
                self.ready_effect_actions.push_back(action);
            }
        }
    }

    pub fn drive_effect_work_once(
        &mut self,
        driver: &mut impl HostEffectDriver,
    ) -> Result<Option<RuntimeTurn>, PersistentEffectDriveError> {
        let Some(work) = self.effect_work.next_work() else {
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
        self.promote_effect_durability();
        if let Some(result) = worker.try_result().map_err(|error| {
            PersistentEffectDriveError::Runtime(PersistentEffectError::Runtime(error.to_string()))
        })? {
            return match result.outcome {
                HostEffectWorkerOutcome::Dispatched(Ok(outcome)) => self
                    .complete_effect_async(result.request.item_id, outcome)
                    .map(Some)
                    .map_err(PersistentEffectDriveError::Runtime),
                HostEffectWorkerOutcome::Dispatched(Err(host)) => {
                    let _ = host;
                    self.mark_effect_reconciliation_async(result.request.item_id)
                        .map(Some)
                        .map_err(PersistentEffectDriveError::Runtime)
                }
                HostEffectWorkerOutcome::Reconciled(Ok(HostEffectReconciliation::Applied(
                    outcome,
                ))) => self
                    .complete_effect_async(result.request.item_id, outcome)
                    .map(Some)
                    .map_err(PersistentEffectDriveError::Runtime),
                HostEffectWorkerOutcome::Reconciled(Ok(HostEffectReconciliation::NotApplied)) => {
                    self.claim_effect_for_dispatch_async(result.request.item_id)
                        .map(Some)
                        .map_err(PersistentEffectDriveError::Runtime)
                }
                HostEffectWorkerOutcome::Reconciled(Err(host)) => {
                    Err(PersistentEffectDriveError::Host(host))
                }
            };
        }
        if worker.is_busy() {
            return Ok(None);
        }
        if let Some(action) = self.ready_effect_actions.pop_front() {
            match action {
                DeferredEffectAction::Dispatch(item) => {
                    submit_background_effect(worker, HostEffectWorkerOperation::Dispatch, &item)?
                }
                DeferredEffectAction::Reconcile(item) => {
                    submit_background_effect(worker, HostEffectWorkerOperation::Reconcile, &item)?
                }
            }
            return Ok(None);
        }
        if !self.pending_effect_durability.is_empty() {
            return Ok(None);
        }
        let Some(work) = self.durable_effect_work.next_work() else {
            return Ok(None);
        };
        match work.kind {
            EffectWorkKind::Dispatch => {
                return self
                    .claim_effect_for_dispatch_async(work.item.item_id)
                    .map(Some)
                    .map_err(PersistentEffectDriveError::Runtime);
            }
            EffectWorkKind::Reconcile => {
                if matches!(work.item.state, DurableOutboxState::Dispatching { .. }) {
                    return self
                        .mark_effect_reconciliation_async(work.item.item_id)
                        .map(Some)
                        .map_err(PersistentEffectDriveError::Runtime);
                } else {
                    submit_background_effect(
                        worker,
                        HostEffectWorkerOperation::Reconcile,
                        &work.item,
                    )?;
                }
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

    fn claim_effect_for_dispatch_async(
        &mut self,
        item_id: OutboxItemId,
    ) -> Result<RuntimeTurn, PersistentEffectError> {
        let reservation = self.reserve_effect_turn()?;
        let item = self.load_effect_item(item_id)?;
        let turn = self
            .runtime
            .begin_effect_dispatch_unsettled(&item)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        let mut claimed = item;
        let attempt = match claimed.state {
            DurableOutboxState::Pending => 1,
            DurableOutboxState::ReconciliationRequired { attempt } => {
                attempt.checked_add(1).ok_or_else(|| {
                    PersistentEffectError::Runtime("effect attempt overflow".to_owned())
                })?
            }
            _ => {
                let rollback = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| format!("; runtime rollback also failed: {error}"))
                    .unwrap_or_default();
                return Err(PersistentEffectError::Runtime(format!(
                    "effect item changed state while claiming dispatch{rollback}"
                )));
            }
        };
        claimed.revision = claimed
            .revision
            .checked_add(1)
            .ok_or_else(|| PersistentEffectError::Runtime("outbox revision overflow".to_owned()))?;
        claimed.updated_turn_sequence = turn.sequence;
        claimed.state = DurableOutboxState::Dispatching { attempt };
        self.enqueue_effect_turn_async(
            reservation,
            turn,
            Some(DeferredEffectAction::Dispatch(claimed)),
        )
    }

    fn mark_effect_reconciliation_async(
        &mut self,
        item_id: OutboxItemId,
    ) -> Result<RuntimeTurn, PersistentEffectError> {
        let reservation = self.reserve_effect_turn()?;
        let item = self.load_effect_item(item_id)?;
        let attempt = item.state.attempt();
        let turn = self
            .runtime
            .require_effect_reconciliation_unsettled(&item)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        let mut next = item;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| PersistentEffectError::Runtime("outbox revision overflow".to_owned()))?;
        next.updated_turn_sequence = turn.sequence;
        next.state = DurableOutboxState::ReconciliationRequired { attempt };
        self.enqueue_effect_turn_async(
            reservation,
            turn,
            Some(DeferredEffectAction::Reconcile(next)),
        )
    }

    fn complete_effect_async(
        &mut self,
        item_id: OutboxItemId,
        outcome: StoredValue,
    ) -> Result<RuntimeTurn, PersistentEffectError> {
        let reservation = self.reserve_effect_turn()?;
        let item = self.load_effect_item(item_id)?;
        let turn = self
            .runtime
            .complete_effect_unsettled(&item, outcome)
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))?;
        self.enqueue_effect_turn_async(reservation, turn, None)
    }

    fn reserve_effect_turn(&self) -> Result<AuthorityTurnReservation, PersistentEffectError> {
        if self.pending_effect_durability.len() >= MAX_PENDING_EFFECT_DURABILITY {
            return Err(PersistentEffectError::Runtime(format!(
                "durable effect lane has {} pending turns, limit is {MAX_PENDING_EFFECT_DURABILITY}",
                self.pending_effect_durability.len()
            )));
        }
        self.persistence
            .try_reserve_turn()
            .map_err(|error| PersistentEffectError::Runtime(error.to_string()))
    }

    fn enqueue_effect_turn_async(
        &mut self,
        reservation: AuthorityTurnReservation,
        mut turn: RuntimeTurn,
        after_acknowledgement: Option<DeferredEffectAction>,
    ) -> Result<RuntimeTurn, PersistentEffectError> {
        let next_effect_work = match self.effect_work.applying(&turn.outbox_changes) {
            Ok(next) => next,
            Err(error) => {
                let rollback = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| format!("; runtime rollback also failed: {error}"))
                    .unwrap_or_default();
                return Err(PersistentEffectError::Runtime(format!(
                    "runtime produced invalid durable effect transitions: {error}{rollback}"
                )));
            }
        };
        let persistence_started = Instant::now();
        let authority = AuthorityTurn::new(turn.sequence, turn.durable_changes.clone())
            .with_outbox_changes(turn.outbox_changes.clone());
        if let Err(error) = reservation.enqueue(authority) {
            turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
            let rollback = self
                .runtime
                .rollback_unsettled_turn()
                .err()
                .map(|error| format!("; runtime rollback also failed: {error}"))
                .unwrap_or_default();
            return Err(PersistentEffectError::Runtime(format!(
                "persistence rejected a durable effect transition: {error}{rollback}"
            )));
        }
        turn.phase_timings.persistence_enqueue_us = duration_us(persistence_started.elapsed());
        self.effect_work = next_effect_work.clone();
        self.pending_effect_durability
            .push_back(PendingEffectDurability {
                turn_sequence: turn.sequence,
                durable_effect_work: next_effect_work,
                after_acknowledgement,
            });
        self.runtime.settle_turn();
        Ok(turn)
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
        self.effect_work
            .item(item_id)
            .cloned()
            .ok_or(PersistentEffectError::MissingItem(item_id))
    }

    fn commit_effect_turn(&mut self, turn: &RuntimeTurn) -> Result<(), PersistentEffectError> {
        let next_effect_work = match self.effect_work.applying(&turn.outbox_changes) {
            Ok(next) => next,
            Err(error) => {
                let rollback_error = self
                    .runtime
                    .rollback_unsettled_turn()
                    .err()
                    .map(|error| error.to_string());
                let rollback = rollback_error.map_or_else(String::new, |rollback| {
                    format!("; runtime rollback also failed: {rollback}")
                });
                return Err(PersistentEffectError::Runtime(format!(
                    "runtime produced invalid durable effect transitions: {error}{rollback}"
                )));
            }
        };
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
        self.promote_effect_durability();
        self.effect_work = next_effect_work.clone();
        self.durable_effect_work = next_effect_work;
        Ok(())
    }

    /// Commits a fully built and settled candidate before swapping runtime
    /// ownership. A failed backend activation leaves the old runtime active.
    pub fn activate_settled_candidate(
        &mut self,
        candidate: LiveRuntime,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
    ) -> Result<ActivationAck, PersistentActivationError> {
        self.activate_settled_candidate_with_artifacts(
            candidate,
            completed_migration_edges,
            None,
            BTreeMap::new(),
        )
    }

    fn activate_settled_candidate_with_artifacts(
        &mut self,
        candidate: LiveRuntime,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
        target_content_artifact_manifest: Option<ContentArtifactManifest>,
        content_artifacts: BTreeMap<ContentArtifactId, ContentArtifact>,
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
        candidate_image.content_artifact_manifest = target_content_artifact_manifest
            .unwrap_or_else(|| current.content_artifact_manifest.clone());
        let next_effect_work = EffectWorkIndex::from_items(candidate_image.outbox.clone());
        let mut batch = ActivationBatch::between(&current, &candidate_image)
            .map_err(|error| PersistentActivationError::Runtime(error.to_string()))?;
        batch.content_artifacts = content_artifacts;
        batch = batch.seal();
        let acknowledgement = self
            .persistence
            .activate(batch)
            .map_err(PersistentActivationError::Persistence)?;
        self.runtime = candidate;
        self.program_artifacts = ProgramArtifactLanes::default();
        self.effect_work = next_effect_work.clone();
        self.durable_effect_work = next_effect_work;
        self.pending_effect_durability.clear();
        self.ready_effect_actions.clear();
        self.generation = self.generation.saturating_add(1);
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
        let replacement_effect_work = EffectWorkIndex::from_items(restore.outbox.clone());
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
        self.program_artifacts = ProgramArtifactLanes::default();
        self.effect_work = replacement_effect_work.clone();
        self.durable_effect_work = replacement_effect_work;
        self.pending_effect_durability.clear();
        self.ready_effect_actions.clear();
        self.last_rebuild_derived_us = rebuild_derived_us;
        self.generation = self.generation.saturating_add(1);
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
        reject_unfinished_outbox(&current, "start over")?;
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
        self.program_artifacts = ProgramArtifactLanes::default();
        self.effect_work = EffectWorkIndex::default();
        self.durable_effect_work = EffectWorkIndex::default();
        self.pending_effect_durability.clear();
        self.ready_effect_actions.clear();
        self.last_rebuild_derived_us = rebuild_derived_us;
        self.generation = self.generation.saturating_add(1);
        Ok(PersistentPlanReset {
            mount,
            acknowledgement,
        })
    }

    pub fn shutdown(&self) -> Result<boon_persistence::ShutdownAck, PersistenceControlError> {
        self.persistence.shutdown()
    }
}

impl ProgramArtifactLanes {
    fn resolve_requests(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut compile_requests = Vec::new();
        let mut drive = ProgramArtifactDrive::default();
        loop {
            let requests = std::mem::take(&mut self.requests);
            if requests.is_empty() {
                break;
            }
            for request in requests {
                let Some(artifact_id) = request.artifact_id else {
                    compile_requests.push(request);
                    continue;
                };
                if let Some(content) = self.cache.get(&artifact_id).cloned() {
                    let result = ProgramArtifact::from_content_artifact(
                        request.compile.revision,
                        request.compile.capability_profile,
                        content,
                    );
                    let completed = self.complete_program(
                        runtime,
                        host,
                        source_sequence,
                        &request.session,
                        &request.request_id,
                        result,
                    )?;
                    drive.merge(completed);
                } else {
                    drive.merge(self.enqueue_load(runtime, host, source_sequence, request)?);
                }
            }
        }
        self.requests = compile_requests;
        Ok(drive)
    }

    fn enqueue_load(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        request: ProgramHostRequest,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let artifact_id = request
            .artifact_id
            .expect("artifact load request carries an artifact identity");
        let session_exists = self.load.pending_by_session.contains_key(&request.session)
            || self
                .load
                .in_flight
                .as_ref()
                .is_some_and(|flight| flight.waiters.contains_key(&request.session));
        if !session_exists && self.load.session_count() >= MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS {
            return self.complete_program(
                runtime,
                host,
                source_sequence,
                &request.session,
                &request.request_id,
                Err(ProgramDiagnostic::artifact(
                    request.compile.revision,
                    format!(
                        "program artifact load has {} pending sessions, limit is {MAX_PROGRAM_ARTIFACT_LOAD_SESSIONS}",
                        self.load.session_count()
                    ),
                )),
            );
        }

        let pending = PendingProgramArtifactLoad {
            candidate_sequence: self.load.next_sequence(),
            mount_epoch: self.load.mount_epoch,
            queue_depth: self
                .load
                .session_count()
                .saturating_add(1)
                .try_into()
                .unwrap_or(u32::MAX),
            queued_at: Instant::now(),
            request,
        };
        let joins_in_flight = self.load.in_flight.as_ref().is_some_and(|flight| {
            flight.mount_epoch == pending.mount_epoch && flight.artifact_id == artifact_id
        });
        if joins_in_flight {
            self.load
                .pending_by_session
                .remove(&pending.request.session);
            self.load
                .in_flight
                .as_mut()
                .expect("joining artifact load has an active flight")
                .waiters
                .insert(pending.request.session.clone(), pending);
        } else {
            self.load
                .pending_by_session
                .insert(pending.request.session.clone(), pending);
        }
        let mut drive = self.drive_load(runtime, host, source_sequence)?;
        drive.poll_required = true;
        Ok(drive)
    }

    fn drive_load(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        if self.load.in_flight.is_some() {
            return Ok(ProgramArtifactDrive::default());
        }
        let Some(session) = self
            .load
            .pending_by_session
            .iter()
            .min_by_key(|(_, candidate)| candidate.candidate_sequence)
            .map(|(session, _)| session.clone())
        else {
            return Ok(ProgramArtifactDrive::default());
        };
        let first = self
            .load
            .pending_by_session
            .remove(&session)
            .expect("selected artifact load candidate exists");
        let artifact_id = first
            .request
            .artifact_id
            .expect("artifact load candidate carries an identity");
        let matching_sessions = self
            .load
            .pending_by_session
            .iter()
            .filter_map(|(session, candidate)| {
                (candidate.request.artifact_id == Some(artifact_id)).then_some(session.clone())
            })
            .collect::<Vec<_>>();
        let mut waiters = BTreeMap::from([(first.request.session.clone(), first)]);
        for session in matching_sessions {
            let candidate = self
                .load
                .pending_by_session
                .remove(&session)
                .expect("matching artifact load candidate exists");
            waiters.insert(session, candidate);
        }
        match runtime.try_load_content_artifact(artifact_id) {
            Ok(ticket) => {
                self.load.in_flight = Some(ProgramArtifactLoadFlight {
                    ticket,
                    mount_epoch: self.load.mount_epoch,
                    artifact_id,
                    waiters,
                    started_at: Instant::now(),
                });
                Ok(ProgramArtifactDrive {
                    poll_required: true,
                    ..ProgramArtifactDrive::default()
                })
            }
            Err(ContentArtifactLoadEnqueueError::Backpressure(_)) => {
                self.load.pending_by_session.extend(
                    waiters
                        .into_values()
                        .map(|candidate| (candidate.request.session.clone(), candidate)),
                );
                Ok(ProgramArtifactDrive {
                    poll_required: true,
                    ..ProgramArtifactDrive::default()
                })
            }
            Err(ContentArtifactLoadEnqueueError::Closed(_)) => {
                let mut drive = ProgramArtifactDrive::default();
                for pending in waiters.into_values() {
                    drive.merge(self.complete_program(
                        runtime,
                        host,
                        source_sequence,
                        &pending.request.session,
                        &pending.request.request_id,
                        Err(ProgramDiagnostic::artifact(
                            pending.request.compile.revision,
                            "persistence coordinator closed before loading the program artifact",
                        )),
                    )?);
                }
                Ok(drive)
            }
        }
    }

    fn poll_loads(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut drive = ProgramArtifactDrive::default();
        for completion in runtime.take_content_artifact_load_completions() {
            if self
                .load
                .in_flight
                .as_ref()
                .is_none_or(|flight| flight.ticket != completion.ticket)
            {
                continue;
            }
            let flight = self
                .load
                .in_flight
                .take()
                .expect("matching artifact load flight exists");
            if flight.mount_epoch != self.load.mount_epoch || completion.id != flight.artifact_id {
                for pending in flight.waiters.into_values() {
                    drive.observations.push(ProgramArtifactLaneObservation {
                        lane: ProgramArtifactLaneKind::Load,
                        request_id: pending.request.request_id.0,
                        revision: pending.request.compile.revision,
                        queue_depth: pending.queue_depth,
                        queue_wait_us: duration_us(
                            flight
                                .started_at
                                .saturating_duration_since(pending.queued_at),
                        ),
                        worker_us: duration_us(flight.started_at.elapsed()),
                        apply_us: 0,
                        end_to_end_us: duration_us(pending.queued_at.elapsed()),
                        outcome: ProgramArtifactLaneOutcome::StaleRejected,
                    });
                }
                continue;
            }
            if let Ok(Some(content)) = &completion.result {
                self.cache.insert(content.id, content.clone());
            }
            for pending in flight.waiters.into_values() {
                let apply_started = Instant::now();
                let result = match &completion.result {
                    Ok(Some(content)) => ProgramArtifact::from_content_artifact(
                        pending.request.compile.revision,
                        pending.request.compile.capability_profile,
                        content.clone(),
                    ),
                    Ok(None) => Err(ProgramDiagnostic::artifact(
                        pending.request.compile.revision,
                        format!(
                            "immutable program artifact {} is missing",
                            flight.artifact_id
                        ),
                    )),
                    Err(error) => Err(ProgramDiagnostic::artifact(
                        pending.request.compile.revision,
                        error.to_string(),
                    )),
                };
                let failed = result.is_err();
                let mut completed = self.complete_program(
                    runtime,
                    host,
                    source_sequence,
                    &pending.request.session,
                    &pending.request.request_id,
                    result,
                )?;
                let completion = completed
                    .completion
                    .as_ref()
                    .expect("program completion reports its disposition");
                let outcome = program_artifact_lane_outcome(completion, failed);
                completed.observations.push(ProgramArtifactLaneObservation {
                    lane: ProgramArtifactLaneKind::Load,
                    request_id: pending.request.request_id.0,
                    revision: pending.request.compile.revision,
                    queue_depth: pending.queue_depth,
                    queue_wait_us: duration_us(
                        flight
                            .started_at
                            .saturating_duration_since(pending.queued_at),
                    ),
                    worker_us: duration_us(flight.started_at.elapsed()),
                    apply_us: duration_us(apply_started.elapsed()),
                    end_to_end_us: duration_us(pending.queued_at.elapsed()),
                    outcome,
                });
                drive.merge(completed);
            }
        }
        drive.merge(self.drive_load(runtime, host, source_sequence)?);
        if drive.changed {
            drive.merge(self.resolve_requests(runtime, host, source_sequence)?);
        }
        drive.poll_required |= self.load.has_pending();
        Ok(drive)
    }

    fn complete_program(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let request_is_current = host.request_is_current(session, request_id);
        let artifact_ownership = host.request_artifact_ownership(session, request_id);
        let artifact_load = host.request_is_artifact_load(session, request_id);
        let (observed, mut drive) =
            if request_is_current && let Some(ownership) = artifact_ownership {
                match result {
                    Ok(artifact) => {
                        let activate_before_store =
                            ownership.retention == ContentArtifactRetention::Replaceable;
                        let pending = self.pending_store(
                            session,
                            request_id,
                            artifact.clone(),
                            ownership,
                            activate_before_store,
                        );
                        if !activate_before_store {
                            self.enqueue_store(runtime, host, source_sequence, pending)?
                        } else {
                            let (activated, mut activated_drive) = self.finish_host_completion(
                                runtime,
                                host,
                                source_sequence,
                                session,
                                request_id,
                                Ok(artifact),
                                false,
                                true,
                            )?;
                            if !matches!(
                                activated.completion,
                                ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
                                    ProgramCompletion::Activated { .. }
                                ))
                            ) {
                                (activated, activated_drive)
                            } else {
                                let (queued, queued_drive) =
                                    self.enqueue_store(runtime, host, source_sequence, pending)?;
                                activated_drive.merge(queued_drive);
                                let completion = if matches!(
                                    queued.completion,
                                    ProgramCompletionObservation::ArtifactStorePending { .. }
                                ) {
                                    activated.completion
                                } else {
                                    queued.completion
                                };
                                (
                                    ObservedProgramCompletion {
                                        changed: activated.changed || queued.changed,
                                        completion,
                                    },
                                    activated_drive,
                                )
                            }
                        }
                    }
                    Err(diagnostic) => self.finish_host_completion(
                        runtime,
                        host,
                        source_sequence,
                        session,
                        request_id,
                        Err(diagnostic),
                        false,
                        false,
                    )?,
                }
            } else {
                self.finish_host_completion(
                    runtime,
                    host,
                    source_sequence,
                    session,
                    request_id,
                    result,
                    artifact_load,
                    false,
                )?
            };
        drive.changed |= observed.changed;
        drive.completion = Some(observed.completion);
        Ok(drive)
    }

    fn finish_host_completion(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
        artifact_load: bool,
        lifecycle_already_dispatched: bool,
    ) -> Result<(ObservedProgramCompletion, ProgramArtifactDrive), PersistentDispatchError> {
        let (completion, update) = host.complete(session, request_id, result);
        let bootstrap = update.bootstrap;
        let lifecycle = if artifact_load || lifecycle_already_dispatched {
            None
        } else {
            match &completion {
                ProgramHostCompletion::Program(ProgramCompletion::Activated { .. }) => host
                    .active_artifact(session)
                    .map(|artifact| ("compiled", compiled_program_payload(artifact, bootstrap))),
                ProgramHostCompletion::Program(ProgramCompletion::Rejected { diagnostic }) => {
                    Some(("rejected", rejected_program_payload(diagnostic)))
                }
                ProgramHostCompletion::Program(ProgramCompletion::Stale { .. })
                | ProgramHostCompletion::Superseded { .. }
                | ProgramHostCompletion::Removed { .. } => None,
            }
        };
        let program_state_changed = matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { .. })
        );
        let mut drive = ProgramArtifactDrive {
            changed: program_state_changed,
            ..ProgramArtifactDrive::default()
        };
        self.apply_host_update(runtime, host, source_sequence, update, &mut drive)?;
        if let Some((intent, payload)) = lifecycle {
            for path in host.lifecycle_source_paths(session, intent) {
                self.dispatch_lifecycle(
                    runtime,
                    host,
                    source_sequence,
                    &path,
                    payload.clone(),
                    None,
                    &mut drive,
                )?;
            }
        }
        let observed = ObservedProgramCompletion {
            changed: drive.changed,
            completion: ProgramCompletionObservation::Host(completion),
        };
        Ok((observed, drive))
    }

    fn apply_host_update(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        update: ProgramHostUpdate,
        drive: &mut ProgramArtifactDrive,
    ) -> Result<(), PersistentDispatchError> {
        drive.changed |= !update.patches.is_empty();
        drive.patches.extend(update.patches);
        self.requests.extend(update.requests);
        for rejection in update.rejections {
            self.dispatch_rejection(runtime, host, source_sequence, rejection, drive)?;
        }
        Ok(())
    }

    fn dispatch_rejection(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        rejection: ProgramRejection,
        drive: &mut ProgramArtifactDrive,
    ) -> Result<(), PersistentDispatchError> {
        let payload = rejected_program_payload(&rejection.diagnostic);
        for path in host.lifecycle_source_paths(&rejection.session, "rejected") {
            self.dispatch_lifecycle(
                runtime,
                host,
                source_sequence,
                &path,
                payload.clone(),
                None,
                drive,
            )?;
        }
        Ok(())
    }

    fn dispatch_lifecycle(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        path: &str,
        payload: SourcePayload,
        content_changes: Option<Vec<DurableContentArtifactChange>>,
        drive: &mut ProgramArtifactDrive,
    ) -> Result<Option<DurabilityTicket>, PersistentDispatchError> {
        let next_sequence = source_sequence.saturating_add(1);
        if host.owns_source_route(path) {
            if content_changes.is_some() {
                return Err(PersistentDispatchError::Runtime(
                    "program lifecycle route resolved inside the restricted child runtime"
                        .to_owned(),
                ));
            }
            let (turn, patches) = host
                .dispatch(next_sequence, path, None, payload)
                .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
            let dispatched_sequence = turn.source_sequence.ok_or_else(|| {
                PersistentDispatchError::Runtime(format!(
                    "child lifecycle dispatch `{path}` produced no source sequence"
                ))
            })?;
            *source_sequence = dispatched_sequence;
            drive.changed |= !patches.is_empty();
            drive.patches.extend(patches);
            drive.turns.push(ProgramArtifactTurn {
                kind: ProgramArtifactTurnKind::Child,
                source_path: path.to_owned(),
                turn,
            });
            return Ok(None);
        }

        let event = runtime
            .runtime()
            .source_event(next_sequence, path, None, payload)
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        let (turn, ticket) = match content_changes {
            Some(changes) => {
                let (turn, ticket) =
                    runtime.dispatch_with_content_artifact_changes(event, changes)?;
                (turn, Some(ticket))
            }
            None => (runtime.dispatch(event)?, None),
        };
        let dispatched_sequence = turn.source_sequence.ok_or_else(|| {
            PersistentDispatchError::Runtime(format!(
                "program lifecycle dispatch `{path}` produced no source sequence"
            ))
        })?;
        *source_sequence = dispatched_sequence;
        let parent_patches = turn.document_patches.clone();
        let parent = runtime
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| PersistentDispatchError::Runtime(error.to_string()))?;
        let update = host.reconcile_with_parent_patches(parent, parent_patches);
        drive.turns.push(ProgramArtifactTurn {
            kind: ProgramArtifactTurnKind::Parent,
            source_path: path.to_owned(),
            turn,
        });
        self.apply_host_update(runtime, host, source_sequence, update, drive)?;
        Ok(ticket)
    }

    fn pending_store(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        artifact: ProgramArtifact,
        ownership: ProgramArtifactOwnership,
        activated_before_store: bool,
    ) -> PendingProgramArtifactStore {
        let queued_at = Instant::now();
        let store_after = if ownership.retention == ContentArtifactRetention::Replaceable {
            queued_at + REPLACEABLE_ARTIFACT_QUIET_PERIOD
        } else {
            queued_at
        };
        PendingProgramArtifactStore {
            candidate_sequence: self.store.next_sequence(),
            mount_epoch: self.store.mount_epoch,
            session: session.clone(),
            request_id: request_id.clone(),
            artifact,
            ownership,
            activated_before_store,
            queued_at,
            store_after,
            queue_depth: self
                .store
                .session_count()
                .saturating_add(1)
                .try_into()
                .unwrap_or(u32::MAX),
            queue_wait_us: 0,
            worker_us: 0,
        }
    }

    fn enqueue_store(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        pending: PendingProgramArtifactStore,
    ) -> Result<(ObservedProgramCompletion, ProgramArtifactDrive), PersistentDispatchError> {
        let session_exists = self.store.pending_by_session.contains_key(&pending.session)
            || self
                .store
                .in_flight
                .as_ref()
                .is_some_and(|flight| flight.waiters.contains_key(&pending.session))
            || self
                .store
                .ready_for_authority
                .values()
                .any(|candidate| candidate.session == pending.session)
            || self
                .store
                .awaiting_durability
                .values()
                .any(|activation| activation.pending.session == pending.session);
        if !session_exists && self.store.session_count() >= MAX_PROGRAM_ARTIFACT_STORE_SESSIONS {
            return self.finish_host_completion(
                runtime,
                host,
                source_sequence,
                &pending.session,
                &pending.request_id,
                Err(ProgramDiagnostic::artifact(
                    pending.artifact.revision(),
                    format!(
                        "program artifact store has {} pending sessions, limit is {MAX_PROGRAM_ARTIFACT_STORE_SESSIONS}",
                        self.store.session_count()
                    ),
                )),
                false,
                false,
            );
        }

        let artifact_id = pending.artifact.id();
        let joins_in_flight = self.store.in_flight.as_ref().is_some_and(|flight| {
            flight.mount_epoch == pending.mount_epoch && flight.artifact_id == artifact_id
        });
        let replaced_pending_bytes = self
            .store
            .pending_by_session
            .get(&pending.session)
            .map_or(0, |replaced| replaced.artifact.content_bytes_len());
        let replaced_flight_bytes = joins_in_flight
            .then(|| {
                self.store
                    .in_flight
                    .as_ref()?
                    .waiters
                    .get(&pending.session)
                    .map(|replaced| replaced.artifact.content_bytes_len())
            })
            .flatten()
            .unwrap_or(0);
        let projected_bytes = self
            .store
            .queued_bytes
            .saturating_sub(replaced_pending_bytes)
            .saturating_sub(replaced_flight_bytes)
            .saturating_add(pending.artifact.content_bytes_len());
        if projected_bytes > MAX_PROGRAM_ARTIFACT_STORE_BYTES {
            return self.finish_host_completion(
                runtime,
                host,
                source_sequence,
                &pending.session,
                &pending.request_id,
                Err(ProgramDiagnostic::artifact(
                    pending.artifact.revision(),
                    format!(
                        "program artifact store would retain {projected_bytes} queued bytes, limit is {MAX_PROGRAM_ARTIFACT_STORE_BYTES}"
                    ),
                )),
                false,
                false,
            );
        }

        let completion = ProgramCompletionObservation::ArtifactStorePending {
            session: pending.session.clone(),
            request_id: pending.request_id.clone(),
            artifact_id,
        };
        if joins_in_flight {
            self.store.pending_by_session.remove(&pending.session);
            self.store
                .in_flight
                .as_mut()
                .expect("joining candidate has an artifact flight")
                .waiters
                .insert(pending.session.clone(), pending);
        } else {
            self.store
                .pending_by_session
                .insert(pending.session.clone(), pending);
        }
        self.store.queued_bytes = projected_bytes;
        let mut drive = self.drive_store(runtime, host, source_sequence)?;
        drive.poll_required = true;
        Ok((
            ObservedProgramCompletion {
                changed: drive.changed,
                completion,
            },
            drive,
        ))
    }

    fn drive_store(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        if self.store.in_flight.is_some()
            || !self.store.ready_for_authority.is_empty()
            || !self.store.awaiting_durability.is_empty()
        {
            return Ok(ProgramArtifactDrive::default());
        }
        let now = Instant::now();
        let Some(session) = self
            .store
            .pending_by_session
            .iter()
            .filter(|(_, candidate)| candidate.store_after <= now)
            .min_by_key(|(_, candidate)| candidate.candidate_sequence)
            .map(|(session, _)| session.clone())
        else {
            return Ok(ProgramArtifactDrive::default());
        };
        let first = self
            .store
            .pending_by_session
            .remove(&session)
            .expect("selected artifact candidate exists");
        let artifact_id = first.artifact.id();
        let matching_sessions = self
            .store
            .pending_by_session
            .iter()
            .filter_map(|(session, candidate)| {
                (candidate.artifact.id() == artifact_id).then_some(session.clone())
            })
            .collect::<Vec<_>>();
        let mut waiters = BTreeMap::from([(first.session.clone(), first)]);
        for session in matching_sessions {
            let candidate = self
                .store
                .pending_by_session
                .remove(&session)
                .expect("matching artifact candidate exists");
            waiters.insert(session, candidate);
        }
        let content = waiters
            .values()
            .next()
            .expect("artifact flight has at least one waiter")
            .artifact
            .to_content_artifact();
        match runtime.try_put_content_artifact(content) {
            Ok(ticket) => {
                self.store.in_flight = Some(ProgramArtifactStoreFlight {
                    ticket,
                    mount_epoch: self.store.mount_epoch,
                    artifact_id,
                    waiters,
                    started_at: Instant::now(),
                });
                Ok(ProgramArtifactDrive {
                    poll_required: true,
                    ..ProgramArtifactDrive::default()
                })
            }
            Err(ContentArtifactStoreEnqueueError::Backpressure(_)) => {
                self.store.pending_by_session.extend(waiters);
                Ok(ProgramArtifactDrive {
                    poll_required: true,
                    ..ProgramArtifactDrive::default()
                })
            }
            Err(ContentArtifactStoreEnqueueError::Closed(_)) => {
                let mut drive = ProgramArtifactDrive::default();
                for waiter in waiters.into_values() {
                    self.store.remove_waiter_bytes(&waiter);
                    let revision = waiter.artifact.revision();
                    let completed = self.finish_store(
                        runtime,
                        host,
                        source_sequence,
                        waiter,
                        Err(ProgramDiagnostic::artifact(
                            revision,
                            "persistence coordinator closed before storing the program artifact",
                        )),
                        false,
                    )?;
                    drive.merge(completed);
                }
                Ok(drive)
            }
        }
    }

    fn poll(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut drive = self.poll_loads(runtime, host, source_sequence)?;
        drive.merge(self.poll_durability(runtime, host, source_sequence)?);
        drive.merge(self.drive_authority(runtime, host, source_sequence)?);
        for completion in runtime.take_content_artifact_store_completions() {
            if self
                .store
                .in_flight
                .as_ref()
                .is_none_or(|flight| flight.ticket != completion.ticket)
            {
                continue;
            }
            let flight = self
                .store
                .in_flight
                .take()
                .expect("matching artifact flight exists");
            let acknowledged = completion.result.and_then(|ack| {
                (ack.id == flight.artifact_id).then_some(()).ok_or_else(|| {
                    boon_persistence::StoreError::InvalidContentArtifact(
                        "persistence acknowledged a different program artifact".to_owned(),
                    )
                })
            });
            let store_worker_us = duration_us(flight.started_at.elapsed());
            if acknowledged.is_ok()
                && flight.mount_epoch == self.store.mount_epoch
                && let Some(artifact) = flight.waiters.values().next()
            {
                self.cache
                    .insert(flight.artifact_id, artifact.artifact.to_content_artifact());
            }
            for mut waiter in flight.waiters.into_values() {
                waiter.queue_wait_us = duration_us(
                    flight
                        .started_at
                        .saturating_duration_since(waiter.queued_at),
                );
                waiter.worker_us = store_worker_us;
                if flight.mount_epoch != self.store.mount_epoch {
                    self.store.remove_waiter_bytes(&waiter);
                    drive.observations.push(ProgramArtifactLaneObservation {
                        lane: ProgramArtifactLaneKind::Store,
                        request_id: waiter.request_id.0,
                        revision: waiter.artifact.revision(),
                        queue_depth: waiter.queue_depth,
                        queue_wait_us: waiter.queue_wait_us,
                        worker_us: waiter.worker_us,
                        apply_us: 0,
                        end_to_end_us: duration_us(waiter.queued_at.elapsed()),
                        outcome: ProgramArtifactLaneOutcome::StaleRejected,
                    });
                    continue;
                }
                match &acknowledged {
                    Ok(()) => {
                        self.store
                            .ready_for_authority
                            .insert(waiter.candidate_sequence, waiter);
                    }
                    Err(error) => {
                        self.store.remove_waiter_bytes(&waiter);
                        let diagnostic = ProgramDiagnostic::artifact(
                            waiter.artifact.revision(),
                            error.to_string(),
                        );
                        drive.merge(self.finish_store(
                            runtime,
                            host,
                            source_sequence,
                            waiter,
                            Err(diagnostic),
                            false,
                        )?);
                    }
                }
            }
        }
        drive.merge(self.drive_authority(runtime, host, source_sequence)?);
        drive.merge(self.poll_durability(runtime, host, source_sequence)?);
        drive.merge(self.drive_store(runtime, host, source_sequence)?);
        if drive.changed {
            drive.merge(self.resolve_requests(runtime, host, source_sequence)?);
        }
        drive.poll_required |= self.store.has_pending() || self.load.has_pending();
        Ok(drive)
    }

    fn drive_authority(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let mut drive = ProgramArtifactDrive::default();
        loop {
            let Some(candidate_sequence) = self
                .store
                .ready_for_authority
                .first_key_value()
                .map(|(sequence, _)| *sequence)
            else {
                return Ok(drive);
            };
            let pending = self
                .store
                .ready_for_authority
                .remove(&candidate_sequence)
                .expect("selected durable artifact candidate exists");
            if !host.request_is_current(&pending.session, &pending.request_id) {
                self.store.remove_waiter_bytes(&pending);
                drive.merge(self.finish_store(
                    runtime,
                    host,
                    source_sequence,
                    pending,
                    Ok(()),
                    false,
                )?);
                continue;
            }

            let paths = host.lifecycle_source_paths(&pending.session, "compiled");
            if paths.len() != 1 {
                self.store.remove_waiter_bytes(&pending);
                let revision = pending.artifact.revision();
                let session = pending.session.clone();
                let request_id = pending.request_id.clone();
                let (observed, completed) = self.finish_host_completion(
                    runtime,
                    host,
                    source_sequence,
                    &session,
                    &request_id,
                    Err(ProgramDiagnostic::artifact(
                        revision,
                        format!(
                            "retained embedded program requires exactly one compiled lifecycle route, found {}",
                            paths.len()
                        ),
                    )),
                    false,
                    false,
                )?;
                let _ = observed;
                drive.merge(completed);
                continue;
            }
            let change = match pending.ownership.retention {
                ContentArtifactRetention::Replaceable => {
                    DurableContentArtifactChange::SetReplaceable {
                        owner_id: pending.ownership.owner,
                        artifact_id: pending.artifact.id(),
                    }
                }
                ContentArtifactRetention::Immutable => {
                    DurableContentArtifactChange::InsertImmutable {
                        owner_id: pending.ownership.owner,
                        artifact_id: pending.artifact.id(),
                    }
                }
            };
            let payload = compiled_program_payload(&pending.artifact, false);
            match self.dispatch_lifecycle(
                runtime,
                host,
                source_sequence,
                &paths[0],
                payload,
                Some(vec![change]),
                &mut drive,
            ) {
                Ok(Some(ticket)) => {
                    self.store.awaiting_durability.insert(
                        ticket.turn_sequence,
                        PendingProgramArtifactActivation { ticket, pending },
                    );
                }
                Ok(None) => unreachable!("durable program lifecycle dispatch returns a ticket"),
                Err(
                    PersistentDispatchError::Backpressure(_)
                    | PersistentDispatchError::PersistenceAdmissionFailed { .. },
                ) => {
                    self.store
                        .ready_for_authority
                        .insert(candidate_sequence, pending);
                    drive.poll_required = true;
                    return Ok(drive);
                }
                Err(error) => {
                    self.store.remove_waiter_bytes(&pending);
                    let revision = pending.artifact.revision();
                    let session = pending.session.clone();
                    let request_id = pending.request_id.clone();
                    let (observed, completed) = self.finish_host_completion(
                        runtime,
                        host,
                        source_sequence,
                        &session,
                        &request_id,
                        Err(ProgramDiagnostic::artifact(revision, error.to_string())),
                        false,
                        false,
                    )?;
                    let _ = observed;
                    drive.merge(completed);
                }
            }
        }
    }

    fn poll_durability(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let acknowledged = self
            .store
            .awaiting_durability
            .iter()
            .filter_map(|(sequence, activation)| {
                runtime
                    .durability_ticket_is_acknowledged(activation.ticket)
                    .then_some(*sequence)
            })
            .collect::<Vec<_>>();
        let mut drive = ProgramArtifactDrive::default();
        for sequence in acknowledged {
            let activation = self
                .store
                .awaiting_durability
                .remove(&sequence)
                .expect("acknowledged program artifact activation exists");
            self.store.remove_waiter_bytes(&activation.pending);
            drive.merge(self.finish_store(
                runtime,
                host,
                source_sequence,
                activation.pending,
                Ok(()),
                true,
            )?);
        }
        Ok(drive)
    }

    fn finish_store(
        &mut self,
        runtime: &mut PersistentRuntime,
        host: &mut ProgramDocumentHost,
        source_sequence: &mut u64,
        pending: PendingProgramArtifactStore,
        result: Result<(), ProgramDiagnostic>,
        authority_committed: bool,
    ) -> Result<ProgramArtifactDrive, PersistentDispatchError> {
        let request_id = pending.request_id.0.clone();
        let revision = pending.artifact.revision();
        let failed = result.is_err();
        let apply_started = Instant::now();
        let (observed, mut drive) = if !pending.activated_before_store || result.is_err() {
            self.finish_host_completion(
                runtime,
                host,
                source_sequence,
                &pending.session,
                &pending.request_id,
                result.map(|()| pending.artifact),
                false,
                authority_committed,
            )?
        } else if !authority_committed {
            (
                ObservedProgramCompletion {
                    changed: false,
                    completion: ProgramCompletionObservation::Host(
                        ProgramHostCompletion::Superseded {
                            session: pending.session,
                            request_id: pending.request_id,
                        },
                    ),
                },
                ProgramArtifactDrive::default(),
            )
        } else {
            (
                ObservedProgramCompletion {
                    changed: false,
                    completion: ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
                        ProgramCompletion::Activated { revision },
                    )),
                },
                ProgramArtifactDrive::default(),
            )
        };
        drive.observations.push(ProgramArtifactLaneObservation {
            lane: ProgramArtifactLaneKind::Store,
            request_id,
            revision,
            queue_depth: pending.queue_depth,
            queue_wait_us: pending.queue_wait_us,
            worker_us: pending.worker_us,
            apply_us: duration_us(apply_started.elapsed()),
            end_to_end_us: duration_us(pending.queued_at.elapsed()),
            outcome: program_artifact_lane_outcome(&observed.completion, failed),
        });
        drive.changed |= observed.changed;
        drive.completion = Some(observed.completion);
        Ok(drive)
    }
}

fn compiled_program_payload(artifact: &ProgramArtifact, bootstrap: bool) -> SourcePayload {
    let mut payload = SourcePayload {
        text: Some(artifact.source_digest().to_owned()),
        ..SourcePayload::default()
    };
    for (name, value) in [
        ("revision", artifact.revision().to_string()),
        ("source_digest", artifact.source_digest().to_owned()),
        ("compiler", artifact.compiler_id().to_owned()),
        ("target", artifact.target_profile_id().to_owned()),
        (
            "capability_profile",
            artifact.capability_profile_id().to_owned(),
        ),
        ("artifact_id", artifact.id_text()),
        ("plan_digest", artifact.plan_digest().to_owned()),
    ] {
        payload.fields.insert(name.to_owned(), Value::Text(value));
    }
    payload
        .fields
        .insert("bootstrap".to_owned(), Value::Bool(bootstrap));
    payload
}

fn rejected_program_payload(diagnostic: &ProgramDiagnostic) -> SourcePayload {
    let mut payload = SourcePayload {
        text: Some(diagnostic.message.clone()),
        ..SourcePayload::default()
    };
    for (name, value) in [
        ("revision", diagnostic.revision.to_string()),
        ("source_path", diagnostic.source_path.clone()),
        ("line", diagnostic.line.to_string()),
        ("column", diagnostic.column.to_string()),
        ("diagnostic", diagnostic.message.clone()),
    ] {
        payload.fields.insert(name.to_owned(), Value::Text(value));
    }
    payload
}

fn program_artifact_lane_outcome(
    completion: &ProgramCompletionObservation,
    failed: bool,
) -> ProgramArtifactLaneOutcome {
    if failed {
        return ProgramArtifactLaneOutcome::Failed;
    }
    match completion {
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Stale { .. },
        ))
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded { .. })
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Removed { .. }) => {
            ProgramArtifactLaneOutcome::StaleRejected
        }
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Rejected { .. },
        )) => ProgramArtifactLaneOutcome::Failed,
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Activated { .. },
        ))
        | ProgramCompletionObservation::ArtifactStorePending { .. } => {
            ProgramArtifactLaneOutcome::Applied
        }
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
        CheckpointBatch, ContentArtifactOwnerId, InMemoryDriver, PersistenceCommand,
        PersistenceResult, ShutdownAck, StoreError,
    };
    use boon_plan_executor::SourcePayload;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};

    fn number(value: i64) -> Value {
        Value::integer(value).unwrap()
    }

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
        load_count: Arc<AtomicUsize>,
    }

    impl PersistenceDriver for SharedPersistenceDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(command, PersistenceCommand::Load(_)) {
                self.load_count.fetch_add(1, Ordering::AcqRel);
            }
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

    struct BlockingCommitDriver {
        inner: InMemoryDriver,
        entered: Arc<AtomicBool>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl PersistenceDriver for BlockingCommitDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(command, PersistenceCommand::Commit(_)) {
                self.entered.store(true, Ordering::Release);
                let (released, changed) = &*self.gate;
                let mut released = released
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*released {
                    released = changed
                        .wait(released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
            self.inner.execute(command)
        }
    }

    fn release_commit_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
        let (released, changed) = &**gate;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        changed.notify_all();
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
        assert_eq!(runtime.generation(), 1);
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Fresh
        );

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
    fn ordinary_dispatch_and_effect_schedule_query_do_not_load_durable_state() {
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-counter-no-effect-load.bn",
            include_str!("../../../examples/counter.bn"),
            boon_plan::ApplicationIdentity::new(
                "dev.boon.persistent-no-effect-load",
                "test",
                "local",
            ),
        )
        .unwrap();
        let storage = SharedPersistenceDriver::default();
        let load_count = Arc::clone(&storage.load_count);
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            compiled.shared_machine_plan(),
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let startup_loads = load_count.load(Ordering::Acquire);

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
        assert!(!runtime.has_effect_work());
        assert!(runtime.effect_work_items().unwrap().is_empty());
        assert_eq!(load_count.load(Ordering::Acquire), startup_loads);

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
            number(1)
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
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
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
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
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
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
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
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
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
        let load_count = Arc::clone(&storage.load_count);
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Fresh
        );
        assert_eq!(
            runtime.runtime.root_value_current("store.result").unwrap(),
            Value::Text("not written".to_owned())
        );
        let startup_loads = load_count.load(Ordering::Acquire);
        assert!(!runtime.has_effect_work());
        assert!(runtime.effect_work_items().unwrap().is_empty());
        assert_eq!(load_count.load(Ordering::Acquire), startup_loads);

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
        assert!(runtime.has_effect_work());
        assert_eq!(load_count.load(Ordering::Acquire), startup_loads);
        assert!(matches!(
            runtime.clear_authority_path("store.result", SessionOptions::default()),
            Err(PersistentActivationError::Runtime(detail))
                if detail.contains("effects are unfinished")
        ));
        let before_claim_loads = load_count.load(Ordering::Acquire);

        let claimed = runtime
            .claim_effect_for_dispatch(pending[0].item.item_id)
            .unwrap();
        assert!(matches!(
            claimed.state,
            DurableOutboxState::Dispatching { attempt: 1 }
        ));
        assert_eq!(load_count.load(Ordering::Acquire), before_claim_loads);
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
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
        let restart_loads = load_count.load(Ordering::Acquire);
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
        assert!(restarted.has_effect_work());
        assert_eq!(load_count.load(Ordering::Acquire), restart_loads);

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
        assert!(!restarted.has_effect_work());
        assert_eq!(load_count.load(Ordering::Acquire), restart_loads);
        let durable = restarted.load_durable_image().unwrap().unwrap();
        assert_eq!(load_count.load(Ordering::Acquire), restart_loads + 1);
        assert!(
            durable.outbox.is_empty(),
            "the atomically persisted effect outcome must consume its outbox obligation"
        );
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

        let mut completion = None;
        loop {
            if let Some(turn) = runtime.poll_effect_worker(&mut worker).unwrap() {
                completion = Some(turn);
            }
            if root.join("output.bin").is_file()
                && runtime.runtime.root_value_current("store.result").unwrap()
                    == Value::Text("output.bin".to_owned())
                && !runtime.has_effect_work()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background effect did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        assert_eq!(completion.unwrap().source_sequence, None);
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
    fn blocked_persistence_neither_blocks_dispatch_nor_releases_host_effects() {
        let identity = boon_plan::ApplicationIdentity::new(
            "dev.boon.persistent-effect-durability-gate",
            "test",
            "local",
        );
        let root = std::env::temp_dir().join(format!(
            "boon-persistent-effect-durability-gate-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let entered = Arc::new(AtomicBool::new(false));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let driver = BlockingCommitDriver {
            inner: InMemoryDriver::default(),
            entered: Arc::clone(&entered),
            gate: Arc::clone(&gate),
        };
        let (mut runtime, _) = PersistentRuntime::from_shared_machine_plan(
            effect_plan(identity),
            SessionOptions::default(),
            driver,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let safety_gate = Arc::clone(&gate);
        let safety_release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(250));
            release_commit_gate(&safety_gate);
        });

        let started = std::time::Instant::now();
        runtime.dispatch(effect_source_event(&runtime, 1)).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(50),
            "dispatch waited for the blocked persistence driver"
        );
        let entered_deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !entered.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < entered_deadline,
                "persistence worker did not enter the blocked commit"
            );
            std::thread::yield_now();
        }
        let mut worker = HostEffectWorker::start(FileEffectDriver::new(&root).unwrap()).unwrap();
        assert!(runtime.poll_effect_worker(&mut worker).unwrap().is_none());
        assert!(!root.join("output.bin").exists());

        release_commit_gate(&gate);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while runtime.has_effect_work() || worker.is_busy() {
            let _ = runtime.poll_effect_worker(&mut worker).unwrap();
            assert!(
                std::time::Instant::now() < deadline,
                "durable effect did not settle after releasing persistence"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            std::fs::read(root.join("output.bin")).unwrap(),
            [1, 2, 3, 4]
        );

        safety_release.join().unwrap();
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
                .any(|value| value == &number(1))
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
        assert_eq!(runtime.generation(), 1);
        let before = runtime.load_durable_image().unwrap().unwrap();
        assert_eq!(before.through_turn_sequence, 1);
        assert!(!before.scalars.is_empty());

        let reset = runtime
            .start_over_machine_plan(Arc::clone(&plan), SessionOptions::default())
            .unwrap();
        assert_eq!(runtime.generation(), 2);
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
            number(0)
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
        assert_eq!(runtime.generation(), 2);
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
    fn start_over_rejects_unfinished_consequential_effects() {
        let identity = boon_plan::ApplicationIdentity::new(
            "dev.boon.persistent-start-over-effect",
            "test",
            "local",
        );
        let plan = effect_plan(identity);
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
        assert_eq!(runtime.generation(), 1);
        runtime.dispatch(effect_source_event(&runtime, 1)).unwrap();
        runtime.barrier().unwrap();
        let before = runtime.load_durable_image().unwrap().unwrap();
        assert!(
            before
                .outbox
                .values()
                .any(|item| !matches!(item.state, DurableOutboxState::Completed { .. }))
        );

        let error = match runtime.start_over_machine_plan(plan, SessionOptions::default()) {
            Ok(_) => panic!("start over must reject an unfinished effect"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("cannot start over while consequential effects are unfinished")
        );
        assert_eq!(runtime.load_durable_image().unwrap(), Some(before));
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
        assert_eq!(runtime.generation(), 1);
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            number(2)
        );

        let preview = runtime
            .preview_state_artifact(&artifact, SessionOptions::default())
            .unwrap();
        assert_eq!(preview.scalar_count, 1);
        assert!(preview.migration.is_none());
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            number(2),
            "preview must not mutate active authority"
        );

        let activation = runtime
            .activate_state_artifact(&artifact, SessionOptions::default())
            .unwrap();
        assert_eq!(runtime.generation(), 2);
        assert!(activation.acknowledgement.epoch > 0);
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            number(1)
        );
        runtime
            .clear_authority_path("store.count", SessionOptions::default())
            .unwrap();
        assert_eq!(runtime.generation(), 3);
        assert_eq!(
            runtime.runtime.root_value_current("store.count").unwrap(),
            number(0)
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

    #[test]
    fn state_artifact_moves_immutable_content_to_an_independent_store_atomically() {
        let source = include_str!("../../../examples/counter.bn");
        let compiled = LiveRuntime::from_source_with_identity(
            "persistent-transfer-source.bn",
            source,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-transfer", "test", "local"),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        let content = ContentArtifact::new(
            "application/vnd.boon.test-program",
            b"immutable child artifact".to_vec(),
        )
        .unwrap();
        let mut source_driver = InMemoryDriver::default();
        source_driver.seed(RestoreImage::empty(
            plan.application.identity.clone(),
            plan.persistence.schema_version,
            plan.persistence.schema_hash,
        ));
        assert!(matches!(
            source_driver.execute(boon_persistence::PersistenceCommand::PutContentArtifact(
                boon_persistence::PutContentArtifactRequest {
                    application: plan.application.identity.clone(),
                    artifact: content.clone(),
                },
            )),
            boon_persistence::PersistenceResult::ContentArtifactStored(Ok(_))
        ));
        let owner_id = ContentArtifactOwnerId([7; 32]);
        let binding = CheckpointBatch {
            application: plan.application.identity.clone(),
            schema_hash: plan.persistence.schema_hash,
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: Vec::new(),
            outbox_changes: Vec::new(),
            content_artifact_changes: vec![DurableContentArtifactChange::InsertImmutable {
                owner_id,
                artifact_id: content.id,
            }],
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            source_driver.execute(boon_persistence::PersistenceCommand::Commit(binding)),
            boon_persistence::PersistenceResult::Committed(Ok(_))
        ));
        let (mut source_runtime, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            source_driver,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        let increment = source_runtime
            .runtime()
            .source_event(
                1,
                "store.sources.increment_button.press",
                None,
                SourcePayload::default(),
            )
            .unwrap();
        source_runtime.dispatch(increment).unwrap();
        source_runtime.barrier().unwrap();
        let artifact = source_runtime.export_state_artifact().unwrap();

        let (mut destination, _) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            InMemoryDriver::default(),
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        destination
            .activate_state_artifact(&artifact, SessionOptions::default())
            .unwrap();
        assert_eq!(
            destination
                .runtime
                .root_value_current("store.count")
                .unwrap(),
            number(1)
        );
        assert_eq!(
            destination.load_content_artifact(content.id).unwrap(),
            Some(content.clone())
        );

        let authority_before = destination.load_durable_image().unwrap().unwrap();
        let mut corrupt = artifact;
        let payload = corrupt
            .windows(content.bytes.len())
            .position(|window| window == content.bytes)
            .expect("transfer contains immutable content payload");
        corrupt[payload] ^= 1;
        assert!(
            destination
                .activate_state_artifact(&corrupt, SessionOptions::default())
                .is_err()
        );
        assert_eq!(
            destination.load_durable_image().unwrap().unwrap(),
            authority_before
        );
        assert_eq!(
            destination.load_content_artifact(content.id).unwrap(),
            Some(content)
        );

        source_runtime.shutdown().unwrap();
        destination.shutdown().unwrap();
    }

    #[test]
    fn persistent_runtime_restores_bare_root_latest_without_storing_derived_fields() {
        let compiled = LiveRuntime::from_source_for_role_with_identity(
            "persistent-root-latest.bn",
            r#"
store: [
    pulse: SOURCE
    count:
        LATEST {
            0
            pulse |> THEN { count + 1 }
        }
    transient:
        LATEST {
            pulse |> THEN { count + 10 }
        }
    derived: count + 20
]
"#,
            boon_plan::ProgramRole::Server,
            boon_plan::ApplicationIdentity::new("dev.boon.persistent-root-latest", "test", "local"),
        )
        .unwrap();
        let plan = compiled.shared_machine_plan();
        assert_eq!(
            plan.persistence
                .memory
                .iter()
                .map(|memory| memory.semantic_path.as_str())
                .collect::<Vec<_>>(),
            ["store.count"]
        );
        let storage = SharedPersistenceDriver::default();
        let (mut runtime, startup) = PersistentRuntime::from_shared_machine_plan(
            Arc::clone(&plan),
            SessionOptions::default(),
            storage.clone(),
            PersistenceWorkerConfig {
                coalesce_delay: std::time::Duration::ZERO,
                ..PersistenceWorkerConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Fresh
        );
        let pulse = runtime
            .runtime()
            .source_event(1, "store.pulse", None, SourcePayload::default())
            .unwrap();
        runtime.dispatch(pulse).unwrap();
        runtime.barrier().unwrap();
        let durable = runtime.load_durable_image().unwrap().unwrap();
        assert_eq!(durable.scalars.len(), 1);
        assert_eq!(durable.through_turn_sequence, 1);
        runtime.shutdown().unwrap();

        let (mut restored, startup) = PersistentRuntime::from_shared_machine_plan(
            plan,
            SessionOptions::default(),
            storage,
            PersistenceWorkerConfig::default(),
        )
        .unwrap();
        assert_eq!(
            startup.disposition,
            PersistentRuntimeStartupDisposition::Restored
        );
        assert_eq!(startup.restore_image.scalars.len(), 1);
        assert_eq!(
            restored.runtime.root_value_current("store.count").unwrap(),
            number(1)
        );
        restored.shutdown().unwrap();
    }
}
