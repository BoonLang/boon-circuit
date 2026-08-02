use std::fmt;
use std::io::{self, Cursor, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use bincode::Options;
use boon_plan::ProgramRole;
use boon_typecheck::TypeDisplayNode;
use serde::{Deserialize, Serialize};

pub use boon_editor::language::{LanguageProjectSnapshot, SourceUnit};
pub use boon_runtime::{
    ApplicationIdentity, MigrationScenario, MigrationSequence, MigrationTestDriver,
    ScenarioExpectation, ScenarioFieldMatch,
};

const MAGIC: [u8; 4] = *b"BNIP";
const VERSION: u16 = 16;
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
const MAX_LANGUAGE_HINTS: usize = 262_144;
const MAX_LANGUAGE_SEMANTICS: usize = 262_144;
const MAX_LANGUAGE_DIAGNOSTICS: usize = 65_536;
const MAX_LANGUAGE_TYPE_DISPLAY_NODES: usize = 65_536;
const MAX_LANGUAGE_TYPE_DISPLAY_CHILDREN: usize = 4_096;
const MAX_LANGUAGE_TYPE_DISPLAY_DEPTH: usize = 128;
const LAST_MESSAGE_TAG: u8 = 27;
pub const VERIFY_BOUNDED_WINDOWS_ENV: &str = "BOON_VERIFY_BOUNDED_WINDOWS";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum Role {
    Preview = 1,
    Dev = 2,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramSource {
    pub role: ProgramRole,
    pub entry_path: String,
    pub units: Vec<SourceUnit>,
    pub application: ApplicationIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PreviewSource {
    BuiltInSingleRole {
        application: ApplicationIdentity,
        entry_path: String,
        units: Vec<SourceUnit>,
    },
    DistributedPackage {
        programs: Vec<ProgramSource>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssetBlob {
    pub url: String,
    pub media_type: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogItem {
    pub id: String,
    pub label: String,
    pub custom: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationStage {
    pub id: String,
    pub label: String,
    pub schema_version: u64,
    pub source: String,
    pub source_files: Vec<String>,
    pub units: Vec<SourceUnit>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationBundle {
    pub initial_stage: String,
    pub launch_stage: String,
    pub test_driver: MigrationTestDriver,
    pub scenario_path: String,
    pub stages: Vec<MigrationStage>,
    #[serde(with = "migration_scenario_wire")]
    pub scenario: MigrationScenario,
}

mod migration_scenario_wire {
    use serde::{Deserialize, Deserializer, Serializer, de, ser};

    use super::{MAX_MIGRATION_SCENARIO_BYTES, MigrationScenario};

    pub fn serialize<S>(scenario: &MigrationScenario, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = toml::to_string(scenario).map_err(ser::Error::custom)?;
        if encoded.len() > MAX_MIGRATION_SCENARIO_BYTES {
            return Err(ser::Error::custom("migration scenario exceeds byte limit"));
        }
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MigrationScenario, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() > MAX_MIGRATION_SCENARIO_BYTES {
            return Err(de::Error::custom("migration scenario exceeds byte limit"));
        }
        toml::from_str(&encoded).map_err(de::Error::custom)
    }
}

// `TypeDisplayNode` intentionally uses an internally tagged serde shape for
// human-readable reports. Bincode cannot decode internally tagged enums, so
// the native IPC boundary projects only that nested tree to an equivalent
// externally tagged wire enum. The public message still owns the editor DTO,
// and the conversion preserves every field without a JSON/TOML side encoding.
mod language_snapshot_wire {
    use boon_contract::SourceBundleDigestV1;
    use boon_editor::language::{
        InspectorHint, LanguageFileIndex, LanguageProjectSnapshot, SemanticDiagnostic, SemanticItem,
    };
    use boon_typecheck::{TypeDisplayField, TypeDisplayFunctionArg, TypeDisplayNode};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct SnapshotRef<'a> {
        revision: u64,
        entrypoint: &'a str,
        source_bundle_digest_v1: SourceBundleDigestV1,
        files: Vec<FileRef<'a>>,
        semantics: &'a [SemanticItem],
        diagnostics: &'a [SemanticDiagnostic],
        inline_out_hints: bool,
    }

    #[derive(Serialize)]
    struct FileRef<'a> {
        path: &'a str,
        inspector_hints: Vec<HintRef<'a>>,
    }

    #[derive(Serialize)]
    struct HintRef<'a> {
        line: usize,
        start: usize,
        end: usize,
        anchor_column: usize,
        category: &'a str,
        compact_label: &'a str,
        detail_label: &'a str,
        display_tree: DisplayNodeRef<'a>,
    }

    // Variant and field order must remain identical to `DisplayNodeOwned`.
    // The protocol roundtrip fixture covers every variant.
    #[derive(Serialize)]
    enum DisplayNodeRef<'a> {
        Scalar {
            label: &'a str,
        },
        Object {
            fields: Vec<DisplayFieldRef<'a>>,
            open: bool,
        },
        TaggedObject {
            tag: &'a str,
            fields: Vec<DisplayFieldRef<'a>>,
            open: bool,
        },
        List {
            item: Box<DisplayNodeRef<'a>>,
        },
        Union {
            variants: Vec<DisplayNodeRef<'a>>,
        },
        Function {
            name: Option<&'a str>,
            args: Vec<DisplayFunctionArgRef<'a>>,
            result: Box<DisplayNodeRef<'a>>,
        },
        Map {
            key: Box<DisplayNodeRef<'a>>,
            value: Box<DisplayNodeRef<'a>>,
        },
        Set {
            item: Box<DisplayNodeRef<'a>>,
        },
        Bits {
            width: u32,
        },
    }

    #[derive(Serialize)]
    struct DisplayFieldRef<'a> {
        name: &'a str,
        ty: DisplayNodeRef<'a>,
    }

    #[derive(Serialize)]
    struct DisplayFunctionArgRef<'a> {
        name: Option<&'a str>,
        ty: DisplayNodeRef<'a>,
    }

    #[derive(Deserialize)]
    struct SnapshotOwned {
        revision: u64,
        entrypoint: String,
        source_bundle_digest_v1: SourceBundleDigestV1,
        files: Vec<FileOwned>,
        semantics: Vec<SemanticItem>,
        diagnostics: Vec<SemanticDiagnostic>,
        inline_out_hints: bool,
    }

    #[derive(Deserialize)]
    struct FileOwned {
        path: String,
        inspector_hints: Vec<HintOwned>,
    }

    #[derive(Deserialize)]
    struct HintOwned {
        line: usize,
        start: usize,
        end: usize,
        anchor_column: usize,
        category: String,
        compact_label: String,
        detail_label: String,
        display_tree: DisplayNodeOwned,
    }

    #[derive(Deserialize)]
    enum DisplayNodeOwned {
        Scalar {
            label: String,
        },
        Object {
            fields: Vec<DisplayFieldOwned>,
            open: bool,
        },
        TaggedObject {
            tag: String,
            fields: Vec<DisplayFieldOwned>,
            open: bool,
        },
        List {
            item: Box<DisplayNodeOwned>,
        },
        Union {
            variants: Vec<DisplayNodeOwned>,
        },
        Function {
            name: Option<String>,
            args: Vec<DisplayFunctionArgOwned>,
            result: Box<DisplayNodeOwned>,
        },
        Map {
            key: Box<DisplayNodeOwned>,
            value: Box<DisplayNodeOwned>,
        },
        Set {
            item: Box<DisplayNodeOwned>,
        },
        Bits {
            width: u32,
        },
    }

    #[derive(Deserialize)]
    struct DisplayFieldOwned {
        name: String,
        ty: DisplayNodeOwned,
    }

    #[derive(Deserialize)]
    struct DisplayFunctionArgOwned {
        name: Option<String>,
        ty: DisplayNodeOwned,
    }

    pub fn serialize<S>(
        snapshot: &LanguageProjectSnapshot,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SnapshotRef {
            revision: snapshot.revision,
            entrypoint: &snapshot.entrypoint,
            source_bundle_digest_v1: snapshot.source_bundle_digest_v1,
            files: snapshot
                .files
                .iter()
                .map(|file| FileRef {
                    path: &file.path,
                    inspector_hints: file
                        .inspector_hints
                        .iter()
                        .map(|hint| HintRef {
                            line: hint.line,
                            start: hint.start,
                            end: hint.end,
                            anchor_column: hint.anchor_column,
                            category: &hint.category,
                            compact_label: &hint.compact_label,
                            detail_label: &hint.detail_label,
                            display_tree: display_node_ref(&hint.display_tree),
                        })
                        .collect(),
                })
                .collect(),
            semantics: &snapshot.semantics,
            diagnostics: &snapshot.diagnostics,
            inline_out_hints: snapshot.inline_out_hints,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<LanguageProjectSnapshot, D::Error>
    where
        D: Deserializer<'de>,
    {
        let snapshot = SnapshotOwned::deserialize(deserializer)?;
        Ok(LanguageProjectSnapshot {
            revision: snapshot.revision,
            entrypoint: snapshot.entrypoint,
            source_bundle_digest_v1: snapshot.source_bundle_digest_v1,
            files: snapshot
                .files
                .into_iter()
                .map(|file| LanguageFileIndex {
                    path: file.path,
                    inspector_hints: file
                        .inspector_hints
                        .into_iter()
                        .map(|hint| InspectorHint {
                            line: hint.line,
                            start: hint.start,
                            end: hint.end,
                            anchor_column: hint.anchor_column,
                            category: hint.category,
                            compact_label: hint.compact_label,
                            detail_label: hint.detail_label,
                            display_tree: display_node_owned(hint.display_tree),
                        })
                        .collect(),
                })
                .collect(),
            semantics: snapshot.semantics,
            diagnostics: snapshot.diagnostics,
            inline_out_hints: snapshot.inline_out_hints,
        })
    }

    fn display_node_ref(node: &TypeDisplayNode) -> DisplayNodeRef<'_> {
        match node {
            TypeDisplayNode::Scalar { label } => DisplayNodeRef::Scalar { label },
            TypeDisplayNode::Object { fields, open } => DisplayNodeRef::Object {
                fields: fields.iter().map(display_field_ref).collect(),
                open: *open,
            },
            TypeDisplayNode::TaggedObject { tag, fields, open } => DisplayNodeRef::TaggedObject {
                tag,
                fields: fields.iter().map(display_field_ref).collect(),
                open: *open,
            },
            TypeDisplayNode::List { item } => DisplayNodeRef::List {
                item: Box::new(display_node_ref(item)),
            },
            TypeDisplayNode::Union { variants } => DisplayNodeRef::Union {
                variants: variants.iter().map(display_node_ref).collect(),
            },
            TypeDisplayNode::Function { name, args, result } => DisplayNodeRef::Function {
                name: name.as_deref(),
                args: args.iter().map(display_function_arg_ref).collect(),
                result: Box::new(display_node_ref(result)),
            },
            TypeDisplayNode::Map { key, value } => DisplayNodeRef::Map {
                key: Box::new(display_node_ref(key)),
                value: Box::new(display_node_ref(value)),
            },
            TypeDisplayNode::Set { item } => DisplayNodeRef::Set {
                item: Box::new(display_node_ref(item)),
            },
            TypeDisplayNode::Bits { width } => DisplayNodeRef::Bits { width: *width },
        }
    }

    fn display_field_ref(field: &TypeDisplayField) -> DisplayFieldRef<'_> {
        DisplayFieldRef {
            name: &field.name,
            ty: display_node_ref(&field.ty),
        }
    }

    fn display_function_arg_ref(arg: &TypeDisplayFunctionArg) -> DisplayFunctionArgRef<'_> {
        DisplayFunctionArgRef {
            name: arg.name.as_deref(),
            ty: display_node_ref(&arg.ty),
        }
    }

    fn display_node_owned(node: DisplayNodeOwned) -> TypeDisplayNode {
        match node {
            DisplayNodeOwned::Scalar { label } => TypeDisplayNode::Scalar { label },
            DisplayNodeOwned::Object { fields, open } => TypeDisplayNode::Object {
                fields: fields.into_iter().map(display_field_owned).collect(),
                open,
            },
            DisplayNodeOwned::TaggedObject { tag, fields, open } => TypeDisplayNode::TaggedObject {
                tag,
                fields: fields.into_iter().map(display_field_owned).collect(),
                open,
            },
            DisplayNodeOwned::List { item } => TypeDisplayNode::List {
                item: Box::new(display_node_owned(*item)),
            },
            DisplayNodeOwned::Union { variants } => TypeDisplayNode::Union {
                variants: variants.into_iter().map(display_node_owned).collect(),
            },
            DisplayNodeOwned::Function { name, args, result } => TypeDisplayNode::Function {
                name,
                args: args.into_iter().map(display_function_arg_owned).collect(),
                result: Box::new(display_node_owned(*result)),
            },
            DisplayNodeOwned::Map { key, value } => TypeDisplayNode::Map {
                key: Box::new(display_node_owned(*key)),
                value: Box::new(display_node_owned(*value)),
            },
            DisplayNodeOwned::Set { item } => TypeDisplayNode::Set {
                item: Box::new(display_node_owned(*item)),
            },
            DisplayNodeOwned::Bits { width } => TypeDisplayNode::Bits { width },
        }
    }

    fn display_field_owned(field: DisplayFieldOwned) -> TypeDisplayField {
        TypeDisplayField {
            name: field.name,
            ty: display_node_owned(field.ty),
        }
    }

    fn display_function_arg_owned(arg: DisplayFunctionArgOwned) -> TypeDisplayFunctionArg {
        TypeDisplayFunctionArg {
            name: arg.name,
            ty: display_node_owned(arg.ty),
        }
    }
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
        validate_string(&self.scenario_path)?;
        let scenario = toml::to_string(&self.scenario)
            .map_err(|error| ProtocolError::InvalidMigration(error.to_string()))?;
        check_limit(
            "migration scenario bytes",
            scenario.len(),
            MAX_MIGRATION_SCENARIO_BYTES,
        )?;

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
            check_limit(
                "migration source file count",
                stage.source_files.len(),
                MAX_MIGRATION_SOURCE_FILES,
            )?;
            validate_strings(
                std::iter::once(stage.label.as_str())
                    .chain(std::iter::once(stage.source.as_str()))
                    .chain(stage.source_files.iter().map(String::as_str)),
            )?;
            validate_source_units(&stage.units, "migration source unit count")?;
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MigrationCommand {
    Preview { stage_id: String },
    Activate { stage_id: String },
    Restart,
    StartOver { confirmed: bool },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum MigrationOperation {
    Opened = 1,
    Previewed = 2,
    Activated = 3,
    Restarted = 4,
    StartedOver = 5,
    Failed = 6,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum AuthoritySelectionKind {
    Scalar = 1,
    IndexedField = 2,
    List = 3,
    Map = 4,
    Set = 5,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum StateArtifactFormat {
    CanonicalCbor = 1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalStateArtifact {
    pub format: StateArtifactFormat,
    pub schema_version: u64,
    pub sha256: [u8; 32],
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
        check_limit(
            "persistence artifact bytes",
            self.bytes.len(),
            MAX_PERSISTENCE_ARTIFACT_BYTES,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::ImportPreview { artifact } => artifact.validate(),
            Self::ClearSelected { selection, .. } => selection.validate(),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistenceOperationStatus {
    pub request_id: u64,
    pub operation: PersistenceOperation,
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistenceCapability {
    pub available: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistenceCapabilities {
    pub clear_selected: PersistenceCapability,
    pub export_state: PersistenceCapability,
    pub import_preview: PersistenceCapability,
    pub activate_import: PersistenceCapability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthoritySummary {
    pub runtime_turn_sequence: u64,
    pub source_event_sequence: u64,
    pub scalar_count: u32,
    pub indexed_field_count: u32,
    pub list_count: u32,
    pub map_count: u32,
    pub set_count: u32,
    pub effect_contract_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistenceTimingSummary {
    pub authority_enqueue_us: u64,
    pub encode_us: u64,
    pub checkpoint_us: u64,
    pub barrier_us: u64,
    pub restore_us: u64,
    pub migration_us: u64,
    pub rebuild_derived_us: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingSummary {
    pub first_turn_sequence: Option<u64>,
    pub last_turn_sequence: Option<u64>,
    pub oldest_age_millis: u64,
    pub turn_count: u64,
    pub queue_depth: u32,
    pub reserved_slots: u32,
    pub accepting_turns: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurableSummary {
    pub epoch: u64,
    pub through_turn_sequence: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum OutboxSampleState {
    Pending = 1,
    Dispatching = 2,
    ReconciliationRequired = 3,
    Completed = 4,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxSample {
    pub item_id: [u8; 32],
    pub invocation_id: [u8; 32],
    pub effect_id: [u8; 32],
    pub state: OutboxSampleState,
    pub attempt: u32,
    pub created_turn_sequence: u64,
    pub updated_turn_sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxSummary {
    pub pending_count: u32,
    pub dispatching_count: u32,
    pub reconciliation_count: u32,
    pub completed_count: u32,
    pub samples: Vec<OutboxSample>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
        validate_application(&self.application)?;
        check_limit(
            "persistence outbox sample count",
            self.outbox.samples.len(),
            MAX_PERSISTENCE_OUTBOX_SAMPLES,
        )?;
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
            check_limit(
                "persistence status bytes",
                value.len(),
                MAX_PERSISTENCE_STATUS_BYTES,
            )?;
        }
        for capability in [
            &self.capabilities.clear_selected,
            &self.capabilities.export_state,
            &self.capabilities.import_preview,
            &self.capabilities.activate_import,
        ] {
            check_limit(
                "persistence status bytes",
                capability.reason.len(),
                MAX_PERSISTENCE_STATUS_BYTES,
            )?;
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum PreviewIntent {
    Replace = 1,
    Run = 2,
    Reset = 3,
    Test = 4,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum FrameMode {
    Idle = 1,
    Burst = 2,
    Probe = 3,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum ProofMode {
    Off = 1,
    Trace = 2,
    Readback = 3,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    PreviewLanguageSnapshot {
        #[serde(with = "language_snapshot_wire")]
        snapshot: LanguageProjectSnapshot,
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
            Self::PreviewLanguageSnapshot { .. } => 27,
        }
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Hello { .. }
            | Self::Ready { .. }
            | Self::DevReset
            | Self::PreviewRuntimeChanged { .. }
            | Self::Shutdown => Ok(()),
            Self::Catalog { entries, active_id } => {
                check_limit("catalog entry count", entries.len(), MAX_CATALOG_ENTRIES)?;
                validate_string(active_id)?;
                for entry in entries {
                    validate_strings([entry.id.as_str(), entry.label.as_str()])?;
                }
                Ok(())
            }
            Self::OpenEditor {
                example_id,
                label,
                application,
                units,
                migration,
                migration_stage,
                ..
            } => {
                validate_strings([example_id.as_str(), label.as_str()])?;
                validate_application(application)?;
                validate_source_units(units, "source unit count")?;
                if let Some(migration) = migration {
                    migration.validate()?;
                }
                validate_optional_strings([migration_stage.as_deref()])
            }
            Self::DevSelectExample { example_id } => validate_string(example_id),
            Self::DevSourceChanged {
                application, units, ..
            }
            | Self::DevRun {
                application, units, ..
            }
            | Self::DevTest {
                application, units, ..
            } => {
                validate_application(application)?;
                validate_source_units(units, "source unit count")
            }
            Self::PreviewApply {
                source,
                test_steps,
                migration,
                migration_stage,
                ..
            } => {
                validate_preview_source(source)?;
                validate_test_steps(test_steps)?;
                if let Some(migration) = migration {
                    migration.validate()?;
                }
                validate_optional_strings([migration_stage.as_deref()])
            }
            Self::PreviewAssets { assets } => validate_assets(assets),
            Self::PreviewStats(stats) => validate_string(&stats.persistence_error),
            Self::PreviewStatus { message, .. } | Self::PreviewTestResult { message, .. } => {
                validate_string(message)
            }
            Self::DevInspect { path, .. } | Self::PreviewInspect { path, .. } => {
                validate_string(path)
            }
            Self::PreviewInspectResult {
                path,
                value,
                authority,
                ..
            } => {
                validate_strings([path.as_str(), value.as_str()])?;
                if let Some(authority) = authority {
                    authority.validate()?;
                }
                Ok(())
            }
            Self::DevMigrationCommand { command, .. }
            | Self::PreviewMigrationCommand { command, .. } => match command {
                MigrationCommand::Preview { stage_id }
                | MigrationCommand::Activate { stage_id } => validate_string(stage_id),
                MigrationCommand::Restart | MigrationCommand::StartOver { .. } => Ok(()),
            },
            Self::PreviewMigrationStatus(status) => {
                validate_string(&status.active_stage)?;
                validate_optional_strings([
                    status.previewed_stage.as_deref(),
                    status.target_stage.as_deref(),
                ])?;
                validate_string(&status.message)
            }
            Self::DevPersistenceCommand { command, .. }
            | Self::PreviewPersistenceCommand { command, .. } => command.validate(),
            Self::PreviewPersistenceSnapshot(snapshot) => snapshot.validate(),
            Self::PreviewPersistenceArtifact { artifact, .. } => artifact.validate(),
            Self::PreviewLanguageSnapshot { snapshot } => validate_language_snapshot(snapshot),
        }
    }
}

fn validate_application(application: &ApplicationIdentity) -> Result<(), ProtocolError> {
    validate_strings([
        application.package_id.as_str(),
        application.state_namespace.as_str(),
        application.deployment_domain.as_str(),
    ])
}

fn validate_source_units(
    units: &[SourceUnit],
    limit_name: &'static str,
) -> Result<(), ProtocolError> {
    check_limit(limit_name, units.len(), MAX_SOURCE_UNITS)?;
    for unit in units {
        validate_strings([unit.path.as_str(), unit.source.as_str()])?;
    }
    Ok(())
}

fn validate_language_snapshot(snapshot: &LanguageProjectSnapshot) -> Result<(), ProtocolError> {
    if snapshot.files.is_empty() {
        return Err(ProtocolError::InvalidLanguageSnapshot(
            "language snapshot has no source files".to_owned(),
        ));
    }
    check_limit(
        "language source file count",
        snapshot.files.len(),
        MAX_SOURCE_UNITS,
    )?;
    validate_string(&snapshot.entrypoint)?;
    let entrypoint = boon_contract::normalize_source_path(&snapshot.entrypoint)
        .map_err(|error| ProtocolError::InvalidLanguageSnapshot(error.to_string()))?;
    if entrypoint != snapshot.entrypoint {
        return Err(ProtocolError::InvalidLanguageSnapshot(
            "language snapshot entrypoint is not canonical".to_owned(),
        ));
    }

    let mut paths = std::collections::BTreeSet::new();
    let mut hint_count = 0usize;
    for file in &snapshot.files {
        validate_string(&file.path)?;
        let path = boon_contract::normalize_source_path(&file.path)
            .map_err(|error| ProtocolError::InvalidLanguageSnapshot(error.to_string()))?;
        if path != file.path {
            return Err(ProtocolError::InvalidLanguageSnapshot(format!(
                "language source path `{}` is not canonical",
                file.path
            )));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(ProtocolError::InvalidLanguageSnapshot(format!(
                "language source path `{}` is duplicated",
                file.path
            )));
        }
        hint_count = hint_count.checked_add(file.inspector_hints.len()).ok_or(
            ProtocolError::LimitExceeded("language hint count", usize::MAX),
        )?;
        check_limit("language hint count", hint_count, MAX_LANGUAGE_HINTS)?;
        for hint in &file.inspector_hints {
            if hint.start > hint.end {
                return Err(ProtocolError::InvalidLanguageSnapshot(format!(
                    "language hint in `{}` has an inverted byte span",
                    file.path
                )));
            }
            validate_strings([
                hint.category.as_str(),
                hint.compact_label.as_str(),
                hint.detail_label.as_str(),
            ])?;
            validate_type_display_node(&hint.display_tree)?;
        }
    }
    if !paths.contains(snapshot.entrypoint.as_str()) {
        return Err(ProtocolError::InvalidLanguageSnapshot(format!(
            "language entrypoint `{}` is absent from its file index",
            snapshot.entrypoint
        )));
    }

    check_limit(
        "language semantic item count",
        snapshot.semantics.len(),
        MAX_LANGUAGE_SEMANTICS,
    )?;
    for item in &snapshot.semantics {
        validate_language_location("semantic item", &item.location, snapshot)?;
        validate_strings([
            item.name.as_str(),
            item.label.as_str(),
            item.detail.as_str(),
        ])?;
    }
    check_limit(
        "language diagnostic count",
        snapshot.diagnostics.len(),
        MAX_LANGUAGE_DIAGNOSTICS,
    )?;
    for diagnostic in &snapshot.diagnostics {
        validate_language_location("diagnostic", &diagnostic.location, snapshot)?;
        validate_string(&diagnostic.message)?;
    }
    Ok(())
}

fn validate_language_location(
    kind: &str,
    location: &boon_editor::language::SourceLocation,
    snapshot: &LanguageProjectSnapshot,
) -> Result<(), ProtocolError> {
    let Some(file) = snapshot.files.get(location.file_index) else {
        return Err(ProtocolError::InvalidLanguageSnapshot(format!(
            "{kind} references missing file index {}",
            location.file_index
        )));
    };
    if location.path != file.path {
        return Err(ProtocolError::InvalidLanguageSnapshot(format!(
            "{kind} path `{}` does not match file index {} (`{}`)",
            location.path, location.file_index, file.path
        )));
    }
    if location.start > location.end {
        return Err(ProtocolError::InvalidLanguageSnapshot(format!(
            "{kind} in `{}` has an inverted byte span",
            location.path
        )));
    }
    Ok(())
}

fn validate_type_display_node(root: &TypeDisplayNode) -> Result<(), ProtocolError> {
    let mut stack = vec![(root, 0usize)];
    let mut node_count = 0usize;
    while let Some((node, depth)) = stack.pop() {
        if depth > MAX_LANGUAGE_TYPE_DISPLAY_DEPTH {
            return Err(ProtocolError::InvalidLanguageSnapshot(
                "language type display tree exceeds its depth limit".to_owned(),
            ));
        }
        node_count = node_count
            .checked_add(1)
            .ok_or(ProtocolError::LimitExceeded(
                "language type display node count",
                usize::MAX,
            ))?;
        check_limit(
            "language type display node count",
            node_count,
            MAX_LANGUAGE_TYPE_DISPLAY_NODES,
        )?;
        let child_depth = depth.saturating_add(1);
        match node {
            TypeDisplayNode::Scalar { label } => validate_string(label)?,
            TypeDisplayNode::Object { fields, .. } => {
                check_limit(
                    "language type display child count",
                    fields.len(),
                    MAX_LANGUAGE_TYPE_DISPLAY_CHILDREN,
                )?;
                for field in fields {
                    validate_string(&field.name)?;
                    stack.push((&field.ty, child_depth));
                }
            }
            TypeDisplayNode::TaggedObject { tag, fields, .. } => {
                validate_string(tag)?;
                check_limit(
                    "language type display child count",
                    fields.len(),
                    MAX_LANGUAGE_TYPE_DISPLAY_CHILDREN,
                )?;
                for field in fields {
                    validate_string(&field.name)?;
                    stack.push((&field.ty, child_depth));
                }
            }
            TypeDisplayNode::List { item } | TypeDisplayNode::Set { item } => {
                stack.push((item.as_ref(), child_depth));
            }
            TypeDisplayNode::Union { variants } => {
                check_limit(
                    "language type display child count",
                    variants.len(),
                    MAX_LANGUAGE_TYPE_DISPLAY_CHILDREN,
                )?;
                stack.extend(variants.iter().map(|variant| (variant, child_depth)));
            }
            TypeDisplayNode::Function { name, args, result } => {
                if let Some(name) = name {
                    validate_string(name)?;
                }
                check_limit(
                    "language type display child count",
                    args.len(),
                    MAX_LANGUAGE_TYPE_DISPLAY_CHILDREN,
                )?;
                for arg in args {
                    if let Some(name) = &arg.name {
                        validate_string(name)?;
                    }
                    stack.push((&arg.ty, child_depth));
                }
                stack.push((result.as_ref(), child_depth));
            }
            TypeDisplayNode::Map { key, value } => {
                stack.push((key.as_ref(), child_depth));
                stack.push((value.as_ref(), child_depth));
            }
            TypeDisplayNode::Bits { .. } => {}
        }
    }
    Ok(())
}

fn validate_preview_source(source: &PreviewSource) -> Result<(), ProtocolError> {
    match source {
        PreviewSource::BuiltInSingleRole {
            application,
            entry_path,
            units,
        } => {
            validate_application(application)?;
            validate_string(entry_path)?;
            validate_source_units(units, "source unit count")
        }
        PreviewSource::DistributedPackage { programs } => {
            check_limit(
                "distributed program count",
                programs.len(),
                MAX_DISTRIBUTED_PROGRAMS,
            )?;
            for program in programs {
                validate_application(&program.application)?;
                validate_string(&program.entry_path)?;
                validate_source_units(&program.units, "source unit count")?;
            }
            Ok(())
        }
    }
}

fn validate_assets(assets: &[AssetBlob]) -> Result<(), ProtocolError> {
    check_limit("asset count", assets.len(), MAX_ASSET_BLOBS)?;
    for asset in assets {
        validate_strings([
            asset.url.as_str(),
            asset.media_type.as_str(),
            asset.sha256.as_str(),
        ])?;
        check_limit("asset blob bytes", asset.bytes.len(), MAX_ASSET_BLOB_BYTES)?;
    }
    Ok(())
}

fn validate_test_steps(steps: &[TestStep]) -> Result<(), ProtocolError> {
    check_limit("test step count", steps.len(), MAX_TEST_STEPS)?;
    for step in steps {
        validate_strings([step.id.as_str(), step.source_path.as_str()])?;
        validate_optional_strings([
            step.action_kind.as_deref(),
            step.target_text.as_deref(),
            step.text.as_deref(),
            step.key.as_deref(),
            step.address.as_deref(),
            step.pointer_x.as_deref(),
            step.pointer_y.as_deref(),
            step.pointer_width.as_deref(),
            step.pointer_height.as_deref(),
        ])?;
        check_limit(
            "test expectation count",
            step.expectations.len(),
            MAX_TEST_EXPECTATIONS_PER_STEP,
        )?;
        for expectation in &step.expectations {
            validate_test_expectation(expectation)?;
        }
    }
    Ok(())
}

fn validate_field_match(value: &ScenarioFieldMatch) -> Result<(), ProtocolError> {
    validate_strings([value.field.as_str(), value.value.as_str()])
}

fn validate_expectation_values(values: &[String]) -> Result<(), ProtocolError> {
    check_limit(
        "test expectation value count",
        values.len(),
        MAX_TEST_EXPECTATION_VALUES,
    )?;
    validate_strings(values.iter().map(String::as_str))
}

fn validate_test_expectation(expectation: &ScenarioExpectation) -> Result<(), ProtocolError> {
    match expectation {
        ScenarioExpectation::RootText { name, value } => {
            validate_strings([name.as_str(), value.as_str()])
        }
        ScenarioExpectation::RootNonEmpty { name } => validate_string(name),
        ScenarioExpectation::ListTexts {
            list,
            field,
            filter,
            values,
        } => {
            validate_strings([list.as_str(), field.as_str()])?;
            if let Some(filter) = filter {
                validate_field_match(filter)?;
            }
            validate_expectation_values(values)
        }
        ScenarioExpectation::RootRowTexts {
            root,
            field,
            values,
        } => {
            validate_strings([root.as_str(), field.as_str()])?;
            validate_expectation_values(values)
        }
        ScenarioExpectation::ListCount { list, filter, .. } => {
            validate_string(list)?;
            validate_field_match(filter)
        }
        ScenarioExpectation::RowFields {
            list,
            key_field,
            key,
            fields,
        } => {
            validate_strings([list.as_str(), key_field.as_str(), key.as_str()])?;
            check_limit(
                "test expectation field count",
                fields.len(),
                MAX_TEST_EXPECTATION_FIELDS,
            )?;
            for (field, value) in fields {
                validate_strings([field.as_str(), value.as_str()])?;
            }
            Ok(())
        }
        ScenarioExpectation::RecomputedRows {
            list,
            key_field,
            field,
            keys,
        } => {
            validate_strings([list.as_str(), key_field.as_str(), field.as_str()])?;
            validate_expectation_values(keys)
        }
        ScenarioExpectation::SemanticDeltaContains(value) => validate_string(value),
        ScenarioExpectation::DocumentChanged => Ok(()),
    }
}

fn validate_string(value: &str) -> Result<(), ProtocolError> {
    check_limit("string bytes", value.len(), MAX_STRING_BYTES)
}

fn validate_strings<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<(), ProtocolError> {
    for value in values {
        validate_string(value)?;
    }
    Ok(())
}

fn validate_optional_strings<'a>(
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> Result<(), ProtocolError> {
    validate_strings(values.into_iter().flatten())
}

fn check_limit(name: &'static str, actual: usize, maximum: usize) -> Result<(), ProtocolError> {
    if actual > maximum {
        Err(ProtocolError::LimitExceeded(name, actual))
    } else {
        Ok(())
    }
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    FrameTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownMessage(u8),
    MismatchedMessageTag { outer: u8, payload: u8 },
    InvalidPayload(bincode::Error),
    InvalidMigration(String),
    InvalidPersistence(String),
    InvalidLanguageSnapshot(String),
    LimitExceeded(&'static str, usize),
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
            Self::MismatchedMessageTag { outer, payload } => write!(
                f,
                "IPC outer message tag {outer} does not match payload tag {payload}"
            ),
            Self::InvalidPayload(error) => write!(f, "IPC payload is invalid: {error}"),
            Self::InvalidMigration(message) => {
                write!(f, "IPC migration data is invalid: {message}")
            }
            Self::InvalidPersistence(message) => {
                write!(f, "IPC persistence data is invalid: {message}")
            }
            Self::InvalidLanguageSnapshot(message) => {
                write!(f, "IPC language snapshot is invalid: {message}")
            }
            Self::LimitExceeded(name, value) => {
                write!(f, "IPC {name} exceeds its limit: {value}")
            }
            Self::TrailingBytes(bytes) => write!(f, "IPC frame has {bytes} trailing bytes"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidPayload(error) => Some(error.as_ref()),
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
    message.validate()?;
    let payload = codec()
        .serialize(message)
        .map_err(ProtocolError::InvalidPayload)?;
    let frame_bytes = HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
    if frame_bytes > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(frame_bytes));
    }

    writer.write_all(&(frame_bytes as u32).to_le_bytes())?;
    writer.write_all(&MAGIC)?;
    writer.write_all(&VERSION.to_le_bytes())?;
    writer.write_all(&[message.tag()])?;
    writer.write_all(&payload)?;
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
    let outer_tag = body[6];
    if !(1..=LAST_MESSAGE_TAG).contains(&outer_tag) {
        return Err(ProtocolError::UnknownMessage(outer_tag));
    }

    let payload = &body[HEADER_BYTES..];
    let mut cursor = Cursor::new(payload);
    let message: Message = codec()
        .with_limit(payload.len() as u64)
        .allow_trailing_bytes()
        .deserialize_from(&mut cursor)
        .map_err(ProtocolError::InvalidPayload)?;
    let consumed = cursor.position() as usize;
    if consumed != payload.len() {
        return Err(ProtocolError::TrailingBytes(payload.len() - consumed));
    }
    if message.tag() != outer_tag {
        return Err(ProtocolError::MismatchedMessageTag {
            outer: outer_tag,
            payload: message.tag(),
        });
    }
    message.validate()?;
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_contract::{SourceBundleDigestV1, SourceBundleUnit};
    use boon_editor::language::{
        InspectorHint, LanguageFileIndex, SemanticDiagnostic, SemanticItem, SemanticKind,
        SourceLocation,
    };
    use boon_typecheck::{DeclId, DiagnosticSeverity, TypeDisplayField, TypeDisplayFunctionArg};

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

    fn language_snapshot() -> LanguageProjectSnapshot {
        let source_units = units();
        LanguageProjectSnapshot {
            revision: 19,
            entrypoint: "examples/main.bn".to_owned(),
            source_bundle_digest_v1: SourceBundleDigestV1::new(
                "examples/main.bn",
                source_units
                    .iter()
                    .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
            )
            .unwrap(),
            files: vec![
                LanguageFileIndex {
                    path: "examples/main.bn".to_owned(),
                    inspector_hints: vec![InspectorHint {
                        line: 0,
                        start: 0,
                        end: 5,
                        anchor_column: 5,
                        category: "definition".to_owned(),
                        compact_label: "Number".to_owned(),
                        detail_label: "Number".to_owned(),
                        display_tree: language_display_tree(),
                    }],
                },
                LanguageFileIndex {
                    path: "examples/view.bn".to_owned(),
                    inspector_hints: Vec::new(),
                },
            ],
            semantics: vec![SemanticItem {
                target: DeclId(7),
                kind: SemanticKind::Declaration,
                location: SourceLocation {
                    file_index: 0,
                    path: "examples/main.bn".to_owned(),
                    line: 0,
                    start: 0,
                    end: 5,
                },
                name: "value".to_owned(),
                label: "value value".to_owned(),
                detail: "Declares value value".to_owned(),
                out_related: false,
            }],
            diagnostics: vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                location: SourceLocation {
                    file_index: 1,
                    path: "examples/view.bn".to_owned(),
                    line: 0,
                    start: 0,
                    end: 4,
                },
                message: "example warning".to_owned(),
            }],
            inline_out_hints: false,
        }
    }

    fn language_display_tree() -> TypeDisplayNode {
        let scalar = || TypeDisplayNode::Scalar {
            label: "Number".to_owned(),
        };
        TypeDisplayNode::Union {
            variants: vec![
                scalar(),
                TypeDisplayNode::Object {
                    fields: vec![TypeDisplayField {
                        name: "width".to_owned(),
                        ty: TypeDisplayNode::Bits { width: 32 },
                    }],
                    open: false,
                },
                TypeDisplayNode::TaggedObject {
                    tag: "Some".to_owned(),
                    fields: vec![TypeDisplayField {
                        name: "value".to_owned(),
                        ty: TypeDisplayNode::List {
                            item: Box::new(scalar()),
                        },
                    }],
                    open: true,
                },
                TypeDisplayNode::Function {
                    name: Some("map".to_owned()),
                    args: vec![TypeDisplayFunctionArg {
                        name: Some("items".to_owned()),
                        ty: TypeDisplayNode::Set {
                            item: Box::new(scalar()),
                        },
                    }],
                    result: Box::new(TypeDisplayNode::Map {
                        key: Box::new(scalar()),
                        value: Box::new(TypeDisplayNode::Union {
                            variants: vec![scalar()],
                        }),
                    }),
                },
            ],
        }
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
                            keys: vec!["A1".to_owned(), "B1".to_owned()],
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
        roundtrip(Message::PreviewLanguageSnapshot {
            snapshot: language_snapshot(),
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
    fn language_snapshot_validation_rejects_noncanonical_and_mismatched_locations() {
        let mut noncanonical = language_snapshot();
        noncanonical.entrypoint = "./examples/main.bn".to_owned();
        assert!(matches!(
            write_message(
                &mut Vec::new(),
                &Message::PreviewLanguageSnapshot {
                    snapshot: noncanonical,
                },
            ),
            Err(ProtocolError::InvalidLanguageSnapshot(_))
        ));

        let mut mismatched = language_snapshot();
        mismatched.semantics[0].location.path = "examples/view.bn".to_owned();
        assert!(matches!(
            write_message(
                &mut Vec::new(),
                &Message::PreviewLanguageSnapshot {
                    snapshot: mismatched,
                },
            ),
            Err(ProtocolError::InvalidLanguageSnapshot(_))
        ));
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
                        entry_path: "RUN.bn".to_owned(),
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
    fn rejects_tag_mismatch_and_trailing_payload_bytes() {
        let mut mismatched = Vec::new();
        write_message(&mut mismatched, &Message::DevReset).expect("encode message");
        mismatched[10] = Message::Ready { role: Role::Dev }.tag();
        assert!(matches!(
            read_message(&mut mismatched.as_slice()),
            Err(ProtocolError::MismatchedMessageTag { .. })
        ));

        let mut trailing = Vec::new();
        write_message(&mut trailing, &Message::DevReset).expect("encode message");
        let length = u32::from_le_bytes(trailing[..4].try_into().expect("length"));
        trailing[..4].copy_from_slice(&(length + 1).to_le_bytes());
        trailing.push(0xff);
        assert!(matches!(
            read_message(&mut trailing.as_slice()),
            Err(ProtocolError::TrailingBytes(1))
        ));
    }
}
