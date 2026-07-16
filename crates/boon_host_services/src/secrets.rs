use std::collections::HashMap;
use std::fmt;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{HostLimit, HostServiceError, HostServiceLimits};

pub const HMAC_SHA256_TAG_BYTES: usize = 32;

const CONFIGURED_SECRET_VERIFICATION_DOMAIN: &[u8] =
    b"boon_host_services/configured-secret-verification/v1";

type HmacSha256 = Hmac<Sha256>;

/// Identifies one secret store. The ID is routing metadata, not secret bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecretStoreId(u64);

impl SecretStoreId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identifies one entry in a secret store. The ID is not secret material.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecretId(u64);

impl SecretId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// An opaque, owner-scoped reference to configured secret material.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecretRef {
    store_id: SecretStoreId,
    secret_id: SecretId,
}

impl SecretRef {
    pub const fn store_id(self) -> SecretStoreId {
        self.store_id
    }

    pub const fn secret_id(self) -> SecretId {
        self.secret_id
    }
}

/// Move-only secret input whose backing bytes are zeroized on drop.
///
/// This type intentionally implements neither `Clone`, `Debug`, nor any
/// serialization or byte-access trait. Once moved into [`crate::HostServices`],
/// the bytes can only be used by verification and HMAC operations.
pub struct SecretMaterial {
    bytes: Zeroizing<Vec<u8>>,
}

impl SecretMaterial {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }

    pub(crate) fn len_bytes(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

/// A typed HMAC-SHA256 authentication tag.
///
/// Tags are outputs rather than configured secret bytes. They can be exported
/// for transport, but formatting still omits their contents.
#[derive(Clone, Copy)]
pub struct HmacSha256Tag([u8; HMAC_SHA256_TAG_BYTES]);

impl HmacSha256Tag {
    pub const fn from_bytes(bytes: [u8; HMAC_SHA256_TAG_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; HMAC_SHA256_TAG_BYTES] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; HMAC_SHA256_TAG_BYTES] {
        self.0
    }
}

impl fmt::Debug for HmacSha256Tag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HmacSha256Tag([REDACTED])")
    }
}

/// Result of a constant-time verification operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct Verification(bool);

impl Verification {
    pub const fn is_verified(self) -> bool {
        self.0
    }
}

struct StoredSecret {
    material: SecretMaterial,
    verification_tag: Zeroizing<[u8; HMAC_SHA256_TAG_BYTES]>,
}

pub(crate) struct SecretStore {
    id: SecretStoreId,
    next_secret_id: u64,
    entries: HashMap<SecretId, StoredSecret>,
}

impl SecretStore {
    pub(crate) fn new(id: SecretStoreId) -> Self {
        Self {
            id,
            next_secret_id: 1,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn insert(
        &mut self,
        material: SecretMaterial,
        limits: &HostServiceLimits,
    ) -> Result<SecretRef, HostServiceError> {
        if material.is_empty() {
            return Err(HostServiceError::EmptySecret);
        }
        enforce_maximum(
            HostLimit::SecretBytes,
            material.len_bytes(),
            limits.max_secret_bytes(),
        )?;
        if self.entries.len() >= limits.max_configured_secrets() {
            return Err(HostServiceError::LimitExceeded {
                limit: HostLimit::ConfiguredSecrets,
                requested: self.entries.len() as u128 + 1,
                maximum: limits.max_configured_secrets() as u128,
            });
        }

        let secret_id = SecretId(self.next_secret_id);
        self.next_secret_id = self
            .next_secret_id
            .checked_add(1)
            .ok_or(HostServiceError::SecretIdExhausted)?;
        let verification_tag = Zeroizing::new(configured_secret_tag(material.bytes()));
        self.entries.insert(
            secret_id,
            StoredSecret {
                material,
                verification_tag,
            },
        );
        Ok(SecretRef {
            store_id: self.id,
            secret_id,
        })
    }

    pub(crate) fn remove(&mut self, secret_ref: SecretRef) -> Result<bool, HostServiceError> {
        self.ensure_store(secret_ref)?;
        Ok(self.entries.remove(&secret_ref.secret_id).is_some())
    }

    pub(crate) fn verify(
        &self,
        secret_ref: SecretRef,
        candidate: &[u8],
        limits: &HostServiceLimits,
    ) -> Result<Verification, HostServiceError> {
        let stored = self.get(secret_ref)?;
        enforce_maximum(
            HostLimit::VerificationCandidateBytes,
            candidate.len(),
            limits.max_verification_candidate_bytes(),
        )?;

        let mut candidate_mac = HmacSha256::new_from_slice(candidate)
            .expect("HMAC-SHA256 accepts keys of every byte length");
        candidate_mac.update(CONFIGURED_SECRET_VERIFICATION_DOMAIN);
        Ok(Verification(
            candidate_mac
                .verify_slice(stored.verification_tag.as_slice())
                .is_ok(),
        ))
    }

    pub(crate) fn hmac_sha256_sign(
        &self,
        secret_ref: SecretRef,
        message: &[u8],
        limits: &HostServiceLimits,
    ) -> Result<HmacSha256Tag, HostServiceError> {
        enforce_maximum(
            HostLimit::HmacMessageBytes,
            message.len(),
            limits.max_hmac_message_bytes(),
        )?;
        let stored = self.get(secret_ref)?;
        let mut mac = HmacSha256::new_from_slice(stored.material.bytes())
            .expect("HMAC-SHA256 accepts keys of every byte length");
        mac.update(message);
        let mut tag = [0; HMAC_SHA256_TAG_BYTES];
        tag.copy_from_slice(mac.finalize().into_bytes().as_slice());
        Ok(HmacSha256Tag(tag))
    }

    pub(crate) fn hmac_sha256_verify(
        &self,
        secret_ref: SecretRef,
        message: &[u8],
        tag: &HmacSha256Tag,
        limits: &HostServiceLimits,
    ) -> Result<Verification, HostServiceError> {
        enforce_maximum(
            HostLimit::HmacMessageBytes,
            message.len(),
            limits.max_hmac_message_bytes(),
        )?;
        let stored = self.get(secret_ref)?;
        let mut mac = HmacSha256::new_from_slice(stored.material.bytes())
            .expect("HMAC-SHA256 accepts keys of every byte length");
        mac.update(message);
        Ok(Verification(mac.verify_slice(tag.as_bytes()).is_ok()))
    }

    fn get(&self, secret_ref: SecretRef) -> Result<&StoredSecret, HostServiceError> {
        self.ensure_store(secret_ref)?;
        self.entries
            .get(&secret_ref.secret_id)
            .ok_or(HostServiceError::SecretNotFound(secret_ref))
    }

    fn ensure_store(&self, secret_ref: SecretRef) -> Result<(), HostServiceError> {
        if secret_ref.store_id != self.id {
            return Err(HostServiceError::SecretNotFound(secret_ref));
        }
        Ok(())
    }
}

impl fmt::Debug for SecretStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretStore")
            .field("store_id", &self.id)
            .field("configured_secret_count", &self.entries.len())
            .field("secret_material", &"[REDACTED]")
            .finish()
    }
}

fn configured_secret_tag(secret: &[u8]) -> [u8; HMAC_SHA256_TAG_BYTES] {
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts keys of every byte length");
    mac.update(CONFIGURED_SECRET_VERIFICATION_DOMAIN);
    let mut tag = [0; HMAC_SHA256_TAG_BYTES];
    tag.copy_from_slice(mac.finalize().into_bytes().as_slice());
    tag
}

fn enforce_maximum(
    limit: HostLimit,
    requested: usize,
    maximum: usize,
) -> Result<(), HostServiceError> {
    if requested > maximum {
        return Err(HostServiceError::LimitExceeded {
            limit,
            requested: requested as u128,
            maximum: maximum as u128,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_verification_normalizes_secrets_to_fixed_width_tags() {
        let short = configured_secret_tag(b"x");
        let long = configured_secret_tag(&vec![b'y'; 8_192]);

        assert_eq!(short.len(), HMAC_SHA256_TAG_BYTES);
        assert_eq!(long.len(), HMAC_SHA256_TAG_BYTES);
        assert_ne!(short, long);
    }
}
