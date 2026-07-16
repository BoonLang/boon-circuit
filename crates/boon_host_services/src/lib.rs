//! Capability-gated host primitives for native Boon hosts.
//!
//! [`HostServices`] always uses production providers: process-local monotonic
//! time, Unix wall time, a bounded system scheduler, and the operating system's
//! secure random source. Deterministic providers are available only through the
//! explicitly named [`testing`] module.

mod config;
mod error;
mod random;
mod scheduler;
mod secrets;
mod time;

pub mod testing;

use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

pub use config::{
    HostCapabilities, HostCapability, HostLimit, HostLimitUnit, HostServiceConfig,
    HostServiceLimits,
};
pub use error::{ConfigError, HostServiceError, TimeError};
pub use random::RandomBytes;
pub use scheduler::{
    CancellationHandle, CancellationOutcome, ScheduleEvent, ScheduleEventKind, ScheduledTimer,
    TimerReceiveError, TimerState,
};
pub use secrets::{
    HMAC_SHA256_TAG_BYTES, HmacSha256Tag, SecretId, SecretMaterial, SecretRef, SecretStoreId,
    Verification,
};
pub use time::{ClockId, MonotonicSnapshot, WallClockSnapshot};

use random::OsRandom;
use scheduler::SystemScheduler;
use secrets::SecretStore;
use time::SystemClock;

static NEXT_HOST_ID: AtomicU64 = AtomicU64::new(1);

const HOST_RUNNING: u8 = 0;
const HOST_SHUT_DOWN: u8 = 1;

/// Production host services bound to system providers.
pub struct HostServices {
    config: HostServiceConfig,
    lifecycle: AtomicU8,
    clock: SystemClock,
    scheduler: Option<SystemScheduler>,
    random: OsRandom,
    secrets: SecretStore,
}

impl HostServices {
    /// Creates a production host using OS clocks and secure randomness.
    pub fn new(config: HostServiceConfig) -> Self {
        let host_id = NEXT_HOST_ID.fetch_add(1, Ordering::Relaxed);
        let clock = SystemClock::new(ClockId::new(host_id));
        let scheduler = config
            .capabilities()
            .allows(HostCapability::Scheduling)
            .then(|| SystemScheduler::new(clock.core(), config.limits().clone()));

        Self {
            config,
            lifecycle: AtomicU8::new(HOST_RUNNING),
            clock,
            scheduler,
            random: OsRandom,
            secrets: SecretStore::new(SecretStoreId::new(host_id)),
        }
    }

    /// Returns this host's immutable configuration.
    pub fn config(&self) -> &HostServiceConfig {
        &self.config
    }

    /// Captures process-local monotonic time in nanoseconds since this host's
    /// private clock origin.
    pub fn monotonic_now(&self) -> Result<MonotonicSnapshot, HostServiceError> {
        self.require(HostCapability::MonotonicClock)?;
        Ok(self.clock.monotonic_now())
    }

    /// Captures wall time as Unix-epoch seconds plus nanoseconds.
    pub fn wall_clock_now(&self) -> Result<WallClockSnapshot, HostServiceError> {
        self.require(HostCapability::WallClock)?;
        Ok(self.clock.wall_clock_now())
    }

    /// Schedules one event at a monotonic snapshot owned by this host.
    pub fn schedule_deadline_at(
        &self,
        deadline: MonotonicSnapshot,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        self.scheduler
            .as_ref()
            .expect("enabled scheduling has a scheduler")
            .schedule_deadline_at(deadline)
    }

    /// Schedules one event after an explicit duration.
    pub fn schedule_deadline_after(
        &self,
        delay: std::time::Duration,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        self.scheduler
            .as_ref()
            .expect("enabled scheduling has a scheduler")
            .schedule_deadline_after(delay)
    }

    /// Schedules a bounded interval. The first event arrives after `period`.
    pub fn schedule_interval(
        &self,
        period: std::time::Duration,
    ) -> Result<ScheduledTimer, HostServiceError> {
        self.require(HostCapability::Scheduling)?;
        self.scheduler
            .as_ref()
            .expect("enabled scheduling has a scheduler")
            .schedule_interval(period)
    }

    /// Returns bytes from the operating system's secure random source.
    pub fn secure_random(&self, byte_count: usize) -> Result<RandomBytes, HostServiceError> {
        self.require(HostCapability::SecureRandom)?;
        random::fill_bounded(&self.random, byte_count, self.config.limits())
    }

    /// Moves secret bytes into this host and returns an opaque reference.
    pub fn configure_secret(
        &mut self,
        material: SecretMaterial,
    ) -> Result<SecretRef, HostServiceError> {
        self.require(HostCapability::SecretStore)?;
        self.secrets.insert(material, self.config.limits())
    }

    /// Removes a configured secret. Its storage is zeroized on drop.
    pub fn remove_secret(&mut self, secret_ref: SecretRef) -> Result<bool, HostServiceError> {
        self.require(HostCapability::SecretStore)?;
        self.secrets.remove(secret_ref)
    }

    /// Verifies a candidate against a configured secret through a fixed-width,
    /// constant-time authentication-tag comparison.
    pub fn verify_configured_secret(
        &self,
        secret_ref: SecretRef,
        candidate: &[u8],
    ) -> Result<Verification, HostServiceError> {
        self.require(HostCapability::SecretVerification)?;
        self.secrets
            .verify(secret_ref, candidate, self.config.limits())
    }

    /// Signs a bounded message with HMAC-SHA256 using a configured secret.
    pub fn hmac_sha256_sign(
        &self,
        secret_ref: SecretRef,
        message: &[u8],
    ) -> Result<HmacSha256Tag, HostServiceError> {
        self.require(HostCapability::HmacSha256)?;
        self.secrets
            .hmac_sha256_sign(secret_ref, message, self.config.limits())
    }

    /// Verifies an HMAC-SHA256 tag in constant time.
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

    /// Stops the scheduler, joins its worker, and rejects subsequent calls.
    /// This operation is idempotent.
    pub fn shutdown(&self) {
        let _ = self.lifecycle.compare_exchange(
            HOST_RUNNING,
            HOST_SHUT_DOWN,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if let Some(scheduler) = &self.scheduler {
            scheduler.shutdown();
        }
    }

    /// Reports whether shutdown has completed or started.
    pub fn is_shutdown(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == HOST_SHUT_DOWN
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
}

impl Default for HostServices {
    fn default() -> Self {
        Self::new(HostServiceConfig::default())
    }
}

impl Drop for HostServices {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl fmt::Debug for HostServices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostServices")
            .field("config", &self.config)
            .field("is_shutdown", &self.is_shutdown())
            .field("clock", &self.clock)
            .field("secrets", &self.secrets)
            .finish_non_exhaustive()
    }
}
