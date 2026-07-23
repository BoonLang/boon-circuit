use boon_plan_executor::{HostValueBinding, HostValueIssuer};
use std::collections::BTreeMap;
use std::fmt;

pub const HOST_CAPABILITY_ID_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostCapabilityErrorKind {
    InvalidConfiguration,
    Capacity,
    DuplicateHandle,
    Foreign,
    Unknown,
    Stale,
    WrongAccess,
    GenerationExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCapabilityError {
    kind: HostCapabilityErrorKind,
    diagnostic: &'static str,
}

impl HostCapabilityError {
    fn new(kind: HostCapabilityErrorKind, diagnostic: &'static str) -> Self {
        Self { kind, diagnostic }
    }

    pub const fn kind(&self) -> HostCapabilityErrorKind {
        self.kind
    }
}

impl fmt::Display for HostCapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.diagnostic)
    }
}

impl std::error::Error for HostCapabilityError {}

struct HostCapabilityEntry<R, A> {
    generation: u32,
    resource: R,
    access: A,
}

#[derive(Debug)]
pub struct ResolvedHostCapability<'a, R> {
    pub handle: [u8; HOST_CAPABILITY_ID_BYTES],
    pub resource: &'a R,
}

/// Bounded issuer-owned registry for process-local host resources.
///
/// Resource identity remains in the host. Boon receives only a
/// [`HostValueBinding`] attached to an ordinary visible structural value.
pub struct HostCapabilityRegistry<R, A>
where
    A: Copy + Eq,
{
    capacity: usize,
    issuer: HostValueIssuer,
    entries: BTreeMap<[u8; HOST_CAPABILITY_ID_BYTES], HostCapabilityEntry<R, A>>,
}

impl<R, A> HostCapabilityRegistry<R, A>
where
    A: Copy + Eq,
{
    pub fn new(
        issuer_identity: [u8; HOST_CAPABILITY_ID_BYTES],
        capacity: usize,
    ) -> Result<Self, HostCapabilityError> {
        if capacity == 0 {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::InvalidConfiguration,
                "host capability capacity must be positive",
            ));
        }
        Ok(Self {
            capacity,
            issuer: HostValueBinding::new_issuer(issuer_identity),
            entries: BTreeMap::new(),
        })
    }

    pub fn register(
        &mut self,
        handle: [u8; HOST_CAPABILITY_ID_BYTES],
        resource: R,
        access: A,
    ) -> Result<HostValueBinding, HostCapabilityError> {
        if self.entries.len() >= self.capacity {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::Capacity,
                "host capability registry reached its bounded capacity",
            ));
        }
        if self.entries.contains_key(&handle) {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::DuplicateHandle,
                "host capability handle is already registered",
            ));
        }
        let generation = 1;
        let binding = self.issuer.mint(handle, generation).map_err(|_| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::InvalidConfiguration,
                "host capability binding could not be minted",
            )
        })?;
        self.entries.insert(
            handle,
            HostCapabilityEntry {
                generation,
                resource,
                access,
            },
        );
        Ok(binding)
    }

    pub fn resolve(
        &self,
        binding: &HostValueBinding,
        expected_access: A,
    ) -> Result<ResolvedHostCapability<'_, R>, HostCapabilityError> {
        let (handle, generation) = self.issuer.open(binding).ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::Foreign,
                "host capability belongs to another issuer",
            )
        })?;
        let entry = self.entries.get(&handle).ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::Unknown,
                "host capability is unknown or revoked",
            )
        })?;
        if entry.generation != generation {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::Stale,
                "host capability generation is stale",
            ));
        }
        if entry.access != expected_access {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::WrongAccess,
                "host capability has the wrong access direction",
            ));
        }
        Ok(ResolvedHostCapability {
            handle,
            resource: &entry.resource,
        })
    }

    pub fn replace(
        &mut self,
        binding: &HostValueBinding,
        resource: R,
    ) -> Result<HostValueBinding, HostCapabilityError> {
        let (handle, generation) = self.issuer.open(binding).ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::Foreign,
                "host capability belongs to another issuer",
            )
        })?;
        let entry = self.entries.get_mut(&handle).ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::Unknown,
                "host capability is unknown or revoked",
            )
        })?;
        if entry.generation != generation {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorKind::Stale,
                "host capability generation is stale",
            ));
        }
        let next_generation = generation.checked_add(1).ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::GenerationExhausted,
                "host capability generation is exhausted",
            )
        })?;
        entry.generation = next_generation;
        entry.resource = resource;
        self.issuer.mint(handle, next_generation).map_err(|_| {
            HostCapabilityError::new(
                HostCapabilityErrorKind::GenerationExhausted,
                "host capability binding could not be advanced",
            )
        })
    }

    pub fn revoke(&mut self, binding: &HostValueBinding) -> bool {
        let Some((handle, generation)) = self.issuer.open(binding) else {
            return false;
        };
        if self
            .entries
            .get(&handle)
            .is_none_or(|entry| entry.generation != generation)
        {
            return false;
        }
        self.entries.remove(&handle);
        true
    }

    pub fn contains(&self, binding: &HostValueBinding) -> bool {
        self.issuer
            .open(binding)
            .is_some_and(|(handle, generation)| {
                self.entries
                    .get(&handle)
                    .is_some_and(|entry| entry.generation == generation)
            })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Access {
        Read,
        Write,
    }

    #[test]
    fn registry_rejects_foreign_stale_wrong_direction_and_revoked_bindings() {
        let mut registry = HostCapabilityRegistry::new([1; 32], 2).unwrap();
        let binding = registry.register([2; 32], "first", Access::Read).unwrap();
        assert_eq!(
            registry.resolve(&binding, Access::Read).unwrap().resource,
            &"first"
        );
        assert_eq!(
            registry
                .resolve(&binding, Access::Write)
                .unwrap_err()
                .kind(),
            HostCapabilityErrorKind::WrongAccess
        );

        let replacement = registry.replace(&binding, "second").unwrap();
        assert_eq!(
            registry.resolve(&binding, Access::Read).unwrap_err().kind(),
            HostCapabilityErrorKind::Stale
        );
        assert_eq!(
            registry
                .resolve(&replacement, Access::Read)
                .unwrap()
                .resource,
            &"second"
        );

        let foreign: HostCapabilityRegistry<&'static str, Access> =
            HostCapabilityRegistry::new([3; 32], 1).unwrap();
        assert_eq!(
            foreign
                .resolve(&replacement, Access::Read)
                .unwrap_err()
                .kind(),
            HostCapabilityErrorKind::Foreign
        );
        assert!(registry.revoke(&replacement));
        assert_eq!(
            registry
                .resolve(&replacement, Access::Read)
                .unwrap_err()
                .kind(),
            HostCapabilityErrorKind::Unknown
        );
    }
}
