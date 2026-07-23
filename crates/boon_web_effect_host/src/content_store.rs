//! Package-scoped, content-addressed browser storage.
//!
//! This module owns immutable content bytes only. It deliberately does not
//! persist Boon runtime state and never exposes IndexedDB or staging identities
//! through a Boon value.

use boon_runtime::{CONTENT_DIGEST_BYTES, ContentRef, MAX_CONTENT_MEDIA_BYTES};
use idb::{
    Database, KeyRange, ObjectStore, Query, TransactionFuture, TransactionMode, TransactionResult,
};
use js_sys::{Reflect, Uint8Array};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::future::IntoFuture;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;

pub(crate) const CONTENT_STORE_SCHEMA_VERSION: u32 = 2;

const DATABASE_NAME_PREFIX: &str = "boon-content::";
const METADATA_STORE: &str = "metadata";
const CHUNK_STORE: &str = "chunks";
const STAGING_STORE: &str = "staging";
const USAGE_STORE: &str = "usage";
const OBJECT_STORES: [&str; 4] = [CHUNK_STORE, METADATA_STORE, STAGING_STORE, USAGE_STORE];
const USAGE_KEY: &str = "usage-v1";

const RECORD_FORMAT_VERSION: u8 = 2;
const OPAQUE_ID_BYTES: usize = 16;
const STAGING_ID_ATTEMPTS: usize = 16;
const MAX_PACKAGE_ID_BYTES: usize = 256;
const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
const MAX_DIAGNOSTIC_BYTES: usize = 512;
const DEFAULT_STAGING_LEASE_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_STAGING_LEASE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

// These absolute bounds cap recovery work even when IndexedDB was modified by
// another implementation. Runtime limits must fit within them.
const ABSOLUTE_MAX_CONTENT_ENTRIES: u32 = 65_535;
const ABSOLUTE_MAX_STAGING_IMPORTS: u32 = 1_024;
const ABSOLUTE_MAX_CHUNKS: u32 = 1_000_000;
const ABSOLUTE_MAX_CHUNK_BYTES: u32 = 16 * 1024 * 1024;
const ABSOLUTE_MAX_READ_BYTES: u32 = 64 * 1024 * 1024;

const METADATA_FIXED_BYTES: usize = 1 + CONTENT_DIGEST_BYTES + OPAQUE_ID_BYTES + 8 + 4 + 4 + 2;
const STAGING_FIXED_BYTES: usize = 1 + OPAQUE_ID_BYTES * 2 + 8 + 8 + 8 + 4 + 4 + 2;
const USAGE_RECORD_BYTES: usize = 1 + 4 + 8 + 4 + 4 + 8 + 4;

pub(crate) type BrowserContentStoreResult<T> = Result<T, BrowserContentStoreError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserContentStoreLimits {
    pub max_content_entries: u32,
    pub max_content_bytes: u64,
    pub max_content_chunks: u32,
    pub max_staging_imports: u32,
    pub max_staging_bytes: u64,
    pub max_staging_chunks: u32,
    pub max_chunks_per_content: u32,
    pub max_chunk_bytes: u32,
    pub max_read_bytes: u32,
    pub max_read_chunks: u32,
    pub staging_lease_ms: u64,
}

impl Default for BrowserContentStoreLimits {
    fn default() -> Self {
        Self {
            max_content_entries: 128,
            max_content_bytes: 512 * 1024 * 1024,
            max_content_chunks: 131_072,
            max_staging_imports: 8,
            max_staging_bytes: 128 * 1024 * 1024,
            max_staging_chunks: 32_768,
            max_chunks_per_content: 16_384,
            max_chunk_bytes: 1024 * 1024,
            max_read_bytes: 8 * 1024 * 1024,
            max_read_chunks: 2_048,
            staging_lease_ms: DEFAULT_STAGING_LEASE_MS,
        }
    }
}

impl BrowserContentStoreLimits {
    pub(crate) fn validate(self) -> BrowserContentStoreResult<Self> {
        let positive = [
            ("max_content_entries", u64::from(self.max_content_entries)),
            ("max_content_bytes", self.max_content_bytes),
            ("max_content_chunks", u64::from(self.max_content_chunks)),
            ("max_staging_imports", u64::from(self.max_staging_imports)),
            ("max_staging_bytes", self.max_staging_bytes),
            ("max_staging_chunks", u64::from(self.max_staging_chunks)),
            (
                "max_chunks_per_content",
                u64::from(self.max_chunks_per_content),
            ),
            ("max_chunk_bytes", u64::from(self.max_chunk_bytes)),
            ("max_read_bytes", u64::from(self.max_read_bytes)),
            ("max_read_chunks", u64::from(self.max_read_chunks)),
            ("staging_lease_ms", self.staging_lease_ms),
        ];
        if let Some((field, _)) = positive.into_iter().find(|(_, value)| *value == 0) {
            return Err(BrowserContentStoreError::InvalidConfiguration {
                reason: format!("{field} must be positive"),
            });
        }
        if self.max_content_entries > ABSOLUTE_MAX_CONTENT_ENTRIES {
            return Err(configuration_limit(
                "max_content_entries",
                ABSOLUTE_MAX_CONTENT_ENTRIES,
            ));
        }
        if self.max_staging_imports > ABSOLUTE_MAX_STAGING_IMPORTS {
            return Err(configuration_limit(
                "max_staging_imports",
                ABSOLUTE_MAX_STAGING_IMPORTS,
            ));
        }
        if self.max_content_chunks > ABSOLUTE_MAX_CHUNKS
            || self.max_staging_chunks > ABSOLUTE_MAX_CHUNKS
            || self.max_chunks_per_content > ABSOLUTE_MAX_CHUNKS
        {
            return Err(configuration_limit(
                "content chunk counts",
                ABSOLUTE_MAX_CHUNKS,
            ));
        }
        if self.max_chunk_bytes > ABSOLUTE_MAX_CHUNK_BYTES {
            return Err(configuration_limit(
                "max_chunk_bytes",
                ABSOLUTE_MAX_CHUNK_BYTES,
            ));
        }
        if self.max_read_bytes > ABSOLUTE_MAX_READ_BYTES {
            return Err(configuration_limit(
                "max_read_bytes",
                ABSOLUTE_MAX_READ_BYTES,
            ));
        }
        if self.staging_lease_ms > MAX_STAGING_LEASE_MS {
            return Err(BrowserContentStoreError::InvalidConfiguration {
                reason: format!(
                    "staging_lease_ms exceeds the bounded maximum {MAX_STAGING_LEASE_MS}"
                ),
            });
        }
        if self.max_content_bytes > MAX_SAFE_INTEGER || self.max_staging_bytes > MAX_SAFE_INTEGER {
            return Err(BrowserContentStoreError::InvalidConfiguration {
                reason: "browser content byte limits exceed the exact JavaScript integer range"
                    .to_owned(),
            });
        }
        if self.max_chunks_per_content > self.max_content_chunks
            || self.max_chunks_per_content > self.max_staging_chunks
        {
            return Err(BrowserContentStoreError::InvalidConfiguration {
                reason: "max_chunks_per_content exceeds a total content or staging chunk limit"
                    .to_owned(),
            });
        }
        if self.max_read_chunks > self.max_chunks_per_content {
            return Err(BrowserContentStoreError::InvalidConfiguration {
                reason: "max_read_chunks exceeds max_chunks_per_content".to_owned(),
            });
        }
        Ok(self)
    }
}

fn configuration_limit(field: &str, limit: u32) -> BrowserContentStoreError {
    BrowserContentStoreError::InvalidConfiguration {
        reason: format!("{field} exceeds the recovery bound {limit}"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserContentStoreError {
    InvalidConfiguration {
        reason: String,
    },
    InvalidInput {
        field: &'static str,
        reason: String,
    },
    LimitExceeded {
        resource: &'static str,
        limit: u64,
    },
    QuotaExceeded {
        operation: String,
        message: String,
    },
    Aborted {
        operation: String,
    },
    VersionMismatch {
        expected: u32,
        actual: Option<u32>,
    },
    SchemaMismatch {
        reason: String,
    },
    Missing {
        resource: &'static str,
    },
    Corrupt {
        resource: &'static str,
        reason: String,
    },
    DigestMismatch {
        phase: &'static str,
    },
    SizeMismatch {
        declared: u64,
        actual: u64,
    },
    SequenceMismatch {
        expected: u32,
        actual: u32,
    },
    Platform {
        operation: String,
        name: Option<String>,
        message: String,
    },
}

impl BrowserContentStoreError {
    pub(crate) const fn is_quota_exceeded(&self) -> bool {
        matches!(self, Self::QuotaExceeded { .. })
    }

    pub(crate) const fn is_version_mismatch(&self) -> bool {
        matches!(self, Self::VersionMismatch { .. })
    }
}

impl fmt::Display for BrowserContentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration { reason } => {
                write!(
                    formatter,
                    "invalid browser content store configuration: {reason}"
                )
            }
            Self::InvalidInput { field, reason } => {
                write!(formatter, "invalid {field}: {reason}")
            }
            Self::LimitExceeded { resource, limit } => {
                write!(formatter, "{resource} exceeds its configured limit {limit}")
            }
            Self::QuotaExceeded { operation, message } => {
                write!(
                    formatter,
                    "IndexedDB quota exceeded during {operation}: {message}"
                )
            }
            Self::Aborted { operation } => {
                write!(
                    formatter,
                    "IndexedDB transaction aborted during {operation}"
                )
            }
            Self::VersionMismatch { expected, actual } => match actual {
                Some(actual) => write!(
                    formatter,
                    "IndexedDB schema version is {actual}, expected {expected}"
                ),
                None => write!(
                    formatter,
                    "IndexedDB rejected schema version {expected} as incompatible"
                ),
            },
            Self::SchemaMismatch { reason } => {
                write!(formatter, "IndexedDB content schema mismatch: {reason}")
            }
            Self::Missing { resource } => write!(formatter, "missing {resource}"),
            Self::Corrupt { resource, reason } => {
                write!(formatter, "corrupt {resource}: {reason}")
            }
            Self::DigestMismatch { phase } => {
                write!(formatter, "SHA-256 mismatch during {phase}")
            }
            Self::SizeMismatch { declared, actual } => write!(
                formatter,
                "content size is {actual} bytes, expected exactly {declared} bytes"
            ),
            Self::SequenceMismatch { expected, actual } => write!(
                formatter,
                "content chunk sequence is {actual}, expected exactly {expected}"
            ),
            Self::Platform {
                operation,
                name,
                message,
            } => {
                if let Some(name) = name {
                    write!(
                        formatter,
                        "IndexedDB {operation} failed ({name}): {message}"
                    )
                } else {
                    write!(formatter, "IndexedDB {operation} failed: {message}")
                }
            }
        }
    }
}

impl std::error::Error for BrowserContentStoreError {}

#[derive(Clone)]
pub(crate) struct BrowserIndexedDbContentStore {
    inner: Rc<ContentStoreInner>,
}

struct ContentStoreInner {
    database: Database,
    database_name: String,
    limits: BrowserContentStoreLimits,
}

impl Drop for ContentStoreInner {
    fn drop(&mut self) {
        self.database.close();
    }
}

impl BrowserIndexedDbContentStore {
    /// Opens the one canonical content database for `package_id`, validates the
    /// exact version/store layout, and atomically removes incomplete imports.
    pub(crate) async fn open(
        package_id: &str,
        limits: BrowserContentStoreLimits,
    ) -> BrowserContentStoreResult<Self> {
        let limits = limits.validate()?;
        let database_name = canonical_database_name(package_id)?;
        let database = Database::builder(&database_name)
            .version(CONTENT_STORE_SCHEMA_VERSION)
            .add_object_store(ObjectStore::builder(METADATA_STORE))
            .add_object_store(ObjectStore::builder(CHUNK_STORE))
            .add_object_store(ObjectStore::builder(STAGING_STORE))
            .add_object_store(ObjectStore::builder(USAGE_STORE))
            .build()
            .await
            .map_err(open_error)?;

        let store = Self {
            inner: Rc::new(ContentStoreInner {
                database,
                database_name,
                limits,
            }),
        };
        store.validate_schema().await?;
        store.recover_storage_at(current_unix_ms()?).await?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn database_name(&self) -> &str {
        &self.inner.database_name
    }

    /// Starts an import by reserving its declared byte count. The returned
    /// owner contains private random staging and storage identities.
    pub(crate) async fn begin_import(
        &self,
        declared_size: u64,
        media: impl Into<String>,
    ) -> BrowserContentStoreResult<BrowserContentImport> {
        if declared_size > MAX_SAFE_INTEGER {
            return Err(BrowserContentStoreError::InvalidInput {
                field: "declared content size",
                reason: "size exceeds the exact JavaScript integer range".to_owned(),
            });
        }
        if declared_size > self.inner.limits.max_content_bytes {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "declared content bytes",
                limit: self.inner.limits.max_content_bytes,
            });
        }
        if declared_size > self.inner.limits.max_staging_bytes {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "one staging import's declared bytes",
                limit: self.inner.limits.max_staging_bytes,
            });
        }
        let media = media.into();
        validate_media(&media)?;

        for _ in 0..STAGING_ID_ATTEMPTS {
            let mut random = [0_u8; OPAQUE_ID_BYTES * 2];
            getrandom::fill(&mut random).map_err(|error| BrowserContentStoreError::Platform {
                operation: "generate opaque content staging identity".to_owned(),
                name: None,
                message: bounded_diagnostic(error.to_string()),
            })?;
            let staging_id = OpaqueId(
                random[..OPAQUE_ID_BYTES]
                    .try_into()
                    .expect("slice has the opaque ID width"),
            );
            let storage_id = OpaqueId(
                random[OPAQUE_ID_BYTES..]
                    .try_into()
                    .expect("slice has the opaque ID width"),
            );
            if staging_id == storage_id {
                continue;
            }
            let record = StagingRecord {
                staging_id,
                storage_id,
                declared_size,
                written_size: 0,
                lease_expires_ms: staging_lease_expiry(self.inner.limits)?,
                next_sequence: 0,
                chunk_bytes: 0,
                media: media.clone(),
            };
            match self.reserve_import(&record).await? {
                ReserveImportOutcome::Reserved => {
                    return Ok(BrowserContentImport {
                        store: self.clone(),
                        state: Some(ActiveImport {
                            record,
                            digest: Sha256::new(),
                        }),
                    });
                }
                ReserveImportOutcome::IdentityCollision => continue,
            }
        }
        Err(BrowserContentStoreError::Platform {
            operation: "generate opaque content staging identity".to_owned(),
            name: None,
            message: "could not mint a unique bounded staging identity".to_owned(),
        })
    }

    /// Resolves immutable storage metadata using only a serializable
    /// `ContentRef`; private storage identities remain inside this module.
    pub(crate) async fn resolve_metadata(
        &self,
        content: &ContentRef,
    ) -> BrowserContentStoreResult<BrowserContentMetadata> {
        let transaction = self
            .inner
            .database
            .transaction(&[METADATA_STORE], TransactionMode::ReadOnly)
            .map_err(|error| idb_error("resolve content metadata", error))?;
        let metadata = transaction
            .object_store(METADATA_STORE)
            .map_err(|error| idb_error("resolve content metadata", error))?;
        let completion = transaction.into_future();
        let operation = load_metadata(&metadata, content, self.inner.limits).await;
        let record = finish_transaction("resolve content metadata", operation, completion).await?;
        Ok(BrowserContentMetadata::from_record(
            content.clone(),
            &record,
        ))
    }

    /// Reads one canonical chunk. The configured chunk bound is checked again
    /// when decoding IndexedDB data.
    pub(crate) async fn read_chunk(
        &self,
        content: &ContentRef,
        sequence: u32,
    ) -> BrowserContentStoreResult<Vec<u8>> {
        let transaction = self
            .inner
            .database
            .transaction(&[METADATA_STORE, CHUNK_STORE], TransactionMode::ReadOnly)
            .map_err(|error| idb_error("read content chunk", error))?;
        let metadata = transaction
            .object_store(METADATA_STORE)
            .map_err(|error| idb_error("read content chunk", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("read content chunk", error))?;
        let completion = transaction.into_future();
        let operation = async {
            let record = load_metadata(&metadata, content, self.inner.limits).await?;
            if sequence >= record.chunk_count {
                return Err(BrowserContentStoreError::InvalidInput {
                    field: "content chunk sequence",
                    reason: format!(
                        "sequence {sequence} is outside the stored chunk count {}",
                        record.chunk_count
                    ),
                });
            }
            let value = chunks
                .get(JsValue::from_str(&chunk_key(record.storage_id, sequence)))
                .map_err(|error| idb_error("read content chunk", error))?
                .await
                .map_err(|error| idb_error("read content chunk", error))?
                .ok_or(BrowserContentStoreError::Missing {
                    resource: "content chunk",
                })?;
            let bytes = stored_bytes(
                "content chunk",
                value,
                self.inner.limits.max_chunk_bytes as usize,
            )?;
            validate_chunk_length(&record, sequence, bytes.len())?;
            Ok(bytes)
        }
        .await;
        finish_transaction("read content chunk", operation, completion).await
    }

    /// Reads a bounded byte range without materializing the complete content.
    /// At most `max_read_chunks` IndexedDB values are requested.
    #[cfg(test)]
    pub(crate) async fn read_range(
        &self,
        content: &ContentRef,
        offset: u64,
        length: u32,
    ) -> BrowserContentStoreResult<Vec<u8>> {
        if length > self.inner.limits.max_read_bytes {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "content range read bytes",
                limit: u64::from(self.inner.limits.max_read_bytes),
            });
        }
        let end = offset.checked_add(u64::from(length)).ok_or_else(|| {
            BrowserContentStoreError::InvalidInput {
                field: "content range",
                reason: "offset and length overflow".to_owned(),
            }
        })?;

        let transaction = self
            .inner
            .database
            .transaction(&[METADATA_STORE, CHUNK_STORE], TransactionMode::ReadOnly)
            .map_err(|error| idb_error("read content range", error))?;
        let metadata = transaction
            .object_store(METADATA_STORE)
            .map_err(|error| idb_error("read content range", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("read content range", error))?;
        let completion = transaction.into_future();
        let operation = async {
            let record = load_metadata(&metadata, content, self.inner.limits).await?;
            if offset > record.size || end > record.size {
                return Err(BrowserContentStoreError::InvalidInput {
                    field: "content range",
                    reason: format!("range {offset}..{end} exceeds stored size {}", record.size),
                });
            }
            if length == 0 {
                return Ok(Vec::new());
            }

            let chunk_bytes = u64::from(record.chunk_bytes);
            let first = u32::try_from(offset / chunk_bytes).map_err(|_| {
                corrupt(
                    "content metadata",
                    "range start exceeds the stored chunk index width",
                )
            })?;
            let last = u32::try_from((end - 1) / chunk_bytes).map_err(|_| {
                corrupt(
                    "content metadata",
                    "range end exceeds the stored chunk index width",
                )
            })?;
            let count = last
                .checked_sub(first)
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| corrupt("content metadata", "range chunk count overflow"))?;
            if count > self.inner.limits.max_read_chunks {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "chunks in one content range read",
                    limit: u64::from(self.inner.limits.max_read_chunks),
                });
            }
            let query = chunk_index_query(record.storage_id, first, last)?;
            let values = chunks
                .get_all(Some(query), Some(count))
                .map_err(|error| idb_error("read content range", error))?
                .await
                .map_err(|error| idb_error("read content range", error))?;
            if values.len() != count as usize {
                return Err(corrupt(
                    "content chunks",
                    "stored range has a missing chunk",
                ));
            }

            let output_capacity = usize::try_from(length).expect("u32 fits usize on wasm32");
            let mut output = Vec::with_capacity(output_capacity);
            for (relative, value) in values.into_iter().enumerate() {
                let sequence = first
                    .checked_add(relative as u32)
                    .ok_or_else(|| corrupt("content chunks", "chunk sequence overflow"))?;
                let bytes = stored_bytes(
                    "content chunk",
                    value,
                    self.inner.limits.max_chunk_bytes as usize,
                )?;
                validate_chunk_length(&record, sequence, bytes.len())?;
                let chunk_start = u64::from(sequence)
                    .checked_mul(chunk_bytes)
                    .ok_or_else(|| corrupt("content metadata", "chunk offset overflow"))?;
                let chunk_end = chunk_start + bytes.len() as u64;
                let copy_start = offset.max(chunk_start) - chunk_start;
                let copy_end = end.min(chunk_end) - chunk_start;
                let copy_start = usize::try_from(copy_start)
                    .map_err(|_| corrupt("content range", "slice start exceeds host range"))?;
                let copy_end = usize::try_from(copy_end)
                    .map_err(|_| corrupt("content range", "slice end exceeds host range"))?;
                output.extend_from_slice(&bytes[copy_start..copy_end]);
            }
            if output.len() != output_capacity {
                return Err(corrupt(
                    "content range",
                    "decoded range length differs from the request",
                ));
            }
            Ok(output)
        }
        .await;
        finish_transaction("read content range", operation, completion).await
    }

    async fn validate_schema(&self) -> BrowserContentStoreResult<()> {
        let actual_version = self
            .inner
            .database
            .version()
            .map_err(|error| idb_error("inspect content schema version", error))?;
        if actual_version != CONTENT_STORE_SCHEMA_VERSION {
            return Err(BrowserContentStoreError::VersionMismatch {
                expected: CONTENT_STORE_SCHEMA_VERSION,
                actual: Some(actual_version),
            });
        }
        if self.inner.database.name() != self.inner.database_name {
            return Err(BrowserContentStoreError::SchemaMismatch {
                reason: "opened database name differs from the canonical package name".to_owned(),
            });
        }

        let actual_stores = self
            .inner
            .database
            .store_names()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_stores = OBJECT_STORES
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if actual_stores != expected_stores {
            return Err(BrowserContentStoreError::SchemaMismatch {
                reason: format!(
                    "object stores are {actual_stores:?}, expected {expected_stores:?}"
                ),
            });
        }

        let transaction = self
            .inner
            .database
            .transaction(&OBJECT_STORES, TransactionMode::ReadOnly)
            .map_err(|error| idb_error("inspect content object stores", error))?;
        let mut stores = Vec::with_capacity(OBJECT_STORES.len());
        for name in OBJECT_STORES {
            stores.push(
                transaction
                    .object_store(name)
                    .map_err(|error| idb_error("inspect content object stores", error))?,
            );
        }
        let completion = transaction.into_future();
        let operation = (|| {
            for store in &stores {
                if store
                    .key_path()
                    .map_err(|error| idb_error("inspect content object-store key path", error))?
                    != None
                    || store.auto_increment()
                    || !store.index_names().is_empty()
                {
                    return Err(BrowserContentStoreError::SchemaMismatch {
                        reason: format!(
                            "object store `{}` must use out-of-line keys, no generator, and no indexes",
                            store.name()
                        ),
                    });
                }
            }
            Ok(())
        })();
        finish_transaction("inspect content object stores", operation, completion).await
    }

    async fn recover_storage_at(&self, now_ms: u64) -> BrowserContentStoreResult<u32> {
        if now_ms > MAX_SAFE_INTEGER {
            return Err(BrowserContentStoreError::InvalidInput {
                field: "content recovery time",
                reason: "time exceeds the exact JavaScript integer range".to_owned(),
            });
        }
        let transaction = self
            .inner
            .database
            .transaction(&OBJECT_STORES, TransactionMode::ReadWrite)
            .map_err(|error| idb_error("clean incomplete content staging", error))?;
        let metadata = transaction
            .object_store(METADATA_STORE)
            .map_err(|error| idb_error("clean incomplete content staging", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("clean incomplete content staging", error))?;
        let staging = transaction
            .object_store(STAGING_STORE)
            .map_err(|error| idb_error("clean incomplete content staging", error))?;
        let usage_store = transaction
            .object_store(USAGE_STORE)
            .map_err(|error| idb_error("clean incomplete content staging", error))?;
        let completion = transaction.into_future();

        let operation = async {
            let metadata_limit = ABSOLUTE_MAX_CONTENT_ENTRIES + 1;
            let metadata_keys = metadata
                .get_all_keys(None, Some(metadata_limit))
                .map_err(|error| idb_error("scan content metadata keys", error))?
                .await
                .map_err(|error| idb_error("scan content metadata keys", error))?;
            let metadata_values = metadata
                .get_all(None, Some(metadata_limit))
                .map_err(|error| idb_error("scan content metadata", error))?
                .await
                .map_err(|error| idb_error("scan content metadata", error))?;
            if metadata_keys.len() != metadata_values.len() {
                return Err(corrupt(
                    "content metadata",
                    "key and value scans have different lengths",
                ));
            }
            if metadata_values.len() > ABSOLUTE_MAX_CONTENT_ENTRIES as usize {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "recoverable content metadata entries",
                    limit: u64::from(ABSOLUTE_MAX_CONTENT_ENTRIES),
                });
            }

            let mut usage = UsageRecord::default();
            let mut published_storage_ids = BTreeSet::new();
            for (key, value) in metadata_keys.into_iter().zip(metadata_values) {
                let key = stored_string_key("content metadata key", key)?;
                let bytes = stored_bytes(
                    "content metadata",
                    value,
                    METADATA_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
                )?;
                let record = MetadataRecord::decode(&bytes)?;
                if key != metadata_key(&record.digest) {
                    return Err(corrupt(
                        "content metadata",
                        "record key differs from its digest",
                    ));
                }
                record.validate(self.inner.limits)?;
                if !published_storage_ids.insert(record.storage_id) {
                    return Err(corrupt(
                        "content metadata",
                        "multiple entries reference one storage identity",
                    ));
                }
                usage.add_published(&record)?;
            }
            usage.validate(self.inner.limits)?;

            let staging_limit = ABSOLUTE_MAX_STAGING_IMPORTS + 1;
            let staging_keys = staging
                .get_all_keys(None, Some(staging_limit))
                .map_err(|error| idb_error("scan content staging keys", error))?
                .await
                .map_err(|error| idb_error("scan content staging keys", error))?;
            let staging_values = staging
                .get_all(None, Some(staging_limit))
                .map_err(|error| idb_error("scan content staging", error))?
                .await
                .map_err(|error| idb_error("scan content staging", error))?;
            if staging_keys.len() != staging_values.len() {
                return Err(corrupt(
                    "content staging",
                    "key and value scans have different lengths",
                ));
            }
            if staging_values.len() > ABSOLUTE_MAX_STAGING_IMPORTS as usize {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "recoverable incomplete staging imports",
                    limit: u64::from(ABSOLUTE_MAX_STAGING_IMPORTS),
                });
            }

            let mut staging_storage_ids = BTreeSet::new();
            let mut expired = Vec::new();
            for (key, value) in staging_keys.into_iter().zip(staging_values) {
                let key = stored_string_key("content staging key", key)?;
                let bytes = stored_bytes(
                    "content staging",
                    value,
                    STAGING_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
                )?;
                let record = StagingRecord::decode(&bytes)?;
                if key != staging_key(record.staging_id) {
                    return Err(corrupt(
                        "content staging",
                        "record key differs from its staging identity",
                    ));
                }
                record.validate_recovery()?;
                if published_storage_ids.contains(&record.storage_id)
                    || !staging_storage_ids.insert(record.storage_id)
                {
                    return Err(corrupt(
                        "content staging",
                        "staging storage identity is not unique",
                    ));
                }
                let stored_chunks = chunks
                    .count(Some(chunk_namespace_query(record.storage_id)?))
                    .map_err(|error| idb_error("count staged content chunks", error))?
                    .await
                    .map_err(|error| idb_error("count staged content chunks", error))?;
                if stored_chunks != record.next_sequence {
                    return Err(corrupt(
                        "content staging",
                        format!(
                            "stored chunk count {stored_chunks} differs from staged sequence {}",
                            record.next_sequence
                        ),
                    ));
                }
                if record.lease_expires_ms <= now_ms {
                    expired.push(record);
                } else {
                    usage.add_staging(&record)?;
                }
            }
            usage.validate(self.inner.limits)?;

            let usage_keys = usage_store
                .get_all_keys(None, Some(2))
                .map_err(|error| idb_error("inspect content usage keys", error))?
                .await
                .map_err(|error| idb_error("inspect content usage keys", error))?;
            if usage_keys.len() > 1
                || usage_keys
                    .first()
                    .map(|key| stored_string_key("content usage key", key.clone()))
                    .transpose()?
                    .as_deref()
                    .is_some_and(|key| key != USAGE_KEY)
            {
                return Err(corrupt(
                    "content usage",
                    "usage store contains a non-canonical key set",
                ));
            }

            for record in &expired {
                chunks
                    .delete(chunk_namespace_query(record.storage_id)?)
                    .map_err(|error| idb_error("delete incomplete content chunks", error))?
                    .await
                    .map_err(|error| idb_error("delete incomplete content chunks", error))?;
                staging
                    .delete(JsValue::from_str(&staging_key(record.staging_id)))
                    .map_err(|error| idb_error("delete expired content staging", error))?
                    .await
                    .map_err(|error| idb_error("delete expired content staging", error))?;
            }

            let actual_chunks = chunks
                .count(None)
                .map_err(|error| idb_error("count published content chunks", error))?
                .await
                .map_err(|error| idb_error("count published content chunks", error))?;
            let expected_chunks = usage
                .published_chunks
                .checked_add(usage.staging_chunks)
                .ok_or_else(|| corrupt("content usage", "total chunk count overflow"))?;
            if actual_chunks != expected_chunks {
                return Err(corrupt(
                    "content chunks",
                    format!(
                        "stored chunk count {actual_chunks} differs from live metadata/staging count {expected_chunks}"
                    ),
                ));
            }
            put_usage(&usage_store, &usage, "rebuild content usage").await?;
            Ok(expired.len() as u32)
        }
        .await;
        finish_transaction("clean incomplete content staging", operation, completion).await
    }

    async fn reserve_import(
        &self,
        record: &StagingRecord,
    ) -> BrowserContentStoreResult<ReserveImportOutcome> {
        let transaction = self
            .inner
            .database
            .transaction(
                &[STAGING_STORE, CHUNK_STORE, USAGE_STORE],
                TransactionMode::ReadWrite,
            )
            .map_err(|error| idb_error("begin content import", error))?;
        let staging = transaction
            .object_store(STAGING_STORE)
            .map_err(|error| idb_error("begin content import", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("begin content import", error))?;
        let usage_store = transaction
            .object_store(USAGE_STORE)
            .map_err(|error| idb_error("begin content import", error))?;
        let completion = transaction.into_future();

        let operation = async {
            let staging_identity = JsValue::from_str(&staging_key(record.staging_id));
            let existing = staging
                .get_key(staging_identity.clone())
                .map_err(|error| idb_error("check content staging identity", error))?
                .await
                .map_err(|error| idb_error("check content staging identity", error))?
                .is_some();
            let storage_chunks = chunks
                .count(Some(chunk_namespace_query(record.storage_id)?))
                .map_err(|error| idb_error("check content storage identity", error))?
                .await
                .map_err(|error| idb_error("check content storage identity", error))?;
            if existing || storage_chunks != 0 {
                return Ok(ReserveImportOutcome::IdentityCollision);
            }

            let mut usage = load_usage(&usage_store, self.inner.limits).await?;
            if usage.staging_imports >= self.inner.limits.max_staging_imports {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "active content staging imports",
                    limit: u64::from(self.inner.limits.max_staging_imports),
                });
            }
            let staging_bytes = usage
                .staging_reserved_bytes
                .checked_add(record.declared_size)
                .ok_or_else(|| corrupt("content usage", "staging byte count overflow"))?;
            if staging_bytes > self.inner.limits.max_staging_bytes {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "reserved content staging bytes",
                    limit: self.inner.limits.max_staging_bytes,
                });
            }
            usage.staging_imports += 1;
            usage.staging_reserved_bytes = staging_bytes;

            let encoded = record.encode();
            let value: JsValue = Uint8Array::from(encoded.as_slice()).into();
            staging
                .add(&value, Some(&staging_identity))
                .map_err(|error| idb_error("insert content staging record", error))?
                .await
                .map_err(|error| idb_error("insert content staging record", error))?;
            put_usage(&usage_store, &usage, "reserve content staging usage").await?;
            Ok(ReserveImportOutcome::Reserved)
        }
        .await;
        finish_transaction("begin content import", operation, completion).await
    }

    async fn append_import_chunk(
        &self,
        current: &StagingRecord,
        next: &StagingRecord,
        bytes: &[u8],
    ) -> BrowserContentStoreResult<()> {
        let transaction = self
            .inner
            .database
            .transaction(
                &[STAGING_STORE, CHUNK_STORE, USAGE_STORE],
                TransactionMode::ReadWrite,
            )
            .map_err(|error| idb_error("append content import chunk", error))?;
        let staging = transaction
            .object_store(STAGING_STORE)
            .map_err(|error| idb_error("append content import chunk", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("append content import chunk", error))?;
        let usage_store = transaction
            .object_store(USAGE_STORE)
            .map_err(|error| idb_error("append content import chunk", error))?;
        let completion = transaction.into_future();

        let operation = async {
            let persisted = load_staging(&staging, current.staging_id).await?;
            if &persisted != current {
                return Err(corrupt(
                    "content staging",
                    "persisted import progress differs from its active owner",
                ));
            }
            let mut usage = load_usage(&usage_store, self.inner.limits).await?;
            if usage.staging_chunks >= self.inner.limits.max_staging_chunks {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "staging content chunks",
                    limit: u64::from(self.inner.limits.max_staging_chunks),
                });
            }
            if current.next_sequence >= self.inner.limits.max_chunks_per_content {
                return Err(BrowserContentStoreError::LimitExceeded {
                    resource: "chunks in one content import",
                    limit: u64::from(self.inner.limits.max_chunks_per_content),
                });
            }

            let chunk_key =
                JsValue::from_str(&chunk_key(current.storage_id, current.next_sequence));
            if chunks
                .get_key(chunk_key.clone())
                .map_err(|error| idb_error("check content chunk key", error))?
                .await
                .map_err(|error| idb_error("check content chunk key", error))?
                .is_some()
            {
                return Err(corrupt(
                    "content chunks",
                    "next canonical chunk key already exists",
                ));
            }

            usage.staging_chunks += 1;
            let chunk_value: JsValue = Uint8Array::from(bytes).into();
            chunks
                .add(&chunk_value, Some(&chunk_key))
                .map_err(|error| idb_error("insert content chunk", error))?
                .await
                .map_err(|error| idb_error("insert content chunk", error))?;

            let encoded = next.encode();
            let staging_value: JsValue = Uint8Array::from(encoded.as_slice()).into();
            staging
                .put(
                    &staging_value,
                    Some(&JsValue::from_str(&staging_key(current.staging_id))),
                )
                .map_err(|error| idb_error("update content staging progress", error))?
                .await
                .map_err(|error| idb_error("update content staging progress", error))?;
            put_usage(&usage_store, &usage, "update content staging usage").await?;
            Ok(())
        }
        .await;
        finish_transaction("append content import chunk", operation, completion).await
    }

    async fn finish_import(
        &self,
        staging_record: &StagingRecord,
        content: &ContentRef,
    ) -> BrowserContentStoreResult<()> {
        let transaction = self
            .inner
            .database
            .transaction(&OBJECT_STORES, TransactionMode::ReadWrite)
            .map_err(|error| idb_error("finish content import", error))?;
        let metadata = transaction
            .object_store(METADATA_STORE)
            .map_err(|error| idb_error("finish content import", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("finish content import", error))?;
        let staging = transaction
            .object_store(STAGING_STORE)
            .map_err(|error| idb_error("finish content import", error))?;
        let usage_store = transaction
            .object_store(USAGE_STORE)
            .map_err(|error| idb_error("finish content import", error))?;
        let completion = transaction.into_future();

        let operation = async {
            let persisted = load_staging(&staging, staging_record.staging_id).await?;
            if &persisted != staging_record {
                return Err(corrupt(
                    "content staging",
                    "persisted import differs at publication",
                ));
            }
            if persisted.written_size != persisted.declared_size {
                return Err(BrowserContentStoreError::SizeMismatch {
                    declared: persisted.declared_size,
                    actual: persisted.written_size,
                });
            }

            let mut usage = load_usage(&usage_store, self.inner.limits).await?;
            let metadata_identity = JsValue::from_str(&metadata_key(&content.digest()));
            let existing = metadata
                .get(metadata_identity.clone())
                .map_err(|error| idb_error("check deduplicated content", error))?
                .await
                .map_err(|error| idb_error("check deduplicated content", error))?;

            usage.remove_staging(&persisted)?;
            match existing {
                Some(value) => {
                    let bytes = stored_bytes(
                        "content metadata",
                        value,
                        METADATA_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
                    )?;
                    let existing = MetadataRecord::decode(&bytes)?;
                    existing.validate(self.inner.limits)?;
                    if existing.digest != content.digest() || existing.size != content.size() {
                        return Err(corrupt(
                            "content metadata",
                            "equal digest key has conflicting content identity",
                        ));
                    }
                    chunks
                        .delete(chunk_namespace_query(persisted.storage_id)?)
                        .map_err(|error| idb_error("discard deduplicated chunks", error))?
                        .await
                        .map_err(|error| idb_error("discard deduplicated chunks", error))?;
                }
                None => {
                    let record = MetadataRecord {
                        digest: content.digest(),
                        storage_id: persisted.storage_id,
                        size: content.size(),
                        chunk_count: persisted.next_sequence,
                        chunk_bytes: persisted.chunk_bytes,
                        media: content.media().to_owned(),
                    };
                    record.validate(self.inner.limits)?;
                    usage.add_published(&record)?;
                    usage.validate(self.inner.limits)?;
                    let encoded = record.encode();
                    let value: JsValue = Uint8Array::from(encoded.as_slice()).into();
                    metadata
                        .add(&value, Some(&metadata_identity))
                        .map_err(|error| idb_error("publish content metadata", error))?
                        .await
                        .map_err(|error| idb_error("publish content metadata", error))?;
                }
            }

            staging
                .delete(JsValue::from_str(&staging_key(persisted.staging_id)))
                .map_err(|error| idb_error("remove published content staging", error))?
                .await
                .map_err(|error| idb_error("remove published content staging", error))?;
            put_usage(&usage_store, &usage, "publish content usage").await?;
            Ok(())
        }
        .await;
        finish_transaction("finish content import", operation, completion).await
    }

    async fn cleanup_import(
        &self,
        staging_id: OpaqueId,
        storage_id: OpaqueId,
    ) -> BrowserContentStoreResult<bool> {
        let transaction = self
            .inner
            .database
            .transaction(
                &[STAGING_STORE, CHUNK_STORE, USAGE_STORE],
                TransactionMode::ReadWrite,
            )
            .map_err(|error| idb_error("abort content import", error))?;
        let staging = transaction
            .object_store(STAGING_STORE)
            .map_err(|error| idb_error("abort content import", error))?;
        let chunks = transaction
            .object_store(CHUNK_STORE)
            .map_err(|error| idb_error("abort content import", error))?;
        let usage_store = transaction
            .object_store(USAGE_STORE)
            .map_err(|error| idb_error("abort content import", error))?;
        let completion = transaction.into_future();

        let operation = async {
            let key = JsValue::from_str(&staging_key(staging_id));
            let Some(value) = staging
                .get(key.clone())
                .map_err(|error| idb_error("resolve aborted content staging", error))?
                .await
                .map_err(|error| idb_error("resolve aborted content staging", error))?
            else {
                return Ok(false);
            };
            let bytes = stored_bytes(
                "content staging",
                value,
                STAGING_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
            )?;
            let record = StagingRecord::decode(&bytes)?;
            if record.staging_id != staging_id || record.storage_id != storage_id {
                return Err(corrupt(
                    "content staging",
                    "abort identity differs from persisted staging ownership",
                ));
            }
            let mut usage = load_usage(&usage_store, self.inner.limits).await?;
            usage.remove_staging(&record)?;

            chunks
                .delete(chunk_namespace_query(storage_id)?)
                .map_err(|error| idb_error("delete aborted content chunks", error))?
                .await
                .map_err(|error| idb_error("delete aborted content chunks", error))?;
            staging
                .delete(key)
                .map_err(|error| idb_error("delete aborted content staging", error))?
                .await
                .map_err(|error| idb_error("delete aborted content staging", error))?;
            put_usage(&usage_store, &usage, "release aborted content usage").await?;
            Ok(true)
        }
        .await;
        finish_transaction("abort content import", operation, completion).await
    }
}

pub(crate) struct BrowserContentImport {
    store: BrowserIndexedDbContentStore,
    state: Option<ActiveImport>,
}

impl fmt::Debug for BrowserContentImport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("BrowserContentImport");
        if let Some(state) = &self.state {
            debug
                .field("declared_size", &state.record.declared_size)
                .field("written_size", &state.record.written_size)
                .field("next_sequence", &state.record.next_sequence)
                .field("media", &state.record.media);
        } else {
            debug.field("complete", &true);
        }
        debug.finish()
    }
}

impl BrowserContentImport {
    pub(crate) fn declared_size(&self) -> u64 {
        self.active()
            .map(|state| state.record.declared_size)
            .unwrap_or(0)
    }

    pub(crate) fn written_size(&self) -> u64 {
        self.active()
            .map(|state| state.record.written_size)
            .unwrap_or(0)
    }

    pub(crate) fn next_sequence(&self) -> u32 {
        self.active()
            .map(|state| state.record.next_sequence)
            .unwrap_or(0)
    }

    /// Appends one exact next chunk in its own read-write transaction.
    pub(crate) async fn append_chunk(
        &mut self,
        sequence: u32,
        bytes: &[u8],
    ) -> BrowserContentStoreResult<BrowserContentImportProgress> {
        self.append_chunk_inner(sequence, bytes, None).await
    }

    /// Additionally verifies the SHA-256 of the complete prefix through this
    /// chunk before writing anything to IndexedDB.
    #[cfg(test)]
    pub(crate) async fn append_verified_chunk(
        &mut self,
        sequence: u32,
        bytes: &[u8],
        expected_prefix_sha256: [u8; CONTENT_DIGEST_BYTES],
    ) -> BrowserContentStoreResult<BrowserContentImportProgress> {
        self.append_chunk_inner(sequence, bytes, Some(expected_prefix_sha256))
            .await
    }

    /// Publishes content atomically after exact size and independently supplied
    /// final SHA-256 checks. Existing digest metadata wins and staged chunks are
    /// discarded in the same transaction.
    pub(crate) async fn finish(
        mut self,
        expected_final_sha256: [u8; CONTENT_DIGEST_BYTES],
    ) -> BrowserContentStoreResult<ContentRef> {
        let state = self.active()?.clone();
        if state.record.written_size != state.record.declared_size {
            return Err(BrowserContentStoreError::SizeMismatch {
                declared: state.record.declared_size,
                actual: state.record.written_size,
            });
        }
        let actual_digest = digest_snapshot(&state.digest);
        if actual_digest != expected_final_sha256 {
            return Err(BrowserContentStoreError::DigestMismatch {
                phase: "final content publication",
            });
        }
        let content = ContentRef::new(
            actual_digest,
            state.record.declared_size,
            state.record.media.clone(),
        )
        .map_err(|error| BrowserContentStoreError::InvalidInput {
            field: "content reference",
            reason: error.to_string(),
        })?;
        self.store.finish_import(&state.record, &content).await?;
        self.state = None;
        Ok(content)
    }

    /// Explicit cancellation reports cleanup failures. Dropping an unfinished
    /// import schedules the same cleanup as a best-effort local task.
    #[cfg(test)]
    pub(crate) async fn abort(mut self) -> BrowserContentStoreResult<bool> {
        let state = self.active()?.clone();
        let removed = self
            .store
            .cleanup_import(state.record.staging_id, state.record.storage_id)
            .await?;
        self.state = None;
        Ok(removed)
    }

    async fn append_chunk_inner(
        &mut self,
        sequence: u32,
        bytes: &[u8],
        expected_prefix_sha256: Option<[u8; CONTENT_DIGEST_BYTES]>,
    ) -> BrowserContentStoreResult<BrowserContentImportProgress> {
        let state = self.active()?.clone();
        if sequence != state.record.next_sequence {
            return Err(BrowserContentStoreError::SequenceMismatch {
                expected: state.record.next_sequence,
                actual: sequence,
            });
        }
        if bytes.is_empty() {
            return Err(BrowserContentStoreError::InvalidInput {
                field: "content chunk",
                reason: "chunks must be non-empty; zero-byte content has no chunks".to_owned(),
            });
        }
        if bytes.len() > self.store.inner.limits.max_chunk_bytes as usize {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "content chunk bytes",
                limit: u64::from(self.store.inner.limits.max_chunk_bytes),
            });
        }
        if sequence >= self.store.inner.limits.max_chunks_per_content {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "chunks in one content import",
                limit: u64::from(self.store.inner.limits.max_chunks_per_content),
            });
        }

        let byte_count = bytes.len() as u64;
        let next_written = state
            .record
            .written_size
            .checked_add(byte_count)
            .ok_or_else(|| BrowserContentStoreError::InvalidInput {
                field: "content chunk",
                reason: "written byte count overflow".to_owned(),
            })?;
        if next_written > state.record.declared_size {
            return Err(BrowserContentStoreError::SizeMismatch {
                declared: state.record.declared_size,
                actual: next_written,
            });
        }

        let chunk_bytes = if sequence == 0 {
            bytes.len() as u32
        } else {
            state.record.chunk_bytes
        };
        let remaining = state.record.declared_size - state.record.written_size;
        let expected_len = remaining.min(u64::from(chunk_bytes));
        if byte_count != expected_len {
            return Err(BrowserContentStoreError::InvalidInput {
                field: "content chunk length",
                reason: format!(
                    "canonical chunk {sequence} contains {byte_count} bytes, expected {expected_len}"
                ),
            });
        }

        let mut next_digest = state.digest.clone();
        next_digest.update(bytes);
        let prefix_digest = digest_snapshot(&next_digest);
        if expected_prefix_sha256.is_some_and(|expected| expected != prefix_digest) {
            return Err(BrowserContentStoreError::DigestMismatch {
                phase: "incremental content import",
            });
        }

        let mut next_record = state.record.clone();
        next_record.written_size = next_written;
        next_record.lease_expires_ms = staging_lease_expiry(self.store.inner.limits)?;
        next_record.next_sequence = next_record.next_sequence.checked_add(1).ok_or_else(|| {
            BrowserContentStoreError::LimitExceeded {
                resource: "content chunk sequence",
                limit: u64::from(u32::MAX),
            }
        })?;
        next_record.chunk_bytes = chunk_bytes;
        self.store
            .append_import_chunk(&state.record, &next_record, bytes)
            .await?;

        let active = self.active_mut()?;
        if active.record != state.record {
            return Err(corrupt(
                "active content import",
                "local progress changed during an awaited append",
            ));
        }
        active.record = next_record.clone();
        active.digest = next_digest;
        Ok(BrowserContentImportProgress {
            written_size: next_record.written_size,
            next_sequence: next_record.next_sequence,
            prefix_sha256: prefix_digest,
        })
    }

    fn active(&self) -> BrowserContentStoreResult<&ActiveImport> {
        self.state
            .as_ref()
            .ok_or(BrowserContentStoreError::InvalidInput {
                field: "content import",
                reason: "import is already finished or aborted".to_owned(),
            })
    }

    fn active_mut(&mut self) -> BrowserContentStoreResult<&mut ActiveImport> {
        self.state
            .as_mut()
            .ok_or(BrowserContentStoreError::InvalidInput {
                field: "content import",
                reason: "import is already finished or aborted".to_owned(),
            })
    }
}

impl Drop for BrowserContentImport {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let store = self.store.clone();
        spawn_local(async move {
            let _ = store
                .cleanup_import(state.record.staging_id, state.record.storage_id)
                .await;
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserContentImportProgress {
    pub written_size: u64,
    pub next_sequence: u32,
    pub prefix_sha256: [u8; CONTENT_DIGEST_BYTES],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserContentMetadata {
    content: ContentRef,
    chunk_count: u32,
    chunk_bytes: u32,
}

impl BrowserContentMetadata {
    fn from_record(content: ContentRef, record: &MetadataRecord) -> Self {
        Self {
            content,
            chunk_count: record.chunk_count,
            chunk_bytes: record.chunk_bytes,
        }
    }

    pub(crate) fn content(&self) -> &ContentRef {
        &self.content
    }

    pub(crate) const fn chunk_count(&self) -> u32 {
        self.chunk_count
    }

    pub(crate) const fn chunk_bytes(&self) -> u32 {
        self.chunk_bytes
    }
}

#[derive(Clone)]
struct ActiveImport {
    record: StagingRecord,
    digest: Sha256,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OpaqueId([u8; OPAQUE_ID_BYTES]);

#[derive(Clone, Debug, Eq, PartialEq)]
struct MetadataRecord {
    digest: [u8; CONTENT_DIGEST_BYTES],
    storage_id: OpaqueId,
    size: u64,
    chunk_count: u32,
    chunk_bytes: u32,
    media: String,
}

impl MetadataRecord {
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(METADATA_FIXED_BYTES + self.media.len());
        encoded.push(RECORD_FORMAT_VERSION);
        encoded.extend_from_slice(&self.digest);
        encoded.extend_from_slice(&self.storage_id.0);
        encoded.extend_from_slice(&self.size.to_be_bytes());
        encoded.extend_from_slice(&self.chunk_count.to_be_bytes());
        encoded.extend_from_slice(&self.chunk_bytes.to_be_bytes());
        encode_media(&mut encoded, &self.media);
        encoded
    }

    fn decode(encoded: &[u8]) -> BrowserContentStoreResult<Self> {
        let mut decoder = RecordDecoder::new(encoded, "content metadata");
        decoder.format()?;
        let digest = decoder.array()?;
        let storage_id = OpaqueId(decoder.array()?);
        let size = decoder.u64()?;
        let chunk_count = decoder.u32()?;
        let chunk_bytes = decoder.u32()?;
        let media = decoder.media()?;
        decoder.finish()?;
        Ok(Self {
            digest,
            storage_id,
            size,
            chunk_count,
            chunk_bytes,
            media,
        })
    }

    fn validate(&self, limits: BrowserContentStoreLimits) -> BrowserContentStoreResult<()> {
        if self.size > MAX_SAFE_INTEGER {
            return Err(corrupt(
                "content metadata",
                "content size exceeds the exact JavaScript integer range",
            ));
        }
        validate_media(&self.media).map_err(|error| corrupt("content metadata", error))?;
        let expected = canonical_chunk_count(self.size, self.chunk_bytes)?;
        if expected != self.chunk_count {
            return Err(corrupt(
                "content metadata",
                "chunk count differs from size and canonical chunk width",
            ));
        }
        if self.chunk_count > limits.max_chunks_per_content {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "chunks in stored content",
                limit: u64::from(limits.max_chunks_per_content),
            });
        }
        if self.chunk_bytes > limits.max_chunk_bytes {
            return Err(BrowserContentStoreError::LimitExceeded {
                resource: "stored content chunk bytes",
                limit: u64::from(limits.max_chunk_bytes),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StagingRecord {
    staging_id: OpaqueId,
    storage_id: OpaqueId,
    declared_size: u64,
    written_size: u64,
    lease_expires_ms: u64,
    next_sequence: u32,
    chunk_bytes: u32,
    media: String,
}

impl StagingRecord {
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(STAGING_FIXED_BYTES + self.media.len());
        encoded.push(RECORD_FORMAT_VERSION);
        encoded.extend_from_slice(&self.staging_id.0);
        encoded.extend_from_slice(&self.storage_id.0);
        encoded.extend_from_slice(&self.declared_size.to_be_bytes());
        encoded.extend_from_slice(&self.written_size.to_be_bytes());
        encoded.extend_from_slice(&self.lease_expires_ms.to_be_bytes());
        encoded.extend_from_slice(&self.next_sequence.to_be_bytes());
        encoded.extend_from_slice(&self.chunk_bytes.to_be_bytes());
        encode_media(&mut encoded, &self.media);
        encoded
    }

    fn decode(encoded: &[u8]) -> BrowserContentStoreResult<Self> {
        let mut decoder = RecordDecoder::new(encoded, "content staging");
        decoder.format()?;
        let staging_id = OpaqueId(decoder.array()?);
        let storage_id = OpaqueId(decoder.array()?);
        let declared_size = decoder.u64()?;
        let written_size = decoder.u64()?;
        let lease_expires_ms = decoder.u64()?;
        let next_sequence = decoder.u32()?;
        let chunk_bytes = decoder.u32()?;
        let media = decoder.media()?;
        decoder.finish()?;
        Ok(Self {
            staging_id,
            storage_id,
            declared_size,
            written_size,
            lease_expires_ms,
            next_sequence,
            chunk_bytes,
            media,
        })
    }

    fn validate_recovery(&self) -> BrowserContentStoreResult<()> {
        if self.declared_size > MAX_SAFE_INTEGER
            || self.written_size > self.declared_size
            || self.lease_expires_ms > MAX_SAFE_INTEGER
        {
            return Err(corrupt(
                "content staging",
                "staged byte counts are outside their canonical bounds",
            ));
        }
        if self.next_sequence > ABSOLUTE_MAX_CHUNKS || self.chunk_bytes > ABSOLUTE_MAX_CHUNK_BYTES {
            return Err(corrupt(
                "content staging",
                "staged chunk fields exceed recovery bounds",
            ));
        }
        validate_media(&self.media).map_err(|error| corrupt("content staging", error))?;
        if self.next_sequence == 0 {
            if self.written_size != 0 || self.chunk_bytes != 0 {
                return Err(corrupt(
                    "content staging",
                    "empty staging progress has non-empty chunk fields",
                ));
            }
            return Ok(());
        }
        if self.chunk_bytes == 0 {
            return Err(corrupt(
                "content staging",
                "non-empty staging progress has zero chunk width",
            ));
        }
        let expected = u64::from(self.next_sequence)
            .checked_mul(u64::from(self.chunk_bytes))
            .ok_or_else(|| corrupt("content staging", "staged byte count overflow"))?
            .min(self.declared_size);
        if self.written_size != expected {
            return Err(corrupt(
                "content staging",
                "written bytes differ from canonical chunk progress",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct UsageRecord {
    published_entries: u32,
    published_bytes: u64,
    published_chunks: u32,
    staging_imports: u32,
    staging_reserved_bytes: u64,
    staging_chunks: u32,
}

impl UsageRecord {
    fn encode(self) -> [u8; USAGE_RECORD_BYTES] {
        let mut encoded = [0_u8; USAGE_RECORD_BYTES];
        let mut offset = 0;
        encoded[offset] = RECORD_FORMAT_VERSION;
        offset += 1;
        for bytes in [
            self.published_entries.to_be_bytes().as_slice(),
            self.published_bytes.to_be_bytes().as_slice(),
            self.published_chunks.to_be_bytes().as_slice(),
            self.staging_imports.to_be_bytes().as_slice(),
            self.staging_reserved_bytes.to_be_bytes().as_slice(),
            self.staging_chunks.to_be_bytes().as_slice(),
        ] {
            encoded[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }
        debug_assert_eq!(offset, USAGE_RECORD_BYTES);
        encoded
    }

    fn decode(encoded: &[u8]) -> BrowserContentStoreResult<Self> {
        let mut decoder = RecordDecoder::new(encoded, "content usage");
        decoder.format()?;
        let record = Self {
            published_entries: decoder.u32()?,
            published_bytes: decoder.u64()?,
            published_chunks: decoder.u32()?,
            staging_imports: decoder.u32()?,
            staging_reserved_bytes: decoder.u64()?,
            staging_chunks: decoder.u32()?,
        };
        decoder.finish()?;
        Ok(record)
    }

    fn validate(self, limits: BrowserContentStoreLimits) -> BrowserContentStoreResult<()> {
        for (resource, actual, limit) in [
            (
                "published content entries",
                u64::from(self.published_entries),
                u64::from(limits.max_content_entries),
            ),
            (
                "published content bytes",
                self.published_bytes,
                limits.max_content_bytes,
            ),
            (
                "published content chunks",
                u64::from(self.published_chunks),
                u64::from(limits.max_content_chunks),
            ),
            (
                "active content staging imports",
                u64::from(self.staging_imports),
                u64::from(limits.max_staging_imports),
            ),
            (
                "reserved content staging bytes",
                self.staging_reserved_bytes,
                limits.max_staging_bytes,
            ),
            (
                "staging content chunks",
                u64::from(self.staging_chunks),
                u64::from(limits.max_staging_chunks),
            ),
        ] {
            if actual > limit {
                return Err(BrowserContentStoreError::LimitExceeded { resource, limit });
            }
        }
        Ok(())
    }

    fn add_published(&mut self, record: &MetadataRecord) -> BrowserContentStoreResult<()> {
        self.published_entries = self
            .published_entries
            .checked_add(1)
            .ok_or_else(|| corrupt("content usage", "published entry count overflow"))?;
        self.published_bytes = self
            .published_bytes
            .checked_add(record.size)
            .ok_or_else(|| corrupt("content usage", "published byte count overflow"))?;
        self.published_chunks = self
            .published_chunks
            .checked_add(record.chunk_count)
            .ok_or_else(|| corrupt("content usage", "published chunk count overflow"))?;
        Ok(())
    }

    fn add_staging(&mut self, record: &StagingRecord) -> BrowserContentStoreResult<()> {
        self.staging_imports = self
            .staging_imports
            .checked_add(1)
            .ok_or_else(|| corrupt("content usage", "staging import count overflow"))?;
        self.staging_reserved_bytes = self
            .staging_reserved_bytes
            .checked_add(record.declared_size)
            .ok_or_else(|| corrupt("content usage", "staging byte count overflow"))?;
        self.staging_chunks = self
            .staging_chunks
            .checked_add(record.next_sequence)
            .ok_or_else(|| corrupt("content usage", "staging chunk count overflow"))?;
        Ok(())
    }

    fn remove_staging(&mut self, record: &StagingRecord) -> BrowserContentStoreResult<()> {
        self.staging_imports = self
            .staging_imports
            .checked_sub(1)
            .ok_or_else(|| corrupt("content usage", "staging import count underflow"))?;
        self.staging_reserved_bytes = self
            .staging_reserved_bytes
            .checked_sub(record.declared_size)
            .ok_or_else(|| corrupt("content usage", "staging byte count underflow"))?;
        self.staging_chunks = self
            .staging_chunks
            .checked_sub(record.next_sequence)
            .ok_or_else(|| corrupt("content usage", "staging chunk count underflow"))?;
        Ok(())
    }
}

struct RecordDecoder<'a> {
    encoded: &'a [u8],
    offset: usize,
    resource: &'static str,
}

impl<'a> RecordDecoder<'a> {
    const fn new(encoded: &'a [u8], resource: &'static str) -> Self {
        Self {
            encoded,
            offset: 0,
            resource,
        }
    }

    fn format(&mut self) -> BrowserContentStoreResult<()> {
        if self.u8()? != RECORD_FORMAT_VERSION {
            return Err(corrupt(self.resource, "record format version differs"));
        }
        Ok(())
    }

    fn u8(&mut self) -> BrowserContentStoreResult<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> BrowserContentStoreResult<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> BrowserContentStoreResult<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> BrowserContentStoreResult<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> BrowserContentStoreResult<[u8; N]> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or_else(|| corrupt(self.resource, "record offset overflow"))?;
        let bytes = self
            .encoded
            .get(self.offset..end)
            .ok_or_else(|| corrupt(self.resource, "record is truncated"))?;
        self.offset = end;
        Ok(bytes.try_into().expect("slice length was checked"))
    }

    fn media(&mut self) -> BrowserContentStoreResult<String> {
        let length = self.u16()? as usize;
        if length == 0 || length > MAX_CONTENT_MEDIA_BYTES {
            return Err(corrupt(
                self.resource,
                "media length is outside its canonical bound",
            ));
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| corrupt(self.resource, "media offset overflow"))?;
        let bytes = self
            .encoded
            .get(self.offset..end)
            .ok_or_else(|| corrupt(self.resource, "media bytes are truncated"))?;
        self.offset = end;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| corrupt(self.resource, "media is not UTF-8"))
    }

    fn finish(self) -> BrowserContentStoreResult<()> {
        if self.offset != self.encoded.len() {
            return Err(corrupt(self.resource, "record has trailing bytes"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReserveImportOutcome {
    Reserved,
    IdentityCollision,
}

async fn load_metadata(
    metadata: &ObjectStore,
    content: &ContentRef,
    limits: BrowserContentStoreLimits,
) -> BrowserContentStoreResult<MetadataRecord> {
    let value = metadata
        .get(JsValue::from_str(&metadata_key(&content.digest())))
        .map_err(|error| idb_error("get content metadata", error))?
        .await
        .map_err(|error| idb_error("get content metadata", error))?
        .ok_or(BrowserContentStoreError::Missing {
            resource: "content metadata",
        })?;
    let bytes = stored_bytes(
        "content metadata",
        value,
        METADATA_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
    )?;
    let record = MetadataRecord::decode(&bytes)?;
    record.validate(limits)?;
    if record.digest != content.digest() || record.size != content.size() {
        return Err(corrupt(
            "content metadata",
            "stored identity differs from the requested ContentRef",
        ));
    }
    Ok(record)
}

async fn load_staging(
    staging: &ObjectStore,
    staging_id: OpaqueId,
) -> BrowserContentStoreResult<StagingRecord> {
    let value = staging
        .get(JsValue::from_str(&staging_key(staging_id)))
        .map_err(|error| idb_error("get content staging", error))?
        .await
        .map_err(|error| idb_error("get content staging", error))?
        .ok_or(BrowserContentStoreError::Missing {
            resource: "content staging",
        })?;
    let bytes = stored_bytes(
        "content staging",
        value,
        STAGING_FIXED_BYTES + MAX_CONTENT_MEDIA_BYTES,
    )?;
    let record = StagingRecord::decode(&bytes)?;
    record.validate_recovery()?;
    if record.staging_id != staging_id {
        return Err(corrupt(
            "content staging",
            "stored identity differs from its key",
        ));
    }
    Ok(record)
}

async fn load_usage(
    usage: &ObjectStore,
    limits: BrowserContentStoreLimits,
) -> BrowserContentStoreResult<UsageRecord> {
    let value = usage
        .get(JsValue::from_str(USAGE_KEY))
        .map_err(|error| idb_error("get content usage", error))?
        .await
        .map_err(|error| idb_error("get content usage", error))?
        .ok_or(BrowserContentStoreError::Missing {
            resource: "content usage record",
        })?;
    let bytes = stored_bytes("content usage", value, USAGE_RECORD_BYTES)?;
    let record = UsageRecord::decode(&bytes)?;
    record.validate(limits)?;
    Ok(record)
}

async fn put_usage(
    usage: &ObjectStore,
    record: &UsageRecord,
    operation: &str,
) -> BrowserContentStoreResult<()> {
    let encoded = record.encode();
    let value: JsValue = Uint8Array::from(encoded.as_slice()).into();
    usage
        .put(&value, Some(&JsValue::from_str(USAGE_KEY)))
        .map_err(|error| idb_error(operation, error))?
        .await
        .map_err(|error| idb_error(operation, error))?;
    Ok(())
}

async fn finish_transaction<T>(
    operation: &str,
    request_result: BrowserContentStoreResult<T>,
    completion: TransactionFuture,
) -> BrowserContentStoreResult<T> {
    match (request_result, completion.await) {
        (Err(request_error), Err(transaction_error)) => {
            let transaction_error = idb_error(operation, transaction_error);
            if transaction_error.is_quota_exceeded() || transaction_error.is_version_mismatch() {
                Err(transaction_error)
            } else {
                Err(request_error)
            }
        }
        (Err(request_error), _) => Err(request_error),
        (Ok(_), Err(transaction_error)) => Err(idb_error(operation, transaction_error)),
        (Ok(value), Ok(TransactionResult::Committed)) => Ok(value),
        (Ok(_), Ok(TransactionResult::Aborted)) => Err(BrowserContentStoreError::Aborted {
            operation: operation.to_owned(),
        }),
    }
}

fn canonical_database_name(package_id: &str) -> BrowserContentStoreResult<String> {
    validate_package_id(package_id)?;
    Ok(format!("{DATABASE_NAME_PREFIX}{package_id}"))
}

fn validate_package_id(package_id: &str) -> BrowserContentStoreResult<()> {
    if package_id.is_empty()
        || package_id.len() > MAX_PACKAGE_ID_BYTES
        || package_id.trim() != package_id
        || !package_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        return Err(BrowserContentStoreError::InvalidInput {
            field: "package ID",
            reason: "must be a bounded canonical browser package identifier".to_owned(),
        });
    }
    Ok(())
}

fn validate_media(media: &str) -> BrowserContentStoreResult<()> {
    ContentRef::new([0; CONTENT_DIGEST_BYTES], 0, media).map_err(|error| {
        BrowserContentStoreError::InvalidInput {
            field: "content media",
            reason: error.to_string(),
        }
    })?;
    Ok(())
}

fn current_unix_ms() -> BrowserContentStoreResult<u64> {
    let now = js_sys::Date::now();
    if !now.is_finite() || now < 0.0 || now.fract() != 0.0 || now > MAX_SAFE_INTEGER as f64 {
        return Err(BrowserContentStoreError::Platform {
            operation: "read browser wall clock for content staging".to_owned(),
            name: None,
            message: "browser wall clock is outside the exact integer range".to_owned(),
        });
    }
    Ok(now as u64)
}

fn staging_lease_expiry(limits: BrowserContentStoreLimits) -> BrowserContentStoreResult<u64> {
    current_unix_ms()?
        .checked_add(limits.staging_lease_ms)
        .filter(|expiry| *expiry <= MAX_SAFE_INTEGER)
        .ok_or_else(|| BrowserContentStoreError::Platform {
            operation: "renew browser content staging lease".to_owned(),
            name: None,
            message: "content staging lease exceeds the exact integer range".to_owned(),
        })
}

fn canonical_chunk_count(size: u64, chunk_bytes: u32) -> BrowserContentStoreResult<u32> {
    if size == 0 {
        if chunk_bytes != 0 {
            return Err(corrupt(
                "content metadata",
                "zero-byte content has a non-zero chunk width",
            ));
        }
        return Ok(0);
    }
    if chunk_bytes == 0 {
        return Err(corrupt(
            "content metadata",
            "non-empty content has zero chunk width",
        ));
    }
    let count = ((size - 1) / u64::from(chunk_bytes)) + 1;
    u32::try_from(count)
        .map_err(|_| corrupt("content metadata", "chunk count exceeds its encoded width"))
}

fn expected_chunk_length(record: &MetadataRecord, sequence: u32) -> BrowserContentStoreResult<u32> {
    if sequence >= record.chunk_count || record.chunk_bytes == 0 {
        return Err(corrupt(
            "content metadata",
            "chunk sequence is outside canonical metadata",
        ));
    }
    let start = u64::from(sequence)
        .checked_mul(u64::from(record.chunk_bytes))
        .ok_or_else(|| corrupt("content metadata", "chunk offset overflow"))?;
    let remaining = record
        .size
        .checked_sub(start)
        .ok_or_else(|| corrupt("content metadata", "chunk starts after content end"))?;
    Ok(remaining.min(u64::from(record.chunk_bytes)) as u32)
}

fn validate_chunk_length(
    record: &MetadataRecord,
    sequence: u32,
    actual: usize,
) -> BrowserContentStoreResult<()> {
    let expected = expected_chunk_length(record, sequence)? as usize;
    if actual != expected {
        return Err(corrupt(
            "content chunk",
            format!("chunk {sequence} has {actual} bytes, expected {expected}"),
        ));
    }
    Ok(())
}

fn metadata_key(digest: &[u8; CONTENT_DIGEST_BYTES]) -> String {
    lowercase_hex(digest)
}

fn staging_key(id: OpaqueId) -> String {
    lowercase_hex(&id.0)
}

fn chunk_key(storage_id: OpaqueId, sequence: u32) -> String {
    format!("{}:{sequence:08x}", lowercase_hex(&storage_id.0))
}

fn chunk_namespace_bounds(storage_id: OpaqueId) -> (String, String) {
    let id = lowercase_hex(&storage_id.0);
    (format!("{id}:"), format!("{id};"))
}

fn chunk_namespace_query(storage_id: OpaqueId) -> BrowserContentStoreResult<Query> {
    let (lower, upper) = chunk_namespace_bounds(storage_id);
    KeyRange::bound(
        &JsValue::from_str(&lower),
        &JsValue::from_str(&upper),
        Some(false),
        Some(true),
    )
    .map(Query::from)
    .map_err(|error| idb_error("create content chunk namespace range", error))
}

#[cfg(test)]
fn chunk_index_query(
    storage_id: OpaqueId,
    first: u32,
    last: u32,
) -> BrowserContentStoreResult<Query> {
    KeyRange::bound(
        &JsValue::from_str(&chunk_key(storage_id, first)),
        &JsValue::from_str(&chunk_key(storage_id, last)),
        Some(false),
        Some(false),
    )
    .map(Query::from)
    .map_err(|error| idb_error("create content chunk index range", error))
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn encode_media(encoded: &mut Vec<u8>, media: &str) {
    let length = u16::try_from(media.len()).expect("validated media length fits u16");
    encoded.extend_from_slice(&length.to_be_bytes());
    encoded.extend_from_slice(media.as_bytes());
}

fn digest_snapshot(digest: &Sha256) -> [u8; CONTENT_DIGEST_BYTES] {
    <[u8; CONTENT_DIGEST_BYTES]>::from(digest.clone().finalize())
}

fn stored_bytes(
    resource: &'static str,
    value: JsValue,
    max_bytes: usize,
) -> BrowserContentStoreResult<Vec<u8>> {
    if !value.is_instance_of::<Uint8Array>() {
        return Err(corrupt(resource, "stored value is not a Uint8Array"));
    }
    let bytes = Uint8Array::new(&value);
    if bytes.length() as usize > max_bytes {
        return Err(corrupt(
            resource,
            format!("stored value exceeds its encoded byte bound {max_bytes}"),
        ));
    }
    Ok(bytes.to_vec())
}

fn stored_string_key(resource: &'static str, key: JsValue) -> BrowserContentStoreResult<String> {
    key.as_string()
        .ok_or_else(|| corrupt(resource, "stored key is not a string"))
}

fn corrupt(resource: &'static str, reason: impl fmt::Display) -> BrowserContentStoreError {
    BrowserContentStoreError::Corrupt {
        resource,
        reason: bounded_diagnostic(reason.to_string()),
    }
}

fn open_error(error: idb::Error) -> BrowserContentStoreError {
    let mapped = idb_error("open content database", error);
    match mapped {
        BrowserContentStoreError::Platform {
            name: Some(name), ..
        } if name.eq_ignore_ascii_case("VersionError") => {
            BrowserContentStoreError::VersionMismatch {
                expected: CONTENT_STORE_SCHEMA_VERSION,
                actual: None,
            }
        }
        BrowserContentStoreError::VersionMismatch { .. } => mapped,
        other => other,
    }
}

fn idb_error(operation: &str, error: idb::Error) -> BrowserContentStoreError {
    let fallback = format!("{error}; {error:?}");
    let (name, message) = idb_exception_parts(&error);
    let message = bounded_diagnostic(message.unwrap_or(fallback));
    match name.as_deref() {
        Some("QuotaExceededError") => BrowserContentStoreError::QuotaExceeded {
            operation: operation.to_owned(),
            message,
        },
        Some("AbortError") => BrowserContentStoreError::Aborted {
            operation: operation.to_owned(),
        },
        Some("VersionError") => BrowserContentStoreError::VersionMismatch {
            expected: CONTENT_STORE_SCHEMA_VERSION,
            actual: None,
        },
        _ => BrowserContentStoreError::Platform {
            operation: operation.to_owned(),
            name,
            message,
        },
    }
}

fn idb_exception_parts(error: &idb::Error) -> (Option<String>, Option<String>) {
    if let idb::Error::DomException(exception) = error {
        return (Some(exception.name()), Some(exception.message()));
    }
    let value = match error {
        idb::Error::AddFailed(value)
        | idb::Error::ClearFailed(value)
        | idb::Error::CountFailed(value)
        | idb::Error::DeleteFailed(value)
        | idb::Error::GetAllFailed(value)
        | idb::Error::GetAllKeysFailed(value)
        | idb::Error::GetFailed(value)
        | idb::Error::GetKeyFailed(value)
        | idb::Error::IndexedDbOpenFailed(value)
        | idb::Error::TransactionAbortError(value)
        | idb::Error::TransactionCommitError(value)
        | idb::Error::TransactionOpenFailed(value)
        | idb::Error::UpdateFailed(value) => Some(value),
        _ => None,
    };
    let Some(value) = value else {
        return (None, None);
    };
    let property = |name: &str| {
        Reflect::get(value, &JsValue::from_str(name))
            .ok()
            .and_then(|value| value.as_string())
            .filter(|value| !value.is_empty())
    };
    (property("name"), property("message"))
}

fn bounded_diagnostic(mut diagnostic: String) -> String {
    if diagnostic.len() <= MAX_DIAGNOSTIC_BYTES {
        return diagnostic;
    }
    let mut boundary = MAX_DIAGNOSTIC_BYTES;
    while !diagnostic.is_char_boundary(boundary) {
        boundary -= 1;
    }
    diagnostic.truncate(boundary);
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;
    use idb::Factory;
    use std::sync::atomic::{AtomicU32, Ordering};
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    static NEXT_DATABASE_ID: AtomicU32 = AtomicU32::new(0);

    #[wasm_bindgen_test]
    fn canonical_names_keys_and_limits_are_deterministic() {
        assert_eq!(
            canonical_database_name("dev.boon/example:client_1").unwrap(),
            "boon-content::dev.boon/example:client_1"
        );
        assert!(canonical_database_name(" dev.boon").is_err());
        assert!(canonical_database_name("dev.boon?").is_err());

        let id = OpaqueId([0xab; OPAQUE_ID_BYTES]);
        assert_eq!(staging_key(id), "abababababababababababababababab");
        assert_eq!(
            chunk_key(id, 42),
            "abababababababababababababababab:0000002a"
        );
        assert!(chunk_key(id, 9) < chunk_key(id, 10));
        assert_eq!(
            chunk_namespace_bounds(id),
            (
                "abababababababababababababababab:".to_owned(),
                "abababababababababababababababab;".to_owned()
            )
        );

        let mut invalid = BrowserContentStoreLimits::default();
        invalid.max_chunk_bytes = 0;
        assert!(matches!(
            invalid.validate(),
            Err(BrowserContentStoreError::InvalidConfiguration { .. })
        ));
    }

    #[wasm_bindgen_test]
    fn bounded_records_round_trip_and_reject_noncanonical_shapes() {
        let metadata = MetadataRecord {
            digest: [7; CONTENT_DIGEST_BYTES],
            storage_id: OpaqueId([3; OPAQUE_ID_BYTES]),
            size: 7,
            chunk_count: 3,
            chunk_bytes: 3,
            media: "text/plain".to_owned(),
        };
        assert_eq!(
            MetadataRecord::decode(&metadata.encode()).unwrap(),
            metadata
        );
        metadata
            .validate(BrowserContentStoreLimits::default())
            .unwrap();

        let staging = StagingRecord {
            staging_id: OpaqueId([1; OPAQUE_ID_BYTES]),
            storage_id: OpaqueId([2; OPAQUE_ID_BYTES]),
            declared_size: 7,
            written_size: 6,
            lease_expires_ms: 123_456,
            next_sequence: 2,
            chunk_bytes: 3,
            media: "text/plain".to_owned(),
        };
        assert_eq!(StagingRecord::decode(&staging.encode()).unwrap(), staging);
        staging.validate_recovery().unwrap();

        let usage = UsageRecord {
            published_entries: 1,
            published_bytes: 7,
            published_chunks: 3,
            staging_imports: 1,
            staging_reserved_bytes: 9,
            staging_chunks: 2,
        };
        assert_eq!(UsageRecord::decode(&usage.encode()).unwrap(), usage);

        let mut trailing = metadata.encode();
        trailing.push(0);
        assert!(matches!(
            MetadataRecord::decode(&trailing),
            Err(BrowserContentStoreError::Corrupt { .. })
        ));
        assert_eq!(canonical_chunk_count(0, 0).unwrap(), 0);
        assert_eq!(canonical_chunk_count(7, 3).unwrap(), 3);
        assert!(canonical_chunk_count(0, 1).is_err());
    }

    #[wasm_bindgen_test(async)]
    async fn indexed_db_imports_stream_deduplicate_read_and_recover_staging() {
        let package_id = format!(
            "test.content-store.{}.{}",
            js_sys::Date::now() as u64,
            NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let database_name = canonical_database_name(&package_id).unwrap();
        delete_database(&database_name).await;

        let mut limits = BrowserContentStoreLimits::default();
        limits.max_content_entries = 2;
        limits.max_content_bytes = 64;
        limits.max_content_chunks = 16;
        limits.max_staging_imports = 1;
        limits.max_staging_bytes = 64;
        limits.max_staging_chunks = 16;
        limits.max_chunks_per_content = 16;
        limits.max_chunk_bytes = 8;
        limits.max_read_bytes = 16;
        limits.max_read_chunks = 8;

        let store = BrowserIndexedDbContentStore::open(&package_id, limits)
            .await
            .unwrap();
        assert_eq!(store.database_name(), database_name);
        let mut import = store.begin_import(6, "text/plain").await.unwrap();
        assert!(matches!(
            import.append_chunk(1, b"abc").await,
            Err(BrowserContentStoreError::SequenceMismatch {
                expected: 0,
                actual: 1
            })
        ));
        import
            .append_verified_chunk(
                0,
                b"abc",
                <[u8; CONTENT_DIGEST_BYTES]>::from(Sha256::digest(b"abc")),
            )
            .await
            .unwrap();
        import.append_chunk(1, b"def").await.unwrap();
        let digest = <[u8; CONTENT_DIGEST_BYTES]>::from(Sha256::digest(b"abcdef"));
        let content = import.finish(digest).await.unwrap();

        let metadata = store.resolve_metadata(&content).await.unwrap();
        assert_eq!(metadata.content(), &content);
        assert_eq!(metadata.chunk_count(), 2);
        assert_eq!(metadata.chunk_bytes(), 3);
        assert_eq!(store.read_chunk(&content, 0).await.unwrap(), b"abc");
        assert_eq!(store.read_range(&content, 2, 3).await.unwrap(), b"cde");

        let mut duplicate = store.begin_import(6, "text/plain").await.unwrap();
        duplicate.append_chunk(0, b"abcdef").await.unwrap();
        assert_eq!(duplicate.finish(digest).await.unwrap(), content);
        assert_eq!(store.read_chunk(&content, 1).await.unwrap(), b"def");

        let mut abandoned = store
            .begin_import(4, "application/octet-stream")
            .await
            .unwrap();
        abandoned.append_chunk(0, b"data").await.unwrap();
        let without_drop_cleanup = abandoned.state.take().unwrap();
        drop(abandoned);
        drop(store);

        let reopened = BrowserIndexedDbContentStore::open(&package_id, limits)
            .await
            .unwrap();
        assert_eq!(
            reopened.read_range(&content, 0, 6).await.unwrap(),
            b"abcdef"
        );
        assert!(matches!(
            reopened.begin_import(1, "application/octet-stream").await,
            Err(BrowserContentStoreError::LimitExceeded {
                resource: "active content staging imports",
                ..
            })
        ));
        assert_eq!(
            reopened
                .recover_storage_at(without_drop_cleanup.record.lease_expires_ms)
                .await
                .unwrap(),
            1
        );
        let pending = reopened
            .begin_import(1, "application/octet-stream")
            .await
            .unwrap();
        assert!(pending.abort().await.unwrap());
        drop(reopened);
        delete_database(&database_name).await;
    }

    async fn delete_database(database_name: &str) {
        Factory::new()
            .unwrap()
            .delete(database_name)
            .unwrap()
            .await
            .unwrap();
    }
}
