use super::{
    ActivationAck, ActivationBatch, BarrierAck, BarrierRequest, CheckpointBatch, CommitAck,
    CompactAck, CompactRequest, DurableChange, DurableOutboxChange, InspectRequest,
    PersistenceCommand, PersistenceDriver, PersistenceInspectorSnapshot, PersistenceResult,
    ResetApplicationAck, ResetApplicationBatch, RestoreImage, RestoreRequest, ShutdownAck,
    ShutdownRequest, StoreError,
};
use boon_plan::ApplicationIdentity;
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityTurn {
    pub turn_sequence: u64,
    pub changes: Vec<DurableChange>,
    pub outbox_changes: Vec<DurableOutboxChange>,
}

impl AuthorityTurn {
    pub fn new(turn_sequence: u64, changes: Vec<DurableChange>) -> Self {
        Self {
            turn_sequence,
            changes,
            outbox_changes: Vec::new(),
        }
    }

    pub fn with_outbox_changes(mut self, changes: Vec<DurableOutboxChange>) -> Self {
        self.outbox_changes = changes;
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
    pub durable_epoch: u64,
    pub durable_through_turn_sequence: u64,
    /// Turns waiting in the channel. Worker-owned batch turns remain visible in
    /// `pending` but are not included here.
    pub queue_depth: usize,
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
}

#[derive(Debug)]
struct SharedState {
    capacity: usize,
    admission: Mutex<AdmissionState>,
    reservation_changed: Condvar,
    queue_depth: AtomicUsize,
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
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            admission: Mutex::new(AdmissionState {
                accepting: false,
                closed: false,
                reservations: 0,
                next_turn_sequence: 1,
                pending: VecDeque::new(),
            }),
            reservation_changed: Condvar::new(),
            queue_depth: AtomicUsize::new(0),
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
        self.durable_epoch.store(epoch, Ordering::Release);
        self.durable_turn
            .store(through_turn_sequence, Ordering::Release);
        Ok(())
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
            durable_epoch: self.durable_epoch.load(Ordering::Acquire),
            durable_through_turn_sequence: self.durable_turn.load(Ordering::Acquire),
            queue_depth: self.queue_depth.load(Ordering::Acquire),
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
                Ok(()) => Ok(()),
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

impl PersistenceCoordinator {
    pub fn start<D>(
        driver: D,
        initial_image: RestoreImage,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, PersistenceStartup), PersistenceWorkerStartError>
    where
        D: PersistenceDriver + Send + 'static,
    {
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

        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let shared = Arc::new(SharedState::new(config.queue_capacity));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("boon-persistence".to_owned())
            .spawn(move || {
                worker_main(
                    driver,
                    initial_image,
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
    initial_image: RestoreImage,
    config: PersistenceWorkerConfig,
    receiver: Receiver<WorkerMessage>,
    startup_sender: SyncSender<Result<PersistenceStartup, StoreError>>,
    shared: Arc<SharedState>,
) where
    D: PersistenceDriver,
{
    let restore_started = Instant::now();
    let startup = start_driver(&mut driver, initial_image);
    shared
        .restore_us
        .store(duration_us(restore_started.elapsed()), Ordering::Release);
    let startup = match startup {
        Ok(startup) => startup,
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
                collect_batch(&receiver, &config, &mut pending, &mut deferred, &shared);
                let _ = flush_pending(&mut driver, &mut durable, &mut pending, &shared);
            }
            control => {
                let should_stop =
                    handle_control(&mut driver, &mut durable, &mut pending, control, &shared);
                if should_stop {
                    break;
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
    let request = RestoreRequest {
        application: initial_image.application.clone(),
        expected_schema_hash: None,
    };
    match driver.execute(PersistenceCommand::Load(request)) {
        PersistenceResult::Loaded(Ok(Some(image))) => {
            if image.application != initial_image.application {
                return Err(StoreError::IdentityMismatch);
            }
            Ok(PersistenceStartup {
                restore_image: image,
                initialized: false,
            })
        }
        PersistenceResult::Loaded(Ok(None)) => {
            match driver.execute(PersistenceCommand::Initialize(initial_image.clone())) {
                PersistenceResult::Initialized(Ok(_)) => Ok(PersistenceStartup {
                    restore_image: initial_image,
                    initialized: true,
                }),
                PersistenceResult::Initialized(Err(error)) => Err(error),
                _ => Err(StoreError::Backend(
                    "driver returned the wrong result for Initialize".to_owned(),
                )),
            }
        }
        PersistenceResult::Loaded(Err(error)) => Err(error),
        _ => Err(StoreError::Backend(
            "driver returned the wrong result for Load".to_owned(),
        )),
    }
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

fn collect_batch(
    receiver: &Receiver<WorkerMessage>,
    config: &PersistenceWorkerConfig,
    pending: &mut Vec<QueuedTurn>,
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
    let encode_started = Instant::now();
    let changes = pending
        .iter()
        .flat_map(|queued| queued.turn.changes.iter().cloned())
        .collect();
    let outbox_changes = pending
        .iter()
        .flat_map(|queued| queued.turn.outbox_changes.iter().cloned())
        .collect();
    let batch = CheckpointBatch {
        application: durable.application.clone(),
        schema_hash: durable.schema_hash,
        base_epoch: durable.epoch,
        next_epoch,
        first_turn_sequence: first.turn.turn_sequence,
        last_turn_sequence,
        changes,
        outbox_changes,
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
        send_control_error(message, error);
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

fn send_control_error(message: WorkerMessage, error: StoreError) {
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
    }

    impl PersistenceDriver for RecordingDriver {
        fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
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
                    value: StoredValue::Number(value),
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

        gate.release();
        coordinator.barrier().unwrap();
        coordinator.try_enqueue_turn(rejected).unwrap();
        coordinator.barrier().unwrap();
        assert_eq!(coordinator.status().durable_through_turn_sequence, 2);
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
            Some(&StoredValue::Number(11))
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
