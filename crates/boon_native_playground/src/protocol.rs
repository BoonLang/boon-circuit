use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use boon_plan::ProgramRole;
use serde::Serialize;

pub use boon_runtime::{
    ApplicationIdentity, MigrationScenario, MigrationSequence, MigrationTestDriver,
    ScenarioExpectation, ScenarioFieldMatch,
};

const MAGIC: [u8; 4] = *b"BNIP";
const VERSION: u16 = 12;
const HEADER_BYTES: usize = MAGIC.len() + 2 + 1;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCE_UNITS: usize = 1_024;
const MAX_DISTRIBUTED_PROGRAMS: usize = 3;
const MAX_CATALOG_ENTRIES: usize = 1_024;
const MAX_TEST_STEPS: usize = 4_096;
const MAX_TEST_EXPECTATIONS_PER_STEP: usize = 128;
const MAX_TEST_EXPECTATION_VALUES: usize = 4_096;
const MAX_TEST_EXPECTATION_FIELDS: usize = 1_024;
const MAX_ASSET_BLOBS: usize = 1_024;
const MAX_ASSET_BLOB_BYTES: usize = 8 * 1024 * 1024;
const MAX_MIGRATION_STAGES: usize = 64;
const MAX_MIGRATION_SOURCE_FILES: usize = 1_024;
const MAX_MIGRATION_SCENARIO_BYTES: usize = 2 * 1024 * 1024;
const MAX_MIGRATION_ID_BYTES: usize = 128;
pub const MAX_PERSISTENCE_OUTBOX_SAMPLES: usize = 16;
pub const MAX_PERSISTENCE_STATUS_BYTES: usize = 4 * 1024;
pub const MAX_PERSISTENCE_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;
const MAX_AUTHORITY_PATH_BYTES: usize = 1024;
pub const VERIFY_BOUNDED_WINDOWS_ENV: &str = "BOON_VERIFY_BOUNDED_WINDOWS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Role {
    Preview = 1,
    Dev = 2,
}

impl Role {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Preview),
            2 => Ok(Self::Dev),
            _ => Err(ProtocolError::InvalidEnum("role", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramSource {
    pub role: ProgramRole,
    pub entry_path: String,
    pub units: Vec<SourceUnit>,
    pub application: ApplicationIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewSource {
    BuiltInSingleRole {
        application: ApplicationIdentity,
        units: Vec<SourceUnit>,
    },
    DistributedPackage {
        programs: Vec<ProgramSource>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBlob {
    pub url: String,
    pub media_type: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogItem {
    pub id: String,
    pub label: String,
    pub custom: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestStep {
    pub id: String,
    pub source_path: String,
    pub action_kind: Option<String>,
    pub target_text: Option<String>,
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub target_occurrence: Option<u64>,
    pub pointer_x: Option<String>,
    pub pointer_y: Option<String>,
    pub pointer_width: Option<String>,
    pub pointer_height: Option<String>,
    pub expectations: Vec<ScenarioExpectation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationStage {
    pub id: String,
    pub label: String,
    pub schema_version: u64,
    pub source: String,
    pub source_files: Vec<String>,
    pub units: Vec<SourceUnit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationBundle {
    pub initial_stage: String,
    pub launch_stage: String,
    pub test_driver: MigrationTestDriver,
    pub scenario_path: String,
    pub stages: Vec<MigrationStage>,
    pub scenario: MigrationScenario,
}

impl MigrationBundle {
    pub fn stage(&self, id: &str) -> Option<&MigrationStage> {
        self.stages.iter().find(|stage| stage.id == id)
    }

    pub fn initial(&self) -> Option<&MigrationStage> {
        self.stage(&self.initial_stage)
    }

    pub fn launch(&self) -> Option<&MigrationStage> {
        self.stage(&self.launch_stage)
    }

    pub fn manifest_sequence(&self) -> Result<MigrationSequence, String> {
        #[derive(Serialize)]
        struct SequenceDocument<'a> {
            initial_stage: &'a str,
            launch_stage: &'a str,
            test_driver: MigrationTestDriver,
            scenario: &'a str,
            #[serde(rename = "stage")]
            stages: Vec<StageDocument<'a>>,
        }

        #[derive(Serialize)]
        struct StageDocument<'a> {
            id: &'a str,
            label: &'a str,
            schema_version: u64,
            source: &'a str,
            source_files: &'a [String],
        }

        let document = SequenceDocument {
            initial_stage: &self.initial_stage,
            launch_stage: &self.launch_stage,
            test_driver: self.test_driver,
            scenario: &self.scenario_path,
            stages: self
                .stages
                .iter()
                .map(|stage| StageDocument {
                    id: &stage.id,
                    label: &stage.label,
                    schema_version: stage.schema_version,
                    source: &stage.source,
                    source_files: &stage.source_files,
                })
                .collect(),
        };
        let encoded = toml::to_string(&document).map_err(|error| error.to_string())?;
        toml::from_str(&encoded).map_err(|error| error.to_string())
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        if self.stages.is_empty() || self.stages.len() > MAX_MIGRATION_STAGES {
            return Err(ProtocolError::LimitExceeded(
                "migration stage count",
                self.stages.len(),
            ));
        }
        validate_migration_id("initial migration stage", &self.initial_stage)?;
        validate_migration_id("migration launch stage", &self.launch_stage)?;
        if self.scenario_path.is_empty() {
            return Err(ProtocolError::InvalidMigration(
                "migration scenario path is empty".to_owned(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut previous_schema_version = None;
        for stage in &self.stages {
            validate_migration_id("migration stage", &stage.id)?;
            if stage.label.is_empty() || stage.source.is_empty() || stage.units.is_empty() {
                return Err(ProtocolError::InvalidMigration(format!(
                    "migration stage `{}` has empty required data",
                    stage.id
                )));
            }
            if !ids.insert(stage.id.as_str()) {
                return Err(ProtocolError::InvalidMigration(format!(
                    "migration stage `{}` is duplicated",
                    stage.id
                )));
            }
            if previous_schema_version.is_some_and(|version| version >= stage.schema_version) {
                return Err(ProtocolError::InvalidMigration(
                    "migration schema versions are not strictly increasing".to_owned(),
                ));
            }
            previous_schema_version = Some(stage.schema_version);
            if stage.source_files.len() > MAX_MIGRATION_SOURCE_FILES {
                return Err(ProtocolError::LimitExceeded(
                    "migration source file count",
                    stage.source_files.len(),
                ));
            }
            if stage.units.len() > MAX_SOURCE_UNITS {
                return Err(ProtocolError::LimitExceeded(
                    "migration source unit count",
                    stage.units.len(),
                ));
            }
        }
        if !ids.contains(self.initial_stage.as_str()) {
            return Err(ProtocolError::InvalidMigration(format!(
                "initial migration stage `{}` is absent",
                self.initial_stage
            )));
        }
        if !ids.contains(self.launch_stage.as_str()) {
            return Err(ProtocolError::InvalidMigration(format!(
                "migration launch stage `{}` is absent",
                self.launch_stage
            )));
        }
        let sequence = self
            .manifest_sequence()
            .map_err(ProtocolError::InvalidMigration)?;
        self.scenario
            .validate(&sequence)
            .map_err(|error| ProtocolError::InvalidMigration(error.to_string()))
    }
}

fn validate_migration_id(name: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.is_empty() {
        Err(ProtocolError::InvalidMigration(format!("{name} is empty")))
    } else if value.len() > MAX_MIGRATION_ID_BYTES {
        Err(ProtocolError::LimitExceeded(name, value.len()))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationCommand {
    Preview { stage_id: String },
    Activate { stage_id: String },
    Restart,
    StartOver { confirmed: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MigrationOperation {
    Opened = 1,
    Previewed = 2,
    Activated = 3,
    Restarted = 4,
    StartedOver = 5,
    Failed = 6,
}

impl MigrationOperation {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Opened),
            2 => Ok(Self::Previewed),
            3 => Ok(Self::Activated),
            4 => Ok(Self::Restarted),
            5 => Ok(Self::StartedOver),
            6 => Ok(Self::Failed),
            _ => Err(ProtocolError::InvalidEnum("migration operation", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationStatus {
    pub request_id: Option<u64>,
    pub revision: u64,
    pub operation: MigrationOperation,
    pub ok: bool,
    pub active_stage: String,
    pub previewed_stage: Option<String>,
    pub target_stage: Option<String>,
    pub target_schema_version: u64,
    pub migration_step_count: u32,
    pub deleted_memory_count: u32,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AuthoritySelectionKind {
    Scalar = 1,
    IndexedField = 2,
    List = 3,
}

impl AuthoritySelectionKind {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Scalar),
            2 => Ok(Self::IndexedField),
            3 => Ok(Self::List),
            _ => Err(ProtocolError::InvalidEnum(
                "authority selection kind",
                value,
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritySelection {
    pub semantic_path: String,
    pub memory_id: [u8; 32],
    pub kind: AuthoritySelectionKind,
    pub row: Option<(u64, u64)>,
    pub leaf_id: Option<[u8; 32]>,
}

impl AuthoritySelection {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.semantic_path.is_empty() || self.semantic_path.len() > MAX_AUTHORITY_PATH_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "authority semantic path bytes",
                self.semantic_path.len(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StateArtifactFormat {
    CanonicalCbor = 1,
}

impl StateArtifactFormat {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::CanonicalCbor),
            _ => Err(ProtocolError::InvalidEnum("state artifact format", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStateArtifact {
    pub format: StateArtifactFormat,
    pub schema_version: u64,
    pub sha256: [u8; 32],
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateArtifactPreviewSummary {
    pub preview_id: u64,
    pub source_schema_version: u64,
    pub target_schema_version: u64,
    pub scalar_count: u32,
    pub list_count: u32,
    pub row_count: u64,
    pub migration_step_count: u32,
    pub deleted_memory_count: u32,
    pub document_node_count: u32,
    pub baseline_runtime_turn_sequence: u64,
    pub baseline_durable_epoch: u64,
    pub baseline_durable_turn_sequence: u64,
}

impl CanonicalStateArtifact {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.bytes.len() > MAX_PERSISTENCE_ARTIFACT_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "persistence artifact bytes",
                self.bytes.len(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceCommand {
    Flush,
    Compact,
    ClearAll {
        confirmed: bool,
    },
    ExportState,
    ImportPreview {
        artifact: CanonicalStateArtifact,
    },
    ActivateImport {
        preview_id: u64,
    },
    ClearSelected {
        selection: AuthoritySelection,
        confirmed: bool,
    },
}

impl PersistenceCommand {
    fn decode(input: &mut Decoder<'_>) -> Result<Self, ProtocolError> {
        match input.u8()? {
            1 => Ok(Self::Flush),
            2 => Ok(Self::Compact),
            3 => Ok(Self::ClearAll {
                confirmed: input.bool()?,
            }),
            4 => Ok(Self::ExportState),
            5 => Ok(Self::ImportPreview {
                artifact: input.state_artifact()?,
            }),
            6 => Ok(Self::ActivateImport {
                preview_id: input.u64()?,
            }),
            7 => Ok(Self::ClearSelected {
                selection: input.authority_selection()?,
                confirmed: input.bool()?,
            }),
            value => Err(ProtocolError::InvalidEnum("persistence command", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PersistenceOperation {
    Flush = 1,
    Compact = 2,
    ClearAll = 3,
    ExportState = 4,
    ImportPreview = 5,
    ActivateImport = 6,
    ClearSelected = 7,
}

impl PersistenceOperation {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Flush),
            2 => Ok(Self::Compact),
            3 => Ok(Self::ClearAll),
            4 => Ok(Self::ExportState),
            5 => Ok(Self::ImportPreview),
            6 => Ok(Self::ActivateImport),
            7 => Ok(Self::ClearSelected),
            _ => Err(ProtocolError::InvalidEnum("persistence operation", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceOperationStatus {
    pub request_id: u64,
    pub operation: PersistenceOperation,
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceCapability {
    pub available: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceCapabilities {
    pub clear_selected: PersistenceCapability,
    pub export_state: PersistenceCapability,
    pub import_preview: PersistenceCapability,
    pub activate_import: PersistenceCapability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritySummary {
    pub runtime_turn_sequence: u64,
    pub source_event_sequence: u64,
    pub scalar_count: u32,
    pub indexed_field_count: u32,
    pub list_count: u32,
    pub effect_contract_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSummary {
    pub epoch: u64,
    pub through_turn_sequence: u64,
    pub scalar_count: u32,
    pub list_count: u32,
    pub row_count: u64,
    pub content_artifact_count: u32,
    pub content_artifact_bytes: u64,
    pub encoded_value_bytes: Option<u64>,
    pub completed_migration_count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PersistenceTimingSummary {
    pub authority_enqueue_us: u64,
    pub encode_us: u64,
    pub checkpoint_us: u64,
    pub barrier_us: u64,
    pub restore_us: u64,
    pub migration_us: u64,
    pub rebuild_derived_us: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSummary {
    pub first_turn_sequence: Option<u64>,
    pub last_turn_sequence: Option<u64>,
    pub oldest_age_millis: u64,
    pub turn_count: u64,
    pub queue_depth: u32,
    pub reserved_slots: u32,
    pub accepting_turns: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableSummary {
    pub epoch: u64,
    pub through_turn_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OutboxSampleState {
    Pending = 1,
    Dispatching = 2,
    ReconciliationRequired = 3,
    Completed = 4,
}

impl OutboxSampleState {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Pending),
            2 => Ok(Self::Dispatching),
            3 => Ok(Self::ReconciliationRequired),
            4 => Ok(Self::Completed),
            _ => Err(ProtocolError::InvalidEnum("outbox sample state", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxSample {
    pub item_id: [u8; 32],
    pub invocation_id: [u8; 32],
    pub effect_id: [u8; 32],
    pub state: OutboxSampleState,
    pub attempt: u32,
    pub created_turn_sequence: u64,
    pub updated_turn_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxSummary {
    pub pending_count: u32,
    pub dispatching_count: u32,
    pub reconciliation_count: u32,
    pub completed_count: u32,
    pub samples: Vec<OutboxSample>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceSnapshot {
    pub snapshot_sequence: u64,
    pub revision: u64,
    pub application: ApplicationIdentity,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
    pub authority: AuthoritySummary,
    pub stored: Option<StoredSummary>,
    pub pending: PendingSummary,
    pub durable: DurableSummary,
    pub timings: PersistenceTimingSummary,
    pub outbox: OutboxSummary,
    pub worker_alive: bool,
    pub capabilities: PersistenceCapabilities,
    pub import_preview: Option<StateArtifactPreviewSummary>,
    pub last_actionable_error: Option<String>,
    pub last_operation: Option<PersistenceOperationStatus>,
}

impl PersistenceSnapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.outbox.samples.len() > MAX_PERSISTENCE_OUTBOX_SAMPLES {
            return Err(ProtocolError::LimitExceeded(
                "persistence outbox sample count",
                self.outbox.samples.len(),
            ));
        }
        if let Some(preview) = self.import_preview.as_ref()
            && preview.preview_id == 0
        {
            return Err(ProtocolError::InvalidPersistence(
                "state artifact preview id must be non-zero".to_owned(),
            ));
        }
        for value in [
            self.last_actionable_error.as_deref(),
            self.last_operation
                .as_ref()
                .map(|operation| operation.message.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if value.len() > MAX_PERSISTENCE_STATUS_BYTES {
                return Err(ProtocolError::LimitExceeded(
                    "persistence status bytes",
                    value.len(),
                ));
            }
        }
        for capability in [
            &self.capabilities.clear_selected,
            &self.capabilities.export_state,
            &self.capabilities.import_preview,
            &self.capabilities.activate_import,
        ] {
            if capability.reason.len() > MAX_PERSISTENCE_STATUS_BYTES {
                return Err(ProtocolError::LimitExceeded(
                    "persistence status bytes",
                    capability.reason.len(),
                ));
            }
            if capability.available && !capability.reason.is_empty() {
                return Err(ProtocolError::InvalidPersistence(
                    "available persistence capability carries a failure reason".to_owned(),
                ));
            }
            if !capability.available && capability.reason.is_empty() {
                return Err(ProtocolError::InvalidPersistence(
                    "unavailable persistence capability omits its reason".to_owned(),
                ));
            }
        }
        let has_first = self.pending.first_turn_sequence.is_some();
        let has_last = self.pending.last_turn_sequence.is_some();
        if has_first != has_last
            || (!has_first && self.pending.turn_count != 0)
            || self
                .pending
                .first_turn_sequence
                .zip(self.pending.last_turn_sequence)
                .is_some_and(|(first, last)| {
                    first > last || self.pending.turn_count != last.saturating_sub(first) + 1
                })
        {
            return Err(ProtocolError::InvalidPersistence(
                "pending turn range and count are inconsistent".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PreviewIntent {
    Replace = 1,
    Run = 2,
    Reset = 3,
    Test = 4,
}

impl PreviewIntent {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Replace),
            2 => Ok(Self::Run),
            3 => Ok(Self::Reset),
            4 => Ok(Self::Test),
            _ => Err(ProtocolError::InvalidEnum("preview intent", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameMode {
    Idle = 1,
    Burst = 2,
    Probe = 3,
}

impl FrameMode {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Idle),
            2 => Ok(Self::Burst),
            3 => Ok(Self::Probe),
            _ => Err(ProtocolError::InvalidEnum("frame mode", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProofMode {
    Off = 1,
    Trace = 2,
    Readback = 3,
}

impl ProofMode {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Off),
            2 => Ok(Self::Trace),
            3 => Ok(Self::Readback),
            _ => Err(ProtocolError::InvalidEnum("proof mode", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewStats {
    pub frame_seq: u64,
    pub source_revision: u64,
    pub frame_mode: FrameMode,
    pub proof_mode: ProofMode,
    pub frames_per_second_milli: u32,
    pub input_to_present_micros: u32,
    pub render_micros: u32,
    pub present_micros: u32,
    pub missed_frames: u64,
    pub dropped_snapshots: u64,
    pub sample_age_millis: u32,
    pub persistence_schema_version: u64,
    pub persistence_durable_epoch: u64,
    pub persistence_durable_turn: u64,
    pub persistence_pending_turns: u32,
    pub persistence_queue_depth: u32,
    pub persistence_accepting: bool,
    pub persistence_worker_alive: bool,
    pub persistence_error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Hello {
        role: Role,
        pid: u32,
    },
    Ready {
        role: Role,
    },
    Catalog {
        entries: Vec<CatalogItem>,
        active_id: String,
    },
    OpenEditor {
        example_id: String,
        label: String,
        application: ApplicationIdentity,
        revision: u64,
        units: Vec<SourceUnit>,
        migration: Option<MigrationBundle>,
        migration_stage: Option<String>,
    },
    DevSelectExample {
        example_id: String,
    },
    DevSourceChanged {
        application: ApplicationIdentity,
        revision: u64,
        units: Vec<SourceUnit>,
    },
    DevRun {
        application: ApplicationIdentity,
        revision: u64,
        units: Vec<SourceUnit>,
    },
    DevReset,
    DevTest {
        request_id: u64,
        application: ApplicationIdentity,
        revision: u64,
        units: Vec<SourceUnit>,
    },
    PreviewApply {
        intent: PreviewIntent,
        request_id: Option<u64>,
        revision: u64,
        source: PreviewSource,
        test_steps: Vec<TestStep>,
        migration: Option<MigrationBundle>,
        migration_stage: Option<String>,
    },
    PreviewAssets {
        assets: Vec<AssetBlob>,
    },
    PreviewStats(PreviewStats),
    PreviewStatus {
        revision: u64,
        ok: bool,
        message: String,
    },
    PreviewRuntimeChanged {
        revision: u64,
        runtime_sequence: u64,
    },
    PreviewTestResult {
        request_id: u64,
        passed: bool,
        message: String,
    },
    DevInspect {
        request_id: u64,
        revision: u64,
        path: String,
    },
    PreviewInspect {
        request_id: u64,
        revision: u64,
        path: String,
    },
    PreviewInspectResult {
        request_id: u64,
        revision: u64,
        runtime_sequence: u64,
        path: String,
        ok: bool,
        value: String,
        authority: Option<AuthoritySelection>,
    },
    DevMigrationCommand {
        request_id: u64,
        revision: u64,
        command: MigrationCommand,
    },
    PreviewMigrationCommand {
        request_id: u64,
        revision: u64,
        command: MigrationCommand,
    },
    PreviewMigrationStatus(MigrationStatus),
    DevPersistenceCommand {
        request_id: u64,
        revision: u64,
        command: PersistenceCommand,
    },
    PreviewPersistenceCommand {
        request_id: u64,
        revision: u64,
        command: PersistenceCommand,
    },
    PreviewPersistenceSnapshot(Box<PersistenceSnapshot>),
    PreviewPersistenceArtifact {
        request_id: u64,
        revision: u64,
        artifact: CanonicalStateArtifact,
    },
    Shutdown,
}

impl Message {
    fn tag(&self) -> u8 {
        match self {
            Self::Hello { .. } => 1,
            Self::Ready { .. } => 2,
            Self::Catalog { .. } => 3,
            Self::OpenEditor { .. } => 4,
            Self::DevSelectExample { .. } => 5,
            Self::DevSourceChanged { .. } => 6,
            Self::DevRun { .. } => 7,
            Self::DevReset => 8,
            Self::DevTest { .. } => 9,
            Self::PreviewApply { .. } => 10,
            Self::PreviewStats(_) => 11,
            Self::PreviewStatus { .. } => 12,
            Self::PreviewTestResult { .. } => 13,
            Self::Shutdown => 14,
            Self::DevInspect { .. } => 15,
            Self::PreviewInspect { .. } => 16,
            Self::PreviewInspectResult { .. } => 17,
            Self::PreviewRuntimeChanged { .. } => 18,
            Self::PreviewAssets { .. } => 19,
            Self::DevMigrationCommand { .. } => 20,
            Self::PreviewMigrationCommand { .. } => 21,
            Self::PreviewMigrationStatus(_) => 22,
            Self::DevPersistenceCommand { .. } => 23,
            Self::PreviewPersistenceCommand { .. } => 24,
            Self::PreviewPersistenceSnapshot(_) => 25,
            Self::PreviewPersistenceArtifact { .. } => 26,
        }
    }

    fn encode_payload(&self, out: &mut Encoder) -> Result<(), ProtocolError> {
        match self {
            Self::Hello { role, pid } => {
                out.u8(*role as u8);
                out.u32(*pid);
            }
            Self::Ready { role } => out.u8(*role as u8),
            Self::Catalog { entries, active_id } => {
                out.catalog(entries)?;
                out.string(active_id)?;
            }
            Self::OpenEditor {
                example_id,
                label,
                application,
                revision,
                units,
                migration,
                migration_stage,
            } => {
                out.string(example_id)?;
                out.string(label)?;
                out.application_identity(application)?;
                out.u64(*revision);
                out.source_units(units)?;
                out.optional_migration_bundle(migration.as_ref())?;
                out.optional_string(migration_stage.as_deref())?;
            }
            Self::DevSelectExample { example_id } => out.string(example_id)?,
            Self::DevSourceChanged {
                application,
                revision,
                units,
            }
            | Self::DevRun {
                application,
                revision,
                units,
            } => {
                out.application_identity(application)?;
                out.u64(*revision);
                out.source_units(units)?;
            }
            Self::DevReset => {}
            Self::DevTest {
                request_id,
                application,
                revision,
                units,
            } => {
                out.u64(*request_id);
                out.application_identity(application)?;
                out.u64(*revision);
                out.source_units(units)?;
            }
            Self::PreviewApply {
                intent,
                request_id,
                revision,
                source,
                test_steps,
                migration,
                migration_stage,
            } => {
                out.u8(*intent as u8);
                out.optional_u64(*request_id);
                out.u64(*revision);
                out.preview_source(source)?;
                out.test_steps(test_steps)?;
                out.optional_migration_bundle(migration.as_ref())?;
                out.optional_string(migration_stage.as_deref())?;
            }
            Self::PreviewAssets { assets } => out.asset_blobs(assets)?,
            Self::PreviewStats(stats) => {
                out.u64(stats.frame_seq);
                out.u64(stats.source_revision);
                out.u8(stats.frame_mode as u8);
                out.u8(stats.proof_mode as u8);
                out.u32(stats.frames_per_second_milli);
                out.u32(stats.input_to_present_micros);
                out.u32(stats.render_micros);
                out.u32(stats.present_micros);
                out.u64(stats.missed_frames);
                out.u64(stats.dropped_snapshots);
                out.u32(stats.sample_age_millis);
                out.u64(stats.persistence_schema_version);
                out.u64(stats.persistence_durable_epoch);
                out.u64(stats.persistence_durable_turn);
                out.u32(stats.persistence_pending_turns);
                out.u32(stats.persistence_queue_depth);
                out.bool(stats.persistence_accepting);
                out.bool(stats.persistence_worker_alive);
                out.string(&stats.persistence_error)?;
            }
            Self::PreviewStatus {
                revision,
                ok,
                message,
            } => {
                out.u64(*revision);
                out.bool(*ok);
                out.string(message)?;
            }
            Self::PreviewRuntimeChanged {
                revision,
                runtime_sequence,
            } => {
                out.u64(*revision);
                out.u64(*runtime_sequence);
            }
            Self::PreviewTestResult {
                request_id,
                passed,
                message,
            } => {
                out.u64(*request_id);
                out.bool(*passed);
                out.string(message)?;
            }
            Self::DevInspect {
                request_id,
                revision,
                path,
            }
            | Self::PreviewInspect {
                request_id,
                revision,
                path,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.string(path)?;
            }
            Self::PreviewInspectResult {
                request_id,
                revision,
                runtime_sequence,
                path,
                ok,
                value,
                authority,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.u64(*runtime_sequence);
                out.string(path)?;
                out.bool(*ok);
                out.string(value)?;
                match authority.as_ref() {
                    Some(selection) => {
                        out.u8(1);
                        out.authority_selection(selection)?;
                    }
                    None => out.u8(0),
                }
            }
            Self::DevMigrationCommand {
                request_id,
                revision,
                command,
            }
            | Self::PreviewMigrationCommand {
                request_id,
                revision,
                command,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.migration_command(command)?;
            }
            Self::PreviewMigrationStatus(status) => out.migration_status(status)?,
            Self::DevPersistenceCommand {
                request_id,
                revision,
                command,
            }
            | Self::PreviewPersistenceCommand {
                request_id,
                revision,
                command,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.persistence_command(command)?;
            }
            Self::PreviewPersistenceSnapshot(snapshot) => {
                out.persistence_snapshot(snapshot)?;
            }
            Self::PreviewPersistenceArtifact {
                request_id,
                revision,
                artifact,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.state_artifact(artifact)?;
            }
            Self::Shutdown => {}
        }
        Ok(())
    }

    fn decode(tag: u8, input: &mut Decoder<'_>) -> Result<Self, ProtocolError> {
        let message = match tag {
            1 => Self::Hello {
                role: Role::decode(input.u8()?)?,
                pid: input.u32()?,
            },
            2 => Self::Ready {
                role: Role::decode(input.u8()?)?,
            },
            3 => Self::Catalog {
                entries: input.catalog()?,
                active_id: input.string()?,
            },
            4 => Self::OpenEditor {
                example_id: input.string()?,
                label: input.string()?,
                application: input.application_identity()?,
                revision: input.u64()?,
                units: input.source_units()?,
                migration: input.optional_migration_bundle()?,
                migration_stage: input.optional_string()?,
            },
            5 => Self::DevSelectExample {
                example_id: input.string()?,
            },
            6 => Self::DevSourceChanged {
                application: input.application_identity()?,
                revision: input.u64()?,
                units: input.source_units()?,
            },
            7 => Self::DevRun {
                application: input.application_identity()?,
                revision: input.u64()?,
                units: input.source_units()?,
            },
            8 => Self::DevReset,
            9 => Self::DevTest {
                request_id: input.u64()?,
                application: input.application_identity()?,
                revision: input.u64()?,
                units: input.source_units()?,
            },
            10 => Self::PreviewApply {
                intent: PreviewIntent::decode(input.u8()?)?,
                request_id: input.optional_u64()?,
                revision: input.u64()?,
                source: input.preview_source()?,
                test_steps: input.test_steps()?,
                migration: input.optional_migration_bundle()?,
                migration_stage: input.optional_string()?,
            },
            11 => Self::PreviewStats(PreviewStats {
                frame_seq: input.u64()?,
                source_revision: input.u64()?,
                frame_mode: FrameMode::decode(input.u8()?)?,
                proof_mode: ProofMode::decode(input.u8()?)?,
                frames_per_second_milli: input.u32()?,
                input_to_present_micros: input.u32()?,
                render_micros: input.u32()?,
                present_micros: input.u32()?,
                missed_frames: input.u64()?,
                dropped_snapshots: input.u64()?,
                sample_age_millis: input.u32()?,
                persistence_schema_version: input.u64()?,
                persistence_durable_epoch: input.u64()?,
                persistence_durable_turn: input.u64()?,
                persistence_pending_turns: input.u32()?,
                persistence_queue_depth: input.u32()?,
                persistence_accepting: input.bool()?,
                persistence_worker_alive: input.bool()?,
                persistence_error: input.string()?,
            }),
            12 => Self::PreviewStatus {
                revision: input.u64()?,
                ok: input.bool()?,
                message: input.string()?,
            },
            13 => Self::PreviewTestResult {
                request_id: input.u64()?,
                passed: input.bool()?,
                message: input.string()?,
            },
            14 => Self::Shutdown,
            15 => Self::DevInspect {
                request_id: input.u64()?,
                revision: input.u64()?,
                path: input.string()?,
            },
            16 => Self::PreviewInspect {
                request_id: input.u64()?,
                revision: input.u64()?,
                path: input.string()?,
            },
            17 => Self::PreviewInspectResult {
                request_id: input.u64()?,
                revision: input.u64()?,
                runtime_sequence: input.u64()?,
                path: input.string()?,
                ok: input.bool()?,
                value: input.string()?,
                authority: match input.u8()? {
                    0 => None,
                    1 => Some(input.authority_selection()?),
                    value => return Err(ProtocolError::InvalidOption(value)),
                },
            },
            18 => Self::PreviewRuntimeChanged {
                revision: input.u64()?,
                runtime_sequence: input.u64()?,
            },
            19 => Self::PreviewAssets {
                assets: input.asset_blobs()?,
            },
            20 => Self::DevMigrationCommand {
                request_id: input.u64()?,
                revision: input.u64()?,
                command: input.migration_command()?,
            },
            21 => Self::PreviewMigrationCommand {
                request_id: input.u64()?,
                revision: input.u64()?,
                command: input.migration_command()?,
            },
            22 => Self::PreviewMigrationStatus(input.migration_status()?),
            23 => Self::DevPersistenceCommand {
                request_id: input.u64()?,
                revision: input.u64()?,
                command: PersistenceCommand::decode(input)?,
            },
            24 => Self::PreviewPersistenceCommand {
                request_id: input.u64()?,
                revision: input.u64()?,
                command: PersistenceCommand::decode(input)?,
            },
            25 => Self::PreviewPersistenceSnapshot(Box::new(input.persistence_snapshot()?)),
            26 => Self::PreviewPersistenceArtifact {
                request_id: input.u64()?,
                revision: input.u64()?,
                artifact: input.state_artifact()?,
            },
            _ => return Err(ProtocolError::UnknownMessage(tag)),
        };
        input.finish()?;
        Ok(message)
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    FrameTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownMessage(u8),
    InvalidEnum(&'static str, u8),
    InvalidBool(u8),
    InvalidOption(u8),
    InvalidUtf8(std::str::Utf8Error),
    InvalidMigration(String),
    InvalidPersistence(String),
    InvalidTest(String),
    LimitExceeded(&'static str, usize),
    Truncated,
    TrailingBytes(usize),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "IPC I/O failed: {error}"),
            Self::FrameTooLarge(bytes) => write!(f, "IPC frame is too large: {bytes} bytes"),
            Self::InvalidMagic => f.write_str("IPC frame has invalid magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "IPC protocol version {version} is unsupported")
            }
            Self::UnknownMessage(tag) => write!(f, "IPC message tag {tag} is unknown"),
            Self::InvalidEnum(name, value) => write!(f, "IPC {name} value {value} is invalid"),
            Self::InvalidBool(value) => write!(f, "IPC bool value {value} is invalid"),
            Self::InvalidOption(value) => write!(f, "IPC option value {value} is invalid"),
            Self::InvalidUtf8(error) => write!(f, "IPC string is not UTF-8: {error}"),
            Self::InvalidMigration(message) => {
                write!(f, "IPC migration data is invalid: {message}")
            }
            Self::InvalidPersistence(message) => {
                write!(f, "IPC persistence data is invalid: {message}")
            }
            Self::InvalidTest(message) => write!(f, "IPC TEST data is invalid: {message}"),
            Self::LimitExceeded(name, value) => {
                write!(f, "IPC {name} exceeds its limit: {value}")
            }
            Self::Truncated => f.write_str("IPC frame is truncated"),
            Self::TrailingBytes(bytes) => write!(f, "IPC frame has {bytes} trailing bytes"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidUtf8(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    pub fn new(stream: UnixStream) -> Self {
        Self { stream }
    }

    pub fn connect(path: &Path, role: Role) -> Result<Self, ProtocolError> {
        let mut connection = Self::new(UnixStream::connect(path)?);
        connection.send(&Message::Hello {
            role,
            pid: std::process::id(),
        })?;
        Ok(connection)
    }

    pub fn try_clone(&self) -> Result<Self, ProtocolError> {
        Ok(Self::new(self.stream.try_clone()?))
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ProtocolError> {
        self.stream.set_read_timeout(timeout)?;
        Ok(())
    }

    pub fn send(&mut self, message: &Message) -> Result<(), ProtocolError> {
        write_message(&mut self.stream, message)
    }

    pub fn receive(&mut self) -> Result<Option<Message>, ProtocolError> {
        read_message(&mut self.stream)
    }
}

pub fn write_message(writer: &mut impl Write, message: &Message) -> Result<(), ProtocolError> {
    let mut body = Encoder::default();
    body.bytes.extend_from_slice(&MAGIC);
    body.u16(VERSION);
    body.u8(message.tag());
    message.encode_payload(&mut body)?;
    if body.bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(body.bytes.len()));
    }
    writer.write_all(&(body.bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&body.bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message(reader: &mut impl Read) -> Result<Option<Message>, ProtocolError> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length[..1]) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if !(HEADER_BYTES..=MAX_FRAME_BYTES).contains(&length) {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    if body[..MAGIC.len()] != MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    let version = u16::from_le_bytes([body[4], body[5]]);
    if version != VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    let tag = body[6];
    let mut input = Decoder::new(&body[HEADER_BYTES..]);
    Message::decode(tag, &mut input).map(Some)
}

#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u64(value);
            }
            None => self.u8(0),
        }
    }

    fn string(&mut self, value: &str) -> Result<(), ProtocolError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded("string bytes", value.len()));
        }
        let projected = self
            .bytes
            .len()
            .checked_add(4)
            .and_then(|length| length.checked_add(value.len()))
            .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
        if projected > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(projected));
        }
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn source_units(&mut self, units: &[SourceUnit]) -> Result<(), ProtocolError> {
        if units.len() > MAX_SOURCE_UNITS {
            return Err(ProtocolError::LimitExceeded(
                "source unit count",
                units.len(),
            ));
        }
        self.u32(units.len() as u32);
        for unit in units {
            self.string(&unit.path)?;
            self.string(&unit.source)?;
        }
        Ok(())
    }

    fn program_sources(&mut self, programs: &[ProgramSource]) -> Result<(), ProtocolError> {
        if programs.len() > MAX_DISTRIBUTED_PROGRAMS {
            return Err(ProtocolError::LimitExceeded(
                "distributed program count",
                programs.len(),
            ));
        }
        self.u32(programs.len() as u32);
        for program in programs {
            self.u8(match program.role {
                ProgramRole::Client => 1,
                ProgramRole::Session => 2,
                ProgramRole::Server => 3,
            });
            self.string(&program.entry_path)?;
            self.source_units(&program.units)?;
            self.application_identity(&program.application)?;
        }
        Ok(())
    }

    fn preview_source(&mut self, source: &PreviewSource) -> Result<(), ProtocolError> {
        match source {
            PreviewSource::BuiltInSingleRole { application, units } => {
                self.u8(1);
                self.application_identity(application)?;
                self.source_units(units)
            }
            PreviewSource::DistributedPackage { programs } => {
                self.u8(2);
                self.program_sources(programs)
            }
        }
    }

    fn application_identity(
        &mut self,
        application: &ApplicationIdentity,
    ) -> Result<(), ProtocolError> {
        self.string(&application.package_id)?;
        self.string(&application.state_namespace)?;
        self.string(&application.deployment_domain)
    }

    fn optional_migration_bundle(
        &mut self,
        migration: Option<&MigrationBundle>,
    ) -> Result<(), ProtocolError> {
        match migration {
            Some(migration) => {
                self.u8(1);
                self.migration_bundle(migration)
            }
            None => {
                self.u8(0);
                Ok(())
            }
        }
    }

    fn migration_bundle(&mut self, migration: &MigrationBundle) -> Result<(), ProtocolError> {
        migration.validate()?;
        self.string(&migration.initial_stage)?;
        self.string(&migration.launch_stage)?;
        self.u8(match migration.test_driver {
            MigrationTestDriver::Migration => 1,
            MigrationTestDriver::Example => 2,
        });
        self.string(&migration.scenario_path)?;
        self.u32(migration.stages.len() as u32);
        for stage in &migration.stages {
            self.string(&stage.id)?;
            self.string(&stage.label)?;
            self.u64(stage.schema_version);
            self.string(&stage.source)?;
            self.string_slice(&stage.source_files, "migration source file count")?;
            self.source_units(&stage.units)?;
        }
        let scenario = toml::to_string(&migration.scenario)
            .map_err(|error| ProtocolError::InvalidMigration(error.to_string()))?;
        if scenario.len() > MAX_MIGRATION_SCENARIO_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "migration scenario bytes",
                scenario.len(),
            ));
        }
        self.string(&scenario)
    }

    fn migration_command(&mut self, command: &MigrationCommand) -> Result<(), ProtocolError> {
        match command {
            MigrationCommand::Preview { stage_id } => {
                validate_migration_id("migration preview stage", stage_id)?;
                self.u8(1);
                self.string(stage_id)
            }
            MigrationCommand::Activate { stage_id } => {
                validate_migration_id("migration activation stage", stage_id)?;
                self.u8(2);
                self.string(stage_id)
            }
            MigrationCommand::Restart => {
                self.u8(3);
                Ok(())
            }
            MigrationCommand::StartOver { confirmed } => {
                self.u8(4);
                self.bool(*confirmed);
                Ok(())
            }
        }
    }

    fn migration_status(&mut self, status: &MigrationStatus) -> Result<(), ProtocolError> {
        validate_migration_id("active migration stage", &status.active_stage)?;
        if let Some(stage) = status.previewed_stage.as_deref() {
            validate_migration_id("previewed migration stage", stage)?;
        }
        if let Some(stage) = status.target_stage.as_deref() {
            validate_migration_id("target migration stage", stage)?;
        }
        self.optional_u64(status.request_id);
        self.u64(status.revision);
        self.u8(status.operation as u8);
        self.bool(status.ok);
        self.string(&status.active_stage)?;
        self.optional_string(status.previewed_stage.as_deref())?;
        self.optional_string(status.target_stage.as_deref())?;
        self.u64(status.target_schema_version);
        self.u32(status.migration_step_count);
        self.u32(status.deleted_memory_count);
        self.string(&status.message)
    }

    fn persistence_command(&mut self, command: &PersistenceCommand) -> Result<(), ProtocolError> {
        match command {
            PersistenceCommand::Flush => self.u8(1),
            PersistenceCommand::Compact => self.u8(2),
            PersistenceCommand::ClearAll { confirmed } => {
                self.u8(3);
                self.bool(*confirmed);
            }
            PersistenceCommand::ExportState => self.u8(4),
            PersistenceCommand::ImportPreview { artifact } => {
                self.u8(5);
                self.state_artifact(artifact)?;
            }
            PersistenceCommand::ActivateImport { preview_id } => {
                self.u8(6);
                self.u64(*preview_id);
            }
            PersistenceCommand::ClearSelected {
                selection,
                confirmed,
            } => {
                self.u8(7);
                self.authority_selection(selection)?;
                self.bool(*confirmed);
            }
        }
        Ok(())
    }

    fn persistence_snapshot(
        &mut self,
        snapshot: &PersistenceSnapshot,
    ) -> Result<(), ProtocolError> {
        snapshot.validate()?;
        self.u64(snapshot.snapshot_sequence);
        self.u64(snapshot.revision);
        self.application_identity(&snapshot.application)?;
        self.u64(snapshot.schema_version);
        self.digest(snapshot.schema_hash);

        self.u64(snapshot.authority.runtime_turn_sequence);
        self.u64(snapshot.authority.source_event_sequence);
        self.u32(snapshot.authority.scalar_count);
        self.u32(snapshot.authority.indexed_field_count);
        self.u32(snapshot.authority.list_count);
        self.u32(snapshot.authority.effect_contract_count);

        match snapshot.stored.as_ref() {
            Some(stored) => {
                self.u8(1);
                self.u64(stored.epoch);
                self.u64(stored.through_turn_sequence);
                self.u32(stored.scalar_count);
                self.u32(stored.list_count);
                self.u64(stored.row_count);
                self.u32(stored.content_artifact_count);
                self.u64(stored.content_artifact_bytes);
                self.optional_u64(stored.encoded_value_bytes);
                self.u32(stored.completed_migration_count);
            }
            None => self.u8(0),
        }

        self.optional_u64(snapshot.pending.first_turn_sequence);
        self.optional_u64(snapshot.pending.last_turn_sequence);
        self.u64(snapshot.pending.oldest_age_millis);
        self.u64(snapshot.pending.turn_count);
        self.u32(snapshot.pending.queue_depth);
        self.u32(snapshot.pending.reserved_slots);
        self.bool(snapshot.pending.accepting_turns);

        self.u64(snapshot.durable.epoch);
        self.u64(snapshot.durable.through_turn_sequence);

        self.u64(snapshot.timings.authority_enqueue_us);
        self.u64(snapshot.timings.encode_us);
        self.u64(snapshot.timings.checkpoint_us);
        self.u64(snapshot.timings.barrier_us);
        self.u64(snapshot.timings.restore_us);
        self.u64(snapshot.timings.migration_us);
        self.u64(snapshot.timings.rebuild_derived_us);

        self.u32(snapshot.outbox.pending_count);
        self.u32(snapshot.outbox.dispatching_count);
        self.u32(snapshot.outbox.reconciliation_count);
        self.u32(snapshot.outbox.completed_count);
        self.u32(snapshot.outbox.samples.len() as u32);
        for sample in &snapshot.outbox.samples {
            self.digest(sample.item_id);
            self.digest(sample.invocation_id);
            self.digest(sample.effect_id);
            self.u8(sample.state as u8);
            self.u32(sample.attempt);
            self.u64(sample.created_turn_sequence);
            self.u64(sample.updated_turn_sequence);
        }

        self.bool(snapshot.worker_alive);
        for capability in [
            &snapshot.capabilities.clear_selected,
            &snapshot.capabilities.export_state,
            &snapshot.capabilities.import_preview,
            &snapshot.capabilities.activate_import,
        ] {
            self.bool(capability.available);
            self.bounded_persistence_string(&capability.reason)?;
        }
        match snapshot.import_preview.as_ref() {
            Some(preview) => {
                self.u8(1);
                self.u64(preview.preview_id);
                self.u64(preview.source_schema_version);
                self.u64(preview.target_schema_version);
                self.u32(preview.scalar_count);
                self.u32(preview.list_count);
                self.u64(preview.row_count);
                self.u32(preview.migration_step_count);
                self.u32(preview.deleted_memory_count);
                self.u32(preview.document_node_count);
                self.u64(preview.baseline_runtime_turn_sequence);
                self.u64(preview.baseline_durable_epoch);
                self.u64(preview.baseline_durable_turn_sequence);
            }
            None => self.u8(0),
        }
        self.optional_bounded_string(snapshot.last_actionable_error.as_deref())?;
        match snapshot.last_operation.as_ref() {
            Some(operation) => {
                self.u8(1);
                self.u64(operation.request_id);
                self.u8(operation.operation as u8);
                self.bool(operation.ok);
                self.bounded_persistence_string(&operation.message)?;
            }
            None => self.u8(0),
        }
        Ok(())
    }

    fn authority_selection(&mut self, selection: &AuthoritySelection) -> Result<(), ProtocolError> {
        selection.validate()?;
        self.string(&selection.semantic_path)?;
        self.digest(selection.memory_id);
        self.u8(selection.kind as u8);
        match selection.row {
            Some((key, generation)) => {
                self.u8(1);
                self.u64(key);
                self.u64(generation);
            }
            None => self.u8(0),
        }
        match selection.leaf_id {
            Some(leaf_id) => {
                self.u8(1);
                self.digest(leaf_id);
            }
            None => self.u8(0),
        }
        Ok(())
    }

    fn state_artifact(&mut self, artifact: &CanonicalStateArtifact) -> Result<(), ProtocolError> {
        artifact.validate()?;
        self.u8(artifact.format as u8);
        self.u64(artifact.schema_version);
        self.digest(artifact.sha256);
        self.u32(artifact.bytes.len() as u32);
        self.bytes.extend_from_slice(&artifact.bytes);
        Ok(())
    }

    fn digest(&mut self, value: [u8; 32]) {
        self.bytes.extend_from_slice(&value);
    }

    fn optional_bounded_string(&mut self, value: Option<&str>) -> Result<(), ProtocolError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.bounded_persistence_string(value)
            }
            None => {
                self.u8(0);
                Ok(())
            }
        }
    }

    fn bounded_persistence_string(&mut self, value: &str) -> Result<(), ProtocolError> {
        if value.len() > MAX_PERSISTENCE_STATUS_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "persistence status bytes",
                value.len(),
            ));
        }
        self.string(value)
    }

    fn string_slice(
        &mut self,
        values: &[String],
        limit_name: &'static str,
    ) -> Result<(), ProtocolError> {
        if values.len() > MAX_MIGRATION_SOURCE_FILES {
            return Err(ProtocolError::LimitExceeded(limit_name, values.len()));
        }
        self.u32(values.len() as u32);
        for value in values {
            self.string(value)?;
        }
        Ok(())
    }

    fn asset_blobs(&mut self, assets: &[AssetBlob]) -> Result<(), ProtocolError> {
        if assets.len() > MAX_ASSET_BLOBS {
            return Err(ProtocolError::LimitExceeded("asset count", assets.len()));
        }
        self.u32(assets.len() as u32);
        for asset in assets {
            self.string(&asset.url)?;
            self.string(&asset.media_type)?;
            self.string(&asset.sha256)?;
            if asset.bytes.len() > MAX_ASSET_BLOB_BYTES {
                return Err(ProtocolError::LimitExceeded(
                    "asset blob bytes",
                    asset.bytes.len(),
                ));
            }
            self.u32(asset.bytes.len() as u32);
            self.bytes.extend_from_slice(&asset.bytes);
        }
        Ok(())
    }

    fn catalog(&mut self, entries: &[CatalogItem]) -> Result<(), ProtocolError> {
        if entries.len() > MAX_CATALOG_ENTRIES {
            return Err(ProtocolError::LimitExceeded(
                "catalog entry count",
                entries.len(),
            ));
        }
        self.u32(entries.len() as u32);
        for entry in entries {
            self.string(&entry.id)?;
            self.string(&entry.label)?;
            self.bool(entry.custom);
        }
        Ok(())
    }

    fn test_steps(&mut self, steps: &[TestStep]) -> Result<(), ProtocolError> {
        if steps.len() > MAX_TEST_STEPS {
            return Err(ProtocolError::LimitExceeded("test step count", steps.len()));
        }
        self.u32(steps.len() as u32);
        for step in steps {
            self.string(&step.id)?;
            self.string(&step.source_path)?;
            self.optional_string(step.action_kind.as_deref())?;
            self.optional_string(step.target_text.as_deref())?;
            self.optional_string(step.text.as_deref())?;
            self.optional_string(step.key.as_deref())?;
            self.optional_string(step.address.as_deref())?;
            self.optional_u64(step.target_key);
            self.optional_u64(step.target_generation);
            self.optional_u64(step.target_occurrence);
            self.optional_string(step.pointer_x.as_deref())?;
            self.optional_string(step.pointer_y.as_deref())?;
            self.optional_string(step.pointer_width.as_deref())?;
            self.optional_string(step.pointer_height.as_deref())?;
            self.test_expectations(&step.expectations)?;
        }
        Ok(())
    }

    fn test_expectations(
        &mut self,
        expectations: &[ScenarioExpectation],
    ) -> Result<(), ProtocolError> {
        if expectations.len() > MAX_TEST_EXPECTATIONS_PER_STEP {
            return Err(ProtocolError::LimitExceeded(
                "test expectations per step",
                expectations.len(),
            ));
        }
        self.u32(expectations.len() as u32);
        for expectation in expectations {
            match expectation {
                ScenarioExpectation::RootText { name, value } => {
                    self.u8(1);
                    self.string(name)?;
                    self.string(value)?;
                }
                ScenarioExpectation::RootNonEmpty { name } => {
                    self.u8(9);
                    self.string(name)?;
                }
                ScenarioExpectation::ListTexts {
                    list,
                    field,
                    filter,
                    values,
                } => {
                    self.u8(2);
                    self.string(list)?;
                    self.string(field)?;
                    self.optional_test_field_match(filter.as_ref())?;
                    self.test_expectation_strings(values)?;
                }
                ScenarioExpectation::RootRowTexts {
                    root,
                    field,
                    values,
                } => {
                    self.u8(3);
                    self.string(root)?;
                    self.string(field)?;
                    self.test_expectation_strings(values)?;
                }
                ScenarioExpectation::ListCount {
                    list,
                    filter,
                    count,
                } => {
                    self.u8(4);
                    self.string(list)?;
                    self.test_field_match(filter)?;
                    self.u64(
                        (*count)
                            .try_into()
                            .map_err(|_| ProtocolError::LimitExceeded("test list count", *count))?,
                    );
                }
                ScenarioExpectation::RowFields {
                    list,
                    key_field,
                    key,
                    fields,
                } => {
                    self.u8(5);
                    self.string(list)?;
                    self.string(key_field)?;
                    self.string(key)?;
                    if fields.len() > MAX_TEST_EXPECTATION_FIELDS {
                        return Err(ProtocolError::LimitExceeded(
                            "test expectation field count",
                            fields.len(),
                        ));
                    }
                    self.u32(fields.len() as u32);
                    for (name, value) in fields {
                        self.string(name)?;
                        self.string(value)?;
                    }
                }
                ScenarioExpectation::RecomputedRows {
                    list,
                    key_field,
                    field,
                    keys,
                } => {
                    self.u8(6);
                    self.string(list)?;
                    self.string(key_field)?;
                    self.string(field)?;
                    self.test_expectation_strings(keys)?;
                }
                ScenarioExpectation::SemanticDeltaContains(value) => {
                    self.u8(7);
                    self.string(value)?;
                }
                ScenarioExpectation::DocumentChanged => self.u8(8),
            }
        }
        Ok(())
    }

    fn optional_test_field_match(
        &mut self,
        value: Option<&ScenarioFieldMatch>,
    ) -> Result<(), ProtocolError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.test_field_match(value)
            }
            None => {
                self.u8(0);
                Ok(())
            }
        }
    }

    fn test_field_match(&mut self, value: &ScenarioFieldMatch) -> Result<(), ProtocolError> {
        self.string(&value.field)?;
        self.string(&value.value)
    }

    fn test_expectation_strings(&mut self, values: &[String]) -> Result<(), ProtocolError> {
        if values.len() > MAX_TEST_EXPECTATION_VALUES {
            return Err(ProtocolError::LimitExceeded(
                "test expectation value count",
                values.len(),
            ));
        }
        self.u32(values.len() as u32);
        for value in values {
            self.string(value)?;
        }
        Ok(())
    }

    fn optional_string(&mut self, value: Option<&str>) -> Result<(), ProtocolError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value)?;
            }
            None => self.u8(0),
        }
        Ok(())
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ProtocolError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(ProtocolError::InvalidBool(value)),
        }
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u64().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn string(&mut self) -> Result<String, ProtocolError> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded("string bytes", length));
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(ProtocolError::InvalidUtf8)?;
        Ok(value.to_owned())
    }

    fn source_units(&mut self) -> Result<Vec<SourceUnit>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_SOURCE_UNITS {
            return Err(ProtocolError::LimitExceeded("source unit count", count));
        }
        (0..count)
            .map(|_| {
                Ok(SourceUnit {
                    path: self.string()?,
                    source: self.string()?,
                })
            })
            .collect()
    }

    fn program_sources(&mut self) -> Result<Vec<ProgramSource>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_DISTRIBUTED_PROGRAMS {
            return Err(ProtocolError::LimitExceeded(
                "distributed program count",
                count,
            ));
        }
        (0..count)
            .map(|_| {
                let role = match self.u8()? {
                    1 => ProgramRole::Client,
                    2 => ProgramRole::Session,
                    3 => ProgramRole::Server,
                    value => return Err(ProtocolError::InvalidEnum("program role", value)),
                };
                Ok(ProgramSource {
                    role,
                    entry_path: self.string()?,
                    units: self.source_units()?,
                    application: self.application_identity()?,
                })
            })
            .collect()
    }

    fn preview_source(&mut self) -> Result<PreviewSource, ProtocolError> {
        match self.u8()? {
            1 => Ok(PreviewSource::BuiltInSingleRole {
                application: self.application_identity()?,
                units: self.source_units()?,
            }),
            2 => Ok(PreviewSource::DistributedPackage {
                programs: self.program_sources()?,
            }),
            value => Err(ProtocolError::InvalidEnum("preview source", value)),
        }
    }

    fn application_identity(&mut self) -> Result<ApplicationIdentity, ProtocolError> {
        Ok(ApplicationIdentity::new(
            self.string()?,
            self.string()?,
            self.string()?,
        ))
    }

    fn optional_migration_bundle(&mut self) -> Result<Option<MigrationBundle>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.migration_bundle().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn migration_bundle(&mut self) -> Result<MigrationBundle, ProtocolError> {
        let initial_stage = self.string()?;
        let launch_stage = self.string()?;
        let test_driver = match self.u8()? {
            1 => MigrationTestDriver::Migration,
            2 => MigrationTestDriver::Example,
            value => return Err(ProtocolError::InvalidEnum("migration test driver", value)),
        };
        let scenario_path = self.string()?;
        let count = self.u32()? as usize;
        if count > MAX_MIGRATION_STAGES {
            return Err(ProtocolError::LimitExceeded("migration stage count", count));
        }
        let mut stages = Vec::with_capacity(count);
        for _ in 0..count {
            stages.push(MigrationStage {
                id: self.string()?,
                label: self.string()?,
                schema_version: self.u64()?,
                source: self.string()?,
                source_files: self
                    .string_vec(MAX_MIGRATION_SOURCE_FILES, "migration source file count")?,
                units: self.source_units()?,
            });
        }
        let encoded_scenario = self.string()?;
        if encoded_scenario.len() > MAX_MIGRATION_SCENARIO_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "migration scenario bytes",
                encoded_scenario.len(),
            ));
        }
        let scenario = toml::from_str(&encoded_scenario)
            .map_err(|error| ProtocolError::InvalidMigration(error.to_string()))?;
        let migration = MigrationBundle {
            initial_stage,
            launch_stage,
            test_driver,
            scenario_path,
            stages,
            scenario,
        };
        migration.validate()?;
        Ok(migration)
    }

    fn migration_command(&mut self) -> Result<MigrationCommand, ProtocolError> {
        match self.u8()? {
            1 => {
                let stage_id = self.string()?;
                validate_migration_id("migration preview stage", &stage_id)?;
                Ok(MigrationCommand::Preview { stage_id })
            }
            2 => {
                let stage_id = self.string()?;
                validate_migration_id("migration activation stage", &stage_id)?;
                Ok(MigrationCommand::Activate { stage_id })
            }
            3 => Ok(MigrationCommand::Restart),
            4 => Ok(MigrationCommand::StartOver {
                confirmed: self.bool()?,
            }),
            value => Err(ProtocolError::InvalidEnum("migration command", value)),
        }
    }

    fn migration_status(&mut self) -> Result<MigrationStatus, ProtocolError> {
        let status = MigrationStatus {
            request_id: self.optional_u64()?,
            revision: self.u64()?,
            operation: MigrationOperation::decode(self.u8()?)?,
            ok: self.bool()?,
            active_stage: self.string()?,
            previewed_stage: self.optional_string()?,
            target_stage: self.optional_string()?,
            target_schema_version: self.u64()?,
            migration_step_count: self.u32()?,
            deleted_memory_count: self.u32()?,
            message: self.string()?,
        };
        validate_migration_id("active migration stage", &status.active_stage)?;
        if let Some(stage) = status.previewed_stage.as_deref() {
            validate_migration_id("previewed migration stage", stage)?;
        }
        if let Some(stage) = status.target_stage.as_deref() {
            validate_migration_id("target migration stage", stage)?;
        }
        Ok(status)
    }

    fn persistence_snapshot(&mut self) -> Result<PersistenceSnapshot, ProtocolError> {
        let snapshot_sequence = self.u64()?;
        let revision = self.u64()?;
        let application = self.application_identity()?;
        let schema_version = self.u64()?;
        let schema_hash = self.digest()?;
        let authority = AuthoritySummary {
            runtime_turn_sequence: self.u64()?,
            source_event_sequence: self.u64()?,
            scalar_count: self.u32()?,
            indexed_field_count: self.u32()?,
            list_count: self.u32()?,
            effect_contract_count: self.u32()?,
        };
        let stored = match self.u8()? {
            0 => None,
            1 => Some(StoredSummary {
                epoch: self.u64()?,
                through_turn_sequence: self.u64()?,
                scalar_count: self.u32()?,
                list_count: self.u32()?,
                row_count: self.u64()?,
                content_artifact_count: self.u32()?,
                content_artifact_bytes: self.u64()?,
                encoded_value_bytes: self.optional_u64()?,
                completed_migration_count: self.u32()?,
            }),
            value => return Err(ProtocolError::InvalidOption(value)),
        };
        let pending = PendingSummary {
            first_turn_sequence: self.optional_u64()?,
            last_turn_sequence: self.optional_u64()?,
            oldest_age_millis: self.u64()?,
            turn_count: self.u64()?,
            queue_depth: self.u32()?,
            reserved_slots: self.u32()?,
            accepting_turns: self.bool()?,
        };
        let durable = DurableSummary {
            epoch: self.u64()?,
            through_turn_sequence: self.u64()?,
        };
        let timings = PersistenceTimingSummary {
            authority_enqueue_us: self.u64()?,
            encode_us: self.u64()?,
            checkpoint_us: self.u64()?,
            barrier_us: self.u64()?,
            restore_us: self.u64()?,
            migration_us: self.u64()?,
            rebuild_derived_us: self.u64()?,
        };
        let pending_count = self.u32()?;
        let dispatching_count = self.u32()?;
        let reconciliation_count = self.u32()?;
        let completed_count = self.u32()?;
        let sample_count = self.u32()? as usize;
        if sample_count > MAX_PERSISTENCE_OUTBOX_SAMPLES {
            return Err(ProtocolError::LimitExceeded(
                "persistence outbox sample count",
                sample_count,
            ));
        }
        let mut samples = Vec::with_capacity(sample_count);
        for _ in 0..sample_count {
            samples.push(OutboxSample {
                item_id: self.digest()?,
                invocation_id: self.digest()?,
                effect_id: self.digest()?,
                state: OutboxSampleState::decode(self.u8()?)?,
                attempt: self.u32()?,
                created_turn_sequence: self.u64()?,
                updated_turn_sequence: self.u64()?,
            });
        }
        let worker_alive = self.bool()?;
        let mut capability = || -> Result<PersistenceCapability, ProtocolError> {
            Ok(PersistenceCapability {
                available: self.bool()?,
                reason: self.bounded_persistence_string()?,
            })
        };
        let capabilities = PersistenceCapabilities {
            clear_selected: capability()?,
            export_state: capability()?,
            import_preview: capability()?,
            activate_import: capability()?,
        };
        let import_preview = match self.u8()? {
            0 => None,
            1 => Some(StateArtifactPreviewSummary {
                preview_id: self.u64()?,
                source_schema_version: self.u64()?,
                target_schema_version: self.u64()?,
                scalar_count: self.u32()?,
                list_count: self.u32()?,
                row_count: self.u64()?,
                migration_step_count: self.u32()?,
                deleted_memory_count: self.u32()?,
                document_node_count: self.u32()?,
                baseline_runtime_turn_sequence: self.u64()?,
                baseline_durable_epoch: self.u64()?,
                baseline_durable_turn_sequence: self.u64()?,
            }),
            value => return Err(ProtocolError::InvalidOption(value)),
        };
        let last_actionable_error = self.optional_bounded_persistence_string()?;
        let last_operation = match self.u8()? {
            0 => None,
            1 => Some(PersistenceOperationStatus {
                request_id: self.u64()?,
                operation: PersistenceOperation::decode(self.u8()?)?,
                ok: self.bool()?,
                message: self.bounded_persistence_string()?,
            }),
            value => return Err(ProtocolError::InvalidOption(value)),
        };
        let snapshot = PersistenceSnapshot {
            snapshot_sequence,
            revision,
            application,
            schema_version,
            schema_hash,
            authority,
            stored,
            pending,
            durable,
            timings,
            outbox: OutboxSummary {
                pending_count,
                dispatching_count,
                reconciliation_count,
                completed_count,
                samples,
            },
            worker_alive,
            capabilities,
            import_preview,
            last_actionable_error,
            last_operation,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn authority_selection(&mut self) -> Result<AuthoritySelection, ProtocolError> {
        let semantic_path = self.string()?;
        let memory_id = self.digest()?;
        let kind = AuthoritySelectionKind::decode(self.u8()?)?;
        let row = match self.u8()? {
            0 => None,
            1 => Some((self.u64()?, self.u64()?)),
            value => return Err(ProtocolError::InvalidOption(value)),
        };
        let leaf_id = match self.u8()? {
            0 => None,
            1 => Some(self.digest()?),
            value => return Err(ProtocolError::InvalidOption(value)),
        };
        let selection = AuthoritySelection {
            semantic_path,
            memory_id,
            kind,
            row,
            leaf_id,
        };
        selection.validate()?;
        Ok(selection)
    }

    fn state_artifact(&mut self) -> Result<CanonicalStateArtifact, ProtocolError> {
        let format = StateArtifactFormat::decode(self.u8()?)?;
        let schema_version = self.u64()?;
        let sha256 = self.digest()?;
        let length = self.u32()? as usize;
        if length > MAX_PERSISTENCE_ARTIFACT_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "persistence artifact bytes",
                length,
            ));
        }
        let artifact = CanonicalStateArtifact {
            format,
            schema_version,
            sha256,
            bytes: self.take(length)?.to_vec(),
        };
        artifact.validate()?;
        Ok(artifact)
    }

    fn digest(&mut self) -> Result<[u8; 32], ProtocolError> {
        Ok(self.take(32)?.try_into().expect("32-byte digest"))
    }

    fn optional_bounded_persistence_string(&mut self) -> Result<Option<String>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.bounded_persistence_string().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn bounded_persistence_string(&mut self) -> Result<String, ProtocolError> {
        let value = self.string()?;
        if value.len() > MAX_PERSISTENCE_STATUS_BYTES {
            return Err(ProtocolError::LimitExceeded(
                "persistence status bytes",
                value.len(),
            ));
        }
        Ok(value)
    }

    fn string_vec(
        &mut self,
        max: usize,
        limit_name: &'static str,
    ) -> Result<Vec<String>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > max {
            return Err(ProtocolError::LimitExceeded(limit_name, count));
        }
        (0..count).map(|_| self.string()).collect()
    }

    fn asset_blobs(&mut self) -> Result<Vec<AssetBlob>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_ASSET_BLOBS {
            return Err(ProtocolError::LimitExceeded("asset count", count));
        }
        (0..count)
            .map(|_| {
                let url = self.string()?;
                let media_type = self.string()?;
                let sha256 = self.string()?;
                let length = self.u32()? as usize;
                if length > MAX_ASSET_BLOB_BYTES {
                    return Err(ProtocolError::LimitExceeded("asset blob bytes", length));
                }
                Ok(AssetBlob {
                    url,
                    media_type,
                    sha256,
                    bytes: self.take(length)?.to_vec(),
                })
            })
            .collect()
    }

    fn catalog(&mut self) -> Result<Vec<CatalogItem>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_CATALOG_ENTRIES {
            return Err(ProtocolError::LimitExceeded("catalog entry count", count));
        }
        (0..count)
            .map(|_| {
                Ok(CatalogItem {
                    id: self.string()?,
                    label: self.string()?,
                    custom: self.bool()?,
                })
            })
            .collect()
    }

    fn test_steps(&mut self) -> Result<Vec<TestStep>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_TEST_STEPS {
            return Err(ProtocolError::LimitExceeded("test step count", count));
        }
        (0..count)
            .map(|_| {
                Ok(TestStep {
                    id: self.string()?,
                    source_path: self.string()?,
                    action_kind: self.optional_string()?,
                    target_text: self.optional_string()?,
                    text: self.optional_string()?,
                    key: self.optional_string()?,
                    address: self.optional_string()?,
                    target_key: self.optional_u64()?,
                    target_generation: self.optional_u64()?,
                    target_occurrence: self.optional_u64()?,
                    pointer_x: self.optional_string()?,
                    pointer_y: self.optional_string()?,
                    pointer_width: self.optional_string()?,
                    pointer_height: self.optional_string()?,
                    expectations: self.test_expectations()?,
                })
            })
            .collect()
    }

    fn test_expectations(&mut self) -> Result<Vec<ScenarioExpectation>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_TEST_EXPECTATIONS_PER_STEP {
            return Err(ProtocolError::LimitExceeded(
                "test expectations per step",
                count,
            ));
        }
        (0..count)
            .map(|_| match self.u8()? {
                1 => Ok(ScenarioExpectation::RootText {
                    name: self.string()?,
                    value: self.string()?,
                }),
                2 => Ok(ScenarioExpectation::ListTexts {
                    list: self.string()?,
                    field: self.string()?,
                    filter: self.optional_test_field_match()?,
                    values: self.test_expectation_strings()?,
                }),
                3 => Ok(ScenarioExpectation::RootRowTexts {
                    root: self.string()?,
                    field: self.string()?,
                    values: self.test_expectation_strings()?,
                }),
                4 => {
                    let list = self.string()?;
                    let filter = self.test_field_match()?;
                    let count = usize::try_from(self.u64()?)
                        .map_err(|_| ProtocolError::LimitExceeded("test list count", usize::MAX))?;
                    Ok(ScenarioExpectation::ListCount {
                        list,
                        filter,
                        count,
                    })
                }
                5 => {
                    let list = self.string()?;
                    let key_field = self.string()?;
                    let key = self.string()?;
                    let field_count = self.u32()? as usize;
                    if field_count > MAX_TEST_EXPECTATION_FIELDS {
                        return Err(ProtocolError::LimitExceeded(
                            "test expectation field count",
                            field_count,
                        ));
                    }
                    let mut fields = std::collections::BTreeMap::new();
                    for _ in 0..field_count {
                        let name = self.string()?;
                        let value = self.string()?;
                        if fields.insert(name, value).is_some() {
                            return Err(ProtocolError::InvalidTest(
                                "duplicate test expectation field".to_owned(),
                            ));
                        }
                    }
                    Ok(ScenarioExpectation::RowFields {
                        list,
                        key_field,
                        key,
                        fields,
                    })
                }
                6 => Ok(ScenarioExpectation::RecomputedRows {
                    list: self.string()?,
                    key_field: self.string()?,
                    field: self.string()?,
                    keys: self.test_expectation_strings()?,
                }),
                7 => Ok(ScenarioExpectation::SemanticDeltaContains(self.string()?)),
                8 => Ok(ScenarioExpectation::DocumentChanged),
                9 => Ok(ScenarioExpectation::RootNonEmpty {
                    name: self.string()?,
                }),
                value => Err(ProtocolError::InvalidEnum("test expectation", value)),
            })
            .collect()
    }

    fn optional_test_field_match(&mut self) -> Result<Option<ScenarioFieldMatch>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.test_field_match().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn test_field_match(&mut self) -> Result<ScenarioFieldMatch, ProtocolError> {
        Ok(ScenarioFieldMatch {
            field: self.string()?,
            value: self.string()?,
        })
    }

    fn test_expectation_strings(&mut self) -> Result<Vec<String>, ProtocolError> {
        self.string_vec(MAX_TEST_EXPECTATION_VALUES, "test expectation value count")
    }

    fn optional_string(&mut self) -> Result<Option<String>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn finish(&self) -> Result<(), ProtocolError> {
        let remaining = self.bytes.len() - self.offset;
        if remaining == 0 {
            Ok(())
        } else {
            Err(ProtocolError::TrailingBytes(remaining))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application() -> ApplicationIdentity {
        ApplicationIdentity::new(
            "dev.boon.example.counter",
            "builtin:example:counter",
            "builtin",
        )
    }

    fn units() -> Vec<SourceUnit> {
        vec![
            SourceUnit {
                path: "examples/main.bn".to_owned(),
                source: "value: 42\n".to_owned(),
            },
            SourceUnit {
                path: "examples/view.bn".to_owned(),
                source: "view: Text[text: value]\n".to_owned(),
            },
        ]
    }

    fn program_sources() -> Vec<ProgramSource> {
        [
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ]
        .into_iter()
        .map(|role| ProgramSource {
            role,
            entry_path: format!("{}/RUN.bn", role.as_str()),
            units: vec![SourceUnit {
                path: format!("{}/RUN.bn", role.as_str()),
                source: format!("value: TEXT {{ {} }}\n", role.as_str()),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.distributed",
                format!("distributed:{}", role.as_str()),
                "test",
            ),
        })
        .collect()
    }

    fn roundtrip(message: Message) {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).expect("encode message");
        let decoded = read_message(&mut bytes.as_slice())
            .expect("decode message")
            .expect("message");
        assert_eq!(decoded, message);
    }

    #[test]
    fn roundtrips_control_and_source_messages() {
        let messages = [
            Message::Hello {
                role: Role::Preview,
                pid: 81,
            },
            Message::Catalog {
                entries: vec![CatalogItem {
                    id: "counter".to_owned(),
                    label: "Counter".to_owned(),
                    custom: false,
                }],
                active_id: "counter".to_owned(),
            },
            Message::OpenEditor {
                example_id: "counter".to_owned(),
                label: "Counter".to_owned(),
                application: application(),
                revision: 7,
                units: units(),
                migration: None,
                migration_stage: None,
            },
            Message::DevInspect {
                request_id: 9,
                revision: 7,
                path: "store.count".to_owned(),
            },
            Message::PreviewInspect {
                request_id: 9,
                revision: 7,
                path: "store.count".to_owned(),
            },
            Message::PreviewInspectResult {
                request_id: 9,
                revision: 7,
                runtime_sequence: 3,
                path: "store.count".to_owned(),
                ok: true,
                value: "3".to_owned(),
                authority: None,
            },
            Message::DevSourceChanged {
                application: application(),
                revision: 8,
                units: units(),
            },
            Message::DevRun {
                application: application(),
                revision: 9,
                units: units(),
            },
            Message::DevTest {
                request_id: 91,
                application: application(),
                revision: 10,
                units: units(),
            },
            Message::PreviewApply {
                intent: PreviewIntent::Test,
                request_id: Some(91),
                revision: 10,
                source: PreviewSource::DistributedPackage {
                    programs: program_sources(),
                },
                test_steps: vec![TestStep {
                    id: "increment".to_owned(),
                    source_path: "store.increment.press".to_owned(),
                    action_kind: Some("click".to_owned()),
                    target_text: Some("+".to_owned()),
                    text: None,
                    key: None,
                    address: None,
                    target_key: Some(79),
                    target_generation: Some(3),
                    target_occurrence: None,
                    pointer_x: Some("216".to_owned()),
                    pointer_y: Some("0".to_owned()),
                    pointer_width: Some("360".to_owned()),
                    pointer_height: Some("1".to_owned()),
                    expectations: vec![
                        ScenarioExpectation::RootText {
                            name: "store.count".to_owned(),
                            value: "1".to_owned(),
                        },
                        ScenarioExpectation::ListTexts {
                            list: "todos".to_owned(),
                            field: "title".to_owned(),
                            filter: Some(ScenarioFieldMatch {
                                field: "completed".to_owned(),
                                value: "false".to_owned(),
                            }),
                            values: vec!["First".to_owned(), "Second".to_owned()],
                        },
                        ScenarioExpectation::RootRowTexts {
                            root: "visible_todos".to_owned(),
                            field: "title".to_owned(),
                            values: vec!["First".to_owned()],
                        },
                        ScenarioExpectation::ListCount {
                            list: "todos".to_owned(),
                            filter: ScenarioFieldMatch {
                                field: "completed".to_owned(),
                                value: "false".to_owned(),
                            },
                            count: 2,
                        },
                        ScenarioExpectation::RowFields {
                            list: "todos".to_owned(),
                            key_field: "id".to_owned(),
                            key: "1".to_owned(),
                            fields: std::collections::BTreeMap::from([
                                ("completed".to_owned(), "false".to_owned()),
                                ("title".to_owned(), "First".to_owned()),
                            ]),
                        },
                        ScenarioExpectation::RecomputedRows {
                            list: "cells".to_owned(),
                            key_field: "address".to_owned(),
                            field: "value".to_owned(),
                            keys: vec!["A0".to_owned(), "B0".to_owned()],
                        },
                        ScenarioExpectation::SemanticDeltaContains(
                            "store.count changed".to_owned(),
                        ),
                        ScenarioExpectation::DocumentChanged,
                    ],
                }],
                migration: None,
                migration_stage: None,
            },
            Message::PreviewAssets {
                assets: vec![AssetBlob {
                    url: "asset://portfolio/hero.webp".to_owned(),
                    media_type: "image/webp".to_owned(),
                    sha256: "abc123".to_owned(),
                    bytes: vec![1, 2, 3, 4],
                }],
            },
            Message::Shutdown,
        ];
        for message in messages {
            roundtrip(message);
        }
    }

    #[test]
    fn roundtrips_preview_feedback() {
        roundtrip(Message::PreviewStats(PreviewStats {
            frame_seq: 144,
            source_revision: 19,
            frame_mode: FrameMode::Burst,
            proof_mode: ProofMode::Off,
            frames_per_second_milli: 59_940,
            input_to_present_micros: 8_311,
            render_micros: 1_203,
            present_micros: 5_022,
            missed_frames: 2,
            dropped_snapshots: 1,
            sample_age_millis: 4,
            persistence_schema_version: 3,
            persistence_durable_epoch: 18,
            persistence_durable_turn: 42,
            persistence_pending_turns: 2,
            persistence_queue_depth: 1,
            persistence_accepting: true,
            persistence_worker_alive: true,
            persistence_error: String::new(),
        }));
        roundtrip(Message::PreviewStatus {
            revision: 19,
            ok: false,
            message: "compile failed on line 3".to_owned(),
        });
        roundtrip(Message::PreviewRuntimeChanged {
            revision: 19,
            runtime_sequence: 8,
        });
        roundtrip(Message::PreviewTestResult {
            request_id: 4,
            passed: true,
            message: "counter scenario passed".to_owned(),
        });
        roundtrip(Message::DevMigrationCommand {
            request_id: 17,
            revision: 19,
            command: MigrationCommand::Preview {
                stage_id: "v2".to_owned(),
            },
        });
        roundtrip(Message::PreviewMigrationCommand {
            request_id: 18,
            revision: 19,
            command: MigrationCommand::StartOver { confirmed: true },
        });
        roundtrip(Message::PreviewMigrationStatus(MigrationStatus {
            request_id: Some(17),
            revision: 19,
            operation: MigrationOperation::Previewed,
            ok: true,
            active_stage: "v1".to_owned(),
            previewed_stage: Some("v2".to_owned()),
            target_stage: Some("v2".to_owned()),
            target_schema_version: 2,
            migration_step_count: 1,
            deleted_memory_count: 0,
            message: "candidate settled without mutation".to_owned(),
        }));
    }

    #[test]
    fn manifest_migration_bundle_roundtrips_with_bounded_typed_stages() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter_migration")
            .unwrap();
        let migration = example.migration.expect("migration bundle");
        let message = Message::OpenEditor {
            example_id: example.id,
            label: example.label,
            application: example.application,
            revision: 1,
            units: example.units,
            migration: Some(migration.clone()),
            migration_stage: Some(migration.initial_stage.clone()),
        };
        roundtrip(message);

        let mut oversized = migration;
        let first_stage = oversized.stages[0].clone();
        oversized.stages = vec![first_stage; MAX_MIGRATION_STAGES + 1];
        let mut bytes = Vec::new();
        assert!(matches!(
            write_message(
                &mut bytes,
                &Message::PreviewApply {
                    intent: PreviewIntent::Replace,
                    request_id: None,
                    revision: 1,
                    source: PreviewSource::BuiltInSingleRole {
                        application: application(),
                        units: units(),
                    },
                    test_steps: Vec::new(),
                    migration: Some(oversized),
                    migration_stage: Some("v1".to_owned()),
                },
            ),
            Err(ProtocolError::LimitExceeded("migration stage count", _))
        ));
    }

    #[test]
    fn kavik_asset_bundle_roundtrips_inside_the_bounded_preview_frame() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("kavik_cz")
            .unwrap();
        let message = Message::PreviewAssets {
            assets: example.assets,
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).expect("portfolio assets should fit one IPC frame");
        assert!(bytes.len() <= MAX_FRAME_BYTES + std::mem::size_of::<u32>());
        assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), Some(message));
    }

    #[test]
    fn stream_roundtrip_preserves_frame_boundaries() {
        let (left, right) = UnixStream::pair().expect("socket pair");
        let sender = std::thread::spawn(move || {
            let mut channel = Connection::new(left);
            channel
                .send(&Message::Ready { role: Role::Dev })
                .expect("send ready");
            channel.send(&Message::DevReset).expect("send reset");
        });
        let mut receiver = Connection::new(right);
        assert_eq!(
            receiver.receive().expect("receive ready"),
            Some(Message::Ready { role: Role::Dev })
        );
        assert_eq!(
            receiver.receive().expect("receive reset"),
            Some(Message::DevReset)
        );
        assert_eq!(receiver.receive().expect("receive eof"), None);
        sender.join().expect("sender thread");
    }

    #[test]
    fn rejects_trailing_payload_bytes() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Message::DevReset).expect("encode message");
        let length = u32::from_le_bytes(bytes[..4].try_into().expect("length"));
        bytes[..4].copy_from_slice(&(length + 1).to_le_bytes());
        bytes.push(0xff);
        assert!(matches!(
            read_message(&mut bytes.as_slice()),
            Err(ProtocolError::TrailingBytes(1))
        ));
    }
}
