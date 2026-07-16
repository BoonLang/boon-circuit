//! Deterministic indexed queries over canonical Boon structural data.

#![forbid(unsafe_code)]

use boon_data::{FiniteReal, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::ops::Bound;
use std::time::Instant;

pub const MAX_QUERY_LIMIT: usize = 10_000;
pub const MAX_INDEX_KEY_PARTS: usize = 8;
pub const MAX_KEYS_PER_ROW: usize = 256;
pub const MAX_QUERY_CANDIDATES: usize = 100_000;

macro_rules! digest_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub [u8; 32]);
    };
}

digest_id!(CollectionId);
digest_id!(IndexId);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum KeyPart {
    Bool(bool),
    Number(FiniteReal),
    Text(String),
    Tag(String),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IndexKey(pub Vec<KeyPart>);

impl IndexKey {
    pub fn new(parts: Vec<KeyPart>) -> Result<Self, QueryError> {
        if parts.is_empty() || parts.len() > MAX_INDEX_KEY_PARTS {
            return Err(QueryError::InvalidPlan(format!(
                "index key must contain 1..={MAX_INDEX_KEY_PARTS} parts"
            )));
        }
        Ok(Self(parts))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RowId(pub String);

impl RowId {
    pub fn new(value: impl Into<String>) -> Result<Self, QueryError> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 {
            return Err(QueryError::InvalidRow(
                "row identity must contain 1..=1024 bytes".to_owned(),
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextNormalization {
    #[default]
    Exact,
    TrimLowercase,
    Tokens,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexFieldPlan {
    pub path: Vec<String>,
    #[serde(default)]
    pub text_normalization: TextNormalization,
    #[serde(default)]
    pub multi_value: bool,
}

impl IndexFieldPlan {
    pub fn field(name: impl Into<String>) -> Self {
        Self {
            path: vec![name.into()],
            text_normalization: TextNormalization::Exact,
            multi_value: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectionPlan {
    pub id: CollectionId,
    pub name: String,
    pub row_id_path: Vec<String>,
    pub schema_hash: [u8; 32],
}

impl CollectionPlan {
    pub fn new(name: impl Into<String>, row_id_path: Vec<String>) -> Result<Self, QueryError> {
        let schema_hash = digest(&("boon.collection.schema.v1", &row_id_path))?;
        Self::new_with_schema_hash(name, row_id_path, schema_hash)
    }

    pub fn new_with_schema_hash(
        name: impl Into<String>,
        row_id_path: Vec<String>,
        schema_hash: [u8; 32],
    ) -> Result<Self, QueryError> {
        let name = canonical_name(name.into(), "collection")?;
        validate_path(&row_id_path, "row identity")?;
        let id = CollectionId(digest(&(
            "boon.collection.v2",
            &name,
            &row_id_path,
            schema_hash,
        ))?);
        Ok(Self {
            id,
            name,
            row_id_path,
            schema_hash,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexOrder {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexPlan {
    pub id: IndexId,
    pub collection: CollectionId,
    pub name: String,
    pub fields: Vec<IndexFieldPlan>,
    pub unique: bool,
    #[serde(default)]
    pub order: IndexOrder,
}

impl IndexPlan {
    pub fn new(
        collection: CollectionId,
        name: impl Into<String>,
        fields: Vec<IndexFieldPlan>,
        unique: bool,
    ) -> Result<Self, QueryError> {
        Self::new_with_order(collection, name, fields, unique, IndexOrder::Ascending)
    }

    pub fn new_with_order(
        collection: CollectionId,
        name: impl Into<String>,
        fields: Vec<IndexFieldPlan>,
        unique: bool,
        order: IndexOrder,
    ) -> Result<Self, QueryError> {
        let name = canonical_name(name.into(), "index")?;
        if fields.is_empty() || fields.len() > MAX_INDEX_KEY_PARTS {
            return Err(QueryError::InvalidPlan(format!(
                "index `{name}` must project 1..={MAX_INDEX_KEY_PARTS} fields"
            )));
        }
        let expanding = fields
            .iter()
            .filter(|field| {
                field.multi_value || field.text_normalization == TextNormalization::Tokens
            })
            .count();
        if expanding > 1 {
            return Err(QueryError::InvalidPlan(format!(
                "index `{name}` may contain at most one expanding field"
            )));
        }
        for field in &fields {
            validate_path(&field.path, "index field")?;
        }
        let id = IndexId(digest(&(
            "boon.index.v1",
            collection,
            &name,
            &fields,
            unique,
            order,
        ))?);
        Ok(Self {
            id,
            collection,
            name,
            fields,
            unique,
            order,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuerySelection {
    Exact {
        key: IndexKey,
    },
    TextPrefix {
        leading: Vec<KeyPart>,
        prefix: String,
    },
    Range {
        lower: Option<IndexKey>,
        lower_inclusive: bool,
        upper: Option<IndexKey>,
        upper_inclusive: bool,
    },
    Union {
        selections: Vec<QuerySelection>,
    },
    Intersection {
        selections: Vec<QuerySelection>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResidualPredicate {
    FieldEqual {
        path: Vec<String>,
        value: Value,
    },
    TextContains {
        path: Vec<String>,
        needle: String,
    },
    NumberRange {
        path: Vec<String>,
        minimum: Option<FiniteReal>,
        maximum: Option<FiniteReal>,
    },
    Wgs84Radius {
        latitude_path: Vec<String>,
        longitude_path: Vec<String>,
        center_latitude: FiniteReal,
        center_longitude: FiniteReal,
        radius_meters: FiniteReal,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryPlan {
    pub index: IndexId,
    pub selection: QuerySelection,
    pub residual: Vec<ResidualPredicate>,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorToken>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CursorToken(Vec<u8>);

impl CursorToken {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, QueryError> {
        if bytes.len() > 16 * 1024 {
            return Err(QueryError::InvalidCursor(
                "cursor exceeds 16 KiB".to_owned(),
            ));
        }
        Ok(Self(bytes))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueryMetrics {
    pub index: Option<IndexId>,
    pub ranges: usize,
    pub keys_visited: usize,
    pub rows_examined: usize,
    pub residual_evaluations: usize,
    pub candidates_selected: usize,
    pub returned: usize,
    pub full_scans: usize,
    pub elapsed_nanos: u64,
}

impl PartialEq for QueryMetrics {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
            && self.ranges == other.ranges
            && self.keys_visited == other.keys_visited
            && self.rows_examined == other.rows_examined
            && self.residual_evaluations == other.residual_evaluations
            && self.candidates_selected == other.candidates_selected
            && self.returned == other.returned
            && self.full_scans == other.full_scans
    }
}

impl Eq for QueryMetrics {}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryRow {
    pub id: RowId,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
    pub next_cursor: Option<CursorToken>,
    pub metrics: QueryMetrics,
}

#[derive(Clone, Debug)]
struct IndexState {
    plan: IndexPlan,
    entries: BTreeMap<IndexKey, BTreeSet<RowId>>,
}

#[derive(Clone, Debug)]
pub struct Collection {
    plan: CollectionPlan,
    rows: BTreeMap<RowId, Value>,
    indexes: BTreeMap<IndexId, IndexState>,
    epoch: u64,
}

impl Collection {
    pub fn new(plan: CollectionPlan, indexes: Vec<IndexPlan>) -> Result<Self, QueryError> {
        let mut states = BTreeMap::new();
        for index in indexes {
            if index.collection != plan.id {
                return Err(QueryError::InvalidPlan(format!(
                    "index `{}` belongs to another collection",
                    index.name
                )));
            }
            if states
                .insert(
                    index.id,
                    IndexState {
                        plan: index,
                        entries: BTreeMap::new(),
                    },
                )
                .is_some()
            {
                return Err(QueryError::InvalidPlan(
                    "duplicate index identity".to_owned(),
                ));
            }
        }
        if states.is_empty() {
            return Err(QueryError::InvalidPlan(
                "collection requires at least one declared index".to_owned(),
            ));
        }
        Ok(Self {
            plan,
            rows: BTreeMap::new(),
            indexes: states,
            epoch: 0,
        })
    }

    pub fn plan(&self) -> &CollectionPlan {
        &self.plan
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn upsert(&mut self, value: Value) -> Result<RowId, QueryError> {
        let row_id = row_id(&value, &self.plan.row_id_path)?;
        let new_keys = self
            .indexes
            .iter()
            .map(|(id, index)| Ok((*id, project_index_keys(&index.plan, &value)?)))
            .collect::<Result<BTreeMap<_, _>, QueryError>>()?;

        for (id, keys) in &new_keys {
            let index = &self.indexes[id];
            if !index.plan.unique {
                continue;
            }
            for key in keys {
                if index
                    .entries
                    .get(key)
                    .is_some_and(|rows| rows.iter().any(|candidate| candidate != &row_id))
                {
                    return Err(QueryError::UniqueConflict {
                        index: index.plan.name.clone(),
                        key: key.clone(),
                    });
                }
            }
        }

        if let Some(previous) = self.rows.get(&row_id) {
            for index in self.indexes.values_mut() {
                for key in project_index_keys(&index.plan, previous)? {
                    remove_index_row(&mut index.entries, &key, &row_id);
                }
            }
        }
        for (id, keys) in new_keys {
            let index = self.indexes.get_mut(&id).expect("prevalidated index");
            for key in keys {
                index.entries.entry(key).or_default().insert(row_id.clone());
            }
        }
        self.rows.insert(row_id.clone(), value);
        self.epoch = self.epoch.checked_add(1).ok_or(QueryError::EpochOverflow)?;
        Ok(row_id)
    }

    pub fn remove(&mut self, row_id: &RowId) -> Result<Option<Value>, QueryError> {
        let Some(previous) = self.rows.remove(row_id) else {
            return Ok(None);
        };
        for index in self.indexes.values_mut() {
            for key in project_index_keys(&index.plan, &previous)? {
                remove_index_row(&mut index.entries, &key, row_id);
            }
        }
        self.epoch = self.epoch.checked_add(1).ok_or(QueryError::EpochOverflow)?;
        Ok(Some(previous))
    }

    pub fn query(&self, plan: &QueryPlan) -> Result<QueryResult, QueryError> {
        let started = Instant::now();
        validate_query(plan)?;
        let index = self
            .indexes
            .get(&plan.index)
            .ok_or(QueryError::UnknownIndex(plan.index))?;
        let normalized = normalize_query_plan(plan, &index.plan)?;
        let fingerprint = query_fingerprint(&normalized)?;
        let cursor = normalized
            .cursor
            .as_ref()
            .map(|cursor| {
                decode_cursor(
                    cursor,
                    self.plan.id,
                    self.plan.schema_hash,
                    index.plan.id,
                    fingerprint,
                    self.epoch,
                )
            })
            .transpose()?;
        let mut metrics = QueryMetrics {
            index: Some(index.plan.id),
            ..QueryMetrics::default()
        };
        let mut candidates = canonicalize_candidates(
            select_candidates(index, &normalized.selection, &mut metrics)?,
            index.plan.order,
        )
        .into_iter()
        .collect::<Vec<_>>();
        if index.plan.order == IndexOrder::Descending {
            candidates.reverse();
        }
        metrics.candidates_selected = candidates.len();
        let mut rows = Vec::with_capacity(normalized.limit.min(candidates.len()));
        let mut last = None;
        let mut has_more = false;
        for (key, row_id) in candidates {
            if cursor
                .as_ref()
                .is_some_and(|cursor| match index.plan.order {
                    IndexOrder::Ascending => {
                        (key.clone(), row_id.clone())
                            <= (cursor.last_key.clone(), cursor.last_row.clone())
                    }
                    IndexOrder::Descending => {
                        (key.clone(), row_id.clone())
                            >= (cursor.last_key.clone(), cursor.last_row.clone())
                    }
                })
            {
                continue;
            }
            let value = self.rows.get(&row_id).ok_or_else(|| {
                QueryError::CorruptIndex(format!(
                    "index `{}` references missing row `{}`",
                    index.plan.name, row_id.0
                ))
            })?;
            metrics.rows_examined += 1;
            if !residual_matches(value, &normalized.residual, &mut metrics)? {
                continue;
            }
            if rows.len() == normalized.limit {
                has_more = true;
                break;
            }
            last = Some((key, row_id.clone()));
            rows.push(QueryRow {
                id: row_id,
                value: value.clone(),
            });
        }
        metrics.returned = rows.len();
        let next_cursor = if has_more {
            last.map(|(last_key, last_row)| {
                encode_cursor(&CursorPayload {
                    version: 2,
                    collection: self.plan.id,
                    schema_hash: self.plan.schema_hash,
                    index: index.plan.id,
                    fingerprint,
                    epoch: self.epoch,
                    last_key,
                    last_row,
                })
            })
            .transpose()?
        } else {
            None
        };
        metrics.elapsed_nanos = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        Ok(QueryResult {
            rows,
            next_cursor,
            metrics,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct CursorPayload {
    version: u8,
    collection: CollectionId,
    schema_hash: [u8; 32],
    index: IndexId,
    fingerprint: [u8; 32],
    epoch: u64,
    last_key: IndexKey,
    last_row: RowId,
}

fn select_candidates(
    index: &IndexState,
    selection: &QuerySelection,
    metrics: &mut QueryMetrics,
) -> Result<BTreeSet<(IndexKey, RowId)>, QueryError> {
    match selection {
        QuerySelection::Exact { key } => {
            metrics.ranges += 1;
            metrics.keys_visited += 1;
            let rows = index.entries.get(key);
            if rows.is_some_and(|rows| rows.len() > MAX_QUERY_CANDIDATES) {
                return Err(QueryError::CandidateBudgetExceeded);
            }
            Ok(rows
                .into_iter()
                .flat_map(|rows| rows.iter().cloned())
                .map(|row| (key.clone(), row))
                .collect())
        }
        QuerySelection::TextPrefix { leading, prefix } => {
            if prefix.is_empty() || leading.len() + 1 > index.plan.fields.len() {
                return Err(QueryError::InvalidQuery(
                    "text prefix requires a non-empty prefix at a declared key part".to_owned(),
                ));
            }
            metrics.ranges += 1;
            let mut lower = leading.clone();
            lower.push(KeyPart::Text(prefix.clone()));
            let lower = IndexKey(lower);
            let mut output = BTreeSet::new();
            for (key, rows) in index.entries.range(lower..) {
                let matches_leading = key.0.get(..leading.len()) == Some(leading.as_slice());
                let matches_prefix = matches!(
                    key.0.get(leading.len()),
                    Some(KeyPart::Text(value)) if value.starts_with(prefix)
                );
                if !matches_leading || !matches_prefix {
                    break;
                }
                metrics.keys_visited += 1;
                output.extend(rows.iter().cloned().map(|row| (key.clone(), row)));
                if output.len() > MAX_QUERY_CANDIDATES {
                    return Err(QueryError::CandidateBudgetExceeded);
                }
            }
            Ok(output)
        }
        QuerySelection::Range {
            lower,
            lower_inclusive,
            upper,
            upper_inclusive,
        } => {
            metrics.ranges += 1;
            let lower = match (lower, lower_inclusive) {
                (Some(value), true) => Bound::Included(value),
                (Some(value), false) => Bound::Excluded(value),
                (None, _) => Bound::Unbounded,
            };
            let upper = match (upper, upper_inclusive) {
                (Some(value), true) => Bound::Included(value),
                (Some(value), false) => Bound::Excluded(value),
                (None, _) => Bound::Unbounded,
            };
            let mut output = BTreeSet::new();
            for (key, rows) in index.entries.range((lower, upper)) {
                metrics.keys_visited += 1;
                output.extend(rows.iter().cloned().map(|row| (key.clone(), row)));
                if output.len() > MAX_QUERY_CANDIDATES {
                    return Err(QueryError::CandidateBudgetExceeded);
                }
            }
            Ok(output)
        }
        QuerySelection::Union { selections } => {
            validate_composite_selection(selections, "union")?;
            let mut output = BTreeSet::new();
            for selection in selections {
                output.extend(select_candidates(index, selection, metrics)?);
                if output.len() > MAX_QUERY_CANDIDATES {
                    return Err(QueryError::CandidateBudgetExceeded);
                }
            }
            Ok(output)
        }
        QuerySelection::Intersection { selections } => {
            validate_composite_selection(selections, "intersection")?;
            let candidate_sets = selections
                .iter()
                .map(|selection| select_candidates(index, selection, metrics))
                .collect::<Result<Vec<_>, _>>()?;
            if candidate_sets.iter().map(BTreeSet::len).sum::<usize>() > MAX_QUERY_CANDIDATES {
                return Err(QueryError::CandidateBudgetExceeded);
            }
            let mut common_rows = candidate_sets
                .first()
                .expect("validated non-empty selection")
                .iter()
                .map(|(_, row)| row.clone())
                .collect::<BTreeSet<_>>();
            for candidates in candidate_sets.iter().skip(1) {
                let rows = candidates
                    .iter()
                    .map(|(_, row)| row.clone())
                    .collect::<BTreeSet<_>>();
                common_rows.retain(|row| rows.contains(row));
            }
            Ok(candidate_sets
                .into_iter()
                .flatten()
                .filter(|(_, row)| common_rows.contains(row))
                .collect())
        }
    }
}

fn canonicalize_candidates(
    candidates: BTreeSet<(IndexKey, RowId)>,
    order: IndexOrder,
) -> BTreeSet<(IndexKey, RowId)> {
    let mut by_row = BTreeMap::<RowId, IndexKey>::new();
    for (key, row) in candidates {
        by_row
            .entry(row)
            .and_modify(|current| {
                if (order == IndexOrder::Ascending && key < *current)
                    || (order == IndexOrder::Descending && key > *current)
                {
                    *current = key.clone();
                }
            })
            .or_insert(key);
    }
    by_row.into_iter().map(|(row, key)| (key, row)).collect()
}

fn validate_composite_selection(
    selections: &[QuerySelection],
    label: &str,
) -> Result<(), QueryError> {
    if selections.is_empty() || selections.len() > 64 {
        return Err(QueryError::InvalidQuery(format!(
            "{label} requires 1..=64 selections"
        )));
    }
    Ok(())
}

fn residual_matches(
    value: &Value,
    predicates: &[ResidualPredicate],
    metrics: &mut QueryMetrics,
) -> Result<bool, QueryError> {
    for predicate in predicates {
        metrics.residual_evaluations += 1;
        let matches = match predicate {
            ResidualPredicate::FieldEqual {
                path,
                value: expected,
            } => value_at_path(value, path) == Some(expected),
            ResidualPredicate::TextContains { path, needle } => {
                let Some(Value::Text(value)) = value_at_path(value, path) else {
                    return Err(QueryError::InvalidRow(format!(
                        "residual path `{}` is not Text",
                        path.join(".")
                    )));
                };
                value.contains(needle)
            }
            ResidualPredicate::NumberRange {
                path,
                minimum,
                maximum,
            } => {
                let Some(Value::Number(value)) = value_at_path(value, path) else {
                    return Err(QueryError::InvalidRow(format!(
                        "residual path `{}` is not Number",
                        path.join(".")
                    )));
                };
                minimum.is_none_or(|minimum| *value >= minimum)
                    && maximum.is_none_or(|maximum| *value <= maximum)
            }
            ResidualPredicate::Wgs84Radius {
                latitude_path,
                longitude_path,
                center_latitude,
                center_longitude,
                radius_meters,
            } => {
                let latitude = number_at_path(value, latitude_path)?;
                let longitude = number_at_path(value, longitude_path)?;
                wgs84_distance_meters(
                    latitude.get(),
                    longitude.get(),
                    center_latitude.get(),
                    center_longitude.get(),
                ) <= radius_meters.get()
            }
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn number_at_path(value: &Value, path: &[String]) -> Result<FiniteReal, QueryError> {
    let Some(Value::Number(value)) = value_at_path(value, path) else {
        return Err(QueryError::InvalidRow(format!(
            "residual path `{}` is not Number",
            path.join(".")
        )));
    };
    Ok(*value)
}

/// Projects one canonical authority row through a compiler-owned index plan.
/// Persistence drivers use this same function so durable acceleration entries
/// cannot drift from in-memory query semantics.
pub fn project_index_keys(plan: &IndexPlan, value: &Value) -> Result<Vec<IndexKey>, QueryError> {
    let mut keys = vec![Vec::with_capacity(plan.fields.len())];
    for field in &plan.fields {
        let field_value = value_at_path(value, &field.path).ok_or_else(|| {
            QueryError::InvalidRow(format!(
                "index `{}` path `{}` is missing",
                plan.name,
                field.path.join(".")
            ))
        })?;
        let parts = projected_parts(field, field_value)?;
        if keys.len().saturating_mul(parts.len()) > MAX_KEYS_PER_ROW {
            return Err(QueryError::InvalidRow(format!(
                "index `{}` expands one row beyond {MAX_KEYS_PER_ROW} keys",
                plan.name
            )));
        }
        let mut expanded = Vec::with_capacity(keys.len() * parts.len());
        for key in &keys {
            for part in &parts {
                let mut next = key.clone();
                next.push(part.clone());
                expanded.push(next);
            }
        }
        keys = expanded;
    }
    keys.into_iter().map(IndexKey::new).collect()
}

fn projected_parts(field: &IndexFieldPlan, value: &Value) -> Result<Vec<KeyPart>, QueryError> {
    if field.multi_value {
        let Value::List(values) = value else {
            return Err(QueryError::InvalidRow(format!(
                "multi-value index path `{}` is not a list",
                field.path.join(".")
            )));
        };
        if values.is_empty() || values.len() > MAX_KEYS_PER_ROW {
            return Err(QueryError::InvalidRow(format!(
                "multi-value index path `{}` must contain 1..={MAX_KEYS_PER_ROW} items",
                field.path.join(".")
            )));
        }
        return values
            .iter()
            .map(|value| scalar_key_part(value, field.text_normalization))
            .collect::<Result<BTreeSet<_>, _>>()
            .map(BTreeSet::into_iter)
            .map(Iterator::collect);
    }
    if field.text_normalization == TextNormalization::Tokens {
        let Value::Text(value) = value else {
            return Err(QueryError::InvalidRow(format!(
                "token index path `{}` is not Text",
                field.path.join(".")
            )));
        };
        let tokens = normalize_tokens(value);
        if tokens.is_empty() || tokens.len() > MAX_KEYS_PER_ROW {
            return Err(QueryError::InvalidRow(format!(
                "token index path `{}` must produce 1..={MAX_KEYS_PER_ROW} tokens",
                field.path.join(".")
            )));
        }
        return Ok(tokens.into_iter().map(KeyPart::Text).collect());
    }
    Ok(vec![scalar_key_part(value, field.text_normalization)?])
}

fn scalar_key_part(value: &Value, normalization: TextNormalization) -> Result<KeyPart, QueryError> {
    match value {
        Value::Bool(value) => Ok(KeyPart::Bool(*value)),
        Value::Number(value) => Ok(KeyPart::Number(*value)),
        Value::Text(value) => Ok(KeyPart::Text(match normalization {
            TextNormalization::Exact => value.clone(),
            TextNormalization::TrimLowercase | TextNormalization::Tokens => normalize_text(value),
        })),
        Value::Variant { tag, fields } if fields.is_empty() => Ok(KeyPart::Tag(tag.clone())),
        _ => Err(QueryError::InvalidRow(
            "index keys support Bool, Number, Text, and fieldless tags".to_owned(),
        )),
    }
}

fn row_id(value: &Value, path: &[String]) -> Result<RowId, QueryError> {
    let Some(Value::Text(value)) = value_at_path(value, path) else {
        return Err(QueryError::InvalidRow(format!(
            "row identity path `{}` is not Text",
            path.join(".")
        )));
    };
    RowId::new(value.clone())
}

fn value_at_path<'a>(mut value: &'a Value, path: &[String]) -> Option<&'a Value> {
    for field in path {
        value = match value {
            Value::Record(fields) => fields.get(field)?,
            Value::Variant { fields, .. } | Value::Error { fields, .. } => fields.get(field)?,
            _ => return None,
        };
    }
    Some(value)
}

fn remove_index_row(
    entries: &mut BTreeMap<IndexKey, BTreeSet<RowId>>,
    key: &IndexKey,
    row_id: &RowId,
) {
    let remove_key = entries.get_mut(key).is_some_and(|rows| {
        rows.remove(row_id);
        rows.is_empty()
    });
    if remove_key {
        entries.remove(key);
    }
}

fn validate_query(plan: &QueryPlan) -> Result<(), QueryError> {
    if plan.limit == 0 || plan.limit > MAX_QUERY_LIMIT {
        return Err(QueryError::InvalidQuery(format!(
            "query limit must be 1..={MAX_QUERY_LIMIT}"
        )));
    }
    if plan.residual.len() > 32 {
        return Err(QueryError::InvalidQuery(
            "query may contain at most 32 residual predicates".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_query_plan(plan: &QueryPlan, index: &IndexPlan) -> Result<QueryPlan, QueryError> {
    Ok(QueryPlan {
        index: plan.index,
        selection: normalize_selection(&plan.selection, index)?,
        residual: plan.residual.clone(),
        limit: plan.limit,
        cursor: plan.cursor.clone(),
    })
}

fn normalize_selection(
    selection: &QuerySelection,
    index: &IndexPlan,
) -> Result<QuerySelection, QueryError> {
    Ok(match selection {
        QuerySelection::Exact { key } => QuerySelection::Exact {
            key: normalize_key(key, index, true)?,
        },
        QuerySelection::TextPrefix { leading, prefix } => {
            if leading.len() >= index.fields.len() {
                return Err(QueryError::InvalidQuery(
                    "text prefix leading key must leave one indexed Text part".to_owned(),
                ));
            }
            let leading = normalize_parts(leading, &index.fields[..leading.len()])?;
            let target = &index.fields[leading.len()];
            let prefix = match target.text_normalization {
                TextNormalization::Exact => prefix.clone(),
                TextNormalization::TrimLowercase | TextNormalization::Tokens => {
                    normalize_text(prefix)
                }
            };
            QuerySelection::TextPrefix { leading, prefix }
        }
        QuerySelection::Range {
            lower,
            lower_inclusive,
            upper,
            upper_inclusive,
        } => QuerySelection::Range {
            lower: lower
                .as_ref()
                .map(|key| normalize_key(key, index, true))
                .transpose()?,
            lower_inclusive: *lower_inclusive,
            upper: upper
                .as_ref()
                .map(|key| normalize_key(key, index, true))
                .transpose()?,
            upper_inclusive: *upper_inclusive,
        },
        QuerySelection::Union { selections } => QuerySelection::Union {
            selections: selections
                .iter()
                .map(|selection| normalize_selection(selection, index))
                .collect::<Result<Vec<_>, _>>()?,
        },
        QuerySelection::Intersection { selections } => QuerySelection::Intersection {
            selections: selections
                .iter()
                .map(|selection| normalize_selection(selection, index))
                .collect::<Result<Vec<_>, _>>()?,
        },
    })
}

fn normalize_key(
    key: &IndexKey,
    index: &IndexPlan,
    require_full_arity: bool,
) -> Result<IndexKey, QueryError> {
    if (require_full_arity && key.0.len() != index.fields.len())
        || key.0.is_empty()
        || key.0.len() > index.fields.len()
    {
        return Err(QueryError::InvalidQuery(format!(
            "index `{}` requires a {}-part key",
            index.name,
            index.fields.len()
        )));
    }
    IndexKey::new(normalize_parts(&key.0, &index.fields[..key.0.len()])?)
}

fn normalize_parts(
    parts: &[KeyPart],
    fields: &[IndexFieldPlan],
) -> Result<Vec<KeyPart>, QueryError> {
    parts
        .iter()
        .zip(fields)
        .map(|(part, field)| match part {
            KeyPart::Text(value) => Ok(KeyPart::Text(match field.text_normalization {
                TextNormalization::Exact => value.clone(),
                TextNormalization::TrimLowercase | TextNormalization::Tokens => {
                    normalize_text(value)
                }
            })),
            other => Ok(other.clone()),
        })
        .collect()
}

fn query_fingerprint(plan: &QueryPlan) -> Result<[u8; 32], QueryError> {
    digest(&(
        "boon.query.v1",
        plan.index,
        &plan.selection,
        &plan.residual,
        plan.limit,
    ))
}

fn encode_cursor(payload: &CursorPayload) -> Result<CursorToken, QueryError> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(payload, &mut bytes)
        .map_err(|error| QueryError::Codec(error.to_string()))?;
    let checksum = Sha256::digest(&bytes);
    bytes.extend_from_slice(&checksum);
    CursorToken::from_bytes(bytes)
}

fn decode_cursor(
    token: &CursorToken,
    collection: CollectionId,
    schema_hash: [u8; 32],
    index: IndexId,
    fingerprint: [u8; 32],
    epoch: u64,
) -> Result<CursorPayload, QueryError> {
    let bytes = token.as_bytes();
    if bytes.len() < 32 {
        return Err(QueryError::InvalidCursor("cursor is truncated".to_owned()));
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - 32);
    if Sha256::digest(payload).as_slice() != checksum {
        return Err(QueryError::InvalidCursor(
            "cursor checksum mismatch".to_owned(),
        ));
    }
    let cursor: CursorPayload = ciborium::de::from_reader(payload)
        .map_err(|error| QueryError::InvalidCursor(error.to_string()))?;
    if cursor.version != 2
        || cursor.collection != collection
        || cursor.schema_hash != schema_hash
        || cursor.index != index
        || cursor.fingerprint != fingerprint
        || cursor.epoch != epoch
    {
        return Err(QueryError::StaleCursor);
    }
    Ok(cursor)
}

fn digest(value: &impl Serialize) -> Result<[u8; 32], QueryError> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)
        .map_err(|error| QueryError::Codec(error.to_string()))?;
    Ok(Sha256::digest(bytes).into())
}

fn canonical_name(value: String, kind: &str) -> Result<String, QueryError> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 256
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(QueryError::InvalidPlan(format!(
            "{kind} name must be a canonical 1..=256 byte identifier"
        )));
    }
    Ok(value)
}

fn validate_path(path: &[String], label: &str) -> Result<(), QueryError> {
    if path.is_empty()
        || path.len() > 32
        || path
            .iter()
            .any(|part| part.is_empty() || part.len() > 256 || part.trim() != part)
    {
        return Err(QueryError::InvalidPlan(format!(
            "{label} path must contain 1..=32 canonical components"
        )));
    }
    Ok(())
}

pub fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn normalize_tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .map(normalize_text)
        .filter(|token| !token.is_empty())
        .collect()
}

pub fn damerau_levenshtein_at_most_one(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    match left.len().abs_diff(right.len()) {
        0 => {
            let differences = left
                .iter()
                .zip(&right)
                .enumerate()
                .filter_map(|(index, (left, right))| (left != right).then_some(index))
                .collect::<Vec<_>>();
            differences.len() == 1
                || (differences.len() == 2
                    && differences[1] == differences[0] + 1
                    && left[differences[0]] == right[differences[1]]
                    && left[differences[1]] == right[differences[0]])
        }
        1 => {
            let (shorter, longer) = if left.len() < right.len() {
                (&left, &right)
            } else {
                (&right, &left)
            };
            let mut short = 0;
            let mut long = 0;
            let mut skipped = false;
            while short < shorter.len() && long < longer.len() {
                if shorter[short] == longer[long] {
                    short += 1;
                    long += 1;
                } else if skipped {
                    return false;
                } else {
                    skipped = true;
                    long += 1;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn wgs84_distance_meters(
    latitude_a: f64,
    longitude_a: f64,
    latitude_b: f64,
    longitude_b: f64,
) -> f64 {
    const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
    let lat_a = latitude_a.to_radians();
    let lat_b = latitude_b.to_radians();
    let delta_lat = (latitude_b - latitude_a).to_radians();
    let delta_lon = (longitude_b - longitude_a).to_radians();
    let haversine = (delta_lat / 2.0).sin().powi(2)
        + lat_a.cos() * lat_b.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_METERS * haversine.sqrt().asin()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    InvalidPlan(String),
    InvalidQuery(String),
    InvalidRow(String),
    InvalidCursor(String),
    StaleCursor,
    UnknownIndex(IndexId),
    UniqueConflict { index: String, key: IndexKey },
    CorruptIndex(String),
    CandidateBudgetExceeded,
    Codec(String),
    EpochOverflow,
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan(message) => write!(formatter, "invalid query plan: {message}"),
            Self::InvalidQuery(message) => write!(formatter, "invalid query: {message}"),
            Self::InvalidRow(message) => write!(formatter, "invalid collection row: {message}"),
            Self::InvalidCursor(message) => write!(formatter, "invalid cursor: {message}"),
            Self::StaleCursor => formatter.write_str("query cursor is stale or incompatible"),
            Self::UnknownIndex(id) => write!(formatter, "unknown index {:02x?}", id.0),
            Self::UniqueConflict { index, key } => {
                write!(formatter, "unique index `{index}` conflicts at {key:?}")
            }
            Self::CorruptIndex(message) => write!(formatter, "corrupt query index: {message}"),
            Self::CandidateBudgetExceeded => write!(
                formatter,
                "indexed query candidate budget of {MAX_QUERY_CANDIDATES} was exceeded"
            ),
            Self::Codec(message) => write!(formatter, "query codec failed: {message}"),
            Self::EpochOverflow => formatter.write_str("collection epoch overflow"),
        }
    }
}

impl Error for QueryError {}
