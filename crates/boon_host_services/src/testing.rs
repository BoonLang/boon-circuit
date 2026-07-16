//! Deterministic providers for tests and simulations.
//!
//! These types are deliberately separate from [`crate::HostServices`]. The
//! production constructor cannot accept a deterministic clock or random source.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc::SyncSender};
use std::time::Duration;

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::random::{RandomProvider, fill_bounded};
use crate::scheduler::{
    CancellationBackend, ScheduleEvent, ScheduleEventKind, ScheduledTimer, TimerControl, TimerId,
    deliver_deadline_event, deliver_interval_event, deterministic_timer, new_timer_control,
    set_timer_completed, set_timer_scheduler_shutdown, sync_event_channel, timer_id_value,
    timer_is_active,
};
use crate::secrets::SecretStore;
use crate::{
    ClockId, HmacSha256Tag, HostCapability, HostLimit, HostServiceConfig, HostServiceError,
    MonotonicSnapshot, RandomBytes, SecretMaterial, SecretRef, SecretStoreId, Verification,
    WallClockSnapshot,
};

const TEST_HOST_RUNNING: u8 = 0;
const TEST_HOST_SHUT_DOWN: u8 = 1;

/// Deterministic provider inputs for one test host.
pub struct DeterministicProviderConfig {
    host_instance_id: u64,
    initial_wall_clock: WallClockSnapshot,
    random_seed: Zeroizing<[u8; 32]>,
}

impl DeterministicProviderConfig {
    pub fn new(
        host_instance_id: u64,
        initial_wall_clock: WallClockSnapshot,
        random_seed: [u8; 32],
    ) -> Self {
        Self {
            host_instance_id,
            initial_wall_clock,
            random_seed: Zeroizing::new(random_seed),
        }
    }

    pub const fn host_instance_id(&self) -> u64 {
        self.host_instance_id
    }

    pub const fn initial_wall_clock(&self) -> WallClockSnapshot {
        self.initial_wall_clock
    }
}

impl fmt::Debug for DeterministicProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeterministicProviderConfig")
            .field("host_instance_id", &self.host_instance_id)
            .field("initial_wall_clock", &self.initial_wall_clock)
            .field("random_seed", &"[REDACTED]")
            .finish()
    }
}

struct DeterministicClock {
    clock_id: ClockId,
    monotonic_nanoseconds: u128,
    wall_clock: WallClockSnapshot,
}

impl DeterministicClock {
    fn monotonic_now(&self) -> MonotonicSnapshot {
        MonotonicSnapshot::new(self.clock_id, self.monotonic_nanoseconds)
    }

    fn wall_clock_now(&self) -> WallClockSnapshot {
        self.wall_clock
    }

    fn advance(&mut self, duration: Duration) -> Result<MonotonicSnapshot, HostServiceError> {
        let monotonic_nanoseconds = self
            .monotonic_nanoseconds
            .checked_add(duration.as_nanos())
            .ok_or(HostServiceError::Time(
                crate::TimeError::DurationOutOfRange {
                    nanoseconds: u128::MAX,
                },
            ))?;
        let wall_clock = self
            .wall_clock
            .checked_add(duration)
            .ok_or(HostServiceError::Time(
                crate::TimeError::DurationOutOfRange {
                    nanoseconds: duration.as_nanos(),
                },
            ))?;
        self.monotonic_nanoseconds = monotonic_nanoseconds;
        self.wall_clock = wall_clock;
        Ok(self.monotonic_now())
    }
}

struct DeterministicRandom {
    seed: Zeroizing<[u8; 32]>,
    next_block: Mutex<u64>,
}

impl RandomProvider for DeterministicRandom {
    fn fill(&self, destination: &mut [u8]) -> Result<(), HostServiceError> {
        let mut next_block = self.next_block.lock().expect("deterministic random lock");
        for chunk in destination.chunks_mut(32) {
            let block_number = *next_block;
            *next_block = next_block
                .checked_add(1)
                .ok_or(HostServiceError::DeterministicRandomExhausted)?;
            let mut hash = Sha256::new();
            hash.update(self.seed.as_slice());
            hash.update(block_number.to_be_bytes());
            let block = hash.finalize();
            chunk.copy_from_slice(&block[..chunk.len()]);
        }
        Ok(())
    }
}

enum DeterministicTimerKind {
    Deadline,
    Interval {
        period: Duration,
        next_sequence: u64,
    },
}

struct DeterministicTask {
    control: Arc<TimerControl>,
    event_sender: SyncSender<ScheduleEvent>,
    scheduled_for: MonotonicSnapshot,
    kind: DeterministicTimerKind,
}

struct DeterministicSchedulerState {
    shutdown: bool,
    next_timer_id: u64,
    tasks: HashMap<u64, DeterministicTask>,
}

struct DeterministicCancellationBackend {
    state: Weak<Mutex<DeterministicSchedulerState>>,
}

impl CancellationBackend for DeterministicCancellationBackend {
    fn cancel(&self, timer_id: TimerId, _control: &TimerControl) {
        if let Some(state) = self.state.upgrade() {
            state
                .lock()
                .expect("deterministic scheduler lock")
                .tasks
                .remove(&timer_id_value(timer_id));
        }
    }
}

struct DeterministicScheduler {
    state: Arc<Mutex<DeterministicSchedulerState>>,
    cancellation_backend: Arc<DeterministicCancellationBackend>,
}

impl DeterministicScheduler {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(DeterministicSchedulerState {
            shutdown: false,
            next_timer_id: 1,
            tasks: HashMap::new(),
        }));
        Self {
            cancellation_backend: Arc::new(DeterministicCancellationBackend {
                state: Arc::downgrade(&state),
            }),
            state,
        }
    }

    fn schedule_deadline(
        &self,
        scheduled_for: MonotonicSnapshot,
        max_timers: usize,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.schedule(scheduled_for, DeterministicTimerKind::Deadline, max_timers)
    }

    fn schedule_interval(
        &self,
        scheduled_for: MonotonicSnapshot,
        period: Duration,
        max_timers: usize,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.schedule(
            scheduled_for,
            DeterministicTimerKind::Interval {
                period,
                next_sequence: 1,
            },
            max_timers,
        )
    }

    fn schedule(
        &self,
        scheduled_for: MonotonicSnapshot,
        kind: DeterministicTimerKind,
        max_timers: usize,
    ) -> Result<ScheduledTimer, HostServiceError> {
        let mut state = self.state.lock().expect("deterministic scheduler lock");
        if state.shutdown {
            return Err(HostServiceError::Shutdown);
        }
        if state.tasks.len() >= max_timers {
            return Err(HostServiceError::LimitExceeded {
                limit: HostLimit::ConcurrentScheduledTimers,
                requested: state.tasks.len() as u128 + 1,
                maximum: max_timers as u128,
            });
        }
        let timer_id = state.next_timer_id;
        state.next_timer_id = state
            .next_timer_id
            .checked_add(1)
            .ok_or(HostServiceError::SchedulerUnavailable)?;
        let control = new_timer_control();
        let (event_sender, receiver) = sync_event_channel();
        state.tasks.insert(
            timer_id,
            DeterministicTask {
                control: Arc::clone(&control),
                event_sender,
                scheduled_for,
                kind,
            },
        );
        drop(state);

        Ok(deterministic_timer(
            timer_id,
            control,
            receiver,
            self.cancellation_backend.clone(),
        ))
    }

    fn advance_to(&self, observed_at: MonotonicSnapshot) {
        let mut state = self.state.lock().expect("deterministic scheduler lock");
        if state.shutdown {
            return;
        }
        let due_ids: Vec<_> = state
            .tasks
            .iter()
            .filter_map(|(timer_id, task)| {
                (task.scheduled_for.nanoseconds_since_clock_origin()
                    <= observed_at.nanoseconds_since_clock_origin())
                .then_some(*timer_id)
            })
            .collect();

        for timer_id in due_ids {
            let Some(mut task) = state.tasks.remove(&timer_id) else {
                continue;
            };
            if !timer_is_active(&task.control) {
                continue;
            }
            match task.kind {
                DeterministicTimerKind::Deadline => {
                    deliver_deadline_event(
                        &task.control,
                        &task.event_sender,
                        ScheduleEvent::new(
                            task.scheduled_for,
                            observed_at,
                            ScheduleEventKind::Deadline,
                        ),
                    );
                }
                DeterministicTimerKind::Interval {
                    period,
                    next_sequence,
                } => {
                    let elapsed_periods = observed_at
                        .nanoseconds_since_clock_origin()
                        .saturating_sub(task.scheduled_for.nanoseconds_since_clock_origin())
                        / period.as_nanos()
                        + 1;
                    let event = ScheduleEvent::new(
                        task.scheduled_for,
                        observed_at,
                        ScheduleEventKind::Interval {
                            sequence: next_sequence,
                            skipped_intervals: u64::try_from(elapsed_periods.saturating_sub(1))
                                .unwrap_or(u64::MAX),
                        },
                    );
                    deliver_interval_event(&task.control, &task.event_sender, event);
                    if timer_is_active(&task.control) {
                        let advance_nanoseconds = period.as_nanos().saturating_mul(elapsed_periods);
                        if let Ok(advance) =
                            crate::time::duration_from_nanoseconds(advance_nanoseconds)
                            && let Some(next) = task.scheduled_for.checked_add(advance)
                        {
                            task.scheduled_for = next;
                            task.kind = DeterministicTimerKind::Interval {
                                period,
                                next_sequence: next_sequence.saturating_add(
                                    u64::try_from(elapsed_periods).unwrap_or(u64::MAX),
                                ),
                            };
                            state.tasks.insert(timer_id, task);
                            continue;
                        }
                        set_timer_completed(&task.control);
                    }
                }
            }
        }
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().expect("deterministic scheduler lock");
        if state.shutdown {
            return;
        }
        state.shutdown = true;
        for task in state.tasks.drain().map(|(_, task)| task) {
            set_timer_scheduler_shutdown(&task.control);
        }
    }
}

/// Fully deterministic host services for tests. Time advances only through
/// [`Self::advance`], and random output is a SHA-256 counter stream seeded by
/// [`DeterministicProviderConfig`].
pub struct DeterministicHostServices {
    config: HostServiceConfig,
    lifecycle: AtomicU8,
    clock: DeterministicClock,
    scheduler: DeterministicScheduler,
    random: DeterministicRandom,
    secrets: SecretStore,
}

impl DeterministicHostServices {
    pub fn new(config: HostServiceConfig, providers: DeterministicProviderConfig) -> Self {
        let host_instance_id = providers.host_instance_id;
        Self {
            config,
            lifecycle: AtomicU8::new(TEST_HOST_RUNNING),
            clock: DeterministicClock {
                clock_id: ClockId::new(host_instance_id),
                monotonic_nanoseconds: 0,
                wall_clock: providers.initial_wall_clock,
            },
            scheduler: DeterministicScheduler::new(),
            random: DeterministicRandom {
                seed: providers.random_seed,
                next_block: Mutex::new(0),
            },
            secrets: SecretStore::new(SecretStoreId::new(host_instance_id)),
        }
    }

    pub fn config(&self) -> &HostServiceConfig {
        &self.config
    }

    pub fn monotonic_now(&self) -> Result<MonotonicSnapshot, HostServiceError> {
        self.require(HostCapability::MonotonicClock)?;
        Ok(self.clock.monotonic_now())
    }

    pub fn wall_clock_now(&self) -> Result<WallClockSnapshot, HostServiceError> {
        self.require(HostCapability::WallClock)?;
        Ok(self.clock.wall_clock_now())
    }

    /// Advances both the host-owned monotonic clock and Unix wall clock by an
    /// explicit duration, then emits every now-due timer event.
    pub fn advance(&mut self, duration: Duration) -> Result<MonotonicSnapshot, HostServiceError> {
        if self.is_shutdown() {
            return Err(HostServiceError::Shutdown);
        }
        let now = self.clock.advance(duration)?;
        self.scheduler.advance_to(now);
        Ok(now)
    }

    pub fn schedule_deadline_at(
        &self,
        deadline: MonotonicSnapshot,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        let now = self.clock.monotonic_now();
        if deadline.clock_id() != now.clock_id() {
            return Err(crate::TimeError::ClockOwnerMismatch {
                expected: now.clock_id(),
                actual: deadline.clock_id(),
            }
            .into());
        }
        let horizon = if deadline.checked_cmp(now)?.is_ge() {
            deadline.duration_since(now)?
        } else {
            Duration::ZERO
        };
        self.enforce_maximum_duration(
            HostLimit::DeadlineHorizon,
            horizon,
            self.config.limits().max_deadline_horizon(),
        )?;
        self.scheduler.schedule_deadline(
            deadline,
            self.config.limits().max_concurrent_scheduled_timers(),
        )
    }

    pub fn schedule_deadline_after(
        &self,
        delay: Duration,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        self.enforce_maximum_duration(
            HostLimit::DeadlineHorizon,
            delay,
            self.config.limits().max_deadline_horizon(),
        )?;
        let now = self.clock.monotonic_now();
        let deadline = now.checked_add(delay).ok_or(HostServiceError::Time(
            crate::TimeError::DurationOutOfRange {
                nanoseconds: now
                    .nanoseconds_since_clock_origin()
                    .saturating_add(delay.as_nanos()),
            },
        ))?;
        self.scheduler.schedule_deadline(
            deadline,
            self.config.limits().max_concurrent_scheduled_timers(),
        )
    }

    pub fn schedule_interval(&self, period: Duration) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        if period < self.config.limits().minimum_interval() {
            return Err(HostServiceError::BelowMinimum {
                limit: HostLimit::MinimumInterval,
                requested: period.as_nanos(),
                minimum: self.config.limits().minimum_interval().as_nanos(),
            });
        }
        self.enforce_maximum_duration(
            HostLimit::MaximumInterval,
            period,
            self.config.limits().maximum_interval(),
        )?;
        let first_tick = self
            .clock
            .monotonic_now()
            .checked_add(period)
            .ok_or(HostServiceError::SchedulerUnavailable)?;
        self.scheduler.schedule_interval(
            first_tick,
            period,
            self.config.limits().max_concurrent_scheduled_timers(),
        )
    }

    pub fn secure_random(&self, byte_count: usize) -> Result<RandomBytes, HostServiceError> {
        self.require(HostCapability::SecureRandom)?;
        fill_bounded(&self.random, byte_count, self.config.limits())
    }

    pub fn configure_secret(
        &mut self,
        material: SecretMaterial,
    ) -> Result<SecretRef, HostServiceError> {
        self.require(HostCapability::SecretStore)?;
        self.secrets.insert(material, self.config.limits())
    }

    pub fn remove_secret(&mut self, secret_ref: SecretRef) -> Result<bool, HostServiceError> {
        self.require(HostCapability::SecretStore)?;
        self.secrets.remove(secret_ref)
    }

    pub fn verify_configured_secret(
        &self,
        secret_ref: SecretRef,
        candidate: &[u8],
    ) -> Result<Verification, HostServiceError> {
        self.require(HostCapability::SecretVerification)?;
        self.secrets
            .verify(secret_ref, candidate, self.config.limits())
    }

    pub fn hmac_sha256_sign(
        &self,
        secret_ref: SecretRef,
        message: &[u8],
    ) -> Result<HmacSha256Tag, HostServiceError> {
        self.require(HostCapability::HmacSha256)?;
        self.secrets
            .hmac_sha256_sign(secret_ref, message, self.config.limits())
    }

    pub fn hmac_sha256_verify(
        &self,
        secret_ref: SecretRef,
        message: &[u8],
        tag: &HmacSha256Tag,
    ) -> Result<Verification, HostServiceError> {
        self.require(HostCapability::HmacSha256)?;
        self.secrets
            .hmac_sha256_verify(secret_ref, message, tag, self.config.limits())
    }

    pub fn shutdown(&self) {
        let _ = self.lifecycle.compare_exchange(
            TEST_HOST_RUNNING,
            TEST_HOST_SHUT_DOWN,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.scheduler.shutdown();
    }

    pub fn is_shutdown(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == TEST_HOST_SHUT_DOWN
    }

    fn require(&self, capability: HostCapability) -> Result<(), HostServiceError> {
        if self.is_shutdown() {
            return Err(HostServiceError::Shutdown);
        }
        if !self.config.capabilities().allows(capability) {
            return Err(HostServiceError::CapabilityDisabled(capability));
        }
        Ok(())
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

impl Drop for DeterministicHostServices {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl fmt::Debug for DeterministicHostServices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeterministicHostServices")
            .field("config", &self.config)
            .field("is_shutdown", &self.is_shutdown())
            .field("clock_id", &self.clock.clock_id)
            .field("monotonic_nanoseconds", &self.clock.monotonic_nanoseconds)
            .field("wall_clock", &self.clock.wall_clock)
            .field("random_seed", &"[REDACTED]")
            .field("secrets", &self.secrets)
            .finish_non_exhaustive()
    }
}
