use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::time::{ClockCore, duration_from_nanoseconds};
use crate::{HostLimit, HostServiceError, HostServiceLimits, MonotonicSnapshot};

const TIMER_ACTIVE: u8 = 0;
const TIMER_CANCELLED: u8 = 1;
const TIMER_COMPLETED: u8 = 2;
const TIMER_SCHEDULER_SHUTDOWN: u8 = 3;

const SCHEDULER_RUNNING: u8 = 0;
const SCHEDULER_SHUTTING_DOWN: u8 = 1;
const SCHEDULER_STOPPED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerState {
    Active,
    Cancelled,
    Completed,
    SchedulerShutdown,
}

impl TimerState {
    fn from_raw(raw: u8) -> Self {
        match raw {
            TIMER_ACTIVE => Self::Active,
            TIMER_CANCELLED => Self::Cancelled,
            TIMER_COMPLETED => Self::Completed,
            TIMER_SCHEDULER_SHUTDOWN => Self::SchedulerShutdown,
            _ => unreachable!("timer state is always a known constant"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleEventKind {
    Deadline,
    Interval {
        sequence: u64,
        skipped_intervals: u64,
    },
}

/// A timer event with explicit clock ownership and nanosecond units through its
/// monotonic snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleEvent {
    scheduled_for: MonotonicSnapshot,
    observed_at: MonotonicSnapshot,
    kind: ScheduleEventKind,
}

impl ScheduleEvent {
    pub const fn scheduled_for(self) -> MonotonicSnapshot {
        self.scheduled_for
    }

    pub const fn observed_at(self) -> MonotonicSnapshot {
        self.observed_at
    }

    pub const fn kind(self) -> ScheduleEventKind {
        self.kind
    }

    pub(crate) const fn new(
        scheduled_for: MonotonicSnapshot,
        observed_at: MonotonicSnapshot,
        kind: ScheduleEventKind,
    ) -> Self {
        Self {
            scheduled_for,
            observed_at,
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationOutcome {
    Cancelled,
    AlreadyCancelled,
    AlreadyCompleted,
    SchedulerShutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerReceiveError {
    Empty,
    Timeout,
    Cancelled,
    Completed,
    SchedulerShutdown,
}

impl fmt::Display for TimerReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("no timer event is currently available"),
            Self::Timeout => formatter.write_str("timed out waiting for a timer event"),
            Self::Cancelled => formatter.write_str("timer was cancelled"),
            Self::Completed => formatter.write_str("timer completed"),
            Self::SchedulerShutdown => formatter.write_str("timer's scheduler shut down"),
        }
    }
}

impl std::error::Error for TimerReceiveError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TimerId(u64);

pub(crate) struct TimerControl {
    state: AtomicU8,
    slot_released: AtomicBool,
    delivery: Mutex<()>,
}

impl TimerControl {
    fn active() -> Self {
        Self {
            state: AtomicU8::new(TIMER_ACTIVE),
            slot_released: AtomicBool::new(false),
            delivery: Mutex::new(()),
        }
    }

    fn state(&self) -> TimerState {
        TimerState::from_raw(self.state.load(Ordering::Acquire))
    }

    fn release_slot(&self, active_count: &AtomicUsize) {
        if !self.slot_released.swap(true, Ordering::AcqRel) {
            active_count.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub(crate) trait CancellationBackend: Send + Sync {
    fn cancel(&self, timer_id: TimerId, control: &TimerControl);
}

/// A clonable cancellation handle. Cancellation is idempotent and does not
/// remove an event already delivered into the timer's one-event queue.
#[derive(Clone)]
pub struct CancellationHandle {
    timer_id: TimerId,
    control: Arc<TimerControl>,
    backend: Arc<dyn CancellationBackend>,
}

impl CancellationHandle {
    pub fn cancel(&self) -> CancellationOutcome {
        let delivery = self.control.delivery.lock().expect("timer delivery lock");
        match self.control.state() {
            TimerState::Active => {
                self.control.state.store(TIMER_CANCELLED, Ordering::Release);
                drop(delivery);
                self.backend.cancel(self.timer_id, &self.control);
                CancellationOutcome::Cancelled
            }
            TimerState::Cancelled => CancellationOutcome::AlreadyCancelled,
            TimerState::Completed => CancellationOutcome::AlreadyCompleted,
            TimerState::SchedulerShutdown => CancellationOutcome::SchedulerShutdown,
        }
    }

    pub fn state(&self) -> TimerState {
        self.control.state()
    }
}

impl fmt::Debug for CancellationHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationHandle")
            .field("timer_id", &self.timer_id.0)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

/// Receiver and cancellation ownership for one scheduled timer.
pub struct ScheduledTimer {
    receiver: Receiver<ScheduleEvent>,
    cancellation: CancellationHandle,
}

impl ScheduledTimer {
    pub fn cancellation_handle(&self) -> CancellationHandle {
        self.cancellation.clone()
    }

    pub fn cancel(&self) -> CancellationOutcome {
        self.cancellation.cancel()
    }

    pub fn state(&self) -> TimerState {
        self.cancellation.state()
    }

    pub fn try_recv(&self) -> Result<ScheduleEvent, TimerReceiveError> {
        match self.receiver.try_recv() {
            Ok(event) => Ok(event),
            Err(TryRecvError::Empty) => Err(self.empty_error(TimerReceiveError::Empty)),
            Err(TryRecvError::Disconnected) => Err(self.empty_error(TimerReceiveError::Completed)),
        }
    }

    pub fn recv(&self) -> Result<ScheduleEvent, TimerReceiveError> {
        match self.receiver.try_recv() {
            Ok(event) => return Ok(event),
            Err(TryRecvError::Disconnected) => {
                return Err(self.empty_error(TimerReceiveError::Completed));
            }
            Err(TryRecvError::Empty) => {}
        }
        let state_error = self.empty_error(TimerReceiveError::Empty);
        if state_error != TimerReceiveError::Empty {
            return Err(state_error);
        }
        self.receiver
            .recv()
            .map_err(|_| self.empty_error(TimerReceiveError::Completed))
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<ScheduleEvent, TimerReceiveError> {
        match self.receiver.try_recv() {
            Ok(event) => return Ok(event),
            Err(TryRecvError::Disconnected) => {
                return Err(self.empty_error(TimerReceiveError::Completed));
            }
            Err(TryRecvError::Empty) => {}
        }
        let state_error = self.empty_error(TimerReceiveError::Empty);
        if state_error != TimerReceiveError::Empty {
            return Err(state_error);
        }
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => Ok(event),
            Err(RecvTimeoutError::Timeout) => Err(self.empty_error(TimerReceiveError::Timeout)),
            Err(RecvTimeoutError::Disconnected) => {
                Err(self.empty_error(TimerReceiveError::Completed))
            }
        }
    }

    fn empty_error(&self, active_error: TimerReceiveError) -> TimerReceiveError {
        match self.state() {
            TimerState::Active => active_error,
            TimerState::Cancelled => TimerReceiveError::Cancelled,
            TimerState::Completed => TimerReceiveError::Completed,
            TimerState::SchedulerShutdown => TimerReceiveError::SchedulerShutdown,
        }
    }
}

impl fmt::Debug for ScheduledTimer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScheduledTimer")
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

impl Drop for ScheduledTimer {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct SchedulerShared {
    lifecycle: AtomicU8,
    active_count: AtomicUsize,
}

struct SystemCancellationBackend {
    command_sender: mpsc::Sender<SystemCommand>,
    shared: Arc<SchedulerShared>,
}

impl CancellationBackend for SystemCancellationBackend {
    fn cancel(&self, timer_id: TimerId, control: &TimerControl) {
        if self
            .command_sender
            .send(SystemCommand::Cancel(timer_id))
            .is_err()
        {
            control.release_slot(&self.shared.active_count);
        }
    }
}

pub(crate) struct SystemScheduler {
    clock: ClockCore,
    limits: HostServiceLimits,
    next_timer_id: AtomicU64,
    command_sender: mpsc::Sender<SystemCommand>,
    cancellation_backend: Arc<SystemCancellationBackend>,
    shared: Arc<SchedulerShared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl SystemScheduler {
    pub(crate) fn new(clock: ClockCore, limits: HostServiceLimits) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let shared = Arc::new(SchedulerShared {
            lifecycle: AtomicU8::new(SCHEDULER_RUNNING),
            active_count: AtomicUsize::new(0),
        });
        let worker_shared = Arc::clone(&shared);
        let worker_clock = clock.clone();
        let worker = thread::Builder::new()
            .name("boon-host-scheduler".to_owned())
            .spawn(move || scheduler_worker(worker_clock, command_receiver, worker_shared))
            .expect("failed to spawn boon host scheduler worker");
        let cancellation_backend = Arc::new(SystemCancellationBackend {
            command_sender: command_sender.clone(),
            shared: Arc::clone(&shared),
        });

        Self {
            clock,
            limits,
            next_timer_id: AtomicU64::new(1),
            command_sender,
            cancellation_backend,
            shared,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub(crate) fn schedule_deadline_at(
        &self,
        deadline: MonotonicSnapshot,
    ) -> Result<ScheduledTimer, HostServiceError> {
        let due_instant = self.clock.instant_for(deadline)?;
        let now = self.clock.monotonic_now();
        let horizon = if deadline.checked_cmp(now)?.is_ge() {
            deadline.duration_since(now)?
        } else {
            Duration::ZERO
        };
        self.enforce_maximum_duration(
            HostLimit::DeadlineHorizon,
            horizon,
            self.limits.max_deadline_horizon(),
        )?;
        self.schedule(SystemTimerKind::Deadline, due_instant, deadline)
    }

    pub(crate) fn schedule_deadline_after(
        &self,
        delay: Duration,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.enforce_maximum_duration(
            HostLimit::DeadlineHorizon,
            delay,
            self.limits.max_deadline_horizon(),
        )?;
        let now = self.clock.monotonic_now();
        let deadline = now.checked_add(delay).ok_or(HostServiceError::Time(
            crate::TimeError::DurationOutOfRange {
                nanoseconds: now
                    .nanoseconds_since_clock_origin()
                    .saturating_add(delay.as_nanos()),
            },
        ))?;
        let due_instant = self.clock.instant_for(deadline)?;
        self.schedule(SystemTimerKind::Deadline, due_instant, deadline)
    }

    pub(crate) fn schedule_interval(
        &self,
        period: Duration,
    ) -> Result<ScheduledTimer, HostServiceError> {
        if period < self.limits.minimum_interval() {
            return Err(HostServiceError::BelowMinimum {
                limit: HostLimit::MinimumInterval,
                requested: period.as_nanos(),
                minimum: self.limits.minimum_interval().as_nanos(),
            });
        }
        self.enforce_maximum_duration(
            HostLimit::MaximumInterval,
            period,
            self.limits.maximum_interval(),
        )?;
        let now = self.clock.monotonic_now();
        let first_tick = now.checked_add(period).ok_or(HostServiceError::Time(
            crate::TimeError::DurationOutOfRange {
                nanoseconds: now
                    .nanoseconds_since_clock_origin()
                    .saturating_add(period.as_nanos()),
            },
        ))?;
        let due_instant = self.clock.instant_for(first_tick)?;
        self.schedule(
            SystemTimerKind::Interval {
                period,
                next_sequence: 1,
            },
            due_instant,
            first_tick,
        )
    }

    pub(crate) fn shutdown(&self) {
        if self
            .shared
            .lifecycle
            .compare_exchange(
                SCHEDULER_RUNNING,
                SCHEDULER_SHUTTING_DOWN,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            let _ = self.command_sender.send(SystemCommand::Shutdown);
        }

        let mut worker_slot = self.worker.lock().expect("scheduler worker lock");
        if let Some(worker) = worker_slot.take() {
            let _ = worker.join();
        }
        self.shared
            .lifecycle
            .store(SCHEDULER_STOPPED, Ordering::Release);
    }

    fn schedule(
        &self,
        kind: SystemTimerKind,
        due_instant: Instant,
        scheduled_for: MonotonicSnapshot,
    ) -> Result<ScheduledTimer, HostServiceError> {
        let timer_id = TimerId(
            self.next_timer_id
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
                .map_err(|_| HostServiceError::SchedulerUnavailable)?,
        );
        self.reserve_slot()?;
        let control = Arc::new(TimerControl::active());
        let (event_sender, receiver) = mpsc::sync_channel(1);
        let task = SystemTask {
            control: Arc::clone(&control),
            event_sender,
            due_instant,
            scheduled_for,
            kind,
        };

        if self
            .command_sender
            .send(SystemCommand::Add(timer_id, task))
            .is_err()
        {
            control
                .state
                .store(TIMER_SCHEDULER_SHUTDOWN, Ordering::Release);
            control.release_slot(&self.shared.active_count);
            return Err(HostServiceError::SchedulerUnavailable);
        }

        Ok(ScheduledTimer {
            receiver,
            cancellation: CancellationHandle {
                timer_id,
                control,
                backend: self.cancellation_backend.clone(),
            },
        })
    }

    fn reserve_slot(&self) -> Result<(), HostServiceError> {
        if self.shared.lifecycle.load(Ordering::Acquire) != SCHEDULER_RUNNING {
            return Err(HostServiceError::Shutdown);
        }
        loop {
            let active = self.shared.active_count.load(Ordering::Acquire);
            if active >= self.limits.max_concurrent_scheduled_timers() {
                return Err(HostServiceError::LimitExceeded {
                    limit: HostLimit::ConcurrentScheduledTimers,
                    requested: active as u128 + 1,
                    maximum: self.limits.max_concurrent_scheduled_timers() as u128,
                });
            }
            if self
                .shared
                .active_count
                .compare_exchange_weak(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                if self.shared.lifecycle.load(Ordering::Acquire) == SCHEDULER_RUNNING {
                    return Ok(());
                }
                self.shared.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(HostServiceError::Shutdown);
            }
        }
    }

    fn enforce_maximum_duration(
        &self,
        limit: HostLimit,
        requested: Duration,
        maximum: Duration,
    ) -> Result<(), HostServiceError> {
        if requested > maximum {
            return Err(HostServiceError::LimitExceeded {
                limit,
                requested: requested.as_nanos(),
                maximum: maximum.as_nanos(),
            });
        }
        Ok(())
    }
}

impl Drop for SystemScheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum SystemCommand {
    Add(TimerId, SystemTask),
    Cancel(TimerId),
    Shutdown,
}

enum SystemTimerKind {
    Deadline,
    Interval {
        period: Duration,
        next_sequence: u64,
    },
}

struct SystemTask {
    control: Arc<TimerControl>,
    event_sender: SyncSender<ScheduleEvent>,
    due_instant: Instant,
    scheduled_for: MonotonicSnapshot,
    kind: SystemTimerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DueEntry {
    instant: Instant,
    timer_id: TimerId,
}

impl Ord for DueEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other
            .instant
            .cmp(&self.instant)
            .then_with(|| other.timer_id.0.cmp(&self.timer_id.0))
    }
}

impl PartialOrd for DueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

fn scheduler_worker(
    clock: ClockCore,
    command_receiver: Receiver<SystemCommand>,
    shared: Arc<SchedulerShared>,
) {
    let mut tasks = HashMap::<TimerId, SystemTask>::new();
    let mut due = BinaryHeap::<DueEntry>::new();

    loop {
        fire_due_tasks(&clock, &shared, &mut tasks, &mut due);
        let command = match due.peek() {
            Some(entry) => match command_receiver
                .recv_timeout(entry.instant.saturating_duration_since(Instant::now()))
            {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => None,
            },
            None => command_receiver.recv().ok(),
        };

        match command {
            Some(SystemCommand::Add(timer_id, task)) => {
                if shared.lifecycle.load(Ordering::Acquire) == SCHEDULER_RUNNING
                    && task.control.state() == TimerState::Active
                {
                    due.push(DueEntry {
                        instant: task.due_instant,
                        timer_id,
                    });
                    tasks.insert(timer_id, task);
                } else {
                    mark_scheduler_shutdown(&task.control, &shared.active_count);
                }
            }
            Some(SystemCommand::Cancel(timer_id)) => {
                if let Some(task) = tasks.remove(&timer_id) {
                    task.control.release_slot(&shared.active_count);
                }
            }
            Some(SystemCommand::Shutdown) | None => break,
        }
    }

    for task in tasks.into_values() {
        mark_scheduler_shutdown(&task.control, &shared.active_count);
    }
    shared.lifecycle.store(SCHEDULER_STOPPED, Ordering::Release);
}

fn fire_due_tasks(
    clock: &ClockCore,
    shared: &SchedulerShared,
    tasks: &mut HashMap<TimerId, SystemTask>,
    due: &mut BinaryHeap<DueEntry>,
) {
    while due
        .peek()
        .is_some_and(|entry| entry.instant <= Instant::now())
    {
        let entry = due.pop().expect("peeked due entry exists");
        let Some(mut task) = tasks.remove(&entry.timer_id) else {
            continue;
        };
        if task.due_instant != entry.instant {
            tasks.insert(entry.timer_id, task);
            continue;
        }
        if task.control.state() != TimerState::Active {
            task.control.release_slot(&shared.active_count);
            continue;
        }

        let observed_at = clock.monotonic_now();
        match task.kind {
            SystemTimerKind::Deadline => {
                deliver_deadline_event(
                    &task.control,
                    &task.event_sender,
                    ScheduleEvent {
                        scheduled_for: task.scheduled_for,
                        observed_at,
                        kind: ScheduleEventKind::Deadline,
                    },
                );
                task.control.release_slot(&shared.active_count);
            }
            SystemTimerKind::Interval {
                period,
                next_sequence,
            } => {
                let elapsed_periods =
                    elapsed_interval_count(task.scheduled_for, observed_at, period);
                let skipped_intervals =
                    u64::try_from(elapsed_periods.saturating_sub(1)).unwrap_or(u64::MAX);
                let event = ScheduleEvent {
                    scheduled_for: task.scheduled_for,
                    observed_at,
                    kind: ScheduleEventKind::Interval {
                        sequence: next_sequence,
                        skipped_intervals,
                    },
                };
                deliver_interval_event(&task.control, &task.event_sender, event);

                if task.control.state() == TimerState::Active {
                    let advance_nanoseconds = period.as_nanos().saturating_mul(elapsed_periods);
                    if let Ok(advance) = duration_from_nanoseconds(advance_nanoseconds)
                        && let (Some(next_snapshot), Some(next_instant)) = (
                            task.scheduled_for.checked_add(advance),
                            task.due_instant.checked_add(advance),
                        )
                    {
                        task.scheduled_for = next_snapshot;
                        task.due_instant = next_instant;
                        task.kind = SystemTimerKind::Interval {
                            period,
                            next_sequence: next_sequence
                                .saturating_add(u64::try_from(elapsed_periods).unwrap_or(u64::MAX)),
                        };
                        due.push(DueEntry {
                            instant: task.due_instant,
                            timer_id: entry.timer_id,
                        });
                        tasks.insert(entry.timer_id, task);
                        continue;
                    }
                    task.control.state.store(TIMER_COMPLETED, Ordering::Release);
                }
                task.control.release_slot(&shared.active_count);
            }
        }
    }
}

fn elapsed_interval_count(
    scheduled_for: MonotonicSnapshot,
    observed_at: MonotonicSnapshot,
    period: Duration,
) -> u128 {
    let late_nanoseconds = observed_at
        .nanoseconds_since_clock_origin()
        .saturating_sub(scheduled_for.nanoseconds_since_clock_origin());
    late_nanoseconds / period.as_nanos() + 1
}

fn mark_scheduler_shutdown(control: &TimerControl, active_count: &AtomicUsize) {
    let _delivery = control.delivery.lock().expect("timer delivery lock");
    if control.state() == TimerState::Active {
        control
            .state
            .store(TIMER_SCHEDULER_SHUTDOWN, Ordering::Release);
    }
    control.release_slot(active_count);
}

pub(crate) fn deliver_deadline_event(
    control: &TimerControl,
    event_sender: &SyncSender<ScheduleEvent>,
    event: ScheduleEvent,
) {
    let _delivery = control.delivery.lock().expect("timer delivery lock");
    if control.state() == TimerState::Active {
        control.state.store(TIMER_COMPLETED, Ordering::Release);
        let _ = event_sender.try_send(event);
    }
}

pub(crate) fn deliver_interval_event(
    control: &TimerControl,
    event_sender: &SyncSender<ScheduleEvent>,
    event: ScheduleEvent,
) {
    let _delivery = control.delivery.lock().expect("timer delivery lock");
    if control.state() != TimerState::Active {
        return;
    }
    if matches!(
        event_sender.try_send(event),
        Err(TrySendError::Disconnected(_))
    ) {
        control.state.store(TIMER_CANCELLED, Ordering::Release);
    }
}

pub(crate) fn deterministic_timer(
    timer_id: u64,
    control: Arc<TimerControl>,
    receiver: Receiver<ScheduleEvent>,
    backend: Arc<dyn CancellationBackend>,
) -> ScheduledTimer {
    ScheduledTimer {
        receiver,
        cancellation: CancellationHandle {
            timer_id: TimerId(timer_id),
            control,
            backend,
        },
    }
}

pub(crate) fn new_timer_control() -> Arc<TimerControl> {
    Arc::new(TimerControl::active())
}

pub(crate) fn set_timer_completed(control: &TimerControl) {
    let _delivery = control.delivery.lock().expect("timer delivery lock");
    if control.state() == TimerState::Active {
        control.state.store(TIMER_COMPLETED, Ordering::Release);
    }
}

pub(crate) fn set_timer_scheduler_shutdown(control: &TimerControl) {
    let _delivery = control.delivery.lock().expect("timer delivery lock");
    if control.state() == TimerState::Active {
        control
            .state
            .store(TIMER_SCHEDULER_SHUTDOWN, Ordering::Release);
    }
}

pub(crate) fn timer_is_active(control: &TimerControl) -> bool {
    control.state() == TimerState::Active
}

pub(crate) fn sync_event_channel() -> (SyncSender<ScheduleEvent>, Receiver<ScheduleEvent>) {
    mpsc::sync_channel(1)
}

pub(crate) fn timer_id_value(timer_id: TimerId) -> u64 {
    timer_id.0
}
