use crate::key::{KeyError, KeySchema, StructuralKey, StructuralValue, lexicographic_successor};
use crate::work::{WorkLimitExceeded, WorkTracker};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, btree_set};
use std::error::Error;
use std::fmt;
use std::ops::{Bound, RangeBounds};

const IDENTITY_BYTES: u64 = 16;
const MAX_COMPOSITE_BRANCHES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexResourceLimits {
    pub max_entries: u64,
    pub max_encoded_key_bytes: u64,
    pub max_payload_bytes: u64,
}

impl IndexResourceLimits {
    pub const UNBOUNDED: Self = Self {
        max_entries: u64::MAX,
        max_encoded_key_bytes: u64::MAX,
        max_payload_bytes: u64::MAX,
    };

    pub const fn new(max_entries: u64, max_encoded_key_bytes: u64, max_payload_bytes: u64) -> Self {
        Self {
            max_entries,
            max_encoded_key_bytes,
            max_payload_bytes,
        }
    }
}

impl Default for IndexResourceLimits {
    fn default() -> Self {
        Self::UNBOUNDED
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexResource {
    Entries,
    EncodedKeyBytes,
    PayloadBytes,
}

/// Compiler-assigned identity of a physical typed key projection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IndexPlanId([u8; 16]);

impl IndexPlanId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Stable identity owned by the canonical list authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RowId([u8; 16]);

impl RowId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Stable order-maintenance token owned by the canonical source list.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceOrderToken([u8; 16]);

impl SourceOrderToken {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// A direct continuation position in index order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorKey {
    key: StructuralKey,
    source_order: SourceOrderToken,
    row_id: RowId,
}

impl CursorKey {
    pub const fn new(key: StructuralKey, source_order: SourceOrderToken, row_id: RowId) -> Self {
        Self {
            key,
            source_order,
            row_id,
        }
    }

    pub const fn key(&self) -> &StructuralKey {
        &self.key
    }

    pub const fn source_order(&self) -> SourceOrderToken {
        self.source_order
    }

    pub const fn row_id(&self) -> RowId {
        self.row_id
    }
}

/// One index hit. It contains no canonical row value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessItem {
    key: StructuralKey,
    source_order: SourceOrderToken,
    row_id: RowId,
}

impl AccessItem {
    pub const fn key(&self) -> &StructuralKey {
        &self.key
    }

    pub const fn source_order(&self) -> SourceOrderToken {
        self.source_order
    }

    pub const fn row_id(&self) -> RowId {
        self.row_id
    }

    pub fn cursor_key(&self) -> CursorKey {
        CursorKey::new(self.key.clone(), self.source_order, self.row_id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    Inserted,
    Removed,
    Updated,
    Unchanged,
    NotFound,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexMetrics {
    pub logical_rows: u64,
    pub index_entries: u64,
    pub insertions: u64,
    pub removals: u64,
    pub updates: u64,
    pub unchanged_updates: u64,
    pub physical_entry_updates: u64,
    pub rebuilds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntegrityReport {
    pub logical_rows: u64,
    pub index_entries: u64,
    pub encoded_key_bytes: u64,
    pub structural_key_bytes: u64,
    pub source_order_bytes: u64,
    pub row_identity_bytes: u64,
}

impl IntegrityReport {
    /// Deterministic payload owned by the index, excluding allocator and tree
    /// node overhead which is target- and allocator-specific.
    pub const fn payload_bytes(self) -> u64 {
        self.encoded_key_bytes
            .saturating_add(self.structural_key_bytes)
            .saturating_add(self.source_order_bytes)
            .saturating_add(self.row_identity_bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderedIndexIntegrityPhase {
    Rows,
    Entries,
    Accounting,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderedIndexIntegrityProgress {
    pub phase: OrderedIndexIntegrityPhase,
    pub completed_steps: u64,
    pub rows_checked: u64,
    pub entries_checked: u64,
    pub accounting_entries_checked: u64,
    pub accounting_rows_checked: u64,
    pub total_rows: u64,
    pub total_entries: u64,
}

pub struct OrderedIndexIntegrityResult {
    index: OrderedIndex,
    report: IntegrityReport,
}

impl OrderedIndexIntegrityResult {
    pub const fn index(&self) -> &OrderedIndex {
        &self.index
    }

    pub const fn report(&self) -> IntegrityReport {
        self.report
    }

    pub fn into_parts(self) -> (OrderedIndex, IntegrityReport) {
        (self.index, self.report)
    }
}

pub enum OrderedIndexIntegrityPoll {
    Pending(OrderedIndexIntegrityProgress),
    Ready(OrderedIndexIntegrityResult),
}

#[derive(Clone, Debug)]
struct RowState {
    keys: Vec<StructuralKey>,
    source_order: SourceOrderToken,
}

struct PreparedRowKeys {
    keys: Vec<StructuralKey>,
    encoded: Vec<Vec<u8>>,
    encoded_bytes: u64,
    structural_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntryOrderKey {
    encoded_key: Vec<u8>,
    suffix: EntrySuffix,
}

impl EntryOrderKey {
    fn before(encoded_key: Vec<u8>) -> Self {
        Self {
            encoded_key,
            suffix: EntrySuffix::BeforeAll,
        }
    }

    fn row(encoded_key: Vec<u8>, source_order: SourceOrderToken, row_id: RowId) -> Self {
        Self {
            encoded_key,
            suffix: EntrySuffix::Row(source_order, row_id),
        }
    }

    fn after(encoded_key: Vec<u8>) -> Self {
        Self {
            encoded_key,
            suffix: EntrySuffix::AfterAll,
        }
    }

    fn row_parts(&self) -> Option<(SourceOrderToken, RowId)> {
        match self.suffix {
            EntrySuffix::Row(source_order, row_id) => Some((source_order, row_id)),
            EntrySuffix::BeforeAll | EntrySuffix::AfterAll => None,
        }
    }
}

impl PartialOrd for EntryOrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EntryOrderKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.encoded_key
            .cmp(&other.encoded_key)
            .then_with(|| self.suffix.cmp(&other.suffix))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EntrySuffix {
    BeforeAll,
    Row(SourceOrderToken, RowId),
    AfterAll,
}

#[derive(Clone, Debug)]
pub struct OrderedIndex {
    plan_id: IndexPlanId,
    schema: KeySchema,
    limits: IndexResourceLimits,
    entries: BTreeSet<EntryOrderKey>,
    rows: BTreeMap<RowId, RowState>,
    encoded_key_bytes: u64,
    structural_key_bytes: u64,
    metrics: IndexMetrics,
    #[cfg(test)]
    drop_probe: Option<std::sync::Arc<()>>,
}

pub struct OrderedIndexIntegrityTask {
    index: Option<OrderedIndex>,
    validation: OrderedIndexIntegrityValidation,
    failed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OrderedIndexIntegrityValidationPhase {
    Rows,
    Entries,
    AccountingEntries,
    AccountingRows,
    Complete,
}

struct OrderedIndexIntegrityValidation {
    phase: OrderedIndexIntegrityValidationPhase,
    initialized: bool,
    row_cursor: Option<RowId>,
    entry_cursor: Option<EntryOrderKey>,
    accounting_entry_cursor: Option<EntryOrderKey>,
    accounting_row_cursor: Option<RowId>,
    completed_steps: u64,
    rows_checked: u64,
    entries_checked: u64,
    accounting_entries_checked: u64,
    accounting_rows_checked: u64,
    total_rows: u64,
    total_entries: u64,
    encoded_key_bytes: u64,
    structural_key_bytes: u64,
    expected_entries: u64,
}

enum OrderedIndexIntegrityValidationPoll {
    Pending,
    Ready(IntegrityReport),
}

impl OrderedIndex {
    pub fn new(plan_id: IndexPlanId, schema: KeySchema) -> Self {
        Self::new_with_limits(plan_id, schema, IndexResourceLimits::UNBOUNDED)
    }

    pub fn new_with_limits(
        plan_id: IndexPlanId,
        schema: KeySchema,
        limits: IndexResourceLimits,
    ) -> Self {
        Self {
            plan_id,
            schema,
            limits,
            entries: BTreeSet::new(),
            rows: BTreeMap::new(),
            encoded_key_bytes: 0,
            structural_key_bytes: 0,
            metrics: IndexMetrics::default(),
            #[cfg(test)]
            drop_probe: None,
        }
    }

    pub fn rebuild<I>(plan_id: IndexPlanId, schema: KeySchema, rows: I) -> Result<Self, AccessError>
    where
        I: IntoIterator<Item = (RowId, SourceOrderToken, StructuralKey)>,
    {
        let mut index = Self::new(plan_id, schema);
        for (row_id, source_order, key) in rows {
            index.insert(row_id, source_order, key)?;
        }
        index.metrics.rebuilds = 1;
        Ok(index)
    }

    pub fn rebuild_many<I, K>(
        plan_id: IndexPlanId,
        schema: KeySchema,
        rows: I,
    ) -> Result<Self, AccessError>
    where
        I: IntoIterator<Item = (RowId, SourceOrderToken, K)>,
        K: IntoIterator<Item = StructuralKey>,
    {
        let mut index = Self::new(plan_id, schema);
        for (row_id, source_order, keys) in rows {
            index.insert_many(row_id, source_order, keys)?;
        }
        index.metrics.rebuilds = 1;
        Ok(index)
    }

    pub fn rebuild_with_limits<I>(
        plan_id: IndexPlanId,
        schema: KeySchema,
        limits: IndexResourceLimits,
        rows: I,
    ) -> Result<Self, AccessError>
    where
        I: IntoIterator<Item = (RowId, SourceOrderToken, StructuralKey)>,
    {
        let mut index = Self::new_with_limits(plan_id, schema, limits);
        for (row_id, source_order, key) in rows {
            index.insert(row_id, source_order, key)?;
        }
        index.metrics.rebuilds = 1;
        Ok(index)
    }

    pub fn rebuild_many_with_limits<I, K>(
        plan_id: IndexPlanId,
        schema: KeySchema,
        limits: IndexResourceLimits,
        rows: I,
    ) -> Result<Self, AccessError>
    where
        I: IntoIterator<Item = (RowId, SourceOrderToken, K)>,
        K: IntoIterator<Item = StructuralKey>,
    {
        let mut index = Self::new_with_limits(plan_id, schema, limits);
        for (row_id, source_order, keys) in rows {
            index.insert_many(row_id, source_order, keys)?;
        }
        index.metrics.rebuilds = 1;
        Ok(index)
    }

    pub const fn schema(&self) -> &KeySchema {
        &self.schema
    }

    pub const fn plan_id(&self) -> IndexPlanId {
        self.plan_id
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn contains(&self, row_id: RowId) -> bool {
        self.rows.contains_key(&row_id)
    }

    pub fn metrics(&self) -> IndexMetrics {
        IndexMetrics {
            logical_rows: usize_to_u64(self.rows.len()),
            index_entries: usize_to_u64(self.entries.len()),
            ..self.metrics
        }
    }

    pub const fn resource_limits(&self) -> IndexResourceLimits {
        self.limits
    }

    pub fn set_max_payload_bytes(&mut self, maximum: u64) -> Result<(), AccessError> {
        check_resource(IndexResource::PayloadBytes, self.payload_bytes(), maximum)?;
        self.limits.max_payload_bytes = maximum;
        Ok(())
    }

    pub fn payload_bytes(&self) -> u64 {
        retained_payload_bytes(
            usize_to_u64(self.entries.len()),
            self.encoded_key_bytes,
            self.structural_key_bytes,
        )
    }

    pub const fn encoded_key_bytes(&self) -> u64 {
        self.encoded_key_bytes
    }

    pub const fn structural_key_bytes(&self) -> u64 {
        self.structural_key_bytes
    }

    pub fn insert(
        &mut self,
        row_id: RowId,
        source_order: SourceOrderToken,
        key: StructuralKey,
    ) -> Result<MutationOutcome, AccessError> {
        self.insert_many(row_id, source_order, [key])
    }

    pub fn insert_many(
        &mut self,
        row_id: RowId,
        source_order: SourceOrderToken,
        keys: impl IntoIterator<Item = StructuralKey>,
    ) -> Result<MutationOutcome, AccessError> {
        if self.rows.contains_key(&row_id) {
            return Err(AccessError::DuplicateRow(row_id));
        }
        let prepared = self.prepare_row_keys(keys)?;
        self.check_projected_resources(
            usize_to_u64(self.entries.len()).saturating_add(usize_to_u64(prepared.keys.len())),
            self.encoded_key_bytes
                .saturating_add(prepared.encoded_bytes),
            self.structural_key_bytes
                .saturating_add(prepared.structural_bytes),
            prepared
                .encoded
                .iter()
                .map(|encoded| usize_to_u64(encoded.len()))
                .max()
                .unwrap_or(0),
        )?;
        for encoded in &prepared.encoded {
            let entry = EntryOrderKey::row(encoded.clone(), source_order, row_id);
            if !self.entries.insert(entry) {
                for rollback in &prepared.encoded {
                    self.entries.remove(&EntryOrderKey::row(
                        rollback.clone(),
                        source_order,
                        row_id,
                    ));
                    if rollback == encoded {
                        break;
                    }
                }
                return Err(AccessError::CorruptIndex(
                    "new row collided with an existing index entry",
                ));
            }
        }
        self.rows.insert(
            row_id,
            RowState {
                keys: prepared.keys,
                source_order,
            },
        );
        self.encoded_key_bytes = self
            .encoded_key_bytes
            .saturating_add(prepared.encoded_bytes);
        self.structural_key_bytes = self
            .structural_key_bytes
            .saturating_add(prepared.structural_bytes);
        self.metrics.insertions = self.metrics.insertions.saturating_add(1);
        self.metrics.physical_entry_updates = self
            .metrics
            .physical_entry_updates
            .saturating_add(usize_to_u64(prepared.encoded.len()));
        Ok(MutationOutcome::Inserted)
    }

    pub fn remove(&mut self, row_id: RowId) -> Result<MutationOutcome, AccessError> {
        let Some(previous) = self.rows.get(&row_id).cloned() else {
            return Ok(MutationOutcome::NotFound);
        };
        let prepared = self.prepare_row_keys(previous.keys.iter().cloned())?;
        for encoded in &prepared.encoded {
            let entry = EntryOrderKey::row(encoded.clone(), previous.source_order, row_id);
            if !self.entries.remove(&entry) {
                return Err(AccessError::CorruptIndex(
                    "row metadata had no corresponding index entry",
                ));
            }
        }
        self.rows.remove(&row_id);
        self.encoded_key_bytes = self
            .encoded_key_bytes
            .saturating_sub(prepared.encoded_bytes);
        self.structural_key_bytes = self
            .structural_key_bytes
            .saturating_sub(prepared.structural_bytes);
        self.metrics.removals = self.metrics.removals.saturating_add(1);
        self.metrics.physical_entry_updates = self
            .metrics
            .physical_entry_updates
            .saturating_add(usize_to_u64(prepared.encoded.len()));
        Ok(MutationOutcome::Removed)
    }

    pub fn update(
        &mut self,
        row_id: RowId,
        source_order: SourceOrderToken,
        key: StructuralKey,
    ) -> Result<MutationOutcome, AccessError> {
        self.update_many(row_id, source_order, [key])
    }

    pub fn update_many(
        &mut self,
        row_id: RowId,
        source_order: SourceOrderToken,
        keys: impl IntoIterator<Item = StructuralKey>,
    ) -> Result<MutationOutcome, AccessError> {
        let prepared = self.prepare_row_keys(keys)?;
        let Some(previous) = self.rows.get(&row_id).cloned() else {
            return Err(AccessError::UnknownRow(row_id));
        };
        if previous.keys == prepared.keys && previous.source_order == source_order {
            self.metrics.unchanged_updates = self.metrics.unchanged_updates.saturating_add(1);
            return Ok(MutationOutcome::Unchanged);
        }

        let old = self.prepare_row_keys(previous.keys.iter().cloned())?;
        let projected_encoded_bytes = self
            .encoded_key_bytes
            .saturating_sub(old.encoded_bytes)
            .saturating_add(prepared.encoded_bytes);
        let projected_structural_bytes = self
            .structural_key_bytes
            .saturating_sub(old.structural_bytes)
            .saturating_add(prepared.structural_bytes);
        let projected_entries = usize_to_u64(self.entries.len())
            .saturating_sub(usize_to_u64(old.encoded.len()))
            .saturating_add(usize_to_u64(prepared.encoded.len()));
        self.check_projected_resources(
            projected_entries,
            projected_encoded_bytes,
            projected_structural_bytes,
            prepared
                .encoded
                .iter()
                .map(|encoded| usize_to_u64(encoded.len()))
                .max()
                .unwrap_or(0),
        )?;
        let old_entries = old
            .encoded
            .iter()
            .map(|encoded| EntryOrderKey::row(encoded.clone(), previous.source_order, row_id))
            .collect::<BTreeSet<_>>();
        let new_entries = prepared
            .encoded
            .iter()
            .map(|encoded| EntryOrderKey::row(encoded.clone(), source_order, row_id))
            .collect::<BTreeSet<_>>();
        if old_entries
            .difference(&new_entries)
            .any(|entry| !self.entries.contains(entry))
        {
            return Err(AccessError::CorruptIndex(
                "row metadata had no corresponding index entry",
            ));
        }
        if new_entries
            .difference(&old_entries)
            .any(|entry| self.entries.contains(entry))
        {
            return Err(AccessError::CorruptIndex(
                "updated row collided with an existing index entry",
            ));
        }
        let removed = old_entries
            .difference(&new_entries)
            .cloned()
            .collect::<Vec<_>>();
        let inserted = new_entries
            .difference(&old_entries)
            .cloned()
            .collect::<Vec<_>>();
        for entry in &removed {
            self.entries.remove(entry);
        }
        for entry in &inserted {
            self.entries.insert(entry.clone());
        }
        self.rows.insert(
            row_id,
            RowState {
                keys: prepared.keys,
                source_order,
            },
        );
        self.encoded_key_bytes = projected_encoded_bytes;
        self.structural_key_bytes = projected_structural_bytes;
        self.metrics.updates = self.metrics.updates.saturating_add(1);
        self.metrics.physical_entry_updates = self
            .metrics
            .physical_entry_updates
            .saturating_add(usize_to_u64(removed.len().saturating_add(inserted.len())));
        Ok(MutationOutcome::Updated)
    }

    fn prepare_row_keys(
        &self,
        keys: impl IntoIterator<Item = StructuralKey>,
    ) -> Result<PreparedRowKeys, AccessError> {
        let mut unique = BTreeMap::new();
        for key in keys {
            let encoded = self.schema.encode(&key)?.into_bytes();
            unique.entry(encoded).or_insert(key);
        }
        let mut prepared = PreparedRowKeys {
            keys: Vec::with_capacity(unique.len()),
            encoded: Vec::with_capacity(unique.len()),
            encoded_bytes: 0,
            structural_bytes: 0,
        };
        for (encoded, key) in unique {
            prepared.encoded_bytes = prepared
                .encoded_bytes
                .saturating_add(usize_to_u64(encoded.len()));
            prepared.structural_bytes = prepared
                .structural_bytes
                .saturating_add(key.payload_bytes());
            prepared.encoded.push(encoded);
            prepared.keys.push(key);
        }
        Ok(prepared)
    }

    fn check_projected_resources(
        &self,
        entries: u64,
        encoded_key_bytes: u64,
        structural_key_bytes: u64,
        candidate_encoded_key_bytes: u64,
    ) -> Result<(), AccessError> {
        check_resource(IndexResource::Entries, entries, self.limits.max_entries)?;
        check_resource(
            IndexResource::EncodedKeyBytes,
            candidate_encoded_key_bytes,
            self.limits.max_encoded_key_bytes,
        )?;
        check_resource(
            IndexResource::PayloadBytes,
            retained_payload_bytes(entries, encoded_key_bytes, structural_key_bytes),
            self.limits.max_payload_bytes,
        )
    }

    pub fn cursor_for(&self, row_id: RowId) -> Option<CursorKey> {
        let state = self.rows.get(&row_id)?;
        let [key] = state.keys.as_slice() else {
            return None;
        };
        Some(CursorKey::new(key.clone(), state.source_order, row_id))
    }

    pub fn cursor_keys_for(&self, row_id: RowId) -> Vec<CursorKey> {
        self.rows
            .get(&row_id)
            .map(|state| {
                state
                    .keys
                    .iter()
                    .cloned()
                    .map(|key| CursorKey::new(key, state.source_order, row_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn into_integrity_task(self) -> OrderedIndexIntegrityTask {
        OrderedIndexIntegrityTask::new(self)
    }

    pub fn validate_integrity(&self) -> Result<IntegrityReport, AccessError> {
        OrderedIndexIntegrityTask::drain(self)
    }

    #[cfg(test)]
    pub(crate) fn test_remove_first_entry_without_accounting(&mut self) {
        if let Some(entry) = self.entries.iter().next().cloned() {
            self.entries.remove(&entry);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_increment_encoded_key_accounting(&mut self) {
        self.encoded_key_bytes = self.encoded_key_bytes.saturating_add(1);
    }

    #[cfg(test)]
    pub(crate) fn test_set_resource_limits_unchecked(&mut self, limits: IndexResourceLimits) {
        self.limits = limits;
    }

    #[cfg(test)]
    pub(crate) fn test_attach_drop_probe(&mut self, probe: std::sync::Arc<()>) {
        self.drop_probe = Some(probe);
    }

    #[cfg(test)]
    pub(crate) fn test_has_drop_probe(&self) -> bool {
        self.drop_probe.is_some()
    }

    /// Traverse the entire index. This is an explicit full scan, never a fallback.
    pub fn scan(&self, after: Option<&CursorKey>) -> Result<AccessStream<'_>, AccessError> {
        self.stream_from_bounds(Bound::Unbounded, Bound::Unbounded, after, true)
    }

    /// Start at the first ordered key for a bounded top-N access.
    ///
    /// Unlike [`Self::scan`], this is a compiler-proven seek used only when no
    /// residual predicate can force traversal beyond the requested result
    /// bound. It is therefore reported as an index seek, not a full scan.
    pub fn ordered_start(
        &self,
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError> {
        self.stream_from_bounds(Bound::Unbounded, Bound::Unbounded, after, false)
    }

    pub fn exact(
        &self,
        key: &StructuralKey,
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError> {
        let encoded = self.schema.encode(key)?.into_bytes();
        self.stream_from_bounds(
            Bound::Included(EntryOrderKey::before(encoded.clone())),
            Bound::Included(EntryOrderKey::after(encoded)),
            after,
            false,
        )
    }

    /// Traverse every key beginning with the supplied complete structural
    /// components. Equal prefix components retain the remaining directed key
    /// order and the stable source-order suffix.
    pub fn key_prefix(
        &self,
        leading: &[StructuralValue],
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError> {
        let encoded_prefix = self.schema.encode_structural_prefix(leading)?;
        let lower = Bound::Included(EntryOrderKey::before(encoded_prefix.clone()));
        let upper = lexicographic_successor(encoded_prefix)
            .map(EntryOrderKey::before)
            .map_or(Bound::Unbounded, Bound::Excluded);
        self.stream_from_bounds(lower, upper, after, false)
    }

    /// Traverse a finite range on the component immediately after `leading`.
    /// Bounds use the schema's directed order and include every trailing key
    /// component for an accepted range value.
    pub fn component_range(
        &self,
        leading: &[StructuralValue],
        lower: Bound<&StructuralValue>,
        upper: Bound<&StructuralValue>,
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError> {
        let encoded_lower = match lower {
            Bound::Included(value) => {
                Some((self.schema.encode_component_prefix(leading, value)?, true))
            }
            Bound::Excluded(value) => {
                Some((self.schema.encode_component_prefix(leading, value)?, false))
            }
            Bound::Unbounded => None,
        };
        let encoded_upper = match upper {
            Bound::Included(value) => {
                Some((self.schema.encode_component_prefix(leading, value)?, true))
            }
            Bound::Excluded(value) => {
                Some((self.schema.encode_component_prefix(leading, value)?, false))
            }
            Bound::Unbounded => None,
        };
        if encoded_lower
            .as_ref()
            .zip(encoded_upper.as_ref())
            .is_some_and(|((lower, lower_inclusive), (upper, upper_inclusive))| {
                lower > upper || (lower == upper && !(*lower_inclusive && *upper_inclusive))
            })
        {
            return Ok(self.empty_stream(after));
        }

        let lower = match encoded_lower {
            Some((encoded, true)) => Bound::Included(EntryOrderKey::before(encoded)),
            Some((encoded, false)) => {
                let Some(successor) = lexicographic_successor(encoded) else {
                    return Ok(self.empty_stream(after));
                };
                Bound::Included(EntryOrderKey::before(successor))
            }
            None => Bound::Unbounded,
        };
        let upper = match encoded_upper {
            Some((encoded, true)) => lexicographic_successor(encoded)
                .map(EntryOrderKey::before)
                .map_or(Bound::Unbounded, Bound::Excluded),
            Some((encoded, false)) => Bound::Excluded(EntryOrderKey::before(encoded)),
            None => Bound::Unbounded,
        };
        self.stream_from_bounds(lower, upper, after, false)
    }

    /// Traverse a range whose bounds are expressed in this schema's directed order.
    pub fn range<R>(
        &self,
        range: R,
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError>
    where
        R: RangeBounds<StructuralKey>,
    {
        let (lower, lower_key) = self.encode_lower_bound(range.start_bound())?;
        let (upper, upper_key) = self.encode_upper_bound(range.end_bound())?;
        if lower_key
            .as_ref()
            .zip(upper_key.as_ref())
            .is_some_and(|(lower, upper)| lower > upper)
        {
            return Err(AccessError::InvalidDirectedRange);
        }
        let full_scan = lower_key.is_none() && upper_key.is_none();
        self.stream_from_bounds(lower, upper, after, full_scan)
    }

    /// Traverse keys whose next Text component starts with `prefix`.
    pub fn text_prefix(
        &self,
        leading: &[StructuralValue],
        prefix: &str,
        after: Option<&CursorKey>,
    ) -> Result<AccessStream<'_>, AccessError> {
        let encoded_prefix = self.schema.encode_text_prefix(leading, prefix)?;
        let lower = Bound::Included(EntryOrderKey::before(encoded_prefix.clone()));
        let upper = lexicographic_successor(encoded_prefix)
            .map(EntryOrderKey::before)
            .map_or(Bound::Unbounded, Bound::Excluded);
        self.stream_from_bounds(lower, upper, after, false)
    }

    pub fn key_matches_prefix(
        &self,
        key: &StructuralKey,
        leading: &[StructuralValue],
    ) -> Result<bool, AccessError> {
        let encoded = self.schema.encode(key)?.into_bytes();
        let prefix = self.schema.encode_structural_prefix(leading)?;
        Ok(encoded.starts_with(&prefix))
    }

    pub fn key_matches_text_prefix(
        &self,
        key: &StructuralKey,
        leading: &[StructuralValue],
        prefix: &str,
    ) -> Result<bool, AccessError> {
        let encoded = self.schema.encode(key)?.into_bytes();
        let prefix = self.schema.encode_text_prefix(leading, prefix)?;
        Ok(encoded.starts_with(&prefix))
    }

    pub fn key_matches_component_range(
        &self,
        key: &StructuralKey,
        leading: &[StructuralValue],
        lower: Bound<&StructuralValue>,
        upper: Bound<&StructuralValue>,
    ) -> Result<bool, AccessError> {
        let encoded = self.schema.encode(key)?.into_bytes();
        let leading_prefix = self.schema.encode_structural_prefix(leading)?;
        if !encoded.starts_with(&leading_prefix) {
            return Ok(false);
        }

        let lower_matches = match lower {
            Bound::Included(value) => {
                encoded >= self.schema.encode_component_prefix(leading, value)?
            }
            Bound::Excluded(value) => self
                .schema
                .encode_component_prefix(leading, value)
                .map(lexicographic_successor)?
                .is_some_and(|bound| encoded >= bound),
            Bound::Unbounded => true,
        };
        let upper_matches = match upper {
            Bound::Included(value) => self
                .schema
                .encode_component_prefix(leading, value)
                .map(lexicographic_successor)?
                .is_none_or(|bound| encoded < bound),
            Bound::Excluded(value) => {
                encoded < self.schema.encode_component_prefix(leading, value)?
            }
            Bound::Unbounded => true,
        };
        Ok(lower_matches && upper_matches)
    }

    fn encode_lower_bound(
        &self,
        bound: Bound<&StructuralKey>,
    ) -> Result<(Bound<EntryOrderKey>, Option<Vec<u8>>), AccessError> {
        match bound {
            Bound::Included(key) => {
                let encoded = self.schema.encode(key)?.into_bytes();
                Ok((
                    Bound::Included(EntryOrderKey::before(encoded.clone())),
                    Some(encoded),
                ))
            }
            Bound::Excluded(key) => {
                let encoded = self.schema.encode(key)?.into_bytes();
                Ok((
                    Bound::Excluded(EntryOrderKey::after(encoded.clone())),
                    Some(encoded),
                ))
            }
            Bound::Unbounded => Ok((Bound::Unbounded, None)),
        }
    }

    fn encode_upper_bound(
        &self,
        bound: Bound<&StructuralKey>,
    ) -> Result<(Bound<EntryOrderKey>, Option<Vec<u8>>), AccessError> {
        match bound {
            Bound::Included(key) => {
                let encoded = self.schema.encode(key)?.into_bytes();
                Ok((
                    Bound::Included(EntryOrderKey::after(encoded.clone())),
                    Some(encoded),
                ))
            }
            Bound::Excluded(key) => {
                let encoded = self.schema.encode(key)?.into_bytes();
                Ok((
                    Bound::Excluded(EntryOrderKey::before(encoded.clone())),
                    Some(encoded),
                ))
            }
            Bound::Unbounded => Ok((Bound::Unbounded, None)),
        }
    }

    fn stream_from_bounds(
        &self,
        mut lower: Bound<EntryOrderKey>,
        upper: Bound<EntryOrderKey>,
        after: Option<&CursorKey>,
        full_scan: bool,
    ) -> Result<AccessStream<'_>, AccessError> {
        if let Some(cursor) = after {
            let encoded = self.schema.encode(cursor.key())?.into_bytes();
            let cursor_bound = Bound::Excluded(EntryOrderKey::row(
                encoded,
                cursor.source_order(),
                cursor.row_id(),
            ));
            lower = stronger_lower_bound(lower, cursor_bound);
        }
        let empty = bounds_are_empty(&lower, &upper);
        let entries = if empty {
            BaseEntries::Empty
        } else {
            BaseEntries::Range(self.entries.range((lower, upper)))
        };
        Ok(AccessStream::base(
            self.plan_id,
            self.schema.clone(),
            BaseSource {
                entries,
                rows: &self.rows,
                schema: &self.schema,
                seek_accounted: false,
                cursor_seek: after.is_some(),
                full_scan,
                last_encoded_key: None,
            },
        ))
    }

    fn empty_stream(&self, after: Option<&CursorKey>) -> AccessStream<'_> {
        AccessStream::base(
            self.plan_id,
            self.schema.clone(),
            BaseSource {
                entries: BaseEntries::Empty,
                rows: &self.rows,
                schema: &self.schema,
                seek_accounted: false,
                cursor_seek: after.is_some(),
                full_scan: false,
                last_encoded_key: None,
            },
        )
    }
}

fn stronger_lower_bound(
    current: Bound<EntryOrderKey>,
    candidate: Bound<EntryOrderKey>,
) -> Bound<EntryOrderKey> {
    match (&current, &candidate) {
        (Bound::Unbounded, _) => candidate,
        (_, Bound::Unbounded) => current,
        (
            Bound::Included(left) | Bound::Excluded(left),
            Bound::Included(right) | Bound::Excluded(right),
        ) => match left.cmp(right) {
            Ordering::Less => candidate,
            Ordering::Greater => current,
            Ordering::Equal => {
                if matches!(current, Bound::Excluded(_)) || matches!(candidate, Bound::Excluded(_))
                {
                    Bound::Excluded(left.clone())
                } else {
                    Bound::Included(left.clone())
                }
            }
        },
    }
}

fn bounds_are_empty(lower: &Bound<EntryOrderKey>, upper: &Bound<EntryOrderKey>) -> bool {
    let (Some((lower_value, lower_inclusive)), Some((upper_value, upper_inclusive))) =
        (bound_value(lower), bound_value(upper))
    else {
        return false;
    };
    match lower_value.cmp(upper_value) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => !(lower_inclusive && upper_inclusive),
    }
}

fn bound_value(bound: &Bound<EntryOrderKey>) -> Option<(&EntryOrderKey, bool)> {
    match bound {
        Bound::Included(value) => Some((value, true)),
        Bound::Excluded(value) => Some((value, false)),
        Bound::Unbounded => None,
    }
}

impl OrderedIndexIntegrityTask {
    fn new(index: OrderedIndex) -> Self {
        let validation = OrderedIndexIntegrityValidation::new(&index);
        Self {
            index: Some(index),
            validation,
            failed: false,
        }
    }

    fn drain(index: &OrderedIndex) -> Result<IntegrityReport, AccessError> {
        let mut validation = OrderedIndexIntegrityValidation::new(index);
        loop {
            match validation.poll(index, usize::MAX)? {
                OrderedIndexIntegrityValidationPoll::Pending => {}
                OrderedIndexIntegrityValidationPoll::Ready(report) => return Ok(report),
            }
        }
    }

    pub fn progress(&self) -> OrderedIndexIntegrityProgress {
        self.validation.progress()
    }

    pub fn poll(&mut self, max_steps: usize) -> Result<OrderedIndexIntegrityPoll, AccessError> {
        if max_steps == 0 {
            return Err(AccessError::InvalidIntegrityPollBudget);
        }
        if self.failed || self.index.is_none() {
            return Err(AccessError::CompletedIntegrityTask);
        }

        let validation_poll = {
            let index = self
                .index
                .as_ref()
                .expect("active integrity task owns its index");
            self.validation.poll(index, max_steps)
        };
        match validation_poll {
            Ok(OrderedIndexIntegrityValidationPoll::Pending) => Ok(
                OrderedIndexIntegrityPoll::Pending(self.validation.progress()),
            ),
            Ok(OrderedIndexIntegrityValidationPoll::Ready(report)) => {
                let index = self
                    .index
                    .take()
                    .expect("ready integrity task owns its index");
                Ok(OrderedIndexIntegrityPoll::Ready(
                    OrderedIndexIntegrityResult { index, report },
                ))
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
}

impl OrderedIndexIntegrityValidation {
    fn new(index: &OrderedIndex) -> Self {
        Self {
            phase: OrderedIndexIntegrityValidationPhase::Rows,
            initialized: false,
            row_cursor: None,
            entry_cursor: None,
            accounting_entry_cursor: None,
            accounting_row_cursor: None,
            completed_steps: 0,
            rows_checked: 0,
            entries_checked: 0,
            accounting_entries_checked: 0,
            accounting_rows_checked: 0,
            total_rows: usize_to_u64(index.rows.len()),
            total_entries: usize_to_u64(index.entries.len()),
            encoded_key_bytes: 0,
            structural_key_bytes: 0,
            expected_entries: 0,
        }
    }

    fn progress(&self) -> OrderedIndexIntegrityProgress {
        let phase = match self.phase {
            OrderedIndexIntegrityValidationPhase::Rows => OrderedIndexIntegrityPhase::Rows,
            OrderedIndexIntegrityValidationPhase::Entries => OrderedIndexIntegrityPhase::Entries,
            OrderedIndexIntegrityValidationPhase::AccountingEntries
            | OrderedIndexIntegrityValidationPhase::AccountingRows => {
                OrderedIndexIntegrityPhase::Accounting
            }
            OrderedIndexIntegrityValidationPhase::Complete => OrderedIndexIntegrityPhase::Complete,
        };
        OrderedIndexIntegrityProgress {
            phase,
            completed_steps: self.completed_steps,
            rows_checked: self.rows_checked,
            entries_checked: self.entries_checked,
            accounting_entries_checked: self.accounting_entries_checked,
            accounting_rows_checked: self.accounting_rows_checked,
            total_rows: self.total_rows,
            total_entries: self.total_entries,
        }
    }

    fn poll(
        &mut self,
        index: &OrderedIndex,
        max_steps: usize,
    ) -> Result<OrderedIndexIntegrityValidationPoll, AccessError> {
        if !self.initialized {
            check_resource(
                IndexResource::Entries,
                self.total_entries,
                index.limits.max_entries,
            )?;
            self.initialized = true;
        }

        let mut remaining = max_steps;
        loop {
            match self.phase {
                OrderedIndexIntegrityValidationPhase::Rows => {
                    let Some((row_id, state)) = next_integrity_row(index, self.row_cursor) else {
                        if self.expected_entries != self.total_entries {
                            return Err(AccessError::CorruptIndex(
                                "logical row key count and index entry count differ",
                            ));
                        }
                        self.phase = OrderedIndexIntegrityValidationPhase::Entries;
                        continue;
                    };
                    for key in &state.keys {
                        let encoded = index.schema.encode(key)?.into_bytes();
                        if !index.entries.contains(&EntryOrderKey::row(
                            encoded,
                            state.source_order,
                            row_id,
                        )) {
                            return Err(AccessError::CorruptIndex(
                                "logical row has no corresponding index entry",
                            ));
                        }
                    }
                    self.expected_entries = self
                        .expected_entries
                        .saturating_add(usize_to_u64(state.keys.len()));
                    self.row_cursor = Some(row_id);
                    self.rows_checked = self.rows_checked.saturating_add(1);
                    self.finish_step(&mut remaining);
                }
                OrderedIndexIntegrityValidationPhase::Entries => {
                    let Some(entry) = next_integrity_entry(index, self.entry_cursor.as_ref())
                    else {
                        self.phase = OrderedIndexIntegrityValidationPhase::AccountingEntries;
                        continue;
                    };
                    let Some((source_order, row_id)) = entry.row_parts() else {
                        return Err(AccessError::CorruptIndex(
                            "index contains a synthetic boundary entry",
                        ));
                    };
                    let Some(state) = index.rows.get(&row_id) else {
                        return Err(AccessError::CorruptIndex(
                            "index entry references an unknown row identity",
                        ));
                    };
                    let key_matches = state.keys.iter().any(|key| {
                        index
                            .schema
                            .encode(key)
                            .is_ok_and(|encoded| encoded.as_bytes() == entry.encoded_key.as_slice())
                    });
                    if !key_matches || source_order != state.source_order {
                        return Err(AccessError::CorruptIndex(
                            "index entry disagrees with logical row metadata",
                        ));
                    }
                    self.entry_cursor = Some(entry.clone());
                    self.entries_checked = self.entries_checked.saturating_add(1);
                    self.finish_step(&mut remaining);
                }
                OrderedIndexIntegrityValidationPhase::AccountingEntries => {
                    let Some(entry) =
                        next_integrity_entry(index, self.accounting_entry_cursor.as_ref())
                    else {
                        self.phase = OrderedIndexIntegrityValidationPhase::AccountingRows;
                        continue;
                    };
                    let entry_bytes = usize_to_u64(entry.encoded_key.len());
                    check_resource(
                        IndexResource::EncodedKeyBytes,
                        entry_bytes,
                        index.limits.max_encoded_key_bytes,
                    )?;
                    self.encoded_key_bytes = self.encoded_key_bytes.saturating_add(entry_bytes);
                    self.accounting_entry_cursor = Some(entry.clone());
                    self.accounting_entries_checked =
                        self.accounting_entries_checked.saturating_add(1);
                    self.finish_step(&mut remaining);
                }
                OrderedIndexIntegrityValidationPhase::AccountingRows => {
                    let Some((row_id, state)) =
                        next_integrity_row(index, self.accounting_row_cursor)
                    else {
                        let report = self.finish(index)?;
                        self.phase = OrderedIndexIntegrityValidationPhase::Complete;
                        return Ok(OrderedIndexIntegrityValidationPoll::Ready(report));
                    };
                    self.structural_key_bytes = self.structural_key_bytes.saturating_add(
                        state.keys.iter().fold(0_u64, |total, key| {
                            total.saturating_add(key.payload_bytes())
                        }),
                    );
                    self.accounting_row_cursor = Some(row_id);
                    self.accounting_rows_checked = self.accounting_rows_checked.saturating_add(1);
                    self.finish_step(&mut remaining);
                }
                OrderedIndexIntegrityValidationPhase::Complete => {
                    unreachable!("completed integrity validation cannot be polled")
                }
            }

            if remaining == 0 {
                return Ok(OrderedIndexIntegrityValidationPoll::Pending);
            }
        }
    }

    fn finish(&self, index: &OrderedIndex) -> Result<IntegrityReport, AccessError> {
        if self.encoded_key_bytes != index.encoded_key_bytes
            || self.structural_key_bytes != index.structural_key_bytes
        {
            return Err(AccessError::CorruptIndex(
                "retained key payload accounting disagrees with index contents",
            ));
        }
        check_resource(
            IndexResource::PayloadBytes,
            retained_payload_bytes(
                self.total_entries,
                self.encoded_key_bytes,
                self.structural_key_bytes,
            ),
            index.limits.max_payload_bytes,
        )?;
        Ok(IntegrityReport {
            logical_rows: self.total_rows,
            index_entries: self.total_entries,
            encoded_key_bytes: self.encoded_key_bytes,
            structural_key_bytes: self.structural_key_bytes,
            source_order_bytes: self.total_entries.saturating_mul(IDENTITY_BYTES),
            row_identity_bytes: self.total_entries.saturating_mul(IDENTITY_BYTES),
        })
    }

    fn finish_step(&mut self, remaining: &mut usize) {
        *remaining -= 1;
        self.completed_steps = self.completed_steps.saturating_add(1);
    }
}

fn next_integrity_row(index: &OrderedIndex, after: Option<RowId>) -> Option<(RowId, &RowState)> {
    match after {
        Some(after) => index
            .rows
            .range((Bound::Excluded(after), Bound::Unbounded))
            .next()
            .map(|(row_id, state)| (*row_id, state)),
        None => index
            .rows
            .iter()
            .next()
            .map(|(row_id, state)| (*row_id, state)),
    }
}

fn next_integrity_entry<'a>(
    index: &'a OrderedIndex,
    after: Option<&EntryOrderKey>,
) -> Option<&'a EntryOrderKey> {
    match after {
        Some(after) => index
            .entries
            .range((Bound::Excluded(after), Bound::Unbounded))
            .next(),
        None => index.entries.iter().next(),
    }
}

enum BaseEntries<'a> {
    Range(btree_set::Range<'a, EntryOrderKey>),
    Empty,
}

struct BaseSource<'a> {
    entries: BaseEntries<'a>,
    rows: &'a BTreeMap<RowId, RowState>,
    schema: &'a KeySchema,
    seek_accounted: bool,
    cursor_seek: bool,
    full_scan: bool,
    last_encoded_key: Option<Vec<u8>>,
}

impl CursorSource for BaseSource<'_> {
    fn pull(&mut self, work: &mut WorkTracker) -> Result<Option<Candidate>, AccessError> {
        if !self.seek_accounted {
            work.begin_seek(self.cursor_seek, self.full_scan)?;
            self.seek_accounted = true;
        }
        let entry = match &mut self.entries {
            BaseEntries::Range(entries) => entries.next(),
            BaseEntries::Empty => None,
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        let Some((source_order, row_id)) = entry.row_parts() else {
            return Err(AccessError::CorruptIndex(
                "access traversal reached a synthetic boundary entry",
            ));
        };
        if self.last_encoded_key.as_deref() != Some(entry.encoded_key.as_slice()) {
            work.visit_key()?;
            self.last_encoded_key = Some(entry.encoded_key.clone());
        }
        work.visit_candidate()?;
        let state = self.rows.get(&row_id).ok_or(AccessError::CorruptIndex(
            "index entry references an unknown row identity",
        ))?;
        if state.source_order != source_order {
            return Err(AccessError::CorruptIndex(
                "index entry has a stale source-order token",
            ));
        }
        let key = state
            .keys
            .iter()
            .find(|key| {
                self.schema
                    .encode(key)
                    .is_ok_and(|encoded| encoded.as_bytes() == entry.encoded_key.as_slice())
            })
            .cloned()
            .ok_or(AccessError::CorruptIndex(
                "index entry has no matching logical row key",
            ))?;
        Ok(Some(Candidate {
            order: entry.clone(),
            item: AccessItem {
                key,
                source_order,
                row_id,
            },
        }))
    }
}

#[derive(Clone)]
struct Candidate {
    order: EntryOrderKey,
    item: AccessItem,
}

trait CursorSource {
    fn pull(&mut self, work: &mut WorkTracker) -> Result<Option<Candidate>, AccessError>;
}

/// A lazy seekable stream in one compatible directed key order.
pub struct AccessStream<'a> {
    plan_id: IndexPlanId,
    schema: KeySchema,
    source: Box<dyn CursorSource + 'a>,
    terminal: bool,
}

impl fmt::Debug for AccessStream<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessStream")
            .field("plan_id", &self.plan_id)
            .field("schema", &self.schema)
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

impl<'a> AccessStream<'a> {
    fn base(plan_id: IndexPlanId, schema: KeySchema, source: BaseSource<'a>) -> Self {
        Self {
            plan_id,
            schema,
            source: Box::new(source),
            terminal: false,
        }
    }

    pub const fn schema(&self) -> &KeySchema {
        &self.schema
    }

    pub const fn plan_id(&self) -> IndexPlanId {
        self.plan_id
    }

    /// Remove compiler-fixed leading key components from stream ordering.
    ///
    /// Expanded membership indexes use the fixed token prefix only for the
    /// physical seek. Union/intersection and page continuation must compare
    /// the remaining semantic order plus source-order and row identity.
    pub fn project_key_prefix(self, component_count: usize) -> Result<Self, AccessError> {
        if component_count == 0 {
            return Ok(self);
        }
        let components = self
            .schema
            .components()
            .get(component_count..)
            .ok_or(AccessError::InvalidProjectionPrefix {
                actual: component_count,
                maximum: self.schema.components().len(),
            })?
            .to_vec();
        let schema = KeySchema::new(components)?;
        Ok(Self {
            plan_id: self.plan_id,
            source: Box::new(ProjectedSource {
                source: self.source,
                schema: schema.clone(),
                removed_components: component_count,
            }),
            schema,
            terminal: self.terminal,
        })
    }

    pub fn next(&mut self, work: &mut WorkTracker) -> Result<Option<AccessItem>, AccessError> {
        if self.terminal {
            return Ok(None);
        }
        match self.source.pull(work) {
            Ok(Some(candidate)) => {
                if let Err(error) = work.return_row() {
                    self.terminal = true;
                    return Err(error.into());
                }
                Ok(Some(candidate.item))
            }
            Ok(None) => {
                self.terminal = true;
                Ok(None)
            }
            Err(error) => {
                self.terminal = true;
                Err(error)
            }
        }
    }

    pub fn union(streams: Vec<Self>) -> Result<Self, AccessError> {
        let (plan_id, schema) = compatible_order(&streams, "union")?;
        let branches = streams
            .into_iter()
            .map(|stream| Branch {
                source: stream.source,
                head: None,
                exhausted: stream.terminal,
            })
            .collect();
        Ok(Self {
            plan_id,
            schema,
            source: Box::new(UnionSource { branches }),
            terminal: false,
        })
    }

    pub fn intersection(streams: Vec<Self>) -> Result<Self, AccessError> {
        let (plan_id, schema) = compatible_order(&streams, "intersection")?;
        let branches = streams
            .into_iter()
            .map(|stream| Branch {
                source: stream.source,
                head: None,
                exhausted: stream.terminal,
            })
            .collect();
        Ok(Self {
            plan_id,
            schema,
            source: Box::new(IntersectionSource { branches }),
            terminal: false,
        })
    }
}

fn compatible_order(
    streams: &[AccessStream<'_>],
    operation: &'static str,
) -> Result<(IndexPlanId, KeySchema), AccessError> {
    if streams.is_empty() || streams.len() > MAX_COMPOSITE_BRANCHES {
        return Err(AccessError::InvalidBranchCount {
            operation,
            actual: streams.len(),
            maximum: MAX_COMPOSITE_BRANCHES,
        });
    }
    let plan_id = streams[0].plan_id;
    let schema = streams[0].schema.clone();
    if streams
        .iter()
        .skip(1)
        .any(|stream| stream.plan_id != plan_id || stream.schema != schema)
    {
        return Err(AccessError::IncompatibleStreamSchemas { operation });
    }
    Ok((plan_id, schema))
}

struct Branch<'a> {
    source: Box<dyn CursorSource + 'a>,
    head: Option<Candidate>,
    exhausted: bool,
}

impl Branch<'_> {
    fn fill(&mut self, work: &mut WorkTracker) -> Result<(), AccessError> {
        if self.head.is_some() || self.exhausted {
            return Ok(());
        }
        work.poll_branch()?;
        self.head = self.source.pull(work)?;
        self.exhausted = self.head.is_none();
        Ok(())
    }
}

struct UnionSource<'a> {
    branches: Vec<Branch<'a>>,
}

struct ProjectedSource<'a> {
    source: Box<dyn CursorSource + 'a>,
    schema: KeySchema,
    removed_components: usize,
}

impl CursorSource for ProjectedSource<'_> {
    fn pull(&mut self, work: &mut WorkTracker) -> Result<Option<Candidate>, AccessError> {
        let Some(candidate) = self.source.pull(work)? else {
            return Ok(None);
        };
        let parts = candidate
            .item
            .key
            .parts()
            .get(self.removed_components..)
            .ok_or(AccessError::CorruptIndex(
                "projected stream key has fewer components than its schema",
            ))?
            .to_vec();
        let key = StructuralKey::new(parts)?;
        let encoded = self.schema.encode(&key)?.into_bytes();
        let order = EntryOrderKey::row(encoded, candidate.item.source_order, candidate.item.row_id);
        Ok(Some(Candidate {
            order,
            item: AccessItem {
                key,
                source_order: candidate.item.source_order,
                row_id: candidate.item.row_id,
            },
        }))
    }
}

impl CursorSource for UnionSource<'_> {
    fn pull(&mut self, work: &mut WorkTracker) -> Result<Option<Candidate>, AccessError> {
        for branch in &mut self.branches {
            branch.fill(work)?;
        }
        let Some(minimum) = self
            .branches
            .iter()
            .filter_map(|branch| branch.head.as_ref())
            .min_by(|left, right| left.order.cmp(&right.order))
            .cloned()
        else {
            return Ok(None);
        };

        let mut duplicate_count = 0_u64;
        for branch in &mut self.branches {
            if branch
                .head
                .as_ref()
                .is_some_and(|candidate| candidate.order == minimum.order)
            {
                branch.head = None;
                duplicate_count = duplicate_count.saturating_add(1);
            }
        }
        work.skip_union_duplicates(duplicate_count.saturating_sub(1));
        Ok(Some(minimum))
    }
}

struct IntersectionSource<'a> {
    branches: Vec<Branch<'a>>,
}

impl CursorSource for IntersectionSource<'_> {
    fn pull(&mut self, work: &mut WorkTracker) -> Result<Option<Candidate>, AccessError> {
        loop {
            for branch in &mut self.branches {
                branch.fill(work)?;
            }
            if self.branches.iter().any(|branch| branch.exhausted) {
                return Ok(None);
            }
            let maximum = self
                .branches
                .iter()
                .filter_map(|branch| branch.head.as_ref())
                .max_by(|left, right| left.order.cmp(&right.order))
                .expect("non-empty compatible branches have heads")
                .order
                .clone();
            if self.branches.iter().all(|branch| {
                branch
                    .head
                    .as_ref()
                    .is_some_and(|candidate| candidate.order == maximum)
            }) {
                let result = self.branches[0]
                    .head
                    .take()
                    .expect("all branches have the same head");
                for branch in self.branches.iter_mut().skip(1) {
                    branch.head = None;
                }
                return Ok(Some(result));
            }
            for branch in &mut self.branches {
                if branch
                    .head
                    .as_ref()
                    .is_some_and(|candidate| candidate.order < maximum)
                {
                    branch.head = None;
                    work.skip_intersection_candidate();
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum AccessError {
    Key(KeyError),
    DuplicateRow(RowId),
    UnknownRow(RowId),
    InvalidDirectedRange,
    InvalidBranchCount {
        operation: &'static str,
        actual: usize,
        maximum: usize,
    },
    IncompatibleStreamSchemas {
        operation: &'static str,
    },
    InvalidProjectionPrefix {
        actual: usize,
        maximum: usize,
    },
    ResourceLimitExceeded {
        resource: IndexResource,
        attempted: u64,
        maximum: u64,
    },
    InvalidIntegrityPollBudget,
    CompletedIntegrityTask,
    CorruptIndex(&'static str),
    WorkLimitExceeded(WorkLimitExceeded),
}

impl fmt::Display for AccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(error) => error.fmt(formatter),
            Self::DuplicateRow(_) => formatter.write_str("row identity already exists in index"),
            Self::UnknownRow(_) => formatter.write_str("row identity does not exist in index"),
            Self::InvalidDirectedRange => {
                formatter.write_str("range start follows range end in directed index order")
            }
            Self::InvalidBranchCount {
                operation,
                actual,
                maximum,
            } => write!(
                formatter,
                "{operation} has {actual} branches; expected 1..={maximum}"
            ),
            Self::IncompatibleStreamSchemas { operation } => {
                write!(
                    formatter,
                    "{operation} streams use incompatible key schemas"
                )
            }
            Self::InvalidProjectionPrefix { actual, maximum } => write!(
                formatter,
                "ordered stream projection removes {actual} key components; maximum is {maximum}"
            ),
            Self::ResourceLimitExceeded {
                resource,
                attempted,
                maximum,
            } => write!(
                formatter,
                "ordered index {resource} limit exceeded: attempted {attempted}, maximum {maximum}"
            ),
            Self::InvalidIntegrityPollBudget => {
                formatter.write_str("ordered index integrity poll requires a positive step budget")
            }
            Self::CompletedIntegrityTask => {
                formatter.write_str("completed ordered index integrity task was polled again")
            }
            Self::CorruptIndex(message) => write!(formatter, "corrupt ordered index: {message}"),
            Self::WorkLimitExceeded(error) => error.fmt(formatter),
        }
    }
}

impl fmt::Display for IndexResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Entries => "entry count",
            Self::EncodedKeyBytes => "encoded key bytes per entry",
            Self::PayloadBytes => "retained payload bytes",
        })
    }
}

fn retained_payload_bytes(entries: u64, encoded_key_bytes: u64, structural_key_bytes: u64) -> u64 {
    encoded_key_bytes
        .saturating_add(structural_key_bytes)
        .saturating_add(entries.saturating_mul(IDENTITY_BYTES.saturating_mul(2)))
}

fn check_resource(
    resource: IndexResource,
    attempted: u64,
    maximum: u64,
) -> Result<(), AccessError> {
    if attempted > maximum {
        Err(AccessError::ResourceLimitExceeded {
            resource,
            attempted,
            maximum,
        })
    } else {
        Ok(())
    }
}

impl Error for AccessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Key(error) => Some(error),
            Self::WorkLimitExceeded(error) => Some(error),
            _ => None,
        }
    }
}

impl From<KeyError> for AccessError {
    fn from(value: KeyError) -> Self {
        Self::Key(value)
    }
}

impl From<WorkLimitExceeded> for AccessError {
    fn from(value: WorkLimitExceeded) -> Self {
        Self::WorkLimitExceeded(value)
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
