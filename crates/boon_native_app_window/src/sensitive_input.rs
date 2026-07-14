use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use boon_host::{
    DocumentNodeId, SensitiveInputEvent, SensitiveInputHandle, SourceBindingId, SurfaceId,
};

pub const MAX_SENSITIVE_INPUT_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SensitiveInputTarget {
    pub node: DocumentNodeId,
    pub binding: Option<SourceBindingId>,
}

impl SensitiveInputTarget {
    pub fn new(node: DocumentNodeId, binding: Option<SourceBindingId>) -> Self {
        Self { node, binding }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensitiveInputError {
    CapacityExceeded,
    HandleExhausted,
    UnknownHandle,
}

impl Display for SensitiveInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapacityExceeded => {
                formatter.write_str("sensitive input exceeds the host-owned buffer limit")
            }
            Self::HandleExhausted => {
                formatter.write_str("sensitive input handle sequence exhausted")
            }
            Self::UnknownHandle => {
                formatter.write_str("sensitive input handle is stale or belongs to another host")
            }
        }
    }
}

impl Error for SensitiveInputError {}

/// Host-owned password draft storage.
///
/// This keeps plaintext out of Boon values, document snapshots, source events,
/// and reports. The contained bytes are overwritten on explicit clear and drop
/// as a best effort; this is not a cryptographic secrecy guarantee.
pub(crate) struct SensitiveInputVault {
    active: Option<SensitiveInputTarget>,
    entries: BTreeMap<SensitiveInputTarget, SensitiveInputEntry>,
    targets_by_handle: BTreeMap<SensitiveInputHandle, SensitiveInputTarget>,
    next_handle: u64,
}

impl Debug for SensitiveInputVault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SensitiveInputVault")
            .field("contents", &"redacted")
            .finish()
    }
}

impl Default for SensitiveInputVault {
    fn default() -> Self {
        Self {
            active: None,
            entries: BTreeMap::new(),
            targets_by_handle: BTreeMap::new(),
            next_handle: 1,
        }
    }
}

impl SensitiveInputVault {
    pub(crate) fn focus(
        &mut self,
        target: SensitiveInputTarget,
    ) -> Result<SensitiveInputHandle, SensitiveInputError> {
        if self.active.as_ref() != Some(&target) {
            self.clear_focus();
        }
        if let Some(entry) = self.entries.get(&target) {
            self.active = Some(target);
            return Ok(entry.handle);
        }
        let handle = SensitiveInputHandle::from_host_sequence(self.next_handle)
            .ok_or(SensitiveInputError::HandleExhausted)?;
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or(SensitiveInputError::HandleExhausted)?;
        self.entries
            .insert(target.clone(), SensitiveInputEntry::new(handle));
        self.targets_by_handle.insert(handle, target.clone());
        self.active = Some(target);
        Ok(handle)
    }

    pub(crate) fn clear_focus(&mut self) {
        let Some(target) = self.active.take() else {
            return;
        };
        if let Some(mut entry) = self.entries.remove(&target) {
            self.targets_by_handle.remove(&entry.handle);
            entry.clear();
        }
    }

    pub(crate) fn restart(&mut self) {
        self.clear_all();
    }

    fn clear_all(&mut self) {
        self.active = None;
        self.targets_by_handle.clear();
        for entry in self.entries.values_mut() {
            entry.clear();
        }
        self.entries.clear();
    }

    pub(crate) fn active_handle(&self) -> Option<SensitiveInputHandle> {
        self.active
            .as_ref()
            .and_then(|target| self.entries.get(target))
            .map(|entry| entry.handle)
    }

    pub(crate) fn with_bytes<R>(
        &self,
        handle: SensitiveInputHandle,
        use_bytes: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, SensitiveInputError> {
        let target = self
            .targets_by_handle
            .get(&handle)
            .ok_or(SensitiveInputError::UnknownHandle)?;
        let entry = self
            .entries
            .get(target)
            .ok_or(SensitiveInputError::UnknownHandle)?;
        Ok(use_bytes(entry.value.as_slice()))
    }

    pub(crate) fn insert_text(
        &mut self,
        text: &str,
    ) -> Result<Option<SensitiveInputHandle>, SensitiveInputError> {
        let Some(entry) = self.active_entry_mut() else {
            return Ok(None);
        };
        entry.insert_text(text)?;
        Ok(Some(entry.handle))
    }

    pub(crate) fn set_preedit(
        &mut self,
        text: &str,
    ) -> Result<Option<SensitiveInputHandle>, SensitiveInputError> {
        let Some(entry) = self.active_entry_mut() else {
            return Ok(None);
        };
        entry.set_preedit(text)?;
        Ok(Some(entry.handle))
    }

    pub(crate) fn clear_preedit(&mut self) -> Option<SensitiveInputHandle> {
        let entry = self.active_entry_mut()?;
        entry.composition.clear();
        Some(entry.handle)
    }

    pub(crate) fn delete_surrounding(
        &mut self,
        before_bytes: u32,
        after_bytes: u32,
    ) -> Option<SensitiveInputHandle> {
        let entry = self.active_entry_mut()?;
        entry.delete_surrounding(before_bytes as usize, after_bytes as usize);
        Some(entry.handle)
    }

    pub(crate) fn edit(&mut self, command: SensitiveEdit) -> Option<SensitiveInputHandle> {
        let entry = self.active_entry_mut()?;
        entry.edit(command);
        Some(entry.handle)
    }

    pub(crate) fn event(
        &self,
        surface: &SurfaceId,
        handle: SensitiveInputHandle,
    ) -> SensitiveInputEvent {
        SensitiveInputEvent {
            surface: surface.clone(),
            handle,
        }
    }

    fn active_entry_mut(&mut self) -> Option<&mut SensitiveInputEntry> {
        let target = self.active.as_ref()?;
        self.entries.get_mut(target)
    }
}

impl Drop for SensitiveInputVault {
    fn drop(&mut self) {
        self.clear_all();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SensitiveEdit {
    Backspace,
    DeleteForward,
    MoveLeft { extend: bool },
    MoveRight { extend: bool },
    MoveHome { extend: bool },
    MoveEnd { extend: bool },
    SelectAll,
    CutSelection,
}

struct SensitiveInputEntry {
    handle: SensitiveInputHandle,
    value: SensitiveBytes,
    composition: SensitiveBytes,
    anchor: usize,
    head: usize,
}

impl SensitiveInputEntry {
    fn new(handle: SensitiveInputHandle) -> Self {
        Self {
            handle,
            value: SensitiveBytes::default(),
            composition: SensitiveBytes::default(),
            anchor: 0,
            head: 0,
        }
    }

    fn clear(&mut self) {
        self.value.clear();
        self.composition.clear();
        self.anchor = 0;
        self.head = 0;
    }

    fn insert_text(&mut self, text: &str) -> Result<(), SensitiveInputError> {
        self.composition.clear();
        let (start, end) = self.selection();
        let next_len = self
            .value
            .len()
            .saturating_sub(end.saturating_sub(start))
            .checked_add(text.len())
            .ok_or(SensitiveInputError::CapacityExceeded)?;
        if next_len > MAX_SENSITIVE_INPUT_BYTES {
            return Err(SensitiveInputError::CapacityExceeded);
        }
        self.value.replace_range(start, end, text.as_bytes());
        self.head = start + text.len();
        self.anchor = self.head;
        Ok(())
    }

    fn set_preedit(&mut self, text: &str) -> Result<(), SensitiveInputError> {
        if text.len() > MAX_SENSITIVE_INPUT_BYTES {
            return Err(SensitiveInputError::CapacityExceeded);
        }
        self.composition.replace(text.as_bytes());
        Ok(())
    }

    fn delete_surrounding(&mut self, before: usize, after: usize) {
        self.delete_selection();
        let text = self.value.as_str();
        let start = previous_boundary_at_or_before(text, self.head.saturating_sub(before));
        let end = next_boundary_at_or_after(text, self.head.saturating_add(after));
        self.value.replace_range(start, end, &[]);
        self.head = start;
        self.anchor = start;
    }

    fn edit(&mut self, command: SensitiveEdit) {
        self.composition.clear();
        match command {
            SensitiveEdit::Backspace => {
                if self.delete_selection() {
                    return;
                }
                let start = previous_char_boundary(self.value.as_str(), self.head);
                self.value.replace_range(start, self.head, &[]);
                self.head = start;
                self.anchor = start;
            }
            SensitiveEdit::DeleteForward => {
                if self.delete_selection() {
                    return;
                }
                let end = next_char_boundary(self.value.as_str(), self.head);
                self.value.replace_range(self.head, end, &[]);
                self.anchor = self.head;
            }
            SensitiveEdit::MoveLeft { extend } => {
                self.move_head(
                    previous_char_boundary(self.value.as_str(), self.head),
                    extend,
                );
            }
            SensitiveEdit::MoveRight { extend } => {
                self.move_head(next_char_boundary(self.value.as_str(), self.head), extend);
            }
            SensitiveEdit::MoveHome { extend } => self.move_head(0, extend),
            SensitiveEdit::MoveEnd { extend } => self.move_head(self.value.len(), extend),
            SensitiveEdit::SelectAll => {
                self.anchor = 0;
                self.head = self.value.len();
            }
            SensitiveEdit::CutSelection => {
                self.delete_selection();
            }
        }
    }

    fn move_head(&mut self, head: usize, extend: bool) {
        self.head = head;
        if !extend {
            self.anchor = head;
        }
    }

    fn selection(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    fn delete_selection(&mut self) -> bool {
        let (start, end) = self.selection();
        if start == end {
            return false;
        }
        self.value.replace_range(start, end, &[]);
        self.anchor = start;
        self.head = start;
        true
    }
}

struct SensitiveBytes {
    bytes: Box<[u8; MAX_SENSITIVE_INPUT_BYTES]>,
    len: usize,
    #[cfg(test)]
    drop_probe: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Default for SensitiveBytes {
    fn default() -> Self {
        Self {
            bytes: Box::new([0; MAX_SENSITIVE_INPUT_BYTES]),
            len: 0,
            #[cfg(test)]
            drop_probe: None,
        }
    }
}

impl Debug for SensitiveBytes {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveBytes(<redacted>)")
    }
}

impl SensitiveBytes {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_slice()).expect("sensitive input must remain UTF-8")
    }

    fn len(&self) -> usize {
        self.len
    }

    fn replace(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= MAX_SENSITIVE_INPUT_BYTES);
        self.clear();
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len();
    }

    fn replace_range(&mut self, start: usize, end: usize, replacement: &[u8]) {
        debug_assert!(self.as_str().is_char_boundary(start));
        debug_assert!(self.as_str().is_char_boundary(end));
        let old_len = self.len;
        let removed = end.saturating_sub(start);
        let next_len = old_len.saturating_sub(removed) + replacement.len();
        debug_assert!(next_len <= MAX_SENSITIVE_INPUT_BYTES);
        self.bytes
            .copy_within(end..old_len, start + replacement.len());
        self.bytes[start..start + replacement.len()].copy_from_slice(replacement);
        if next_len < old_len {
            self.bytes[next_len..old_len].fill(0);
        }
        self.len = next_len;
    }

    fn clear(&mut self) {
        self.bytes[..self.len].fill(0);
        self.len = 0;
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.bytes.fill(0);
        self.len = 0;
        #[cfg(test)]
        if let Some(probe) = &self.drop_probe {
            probe.store(
                self.bytes.iter().all(|byte| *byte == 0),
                std::sync::atomic::Ordering::Release,
            );
        }
    }
}

fn previous_char_boundary(text: &str, offset: usize) -> usize {
    let offset = previous_boundary_at_or_before(text, offset);
    text[..offset]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, offset: usize) -> usize {
    let offset = next_boundary_at_or_after(text, offset);
    text[offset..]
        .chars()
        .next()
        .map(|character| offset + character.len_utf8())
        .unwrap_or(text.len())
}

fn previous_boundary_at_or_before(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset = offset.saturating_sub(1);
    }
    offset
}

fn next_boundary_at_or_after(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset = offset.saturating_add(1).min(text.len());
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const SENTINEL: &str = "sensitive-SENTINEL-7f2d9b";

    fn target(name: &str) -> SensitiveInputTarget {
        SensitiveInputTarget::new(
            DocumentNodeId(name.to_owned()),
            Some(SourceBindingId(format!("binding:{name}"))),
        )
    }

    #[test]
    fn opaque_handle_borrows_bytes_without_exposing_them_in_debug() {
        let mut vault = SensitiveInputVault::default();
        let handle = vault.focus(target("password")).unwrap();
        assert_eq!(vault.insert_text(SENTINEL).unwrap(), Some(handle));
        assert_eq!(
            vault.with_bytes(handle, |bytes| bytes == SENTINEL.as_bytes()),
            Ok(true)
        );
        let debug = format!("{vault:?} {handle:?}");
        assert!(!debug.contains(SENTINEL));
        assert!(!debug.contains("7f2d9b"));
    }

    #[test]
    fn editing_is_utf8_safe_and_restart_invalidates_handles() {
        let mut vault = SensitiveInputVault::default();
        let handle = vault.focus(target("password")).unwrap();
        vault.insert_text("aé🙂z").unwrap();
        vault.edit(SensitiveEdit::MoveLeft { extend: false });
        vault.edit(SensitiveEdit::Backspace);
        vault.edit(SensitiveEdit::SelectAll);
        vault.insert_text(SENTINEL).unwrap();
        assert_eq!(
            vault.with_bytes(handle, |bytes| bytes == SENTINEL.as_bytes()),
            Ok(true)
        );
        vault.restart();
        assert_eq!(
            vault.with_bytes(handle, |_| ()),
            Err(SensitiveInputError::UnknownHandle)
        );
    }

    #[test]
    fn focus_change_clears_the_previous_draft() {
        let mut vault = SensitiveInputVault::default();
        let first = vault.focus(target("first")).unwrap();
        vault.insert_text(SENTINEL).unwrap();
        vault.focus(target("second")).unwrap();
        assert_eq!(
            vault.with_bytes(first, |_| ()),
            Err(SensitiveInputError::UnknownHandle)
        );
    }

    #[test]
    fn sensitive_bytes_are_overwritten_by_drop() {
        let probe = Arc::new(AtomicBool::new(false));
        let mut bytes = SensitiveBytes::default();
        bytes.drop_probe = Some(Arc::clone(&probe));
        bytes.replace(SENTINEL.as_bytes());
        drop(bytes);
        assert!(probe.load(Ordering::Acquire));
    }
}
