use super::{
    ApplicationTransfer, CheckpointBatch, ContentArtifact, ContentArtifactId, DurableChange,
    DurableEffectRow, DurableOutboxChange, DurableOutboxItem, DurableOutboxState, OutboxItemId,
    RestoreImage, StoredList, StoredRow, StoredScalar, StoredValue, validate_application_transfer,
};
use boon_plan::{
    ApplicationIdentity, EffectId, EffectInvocationId, MemoryId, MemoryLeafId, MigrationEdgeId,
};
use minicbor::{Decoder, Encoder};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const RESTORE_IMAGE_FORMAT: u32 = 3;
const APPLICATION_TRANSFER_FORMAT: u32 = 1;
const CHECKPOINT_BATCH_FORMAT: u32 = 2;
const OUTBOX_RECORD_FORMAT: u32 = 2;
const BLOB_RECORD_FORMAT: u32 = 1;
pub const INLINE_BYTES_THRESHOLD: usize = 16 * 1024;
type CborEncoder<'a> = Encoder<&'a mut Vec<u8>>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BlobDigest(pub [u8; 32]);

impl BlobDigest {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlobRecord {
    pub digest: BlobDigest,
    pub length: u64,
    pub reference_count: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EncodedComponent {
    pub bytes: Vec<u8>,
    pub blobs: BTreeMap<BlobDigest, BlobRecord>,
    pub references: BTreeMap<BlobDigest, u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    pub max_total_bytes: usize,
    pub max_text_bytes: usize,
    pub max_blob_bytes: usize,
    pub max_collection_items: usize,
    pub max_value_depth: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 64 * 1024 * 1024,
            max_text_bytes: 1024 * 1024,
            max_blob_bytes: 32 * 1024 * 1024,
            max_collection_items: 1_000_000,
            max_value_depth: 64,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodecError(String);

impl CodecError {
    fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CodecError {}

pub fn encode_restore_image(image: &RestoreImage) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(10)
        .and_then(|encoder| encoder.u32(RESTORE_IMAGE_FORMAT))
        .map_err(encode_error)?;
    encode_application(&mut encoder, &image.application)?;
    encoder
        .u64(image.schema_version)
        .and_then(|encoder| encoder.bytes(&image.schema_hash))
        .and_then(|encoder| encoder.u64(image.epoch))
        .and_then(|encoder| encoder.u64(image.through_turn_sequence))
        .map_err(encode_error)?;

    encoder
        .map(image.scalars.len() as u64)
        .map_err(encode_error)?;
    for (memory, scalar) in &image.scalars {
        encode_digest(&mut encoder, memory.as_bytes())?;
        encode_scalar(&mut encoder, scalar)?;
    }

    encoder
        .map(image.lists.len() as u64)
        .map_err(encode_error)?;
    for (memory, list) in &image.lists {
        encode_digest(&mut encoder, memory.as_bytes())?;
        encode_list(&mut encoder, list)?;
    }

    encoder
        .array(image.completed_migration_edges.len() as u64)
        .map_err(encode_error)?;
    for edge in &image.completed_migration_edges {
        encode_digest(&mut encoder, edge.as_bytes())?;
    }
    encoder
        .map(image.outbox.len() as u64)
        .map_err(encode_error)?;
    for (item_id, item) in &image.outbox {
        encode_digest(&mut encoder, item_id.as_bytes())?;
        encode_outbox_item(&mut encoder, item)?;
    }
    Ok(bytes)
}

pub fn decode_restore_image(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<RestoreImage, CodecError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(CodecError::new("restore image exceeds total byte limit"));
    }
    let mut decoder = Decoder::new(bytes);
    let root_len = definite_len(decoder.array().map_err(decode_error)?, "restore image")?;
    let format = decoder.u32().map_err(decode_error)?;
    if !matches!((format, root_len), (1, 9) | (RESTORE_IMAGE_FORMAT, 10)) {
        return Err(CodecError::new(format!(
            "unsupported restore image format {format} with {root_len} fields"
        )));
    }
    let application = decode_application(&mut decoder, limits)?;
    let schema_version = decoder.u64().map_err(decode_error)?;
    let schema_hash = decode_digest(&mut decoder)?;
    let epoch = decoder.u64().map_err(decode_error)?;
    let through_turn_sequence = decoder.u64().map_err(decode_error)?;

    let scalar_count = collection_len(&mut decoder, limits, "scalar map", false)?;
    let mut scalars = BTreeMap::new();
    for _ in 0..scalar_count {
        let memory = MemoryId(decode_digest(&mut decoder)?);
        let scalar = decode_scalar(&mut decoder, limits)?;
        if scalars.insert(memory, scalar).is_some() {
            return Err(CodecError::new("restore image repeats a scalar memory ID"));
        }
    }

    let list_count = collection_len(&mut decoder, limits, "list map", false)?;
    let mut lists = BTreeMap::new();
    for _ in 0..list_count {
        let memory = MemoryId(decode_digest(&mut decoder)?);
        let list = decode_list(&mut decoder, limits)?;
        if lists.insert(memory, list).is_some() {
            return Err(CodecError::new("restore image repeats a list memory ID"));
        }
    }

    let edge_count = collection_len(&mut decoder, limits, "migration edge list", true)?;
    let mut completed_migration_edges = BTreeSet::new();
    for _ in 0..edge_count {
        let edge = MigrationEdgeId(decode_digest(&mut decoder)?);
        if !completed_migration_edges.insert(edge) {
            return Err(CodecError::new("restore image repeats a migration edge ID"));
        }
    }
    let mut outbox = BTreeMap::new();
    if format >= 2 {
        let item_count = collection_len(&mut decoder, limits, "outbox map", false)?;
        for _ in 0..item_count {
            let item_id = OutboxItemId(decode_digest(&mut decoder)?);
            let item = decode_outbox_item(&mut decoder, limits)?;
            if item.item_id != item_id {
                return Err(CodecError::new(
                    "restore image outbox key does not match item ID",
                ));
            }
            if outbox.insert(item_id, item).is_some() {
                return Err(CodecError::new("restore image repeats an outbox item ID"));
            }
        }
    }
    if decoder.position() != bytes.len() {
        return Err(CodecError::new("restore image has trailing bytes"));
    }

    Ok(RestoreImage {
        application,
        schema_version,
        schema_hash,
        epoch,
        through_turn_sequence,
        scalars,
        lists,
        completed_migration_edges,
        outbox,
    })
}

pub fn encode_application_transfer(transfer: &ApplicationTransfer) -> Result<Vec<u8>, CodecError> {
    validate_application_transfer(transfer).map_err(|error| CodecError::new(error.to_string()))?;
    let restore = encode_restore_image(&transfer.restore_image)?;
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(3)
        .and_then(|encoder| encoder.u32(APPLICATION_TRANSFER_FORMAT))
        .and_then(|encoder| encoder.bytes(&restore))
        .and_then(|encoder| encoder.map(transfer.content_artifacts.len() as u64))
        .map_err(encode_error)?;
    for (id, artifact) in &transfer.content_artifacts {
        encode_digest(&mut encoder, id.as_bytes())?;
        encoder
            .array(2)
            .and_then(|encoder| encoder.str(&artifact.media_type))
            .and_then(|encoder| encoder.bytes(&artifact.bytes))
            .map_err(encode_error)?;
    }
    Ok(bytes)
}

pub fn decode_application_transfer(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ApplicationTransfer, CodecError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(CodecError::new(
            "application transfer exceeds total byte limit",
        ));
    }
    let mut decoder = Decoder::new(bytes);
    let root_len = definite_len(
        decoder.array().map_err(decode_error)?,
        "application transfer",
    )?;
    let format = decoder.u32().map_err(decode_error)?;
    if format != APPLICATION_TRANSFER_FORMAT || root_len != 3 {
        return Err(CodecError::new(format!(
            "unsupported application transfer format {format} with {root_len} fields"
        )));
    }
    let restore_bytes = decoder.bytes().map_err(decode_error)?;
    if restore_bytes.len() > limits.max_total_bytes {
        return Err(CodecError::new(
            "application transfer restore image exceeds total byte limit",
        ));
    }
    let restore_image = decode_restore_image(restore_bytes, limits)?;
    let artifact_count = collection_len(&mut decoder, limits, "content artifact map", false)?;
    let mut content_artifacts = BTreeMap::new();
    let mut artifact_bytes = 0usize;
    for _ in 0..artifact_count {
        let id = ContentArtifactId(decode_digest(&mut decoder)?);
        let len = definite_len(decoder.array().map_err(decode_error)?, "content artifact")?;
        if len != 2 {
            return Err(CodecError::new(format!(
                "content artifact has {len} fields, expected 2"
            )));
        }
        let media_type = decode_text(&mut decoder, limits)?;
        let payload = decoder.bytes().map_err(decode_error)?;
        artifact_bytes = artifact_bytes
            .checked_add(payload.len())
            .ok_or_else(|| CodecError::new("content artifact byte count overflow"))?;
        if artifact_bytes > limits.max_total_bytes {
            return Err(CodecError::new("content artifacts exceed total byte limit"));
        }
        let artifact = ContentArtifact {
            id,
            media_type,
            bytes: payload.to_vec(),
        };
        if content_artifacts.insert(id, artifact).is_some() {
            return Err(CodecError::new(
                "application transfer repeats a content artifact ID",
            ));
        }
    }
    if decoder.position() != bytes.len() {
        return Err(CodecError::new("application transfer has trailing bytes"));
    }
    let transfer = ApplicationTransfer {
        restore_image,
        content_artifacts,
    };
    validate_application_transfer(&transfer).map_err(|error| CodecError::new(error.to_string()))?;
    Ok(transfer)
}

pub fn encode_checkpoint_batch(batch: &CheckpointBatch) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(10)
        .and_then(|encoder| encoder.u32(CHECKPOINT_BATCH_FORMAT))
        .map_err(encode_error)?;
    encode_application(&mut encoder, &batch.application)?;
    encoder
        .bytes(&batch.schema_hash)
        .and_then(|encoder| encoder.u64(batch.base_epoch))
        .and_then(|encoder| encoder.u64(batch.next_epoch))
        .and_then(|encoder| encoder.u64(batch.first_turn_sequence))
        .and_then(|encoder| encoder.u64(batch.last_turn_sequence))
        .and_then(|encoder| encoder.array(batch.changes.len() as u64))
        .map_err(encode_error)?;
    for change in &batch.changes {
        encode_durable_change(&mut encoder, change)?;
    }
    encoder
        .array(batch.outbox_changes.len() as u64)
        .map_err(encode_error)?;
    for change in &batch.outbox_changes {
        encode_outbox_change(&mut encoder, change)?;
    }
    encoder.bytes(&batch.checksum).map_err(encode_error)?;
    Ok(bytes)
}

pub fn decode_checkpoint_batch(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<CheckpointBatch, CodecError> {
    component_size(bytes, limits, "checkpoint batch")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 10, "checkpoint batch")?;
    let format = decoder.u32().map_err(decode_error)?;
    if format != CHECKPOINT_BATCH_FORMAT {
        return Err(CodecError::new(format!(
            "unsupported checkpoint batch format {format}"
        )));
    }
    let application = decode_application(&mut decoder, limits)?;
    let schema_hash = decode_digest(&mut decoder)?;
    let base_epoch = decoder.u64().map_err(decode_error)?;
    let next_epoch = decoder.u64().map_err(decode_error)?;
    let first_turn_sequence = decoder.u64().map_err(decode_error)?;
    let last_turn_sequence = decoder.u64().map_err(decode_error)?;
    let change_count = collection_len(&mut decoder, limits, "durable changes", true)?;
    let mut changes = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        changes.push(decode_durable_change(&mut decoder, limits)?);
    }
    let outbox_count = collection_len(&mut decoder, limits, "outbox changes", true)?;
    let mut outbox_changes = Vec::with_capacity(outbox_count);
    for _ in 0..outbox_count {
        outbox_changes.push(decode_outbox_change(&mut decoder, limits)?);
    }
    let checksum = decode_digest(&mut decoder)?;
    reject_trailing(&decoder, bytes, "checkpoint batch")?;
    Ok(CheckpointBatch {
        application,
        schema_hash,
        base_epoch,
        next_epoch,
        first_turn_sequence,
        last_turn_sequence,
        changes,
        outbox_changes,
        checksum,
    })
}

pub(crate) fn encode_outbox_record(item: &DurableOutboxItem) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = Encoder::new(&mut bytes);
        encoder
            .array(2)
            .and_then(|encoder| encoder.u32(OUTBOX_RECORD_FORMAT))
            .map_err(encode_error)?;
        encode_outbox_item(&mut encoder, item)?;
    }
    component_size(&bytes, DecodeLimits::default(), "outbox record")?;
    Ok(bytes)
}

pub(crate) fn decode_outbox_record(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<DurableOutboxItem, CodecError> {
    component_size(bytes, limits, "outbox record")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 2, "outbox record")?;
    let format = decoder.u32().map_err(decode_error)?;
    if format != OUTBOX_RECORD_FORMAT {
        return Err(CodecError::new(format!(
            "unsupported outbox record format {format}"
        )));
    }
    let item = decode_outbox_item(&mut decoder, limits)?;
    reject_trailing(&decoder, bytes, "outbox record")?;
    Ok(item)
}

pub(crate) fn encode_scalar_component(
    scalar: &StoredScalar,
) -> Result<EncodedComponent, CodecError> {
    let mut bytes = Vec::new();
    let mut blobs = BTreeMap::new();
    let mut references = BTreeMap::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(2)
        .and_then(|encoder| encoder.bool(scalar.touched))
        .map_err(encode_error)?;
    encode_component_value(&mut encoder, &scalar.value, 0, &mut blobs, &mut references)?;
    Ok(EncodedComponent {
        bytes,
        blobs,
        references,
    })
}

pub(crate) fn decode_scalar_component(
    bytes: &[u8],
    limits: DecodeLimits,
    blobs: &BTreeMap<BlobDigest, BlobRecord>,
) -> Result<StoredScalar, CodecError> {
    component_size(bytes, limits, "stored scalar")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 2, "stored scalar")?;
    let touched = decoder.bool().map_err(decode_error)?;
    let mut references = BTreeMap::new();
    let value = decode_component_value(&mut decoder, limits, 0, Some(blobs), &mut references)?;
    reject_trailing(&decoder, bytes, "stored scalar")?;
    Ok(StoredScalar { touched, value })
}

pub(crate) fn scalar_component_blob_references(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<BTreeMap<BlobDigest, u64>, CodecError> {
    component_size(bytes, limits, "stored scalar")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 2, "stored scalar")?;
    decoder.bool().map_err(decode_error)?;
    let mut references = BTreeMap::new();
    decode_component_value(&mut decoder, limits, 0, None, &mut references)?;
    reject_trailing(&decoder, bytes, "stored scalar")?;
    Ok(references)
}

pub(crate) fn encode_row_component(row: &StoredRow) -> Result<EncodedComponent, CodecError> {
    let mut bytes = Vec::new();
    let mut blobs = BTreeMap::new();
    let mut references = BTreeMap::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(4)
        .and_then(|encoder| encoder.u64(row.key))
        .and_then(|encoder| encoder.u64(row.generation))
        .and_then(|encoder| encoder.map(row.fields.len() as u64))
        .map_err(encode_error)?;
    for (field, value) in &row.fields {
        encode_digest(&mut encoder, field.as_bytes())?;
        encode_component_value(&mut encoder, value, 0, &mut blobs, &mut references)?;
    }
    encoder
        .array(row.touched_fields.len() as u64)
        .map_err(encode_error)?;
    for field in &row.touched_fields {
        encode_digest(&mut encoder, field.as_bytes())?;
    }
    Ok(EncodedComponent {
        bytes,
        blobs,
        references,
    })
}

pub(crate) fn decode_row_component(
    bytes: &[u8],
    limits: DecodeLimits,
    blobs: &BTreeMap<BlobDigest, BlobRecord>,
) -> Result<StoredRow, CodecError> {
    component_size(bytes, limits, "stored row")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 4, "stored row")?;
    let key = decoder.u64().map_err(decode_error)?;
    let generation = decoder.u64().map_err(decode_error)?;
    let field_count = collection_len(&mut decoder, limits, "row fields", false)?;
    let mut fields = BTreeMap::new();
    let mut references = BTreeMap::new();
    for _ in 0..field_count {
        let field = MemoryLeafId(decode_digest(&mut decoder)?);
        let value = decode_component_value(&mut decoder, limits, 0, Some(blobs), &mut references)?;
        if fields.insert(field, value).is_some() {
            return Err(CodecError::new("stored row repeats a field ID"));
        }
    }
    let touched_count = collection_len(&mut decoder, limits, "touched row fields", true)?;
    let mut touched_fields = BTreeSet::new();
    for _ in 0..touched_count {
        let field = MemoryLeafId(decode_digest(&mut decoder)?);
        if !touched_fields.insert(field) {
            return Err(CodecError::new("stored row repeats a touched field ID"));
        }
    }
    reject_trailing(&decoder, bytes, "stored row")?;
    Ok(StoredRow {
        key,
        generation,
        fields,
        touched_fields,
    })
}

pub(crate) fn row_component_blob_references(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<BTreeMap<BlobDigest, u64>, CodecError> {
    component_size(bytes, limits, "stored row")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 4, "stored row")?;
    decoder.u64().map_err(decode_error)?;
    decoder.u64().map_err(decode_error)?;
    let field_count = collection_len(&mut decoder, limits, "row fields", false)?;
    let mut references = BTreeMap::new();
    for _ in 0..field_count {
        decode_digest(&mut decoder)?;
        decode_component_value(&mut decoder, limits, 0, None, &mut references)?;
    }
    let touched_count = collection_len(&mut decoder, limits, "touched row fields", true)?;
    for _ in 0..touched_count {
        decode_digest(&mut decoder)?;
    }
    reject_trailing(&decoder, bytes, "stored row")?;
    Ok(references)
}

pub(crate) fn encode_blob_record(record: &BlobRecord) -> Result<Vec<u8>, CodecError> {
    validate_blob_record(record, DecodeLimits::default())?;
    let mut bytes = Vec::new();
    Encoder::new(&mut bytes)
        .array(5)
        .and_then(|encoder| encoder.u32(BLOB_RECORD_FORMAT))
        .and_then(|encoder| encoder.bytes(&record.digest.0))
        .and_then(|encoder| encoder.u64(record.length))
        .and_then(|encoder| encoder.u64(record.reference_count))
        .and_then(|encoder| encoder.bytes(&record.bytes))
        .map_err(encode_error)?;
    Ok(bytes)
}

pub(crate) fn decode_blob_record(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<BlobRecord, CodecError> {
    component_size(bytes, limits, "blob record")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 5, "blob record")?;
    let format = decoder.u32().map_err(decode_error)?;
    if format != BLOB_RECORD_FORMAT {
        return Err(CodecError::new(format!(
            "unsupported blob record format {format}"
        )));
    }
    let digest = BlobDigest(decode_digest(&mut decoder)?);
    let length = decoder.u64().map_err(decode_error)?;
    let reference_count = decoder.u64().map_err(decode_error)?;
    let payload = decoder.bytes().map_err(decode_error)?;
    if payload.len() > limits.max_blob_bytes {
        return Err(CodecError::new("blob payload exceeds decode limit"));
    }
    let record = BlobRecord {
        digest,
        length,
        reference_count,
        bytes: payload.to_vec(),
    };
    validate_blob_record(&record, limits)?;
    reject_trailing(&decoder, bytes, "blob record")?;
    Ok(record)
}

fn validate_blob_record(record: &BlobRecord, limits: DecodeLimits) -> Result<(), CodecError> {
    if record.bytes.len() > limits.max_blob_bytes {
        return Err(CodecError::new("blob payload exceeds decode limit"));
    }
    if record.length != record.bytes.len() as u64 {
        return Err(CodecError::new(
            "blob payload length does not match metadata",
        ));
    }
    if record.digest != BlobDigest::of(&record.bytes) {
        return Err(CodecError::new(
            "blob payload digest does not match its content",
        ));
    }
    if record.reference_count == 0 {
        return Err(CodecError::new("blob record has zero references"));
    }
    Ok(())
}

fn component_size(bytes: &[u8], limits: DecodeLimits, label: &str) -> Result<(), CodecError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(CodecError::new(format!("{label} exceeds total byte limit")));
    }
    Ok(())
}

fn reject_trailing(decoder: &Decoder<'_>, bytes: &[u8], label: &str) -> Result<(), CodecError> {
    if decoder.position() != bytes.len() {
        return Err(CodecError::new(format!("{label} has trailing bytes")));
    }
    Ok(())
}

fn encode_durable_change(
    encoder: &mut CborEncoder<'_>,
    change: &DurableChange,
) -> Result<(), CodecError> {
    match change {
        DurableChange::SetScalar { memory_id, value } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
            encode_scalar(encoder, value)?;
        }
        DurableChange::DeleteScalar { memory_id } => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(1))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
        }
        DurableChange::SetList { memory_id, value } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(2))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
            encode_list(encoder, value)?;
        }
        DurableChange::SetRowField {
            memory_id,
            row_key,
            row_generation,
            field_id,
            value,
        } => {
            encoder
                .array(6)
                .and_then(|encoder| encoder.u8(3))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
            encoder
                .u64(*row_key)
                .and_then(|encoder| encoder.u64(*row_generation))
                .map_err(encode_error)?;
            encode_digest(encoder, field_id.as_bytes())?;
            encode_value(encoder, value, 0)?;
        }
        DurableChange::InsertRow {
            memory_id,
            index,
            row,
            next_key,
        } => {
            encoder
                .array(5)
                .and_then(|encoder| encoder.u8(4))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
            encoder.u64(*index).map_err(encode_error)?;
            encode_row(encoder, row)?;
            encoder.u64(*next_key).map_err(encode_error)?;
        }
        DurableChange::RemoveRow {
            memory_id,
            row_key,
            row_generation,
            next_key,
        } => {
            encoder
                .array(5)
                .and_then(|encoder| encoder.u8(5))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
            encoder
                .u64(*row_key)
                .and_then(|encoder| encoder.u64(*row_generation))
                .and_then(|encoder| encoder.u64(*next_key))
                .map_err(encode_error)?;
        }
        DurableChange::DeleteList { memory_id } => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(6))
                .map_err(encode_error)?;
            encode_digest(encoder, memory_id.as_bytes())?;
        }
    }
    Ok(())
}

fn decode_durable_change(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<DurableChange, CodecError> {
    let len = definite_len(decoder.array().map_err(decode_error)?, "durable change")?;
    let tag = decoder.u8().map_err(decode_error)?;
    match (tag, len) {
        (0, 3) => Ok(DurableChange::SetScalar {
            memory_id: MemoryId(decode_digest(decoder)?),
            value: decode_scalar(decoder, limits)?,
        }),
        (1, 2) => Ok(DurableChange::DeleteScalar {
            memory_id: MemoryId(decode_digest(decoder)?),
        }),
        (2, 3) => Ok(DurableChange::SetList {
            memory_id: MemoryId(decode_digest(decoder)?),
            value: decode_list(decoder, limits)?,
        }),
        (3, 6) => Ok(DurableChange::SetRowField {
            memory_id: MemoryId(decode_digest(decoder)?),
            row_key: decoder.u64().map_err(decode_error)?,
            row_generation: decoder.u64().map_err(decode_error)?,
            field_id: MemoryLeafId(decode_digest(decoder)?),
            value: decode_value(decoder, limits, 0)?,
        }),
        (4, 5) => Ok(DurableChange::InsertRow {
            memory_id: MemoryId(decode_digest(decoder)?),
            index: decoder.u64().map_err(decode_error)?,
            row: decode_row(decoder, limits)?,
            next_key: decoder.u64().map_err(decode_error)?,
        }),
        (5, 5) => Ok(DurableChange::RemoveRow {
            memory_id: MemoryId(decode_digest(decoder)?),
            row_key: decoder.u64().map_err(decode_error)?,
            row_generation: decoder.u64().map_err(decode_error)?,
            next_key: decoder.u64().map_err(decode_error)?,
        }),
        (6, 2) => Ok(DurableChange::DeleteList {
            memory_id: MemoryId(decode_digest(decoder)?),
        }),
        _ => Err(CodecError::new(format!(
            "unknown durable change tag {tag} with array length {len}"
        ))),
    }
}

fn encode_outbox_item(
    encoder: &mut CborEncoder<'_>,
    item: &DurableOutboxItem,
) -> Result<(), CodecError> {
    encoder.array(10).map_err(encode_error)?;
    encode_digest(encoder, item.item_id.as_bytes())?;
    encode_digest(encoder, item.invocation_id.as_bytes())?;
    encode_digest(encoder, item.effect_id.as_bytes())?;
    encode_effect_row(encoder, item.target_row)?;
    encode_value(encoder, &item.idempotency_key, 0)?;
    encode_value(encoder, &item.intent, 0)?;
    encode_outbox_state(encoder, &item.state)?;
    encoder
        .u64(item.revision)
        .and_then(|encoder| encoder.u64(item.created_turn_sequence))
        .and_then(|encoder| encoder.u64(item.updated_turn_sequence))
        .map_err(encode_error)?;
    Ok(())
}

fn decode_outbox_item(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<DurableOutboxItem, CodecError> {
    expect_array(decoder, 10, "outbox item")?;
    Ok(DurableOutboxItem {
        item_id: OutboxItemId(decode_digest(decoder)?),
        invocation_id: EffectInvocationId(decode_digest(decoder)?),
        effect_id: EffectId(decode_digest(decoder)?),
        target_row: decode_effect_row(decoder)?,
        idempotency_key: decode_value(decoder, limits, 0)?,
        intent: decode_value(decoder, limits, 0)?,
        state: decode_outbox_state(decoder, limits)?,
        revision: decoder.u64().map_err(decode_error)?,
        created_turn_sequence: decoder.u64().map_err(decode_error)?,
        updated_turn_sequence: decoder.u64().map_err(decode_error)?,
    })
}

fn encode_effect_row(
    encoder: &mut CborEncoder<'_>,
    row: Option<DurableEffectRow>,
) -> Result<(), CodecError> {
    match row {
        Some(row) => {
            encoder
                .array(4)
                .and_then(|encoder| encoder.u8(1))
                .map_err(encode_error)?;
            encode_digest(encoder, row.list_memory_id.as_bytes())?;
            encoder
                .u64(row.row_key)
                .and_then(|encoder| encoder.u64(row.row_generation))
                .map_err(encode_error)?;
        }
        None => {
            encoder
                .array(1)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
        }
    }
    Ok(())
}

fn decode_effect_row(decoder: &mut Decoder<'_>) -> Result<Option<DurableEffectRow>, CodecError> {
    let len = definite_len(decoder.array().map_err(decode_error)?, "effect target row")?;
    match (decoder.u8().map_err(decode_error)?, len) {
        (0, 1) => Ok(None),
        (1, 4) => Ok(Some(DurableEffectRow {
            list_memory_id: MemoryId(decode_digest(decoder)?),
            row_key: decoder.u64().map_err(decode_error)?,
            row_generation: decoder.u64().map_err(decode_error)?,
        })),
        (tag, len) => Err(CodecError::new(format!(
            "unknown effect target row tag {tag} with array length {len}"
        ))),
    }
}

fn encode_outbox_state(
    encoder: &mut CborEncoder<'_>,
    state: &DurableOutboxState,
) -> Result<(), CodecError> {
    match state {
        DurableOutboxState::Pending => {
            encoder
                .array(1)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
        }
        DurableOutboxState::Dispatching { attempt } => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(1))
                .and_then(|encoder| encoder.u32(*attempt))
                .map_err(encode_error)?;
        }
        DurableOutboxState::ReconciliationRequired { attempt } => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(2))
                .and_then(|encoder| encoder.u32(*attempt))
                .map_err(encode_error)?;
        }
        DurableOutboxState::Completed { attempt, outcome } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(3))
                .and_then(|encoder| encoder.u32(*attempt))
                .map_err(encode_error)?;
            encode_value(encoder, outcome, 0)?;
        }
    }
    Ok(())
}

fn decode_outbox_state(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<DurableOutboxState, CodecError> {
    let len = definite_len(decoder.array().map_err(decode_error)?, "outbox state")?;
    let tag = decoder.u8().map_err(decode_error)?;
    match (tag, len) {
        (0, 1) => Ok(DurableOutboxState::Pending),
        (1, 2) => Ok(DurableOutboxState::Dispatching {
            attempt: decoder.u32().map_err(decode_error)?,
        }),
        (2, 2) => Ok(DurableOutboxState::ReconciliationRequired {
            attempt: decoder.u32().map_err(decode_error)?,
        }),
        (3, 3) => Ok(DurableOutboxState::Completed {
            attempt: decoder.u32().map_err(decode_error)?,
            outcome: decode_value(decoder, limits, 0)?,
        }),
        _ => Err(CodecError::new(format!(
            "unknown outbox state tag {tag} with array length {len}"
        ))),
    }
}

fn encode_outbox_change(
    encoder: &mut CborEncoder<'_>,
    change: &DurableOutboxChange,
) -> Result<(), CodecError> {
    match change {
        DurableOutboxChange::Enqueue { item } => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
            encode_outbox_item(encoder, item)?;
        }
        DurableOutboxChange::BeginDispatch {
            item_id,
            expected_revision,
            next_revision,
            attempt,
            turn_sequence,
        }
        | DurableOutboxChange::RequireReconciliation {
            item_id,
            expected_revision,
            next_revision,
            attempt,
            turn_sequence,
        } => {
            let tag = u8::from(matches!(
                change,
                DurableOutboxChange::RequireReconciliation { .. }
            )) + 1;
            encoder
                .array(6)
                .and_then(|encoder| encoder.u8(tag))
                .map_err(encode_error)?;
            encode_digest(encoder, item_id.as_bytes())?;
            encoder
                .u64(*expected_revision)
                .and_then(|encoder| encoder.u64(*next_revision))
                .and_then(|encoder| encoder.u32(*attempt))
                .and_then(|encoder| encoder.u64(*turn_sequence))
                .map_err(encode_error)?;
        }
        DurableOutboxChange::Complete {
            item_id,
            expected_revision,
            next_revision,
            attempt,
            outcome,
            turn_sequence,
        } => {
            encoder
                .array(7)
                .and_then(|encoder| encoder.u8(3))
                .map_err(encode_error)?;
            encode_digest(encoder, item_id.as_bytes())?;
            encoder
                .u64(*expected_revision)
                .and_then(|encoder| encoder.u64(*next_revision))
                .and_then(|encoder| encoder.u32(*attempt))
                .map_err(encode_error)?;
            encode_value(encoder, outcome, 0)?;
            encoder.u64(*turn_sequence).map_err(encode_error)?;
        }
    }
    Ok(())
}

fn decode_outbox_change(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<DurableOutboxChange, CodecError> {
    let len = definite_len(decoder.array().map_err(decode_error)?, "outbox change")?;
    let tag = decoder.u8().map_err(decode_error)?;
    match (tag, len) {
        (0, 2) => Ok(DurableOutboxChange::Enqueue {
            item: decode_outbox_item(decoder, limits)?,
        }),
        (1, 6) | (2, 6) => {
            let item_id = OutboxItemId(decode_digest(decoder)?);
            let expected_revision = decoder.u64().map_err(decode_error)?;
            let next_revision = decoder.u64().map_err(decode_error)?;
            let attempt = decoder.u32().map_err(decode_error)?;
            let turn_sequence = decoder.u64().map_err(decode_error)?;
            if tag == 1 {
                Ok(DurableOutboxChange::BeginDispatch {
                    item_id,
                    expected_revision,
                    next_revision,
                    attempt,
                    turn_sequence,
                })
            } else {
                Ok(DurableOutboxChange::RequireReconciliation {
                    item_id,
                    expected_revision,
                    next_revision,
                    attempt,
                    turn_sequence,
                })
            }
        }
        (3, 7) => Ok(DurableOutboxChange::Complete {
            item_id: OutboxItemId(decode_digest(decoder)?),
            expected_revision: decoder.u64().map_err(decode_error)?,
            next_revision: decoder.u64().map_err(decode_error)?,
            attempt: decoder.u32().map_err(decode_error)?,
            outcome: decode_value(decoder, limits, 0)?,
            turn_sequence: decoder.u64().map_err(decode_error)?,
        }),
        _ => Err(CodecError::new(format!(
            "unknown outbox change tag {tag} with array length {len}"
        ))),
    }
}

fn encode_application(
    encoder: &mut CborEncoder<'_>,
    application: &ApplicationIdentity,
) -> Result<(), CodecError> {
    encoder
        .array(3)
        .and_then(|encoder| encoder.str(&application.package_id))
        .and_then(|encoder| encoder.str(&application.state_namespace))
        .and_then(|encoder| encoder.str(&application.deployment_domain))
        .map_err(encode_error)?;
    Ok(())
}

fn decode_application(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<ApplicationIdentity, CodecError> {
    expect_array(decoder, 3, "application identity")?;
    Ok(ApplicationIdentity::new(
        decode_text(decoder, limits)?,
        decode_text(decoder, limits)?,
        decode_text(decoder, limits)?,
    ))
}

fn encode_scalar(encoder: &mut CborEncoder<'_>, scalar: &StoredScalar) -> Result<(), CodecError> {
    encoder
        .array(2)
        .and_then(|encoder| encoder.bool(scalar.touched))
        .map_err(encode_error)?;
    encode_value(encoder, &scalar.value, 0)
}

fn decode_scalar(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
) -> Result<StoredScalar, CodecError> {
    expect_array(decoder, 2, "stored scalar")?;
    Ok(StoredScalar {
        touched: decoder.bool().map_err(decode_error)?,
        value: decode_value(decoder, limits, 0)?,
    })
}

fn encode_list(encoder: &mut CborEncoder<'_>, list: &StoredList) -> Result<(), CodecError> {
    encoder
        .array(3)
        .and_then(|encoder| encoder.bool(list.touched))
        .and_then(|encoder| encoder.u64(list.next_key))
        .and_then(|encoder| encoder.array(list.rows.len() as u64))
        .map_err(encode_error)?;
    for row in &list.rows {
        encode_row(encoder, row)?;
    }
    Ok(())
}

fn decode_list(decoder: &mut Decoder<'_>, limits: DecodeLimits) -> Result<StoredList, CodecError> {
    expect_array(decoder, 3, "stored list")?;
    let touched = decoder.bool().map_err(decode_error)?;
    let next_key = decoder.u64().map_err(decode_error)?;
    let row_count = collection_len(decoder, limits, "stored rows", true)?;
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        rows.push(decode_row(decoder, limits)?);
    }
    Ok(StoredList {
        touched,
        next_key,
        rows,
    })
}

fn encode_row(encoder: &mut CborEncoder<'_>, row: &StoredRow) -> Result<(), CodecError> {
    encoder
        .array(4)
        .and_then(|encoder| encoder.u64(row.key))
        .and_then(|encoder| encoder.u64(row.generation))
        .and_then(|encoder| encoder.map(row.fields.len() as u64))
        .map_err(encode_error)?;
    for (field, value) in &row.fields {
        encode_digest(encoder, field.as_bytes())?;
        encode_value(encoder, value, 0)?;
    }
    encoder
        .array(row.touched_fields.len() as u64)
        .map_err(encode_error)?;
    for field in &row.touched_fields {
        encode_digest(encoder, field.as_bytes())?;
    }
    Ok(())
}

fn decode_row(decoder: &mut Decoder<'_>, limits: DecodeLimits) -> Result<StoredRow, CodecError> {
    expect_array(decoder, 4, "stored row")?;
    let key = decoder.u64().map_err(decode_error)?;
    let generation = decoder.u64().map_err(decode_error)?;
    let field_count = collection_len(decoder, limits, "row fields", false)?;
    let mut fields = BTreeMap::new();
    for _ in 0..field_count {
        let field = MemoryLeafId(decode_digest(decoder)?);
        let value = decode_value(decoder, limits, 0)?;
        if fields.insert(field, value).is_some() {
            return Err(CodecError::new("stored row repeats a field ID"));
        }
    }
    let touched_count = collection_len(decoder, limits, "touched row fields", true)?;
    let mut touched_fields = BTreeSet::new();
    for _ in 0..touched_count {
        let field = MemoryLeafId(decode_digest(decoder)?);
        if !touched_fields.insert(field) {
            return Err(CodecError::new("stored row repeats a touched field ID"));
        }
    }
    Ok(StoredRow {
        key,
        generation,
        fields,
        touched_fields,
    })
}

fn encode_component_value(
    encoder: &mut CborEncoder<'_>,
    value: &StoredValue,
    depth: usize,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
    references: &mut BTreeMap<BlobDigest, u64>,
) -> Result<(), CodecError> {
    if depth > 64 {
        return Err(CodecError::new(
            "stored value nesting exceeds encoder limit",
        ));
    }
    match value {
        StoredValue::Bytes(value) if value.len() > INLINE_BYTES_THRESHOLD => {
            let digest = BlobDigest::of(value);
            let count = references.entry(digest).or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| CodecError::new("blob reference count overflow"))?;
            match blobs.entry(digest) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(BlobRecord {
                        digest,
                        length: value.len() as u64,
                        reference_count: 1,
                        bytes: value.clone(),
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let record = entry.get_mut();
                    if record.bytes != *value {
                        return Err(CodecError::new("blob digest collision"));
                    }
                    record.reference_count = record
                        .reference_count
                        .checked_add(1)
                        .ok_or_else(|| CodecError::new("blob reference count overflow"))?;
                }
            }
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(9))
                .and_then(|encoder| encoder.bytes(&digest.0))
                .and_then(|encoder| encoder.u64(value.len() as u64))
                .map_err(encode_error)?;
        }
        StoredValue::Null => {
            encoder
                .array(1)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
        }
        StoredValue::Bool(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(1))
                .and_then(|encoder| encoder.bool(*value))
                .map_err(encode_error)?;
        }
        StoredValue::Number(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(2))
                .and_then(|encoder| encoder.i64(*value))
                .map_err(encode_error)?;
        }
        StoredValue::Text(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(3))
                .and_then(|encoder| encoder.str(value))
                .map_err(encode_error)?;
        }
        StoredValue::Bytes(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(4))
                .and_then(|encoder| encoder.bytes(value))
                .map_err(encode_error)?;
        }
        StoredValue::List(values) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(5))
                .and_then(|encoder| encoder.array(values.len() as u64))
                .map_err(encode_error)?;
            for value in values {
                encode_component_value(encoder, value, depth + 1, blobs, references)?;
            }
        }
        StoredValue::Record(fields) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(6))
                .and_then(|encoder| encoder.map(fields.len() as u64))
                .map_err(encode_error)?;
            for (name, value) in fields {
                encoder.str(name).map_err(encode_error)?;
                encode_component_value(encoder, value, depth + 1, blobs, references)?;
            }
        }
        StoredValue::Variant { tag, fields } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(7))
                .and_then(|encoder| encoder.str(tag))
                .and_then(|encoder| encoder.map(fields.len() as u64))
                .map_err(encode_error)?;
            for (name, value) in fields {
                encoder.str(name).map_err(encode_error)?;
                encode_component_value(encoder, value, depth + 1, blobs, references)?;
            }
        }
        StoredValue::Error { code, fields } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(8))
                .and_then(|encoder| encoder.str(code))
                .and_then(|encoder| encoder.map(fields.len() as u64))
                .map_err(encode_error)?;
            for (name, value) in fields {
                encoder.str(name).map_err(encode_error)?;
                encode_component_value(encoder, value, depth + 1, blobs, references)?;
            }
        }
    }
    Ok(())
}

fn decode_component_value(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    depth: usize,
    blobs: Option<&BTreeMap<BlobDigest, BlobRecord>>,
    references: &mut BTreeMap<BlobDigest, u64>,
) -> Result<StoredValue, CodecError> {
    if depth > limits.max_value_depth {
        return Err(CodecError::new("stored value nesting exceeds decode limit"));
    }
    let len = definite_len(decoder.array().map_err(decode_error)?, "stored value")?;
    let tag = decoder.u8().map_err(decode_error)?;
    match (tag, len) {
        (0, 1) => Ok(StoredValue::Null),
        (1, 2) => Ok(StoredValue::Bool(decoder.bool().map_err(decode_error)?)),
        (2, 2) => Ok(StoredValue::Number(decoder.i64().map_err(decode_error)?)),
        (3, 2) => Ok(StoredValue::Text(decode_text(decoder, limits)?)),
        (4, 2) => {
            let bytes = decoder.bytes().map_err(decode_error)?;
            if bytes.len() > limits.max_blob_bytes {
                return Err(CodecError::new("stored byte value exceeds decode limit"));
            }
            Ok(StoredValue::Bytes(bytes.to_vec()))
        }
        (5, 2) => {
            let count = collection_len(decoder, limits, "stored value list", true)?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(decode_component_value(
                    decoder,
                    limits,
                    depth + 1,
                    blobs,
                    references,
                )?);
            }
            Ok(StoredValue::List(values))
        }
        (6, 2) => Ok(StoredValue::Record(decode_component_fields(
            decoder,
            limits,
            depth + 1,
            blobs,
            references,
        )?)),
        (7, 3) => Ok(StoredValue::Variant {
            tag: decode_text(decoder, limits)?,
            fields: decode_component_fields(decoder, limits, depth + 1, blobs, references)?,
        }),
        (8, 3) => Ok(StoredValue::Error {
            code: decode_text(decoder, limits)?,
            fields: decode_component_fields(decoder, limits, depth + 1, blobs, references)?,
        }),
        (9, 3) => {
            let digest = BlobDigest(decode_digest(decoder)?);
            let length = decoder.u64().map_err(decode_error)?;
            let count = references.entry(digest).or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| CodecError::new("blob reference count overflow"))?;
            let Some(blobs) = blobs else {
                return Ok(StoredValue::Bytes(Vec::new()));
            };
            let record = blobs
                .get(&digest)
                .ok_or_else(|| CodecError::new("stored value references a missing blob"))?;
            if record.length != length {
                return Err(CodecError::new(
                    "stored value blob length does not match blob metadata",
                ));
            }
            Ok(StoredValue::Bytes(record.bytes.clone()))
        }
        _ => Err(CodecError::new(format!(
            "unknown stored value tag {tag} with array length {len}"
        ))),
    }
}

fn decode_component_fields(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    depth: usize,
    blobs: Option<&BTreeMap<BlobDigest, BlobRecord>>,
    references: &mut BTreeMap<BlobDigest, u64>,
) -> Result<BTreeMap<String, StoredValue>, CodecError> {
    let count = collection_len(decoder, limits, "stored value fields", false)?;
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let name = decode_text(decoder, limits)?;
        let value = decode_component_value(decoder, limits, depth, blobs, references)?;
        if fields.insert(name, value).is_some() {
            return Err(CodecError::new("stored value repeats a record field"));
        }
    }
    Ok(fields)
}

fn encode_value(
    encoder: &mut CborEncoder<'_>,
    value: &StoredValue,
    depth: usize,
) -> Result<(), CodecError> {
    if depth > 64 {
        return Err(CodecError::new(
            "stored value nesting exceeds encoder limit",
        ));
    }
    match value {
        StoredValue::Null => {
            encoder
                .array(1)
                .and_then(|encoder| encoder.u8(0))
                .map_err(encode_error)?;
        }
        StoredValue::Bool(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(1))
                .and_then(|encoder| encoder.bool(*value))
                .map_err(encode_error)?;
        }
        StoredValue::Number(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(2))
                .and_then(|encoder| encoder.i64(*value))
                .map_err(encode_error)?;
        }
        StoredValue::Text(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(3))
                .and_then(|encoder| encoder.str(value))
                .map_err(encode_error)?;
        }
        StoredValue::Bytes(value) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(4))
                .and_then(|encoder| encoder.bytes(value))
                .map_err(encode_error)?;
        }
        StoredValue::List(values) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(5))
                .and_then(|encoder| encoder.array(values.len() as u64))
                .map_err(encode_error)?;
            for value in values {
                encode_value(encoder, value, depth + 1)?;
            }
        }
        StoredValue::Record(fields) => {
            encoder
                .array(2)
                .and_then(|encoder| encoder.u8(6))
                .map_err(encode_error)?;
            encode_value_fields(encoder, fields, depth + 1)?;
        }
        StoredValue::Variant { tag, fields } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(7))
                .and_then(|encoder| encoder.str(tag))
                .map_err(encode_error)?;
            encode_value_fields(encoder, fields, depth + 1)?;
        }
        StoredValue::Error { code, fields } => {
            encoder
                .array(3)
                .and_then(|encoder| encoder.u8(8))
                .and_then(|encoder| encoder.str(code))
                .map_err(encode_error)?;
            encode_value_fields(encoder, fields, depth + 1)?;
        }
    }
    Ok(())
}

fn decode_value(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    depth: usize,
) -> Result<StoredValue, CodecError> {
    if depth > limits.max_value_depth {
        return Err(CodecError::new("stored value nesting exceeds decode limit"));
    }
    let len = definite_len(decoder.array().map_err(decode_error)?, "stored value")?;
    let tag = decoder.u8().map_err(decode_error)?;
    match (tag, len) {
        (0, 1) => Ok(StoredValue::Null),
        (1, 2) => Ok(StoredValue::Bool(decoder.bool().map_err(decode_error)?)),
        (2, 2) => Ok(StoredValue::Number(decoder.i64().map_err(decode_error)?)),
        (3, 2) => Ok(StoredValue::Text(decode_text(decoder, limits)?)),
        (4, 2) => {
            let bytes = decoder.bytes().map_err(decode_error)?;
            if bytes.len() > limits.max_blob_bytes {
                return Err(CodecError::new("stored byte value exceeds decode limit"));
            }
            Ok(StoredValue::Bytes(bytes.to_vec()))
        }
        (5, 2) => {
            let count = collection_len(decoder, limits, "stored value list", true)?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(decode_value(decoder, limits, depth + 1)?);
            }
            Ok(StoredValue::List(values))
        }
        (6, 2) => Ok(StoredValue::Record(decode_value_fields(
            decoder,
            limits,
            depth + 1,
        )?)),
        (7, 3) => Ok(StoredValue::Variant {
            tag: decode_text(decoder, limits)?,
            fields: decode_value_fields(decoder, limits, depth + 1)?,
        }),
        (8, 3) => Ok(StoredValue::Error {
            code: decode_text(decoder, limits)?,
            fields: decode_value_fields(decoder, limits, depth + 1)?,
        }),
        _ => Err(CodecError::new(format!(
            "unknown stored value tag {tag} with array length {len}"
        ))),
    }
}

fn encode_value_fields(
    encoder: &mut CborEncoder<'_>,
    fields: &BTreeMap<String, StoredValue>,
    depth: usize,
) -> Result<(), CodecError> {
    encoder.map(fields.len() as u64).map_err(encode_error)?;
    for (name, value) in fields {
        encoder.str(name).map_err(encode_error)?;
        encode_value(encoder, value, depth)?;
    }
    Ok(())
}

fn decode_value_fields(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    depth: usize,
) -> Result<BTreeMap<String, StoredValue>, CodecError> {
    let count = collection_len(decoder, limits, "stored value fields", false)?;
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let name = decode_text(decoder, limits)?;
        let value = decode_value(decoder, limits, depth)?;
        if fields.insert(name, value).is_some() {
            return Err(CodecError::new("stored value repeats a record field"));
        }
    }
    Ok(fields)
}

fn encode_digest(encoder: &mut CborEncoder<'_>, digest: &[u8; 32]) -> Result<(), CodecError> {
    encoder.bytes(digest).map_err(encode_error)?;
    Ok(())
}

fn decode_digest(decoder: &mut Decoder<'_>) -> Result<[u8; 32], CodecError> {
    decoder
        .bytes()
        .map_err(decode_error)?
        .try_into()
        .map_err(|_| CodecError::new("digest must contain exactly 32 bytes"))
}

fn decode_text(decoder: &mut Decoder<'_>, limits: DecodeLimits) -> Result<String, CodecError> {
    let value = decoder.str().map_err(decode_error)?;
    if value.len() > limits.max_text_bytes {
        return Err(CodecError::new("text exceeds decode limit"));
    }
    Ok(value.to_owned())
}

fn expect_array(decoder: &mut Decoder<'_>, expected: usize, label: &str) -> Result<(), CodecError> {
    let actual = definite_len(decoder.array().map_err(decode_error)?, label)?;
    if actual != expected {
        return Err(CodecError::new(format!(
            "{label} has {actual} fields, expected {expected}"
        )));
    }
    Ok(())
}

fn collection_len(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    label: &str,
    array: bool,
) -> Result<usize, CodecError> {
    let length = if array {
        decoder.array().map_err(decode_error)?
    } else {
        decoder.map().map_err(decode_error)?
    };
    let length = definite_len(length, label)?;
    if length > limits.max_collection_items {
        return Err(CodecError::new(format!(
            "{label} exceeds collection item limit"
        )));
    }
    Ok(length)
}

fn definite_len(length: Option<u64>, label: &str) -> Result<usize, CodecError> {
    let length =
        length.ok_or_else(|| CodecError::new(format!("{label} must use definite length")))?;
    usize::try_from(length).map_err(|_| CodecError::new(format!("{label} length overflows usize")))
}

fn encode_error<E: fmt::Debug>(error: E) -> CodecError {
    CodecError::new(format!("CBOR encode failed: {error:?}"))
}

fn decode_error<E: fmt::Display>(error: E) -> CodecError {
    CodecError::new(format!("CBOR decode failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{MemoryKind, MemoryOwnerPath};

    fn memory(name: &str) -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "codec".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            name,
            MemoryKind::Scalar,
        )
        .unwrap()
    }

    fn outbox_item(turn_sequence: u64) -> DurableOutboxItem {
        let effect = EffectId::from_host_operation("Test/send").unwrap();
        DurableOutboxItem::pending(
            EffectInvocationId::from_semantic_route(effect, "test.send", "store.result").unwrap(),
            effect,
            StoredValue::Text("stable-key".to_owned()),
            StoredValue::Number(42),
            None,
            turn_sequence,
        )
    }

    #[test]
    fn restore_image_round_trips_canonical_cbor() {
        let mut image = RestoreImage::empty(
            ApplicationIdentity::new("dev.boon.codec", "test", "local"),
            3,
            [7; 32],
        );
        image.epoch = 4;
        image.through_turn_sequence = 9;
        image.scalars.insert(
            memory("value"),
            StoredScalar {
                touched: true,
                value: StoredValue::Record(BTreeMap::from([
                    ("a".to_owned(), StoredValue::Number(3)),
                    ("b".to_owned(), StoredValue::Text("text".to_owned())),
                ])),
            },
        );
        let item = outbox_item(9);
        image.outbox.insert(item.item_id, item);
        let bytes = encode_restore_image(&image).unwrap();
        assert_eq!(
            decode_restore_image(&bytes, DecodeLimits::default()).unwrap(),
            image
        );
        assert_eq!(encode_restore_image(&image).unwrap(), bytes);
    }

    #[test]
    fn application_transfer_round_trips_authority_and_artifacts_canonically() {
        let mut restore_image = RestoreImage::empty(
            ApplicationIdentity::new("dev.boon.codec-transfer", "test", "local"),
            2,
            [4; 32],
        );
        restore_image.epoch = 3;
        restore_image.through_turn_sequence = 7;
        restore_image.scalars.insert(
            memory("artifact_id"),
            StoredScalar {
                touched: true,
                value: StoredValue::Text("published".to_owned()),
            },
        );
        let first = ContentArtifact::new(
            "application/vnd.boon.program",
            b"first immutable program".to_vec(),
        )
        .unwrap();
        let second = ContentArtifact::new(
            "application/vnd.boon.program",
            b"second immutable program".to_vec(),
        )
        .unwrap();
        let transfer = ApplicationTransfer {
            restore_image,
            content_artifacts: BTreeMap::from([
                (first.id, first.clone()),
                (second.id, second.clone()),
            ]),
        };
        let bytes = encode_application_transfer(&transfer).unwrap();
        assert_eq!(
            decode_application_transfer(&bytes, DecodeLimits::default()).unwrap(),
            transfer
        );
        assert_eq!(encode_application_transfer(&transfer).unwrap(), bytes);

        let mut corrupt = bytes;
        let payload = corrupt
            .windows(first.bytes.len())
            .position(|window| window == first.bytes)
            .expect("encoded transfer contains first artifact payload");
        corrupt[payload] ^= 1;
        assert!(decode_application_transfer(&corrupt, DecodeLimits::default()).is_err());
    }

    #[test]
    fn decode_rejects_trailing_bytes_and_oversize_input() {
        let image = RestoreImage::empty(
            ApplicationIdentity::new("dev.boon.codec", "test", "local"),
            1,
            [1; 32],
        );
        let mut bytes = encode_restore_image(&image).unwrap();
        bytes.push(0);
        assert!(decode_restore_image(&bytes, DecodeLimits::default()).is_err());
        assert!(
            decode_restore_image(
                &bytes,
                DecodeLimits {
                    max_total_bytes: 1,
                    ..DecodeLimits::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn checkpoint_batch_round_trips_outbox_transitions_canonically() {
        let item = outbox_item(1);
        let batch = CheckpointBatch {
            application: ApplicationIdentity::new("dev.boon.codec", "test", "local"),
            schema_hash: [3; 32],
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: vec![DurableChange::SetScalar {
                memory_id: memory("value"),
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Number(1),
                },
            }],
            outbox_changes: vec![DurableOutboxChange::Enqueue { item }],
            checksum: [0; 32],
        }
        .seal();
        let bytes = encode_checkpoint_batch(&batch).unwrap();
        assert_eq!(
            decode_checkpoint_batch(&bytes, DecodeLimits::default()).unwrap(),
            batch
        );
        assert_eq!(encode_checkpoint_batch(&batch).unwrap(), bytes);
    }

    #[test]
    fn large_bytes_externalize_and_blob_records_validate_content() {
        let inline = StoredScalar {
            touched: true,
            value: StoredValue::Bytes(vec![1; INLINE_BYTES_THRESHOLD]),
        };
        assert!(encode_scalar_component(&inline).unwrap().blobs.is_empty());

        let payload = vec![7; INLINE_BYTES_THRESHOLD + 1];
        let scalar = StoredScalar {
            touched: true,
            value: StoredValue::Record(BTreeMap::from([
                ("first".to_owned(), StoredValue::Bytes(payload.clone())),
                ("second".to_owned(), StoredValue::Bytes(payload.clone())),
            ])),
        };
        let encoded = encode_scalar_component(&scalar).unwrap();
        assert_eq!(encoded.blobs.len(), 1);
        let record = encoded.blobs.values().next().unwrap();
        assert_eq!(record.reference_count, 2);
        assert_eq!(
            decode_scalar_component(&encoded.bytes, DecodeLimits::default(), &encoded.blobs)
                .unwrap(),
            scalar
        );
        assert!(
            decode_scalar_component(&encoded.bytes, DecodeLimits::default(), &BTreeMap::new())
                .is_err()
        );

        let mut corrupt_bytes = encode_blob_record(record).unwrap();
        *corrupt_bytes.last_mut().unwrap() ^= 1;
        assert!(decode_blob_record(&corrupt_bytes, DecodeLimits::default()).is_err());
    }
}
