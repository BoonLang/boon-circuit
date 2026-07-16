//! Transactional redb persistence for generic [`boon_query`] collections.
//!
//! Canonical rows are the durable data authority. Derived index entries and an
//! epoch-ordered mutation journal are stored in the same redb transaction. The
//! journal lets reopening reconstruct the exact in-memory collection epoch, so
//! cursor validity is unchanged across process restarts.

#![forbid(unsafe_code)]

use boon_data::Value;
use boon_query::{
    Collection, CollectionId, CollectionPlan, IndexId, IndexKey, IndexPlan, QueryError, QueryPlan,
    QueryResult, RowId, project_index_keys,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::Path;

const FORMAT_VERSION: u32 = 1;
const AUTHORITY_KEY: &str = "authority";
const AUTHORITY_BINDING: &str = "boon.query.redb.authority.v1";
const ROW_BINDING: &str = "boon.query.redb.row.v1";
const JOURNAL_BINDING: &str = "boon.query.redb.journal.v1";
const INDEX_MARKER: &[u8] = b"\x01";

const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("boon_query_redb.metadata.v1");
const ROWS: TableDefinition<&str, &[u8]> = TableDefinition::new("boon_query_redb.rows.v1");
const INDEX_ENTRIES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("boon_query_redb.index_entries.v1");
const JOURNAL: TableDefinition<u64, &[u8]> = TableDefinition::new("boon_query_redb.journal.v1");

/// Controls whether opening requires ready indexes or may rebuild them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OpenPolicy {
    /// Reject stale, corrupt, or plan-incompatible derived indexes.
    #[default]
    RequireReady,
    /// Rebuild only derived indexes after canonical rows and the journal pass
    /// validation. Collection-plan incompatibility and canonical corruption
    /// are never rebuildable.
    RebuildIndexes,
}

/// Durable status for one declared index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexAuthorityStatus {
    pub plan_hash: [u8; 32],
    pub epoch: u64,
}

/// Durable collection authority observed by an opened collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityStatus {
    pub collection: CollectionId,
    pub collection_plan_hash: [u8; 32],
    pub collection_epoch: u64,
    pub row_count: u64,
    pub indexes: BTreeMap<IndexId, IndexAuthorityStatus>,
}

/// Failure to open, validate, mutate, or query a persistent collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistentQueryError {
    Storage(String),
    Codec(String),
    Query(QueryError),
    CorruptAuthority(String),
    IncompatibleCollectionPlan {
        stored: [u8; 32],
        requested: [u8; 32],
    },
    IncompatibleIndexPlans,
    IndexesNotReady {
        collection_epoch: u64,
    },
}

impl fmt::Display for PersistentQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(message) => write!(formatter, "redb storage failed: {message}"),
            Self::Codec(message) => write!(formatter, "persistent query codec failed: {message}"),
            Self::Query(error) => error.fmt(formatter),
            Self::CorruptAuthority(message) => {
                write!(formatter, "corrupt persistent query authority: {message}")
            }
            Self::IncompatibleCollectionPlan { stored, requested } => write!(
                formatter,
                "incompatible collection plan (stored {}, requested {})",
                abbreviated_hash(stored),
                abbreviated_hash(requested)
            ),
            Self::IncompatibleIndexPlans => {
                formatter.write_str("incompatible persistent index plans")
            }
            Self::IndexesNotReady { collection_epoch } => write!(
                formatter,
                "persistent indexes are not ready at collection epoch {collection_epoch}"
            ),
        }
    }
}

impl Error for PersistentQueryError {}

impl From<QueryError> for PersistentQueryError {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredIndexAuthority {
    plan_hash: [u8; 32],
    epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredAuthority {
    format_version: u32,
    collection: CollectionId,
    collection_plan_hash: [u8; 32],
    collection_epoch: u64,
    row_count: u64,
    indexes: BTreeMap<IndexId, StoredIndexAuthority>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JournalMutation {
    Upsert { row_id: RowId, value: Value },
    Remove { row_id: RowId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredIndexEntry {
    index: IndexId,
    key: IndexKey,
    row_id: RowId,
}

#[derive(Clone)]
struct UpsertDelta {
    epoch: u64,
    row_id: RowId,
    old_value: Option<Value>,
    new_value: Value,
}

struct PreparedUpsert {
    epoch: u64,
    row_id: RowId,
    old_row_blob: Option<Vec<u8>>,
    new_row_blob: Vec<u8>,
    old_entries: Vec<Vec<u8>>,
    new_entries: Vec<Vec<u8>>,
    journal_blob: Vec<u8>,
}

/// A generic persistent collection whose query behavior is owned by
/// [`boon_query::Collection`].
pub struct RedbCollection {
    database: Database,
    engine: Collection,
    rows: BTreeMap<RowId, Value>,
    index_plans: BTreeMap<IndexId, IndexPlan>,
    authority: StoredAuthority,
}

impl RedbCollection {
    /// Opens or creates a collection and rejects any authority that is not
    /// already compatible and index-ready.
    pub fn open(
        path: impl AsRef<Path>,
        collection_plan: CollectionPlan,
        index_plans: Vec<IndexPlan>,
    ) -> Result<Self, PersistentQueryError> {
        Self::open_with_policy(path, collection_plan, index_plans, OpenPolicy::RequireReady)
    }

    /// Opens or creates a collection, explicitly rebuilding derived indexes
    /// when canonical authority is valid but indexes are not ready.
    pub fn open_rebuilding_indexes(
        path: impl AsRef<Path>,
        collection_plan: CollectionPlan,
        index_plans: Vec<IndexPlan>,
    ) -> Result<Self, PersistentQueryError> {
        Self::open_with_policy(
            path,
            collection_plan,
            index_plans,
            OpenPolicy::RebuildIndexes,
        )
    }

    pub fn open_with_policy(
        path: impl AsRef<Path>,
        collection_plan: CollectionPlan,
        index_plans: Vec<IndexPlan>,
        policy: OpenPolicy,
    ) -> Result<Self, PersistentQueryError> {
        let (empty_engine, index_plans) = validate_plans(&collection_plan, index_plans)?;
        let requested_collection_hash = collection_plan_hash(&collection_plan)?;
        let requested_indexes = requested_index_authority(&index_plans, 0)?;
        let initial_authority = StoredAuthority {
            format_version: FORMAT_VERSION,
            collection: collection_plan.id,
            collection_plan_hash: requested_collection_hash,
            collection_epoch: 0,
            row_count: 0,
            indexes: requested_indexes.clone(),
        };
        let database = Database::create(path).storage()?;
        let authority_blob = initialize_or_read_authority(&database, &initial_authority)?;
        let mut authority: StoredAuthority =
            decode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &authority_blob)?;

        if authority.format_version != FORMAT_VERSION {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "unsupported format version {}",
                authority.format_version
            )));
        }
        if authority.collection != collection_plan.id
            || authority.collection_plan_hash != requested_collection_hash
        {
            return Err(PersistentQueryError::IncompatibleCollectionPlan {
                stored: authority.collection_plan_hash,
                requested: requested_collection_hash,
            });
        }

        let index_plans_match = index_authority_matches_plans(&authority, &requested_indexes);
        if !index_plans_match && policy == OpenPolicy::RequireReady {
            return Err(PersistentQueryError::IncompatibleIndexPlans);
        }

        let journal = load_journal(&database, authority.collection_epoch)?;
        let (engine, replayed_rows) = replay_journal(empty_engine, &journal)?;
        let rows = load_rows(&database)?;
        let actual_row_count = u64::try_from(rows.len()).map_err(|_| {
            PersistentQueryError::CorruptAuthority(
                "canonical row count does not fit durable metadata".to_owned(),
            )
        })?;
        if authority.row_count != actual_row_count {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "metadata declares {} rows but canonical storage contains {actual_row_count}",
                authority.row_count
            )));
        }
        if replayed_rows != rows {
            return Err(PersistentQueryError::CorruptAuthority(
                "canonical rows disagree with the epoch journal".to_owned(),
            ));
        }
        if engine.epoch() != authority.collection_epoch {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "journal restored epoch {} but metadata declares {}",
                engine.epoch(),
                authority.collection_epoch
            )));
        }

        let expected_entries = expected_index_entries(&rows, &index_plans)?;
        match policy {
            OpenPolicy::RequireReady => {
                if !index_epochs_ready(&authority, &requested_indexes) {
                    return Err(PersistentQueryError::IndexesNotReady {
                        collection_epoch: authority.collection_epoch,
                    });
                }
                if load_index_entries(&database)? != expected_entries {
                    return Err(PersistentQueryError::CorruptAuthority(
                        "derived index entries disagree with canonical rows".to_owned(),
                    ));
                }
            }
            OpenPolicy::RebuildIndexes => {
                authority.indexes =
                    requested_index_authority(&index_plans, authority.collection_epoch)?;
                replace_indexes(&database, &authority_blob, &expected_entries, &authority)?;
            }
        }

        Ok(Self {
            database,
            engine,
            rows,
            index_plans,
            authority,
        })
    }

    pub fn plan(&self) -> &CollectionPlan {
        self.engine.plan()
    }

    pub fn epoch(&self) -> u64 {
        self.engine.epoch()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn authority_status(&self) -> AuthorityStatus {
        AuthorityStatus {
            collection: self.authority.collection,
            collection_plan_hash: self.authority.collection_plan_hash,
            collection_epoch: self.authority.collection_epoch,
            row_count: self.authority.row_count,
            indexes: self
                .authority
                .indexes
                .iter()
                .map(|(id, status)| {
                    (
                        *id,
                        IndexAuthorityStatus {
                            plan_hash: status.plan_hash,
                            epoch: status.epoch,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Executes exactly the same query plan through the in-memory query engine
    /// restored from durable authority.
    pub fn query(&self, plan: &QueryPlan) -> Result<QueryResult, PersistentQueryError> {
        self.engine.query(plan).map_err(Into::into)
    }

    pub fn upsert(&mut self, value: Value) -> Result<RowId, PersistentQueryError> {
        let mut row_ids = self.upsert_batch([value])?;
        Ok(row_ids.pop().expect("one input produces one row identity"))
    }

    /// Applies all rows in one redb transaction. Each row remains an individual
    /// collection mutation and therefore advances the cursor epoch once.
    pub fn upsert_batch(
        &mut self,
        values: impl IntoIterator<Item = Value>,
    ) -> Result<Vec<RowId>, PersistentQueryError> {
        let values = values.into_iter().collect::<Vec<_>>();
        if values.is_empty() {
            return Ok(Vec::new());
        }

        let previous_engine = self.engine.clone();
        let mut next_rows = self.rows.clone();
        let mut deltas = Vec::with_capacity(values.len());
        let mut row_ids = Vec::with_capacity(values.len());
        for value in values {
            let row_id = match self.engine.upsert(value.clone()) {
                Ok(row_id) => row_id,
                Err(error) => {
                    self.engine = previous_engine;
                    return Err(error.into());
                }
            };
            let old_value = next_rows.insert(row_id.clone(), value.clone());
            deltas.push(UpsertDelta {
                epoch: self.engine.epoch(),
                row_id: row_id.clone(),
                old_value,
                new_value: value,
            });
            row_ids.push(row_id);
        }

        let result = self.persist_upserts(&deltas, &next_rows);
        match result {
            Ok(authority) => {
                self.rows = next_rows;
                self.authority = authority;
                Ok(row_ids)
            }
            Err(error) => {
                self.engine = previous_engine;
                Err(error)
            }
        }
    }

    pub fn remove(&mut self, row_id: &RowId) -> Result<Option<Value>, PersistentQueryError> {
        let Some(old_value) = self.rows.get(row_id).cloned() else {
            return Ok(None);
        };
        let previous_engine = self.engine.clone();
        let removed = match self.engine.remove(row_id) {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.engine = previous_engine;
                return Err(PersistentQueryError::CorruptAuthority(format!(
                    "in-memory engine is missing canonical row `{}`",
                    row_id.0
                )));
            }
            Err(error) => {
                self.engine = previous_engine;
                return Err(error.into());
            }
        };
        if removed != old_value {
            self.engine = previous_engine;
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "in-memory row `{}` disagrees with canonical authority",
                row_id.0
            )));
        }

        let result = self.persist_remove(row_id, &old_value);
        match result {
            Ok(authority) => {
                self.rows.remove(row_id);
                self.authority = authority;
                Ok(Some(old_value))
            }
            Err(error) => {
                self.engine = previous_engine;
                Err(error)
            }
        }
    }

    /// Rewrites all derived entries from currently validated canonical rows.
    pub fn rebuild_indexes(&mut self) -> Result<(), PersistentQueryError> {
        let entries = expected_index_entries(&self.rows, &self.index_plans)?;
        let mut authority = self.authority.clone();
        authority.indexes =
            requested_index_authority(&self.index_plans, authority.collection_epoch)?;
        let old_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &self.authority)?;
        replace_indexes(&self.database, &old_blob, &entries, &authority)?;
        self.authority = authority;
        Ok(())
    }

    fn persist_upserts(
        &self,
        deltas: &[UpsertDelta],
        next_rows: &BTreeMap<RowId, Value>,
    ) -> Result<StoredAuthority, PersistentQueryError> {
        let mut prepared = Vec::with_capacity(deltas.len());
        for delta in deltas {
            prepared.push(PreparedUpsert {
                epoch: delta.epoch,
                row_id: delta.row_id.clone(),
                old_row_blob: delta
                    .old_value
                    .as_ref()
                    .map(|value| encode_checked(ROW_BINDING, &delta.row_id.0, value))
                    .transpose()?,
                new_row_blob: encode_checked(ROW_BINDING, &delta.row_id.0, &delta.new_value)?,
                old_entries: delta
                    .old_value
                    .as_ref()
                    .map(|value| index_entries_for_row(&delta.row_id, value, &self.index_plans))
                    .transpose()?
                    .unwrap_or_default(),
                new_entries: index_entries_for_row(
                    &delta.row_id,
                    &delta.new_value,
                    &self.index_plans,
                )?,
                journal_blob: encode_checked(
                    JOURNAL_BINDING,
                    &delta.epoch,
                    &JournalMutation::Upsert {
                        row_id: delta.row_id.clone(),
                        value: delta.new_value.clone(),
                    },
                )?,
            });
        }

        let mut authority = self.authority.clone();
        authority.collection_epoch = self.engine.epoch();
        authority.row_count = u64::try_from(next_rows.len()).map_err(|_| {
            PersistentQueryError::CorruptAuthority(
                "canonical row count exceeds durable metadata".to_owned(),
            )
        })?;
        for index in authority.indexes.values_mut() {
            index.epoch = authority.collection_epoch;
        }
        let old_authority_blob =
            encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &self.authority)?;
        let new_authority_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &authority)?;

        let transaction = self.database.begin_write().storage()?;
        verify_authority_blob(&transaction, &old_authority_blob)?;
        {
            let mut rows = transaction.open_table(ROWS).storage()?;
            for delta in &prepared {
                let previous = rows
                    .insert(delta.row_id.0.as_str(), delta.new_row_blob.as_slice())
                    .storage()?;
                let previous = previous.map(|value| value.value().to_vec());
                if previous != delta.old_row_blob {
                    return Err(PersistentQueryError::CorruptAuthority(format!(
                        "canonical row `{}` changed outside this collection",
                        delta.row_id.0
                    )));
                }
            }
        }
        {
            let mut entries = transaction.open_table(INDEX_ENTRIES).storage()?;
            for delta in &prepared {
                for entry in &delta.old_entries {
                    if entries.remove(entry.as_slice()).storage()?.is_none() {
                        return Err(PersistentQueryError::CorruptAuthority(
                            "an indexed row was missing a derived entry".to_owned(),
                        ));
                    }
                }
                for entry in &delta.new_entries {
                    if entries
                        .insert(entry.as_slice(), INDEX_MARKER)
                        .storage()?
                        .is_some()
                    {
                        return Err(PersistentQueryError::CorruptAuthority(
                            "a derived index entry already existed unexpectedly".to_owned(),
                        ));
                    }
                }
            }
        }
        {
            let mut journal = transaction.open_table(JOURNAL).storage()?;
            for delta in &prepared {
                if journal
                    .insert(delta.epoch, delta.journal_blob.as_slice())
                    .storage()?
                    .is_some()
                {
                    return Err(PersistentQueryError::CorruptAuthority(format!(
                        "journal epoch {} already exists",
                        delta.epoch
                    )));
                }
            }
        }
        {
            let mut metadata = transaction.open_table(METADATA).storage()?;
            metadata
                .insert(AUTHORITY_KEY, new_authority_blob.as_slice())
                .storage()?;
        }
        transaction.commit().storage()?;
        Ok(authority)
    }

    fn persist_remove(
        &self,
        row_id: &RowId,
        old_value: &Value,
    ) -> Result<StoredAuthority, PersistentQueryError> {
        let old_row_blob = encode_checked(ROW_BINDING, &row_id.0, old_value)?;
        let old_entries = index_entries_for_row(row_id, old_value, &self.index_plans)?;
        let epoch = self.engine.epoch();
        let journal_blob = encode_checked(
            JOURNAL_BINDING,
            &epoch,
            &JournalMutation::Remove {
                row_id: row_id.clone(),
            },
        )?;
        let mut authority = self.authority.clone();
        authority.collection_epoch = epoch;
        authority.row_count = authority.row_count.checked_sub(1).ok_or_else(|| {
            PersistentQueryError::CorruptAuthority(
                "cannot remove a row from zero durable row count".to_owned(),
            )
        })?;
        for index in authority.indexes.values_mut() {
            index.epoch = epoch;
        }
        let old_authority_blob =
            encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &self.authority)?;
        let new_authority_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &authority)?;

        let transaction = self.database.begin_write().storage()?;
        verify_authority_blob(&transaction, &old_authority_blob)?;
        {
            let mut rows = transaction.open_table(ROWS).storage()?;
            let removed = rows
                .remove(row_id.0.as_str())
                .storage()?
                .map(|value| value.value().to_vec());
            if removed.as_deref() != Some(old_row_blob.as_slice()) {
                return Err(PersistentQueryError::CorruptAuthority(format!(
                    "canonical row `{}` changed outside this collection",
                    row_id.0
                )));
            }
        }
        {
            let mut entries = transaction.open_table(INDEX_ENTRIES).storage()?;
            for entry in old_entries {
                if entries.remove(entry.as_slice()).storage()?.is_none() {
                    return Err(PersistentQueryError::CorruptAuthority(
                        "removed row was missing a derived index entry".to_owned(),
                    ));
                }
            }
        }
        {
            let mut journal = transaction.open_table(JOURNAL).storage()?;
            if journal
                .insert(epoch, journal_blob.as_slice())
                .storage()?
                .is_some()
            {
                return Err(PersistentQueryError::CorruptAuthority(format!(
                    "journal epoch {epoch} already exists"
                )));
            }
        }
        {
            let mut metadata = transaction.open_table(METADATA).storage()?;
            metadata
                .insert(AUTHORITY_KEY, new_authority_blob.as_slice())
                .storage()?;
        }
        transaction.commit().storage()?;
        Ok(authority)
    }
}

/// Explicit name for callers that prefer the storage-neutral concept.
pub type PersistentCollection = RedbCollection;

fn validate_plans(
    collection_plan: &CollectionPlan,
    index_plans: Vec<IndexPlan>,
) -> Result<(Collection, BTreeMap<IndexId, IndexPlan>), PersistentQueryError> {
    let mut by_id = BTreeMap::new();
    for plan in index_plans {
        if by_id.insert(plan.id, plan).is_some() {
            return Err(QueryError::InvalidPlan("duplicate index identity".to_owned()).into());
        }
    }
    let engine = Collection::new(collection_plan.clone(), by_id.values().cloned().collect())?;
    Ok((engine, by_id))
}

fn collection_plan_hash(plan: &CollectionPlan) -> Result<[u8; 32], PersistentQueryError> {
    digest_value(&("boon.query.redb.collection-plan.v1", plan))
}

fn index_plan_hash(plan: &IndexPlan) -> Result<[u8; 32], PersistentQueryError> {
    digest_value(&("boon.query.redb.index-plan.v1", plan))
}

fn requested_index_authority(
    plans: &BTreeMap<IndexId, IndexPlan>,
    epoch: u64,
) -> Result<BTreeMap<IndexId, StoredIndexAuthority>, PersistentQueryError> {
    plans
        .iter()
        .map(|(id, plan)| {
            Ok((
                *id,
                StoredIndexAuthority {
                    plan_hash: index_plan_hash(plan)?,
                    epoch,
                },
            ))
        })
        .collect()
}

fn index_authority_matches_plans(
    authority: &StoredAuthority,
    requested: &BTreeMap<IndexId, StoredIndexAuthority>,
) -> bool {
    authority.indexes.len() == requested.len()
        && requested.iter().all(|(id, expected)| {
            authority
                .indexes
                .get(id)
                .is_some_and(|stored| stored.plan_hash == expected.plan_hash)
        })
}

fn index_epochs_ready(
    authority: &StoredAuthority,
    requested: &BTreeMap<IndexId, StoredIndexAuthority>,
) -> bool {
    index_authority_matches_plans(authority, requested)
        && authority
            .indexes
            .values()
            .all(|index| index.epoch == authority.collection_epoch)
}

fn initialize_or_read_authority(
    database: &Database,
    initial: &StoredAuthority,
) -> Result<Vec<u8>, PersistentQueryError> {
    let initial_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, initial)?;
    let transaction = database.begin_write().storage()?;
    let (existing, metadata_len) = {
        let metadata = transaction.open_table(METADATA).storage()?;
        let existing = metadata
            .get(AUTHORITY_KEY)
            .storage()?
            .map(|value| value.value().to_vec());
        (existing, metadata.len().storage()?)
    };
    let rows_len = {
        let rows = transaction.open_table(ROWS).storage()?;
        rows.len().storage()?
    };
    let indexes_len = {
        let indexes = transaction.open_table(INDEX_ENTRIES).storage()?;
        indexes.len().storage()?
    };
    let journal_len = {
        let journal = transaction.open_table(JOURNAL).storage()?;
        journal.len().storage()?
    };

    let authority_blob = match existing {
        Some(blob) => {
            if metadata_len != 1 {
                return Err(PersistentQueryError::CorruptAuthority(
                    "metadata contains undeclared authority records".to_owned(),
                ));
            }
            blob
        }
        None => {
            if metadata_len != 0 || rows_len != 0 || indexes_len != 0 || journal_len != 0 {
                return Err(PersistentQueryError::CorruptAuthority(
                    "storage contains data without collection authority".to_owned(),
                ));
            }
            {
                let mut metadata = transaction.open_table(METADATA).storage()?;
                metadata
                    .insert(AUTHORITY_KEY, initial_blob.as_slice())
                    .storage()?;
            }
            initial_blob
        }
    };
    transaction.commit().storage()?;
    Ok(authority_blob)
}

fn load_journal(
    database: &Database,
    collection_epoch: u64,
) -> Result<Vec<(u64, JournalMutation)>, PersistentQueryError> {
    let transaction = database.begin_read().storage()?;
    let journal = transaction.open_table(JOURNAL).storage()?;
    let mut mutations = Vec::new();
    let mut previous_epoch = 0u64;
    for entry in journal.iter().storage()? {
        let (epoch, blob) = entry.storage()?;
        let epoch = epoch.value();
        let expected = previous_epoch.checked_add(1).ok_or_else(|| {
            PersistentQueryError::CorruptAuthority("journal epoch overflow".to_owned())
        })?;
        if epoch != expected {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "journal jumps from epoch {previous_epoch} to {epoch}"
            )));
        }
        if epoch > collection_epoch {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "journal contains epoch {epoch} after declared epoch {collection_epoch}"
            )));
        }
        let mutation = decode_checked(JOURNAL_BINDING, &epoch, blob.value())?;
        mutations.push((epoch, mutation));
        previous_epoch = epoch;
    }
    if previous_epoch != collection_epoch {
        return Err(PersistentQueryError::CorruptAuthority(format!(
            "journal ends at epoch {previous_epoch} but metadata declares {collection_epoch}"
        )));
    }
    Ok(mutations)
}

fn replay_journal(
    mut engine: Collection,
    journal: &[(u64, JournalMutation)],
) -> Result<(Collection, BTreeMap<RowId, Value>), PersistentQueryError> {
    let mut rows = BTreeMap::new();
    for (epoch, mutation) in journal {
        match mutation {
            JournalMutation::Upsert { row_id, value } => {
                let restored_id = engine.upsert(value.clone()).map_err(|error| {
                    PersistentQueryError::CorruptAuthority(format!(
                        "journal epoch {epoch} contains an invalid upsert: {error}"
                    ))
                })?;
                if &restored_id != row_id {
                    return Err(PersistentQueryError::CorruptAuthority(format!(
                        "journal epoch {epoch} declares row `{}` but value restores as `{}`",
                        row_id.0, restored_id.0
                    )));
                }
                rows.insert(row_id.clone(), value.clone());
            }
            JournalMutation::Remove { row_id } => {
                let removed = engine.remove(row_id).map_err(|error| {
                    PersistentQueryError::CorruptAuthority(format!(
                        "journal epoch {epoch} contains an invalid removal: {error}"
                    ))
                })?;
                let expected = rows.remove(row_id);
                if removed.is_none() || removed != expected {
                    return Err(PersistentQueryError::CorruptAuthority(format!(
                        "journal epoch {epoch} removes missing row `{}`",
                        row_id.0
                    )));
                }
            }
        }
        if engine.epoch() != *epoch {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "journal mutation {epoch} restored engine epoch {}",
                engine.epoch()
            )));
        }
    }
    Ok((engine, rows))
}

fn load_rows(database: &Database) -> Result<BTreeMap<RowId, Value>, PersistentQueryError> {
    let transaction = database.begin_read().storage()?;
    let table = transaction.open_table(ROWS).storage()?;
    let mut rows = BTreeMap::new();
    for entry in table.iter().storage()? {
        let (row_id, blob) = entry.storage()?;
        let row_id = RowId::new(row_id.value().to_owned()).map_err(|error| {
            PersistentQueryError::CorruptAuthority(format!(
                "canonical row has invalid identity: {error}"
            ))
        })?;
        let value = decode_checked(ROW_BINDING, &row_id.0, blob.value())?;
        if rows.insert(row_id.clone(), value).is_some() {
            return Err(PersistentQueryError::CorruptAuthority(format!(
                "canonical row `{}` is duplicated",
                row_id.0
            )));
        }
    }
    Ok(rows)
}

fn load_index_entries(database: &Database) -> Result<BTreeSet<Vec<u8>>, PersistentQueryError> {
    let transaction = database.begin_read().storage()?;
    let table = transaction.open_table(INDEX_ENTRIES).storage()?;
    let mut entries = BTreeSet::new();
    for entry in table.iter().storage()? {
        let (key, marker) = entry.storage()?;
        if marker.value() != INDEX_MARKER {
            return Err(PersistentQueryError::CorruptAuthority(
                "derived index entry has an invalid marker".to_owned(),
            ));
        }
        let bytes = key.value().to_vec();
        let decoded: StoredIndexEntry = from_cbor(&bytes).map_err(|error| {
            PersistentQueryError::CorruptAuthority(format!(
                "derived index entry cannot be decoded: {error}"
            ))
        })?;
        if to_cbor(&decoded)? != bytes {
            return Err(PersistentQueryError::CorruptAuthority(
                "derived index entry is not canonically encoded".to_owned(),
            ));
        }
        entries.insert(bytes);
    }
    Ok(entries)
}

fn expected_index_entries(
    rows: &BTreeMap<RowId, Value>,
    plans: &BTreeMap<IndexId, IndexPlan>,
) -> Result<BTreeSet<Vec<u8>>, PersistentQueryError> {
    let mut entries = BTreeSet::new();
    for (row_id, value) in rows {
        entries.extend(index_entries_for_row(row_id, value, plans)?);
    }
    Ok(entries)
}

fn index_entries_for_row(
    row_id: &RowId,
    value: &Value,
    plans: &BTreeMap<IndexId, IndexPlan>,
) -> Result<Vec<Vec<u8>>, PersistentQueryError> {
    let mut entries = Vec::new();
    for (index, plan) in plans {
        for key in project_index_keys(plan, value)? {
            entries.push(to_cbor(&StoredIndexEntry {
                index: *index,
                key,
                row_id: row_id.clone(),
            })?);
        }
    }
    Ok(entries)
}

fn replace_indexes(
    database: &Database,
    expected_authority_blob: &[u8],
    entries: &BTreeSet<Vec<u8>>,
    authority: &StoredAuthority,
) -> Result<(), PersistentQueryError> {
    let authority_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, authority)?;
    let transaction = database.begin_write().storage()?;
    verify_authority_blob(&transaction, expected_authority_blob)?;
    {
        let mut table = transaction.open_table(INDEX_ENTRIES).storage()?;
        table.retain(|_, _| false).storage()?;
        for entry in entries {
            if table
                .insert(entry.as_slice(), INDEX_MARKER)
                .storage()?
                .is_some()
            {
                return Err(PersistentQueryError::CorruptAuthority(
                    "duplicate entry while rebuilding indexes".to_owned(),
                ));
            }
        }
    }
    {
        let mut metadata = transaction.open_table(METADATA).storage()?;
        metadata
            .insert(AUTHORITY_KEY, authority_blob.as_slice())
            .storage()?;
    }
    transaction.commit().storage()?;
    Ok(())
}

fn verify_authority_blob(
    transaction: &redb::WriteTransaction,
    expected: &[u8],
) -> Result<(), PersistentQueryError> {
    let metadata = transaction.open_table(METADATA).storage()?;
    let actual = metadata
        .get(AUTHORITY_KEY)
        .storage()?
        .map(|value| value.value().to_vec());
    if actual.as_deref() != Some(expected) {
        return Err(PersistentQueryError::CorruptAuthority(
            "collection authority changed outside this handle".to_owned(),
        ));
    }
    Ok(())
}

fn encode_checked<T: Serialize, B: Serialize + ?Sized>(
    domain: &str,
    binding: &B,
    value: &T,
) -> Result<Vec<u8>, PersistentQueryError> {
    let payload = to_cbor(value)?;
    let checksum = digest_value(&("boon.query.redb.checked.v1", domain, binding, &payload))?;
    let mut bytes = Vec::with_capacity(checksum.len() + payload.len());
    bytes.extend_from_slice(&checksum);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_checked<T: DeserializeOwned, B: Serialize + ?Sized>(
    domain: &str,
    binding: &B,
    bytes: &[u8],
) -> Result<T, PersistentQueryError> {
    if bytes.len() < 32 {
        return Err(PersistentQueryError::CorruptAuthority(format!(
            "{domain} record is truncated"
        )));
    }
    let (stored_checksum, payload) = bytes.split_at(32);
    let expected_checksum =
        digest_value(&("boon.query.redb.checked.v1", domain, binding, payload))?;
    if stored_checksum != expected_checksum {
        return Err(PersistentQueryError::CorruptAuthority(format!(
            "{domain} checksum mismatch"
        )));
    }
    from_cbor(payload).map_err(|error| {
        PersistentQueryError::CorruptAuthority(format!(
            "{domain} record cannot be decoded: {error}"
        ))
    })
}

fn digest_value(value: &impl Serialize) -> Result<[u8; 32], PersistentQueryError> {
    Ok(Sha256::digest(to_cbor(value)?).into())
}

fn to_cbor(value: &impl Serialize) -> Result<Vec<u8>, PersistentQueryError> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)
        .map_err(|error| PersistentQueryError::Codec(error.to_string()))?;
    Ok(bytes)
}

fn from_cbor<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PersistentQueryError> {
    ciborium::de::from_reader(bytes).map_err(|error| PersistentQueryError::Codec(error.to_string()))
}

fn abbreviated_hash(hash: &[u8; 32]) -> String {
    hash[..6].iter().map(|byte| format!("{byte:02x}")).collect()
}

trait StorageResult<T> {
    fn storage(self) -> Result<T, PersistentQueryError>;
}

impl<T, E: fmt::Display> StorageResult<T> for Result<T, E> {
    fn storage(self) -> Result<T, PersistentQueryError> {
        self.map_err(|error| PersistentQueryError::Storage(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_query::IndexFieldPlan;
    use tempfile::TempDir;

    #[test]
    fn stale_index_epoch_requires_explicit_rebuild() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("stale-index.redb");
        let collection = CollectionPlan::new("records", vec!["id".to_owned()]).unwrap();
        let index = IndexPlan::new(
            collection.id,
            "record_name",
            vec![IndexFieldPlan::field("name")],
            true,
        )
        .unwrap();
        let mut persistent =
            RedbCollection::open(&path, collection.clone(), vec![index.clone()]).unwrap();
        persistent
            .upsert(Value::Record(BTreeMap::from([
                ("id".to_owned(), Value::Text("1".to_owned())),
                ("name".to_owned(), Value::Text("one".to_owned())),
            ])))
            .unwrap();
        drop(persistent);

        let database = Database::create(&path).unwrap();
        let transaction = database.begin_write().unwrap();
        {
            let mut metadata = transaction.open_table(METADATA).unwrap();
            let blob = metadata.get(AUTHORITY_KEY).unwrap().unwrap();
            let mut authority: StoredAuthority =
                decode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, blob.value()).unwrap();
            drop(blob);
            authority.indexes.get_mut(&index.id).unwrap().epoch = 0;
            let stale_blob = encode_checked(AUTHORITY_BINDING, &AUTHORITY_KEY, &authority).unwrap();
            metadata
                .insert(AUTHORITY_KEY, stale_blob.as_slice())
                .unwrap();
        }
        transaction.commit().unwrap();
        drop(database);

        let error = RedbCollection::open(&path, collection.clone(), vec![index.clone()])
            .err()
            .expect("stale index epoch must not become ready");
        assert_eq!(
            error,
            PersistentQueryError::IndexesNotReady {
                collection_epoch: 1
            }
        );

        let rebuilt =
            RedbCollection::open_rebuilding_indexes(&path, collection, vec![index.clone()])
                .unwrap();
        assert_eq!(rebuilt.authority_status().indexes[&index.id].epoch, 1);
    }
}
