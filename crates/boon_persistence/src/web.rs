#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::codec::{
    BlobDigest, BlobRecord, EncodedComponent, decode_row_component, decode_scalar_component,
    encode_blob_record, encode_outbox_record, encode_row_component, encode_scalar_component,
};
#[cfg(target_arch = "wasm32")]
use super::codec::{
    decode_blob_record, decode_outbox_record, decode_protocol_state_record,
    encode_protocol_state_record, row_component_blob_references, scalar_component_blob_references,
};
use super::{
    ActivationBatch, CheckpointBatch, ContentArtifact, ContentArtifactBinding, ContentArtifactId,
    ContentArtifactManifest, ContentArtifactOwnerId, ContentArtifactRetention, DecodeLimits,
    DurableChange, DurableOutboxChange, OutboxItemId, PersistenceResult, ProtocolStateKey,
    RestoreImage, StoreError, StoredRow, encode_restore_image, validate_content_artifact,
};
use boon_plan::{ApplicationIdentity, MemoryId, MigrationEdgeId};
use minicbor::{Decoder, Encoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[cfg(target_arch = "wasm32")]
use super::{
    ActivationAck, ApplicationTransfer, BarrierAck, BarrierRequest, CommitAck, CompactAck,
    CompactRequest, DurableContentArtifactChange, DurableProtocolStateChange,
    ExportApplicationRequest, InspectRequest, LoadContentArtifactRequest, PersistenceCommand,
    PersistenceInspectorSnapshot, ProtocolStateSnapshot, PutContentArtifactAck,
    PutContentArtifactRequest, ResetApplicationAck, ResetApplicationBatch, RestoreRequest,
    ShutdownAck, StoredList, apply_durable_content_artifact_changes,
    apply_durable_protocol_state_changes, exact_content_artifact_closure,
    inspector_snapshot_with_artifacts, validate_application_transfer,
    validate_content_artifact_manifest, validate_content_artifact_storage, validate_protocol_state,
};
#[cfg(target_arch = "wasm32")]
use futures::channel::{mpsc, oneshot};
#[cfg(target_arch = "wasm32")]
use futures::future::{Either, select};
#[cfg(target_arch = "wasm32")]
use futures::pin_mut;
#[cfg(target_arch = "wasm32")]
use futures::{SinkExt, StreamExt};
#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;
#[cfg(target_arch = "wasm32")]
use js_sys::{Reflect, Uint8Array};
#[cfg(target_arch = "wasm32")]
use rexie::{KeyRange, ObjectStore, Rexie, Transaction, TransactionMode};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::{JsFuture, spawn_local};
#[cfg(target_arch = "wasm32")]
use web_sys::{DomException, StorageManager, WorkerGlobalScope};

const DATABASE_VERSION: u32 = 3;
const COMPONENT_FORMAT: u32 = 1;
const MAX_CHECKPOINT_RECORDS_PER_APPLICATION: usize = 64;
const DEFAULT_UPGRADE_TIMEOUT_MS: u32 = 15_000;
const STORAGE_STATUS_TIMEOUT_MS: u32 = 2_000;
const DEFAULT_COMMAND_QUEUE_CAPACITY: usize = 64;
const MAX_COMMAND_QUEUE_CAPACITY: usize = 4_096;
const MAX_STATUS_DETAIL_BYTES: usize = 1_024;
const HEX_PREFIX_UPPER_SENTINEL: char = 'g';

const META: &str = "meta";
const SLOTS: &str = "slots";
const LISTS: &str = "lists";
const ROWS: &str = "rows";
const CHECKPOINTS: &str = "checkpoints";
const MIGRATIONS: &str = "migrations";
const OUTBOX: &str = "outbox";
const PROTOCOL_STATE: &str = "protocol_state";
const BLOBS: &str = "blobs";
const ARTIFACTS: &str = "artifacts";
const ARTIFACT_OWNERS: &str = "artifact_owners";

const STORE_NAMES: [&str; 11] = [
    META,
    SLOTS,
    LISTS,
    ROWS,
    CHECKPOINTS,
    MIGRATIONS,
    OUTBOX,
    PROTOCOL_STATE,
    BLOBS,
    ARTIFACTS,
    ARTIFACT_OWNERS,
];
const LOAD_STORES: [&str; 9] = [
    META,
    SLOTS,
    LISTS,
    ROWS,
    MIGRATIONS,
    OUTBOX,
    BLOBS,
    ARTIFACTS,
    ARTIFACT_OWNERS,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFailureKind {
    QuotaExceeded,
    MissingOrEvicted,
    PrivateModeOrUnavailable,
    UpgradeBlocked,
    Timeout,
    TransactionAborted,
    VersionChangeClosed,
    VersionMismatch,
    Backend,
}

impl BrowserFailureKind {
    fn code(self) -> &'static str {
        match self {
            Self::QuotaExceeded => "quota_exceeded",
            Self::MissingOrEvicted => "missing_or_evicted",
            Self::PrivateModeOrUnavailable => "private_mode_or_unavailable",
            Self::UpgradeBlocked => "upgrade_blocked",
            Self::Timeout => "timeout",
            Self::TransactionAborted => "transaction_aborted",
            Self::VersionChangeClosed => "version_change_closed",
            Self::VersionMismatch => "version_mismatch",
            Self::Backend => "backend",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        match code {
            "quota_exceeded" => Some(Self::QuotaExceeded),
            "missing_or_evicted" => Some(Self::MissingOrEvicted),
            "private_mode_or_unavailable" => Some(Self::PrivateModeOrUnavailable),
            "upgrade_blocked" => Some(Self::UpgradeBlocked),
            "timeout" => Some(Self::Timeout),
            "transaction_aborted" => Some(Self::TransactionAborted),
            "version_change_closed" => Some(Self::VersionChangeClosed),
            "version_mismatch" => Some(Self::VersionMismatch),
            "backend" => Some(Self::Backend),
            _ => None,
        }
    }
}

pub fn browser_failure_kind(error: &StoreError) -> Option<BrowserFailureKind> {
    if matches!(error, StoreError::MissingApplication) {
        return Some(BrowserFailureKind::MissingOrEvicted);
    }
    let StoreError::Backend(detail) = error else {
        return None;
    };
    let rest = detail.strip_prefix("indexeddb/")?;
    let code = rest.split_once(':').map_or(rest, |(code, _)| code);
    BrowserFailureKind::from_code(code)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum BrowserPersistenceGrant {
    Granted,
    Denied,
    TimedOut { timeout_ms: u32 },
    Unavailable { detail: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserStorageStatus {
    pub persistence: BrowserPersistenceGrant,
    pub usage_bytes: Option<u64>,
    pub quota_bytes: Option<u64>,
    pub estimate_error: Option<String>,
    pub quota_failure: Option<BrowserFailureKind>,
    pub missing_or_evicted: bool,
    pub last_operation_failure: Option<BrowserFailureKind>,
    pub last_status_detail: Option<String>,
}

impl BrowserStorageStatus {
    pub fn eviction_risk(&self) -> bool {
        !matches!(self.persistence, BrowserPersistenceGrant::Granted)
            || self.quota_failure.is_some()
            || self.missing_or_evicted
    }

    pub fn available_bytes(&self) -> Option<u64> {
        self.quota_bytes
            .zip(self.usage_bytes)
            .map(|(quota, usage)| quota.saturating_sub(usage))
    }

    fn record_result(&mut self, result: &PersistenceResult) {
        self.last_operation_failure =
            persistence_result_error(result).and_then(browser_failure_kind);
        self.missing_or_evicted = matches!(
            result,
            PersistenceResult::Loaded(Ok(None))
                | PersistenceResult::ProtocolStateLoaded(Err(StoreError::MissingApplication))
                | PersistenceResult::Committed(Err(StoreError::MissingApplication))
                | PersistenceResult::Activated(Err(StoreError::MissingApplication))
                | PersistenceResult::ApplicationReset(Err(StoreError::MissingApplication))
                | PersistenceResult::BarrierComplete(Err(StoreError::MissingApplication))
                | PersistenceResult::Compacted(Err(StoreError::MissingApplication))
                | PersistenceResult::ApplicationExported(Err(StoreError::MissingApplication))
                | PersistenceResult::ContentArtifactStored(Err(StoreError::MissingApplication))
                | PersistenceResult::ContentArtifactLoaded(Err(StoreError::MissingApplication))
        );
        if self.missing_or_evicted {
            self.last_operation_failure = Some(BrowserFailureKind::MissingOrEvicted);
            self.last_status_detail =
                Some("durable application state is missing or was evicted".to_owned());
        } else if let Some(error) = persistence_result_error(result) {
            self.last_status_detail = Some(bounded_status_detail(error.to_string()));
        } else {
            self.last_status_detail = None;
        }
    }
}

#[cfg(target_arch = "wasm32")]
/// Bounded async coordinator for the browser-local IndexedDB persistence worker.
///
/// Polling this handle only enqueues owned DTOs. The local worker yields to the browser event
/// loop before opening IndexedDB and before executing every command, so UI/input/render
/// callbacks never perform IndexedDB work synchronously. Rexie handles remain worker-owned.
pub struct RexieDriver {
    sender: mpsc::Sender<CoordinatorRequest>,
    limits: std::rc::Rc<std::cell::Cell<DecodeLimits>>,
    queue_capacity: usize,
    closed: bool,
    storage_status: BrowserStorageStatus,
    _local: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(target_arch = "wasm32")]
struct IndexedDbBackend {
    database: Option<rexie::Rexie>,
    limits: DecodeLimits,
    storage_status: BrowserStorageStatus,
    _local: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(target_arch = "wasm32")]
enum CoordinatorRequest {
    Execute {
        command: PersistenceCommand,
        response: oneshot::Sender<CoordinatorResponse>,
    },
    RefreshStorageStatus {
        response: oneshot::Sender<BrowserStorageStatus>,
    },
}

#[cfg(target_arch = "wasm32")]
struct CoordinatorResponse {
    result: PersistenceResult,
    storage_status: BrowserStorageStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MetaRecord {
    application: ApplicationIdentity,
    schema_version: u64,
    schema_hash: [u8; 32],
    epoch: u64,
    through_turn_sequence: u64,
    clean_shutdown: bool,
    initialization_checksum: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RowRef {
    key: u64,
    generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ListRecord {
    touched: bool,
    next_key: u64,
    rows: Vec<RowRef>,
}

#[derive(Clone, Copy)]
enum CheckpointKind {
    Checkpoint = 0,
    Activation = 1,
    Reset = 2,
}

struct CheckpointRecord {
    kind: CheckpointKind,
    base_epoch: u64,
    next_epoch: u64,
    first_turn_sequence: u64,
    last_turn_sequence: u64,
    schema_hash: [u8; 32],
    checksum: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrefixRange {
    lower: String,
    upper_exclusive: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SparseTransactionPlan {
    stores: BTreeSet<&'static str>,
}

impl SparseTransactionPlan {
    fn checkpoint(batch: &CheckpointBatch) -> Self {
        let mut plan = Self::with_required([META, CHECKPOINTS]);
        for change in &batch.changes {
            plan.include_authority_change(change);
        }
        if !batch.outbox_changes.is_empty() {
            plan.stores.insert(OUTBOX);
        }
        if !batch.protocol_state_changes.is_empty() {
            plan.stores.insert(PROTOCOL_STATE);
        }
        if !batch.content_artifact_changes.is_empty() {
            plan.stores.extend([ARTIFACTS, ARTIFACT_OWNERS]);
        }
        plan
    }

    fn activation(batch: &ActivationBatch) -> Self {
        let mut plan = Self::with_required([META, CHECKPOINTS, ARTIFACTS, ARTIFACT_OWNERS]);
        for change in &batch.authority_changes {
            plan.include_authority_change(change);
        }
        if !batch.deleted_memory.is_empty() {
            plan.stores.extend([SLOTS, LISTS, ROWS, BLOBS]);
        }
        if !batch.completed_migration_edges.is_empty() {
            plan.stores.insert(MIGRATIONS);
        }
        plan
    }

    fn with_required<const N: usize>(stores: [&'static str; N]) -> Self {
        Self {
            stores: stores.into_iter().collect(),
        }
    }

    fn include_authority_change(&mut self, change: &DurableChange) {
        self.stores.insert(BLOBS);
        match change {
            DurableChange::SetScalar { .. } => {
                self.stores.extend([SLOTS, LISTS]);
            }
            DurableChange::DeleteScalar { .. } => {
                self.stores.insert(SLOTS);
            }
            DurableChange::SetList { .. } | DurableChange::SetRowField { .. } => {
                self.stores.extend([SLOTS, LISTS, ROWS]);
            }
            DurableChange::InsertRow { .. }
            | DurableChange::RemoveRow { .. }
            | DurableChange::DeleteList { .. } => {
                self.stores.extend([LISTS, ROWS]);
            }
        }
    }

    fn store_names(&self) -> Vec<&'static str> {
        self.stores.iter().copied().collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentCodecError(String);

impl ComponentCodecError {
    fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for ComponentCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ComponentCodecError {}

fn encode_meta(meta: &MetaRecord) -> Result<Vec<u8>, ComponentCodecError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(10)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.str(&meta.application.package_id))
        .and_then(|encoder| encoder.str(&meta.application.state_namespace))
        .and_then(|encoder| encoder.str(&meta.application.deployment_domain))
        .and_then(|encoder| encoder.u64(meta.schema_version))
        .and_then(|encoder| encoder.bytes(&meta.schema_hash))
        .and_then(|encoder| encoder.u64(meta.epoch))
        .and_then(|encoder| encoder.u64(meta.through_turn_sequence))
        .and_then(|encoder| encoder.bool(meta.clean_shutdown))
        .and_then(|encoder| encoder.bytes(&meta.initialization_checksum))
        .map_err(encode_error)?;
    Ok(bytes)
}

fn decode_meta(bytes: &[u8], limits: DecodeLimits) -> Result<MetaRecord, ComponentCodecError> {
    component_size(bytes, limits, "metadata")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 10, "metadata")?;
    expect_format(&mut decoder, "metadata")?;
    let application = ApplicationIdentity::new(
        decode_text(&mut decoder, limits)?,
        decode_text(&mut decoder, limits)?,
        decode_text(&mut decoder, limits)?,
    );
    let schema_version = decoder.u64().map_err(decode_error)?;
    let schema_hash = decode_digest(&mut decoder)?;
    let epoch = decoder.u64().map_err(decode_error)?;
    let through_turn_sequence = decoder.u64().map_err(decode_error)?;
    let clean_shutdown = decoder.bool().map_err(decode_error)?;
    let initialization_checksum = decode_digest(&mut decoder)?;
    reject_trailing(&decoder, bytes, "metadata")?;
    Ok(MetaRecord {
        application,
        schema_version,
        schema_hash,
        epoch,
        through_turn_sequence,
        clean_shutdown,
        initialization_checksum,
    })
}

fn encode_list_record(list: &ListRecord) -> Result<Vec<u8>, ComponentCodecError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(4)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.bool(list.touched))
        .and_then(|encoder| encoder.u64(list.next_key))
        .and_then(|encoder| encoder.array(list.rows.len() as u64))
        .map_err(encode_error)?;
    for row in &list.rows {
        encoder
            .array(2)
            .and_then(|encoder| encoder.u64(row.key))
            .and_then(|encoder| encoder.u64(row.generation))
            .map_err(encode_error)?;
    }
    Ok(bytes)
}

fn decode_list_record(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ListRecord, ComponentCodecError> {
    component_size(bytes, limits, "list metadata")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 4, "list metadata")?;
    expect_format(&mut decoder, "list metadata")?;
    let touched = decoder.bool().map_err(decode_error)?;
    let next_key = decoder.u64().map_err(decode_error)?;
    let count = collection_len(&mut decoder, limits, "list row order", true)?;
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        expect_array(&mut decoder, 2, "row identity")?;
        rows.push(RowRef {
            key: decoder.u64().map_err(decode_error)?,
            generation: decoder.u64().map_err(decode_error)?,
        });
    }
    reject_trailing(&decoder, bytes, "list metadata")?;
    let record = ListRecord {
        touched,
        next_key,
        rows,
    };
    validate_list_record(&record)?;
    Ok(record)
}

fn encode_checkpoint_record(record: &CheckpointRecord) -> Result<Vec<u8>, ComponentCodecError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(9)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.u8(record.kind as u8))
        .and_then(|encoder| encoder.u64(record.base_epoch))
        .and_then(|encoder| encoder.u64(record.next_epoch))
        .and_then(|encoder| encoder.u64(record.first_turn_sequence))
        .and_then(|encoder| encoder.u64(record.last_turn_sequence))
        .and_then(|encoder| encoder.bytes(&record.schema_hash))
        .and_then(|encoder| encoder.bytes(&record.checksum))
        .and_then(|encoder| encoder.bool(true))
        .map_err(encode_error)?;
    Ok(bytes)
}

fn encode_content_artifact(artifact: &ContentArtifact) -> Result<Vec<u8>, ComponentCodecError> {
    validate_content_artifact(artifact)
        .map_err(|error| ComponentCodecError::new(error.to_string()))?;
    let mut bytes = Vec::new();
    Encoder::new(&mut bytes)
        .array(3)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.str(&artifact.media_type))
        .and_then(|encoder| encoder.bytes(&artifact.bytes))
        .map_err(encode_error)?;
    Ok(bytes)
}

fn decode_content_artifact(
    id: ContentArtifactId,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ContentArtifact, ComponentCodecError> {
    component_size(bytes, limits, "content artifact")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 3, "content artifact")?;
    expect_format(&mut decoder, "content artifact")?;
    let media_type = decoder.str().map_err(decode_error)?;
    if media_type.len() > super::MAX_CONTENT_ARTIFACT_MEDIA_TYPE_BYTES {
        return Err(ComponentCodecError::new(
            "content artifact media type exceeds byte limit",
        ));
    }
    let media_type = media_type.to_owned();
    let payload = decoder.bytes().map_err(decode_error)?;
    if payload.len() > super::MAX_CONTENT_ARTIFACT_BYTES {
        return Err(ComponentCodecError::new(
            "content artifact payload exceeds byte limit",
        ));
    }
    let payload = payload.to_vec();
    reject_trailing(&decoder, bytes, "content artifact")?;
    let artifact = ContentArtifact {
        id,
        media_type,
        bytes: payload,
    };
    validate_content_artifact(&artifact)
        .map_err(|error| ComponentCodecError::new(error.to_string()))?;
    Ok(artifact)
}

fn encode_content_artifact_binding(
    binding: ContentArtifactBinding,
) -> Result<Vec<u8>, ComponentCodecError> {
    let mut bytes = Vec::new();
    Encoder::new(&mut bytes)
        .array(3)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.bytes(binding.artifact_id.as_bytes()))
        .and_then(|encoder| {
            encoder.u8(match binding.retention {
                ContentArtifactRetention::Replaceable => 0,
                ContentArtifactRetention::Immutable => 1,
            })
        })
        .map_err(encode_error)?;
    Ok(bytes)
}

fn decode_content_artifact_binding(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ContentArtifactBinding, ComponentCodecError> {
    component_size(bytes, limits, "content artifact binding")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 3, "content artifact binding")?;
    expect_format(&mut decoder, "content artifact binding")?;
    let artifact_id = ContentArtifactId(decode_digest(&mut decoder)?);
    let retention = match decoder.u8().map_err(decode_error)? {
        0 => ContentArtifactRetention::Replaceable,
        1 => ContentArtifactRetention::Immutable,
        tag => {
            return Err(ComponentCodecError::new(format!(
                "unknown content artifact retention tag {tag}"
            )));
        }
    };
    reject_trailing(&decoder, bytes, "content artifact binding")?;
    Ok(ContentArtifactBinding {
        artifact_id,
        retention,
    })
}

fn decode_digest(decoder: &mut Decoder<'_>) -> Result<[u8; 32], ComponentCodecError> {
    decoder
        .bytes()
        .map_err(decode_error)?
        .try_into()
        .map_err(|_| ComponentCodecError::new("digest must contain exactly 32 bytes"))
}

fn decode_text(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<String, ComponentCodecError> {
    let value = decoder.str().map_err(decode_error)?;
    if value.len() > limits.max_text_bytes {
        return Err(ComponentCodecError::new("text exceeds decode limit"));
    }
    Ok(value.to_owned())
}

fn expect_array(
    decoder: &mut Decoder<'_>,
    expected: usize,
    label: &str,
) -> Result<(), ComponentCodecError> {
    let actual = definite_len(decoder.array().map_err(decode_error)?, label)?;
    if actual != expected {
        return Err(ComponentCodecError::new(format!(
            "{label} has {actual} fields, expected {expected}"
        )));
    }
    Ok(())
}

fn expect_format(decoder: &mut Decoder<'_>, label: &str) -> Result<(), ComponentCodecError> {
    let format = decoder.u32().map_err(decode_error)?;
    if format != COMPONENT_FORMAT {
        return Err(ComponentCodecError::new(format!(
            "unsupported {label} format {format}"
        )));
    }
    Ok(())
}

fn collection_len(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    label: &str,
    array: bool,
) -> Result<usize, ComponentCodecError> {
    let length = if array {
        decoder.array().map_err(decode_error)?
    } else {
        decoder.map().map_err(decode_error)?
    };
    let length = definite_len(length, label)?;
    if length > limits.max_collection_items {
        return Err(ComponentCodecError::new(format!(
            "{label} exceeds collection item limit"
        )));
    }
    Ok(length)
}

fn definite_len(length: Option<u64>, label: &str) -> Result<usize, ComponentCodecError> {
    let length = length
        .ok_or_else(|| ComponentCodecError::new(format!("{label} must use definite length")))?;
    usize::try_from(length)
        .map_err(|_| ComponentCodecError::new(format!("{label} length overflows usize")))
}

fn component_size(
    bytes: &[u8],
    limits: DecodeLimits,
    label: &str,
) -> Result<(), ComponentCodecError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(ComponentCodecError::new(format!(
            "{label} exceeds total byte limit"
        )));
    }
    Ok(())
}

fn reject_trailing(
    decoder: &Decoder<'_>,
    bytes: &[u8],
    label: &str,
) -> Result<(), ComponentCodecError> {
    if decoder.position() != bytes.len() {
        return Err(ComponentCodecError::new(format!(
            "{label} has trailing bytes"
        )));
    }
    Ok(())
}

fn validate_list_record(list: &ListRecord) -> Result<(), ComponentCodecError> {
    let unique = list.rows.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != list.rows.len() {
        return Err(ComponentCodecError::new(
            "list order repeats a row identity",
        ));
    }
    if !list.touched && list.next_key != 0 {
        return Err(ComponentCodecError::new(
            "sparse row overrides must not replace list allocator state",
        ));
    }
    if list.touched {
        let minimum_next = list
            .rows
            .iter()
            .fold(1u64, |next, row| next.max(row.key.saturating_add(1)));
        if list.next_key < minimum_next {
            return Err(ComponentCodecError::new(format!(
                "next key {} is below {}",
                list.next_key, minimum_next
            )));
        }
    }
    Ok(())
}

fn encode_error<E: fmt::Debug>(error: E) -> ComponentCodecError {
    ComponentCodecError::new(format!("CBOR encode failed: {error:?}"))
}

fn decode_error<E: fmt::Display>(error: E) -> ComponentCodecError {
    ComponentCodecError::new(format!("CBOR decode failed: {error}"))
}

fn restore_checksum(image: &RestoreImage) -> Result<[u8; 32], StoreError> {
    let bytes = encode_restore_image(image).map_err(codec_backend)?;
    Ok(Sha256::digest(bytes).into())
}

fn application_key(application: &ApplicationIdentity) -> [u8; 32] {
    let mut hasher = Sha256::new();
    super::hash_application(&mut hasher, application);
    hasher.finalize().into()
}

fn application_storage_key(application: &ApplicationIdentity) -> String {
    encode_hex(&application_key(application))
}

fn prefix_range(prefix: &str) -> Result<PrefixRange, StoreError> {
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(corrupt(
            "IndexedDB key prefix is not canonical lowercase hex",
        ));
    }
    let mut upper_exclusive = String::with_capacity(prefix.len() + 1);
    upper_exclusive.push_str(prefix);
    upper_exclusive.push(HEX_PREFIX_UPPER_SENTINEL);
    Ok(PrefixRange {
        lower: prefix.to_owned(),
        upper_exclusive,
    })
}

fn bounded_scan_limit(limits: DecodeLimits) -> Result<u32, StoreError> {
    let limit = limits
        .max_collection_items
        .checked_add(1)
        .ok_or_else(|| corrupt("IndexedDB scan limit overflow"))?;
    u32::try_from(limit)
        .map_err(|_| corrupt("IndexedDB scan limit exceeds the browser API's u32 bound"))
}

#[cfg(target_arch = "wasm32")]
fn indexed_db_prefix_range(prefix: &str) -> Result<KeyRange, StoreError> {
    let range = prefix_range(prefix)?;
    KeyRange::bound(
        &JsValue::from_str(&range.lower),
        &JsValue::from_str(&range.upper_exclusive),
        Some(false),
        Some(true),
    )
    .map_err(|error| indexed_db_error("create key prefix range", error))
}

fn memory_storage_key(application: &ApplicationIdentity, memory: MemoryId) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(memory.as_bytes())
    )
}

fn row_storage_key(application: &ApplicationIdentity, memory: MemoryId, row: RowRef) -> String {
    format!(
        "{}{:016x}{:016x}",
        memory_storage_key(application, memory),
        row.key,
        row.generation
    )
}

fn checkpoint_storage_key(application: &ApplicationIdentity, epoch: u64) -> String {
    format!("{}{:016x}", application_storage_key(application), epoch)
}

fn migration_storage_key(application: &ApplicationIdentity, edge: MigrationEdgeId) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(edge.as_bytes())
    )
}

fn outbox_storage_key(application: &ApplicationIdentity, item_id: OutboxItemId) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(item_id.as_bytes())
    )
}

fn protocol_state_storage_key(application: &ApplicationIdentity, key: ProtocolStateKey) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(key.as_bytes())
    )
}

fn blob_storage_key(application: &ApplicationIdentity, digest: BlobDigest) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(&digest.0)
    )
}

fn content_artifact_storage_key(
    application: &ApplicationIdentity,
    artifact_id: ContentArtifactId,
) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(artifact_id.as_bytes())
    )
}

fn content_artifact_owner_storage_key(
    application: &ApplicationIdentity,
    owner_id: ContentArtifactOwnerId,
) -> String {
    format!(
        "{}{}",
        application_storage_key(application),
        encode_hex(owner_id.as_bytes())
    )
}

fn outbox_from_storage_key(key: &str) -> Result<OutboxItemId, StoreError> {
    if key.len() != 128 {
        return Err(corrupt("invalid outbox key"));
    }
    Ok(OutboxItemId(decode_hex_digest(&key[64..])?))
}

fn protocol_state_from_storage_key(key: &str) -> Result<ProtocolStateKey, StoreError> {
    if key.len() != 128 {
        return Err(corrupt("invalid protocol-state key"));
    }
    Ok(ProtocolStateKey(decode_hex_digest(&key[64..])?))
}

fn blob_from_storage_key(key: &str) -> Result<BlobDigest, StoreError> {
    if key.len() != 128 {
        return Err(corrupt("invalid blob key"));
    }
    Ok(BlobDigest(decode_hex_digest(&key[64..])?))
}

fn content_artifact_from_storage_key(key: &str) -> Result<ContentArtifactId, StoreError> {
    if key.len() != 128 {
        return Err(corrupt("invalid content artifact key"));
    }
    Ok(ContentArtifactId(decode_hex_digest(&key[64..])?))
}

fn content_artifact_owner_from_storage_key(
    key: &str,
) -> Result<ContentArtifactOwnerId, StoreError> {
    if key.len() != 128 {
        return Err(corrupt("invalid content artifact owner key"));
    }
    Ok(ContentArtifactOwnerId(decode_hex_digest(&key[64..])?))
}

fn merge_blob_references(
    target: &mut BTreeMap<BlobDigest, u64>,
    source: &BTreeMap<BlobDigest, u64>,
) -> Result<(), StoreError> {
    for (digest, count) in source {
        let current = target.entry(*digest).or_default();
        *current = current
            .checked_add(*count)
            .ok_or_else(|| corrupt("blob reference count overflow"))?;
    }
    Ok(())
}

fn validate_blob_reference_counts(
    blobs: &BTreeMap<BlobDigest, BlobRecord>,
    actual: &BTreeMap<BlobDigest, u64>,
) -> Result<(), StoreError> {
    if blobs.len() != actual.len()
        || blobs
            .iter()
            .any(|(digest, record)| actual.get(digest).copied() != Some(record.reference_count))
    {
        return Err(corrupt(
            "blob store reference counts do not match scalar and row records",
        ));
    }
    Ok(())
}

fn memory_from_storage_key(key: &str, label: &str) -> Result<MemoryId, StoreError> {
    if key.len() != 128 {
        return Err(corrupt(format!("invalid {label} key")));
    }
    Ok(MemoryId(decode_hex_digest(&key[64..])?))
}

fn row_from_storage_key(key: &str) -> Result<(MemoryId, RowRef), StoreError> {
    if key.len() != 160 {
        return Err(corrupt("invalid row key"));
    }
    let memory = MemoryId(decode_hex_digest(&key[64..128])?);
    let row = RowRef {
        key: decode_hex_u64(&key[128..144])?,
        generation: decode_hex_u64(&key[144..160])?,
    };
    Ok((memory, row))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex_digest(value: &str) -> Result<[u8; 32], StoreError> {
    if value.len() != 64 {
        return Err(corrupt("digest key must contain 64 hexadecimal digits"));
    }
    let mut digest = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (decode_nibble(pair[0])? << 4) | decode_nibble(pair[1])?;
    }
    Ok(digest)
}

fn decode_nibble(value: u8) -> Result<u8, StoreError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(corrupt(
            "storage key contains non-canonical hexadecimal digits",
        )),
    }
}

fn decode_hex_u64(value: &str) -> Result<u64, StoreError> {
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(corrupt("invalid hexadecimal u64 storage key component"));
    }
    u64::from_str_radix(value, 16).map_err(|_| corrupt("invalid u64 storage key component"))
}

fn codec_backend(error: impl fmt::Display) -> StoreError {
    StoreError::Backend(format!("durable CBOR: {error}"))
}

fn corrupt(detail: impl Into<String>) -> StoreError {
    StoreError::Backend(format!("corrupt durable state: {}", detail.into()))
}

#[cfg(target_arch = "wasm32")]
struct LoadedApplication {
    image: RestoreImage,
    meta: MetaRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoreMutation {
    Put {
        store: &'static str,
        key: String,
        value: Vec<u8>,
    },
    Delete {
        store: &'static str,
        key: String,
    },
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
enum CoordinatorResultKind {
    Load,
    LoadProtocolState,
    Initialize,
    Commit,
    Activate,
    ResetApplication,
    Barrier,
    Inspect,
    Compact,
    ExportApplication,
    PutContentArtifact,
    LoadContentArtifact,
    Shutdown,
}

#[cfg(target_arch = "wasm32")]
impl CoordinatorResultKind {
    fn from_command(command: &PersistenceCommand) -> Self {
        match command {
            PersistenceCommand::Load(_) => Self::Load,
            PersistenceCommand::LoadProtocolState(_) => Self::LoadProtocolState,
            PersistenceCommand::Initialize(_) => Self::Initialize,
            PersistenceCommand::Commit(_) => Self::Commit,
            PersistenceCommand::Activate(_) => Self::Activate,
            PersistenceCommand::ResetApplication(_) => Self::ResetApplication,
            PersistenceCommand::Barrier(_) => Self::Barrier,
            PersistenceCommand::Inspect(_) => Self::Inspect,
            PersistenceCommand::Compact(_) => Self::Compact,
            PersistenceCommand::ExportApplication(_) => Self::ExportApplication,
            PersistenceCommand::PutContentArtifact(_) => Self::PutContentArtifact,
            PersistenceCommand::LoadContentArtifact(_) => Self::LoadContentArtifact,
            PersistenceCommand::Shutdown(_) => Self::Shutdown,
        }
    }

    fn error_result(self, error: StoreError) -> PersistenceResult {
        match self {
            Self::Load => PersistenceResult::Loaded(Err(error)),
            Self::LoadProtocolState => PersistenceResult::ProtocolStateLoaded(Err(error)),
            Self::Initialize => PersistenceResult::Initialized(Err(error)),
            Self::Commit => PersistenceResult::Committed(Err(error)),
            Self::Activate => PersistenceResult::Activated(Err(error)),
            Self::ResetApplication => PersistenceResult::ApplicationReset(Err(error)),
            Self::Barrier => PersistenceResult::BarrierComplete(Err(error)),
            Self::Inspect => PersistenceResult::Inspected(Err(error)),
            Self::Compact => PersistenceResult::Compacted(Err(error)),
            Self::ExportApplication => PersistenceResult::ApplicationExported(Err(error)),
            Self::PutContentArtifact => PersistenceResult::ContentArtifactStored(Err(error)),
            Self::LoadContentArtifact => PersistenceResult::ContentArtifactLoaded(Err(error)),
            Self::Shutdown => PersistenceResult::ShutdownComplete(Err(error)),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl RexieDriver {
    /// Starts a bounded browser-local persistence worker and waits for its asynchronous open.
    pub async fn open(database_name: impl Into<String>) -> Result<Self, StoreError> {
        Self::open_with_options(
            database_name,
            DEFAULT_UPGRADE_TIMEOUT_MS,
            DEFAULT_COMMAND_QUEUE_CAPACITY,
        )
        .await
    }

    pub async fn open_with_upgrade_timeout(
        database_name: impl Into<String>,
        upgrade_timeout_ms: u32,
    ) -> Result<Self, StoreError> {
        Self::open_with_options(
            database_name,
            upgrade_timeout_ms,
            DEFAULT_COMMAND_QUEUE_CAPACITY,
        )
        .await
    }

    pub async fn open_with_options(
        database_name: impl Into<String>,
        upgrade_timeout_ms: u32,
        command_queue_capacity: usize,
    ) -> Result<Self, StoreError> {
        validate_command_queue_capacity(command_queue_capacity)?;
        if upgrade_timeout_ms == 0 {
            return Err(indexed_db_failure(
                BrowserFailureKind::Timeout,
                "database upgrade timeout must be positive",
            ));
        }
        let database_name = database_name.into();
        let (sender, receiver) = mpsc::channel(command_queue_capacity);
        let (startup_sender, startup_receiver) = oneshot::channel();
        let limits = std::rc::Rc::new(std::cell::Cell::new(DecodeLimits::default()));
        let worker_limits = std::rc::Rc::clone(&limits);
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            match IndexedDbBackend::open_with_upgrade_timeout(database_name, upgrade_timeout_ms)
                .await
            {
                Ok(backend) => {
                    let status = backend.storage_status.clone();
                    if startup_sender.send(Ok(status)).is_ok() {
                        run_coordinator_worker(backend, receiver, worker_limits).await;
                    } else {
                        backend.close_without_shutdown();
                    }
                }
                Err(error) => {
                    let _ = startup_sender.send(Err(error));
                }
            }
        });
        let storage_status = startup_receiver.await.map_err(|_| {
            indexed_db_failure(
                BrowserFailureKind::PrivateModeOrUnavailable,
                "browser persistence worker stopped during startup",
            )
        })??;
        Ok(Self {
            sender,
            limits,
            queue_capacity: command_queue_capacity,
            closed: false,
            storage_status,
            _local: std::marker::PhantomData,
        })
    }

    /// Deletes a closed browser database after yielding out of the caller's event callback.
    pub async fn delete_database(database_name: &str) -> Result<(), StoreError> {
        TimeoutFuture::new(0).await;
        let delete = Rexie::delete(database_name);
        let timeout = TimeoutFuture::new(DEFAULT_UPGRADE_TIMEOUT_MS);
        pin_mut!(delete);
        pin_mut!(timeout);
        match select(delete, timeout).await {
            Either::Left((result, _)) => {
                result.map_err(|error| indexed_db_error("delete database", error))
            }
            Either::Right(((), _)) => Err(indexed_db_failure(
                BrowserFailureKind::UpgradeBlocked,
                format!(
                    "database deletion did not complete within {DEFAULT_UPGRADE_TIMEOUT_MS} ms; an open connection may be blocking it"
                ),
            )),
        }
    }

    pub fn with_decode_limits(self, limits: DecodeLimits) -> Self {
        self.limits.set(limits);
        self
    }

    pub fn storage_status(&self) -> &BrowserStorageStatus {
        &self.storage_status
    }

    pub const fn command_queue_capacity(&self) -> usize {
        self.queue_capacity
    }

    pub async fn refresh_storage_status(&mut self) -> &BrowserStorageStatus {
        if self.closed {
            self.record_coordinator_failure(
                BrowserFailureKind::PrivateModeOrUnavailable,
                "browser persistence worker is closed",
            );
            return &self.storage_status;
        }
        let (response, receiver) = oneshot::channel();
        if self
            .sender
            .send(CoordinatorRequest::RefreshStorageStatus { response })
            .await
            .is_err()
        {
            self.record_coordinator_failure(
                BrowserFailureKind::PrivateModeOrUnavailable,
                "browser persistence worker is unavailable",
            );
            return &self.storage_status;
        }
        match receiver.await {
            Ok(status) => self.storage_status = status,
            Err(_) => self.record_coordinator_failure(
                BrowserFailureKind::PrivateModeOrUnavailable,
                "browser persistence worker dropped a status response",
            ),
        }
        &self.storage_status
    }

    /// Enqueues one owned target-neutral command and asynchronously awaits the worker response.
    pub async fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
        let kind = CoordinatorResultKind::from_command(&command);
        if self.closed {
            return kind.error_result(StoreError::Closed);
        }
        let is_shutdown = matches!(kind, CoordinatorResultKind::Shutdown);
        let (response, receiver) = oneshot::channel();
        if self
            .sender
            .send(CoordinatorRequest::Execute { command, response })
            .await
            .is_err()
        {
            self.closed = true;
            self.record_coordinator_failure(
                BrowserFailureKind::PrivateModeOrUnavailable,
                "browser persistence worker is unavailable",
            );
            return kind.error_result(StoreError::Closed);
        }
        let response = match receiver.await {
            Ok(response) => response,
            Err(_) => {
                self.closed = true;
                self.record_coordinator_failure(
                    BrowserFailureKind::PrivateModeOrUnavailable,
                    "browser persistence worker dropped a command response",
                );
                return kind.error_result(StoreError::Closed);
            }
        };
        self.storage_status = response.storage_status;
        if is_shutdown {
            self.closed = true;
        }
        response.result
    }

    fn record_coordinator_failure(&mut self, kind: BrowserFailureKind, detail: &str) {
        self.storage_status.last_operation_failure = Some(kind);
        self.storage_status.last_status_detail = Some(bounded_status_detail(detail));
    }
}

#[cfg(target_arch = "wasm32")]
async fn run_coordinator_worker(
    mut backend: IndexedDbBackend,
    mut receiver: mpsc::Receiver<CoordinatorRequest>,
    limits: std::rc::Rc<std::cell::Cell<DecodeLimits>>,
) {
    while let Some(request) = receiver.next().await {
        TimeoutFuture::new(0).await;
        backend.limits = limits.get();
        match request {
            CoordinatorRequest::Execute { command, response } => {
                let is_shutdown = matches!(&command, PersistenceCommand::Shutdown(_));
                let result = backend.execute(command).await;
                backend.storage_status.record_result(&result);
                let _ = response.send(CoordinatorResponse {
                    result,
                    storage_status: backend.storage_status.clone(),
                });
                if is_shutdown {
                    return;
                }
            }
            CoordinatorRequest::RefreshStorageStatus { response } => {
                let previous = backend.storage_status.clone();
                backend.storage_status = request_storage_status().await;
                backend.storage_status.missing_or_evicted = previous.missing_or_evicted;
                backend.storage_status.last_operation_failure = previous.last_operation_failure;
                backend.storage_status.last_status_detail = previous.last_status_detail;
                let _ = response.send(backend.storage_status.clone());
            }
        }
    }
    backend.close_without_shutdown();
}

fn validate_command_queue_capacity(capacity: usize) -> Result<(), StoreError> {
    if (1..=MAX_COMMAND_QUEUE_CAPACITY).contains(&capacity) {
        Ok(())
    } else {
        Err(indexed_db_failure(
            BrowserFailureKind::Backend,
            format!(
                "command queue capacity {capacity} is outside 1..={MAX_COMMAND_QUEUE_CAPACITY}"
            ),
        ))
    }
}

#[cfg(target_arch = "wasm32")]
impl IndexedDbBackend {
    async fn open_with_upgrade_timeout(
        database_name: impl Into<String>,
        upgrade_timeout_ms: u32,
    ) -> Result<Self, StoreError> {
        let database_name = database_name.into();
        let mut builder = Rexie::builder(&database_name).version(DATABASE_VERSION);
        for store in STORE_NAMES {
            builder = builder.add_object_store(ObjectStore::new(store));
        }

        let open = builder.build();
        let timeout = TimeoutFuture::new(upgrade_timeout_ms);
        pin_mut!(open);
        pin_mut!(timeout);
        let database = match select(open, timeout).await {
            Either::Left((result, _)) => {
                result.map_err(|error| indexed_db_error("open database", error))?
            }
            Either::Right(((), _)) => {
                return Err(indexed_db_failure(
                    BrowserFailureKind::UpgradeBlocked,
                    format!(
                        "database upgrade did not complete within {upgrade_timeout_ms} ms; another open connection may be blocking it"
                    ),
                ));
            }
        };
        let expected_stores = STORE_NAMES
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let actual_stores = database.store_names().into_iter().collect::<BTreeSet<_>>();
        if actual_stores != expected_stores {
            database.close();
            return Err(indexed_db_failure(
                BrowserFailureKind::VersionMismatch,
                format!(
                    "database version {DATABASE_VERSION} has object stores {actual_stores:?}, expected {expected_stores:?}"
                ),
            ));
        }
        let storage_status = request_storage_status().await;
        Ok(Self {
            database: Some(database),
            limits: DecodeLimits::default(),
            storage_status,
            _local: std::marker::PhantomData,
        })
    }

    async fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
        if self.database.is_none() {
            return super::error_result(command, StoreError::Closed);
        }
        match command {
            PersistenceCommand::Load(request) => {
                PersistenceResult::Loaded(self.load(request).await)
            }
            PersistenceCommand::LoadProtocolState(request) => {
                PersistenceResult::ProtocolStateLoaded(
                    self.load_protocol_state(&request.application).await,
                )
            }
            PersistenceCommand::Initialize(image) => {
                PersistenceResult::Initialized(self.initialize(image).await)
            }
            PersistenceCommand::Commit(batch) => {
                PersistenceResult::Committed(self.commit(batch).await)
            }
            PersistenceCommand::Activate(batch) => {
                PersistenceResult::Activated(self.activate(batch).await)
            }
            PersistenceCommand::ResetApplication(batch) => {
                PersistenceResult::ApplicationReset(self.reset_application(batch).await)
            }
            PersistenceCommand::Barrier(request) => {
                PersistenceResult::BarrierComplete(self.barrier(request).await)
            }
            PersistenceCommand::Inspect(request) => {
                PersistenceResult::Inspected(self.inspect(request).await)
            }
            PersistenceCommand::Compact(request) => {
                PersistenceResult::Compacted(self.compact(request).await)
            }
            PersistenceCommand::ExportApplication(request) => {
                PersistenceResult::ApplicationExported(self.export_application(request).await)
            }
            PersistenceCommand::PutContentArtifact(request) => {
                PersistenceResult::ContentArtifactStored(self.put_content_artifact(request).await)
            }
            PersistenceCommand::LoadContentArtifact(request) => {
                PersistenceResult::ContentArtifactLoaded(self.load_content_artifact(request).await)
            }
            PersistenceCommand::Shutdown(_) => {
                PersistenceResult::ShutdownComplete(self.shutdown().await)
            }
        }
    }

    fn database(&self) -> Result<&Rexie, StoreError> {
        self.database.as_ref().ok_or(StoreError::Closed)
    }

    fn close_without_shutdown(mut self) {
        if let Some(database) = self.database.take() {
            database.close();
        }
    }

    async fn load(&self, request: RestoreRequest) -> Result<Option<RestoreImage>, StoreError> {
        let transaction = self
            .database()?
            .transaction(&LOAD_STORES, TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start load transaction", error))?;
        let loaded = match load_application(&transaction, &request.application, self.limits).await {
            Ok(loaded) => loaded,
            Err(error) => return abort_with(transaction, error).await,
        };
        let image = loaded.map(|loaded| loaded.image);
        if let (Some(expected), Some(image)) = (request.expected_schema_hash, image.as_ref())
            && image.schema_hash != expected
        {
            return abort_with(transaction, StoreError::SchemaMismatch).await;
        }
        commit_with(transaction, image).await
    }

    async fn load_protocol_state(
        &self,
        application: &ApplicationIdentity,
    ) -> Result<ProtocolStateSnapshot, StoreError> {
        let transaction = self
            .database()?
            .transaction(&[META, PROTOCOL_STATE], TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start protocol-state load transaction", error))?;
        match read_meta(&transaction, application, self.limits).await {
            Ok(Some(_)) => {}
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        }
        let snapshot =
            match load_protocol_state_records(&transaction, application, self.limits).await {
                Ok(snapshot) => snapshot,
                Err(error) => return abort_with(transaction, error).await,
            };
        commit_with(transaction, snapshot).await
    }

    async fn initialize(&self, image: RestoreImage) -> Result<CommitAck, StoreError> {
        super::validate_initial_image(&image)?;
        if image
            .scalars
            .keys()
            .any(|memory| image.lists.contains_key(memory))
        {
            return Err(StoreError::InvalidAuthority(
                "one memory ID cannot be both a scalar and a list".to_owned(),
            ));
        }

        let transaction = self
            .database()?
            .transaction(&STORE_NAMES, TransactionMode::ReadWrite)
            .map_err(|error| indexed_db_error("start initialize transaction", error))?;
        let existing = match load_application(&transaction, &image.application, self.limits).await {
            Ok(existing) => existing,
            Err(error) => return abort_with(transaction, error).await,
        };
        let initialization_checksum = match restore_checksum(&image) {
            Ok(checksum) => checksum,
            Err(error) => return abort_with(transaction, error).await,
        };
        if let Some(existing) = existing {
            if existing.image == image
                && existing.meta.initialization_checksum == initialization_checksum
            {
                return commit_with(
                    transaction,
                    CommitAck {
                        epoch: existing.image.epoch,
                        through_turn_sequence: existing.image.through_turn_sequence,
                    },
                )
                .await;
            }
            return abort_with(transaction, StoreError::IdentityMismatch).await;
        }

        let meta = MetaRecord {
            application: image.application.clone(),
            schema_version: image.schema_version,
            schema_hash: image.schema_hash,
            epoch: image.epoch,
            through_turn_sequence: image.through_turn_sequence,
            clean_shutdown: false,
            initialization_checksum,
        };
        let mut mutations = match stage_initial_image(&image) {
            Ok(mutations) => mutations,
            Err(error) => return abort_with(transaction, error).await,
        };
        match meta_mutation(&meta) {
            Ok(mutation) => mutations.push(mutation),
            Err(error) => return abort_with(transaction, error).await,
        }
        if let Err(error) = apply_mutations(&transaction, mutations).await {
            return abort_with(transaction, error).await;
        }
        commit_with(
            transaction,
            CommitAck {
                epoch: image.epoch,
                through_turn_sequence: image.through_turn_sequence,
            },
        )
        .await
    }

    async fn commit(&self, batch: CheckpointBatch) -> Result<CommitAck, StoreError> {
        super::validate_checkpoint(&batch)?;
        let plan = SparseTransactionPlan::checkpoint(&batch);
        let store_names = plan.store_names();
        let transaction = self
            .database()?
            .transaction(&store_names, TransactionMode::ReadWrite)
            .map_err(|error| indexed_db_error("start checkpoint transaction", error))?;
        let mut meta = match read_meta(&transaction, &batch.application, self.limits).await {
            Ok(Some(meta)) => meta,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        if let Err(error) = validate_checkpoint_header(&meta, &batch) {
            return abort_with(transaction, error).await;
        }
        if let Err(error) = apply_sparse_changes(
            &transaction,
            &batch.application,
            &batch.changes,
            self.limits,
        )
        .await
        {
            return abort_with(transaction, error).await;
        }
        if let Err(error) = apply_sparse_outbox_changes(
            &transaction,
            &batch.application,
            &batch.outbox_changes,
            self.limits,
        )
        .await
        {
            return abort_with(transaction, error).await;
        }
        if !batch.protocol_state_changes.is_empty()
            && let Err(error) = apply_sparse_protocol_state_changes(
                &transaction,
                &batch.application,
                &batch.protocol_state_changes,
                self.limits,
            )
            .await
        {
            return abort_with(transaction, error).await;
        }
        if !batch.content_artifact_changes.is_empty()
            && let Err(error) = apply_sparse_content_artifact_changes(
                &transaction,
                &batch.application,
                &batch.content_artifact_changes,
                self.limits,
            )
            .await
        {
            return abort_with(transaction, error).await;
        }

        let checkpoint = CheckpointRecord {
            kind: CheckpointKind::Checkpoint,
            base_epoch: batch.base_epoch,
            next_epoch: batch.next_epoch,
            first_turn_sequence: batch.first_turn_sequence,
            last_turn_sequence: batch.last_turn_sequence,
            schema_hash: batch.schema_hash,
            checksum: batch.checksum,
        };
        let mutations =
            match stage_checkpoint(&transaction, &batch.application, checkpoint, self.limits).await
            {
                Ok(mutations) => mutations,
                Err(error) => return abort_with(transaction, error).await,
            };
        if let Err(error) = apply_mutations(&transaction, mutations).await {
            return abort_with(transaction, error).await;
        }

        meta.epoch = batch.next_epoch;
        meta.through_turn_sequence = batch.last_turn_sequence;
        meta.clean_shutdown = false;
        let meta = match meta_mutation(&meta) {
            Ok(mutation) => mutation,
            Err(error) => return abort_with(transaction, error).await,
        };
        if let Err(error) = apply_mutations(&transaction, vec![meta]).await {
            return abort_with(transaction, error).await;
        }
        commit_with(
            transaction,
            CommitAck {
                epoch: batch.next_epoch,
                through_turn_sequence: batch.last_turn_sequence,
            },
        )
        .await
    }

    async fn activate(&self, batch: ActivationBatch) -> Result<ActivationAck, StoreError> {
        super::validate_activation(&batch)?;
        let plan = SparseTransactionPlan::activation(&batch);
        let store_names = plan.store_names();
        let transaction = self
            .database()?
            .transaction(&store_names, TransactionMode::ReadWrite)
            .map_err(|error| indexed_db_error("start activation transaction", error))?;
        let mut meta = match read_meta(&transaction, &batch.application, self.limits).await {
            Ok(Some(meta)) => meta,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        if let Err(error) = validate_activation_header(&meta, &batch) {
            return abort_with(transaction, error).await;
        }
        if let Err(error) = replace_activation_content_artifacts(
            &transaction,
            &batch.application,
            &batch.target_content_artifact_manifest,
            &batch.content_artifacts,
            self.limits,
        )
        .await
        {
            return abort_with(transaction, error).await;
        }
        if let Err(error) = apply_sparse_changes(
            &transaction,
            &batch.application,
            &batch.authority_changes,
            self.limits,
        )
        .await
        {
            return abort_with(transaction, error).await;
        }
        for memory in &batch.deleted_memory {
            if let Err(error) =
                delete_memory_sparse(&transaction, &batch.application, *memory, self.limits).await
            {
                return abort_with(transaction, error).await;
            }
        }
        if !batch.completed_migration_edges.is_empty() {
            let mut mutations = Vec::with_capacity(batch.completed_migration_edges.len());
            for edge in &batch.completed_migration_edges {
                mutations.push(StoreMutation::Put {
                    store: MIGRATIONS,
                    key: migration_storage_key(&batch.application, *edge),
                    value: batch.next_epoch.to_be_bytes().to_vec(),
                });
            }
            if let Err(error) = apply_mutations(&transaction, mutations).await {
                return abort_with(transaction, error).await;
            }
        }

        let checkpoint = CheckpointRecord {
            kind: CheckpointKind::Activation,
            base_epoch: batch.expected_base_epoch,
            next_epoch: batch.next_epoch,
            first_turn_sequence: meta.through_turn_sequence,
            last_turn_sequence: batch.through_turn_sequence,
            schema_hash: batch.target_schema_hash,
            checksum: batch.checksum,
        };
        let mutations =
            match stage_checkpoint(&transaction, &batch.application, checkpoint, self.limits).await
            {
                Ok(mutations) => mutations,
                Err(error) => return abort_with(transaction, error).await,
            };
        if let Err(error) = apply_mutations(&transaction, mutations).await {
            return abort_with(transaction, error).await;
        }

        meta.schema_version = batch.target_schema_version;
        meta.schema_hash = batch.target_schema_hash;
        meta.epoch = batch.next_epoch;
        meta.through_turn_sequence = batch.through_turn_sequence;
        meta.clean_shutdown = false;
        let mutation = match meta_mutation(&meta) {
            Ok(mutation) => mutation,
            Err(error) => return abort_with(transaction, error).await,
        };
        if let Err(error) = apply_mutations(&transaction, vec![mutation]).await {
            return abort_with(transaction, error).await;
        }
        commit_with(
            transaction,
            ActivationAck {
                epoch: meta.epoch,
                schema_version: meta.schema_version,
                schema_hash: meta.schema_hash,
                through_turn_sequence: meta.through_turn_sequence,
            },
        )
        .await
    }

    async fn reset_application(
        &self,
        batch: ResetApplicationBatch,
    ) -> Result<ResetApplicationAck, StoreError> {
        super::validate_reset(&batch)?;
        let transaction = self
            .database()?
            .transaction(&STORE_NAMES, TransactionMode::ReadWrite)
            .map_err(|error| indexed_db_error("start reset transaction", error))?;
        let current = match read_meta(&transaction, &batch.application, self.limits).await {
            Ok(Some(meta)) => meta,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        if current.epoch != batch.expected_base_epoch
            || batch.next_epoch
                != match batch.expected_base_epoch.checked_add(1) {
                    Some(epoch) => epoch,
                    None => return abort_with(transaction, StoreError::StaleEpoch).await,
                }
        {
            return abort_with(transaction, StoreError::StaleEpoch).await;
        }
        if current.schema_hash != batch.source_schema_hash {
            return abort_with(transaction, StoreError::SchemaMismatch).await;
        }
        let prefix = application_storage_key(&batch.application);
        for store in [
            SLOTS,
            LISTS,
            ROWS,
            MIGRATIONS,
            OUTBOX,
            PROTOCOL_STATE,
            BLOBS,
            ARTIFACTS,
            ARTIFACT_OWNERS,
            CHECKPOINTS,
        ] {
            if let Err(error) =
                delete_prefix_records(&transaction, store, &prefix, self.limits).await
            {
                return abort_with(transaction, error).await;
            }
        }

        let reset_record = CheckpointRecord {
            kind: CheckpointKind::Reset,
            base_epoch: batch.expected_base_epoch,
            next_epoch: batch.next_epoch,
            first_turn_sequence: current.through_turn_sequence,
            last_turn_sequence: current.through_turn_sequence,
            schema_hash: batch.default_image.schema_hash,
            checksum: batch.checksum,
        };
        let mut mutations = vec![StoreMutation::Put {
            store: CHECKPOINTS,
            key: checkpoint_storage_key(&batch.application, batch.next_epoch),
            value: match encode_checkpoint_record(&reset_record) {
                Ok(value) => value,
                Err(error) => return abort_with(transaction, codec_backend(error)).await,
            },
        }];
        let meta = MetaRecord {
            application: batch.application.clone(),
            schema_version: batch.default_image.schema_version,
            schema_hash: batch.default_image.schema_hash,
            epoch: batch.next_epoch,
            through_turn_sequence: current.through_turn_sequence,
            clean_shutdown: false,
            initialization_checksum: match restore_checksum(&batch.default_image) {
                Ok(checksum) => checksum,
                Err(error) => return abort_with(transaction, error).await,
            },
        };
        match meta_mutation(&meta) {
            Ok(mutation) => mutations.push(mutation),
            Err(error) => return abort_with(transaction, error).await,
        }
        if let Err(error) = apply_mutations(&transaction, mutations).await {
            return abort_with(transaction, error).await;
        }
        commit_with(
            transaction,
            ResetApplicationAck {
                epoch: meta.epoch,
                schema_version: meta.schema_version,
                schema_hash: meta.schema_hash,
                through_turn_sequence: meta.through_turn_sequence,
            },
        )
        .await
    }

    async fn barrier(&self, request: BarrierRequest) -> Result<BarrierAck, StoreError> {
        let transaction = self
            .database()?
            .transaction(&[META], TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start barrier transaction", error))?;
        let meta = match read_meta(&transaction, &request.application, self.limits).await {
            Ok(Some(meta)) => meta,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        if meta.epoch < request.through_epoch {
            return abort_with(transaction, StoreError::StaleEpoch).await;
        }
        commit_with(transaction, BarrierAck { epoch: meta.epoch }).await
    }

    async fn inspect(
        &self,
        request: InspectRequest,
    ) -> Result<Option<PersistenceInspectorSnapshot>, StoreError> {
        let transaction = self
            .database()?
            .transaction(&LOAD_STORES, TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start inspect transaction", error))?;
        let snapshot =
            match inspect_application(&transaction, &request.application, self.limits).await {
                Ok(snapshot) => snapshot,
                Err(error) => return abort_with(transaction, error).await,
            };
        commit_with(transaction, snapshot).await
    }

    async fn compact(&self, request: CompactRequest) -> Result<CompactAck, StoreError> {
        let transaction = self
            .database()?
            .transaction(
                &[
                    META,
                    CHECKPOINTS,
                    SLOTS,
                    ROWS,
                    BLOBS,
                    ARTIFACTS,
                    ARTIFACT_OWNERS,
                ],
                TransactionMode::ReadWrite,
            )
            .map_err(|error| indexed_db_error("start compact transaction", error))?;
        let meta = match read_meta(&transaction, &request.application, self.limits).await {
            Ok(Some(meta)) => meta,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        let mut mutations =
            match stage_checkpoint_pruning(&transaction, &request.application, self.limits).await {
                Ok(mutations) => mutations,
                Err(error) => return abort_with(transaction, error).await,
            };
        match stage_blob_reclamation(&transaction, &request.application, self.limits).await {
            Ok(blob_mutations) => mutations.extend(blob_mutations),
            Err(error) => return abort_with(transaction, error).await,
        }
        match stage_content_artifact_reclamation(&transaction, &request.application, self.limits)
            .await
        {
            Ok(artifact_mutations) => mutations.extend(artifact_mutations),
            Err(error) => return abort_with(transaction, error).await,
        }
        if let Err(error) = apply_mutations(&transaction, mutations).await {
            return abort_with(transaction, error).await;
        }
        commit_with(transaction, CompactAck { epoch: meta.epoch }).await
    }

    async fn put_content_artifact(
        &self,
        request: PutContentArtifactRequest,
    ) -> Result<PutContentArtifactAck, StoreError> {
        validate_content_artifact(&request.artifact)?;
        let transaction = self
            .database()?
            .transaction(
                &[META, ARTIFACTS, ARTIFACT_OWNERS],
                TransactionMode::ReadWrite,
            )
            .map_err(|error| indexed_db_error("start artifact staging transaction", error))?;
        match read_meta(&transaction, &request.application, self.limits).await {
            Ok(Some(_)) => {}
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        }
        let manifest =
            match load_content_artifact_manifest(&transaction, &request.application, self.limits)
                .await
            {
                Ok(manifest) => manifest,
                Err(error) => return abort_with(transaction, error).await,
            };
        let mut artifacts =
            match load_content_artifacts(&transaction, &request.application, self.limits).await {
                Ok(artifacts) => artifacts,
                Err(error) => return abort_with(transaction, error).await,
            };
        let already_present = match artifacts.get(&request.artifact.id) {
            Some(existing) if existing == &request.artifact => true,
            Some(_) => {
                return abort_with(
                    transaction,
                    StoreError::InvalidContentArtifact(
                        "content digest collides with different artifact bytes".to_owned(),
                    ),
                )
                .await;
            }
            None => {
                artifacts.insert(request.artifact.id, request.artifact.clone());
                false
            }
        };
        if let Err(error) = validate_content_artifact_storage(&manifest, &artifacts) {
            return abort_with(transaction, error).await;
        }
        if !already_present {
            let mutation = StoreMutation::Put {
                store: ARTIFACTS,
                key: content_artifact_storage_key(&request.application, request.artifact.id),
                value: match encode_content_artifact(&request.artifact) {
                    Ok(value) => value,
                    Err(error) => return abort_with(transaction, codec_backend(error)).await,
                },
            };
            if let Err(error) = apply_mutations(&transaction, vec![mutation]).await {
                return abort_with(transaction, error).await;
            }
        }
        commit_with(
            transaction,
            PutContentArtifactAck {
                id: request.artifact.id,
                stored_bytes: request.artifact.bytes.len().try_into().unwrap_or(u64::MAX),
                already_present,
            },
        )
        .await
    }

    async fn load_content_artifact(
        &self,
        request: LoadContentArtifactRequest,
    ) -> Result<Option<ContentArtifact>, StoreError> {
        let transaction = self
            .database()?
            .transaction(&[META, ARTIFACTS], TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start artifact load transaction", error))?;
        match read_meta(&transaction, &request.application, self.limits).await {
            Ok(Some(_)) => {}
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        }
        let key = content_artifact_storage_key(&request.application, request.id);
        let bytes = match read_store_bytes(&transaction, ARTIFACTS, &key).await {
            Ok(bytes) => bytes,
            Err(error) => return abort_with(transaction, error).await,
        };
        let artifact = match bytes {
            Some(bytes) => match decode_content_artifact(request.id, &bytes, self.limits) {
                Ok(artifact) => Some(artifact),
                Err(error) => return abort_with(transaction, codec_backend(error)).await,
            },
            None => None,
        };
        commit_with(transaction, artifact).await
    }

    async fn export_application(
        &self,
        request: ExportApplicationRequest,
    ) -> Result<ApplicationTransfer, StoreError> {
        let transaction = self
            .database()?
            .transaction(&LOAD_STORES, TransactionMode::ReadOnly)
            .map_err(|error| indexed_db_error("start export transaction", error))?;
        let loaded = match load_application(&transaction, &request.application, self.limits).await {
            Ok(Some(loaded)) => loaded,
            Ok(None) => return abort_with(transaction, StoreError::MissingApplication).await,
            Err(error) => return abort_with(transaction, error).await,
        };
        let available =
            match load_content_artifacts(&transaction, &request.application, self.limits).await {
                Ok(artifacts) => artifacts,
                Err(error) => return abort_with(transaction, error).await,
            };
        let content_artifacts = match exact_content_artifact_closure(
            &loaded.image.content_artifact_manifest,
            &available,
        ) {
            Ok(artifacts) => artifacts,
            Err(error) => return abort_with(transaction, error).await,
        };
        let transfer = ApplicationTransfer {
            restore_image: loaded.image,
            content_artifacts,
        };
        if let Err(error) = validate_application_transfer(&transfer) {
            return abort_with(transaction, error).await;
        }
        commit_with(transaction, transfer).await
    }

    async fn shutdown(&mut self) -> Result<ShutdownAck, StoreError> {
        let database = self.database.take().ok_or(StoreError::Closed)?;
        let transaction = match database.transaction(&[META], TransactionMode::ReadWrite) {
            Ok(transaction) => transaction,
            Err(error) => {
                database.close();
                return Err(indexed_db_error("start shutdown transaction", error));
            }
        };
        let result = mark_clean_shutdown(&transaction, self.limits).await;
        let result = match result {
            Ok(mutations) => match apply_mutations(&transaction, mutations).await {
                Ok(()) => commit_with(transaction, ShutdownAck).await,
                Err(error) => abort_with(transaction, error).await,
            },
            Err(error) => abort_with(transaction, error).await,
        };
        database.close();
        result
    }
}

#[cfg(target_arch = "wasm32")]
fn validate_checkpoint_header(
    meta: &MetaRecord,
    batch: &CheckpointBatch,
) -> Result<(), StoreError> {
    if meta.schema_hash != batch.schema_hash {
        return Err(StoreError::SchemaMismatch);
    }
    if meta.epoch != batch.base_epoch
        || batch.next_epoch
            != batch
                .base_epoch
                .checked_add(1)
                .ok_or(StoreError::StaleEpoch)?
    {
        return Err(StoreError::StaleEpoch);
    }
    if batch.first_turn_sequence
        != meta
            .through_turn_sequence
            .checked_add(1)
            .ok_or(StoreError::NonContiguousTurn)?
        || batch.last_turn_sequence < batch.first_turn_sequence
    {
        return Err(StoreError::NonContiguousTurn);
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn validate_activation_header(
    meta: &MetaRecord,
    batch: &ActivationBatch,
) -> Result<(), StoreError> {
    if meta.epoch != batch.expected_base_epoch
        || batch.next_epoch
            != batch
                .expected_base_epoch
                .checked_add(1)
                .ok_or(StoreError::StaleEpoch)?
    {
        return Err(StoreError::StaleEpoch);
    }
    if meta.schema_hash != batch.source_schema_hash {
        return Err(StoreError::SchemaMismatch);
    }
    if batch.through_turn_sequence < meta.through_turn_sequence {
        return Err(StoreError::NonContiguousTurn);
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn apply_sparse_changes(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    changes: &[DurableChange],
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    for change in changes {
        match change {
            DurableChange::SetScalar { memory_id, value } => {
                if store_key_exists(
                    transaction,
                    LISTS,
                    &memory_storage_key(application, *memory_id),
                )
                .await?
                {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a list"
                    )));
                }
                let key = memory_storage_key(application, *memory_id);
                let old = read_store_bytes(transaction, SLOTS, &key).await?;
                let encoded = encode_scalar_component(value).map_err(codec_backend)?;
                replace_encoded_component(
                    transaction,
                    application,
                    SLOTS,
                    key,
                    old,
                    Some(encoded),
                    limits,
                )
                .await?;
            }
            DurableChange::DeleteScalar { memory_id } => {
                delete_scalar_sparse(transaction, application, *memory_id, limits).await?;
            }
            DurableChange::SetList { memory_id, value } => {
                if store_key_exists(
                    transaction,
                    SLOTS,
                    &memory_storage_key(application, *memory_id),
                )
                .await?
                {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                super::validate_list(value)?;
                replace_list_sparse(transaction, application, *memory_id, value, limits).await?;
            }
            DurableChange::SetRowField {
                memory_id,
                row_key,
                row_generation,
                field_id,
                value,
            } => {
                if store_key_exists(
                    transaction,
                    SLOTS,
                    &memory_storage_key(application, *memory_id),
                )
                .await?
                {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                let mut list =
                    load_list_record_sparse(transaction, application, *memory_id, limits)
                        .await?
                        .unwrap_or(ListRecord {
                            touched: false,
                            next_key: 0,
                            rows: Vec::new(),
                        });
                let row_ref = RowRef {
                    key: *row_key,
                    generation: *row_generation,
                };
                let storage_key = row_storage_key(application, *memory_id, row_ref);
                let (mut row, old) = if list.rows.contains(&row_ref) {
                    let old = read_store_bytes(transaction, ROWS, &storage_key)
                        .await?
                        .ok_or_else(|| corrupt("list references a missing row"))?;
                    let row = decode_sparse_row(
                        transaction,
                        application,
                        *memory_id,
                        row_ref,
                        &old,
                        limits,
                    )
                    .await?;
                    (row, Some(old))
                } else if list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "list {memory_id} has no row {row_key}:{row_generation}"
                    )));
                } else {
                    list.rows.push(row_ref);
                    (
                        StoredRow {
                            key: *row_key,
                            generation: *row_generation,
                            fields: BTreeMap::new(),
                            touched_fields: BTreeSet::new(),
                        },
                        read_store_bytes(transaction, ROWS, &storage_key).await?,
                    )
                };
                row.fields.insert(*field_id, value.clone());
                row.touched_fields.insert(*field_id);
                validate_stored_row(&row, !list.touched)?;
                let encoded = encode_row_component(&row).map_err(codec_backend)?;
                replace_encoded_component(
                    transaction,
                    application,
                    ROWS,
                    storage_key,
                    old,
                    Some(encoded),
                    limits,
                )
                .await?;
                save_list_record_sparse(transaction, application, *memory_id, &list).await?;
            }
            DurableChange::InsertRow {
                memory_id,
                index,
                row,
                next_key,
            } => {
                let mut list = load_list_record_sparse(
                    transaction,
                    application,
                    *memory_id,
                    limits,
                )
                .await?
                .ok_or_else(|| {
                    StoreError::InvalidAuthority(format!(
                        "cannot insert into list {memory_id} before its structure is materialized"
                    ))
                })?;
                if !list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "cannot insert into sparse override list {memory_id}"
                    )));
                }
                let row_ref = RowRef {
                    key: row.key,
                    generation: row.generation,
                };
                if list.rows.contains(&row_ref) {
                    return Err(StoreError::InvalidAuthority(format!(
                        "list {memory_id} already has row {}:{}",
                        row.key, row.generation
                    )));
                }
                let index = usize::try_from(*index).map_err(|_| {
                    StoreError::InvalidAuthority(format!(
                        "row insertion index {index} does not fit this target"
                    ))
                })?;
                if index > list.rows.len() {
                    return Err(StoreError::InvalidAuthority(format!(
                        "row insertion index {index} exceeds list {memory_id} length {}",
                        list.rows.len()
                    )));
                }
                validate_stored_row(row, false)?;
                let key = row_storage_key(application, *memory_id, row_ref);
                if read_store_bytes(transaction, ROWS, &key).await?.is_some() {
                    return Err(corrupt("row store contains unreferenced authority"));
                }
                list.rows.insert(index, row_ref);
                list.next_key = *next_key;
                validate_list_record(&list).map_err(codec_backend)?;
                let encoded = encode_row_component(row).map_err(codec_backend)?;
                replace_encoded_component(
                    transaction,
                    application,
                    ROWS,
                    key,
                    None,
                    Some(encoded),
                    limits,
                )
                .await?;
                save_list_record_sparse(transaction, application, *memory_id, &list).await?;
            }
            DurableChange::RemoveRow {
                memory_id,
                row_key,
                row_generation,
                next_key,
            } => {
                let mut list =
                    load_list_record_sparse(transaction, application, *memory_id, limits)
                        .await?
                        .ok_or_else(|| {
                            StoreError::InvalidAuthority(format!(
                                "cannot remove from missing list {memory_id}"
                            ))
                        })?;
                if !list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "cannot remove from sparse override list {memory_id}"
                    )));
                }
                let row_ref = RowRef {
                    key: *row_key,
                    generation: *row_generation,
                };
                let index = list
                    .rows
                    .iter()
                    .position(|row| *row == row_ref)
                    .ok_or_else(|| {
                        StoreError::InvalidAuthority(format!(
                            "list {memory_id} has no row {row_key}:{row_generation}"
                        ))
                    })?;
                list.rows.remove(index);
                list.next_key = *next_key;
                validate_list_record(&list).map_err(codec_backend)?;
                let key = row_storage_key(application, *memory_id, row_ref);
                let old = read_store_bytes(transaction, ROWS, &key)
                    .await?
                    .ok_or_else(|| corrupt("list references a missing row"))?;
                replace_encoded_component(
                    transaction,
                    application,
                    ROWS,
                    key,
                    Some(old),
                    None,
                    limits,
                )
                .await?;
                save_list_record_sparse(transaction, application, *memory_id, &list).await?;
            }
            DurableChange::DeleteList { memory_id } => {
                delete_list_sparse(transaction, application, *memory_id, limits).await?;
            }
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn apply_sparse_outbox_changes(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    changes: &[DurableOutboxChange],
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    for change in changes {
        let item_id = match change {
            DurableOutboxChange::Enqueue { item } => item.item_id,
            DurableOutboxChange::BeginDispatch { item_id, .. }
            | DurableOutboxChange::RequireReconciliation { item_id, .. }
            | DurableOutboxChange::Complete { item_id, .. } => *item_id,
        };
        let key = outbox_storage_key(application, item_id);
        let existing = read_store_bytes(transaction, OUTBOX, &key).await?;
        let mut outbox = BTreeMap::new();
        if let Some(bytes) = existing.as_ref() {
            let item = decode_outbox_record(bytes, limits).map_err(codec_backend)?;
            if item.item_id != item_id {
                return Err(corrupt("outbox payload identity does not match its key"));
            }
            outbox.insert(item_id, item);
        }
        super::apply_durable_outbox_changes(&mut outbox, std::slice::from_ref(change))?;
        super::validate_outbox(&outbox)?;
        match outbox.get(&item_id) {
            Some(item) => {
                let value = encode_outbox_record(item).map_err(codec_backend)?;
                if existing.as_deref() != Some(value.as_slice()) {
                    apply_mutations(
                        transaction,
                        vec![StoreMutation::Put {
                            store: OUTBOX,
                            key,
                            value,
                        }],
                    )
                    .await?;
                }
            }
            None if existing.is_some() => {
                apply_mutations(
                    transaction,
                    vec![StoreMutation::Delete { store: OUTBOX, key }],
                )
                .await?;
            }
            None => return Err(corrupt("outbox transition has no target item")),
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn load_protocol_state_records(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<ProtocolStateSnapshot, StoreError> {
    let prefix = application_storage_key(application);
    let mut records = BTreeMap::new();
    let mut decoded_bytes = 0usize;
    for (storage_key, bytes) in scan_prefix(transaction, PROTOCOL_STATE, &prefix, limits).await? {
        decoded_bytes = decoded_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| corrupt("protocol-state byte count overflow"))?;
        if decoded_bytes > limits.max_total_bytes {
            return Err(corrupt("protocol-state records exceed decode limit"));
        }
        let key = protocol_state_from_storage_key(&storage_key)?;
        let record = decode_protocol_state_record(&bytes, limits).map_err(codec_backend)?;
        if records.insert(key, record).is_some() {
            return Err(corrupt("duplicate protocol-state key"));
        }
    }
    validate_protocol_state(&records)?;
    Ok(ProtocolStateSnapshot { records })
}

#[cfg(target_arch = "wasm32")]
async fn apply_sparse_protocol_state_changes(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    changes: &[DurableProtocolStateChange],
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let current = load_protocol_state_records(transaction, application, limits).await?;
    let mut candidate = current.clone();
    apply_durable_protocol_state_changes(&mut candidate.records, changes)?;

    let mut mutations = Vec::new();
    for key in current
        .records
        .keys()
        .filter(|key| !candidate.records.contains_key(key))
    {
        mutations.push(StoreMutation::Delete {
            store: PROTOCOL_STATE,
            key: protocol_state_storage_key(application, *key),
        });
    }
    for (key, record) in &candidate.records {
        if current.records.get(key) == Some(record) {
            continue;
        }
        mutations.push(StoreMutation::Put {
            store: PROTOCOL_STATE,
            key: protocol_state_storage_key(application, *key),
            value: encode_protocol_state_record(record).map_err(codec_backend)?,
        });
    }
    apply_mutations(transaction, mutations).await
}

#[cfg(target_arch = "wasm32")]
async fn replace_list_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    list: &StoredList,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    delete_rows_sparse(transaction, application, memory, limits).await?;
    let record = ListRecord {
        touched: list.touched,
        next_key: list.next_key,
        rows: list
            .rows
            .iter()
            .map(|row| RowRef {
                key: row.key,
                generation: row.generation,
            })
            .collect(),
    };
    save_list_record_sparse(transaction, application, memory, &record).await?;
    for row in &list.rows {
        let row_ref = RowRef {
            key: row.key,
            generation: row.generation,
        };
        validate_stored_row(row, !list.touched)?;
        let encoded = encode_row_component(row).map_err(codec_backend)?;
        replace_encoded_component(
            transaction,
            application,
            ROWS,
            row_storage_key(application, memory, row_ref),
            None,
            Some(encoded),
            limits,
        )
        .await?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn delete_memory_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    delete_scalar_sparse(transaction, application, memory, limits).await?;
    delete_list_sparse(transaction, application, memory, limits).await
}

#[cfg(target_arch = "wasm32")]
async fn delete_scalar_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let key = memory_storage_key(application, memory);
    let Some(old) = read_store_bytes(transaction, SLOTS, &key).await? else {
        return Ok(());
    };
    replace_encoded_component(
        transaction,
        application,
        SLOTS,
        key,
        Some(old),
        None,
        limits,
    )
    .await
}

#[cfg(target_arch = "wasm32")]
async fn delete_list_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    apply_mutations(
        transaction,
        vec![StoreMutation::Delete {
            store: LISTS,
            key: memory_storage_key(application, memory),
        }],
    )
    .await?;
    delete_rows_sparse(transaction, application, memory, limits).await
}

#[cfg(target_arch = "wasm32")]
async fn delete_rows_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let prefix = memory_storage_key(application, memory);
    for (key, old) in scan_prefix(transaction, ROWS, &prefix, limits).await? {
        let (stored_memory, _) = row_from_storage_key(&key)?;
        if stored_memory != memory {
            return Err(corrupt("row range returned another memory identity"));
        }
        replace_encoded_component(transaction, application, ROWS, key, Some(old), None, limits)
            .await?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn load_list_record_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<Option<ListRecord>, StoreError> {
    let key = memory_storage_key(application, memory);
    read_store_bytes(transaction, LISTS, &key)
        .await?
        .map(|bytes| decode_list_record(&bytes, limits).map_err(codec_backend))
        .transpose()
}

#[cfg(target_arch = "wasm32")]
async fn save_list_record_sparse(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    list: &ListRecord,
) -> Result<(), StoreError> {
    validate_list_record(list).map_err(codec_backend)?;
    apply_mutations(
        transaction,
        vec![StoreMutation::Put {
            store: LISTS,
            key: memory_storage_key(application, memory),
            value: encode_list_record(list).map_err(codec_backend)?,
        }],
    )
    .await
}

#[cfg(target_arch = "wasm32")]
async fn decode_sparse_row(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    memory: MemoryId,
    row_ref: RowRef,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<StoredRow, StoreError> {
    let references = row_component_blob_references(bytes, limits).map_err(codec_backend)?;
    let blobs = load_referenced_blobs(transaction, application, &references, limits).await?;
    let row = decode_row_component(bytes, limits, &blobs).map_err(codec_backend)?;
    if row.key != row_ref.key || row.generation != row_ref.generation {
        return Err(corrupt("row payload identity does not match its key"));
    }
    let key = row_storage_key(application, memory, row_ref);
    if row_from_storage_key(&key)? != (memory, row_ref) {
        return Err(corrupt("row key identity is not canonical"));
    }
    Ok(row)
}

#[cfg(target_arch = "wasm32")]
async fn load_referenced_blobs(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    references: &BTreeMap<BlobDigest, u64>,
    limits: DecodeLimits,
) -> Result<BTreeMap<BlobDigest, BlobRecord>, StoreError> {
    if references.len() > limits.max_collection_items {
        return Err(corrupt("component blob references exceed item limit"));
    }
    let mut blobs = BTreeMap::new();
    let mut decoded_bytes = 0usize;
    for digest in references.keys() {
        let key = blob_storage_key(application, *digest);
        let bytes = read_store_bytes(transaction, BLOBS, &key)
            .await?
            .ok_or_else(|| corrupt("component references a missing blob"))?;
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let record = decode_blob_record(&bytes, limits).map_err(codec_backend)?;
        if record.digest != *digest {
            return Err(corrupt("blob payload digest does not match its key"));
        }
        blobs.insert(*digest, record);
    }
    for (digest, count) in references {
        let record = blobs
            .get(digest)
            .ok_or_else(|| corrupt("component references a missing blob"))?;
        if record.reference_count < *count {
            return Err(corrupt("blob reference count is below component usage"));
        }
    }
    Ok(blobs)
}

#[cfg(target_arch = "wasm32")]
async fn replace_encoded_component(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    store: &'static str,
    key: String,
    old: Option<Vec<u8>>,
    new: Option<EncodedComponent>,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let old_references = old
        .as_deref()
        .map(|bytes| component_blob_references(store, bytes, limits))
        .transpose()?
        .unwrap_or_default();
    let new_references = new
        .as_ref()
        .map(|component| component.references.clone())
        .unwrap_or_default();
    let new_blobs = new
        .as_ref()
        .map(|component| &component.blobs)
        .cloned()
        .unwrap_or_default();
    apply_blob_reference_transition(
        transaction,
        application,
        &old_references,
        &new_references,
        &new_blobs,
        limits,
    )
    .await?;
    let mutation = match new {
        Some(component) => StoreMutation::Put {
            store,
            key,
            value: component.bytes,
        },
        None => StoreMutation::Delete { store, key },
    };
    apply_mutations(transaction, vec![mutation]).await
}

#[cfg(target_arch = "wasm32")]
fn component_blob_references(
    store: &'static str,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<BTreeMap<BlobDigest, u64>, StoreError> {
    match store {
        SLOTS => scalar_component_blob_references(bytes, limits).map_err(codec_backend),
        ROWS => row_component_blob_references(bytes, limits).map_err(codec_backend),
        _ => Err(corrupt(format!(
            "store {store} does not contain blob-bearing components"
        ))),
    }
}

#[cfg(target_arch = "wasm32")]
async fn apply_blob_reference_transition(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    old: &BTreeMap<BlobDigest, u64>,
    new: &BTreeMap<BlobDigest, u64>,
    new_blobs: &BTreeMap<BlobDigest, BlobRecord>,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let digests = old
        .keys()
        .chain(new.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    if digests.len() > limits.max_collection_items {
        return Err(corrupt("component blob delta exceeds item limit"));
    }
    for digest in digests {
        let old_count = old.get(&digest).copied().unwrap_or(0);
        let new_count = new.get(&digest).copied().unwrap_or(0);
        let key = blob_storage_key(application, digest);
        let stored = read_store_bytes(transaction, BLOBS, &key)
            .await?
            .map(|bytes| decode_blob_record(&bytes, limits).map_err(codec_backend))
            .transpose()?;
        if old_count > 0 && stored.is_none() {
            return Err(corrupt("component references a missing blob"));
        }
        if let Some(record) = stored.as_ref() {
            if record.digest != digest {
                return Err(corrupt("blob payload digest does not match its key"));
            }
            if record.reference_count < old_count {
                return Err(corrupt("blob reference count underflow"));
            }
        }
        if new_count > 0 {
            let candidate = new_blobs
                .get(&digest)
                .ok_or_else(|| corrupt("encoded component omitted referenced blob content"))?;
            if candidate.reference_count != new_count {
                return Err(corrupt("encoded component blob count is inconsistent"));
            }
            if let Some(record) = stored.as_ref()
                && (record.length != candidate.length || record.bytes != candidate.bytes)
            {
                return Err(corrupt("blob digest collision"));
            }
        }
        let current_count = stored
            .as_ref()
            .map(|record| record.reference_count)
            .unwrap_or(0);
        let next_count = current_count
            .checked_sub(old_count)
            .and_then(|count| count.checked_add(new_count))
            .ok_or_else(|| corrupt("blob reference count overflow"))?;
        if next_count == current_count {
            continue;
        }
        let mutation = if next_count == 0 {
            StoreMutation::Delete { store: BLOBS, key }
        } else {
            let mut record = match stored {
                Some(record) => record,
                None => new_blobs
                    .get(&digest)
                    .cloned()
                    .ok_or_else(|| corrupt("new blob content is missing"))?,
            };
            record.reference_count = next_count;
            StoreMutation::Put {
                store: BLOBS,
                key,
                value: encode_blob_record(&record).map_err(codec_backend)?,
            }
        };
        apply_mutations(transaction, vec![mutation]).await?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn read_store_bytes(
    transaction: &Transaction,
    store_name: &'static str,
    key: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let store = transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?;
    store
        .get(JsValue::from_str(key))
        .await
        .map_err(|error| indexed_db_error(&format!("read {store_name} record"), error))?
        .map(js_bytes)
        .transpose()
}

#[cfg(target_arch = "wasm32")]
async fn store_key_exists(
    transaction: &Transaction,
    store_name: &'static str,
    key: &str,
) -> Result<bool, StoreError> {
    transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?
        .key_exists(JsValue::from_str(key))
        .await
        .map_err(|error| indexed_db_error(&format!("read {store_name} key"), error))
}

fn validate_stored_row(row: &StoredRow, sparse: bool) -> Result<(), StoreError> {
    if !row
        .touched_fields
        .iter()
        .all(|field| row.fields.contains_key(field))
    {
        return Err(StoreError::InvalidAuthority(format!(
            "row {}:{} touches a missing field",
            row.key, row.generation
        )));
    }
    if sparse
        && (row.touched_fields.is_empty()
            || row
                .fields
                .keys()
                .any(|field| !row.touched_fields.contains(field)))
    {
        return Err(StoreError::InvalidAuthority(format!(
            "sparse row {}:{} contains non-override fields",
            row.key, row.generation
        )));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn inspect_application(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Option<PersistenceInspectorSnapshot>, StoreError> {
    let Some(meta) = read_meta(transaction, application, limits).await? else {
        return Ok(None);
    };
    let prefix = application_storage_key(application);
    let scalar_count = count_prefix(transaction, SLOTS, &prefix, limits).await?;
    let list_count = count_prefix(transaction, LISTS, &prefix, limits).await?;
    let row_count = count_prefix(transaction, ROWS, &prefix, limits).await?;
    let completed_migration_count = count_prefix(transaction, MIGRATIONS, &prefix, limits).await?;
    let mut outbox = BTreeMap::new();
    let mut decoded_bytes = 0usize;
    for (key, bytes) in scan_prefix(transaction, OUTBOX, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let item_id = outbox_from_storage_key(&key)?;
        let item = decode_outbox_record(&bytes, limits).map_err(codec_backend)?;
        if item.item_id != item_id {
            return Err(corrupt("outbox payload identity does not match its key"));
        }
        if outbox.insert(item_id, item).is_some() {
            return Err(corrupt("duplicate outbox item key"));
        }
    }
    super::validate_outbox(&outbox)?;
    let content_artifact_manifest =
        load_content_artifact_manifest(transaction, application, limits).await?;
    let content_artifacts = load_content_artifacts(transaction, application, limits).await?;
    let artifact_stats =
        validate_content_artifact_storage(&content_artifact_manifest, &content_artifacts)?;
    let mut image = RestoreImage::empty(
        meta.application.clone(),
        meta.schema_version,
        meta.schema_hash,
    );
    image.epoch = meta.epoch;
    image.through_turn_sequence = meta.through_turn_sequence;
    image.outbox = outbox;
    image.content_artifact_manifest = content_artifact_manifest;
    let mut snapshot = inspector_snapshot_with_artifacts(&image, artifact_stats);
    snapshot.scalar_count = scalar_count;
    snapshot.list_count = list_count;
    snapshot.row_count = row_count;
    snapshot.encoded_value_bytes = None;
    snapshot.completed_migration_count = completed_migration_count;
    Ok(Some(snapshot))
}

#[cfg(target_arch = "wasm32")]
async fn stage_blob_reclamation(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Vec<StoreMutation>, StoreError> {
    let prefix = application_storage_key(application);
    let mut actual = BTreeMap::new();
    for (_, bytes) in scan_prefix(transaction, SLOTS, &prefix, limits).await? {
        merge_blob_references(
            &mut actual,
            &scalar_component_blob_references(&bytes, limits).map_err(codec_backend)?,
        )?;
    }
    for (_, bytes) in scan_prefix(transaction, ROWS, &prefix, limits).await? {
        merge_blob_references(
            &mut actual,
            &row_component_blob_references(&bytes, limits).map_err(codec_backend)?,
        )?;
    }

    let mut current = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, BLOBS, &prefix, limits).await? {
        let digest = blob_from_storage_key(&key)?;
        let record = decode_blob_record(&bytes, limits).map_err(codec_backend)?;
        if record.digest != digest {
            return Err(corrupt("blob payload digest does not match its key"));
        }
        current.insert(digest, record);
    }

    let mut mutations = Vec::new();
    for digest in current.keys() {
        if !actual.contains_key(digest) {
            mutations.push(StoreMutation::Delete {
                store: BLOBS,
                key: blob_storage_key(application, *digest),
            });
        }
    }
    for (digest, reference_count) in actual {
        let mut record = current
            .get(&digest)
            .cloned()
            .ok_or_else(|| corrupt("component references a missing blob during compaction"))?;
        if record.reference_count != reference_count {
            record.reference_count = reference_count;
            mutations.push(StoreMutation::Put {
                store: BLOBS,
                key: blob_storage_key(application, digest),
                value: encode_blob_record(&record).map_err(codec_backend)?,
            });
        }
    }
    Ok(mutations)
}

#[cfg(target_arch = "wasm32")]
async fn stage_content_artifact_reclamation(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Vec<StoreMutation>, StoreError> {
    let manifest = load_content_artifact_manifest(transaction, application, limits).await?;
    let artifacts = load_content_artifacts(transaction, application, limits).await?;
    exact_content_artifact_closure(&manifest, &artifacts)?;
    let reachable = manifest.reachable_artifact_ids();
    Ok(artifacts
        .keys()
        .filter(|id| !reachable.contains(id))
        .map(|id| StoreMutation::Delete {
            store: ARTIFACTS,
            key: content_artifact_storage_key(application, *id),
        })
        .collect())
}

#[cfg(target_arch = "wasm32")]
async fn request_storage_status() -> BrowserStorageStatus {
    let Some(manager) = storage_manager() else {
        let detail = bounded_status_detail("StorageManager is unavailable in this browser context");
        return BrowserStorageStatus {
            persistence: BrowserPersistenceGrant::Unavailable {
                detail: detail.clone(),
            },
            usage_bytes: None,
            quota_bytes: None,
            estimate_error: Some(detail.clone()),
            quota_failure: Some(BrowserFailureKind::PrivateModeOrUnavailable),
            missing_or_evicted: false,
            last_operation_failure: Some(BrowserFailureKind::PrivateModeOrUnavailable),
            last_status_detail: Some(detail),
        };
    };

    let (persistence, persistence_failure, persistence_detail) = match manager.persist() {
        Ok(promise) => match await_browser_promise(
            promise,
            STORAGE_STATUS_TIMEOUT_MS,
            "StorageManager.persist()",
        )
        .await
        {
            Ok(value) if value.as_bool() == Some(true) => {
                (BrowserPersistenceGrant::Granted, None, None)
            }
            Ok(value) if value.as_bool() == Some(false) => {
                (BrowserPersistenceGrant::Denied, None, None)
            }
            Ok(_) => {
                let detail =
                    bounded_status_detail("StorageManager.persist() returned a non-boolean value");
                (
                    BrowserPersistenceGrant::Unavailable {
                        detail: detail.clone(),
                    },
                    Some(BrowserFailureKind::PrivateModeOrUnavailable),
                    Some(detail),
                )
            }
            Err(failure) if failure.kind == BrowserFailureKind::Timeout => (
                BrowserPersistenceGrant::TimedOut {
                    timeout_ms: STORAGE_STATUS_TIMEOUT_MS,
                },
                Some(failure.kind),
                Some(failure.detail),
            ),
            Err(failure) => (
                BrowserPersistenceGrant::Unavailable {
                    detail: failure.detail.clone(),
                },
                Some(failure.kind),
                Some(failure.detail),
            ),
        },
        Err(error) => {
            let detail = bounded_status_detail(js_error_detail(&error));
            (
                BrowserPersistenceGrant::Unavailable {
                    detail: detail.clone(),
                },
                Some(classify_indexed_db_detail(&detail)),
                Some(detail),
            )
        }
    };

    let (usage_bytes, quota_bytes, estimate_error, mut quota_failure) = match manager.estimate() {
        Ok(promise) => match await_browser_promise(
            promise,
            STORAGE_STATUS_TIMEOUT_MS,
            "StorageManager.estimate()",
        )
        .await
        {
            Ok(estimate) => (
                reflected_u64(&estimate, "usage"),
                reflected_u64(&estimate, "quota"),
                None,
                None,
            ),
            Err(failure) => (None, None, Some(failure.detail), Some(failure.kind)),
        },
        Err(error) => {
            let detail = bounded_status_detail(js_error_detail(&error));
            let kind = classify_indexed_db_detail(&detail);
            (None, None, Some(detail), Some(kind))
        }
    };
    if usage_bytes
        .zip(quota_bytes)
        .is_some_and(|(usage, quota)| usage > quota)
    {
        quota_failure = Some(BrowserFailureKind::QuotaExceeded);
    }
    let last_operation_failure = quota_failure.or(persistence_failure);
    let last_status_detail = estimate_error.clone().or(persistence_detail);
    BrowserStorageStatus {
        persistence,
        usage_bytes,
        quota_bytes,
        estimate_error,
        quota_failure,
        missing_or_evicted: false,
        last_operation_failure,
        last_status_detail,
    }
}

#[cfg(target_arch = "wasm32")]
struct BrowserPromiseFailure {
    kind: BrowserFailureKind,
    detail: String,
}

#[cfg(target_arch = "wasm32")]
async fn await_browser_promise(
    promise: js_sys::Promise,
    timeout_ms: u32,
    label: &str,
) -> Result<JsValue, BrowserPromiseFailure> {
    let future = JsFuture::from(promise);
    let timeout = TimeoutFuture::new(timeout_ms);
    pin_mut!(future);
    pin_mut!(timeout);
    match select(future, timeout).await {
        Either::Left((result, _)) => result.map_err(|error| {
            let detail = bounded_status_detail(js_error_detail(&error));
            BrowserPromiseFailure {
                kind: classify_indexed_db_detail(&detail),
                detail,
            }
        }),
        Either::Right(((), _)) => Err(BrowserPromiseFailure {
            kind: BrowserFailureKind::Timeout,
            detail: bounded_status_detail(format!("{label} did not settle within {timeout_ms} ms")),
        }),
    }
}

#[cfg(target_arch = "wasm32")]
fn storage_manager() -> Option<StorageManager> {
    if let Some(window) = web_sys::window() {
        return Some(window.navigator().storage());
    }
    js_sys::global()
        .dyn_into::<WorkerGlobalScope>()
        .ok()
        .map(|worker| worker.navigator().storage())
}

#[cfg(target_arch = "wasm32")]
fn reflected_u64(value: &JsValue, field: &str) -> Option<u64> {
    let value = Reflect::get(value, &JsValue::from_str(field))
        .ok()?
        .as_f64()?;
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64)
        .then_some(value as u64)
}

#[cfg(target_arch = "wasm32")]
fn js_error_detail(error: &JsValue) -> String {
    if let Some(exception) = error.dyn_ref::<DomException>() {
        return format!("{}: {}", exception.name(), exception.message());
    }
    error
        .as_string()
        .unwrap_or_else(|| format!("JavaScript exception: {error:?}"))
}

#[cfg(target_arch = "wasm32")]
async fn load_content_artifact_manifest(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<ContentArtifactManifest, StoreError> {
    let prefix = application_storage_key(application);
    let mut bindings = BTreeMap::new();
    let mut decoded_bytes = 0usize;
    let owner_limits = DecodeLimits {
        max_collection_items: super::MAX_CONTENT_ARTIFACT_OWNERS,
        ..limits
    };
    for (key, bytes) in scan_prefix(transaction, ARTIFACT_OWNERS, &prefix, owner_limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let owner_id = content_artifact_owner_from_storage_key(&key)?;
        let binding = decode_content_artifact_binding(&bytes, limits).map_err(codec_backend)?;
        if bindings.insert(owner_id, binding).is_some() {
            return Err(corrupt("duplicate content artifact owner key"));
        }
    }
    let manifest = ContentArtifactManifest { bindings };
    validate_content_artifact_manifest(&manifest)?;
    Ok(manifest)
}

#[cfg(target_arch = "wasm32")]
async fn load_content_artifacts(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<BTreeMap<ContentArtifactId, ContentArtifact>, StoreError> {
    let prefix = application_storage_key(application);
    let mut artifacts = BTreeMap::new();
    let artifact_limits = DecodeLimits {
        max_collection_items: super::MAX_RETAINED_CONTENT_ARTIFACTS
            + super::MAX_STAGED_CONTENT_ARTIFACTS,
        ..limits
    };
    let mut total_bytes = 0usize;
    for (key, bytes) in scan_prefix(transaction, ARTIFACTS, &prefix, artifact_limits).await? {
        let id = content_artifact_from_storage_key(&key)?;
        let artifact = decode_content_artifact(id, &bytes, limits).map_err(codec_backend)?;
        total_bytes = total_bytes
            .checked_add(artifact.bytes.len())
            .ok_or_else(|| corrupt("content artifact byte count overflow"))?;
        if total_bytes
            > super::MAX_RETAINED_CONTENT_ARTIFACT_BYTES + super::MAX_STAGED_CONTENT_ARTIFACT_BYTES
        {
            return Err(corrupt("content artifact store exceeds bounded byte count"));
        }
        if artifacts.insert(id, artifact).is_some() {
            return Err(corrupt("duplicate content artifact key"));
        }
        if artifacts.len()
            > super::MAX_RETAINED_CONTENT_ARTIFACTS + super::MAX_STAGED_CONTENT_ARTIFACTS
        {
            return Err(corrupt(
                "content artifact store exceeds bounded record count",
            ));
        }
    }
    Ok(artifacts)
}

#[cfg(target_arch = "wasm32")]
async fn apply_sparse_content_artifact_changes(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    changes: &[DurableContentArtifactChange],
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let current = load_content_artifact_manifest(transaction, application, limits).await?;
    let mut candidate = current.clone();
    apply_durable_content_artifact_changes(&mut candidate, changes)?;
    let artifacts = load_content_artifacts(transaction, application, limits).await?;
    validate_content_artifact_storage(&candidate, &artifacts)?;
    let mut mutations = Vec::new();
    for owner_id in current.bindings.keys() {
        if !candidate.bindings.contains_key(owner_id) {
            mutations.push(StoreMutation::Delete {
                store: ARTIFACT_OWNERS,
                key: content_artifact_owner_storage_key(application, *owner_id),
            });
        }
    }
    for (owner_id, binding) in &candidate.bindings {
        if current.bindings.get(owner_id) == Some(binding) {
            continue;
        }
        mutations.push(StoreMutation::Put {
            store: ARTIFACT_OWNERS,
            key: content_artifact_owner_storage_key(application, *owner_id),
            value: encode_content_artifact_binding(*binding).map_err(codec_backend)?,
        });
    }
    apply_mutations(transaction, mutations).await
}

#[cfg(target_arch = "wasm32")]
async fn replace_activation_content_artifacts(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    target_manifest: &ContentArtifactManifest,
    supplied: &BTreeMap<ContentArtifactId, ContentArtifact>,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let mut available = load_content_artifacts(transaction, application, limits).await?;
    for (id, artifact) in supplied {
        match available.get(id) {
            Some(existing) if existing == artifact => {}
            Some(_) => {
                return Err(StoreError::InvalidContentArtifact(
                    "content digest collides with different artifact bytes".to_owned(),
                ));
            }
            None => {
                available.insert(*id, artifact.clone());
            }
        }
    }
    let target_artifacts = exact_content_artifact_closure(target_manifest, &available)?;
    validate_content_artifact_storage(target_manifest, &target_artifacts)?;
    let prefix = application_storage_key(application);
    delete_prefix_records(transaction, ARTIFACTS, &prefix, limits).await?;
    delete_prefix_records(transaction, ARTIFACT_OWNERS, &prefix, limits).await?;
    let mut mutations = Vec::with_capacity(target_artifacts.len() + target_manifest.bindings.len());
    for (id, artifact) in target_artifacts {
        mutations.push(StoreMutation::Put {
            store: ARTIFACTS,
            key: content_artifact_storage_key(application, id),
            value: encode_content_artifact(&artifact).map_err(codec_backend)?,
        });
    }
    for (owner_id, binding) in &target_manifest.bindings {
        mutations.push(StoreMutation::Put {
            store: ARTIFACT_OWNERS,
            key: content_artifact_owner_storage_key(application, *owner_id),
            value: encode_content_artifact_binding(*binding).map_err(codec_backend)?,
        });
    }
    apply_mutations(transaction, mutations).await
}

#[cfg(target_arch = "wasm32")]
async fn load_application(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Option<LoadedApplication>, StoreError> {
    let Some(meta) = read_meta(transaction, application, limits).await? else {
        return Ok(None);
    };
    let prefix = application_storage_key(application);
    let mut decoded_bytes = 0usize;
    let mut blobs = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, BLOBS, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let digest = blob_from_storage_key(&key)?;
        let record = decode_blob_record(&bytes, limits).map_err(codec_backend)?;
        if record.digest != digest {
            return Err(corrupt("blob payload digest does not match its key"));
        }
        if blobs.insert(digest, record).is_some() {
            return Err(corrupt("duplicate blob digest key"));
        }
    }
    let mut actual_blob_references = BTreeMap::new();

    let mut scalars = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, SLOTS, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let memory = memory_from_storage_key(&key, "slot")?;
        merge_blob_references(
            &mut actual_blob_references,
            &scalar_component_blob_references(&bytes, limits).map_err(codec_backend)?,
        )?;
        let scalar = decode_scalar_component(&bytes, limits, &blobs).map_err(codec_backend)?;
        if scalars.insert(memory, scalar).is_some() {
            return Err(corrupt("duplicate scalar memory key"));
        }
    }

    let mut row_records = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, ROWS, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let (memory, row_ref) = row_from_storage_key(&key)?;
        merge_blob_references(
            &mut actual_blob_references,
            &row_component_blob_references(&bytes, limits).map_err(codec_backend)?,
        )?;
        let row = decode_row_component(&bytes, limits, &blobs).map_err(codec_backend)?;
        if row.key != row_ref.key || row.generation != row_ref.generation {
            return Err(corrupt("row payload identity does not match its key"));
        }
        if row_records.insert((memory, row_ref), row).is_some() {
            return Err(corrupt("duplicate row key"));
        }
    }

    let mut lists = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, LISTS, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let memory = memory_from_storage_key(&key, "list")?;
        if scalars.contains_key(&memory) {
            return Err(corrupt("one memory key is both a scalar and a list"));
        }
        let record = decode_list_record(&bytes, limits).map_err(codec_backend)?;
        let mut rows = Vec::with_capacity(record.rows.len());
        for row_ref in &record.rows {
            let row = row_records
                .remove(&(memory, *row_ref))
                .ok_or_else(|| corrupt("list order references a missing row"))?;
            rows.push(row);
        }
        let list = StoredList {
            touched: record.touched,
            next_key: record.next_key,
            rows,
        };
        super::validate_list(&list)?;
        if lists.insert(memory, list).is_some() {
            return Err(corrupt("duplicate list memory key"));
        }
    }
    if !row_records.is_empty() {
        return Err(corrupt("row store contains unreferenced authority"));
    }

    let mut completed_migration_edges = BTreeSet::new();
    for (key, bytes) in scan_prefix(transaction, MIGRATIONS, &prefix, limits).await? {
        if key.len() != 128 || bytes.len() != 8 {
            return Err(corrupt("invalid migration record"));
        }
        let edge = MigrationEdgeId(decode_hex_digest(&key[64..])?);
        if !completed_migration_edges.insert(edge) {
            return Err(corrupt("duplicate migration edge key"));
        }
    }

    let mut outbox = BTreeMap::new();
    for (key, bytes) in scan_prefix(transaction, OUTBOX, &prefix, limits).await? {
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let item_id = outbox_from_storage_key(&key)?;
        let item = decode_outbox_record(&bytes, limits).map_err(codec_backend)?;
        if item.item_id != item_id {
            return Err(corrupt("outbox payload identity does not match its key"));
        }
        if outbox.insert(item_id, item).is_some() {
            return Err(corrupt("duplicate outbox item key"));
        }
    }
    super::validate_outbox(&outbox)?;
    let content_artifact_manifest =
        load_content_artifact_manifest(transaction, application, limits).await?;
    let content_artifacts = load_content_artifacts(transaction, application, limits).await?;
    validate_content_artifact_storage(&content_artifact_manifest, &content_artifacts)?;
    validate_blob_reference_counts(&blobs, &actual_blob_references)?;

    let image = RestoreImage {
        application: meta.application.clone(),
        schema_version: meta.schema_version,
        schema_hash: meta.schema_hash,
        epoch: meta.epoch,
        through_turn_sequence: meta.through_turn_sequence,
        scalars,
        lists,
        completed_migration_edges,
        outbox,
        content_artifact_manifest,
    };
    Ok(Some(LoadedApplication { image, meta }))
}

#[cfg(target_arch = "wasm32")]
async fn read_meta(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Option<MetaRecord>, StoreError> {
    let store = transaction
        .store(META)
        .map_err(|error| indexed_db_error("open meta store", error))?;
    let key = application_storage_key(application);
    let Some(value) = store
        .get(JsValue::from_str(&key))
        .await
        .map_err(|error| indexed_db_error("read application metadata", error))?
    else {
        return Ok(None);
    };
    let bytes = js_bytes(value)?;
    let meta = decode_meta(&bytes, limits).map_err(codec_backend)?;
    if &meta.application != application {
        return Err(StoreError::IdentityMismatch);
    }
    Ok(Some(meta))
}

#[cfg(target_arch = "wasm32")]
async fn scan_prefix(
    transaction: &Transaction,
    store_name: &'static str,
    prefix: &str,
    limits: DecodeLimits,
) -> Result<Vec<(String, Vec<u8>)>, StoreError> {
    let store = transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?;
    let range = indexed_db_prefix_range(prefix)?;
    let entries = store
        .scan(Some(range), Some(bounded_scan_limit(limits)?), None, None)
        .await
        .map_err(|error| indexed_db_error(&format!("scan {store_name} store"), error))?;
    if entries.len() > limits.max_collection_items {
        return Err(corrupt(format!(
            "{store_name} prefix contains more than {} records",
            limits.max_collection_items
        )));
    }
    let mut matching = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let key = js_key(key)?;
        if !key.starts_with(prefix) {
            return Err(corrupt(format!(
                "{store_name} range returned a key outside its requested prefix"
            )));
        }
        matching.push((key, js_bytes(value)?));
    }
    Ok(matching)
}

#[cfg(target_arch = "wasm32")]
async fn scan_keys_prefix(
    transaction: &Transaction,
    store_name: &'static str,
    prefix: &str,
    limits: DecodeLimits,
) -> Result<Vec<String>, StoreError> {
    let store = transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?;
    let range = indexed_db_prefix_range(prefix)?;
    let keys = store
        .get_all_keys(Some(range), Some(bounded_scan_limit(limits)?))
        .await
        .map_err(|error| indexed_db_error(&format!("scan {store_name} keys"), error))?;
    if keys.len() > limits.max_collection_items {
        return Err(corrupt(format!(
            "{store_name} prefix contains more than {} keys",
            limits.max_collection_items
        )));
    }
    let mut matching = Vec::with_capacity(keys.len());
    for key in keys {
        let key = js_key(key)?;
        if !key.starts_with(prefix) {
            return Err(corrupt(format!(
                "{store_name} range returned a key outside its requested prefix"
            )));
        }
        matching.push(key);
    }
    Ok(matching)
}

#[cfg(target_arch = "wasm32")]
async fn count_prefix(
    transaction: &Transaction,
    store_name: &'static str,
    prefix: &str,
    limits: DecodeLimits,
) -> Result<usize, StoreError> {
    let store = transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?;
    let count = store
        .count(Some(indexed_db_prefix_range(prefix)?))
        .await
        .map_err(|error| indexed_db_error(&format!("count {store_name} records"), error))?
        as usize;
    if count > limits.max_collection_items {
        return Err(corrupt(format!(
            "{store_name} prefix contains {count} records, exceeding limit {}",
            limits.max_collection_items
        )));
    }
    Ok(count)
}

#[cfg(target_arch = "wasm32")]
async fn delete_prefix_records(
    transaction: &Transaction,
    store_name: &'static str,
    prefix: &str,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let keys = scan_keys_prefix(transaction, store_name, prefix, limits).await?;
    let store = transaction
        .store(store_name)
        .map_err(|error| indexed_db_error(&format!("open {store_name} store"), error))?;
    for key in keys {
        store
            .delete(JsValue::from_str(&key))
            .await
            .map_err(|error| indexed_db_error(&format!("delete {store_name} record"), error))?;
    }
    Ok(())
}

fn stage_initial_image(image: &RestoreImage) -> Result<Vec<StoreMutation>, StoreError> {
    let application = &image.application;
    let mut mutations = Vec::new();

    for (memory, scalar) in &image.scalars {
        mutations.push(StoreMutation::Put {
            store: SLOTS,
            key: memory_storage_key(application, *memory),
            value: encode_scalar_component(scalar)
                .map_err(codec_backend)?
                .bytes,
        });
    }

    for (memory, list) in &image.lists {
        super::validate_list(list)?;
        let record = ListRecord {
            touched: list.touched,
            next_key: list.next_key,
            rows: list
                .rows
                .iter()
                .map(|row| RowRef {
                    key: row.key,
                    generation: row.generation,
                })
                .collect(),
        };
        mutations.push(StoreMutation::Put {
            store: LISTS,
            key: memory_storage_key(application, *memory),
            value: encode_list_record(&record).map_err(codec_backend)?,
        });
        for row in &list.rows {
            mutations.push(StoreMutation::Put {
                store: ROWS,
                key: row_storage_key(
                    application,
                    *memory,
                    RowRef {
                        key: row.key,
                        generation: row.generation,
                    },
                ),
                value: encode_row_component(row).map_err(codec_backend)?.bytes,
            });
        }
    }

    for edge in &image.completed_migration_edges {
        mutations.push(StoreMutation::Put {
            store: MIGRATIONS,
            key: migration_storage_key(application, *edge),
            value: image.epoch.to_be_bytes().to_vec(),
        });
    }

    for (item_id, item) in &image.outbox {
        mutations.push(StoreMutation::Put {
            store: OUTBOX,
            key: outbox_storage_key(application, *item_id),
            value: encode_outbox_record(item).map_err(codec_backend)?,
        });
    }

    for (digest, record) in collect_image_blobs(image)? {
        mutations.push(StoreMutation::Put {
            store: BLOBS,
            key: blob_storage_key(application, digest),
            value: encode_blob_record(&record).map_err(codec_backend)?,
        });
    }
    for (owner_id, binding) in &image.content_artifact_manifest.bindings {
        mutations.push(StoreMutation::Put {
            store: ARTIFACT_OWNERS,
            key: content_artifact_owner_storage_key(application, *owner_id),
            value: encode_content_artifact_binding(*binding).map_err(codec_backend)?,
        });
    }
    Ok(mutations)
}

fn collect_image_blobs(
    image: &RestoreImage,
) -> Result<BTreeMap<BlobDigest, BlobRecord>, StoreError> {
    let mut blobs = BTreeMap::new();
    for scalar in image.scalars.values() {
        merge_encoded_blobs(
            &mut blobs,
            encode_scalar_component(scalar).map_err(codec_backend)?,
        )?;
    }
    for row in image.lists.values().flat_map(|list| &list.rows) {
        merge_encoded_blobs(
            &mut blobs,
            encode_row_component(row).map_err(codec_backend)?,
        )?;
    }
    Ok(blobs)
}

fn merge_encoded_blobs(
    target: &mut BTreeMap<BlobDigest, BlobRecord>,
    encoded: EncodedComponent,
) -> Result<(), StoreError> {
    for (digest, record) in encoded.blobs {
        match target.entry(digest) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(record);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let current = entry.get_mut();
                if current.length != record.length || current.bytes != record.bytes {
                    return Err(corrupt("blob digest collision"));
                }
                current.reference_count = current
                    .reference_count
                    .checked_add(record.reference_count)
                    .ok_or_else(|| corrupt("blob reference count overflow"))?;
            }
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn meta_mutation(meta: &MetaRecord) -> Result<StoreMutation, StoreError> {
    Ok(StoreMutation::Put {
        store: META,
        key: application_storage_key(&meta.application),
        value: encode_meta(meta).map_err(codec_backend)?,
    })
}

#[cfg(target_arch = "wasm32")]
async fn stage_checkpoint(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    record: CheckpointRecord,
    limits: DecodeLimits,
) -> Result<Vec<StoreMutation>, StoreError> {
    let prefix = application_storage_key(application);
    let mut keys = checkpoint_keys(transaction, &prefix, limits).await?;
    let key = checkpoint_storage_key(application, record.next_epoch);
    keys.insert(key.clone());
    let mut mutations = vec![StoreMutation::Put {
        store: CHECKPOINTS,
        key,
        value: encode_checkpoint_record(&record).map_err(codec_backend)?,
    }];
    let remove_count = keys
        .len()
        .saturating_sub(MAX_CHECKPOINT_RECORDS_PER_APPLICATION);
    mutations.extend(
        keys.into_iter()
            .take(remove_count)
            .map(|key| StoreMutation::Delete {
                store: CHECKPOINTS,
                key,
            }),
    );
    Ok(mutations)
}

#[cfg(target_arch = "wasm32")]
async fn stage_checkpoint_pruning(
    transaction: &Transaction,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Vec<StoreMutation>, StoreError> {
    let prefix = application_storage_key(application);
    let keys = checkpoint_keys(transaction, &prefix, limits).await?;
    let remove_count = keys
        .len()
        .saturating_sub(MAX_CHECKPOINT_RECORDS_PER_APPLICATION);
    Ok(keys
        .into_iter()
        .take(remove_count)
        .map(|key| StoreMutation::Delete {
            store: CHECKPOINTS,
            key,
        })
        .collect())
}

#[cfg(target_arch = "wasm32")]
async fn checkpoint_keys(
    transaction: &Transaction,
    application_prefix: &str,
    limits: DecodeLimits,
) -> Result<BTreeSet<String>, StoreError> {
    let checkpoint_limits = DecodeLimits {
        max_collection_items: MAX_CHECKPOINT_RECORDS_PER_APPLICATION,
        ..limits
    };
    let keys = scan_keys_prefix(
        transaction,
        CHECKPOINTS,
        application_prefix,
        checkpoint_limits,
    )
    .await?;
    let mut checked = BTreeSet::new();
    for key in keys {
        if key.len() != 80 {
            return Err(corrupt("invalid checkpoint key"));
        }
        decode_hex_u64(&key[64..])?;
        checked.insert(key);
    }
    Ok(checked)
}

#[cfg(target_arch = "wasm32")]
async fn mark_clean_shutdown(
    transaction: &Transaction,
    limits: DecodeLimits,
) -> Result<Vec<StoreMutation>, StoreError> {
    let store = transaction
        .store(META)
        .map_err(|error| indexed_db_error("open meta store", error))?;
    let entries = store
        .scan(None, Some(bounded_scan_limit(limits)?), None, None)
        .await
        .map_err(|error| indexed_db_error("scan application metadata", error))?;
    if entries.len() > limits.max_collection_items {
        return Err(corrupt(format!(
            "metadata contains more than {} applications",
            limits.max_collection_items
        )));
    }
    let mut mutations = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let key = js_key(key)?;
        let bytes = js_bytes(value)?;
        let mut meta = decode_meta(&bytes, limits).map_err(codec_backend)?;
        if key != application_storage_key(&meta.application) {
            return Err(corrupt("metadata key does not match application identity"));
        }
        meta.clean_shutdown = true;
        mutations.push(StoreMutation::Put {
            store: META,
            key,
            value: encode_meta(&meta).map_err(codec_backend)?,
        });
    }
    Ok(mutations)
}

#[cfg(target_arch = "wasm32")]
async fn apply_mutations(
    transaction: &Transaction,
    mutations: Vec<StoreMutation>,
) -> Result<(), StoreError> {
    for mutation in mutations {
        match mutation {
            StoreMutation::Put { store, key, value } => {
                let object_store = transaction
                    .store(store)
                    .map_err(|error| indexed_db_error(&format!("open {store} store"), error))?;
                let key = JsValue::from_str(&key);
                let value = Uint8Array::from(value.as_slice()).into();
                object_store
                    .put(&value, Some(&key))
                    .await
                    .map_err(|error| {
                        indexed_db_error(&format!("write {store} component"), error)
                    })?;
            }
            StoreMutation::Delete { store, key } => {
                let object_store = transaction
                    .store(store)
                    .map_err(|error| indexed_db_error(&format!("open {store} store"), error))?;
                object_store
                    .delete(JsValue::from_str(&key))
                    .await
                    .map_err(|error| {
                        indexed_db_error(&format!("delete {store} component"), error)
                    })?;
            }
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn commit_with<T>(transaction: Transaction, value: T) -> Result<T, StoreError> {
    transaction
        .commit()
        .await
        .map_err(|error| indexed_db_error("commit transaction", error))?;
    Ok(value)
}

#[cfg(target_arch = "wasm32")]
async fn abort_with<T>(transaction: Transaction, error: StoreError) -> Result<T, StoreError> {
    let _ = transaction.abort().await;
    Err(error)
}

#[cfg(target_arch = "wasm32")]
fn js_key(value: JsValue) -> Result<String, StoreError> {
    value
        .as_string()
        .ok_or_else(|| corrupt("IndexedDB component key is not a string"))
}

#[cfg(target_arch = "wasm32")]
fn js_bytes(value: JsValue) -> Result<Vec<u8>, StoreError> {
    if !value.is_instance_of::<Uint8Array>() {
        return Err(corrupt("IndexedDB component value is not a Uint8Array"));
    }
    Ok(Uint8Array::new(&value).to_vec())
}

#[cfg(target_arch = "wasm32")]
fn add_decode_bytes(
    total: &mut usize,
    added: usize,
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    *total = total
        .checked_add(added)
        .ok_or_else(|| corrupt("restore byte count overflow"))?;
    if *total > limits.max_total_bytes {
        return Err(corrupt("restore components exceed total byte limit"));
    }
    Ok(())
}

fn persistence_result_error(result: &PersistenceResult) -> Option<&StoreError> {
    match result {
        PersistenceResult::Loaded(Err(error))
        | PersistenceResult::ProtocolStateLoaded(Err(error))
        | PersistenceResult::Initialized(Err(error))
        | PersistenceResult::Committed(Err(error))
        | PersistenceResult::Activated(Err(error))
        | PersistenceResult::ApplicationReset(Err(error))
        | PersistenceResult::BarrierComplete(Err(error))
        | PersistenceResult::Inspected(Err(error))
        | PersistenceResult::Compacted(Err(error))
        | PersistenceResult::ApplicationExported(Err(error))
        | PersistenceResult::ContentArtifactStored(Err(error))
        | PersistenceResult::ContentArtifactLoaded(Err(error))
        | PersistenceResult::ShutdownComplete(Err(error)) => Some(error),
        _ => None,
    }
}

fn bounded_status_detail(detail: impl Into<String>) -> String {
    let mut detail = detail.into();
    if detail.len() <= MAX_STATUS_DETAIL_BYTES {
        return detail;
    }
    let mut end = MAX_STATUS_DETAIL_BYTES.saturating_sub(3);
    while !detail.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    detail.truncate(end);
    detail.push_str("...");
    detail
}

fn classify_indexed_db_detail(detail: &str) -> BrowserFailureKind {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("quotaexceeded") || lower.contains("quota exceeded") {
        BrowserFailureKind::QuotaExceeded
    } else if lower.contains("securityerror")
        || lower.contains("notallowederror")
        || lower.contains("unknownerror")
        || lower.contains("private mode")
        || lower.contains("indexeddb is unavailable")
    {
        BrowserFailureKind::PrivateModeOrUnavailable
    } else if lower.contains("upgrade blocked") || lower.contains("blocked upgrade") {
        BrowserFailureKind::UpgradeBlocked
    } else if lower.contains("timed out") || lower.contains("timeout") {
        BrowserFailureKind::Timeout
    } else if lower.contains("transactionabort")
        || lower.contains("transaction abort")
        || lower.contains("transactioncommitfailed")
        || lower.contains("aborterror")
    {
        BrowserFailureKind::TransactionAborted
    } else if lower.contains("invalidstateerror")
        || lower.contains("versionchange")
        || lower.contains("version change")
        || lower.contains("database is closed")
    {
        BrowserFailureKind::VersionChangeClosed
    } else if lower.contains("versionerror") || lower.contains("version error") {
        BrowserFailureKind::VersionMismatch
    } else {
        BrowserFailureKind::Backend
    }
}

#[cfg(target_arch = "wasm32")]
fn indexed_db_error(context: &str, error: impl fmt::Display + fmt::Debug) -> StoreError {
    let detail = bounded_status_detail(format!("{context}: {error}; {error:?}"));
    let kind = classify_indexed_db_detail(&detail);
    indexed_db_failure(kind, detail)
}

fn indexed_db_failure(kind: BrowserFailureKind, detail: impl fmt::Display) -> StoreError {
    StoreError::Backend(format!(
        "indexeddb/{}: {}",
        kind.code(),
        bounded_status_detail(detail.to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::codec::{decode_blob_record, decode_outbox_record};
    use crate::{
        DurableContentArtifactChange, DurableProtocolStateChange, StoredScalar, StoredValue,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use crate::{DurableEffectRow, DurableOutboxItem, StoredList, StoredRow};
    use boon_plan::MemoryLeafId;
    #[cfg(not(target_arch = "wasm32"))]
    use boon_plan::{EffectId, EffectInvocationId, MemoryKind, MemoryOwnerPath};

    fn number(value: i64) -> StoredValue {
        StoredValue::integer(value).unwrap()
    }

    fn application() -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.web", "golden", "browser")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn memory(name: &str, kind: MemoryKind) -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "web_golden".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            name,
            kind,
        )
        .unwrap()
    }

    fn digest(bytes: &[u8]) -> String {
        encode_hex(&Sha256::digest(bytes))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn browser_scalar_and_row_records_match_the_native_component_codec() {
        let scalar = StoredScalar {
            touched: true,
            value: StoredValue::Record(BTreeMap::from([
                ("a".to_owned(), number(-7)),
                (
                    "b".to_owned(),
                    StoredValue::List(vec![
                        StoredValue::Bool(true),
                        StoredValue::Text("Boon".to_owned()),
                    ]),
                ),
            ])),
        };
        let scalar_bytes = encode_scalar_component(&scalar).unwrap().bytes;
        assert_eq!(
            decode_scalar_component(&scalar_bytes, DecodeLimits::default(), &BTreeMap::new(),)
                .unwrap(),
            scalar
        );

        let list = memory("rows", MemoryKind::List);
        let first = MemoryLeafId::from_memory_path(list, "first").unwrap();
        let second = MemoryLeafId::from_memory_path(list, "second").unwrap();
        let row = StoredRow {
            key: 9,
            generation: 3,
            fields: BTreeMap::from([
                (first, StoredValue::Text("text".to_owned())),
                (second, StoredValue::Bytes(vec![0, 1, 2, 255].into())),
            ]),
            touched_fields: BTreeSet::from([first, second]),
        };
        let row_bytes = encode_row_component(&row).unwrap().bytes;
        assert_eq!(
            decode_row_component(&row_bytes, DecodeLimits::default(), &BTreeMap::new()).unwrap(),
            row
        );
    }

    #[test]
    fn browser_component_encodings_have_stable_goldens() {
        let meta = MetaRecord {
            application: application(),
            schema_version: 3,
            schema_hash: [0x11; 32],
            epoch: 7,
            through_turn_sequence: 19,
            clean_shutdown: false,
            initialization_checksum: [0x22; 32],
        };
        let list = ListRecord {
            touched: true,
            next_key: 12,
            rows: vec![
                RowRef {
                    key: 2,
                    generation: 1,
                },
                RowRef {
                    key: 9,
                    generation: 4,
                },
            ],
        };
        let checkpoint = CheckpointRecord {
            kind: CheckpointKind::Activation,
            base_epoch: 6,
            next_epoch: 7,
            first_turn_sequence: 17,
            last_turn_sequence: 19,
            schema_hash: [0x11; 32],
            checksum: [0x33; 32],
        };

        let meta_bytes = encode_meta(&meta).unwrap();
        let list_bytes = encode_list_record(&list).unwrap();
        let checkpoint_bytes = encode_checkpoint_record(&checkpoint).unwrap();
        assert_eq!(
            decode_meta(&meta_bytes, DecodeLimits::default()).unwrap(),
            meta
        );
        assert_eq!(
            decode_list_record(&list_bytes, DecodeLimits::default()).unwrap(),
            list
        );
        assert_eq!(
            (
                digest(&meta_bytes),
                digest(&list_bytes),
                digest(&checkpoint_bytes),
            ),
            (
                "dcead203cbea9ef5b6f3ca6594f2c52757da82f09de18d36a5dcffd1f67376f0".to_owned(),
                "863f07dbb1aa47deb2c72da21816526daecbe8a63be38a510021c86b08cfd011".to_owned(),
                "1c225555fb7a2e5c16308493b20a803dc47a1fdd06429b8c0c630e5b8f05eece".to_owned(),
            )
        );
    }

    #[test]
    fn browser_component_keys_have_stable_goldens() {
        const APP: &str = "985614200bdf68926a748b4f177f416295d08db8ab9600b5f499759a25c18a16";
        let app = application();
        let memory = MemoryId([0x33; 32]);
        let edge = MigrationEdgeId([0x44; 32]);
        assert_eq!(
            (
                application_storage_key(&app),
                memory_storage_key(&app, memory),
                row_storage_key(
                    &app,
                    memory,
                    RowRef {
                        key: 7,
                        generation: 2,
                    },
                ),
                checkpoint_storage_key(&app, 5),
                migration_storage_key(&app, edge),
            ),
            (
                APP.to_owned(),
                concat!(
                    "985614200bdf68926a748b4f177f416295d08db8ab9600b5f499759a25c18a16",
                    "3333333333333333333333333333333333333333333333333333333333333333"
                )
                .to_owned(),
                concat!(
                    "985614200bdf68926a748b4f177f416295d08db8ab9600b5f499759a25c18a16",
                    "3333333333333333333333333333333333333333333333333333333333333333",
                    "00000000000000070000000000000002"
                )
                .to_owned(),
                concat!(
                    "985614200bdf68926a748b4f177f416295d08db8ab9600b5f499759a25c18a16",
                    "0000000000000005"
                )
                .to_owned(),
                concat!(
                    "985614200bdf68926a748b4f177f416295d08db8ab9600b5f499759a25c18a16",
                    "4444444444444444444444444444444444444444444444444444444444444444"
                )
                .to_owned(),
            )
        );
    }

    #[test]
    fn browser_artifact_owner_records_and_keys_are_canonical() {
        let artifact = ContentArtifact::new("text/plain", b"browser artifact".to_vec()).unwrap();
        let owner_id = ContentArtifactOwnerId([0x44; 32]);
        let binding = ContentArtifactBinding {
            artifact_id: artifact.id,
            retention: ContentArtifactRetention::Immutable,
        };
        let artifact_bytes = encode_content_artifact(&artifact).unwrap();
        let binding_bytes = encode_content_artifact_binding(binding).unwrap();
        assert_eq!(
            decode_content_artifact(artifact.id, &artifact_bytes, DecodeLimits::default()).unwrap(),
            artifact
        );
        assert_eq!(
            decode_content_artifact_binding(&binding_bytes, DecodeLimits::default()).unwrap(),
            binding
        );
        assert_eq!(encode_content_artifact(&artifact).unwrap(), artifact_bytes);
        assert_eq!(
            encode_content_artifact_binding(binding).unwrap(),
            binding_bytes
        );
        let artifact_key = content_artifact_storage_key(&application(), artifact.id);
        let owner_key = content_artifact_owner_storage_key(&application(), owner_id);
        assert_eq!(
            content_artifact_from_storage_key(&artifact_key).unwrap(),
            artifact.id
        );
        assert_eq!(
            content_artifact_owner_from_storage_key(&owner_key).unwrap(),
            owner_id
        );
    }

    #[test]
    fn indexeddb_prefix_ranges_and_scan_caps_are_deterministic() {
        let range = prefix_range("09af").unwrap();
        assert_eq!(range.lower, "09af");
        assert_eq!(range.upper_exclusive, "09afg");
        for matching in ["09af", "09af0", "09afffff"] {
            assert!(matching >= range.lower.as_str());
            assert!(matching < range.upper_exclusive.as_str());
        }
        assert!(prefix_range("").is_err());
        assert!(prefix_range("09AF").is_err());

        let limits = DecodeLimits {
            max_collection_items: 17,
            ..DecodeLimits::default()
        };
        assert_eq!(bounded_scan_limit(limits).unwrap(), 18);
        assert!(
            bounded_scan_limit(DecodeLimits {
                max_collection_items: u32::MAX as usize,
                ..DecodeLimits::default()
            })
            .is_err()
        );
    }

    #[test]
    fn sparse_transaction_plans_include_only_change_dependencies() {
        let app = application();
        let memory = MemoryId([0x31; 32]);
        let field = MemoryLeafId::from_memory_path(memory, "value").unwrap();
        let checkpoint = CheckpointBatch {
            application: app.clone(),
            schema_hash: [1; 32],
            base_epoch: 4,
            next_epoch: 5,
            first_turn_sequence: 8,
            last_turn_sequence: 8,
            changes: vec![DurableChange::SetRowField {
                memory_id: memory,
                row_key: 3,
                row_generation: 1,
                field_id: field,
                value: number(9),
            }],
            outbox_changes: vec![DurableOutboxChange::BeginDispatch {
                item_id: OutboxItemId([0x41; 32]),
                expected_revision: 0,
                next_revision: 1,
                attempt: 1,
                turn_sequence: 8,
            }],
            protocol_state_changes: vec![DurableProtocolStateChange::Put {
                key: ProtocolStateKey([0x45; 32]),
                expected_revision: None,
                next_revision: 1,
                payload: vec![0x46].into(),
                turn_sequence: 8,
            }],
            content_artifact_changes: vec![DurableContentArtifactChange::SetReplaceable {
                owner_id: ContentArtifactOwnerId([0x42; 32]),
                artifact_id: ContentArtifactId([0x43; 32]),
            }],
            checksum: [0; 32],
        };
        assert_eq!(
            SparseTransactionPlan::checkpoint(&checkpoint).stores,
            BTreeSet::from([
                META,
                SLOTS,
                LISTS,
                ROWS,
                CHECKPOINTS,
                OUTBOX,
                PROTOCOL_STATE,
                BLOBS,
                ARTIFACTS,
                ARTIFACT_OWNERS,
            ])
        );

        let activation = ActivationBatch {
            application: app,
            expected_base_epoch: 5,
            next_epoch: 6,
            source_schema_hash: [1; 32],
            target_schema_version: 2,
            target_schema_hash: [2; 32],
            through_turn_sequence: 8,
            authority_changes: Vec::new(),
            completed_migration_edges: vec![MigrationEdgeId([0x51; 32])],
            deleted_memory: vec![memory],
            target_content_artifact_manifest: ContentArtifactManifest::default(),
            content_artifacts: BTreeMap::new(),
            checksum: [0; 32],
        };
        assert_eq!(
            SparseTransactionPlan::activation(&activation).stores,
            BTreeSet::from([
                META,
                SLOTS,
                LISTS,
                ROWS,
                CHECKPOINTS,
                MIGRATIONS,
                BLOBS,
                ARTIFACTS,
                ARTIFACT_OWNERS,
            ])
        );
    }

    #[test]
    fn browser_component_decoders_enforce_limits_and_canonical_keys() {
        let scalar = StoredScalar {
            touched: true,
            value: StoredValue::Text("bounded".to_owned()),
        };
        let mut bytes = encode_scalar_component(&scalar).unwrap().bytes;
        bytes.push(0);
        assert!(
            decode_scalar_component(&bytes, DecodeLimits::default(), &BTreeMap::new()).is_err()
        );
        assert!(
            decode_scalar_component(
                &bytes,
                DecodeLimits {
                    max_total_bytes: 1,
                    ..DecodeLimits::default()
                },
                &BTreeMap::new(),
            )
            .is_err()
        );
        assert!(decode_hex_digest(&"A".repeat(64)).is_err());
        assert!(decode_hex_u64("000000000000000A").is_err());
    }

    #[test]
    fn indexeddb_failure_categories_are_stable() {
        for kind in [
            BrowserFailureKind::QuotaExceeded,
            BrowserFailureKind::MissingOrEvicted,
            BrowserFailureKind::PrivateModeOrUnavailable,
            BrowserFailureKind::UpgradeBlocked,
            BrowserFailureKind::Timeout,
            BrowserFailureKind::TransactionAborted,
            BrowserFailureKind::VersionChangeClosed,
            BrowserFailureKind::VersionMismatch,
            BrowserFailureKind::Backend,
        ] {
            let error = indexed_db_failure(kind, "fixture");
            assert_eq!(browser_failure_kind(&error), Some(kind));
        }
        assert_eq!(browser_failure_kind(&StoreError::Closed), None);
        assert_eq!(
            browser_failure_kind(&StoreError::MissingApplication),
            Some(BrowserFailureKind::MissingOrEvicted)
        );
        assert_eq!(
            classify_indexed_db_detail("QuotaExceededError"),
            BrowserFailureKind::QuotaExceeded
        );
        assert_eq!(
            classify_indexed_db_detail("AbortError: transaction abort"),
            BrowserFailureKind::TransactionAborted
        );
        assert_eq!(
            classify_indexed_db_detail("operation timed out"),
            BrowserFailureKind::Timeout
        );
        assert_eq!(
            classify_indexed_db_detail("upgrade blocked by another connection"),
            BrowserFailureKind::UpgradeBlocked
        );

        let mut status = BrowserStorageStatus {
            persistence: BrowserPersistenceGrant::Denied,
            usage_bytes: Some(400),
            quota_bytes: Some(1_000),
            estimate_error: None,
            quota_failure: None,
            missing_or_evicted: false,
            last_operation_failure: None,
            last_status_detail: None,
        };
        assert!(status.eviction_risk());
        assert_eq!(status.available_bytes(), Some(600));
        status.record_result(&PersistenceResult::Loaded(Ok(None)));
        assert!(status.missing_or_evicted);
        assert_eq!(
            status.last_operation_failure,
            Some(BrowserFailureKind::MissingOrEvicted)
        );
        status.record_result(&PersistenceResult::Loaded(Ok(Some(RestoreImage::empty(
            application(),
            1,
            [1; 32],
        )))));
        assert!(!status.missing_or_evicted);
        assert_eq!(status.last_operation_failure, None);
    }

    #[test]
    fn coordinator_queue_and_status_details_are_bounded() {
        assert!(validate_command_queue_capacity(1).is_ok());
        assert!(validate_command_queue_capacity(DEFAULT_COMMAND_QUEUE_CAPACITY).is_ok());
        assert!(validate_command_queue_capacity(MAX_COMMAND_QUEUE_CAPACITY).is_ok());
        for invalid in [0, MAX_COMMAND_QUEUE_CAPACITY + 1, usize::MAX] {
            assert_eq!(
                browser_failure_kind(&validate_command_queue_capacity(invalid).unwrap_err()),
                Some(BrowserFailureKind::Backend)
            );
        }
        let bounded = bounded_status_detail("x".repeat(MAX_STATUS_DETAIL_BYTES * 2));
        assert_eq!(bounded.len(), MAX_STATUS_DETAIL_BYTES);
        assert!(bounded.ends_with("..."));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rexie_initial_image_uses_canonical_outbox_and_shared_blob_records() {
        let app = application();
        let scalar_memory = memory("payload", MemoryKind::Scalar);
        let list_memory = memory("rows", MemoryKind::List);
        let field = MemoryLeafId::from_memory_path(list_memory, "payload").unwrap();
        let payload = vec![0x6a; crate::INLINE_BYTES_THRESHOLD + 1];
        let mut candidate = RestoreImage::empty(app.clone(), 1, [1; 32]);
        candidate.scalars.insert(
            scalar_memory,
            StoredScalar {
                touched: true,
                value: StoredValue::Bytes(payload.clone().into()),
            },
        );
        candidate.lists.insert(
            list_memory,
            StoredList {
                touched: true,
                next_key: 1,
                rows: vec![StoredRow {
                    key: 0,
                    generation: 1,
                    fields: BTreeMap::from([(field, StoredValue::Bytes(payload.into()))]),
                    touched_fields: BTreeSet::from([field]),
                }],
            },
        );
        let effect = EffectId::from_host_operation("Test/send").unwrap();
        let invocation = EffectInvocationId::from_result_owner(effect, "test/target").unwrap();
        let item = DurableOutboxItem::pending(
            invocation,
            effect,
            StoredValue::Text("key".to_owned()),
            number(1),
            Some(DurableEffectRow {
                list_memory_id: list_memory,
                row_key: 0,
                row_generation: 1,
            }),
            1,
        );
        assert_eq!(
            item.item_id,
            OutboxItemId::from_invocation(
                invocation,
                effect,
                &item.idempotency_key,
                item.target_row,
            )
        );
        candidate.outbox.insert(item.item_id, item.clone());

        let mutations = stage_initial_image(&candidate).unwrap();
        let blob_records = mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::Put {
                    store: BLOBS,
                    value,
                    ..
                } => Some(decode_blob_record(value, DecodeLimits::default()).unwrap()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(blob_records.len(), 1);
        assert_eq!(blob_records[0].reference_count, 2);
        assert!(mutations.iter().any(|mutation| matches!(
            mutation,
            StoreMutation::Put { store: OUTBOX, value, .. }
                if decode_outbox_record(value, DecodeLimits::default()).unwrap() == item
        )));
    }
}
