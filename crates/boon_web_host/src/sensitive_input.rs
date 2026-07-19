use boon_host::{MAX_SENSITIVE_INPUT_BYTES, SemanticId, SensitiveInputHandle};
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSensitiveInputError {
    CapacityExceeded,
    HandleExhausted,
}

impl Display for BrowserSensitiveInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapacityExceeded => {
                formatter.write_str("sensitive input exceeds the host-owned buffer limit")
            }
            Self::HandleExhausted => {
                formatter.write_str("sensitive input handle sequence exhausted")
            }
        }
    }
}

impl Error for BrowserSensitiveInputError {}

struct SensitiveEntry {
    semantic_id: SemanticId,
    handle: SensitiveInputHandle,
    bytes: Vec<u8>,
}

impl SensitiveEntry {
    fn clear(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }
}

/// One browser-tab-local sensitive draft.
///
/// The bytes never enter a `SourcePayload`, runtime value, transport frame, or
/// retained document. Replacing focus invalidates the previous handle.
pub(crate) struct BrowserSensitiveInputVault {
    active: Option<SensitiveEntry>,
    next_handle: u64,
}

impl Default for BrowserSensitiveInputVault {
    fn default() -> Self {
        Self {
            active: None,
            next_handle: 1,
        }
    }
}

impl Debug for BrowserSensitiveInputVault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserSensitiveInputVault")
            .field("contents", &"redacted")
            .finish()
    }
}

impl BrowserSensitiveInputVault {
    pub(crate) fn replace(
        &mut self,
        semantic_id: SemanticId,
        text: String,
    ) -> Result<SensitiveInputHandle, BrowserSensitiveInputError> {
        if text.len() > MAX_SENSITIVE_INPUT_BYTES {
            return Err(BrowserSensitiveInputError::CapacityExceeded);
        }
        let bytes = text.into_bytes();
        if let Some(entry) = self
            .active
            .as_mut()
            .filter(|entry| entry.semantic_id == semantic_id)
        {
            entry.clear();
            entry.bytes = bytes;
            return Ok(entry.handle);
        }

        self.clear_all();
        let handle = SensitiveInputHandle::from_host_sequence(self.next_handle)
            .ok_or(BrowserSensitiveInputError::HandleExhausted)?;
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or(BrowserSensitiveInputError::HandleExhausted)?;
        self.active = Some(SensitiveEntry {
            semantic_id,
            handle,
            bytes,
        });
        Ok(handle)
    }

    pub(crate) fn owns(&self, semantic_id: &SemanticId, handle: SensitiveInputHandle) -> bool {
        self.active
            .as_ref()
            .is_some_and(|entry| entry.semantic_id == *semantic_id && entry.handle == handle)
    }

    pub(crate) fn clear(&mut self, semantic_id: &SemanticId) {
        if self
            .active
            .as_ref()
            .is_some_and(|entry| entry.semantic_id == *semantic_id)
        {
            self.clear_all();
        }
    }

    pub(crate) fn retain(&mut self, keep: impl FnOnce(&SemanticId) -> bool) {
        if self
            .active
            .as_ref()
            .is_some_and(|entry| !keep(&entry.semantic_id))
        {
            self.clear_all();
        }
    }

    fn clear_all(&mut self) {
        if let Some(mut entry) = self.active.take() {
            entry.clear();
        }
    }
}

impl Drop for BrowserSensitiveInputVault {
    fn drop(&mut self) {
        self.clear_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_bounded_redacted_and_invalidates_old_focus() {
        let first = SemanticId("semantic:first".to_owned());
        let second = SemanticId("semantic:second".to_owned());
        let mut vault = BrowserSensitiveInputVault::default();
        let first_handle = vault
            .replace(first.clone(), "secret-one".to_owned())
            .unwrap();
        assert!(vault.owns(&first, first_handle));
        assert!(!format!("{vault:?}").contains("secret-one"));

        let same_handle = vault
            .replace(first.clone(), "secret-two".to_owned())
            .unwrap();
        assert_eq!(same_handle, first_handle);
        let second_handle = vault
            .replace(second.clone(), "secret-three".to_owned())
            .unwrap();
        assert_ne!(second_handle, first_handle);
        assert!(!vault.owns(&first, first_handle));
        assert!(vault.owns(&second, second_handle));

        vault.retain(|id| id == &second);
        assert!(vault.owns(&second, second_handle));
        vault.retain(|_| false);
        assert!(!vault.owns(&second, second_handle));
        let second_handle = vault
            .replace(second.clone(), "secret-four".to_owned())
            .unwrap();
        vault.clear(&second);
        assert!(!vault.owns(&second, second_handle));
        assert_eq!(
            vault.replace(
                first,
                "x".repeat(MAX_SENSITIVE_INPUT_BYTES.saturating_add(1)),
            ),
            Err(BrowserSensitiveInputError::CapacityExceeded)
        );
    }
}
