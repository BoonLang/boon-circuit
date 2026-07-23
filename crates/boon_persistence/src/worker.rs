use super::{
    ActivationAck, ActivationBatch, ApplicationTransfer, BarrierAck, BarrierRequest,
    CheckpointBatch, CommitAck, CompactAck, CompactRequest, ContentArtifact, ContentArtifactId,
    DurableChange, DurableContentArtifactChange, DurableOutboxChange, ExportApplicationRequest,
    InspectRequest, LoadContentArtifactRequest, PersistenceCommand, PersistenceDriver,
    PersistenceInspectorSnapshot, PersistenceResult, PutContentArtifactAck,
    PutContentArtifactRequest, ResetApplicationAck, ResetApplicationBatch, RestoreImage,
    RestoreRequest, ShutdownAck, ShutdownRequest, StoreError,
};
use boon_plan::ApplicationIdentity;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A complete authority turn accepted by the persistence coordinator.
///
/// Turns are the unit of admission and recovery. A turn is either returned to
/// the caller unchanged or accepted in full; the coordinator never admits a
/// prefix of its changes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuthorityTurn {
    pub turn_sequence: u64,
    pub changes: Vec<DurableChange>,
    pub outbox_changes: Vec<DurableOutboxChange>,
    #[serde(default)]
    pub content_artifact_changes: Vec<DurableContentArtifactChange>,
}

impl AuthorityTurn {
    pub fn new(turn_sequence: u64, changes: Vec<DurableChange>) -> Self {
        Self {
            turn_sequence,
            changes,
            outbox_changes: Vec::new(),
            content_artifact_changes: Vec::new(),
        }
    }

    pub fn with_outbox_changes(mut self, changes: Vec<DurableOutboxChange>) -> Self {
        self.outbox_changes = changes;
        self
    }

    pub fn with_content_artifact_changes(
        mut self,
        changes: Vec<DurableContentArtifactChange>,
    ) -> Self {
        self.content_artifact_changes = changes;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceWorkerConfig {
    /// Maximum number of complete accepted-but-not-durable turns.
    pub queue_capacity: usize,
    /// Maximum number of complete turns combined into one checkpoint.
    pub max_batch_turns: usize,
    /// Short worker-side collection window used to combine adjacent turns.
    pub coalesce_delay: Duration,
}

impl Default for PersistenceWorkerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 64,
            max_batch_turns: 64,
            coalesce_delay: Duration::from_millis(2),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceStartup {
    pub restore_image: RestoreImage,
    pub initialized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceWorkerStartError {
    InvalidConfig(&'static str),
    Spawn(String),
    Store(StoreError),
    WorkerExited,
}

/// Result of resolving runtime state after the persistence worker has loaded
/// canonical authority. The value is returned to the caller only after the
/// worker has adopted or initialized the matching durable image.
pub enum PersistenceStartupResolution<T> {
    AdoptLoaded {
        restore_image: RestoreImage,
        value: T,
    },
    Initialize {
        initial_image: RestoreImage,
        value: T,
    },
}

#[derive(Debug)]
pub enum PersistenceResolvedStartError<E> {
    Persistence(PersistenceWorkerStartError),
    Resolver(E),
}

impl<E: fmt::Display> fmt::Display for PersistenceResolvedStartError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(error) => error.fmt(formatter),
            Self::Resolver(error) => {
                write!(formatter, "persistence startup resolver failed: {error}")
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for PersistenceResolvedStartError<E> {}

impl fmt::Display for PersistenceWorkerStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(detail) => write!(formatter, "invalid worker config: {detail}"),
            Self::Spawn(detail) => {
                write!(formatter, "failed to spawn persistence worker: {detail}")
            }
            Self::Store(error) => write!(formatter, "persistence startup failed: {error}"),
            Self::WorkerExited => formatter.write_str("persistence worker exited during startup"),
        }
    }
}

impl std::error::Error for PersistenceWorkerStartError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceControlError {
    Closed,
    WorkerExited,
    Store(StoreError),
    Protocol(String),
    WorkerPanicked,
}

impl fmt::Display for PersistenceControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("persistence coordinator is closed"),
            Self::WorkerExited => formatter.write_str("persistence worker exited"),
            Self::Store(error) => write!(formatter, "persistence operation failed: {error}"),
            Self::Protocol(detail) => write!(formatter, "invalid persistence response: {detail}"),
            Self::WorkerPanicked => formatter.write_str("persistence worker panicked"),
        }
    }
}

impl std::error::Error for PersistenceControlError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnReservationError {
    Backpressure {
        capacity: usize,
        pending_turns: usize,
        reserved_slots: usize,
    },
    ControlInProgress,
    Closed,
}

impl fmt::Display for TurnReservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure {
                capacity,
                pending_turns,
                reserved_slots,
            } => write!(
                formatter,
                "persistence backpressure: {pending_turns} turns and {reserved_slots} reservations use capacity {capacity}"
            ),
            Self::ControlInProgress => {
                formatter.write_str("persistence admission is paused for a control operation")
            }
            Self::Closed => formatter.write_str("persistence coordinator is closed"),
        }
    }
}

impl std::error::Error for TurnReservationError {}

/// An admission failure that preserves ownership of the complete unaccepted
/// turn so the runtime can retry it or expose visible backpressure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnEnqueueError {
    Backpressure {
        turn: AuthorityTurn,
        capacity: usize,
        pending_turns: usize,
        reserved_slots: usize,
    },
    NonContiguous {
        turn: AuthorityTurn,
        expected_turn_sequence: u64,
    },
    ControlInProgress {
        turn: AuthorityTurn,
    },
    Closed {
        turn: AuthorityTurn,
    },
}

impl TurnEnqueueError {
    pub fn into_turn(self) -> AuthorityTurn {
        match self {
            Self::Backpressure { turn, .. }
            | Self::NonContiguous { turn, .. }
            | Self::ControlInProgress { turn }
            | Self::Closed { turn } => turn,
        }
    }
}

impl fmt::Display for TurnEnqueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure {
                capacity,
                pending_turns,
                reserved_slots,
                ..
            } => write!(
                formatter,
                "persistence backpressure: {pending_turns} turns and {reserved_slots} reservations use capacity {capacity}"
            ),
            Self::NonContiguous {
                turn,
                expected_turn_sequence,
            } => write!(
                formatter,
                "authority turn {} is not contiguous; expected {expected_turn_sequence}",
                turn.turn_sequence
            ),
            Self::ControlInProgress { .. } => {
                formatter.write_str("persistence admission is paused for a control operation")
            }
            Self::Closed { .. } => formatter.write_str("persistence coordinator is closed"),
        }
    }
}

impl std::error::Error for TurnEnqueueError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingTurnRange {
    pub first_turn_sequence: u64,
    pub last_turn_sequence: u64,
    pub age: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceWorkerStatus {
    pub pending: Option<PendingTurnRange>,
    /// Whether one worker-owned checkpoint transaction is currently running.
    pub checkpoint_batch_in_flight: bool,
    /// Checkpoint batches waiting behind the transaction currently in flight.
    pub queued_checkpoint_batches: usize,
    /// Checkpoint commits needed to make the currently accepted authority
    /// durable. Multiple logical turns may share one worker-owned checkpoint.
    pub pending_checkpoint_batches: usize,
    /// Lifetime high-water mark for `pending_checkpoint_batches`.
    ///
    /// This is updated while holding the admission lock, so observers cannot
    /// miss a transient backlog between status samples.
    pub pending_checkpoint_batches_peak: usize,
    pub durable_epoch: u64,
    pub durable_through_turn_sequence: u64,
    /// Turns waiting in the channel. Worker-owned batch turns remain visible in
    /// `pending` but are not included here.
    pub queue_depth: usize,
    pub pending_content_artifact_stores: usize,
    pub pending_content_artifact_loads: usize,
    pub reserved_slots: usize,
    pub accepting_turns: bool,
    pub worker_alive: bool,
    pub timings: PersistenceWorkerTimings,
    pub last_error: Option<StoreError>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PersistenceWorkerTimings {
    pub authority_enqueue_us: u64,
    pub encode_us: u64,
    pub checkpoint_us: u64,
    pub barrier_us: u64,
    pub restore_us: u64,
    pub migration_us: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentArtifactStoreTicket(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentArtifactStoreCompletion {
    pub ticket: ContentArtifactStoreTicket,
    pub result: Result<PutContentArtifactAck, StoreError>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentArtifactLoadTicket(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentArtifactLoadCompletion {
    pub ticket: ContentArtifactLoadTicket,
    pub id: ContentArtifactId,
    pub result: Result<Option<ContentArtifact>, StoreError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentArtifactLoadEnqueueError {
    Backpressure(ContentArtifactId),
    Closed(ContentArtifactId),
}

impl ContentArtifactLoadEnqueueError {
    pub const fn artifact_id(self) -> ContentArtifactId {
        match self {
            Self::Backpressure(id) | Self::Closed(id) => id,
        }
    }
}

impl fmt::Display for ContentArtifactLoadEnqueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure(_) => formatter.write_str("content artifact load queue is full"),
            Self::Closed(_) => formatter.write_str("persistence coordinator is closed"),
        }
    }
}

impl std::error::Error for ContentArtifactLoadEnqueueError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentArtifactStoreEnqueueError {
    Backpressure(ContentArtifact),
    Closed(ContentArtifact),
}

impl ContentArtifactStoreEnqueueError {
    pub fn into_artifact(self) -> ContentArtifact {
        match self {
            Self::Backpressure(artifact) | Self::Closed(artifact) => artifact,
        }
    }
}

impl fmt::Display for ContentArtifactStoreEnqueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure(_) => formatter.write_str("content artifact store queue is full"),
            Self::Closed(_) => formatter.write_str("persistence coordinator is closed"),
        }
    }
}

impl std::error::Error for ContentArtifactStoreEnqueueError {}

#[derive(Clone, Debug)]
struct PendingMeta {
    turn_sequence: u64,
    accepted_at: Instant,
}

#[derive(Debug)]
struct AdmissionState {
    accepting: bool,
    closed: bool,
    reservations: usize,
    next_turn_sequence: u64,
    pending: VecDeque<PendingMeta>,
    active_checkpoint_turns: usize,
    pending_checkpoint_batches_peak: usize,
}

#[derive(Debug)]
struct SharedState {
    capacity: usize,
    max_batch_turns: usize,
    admission: Mutex<AdmissionState>,
    reservation_changed: Condvar,
    queue_depth: AtomicUsize,
    pending_content_artifact_stores: AtomicUsize,
    pending_content_artifact_loads: AtomicUsize,
    next_content_artifact_ticket: AtomicU64,
    next_content_artifact_load_ticket: AtomicU64,
    content_artifact_completions: Mutex<VecDeque<ContentArtifactStoreCompletion>>,
    content_artifact_load_completions: Mutex<VecDeque<ContentArtifactLoadCompletion>>,
    durable_epoch: AtomicU64,
    durable_turn: AtomicU64,
    worker_alive: AtomicBool,
    authority_enqueue_us: AtomicU64,
    encode_us: AtomicU64,
    checkpoint_us: AtomicU64,
    barrier_us: AtomicU64,
    restore_us: AtomicU64,
    migration_us: AtomicU64,
    last_error: Mutex<Option<StoreError>>,
}

impl SharedState {
    fn new(capacity: usize, max_batch_turns: usize) -> Self {
        Self {
            capacity,
            max_batch_turns,
            admission: Mutex::new(AdmissionState {
                accepting: false,
                closed: false,
                reservations: 0,
                next_turn_sequence: 1,
                pending: VecDeque::new(),
                active_checkpoint_turns: 0,
                pending_checkpoint_batches_peak: 0,
            }),
            reservation_changed: Condvar::new(),
            queue_depth: AtomicUsize::new(0),
            pending_content_artifact_stores: AtomicUsize::new(0),
            pending_content_artifact_loads: AtomicUsize::new(0),
            next_content_artifact_ticket: AtomicU64::new(1),
            next_content_artifact_load_ticket: AtomicU64::new(1),
            content_artifact_completions: Mutex::new(VecDeque::new()),
            content_artifact_load_completions: Mutex::new(VecDeque::new()),
            durable_epoch: AtomicU64::new(0),
            durable_turn: AtomicU64::new(0),
            worker_alive: AtomicBool::new(true),
            authority_enqueue_us: AtomicU64::new(0),
            encode_us: AtomicU64::new(0),
            checkpoint_us: AtomicU64::new(0),
            barrier_us: AtomicU64::new(0),
            restore_us: AtomicU64::new(0),
            migration_us: AtomicU64::new(0),
            last_error: Mutex::new(None),
        }
    }

    fn finish_startup(&self, image: &RestoreImage) {
        self.durable_epoch.store(image.epoch, Ordering::Release);
        self.durable_turn
            .store(image.through_turn_sequence, Ordering::Release);
        let mut admission = lock(&self.admission);
        admission.next_turn_sequence = image.through_turn_sequence.saturating_add(1);
        admission.accepting = true;
    }

    fn record_store_error(&self, error: StoreError) {
        *lock(&self.last_error) = Some(error);
    }

    fn mark_durable(&self, epoch: u64, through_turn_sequence: u64) -> Result<(), StoreError> {
        let mut admission = lock(&self.admission);
        let Some(first) = admission.pending.front() else {
            return Err(StoreError::Backend(
                "worker committed a checkpoint with no admitted turns".to_owned(),
            ));
        };
        let expected_first = self.durable_turn.load(Ordering::Acquire).saturating_add(1);
        if first.turn_sequence != expected_first || through_turn_sequence < first.turn_sequence {
            return Err(StoreError::Backend(format!(
                "worker committed invalid pending range {}..={through_turn_sequence}; expected first {expected_first}",
                first.turn_sequence
            )));
        }
        while admission
            .pending
            .front()
            .is_some_and(|pending| pending.turn_sequence <= through_turn_sequence)
        {
            admission.pending.pop_front();
        }
        if admission
            .pending
            .front()
            .is_some_and(|pending| pending.turn_sequence != through_turn_sequence.saturating_add(1))
        {
            return Err(StoreError::Backend(
                "worker durable acknowledgement split the admitted turn sequence".to_owned(),
            ));
        }
        admission.active_checkpoint_turns = 0;
        self.durable_epoch.store(epoch, Ordering::Release);
        self.durable_turn
            .store(through_turn_sequence, Ordering::Release);
        Ok(())
    }

    fn begin_checkpoint_batch(&self, turn_count: usize) {
        let mut admission = lock(&self.admission);
        if admission.active_checkpoint_turns == 0 {
            admission.active_checkpoint_turns = turn_count;
        } else {
            debug_assert_eq!(admission.active_checkpoint_turns, turn_count);
        }
        update_pending_checkpoint_batches_peak(&mut admission, self.max_batch_turns);
    }

    fn clear_checkpoint_batch(&self) {
        lock(&self.admission).active_checkpoint_turns = 0;
    }

    fn mark_activation(&self, epoch: u64, through_turn_sequence: u64) {
        self.durable_epoch.store(epoch, Ordering::Release);
        self.durable_turn
            .store(through_turn_sequence, Ordering::Release);
        let mut admission = lock(&self.admission);
        admission.next_turn_sequence = through_turn_sequence.saturating_add(1);
    }

    fn status(&self) -> PersistenceWorkerStatus {
        let now = Instant::now();
        let admission = lock(&self.admission);
        let pending_checkpoint_batches =
            pending_checkpoint_batches(&admission, self.max_batch_turns);
        let checkpoint_batch_in_flight = admission.active_checkpoint_turns > 0;
        let pending = admission.pending.front().map(|first| PendingTurnRange {
            first_turn_sequence: first.turn_sequence,
            last_turn_sequence: admission
                .pending
                .back()
                .expect("front exists when back is read")
                .turn_sequence,
            age: now.saturating_duration_since(first.accepted_at),
        });
        PersistenceWorkerStatus {
            pending,
            checkpoint_batch_in_flight,
            queued_checkpoint_batches: queued_checkpoint_batches(&admission, self.max_batch_turns),
            pending_checkpoint_batches,
            pending_checkpoint_batches_peak: admission.pending_checkpoint_batches_peak,
            durable_epoch: self.durable_epoch.load(Ordering::Acquire),
            durable_through_turn_sequence: self.durable_turn.load(Ordering::Acquire),
            queue_depth: self.queue_depth.load(Ordering::Acquire),
            pending_content_artifact_stores: self
                .pending_content_artifact_stores
                .load(Ordering::Acquire),
            pending_content_artifact_loads: self
                .pending_content_artifact_loads
                .load(Ordering::Acquire),
            reserved_slots: admission.reservations,
            accepting_turns: admission.accepting && !admission.closed,
            worker_alive: self.worker_alive.load(Ordering::Acquire),
            timings: PersistenceWorkerTimings {
                authority_enqueue_us: self.authority_enqueue_us.load(Ordering::Acquire),
                encode_us: self.encode_us.load(Ordering::Acquire),
                checkpoint_us: self.checkpoint_us.load(Ordering::Acquire),
                barrier_us: self.barrier_us.load(Ordering::Acquire),
                restore_us: self.restore_us.load(Ordering::Acquire),
                migration_us: self.migration_us.load(Ordering::Acquire),
            },
            last_error: lock(&self.last_error).clone(),
        }
    }
}

fn pending_checkpoint_batches(admission: &AdmissionState, max_batch_turns: usize) -> usize {
    usize::from(admission.active_checkpoint_turns > 0)
        .saturating_add(queued_checkpoint_batches(admission, max_batch_turns))
}

fn queued_checkpoint_batches(admission: &AdmissionState, max_batch_turns: usize) -> usize {
    let active_checkpoint_turns = admission
        .active_checkpoint_turns
        .min(admission.pending.len());
    let queued_checkpoint_turns = admission
        .pending
        .len()
        .saturating_sub(active_checkpoint_turns);
    queued_checkpoint_turns.div_ceil(max_batch_turns)
}

fn update_pending_checkpoint_batches_peak(admission: &mut AdmissionState, max_batch_turns: usize) {
    admission.pending_checkpoint_batches_peak = admission
        .pending_checkpoint_batches_peak
        .max(pending_checkpoint_batches(admission, max_batch_turns));
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

struct QueuedTurn {
    turn: AuthorityTurn,
}

enum ControlReply {
    Load(Result<Option<RestoreImage>, StoreError>),
    Barrier(Result<BarrierAck, StoreError>),
    ImmediateTurn(Result<CommitAck, StoreError>),
    Inspect(Result<Option<PersistenceInspectorSnapshot>, StoreError>),
    Activate(Result<ActivationAck, StoreError>),
    ResetApplication(Result<ResetApplicationAck, StoreError>),
    Compact(Result<CompactAck, StoreError>),
    ExportApplication(Result<ApplicationTransfer, StoreError>),
    PutContentArtifact(Result<PutContentArtifactAck, StoreError>),
    LoadContentArtifact(Result<Option<ContentArtifact>, StoreError>),
    Shutdown(Result<ShutdownAck, StoreError>),
}

enum WorkerMessage {
    Turn(QueuedTurn),
    Load(SyncSender<ControlReply>),
    Barrier(SyncSender<ControlReply>),
    ImmediateTurn(Box<AuthorityTurn>, SyncSender<ControlReply>),
    Inspect(SyncSender<ControlReply>),
    Activate(Box<ActivationBatch>, SyncSender<ControlReply>),
    ResetApplication(Box<ResetApplicationBatch>, SyncSender<ControlReply>),
    Compact(SyncSender<ControlReply>),
    ExportApplication(SyncSender<ControlReply>),
    PutContentArtifact(Box<ContentArtifact>, SyncSender<ControlReply>),
    PutContentArtifactAsync(ContentArtifactStoreTicket, Box<ContentArtifact>),
    LoadContentArtifact(ContentArtifactId, SyncSender<ControlReply>),
    LoadContentArtifactAsync(ContentArtifactLoadTicket, ContentArtifactId),
    Shutdown(SyncSender<ControlReply>),
}

/// Capacity reservation for a single complete authority turn.
///
/// Reserving lets the runtime prove queue capacity before constructing a large
/// delta vector. Dropping an unused reservation returns its slot immediately.
pub struct AuthorityTurnReservation {
    shared: Arc<SharedState>,
    sender: SyncSender<WorkerMessage>,
    active: bool,
}

impl AuthorityTurnReservation {
    pub fn enqueue(mut self, turn: AuthorityTurn) -> Result<(), TurnEnqueueError> {
        let started = Instant::now();
        let result = (|| {
            let mut admission = lock(&self.shared.admission);
            admission.reservations = admission.reservations.saturating_sub(1);
            self.active = false;
            self.shared.reservation_changed.notify_all();

            if admission.closed || !self.shared.worker_alive.load(Ordering::Acquire) {
                return Err(TurnEnqueueError::Closed { turn });
            }
            if turn.turn_sequence != admission.next_turn_sequence {
                return Err(TurnEnqueueError::NonContiguous {
                    turn,
                    expected_turn_sequence: admission.next_turn_sequence,
                });
            }

            let accepted_at = Instant::now();
            admission.pending.push_back(PendingMeta {
                turn_sequence: turn.turn_sequence,
                accepted_at,
            });
            admission.next_turn_sequence = turn.turn_sequence.saturating_add(1);
            self.shared.queue_depth.fetch_add(1, Ordering::AcqRel);

            match self
                .sender
                .try_send(WorkerMessage::Turn(QueuedTurn { turn }))
            {
                Ok(()) => {
                    update_pending_checkpoint_batches_peak(
                        &mut admission,
                        self.shared.max_batch_turns,
                    );
                    Ok(())
                }
                Err(TrySendError::Full(WorkerMessage::Turn(queued))) => {
                    self.shared.queue_depth.fetch_sub(1, Ordering::AcqRel);
                    admission.pending.pop_back();
                    admission.next_turn_sequence = queued.turn.turn_sequence;
                    Err(TurnEnqueueError::Backpressure {
                        turn: queued.turn,
                        capacity: self.shared.capacity,
                        pending_turns: admission.pending.len(),
                        reserved_slots: admission.reservations,
                    })
                }
                Err(TrySendError::Disconnected(WorkerMessage::Turn(queued))) => {
                    self.shared.queue_depth.fetch_sub(1, Ordering::AcqRel);
                    admission.pending.pop_back();
                    admission.next_turn_sequence = queued.turn.turn_sequence;
                    self.shared.worker_alive.store(false, Ordering::Release);
                    admission.closed = true;
                    admission.accepting = false;
                    Err(TurnEnqueueError::Closed { turn: queued.turn })
                }
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    unreachable!("turn send can only return a turn message")
                }
            }
        })();
        self.shared
            .authority_enqueue_us
            .store(duration_us(started.elapsed()), Ordering::Release);
        result
    }
}

impl Drop for AuthorityTurnReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut admission = lock(&self.shared.admission);
        admission.reservations = admission.reservations.saturating_sub(1);
        self.shared.reservation_changed.notify_all();
    }
}

/// Native persistence coordinator with bounded, nonblocking turn admission.
///
/// The driver is created and used exclusively on the dedicated worker thread.
/// Input and render paths only reserve capacity and move an already-built turn
/// into a bounded channel; checkpoint construction, checksumming, encoding, and
/// database work stay on the worker.
pub struct PersistenceCoordinator {
    sender: SyncSender<WorkerMessage>,
    shared: Arc<SharedState>,
    control_lock: Mutex<()>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

enum WorkerStartup {
    InitialImage(RestoreImage),
    ResolveAfterLoad {
        application: ApplicationIdentity,
        loaded: SyncSender<Result<Option<RestoreImage>, StoreError>>,
        resolution: Receiver<WorkerStartupResolution>,
    },
}

enum WorkerStartupResolution {
    AdoptLoaded(RestoreImage),
    Initialize(RestoreImage),
    Abort,
}

impl PersistenceCoordinator {
    pub fn start<D>(
        driver: D,
        initial_image: RestoreImage,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, PersistenceStartup), PersistenceWorkerStartError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        validate_worker_config(&config)?;

        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let shared = Arc::new(SharedState::new(
            config.queue_capacity,
            config.max_batch_turns,
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("boon-persistence".to_owned())
            .spawn(move || {
                worker_main(
                    driver,
                    WorkerStartup::InitialImage(initial_image),
                    config,
                    receiver,
                    startup_sender,
                    worker_shared,
                );
            })
            .map_err(|error| PersistenceWorkerStartError::Spawn(error.to_string()))?;

        let startup = match startup_receiver.recv() {
            Ok(Ok(startup)) => startup,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(PersistenceWorkerStartError::Store(error));
            }
            Err(_) => {
                let _ = worker.join();
                return Err(PersistenceWorkerStartError::WorkerExited);
            }
        };

        Ok((
            Self {
                sender,
                shared,
                control_lock: Mutex::new(()),
                worker: Mutex::new(Some(worker)),
            },
            startup,
        ))
    }

    /// Start the persistence worker by loading authority before constructing
    /// runtime-derived defaults or indexes.
    ///
    /// The driver remains owned by the worker throughout. The resolver runs on
    /// the caller after `Load` and returns exactly one runtime value paired with
    /// either the loaded image or an initial image for a missing application.
    pub fn start_resolved<D, T, E, F>(
        driver: D,
        application: ApplicationIdentity,
        config: PersistenceWorkerConfig,
        resolver: F,
    ) -> Result<(Self, PersistenceStartup, T), PersistenceResolvedStartError<E>>
    where
        D: PersistenceDriver + Send + 'static,
        T: Send + 'static,
        F: FnOnce(Option<RestoreImage>) -> Result<PersistenceStartupResolution<T>, E>,
    {
        validate_worker_config(&config).map_err(PersistenceResolvedStartError::Persistence)?;

        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let (loaded_sender, loaded_receiver) = mpsc::sync_channel(1);
        let (resolution_sender, resolution_receiver) = mpsc::sync_channel(1);
        let shared = Arc::new(SharedState::new(
            config.queue_capacity,
            config.max_batch_turns,
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("boon-persistence".to_owned())
            .spawn(move || {
                worker_main(
                    driver,
                    WorkerStartup::ResolveAfterLoad {
                        application,
                        loaded: loaded_sender,
                        resolution: resolution_receiver,
                    },
                    config,
                    receiver,
                    startup_sender,
                    worker_shared,
                );
            })
            .map_err(|error| {
                PersistenceResolvedStartError::Persistence(PersistenceWorkerStartError::Spawn(
                    error.to_string(),
                ))
            })?;

        let loaded = match loaded_receiver.recv() {
            Ok(Ok(loaded)) => loaded,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(PersistenceResolvedStartError::Persistence(
                    PersistenceWorkerStartError::Store(error),
                ));
            }
            Err(_) => {
                let _ = worker.join();
                return Err(PersistenceResolvedStartError::Persistence(
                    PersistenceWorkerStartError::WorkerExited,
                ));
            }
        };
        let resolution = match resolver(loaded) {
            Ok(resolution) => resolution,
            Err(error) => {
                let _ = resolution_sender.send(WorkerStartupResolution::Abort);
                let _ = worker.join();
                return Err(PersistenceResolvedStartError::Resolver(error));
            }
        };
        let (resolution, value) = match resolution {
            PersistenceStartupResolution::AdoptLoaded {
                restore_image,
                value,
            } => (WorkerStartupResolution::AdoptLoaded(restore_image), value),
            PersistenceStartupResolution::Initialize {
                initial_image,
                value,
            } => (WorkerStartupResolution::Initialize(initial_image), value),
        };
        if resolution_sender.send(resolution).is_err() {
            let _ = worker.join();
            return Err(PersistenceResolvedStartError::Persistence(
                PersistenceWorkerStartError::WorkerExited,
            ));
        }

        let startup = match startup_receiver.recv() {
            Ok(Ok(startup)) => startup,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(PersistenceResolvedStartError::Persistence(
                    PersistenceWorkerStartError::Store(error),
                ));
            }
            Err(_) => {
                let _ = worker.join();
                return Err(PersistenceResolvedStartError::Persistence(
                    PersistenceWorkerStartError::WorkerExited,
                ));
            }
        };

        Ok((
            Self {
                sender,
                shared,
                control_lock: Mutex::new(()),
                worker: Mutex::new(Some(worker)),
            },
            startup,
            value,
        ))
    }

    pub fn status(&self) -> PersistenceWorkerStatus {
        self.shared.status()
    }

    pub fn try_reserve_turn(&self) -> Result<AuthorityTurnReservation, TurnReservationError> {
        let mut admission = lock(&self.shared.admission);
        if admission.closed || !self.shared.worker_alive.load(Ordering::Acquire) {
            return Err(TurnReservationError::Closed);
        }
        if !admission.accepting {
            return Err(TurnReservationError::ControlInProgress);
        }
        if admission.pending.len() + admission.reservations >= self.shared.capacity {
            return Err(TurnReservationError::Backpressure {
                capacity: self.shared.capacity,
                pending_turns: admission.pending.len(),
                reserved_slots: admission.reservations,
            });
        }
        admission.reservations += 1;
        Ok(AuthorityTurnReservation {
            shared: Arc::clone(&self.shared),
            sender: self.sender.clone(),
            active: true,
        })
    }

    pub fn try_enqueue_turn(&self, turn: AuthorityTurn) -> Result<(), TurnEnqueueError> {
        match self.try_reserve_turn() {
            Ok(reservation) => reservation.enqueue(turn),
            Err(TurnReservationError::Backpressure {
                capacity,
                pending_turns,
                reserved_slots,
            }) => Err(TurnEnqueueError::Backpressure {
                turn,
                capacity,
                pending_turns,
                reserved_slots,
            }),
            Err(TurnReservationError::ControlInProgress) => {
                Err(TurnEnqueueError::ControlInProgress { turn })
            }
            Err(TurnReservationError::Closed) => Err(TurnEnqueueError::Closed { turn }),
        }
    }

    pub fn barrier(&self) -> Result<BarrierAck, PersistenceControlError> {
        match self.control_request(WorkerMessage::Barrier, false)? {
            ControlReply::Barrier(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-barrier response".to_owned(),
            )),
        }
    }

    pub fn commit_immediate(
        &self,
        turn: AuthorityTurn,
    ) -> Result<CommitAck, PersistenceControlError> {
        match self.control_request(
            |reply| WorkerMessage::ImmediateTurn(Box::new(turn), reply),
            false,
        )? {
            ControlReply::ImmediateTurn(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-immediate-turn response".to_owned(),
            )),
        }
    }

    pub fn load(&self) -> Result<Option<RestoreImage>, PersistenceControlError> {
        match self.control_request(WorkerMessage::Load, false)? {
            ControlReply::Load(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-load response".to_owned(),
            )),
        }
    }

    pub fn inspect(&self) -> Result<Option<PersistenceInspectorSnapshot>, PersistenceControlError> {
        match self.control_request(WorkerMessage::Inspect, false)? {
            ControlReply::Inspect(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-inspect response".to_owned(),
            )),
        }
    }

    pub fn activate(
        &self,
        batch: ActivationBatch,
    ) -> Result<ActivationAck, PersistenceControlError> {
        match self.control_request(
            |reply| WorkerMessage::Activate(Box::new(batch), reply),
            false,
        )? {
            ControlReply::Activate(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-activation response".to_owned(),
            )),
        }
    }

    pub fn reset_application(
        &self,
        batch: ResetApplicationBatch,
    ) -> Result<ResetApplicationAck, PersistenceControlError> {
        match self.control_request(
            |reply| WorkerMessage::ResetApplication(Box::new(batch), reply),
            false,
        )? {
            ControlReply::ResetApplication(result) => {
                result.map_err(PersistenceControlError::Store)
            }
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-reset response".to_owned(),
            )),
        }
    }

    pub fn compact(&self) -> Result<CompactAck, PersistenceControlError> {
        match self.control_request(WorkerMessage::Compact, false)? {
            ControlReply::Compact(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-compact response".to_owned(),
            )),
        }
    }

    pub fn export_application(&self) -> Result<ApplicationTransfer, PersistenceControlError> {
        match self.control_request(WorkerMessage::ExportApplication, false)? {
            ControlReply::ExportApplication(result) => {
                result.map_err(PersistenceControlError::Store)
            }
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-application-export response".to_owned(),
            )),
        }
    }

    pub fn put_content_artifact(
        &self,
        artifact: ContentArtifact,
    ) -> Result<PutContentArtifactAck, PersistenceControlError> {
        match self.control_request(
            |reply| WorkerMessage::PutContentArtifact(Box::new(artifact), reply),
            false,
        )? {
            ControlReply::PutContentArtifact(result) => {
                result.map_err(PersistenceControlError::Store)
            }
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-artifact-store response".to_owned(),
            )),
        }
    }

    pub fn try_put_content_artifact(
        &self,
        artifact: ContentArtifact,
    ) -> Result<ContentArtifactStoreTicket, ContentArtifactStoreEnqueueError> {
        {
            let admission = lock(&self.shared.admission);
            if admission.closed || !self.shared.worker_alive.load(Ordering::Acquire) {
                return Err(ContentArtifactStoreEnqueueError::Closed(artifact));
            }
        }
        let ticket = ContentArtifactStoreTicket(
            self.shared
                .next_content_artifact_ticket
                .fetch_add(1, Ordering::AcqRel),
        );
        self.shared
            .pending_content_artifact_stores
            .fetch_add(1, Ordering::AcqRel);
        match self.sender.try_send(WorkerMessage::PutContentArtifactAsync(
            ticket,
            Box::new(artifact),
        )) {
            Ok(()) => Ok(ticket),
            Err(TrySendError::Full(WorkerMessage::PutContentArtifactAsync(_, artifact))) => {
                self.shared
                    .pending_content_artifact_stores
                    .fetch_sub(1, Ordering::AcqRel);
                Err(ContentArtifactStoreEnqueueError::Backpressure(*artifact))
            }
            Err(TrySendError::Disconnected(WorkerMessage::PutContentArtifactAsync(
                _,
                artifact,
            ))) => {
                self.shared
                    .pending_content_artifact_stores
                    .fetch_sub(1, Ordering::AcqRel);
                self.mark_worker_exited();
                Err(ContentArtifactStoreEnqueueError::Closed(*artifact))
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                unreachable!("artifact send can only return an artifact message")
            }
        }
    }

    pub fn take_content_artifact_store_completions(&self) -> Vec<ContentArtifactStoreCompletion> {
        lock(&self.shared.content_artifact_completions)
            .drain(..)
            .collect()
    }

    pub fn load_content_artifact(
        &self,
        id: ContentArtifactId,
    ) -> Result<Option<ContentArtifact>, PersistenceControlError> {
        match self.control_request(|reply| WorkerMessage::LoadContentArtifact(id, reply), false)? {
            ControlReply::LoadContentArtifact(result) => {
                result.map_err(PersistenceControlError::Store)
            }
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-artifact-load response".to_owned(),
            )),
        }
    }

    pub fn try_load_content_artifact(
        &self,
        id: ContentArtifactId,
    ) -> Result<ContentArtifactLoadTicket, ContentArtifactLoadEnqueueError> {
        {
            let admission = lock(&self.shared.admission);
            if admission.closed || !self.shared.worker_alive.load(Ordering::Acquire) {
                return Err(ContentArtifactLoadEnqueueError::Closed(id));
            }
        }
        let ticket = ContentArtifactLoadTicket(
            self.shared
                .next_content_artifact_load_ticket
                .fetch_add(1, Ordering::AcqRel),
        );
        self.shared
            .pending_content_artifact_loads
            .fetch_add(1, Ordering::AcqRel);
        match self
            .sender
            .try_send(WorkerMessage::LoadContentArtifactAsync(ticket, id))
        {
            Ok(()) => Ok(ticket),
            Err(TrySendError::Full(WorkerMessage::LoadContentArtifactAsync(_, id))) => {
                self.shared
                    .pending_content_artifact_loads
                    .fetch_sub(1, Ordering::AcqRel);
                Err(ContentArtifactLoadEnqueueError::Backpressure(id))
            }
            Err(TrySendError::Disconnected(WorkerMessage::LoadContentArtifactAsync(_, id))) => {
                self.shared
                    .pending_content_artifact_loads
                    .fetch_sub(1, Ordering::AcqRel);
                self.mark_worker_exited();
                Err(ContentArtifactLoadEnqueueError::Closed(id))
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                unreachable!("artifact load send can only return an artifact load message")
            }
        }
    }

    pub fn take_content_artifact_load_completions(&self) -> Vec<ContentArtifactLoadCompletion> {
        lock(&self.shared.content_artifact_load_completions)
            .drain(..)
            .collect()
    }

    pub fn shutdown(&self) -> Result<ShutdownAck, PersistenceControlError> {
        let reply = self.control_request(WorkerMessage::Shutdown, true)?;
        let result = match reply {
            ControlReply::Shutdown(result) => result.map_err(PersistenceControlError::Store),
            _ => Err(PersistenceControlError::Protocol(
                "worker returned a non-shutdown response".to_owned(),
            )),
        };
        if result.is_ok()
            && let Some(worker) = lock(&self.worker).take()
            && worker.join().is_err()
        {
            return Err(PersistenceControlError::WorkerPanicked);
        }
        result
    }

    fn control_request<F>(
        &self,
        build_message: F,
        terminal: bool,
    ) -> Result<ControlReply, PersistenceControlError>
    where
        F: FnOnce(SyncSender<ControlReply>) -> WorkerMessage,
    {
        let _control = lock(&self.control_lock);
        {
            let mut admission = lock(&self.shared.admission);
            if admission.closed || !self.shared.worker_alive.load(Ordering::Acquire) {
                return Err(PersistenceControlError::Closed);
            }
            admission.accepting = false;
            while admission.reservations != 0 {
                admission = self
                    .shared
                    .reservation_changed
                    .wait(admission)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }

        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        if self.sender.send(build_message(reply_sender)).is_err() {
            self.mark_worker_exited();
            return Err(PersistenceControlError::WorkerExited);
        }
        let reply = match reply_receiver.recv() {
            Ok(reply) => reply,
            Err(_) => {
                self.mark_worker_exited();
                return Err(PersistenceControlError::WorkerExited);
            }
        };

        let terminal_succeeded = terminal && matches!(&reply, ControlReply::Shutdown(Ok(_)));
        let mut admission = lock(&self.shared.admission);
        if terminal_succeeded {
            admission.closed = true;
            admission.accepting = false;
        } else if !admission.closed && self.shared.worker_alive.load(Ordering::Acquire) {
            admission.accepting = true;
        }
        Ok(reply)
    }

    fn mark_worker_exited(&self) {
        self.shared.worker_alive.store(false, Ordering::Release);
        let mut admission = lock(&self.shared.admission);
        admission.closed = true;
        admission.accepting = false;
        self.shared.reservation_changed.notify_all();
    }
}

impl Drop for PersistenceCoordinator {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_main<D>(
    mut driver: D,
    startup: WorkerStartup,
    config: PersistenceWorkerConfig,
    receiver: Receiver<WorkerMessage>,
    startup_sender: SyncSender<Result<PersistenceStartup, StoreError>>,
    shared: Arc<SharedState>,
) where
    D: PersistenceDriver,
{
    let restore_started = Instant::now();
    let startup = match startup {
        WorkerStartup::InitialImage(initial_image) => {
            start_driver(&mut driver, initial_image).map(Some)
        }
        WorkerStartup::ResolveAfterLoad {
            application,
            loaded,
            resolution,
        } => start_driver_resolved(&mut driver, application, loaded, resolution),
    };
    shared
        .restore_us
        .store(duration_us(restore_started.elapsed()), Ordering::Release);
    let startup = match startup {
        Ok(Some(startup)) => startup,
        Ok(None) => {
            let _ = driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
            shared.worker_alive.store(false, Ordering::Release);
            return;
        }
        Err(error) => {
            shared.record_store_error(error.clone());
            shared.worker_alive.store(false, Ordering::Release);
            let _ = startup_sender.send(Err(error));
            return;
        }
    };
    shared.finish_startup(&startup.restore_image);
    let mut durable = DurableCursor::from_image(&startup.restore_image);
    if startup_sender.send(Ok(startup)).is_err() {
        let _ = driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
        shared.worker_alive.store(false, Ordering::Release);
        return;
    }

    let mut pending = Vec::new();
    let mut deferred = None;
    loop {
        let message = match deferred.take() {
            Some(message) => message,
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => {
                    let _ = flush_pending(&mut driver, &mut durable, &mut pending, &shared);
                    let _ = driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
                    break;
                }
            },
        };

        match message {
            WorkerMessage::Turn(turn) => {
                shared.queue_depth.fetch_sub(1, Ordering::AcqRel);
                pending.push(turn);
                let mut async_artifacts = Vec::new();
                collect_batch(
                    &receiver,
                    &config,
                    &mut pending,
                    &mut async_artifacts,
                    &mut deferred,
                    &shared,
                );
                shared.begin_checkpoint_batch(pending.len());
                let _ = flush_pending(&mut driver, &mut durable, &mut pending, &shared);
                for artifact in async_artifacts {
                    assert!(
                        handle_async_artifact(&mut driver, &durable, artifact, &shared).is_ok(),
                        "batch collection must retain only asynchronous artifact messages"
                    );
                }
            }
            message => {
                if let Err(control) = handle_async_artifact(&mut driver, &durable, message, &shared)
                {
                    let should_stop =
                        handle_control(&mut driver, &mut durable, &mut pending, control, &shared);
                    if should_stop {
                        break;
                    }
                }
            }
        }
    }

    shared.worker_alive.store(false, Ordering::Release);
    let mut admission = lock(&shared.admission);
    admission.accepting = false;
    admission.closed = true;
    shared.reservation_changed.notify_all();
}

fn start_driver<D>(
    driver: &mut D,
    initial_image: RestoreImage,
) -> Result<PersistenceStartup, StoreError>
where
    D: PersistenceDriver,
{
    let application = initial_image.application.clone();
    let (restore_image, initialized) = match load_driver(driver, application.clone())? {
        Some(image) => (image, false),
        None => (initialize_driver(driver, initial_image)?, true),
    };
    Ok(PersistenceStartup {
        restore_image,
        initialized,
    })
}

fn start_driver_resolved<D>(
    driver: &mut D,
    application: ApplicationIdentity,
    loaded_sender: SyncSender<Result<Option<RestoreImage>, StoreError>>,
    resolution_receiver: Receiver<WorkerStartupResolution>,
) -> Result<Option<PersistenceStartup>, StoreError>
where
    D: PersistenceDriver,
{
    let loaded = match load_driver(driver, application.clone()) {
        Ok(loaded) => loaded,
        Err(error) => {
            let _ = loaded_sender.send(Err(error.clone()));
            return Err(error);
        }
    };
    let had_loaded = loaded.is_some();
    if loaded_sender.send(Ok(loaded)).is_err() {
        return Ok(None);
    }
    let resolution = match resolution_receiver.recv() {
        Ok(WorkerStartupResolution::Abort) | Err(_) => return Ok(None),
        Ok(resolution) => resolution,
    };
    let startup = match (had_loaded, resolution) {
        (true, WorkerStartupResolution::AdoptLoaded(restore_image)) => {
            if restore_image.application != application {
                return Err(StoreError::IdentityMismatch);
            }
            PersistenceStartup {
                restore_image,
                initialized: false,
            }
        }
        (false, WorkerStartupResolution::Initialize(initial_image)) => PersistenceStartup {
            restore_image: initialize_driver(driver, initial_image)?,
            initialized: true,
        },
        (true, WorkerStartupResolution::Initialize(_)) => {
            return Err(StoreError::Backend(
                "startup resolver attempted to initialize an existing application".to_owned(),
            ));
        }
        (false, WorkerStartupResolution::AdoptLoaded(_)) => {
            return Err(StoreError::Backend(
                "startup resolver attempted to adopt a missing application".to_owned(),
            ));
        }
        (_, WorkerStartupResolution::Abort) => unreachable!(),
    };
    Ok(Some(startup))
}

fn load_driver<D>(
    driver: &mut D,
    application: ApplicationIdentity,
) -> Result<Option<RestoreImage>, StoreError>
where
    D: PersistenceDriver,
{
    let request = RestoreRequest {
        application: application.clone(),
        expected_schema_hash: None,
    };
    match driver.execute(PersistenceCommand::Load(request)) {
        PersistenceResult::Loaded(Ok(Some(image))) => {
            if image.application != application {
                return Err(StoreError::IdentityMismatch);
            }
            Ok(Some(image))
        }
        PersistenceResult::Loaded(Ok(None)) => Ok(None),
        PersistenceResult::Loaded(Err(error)) => Err(error),
        _ => Err(StoreError::Backend(
            "driver returned the wrong result for Load".to_owned(),
        )),
    }
}

fn initialize_driver<D>(
    driver: &mut D,
    initial_image: RestoreImage,
) -> Result<RestoreImage, StoreError>
where
    D: PersistenceDriver,
{
    match driver.execute(PersistenceCommand::Initialize(initial_image.clone())) {
        PersistenceResult::Initialized(Ok(_)) => Ok(initial_image),
        PersistenceResult::Initialized(Err(error)) => Err(error),
        _ => Err(StoreError::Backend(
            "driver returned the wrong result for Initialize".to_owned(),
        )),
    }
}

fn validate_worker_config(
    config: &PersistenceWorkerConfig,
) -> Result<(), PersistenceWorkerStartError> {
    if config.queue_capacity == 0 {
        return Err(PersistenceWorkerStartError::InvalidConfig(
            "queue_capacity must be positive",
        ));
    }
    if config.max_batch_turns == 0 {
        return Err(PersistenceWorkerStartError::InvalidConfig(
            "max_batch_turns must be positive",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct DurableCursor {
    application: ApplicationIdentity,
    schema_hash: [u8; 32],
    epoch: u64,
    through_turn_sequence: u64,
}

impl DurableCursor {
    fn from_image(image: &RestoreImage) -> Self {
        Self {
            application: image.application.clone(),
            schema_hash: image.schema_hash,
            epoch: image.epoch,
            through_turn_sequence: image.through_turn_sequence,
        }
    }
}

fn handle_async_artifact<D>(
    driver: &mut D,
    durable: &DurableCursor,
    message: WorkerMessage,
    shared: &SharedState,
) -> Result<(), WorkerMessage>
where
    D: PersistenceDriver,
{
    match message {
        WorkerMessage::PutContentArtifactAsync(ticket, artifact) => {
            let result = match driver.execute(PersistenceCommand::PutContentArtifact(
                PutContentArtifactRequest {
                    application: durable.application.clone(),
                    artifact: *artifact,
                },
            )) {
                PersistenceResult::ContentArtifactStored(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for PutContentArtifact".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            lock(&shared.content_artifact_completions)
                .push_back(ContentArtifactStoreCompletion { ticket, result });
            shared
                .pending_content_artifact_stores
                .fetch_sub(1, Ordering::AcqRel);
            Ok(())
        }
        WorkerMessage::LoadContentArtifactAsync(ticket, id) => {
            let result = match driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: durable.application.clone(),
                    id,
                },
            )) {
                PersistenceResult::ContentArtifactLoaded(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for LoadContentArtifact".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            lock(&shared.content_artifact_load_completions)
                .push_back(ContentArtifactLoadCompletion { ticket, id, result });
            shared
                .pending_content_artifact_loads
                .fetch_sub(1, Ordering::AcqRel);
            Ok(())
        }
        message => Err(message),
    }
}

fn collect_batch(
    receiver: &Receiver<WorkerMessage>,
    config: &PersistenceWorkerConfig,
    pending: &mut Vec<QueuedTurn>,
    async_artifacts: &mut Vec<WorkerMessage>,
    deferred: &mut Option<WorkerMessage>,
    shared: &SharedState,
) {
    if pending.len() >= config.max_batch_turns {
        return;
    }
    let deadline = Instant::now() + config.coalesce_delay;
    loop {
        if pending.len() >= config.max_batch_turns {
            return;
        }
        let received = if config.coalesce_delay.is_zero() {
            match receiver.try_recv() {
                Ok(message) => message,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };
            match receiver.recv_timeout(remaining) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        };
        match received {
            WorkerMessage::Turn(turn) => {
                shared.queue_depth.fetch_sub(1, Ordering::AcqRel);
                pending.push(turn);
            }
            message @ (WorkerMessage::PutContentArtifactAsync(_, _)
            | WorkerMessage::LoadContentArtifactAsync(_, _)) => {
                async_artifacts.push(message);
            }
            control => {
                *deferred = Some(control);
                return;
            }
        }
    }
}

fn flush_pending<D>(
    driver: &mut D,
    durable: &mut DurableCursor,
    pending: &mut Vec<QueuedTurn>,
    shared: &SharedState,
) -> Result<(), StoreError>
where
    D: PersistenceDriver,
{
    let Some(first) = pending.first() else {
        return Ok(());
    };
    let expected_first = durable.through_turn_sequence.saturating_add(1);
    if first.turn.turn_sequence != expected_first
        || pending
            .windows(2)
            .any(|pair| pair[1].turn.turn_sequence != pair[0].turn.turn_sequence.saturating_add(1))
    {
        let error = StoreError::NonContiguousTurn;
        shared.record_store_error(error.clone());
        return Err(error);
    }

    let next_epoch = durable
        .epoch
        .checked_add(1)
        .ok_or_else(|| StoreError::Backend("persistence epoch overflow".to_owned()))?;
    let last_turn_sequence = pending
        .last()
        .expect("first pending turn exists")
        .turn
        .turn_sequence;
    let first_turn_sequence = first.turn.turn_sequence;
    shared.begin_checkpoint_batch(pending.len());
    let result = (|| {
        let encode_started = Instant::now();
        let changes = pending
            .iter()
            .flat_map(|queued| queued.turn.changes.iter().cloned())
            .collect();
        let outbox_changes = pending
            .iter()
            .flat_map(|queued| queued.turn.outbox_changes.iter().cloned())
            .collect();
        let content_artifact_changes = pending
            .iter()
            .flat_map(|queued| queued.turn.content_artifact_changes.iter().cloned())
            .collect();
        let batch = CheckpointBatch {
            application: durable.application.clone(),
            schema_hash: durable.schema_hash,
            base_epoch: durable.epoch,
            next_epoch,
            first_turn_sequence,
            last_turn_sequence,
            changes,
            outbox_changes,
            content_artifact_changes,
            checksum: [0; 32],
        }
        .seal();
        shared
            .encode_us
            .store(duration_us(encode_started.elapsed()), Ordering::Release);

        let checkpoint_started = Instant::now();
        let result = driver.execute(PersistenceCommand::Commit(batch));
        shared
            .checkpoint_us
            .store(duration_us(checkpoint_started.elapsed()), Ordering::Release);
        match result {
            PersistenceResult::Committed(Ok(ack)) => {
                if ack.epoch != next_epoch || ack.through_turn_sequence != last_turn_sequence {
                    let error = StoreError::Backend(
                        "driver returned an inconsistent checkpoint acknowledgement".to_owned(),
                    );
                    shared.record_store_error(error.clone());
                    return Err(error);
                }
                shared.mark_durable(ack.epoch, ack.through_turn_sequence)?;
                durable.epoch = ack.epoch;
                durable.through_turn_sequence = ack.through_turn_sequence;
                pending.clear();
                Ok(())
            }
            PersistenceResult::Committed(Err(error)) => {
                shared.record_store_error(error.clone());
                Err(error)
            }
            _ => {
                let error =
                    StoreError::Backend("driver returned the wrong result for Commit".to_owned());
                shared.record_store_error(error.clone());
                Err(error)
            }
        }
    })();
    shared.clear_checkpoint_batch();
    result
}

fn commit_immediate_turn<D>(
    driver: &mut D,
    durable: &mut DurableCursor,
    turn: AuthorityTurn,
    shared: &SharedState,
) -> Result<CommitAck, StoreError>
where
    D: PersistenceDriver,
{
    let expected = durable
        .through_turn_sequence
        .checked_add(1)
        .ok_or_else(|| StoreError::Backend("persistence turn sequence overflow".to_owned()))?;
    if turn.turn_sequence != expected {
        return Err(StoreError::NonContiguousTurn);
    }
    let next_epoch = durable
        .epoch
        .checked_add(1)
        .ok_or_else(|| StoreError::Backend("persistence epoch overflow".to_owned()))?;
    let encode_started = Instant::now();
    let batch = CheckpointBatch {
        application: durable.application.clone(),
        schema_hash: durable.schema_hash,
        base_epoch: durable.epoch,
        next_epoch,
        first_turn_sequence: turn.turn_sequence,
        last_turn_sequence: turn.turn_sequence,
        changes: turn.changes,
        outbox_changes: turn.outbox_changes,
        content_artifact_changes: turn.content_artifact_changes,
        checksum: [0; 32],
    }
    .seal();
    shared
        .encode_us
        .store(duration_us(encode_started.elapsed()), Ordering::Release);
    let checkpoint_started = Instant::now();
    let driver_result = driver.execute(PersistenceCommand::Commit(batch));
    shared
        .checkpoint_us
        .store(duration_us(checkpoint_started.elapsed()), Ordering::Release);
    let result = match driver_result {
        PersistenceResult::Committed(result) => result,
        _ => Err(StoreError::Backend(
            "driver returned the wrong result for immediate Commit".to_owned(),
        )),
    }?;
    if result.epoch != next_epoch || result.through_turn_sequence != turn.turn_sequence {
        return Err(StoreError::Backend(
            "driver returned an inconsistent immediate checkpoint acknowledgement".to_owned(),
        ));
    }
    durable.epoch = result.epoch;
    durable.through_turn_sequence = result.through_turn_sequence;
    shared.mark_activation(result.epoch, result.through_turn_sequence);
    Ok(result)
}

fn handle_control<D>(
    driver: &mut D,
    durable: &mut DurableCursor,
    pending: &mut Vec<QueuedTurn>,
    message: WorkerMessage,
    shared: &SharedState,
) -> bool
where
    D: PersistenceDriver,
{
    debug_assert!(!matches!(&message, WorkerMessage::Turn(_)));
    if let Err(error) = flush_pending(driver, durable, pending, shared) {
        send_control_error(message, error, shared);
        return false;
    }

    match message {
        WorkerMessage::Load(reply) => {
            let request = RestoreRequest {
                application: durable.application.clone(),
                expected_schema_hash: Some(durable.schema_hash),
            };
            let result = match driver.execute(PersistenceCommand::Load(request)) {
                PersistenceResult::Loaded(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Load".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::Load(result));
            false
        }
        WorkerMessage::Barrier(reply) => {
            let request = BarrierRequest {
                application: durable.application.clone(),
                through_epoch: durable.epoch,
            };
            let started = Instant::now();
            let driver_result = driver.execute(PersistenceCommand::Barrier(request));
            shared
                .barrier_us
                .store(duration_us(started.elapsed()), Ordering::Release);
            let result = match driver_result {
                PersistenceResult::BarrierComplete(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Barrier".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::Barrier(result));
            false
        }
        WorkerMessage::ImmediateTurn(turn, reply) => {
            let result = commit_immediate_turn(driver, durable, *turn, shared);
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::ImmediateTurn(result));
            false
        }
        WorkerMessage::Inspect(reply) => {
            let request = InspectRequest {
                application: durable.application.clone(),
            };
            let result = match driver.execute(PersistenceCommand::Inspect(request)) {
                PersistenceResult::Inspected(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Inspect".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::Inspect(result));
            false
        }
        WorkerMessage::Activate(batch, reply) => {
            let started = Instant::now();
            let driver_result = driver.execute(PersistenceCommand::Activate(*batch));
            shared
                .migration_us
                .store(duration_us(started.elapsed()), Ordering::Release);
            let result = match driver_result {
                PersistenceResult::Activated(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Activate".to_owned(),
                )),
            };
            if let Ok(ack) = &result {
                durable.epoch = ack.epoch;
                durable.schema_hash = ack.schema_hash;
                durable.through_turn_sequence = ack.through_turn_sequence;
                shared.mark_activation(ack.epoch, ack.through_turn_sequence);
            }
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::Activate(result));
            false
        }
        WorkerMessage::ResetApplication(batch, reply) => {
            let result = match driver.execute(PersistenceCommand::ResetApplication(*batch)) {
                PersistenceResult::ApplicationReset(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for ResetApplication".to_owned(),
                )),
            };
            if let Ok(ack) = &result {
                durable.epoch = ack.epoch;
                durable.schema_hash = ack.schema_hash;
                durable.through_turn_sequence = ack.through_turn_sequence;
                shared.mark_activation(ack.epoch, ack.through_turn_sequence);
            }
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::ResetApplication(result));
            false
        }
        WorkerMessage::Compact(reply) => {
            let request = CompactRequest {
                application: durable.application.clone(),
            };
            let result = match driver.execute(PersistenceCommand::Compact(request)) {
                PersistenceResult::Compacted(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Compact".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::Compact(result));
            false
        }
        WorkerMessage::ExportApplication(reply) => {
            let result = match driver.execute(PersistenceCommand::ExportApplication(
                ExportApplicationRequest {
                    application: durable.application.clone(),
                },
            )) {
                PersistenceResult::ApplicationExported(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for ExportApplication".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::ExportApplication(result));
            false
        }
        WorkerMessage::PutContentArtifact(artifact, reply) => {
            let result = match driver.execute(PersistenceCommand::PutContentArtifact(
                PutContentArtifactRequest {
                    application: durable.application.clone(),
                    artifact: *artifact,
                },
            )) {
                PersistenceResult::ContentArtifactStored(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for PutContentArtifact".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::PutContentArtifact(result));
            false
        }
        WorkerMessage::LoadContentArtifact(id, reply) => {
            let result = match driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: durable.application.clone(),
                    id,
                },
            )) {
                PersistenceResult::ContentArtifactLoaded(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for LoadContentArtifact".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let _ = reply.send(ControlReply::LoadContentArtifact(result));
            false
        }
        WorkerMessage::PutContentArtifactAsync(_, _)
        | WorkerMessage::LoadContentArtifactAsync(_, _) => {
            unreachable!("async artifact work is dispatched without a checkpoint boundary")
        }
        WorkerMessage::Shutdown(reply) => {
            let result = match driver.execute(PersistenceCommand::Shutdown(ShutdownRequest)) {
                PersistenceResult::ShutdownComplete(result) => result,
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Shutdown".to_owned(),
                )),
            };
            record_result_error(shared, &result);
            let succeeded = result.is_ok();
            let _ = reply.send(ControlReply::Shutdown(result));
            succeeded
        }
        WorkerMessage::Turn(_) => unreachable!("turns are handled before control dispatch"),
    }
}

fn send_control_error(message: WorkerMessage, error: StoreError, shared: &SharedState) {
    match message {
        WorkerMessage::Load(reply) => {
            let _ = reply.send(ControlReply::Load(Err(error)));
        }
        WorkerMessage::Barrier(reply) => {
            let _ = reply.send(ControlReply::Barrier(Err(error)));
        }
        WorkerMessage::ImmediateTurn(_, reply) => {
            let _ = reply.send(ControlReply::ImmediateTurn(Err(error)));
        }
        WorkerMessage::Inspect(reply) => {
            let _ = reply.send(ControlReply::Inspect(Err(error)));
        }
        WorkerMessage::Activate(_, reply) => {
            let _ = reply.send(ControlReply::Activate(Err(error)));
        }
        WorkerMessage::ResetApplication(_, reply) => {
            let _ = reply.send(ControlReply::ResetApplication(Err(error)));
        }
        WorkerMessage::Compact(reply) => {
            let _ = reply.send(ControlReply::Compact(Err(error)));
        }
        WorkerMessage::ExportApplication(reply) => {
            let _ = reply.send(ControlReply::ExportApplication(Err(error)));
        }
        WorkerMessage::PutContentArtifact(_, reply) => {
            let _ = reply.send(ControlReply::PutContentArtifact(Err(error)));
        }
        WorkerMessage::PutContentArtifactAsync(ticket, _) => {
            lock(&shared.content_artifact_completions).push_back(ContentArtifactStoreCompletion {
                ticket,
                result: Err(error),
            });
            shared
                .pending_content_artifact_stores
                .fetch_sub(1, Ordering::AcqRel);
        }
        WorkerMessage::LoadContentArtifact(_, reply) => {
            let _ = reply.send(ControlReply::LoadContentArtifact(Err(error)));
        }
        WorkerMessage::LoadContentArtifactAsync(ticket, id) => {
            lock(&shared.content_artifact_load_completions).push_back(
                ContentArtifactLoadCompletion {
                    ticket,
                    id,
                    result: Err(error),
                },
            );
            shared
                .pending_content_artifact_loads
                .fetch_sub(1, Ordering::AcqRel);
        }
        WorkerMessage::Shutdown(reply) => {
            let _ = reply.send(ControlReply::Shutdown(Err(error)));
        }
        WorkerMessage::Turn(_) => unreachable!("turn is not a control message"),
    }
}

fn record_result_error<T>(shared: &SharedState, result: &Result<T, StoreError>) {
    if let Err(error) = result {
        shared.record_store_error(error.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryDriver, StoredScalar, StoredValue};
    use boon_plan::{MemoryId, MemoryKind, MemoryOwnerPath};
    use std::sync::Condvar;

    fn number(value: i64) -> StoredValue {
        StoredValue::integer(value).unwrap()
    }

    #[derive(Clone, Debug, Default)]
    struct DriverTrace {
        commits: Arc<Mutex<Vec<(u64, u64)>>>,
        shutdowns: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CommitGate {
        entered: Mutex<bool>,
        entered_changed: Condvar,
        released: Mutex<bool>,
        released_changed: Condvar,
    }

    impl CommitGate {
        fn new() -> Self {
            Self {
                entered: Mutex::new(false),
                entered_changed: Condvar::new(),
                released: Mutex::new(false),
                released_changed: Condvar::new(),
            }
        }

        fn wait_until_entered(&self) {
            let mut entered = lock(&self.entered);
            while !*entered {
                entered = self
                    .entered_changed
                    .wait(entered)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }

        fn release(&self) {
            *lock(&self.released) = true;
            self.released_changed.notify_all();
        }

        fn block_commit(&self) {
            *lock(&self.entered) = true;
            self.entered_changed.notify_all();
            let mut released = lock(&self.released);
            while !*released {
                released = self
                    .released_changed
                    .wait(released)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
    }

    struct RecordingDriver {
        inner: InMemoryDriver,
        trace: DriverTrace,
        commit_gate: Option<Arc<CommitGate>>,
        artifact_gate: Option<Arc<CommitGate>>,
        artifact_delay: Duration,
    }

    impl PersistenceDriver for RecordingDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(
                &command,
                PersistenceCommand::PutContentArtifact(_)
                    | PersistenceCommand::LoadContentArtifact(_)
            ) {
                if let Some(gate) = &self.artifact_gate {
                    gate.block_commit();
                }
                thread::sleep(self.artifact_delay);
            }
            if let PersistenceCommand::Commit(batch) = &command {
                if let Some(gate) = &self.commit_gate {
                    gate.block_commit();
                }
                lock(&self.trace.commits)
                    .push((batch.first_turn_sequence, batch.last_turn_sequence));
            }
            if matches!(command, PersistenceCommand::Shutdown(_)) {
                self.trace.shutdowns.fetch_add(1, Ordering::AcqRel);
            }
            self.inner.execute(command)
        }
    }

    struct FailNextCommitDriver {
        inner: InMemoryDriver,
        fail_next_commit: Arc<AtomicBool>,
    }

    impl PersistenceDriver for FailNextCommitDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
            if matches!(command, PersistenceCommand::Commit(_))
                && self.fail_next_commit.swap(false, Ordering::AcqRel)
            {
                return PersistenceResult::Committed(Err(StoreError::Backend(
                    "injected commit failure".to_owned(),
                )));
            }
            self.inner.execute(command)
        }
    }

    fn application() -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.persistence-worker", "test", "local")
    }

    fn initial_image() -> RestoreImage {
        RestoreImage::empty(application(), 1, [7; 32])
    }

    fn memory() -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "counter".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            "count",
            MemoryKind::Scalar,
        )
        .unwrap()
    }

    fn turn(sequence: u64, value: i64) -> AuthorityTurn {
        AuthorityTurn::new(
            sequence,
            vec![DurableChange::SetScalar {
                memory_id: memory(),
                value: StoredScalar {
                    touched: true,
                    value: number(value),
                },
            }],
        )
    }

    fn config(capacity: usize, coalesce_delay: Duration) -> PersistenceWorkerConfig {
        PersistenceWorkerConfig {
            queue_capacity: capacity,
            max_batch_turns: capacity,
            coalesce_delay,
        }
    }

    fn coordinator(
        capacity: usize,
        coalesce_delay: Duration,
        gate: Option<Arc<CommitGate>>,
    ) -> (PersistenceCoordinator, DriverTrace) {
        let trace = DriverTrace::default();
        let driver = RecordingDriver {
            inner: InMemoryDriver::default(),
            trace: trace.clone(),
            commit_gate: gate,
            artifact_gate: None,
            artifact_delay: Duration::ZERO,
        };
        let (coordinator, startup) = PersistenceCoordinator::start(
            driver,
            initial_image(),
            config(capacity, coalesce_delay),
        )
        .unwrap();
        assert!(startup.initialized);
        (coordinator, trace)
    }

    #[test]
    fn resolved_start_loads_before_adopting_or_initializing_authority() {
        let (fresh, startup, value) = PersistenceCoordinator::start_resolved(
            InMemoryDriver::default(),
            application(),
            config(4, Duration::ZERO),
            |loaded| -> Result<_, &'static str> {
                assert_eq!(loaded, None);
                Ok(PersistenceStartupResolution::Initialize {
                    initial_image: initial_image(),
                    value: 17_u32,
                })
            },
        )
        .unwrap();
        assert!(startup.initialized);
        assert_eq!(startup.restore_image, initial_image());
        assert_eq!(value, 17);
        fresh.shutdown().unwrap();

        let mut restored_image = initial_image();
        restored_image.epoch = 9;
        let mut driver = InMemoryDriver::default();
        driver.seed(restored_image.clone());
        let expected = restored_image.clone();
        let (restored, startup, value) = PersistenceCoordinator::start_resolved(
            driver,
            application(),
            config(4, Duration::ZERO),
            move |loaded| -> Result<_, &'static str> {
                assert_eq!(loaded.as_ref(), Some(&expected));
                Ok(PersistenceStartupResolution::AdoptLoaded {
                    restore_image: loaded.expect("loaded authority"),
                    value: 29_u32,
                })
            },
        )
        .unwrap();
        assert!(!startup.initialized);
        assert_eq!(startup.restore_image, restored_image);
        assert_eq!(value, 29);
        restored.shutdown().unwrap();
    }

    #[test]
    fn resolved_start_aborts_the_worker_when_runtime_resolution_fails() {
        let trace = DriverTrace::default();
        let driver = RecordingDriver {
            inner: InMemoryDriver::default(),
            trace: trace.clone(),
            commit_gate: None,
            artifact_gate: None,
            artifact_delay: Duration::ZERO,
        };
        let error = match PersistenceCoordinator::start_resolved(
            driver,
            application(),
            config(4, Duration::ZERO),
            |_loaded| -> Result<PersistenceStartupResolution<()>, _> {
                Err("runtime build failed")
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("failed runtime resolution must abort startup"),
        };
        assert!(matches!(
            error,
            PersistenceResolvedStartError::Resolver("runtime build failed")
        ));
        assert_eq!(trace.shutdowns.load(Ordering::Acquire), 1);
    }

    #[test]
    fn bounded_backpressure_returns_the_complete_turn_without_dropping_it() {
        let gate = Arc::new(CommitGate::new());
        let (coordinator, _trace) = coordinator(1, Duration::ZERO, Some(Arc::clone(&gate)));
        coordinator.try_enqueue_turn(turn(1, 10)).unwrap();
        gate.wait_until_entered();

        let rejected = match coordinator.try_enqueue_turn(turn(2, 20)) {
            Err(TurnEnqueueError::Backpressure { turn, .. }) => turn,
            other => panic!("expected visible backpressure, got {other:?}"),
        };
        assert_eq!(rejected, turn(2, 20));
        let status = coordinator.status();
        assert_eq!(
            status
                .pending
                .as_ref()
                .map(|pending| (pending.first_turn_sequence, pending.last_turn_sequence)),
            Some((1, 1))
        );
        assert_eq!(status.pending_checkpoint_batches, 1);

        gate.release();
        coordinator.barrier().unwrap();
        coordinator.try_enqueue_turn(rejected).unwrap();
        coordinator.barrier().unwrap();
        assert_eq!(coordinator.status().durable_through_turn_sequence, 2);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn status_counts_checkpoint_batches_instead_of_logical_turn_span() {
        let gate = Arc::new(CommitGate::new());
        let trace = DriverTrace::default();
        let driver = RecordingDriver {
            inner: InMemoryDriver::default(),
            trace,
            commit_gate: Some(Arc::clone(&gate)),
            artifact_gate: None,
            artifact_delay: Duration::ZERO,
        };
        let (coordinator, startup) = PersistenceCoordinator::start(
            driver,
            initial_image(),
            PersistenceWorkerConfig {
                queue_capacity: 4,
                max_batch_turns: 2,
                coalesce_delay: Duration::ZERO,
            },
        )
        .unwrap();
        assert!(startup.initialized);

        coordinator.try_enqueue_turn(turn(1, 1)).unwrap();
        gate.wait_until_entered();
        coordinator.try_enqueue_turn(turn(2, 2)).unwrap();
        coordinator.try_enqueue_turn(turn(3, 3)).unwrap();

        let status = coordinator.status();
        assert_eq!(
            status
                .pending
                .as_ref()
                .map(|pending| (pending.first_turn_sequence, pending.last_turn_sequence)),
            Some((1, 3))
        );
        assert_eq!(status.pending_checkpoint_batches, 2);
        assert!(status.checkpoint_batch_in_flight);
        assert_eq!(status.queued_checkpoint_batches, 1);
        assert_eq!(status.pending_checkpoint_batches_peak, 2);

        gate.release();
        coordinator.barrier().unwrap();
        let status = coordinator.status();
        assert_eq!(status.pending_checkpoint_batches, 0);
        assert!(!status.checkpoint_batch_in_flight);
        assert_eq!(status.queued_checkpoint_batches, 0);
        assert_eq!(status.pending_checkpoint_batches_peak, 2);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn worker_batches_only_complete_contiguous_turns() {
        let (coordinator, trace) = coordinator(8, Duration::from_millis(50), None);
        coordinator.try_enqueue_turn(turn(1, 1)).unwrap();
        coordinator.try_enqueue_turn(turn(2, 2)).unwrap();
        coordinator.try_enqueue_turn(turn(3, 3)).unwrap();
        coordinator.barrier().unwrap();

        assert_eq!(&*lock(&trace.commits), &[(1, 3)]);
        assert_eq!(coordinator.status().durable_through_turn_sequence, 3);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn worker_batches_content_artifact_changes_with_their_authority_turns() {
        let (coordinator, trace) = coordinator(8, Duration::from_millis(50), None);
        let first = ContentArtifact::new("text/plain", b"first owner".to_vec()).unwrap();
        let second = ContentArtifact::new("text/plain", b"second owner".to_vec()).unwrap();
        coordinator.put_content_artifact(first.clone()).unwrap();
        coordinator.put_content_artifact(second.clone()).unwrap();
        let first_owner = super::super::ContentArtifactOwnerId([0x21; 32]);
        let second_owner = super::super::ContentArtifactOwnerId([0x22; 32]);
        coordinator
            .try_enqueue_turn(
                AuthorityTurn::new(1, Vec::new()).with_content_artifact_changes(vec![
                    DurableContentArtifactChange::SetReplaceable {
                        owner_id: first_owner,
                        artifact_id: first.id,
                    },
                ]),
            )
            .unwrap();
        coordinator
            .try_enqueue_turn(
                AuthorityTurn::new(2, Vec::new()).with_content_artifact_changes(vec![
                    DurableContentArtifactChange::InsertImmutable {
                        owner_id: second_owner,
                        artifact_id: second.id,
                    },
                ]),
            )
            .unwrap();
        coordinator.barrier().unwrap();

        assert_eq!(&*lock(&trace.commits), &[(1, 2)]);
        let restored = coordinator.load().unwrap().unwrap();
        assert_eq!(
            restored.content_artifact_manifest.bindings[&first_owner].artifact_id,
            first.id
        );
        assert_eq!(
            restored.content_artifact_manifest.bindings[&second_owner].artifact_id,
            second.id
        );
        let snapshot = coordinator.inspect().unwrap().unwrap();
        assert_eq!(snapshot.content_artifact_owner_count, 2);
        assert_eq!(snapshot.content_artifact_retained_count, 2);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn barrier_flushes_every_preceding_accepted_turn() {
        let (coordinator, trace) = coordinator(4, Duration::from_secs(1), None);
        coordinator.try_enqueue_turn(turn(1, 11)).unwrap();
        let acknowledgement = coordinator.barrier().unwrap();

        assert_eq!(acknowledgement.epoch, 1);
        assert_eq!(&*lock(&trace.commits), &[(1, 1)]);
        let inspected = coordinator.inspect().unwrap().unwrap();
        assert_eq!(inspected.through_turn_sequence, 1);
        assert!(coordinator.status().pending.is_none());
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn immediate_turn_flushes_older_work_then_commits_in_its_own_checkpoint() {
        let (coordinator, trace) = coordinator(4, Duration::from_secs(1), None);
        coordinator.try_enqueue_turn(turn(1, 11)).unwrap();

        let acknowledgement = coordinator.commit_immediate(turn(2, 22)).unwrap();

        assert_eq!(acknowledgement.through_turn_sequence, 2);
        assert_eq!(&*lock(&trace.commits), &[(1, 1), (2, 2)]);
        assert_eq!(coordinator.status().durable_through_turn_sequence, 2);
        assert!(coordinator.status().pending.is_none());
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn content_artifact_store_is_acknowledged_without_pausing_turn_admission() {
        let (coordinator, _trace) = coordinator(4, Duration::ZERO, None);
        let artifact = ContentArtifact::new(
            "application/vnd.boon.worker-test",
            b"background artifact".to_vec(),
        )
        .unwrap();
        let ticket = coordinator
            .try_put_content_artifact(artifact.clone())
            .unwrap();
        let reservation = coordinator
            .try_reserve_turn()
            .expect("artifact storage must not pause authority admission");
        drop(reservation);

        let deadline = Instant::now() + Duration::from_secs(1);
        let completion = loop {
            if let Some(completion) = coordinator
                .take_content_artifact_store_completions()
                .into_iter()
                .next()
            {
                break completion;
            }
            assert!(
                Instant::now() < deadline,
                "artifact store was not acknowledged"
            );
            thread::yield_now();
        };
        assert_eq!(completion.ticket, ticket);
        assert_eq!(completion.result.unwrap().id, artifact.id);
        assert_eq!(coordinator.status().pending_content_artifact_stores, 0);
        assert_eq!(
            coordinator.load_content_artifact(artifact.id).unwrap(),
            Some(artifact)
        );
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn asynchronous_artifact_work_does_not_split_an_authority_batch() {
        let trace = DriverTrace::default();
        let driver = RecordingDriver {
            inner: InMemoryDriver::default(),
            trace: trace.clone(),
            commit_gate: None,
            artifact_gate: None,
            artifact_delay: Duration::from_millis(150),
        };
        let (coordinator, startup) = PersistenceCoordinator::start(
            driver,
            initial_image(),
            config(8, Duration::from_millis(100)),
        )
        .unwrap();
        assert!(startup.initialized);
        let artifact = ContentArtifact::new(
            "application/vnd.boon.worker-test",
            b"interleaved background artifact".to_vec(),
        )
        .unwrap();

        coordinator.try_enqueue_turn(turn(1, 11)).unwrap();
        let ticket = coordinator
            .try_put_content_artifact(artifact.clone())
            .unwrap();
        coordinator.try_enqueue_turn(turn(2, 22)).unwrap();
        coordinator.barrier().unwrap();

        assert_eq!(&*lock(&trace.commits), &[(1, 2)]);
        let completion = coordinator
            .take_content_artifact_store_completions()
            .into_iter()
            .find(|completion| completion.ticket == ticket)
            .expect("interleaved artifact store must complete");
        assert_eq!(completion.result.unwrap().id, artifact.id);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn sealed_authority_is_durable_before_slow_artifact_work_runs() {
        let trace = DriverTrace::default();
        let artifact_gate = Arc::new(CommitGate::new());
        let driver = RecordingDriver {
            inner: InMemoryDriver::default(),
            trace: trace.clone(),
            commit_gate: None,
            artifact_gate: Some(Arc::clone(&artifact_gate)),
            artifact_delay: Duration::ZERO,
        };
        let (coordinator, _) = PersistenceCoordinator::start(
            driver,
            initial_image(),
            config(8, Duration::from_millis(100)),
        )
        .unwrap();
        let artifact = ContentArtifact::new("application/test", vec![1]).unwrap();

        coordinator.try_enqueue_turn(turn(1, 11)).unwrap();
        coordinator.try_put_content_artifact(artifact).unwrap();
        artifact_gate.wait_until_entered();
        assert_eq!(coordinator.status().durable_through_turn_sequence, 1);
        coordinator.try_enqueue_turn(turn(2, 22)).unwrap();
        let status = coordinator.status();
        assert_eq!(status.pending_checkpoint_batches, 1);
        assert_eq!(status.pending_checkpoint_batches_peak, 1);

        artifact_gate.release();
        coordinator.barrier().unwrap();
        assert_eq!(&*lock(&trace.commits), &[(1, 1), (2, 2)]);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn content_artifact_load_is_acknowledged_without_pausing_turn_admission() {
        let (coordinator, _trace) = coordinator(4, Duration::ZERO, None);
        let artifact = ContentArtifact::new(
            "application/vnd.boon.worker-test",
            b"background artifact load".to_vec(),
        )
        .unwrap();
        coordinator.put_content_artifact(artifact.clone()).unwrap();

        let ticket = coordinator.try_load_content_artifact(artifact.id).unwrap();
        let reservation = coordinator
            .try_reserve_turn()
            .expect("artifact loading must not pause authority admission");
        drop(reservation);

        let deadline = Instant::now() + Duration::from_secs(1);
        let completion = loop {
            if let Some(completion) = coordinator
                .take_content_artifact_load_completions()
                .into_iter()
                .next()
            {
                break completion;
            }
            assert!(
                Instant::now() < deadline,
                "artifact load was not acknowledged"
            );
            thread::yield_now();
        };
        assert_eq!(completion.ticket, ticket);
        assert_eq!(completion.id, artifact.id);
        assert_eq!(completion.result.unwrap(), Some(artifact));
        assert_eq!(coordinator.status().pending_content_artifact_loads, 0);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn failed_immediate_turn_is_not_retained_as_pending_work() {
        let fail_next_commit = Arc::new(AtomicBool::new(false));
        let driver = FailNextCommitDriver {
            inner: InMemoryDriver::default(),
            fail_next_commit: Arc::clone(&fail_next_commit),
        };
        let (coordinator, startup) =
            PersistenceCoordinator::start(driver, initial_image(), config(4, Duration::ZERO))
                .unwrap();
        assert!(startup.initialized);
        coordinator.commit_immediate(turn(1, 11)).unwrap();
        fail_next_commit.store(true, Ordering::Release);

        assert!(coordinator.commit_immediate(turn(2, 22)).is_err());
        assert_eq!(coordinator.status().durable_through_turn_sequence, 1);
        assert!(coordinator.status().pending.is_none());
        let durable = coordinator.load().unwrap().unwrap();
        assert_eq!(durable.through_turn_sequence, 1);
        assert_eq!(
            durable.scalars.get(&memory()).map(|value| &value.value),
            Some(&number(11))
        );

        coordinator.commit_immediate(turn(2, 22)).unwrap();
        assert_eq!(coordinator.status().durable_through_turn_sequence, 2);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn a_reserved_turn_remains_admissible_while_a_barrier_waits() {
        let (coordinator, trace) = coordinator(2, Duration::ZERO, None);
        let coordinator = Arc::new(coordinator);
        let reservation = coordinator.try_reserve_turn().unwrap();
        let barrier_coordinator = Arc::clone(&coordinator);
        let barrier = thread::spawn(move || barrier_coordinator.barrier());
        let deadline = Instant::now() + Duration::from_secs(1);
        while coordinator.status().accepting_turns {
            assert!(Instant::now() < deadline, "barrier did not pause admission");
            thread::yield_now();
        }

        reservation.enqueue(turn(1, 17)).unwrap();
        barrier.join().unwrap().unwrap();
        assert_eq!(&*lock(&trace.commits), &[(1, 1)]);
        assert_eq!(coordinator.status().durable_through_turn_sequence, 1);
        coordinator.shutdown().unwrap();
    }

    #[test]
    fn shutdown_flushes_and_joins_the_worker() {
        let (coordinator, trace) = coordinator(4, Duration::from_secs(1), None);
        coordinator.try_enqueue_turn(turn(1, 9)).unwrap();
        coordinator.shutdown().unwrap();

        assert_eq!(trace.shutdowns.load(Ordering::Acquire), 1);
        let status = coordinator.status();
        assert_eq!(status.durable_through_turn_sequence, 1);
        assert!(!status.worker_alive);
        assert!(!status.accepting_turns);
        assert!(matches!(
            coordinator.try_enqueue_turn(turn(2, 10)),
            Err(TurnEnqueueError::Closed { .. })
        ));
    }
}
