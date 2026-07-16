use std::fmt;

use crate::{ClockId, HostCapability, HostLimit, SecretRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    ZeroLimit(HostLimit),
    InvalidIntervalRange {
        minimum_nanoseconds: u128,
        maximum_nanoseconds: u128,
    },
    MissingCapabilityDependency {
        capability: HostCapability,
        required: HostCapability,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit(limit) => write!(formatter, "{limit:?} must be greater than zero"),
            Self::InvalidIntervalRange {
                minimum_nanoseconds,
                maximum_nanoseconds,
            } => write!(
                formatter,
                "minimum interval ({minimum_nanoseconds} nanoseconds) exceeds maximum interval ({maximum_nanoseconds} nanoseconds)"
            ),
            Self::MissingCapabilityDependency {
                capability,
                required,
            } => write!(
                formatter,
                "{capability:?} requires the {required:?} capability"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeError {
    ClockOwnerMismatch {
        expected: ClockId,
        actual: ClockId,
    },
    EarlierSnapshot {
        earlier_nanoseconds: u128,
        later_nanoseconds: u128,
    },
    DurationOutOfRange {
        nanoseconds: u128,
    },
    InvalidSubsecondNanoseconds {
        nanoseconds: u32,
    },
}

impl fmt::Display for TimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClockOwnerMismatch { expected, actual } => write!(
                formatter,
                "monotonic snapshot belongs to clock {}, expected clock {}",
                actual.get(),
                expected.get()
            ),
            Self::EarlierSnapshot {
                earlier_nanoseconds,
                later_nanoseconds,
            } => write!(
                formatter,
                "snapshot at {later_nanoseconds} nanoseconds precedes {earlier_nanoseconds} nanoseconds"
            ),
            Self::DurationOutOfRange { nanoseconds } => {
                write!(
                    formatter,
                    "{nanoseconds} nanoseconds exceed Duration's range"
                )
            }
            Self::InvalidSubsecondNanoseconds { nanoseconds } => write!(
                formatter,
                "wall-clock subsecond value {nanoseconds} must be below 1000000000 nanoseconds"
            ),
        }
    }
}

impl std::error::Error for TimeError {}

#[derive(Debug)]
pub enum HostServiceError {
    CapabilityDisabled(HostCapability),
    Shutdown,
    LimitExceeded {
        limit: HostLimit,
        requested: u128,
        maximum: u128,
    },
    BelowMinimum {
        limit: HostLimit,
        requested: u128,
        minimum: u128,
    },
    Time(TimeError),
    EmptySecret,
    SecretNotFound(SecretRef),
    SecretIdExhausted,
    OsRandomUnavailable(getrandom::Error),
    DeterministicRandomExhausted,
    SchedulerUnavailable,
}

impl fmt::Display for HostServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDisabled(capability) => {
                write!(formatter, "host capability {capability:?} is disabled")
            }
            Self::Shutdown => formatter.write_str("host services are shut down"),
            Self::LimitExceeded {
                limit,
                requested,
                maximum,
            } => write!(
                formatter,
                "{limit:?} requested {requested} {:?}, maximum is {maximum} {:?}",
                limit.unit(),
                limit.unit()
            ),
            Self::BelowMinimum {
                limit,
                requested,
                minimum,
            } => write!(
                formatter,
                "{limit:?} requested {requested} {:?}, minimum is {minimum} {:?}",
                limit.unit(),
                limit.unit()
            ),
            Self::Time(error) => error.fmt(formatter),
            Self::EmptySecret => formatter.write_str("configured secrets must not be empty"),
            Self::SecretNotFound(secret_ref) => write!(
                formatter,
                "secret {} is not configured in store {}",
                secret_ref.secret_id().get(),
                secret_ref.store_id().get()
            ),
            Self::SecretIdExhausted => formatter.write_str("secret reference IDs are exhausted"),
            Self::OsRandomUnavailable(error) => {
                write!(
                    formatter,
                    "operating-system secure randomness failed: {error}"
                )
            }
            Self::DeterministicRandomExhausted => {
                formatter.write_str("deterministic random provider counter is exhausted")
            }
            Self::SchedulerUnavailable => formatter.write_str("scheduler worker is unavailable"),
        }
    }
}

impl std::error::Error for HostServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Time(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TimeError> for HostServiceError {
    fn from(error: TimeError) -> Self {
        Self::Time(error)
    }
}
