use boon_plan::FiniteReal;
use boon_runtime::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

const CONTENT_DIGEST_BYTES: usize = 32;
const TEMP_TOKEN_BYTES: usize = 16;
const TEMP_TOKEN_ATTEMPTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentStoreLimits {
    pub max_entries: usize,
    pub max_bytes: u64,
}

impl ContentStoreLimits {
    pub const fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentStoreErrorKind {
    InvalidConfiguration,
    Capacity,
    InvalidReference,
    Missing,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentStoreError {
    kind: ContentStoreErrorKind,
    diagnostic: String,
}

impl ContentStoreError {
    fn new(kind: ContentStoreErrorKind, diagnostic: impl fmt::Display) -> Self {
        Self {
            kind,
            diagnostic: super::bounded_diagnostic(diagnostic.to_string()),
        }
    }

    pub const fn kind(&self) -> ContentStoreErrorKind {
        self.kind
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl fmt::Display for ContentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for ContentStoreError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentRef {
    digest: [u8; CONTENT_DIGEST_BYTES],
    byte_count: u64,
}

impl ContentRef {
    pub fn new(digest: [u8; CONTENT_DIGEST_BYTES], byte_count: u64) -> Self {
        Self { digest, byte_count }
    }

    pub const fn digest(self) -> [u8; CONTENT_DIGEST_BYTES] {
        self.digest
    }

    pub const fn byte_count(self) -> u64 {
        self.byte_count
    }

    pub fn value(self) -> Result<Value, ContentStoreError> {
        let byte_count = i64::try_from(self.byte_count).map_err(|_| {
            ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content byte count exceeds the Boon Number range",
            )
        })?;
        let byte_count = FiniteReal::from_i64_exact(byte_count).map_err(|_| {
            ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content byte count is not exactly representable as a Boon Number",
            )
        })?;
        Ok(Value::Record(BTreeMap::from([
            ("digest".to_owned(), Value::Bytes(self.digest.to_vec())),
            ("byte_count".to_owned(), Value::Number(byte_count)),
        ])))
    }

    pub fn from_value(value: &Value) -> Result<Self, ContentStoreError> {
        let Value::Record(fields) = value else {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content reference must be a record",
            ));
        };
        if fields.len() != 2 || !fields.contains_key("digest") || !fields.contains_key("byte_count")
        {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content reference fields differ from the typed contract",
            ));
        }
        let digest = match fields.get("digest") {
            Some(Value::Bytes(digest)) => <[u8; CONTENT_DIGEST_BYTES]>::try_from(digest.as_slice())
                .map_err(|_| {
                    ContentStoreError::new(
                        ContentStoreErrorKind::InvalidReference,
                        "content digest must contain exactly 32 bytes",
                    )
                })?,
            _ => {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "content digest must be Bytes",
                ));
            }
        };
        let byte_count = match fields.get("byte_count") {
            Some(Value::Number(byte_count)) => byte_count
                .to_i64_exact()
                .ok()
                .and_then(|byte_count| u64::try_from(byte_count).ok())
                .ok_or_else(|| {
                    ContentStoreError::new(
                        ContentStoreErrorKind::InvalidReference,
                        "content byte count must be a non-negative exact whole Number",
                    )
                })?,
            _ => {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "content byte count must be Number",
                ));
            }
        };
        Ok(Self { digest, byte_count })
    }
}

#[derive(Clone)]
pub struct ContentStore {
    inner: Arc<Mutex<ContentStoreState>>,
}

struct ContentStoreState {
    root: PathBuf,
    limits: ContentStoreLimits,
    entries: BTreeMap<[u8; CONTENT_DIGEST_BYTES], ContentEntry>,
    stored_bytes: u64,
    pending_bytes: u64,
    pending_writers: usize,
    use_sequence: u64,
}

struct ContentEntry {
    path: PathBuf,
    byte_count: u64,
    pin_count: usize,
    last_used: u64,
}

impl Drop for ContentStoreState {
    fn drop(&mut self) {
        for entry in self.entries.values() {
            let _ = fs::remove_file(&entry.path);
        }
        let _ = fs::remove_dir(&self.root);
    }
}

impl ContentStore {
    pub fn new(
        root: impl Into<PathBuf>,
        limits: ContentStoreLimits,
    ) -> Result<Self, ContentStoreError> {
        if limits.max_entries == 0 || limits.max_bytes == 0 {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidConfiguration,
                "content store entry and byte limits must be positive",
            ));
        }
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidConfiguration,
                "content store root must not be empty",
            ));
        }
        fs::create_dir_all(&root).map_err(|error| io_error("create content store", error))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(ContentStoreState {
                root,
                limits,
                entries: BTreeMap::new(),
                stored_bytes: 0,
                pending_bytes: 0,
                pending_writers: 0,
                use_sequence: 0,
            })),
        })
    }

    pub fn limits(&self) -> ContentStoreLimits {
        self.lock().limits
    }

    pub fn entry_count(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn pending_writer_count(&self) -> usize {
        self.lock().pending_writers
    }

    pub fn stored_bytes(&self) -> u64 {
        self.lock().stored_bytes
    }

    pub fn contains(&self, content: ContentRef) -> bool {
        self.lock()
            .entries
            .get(&content.digest)
            .is_some_and(|entry| entry.byte_count == content.byte_count)
    }

    pub fn insert_bytes(&self, bytes: &[u8]) -> Result<ContentRef, ContentStoreError> {
        use sha2::{Digest, Sha256};

        let byte_count = u64::try_from(bytes.len()).map_err(|_| {
            ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "content byte length exceeds the host range",
            )
        })?;
        let digest = <[u8; CONTENT_DIGEST_BYTES]>::from(Sha256::digest(bytes));
        let content = ContentRef::new(digest, byte_count);
        let mut writer = self.begin_write(byte_count)?;
        writer.write_chunk(bytes)?;
        writer.finish(content)
    }

    pub fn begin_write(&self, expected_bytes: u64) -> Result<ContentWriter, ContentStoreError> {
        let (root, temp_path) = {
            let mut state = self.lock();
            if expected_bytes > state.limits.max_bytes {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content exceeds the configured byte capacity",
                ));
            }
            make_room(&mut state, expected_bytes, true)?;
            let temp_path = unique_temp_path(&state.root)?;
            state.pending_bytes =
                state
                    .pending_bytes
                    .checked_add(expected_bytes)
                    .ok_or_else(|| {
                        ContentStoreError::new(
                            ContentStoreErrorKind::Capacity,
                            "content pending-byte count overflow",
                        )
                    })?;
            state.pending_writers = state.pending_writers.checked_add(1).ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content pending-writer count overflow",
                )
            })?;
            (state.root.clone(), temp_path)
        };
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| {
                self.release_pending(expected_bytes);
                io_error("create content materialization", error)
            })?;
        Ok(ContentWriter {
            store: self.clone(),
            root,
            temp_path,
            file: Some(file),
            expected_bytes,
            written_bytes: 0,
            finished: false,
        })
    }

    pub fn resolve(&self, content: ContentRef) -> Result<ContentLease, ContentStoreError> {
        let path = {
            let mut state = self.lock();
            let use_sequence = next_use_sequence(&mut state);
            let entry = state.entries.get_mut(&content.digest).ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Missing,
                    "content is absent from the bounded host store",
                )
            })?;
            if entry.byte_count != content.byte_count {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "content digest and byte count disagree with the host store",
                ));
            }
            entry.pin_count = entry.pin_count.checked_add(1).ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content lease count overflow",
                )
            })?;
            entry.last_used = use_sequence;
            entry.path.clone()
        };
        Ok(ContentLease {
            store: self.clone(),
            content,
            path,
        })
    }

    pub fn remove(&self, content: ContentRef) -> bool {
        let entry =
            {
                let mut state = self.lock();
                if state.entries.get(&content.digest).is_none_or(|entry| {
                    entry.byte_count != content.byte_count || entry.pin_count > 0
                }) {
                    return false;
                }
                let entry = state
                    .entries
                    .remove(&content.digest)
                    .expect("removable content was checked");
                state.stored_bytes = state.stored_bytes.saturating_sub(entry.byte_count);
                entry
            };
        let _ = fs::remove_file(entry.path);
        true
    }

    fn finish_write(
        &self,
        temp_path: &Path,
        expected_bytes: u64,
        content: ContentRef,
    ) -> Result<(), ContentStoreError> {
        let mut stale_path = None;
        let mut state = self.lock();
        release_pending_locked(&mut state, expected_bytes);
        if let Some(existing) = state.entries.get(&content.digest) {
            if existing.byte_count != content.byte_count {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "equal content digests have conflicting byte counts",
                ));
            }
            stale_path = Some(temp_path.to_path_buf());
            let use_sequence = next_use_sequence(&mut state);
            state
                .entries
                .get_mut(&content.digest)
                .expect("existing content was checked")
                .last_used = use_sequence;
        } else {
            make_room(&mut state, content.byte_count, true)?;
            let final_path = state.root.join(hex_digest(content.digest));
            if final_path.exists() {
                fs::remove_file(&final_path)
                    .map_err(|error| io_error("replace stale content file", error))?;
            }
            fs::rename(temp_path, &final_path)
                .map_err(|error| io_error("publish content materialization", error))?;
            let use_sequence = next_use_sequence(&mut state);
            state.entries.insert(
                content.digest,
                ContentEntry {
                    path: final_path,
                    byte_count: content.byte_count,
                    pin_count: 0,
                    last_used: use_sequence,
                },
            );
            state.stored_bytes = state
                .stored_bytes
                .checked_add(content.byte_count)
                .expect("content capacity check prevents byte overflow");
        }
        drop(state);
        if let Some(stale_path) = stale_path {
            let _ = fs::remove_file(stale_path);
        }
        Ok(())
    }

    fn release_pending(&self, expected_bytes: u64) {
        release_pending_locked(&mut self.lock(), expected_bytes);
    }

    fn release_lease(&self, content: ContentRef) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&content.digest)
            && entry.byte_count == content.byte_count
        {
            entry.pin_count = entry.pin_count.saturating_sub(1);
        }
    }

    fn lock(&self) -> MutexGuard<'_, ContentStoreState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct ContentWriter {
    store: ContentStore,
    root: PathBuf,
    temp_path: PathBuf,
    file: Option<File>,
    expected_bytes: u64,
    written_bytes: u64,
    finished: bool,
}

impl ContentWriter {
    pub fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), ContentStoreError> {
        let byte_count = u64::try_from(bytes.len()).map_err(|_| {
            ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "content chunk length exceeds the host range",
            )
        })?;
        let written_bytes = self.written_bytes.checked_add(byte_count).ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "content materialization byte count overflow",
            )
        })?;
        if written_bytes > self.expected_bytes {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content materialization exceeded the selected file size",
            ));
        }
        self.file
            .as_mut()
            .expect("unfinished content writer owns a file")
            .write_all(bytes)
            .map_err(|error| io_error("write content materialization", error))?;
        self.written_bytes = written_bytes;
        Ok(())
    }

    pub fn finish(mut self, content: ContentRef) -> Result<ContentRef, ContentStoreError> {
        if self.written_bytes != self.expected_bytes || content.byte_count != self.written_bytes {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content materialization length differs from its descriptor",
            ));
        }
        let mut file = self
            .file
            .take()
            .expect("unfinished content writer owns a file");
        file.flush()
            .map_err(|error| io_error("flush content materialization", error))?;
        drop(file);
        self.finished = true;
        if let Err(error) = self
            .store
            .finish_write(&self.temp_path, self.expected_bytes, content)
        {
            let _ = fs::remove_file(&self.temp_path);
            return Err(error);
        }
        Ok(content)
    }

    pub fn written_bytes(&self) -> u64 {
        self.written_bytes
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for ContentWriter {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.file.take();
        let _ = fs::remove_file(&self.temp_path);
        self.store.release_pending(self.expected_bytes);
    }
}

pub struct ContentLease {
    store: ContentStore,
    content: ContentRef,
    path: PathBuf,
}

impl ContentLease {
    pub const fn content(&self) -> ContentRef {
        self.content
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        self.store.release_lease(self.content);
    }
}

fn make_room(
    state: &mut ContentStoreState,
    incoming_bytes: u64,
    reserve_entry: bool,
) -> Result<(), ContentStoreError> {
    loop {
        let entry_count = state
            .entries
            .len()
            .checked_add(state.pending_writers)
            .and_then(|count| count.checked_add(usize::from(reserve_entry)))
            .ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content entry count overflow",
                )
            })?;
        let byte_count = state
            .stored_bytes
            .checked_add(state.pending_bytes)
            .and_then(|bytes| bytes.checked_add(incoming_bytes))
            .ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content byte capacity overflow",
                )
            })?;
        if entry_count <= state.limits.max_entries && byte_count <= state.limits.max_bytes {
            return Ok(());
        }
        let victim = state
            .entries
            .iter()
            .filter(|(_, entry)| entry.pin_count == 0)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(digest, _)| *digest)
            .ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content store capacity is pinned or reserved",
                )
            })?;
        let entry = state
            .entries
            .remove(&victim)
            .expect("eviction victim exists");
        state.stored_bytes = state.stored_bytes.saturating_sub(entry.byte_count);
        let _ = fs::remove_file(entry.path);
    }
}

fn release_pending_locked(state: &mut ContentStoreState, expected_bytes: u64) {
    state.pending_bytes = state.pending_bytes.saturating_sub(expected_bytes);
    state.pending_writers = state.pending_writers.saturating_sub(1);
}

fn next_use_sequence(state: &mut ContentStoreState) -> u64 {
    state.use_sequence = state.use_sequence.wrapping_add(1).max(1);
    state.use_sequence
}

fn unique_temp_path(root: &Path) -> Result<PathBuf, ContentStoreError> {
    for _ in 0..TEMP_TOKEN_ATTEMPTS {
        let mut token = [0_u8; TEMP_TOKEN_BYTES];
        getrandom::fill(&mut token).map_err(|error| {
            ContentStoreError::new(
                ContentStoreErrorKind::Io,
                format_args!("cannot generate content temp identity: {error}"),
            )
        })?;
        let path = root.join(format!(".partial-{}", hex_bytes(&token)));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ContentStoreError::new(
        ContentStoreErrorKind::Io,
        "cannot generate a unique content temp path",
    ))
}

fn hex_digest(digest: [u8; CONTENT_DIGEST_BYTES]) -> String {
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn io_error(action: &str, error: io::Error) -> ContentStoreError {
    ContentStoreError::new(
        ContentStoreErrorKind::Io,
        format_args!("cannot {action}: {:?}", error.kind()),
    )
}
