use boon_plan::{
    ApplicationIdentity, DataTypeFieldPlan, DataTypePlan, EffectId, EffectInvocationId,
    EffectOutboxSchema, MemoryId, MemoryLeafId, MigrationEdgeId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod codec;
mod migration;

#[cfg(any(target_arch = "wasm32", test))]
mod web;

#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(not(target_arch = "wasm32"))]
mod native;

pub use codec::{
    CodecError, DecodeLimits, INLINE_BYTES_THRESHOLD, decode_checkpoint_batch,
    decode_restore_image, encode_checkpoint_batch, encode_restore_image,
};
pub use migration::*;
#[cfg(not(target_arch = "wasm32"))]
pub use native::RedbDriver;
#[cfg(target_arch = "wasm32")]
pub use web::{
    BrowserFailureKind, BrowserPersistenceGrant, BrowserStorageStatus, RexieDriver,
    browser_failure_kind,
};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::*;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StoredValue {
    Null,
    Bool(bool),
    Number(i64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<StoredValue>),
    Record(BTreeMap<String, StoredValue>),
    Variant {
        tag: String,
        fields: BTreeMap<String, StoredValue>,
    },
    Error {
        code: String,
        fields: BTreeMap<String, StoredValue>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredScalar {
    pub touched: bool,
    pub value: StoredValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredRow {
    pub key: u64,
    pub generation: u64,
    pub fields: BTreeMap<MemoryLeafId, StoredValue>,
    pub touched_fields: BTreeSet<MemoryLeafId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredList {
    pub touched: bool,
    pub next_key: u64,
    pub rows: Vec<StoredRow>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct OutboxItemId(pub [u8; 32]);

impl OutboxItemId {
    pub fn from_invocation(
        invocation_id: EffectInvocationId,
        effect_id: EffectId,
        idempotency_key: &StoredValue,
        target_row: Option<DurableEffectRow>,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.outbox.item.v3");
        hasher.update(invocation_id.as_bytes());
        hasher.update(effect_id.as_bytes());
        hash_stored_value(&mut hasher, idempotency_key);
        match target_row {
            Some(row) => {
                hasher.update([1]);
                hasher.update(row.list_memory_id.as_bytes());
                hasher.update(row.row_key.to_be_bytes());
                hasher.update(row.row_generation.to_be_bytes());
            }
            None => hasher.update([0]),
        }
        Self(hasher.finalize().into())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn canonical_intent_key(intent: &StoredValue) -> StoredValue {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.effect-intent-key.v1");
    hash_stored_value(&mut hasher, intent);
    StoredValue::Bytes(hasher.finalize().to_vec())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DurableEffectRow {
    pub list_memory_id: MemoryId,
    pub row_key: u64,
    pub row_generation: u64,
}

impl fmt::Display for OutboxItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurableOutboxState {
    Pending,
    Dispatching { attempt: u32 },
    ReconciliationRequired { attempt: u32 },
    Completed { attempt: u32, outcome: StoredValue },
}

impl DurableOutboxState {
    pub const fn attempt(&self) -> u32 {
        match self {
            Self::Pending => 0,
            Self::Dispatching { attempt }
            | Self::ReconciliationRequired { attempt }
            | Self::Completed { attempt, .. } => *attempt,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DurableOutboxItem {
    pub item_id: OutboxItemId,
    pub invocation_id: EffectInvocationId,
    pub effect_id: EffectId,
    pub target_row: Option<DurableEffectRow>,
    pub idempotency_key: StoredValue,
    pub intent: StoredValue,
    pub state: DurableOutboxState,
    pub revision: u64,
    pub created_turn_sequence: u64,
    pub updated_turn_sequence: u64,
}

impl DurableOutboxItem {
    pub fn pending(
        invocation_id: EffectInvocationId,
        effect_id: EffectId,
        idempotency_key: StoredValue,
        intent: StoredValue,
        target_row: Option<DurableEffectRow>,
        turn_sequence: u64,
    ) -> Self {
        Self {
            item_id: OutboxItemId::from_invocation(
                invocation_id,
                effect_id,
                &idempotency_key,
                target_row,
            ),
            invocation_id,
            effect_id,
            target_row,
            idempotency_key,
            intent,
            state: DurableOutboxState::Pending,
            revision: 0,
            created_turn_sequence: turn_sequence,
            updated_turn_sequence: turn_sequence,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum DurableOutboxChange {
    Enqueue {
        item: DurableOutboxItem,
    },
    BeginDispatch {
        item_id: OutboxItemId,
        expected_revision: u64,
        next_revision: u64,
        attempt: u32,
        turn_sequence: u64,
    },
    RequireReconciliation {
        item_id: OutboxItemId,
        expected_revision: u64,
        next_revision: u64,
        attempt: u32,
        turn_sequence: u64,
    },
    Complete {
        item_id: OutboxItemId,
        expected_revision: u64,
        next_revision: u64,
        attempt: u32,
        outcome: StoredValue,
        turn_sequence: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestoreImage {
    pub application: ApplicationIdentity,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub epoch: u64,
    pub through_turn_sequence: u64,
    pub scalars: BTreeMap<MemoryId, StoredScalar>,
    pub lists: BTreeMap<MemoryId, StoredList>,
    pub completed_migration_edges: BTreeSet<MigrationEdgeId>,
    pub outbox: BTreeMap<OutboxItemId, DurableOutboxItem>,
}

impl RestoreImage {
    pub fn empty(
        application: ApplicationIdentity,
        schema_version: u64,
        schema_hash: [u8; 32],
    ) -> Self {
        Self {
            application,
            schema_version,
            schema_hash,
            epoch: 0,
            through_turn_sequence: 0,
            scalars: BTreeMap::new(),
            lists: BTreeMap::new(),
            completed_migration_edges: BTreeSet::new(),
            outbox: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurableChange {
    SetScalar {
        memory_id: MemoryId,
        value: StoredScalar,
    },
    DeleteScalar {
        memory_id: MemoryId,
    },
    SetList {
        memory_id: MemoryId,
        value: StoredList,
    },
    SetRowField {
        memory_id: MemoryId,
        row_key: u64,
        row_generation: u64,
        field_id: MemoryLeafId,
        value: StoredValue,
    },
    InsertRow {
        memory_id: MemoryId,
        index: u64,
        row: StoredRow,
        next_key: u64,
    },
    RemoveRow {
        memory_id: MemoryId,
        row_key: u64,
        row_generation: u64,
        next_key: u64,
    },
    DeleteList {
        memory_id: MemoryId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckpointBatch {
    pub application: ApplicationIdentity,
    pub schema_hash: [u8; 32],
    pub base_epoch: u64,
    pub next_epoch: u64,
    pub first_turn_sequence: u64,
    pub last_turn_sequence: u64,
    pub changes: Vec<DurableChange>,
    pub outbox_changes: Vec<DurableOutboxChange>,
    pub checksum: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResetApplicationBatch {
    pub application: ApplicationIdentity,
    pub expected_base_epoch: u64,
    pub next_epoch: u64,
    pub source_schema_hash: [u8; 32],
    pub default_image: RestoreImage,
    pub checksum: [u8; 32],
}

impl ResetApplicationBatch {
    pub fn seal(mut self) -> Self {
        self.checksum = reset_checksum(&self);
        self
    }
}

impl CheckpointBatch {
    pub fn seal(mut self) -> Self {
        self.checksum = checkpoint_checksum(&self);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivationBatch {
    pub application: ApplicationIdentity,
    pub expected_base_epoch: u64,
    pub next_epoch: u64,
    pub source_schema_hash: [u8; 32],
    pub target_schema_version: u64,
    pub target_schema_hash: [u8; 32],
    pub through_turn_sequence: u64,
    pub authority_changes: Vec<DurableChange>,
    pub completed_migration_edges: Vec<MigrationEdgeId>,
    pub deleted_memory: Vec<MemoryId>,
    pub checksum: [u8; 32],
}

impl ActivationBatch {
    pub fn seal(mut self) -> Self {
        self.checksum = activation_checksum(&self);
        self
    }

    pub fn between(current: &RestoreImage, candidate: &RestoreImage) -> Result<Self, StoreError> {
        if current.application != candidate.application {
            return Err(StoreError::IdentityMismatch);
        }
        if candidate.through_turn_sequence < current.through_turn_sequence {
            return Err(StoreError::NonContiguousTurn);
        }
        for list in candidate.lists.values() {
            validate_list(list)?;
        }
        if candidate
            .scalars
            .keys()
            .any(|memory| candidate.lists.contains_key(memory))
        {
            return Err(StoreError::InvalidAuthority(
                "candidate uses one memory ID as both scalar and list".to_owned(),
            ));
        }
        if current.outbox != candidate.outbox {
            return Err(StoreError::InvalidOutboxTransition(
                "schema activation cannot mutate the effect outbox".to_owned(),
            ));
        }

        let mut authority_changes = Vec::new();
        for (memory_id, value) in &candidate.scalars {
            if current.scalars.get(memory_id) != Some(value) {
                authority_changes.push(DurableChange::SetScalar {
                    memory_id: *memory_id,
                    value: value.clone(),
                });
            }
        }
        for (memory_id, value) in &candidate.lists {
            if current.lists.get(memory_id) != Some(value) {
                authority_changes.push(DurableChange::SetList {
                    memory_id: *memory_id,
                    value: value.clone(),
                });
            }
        }
        let deleted_memory = current
            .scalars
            .keys()
            .chain(current.lists.keys())
            .filter(|memory| {
                !candidate.scalars.contains_key(memory) && !candidate.lists.contains_key(memory)
            })
            .copied()
            .collect::<Vec<_>>();
        let completed_migration_edges = candidate
            .completed_migration_edges
            .difference(&current.completed_migration_edges)
            .copied()
            .collect::<Vec<_>>();

        Ok(Self {
            application: current.application.clone(),
            expected_base_epoch: current.epoch,
            next_epoch: current.epoch.saturating_add(1),
            source_schema_hash: current.schema_hash,
            target_schema_version: candidate.schema_version,
            target_schema_hash: candidate.schema_hash,
            through_turn_sequence: candidate.through_turn_sequence,
            authority_changes,
            completed_migration_edges,
            deleted_memory,
            checksum: [0; 32],
        }
        .seal())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub application: ApplicationIdentity,
    pub expected_schema_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BarrierRequest {
    pub application: ApplicationIdentity,
    pub through_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InspectRequest {
    pub application: ApplicationIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompactRequest {
    pub application: ApplicationIdentity,
}

pub const MAX_CONTENT_ARTIFACT_MEDIA_TYPE_BYTES: usize = 128;
pub const MAX_CONTENT_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ContentArtifactId(pub [u8; 32]);

impl ContentArtifactId {
    pub fn for_content(media_type: &str, bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"boon.content-artifact.v1");
        hasher.update((media_type.len() as u64).to_be_bytes());
        hasher.update(media_type.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_hex(value: &str) -> Result<Self, String> {
        if value.len() != 64 {
            return Err(format!(
                "content artifact ID has {} hexadecimal digits, expected 64",
                value.len()
            ));
        }
        let mut digest = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_lower_hex_digit(pair[0])?;
            let low = decode_lower_hex_digit(pair[1])?;
            digest[index] = (high << 4) | low;
        }
        Ok(Self(digest))
    }
}

fn decode_lower_hex_digit(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err("content artifact ID must use lowercase hexadecimal digits".to_owned()),
    }
}

impl fmt::Display for ContentArtifactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentArtifact {
    pub id: ContentArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl ContentArtifact {
    pub fn new(media_type: impl Into<String>, bytes: Vec<u8>) -> Result<Self, StoreError> {
        let media_type = media_type.into();
        let artifact = Self {
            id: ContentArtifactId::for_content(&media_type, &bytes),
            media_type,
            bytes,
        };
        validate_content_artifact(&artifact)?;
        Ok(artifact)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PutContentArtifactRequest {
    pub application: ApplicationIdentity,
    pub artifact: ContentArtifact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadContentArtifactRequest {
    pub application: ApplicationIdentity,
    pub id: ContentArtifactId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShutdownRequest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PersistenceCommand {
    Load(RestoreRequest),
    Initialize(RestoreImage),
    Commit(CheckpointBatch),
    Activate(ActivationBatch),
    ResetApplication(ResetApplicationBatch),
    Barrier(BarrierRequest),
    Inspect(InspectRequest),
    Compact(CompactRequest),
    PutContentArtifact(PutContentArtifactRequest),
    LoadContentArtifact(LoadContentArtifactRequest),
    Shutdown(ShutdownRequest),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommitAck {
    pub epoch: u64,
    pub through_turn_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivationAck {
    pub epoch: u64,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub through_turn_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResetApplicationAck {
    pub epoch: u64,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub through_turn_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BarrierAck {
    pub epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistenceInspectorSnapshot {
    pub application: ApplicationIdentity,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub epoch: u64,
    pub through_turn_sequence: u64,
    pub scalar_count: usize,
    pub list_count: usize,
    pub row_count: usize,
    pub content_artifact_count: usize,
    pub content_artifact_bytes: u64,
    /// Encoded backend bytes when the driver can report them without loading
    /// or serializing the complete application. `None` is an honest unknown.
    pub encoded_value_bytes: Option<u64>,
    pub completed_migration_count: usize,
    pub outbox_pending_count: usize,
    pub outbox_dispatching_count: usize,
    pub outbox_reconciliation_count: usize,
    pub outbox_completed_count: usize,
    pub outbox_samples: Vec<OutboxInspectorSample>,
}

pub const MAX_OUTBOX_INSPECTOR_SAMPLES: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboxInspectorSample {
    pub item_id: OutboxItemId,
    pub invocation_id: EffectInvocationId,
    pub effect_id: EffectId,
    pub state: OutboxInspectorState,
    pub attempt: u32,
    pub created_turn_sequence: u64,
    pub updated_turn_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxInspectorState {
    Pending,
    Dispatching,
    ReconciliationRequired,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompactAck {
    pub epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PutContentArtifactAck {
    pub id: ContentArtifactId,
    pub stored_bytes: u64,
    pub already_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShutdownAck;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PersistenceResult {
    Loaded(Result<Option<RestoreImage>, StoreError>),
    Initialized(Result<CommitAck, StoreError>),
    Committed(Result<CommitAck, StoreError>),
    Activated(Result<ActivationAck, StoreError>),
    ApplicationReset(Result<ResetApplicationAck, StoreError>),
    BarrierComplete(Result<BarrierAck, StoreError>),
    Inspected(Result<Option<PersistenceInspectorSnapshot>, StoreError>),
    Compacted(Result<CompactAck, StoreError>),
    ContentArtifactStored(Result<PutContentArtifactAck, StoreError>),
    ContentArtifactLoaded(Result<Option<ContentArtifact>, StoreError>),
    ShutdownComplete(Result<ShutdownAck, StoreError>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StoreError {
    Closed,
    MissingApplication,
    IdentityMismatch,
    SchemaMismatch,
    StaleEpoch,
    NonContiguousTurn,
    InvalidChecksum,
    InvalidAuthority(String),
    InvalidOutboxTransition(String),
    InvalidContentArtifact(String),
    Backend(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("persistence driver is closed"),
            Self::MissingApplication => formatter.write_str("application state does not exist"),
            Self::IdentityMismatch => formatter.write_str("application identity does not match"),
            Self::SchemaMismatch => formatter.write_str("persistence schema does not match"),
            Self::StaleEpoch => formatter.write_str("persistence epoch is stale"),
            Self::NonContiguousTurn => formatter.write_str("turn range is not contiguous"),
            Self::InvalidChecksum => formatter.write_str("batch checksum is invalid"),
            Self::InvalidAuthority(detail) => write!(formatter, "invalid authority: {detail}"),
            Self::InvalidOutboxTransition(detail) => {
                write!(formatter, "invalid outbox transition: {detail}")
            }
            Self::InvalidContentArtifact(detail) => {
                write!(formatter, "invalid content artifact: {detail}")
            }
            Self::Backend(detail) => write!(formatter, "persistence backend failed: {detail}"),
        }
    }
}

impl std::error::Error for StoreError {}

pub trait PersistenceDriver {
    fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult;
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryDriver {
    applications: BTreeMap<ApplicationIdentity, RestoreImage>,
    artifacts: BTreeMap<(ApplicationIdentity, ContentArtifactId), ContentArtifact>,
    closed: bool,
    fail_next_activation: bool,
}

impl InMemoryDriver {
    pub fn seed(&mut self, image: RestoreImage) {
        self.applications.insert(image.application.clone(), image);
    }

    pub fn fail_next_activation(&mut self) {
        self.fail_next_activation = true;
    }

    pub fn image(&self, application: &ApplicationIdentity) -> Option<&RestoreImage> {
        self.applications.get(application)
    }

    fn load(&self, request: RestoreRequest) -> Result<Option<RestoreImage>, StoreError> {
        let image = self.applications.get(&request.application).cloned();
        if let (Some(expected), Some(image)) = (request.expected_schema_hash, image.as_ref())
            && expected != image.schema_hash
        {
            return Err(StoreError::SchemaMismatch);
        }
        Ok(image)
    }

    fn initialize(&mut self, image: RestoreImage) -> Result<CommitAck, StoreError> {
        validate_initial_image(&image)?;
        if let Some(existing) = self.applications.get(&image.application) {
            if existing == &image {
                return Ok(CommitAck {
                    epoch: existing.epoch,
                    through_turn_sequence: existing.through_turn_sequence,
                });
            }
            return Err(StoreError::IdentityMismatch);
        }
        let ack = CommitAck {
            epoch: image.epoch,
            through_turn_sequence: image.through_turn_sequence,
        };
        self.applications.insert(image.application.clone(), image);
        Ok(ack)
    }

    fn commit(&mut self, batch: CheckpointBatch) -> Result<CommitAck, StoreError> {
        let image = self
            .applications
            .get_mut(&batch.application)
            .ok_or(StoreError::MissingApplication)?;
        apply_checkpoint_to_image(image, &batch)
    }

    fn activate(&mut self, batch: ActivationBatch) -> Result<ActivationAck, StoreError> {
        if self.fail_next_activation {
            self.fail_next_activation = false;
            return Err(StoreError::Backend(
                "injected activation failure".to_owned(),
            ));
        }
        let current = self
            .applications
            .get(&batch.application)
            .cloned()
            .ok_or(StoreError::MissingApplication)?;
        let (candidate, ack) = apply_activation_to_image(current, &batch)?;
        self.applications.insert(batch.application, candidate);
        Ok(ack)
    }

    fn reset_application(
        &mut self,
        batch: ResetApplicationBatch,
    ) -> Result<ResetApplicationAck, StoreError> {
        let current = self
            .applications
            .get(&batch.application)
            .cloned()
            .ok_or(StoreError::MissingApplication)?;
        let (reset, ack) = apply_reset_to_image(current, &batch)?;
        self.artifacts
            .retain(|(application, _), _| application != &batch.application);
        self.applications.insert(batch.application, reset);
        Ok(ack)
    }

    fn put_content_artifact(
        &mut self,
        request: PutContentArtifactRequest,
    ) -> Result<PutContentArtifactAck, StoreError> {
        validate_content_artifact(&request.artifact)?;
        if !self.applications.contains_key(&request.application) {
            return Err(StoreError::MissingApplication);
        }
        let key = (request.application, request.artifact.id);
        let already_present = match self.artifacts.get(&key) {
            Some(existing) if existing == &request.artifact => true,
            Some(_) => {
                return Err(StoreError::InvalidContentArtifact(
                    "content digest collides with different artifact bytes".to_owned(),
                ));
            }
            None => {
                self.artifacts.insert(key, request.artifact.clone());
                false
            }
        };
        Ok(PutContentArtifactAck {
            id: request.artifact.id,
            stored_bytes: request.artifact.bytes.len().try_into().unwrap_or(u64::MAX),
            already_present,
        })
    }

    fn load_content_artifact(
        &self,
        request: LoadContentArtifactRequest,
    ) -> Result<Option<ContentArtifact>, StoreError> {
        if !self.applications.contains_key(&request.application) {
            return Err(StoreError::MissingApplication);
        }
        let artifact = self
            .artifacts
            .get(&(request.application, request.id))
            .cloned();
        if let Some(artifact) = &artifact {
            validate_content_artifact(artifact)?;
        }
        Ok(artifact)
    }
}

impl PersistenceDriver for InMemoryDriver {
    fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
        if self.closed && !matches!(command, PersistenceCommand::Shutdown(_)) {
            return error_result(command, StoreError::Closed);
        }
        match command {
            PersistenceCommand::Load(request) => PersistenceResult::Loaded(self.load(request)),
            PersistenceCommand::Initialize(image) => {
                PersistenceResult::Initialized(self.initialize(image))
            }
            PersistenceCommand::Commit(batch) => PersistenceResult::Committed(self.commit(batch)),
            PersistenceCommand::Activate(batch) => {
                PersistenceResult::Activated(self.activate(batch))
            }
            PersistenceCommand::ResetApplication(batch) => {
                PersistenceResult::ApplicationReset(self.reset_application(batch))
            }
            PersistenceCommand::Barrier(request) => {
                let result = self
                    .applications
                    .get(&request.application)
                    .ok_or(StoreError::MissingApplication)
                    .and_then(|image| {
                        (image.epoch >= request.through_epoch)
                            .then_some(BarrierAck { epoch: image.epoch })
                            .ok_or(StoreError::StaleEpoch)
                    });
                PersistenceResult::BarrierComplete(result)
            }
            PersistenceCommand::Inspect(request) => {
                let snapshot = self
                    .applications
                    .get(&request.application)
                    .map(inspector_snapshot)
                    .map(|mut snapshot| {
                        for ((application, _), artifact) in &self.artifacts {
                            if application == &request.application {
                                snapshot.content_artifact_count =
                                    snapshot.content_artifact_count.saturating_add(1);
                                snapshot.content_artifact_bytes =
                                    snapshot.content_artifact_bytes.saturating_add(
                                        artifact.bytes.len().try_into().unwrap_or(u64::MAX),
                                    );
                            }
                        }
                        snapshot
                    });
                PersistenceResult::Inspected(Ok(snapshot))
            }
            PersistenceCommand::Compact(request) => {
                let result = self
                    .applications
                    .get(&request.application)
                    .map(|image| CompactAck { epoch: image.epoch })
                    .ok_or(StoreError::MissingApplication);
                PersistenceResult::Compacted(result)
            }
            PersistenceCommand::PutContentArtifact(request) => {
                PersistenceResult::ContentArtifactStored(self.put_content_artifact(request))
            }
            PersistenceCommand::LoadContentArtifact(request) => {
                PersistenceResult::ContentArtifactLoaded(self.load_content_artifact(request))
            }
            PersistenceCommand::Shutdown(_) => {
                self.closed = true;
                PersistenceResult::ShutdownComplete(Ok(ShutdownAck))
            }
        }
    }
}

fn apply_changes(image: &mut RestoreImage, changes: &[DurableChange]) -> Result<(), StoreError> {
    for change in changes {
        match change {
            DurableChange::SetScalar { memory_id, value } => {
                if image.lists.contains_key(memory_id) {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a list"
                    )));
                }
                image.scalars.insert(*memory_id, value.clone());
            }
            DurableChange::DeleteScalar { memory_id } => {
                image.scalars.remove(memory_id);
            }
            DurableChange::SetList { memory_id, value } => {
                validate_list(value)?;
                if image.scalars.contains_key(memory_id) {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                image.lists.insert(*memory_id, value.clone());
            }
            DurableChange::SetRowField {
                memory_id,
                row_key,
                row_generation,
                field_id,
                value,
            } => {
                if image.scalars.contains_key(memory_id) {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                let list = image.lists.entry(*memory_id).or_insert_with(|| StoredList {
                    touched: false,
                    next_key: 0,
                    rows: Vec::new(),
                });
                let row = match list
                    .rows
                    .iter_mut()
                    .find(|row| row.key == *row_key && row.generation == *row_generation)
                {
                    Some(row) => row,
                    None if list.touched => {
                        return Err(StoreError::InvalidAuthority(format!(
                            "list {memory_id} has no row {row_key}:{row_generation}"
                        )));
                    }
                    None => {
                        list.rows.push(StoredRow {
                            key: *row_key,
                            generation: *row_generation,
                            fields: BTreeMap::new(),
                            touched_fields: BTreeSet::new(),
                        });
                        list.rows.last_mut().expect("row was appended")
                    }
                };
                row.fields.insert(*field_id, value.clone());
                row.touched_fields.insert(*field_id);
            }
            DurableChange::InsertRow {
                memory_id,
                index,
                row,
                next_key,
            } => {
                let list = image.lists.get_mut(memory_id).ok_or_else(|| {
                    StoreError::InvalidAuthority(format!(
                        "cannot insert into list {memory_id} before its structure is materialized"
                    ))
                })?;
                if !list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "cannot insert into sparse override list {memory_id}"
                    )));
                }
                if list.rows.iter().any(|candidate| {
                    candidate.key == row.key && candidate.generation == row.generation
                }) {
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
                list.rows.insert(index, row.clone());
                list.next_key = *next_key;
                validate_list(list)?;
            }
            DurableChange::RemoveRow {
                memory_id,
                row_key,
                row_generation,
                next_key,
            } => {
                let list = image.lists.get_mut(memory_id).ok_or_else(|| {
                    StoreError::InvalidAuthority(format!(
                        "cannot remove from missing list {memory_id}"
                    ))
                })?;
                if !list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "cannot remove from sparse override list {memory_id}"
                    )));
                }
                let index = list
                    .rows
                    .iter()
                    .position(|row| row.key == *row_key && row.generation == *row_generation)
                    .ok_or_else(|| {
                        StoreError::InvalidAuthority(format!(
                            "list {memory_id} has no row {row_key}:{row_generation}"
                        ))
                    })?;
                list.rows.remove(index);
                list.next_key = *next_key;
                validate_list(list)?;
            }
            DurableChange::DeleteList { memory_id } => {
                image.lists.remove(memory_id);
            }
        }
    }
    Ok(())
}

/// Applies validated durable effect transitions to an acknowledged outbox image.
///
/// Runtime hosts use the same transition function to keep a local scheduling
/// index synchronized after a persistence commit succeeds. The durable store
/// remains authoritative across restart; this helper prevents the hot scheduler
/// path from reloading that store merely to rediscover pending work.
pub fn apply_durable_outbox_changes(
    outbox: &mut BTreeMap<OutboxItemId, DurableOutboxItem>,
    changes: &[DurableOutboxChange],
) -> Result<(), StoreError> {
    for change in changes {
        match change {
            DurableOutboxChange::Enqueue { item } => {
                validate_outbox_item(item)?;
                if !matches!(item.state, DurableOutboxState::Pending) || item.revision != 0 {
                    return Err(StoreError::InvalidOutboxTransition(
                        "new outbox items must start pending at revision zero".to_owned(),
                    ));
                }
                match outbox.get(&item.item_id) {
                    Some(existing) if existing == item => {}
                    Some(_) => {
                        return Err(StoreError::InvalidOutboxTransition(format!(
                            "outbox item {} already exists with different durable content",
                            item.item_id
                        )));
                    }
                    None => {
                        outbox.insert(item.item_id, item.clone());
                    }
                }
            }
            DurableOutboxChange::BeginDispatch {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                turn_sequence,
            } => {
                let desired = DurableOutboxState::Dispatching { attempt: *attempt };
                transition_outbox_item(
                    outbox,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *turn_sequence,
                    desired,
                    |state| match state {
                        DurableOutboxState::Pending => *attempt == 1,
                        DurableOutboxState::ReconciliationRequired { attempt: previous } => {
                            *attempt == previous.saturating_add(1)
                        }
                        _ => false,
                    },
                )?;
            }
            DurableOutboxChange::RequireReconciliation {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                turn_sequence,
            } => {
                let desired = DurableOutboxState::ReconciliationRequired { attempt: *attempt };
                transition_outbox_item(
                    outbox,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *turn_sequence,
                    desired,
                    |state| matches!(state, DurableOutboxState::Dispatching { attempt: current } if current == attempt),
                )?;
            }
            DurableOutboxChange::Complete {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                outcome,
                turn_sequence,
            } => {
                let desired = DurableOutboxState::Completed {
                    attempt: *attempt,
                    outcome: outcome.clone(),
                };
                transition_outbox_item(
                    outbox,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *turn_sequence,
                    desired,
                    |state| match state {
                        DurableOutboxState::Dispatching { attempt: current }
                        | DurableOutboxState::ReconciliationRequired { attempt: current } => {
                            current == attempt
                        }
                        _ => false,
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn transition_outbox_item(
    outbox: &mut BTreeMap<OutboxItemId, DurableOutboxItem>,
    item_id: OutboxItemId,
    expected_revision: u64,
    next_revision: u64,
    turn_sequence: u64,
    desired_state: DurableOutboxState,
    allowed: impl FnOnce(&DurableOutboxState) -> bool,
) -> Result<(), StoreError> {
    if next_revision
        != expected_revision.checked_add(1).ok_or_else(|| {
            StoreError::InvalidOutboxTransition("outbox revision overflow".to_owned())
        })?
    {
        return Err(StoreError::InvalidOutboxTransition(
            "outbox revisions must advance by exactly one".to_owned(),
        ));
    }
    let item = outbox.get_mut(&item_id).ok_or_else(|| {
        StoreError::InvalidOutboxTransition(format!("outbox item {item_id} does not exist"))
    })?;

    if item.revision == next_revision
        && item.updated_turn_sequence == turn_sequence
        && item.state == desired_state
    {
        return Ok(());
    }
    if item.revision != expected_revision {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {item_id} is at revision {}, expected {expected_revision}",
            item.revision
        )));
    }
    if turn_sequence < item.updated_turn_sequence {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {item_id} cannot move backwards in turn order"
        )));
    }
    if !allowed(&item.state) {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {item_id} cannot transition from {:?} to {:?}",
            item.state, desired_state
        )));
    }
    item.state = desired_state;
    item.revision = next_revision;
    item.updated_turn_sequence = turn_sequence;
    Ok(())
}

fn validate_outbox(outbox: &BTreeMap<OutboxItemId, DurableOutboxItem>) -> Result<(), StoreError> {
    for (item_id, item) in outbox {
        if item_id != &item.item_id {
            return Err(StoreError::InvalidOutboxTransition(
                "outbox map key does not match item ID".to_owned(),
            ));
        }
        validate_outbox_item(item)?;
    }
    Ok(())
}

fn validate_outbox_item(item: &DurableOutboxItem) -> Result<(), StoreError> {
    if item.item_id
        != OutboxItemId::from_invocation(
            item.invocation_id,
            item.effect_id,
            &item.idempotency_key,
            item.target_row,
        )
    {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {} does not match its effect and idempotency key",
            item.item_id
        )));
    }
    if item.updated_turn_sequence < item.created_turn_sequence {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {} has decreasing turn provenance",
            item.item_id
        )));
    }
    match &item.state {
        DurableOutboxState::Pending if item.revision == 0 => Ok(()),
        DurableOutboxState::Pending => Err(StoreError::InvalidOutboxTransition(format!(
            "pending outbox item {} must be at revision zero",
            item.item_id
        ))),
        state if state.attempt() == 0 || item.revision == 0 => {
            Err(StoreError::InvalidOutboxTransition(format!(
                "active outbox item {} must have a positive attempt and revision",
                item.item_id
            )))
        }
        _ => Ok(()),
    }
}

pub fn validate_outbox_item_schema(
    item: &DurableOutboxItem,
    schema: &EffectOutboxSchema,
) -> Result<(), StoreError> {
    if item.effect_id != schema.effect_id {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {} does not match effect schema {}",
            item.item_id, schema.effect_id
        )));
    }
    if !schema.invocation_ids.contains(&item.invocation_id) {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "outbox item {} invocation {} is absent from effect schema {}",
            item.item_id, item.invocation_id, schema.effect_id
        )));
    }
    validate_stored_value_type(&item.intent, &schema.intent_type, "intent")?;
    validate_stored_value_type(
        &item.idempotency_key,
        &schema.idempotency_key_type,
        "idempotency key",
    )?;
    if let DurableOutboxState::Completed { outcome, .. } = &item.state {
        validate_stored_value_type(outcome, &schema.result_type, "outcome")?;
    }
    Ok(())
}

fn validate_stored_value_type(
    value: &StoredValue,
    data_type: &DataTypePlan,
    path: &str,
) -> Result<(), StoreError> {
    let valid = match (value, data_type) {
        (StoredValue::Null, DataTypePlan::Null)
        | (StoredValue::Bool(_), DataTypePlan::Bool)
        | (StoredValue::Number(_), DataTypePlan::Number)
        | (StoredValue::Text(_), DataTypePlan::Text) => true,
        (StoredValue::Number(value), DataTypePlan::Byte) => (0..=u8::MAX as i64).contains(value),
        (StoredValue::Bytes(value), DataTypePlan::Bytes { fixed_len }) => {
            fixed_len.is_none_or(|expected| u64::try_from(value.len()).ok() == Some(expected))
        }
        (StoredValue::List(values), DataTypePlan::List { item }) => {
            for (index, value) in values.iter().enumerate() {
                validate_stored_value_type(value, item, &format!("{path}[{index}]"))?;
            }
            true
        }
        (StoredValue::Record(values), DataTypePlan::Record { fields, open }) => {
            validate_stored_fields(values, fields, *open, path)?;
            true
        }
        (StoredValue::Variant { tag, fields }, DataTypePlan::Variant { variants }) => {
            let variant = variants
                .iter()
                .find(|variant| &variant.tag == tag)
                .ok_or_else(|| {
                    StoreError::InvalidOutboxTransition(format!(
                        "{path} has unknown variant tag `{tag}`"
                    ))
                })?;
            validate_stored_fields(fields, &variant.fields, variant.open, path)?;
            true
        }
        (
            StoredValue::Error { fields, .. },
            DataTypePlan::Error {
                fields: expected,
                open,
            },
        ) => {
            validate_stored_fields(fields, expected, *open, path)?;
            true
        }
        (_, DataTypePlan::Unknown) => false,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidOutboxTransition(format!(
            "{path} does not match its closed effect schema"
        )))
    }
}

fn validate_stored_fields(
    values: &BTreeMap<String, StoredValue>,
    expected: &[DataTypeFieldPlan],
    open: bool,
    path: &str,
) -> Result<(), StoreError> {
    for field in expected {
        let value = values.get(&field.name).ok_or_else(|| {
            StoreError::InvalidOutboxTransition(format!(
                "{path} is missing required field `{}`",
                field.name
            ))
        })?;
        validate_stored_value_type(value, &field.data_type, &format!("{path}.{}", field.name))?;
    }
    if !open
        && values
            .keys()
            .any(|name| !expected.iter().any(|field| &field.name == name))
    {
        return Err(StoreError::InvalidOutboxTransition(format!(
            "{path} contains a field outside its closed effect schema"
        )));
    }
    Ok(())
}

fn apply_checkpoint_to_image(
    image: &mut RestoreImage,
    batch: &CheckpointBatch,
) -> Result<CommitAck, StoreError> {
    validate_checkpoint(batch)?;
    if image.application != batch.application {
        return Err(StoreError::IdentityMismatch);
    }
    if image.schema_hash != batch.schema_hash {
        return Err(StoreError::SchemaMismatch);
    }
    if image.epoch != batch.base_epoch || batch.next_epoch != batch.base_epoch + 1 {
        return Err(StoreError::StaleEpoch);
    }
    if batch.first_turn_sequence != image.through_turn_sequence + 1
        || batch.last_turn_sequence < batch.first_turn_sequence
    {
        return Err(StoreError::NonContiguousTurn);
    }
    let mut candidate = image.clone();
    apply_changes(&mut candidate, &batch.changes)?;
    apply_durable_outbox_changes(&mut candidate.outbox, &batch.outbox_changes)?;
    validate_outbox(&candidate.outbox)?;
    candidate.epoch = batch.next_epoch;
    candidate.through_turn_sequence = batch.last_turn_sequence;
    *image = candidate;
    Ok(CommitAck {
        epoch: image.epoch,
        through_turn_sequence: image.through_turn_sequence,
    })
}

fn apply_reset_to_image(
    current: RestoreImage,
    batch: &ResetApplicationBatch,
) -> Result<(RestoreImage, ResetApplicationAck), StoreError> {
    validate_reset(batch)?;
    if current.application != batch.application
        || batch.default_image.application != batch.application
    {
        return Err(StoreError::IdentityMismatch);
    }
    if current.epoch != batch.expected_base_epoch
        || batch.next_epoch
            != batch
                .expected_base_epoch
                .checked_add(1)
                .ok_or(StoreError::StaleEpoch)?
    {
        return Err(StoreError::StaleEpoch);
    }
    if current.schema_hash != batch.source_schema_hash {
        return Err(StoreError::SchemaMismatch);
    }

    let mut reset = batch.default_image.clone();
    reset.epoch = batch.next_epoch;
    reset.through_turn_sequence = current.through_turn_sequence;
    let ack = ResetApplicationAck {
        epoch: reset.epoch,
        schema_version: reset.schema_version,
        schema_hash: reset.schema_hash,
        through_turn_sequence: reset.through_turn_sequence,
    };
    Ok((reset, ack))
}

fn apply_activation_to_image(
    current: RestoreImage,
    batch: &ActivationBatch,
) -> Result<(RestoreImage, ActivationAck), StoreError> {
    validate_activation(batch)?;
    if current.application != batch.application {
        return Err(StoreError::IdentityMismatch);
    }
    if current.epoch != batch.expected_base_epoch
        || batch.next_epoch != batch.expected_base_epoch + 1
    {
        return Err(StoreError::StaleEpoch);
    }
    if current.schema_hash != batch.source_schema_hash {
        return Err(StoreError::SchemaMismatch);
    }
    if batch.through_turn_sequence < current.through_turn_sequence {
        return Err(StoreError::NonContiguousTurn);
    }

    let mut candidate = current;
    apply_changes(&mut candidate, &batch.authority_changes)?;
    for memory in &batch.deleted_memory {
        candidate.scalars.remove(memory);
        candidate.lists.remove(memory);
    }
    candidate
        .completed_migration_edges
        .extend(batch.completed_migration_edges.iter().copied());
    candidate.schema_version = batch.target_schema_version;
    candidate.schema_hash = batch.target_schema_hash;
    candidate.epoch = batch.next_epoch;
    candidate.through_turn_sequence = batch.through_turn_sequence;

    let ack = ActivationAck {
        epoch: candidate.epoch,
        schema_version: candidate.schema_version,
        schema_hash: candidate.schema_hash,
        through_turn_sequence: candidate.through_turn_sequence,
    };
    Ok((candidate, ack))
}

fn validate_list(list: &StoredList) -> Result<(), StoreError> {
    let mut rows = BTreeSet::new();
    let mut minimum_next = 1u64;
    for row in &list.rows {
        if !rows.insert((row.key, row.generation)) {
            return Err(StoreError::InvalidAuthority(format!(
                "duplicate row {}:{}",
                row.key, row.generation
            )));
        }
        if !row
            .touched_fields
            .is_subset(&row.fields.keys().copied().collect())
        {
            return Err(StoreError::InvalidAuthority(format!(
                "row {}:{} touches a missing field",
                row.key, row.generation
            )));
        }
        if !list.touched
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
        minimum_next = minimum_next.max(row.key.saturating_add(1));
    }
    if !list.touched && list.next_key != 0 {
        return Err(StoreError::InvalidAuthority(
            "sparse row overrides must not replace list allocator state".to_owned(),
        ));
    }
    if list.touched && list.next_key < minimum_next {
        return Err(StoreError::InvalidAuthority(format!(
            "next key {} is below {}",
            list.next_key, minimum_next
        )));
    }
    Ok(())
}

fn validate_checkpoint(batch: &CheckpointBatch) -> Result<(), StoreError> {
    if batch.checksum != checkpoint_checksum(batch) {
        return Err(StoreError::InvalidChecksum);
    }
    for change in &batch.outbox_changes {
        let turn = match change {
            DurableOutboxChange::Enqueue { item } => item.created_turn_sequence,
            DurableOutboxChange::BeginDispatch { turn_sequence, .. }
            | DurableOutboxChange::RequireReconciliation { turn_sequence, .. }
            | DurableOutboxChange::Complete { turn_sequence, .. } => *turn_sequence,
        };
        if turn < batch.first_turn_sequence || turn > batch.last_turn_sequence {
            return Err(StoreError::InvalidOutboxTransition(format!(
                "outbox transition turn {turn} is outside checkpoint range {}..={}",
                batch.first_turn_sequence, batch.last_turn_sequence
            )));
        }
    }
    Ok(())
}

fn validate_initial_image(image: &RestoreImage) -> Result<(), StoreError> {
    if image.epoch != 0 || image.through_turn_sequence != 0 {
        return Err(StoreError::InvalidAuthority(
            "initial restore image must start at epoch and turn zero".to_owned(),
        ));
    }
    for list in image.lists.values() {
        validate_list(list)?;
    }
    validate_outbox(&image.outbox)?;
    Ok(())
}

fn validate_reset(batch: &ResetApplicationBatch) -> Result<(), StoreError> {
    if batch.checksum != reset_checksum(batch) {
        return Err(StoreError::InvalidChecksum);
    }
    validate_initial_image(&batch.default_image)?;
    if batch.application != batch.default_image.application {
        return Err(StoreError::IdentityMismatch);
    }
    if !batch.default_image.scalars.is_empty()
        || !batch.default_image.lists.is_empty()
        || !batch.default_image.completed_migration_edges.is_empty()
        || !batch.default_image.outbox.is_empty()
    {
        return Err(StoreError::InvalidAuthority(
            "start-over default image must contain no durable authority, migration history, or outbox work"
                .to_owned(),
        ));
    }
    if batch.default_image.schema_version == 0 {
        return Err(StoreError::InvalidAuthority(
            "start-over target schema version must be positive".to_owned(),
        ));
    }
    Ok(())
}

fn validate_activation(batch: &ActivationBatch) -> Result<(), StoreError> {
    if batch.checksum != activation_checksum(batch) {
        return Err(StoreError::InvalidChecksum);
    }
    if batch.target_schema_version == 0 {
        return Err(StoreError::InvalidAuthority(
            "target schema version must be positive".to_owned(),
        ));
    }
    Ok(())
}

pub fn validate_content_artifact(artifact: &ContentArtifact) -> Result<(), StoreError> {
    if artifact.media_type.is_empty()
        || artifact.media_type.len() > MAX_CONTENT_ARTIFACT_MEDIA_TYPE_BYTES
    {
        return Err(StoreError::InvalidContentArtifact(format!(
            "media type byte length {} is outside 1..={MAX_CONTENT_ARTIFACT_MEDIA_TYPE_BYTES}",
            artifact.media_type.len()
        )));
    }
    if artifact.bytes.len() > MAX_CONTENT_ARTIFACT_BYTES {
        return Err(StoreError::InvalidContentArtifact(format!(
            "artifact byte length {} exceeds {MAX_CONTENT_ARTIFACT_BYTES}",
            artifact.bytes.len()
        )));
    }
    let expected = ContentArtifactId::for_content(&artifact.media_type, &artifact.bytes);
    if artifact.id != expected {
        return Err(StoreError::InvalidContentArtifact(format!(
            "content digest {} does not match payload digest {expected}",
            artifact.id
        )));
    }
    Ok(())
}

fn checkpoint_checksum(batch: &CheckpointBatch) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_application(&mut hasher, &batch.application);
    hasher.update(batch.schema_hash);
    hasher.update(batch.base_epoch.to_be_bytes());
    hasher.update(batch.next_epoch.to_be_bytes());
    hasher.update(batch.first_turn_sequence.to_be_bytes());
    hasher.update(batch.last_turn_sequence.to_be_bytes());
    hash_changes(&mut hasher, &batch.changes);
    hash_outbox_changes(&mut hasher, &batch.outbox_changes);
    hasher.finalize().into()
}

fn reset_checksum(batch: &ResetApplicationBatch) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.persistence.reset.v1");
    hash_application(&mut hasher, &batch.application);
    hasher.update(batch.expected_base_epoch.to_be_bytes());
    hasher.update(batch.next_epoch.to_be_bytes());
    hasher.update(batch.source_schema_hash);
    hash_application(&mut hasher, &batch.default_image.application);
    hasher.update(batch.default_image.schema_version.to_be_bytes());
    hasher.update(batch.default_image.schema_hash);
    hasher.update(batch.default_image.epoch.to_be_bytes());
    hasher.update(batch.default_image.through_turn_sequence.to_be_bytes());
    hasher.finalize().into()
}

fn activation_checksum(batch: &ActivationBatch) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_application(&mut hasher, &batch.application);
    hasher.update(batch.expected_base_epoch.to_be_bytes());
    hasher.update(batch.next_epoch.to_be_bytes());
    hasher.update(batch.source_schema_hash);
    hasher.update(batch.target_schema_version.to_be_bytes());
    hasher.update(batch.target_schema_hash);
    hasher.update(batch.through_turn_sequence.to_be_bytes());
    hash_changes(&mut hasher, &batch.authority_changes);
    for edge in &batch.completed_migration_edges {
        hasher.update(edge.as_bytes());
    }
    for memory in &batch.deleted_memory {
        hasher.update(memory.as_bytes());
    }
    hasher.finalize().into()
}

fn hash_application(hasher: &mut Sha256, application: &ApplicationIdentity) {
    for part in [
        &application.package_id,
        &application.state_namespace,
        &application.deployment_domain,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
}

fn hash_changes(hasher: &mut Sha256, changes: &[DurableChange]) {
    for change in changes {
        match change {
            DurableChange::SetScalar { memory_id, value } => {
                hasher.update([0]);
                hasher.update(memory_id.as_bytes());
                hasher.update([u8::from(value.touched)]);
                hash_stored_value(hasher, &value.value);
            }
            DurableChange::DeleteScalar { memory_id } => {
                hasher.update([1]);
                hasher.update(memory_id.as_bytes());
            }
            DurableChange::SetList { memory_id, value } => {
                hasher.update([2]);
                hasher.update(memory_id.as_bytes());
                hasher.update([u8::from(value.touched)]);
                hasher.update(value.next_key.to_be_bytes());
                hasher.update((value.rows.len() as u64).to_be_bytes());
                for row in &value.rows {
                    hash_stored_row(hasher, row);
                }
            }
            DurableChange::SetRowField {
                memory_id,
                row_key,
                row_generation,
                field_id,
                value,
            } => {
                hasher.update([3]);
                hasher.update(memory_id.as_bytes());
                hasher.update(row_key.to_be_bytes());
                hasher.update(row_generation.to_be_bytes());
                hasher.update(field_id.as_bytes());
                hash_stored_value(hasher, value);
            }
            DurableChange::InsertRow {
                memory_id,
                index,
                row,
                next_key,
            } => {
                hasher.update([4]);
                hasher.update(memory_id.as_bytes());
                hasher.update(index.to_be_bytes());
                hash_stored_row(hasher, row);
                hasher.update(next_key.to_be_bytes());
            }
            DurableChange::RemoveRow {
                memory_id,
                row_key,
                row_generation,
                next_key,
            } => {
                hasher.update([5]);
                hasher.update(memory_id.as_bytes());
                hasher.update(row_key.to_be_bytes());
                hasher.update(row_generation.to_be_bytes());
                hasher.update(next_key.to_be_bytes());
            }
            DurableChange::DeleteList { memory_id } => {
                hasher.update([6]);
                hasher.update(memory_id.as_bytes());
            }
        }
    }
}

fn hash_outbox_changes(hasher: &mut Sha256, changes: &[DurableOutboxChange]) {
    hasher.update((changes.len() as u64).to_be_bytes());
    for change in changes {
        match change {
            DurableOutboxChange::Enqueue { item } => {
                hasher.update([0]);
                hash_outbox_item(hasher, item);
            }
            DurableOutboxChange::BeginDispatch {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                turn_sequence,
            } => {
                hasher.update([1]);
                hash_outbox_transition_header(
                    hasher,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *attempt,
                    *turn_sequence,
                );
            }
            DurableOutboxChange::RequireReconciliation {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                turn_sequence,
            } => {
                hasher.update([2]);
                hash_outbox_transition_header(
                    hasher,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *attempt,
                    *turn_sequence,
                );
            }
            DurableOutboxChange::Complete {
                item_id,
                expected_revision,
                next_revision,
                attempt,
                outcome,
                turn_sequence,
            } => {
                hasher.update([3]);
                hash_outbox_transition_header(
                    hasher,
                    *item_id,
                    *expected_revision,
                    *next_revision,
                    *attempt,
                    *turn_sequence,
                );
                hash_stored_value(hasher, outcome);
            }
        }
    }
}

fn hash_outbox_transition_header(
    hasher: &mut Sha256,
    item_id: OutboxItemId,
    expected_revision: u64,
    next_revision: u64,
    attempt: u32,
    turn_sequence: u64,
) {
    hasher.update(item_id.as_bytes());
    hasher.update(expected_revision.to_be_bytes());
    hasher.update(next_revision.to_be_bytes());
    hasher.update(attempt.to_be_bytes());
    hasher.update(turn_sequence.to_be_bytes());
}

fn hash_outbox_item(hasher: &mut Sha256, item: &DurableOutboxItem) {
    hasher.update(item.item_id.as_bytes());
    hasher.update(item.invocation_id.as_bytes());
    hasher.update(item.effect_id.as_bytes());
    match item.target_row {
        Some(row) => {
            hasher.update([1]);
            hasher.update(row.list_memory_id.as_bytes());
            hasher.update(row.row_key.to_be_bytes());
            hasher.update(row.row_generation.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    hash_stored_value(hasher, &item.idempotency_key);
    hash_stored_value(hasher, &item.intent);
    match &item.state {
        DurableOutboxState::Pending => hasher.update([0]),
        DurableOutboxState::Dispatching { attempt } => {
            hasher.update([1]);
            hasher.update(attempt.to_be_bytes());
        }
        DurableOutboxState::ReconciliationRequired { attempt } => {
            hasher.update([2]);
            hasher.update(attempt.to_be_bytes());
        }
        DurableOutboxState::Completed { attempt, outcome } => {
            hasher.update([3]);
            hasher.update(attempt.to_be_bytes());
            hash_stored_value(hasher, outcome);
        }
    }
    hasher.update(item.revision.to_be_bytes());
    hasher.update(item.created_turn_sequence.to_be_bytes());
    hasher.update(item.updated_turn_sequence.to_be_bytes());
}

fn hash_stored_row(hasher: &mut Sha256, row: &StoredRow) {
    hasher.update(row.key.to_be_bytes());
    hasher.update(row.generation.to_be_bytes());
    hasher.update((row.fields.len() as u64).to_be_bytes());
    for (field, field_value) in &row.fields {
        hasher.update(field.as_bytes());
        hash_stored_value(hasher, field_value);
    }
    hasher.update((row.touched_fields.len() as u64).to_be_bytes());
    for field in &row.touched_fields {
        hasher.update(field.as_bytes());
    }
}

fn hash_stored_value(hasher: &mut Sha256, value: &StoredValue) {
    match value {
        StoredValue::Null => hasher.update([0]),
        StoredValue::Bool(value) => {
            hasher.update([1, u8::from(*value)]);
        }
        StoredValue::Number(value) => {
            hasher.update([2]);
            hasher.update(value.to_be_bytes());
        }
        StoredValue::Text(value) => {
            hasher.update([3]);
            hash_text(hasher, value);
        }
        StoredValue::Bytes(value) => {
            hasher.update([4]);
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value);
        }
        StoredValue::List(values) => {
            hasher.update([5]);
            hasher.update((values.len() as u64).to_be_bytes());
            for value in values {
                hash_stored_value(hasher, value);
            }
        }
        StoredValue::Record(fields) => {
            hasher.update([6]);
            hash_value_fields(hasher, fields);
        }
        StoredValue::Variant { tag, fields } => {
            hasher.update([7]);
            hash_text(hasher, tag);
            hash_value_fields(hasher, fields);
        }
        StoredValue::Error { code, fields } => {
            hasher.update([8]);
            hash_text(hasher, code);
            hash_value_fields(hasher, fields);
        }
    }
}

fn hash_value_fields(hasher: &mut Sha256, fields: &BTreeMap<String, StoredValue>) {
    hasher.update((fields.len() as u64).to_be_bytes());
    for (name, value) in fields {
        hash_text(hasher, name);
        hash_stored_value(hasher, value);
    }
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn inspector_snapshot(image: &RestoreImage) -> PersistenceInspectorSnapshot {
    let mut outbox_pending_count = 0;
    let mut outbox_dispatching_count = 0;
    let mut outbox_reconciliation_count = 0;
    let mut outbox_completed_count = 0;
    let mut outbox_samples =
        Vec::with_capacity(image.outbox.len().min(MAX_OUTBOX_INSPECTOR_SAMPLES));
    for item in image.outbox.values() {
        let state = match item.state {
            DurableOutboxState::Pending => {
                outbox_pending_count += 1;
                OutboxInspectorState::Pending
            }
            DurableOutboxState::Dispatching { .. } => {
                outbox_dispatching_count += 1;
                OutboxInspectorState::Dispatching
            }
            DurableOutboxState::ReconciliationRequired { .. } => {
                outbox_reconciliation_count += 1;
                OutboxInspectorState::ReconciliationRequired
            }
            DurableOutboxState::Completed { .. } => {
                outbox_completed_count += 1;
                OutboxInspectorState::Completed
            }
        };
        if outbox_samples.len() < MAX_OUTBOX_INSPECTOR_SAMPLES {
            outbox_samples.push(OutboxInspectorSample {
                item_id: item.item_id,
                invocation_id: item.invocation_id,
                effect_id: item.effect_id,
                state,
                attempt: item.state.attempt(),
                created_turn_sequence: item.created_turn_sequence,
                updated_turn_sequence: item.updated_turn_sequence,
            });
        }
    }
    PersistenceInspectorSnapshot {
        application: image.application.clone(),
        schema_version: image.schema_version,
        schema_hash: image.schema_hash,
        epoch: image.epoch,
        through_turn_sequence: image.through_turn_sequence,
        scalar_count: image.scalars.len(),
        list_count: image.lists.len(),
        row_count: image.lists.values().map(|list| list.rows.len()).sum(),
        content_artifact_count: 0,
        content_artifact_bytes: 0,
        encoded_value_bytes: None,
        completed_migration_count: image.completed_migration_edges.len(),
        outbox_pending_count,
        outbox_dispatching_count,
        outbox_reconciliation_count,
        outbox_completed_count,
        outbox_samples,
    }
}

fn error_result(command: PersistenceCommand, error: StoreError) -> PersistenceResult {
    match command {
        PersistenceCommand::Load(_) => PersistenceResult::Loaded(Err(error)),
        PersistenceCommand::Initialize(_) => PersistenceResult::Initialized(Err(error)),
        PersistenceCommand::Commit(_) => PersistenceResult::Committed(Err(error)),
        PersistenceCommand::Activate(_) => PersistenceResult::Activated(Err(error)),
        PersistenceCommand::ResetApplication(_) => PersistenceResult::ApplicationReset(Err(error)),
        PersistenceCommand::Barrier(_) => PersistenceResult::BarrierComplete(Err(error)),
        PersistenceCommand::Inspect(_) => PersistenceResult::Inspected(Err(error)),
        PersistenceCommand::Compact(_) => PersistenceResult::Compacted(Err(error)),
        PersistenceCommand::PutContentArtifact(_) => {
            PersistenceResult::ContentArtifactStored(Err(error))
        }
        PersistenceCommand::LoadContentArtifact(_) => {
            PersistenceResult::ContentArtifactLoaded(Err(error))
        }
        PersistenceCommand::Shutdown(_) => PersistenceResult::ShutdownComplete(Err(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{MemoryKind, MemoryOwnerPath};

    fn application() -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.counter", "manual", "local")
    }

    fn memory(name: &str) -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "counter".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            name,
            MemoryKind::Scalar,
        )
        .unwrap()
    }

    fn list_memory(name: &str) -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "counter".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            name,
            MemoryKind::List,
        )
        .unwrap()
    }

    fn seeded_driver() -> InMemoryDriver {
        let mut driver = InMemoryDriver::default();
        driver.seed(RestoreImage::empty(application(), 1, [1; 32]));
        driver
    }

    #[test]
    fn content_artifacts_are_exact_idempotent_and_removed_by_reset() {
        let mut driver = seeded_driver();
        let artifact = ContentArtifact::new(
            "application/vnd.boon.test-artifact",
            b"immutable bytes".to_vec(),
        )
        .unwrap();
        let put = PutContentArtifactRequest {
            application: application(),
            artifact: artifact.clone(),
        };
        assert!(matches!(
            driver.execute(PersistenceCommand::PutContentArtifact(put.clone())),
            PersistenceResult::ContentArtifactStored(Ok(PutContentArtifactAck {
                already_present: false,
                ..
            }))
        ));
        assert!(matches!(
            driver.execute(PersistenceCommand::PutContentArtifact(put)),
            PersistenceResult::ContentArtifactStored(Ok(PutContentArtifactAck {
                already_present: true,
                ..
            }))
        ));
        assert_eq!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: application(),
                    id: artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(Some(artifact.clone())))
        );
        let snapshot = match driver.execute(PersistenceCommand::Inspect(InspectRequest {
            application: application(),
        })) {
            PersistenceResult::Inspected(Ok(Some(snapshot))) => snapshot,
            result => panic!("unexpected inspector result: {result:?}"),
        };
        assert_eq!(snapshot.content_artifact_count, 1);
        assert_eq!(snapshot.content_artifact_bytes, artifact.bytes.len() as u64);

        let mut corrupt = artifact.clone();
        corrupt.bytes.push(0xff);
        assert!(matches!(
            driver.execute(PersistenceCommand::PutContentArtifact(
                PutContentArtifactRequest {
                    application: application(),
                    artifact: corrupt,
                }
            )),
            PersistenceResult::ContentArtifactStored(Err(StoreError::InvalidContentArtifact(_)))
        ));

        let reset = ResetApplicationBatch {
            application: application(),
            expected_base_epoch: 0,
            next_epoch: 1,
            source_schema_hash: [1; 32],
            default_image: RestoreImage::empty(application(), 1, [1; 32]),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::ResetApplication(reset)),
            PersistenceResult::ApplicationReset(Ok(_))
        ));
        assert_eq!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: application(),
                    id: artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(None))
        );
        let snapshot = match driver.execute(PersistenceCommand::Inspect(InspectRequest {
            application: application(),
        })) {
            PersistenceResult::Inspected(Ok(Some(snapshot))) => snapshot,
            result => panic!("unexpected inspector result: {result:?}"),
        };
        assert_eq!(snapshot.content_artifact_count, 0);
        assert_eq!(snapshot.content_artifact_bytes, 0);
    }

    fn effect() -> EffectId {
        EffectId::from_host_operation("Test/send").unwrap()
    }

    fn invocation() -> EffectInvocationId {
        EffectInvocationId::from_semantic_route(effect(), "test.send", "store.result").unwrap()
    }

    fn pending_outbox(key: i64, turn_sequence: u64) -> DurableOutboxItem {
        DurableOutboxItem::pending(
            invocation(),
            effect(),
            StoredValue::Number(key),
            StoredValue::Record(BTreeMap::from([(
                "amount".to_owned(),
                StoredValue::Number(key * 10),
            )])),
            None,
            turn_sequence,
        )
    }

    #[test]
    fn indexed_effect_rows_keep_distinct_local_completion_obligations() {
        let intent = StoredValue::Record(BTreeMap::from([(
            "amount".to_owned(),
            StoredValue::Number(10),
        )]));
        let key = canonical_intent_key(&intent);
        let row = |row_key| DurableEffectRow {
            list_memory_id: memory("todos"),
            row_key,
            row_generation: 1,
        };
        let first = DurableOutboxItem::pending(
            invocation(),
            effect(),
            key.clone(),
            intent.clone(),
            Some(row(1)),
            1,
        );
        let second =
            DurableOutboxItem::pending(invocation(), effect(), key, intent, Some(row(2)), 1);

        assert_ne!(first.item_id, second.item_id);
        assert_eq!(first.idempotency_key, second.idempotency_key);
    }

    #[test]
    fn checkpoint_requires_a_contiguous_epoch_and_turn_range() {
        let mut driver = seeded_driver();
        let batch = CheckpointBatch {
            application: application(),
            schema_hash: [1; 32],
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: vec![DurableChange::SetScalar {
                memory_id: memory("count"),
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Number(0),
                },
            }],
            outbox_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(batch.clone())),
            PersistenceResult::Committed(Ok(CommitAck { epoch: 1, .. }))
        ));
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(batch)),
            PersistenceResult::Committed(Err(StoreError::StaleEpoch))
        ));
    }

    #[test]
    fn row_field_checkpoints_remain_sparse_and_can_coalesce_per_list() {
        let mut driver = seeded_driver();
        let list = list_memory("cells");
        let field = MemoryLeafId::from_memory_path(list, "formula_text").unwrap();
        let batch = CheckpointBatch {
            application: application(),
            schema_hash: [1; 32],
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 2,
            changes: vec![
                DurableChange::SetRowField {
                    memory_id: list,
                    row_key: 2,
                    row_generation: 1,
                    field_id: field,
                    value: StoredValue::Text("=A1".to_owned()),
                },
                DurableChange::SetRowField {
                    memory_id: list,
                    row_key: 2,
                    row_generation: 1,
                    field_id: field,
                    value: StoredValue::Text("=A1+1".to_owned()),
                },
            ],
            outbox_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal();

        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(batch)),
            PersistenceResult::Committed(Ok(CommitAck { epoch: 1, .. }))
        ));
        let stored = &driver.image(&application()).unwrap().lists[&list];
        assert!(!stored.touched);
        assert_eq!(stored.next_key, 0);
        assert_eq!(stored.rows.len(), 1);
        assert_eq!(
            stored.rows[0].fields[&field],
            StoredValue::Text("=A1+1".to_owned())
        );
    }

    #[test]
    fn failed_activation_leaves_the_old_store_unchanged() {
        let mut driver = seeded_driver();
        let old = driver.image(&application()).unwrap().clone();
        driver.fail_next_activation();
        let batch = ActivationBatch {
            application: application(),
            expected_base_epoch: 0,
            next_epoch: 1,
            source_schema_hash: [1; 32],
            target_schema_version: 2,
            target_schema_hash: [2; 32],
            through_turn_sequence: 0,
            authority_changes: vec![DurableChange::SetScalar {
                memory_id: memory("click_count"),
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Number(7),
                },
            }],
            completed_migration_edges: Vec::new(),
            deleted_memory: vec![memory("count")],
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Activate(batch)),
            PersistenceResult::Activated(Err(StoreError::Backend(_)))
        ));
        assert_eq!(driver.image(&application()), Some(&old));
    }

    #[test]
    fn activation_changes_schema_and_authority_atomically() {
        let mut driver = seeded_driver();
        let old_memory = memory("count");
        driver
            .applications
            .get_mut(&application())
            .unwrap()
            .scalars
            .insert(
                old_memory,
                StoredScalar {
                    touched: true,
                    value: StoredValue::Number(3),
                },
            );
        let new_memory = memory("click_count");
        let batch = ActivationBatch {
            application: application(),
            expected_base_epoch: 0,
            next_epoch: 1,
            source_schema_hash: [1; 32],
            target_schema_version: 2,
            target_schema_hash: [2; 32],
            through_turn_sequence: 4,
            authority_changes: vec![DurableChange::SetScalar {
                memory_id: new_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Number(3),
                },
            }],
            completed_migration_edges: Vec::new(),
            deleted_memory: vec![old_memory],
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Activate(batch)),
            PersistenceResult::Activated(Ok(ActivationAck {
                epoch: 1,
                schema_version: 2,
                ..
            }))
        ));
        let image = driver.image(&application()).unwrap();
        assert!(!image.scalars.contains_key(&old_memory));
        assert_eq!(image.scalars[&new_memory].value, StoredValue::Number(3));
        assert_eq!(image.schema_hash, [2; 32]);
        assert_eq!(image.through_turn_sequence, 4);
    }

    #[test]
    fn activation_batch_is_derived_from_complete_candidate_authority() {
        let old_memory = memory("count");
        let new_memory = memory("click_count");
        let mut current = RestoreImage::empty(application(), 1, [1; 32]);
        current.epoch = 4;
        current.through_turn_sequence = 8;
        current.scalars.insert(
            old_memory,
            StoredScalar {
                touched: true,
                value: StoredValue::Number(3),
            },
        );
        let mut candidate = RestoreImage::empty(application(), 2, [2; 32]);
        candidate.through_turn_sequence = 8;
        candidate.scalars.insert(
            new_memory,
            StoredScalar {
                touched: true,
                value: StoredValue::Number(3),
            },
        );

        let batch = ActivationBatch::between(&current, &candidate).unwrap();
        assert_eq!(batch.expected_base_epoch, 4);
        assert_eq!(batch.next_epoch, 5);
        assert_eq!(batch.deleted_memory, vec![old_memory]);
        assert!(matches!(
            batch.authority_changes.as_slice(),
            [DurableChange::SetScalar { memory_id, .. }] if *memory_id == new_memory
        ));
        let (activated, ack) = apply_activation_to_image(current, &batch).unwrap();
        assert_eq!(ack.epoch, 5);
        assert!(!activated.scalars.contains_key(&old_memory));
        assert_eq!(activated.scalars[&new_memory].value, StoredValue::Number(3));
    }

    #[test]
    fn outbox_transitions_are_replay_safe_bounded_and_atomic_with_authority() {
        let mut driver = seeded_driver();
        let items = (0..20)
            .map(|key| pending_outbox(key, 1))
            .collect::<Vec<_>>();
        let first_id = items[0].item_id;
        let mut outbox_changes = items
            .iter()
            .cloned()
            .map(|item| DurableOutboxChange::Enqueue { item })
            .collect::<Vec<_>>();
        outbox_changes.push(DurableOutboxChange::Enqueue {
            item: items[0].clone(),
        });
        let enqueue = CheckpointBatch {
            application: application(),
            schema_hash: [1; 32],
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: Vec::new(),
            outbox_changes,
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(enqueue)),
            PersistenceResult::Committed(Ok(_))
        ));
        let snapshot = match driver.execute(PersistenceCommand::Inspect(InspectRequest {
            application: application(),
        })) {
            PersistenceResult::Inspected(Ok(Some(snapshot))) => snapshot,
            result => panic!("unexpected inspector result: {result:?}"),
        };
        assert_eq!(snapshot.outbox_pending_count, 20);
        assert_eq!(snapshot.outbox_samples.len(), MAX_OUTBOX_INSPECTOR_SAMPLES);

        let dispatch = DurableOutboxChange::BeginDispatch {
            item_id: first_id,
            expected_revision: 0,
            next_revision: 1,
            attempt: 1,
            turn_sequence: 2,
        };
        let begin = CheckpointBatch {
            application: application(),
            schema_hash: [1; 32],
            base_epoch: 1,
            next_epoch: 2,
            first_turn_sequence: 2,
            last_turn_sequence: 2,
            changes: Vec::new(),
            outbox_changes: vec![dispatch.clone(), dispatch],
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(begin)),
            PersistenceResult::Committed(Ok(_))
        ));
        assert_eq!(
            driver.image(&application()).unwrap().outbox[&first_id].state,
            DurableOutboxState::Dispatching { attempt: 1 }
        );

        let before_invalid = driver.image(&application()).unwrap().clone();
        let invalid = CheckpointBatch {
            application: application(),
            schema_hash: [1; 32],
            base_epoch: 2,
            next_epoch: 3,
            first_turn_sequence: 3,
            last_turn_sequence: 3,
            changes: vec![DurableChange::SetScalar {
                memory_id: memory("must_not_commit"),
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Bool(true),
                },
            }],
            outbox_changes: vec![DurableOutboxChange::BeginDispatch {
                item_id: first_id,
                expected_revision: 0,
                next_revision: 1,
                attempt: 1,
                turn_sequence: 3,
            }],
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(invalid)),
            PersistenceResult::Committed(Err(StoreError::InvalidOutboxTransition(_)))
        ));
        assert_eq!(driver.image(&application()), Some(&before_invalid));
    }

    #[test]
    fn reset_application_clears_every_durable_domain_and_preserves_monotonicity() {
        let mut driver = seeded_driver();
        let current = driver.applications.get_mut(&application()).unwrap();
        current.epoch = 4;
        current.through_turn_sequence = 9;
        current.scalars.insert(
            memory("count"),
            StoredScalar {
                touched: true,
                value: StoredValue::Number(7),
            },
        );
        let item = pending_outbox(7, 9);
        current.outbox.insert(item.item_id, item);
        current
            .completed_migration_edges
            .insert(MigrationEdgeId([9; 32]));

        let default_image = RestoreImage::empty(application(), 2, [2; 32]);
        let batch = ResetApplicationBatch {
            application: application(),
            expected_base_epoch: 4,
            next_epoch: 5,
            source_schema_hash: [1; 32],
            default_image,
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::ResetApplication(batch.clone())),
            PersistenceResult::ApplicationReset(Ok(ResetApplicationAck {
                epoch: 5,
                schema_version: 2,
                through_turn_sequence: 9,
                ..
            }))
        ));
        let reset = driver.image(&application()).unwrap();
        assert!(reset.scalars.is_empty());
        assert!(reset.lists.is_empty());
        assert!(reset.completed_migration_edges.is_empty());
        assert!(reset.outbox.is_empty());
        assert_eq!(reset.schema_hash, [2; 32]);
        assert_eq!(reset.epoch, 5);
        assert_eq!(reset.through_turn_sequence, 9);
        assert!(matches!(
            driver.execute(PersistenceCommand::ResetApplication(batch)),
            PersistenceResult::ApplicationReset(Err(StoreError::StaleEpoch))
        ));
    }
}
