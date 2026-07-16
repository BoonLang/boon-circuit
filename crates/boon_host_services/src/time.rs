use std::fmt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::TimeError;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Identifies the private origin that owns a monotonic snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClockId(u64);

impl ClockId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A process-local monotonic offset owned by one host clock.
///
/// This value deliberately contains no [`Instant`] and has no serialization
/// implementation. Its numeric unit is always nanoseconds since `clock_id`'s
/// private origin.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MonotonicSnapshot {
    clock_id: ClockId,
    nanoseconds_since_clock_origin: u128,
}

impl MonotonicSnapshot {
    pub const fn clock_id(self) -> ClockId {
        self.clock_id
    }

    pub const fn nanoseconds_since_clock_origin(self) -> u128 {
        self.nanoseconds_since_clock_origin
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        Some(Self {
            clock_id: self.clock_id,
            nanoseconds_since_clock_origin: self
                .nanoseconds_since_clock_origin
                .checked_add(duration.as_nanos())?,
        })
    }

    /// Compares two offsets only when they share the same private clock owner.
    pub fn checked_cmp(self, other: Self) -> Result<std::cmp::Ordering, TimeError> {
        if self.clock_id != other.clock_id {
            return Err(TimeError::ClockOwnerMismatch {
                expected: self.clock_id,
                actual: other.clock_id,
            });
        }
        Ok(self
            .nanoseconds_since_clock_origin
            .cmp(&other.nanoseconds_since_clock_origin))
    }

    pub fn duration_since(self, earlier: Self) -> Result<Duration, TimeError> {
        if self.clock_id != earlier.clock_id {
            return Err(TimeError::ClockOwnerMismatch {
                expected: self.clock_id,
                actual: earlier.clock_id,
            });
        }
        let nanoseconds = self
            .nanoseconds_since_clock_origin
            .checked_sub(earlier.nanoseconds_since_clock_origin)
            .ok_or(TimeError::EarlierSnapshot {
                earlier_nanoseconds: earlier.nanoseconds_since_clock_origin,
                later_nanoseconds: self.nanoseconds_since_clock_origin,
            })?;
        duration_from_nanoseconds(nanoseconds)
    }

    pub(crate) const fn new(clock_id: ClockId, nanoseconds: u128) -> Self {
        Self {
            clock_id,
            nanoseconds_since_clock_origin: nanoseconds,
        }
    }
}

/// UTC wall time represented canonically relative to the Unix epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WallClockSnapshot {
    unix_epoch_seconds: i128,
    nanoseconds_within_second: u32,
}

impl WallClockSnapshot {
    pub fn from_unix_parts(
        unix_epoch_seconds: i128,
        nanoseconds_within_second: u32,
    ) -> Result<Self, TimeError> {
        if nanoseconds_within_second >= NANOS_PER_SECOND as u32 {
            return Err(TimeError::InvalidSubsecondNanoseconds {
                nanoseconds: nanoseconds_within_second,
            });
        }
        Ok(Self {
            unix_epoch_seconds,
            nanoseconds_within_second,
        })
    }

    /// Whole seconds using floor semantics relative to the Unix epoch.
    pub const fn unix_epoch_seconds(self) -> i128 {
        self.unix_epoch_seconds
    }

    /// Nanoseconds in the canonical range `0..1_000_000_000`.
    pub const fn nanoseconds_within_second(self) -> u32 {
        self.nanoseconds_within_second
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let added_seconds = i128::from(duration.as_secs());
        let nanos = u64::from(self.nanoseconds_within_second)
            .checked_add(u64::from(duration.subsec_nanos()))?;
        let carry = nanos / NANOS_PER_SECOND as u64;
        let unix_epoch_seconds = self
            .unix_epoch_seconds
            .checked_add(added_seconds)?
            .checked_add(i128::from(carry))?;
        Some(Self {
            unix_epoch_seconds,
            nanoseconds_within_second: (nanos % NANOS_PER_SECOND as u64) as u32,
        })
    }

    pub(crate) fn from_system_time(system_time: SystemTime) -> Self {
        match system_time.duration_since(UNIX_EPOCH) {
            Ok(duration) => Self {
                unix_epoch_seconds: i128::from(duration.as_secs()),
                nanoseconds_within_second: duration.subsec_nanos(),
            },
            Err(error) => {
                let duration = error.duration();
                if duration.subsec_nanos() == 0 {
                    Self {
                        unix_epoch_seconds: -i128::from(duration.as_secs()),
                        nanoseconds_within_second: 0,
                    }
                } else {
                    Self {
                        unix_epoch_seconds: -i128::from(duration.as_secs()) - 1,
                        nanoseconds_within_second: 1_000_000_000 - duration.subsec_nanos(),
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ClockCore {
    id: ClockId,
    origin: Instant,
}

impl ClockCore {
    pub(crate) fn monotonic_now(&self) -> MonotonicSnapshot {
        MonotonicSnapshot::new(self.id, self.origin.elapsed().as_nanos())
    }

    pub(crate) fn instant_for(&self, snapshot: MonotonicSnapshot) -> Result<Instant, TimeError> {
        if snapshot.clock_id() != self.id {
            return Err(TimeError::ClockOwnerMismatch {
                expected: self.id,
                actual: snapshot.clock_id(),
            });
        }
        let offset = duration_from_nanoseconds(snapshot.nanoseconds_since_clock_origin())?;
        self.origin
            .checked_add(offset)
            .ok_or(TimeError::DurationOutOfRange {
                nanoseconds: snapshot.nanoseconds_since_clock_origin(),
            })
    }
}

pub(crate) struct SystemClock {
    core: ClockCore,
}

impl SystemClock {
    pub(crate) fn new(id: ClockId) -> Self {
        Self {
            core: ClockCore {
                id,
                origin: Instant::now(),
            },
        }
    }

    pub(crate) fn core(&self) -> ClockCore {
        self.core.clone()
    }

    pub(crate) fn monotonic_now(&self) -> MonotonicSnapshot {
        self.core.monotonic_now()
    }

    pub(crate) fn wall_clock_now(&self) -> WallClockSnapshot {
        WallClockSnapshot::from_system_time(SystemTime::now())
    }
}

impl fmt::Debug for SystemClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemClock")
            .field("clock_id", &self.core.id)
            .finish_non_exhaustive()
    }
}

pub(crate) fn duration_from_nanoseconds(nanoseconds: u128) -> Result<Duration, TimeError> {
    let seconds = nanoseconds / NANOS_PER_SECOND;
    let subsecond_nanoseconds = (nanoseconds % NANOS_PER_SECOND) as u32;
    let seconds =
        u64::try_from(seconds).map_err(|_| TimeError::DurationOutOfRange { nanoseconds })?;
    Ok(Duration::new(seconds, subsecond_nanoseconds))
}
