use boon_runtime::{CONTENT_DIGEST_BYTES, ContentRef, ContentRefError};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

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
    Corrupt,
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

impl From<ContentRefError> for ContentStoreError {
    fn from(error: ContentRefError) -> Self {
        Self::new(ContentStoreErrorKind::InvalidReference, error)
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
    durable_roots: BTreeSet<[u8; CONTENT_DIGEST_BYTES]>,
    stored_bytes: u64,
    pending_bytes: u64,
    pending_writers: usize,
    use_sequence: u64,
}

struct ContentEntry {
    path: PathBuf,
    size: u64,
    pin_count: usize,
    last_used: u64,
    integrity: ContentIntegrity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentIntegrity {
    Unverified,
    Verified,
    Corrupt,
}

impl ContentStore {
    pub fn new(
        root: impl Into<PathBuf>,
        limits: ContentStoreLimits,
    ) -> Result<Self, ContentStoreError> {
        Self::new_with_durable_roots(root, limits, std::iter::empty())
    }

    pub fn new_with_durable_roots(
        root: impl Into<PathBuf>,
        limits: ContentStoreLimits,
        durable_roots: impl IntoIterator<Item = ContentRef>,
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
        let recovered = recover_entries(&root, limits)?;
        let durable_roots = validate_durable_roots(&recovered.entries, durable_roots)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(ContentStoreState {
                root,
                limits,
                entries: recovered.entries,
                durable_roots,
                stored_bytes: recovered.stored_bytes,
                pending_bytes: 0,
                pending_writers: 0,
                use_sequence: recovered.use_sequence,
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

    pub fn durable_root_count(&self) -> usize {
        self.lock().durable_roots.len()
    }

    pub fn replace_durable_roots(
        &self,
        durable_roots: impl IntoIterator<Item = ContentRef>,
    ) -> Result<(), ContentStoreError> {
        let mut state = self.lock();
        let roots = validate_durable_roots(&state.entries, durable_roots)?;
        state.durable_roots = roots;
        Ok(())
    }

    pub fn contains(&self, content: &ContentRef) -> bool {
        self.lock()
            .entries
            .get(&content.digest())
            .is_some_and(|entry| {
                entry.size == content.size() && entry.integrity != ContentIntegrity::Corrupt
            })
    }

    pub fn insert_bytes(
        &self,
        bytes: &[u8],
        media: impl Into<Arc<str>>,
    ) -> Result<ContentRef, ContentStoreError> {
        let size = u64::try_from(bytes.len()).map_err(|_| {
            ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "content byte length exceeds the host range",
            )
        })?;
        let digest = <[u8; CONTENT_DIGEST_BYTES]>::from(Sha256::digest(bytes));
        let content = ContentRef::new(digest, size, media)?;
        let mut writer = self.begin_write(size)?;
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
            digest: Sha256::new(),
            finished: false,
        })
    }

    pub fn resolve(&self, content: &ContentRef) -> Result<ContentLease, ContentStoreError> {
        let (path, verify) = {
            let mut state = self.lock();
            let use_sequence = next_use_sequence(&mut state);
            let entry = state.entries.get_mut(&content.digest()).ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Missing,
                    "content is absent from the bounded host store",
                )
            })?;
            if entry.size != content.size() {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "content digest and size disagree with the host store",
                ));
            }
            if entry.integrity == ContentIntegrity::Corrupt {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::Corrupt,
                    "content failed its durable integrity check",
                ));
            }
            entry.pin_count = entry.pin_count.checked_add(1).ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content lease count overflow",
                )
            })?;
            entry.last_used = use_sequence;
            (
                entry.path.clone(),
                entry.integrity == ContentIntegrity::Unverified,
            )
        };
        if verify {
            match verify_file(&path, content) {
                Ok(()) => self.mark_verified(content),
                Err(error) => {
                    self.release_failed_verification(
                        content,
                        error.kind() == ContentStoreErrorKind::Corrupt,
                    );
                    return Err(error);
                }
            }
        }
        Ok(ContentLease {
            store: self.clone(),
            content: content.clone(),
            path,
        })
    }

    pub fn remove(&self, content: &ContentRef) -> Result<bool, ContentStoreError> {
        let mut state = self.lock();
        let Some(entry) = state.entries.get(&content.digest()) else {
            return Ok(false);
        };
        if entry.size != content.size()
            || entry.pin_count > 0
            || state.durable_roots.contains(&content.digest())
        {
            return Ok(false);
        }
        fs::remove_file(&entry.path).map_err(|error| io_error("remove content", error))?;
        let size = entry.size;
        state.entries.remove(&content.digest());
        state.stored_bytes = state.stored_bytes.saturating_sub(size);
        sync_directory(&state.root)?;
        Ok(true)
    }

    fn finish_write(
        &self,
        temp_path: &Path,
        expected_bytes: u64,
        content: &ContentRef,
    ) -> Result<(), ContentStoreError> {
        let mut stale_path = None;
        let mut state = self.lock();
        release_pending_locked(&mut state, expected_bytes);
        if let Some(existing) = state.entries.get(&content.digest()) {
            if existing.size != content.size() {
                return Err(ContentStoreError::new(
                    ContentStoreErrorKind::InvalidReference,
                    "equal content digests have conflicting sizes",
                ));
            }
            if existing.integrity == ContentIntegrity::Verified {
                stale_path = Some(temp_path.to_path_buf());
                let use_sequence = next_use_sequence(&mut state);
                state
                    .entries
                    .get_mut(&content.digest())
                    .expect("existing content was checked")
                    .last_used = use_sequence;
            } else {
                let final_path = existing.path.clone();
                publish_temp_file(temp_path, &final_path)?;
                sync_directory(&state.root)?;
                let use_sequence = next_use_sequence(&mut state);
                let existing = state
                    .entries
                    .get_mut(&content.digest())
                    .expect("existing content was checked");
                existing.integrity = ContentIntegrity::Verified;
                existing.last_used = use_sequence;
            }
        } else {
            make_room(&mut state, content.size(), true)?;
            let final_path = state.root.join(hex_digest(content.digest()));
            publish_temp_file(temp_path, &final_path)?;
            sync_directory(&state.root)?;
            let use_sequence = next_use_sequence(&mut state);
            state.entries.insert(
                content.digest(),
                ContentEntry {
                    path: final_path,
                    size: content.size(),
                    pin_count: 0,
                    last_used: use_sequence,
                    integrity: ContentIntegrity::Verified,
                },
            );
            state.stored_bytes = state
                .stored_bytes
                .checked_add(content.size())
                .expect("content capacity check prevents byte overflow");
        }
        drop(state);
        if let Some(stale_path) = stale_path {
            fs::remove_file(stale_path)
                .map_err(|error| io_error("remove duplicate content materialization", error))?;
        }
        Ok(())
    }

    fn release_pending(&self, expected_bytes: u64) {
        release_pending_locked(&mut self.lock(), expected_bytes);
    }

    fn release_lease(&self, content: &ContentRef) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&content.digest())
            && entry.size == content.size()
        {
            entry.pin_count = entry.pin_count.saturating_sub(1);
        }
    }

    fn mark_verified(&self, content: &ContentRef) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&content.digest())
            && entry.size == content.size()
            && entry.integrity == ContentIntegrity::Unverified
        {
            entry.integrity = ContentIntegrity::Verified;
        }
    }

    fn release_failed_verification(&self, content: &ContentRef, corrupt: bool) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&content.digest())
            && entry.size == content.size()
        {
            entry.pin_count = entry.pin_count.saturating_sub(1);
            if corrupt {
                entry.integrity = ContentIntegrity::Corrupt;
            }
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
    digest: Sha256,
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
        self.digest.update(bytes);
        self.written_bytes = written_bytes;
        Ok(())
    }

    pub fn finish(mut self, content: ContentRef) -> Result<ContentRef, ContentStoreError> {
        if self.written_bytes != self.expected_bytes || content.size() != self.written_bytes {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content materialization length differs from its descriptor",
            ));
        }
        let actual_digest = <[u8; CONTENT_DIGEST_BYTES]>::from(self.digest.clone().finalize());
        if actual_digest != content.digest() {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "content materialization digest differs from its descriptor",
            ));
        }
        let mut file = self
            .file
            .take()
            .expect("unfinished content writer owns a file");
        file.flush()
            .map_err(|error| io_error("flush content materialization", error))?;
        file.sync_all()
            .map_err(|error| io_error("sync content materialization", error))?;
        drop(file);
        self.finished = true;
        if let Err(error) = self
            .store
            .finish_write(&self.temp_path, self.expected_bytes, &content)
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
    pub const fn content(&self) -> &ContentRef {
        &self.content
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        self.store.release_lease(&self.content);
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
            .filter(|(digest, entry)| {
                entry.pin_count == 0 && !state.durable_roots.contains(*digest)
            })
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(digest, _)| *digest)
            .ok_or_else(|| {
                ContentStoreError::new(
                    ContentStoreErrorKind::Capacity,
                    "content store capacity is pinned or reserved",
                )
            })?;
        let path = state
            .entries
            .get(&victim)
            .expect("eviction victim exists")
            .path
            .clone();
        fs::remove_file(&path).map_err(|error| io_error("evict content", error))?;
        let entry = state
            .entries
            .remove(&victim)
            .expect("eviction victim exists");
        state.stored_bytes = state.stored_bytes.saturating_sub(entry.size);
        sync_directory(&state.root)?;
    }
}

fn validate_durable_roots(
    entries: &BTreeMap<[u8; CONTENT_DIGEST_BYTES], ContentEntry>,
    roots: impl IntoIterator<Item = ContentRef>,
) -> Result<BTreeSet<[u8; CONTENT_DIGEST_BYTES]>, ContentStoreError> {
    let mut validated = BTreeSet::new();
    for content in roots {
        let entry = entries.get(&content.digest()).ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::Missing,
                "durable state references content absent from the host store",
            )
        })?;
        if entry.size != content.size() {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidReference,
                "durable content digest and size disagree with the host store",
            ));
        }
        if entry.integrity == ContentIntegrity::Corrupt {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::Corrupt,
                "durable state references content that failed integrity verification",
            ));
        }
        validated.insert(content.digest());
    }
    Ok(validated)
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

#[cfg(not(windows))]
fn publish_temp_file(temp_path: &Path, final_path: &Path) -> Result<(), ContentStoreError> {
    fs::rename(temp_path, final_path)
        .map_err(|error| io_error("atomically publish content materialization", error))
}

#[cfg(windows)]
fn publish_temp_file(temp_path: &Path, final_path: &Path) -> Result<(), ContentStoreError> {
    use atomic_write_file::AtomicWriteFile;

    let mut source = File::open(temp_path)
        .map_err(|error| io_error("open content materialization for publication", error))?;
    let mut target = AtomicWriteFile::options()
        .open(final_path)
        .map_err(|error| io_error("open atomic content publication", error))?;
    io::copy(&mut source, &mut target)
        .map_err(|error| io_error("copy atomic content publication", error))?;
    target
        .commit()
        .map_err(|error| io_error("commit atomic content publication", error))?;
    fs::remove_file(temp_path)
        .map_err(|error| io_error("remove published content materialization", error))
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

struct RecoveredEntries {
    entries: BTreeMap<[u8; CONTENT_DIGEST_BYTES], ContentEntry>,
    stored_bytes: u64,
    use_sequence: u64,
}

fn recover_entries(
    root: &Path,
    limits: ContentStoreLimits,
) -> Result<RecoveredEntries, ContentStoreError> {
    let mut entries = BTreeMap::new();
    let mut stored_bytes = 0_u64;
    let mut removed_partial = false;
    for entry in fs::read_dir(root).map_err(|error| io_error("scan content store", error))? {
        let entry = entry.map_err(|error| io_error("scan content store entry", error))?;
        let file_name = entry.file_name();
        let file_name = file_name.to_str().ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::InvalidConfiguration,
                "content store contains a non-UTF-8 file name",
            )
        })?;
        if file_name.starts_with(".partial-") {
            fs::remove_file(entry.path())
                .map_err(|error| io_error("remove abandoned content materialization", error))?;
            removed_partial = true;
            continue;
        }
        let digest = parse_digest(file_name).ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::InvalidConfiguration,
                "content store contains an unexpected entry",
            )
        })?;
        let metadata = entry
            .metadata()
            .map_err(|error| io_error("inspect recovered content", error))?;
        if !metadata.is_file() {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::InvalidConfiguration,
                "content store digest entry is not a regular file",
            ));
        }
        let size = metadata.len();
        stored_bytes = stored_bytes.checked_add(size).ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "recovered content byte count overflow",
            )
        })?;
        if entries.len() >= limits.max_entries || stored_bytes > limits.max_bytes {
            return Err(ContentStoreError::new(
                ContentStoreErrorKind::Capacity,
                "recovered content exceeds the configured store limits",
            ));
        }
        entries.insert(
            digest,
            ContentEntry {
                path: entry.path(),
                size,
                pin_count: 0,
                last_used: entries.len() as u64 + 1,
                integrity: ContentIntegrity::Unverified,
            },
        );
    }
    if removed_partial {
        sync_directory(root)?;
    }
    Ok(RecoveredEntries {
        use_sequence: entries.len() as u64,
        entries,
        stored_bytes,
    })
}

fn verify_file(path: &Path, content: &ContentRef) -> Result<(), ContentStoreError> {
    let mut file = File::open(path).map_err(|error| io_error("open recovered content", error))?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error("read recovered content", error))?;
        if read == 0 {
            break;
        }
        size = size.checked_add(read as u64).ok_or_else(|| {
            ContentStoreError::new(
                ContentStoreErrorKind::Corrupt,
                "recovered content size overflow",
            )
        })?;
        digest.update(&buffer[..read]);
    }
    let digest = <[u8; CONTENT_DIGEST_BYTES]>::from(digest.finalize());
    if size != content.size() || digest != content.digest() {
        return Err(ContentStoreError::new(
            ContentStoreErrorKind::Corrupt,
            "recovered content differs from its digest or size descriptor",
        ));
    }
    Ok(())
}

fn parse_digest(value: &str) -> Option<[u8; CONTENT_DIGEST_BYTES]> {
    if value.len() != CONTENT_DIGEST_BYTES * 2 {
        return None;
    }
    let mut digest = [0_u8; CONTENT_DIGEST_BYTES];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(digest)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn sync_directory(path: &Path) -> Result<(), ContentStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync content store directory", error))
}

fn io_error(action: &str, error: io::Error) -> ContentStoreError {
    ContentStoreError::new(
        ContentStoreErrorKind::Io,
        format_args!("cannot {action}: {:?}", error.kind()),
    )
}
