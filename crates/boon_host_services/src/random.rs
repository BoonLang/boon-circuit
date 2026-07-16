use std::fmt;

use crate::{HostLimit, HostServiceError, HostServiceLimits};

/// Owned random bytes. Formatting reports only the byte count.
pub struct RandomBytes(Vec<u8>);

impl RandomBytes {
    pub const fn len_bytes(&self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for RandomBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RandomBytes")
            .field("byte_count", &self.0.len())
            .finish_non_exhaustive()
    }
}

pub(crate) trait RandomProvider {
    fn fill(&self, destination: &mut [u8]) -> Result<(), HostServiceError>;
}

pub(crate) struct OsRandom;

impl RandomProvider for OsRandom {
    fn fill(&self, destination: &mut [u8]) -> Result<(), HostServiceError> {
        getrandom::fill(destination).map_err(HostServiceError::OsRandomUnavailable)
    }
}

pub(crate) fn fill_bounded(
    provider: &impl RandomProvider,
    byte_count: usize,
    limits: &HostServiceLimits,
) -> Result<RandomBytes, HostServiceError> {
    if byte_count > limits.max_random_bytes_per_request() {
        return Err(HostServiceError::LimitExceeded {
            limit: HostLimit::RandomBytesPerRequest,
            requested: byte_count as u128,
            maximum: limits.max_random_bytes_per_request() as u128,
        });
    }
    let mut bytes = vec![0; byte_count];
    provider.fill(&mut bytes)?;
    Ok(RandomBytes(bytes))
}
