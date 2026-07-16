use std::time::Duration;

use crate::ConfigError;

/// A host operation that can be independently enabled or denied.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum HostCapability {
    MonotonicClock = 0,
    WallClock = 1,
    Scheduling = 2,
    SecureRandom = 3,
    SecretStore = 4,
    SecretVerification = 5,
    HmacSha256 = 6,
}

impl HostCapability {
    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// An immutable set of granted host capabilities.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct HostCapabilities {
    bits: u8,
}

impl HostCapabilities {
    pub const NONE: Self = Self { bits: 0 };
    pub const ALL: Self = Self { bits: (1 << 7) - 1 };

    /// Returns a set with `capability` granted.
    pub const fn with(mut self, capability: HostCapability) -> Self {
        self.bits |= capability.bit();
        self
    }

    /// Returns a set with `capability` denied.
    pub const fn without(mut self, capability: HostCapability) -> Self {
        self.bits &= !capability.bit();
        self
    }

    /// Reports whether `capability` is granted.
    pub const fn allows(self, capability: HostCapability) -> bool {
        self.bits & capability.bit() != 0
    }
}

impl Default for HostCapabilities {
    fn default() -> Self {
        Self::ALL
    }
}

impl std::fmt::Debug for HostCapabilities {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut set = formatter.debug_set();
        for capability in [
            HostCapability::MonotonicClock,
            HostCapability::WallClock,
            HostCapability::Scheduling,
            HostCapability::SecureRandom,
            HostCapability::SecretStore,
            HostCapability::SecretVerification,
            HostCapability::HmacSha256,
        ] {
            if self.allows(capability) {
                set.entry(&capability);
            }
        }
        set.finish()
    }
}

/// Named configurable limits used in errors and diagnostics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HostLimit {
    RandomBytesPerRequest,
    ConcurrentScheduledTimers,
    MinimumInterval,
    MaximumInterval,
    DeadlineHorizon,
    ConfiguredSecrets,
    SecretBytes,
    VerificationCandidateBytes,
    HmacMessageBytes,
}

impl HostLimit {
    pub const fn unit(self) -> HostLimitUnit {
        match self {
            Self::ConcurrentScheduledTimers | Self::ConfiguredSecrets => HostLimitUnit::Items,
            Self::MinimumInterval | Self::MaximumInterval | Self::DeadlineHorizon => {
                HostLimitUnit::Nanoseconds
            }
            Self::RandomBytesPerRequest
            | Self::SecretBytes
            | Self::VerificationCandidateBytes
            | Self::HmacMessageBytes => HostLimitUnit::Bytes,
        }
    }
}

/// Unit attached to every reported host limit value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HostLimitUnit {
    Bytes,
    Items,
    Nanoseconds,
}

/// Resource and duration limits enforced by all providers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostServiceLimits {
    max_random_bytes_per_request: usize,
    max_concurrent_scheduled_timers: usize,
    minimum_interval: Duration,
    maximum_interval: Duration,
    max_deadline_horizon: Duration,
    max_configured_secrets: usize,
    max_secret_bytes: usize,
    max_verification_candidate_bytes: usize,
    max_hmac_message_bytes: usize,
}

impl HostServiceLimits {
    pub fn with_max_random_bytes_per_request(mut self, byte_count: usize) -> Self {
        self.max_random_bytes_per_request = byte_count;
        self
    }

    pub fn with_max_concurrent_scheduled_timers(mut self, timer_count: usize) -> Self {
        self.max_concurrent_scheduled_timers = timer_count;
        self
    }

    pub fn with_interval_bounds(mut self, minimum: Duration, maximum: Duration) -> Self {
        self.minimum_interval = minimum;
        self.maximum_interval = maximum;
        self
    }

    pub fn with_max_deadline_horizon(mut self, horizon: Duration) -> Self {
        self.max_deadline_horizon = horizon;
        self
    }

    pub fn with_max_configured_secrets(mut self, secret_count: usize) -> Self {
        self.max_configured_secrets = secret_count;
        self
    }

    pub fn with_max_secret_bytes(mut self, byte_count: usize) -> Self {
        self.max_secret_bytes = byte_count;
        self
    }

    pub fn with_max_verification_candidate_bytes(mut self, byte_count: usize) -> Self {
        self.max_verification_candidate_bytes = byte_count;
        self
    }

    pub fn with_max_hmac_message_bytes(mut self, byte_count: usize) -> Self {
        self.max_hmac_message_bytes = byte_count;
        self
    }

    pub const fn max_random_bytes_per_request(&self) -> usize {
        self.max_random_bytes_per_request
    }

    pub const fn max_concurrent_scheduled_timers(&self) -> usize {
        self.max_concurrent_scheduled_timers
    }

    pub const fn minimum_interval(&self) -> Duration {
        self.minimum_interval
    }

    pub const fn maximum_interval(&self) -> Duration {
        self.maximum_interval
    }

    pub const fn max_deadline_horizon(&self) -> Duration {
        self.max_deadline_horizon
    }

    pub const fn max_configured_secrets(&self) -> usize {
        self.max_configured_secrets
    }

    pub const fn max_secret_bytes(&self) -> usize {
        self.max_secret_bytes
    }

    pub const fn max_verification_candidate_bytes(&self) -> usize {
        self.max_verification_candidate_bytes
    }

    pub const fn max_hmac_message_bytes(&self) -> usize {
        self.max_hmac_message_bytes
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        for (limit, value) in [
            (
                HostLimit::RandomBytesPerRequest,
                self.max_random_bytes_per_request,
            ),
            (
                HostLimit::ConcurrentScheduledTimers,
                self.max_concurrent_scheduled_timers,
            ),
            (HostLimit::ConfiguredSecrets, self.max_configured_secrets),
            (HostLimit::SecretBytes, self.max_secret_bytes),
            (
                HostLimit::VerificationCandidateBytes,
                self.max_verification_candidate_bytes,
            ),
            (HostLimit::HmacMessageBytes, self.max_hmac_message_bytes),
        ] {
            if value == 0 {
                return Err(ConfigError::ZeroLimit(limit));
            }
        }

        if self.minimum_interval.is_zero() {
            return Err(ConfigError::ZeroLimit(HostLimit::MinimumInterval));
        }
        if self.maximum_interval.is_zero() {
            return Err(ConfigError::ZeroLimit(HostLimit::MaximumInterval));
        }
        if self.max_deadline_horizon.is_zero() {
            return Err(ConfigError::ZeroLimit(HostLimit::DeadlineHorizon));
        }
        if self.minimum_interval > self.maximum_interval {
            return Err(ConfigError::InvalidIntervalRange {
                minimum_nanoseconds: self.minimum_interval.as_nanos(),
                maximum_nanoseconds: self.maximum_interval.as_nanos(),
            });
        }
        Ok(())
    }
}

impl Default for HostServiceLimits {
    fn default() -> Self {
        Self {
            max_random_bytes_per_request: 64 * 1024,
            max_concurrent_scheduled_timers: 1_024,
            minimum_interval: Duration::from_millis(1),
            maximum_interval: Duration::from_secs(24 * 60 * 60),
            max_deadline_horizon: Duration::from_secs(30 * 24 * 60 * 60),
            max_configured_secrets: 128,
            max_secret_bytes: 64 * 1024,
            max_verification_candidate_bytes: 64 * 1024,
            max_hmac_message_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Validated capabilities and limits for one host instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostServiceConfig {
    capabilities: HostCapabilities,
    limits: HostServiceLimits,
}

impl HostServiceConfig {
    pub fn new(
        capabilities: HostCapabilities,
        limits: HostServiceLimits,
    ) -> Result<Self, ConfigError> {
        limits.validate()?;
        for (capability, required) in [
            (HostCapability::Scheduling, HostCapability::MonotonicClock),
            (
                HostCapability::SecretVerification,
                HostCapability::SecretStore,
            ),
            (HostCapability::HmacSha256, HostCapability::SecretStore),
        ] {
            if capabilities.allows(capability) && !capabilities.allows(required) {
                return Err(ConfigError::MissingCapabilityDependency {
                    capability,
                    required,
                });
            }
        }
        Ok(Self {
            capabilities,
            limits,
        })
    }

    pub const fn capabilities(&self) -> HostCapabilities {
        self.capabilities
    }

    pub const fn limits(&self) -> &HostServiceLimits {
        &self.limits
    }
}

impl Default for HostServiceConfig {
    fn default() -> Self {
        Self::new(HostCapabilities::ALL, HostServiceLimits::default())
            .expect("default host service configuration is valid")
    }
}
