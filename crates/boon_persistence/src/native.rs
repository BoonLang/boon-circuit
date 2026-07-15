use super::codec::{
    BlobDigest, BlobRecord, EncodedComponent, decode_blob_record, decode_outbox_record,
    decode_row_component, decode_scalar_component, encode_blob_record, encode_outbox_record,
    encode_row_component, encode_scalar_component, row_component_blob_references,
    scalar_component_blob_references,
};
use super::{
    ActivationAck, ActivationBatch, ApplicationTransfer, BarrierAck, CheckpointBatch, CommitAck,
    CompactAck, ContentArtifact, ContentArtifactBinding, ContentArtifactId,
    ContentArtifactManifest, ContentArtifactOwnerId, ContentArtifactRetention, DecodeLimits,
    DurableChange, DurableOutboxItem, ExportApplicationRequest, LoadContentArtifactRequest,
    OutboxItemId, PersistenceCommand, PersistenceDriver, PersistenceInspectorSnapshot,
    PersistenceResult, PutContentArtifactAck, PutContentArtifactRequest, ResetApplicationAck,
    ResetApplicationBatch, RestoreImage, ShutdownAck, StoreError, StoredList, StoredRow,
    apply_durable_content_artifact_changes, encode_restore_image, exact_content_artifact_closure,
    hash_application, inspector_snapshot_with_artifacts, validate_activation,
    validate_application_transfer, validate_checkpoint, validate_content_artifact,
    validate_content_artifact_manifest, validate_content_artifact_storage, validate_initial_image,
    validate_list,
};
use boon_plan::{ApplicationIdentity, MemoryId, MigrationEdgeId};
use minicbor::{Decoder, Encoder};
use redb::{
    Database, Durability, ReadOnlyTable, ReadableDatabase, ReadableTable, Table, TableDefinition,
    WriteTransaction,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

const META: TableDefinition<&[u8], &[u8]> = TableDefinition::new("META");
const SLOTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("SLOTS");
const LISTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("LISTS");
const ROWS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("ROWS");
const CHECKPOINTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("CHECKPOINTS");
const MIGRATIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("MIGRATIONS");
const OUTBOX: TableDefinition<&[u8], &[u8]> = TableDefinition::new("OUTBOX");
const BLOBS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("BLOBS");
const ARTIFACTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("ARTIFACTS");
const ARTIFACT_OWNERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("ARTIFACT_OWNERS");

const COMPONENT_FORMAT: u32 = 1;
const MAX_CHECKPOINT_RECORDS_PER_APPLICATION: usize = 64;

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

pub struct RedbDriver {
    database: Option<Database>,
    limits: DecodeLimits,
}

impl RedbDriver {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let database = Database::create(path).map_err(backend)?;
        let mut transaction = database.begin_write().map_err(backend)?;
        transaction
            .set_durability(Durability::Immediate)
            .map_err(backend)?;
        create_tables(&transaction)?;
        transaction.commit().map_err(backend)?;
        Ok(Self {
            database: Some(database),
            limits: DecodeLimits::default(),
        })
    }

    pub fn with_decode_limits(mut self, limits: DecodeLimits) -> Self {
        self.limits = limits;
        self
    }

    fn database(&self) -> Result<&Database, StoreError> {
        self.database.as_ref().ok_or(StoreError::Closed)
    }

    fn load_image(
        &self,
        application: &ApplicationIdentity,
    ) -> Result<Option<RestoreImage>, StoreError> {
        let transaction = self.database()?.begin_read().map_err(backend)?;
        let meta_table = transaction.open_table(META).map_err(backend)?;
        let Some(meta) = read_meta(&meta_table, application, self.limits)? else {
            return Ok(None);
        };
        drop(meta_table);

        let app = application_key(application);
        let mut decoded_bytes = 0usize;
        let blobs = load_blobs_read(
            &transaction.open_table(BLOBS).map_err(backend)?,
            &app,
            self.limits,
            &mut decoded_bytes,
        )?;
        let mut actual_blob_references = BTreeMap::new();
        let mut scalars = BTreeMap::new();
        {
            let table = transaction.open_table(SLOTS).map_err(backend)?;
            for entry in table.iter().map_err(backend)? {
                let (key, value) = entry.map_err(backend)?;
                let key = key.value();
                if !key.starts_with(&app) {
                    continue;
                }
                let memory = memory_from_key(key, "slot")?;
                let bytes = value.value();
                add_decode_bytes(&mut decoded_bytes, bytes.len(), self.limits)?;
                merge_blob_references(
                    &mut actual_blob_references,
                    &scalar_component_blob_references(bytes, self.limits).map_err(codec)?,
                )?;
                let scalar = decode_scalar_component(bytes, self.limits, &blobs).map_err(codec)?;
                if scalars.insert(memory, scalar).is_some() {
                    return Err(corrupt("duplicate scalar memory key"));
                }
            }
        }

        let mut lists = BTreeMap::new();
        let mut expected_rows = BTreeSet::new();
        {
            let list_table = transaction.open_table(LISTS).map_err(backend)?;
            let row_table = transaction.open_table(ROWS).map_err(backend)?;
            for entry in list_table.iter().map_err(backend)? {
                let (key, value) = entry.map_err(backend)?;
                let key = key.value();
                if !key.starts_with(&app) {
                    continue;
                }
                let memory = memory_from_key(key, "list")?;
                if scalars.contains_key(&memory) {
                    return Err(corrupt("one memory key is both a scalar and a list"));
                }
                let bytes = value.value();
                add_decode_bytes(&mut decoded_bytes, bytes.len(), self.limits)?;
                let record = decode_list_record(bytes, self.limits)?;
                let mut rows = Vec::with_capacity(record.rows.len());
                for row_ref in &record.rows {
                    let row_key = row_storage_key(&app, memory, *row_ref);
                    if !expected_rows.insert(row_key.to_vec()) {
                        return Err(corrupt("list repeats a row identity"));
                    }
                    let bytes = row_table
                        .get(row_key.as_slice())
                        .map_err(backend)?
                        .map(|value| value.value().to_vec())
                        .ok_or_else(|| corrupt("list order references a missing row"))?;
                    add_decode_bytes(&mut decoded_bytes, bytes.len(), self.limits)?;
                    merge_blob_references(
                        &mut actual_blob_references,
                        &row_component_blob_references(&bytes, self.limits).map_err(codec)?,
                    )?;
                    let row = decode_row_component(&bytes, self.limits, &blobs).map_err(codec)?;
                    if row.key != row_ref.key || row.generation != row_ref.generation {
                        return Err(corrupt("row payload identity does not match its key"));
                    }
                    rows.push(row);
                }
                let list = StoredList {
                    touched: record.touched,
                    next_key: record.next_key,
                    rows,
                };
                validate_list(&list)?;
                if lists.insert(memory, list).is_some() {
                    return Err(corrupt("duplicate list memory key"));
                }
            }

            let mut actual_rows = BTreeSet::new();
            for entry in row_table.iter().map_err(backend)? {
                let (key, _) = entry.map_err(backend)?;
                let key = key.value();
                if key.starts_with(&app) {
                    validate_row_key(key)?;
                    actual_rows.insert(key.to_vec());
                }
            }
            if actual_rows != expected_rows {
                return Err(corrupt("row table contains unreferenced authority"));
            }
        }

        let mut completed_migration_edges = BTreeSet::new();
        {
            let table = transaction.open_table(MIGRATIONS).map_err(backend)?;
            for entry in table.iter().map_err(backend)? {
                let (key, value) = entry.map_err(backend)?;
                let key = key.value();
                if !key.starts_with(&app) {
                    continue;
                }
                if key.len() != 64 || value.value().len() != 8 {
                    return Err(corrupt("invalid migration record"));
                }
                let mut digest = [0; 32];
                digest.copy_from_slice(&key[32..64]);
                completed_migration_edges.insert(MigrationEdgeId(digest));
            }
        }

        let outbox = load_outbox_table(
            &transaction.open_table(OUTBOX).map_err(backend)?,
            &app,
            self.limits,
            &mut decoded_bytes,
        )?;
        let content_artifact_manifest = load_content_artifact_manifest(
            &transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?,
            &app,
            self.limits,
            &mut decoded_bytes,
        )?;
        let content_artifacts = load_content_artifacts(
            &transaction.open_table(ARTIFACTS).map_err(backend)?,
            &app,
            self.limits,
        )?;
        validate_content_artifact_storage(&content_artifact_manifest, &content_artifacts)?;
        validate_blob_reference_counts(&blobs, &actual_blob_references)?;

        Ok(Some(RestoreImage {
            application: meta.application,
            schema_version: meta.schema_version,
            schema_hash: meta.schema_hash,
            epoch: meta.epoch,
            through_turn_sequence: meta.through_turn_sequence,
            scalars,
            lists,
            completed_migration_edges,
            outbox,
            content_artifact_manifest,
        }))
    }

    fn initialize(&self, image: RestoreImage) -> Result<CommitAck, StoreError> {
        validate_initial_image(&image)?;
        if image
            .scalars
            .keys()
            .any(|memory| image.lists.contains_key(memory))
        {
            return Err(StoreError::InvalidAuthority(
                "one memory ID cannot be both a scalar and a list".to_owned(),
            ));
        }
        let initialization_checksum = restore_checksum(&image)?;
        let app = application_key(&image.application);
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;

        {
            let mut meta_table = transaction.open_table(META).map_err(backend)?;
            if let Some(existing) = read_meta_write(&meta_table, &image.application, self.limits)? {
                if existing.schema_version == image.schema_version
                    && existing.schema_hash == image.schema_hash
                    && existing.epoch == image.epoch
                    && existing.through_turn_sequence == image.through_turn_sequence
                    && existing.initialization_checksum == initialization_checksum
                {
                    return Ok(CommitAck {
                        epoch: existing.epoch,
                        through_turn_sequence: existing.through_turn_sequence,
                    });
                }
                return Err(StoreError::IdentityMismatch);
            }

            let mut slots = transaction.open_table(SLOTS).map_err(backend)?;
            let mut lists = transaction.open_table(LISTS).map_err(backend)?;
            let mut rows = transaction.open_table(ROWS).map_err(backend)?;
            let mut migrations = transaction.open_table(MIGRATIONS).map_err(backend)?;
            let mut outbox = transaction.open_table(OUTBOX).map_err(backend)?;
            let mut blob_table = transaction.open_table(BLOBS).map_err(backend)?;
            let current_blobs = BTreeMap::new();
            let mut candidate_blobs = BTreeMap::new();

            for (memory, scalar) in &image.scalars {
                save_scalar(
                    &mut slots,
                    &app,
                    *memory,
                    scalar,
                    self.limits,
                    &mut candidate_blobs,
                )?;
            }
            for (memory, list) in &image.lists {
                replace_list(
                    &mut lists,
                    &mut rows,
                    &app,
                    *memory,
                    list,
                    self.limits,
                    &mut candidate_blobs,
                )?;
            }
            for edge in &image.completed_migration_edges {
                let key = migration_storage_key(&app, *edge);
                migrations
                    .insert(key.as_slice(), image.epoch.to_be_bytes().as_slice())
                    .map_err(backend)?;
            }
            replace_outbox(&mut outbox, &app, &BTreeMap::new(), &image.outbox)?;
            sync_blobs(&mut blob_table, &app, &current_blobs, &candidate_blobs)?;

            let meta = MetaRecord {
                application: image.application.clone(),
                schema_version: image.schema_version,
                schema_hash: image.schema_hash,
                epoch: image.epoch,
                through_turn_sequence: image.through_turn_sequence,
                clean_shutdown: false,
                initialization_checksum,
            };
            let bytes = encode_meta(&meta)?;
            meta_table
                .insert(app.as_slice(), bytes.as_slice())
                .map_err(backend)?;
        }

        transaction.commit().map_err(backend)?;
        Ok(CommitAck {
            epoch: image.epoch,
            through_turn_sequence: image.through_turn_sequence,
        })
    }

    fn commit(&self, batch: CheckpointBatch) -> Result<CommitAck, StoreError> {
        validate_checkpoint(&batch)?;
        let app = application_key(&batch.application);
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;

        let ack;
        {
            let mut meta_table = transaction.open_table(META).map_err(backend)?;
            let mut meta = read_meta_write(&meta_table, &batch.application, self.limits)?
                .ok_or(StoreError::MissingApplication)?;
            validate_checkpoint_header(&meta, &batch)?;

            let mut outbox = transaction.open_table(OUTBOX).map_err(backend)?;
            let mut decoded_bytes = 0;
            let current_outbox = load_outbox_table(&outbox, &app, self.limits, &mut decoded_bytes)?;
            let mut candidate_outbox = current_outbox.clone();
            super::apply_durable_outbox_changes(&mut candidate_outbox, &batch.outbox_changes)?;
            super::validate_outbox(&candidate_outbox)?;

            let mut artifact_owners = transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?;
            let mut owner_decode_bytes = 0;
            let current_manifest = load_content_artifact_manifest(
                &artifact_owners,
                &app,
                self.limits,
                &mut owner_decode_bytes,
            )?;
            let mut candidate_manifest = current_manifest.clone();
            apply_durable_content_artifact_changes(
                &mut candidate_manifest,
                &batch.content_artifact_changes,
            )?;
            let artifacts = transaction.open_table(ARTIFACTS).map_err(backend)?;
            let available_artifacts = load_content_artifacts(&artifacts, &app, self.limits)?;
            validate_content_artifact_storage(&candidate_manifest, &available_artifacts)?;
            drop(artifacts);
            replace_content_artifact_manifest(
                &mut artifact_owners,
                &app,
                &current_manifest,
                &candidate_manifest,
            )?;
            drop(artifact_owners);

            let mut slots = transaction.open_table(SLOTS).map_err(backend)?;
            let mut lists = transaction.open_table(LISTS).map_err(backend)?;
            let mut rows = transaction.open_table(ROWS).map_err(backend)?;
            let mut blob_table = transaction.open_table(BLOBS).map_err(backend)?;
            let current_blobs = load_blobs_write(&blob_table, &app, self.limits)?;
            let mut candidate_blobs = current_blobs.clone();
            apply_changes(
                &mut slots,
                &mut lists,
                &mut rows,
                &app,
                &batch.changes,
                self.limits,
                &mut candidate_blobs,
            )?;
            sync_blobs(&mut blob_table, &app, &current_blobs, &candidate_blobs)?;
            replace_outbox(&mut outbox, &app, &current_outbox, &candidate_outbox)?;

            let mut checkpoints = transaction.open_table(CHECKPOINTS).map_err(backend)?;
            record_checkpoint(
                &mut checkpoints,
                &app,
                CheckpointRecord {
                    kind: CheckpointKind::Checkpoint,
                    base_epoch: batch.base_epoch,
                    next_epoch: batch.next_epoch,
                    first_turn_sequence: batch.first_turn_sequence,
                    last_turn_sequence: batch.last_turn_sequence,
                    schema_hash: batch.schema_hash,
                    checksum: batch.checksum,
                },
            )?;

            meta.epoch = batch.next_epoch;
            meta.through_turn_sequence = batch.last_turn_sequence;
            meta.clean_shutdown = false;
            let bytes = encode_meta(&meta)?;
            meta_table
                .insert(app.as_slice(), bytes.as_slice())
                .map_err(backend)?;
            ack = CommitAck {
                epoch: meta.epoch,
                through_turn_sequence: meta.through_turn_sequence,
            };
        }

        transaction.commit().map_err(backend)?;
        Ok(ack)
    }

    fn reset_application(
        &self,
        batch: ResetApplicationBatch,
    ) -> Result<ResetApplicationAck, StoreError> {
        super::validate_reset(&batch)?;
        let app = application_key(&batch.application);
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;

        let ack;
        {
            let mut meta_table = transaction.open_table(META).map_err(backend)?;
            let current = read_meta_write(&meta_table, &batch.application, self.limits)?
                .ok_or(StoreError::MissingApplication)?;
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

            delete_prefix(&mut transaction.open_table(SLOTS).map_err(backend)?, &app)?;
            delete_prefix(&mut transaction.open_table(LISTS).map_err(backend)?, &app)?;
            delete_prefix(&mut transaction.open_table(ROWS).map_err(backend)?, &app)?;
            delete_prefix(
                &mut transaction.open_table(MIGRATIONS).map_err(backend)?,
                &app,
            )?;
            delete_prefix(&mut transaction.open_table(OUTBOX).map_err(backend)?, &app)?;
            delete_prefix(&mut transaction.open_table(BLOBS).map_err(backend)?, &app)?;
            delete_prefix(
                &mut transaction.open_table(ARTIFACTS).map_err(backend)?,
                &app,
            )?;
            delete_prefix(
                &mut transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?,
                &app,
            )?;
            let mut checkpoints = transaction.open_table(CHECKPOINTS).map_err(backend)?;
            delete_prefix(&mut checkpoints, &app)?;
            record_checkpoint(
                &mut checkpoints,
                &app,
                CheckpointRecord {
                    kind: CheckpointKind::Reset,
                    base_epoch: batch.expected_base_epoch,
                    next_epoch: batch.next_epoch,
                    first_turn_sequence: current.through_turn_sequence,
                    last_turn_sequence: current.through_turn_sequence,
                    schema_hash: batch.default_image.schema_hash,
                    checksum: batch.checksum,
                },
            )?;

            let meta = MetaRecord {
                application: batch.application.clone(),
                schema_version: batch.default_image.schema_version,
                schema_hash: batch.default_image.schema_hash,
                epoch: batch.next_epoch,
                through_turn_sequence: current.through_turn_sequence,
                clean_shutdown: false,
                initialization_checksum: restore_checksum(&batch.default_image)?,
            };
            let bytes = encode_meta(&meta)?;
            meta_table
                .insert(app.as_slice(), bytes.as_slice())
                .map_err(backend)?;
            ack = ResetApplicationAck {
                epoch: meta.epoch,
                schema_version: meta.schema_version,
                schema_hash: meta.schema_hash,
                through_turn_sequence: meta.through_turn_sequence,
            };
        }
        transaction.commit().map_err(backend)?;
        Ok(ack)
    }

    fn activate(&self, batch: ActivationBatch) -> Result<ActivationAck, StoreError> {
        validate_activation(&batch)?;
        let app = application_key(&batch.application);
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;

        let ack;
        {
            let mut meta_table = transaction.open_table(META).map_err(backend)?;
            let mut meta = read_meta_write(&meta_table, &batch.application, self.limits)?
                .ok_or(StoreError::MissingApplication)?;
            validate_activation_header(&meta, &batch)?;

            let mut artifacts = transaction.open_table(ARTIFACTS).map_err(backend)?;
            let mut available_artifacts = load_content_artifacts(&artifacts, &app, self.limits)?;
            for (id, artifact) in &batch.content_artifacts {
                match available_artifacts.get(id) {
                    Some(existing) if existing == artifact => {}
                    Some(_) => {
                        return Err(StoreError::InvalidContentArtifact(
                            "content digest collides with different artifact bytes".to_owned(),
                        ));
                    }
                    None => {
                        available_artifacts.insert(*id, artifact.clone());
                    }
                }
            }
            let target_artifacts = exact_content_artifact_closure(
                &batch.target_content_artifact_manifest,
                &available_artifacts,
            )?;
            validate_content_artifact_storage(
                &batch.target_content_artifact_manifest,
                &target_artifacts,
            )?;
            delete_prefix(&mut artifacts, &app)?;
            for (id, artifact) in &target_artifacts {
                let key = content_artifact_storage_key(&app, *id);
                let bytes = encode_content_artifact(artifact)?;
                artifacts
                    .insert(key.as_slice(), bytes.as_slice())
                    .map_err(backend)?;
            }
            drop(artifacts);

            let mut artifact_owners = transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?;
            let mut owner_decode_bytes = 0;
            let current_manifest = load_content_artifact_manifest(
                &artifact_owners,
                &app,
                self.limits,
                &mut owner_decode_bytes,
            )?;
            replace_content_artifact_manifest(
                &mut artifact_owners,
                &app,
                &current_manifest,
                &batch.target_content_artifact_manifest,
            )?;
            drop(artifact_owners);

            let mut slots = transaction.open_table(SLOTS).map_err(backend)?;
            let mut lists = transaction.open_table(LISTS).map_err(backend)?;
            let mut rows = transaction.open_table(ROWS).map_err(backend)?;
            let mut blob_table = transaction.open_table(BLOBS).map_err(backend)?;
            let current_blobs = load_blobs_write(&blob_table, &app, self.limits)?;
            let mut candidate_blobs = current_blobs.clone();
            apply_changes(
                &mut slots,
                &mut lists,
                &mut rows,
                &app,
                &batch.authority_changes,
                self.limits,
                &mut candidate_blobs,
            )?;
            for memory in &batch.deleted_memory {
                delete_memory(
                    &mut slots,
                    &mut lists,
                    &mut rows,
                    &app,
                    *memory,
                    self.limits,
                    &mut candidate_blobs,
                )?;
            }
            sync_blobs(&mut blob_table, &app, &current_blobs, &candidate_blobs)?;

            let mut migrations = transaction.open_table(MIGRATIONS).map_err(backend)?;
            for edge in &batch.completed_migration_edges {
                let key = migration_storage_key(&app, *edge);
                migrations
                    .insert(key.as_slice(), batch.next_epoch.to_be_bytes().as_slice())
                    .map_err(backend)?;
            }

            let mut checkpoints = transaction.open_table(CHECKPOINTS).map_err(backend)?;
            record_checkpoint(
                &mut checkpoints,
                &app,
                CheckpointRecord {
                    kind: CheckpointKind::Activation,
                    base_epoch: batch.expected_base_epoch,
                    next_epoch: batch.next_epoch,
                    first_turn_sequence: meta.through_turn_sequence,
                    last_turn_sequence: batch.through_turn_sequence,
                    schema_hash: batch.target_schema_hash,
                    checksum: batch.checksum,
                },
            )?;

            meta.schema_version = batch.target_schema_version;
            meta.schema_hash = batch.target_schema_hash;
            meta.epoch = batch.next_epoch;
            meta.through_turn_sequence = batch.through_turn_sequence;
            meta.clean_shutdown = false;
            let bytes = encode_meta(&meta)?;
            meta_table
                .insert(app.as_slice(), bytes.as_slice())
                .map_err(backend)?;
            ack = ActivationAck {
                epoch: meta.epoch,
                schema_version: meta.schema_version,
                schema_hash: meta.schema_hash,
                through_turn_sequence: meta.through_turn_sequence,
            };
        }

        transaction.commit().map_err(backend)?;
        Ok(ack)
    }

    fn barrier(
        &self,
        application: &ApplicationIdentity,
        through_epoch: u64,
    ) -> Result<BarrierAck, StoreError> {
        let transaction = self.database()?.begin_read().map_err(backend)?;
        let table = transaction.open_table(META).map_err(backend)?;
        let meta =
            read_meta(&table, application, self.limits)?.ok_or(StoreError::MissingApplication)?;
        (meta.epoch >= through_epoch)
            .then_some(BarrierAck { epoch: meta.epoch })
            .ok_or(StoreError::StaleEpoch)
    }

    fn inspect(
        &self,
        application: &ApplicationIdentity,
    ) -> Result<Option<PersistenceInspectorSnapshot>, StoreError> {
        let transaction = self.database()?.begin_read().map_err(backend)?;
        let meta_table = transaction.open_table(META).map_err(backend)?;
        let Some(meta) = read_meta(&meta_table, application, self.limits)? else {
            return Ok(None);
        };
        let app = application_key(application);
        let (scalar_count, scalar_bytes) =
            prefix_stats(&transaction.open_table(SLOTS).map_err(backend)?, &app)?;
        let (list_count, list_bytes) =
            prefix_stats(&transaction.open_table(LISTS).map_err(backend)?, &app)?;
        let (row_count, row_bytes) =
            prefix_stats(&transaction.open_table(ROWS).map_err(backend)?, &app)?;
        let (completed_migration_count, migration_bytes) =
            prefix_stats(&transaction.open_table(MIGRATIONS).map_err(backend)?, &app)?;
        let (_, meta_bytes) = prefix_stats(&transaction.open_table(META).map_err(backend)?, &app)?;
        let (_, checkpoint_bytes) =
            prefix_stats(&transaction.open_table(CHECKPOINTS).map_err(backend)?, &app)?;
        let (_, outbox_bytes) =
            prefix_stats(&transaction.open_table(OUTBOX).map_err(backend)?, &app)?;
        let (_, blob_bytes) = prefix_stats(&transaction.open_table(BLOBS).map_err(backend)?, &app)?;
        let (_, content_artifact_record_bytes) =
            prefix_stats(&transaction.open_table(ARTIFACTS).map_err(backend)?, &app)?;
        let (_, content_artifact_owner_record_bytes) = prefix_stats(
            &transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?,
            &app,
        )?;
        let encoded_value_bytes = [
            scalar_bytes,
            list_bytes,
            row_bytes,
            migration_bytes,
            meta_bytes,
            checkpoint_bytes,
            outbox_bytes,
            blob_bytes,
            content_artifact_record_bytes,
            content_artifact_owner_record_bytes,
        ]
        .into_iter()
        .try_fold(0u64, u64::checked_add)
        .ok_or_else(|| corrupt("stored byte count overflow"))?;
        let mut decoded_bytes = 0;
        let outbox = load_outbox_table(
            &transaction.open_table(OUTBOX).map_err(backend)?,
            &app,
            self.limits,
            &mut decoded_bytes,
        )?;
        let content_artifact_manifest = load_content_artifact_manifest(
            &transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?,
            &app,
            self.limits,
            &mut decoded_bytes,
        )?;
        let content_artifacts = load_content_artifacts(
            &transaction.open_table(ARTIFACTS).map_err(backend)?,
            &app,
            self.limits,
        )?;
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
        let outbox_snapshot = inspector_snapshot_with_artifacts(&image, artifact_stats);
        Ok(Some(PersistenceInspectorSnapshot {
            application: meta.application,
            schema_version: meta.schema_version,
            schema_hash: meta.schema_hash,
            epoch: meta.epoch,
            through_turn_sequence: meta.through_turn_sequence,
            scalar_count,
            list_count,
            row_count,
            content_artifact_count: outbox_snapshot.content_artifact_count,
            content_artifact_bytes: outbox_snapshot.content_artifact_bytes,
            content_artifact_owner_count: outbox_snapshot.content_artifact_owner_count,
            content_artifact_retained_count: outbox_snapshot.content_artifact_retained_count,
            content_artifact_retained_bytes: outbox_snapshot.content_artifact_retained_bytes,
            content_artifact_orphan_count: outbox_snapshot.content_artifact_orphan_count,
            content_artifact_orphan_bytes: outbox_snapshot.content_artifact_orphan_bytes,
            encoded_value_bytes: Some(encoded_value_bytes),
            completed_migration_count,
            outbox_pending_count: outbox_snapshot.outbox_pending_count,
            outbox_dispatching_count: outbox_snapshot.outbox_dispatching_count,
            outbox_reconciliation_count: outbox_snapshot.outbox_reconciliation_count,
            outbox_completed_count: outbox_snapshot.outbox_completed_count,
            outbox_samples: outbox_snapshot.outbox_samples,
        }))
    }

    fn compact(&mut self, application: &ApplicationIdentity) -> Result<CompactAck, StoreError> {
        let epoch = {
            let transaction = self.database()?.begin_read().map_err(backend)?;
            let table = transaction.open_table(META).map_err(backend)?;
            read_meta(&table, application, self.limits)?
                .ok_or(StoreError::MissingApplication)?
                .epoch
        };
        {
            let app = application_key(application);
            let database = self.database()?;
            let mut transaction = database.begin_write().map_err(backend)?;
            immediate(&mut transaction)?;
            {
                let slots = transaction.open_table(SLOTS).map_err(backend)?;
                let rows = transaction.open_table(ROWS).map_err(backend)?;
                let mut blob_table = transaction.open_table(BLOBS).map_err(backend)?;
                reclaim_blobs(&slots, &rows, &mut blob_table, &app, self.limits)?;
            }
            {
                let artifact_owners = transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?;
                let mut decoded_bytes = 0;
                let manifest = load_content_artifact_manifest(
                    &artifact_owners,
                    &app,
                    self.limits,
                    &mut decoded_bytes,
                )?;
                drop(artifact_owners);
                let mut artifacts = transaction.open_table(ARTIFACTS).map_err(backend)?;
                let available = load_content_artifacts(&artifacts, &app, self.limits)?;
                exact_content_artifact_closure(&manifest, &available)?;
                let reachable = manifest.reachable_artifact_ids();
                for id in available.keys().filter(|id| !reachable.contains(id)) {
                    let key = content_artifact_storage_key(&app, *id);
                    artifacts.remove(key.as_slice()).map_err(backend)?;
                }
            }
            transaction.commit().map_err(backend)?;
        }
        self.database
            .as_mut()
            .ok_or(StoreError::Closed)?
            .compact()
            .map_err(backend)?;
        Ok(CompactAck { epoch })
    }

    fn put_content_artifact(
        &self,
        request: PutContentArtifactRequest,
    ) -> Result<PutContentArtifactAck, StoreError> {
        validate_content_artifact(&request.artifact)?;
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;
        let already_present;
        {
            let meta = transaction.open_table(META).map_err(backend)?;
            if read_meta_write(&meta, &request.application, self.limits)?.is_none() {
                return Err(StoreError::MissingApplication);
            }
            let app = application_key(&request.application);
            let artifact_owners = transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?;
            let mut decoded_bytes = 0;
            let manifest = load_content_artifact_manifest(
                &artifact_owners,
                &app,
                self.limits,
                &mut decoded_bytes,
            )?;
            drop(artifact_owners);
            let key = content_artifact_storage_key(&app, request.artifact.id);
            let mut artifacts = transaction.open_table(ARTIFACTS).map_err(backend)?;
            let mut available = load_content_artifacts(&artifacts, &app, self.limits)?;
            already_present = match available.get(&request.artifact.id) {
                Some(existing) => {
                    if existing != &request.artifact {
                        return Err(StoreError::InvalidContentArtifact(
                            "content digest collides with different artifact bytes".to_owned(),
                        ));
                    }
                    true
                }
                None => {
                    available.insert(request.artifact.id, request.artifact.clone());
                    validate_content_artifact_storage(&manifest, &available)?;
                    let bytes = encode_content_artifact(&request.artifact)?;
                    artifacts
                        .insert(key.as_slice(), bytes.as_slice())
                        .map_err(backend)?;
                    false
                }
            };
            if already_present {
                validate_content_artifact_storage(&manifest, &available)?;
            }
        }
        transaction.commit().map_err(backend)?;
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
        let transaction = self.database()?.begin_read().map_err(backend)?;
        let meta = transaction.open_table(META).map_err(backend)?;
        if read_meta(&meta, &request.application, self.limits)?.is_none() {
            return Err(StoreError::MissingApplication);
        }
        let app = application_key(&request.application);
        let key = content_artifact_storage_key(&app, request.id);
        transaction
            .open_table(ARTIFACTS)
            .map_err(backend)?
            .get(key.as_slice())
            .map_err(backend)?
            .map(|value| decode_content_artifact(request.id, value.value(), self.limits))
            .transpose()
    }

    fn export_application(
        &self,
        request: ExportApplicationRequest,
    ) -> Result<ApplicationTransfer, StoreError> {
        // RedbDriver is owned exclusively by the persistence worker. No write
        // can interleave between these reads within one driver command.
        let restore_image = self
            .load_image(&request.application)?
            .ok_or(StoreError::MissingApplication)?;
        let transaction = self.database()?.begin_read().map_err(backend)?;
        let table = transaction.open_table(ARTIFACTS).map_err(backend)?;
        let app = application_key(&request.application);
        let available = load_content_artifacts(&table, &app, self.limits)?;
        let content_artifacts =
            exact_content_artifact_closure(&restore_image.content_artifact_manifest, &available)?;
        let transfer = ApplicationTransfer {
            restore_image,
            content_artifacts,
        };
        validate_application_transfer(&transfer)?;
        Ok(transfer)
    }

    fn mark_clean_shutdown(&self) -> Result<(), StoreError> {
        let database = self.database()?;
        let mut transaction = database.begin_write().map_err(backend)?;
        immediate(&mut transaction)?;
        {
            let mut table = transaction.open_table(META).map_err(backend)?;
            let records = {
                let mut records = Vec::new();
                for entry in table.iter().map_err(backend)? {
                    let (key, value) = entry.map_err(backend)?;
                    let mut meta = decode_meta(value.value(), self.limits)?;
                    meta.clean_shutdown = true;
                    records.push((key.value().to_vec(), encode_meta(&meta)?));
                }
                records
            };
            for (key, value) in records {
                table
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(backend)?;
            }
        }
        transaction.commit().map_err(backend)
    }
}

fn reclaim_blobs(
    slots: &Table<'_, &[u8], &[u8]>,
    rows: &Table<'_, &[u8], &[u8]>,
    blob_table: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    limits: DecodeLimits,
) -> Result<(), StoreError> {
    let mut actual = BTreeMap::new();
    for entry in slots.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        if key.value().starts_with(app) {
            merge_blob_references(
                &mut actual,
                &scalar_component_blob_references(value.value(), limits).map_err(codec)?,
            )?;
        }
    }
    for entry in rows.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        if key.value().starts_with(app) {
            merge_blob_references(
                &mut actual,
                &row_component_blob_references(value.value(), limits).map_err(codec)?,
            )?;
        }
    }
    let current = load_blobs_write(blob_table, app, limits)?;
    let mut candidate = BTreeMap::new();
    for (digest, reference_count) in actual {
        let mut record = current
            .get(&digest)
            .cloned()
            .ok_or_else(|| corrupt("component references a missing blob during compaction"))?;
        record.reference_count = reference_count;
        candidate.insert(digest, record);
    }
    sync_blobs(blob_table, app, &current, &candidate)
}

impl PersistenceDriver for RedbDriver {
    fn execute(&mut self, command: PersistenceCommand) -> PersistenceResult {
        match command {
            PersistenceCommand::Load(request) => {
                let result = self.load_image(&request.application).and_then(|image| {
                    if let (Some(expected), Some(image)) =
                        (request.expected_schema_hash, image.as_ref())
                        && image.schema_hash != expected
                    {
                        return Err(StoreError::SchemaMismatch);
                    }
                    Ok(image)
                });
                PersistenceResult::Loaded(result)
            }
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
            PersistenceCommand::Barrier(request) => PersistenceResult::BarrierComplete(
                self.barrier(&request.application, request.through_epoch),
            ),
            PersistenceCommand::Inspect(request) => {
                PersistenceResult::Inspected(self.inspect(&request.application))
            }
            PersistenceCommand::Compact(request) => {
                PersistenceResult::Compacted(self.compact(&request.application))
            }
            PersistenceCommand::ExportApplication(request) => {
                PersistenceResult::ApplicationExported(self.export_application(request))
            }
            PersistenceCommand::PutContentArtifact(request) => {
                PersistenceResult::ContentArtifactStored(self.put_content_artifact(request))
            }
            PersistenceCommand::LoadContentArtifact(request) => {
                PersistenceResult::ContentArtifactLoaded(self.load_content_artifact(request))
            }
            PersistenceCommand::Shutdown(_) => {
                let result = self.mark_clean_shutdown();
                self.database.take();
                PersistenceResult::ShutdownComplete(result.map(|()| ShutdownAck))
            }
        }
    }
}

fn create_tables(transaction: &WriteTransaction) -> Result<(), StoreError> {
    transaction.open_table(META).map_err(backend)?;
    transaction.open_table(SLOTS).map_err(backend)?;
    transaction.open_table(LISTS).map_err(backend)?;
    transaction.open_table(ROWS).map_err(backend)?;
    transaction.open_table(CHECKPOINTS).map_err(backend)?;
    transaction.open_table(MIGRATIONS).map_err(backend)?;
    transaction.open_table(OUTBOX).map_err(backend)?;
    transaction.open_table(BLOBS).map_err(backend)?;
    transaction.open_table(ARTIFACTS).map_err(backend)?;
    transaction.open_table(ARTIFACT_OWNERS).map_err(backend)?;
    Ok(())
}

fn load_outbox_table<T>(
    table: &T,
    app: &[u8; 32],
    limits: DecodeLimits,
    decoded_bytes: &mut usize,
) -> Result<BTreeMap<OutboxItemId, DurableOutboxItem>, StoreError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let mut outbox = BTreeMap::new();
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        let key = key.value();
        if !key.starts_with(app) {
            continue;
        }
        if key.len() != 64 {
            return Err(corrupt("invalid outbox key"));
        }
        let mut digest = [0; 32];
        digest.copy_from_slice(&key[32..]);
        let item_id = OutboxItemId(digest);
        let bytes = value.value();
        add_decode_bytes(decoded_bytes, bytes.len(), limits)?;
        let item = decode_outbox_record(bytes, limits).map_err(codec)?;
        if item.item_id != item_id {
            return Err(corrupt("outbox payload identity does not match its key"));
        }
        if outbox.insert(item_id, item).is_some() {
            return Err(corrupt("duplicate outbox item key"));
        }
    }
    super::validate_outbox(&outbox)?;
    Ok(outbox)
}

fn replace_outbox(
    table: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    current: &BTreeMap<OutboxItemId, DurableOutboxItem>,
    candidate: &BTreeMap<OutboxItemId, DurableOutboxItem>,
) -> Result<(), StoreError> {
    for item_id in current.keys() {
        if !candidate.contains_key(item_id) {
            let key = outbox_storage_key(app, *item_id);
            table.remove(key.as_slice()).map_err(backend)?;
        }
    }
    for (item_id, item) in candidate {
        if current.get(item_id) == Some(item) {
            continue;
        }
        let key = outbox_storage_key(app, *item_id);
        let bytes = encode_outbox_record(item).map_err(codec)?;
        table
            .insert(key.as_slice(), bytes.as_slice())
            .map_err(backend)?;
    }
    Ok(())
}

fn load_content_artifact_manifest<T>(
    table: &T,
    app: &[u8; 32],
    limits: DecodeLimits,
    decoded_bytes: &mut usize,
) -> Result<ContentArtifactManifest, StoreError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let mut bindings = BTreeMap::new();
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        let key = key.value();
        if !key.starts_with(app) {
            continue;
        }
        if key.len() != 64 {
            return Err(corrupt("invalid content artifact owner key"));
        }
        let mut owner = [0; 32];
        owner.copy_from_slice(&key[32..]);
        let bytes = value.value();
        add_decode_bytes(decoded_bytes, bytes.len(), limits)?;
        if bindings
            .insert(
                ContentArtifactOwnerId(owner),
                decode_content_artifact_binding(bytes, limits)?,
            )
            .is_some()
        {
            return Err(corrupt("duplicate content artifact owner key"));
        }
    }
    let manifest = ContentArtifactManifest { bindings };
    validate_content_artifact_manifest(&manifest)?;
    Ok(manifest)
}

fn replace_content_artifact_manifest(
    table: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    current: &ContentArtifactManifest,
    candidate: &ContentArtifactManifest,
) -> Result<(), StoreError> {
    validate_content_artifact_manifest(candidate)?;
    for owner_id in current.bindings.keys() {
        if !candidate.bindings.contains_key(owner_id) {
            let key = content_artifact_owner_storage_key(app, *owner_id);
            table.remove(key.as_slice()).map_err(backend)?;
        }
    }
    for (owner_id, binding) in &candidate.bindings {
        if current.bindings.get(owner_id) == Some(binding) {
            continue;
        }
        let key = content_artifact_owner_storage_key(app, *owner_id);
        let bytes = encode_content_artifact_binding(*binding)?;
        table
            .insert(key.as_slice(), bytes.as_slice())
            .map_err(backend)?;
    }
    Ok(())
}

fn load_content_artifacts<T>(
    table: &T,
    app: &[u8; 32],
    limits: DecodeLimits,
) -> Result<BTreeMap<ContentArtifactId, ContentArtifact>, StoreError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let mut artifacts = BTreeMap::new();
    let mut total_bytes = 0usize;
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        let key = key.value();
        if !key.starts_with(app) {
            continue;
        }
        if key.len() != 64 {
            return Err(corrupt("invalid content artifact key"));
        }
        let mut digest = [0; 32];
        digest.copy_from_slice(&key[32..]);
        let id = ContentArtifactId(digest);
        let artifact = decode_content_artifact(id, value.value(), limits)?;
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

fn load_blobs_read(
    table: &ReadOnlyTable<&[u8], &[u8]>,
    app: &[u8; 32],
    limits: DecodeLimits,
    decoded_bytes: &mut usize,
) -> Result<BTreeMap<BlobDigest, BlobRecord>, StoreError> {
    let mut blobs = BTreeMap::new();
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        let key = key.value();
        if !key.starts_with(app) {
            continue;
        }
        let digest = blob_digest_from_key(key)?;
        let bytes = value.value();
        add_decode_bytes(decoded_bytes, bytes.len(), limits)?;
        let record = decode_blob_record(bytes, limits).map_err(codec)?;
        if record.digest != digest {
            return Err(corrupt("blob payload digest does not match its key"));
        }
        if blobs.insert(digest, record).is_some() {
            return Err(corrupt("duplicate blob digest key"));
        }
    }
    Ok(blobs)
}

fn load_blobs_write(
    table: &Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    limits: DecodeLimits,
) -> Result<BTreeMap<BlobDigest, BlobRecord>, StoreError> {
    let mut blobs = BTreeMap::new();
    let mut decoded_bytes = 0;
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        let key = key.value();
        if !key.starts_with(app) {
            continue;
        }
        let digest = blob_digest_from_key(key)?;
        let bytes = value.value();
        add_decode_bytes(&mut decoded_bytes, bytes.len(), limits)?;
        let record = decode_blob_record(bytes, limits).map_err(codec)?;
        if record.digest != digest {
            return Err(corrupt("blob payload digest does not match its key"));
        }
        if blobs.insert(digest, record).is_some() {
            return Err(corrupt("duplicate blob digest key"));
        }
    }
    Ok(blobs)
}

fn sync_blobs(
    table: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    current: &BTreeMap<BlobDigest, BlobRecord>,
    candidate: &BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    for digest in current.keys() {
        if !candidate.contains_key(digest) {
            let key = blob_storage_key(app, *digest);
            table.remove(key.as_slice()).map_err(backend)?;
        }
    }
    for (digest, record) in candidate {
        if current.get(digest) == Some(record) {
            continue;
        }
        let key = blob_storage_key(app, *digest);
        let bytes = encode_blob_record(record).map_err(codec)?;
        table
            .insert(key.as_slice(), bytes.as_slice())
            .map_err(backend)?;
    }
    Ok(())
}

fn apply_component_blob_delta(
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
    old_references: &BTreeMap<BlobDigest, u64>,
    encoded: Option<&EncodedComponent>,
) -> Result<(), StoreError> {
    let new_references = encoded.map_or_else(BTreeMap::new, |encoded| encoded.references.clone());
    let digests = old_references
        .keys()
        .chain(new_references.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for digest in digests {
        let old_count = old_references.get(&digest).copied().unwrap_or(0);
        let new_count = new_references.get(&digest).copied().unwrap_or(0);
        if old_count == new_count {
            continue;
        }
        let existing_count = blobs
            .get(&digest)
            .map_or(0, |record| record.reference_count);
        let retained = existing_count.checked_sub(old_count).ok_or_else(|| {
            corrupt("blob reference count is below the component reference count")
        })?;
        let final_count = retained
            .checked_add(new_count)
            .ok_or_else(|| corrupt("blob reference count overflow"))?;
        if final_count == 0 {
            blobs.remove(&digest);
            continue;
        }
        if let Some(record) = blobs.get_mut(&digest) {
            record.reference_count = final_count;
            if let Some(payload) = encoded.and_then(|encoded| encoded.blobs.get(&digest))
                && (record.length != payload.length || record.bytes != payload.bytes)
            {
                return Err(corrupt("blob digest collision"));
            }
        } else {
            let payload = encoded
                .and_then(|encoded| encoded.blobs.get(&digest))
                .ok_or_else(|| corrupt("new component references a missing blob payload"))?;
            let mut record = payload.clone();
            record.reference_count = final_count;
            blobs.insert(digest, record);
        }
    }
    Ok(())
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
            "blob table reference counts do not match scalar and row records",
        ));
    }
    Ok(())
}

fn blob_digest_from_key(key: &[u8]) -> Result<BlobDigest, StoreError> {
    if key.len() != 64 {
        return Err(corrupt("invalid blob key"));
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&key[32..]);
    Ok(BlobDigest(digest))
}

fn delete_prefix(table: &mut Table<'_, &[u8], &[u8]>, prefix: &[u8]) -> Result<(), StoreError> {
    let keys = table
        .iter()
        .map_err(backend)?
        .map(|entry| entry.map(|(key, _)| key.value().to_vec()).map_err(backend))
        .collect::<Result<Vec<_>, _>>()?;
    for key in keys {
        if key.starts_with(prefix) {
            table.remove(key.as_slice()).map_err(backend)?;
        }
    }
    Ok(())
}

fn immediate(transaction: &mut WriteTransaction) -> Result<(), StoreError> {
    transaction
        .set_durability(Durability::Immediate)
        .map_err(backend)
}

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

fn apply_changes(
    slots: &mut Table<'_, &[u8], &[u8]>,
    lists: &mut Table<'_, &[u8], &[u8]>,
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    changes: &[DurableChange],
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    for change in changes {
        match change {
            DurableChange::SetScalar { memory_id, value } => {
                if list_exists(lists, app, *memory_id)? {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a list"
                    )));
                }
                save_scalar(slots, app, *memory_id, value, limits, blobs)?;
            }
            DurableChange::DeleteScalar { memory_id } => {
                delete_scalar(slots, app, *memory_id, limits, blobs)?;
            }
            DurableChange::SetList { memory_id, value } => {
                if slot_exists(slots, app, *memory_id)? {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                validate_list(value)?;
                replace_list(lists, rows, app, *memory_id, value, limits, blobs)?;
            }
            DurableChange::SetRowField {
                memory_id,
                row_key,
                row_generation,
                field_id,
                value,
            } => {
                if slot_exists(slots, app, *memory_id)? {
                    return Err(StoreError::InvalidAuthority(format!(
                        "memory {memory_id} is already a scalar"
                    )));
                }
                let mut list = load_list(lists, app, *memory_id, limits)?.unwrap_or(ListRecord {
                    touched: false,
                    next_key: 0,
                    rows: Vec::new(),
                });
                let row_ref = RowRef {
                    key: *row_key,
                    generation: *row_generation,
                };
                let mut row = if list.rows.contains(&row_ref) {
                    load_row(rows, app, *memory_id, row_ref, limits, blobs)?
                        .ok_or_else(|| corrupt("list references a missing row"))?
                } else if list.touched {
                    return Err(StoreError::InvalidAuthority(format!(
                        "list {memory_id} has no row {row_key}:{row_generation}"
                    )));
                } else {
                    list.rows.push(row_ref);
                    StoredRow {
                        key: *row_key,
                        generation: *row_generation,
                        fields: BTreeMap::new(),
                        touched_fields: BTreeSet::new(),
                    }
                };
                row.fields.insert(*field_id, value.clone());
                row.touched_fields.insert(*field_id);
                validate_row(&row, !list.touched)?;
                save_row(rows, app, *memory_id, &row, limits, blobs)?;
                save_list(lists, app, *memory_id, &list)?;
            }
            DurableChange::InsertRow {
                memory_id,
                index,
                row,
                next_key,
            } => {
                let mut list = load_list(lists, app, *memory_id, limits)?.ok_or_else(|| {
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
                validate_row(row, false)?;
                list.rows.insert(index, row_ref);
                list.next_key = *next_key;
                validate_list_record(&list)?;
                save_row(rows, app, *memory_id, row, limits, blobs)?;
                save_list(lists, app, *memory_id, &list)?;
            }
            DurableChange::RemoveRow {
                memory_id,
                row_key,
                row_generation,
                next_key,
            } => {
                let mut list = load_list(lists, app, *memory_id, limits)?.ok_or_else(|| {
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
                validate_list_record(&list)?;
                delete_row(rows, app, *memory_id, row_ref, limits, blobs)?;
                save_list(lists, app, *memory_id, &list)?;
            }
            DurableChange::DeleteList { memory_id } => {
                delete_list(lists, rows, app, *memory_id, limits, blobs)?;
            }
        }
    }
    Ok(())
}

fn replace_list(
    lists: &mut Table<'_, &[u8], &[u8]>,
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    list: &StoredList,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    validate_list(list)?;
    delete_rows(rows, app, memory, limits, blobs)?;
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
    for row in &list.rows {
        save_row(rows, app, memory, row, limits, blobs)?;
    }
    save_list(lists, app, memory, &record)
}

fn delete_memory(
    slots: &mut Table<'_, &[u8], &[u8]>,
    lists: &mut Table<'_, &[u8], &[u8]>,
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    delete_scalar(slots, app, memory, limits, blobs)?;
    delete_list(lists, rows, app, memory, limits, blobs)
}

fn delete_list(
    lists: &mut Table<'_, &[u8], &[u8]>,
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let key = memory_storage_key(app, memory);
    lists.remove(key.as_slice()).map_err(backend)?;
    delete_rows(rows, app, memory, limits, blobs)
}

fn delete_rows(
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let prefix = memory_storage_key(app, memory);
    let entries = rows
        .iter()
        .map_err(backend)?
        .map(|entry| {
            entry
                .map(|(key, value)| (key.value().to_vec(), value.value().to_vec()))
                .map_err(backend)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (key, bytes) in entries {
        if key.starts_with(&prefix) {
            let references = row_component_blob_references(&bytes, limits).map_err(codec)?;
            apply_component_blob_delta(blobs, &references, None)?;
            rows.remove(key.as_slice()).map_err(backend)?;
        }
    }
    Ok(())
}

fn slot_exists(
    slots: &Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
) -> Result<bool, StoreError> {
    let key = memory_storage_key(app, memory);
    Ok(slots.get(key.as_slice()).map_err(backend)?.is_some())
}

fn list_exists(
    lists: &Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
) -> Result<bool, StoreError> {
    let key = memory_storage_key(app, memory);
    Ok(lists.get(key.as_slice()).map_err(backend)?.is_some())
}

fn load_list(
    lists: &Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    limits: DecodeLimits,
) -> Result<Option<ListRecord>, StoreError> {
    let key = memory_storage_key(app, memory);
    lists
        .get(key.as_slice())
        .map_err(backend)?
        .map(|value| decode_list_record(value.value(), limits))
        .transpose()
}

fn save_list(
    lists: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    list: &ListRecord,
) -> Result<(), StoreError> {
    validate_list_record(list)?;
    let key = memory_storage_key(app, memory);
    let bytes = encode_list_record(list)?;
    lists
        .insert(key.as_slice(), bytes.as_slice())
        .map_err(backend)?;
    Ok(())
}

fn load_row(
    rows: &Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    row_ref: RowRef,
    limits: DecodeLimits,
    blobs: &BTreeMap<BlobDigest, BlobRecord>,
) -> Result<Option<StoredRow>, StoreError> {
    let key = row_storage_key(app, memory, row_ref);
    rows.get(key.as_slice())
        .map_err(backend)?
        .map(|value| decode_row_component(value.value(), limits, blobs).map_err(codec))
        .transpose()
}

fn save_row(
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    row: &StoredRow,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let row_ref = RowRef {
        key: row.key,
        generation: row.generation,
    };
    let key = row_storage_key(app, memory, row_ref);
    let old_references = rows
        .get(key.as_slice())
        .map_err(backend)?
        .map(|value| row_component_blob_references(value.value(), limits).map_err(codec))
        .transpose()?
        .unwrap_or_default();
    let encoded = encode_row_component(row).map_err(codec)?;
    apply_component_blob_delta(blobs, &old_references, Some(&encoded))?;
    rows.insert(key.as_slice(), encoded.bytes.as_slice())
        .map_err(backend)?;
    Ok(())
}

fn delete_row(
    rows: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    row_ref: RowRef,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let key = row_storage_key(app, memory, row_ref);
    if let Some(value) = rows.get(key.as_slice()).map_err(backend)? {
        let references = row_component_blob_references(value.value(), limits).map_err(codec)?;
        apply_component_blob_delta(blobs, &references, None)?;
    }
    rows.remove(key.as_slice()).map_err(backend)?;
    Ok(())
}

fn save_scalar(
    slots: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    scalar: &super::StoredScalar,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let key = memory_storage_key(app, memory);
    let old_references = slots
        .get(key.as_slice())
        .map_err(backend)?
        .map(|value| scalar_component_blob_references(value.value(), limits).map_err(codec))
        .transpose()?
        .unwrap_or_default();
    let encoded = encode_scalar_component(scalar).map_err(codec)?;
    apply_component_blob_delta(blobs, &old_references, Some(&encoded))?;
    slots
        .insert(key.as_slice(), encoded.bytes.as_slice())
        .map_err(backend)?;
    Ok(())
}

fn delete_scalar(
    slots: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    memory: MemoryId,
    limits: DecodeLimits,
    blobs: &mut BTreeMap<BlobDigest, BlobRecord>,
) -> Result<(), StoreError> {
    let key = memory_storage_key(app, memory);
    if let Some(value) = slots.get(key.as_slice()).map_err(backend)? {
        let references = scalar_component_blob_references(value.value(), limits).map_err(codec)?;
        apply_component_blob_delta(blobs, &references, None)?;
    }
    slots.remove(key.as_slice()).map_err(backend)?;
    Ok(())
}

fn validate_row(row: &StoredRow, sparse: bool) -> Result<(), StoreError> {
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

fn validate_list_record(list: &ListRecord) -> Result<(), StoreError> {
    let unique = list.rows.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != list.rows.len() {
        return Err(StoreError::InvalidAuthority(
            "list order repeats a row identity".to_owned(),
        ));
    }
    if !list.touched && list.next_key != 0 {
        return Err(StoreError::InvalidAuthority(
            "sparse row overrides must not replace list allocator state".to_owned(),
        ));
    }
    if list.touched {
        let minimum_next = list
            .rows
            .iter()
            .fold(1u64, |next, row| next.max(row.key.saturating_add(1)));
        if list.next_key < minimum_next {
            return Err(StoreError::InvalidAuthority(format!(
                "next key {} is below {}",
                list.next_key, minimum_next
            )));
        }
    }
    Ok(())
}

fn record_checkpoint(
    table: &mut Table<'_, &[u8], &[u8]>,
    app: &[u8; 32],
    record: CheckpointRecord,
) -> Result<(), StoreError> {
    let key = checkpoint_storage_key(app, record.next_epoch);
    let value = encode_checkpoint_record(&record)?;
    table
        .insert(key.as_slice(), value.as_slice())
        .map_err(backend)?;

    let keys = {
        let mut keys = Vec::new();
        for entry in table.iter().map_err(backend)? {
            let (key, _) = entry.map_err(backend)?;
            if key.value().starts_with(app) {
                if key.value().len() != 40 {
                    return Err(corrupt("invalid checkpoint key"));
                }
                keys.push(key.value().to_vec());
            }
        }
        keys
    };
    let remove_count = keys
        .len()
        .saturating_sub(MAX_CHECKPOINT_RECORDS_PER_APPLICATION);
    for key in keys.into_iter().take(remove_count) {
        table.remove(key.as_slice()).map_err(backend)?;
    }
    Ok(())
}

fn read_meta(
    table: &ReadOnlyTable<&[u8], &[u8]>,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Option<MetaRecord>, StoreError> {
    let key = application_key(application);
    table
        .get(key.as_slice())
        .map_err(backend)?
        .map(|value| decode_and_validate_meta(value.value(), application, limits))
        .transpose()
}

fn read_meta_write(
    table: &Table<'_, &[u8], &[u8]>,
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<Option<MetaRecord>, StoreError> {
    let key = application_key(application);
    table
        .get(key.as_slice())
        .map_err(backend)?
        .map(|value| decode_and_validate_meta(value.value(), application, limits))
        .transpose()
}

fn decode_and_validate_meta(
    bytes: &[u8],
    application: &ApplicationIdentity,
    limits: DecodeLimits,
) -> Result<MetaRecord, StoreError> {
    let meta = decode_meta(bytes, limits)?;
    if &meta.application != application {
        return Err(StoreError::IdentityMismatch);
    }
    Ok(meta)
}

fn prefix_stats(
    table: &ReadOnlyTable<&[u8], &[u8]>,
    prefix: &[u8],
) -> Result<(usize, u64), StoreError> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in table.iter().map_err(backend)? {
        let (key, value) = entry.map_err(backend)?;
        if key.value().starts_with(prefix) {
            count = count
                .checked_add(1)
                .ok_or_else(|| corrupt("table entry count overflow"))?;
            let entry_bytes = key
                .value()
                .len()
                .checked_add(value.value().len())
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or_else(|| corrupt("table entry byte count overflow"))?;
            bytes = bytes
                .checked_add(entry_bytes)
                .ok_or_else(|| corrupt("table byte count overflow"))?;
        }
    }
    Ok((count, bytes))
}

#[cfg(test)]
fn count_prefix(table: &ReadOnlyTable<&[u8], &[u8]>, prefix: &[u8]) -> Result<usize, StoreError> {
    prefix_stats(table, prefix).map(|(count, _)| count)
}

fn encode_meta(meta: &MetaRecord) -> Result<Vec<u8>, StoreError> {
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
        .map_err(cbor_encode)?;
    Ok(bytes)
}

fn decode_meta(bytes: &[u8], limits: DecodeLimits) -> Result<MetaRecord, StoreError> {
    component_size(bytes, limits, "metadata")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 10, "metadata")?;
    expect_format(&mut decoder, "metadata")?;
    let application = ApplicationIdentity::new(
        decode_text(&mut decoder, limits)?,
        decode_text(&mut decoder, limits)?,
        decode_text(&mut decoder, limits)?,
    );
    let schema_version = decoder.u64().map_err(cbor_decode)?;
    let schema_hash = decode_digest(&mut decoder)?;
    let epoch = decoder.u64().map_err(cbor_decode)?;
    let through_turn_sequence = decoder.u64().map_err(cbor_decode)?;
    let clean_shutdown = decoder.bool().map_err(cbor_decode)?;
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

fn encode_list_record(list: &ListRecord) -> Result<Vec<u8>, StoreError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes);
    encoder
        .array(4)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.bool(list.touched))
        .and_then(|encoder| encoder.u64(list.next_key))
        .and_then(|encoder| encoder.array(list.rows.len() as u64))
        .map_err(cbor_encode)?;
    for row in &list.rows {
        encoder
            .array(2)
            .and_then(|encoder| encoder.u64(row.key))
            .and_then(|encoder| encoder.u64(row.generation))
            .map_err(cbor_encode)?;
    }
    Ok(bytes)
}

fn decode_list_record(bytes: &[u8], limits: DecodeLimits) -> Result<ListRecord, StoreError> {
    component_size(bytes, limits, "list metadata")?;
    let mut decoder = Decoder::new(bytes);
    expect_array(&mut decoder, 4, "list metadata")?;
    expect_format(&mut decoder, "list metadata")?;
    let touched = decoder.bool().map_err(cbor_decode)?;
    let next_key = decoder.u64().map_err(cbor_decode)?;
    let count = collection_len(&mut decoder, limits, "list row order")?;
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        expect_array(&mut decoder, 2, "row identity")?;
        rows.push(RowRef {
            key: decoder.u64().map_err(cbor_decode)?,
            generation: decoder.u64().map_err(cbor_decode)?,
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

fn encode_checkpoint_record(record: &CheckpointRecord) -> Result<Vec<u8>, StoreError> {
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
        .map_err(cbor_encode)?;
    Ok(bytes)
}

fn encode_content_artifact(artifact: &ContentArtifact) -> Result<Vec<u8>, StoreError> {
    validate_content_artifact(artifact)?;
    let mut bytes = Vec::new();
    Encoder::new(&mut bytes)
        .array(3)
        .and_then(|encoder| encoder.u32(COMPONENT_FORMAT))
        .and_then(|encoder| encoder.str(&artifact.media_type))
        .and_then(|encoder| encoder.bytes(&artifact.bytes))
        .map_err(cbor_encode)?;
    Ok(bytes)
}

fn encode_content_artifact_binding(binding: ContentArtifactBinding) -> Result<Vec<u8>, StoreError> {
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
        .map_err(cbor_encode)?;
    Ok(bytes)
}

fn decode_content_artifact_binding(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ContentArtifactBinding, StoreError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(corrupt(
            "encoded content artifact binding exceeds decode byte limit",
        ));
    }
    let mut decoder = Decoder::new(bytes);
    if decoder.array().map_err(cbor_decode)? != Some(3)
        || decoder.u32().map_err(cbor_decode)? != COMPONENT_FORMAT
    {
        return Err(corrupt("unsupported content artifact binding format"));
    }
    let artifact_id = ContentArtifactId(decode_digest(&mut decoder)?);
    let retention = match decoder.u8().map_err(cbor_decode)? {
        0 => ContentArtifactRetention::Replaceable,
        1 => ContentArtifactRetention::Immutable,
        _ => return Err(corrupt("unknown content artifact retention tag")),
    };
    if decoder.position() != bytes.len() {
        return Err(corrupt("content artifact binding has trailing bytes"));
    }
    Ok(ContentArtifactBinding {
        artifact_id,
        retention,
    })
}

fn decode_content_artifact(
    id: ContentArtifactId,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<ContentArtifact, StoreError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(corrupt(
            "encoded content artifact exceeds decode byte limit",
        ));
    }
    let mut decoder = Decoder::new(bytes);
    if decoder.array().map_err(cbor_decode)? != Some(3)
        || decoder.u32().map_err(cbor_decode)? != COMPONENT_FORMAT
    {
        return Err(corrupt("unsupported content artifact record format"));
    }
    let media_type = decoder.str().map_err(cbor_decode)?;
    if media_type.len() > super::MAX_CONTENT_ARTIFACT_MEDIA_TYPE_BYTES {
        return Err(corrupt("content artifact media type exceeds byte limit"));
    }
    let media_type = media_type.to_owned();
    let payload = decoder.bytes().map_err(cbor_decode)?;
    if payload.len() > super::MAX_CONTENT_ARTIFACT_BYTES {
        return Err(corrupt("content artifact payload exceeds byte limit"));
    }
    let payload = payload.to_vec();
    if decoder.position() != bytes.len() {
        return Err(corrupt("content artifact record has trailing bytes"));
    }
    let artifact = ContentArtifact {
        id,
        media_type,
        bytes: payload,
    };
    validate_content_artifact(&artifact)?;
    Ok(artifact)
}

fn restore_checksum(image: &RestoreImage) -> Result<[u8; 32], StoreError> {
    let bytes = encode_restore_image(image).map_err(codec)?;
    Ok(Sha256::digest(bytes).into())
}

fn application_key(application: &ApplicationIdentity) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_application(&mut hasher, application);
    hasher.finalize().into()
}

fn memory_storage_key(app: &[u8; 32], memory: MemoryId) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(memory.as_bytes());
    key
}

fn row_storage_key(app: &[u8; 32], memory: MemoryId, row: RowRef) -> [u8; 80] {
    let mut key = [0; 80];
    key[..64].copy_from_slice(&memory_storage_key(app, memory));
    key[64..72].copy_from_slice(&row.key.to_be_bytes());
    key[72..80].copy_from_slice(&row.generation.to_be_bytes());
    key
}

fn checkpoint_storage_key(app: &[u8; 32], epoch: u64) -> [u8; 40] {
    let mut key = [0; 40];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(&epoch.to_be_bytes());
    key
}

fn migration_storage_key(app: &[u8; 32], edge: MigrationEdgeId) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(edge.as_bytes());
    key
}

fn outbox_storage_key(app: &[u8; 32], item_id: OutboxItemId) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(item_id.as_bytes());
    key
}

fn blob_storage_key(app: &[u8; 32], digest: BlobDigest) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(&digest.0);
    key
}

fn content_artifact_storage_key(app: &[u8; 32], artifact: ContentArtifactId) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(artifact.as_bytes());
    key
}

fn content_artifact_owner_storage_key(app: &[u8; 32], owner: ContentArtifactOwnerId) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(app);
    key[32..].copy_from_slice(owner.as_bytes());
    key
}

fn memory_from_key(key: &[u8], label: &str) -> Result<MemoryId, StoreError> {
    if key.len() != 64 {
        return Err(corrupt(format!("invalid {label} key")));
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&key[32..]);
    Ok(MemoryId(digest))
}

fn validate_row_key(key: &[u8]) -> Result<(), StoreError> {
    if key.len() != 80 {
        return Err(corrupt("invalid row key"));
    }
    Ok(())
}

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

fn component_size(bytes: &[u8], limits: DecodeLimits, label: &str) -> Result<(), StoreError> {
    if bytes.len() > limits.max_total_bytes {
        return Err(corrupt(format!("{label} exceeds total byte limit")));
    }
    Ok(())
}

fn expect_array(decoder: &mut Decoder<'_>, expected: usize, label: &str) -> Result<(), StoreError> {
    let actual = decoder
        .array()
        .map_err(cbor_decode)?
        .ok_or_else(|| corrupt(format!("{label} must use a definite array")))?;
    if actual != expected as u64 {
        return Err(corrupt(format!(
            "{label} has {actual} fields, expected {expected}"
        )));
    }
    Ok(())
}

fn expect_format(decoder: &mut Decoder<'_>, label: &str) -> Result<(), StoreError> {
    let format = decoder.u32().map_err(cbor_decode)?;
    if format != COMPONENT_FORMAT {
        return Err(corrupt(format!("unsupported {label} format {format}")));
    }
    Ok(())
}

fn collection_len(
    decoder: &mut Decoder<'_>,
    limits: DecodeLimits,
    label: &str,
) -> Result<usize, StoreError> {
    let count = decoder
        .array()
        .map_err(cbor_decode)?
        .ok_or_else(|| corrupt(format!("{label} must use a definite array")))?;
    let count = usize::try_from(count).map_err(|_| corrupt(format!("{label} is too large")))?;
    if count > limits.max_collection_items {
        return Err(corrupt(format!("{label} exceeds collection item limit")));
    }
    Ok(count)
}

fn decode_text(decoder: &mut Decoder<'_>, limits: DecodeLimits) -> Result<String, StoreError> {
    let value = decoder.str().map_err(cbor_decode)?;
    if value.len() > limits.max_text_bytes {
        return Err(corrupt("metadata text exceeds decode limit"));
    }
    Ok(value.to_owned())
}

fn decode_digest(decoder: &mut Decoder<'_>) -> Result<[u8; 32], StoreError> {
    decoder
        .bytes()
        .map_err(cbor_decode)?
        .try_into()
        .map_err(|_| corrupt("digest must contain exactly 32 bytes"))
}

fn reject_trailing(decoder: &Decoder<'_>, bytes: &[u8], label: &str) -> Result<(), StoreError> {
    if decoder.position() != bytes.len() {
        return Err(corrupt(format!("{label} has trailing bytes")));
    }
    Ok(())
}

fn backend(error: impl fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

fn codec(error: impl fmt::Display) -> StoreError {
    StoreError::Backend(format!("durable CBOR: {error}"))
}

fn cbor_encode(error: impl fmt::Debug) -> StoreError {
    codec(format!("encode failed: {error:?}"))
}

fn cbor_decode(error: impl fmt::Display) -> StoreError {
    codec(format!("decode failed: {error}"))
}

fn corrupt(detail: impl Into<String>) -> StoreError {
    StoreError::Backend(format!("corrupt durable state: {}", detail.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompactRequest, DurableOutboxState, InspectRequest, PersistenceCommand, PersistenceResult,
        RestoreRequest, ShutdownRequest, StoredScalar, StoredValue,
    };
    use boon_plan::{EffectId, MemoryKind, MemoryLeafId, MemoryOwnerPath};
    use redb::TableHandle;

    fn application() -> ApplicationIdentity {
        ApplicationIdentity::new("dev.boon.redb", "test", "local")
    }

    fn memory(name: &str, kind: MemoryKind) -> MemoryId {
        MemoryId::from_identity(
            &MemoryOwnerPath {
                canonical_module: "redb_test".to_owned(),
                named_owner_path: "store".to_owned(),
            },
            name,
            kind,
        )
        .unwrap()
    }

    fn scalar(name: &str) -> MemoryId {
        memory(name, MemoryKind::Scalar)
    }

    fn list(name: &str) -> MemoryId {
        memory(name, MemoryKind::List)
    }

    fn load(driver: &mut RedbDriver, application: ApplicationIdentity) -> RestoreImage {
        match driver.execute(PersistenceCommand::Load(RestoreRequest {
            application,
            expected_schema_hash: None,
        })) {
            PersistenceResult::Loaded(Ok(Some(image))) => image,
            result => panic!("unexpected load result: {result:?}"),
        }
    }

    fn initialize(driver: &mut RedbDriver, image: RestoreImage) {
        assert!(matches!(
            driver.execute(PersistenceCommand::Initialize(image)),
            PersistenceResult::Initialized(Ok(_))
        ));
    }

    fn row(key: u64, text: &str, field: MemoryLeafId) -> StoredRow {
        StoredRow {
            key,
            generation: 1,
            fields: BTreeMap::from([(field, StoredValue::Text(text.to_owned()))]),
            touched_fields: BTreeSet::from([field]),
        }
    }

    fn pending_outbox(turn_sequence: u64) -> DurableOutboxItem {
        let effect = EffectId::from_host_operation("Test/send").unwrap();
        DurableOutboxItem::pending(
            boon_plan::EffectInvocationId::from_semantic_route(effect, "test.send", "store.result")
                .unwrap(),
            effect,
            StoredValue::Text("request-1".to_owned()),
            StoredValue::Record(BTreeMap::from([(
                "amount".to_owned(),
                StoredValue::Number(12),
            )])),
            None,
            turn_sequence,
        )
    }

    fn checkpoint(
        application: ApplicationIdentity,
        schema_hash: [u8; 32],
        base_epoch: u64,
        turn_sequence: u64,
        changes: Vec<DurableChange>,
        outbox_changes: Vec<super::super::DurableOutboxChange>,
    ) -> CheckpointBatch {
        CheckpointBatch {
            application,
            schema_hash,
            base_epoch,
            next_epoch: base_epoch + 1,
            first_turn_sequence: turn_sequence,
            last_turn_sequence: turn_sequence,
            changes,
            outbox_changes,
            content_artifact_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal()
    }

    fn put_artifact(
        driver: &mut RedbDriver,
        application: &ApplicationIdentity,
        artifact: &ContentArtifact,
    ) {
        assert!(matches!(
            driver.execute(PersistenceCommand::PutContentArtifact(
                PutContentArtifactRequest {
                    application: application.clone(),
                    artifact: artifact.clone(),
                }
            )),
            PersistenceResult::ContentArtifactStored(Ok(_))
        ));
    }

    fn artifact_checkpoint(
        application: ApplicationIdentity,
        schema_hash: [u8; 32],
        base_epoch: u64,
        turn_sequence: u64,
        content_artifact_changes: Vec<super::super::DurableContentArtifactChange>,
    ) -> CheckpointBatch {
        CheckpointBatch {
            application,
            schema_hash,
            base_epoch,
            next_epoch: base_epoch + 1,
            first_turn_sequence: turn_sequence,
            last_turn_sequence: turn_sequence,
            changes: Vec::new(),
            outbox_changes: Vec::new(),
            content_artifact_changes,
            checksum: [0; 32],
        }
        .seal()
    }

    #[test]
    fn redb_uses_component_tables_and_survives_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let image = RestoreImage::empty(application(), 1, [4; 32]);
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, image.clone());
            let transaction = driver.database().unwrap().begin_read().unwrap();
            let names = transaction
                .list_tables()
                .unwrap()
                .map(|table| table.name().to_owned())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                names,
                BTreeSet::from([
                    "ARTIFACTS".to_owned(),
                    "ARTIFACT_OWNERS".to_owned(),
                    "BLOBS".to_owned(),
                    "CHECKPOINTS".to_owned(),
                    "LISTS".to_owned(),
                    "META".to_owned(),
                    "MIGRATIONS".to_owned(),
                    "OUTBOX".to_owned(),
                    "ROWS".to_owned(),
                    "SLOTS".to_owned(),
                ])
            );
            drop(transaction);
            driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
        }
        let mut driver = RedbDriver::open(&path).unwrap();
        assert_eq!(load(&mut driver, application()), image);
    }

    #[test]
    fn content_artifact_survives_reopen_is_idempotent_and_reset_removes_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let artifact = ContentArtifact::new(
            "application/vnd.boon.test-artifact",
            b"durable immutable program".to_vec(),
        )
        .unwrap();
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [4; 32]));
            assert!(matches!(
                driver.execute(PersistenceCommand::PutContentArtifact(
                    PutContentArtifactRequest {
                        application: app.clone(),
                        artifact: artifact.clone(),
                    }
                )),
                PersistenceResult::ContentArtifactStored(Ok(PutContentArtifactAck {
                    already_present: false,
                    ..
                }))
            ));
        }

        let mut driver = RedbDriver::open(&path).unwrap();
        assert_eq!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: app.clone(),
                    id: artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(Some(artifact.clone())))
        );
        let snapshot = match driver.execute(PersistenceCommand::Inspect(InspectRequest {
            application: app.clone(),
        })) {
            PersistenceResult::Inspected(Ok(Some(snapshot))) => snapshot,
            result => panic!("unexpected inspector result: {result:?}"),
        };
        assert_eq!(snapshot.content_artifact_count, 1);
        assert_eq!(snapshot.content_artifact_bytes, artifact.bytes.len() as u64);
        assert_eq!(snapshot.content_artifact_orphan_count, 1);
        assert!(matches!(
            driver.execute(PersistenceCommand::PutContentArtifact(
                PutContentArtifactRequest {
                    application: app.clone(),
                    artifact: artifact.clone(),
                }
            )),
            PersistenceResult::ContentArtifactStored(Ok(PutContentArtifactAck {
                already_present: true,
                ..
            }))
        ));
        let reset = ResetApplicationBatch {
            application: app.clone(),
            expected_base_epoch: 0,
            next_epoch: 1,
            source_schema_hash: [4; 32],
            default_image: RestoreImage::empty(app.clone(), 1, [4; 32]),
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
                    application: app,
                    id: artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(None))
        );
    }

    #[test]
    fn artifact_owners_survive_reopen_export_exactly_and_compact_orphans() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let retained = ContentArtifact::new("text/plain", b"retained".to_vec()).unwrap();
        let orphan = ContentArtifact::new("text/plain", b"orphan".to_vec()).unwrap();
        let owner = ContentArtifactOwnerId([0x61; 32]);
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [4; 32]));
            put_artifact(&mut driver, &app, &retained);
            put_artifact(&mut driver, &app, &orphan);
            let batch = artifact_checkpoint(
                app.clone(),
                [4; 32],
                0,
                1,
                vec![super::super::DurableContentArtifactChange::SetReplaceable {
                    owner_id: owner,
                    artifact_id: retained.id,
                }],
            );
            assert!(matches!(
                driver.execute(PersistenceCommand::Commit(batch)),
                PersistenceResult::Committed(Ok(_))
            ));
        }

        let mut driver = RedbDriver::open(&path).unwrap();
        let restored = load(&mut driver, app.clone());
        assert_eq!(
            restored.content_artifact_manifest.bindings[&owner].artifact_id,
            retained.id
        );
        let transfer = match driver.execute(PersistenceCommand::ExportApplication(
            ExportApplicationRequest {
                application: app.clone(),
            },
        )) {
            PersistenceResult::ApplicationExported(Ok(transfer)) => transfer,
            result => panic!("unexpected export result: {result:?}"),
        };
        assert_eq!(
            transfer.content_artifacts,
            BTreeMap::from([(retained.id, retained.clone())])
        );
        let snapshot = match driver.execute(PersistenceCommand::Inspect(InspectRequest {
            application: app.clone(),
        })) {
            PersistenceResult::Inspected(Ok(Some(snapshot))) => snapshot,
            result => panic!("unexpected inspector result: {result:?}"),
        };
        assert_eq!(snapshot.content_artifact_owner_count, 1);
        assert_eq!(snapshot.content_artifact_retained_count, 1);
        assert_eq!(snapshot.content_artifact_orphan_count, 1);
        assert!(matches!(
            driver.execute(PersistenceCommand::Compact(CompactRequest {
                application: app.clone(),
            })),
            PersistenceResult::Compacted(Ok(_))
        ));
        assert!(matches!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: app.clone(),
                    id: orphan.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(None))
        ));
        assert_eq!(
            load(&mut driver, app).content_artifact_manifest,
            restored.content_artifact_manifest
        );
    }

    #[test]
    fn metadata_key_collision_is_rejected_by_full_application_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [4; 32]));

        let database = driver.database().unwrap();
        let mut transaction = database.begin_write().unwrap();
        immediate(&mut transaction).unwrap();
        {
            let mut table = transaction.open_table(META).unwrap();
            let collision = MetaRecord {
                application: ApplicationIdentity::new("dev.boon.other", "test", "local"),
                schema_version: 1,
                schema_hash: [4; 32],
                epoch: 0,
                through_turn_sequence: 0,
                clean_shutdown: false,
                initialization_checksum: [0; 32],
            };
            table
                .insert(
                    application_key(&app).as_slice(),
                    encode_meta(&collision).unwrap().as_slice(),
                )
                .unwrap();
        }
        transaction.commit().unwrap();

        assert_eq!(
            driver.execute(PersistenceCommand::Load(RestoreRequest {
                application: app,
                expected_schema_hash: None,
            })),
            PersistenceResult::Loaded(Err(StoreError::IdentityMismatch))
        );
    }

    #[test]
    fn repeated_sparse_row_changes_update_one_override_without_materializing_the_list() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let cells = list("cells");
        let formula = MemoryLeafId::from_memory_path(cells, "formula_text").unwrap();
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [1; 32]));
            let batch = CheckpointBatch {
                application: app.clone(),
                schema_hash: [1; 32],
                base_epoch: 0,
                next_epoch: 1,
                first_turn_sequence: 1,
                last_turn_sequence: 2,
                changes: vec![
                    DurableChange::SetRowField {
                        memory_id: cells,
                        row_key: 2,
                        row_generation: 1,
                        field_id: formula,
                        value: StoredValue::Text("=A1".to_owned()),
                    },
                    DurableChange::SetRowField {
                        memory_id: cells,
                        row_key: 2,
                        row_generation: 1,
                        field_id: formula,
                        value: StoredValue::Text("=A1+1".to_owned()),
                    },
                ],
                outbox_changes: Vec::new(),
                content_artifact_changes: Vec::new(),
                checksum: [0; 32],
            }
            .seal();
            assert!(matches!(
                driver.execute(PersistenceCommand::Commit(batch)),
                PersistenceResult::Committed(Ok(CommitAck { epoch: 1, .. }))
            ));
            driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
        }
        let mut driver = RedbDriver::open(&path).unwrap();
        let image = load(&mut driver, app);
        let stored = &image.lists[&cells];
        assert!(!stored.touched);
        assert_eq!(stored.next_key, 0);
        assert_eq!(stored.rows.len(), 1);
        assert_eq!(
            stored.rows[0].fields[&formula],
            StoredValue::Text("=A1+1".to_owned())
        );
    }

    #[test]
    fn structural_list_changes_preserve_order_generation_and_allocator_after_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let todos = list("todos");
        let title = MemoryLeafId::from_memory_path(todos, "title").unwrap();
        let first = row(0, "first", title);
        let second = row(1, "second", title);
        let third = row(2, "third", title);
        let mut image = RestoreImage::empty(app.clone(), 1, [2; 32]);
        image.lists.insert(
            todos,
            StoredList {
                touched: true,
                next_key: 2,
                rows: vec![first.clone(), second.clone()],
            },
        );
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, image);
            let batch = CheckpointBatch {
                application: app.clone(),
                schema_hash: [2; 32],
                base_epoch: 0,
                next_epoch: 1,
                first_turn_sequence: 1,
                last_turn_sequence: 2,
                changes: vec![
                    DurableChange::RemoveRow {
                        memory_id: todos,
                        row_key: first.key,
                        row_generation: first.generation,
                        next_key: 2,
                    },
                    DurableChange::InsertRow {
                        memory_id: todos,
                        index: 0,
                        row: third.clone(),
                        next_key: 3,
                    },
                ],
                outbox_changes: Vec::new(),
                content_artifact_changes: Vec::new(),
                checksum: [0; 32],
            }
            .seal();
            assert!(matches!(
                driver.execute(PersistenceCommand::Commit(batch)),
                PersistenceResult::Committed(Ok(_))
            ));
            driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
        }
        let mut driver = RedbDriver::open(&path).unwrap();
        let stored = &load(&mut driver, app).lists[&todos];
        assert_eq!(stored.next_key, 3);
        assert_eq!(
            stored
                .rows
                .iter()
                .map(|row| (row.key, row.generation))
                .collect::<Vec<_>>(),
            vec![(2, 1), (1, 1)]
        );
        assert_eq!(stored.rows[0], third);
        assert_eq!(stored.rows[1], second);
    }

    #[test]
    fn failed_and_stale_transactions_leave_all_component_tables_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let count = scalar("count");
        let missing = list("missing");
        let field = MemoryLeafId::from_memory_path(missing, "text").unwrap();
        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [3; 32]));
        let failing = CheckpointBatch {
            application: app.clone(),
            schema_hash: [3; 32],
            base_epoch: 0,
            next_epoch: 1,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: vec![
                DurableChange::SetScalar {
                    memory_id: count,
                    value: StoredScalar {
                        touched: true,
                        value: StoredValue::Number(9),
                    },
                },
                DurableChange::InsertRow {
                    memory_id: missing,
                    index: 0,
                    row: row(0, "invalid", field),
                    next_key: 1,
                },
            ],
            outbox_changes: Vec::new(),
            content_artifact_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(failing)),
            PersistenceResult::Committed(Err(StoreError::InvalidAuthority(_)))
        ));
        assert_eq!(load(&mut driver, app.clone()).epoch, 0);
        assert!(!load(&mut driver, app.clone()).scalars.contains_key(&count));

        let stale = CheckpointBatch {
            application: app.clone(),
            schema_hash: [3; 32],
            base_epoch: 7,
            next_epoch: 8,
            first_turn_sequence: 1,
            last_turn_sequence: 1,
            changes: vec![DurableChange::SetScalar {
                memory_id: count,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Number(10),
                },
            }],
            outbox_changes: Vec::new(),
            content_artifact_changes: Vec::new(),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(stale)),
            PersistenceResult::Committed(Err(StoreError::StaleEpoch))
        ));
        let restored = load(&mut driver, app);
        assert_eq!(restored.epoch, 0);
        assert!(restored.scalars.is_empty());
        assert!(restored.lists.is_empty());
    }

    #[test]
    fn activation_atomically_changes_schema_deletes_old_memory_and_records_edge() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let old = scalar("count");
        let new = scalar("click_count");
        let stale_artifact = ContentArtifact::new("text/plain", b"stale".to_vec()).unwrap();
        let target_artifact = ContentArtifact::new("text/plain", b"target".to_vec()).unwrap();
        let artifact_owner = ContentArtifactOwnerId([0x81; 32]);
        let mut image = RestoreImage::empty(app.clone(), 1, [5; 32]);
        image.scalars.insert(
            old,
            StoredScalar {
                touched: true,
                value: StoredValue::Number(7),
            },
        );
        let edge = MigrationEdgeId::from_schema_transition(
            1,
            2,
            [5; 32],
            boon_plan::MigrationRecipeId([6; 32]),
        )
        .unwrap();
        {
            let mut driver = RedbDriver::open(&path).unwrap();
            initialize(&mut driver, image);
            put_artifact(&mut driver, &app, &stale_artifact);
            let batch = ActivationBatch {
                application: app.clone(),
                expected_base_epoch: 0,
                next_epoch: 1,
                source_schema_hash: [5; 32],
                target_schema_version: 2,
                target_schema_hash: [6; 32],
                through_turn_sequence: 0,
                authority_changes: vec![DurableChange::SetScalar {
                    memory_id: new,
                    value: StoredScalar {
                        touched: true,
                        value: StoredValue::Number(7),
                    },
                }],
                completed_migration_edges: vec![edge],
                deleted_memory: vec![old],
                target_content_artifact_manifest: ContentArtifactManifest {
                    bindings: BTreeMap::from([(
                        artifact_owner,
                        ContentArtifactBinding {
                            artifact_id: target_artifact.id,
                            retention: ContentArtifactRetention::Immutable,
                        },
                    )]),
                },
                content_artifacts: BTreeMap::from([(target_artifact.id, target_artifact.clone())]),
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
            driver.execute(PersistenceCommand::Shutdown(ShutdownRequest));
        }
        let mut driver = RedbDriver::open(&path).unwrap();
        let restored = load(&mut driver, app.clone());
        assert_eq!(restored.schema_version, 2);
        assert_eq!(restored.schema_hash, [6; 32]);
        assert!(!restored.scalars.contains_key(&old));
        assert_eq!(restored.scalars[&new].value, StoredValue::Number(7));
        assert!(restored.completed_migration_edges.contains(&edge));
        assert_eq!(
            restored.content_artifact_manifest.bindings[&artifact_owner].artifact_id,
            target_artifact.id
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: app.clone(),
                    id: stale_artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(None))
        ));
        assert!(matches!(
            driver.execute(PersistenceCommand::LoadContentArtifact(
                LoadContentArtifactRequest {
                    application: app,
                    id: target_artifact.id,
                }
            )),
            PersistenceResult::ContentArtifactLoaded(Ok(Some(_)))
        ));
    }

    #[test]
    fn checkpoint_metadata_is_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [8; 32]));
        for sequence in 1..=MAX_CHECKPOINT_RECORDS_PER_APPLICATION as u64 + 5 {
            let batch = CheckpointBatch {
                application: app.clone(),
                schema_hash: [8; 32],
                base_epoch: sequence - 1,
                next_epoch: sequence,
                first_turn_sequence: sequence,
                last_turn_sequence: sequence,
                changes: Vec::new(),
                outbox_changes: Vec::new(),
                content_artifact_changes: Vec::new(),
                checksum: [0; 32],
            }
            .seal();
            assert!(matches!(
                driver.execute(PersistenceCommand::Commit(batch)),
                PersistenceResult::Committed(Ok(_))
            ));
        }
        let transaction = driver.database().unwrap().begin_read().unwrap();
        let table = transaction.open_table(CHECKPOINTS).unwrap();
        assert_eq!(
            count_prefix(&table, &application_key(&app)).unwrap(),
            MAX_CHECKPOINT_RECORDS_PER_APPLICATION
        );
    }

    #[test]
    fn outbox_survives_every_external_effect_crash_boundary_without_replaying_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let pending_memory = scalar("pending");
        let completed_memory = scalar("completed");
        let item = pending_outbox(1);
        let item_id = item.item_id;

        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, RestoreImage::empty(app.clone(), 1, [9; 32]));
        let intent = checkpoint(
            app.clone(),
            [9; 32],
            0,
            1,
            vec![DurableChange::SetScalar {
                memory_id: pending_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Bool(true),
                },
            }],
            vec![super::super::DurableOutboxChange::Enqueue { item: item.clone() }],
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(intent)),
            PersistenceResult::Committed(Ok(_))
        ));
        drop(driver);

        let mut driver = RedbDriver::open(&path).unwrap();
        let restored = load(&mut driver, app.clone());
        assert_eq!(restored.outbox[&item_id].state, DurableOutboxState::Pending);
        assert_eq!(
            restored.scalars[&pending_memory].value,
            StoredValue::Bool(true)
        );
        let dispatch = checkpoint(
            app.clone(),
            [9; 32],
            1,
            2,
            Vec::new(),
            vec![super::super::DurableOutboxChange::BeginDispatch {
                item_id,
                expected_revision: 0,
                next_revision: 1,
                attempt: 1,
                turn_sequence: 2,
            }],
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(dispatch)),
            PersistenceResult::Committed(Ok(_))
        ));
        drop(driver);

        let mut driver = RedbDriver::open(&path).unwrap();
        let restored = load(&mut driver, app.clone());
        assert_eq!(
            restored.outbox[&item_id].state,
            DurableOutboxState::Dispatching { attempt: 1 }
        );
        assert_eq!(
            restored.outbox[&item_id].item_id,
            OutboxItemId::from_invocation(
                item.invocation_id,
                item.effect_id,
                &item.idempotency_key,
                item.target_row,
            )
        );
        let outcome = checkpoint(
            app.clone(),
            [9; 32],
            2,
            3,
            vec![DurableChange::SetScalar {
                memory_id: completed_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Bool(true),
                },
            }],
            vec![super::super::DurableOutboxChange::Complete {
                item_id,
                expected_revision: 1,
                next_revision: 2,
                attempt: 1,
                outcome: StoredValue::Text("remote-ok".to_owned()),
                turn_sequence: 3,
            }],
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(outcome.clone())),
            PersistenceResult::Committed(Ok(_))
        ));
        let acknowledged = load(&mut driver, app.clone());
        assert!(
            !acknowledged.outbox.contains_key(&item_id),
            "the atomically acknowledged outcome must consume its durable obligation"
        );
        assert_eq!(
            acknowledged.scalars[&completed_memory].value,
            StoredValue::Bool(true)
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(outcome)),
            PersistenceResult::Committed(Err(StoreError::StaleEpoch))
        ));
        assert_eq!(load(&mut driver, app), acknowledged);
    }

    #[test]
    fn shared_large_blobs_are_reference_counted_reclaimed_and_compacted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let app_key = application_key(&app);
        let payload = vec![0x5a; super::super::INLINE_BYTES_THRESHOLD + 1];
        let scalar_memory = scalar("payload");
        let list_memory = list("rows");
        let field = MemoryLeafId::from_memory_path(list_memory, "payload").unwrap();
        let mut image = RestoreImage::empty(app.clone(), 1, [10; 32]);
        image.scalars.insert(
            scalar_memory,
            StoredScalar {
                touched: true,
                value: StoredValue::Bytes(payload.clone()),
            },
        );
        image.lists.insert(
            list_memory,
            StoredList {
                touched: true,
                next_key: 1,
                rows: vec![StoredRow {
                    key: 0,
                    generation: 1,
                    fields: BTreeMap::from([(field, StoredValue::Bytes(payload.clone()))]),
                    touched_fields: BTreeSet::from([field]),
                }],
            },
        );
        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, image);
        {
            let transaction = driver.database().unwrap().begin_read().unwrap();
            let table = transaction.open_table(BLOBS).unwrap();
            assert_eq!(count_prefix(&table, &app_key).unwrap(), 1);
            let record = table.iter().unwrap().next().unwrap().unwrap().1;
            assert_eq!(
                decode_blob_record(record.value(), DecodeLimits::default())
                    .unwrap()
                    .reference_count,
                2
            );
        }

        let overwrite = checkpoint(
            app.clone(),
            [10; 32],
            0,
            1,
            vec![DurableChange::SetScalar {
                memory_id: scalar_memory,
                value: StoredScalar {
                    touched: true,
                    value: StoredValue::Bytes(vec![1, 2, 3]),
                },
            }],
            Vec::new(),
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(overwrite)),
            PersistenceResult::Committed(Ok(_))
        ));
        let delete = checkpoint(
            app.clone(),
            [10; 32],
            1,
            2,
            vec![DurableChange::DeleteList {
                memory_id: list_memory,
            }],
            Vec::new(),
        );
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(delete)),
            PersistenceResult::Committed(Ok(_))
        ));
        {
            let transaction = driver.database().unwrap().begin_read().unwrap();
            let table = transaction.open_table(BLOBS).unwrap();
            assert_eq!(count_prefix(&table, &app_key).unwrap(), 0);
        }

        let orphan_payload = vec![0x33; super::super::INLINE_BYTES_THRESHOLD + 2];
        let orphan = BlobRecord {
            digest: BlobDigest::of(&orphan_payload),
            length: orphan_payload.len() as u64,
            reference_count: 1,
            bytes: orphan_payload,
        };
        {
            let transaction = driver.database().unwrap().begin_write().unwrap();
            {
                let mut table = transaction.open_table(BLOBS).unwrap();
                let key = blob_storage_key(&app_key, orphan.digest);
                let bytes = encode_blob_record(&orphan).unwrap();
                table.insert(key.as_slice(), bytes.as_slice()).unwrap();
            }
            transaction.commit().unwrap();
        }
        assert!(matches!(
            driver.execute(PersistenceCommand::Compact(CompactRequest {
                application: app.clone(),
            })),
            PersistenceResult::Compacted(Ok(_))
        ));
        let transaction = driver.database().unwrap().begin_read().unwrap();
        let table = transaction.open_table(BLOBS).unwrap();
        assert_eq!(count_prefix(&table, &app_key).unwrap(), 0);
        assert_eq!(
            load(&mut driver, app).scalars[&scalar_memory].value,
            StoredValue::Bytes(vec![1, 2, 3])
        );
    }

    #[test]
    fn reset_application_is_one_redb_transaction_and_clears_component_domains() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.redb");
        let app = application();
        let item = pending_outbox(1);
        let artifact = ContentArtifact::new("text/plain", b"reset-owned".to_vec()).unwrap();
        let artifact_owner = ContentArtifactOwnerId([0x91; 32]);
        let mut image = RestoreImage::empty(app.clone(), 1, [11; 32]);
        image.scalars.insert(
            scalar("payload"),
            StoredScalar {
                touched: true,
                value: StoredValue::Bytes(vec![4; super::super::INLINE_BYTES_THRESHOLD + 1]),
            },
        );
        let mut driver = RedbDriver::open(&path).unwrap();
        initialize(&mut driver, image);
        put_artifact(&mut driver, &app, &artifact);
        let mut enqueue = checkpoint(
            app.clone(),
            [11; 32],
            0,
            1,
            Vec::new(),
            vec![super::super::DurableOutboxChange::Enqueue { item }],
        );
        enqueue.content_artifact_changes = vec![
            super::super::DurableContentArtifactChange::InsertImmutable {
                owner_id: artifact_owner,
                artifact_id: artifact.id,
            },
        ];
        let enqueue = enqueue.seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::Commit(enqueue)),
            PersistenceResult::Committed(Ok(_))
        ));
        let batch = ResetApplicationBatch {
            application: app.clone(),
            expected_base_epoch: 1,
            next_epoch: 2,
            source_schema_hash: [11; 32],
            default_image: RestoreImage::empty(app.clone(), 2, [12; 32]),
            checksum: [0; 32],
        }
        .seal();
        assert!(matches!(
            driver.execute(PersistenceCommand::ResetApplication(batch)),
            PersistenceResult::ApplicationReset(Ok(ResetApplicationAck {
                epoch: 2,
                schema_version: 2,
                through_turn_sequence: 1,
                ..
            }))
        ));
        let reset = load(&mut driver, app.clone());
        assert!(reset.scalars.is_empty());
        assert!(reset.lists.is_empty());
        assert!(reset.outbox.is_empty());
        assert!(reset.completed_migration_edges.is_empty());
        assert!(reset.content_artifact_manifest.bindings.is_empty());
        let transaction = driver.database().unwrap().begin_read().unwrap();
        for definition in [
            SLOTS,
            LISTS,
            ROWS,
            MIGRATIONS,
            OUTBOX,
            BLOBS,
            ARTIFACTS,
            ARTIFACT_OWNERS,
        ] {
            let table = transaction.open_table(definition).unwrap();
            assert_eq!(count_prefix(&table, &application_key(&app)).unwrap(), 0);
        }
        let checkpoints = transaction.open_table(CHECKPOINTS).unwrap();
        assert_eq!(
            count_prefix(&checkpoints, &application_key(&app)).unwrap(),
            1
        );
    }
}
