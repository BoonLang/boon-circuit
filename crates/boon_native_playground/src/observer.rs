use std::fmt;
use std::io::{self, Cursor, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bincode::Options;
use serde::{Deserialize, Serialize};

pub const OBSERVER_SOCKET_ENV: &str = "BOON_VERIFY_OBSERVER_SOCKET";
pub const NATIVE_SESSION_ID_ENV: &str = "BOON_VERIFY_NATIVE_SESSION_ID";
pub const PROOF_MODE_ENV: &str = "BOON_VERIFY_PROOF_MODE";
pub const PROOF_ARTIFACT_DIR_ENV: &str = "BOON_VERIFY_PROOF_ARTIFACT_DIR";
pub const PROOF_SAMPLE_ORDINAL_ENV: &str = "BOON_VERIFY_PROOF_SAMPLE_ORDINAL";
pub const STATE_EVIDENCE_STEPS_ENV: &str = "BOON_VERIFY_STATE_EVIDENCE_STEPS";
pub const STATE_MOUNT_EVIDENCE_ENV: &str = "BOON_VERIFY_STATE_MOUNT_EVIDENCE";
pub const PERSISTENCE_EVIDENCE_ENV: &str = "BOON_VERIFY_PERSISTENCE_EVIDENCE";
pub const MIGRATION_EVIDENCE_ENV: &str = "BOON_VERIFY_MIGRATION_EVIDENCE";
pub const PROFILE_BENCHMARK_ENV: &str = "BOON_VERIFY_PROFILE_BENCHMARK";
pub const PROFILE_BENCHMARK_STEPS_ENV: &str = "BOON_VERIFY_PROFILE_BENCHMARK_STEPS";
pub const PRODUCT_PROOF_AFTER_TEST_ENV: &str = "BOON_VERIFY_PRODUCT_PROOF_AFTER_TEST";
pub const RESPONSIVE_EVIDENCE_WIDTH_ENV: &str = "BOON_VERIFY_RESPONSIVE_EVIDENCE_WIDTH";
pub const RESPONSIVE_NAVIGATION_SOURCES_ENV: &str = "BOON_VERIFY_RESPONSIVE_NAVIGATION_SOURCES";
pub const SCROLL_PROOF_ORDINAL_ENV: &str = "BOON_VERIFY_SCROLL_PROOF_ORDINAL";
pub const STALE_PROGRAM_EVIDENCE_ENV: &str = "BOON_VERIFY_STALE_PROGRAM_EVIDENCE";
pub const NATIVE_WORKFLOW_STEPS_ENV: &str = "BOON_VERIFY_NATIVE_WORKFLOW_STEPS";
pub const NATIVE_WORKFLOW_PROOF_STEPS_ENV: &str = "BOON_VERIFY_NATIVE_WORKFLOW_PROOF_STEPS";

const MAGIC: [u8; 4] = *b"BNVO";
const VERSION: u16 = 10;
const HEADER_BYTES: usize = 7;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024;
const CLIENT_QUEUE_DEPTH: usize = 512;
const LAST_EVENT_TAG: u8 = 29;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum ObserverRole {
    Preview = 1,
    Dev = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum TestPointerPhase {
    Move = 1,
    Hover = 2,
    Down = 3,
    Up = 4,
    State = 5,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum PersistenceEvidenceKind {
    Exported = 1,
    CorruptionRejected = 2,
    ClearedAndStartedOver = 3,
    ImportPreviewed = 4,
    ImportActivated = 5,
    MigrationActivated = 6,
    MigrationProductRestored = 7,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum StartupDisposition {
    Fresh = 1,
    Restored = 2,
    Migrated = 3,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StartupMigrationEvidence {
    pub source_schema_version: u64,
    pub source_schema_hash: String,
    pub target_schema_version: u64,
    pub target_schema_hash: String,
    pub step_count: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum InputKind {
    PointerMove = 1,
    PointerButton = 2,
    Wheel = 3,
    Keyboard = 4,
    Text = 5,
    Ime = 6,
    Focus = 7,
    Resize = 8,
    Accessibility = 9,
    Close = 10,
    Sensitive = 11,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum AsyncLaneKind {
    ChildProgramCompile = 1,
    PersistenceTurn = 2,
    ProgramArtifactStore = 3,
    ProgramArtifactLoad = 4,
    ProofReadback = 5,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum AsyncLaneOutcome {
    Applied = 1,
    StaleRejected = 2,
    Failed = 3,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct FrameEvidenceKey {
    pub surface_id: String,
    pub process_id: u32,
    pub session_id: String,
    pub frame_id: u64,
    pub input_id: u64,
    pub content_id: u64,
    pub layout_id: u64,
    pub render_id: u64,
    pub surface_epoch: u64,
    pub present_id: u64,
    pub proof_id: u64,
}

impl FrameEvidenceKey {
    pub fn is_complete(&self) -> bool {
        !self.surface_id.is_empty()
            && self.surface_id.len() <= MAX_STRING_BYTES
            && self.process_id != 0
            && !self.session_id.is_empty()
            && self.session_id.len() <= MAX_STRING_BYTES
            && self.frame_id != 0
            && self.input_id != 0
            && self.content_id != 0
            && self.layout_id != 0
            && self.render_id != 0
            && self.surface_epoch != 0
            && self.present_id != 0
            && self.proof_id != 0
    }

    pub fn same_producer_surface(&self, other: &Self) -> bool {
        self.surface_id == other.surface_id
            && self.process_id == other.process_id
            && self.session_id == other.session_id
    }

    fn validate(&self) -> Result<(), ObserverError> {
        validate_strings([self.surface_id.as_str(), self.session_id.as_str()])
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RoleMetadata {
    pub role: ObserverRole,
    pub pid: u32,
    pub surface_id: String,
    pub session_id: String,
    pub surface_epoch: u64,
    pub logical_width: f32,
    pub logical_height: f32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale: f64,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub adapter_device_type: String,
    pub software_adapter: bool,
    pub surface_format: String,
    pub present_mode: String,
    pub window_backend: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct InputAccepted {
    pub role: ObserverRole,
    pub event_sequence: u64,
    pub real_os: bool,
    pub callback_to_host_ns: u64,
    pub surface_epoch: u64,
    pub kind: InputKind,
    pub pointer_button_pressed: Option<bool>,
    pub pointer_x: Option<f32>,
    pub pointer_y: Option<f32>,
    pub target: Option<String>,
    pub target_source_path: Option<String>,
    pub event_digest: String,
    pub visible_change: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FramePresented {
    pub role: ObserverRole,
    pub key: FrameEvidenceKey,
    pub event_sequence: Option<u64>,
    pub input_kind: Option<InputKind>,
    pub callback_to_host_ns: u64,
    pub input_to_present_us: u64,
    pub event_dispatch_us: u64,
    pub executor_us: u64,
    pub runtime_document_us: u64,
    pub document_update_us: u64,
    pub render_us: u64,
    pub document_scene_convert_us: u64,
    pub scene_key_us: u64,
    pub rect_vertices_us: u64,
    pub asset_prepare_us: u64,
    pub quad_batch_key_us: u64,
    pub quad_upload_us: u64,
    pub draw_pass_us: u64,
    pub retained_metrics_us: u64,
    pub text_render_us: u64,
    pub submit_us: u64,
    pub present_us: u64,
    pub frame_us: u64,
    pub observer_drop_count: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProofArtifact {
    pub path: String,
    pub sha256: String,
    pub byte_len: u64,
    pub capture_method: String,
    pub capture_token_digest: String,
    pub nonblank_samples: u64,
    pub unique_rgba_values: u64,
}

impl ProofArtifact {
    fn validate(&self) -> Result<(), ObserverError> {
        validate_strings([
            self.path.as_str(),
            self.sha256.as_str(),
            self.capture_method.as_str(),
            self.capture_token_digest.as_str(),
        ])
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ObserverEvent {
    RoleMetadata(RoleMetadata),
    InputAccepted(InputAccepted),
    FramePresented(FramePresented),
    SourceSwitchAcknowledged {
        revision: u64,
        elapsed_us: u64,
    },
    SourceSwitchFinal {
        revision: u64,
        elapsed_us: u64,
        compile_us: u64,
        post_compile_us: u64,
        key: FrameEvidenceKey,
    },
    TestTarget {
        request_id: u64,
        node: String,
        source_path: String,
        x: f32,
        y: f32,
    },
    TestCompleted {
        request_id: u64,
        passed: bool,
        semantic_assertions_proven: bool,
        completed_steps: u32,
        message: String,
    },
    TestPointerFrame {
        request_id: u64,
        step_index: u32,
        phase: TestPointerPhase,
        x: f32,
        y: f32,
        target: Option<String>,
        runtime_sequence: u64,
        key: FrameEvidenceKey,
    },
    ProofRequested {
        key: FrameEvidenceKey,
        snapshot_prepare_us: u64,
    },
    ProofCompleted {
        key: FrameEvidenceKey,
        completed_after_key: FrameEvidenceKey,
        elapsed_us: u64,
        replaced_count: u64,
        result_drop_count: u64,
        artifact: Option<ProofArtifact>,
        error: Option<String>,
    },
    RoleTarget {
        role: ObserverRole,
        node: String,
        x: f32,
        y: f32,
    },
    SourceFailed {
        revision: u64,
        stage: String,
        message: String,
    },
    StateMounted {
        disposition: StartupDisposition,
        schema_version: u64,
        schema_hash: String,
        migration: Option<StartupMigrationEvidence>,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        state_digest: String,
        key: FrameEvidenceKey,
    },
    ScenarioCheckpoint {
        request_id: u64,
        step_id: String,
        assertion_count: u32,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        state_digest: String,
        key: FrameEvidenceKey,
    },
    PersistenceEvidence {
        kind: PersistenceEvidenceKind,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        before_state_digest: String,
        after_state_digest: String,
        key: FrameEvidenceKey,
    },
    ResponsiveLayoutEvidence {
        resize_sequence: u64,
        logical_width: u32,
        logical_height: u32,
        baseline_key: FrameEvidenceKey,
        baseline_action_count: u32,
        baseline_action_digest: String,
        action_count: u32,
        action_digest: String,
        state_digest: String,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        key: FrameEvidenceKey,
    },
    ProfileSample {
        ordinal: u32,
        input_sequence: u64,
        callback_to_host_ns: u64,
        editor_visible_us: u64,
        preview_visible_us: u64,
        compile_us: u64,
        parent_dispatch_us: u64,
        parent_executor_us: u64,
        parent_runtime_document_us: u64,
        parent_persistence_us: u64,
        completion_us: u64,
        completion_executor_us: u64,
        completion_runtime_document_us: u64,
        completion_persistence_us: u64,
        document_us: u64,
        interaction_us: u64,
        demand_us: u64,
        present_us: u64,
        patch_count: u32,
        full_lowered: bool,
        interaction_frame_block_us: u64,
        pending_child_artifacts: u32,
        pending_program_artifact_stores: u32,
        pending_program_artifact_loads: u32,
        pending_persistence_artifact_stores: u32,
        pending_persistence_artifact_loads: u32,
        /// Durable checkpoint batches queued behind the transaction in flight.
        pending_durable_batches: u32,
        trusted_parent_rebuilds: u32,
        source_revision: u64,
        runtime_sequence: u64,
        editor_key: FrameEvidenceKey,
        key: FrameEvidenceKey,
    },
    StaleProgramRejected {
        session: String,
        stale_revision: u64,
        latest_revision: u64,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        state_digest: String,
        key: FrameEvidenceKey,
    },
    ProfileInputTarget {
        node: String,
        source_path: String,
        x: f32,
        y: f32,
        sample_count: u32,
        key: FrameEvidenceKey,
    },
    ProfileInputSeeded {
        input_sequence: u64,
        callback_to_host_ns: u64,
        compile_us: u64,
        pending_child_artifacts: u32,
        editor_key: FrameEvidenceKey,
        key: FrameEvidenceKey,
    },
    ResponsiveResizeReady {
        desired_width: u32,
        desired_height: u32,
        current_width: u32,
        current_height: u32,
        baseline_action_count: u32,
        baseline_action_digest: String,
        key: FrameEvidenceKey,
    },
    ResponsiveResizeObserved {
        event_sequence: u64,
        logical_width: u32,
        logical_height: u32,
        previous_surface_epoch: u64,
        key: FrameEvidenceKey,
    },
    ScrollProofFrame {
        ordinal: u32,
        key: FrameEvidenceKey,
    },
    NativeWorkflowReady {
        test_request_id: u64,
        step_count: u32,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        state_digest: String,
        key: FrameEvidenceKey,
    },
    NativeWorkflowTarget {
        request_id: u64,
        ordinal: u32,
        step_id: String,
        source_path: String,
        action_kind: String,
        action_digest: String,
        node: String,
        x: f32,
        y: f32,
        key: FrameEvidenceKey,
    },
    NativeWorkflowStep {
        request_id: u64,
        ordinal: u32,
        step_id: String,
        source_path: String,
        action_kind: String,
        action_digest: String,
        input_first_sequence: u64,
        input_last_sequence: u64,
        input_event_count: u32,
        input_event_digest: String,
        assertion_count: u32,
        source_revision: u64,
        runtime_sequence: u64,
        durable_epoch: u64,
        durable_turn_sequence: u64,
        durable_acked: bool,
        before_state_digest: String,
        state_digest: String,
        key: FrameEvidenceKey,
    },
    NativeWorkflowCompleted {
        test_request_id: u64,
        step_count: u32,
        initial_state_digest: String,
        final_state_digest: String,
        key: FrameEvidenceKey,
    },
    AsyncLaneCompleted {
        lane: AsyncLaneKind,
        request_id: String,
        revision: u64,
        queue_depth: u32,
        queue_wait_us: u64,
        worker_us: u64,
        apply_us: u64,
        end_to_end_us: u64,
        outcome: AsyncLaneOutcome,
        key: FrameEvidenceKey,
    },
    AsyncLaneCompletedBeforePresent {
        surface_id: String,
        process_id: u32,
        lane: AsyncLaneKind,
        request_id: String,
        revision: u64,
        queue_depth: u32,
        queue_wait_us: u64,
        worker_us: u64,
        apply_us: u64,
        end_to_end_us: u64,
        outcome: AsyncLaneOutcome,
    },
}

impl ObserverEvent {
    fn tag(&self) -> u8 {
        match self {
            Self::RoleMetadata(_) => 1,
            Self::InputAccepted(_) => 2,
            Self::FramePresented(_) => 3,
            Self::SourceSwitchAcknowledged { .. } => 4,
            Self::SourceSwitchFinal { .. } => 5,
            Self::TestTarget { .. } => 6,
            Self::TestCompleted { .. } => 7,
            Self::ProofRequested { .. } => 8,
            Self::ProofCompleted { .. } => 9,
            Self::RoleTarget { .. } => 10,
            Self::SourceFailed { .. } => 11,
            Self::TestPointerFrame { .. } => 12,
            Self::StateMounted { .. } => 13,
            Self::ScenarioCheckpoint { .. } => 14,
            Self::PersistenceEvidence { .. } => 15,
            Self::ResponsiveLayoutEvidence { .. } => 16,
            Self::ProfileSample { .. } => 17,
            Self::StaleProgramRejected { .. } => 18,
            Self::ProfileInputTarget { .. } => 19,
            Self::ProfileInputSeeded { .. } => 20,
            Self::ResponsiveResizeReady { .. } => 21,
            Self::ResponsiveResizeObserved { .. } => 22,
            Self::ScrollProofFrame { .. } => 23,
            Self::NativeWorkflowReady { .. } => 24,
            Self::NativeWorkflowTarget { .. } => 25,
            Self::NativeWorkflowStep { .. } => 26,
            Self::NativeWorkflowCompleted { .. } => 27,
            Self::AsyncLaneCompleted { .. } => 28,
            Self::AsyncLaneCompletedBeforePresent { .. } => 29,
        }
    }

    fn validate(&self) -> Result<(), ObserverError> {
        match self {
            Self::RoleMetadata(value) => validate_strings([
                value.surface_id.as_str(),
                value.session_id.as_str(),
                value.adapter_name.as_str(),
                value.adapter_backend.as_str(),
                value.adapter_device_type.as_str(),
                value.surface_format.as_str(),
                value.present_mode.as_str(),
                value.window_backend.as_str(),
            ]),
            Self::InputAccepted(value) => {
                validate_strings([value.event_digest.as_str()])?;
                validate_optional_strings([
                    value.target.as_deref(),
                    value.target_source_path.as_deref(),
                ])
            }
            Self::FramePresented(value) => value.key.validate(),
            Self::SourceSwitchAcknowledged { .. } => Ok(()),
            Self::SourceSwitchFinal { key, .. }
            | Self::ProofRequested { key, .. }
            | Self::ScrollProofFrame { key, .. }
            | Self::ResponsiveResizeObserved { key, .. } => key.validate(),
            Self::TestTarget {
                node, source_path, ..
            } => validate_strings([node.as_str(), source_path.as_str()]),
            Self::TestCompleted { message, .. } => validate_strings([message.as_str()]),
            Self::TestPointerFrame { target, key, .. } => {
                validate_optional_strings([target.as_deref()])?;
                key.validate()
            }
            Self::ProofCompleted {
                key,
                completed_after_key,
                artifact,
                error,
                ..
            } => {
                key.validate()?;
                completed_after_key.validate()?;
                if let Some(artifact) = artifact {
                    artifact.validate()?;
                }
                validate_optional_strings([error.as_deref()])
            }
            Self::RoleTarget { node, .. } => validate_strings([node.as_str()]),
            Self::SourceFailed { stage, message, .. } => {
                validate_strings([stage.as_str(), message.as_str()])
            }
            Self::StateMounted {
                schema_hash,
                migration,
                state_digest,
                key,
                ..
            } => {
                validate_strings([schema_hash.as_str(), state_digest.as_str()])?;
                if let Some(migration) = migration {
                    validate_strings([
                        migration.source_schema_hash.as_str(),
                        migration.target_schema_hash.as_str(),
                    ])?;
                }
                key.validate()
            }
            Self::ScenarioCheckpoint {
                step_id,
                state_digest,
                key,
                ..
            } => {
                validate_strings([step_id.as_str(), state_digest.as_str()])?;
                key.validate()
            }
            Self::PersistenceEvidence {
                before_state_digest,
                after_state_digest,
                key,
                ..
            } => {
                validate_strings([before_state_digest.as_str(), after_state_digest.as_str()])?;
                key.validate()
            }
            Self::ResponsiveLayoutEvidence {
                baseline_key,
                baseline_action_digest,
                action_digest,
                state_digest,
                key,
                ..
            } => {
                baseline_key.validate()?;
                validate_strings([
                    baseline_action_digest.as_str(),
                    action_digest.as_str(),
                    state_digest.as_str(),
                ])?;
                key.validate()
            }
            Self::ProfileSample {
                editor_key, key, ..
            }
            | Self::ProfileInputSeeded {
                editor_key, key, ..
            } => {
                editor_key.validate()?;
                key.validate()
            }
            Self::StaleProgramRejected {
                session,
                state_digest,
                key,
                ..
            } => {
                validate_strings([session.as_str(), state_digest.as_str()])?;
                key.validate()
            }
            Self::ProfileInputTarget {
                node,
                source_path,
                key,
                ..
            } => {
                validate_strings([node.as_str(), source_path.as_str()])?;
                key.validate()
            }
            Self::ResponsiveResizeReady {
                baseline_action_digest,
                key,
                ..
            } => {
                validate_strings([baseline_action_digest.as_str()])?;
                key.validate()
            }
            Self::NativeWorkflowReady {
                state_digest, key, ..
            } => {
                validate_strings([state_digest.as_str()])?;
                key.validate()
            }
            Self::NativeWorkflowTarget {
                step_id,
                source_path,
                action_kind,
                action_digest,
                node,
                key,
                ..
            } => {
                validate_strings([
                    step_id.as_str(),
                    source_path.as_str(),
                    action_kind.as_str(),
                    action_digest.as_str(),
                    node.as_str(),
                ])?;
                key.validate()
            }
            Self::NativeWorkflowStep {
                step_id,
                source_path,
                action_kind,
                action_digest,
                input_event_digest,
                before_state_digest,
                state_digest,
                key,
                ..
            } => {
                validate_strings([
                    step_id.as_str(),
                    source_path.as_str(),
                    action_kind.as_str(),
                    action_digest.as_str(),
                    input_event_digest.as_str(),
                    before_state_digest.as_str(),
                    state_digest.as_str(),
                ])?;
                key.validate()
            }
            Self::NativeWorkflowCompleted {
                initial_state_digest,
                final_state_digest,
                key,
                ..
            } => {
                validate_strings([initial_state_digest.as_str(), final_state_digest.as_str()])?;
                key.validate()
            }
            Self::AsyncLaneCompleted {
                request_id, key, ..
            } => {
                validate_strings([request_id.as_str()])?;
                key.validate()
            }
            Self::AsyncLaneCompletedBeforePresent {
                surface_id,
                request_id,
                ..
            } => validate_strings([surface_id.as_str(), request_id.as_str()]),
        }
    }
}

fn validate_strings<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<(), ObserverError> {
    for value in values {
        if value.len() > MAX_STRING_BYTES {
            return Err(ObserverError::StringTooLarge(value.len()));
        }
    }
    Ok(())
}

fn validate_optional_strings<'a>(
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> Result<(), ObserverError> {
    validate_strings(values.into_iter().flatten())
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

pub struct ObserverClient {
    sender: Option<mpsc::SyncSender<ObserverEvent>>,
    dropped: Arc<AtomicU64>,
    writer: Option<JoinHandle<()>>,
}

impl ObserverClient {
    pub fn from_env() -> Result<Option<Self>, ObserverError> {
        let Some(path) = std::env::var_os(OBSERVER_SOCKET_ENV) else {
            return Ok(None);
        };
        Self::connect(Path::new(&path)).map(Some)
    }

    pub fn connect(path: &Path) -> Result<Self, ObserverError> {
        let stream = UnixStream::connect(path)?;
        stream.set_write_timeout(Some(Duration::from_millis(250)))?;
        let (sender, receiver) = mpsc::sync_channel(CLIENT_QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(0));
        let thread_dropped = Arc::clone(&dropped);
        let writer = thread::Builder::new()
            .name("boon-verifier-observer".to_owned())
            .spawn(move || observer_writer(stream, receiver, thread_dropped))?;
        Ok(Self {
            sender: Some(sender),
            dropped,
            writer: Some(writer),
        })
    }

    pub fn emit(&self, event: ObserverEvent) {
        let Some(sender) = &self.sender else {
            return;
        };
        if sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for ObserverClient {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn observer_writer(
    mut stream: UnixStream,
    receiver: mpsc::Receiver<ObserverEvent>,
    dropped: Arc<AtomicU64>,
) {
    for event in receiver {
        if write_event(&mut stream, &event).is_err() {
            dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
}

pub fn write_event(writer: &mut impl Write, event: &ObserverEvent) -> Result<(), ObserverError> {
    event.validate()?;
    let payload = codec()
        .serialize(event)
        .map_err(ObserverError::InvalidPayload)?;
    let frame_bytes = HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(ObserverError::FrameTooLarge(usize::MAX))?;
    if frame_bytes > MAX_EVENT_BYTES {
        return Err(ObserverError::FrameTooLarge(frame_bytes));
    }

    writer.write_all(&(frame_bytes as u32).to_le_bytes())?;
    writer.write_all(&MAGIC)?;
    writer.write_all(&VERSION.to_le_bytes())?;
    writer.write_all(&[event.tag()])?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_event(reader: &mut impl Read) -> Result<Option<ObserverEvent>, ObserverError> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length[..1]) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if !(HEADER_BYTES..=MAX_EVENT_BYTES).contains(&length) {
        return Err(ObserverError::FrameTooLarge(length));
    }

    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    if bytes[..MAGIC.len()] != MAGIC {
        return Err(ObserverError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(ObserverError::UnsupportedVersion(version));
    }
    let outer_tag = bytes[6];
    if !(1..=LAST_EVENT_TAG).contains(&outer_tag) {
        return Err(ObserverError::UnknownEvent(outer_tag));
    }

    let payload = &bytes[HEADER_BYTES..];
    let mut cursor = Cursor::new(payload);
    let event: ObserverEvent = codec()
        .with_limit(payload.len() as u64)
        .allow_trailing_bytes()
        .deserialize_from(&mut cursor)
        .map_err(ObserverError::InvalidPayload)?;
    let consumed = cursor.position() as usize;
    if consumed != payload.len() {
        return Err(ObserverError::TrailingBytes(payload.len() - consumed));
    }
    if event.tag() != outer_tag {
        return Err(ObserverError::MismatchedEventTag {
            outer: outer_tag,
            payload: event.tag(),
        });
    }
    event.validate()?;
    Ok(Some(event))
}

#[derive(Debug)]
pub enum ObserverError {
    Io(io::Error),
    FrameTooLarge(usize),
    StringTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownEvent(u8),
    MismatchedEventTag { outer: u8, payload: u8 },
    InvalidPayload(bincode::Error),
    TrailingBytes(usize),
}

impl fmt::Display for ObserverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "observer I/O failed: {error}"),
            Self::FrameTooLarge(bytes) => write!(formatter, "observer frame is {bytes} bytes"),
            Self::StringTooLarge(bytes) => write!(formatter, "observer string is {bytes} bytes"),
            Self::InvalidMagic => formatter.write_str("observer frame magic is invalid"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "observer protocol version {version} is unsupported"
                )
            }
            Self::UnknownEvent(tag) => write!(formatter, "observer event tag {tag} is unknown"),
            Self::MismatchedEventTag { outer, payload } => write!(
                formatter,
                "observer outer event tag {outer} does not match payload tag {payload}"
            ),
            Self::InvalidPayload(error) => {
                write!(formatter, "observer event payload is invalid: {error}")
            }
            Self::TrailingBytes(bytes) => {
                write!(formatter, "observer frame has {bytes} trailing bytes")
            }
        }
    }
}

impl std::error::Error for ObserverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidPayload(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for ObserverError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> FrameEvidenceKey {
        FrameEvidenceKey {
            surface_id: "preview".to_owned(),
            process_id: 42,
            session_id: "session".to_owned(),
            frame_id: 1,
            input_id: 2,
            content_id: 3,
            layout_id: 4,
            render_id: 5,
            surface_epoch: 6,
            present_id: 7,
            proof_id: 8,
        }
    }

    #[test]
    fn nested_event_roundtrips_through_bounded_little_endian_frame() {
        let event = ObserverEvent::NativeWorkflowStep {
            request_id: 11,
            ordinal: 2,
            step_id: "increment".to_owned(),
            source_path: "counter.increment.press".to_owned(),
            action_kind: "click".to_owned(),
            action_digest: "action".to_owned(),
            input_first_sequence: 17,
            input_last_sequence: 18,
            input_event_count: 2,
            input_event_digest: "input".to_owned(),
            assertion_count: 3,
            source_revision: 19,
            runtime_sequence: 20,
            durable_epoch: 21,
            durable_turn_sequence: 22,
            durable_acked: true,
            before_state_digest: "before".to_owned(),
            state_digest: "after".to_owned(),
            key: key(),
        };
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).expect("encode event");
        assert_eq!(
            read_event(&mut bytes.as_slice()).expect("decode event"),
            Some(event)
        );
    }

    #[test]
    fn decoder_rejects_outer_tag_mismatch_and_trailing_bytes() {
        let event = ObserverEvent::ScrollProofFrame {
            ordinal: 3,
            key: key(),
        };
        let mut mismatched = Vec::new();
        write_event(&mut mismatched, &event).expect("encode event");
        mismatched[10] = 1;
        assert!(matches!(
            read_event(&mut mismatched.as_slice()),
            Err(ObserverError::MismatchedEventTag { .. })
        ));

        let mut trailing = Vec::new();
        write_event(&mut trailing, &event).expect("encode event");
        let length = u32::from_le_bytes(trailing[..4].try_into().expect("length"));
        trailing[..4].copy_from_slice(&(length + 1).to_le_bytes());
        trailing.push(0xff);
        assert!(matches!(
            read_event(&mut trailing.as_slice()),
            Err(ObserverError::TrailingBytes(1))
        ));
    }

    #[test]
    fn encoder_and_decoder_enforce_string_ceiling() {
        let oversized = ObserverEvent::TestCompleted {
            request_id: 1,
            passed: false,
            semantic_assertions_proven: false,
            completed_steps: 0,
            message: "x".repeat(MAX_STRING_BYTES + 1),
        };
        assert!(matches!(
            write_event(&mut Vec::new(), &oversized),
            Err(ObserverError::StringTooLarge(_))
        ));
    }
}
