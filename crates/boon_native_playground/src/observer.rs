use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
pub const SCROLL_PROOF_ORDINAL_ENV: &str = "BOON_VERIFY_SCROLL_PROOF_ORDINAL";
pub const STALE_PROGRAM_EVIDENCE_ENV: &str = "BOON_VERIFY_STALE_PROGRAM_EVIDENCE";
pub const NATIVE_WORKFLOW_STEPS_ENV: &str = "BOON_VERIFY_NATIVE_WORKFLOW_STEPS";
pub const NATIVE_WORKFLOW_PROOF_STEPS_ENV: &str = "BOON_VERIFY_NATIVE_WORKFLOW_PROOF_STEPS";

const MAGIC: [u8; 4] = *b"BNVO";
const VERSION: u16 = 9;
const HEADER_BYTES: usize = 7;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024;
const CLIENT_QUEUE_DEPTH: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ObserverRole {
    Preview = 1,
    Dev = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TestPointerPhase {
    Move = 1,
    Hover = 2,
    Down = 3,
    Up = 4,
    State = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StartupDisposition {
    Fresh = 1,
    Restored = 2,
    Migrated = 3,
}

impl StartupDisposition {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Fresh),
            2 => Ok(Self::Restored),
            3 => Ok(Self::Migrated),
            _ => Err(ObserverError::InvalidEnum("startup disposition", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupMigrationEvidence {
    pub source_schema_version: u64,
    pub source_schema_hash: String,
    pub target_schema_version: u64,
    pub target_schema_hash: String,
    pub step_count: u32,
}

impl PersistenceEvidenceKind {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Exported),
            2 => Ok(Self::CorruptionRejected),
            3 => Ok(Self::ClearedAndStartedOver),
            4 => Ok(Self::ImportPreviewed),
            5 => Ok(Self::ImportActivated),
            6 => Ok(Self::MigrationActivated),
            7 => Ok(Self::MigrationProductRestored),
            _ => Err(ObserverError::InvalidEnum(
                "persistence evidence kind",
                value,
            )),
        }
    }
}

impl TestPointerPhase {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Move),
            2 => Ok(Self::Hover),
            3 => Ok(Self::Down),
            4 => Ok(Self::Up),
            5 => Ok(Self::State),
            _ => Err(ObserverError::InvalidEnum("test pointer phase", value)),
        }
    }
}

impl ObserverRole {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Preview),
            2 => Ok(Self::Dev),
            _ => Err(ObserverError::InvalidEnum("role", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

impl InputKind {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::PointerMove),
            2 => Ok(Self::PointerButton),
            3 => Ok(Self::Wheel),
            4 => Ok(Self::Keyboard),
            5 => Ok(Self::Text),
            6 => Ok(Self::Ime),
            7 => Ok(Self::Focus),
            8 => Ok(Self::Resize),
            9 => Ok(Self::Accessibility),
            10 => Ok(Self::Close),
            11 => Ok(Self::Sensitive),
            _ => Err(ObserverError::InvalidEnum("input kind", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum AsyncLaneKind {
    ChildProgramCompile = 1,
    PersistenceTurn = 2,
    ProgramArtifactStore = 3,
    ProgramArtifactLoad = 4,
    ProofReadback = 5,
}

impl AsyncLaneKind {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::ChildProgramCompile),
            2 => Ok(Self::PersistenceTurn),
            3 => Ok(Self::ProgramArtifactStore),
            4 => Ok(Self::ProgramArtifactLoad),
            5 => Ok(Self::ProofReadback),
            _ => Err(ObserverError::InvalidEnum("async lane kind", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum AsyncLaneOutcome {
    Applied = 1,
    StaleRejected = 2,
    Failed = 3,
}

impl AsyncLaneOutcome {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Applied),
            2 => Ok(Self::StaleRejected),
            3 => Ok(Self::Failed),
            _ => Err(ObserverError::InvalidEnum("async lane outcome", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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

    fn encode(&self, out: &mut Encoder) -> Result<(), ObserverError> {
        out.string(&self.surface_id)?;
        out.u32(self.process_id);
        out.string(&self.session_id)?;
        out.u64(self.frame_id);
        out.u64(self.input_id);
        out.u64(self.content_id);
        out.u64(self.layout_id);
        out.u64(self.render_id);
        out.u64(self.surface_epoch);
        out.u64(self.present_id);
        out.u64(self.proof_id);
        Ok(())
    }

    fn decode(input: &mut Decoder<'_>) -> Result<Self, ObserverError> {
        Ok(Self {
            surface_id: input.string()?,
            process_id: input.u32()?,
            session_id: input.string()?,
            frame_id: input.u64()?,
            input_id: input.u64()?,
            content_id: input.u64()?,
            layout_id: input.u64()?,
            render_id: input.u64()?,
            surface_epoch: input.u64()?,
            present_id: input.u64()?,
            proof_id: input.u64()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct ProofArtifact {
    pub path: String,
    pub sha256: String,
    pub byte_len: u64,
    pub capture_method: String,
    pub capture_token_digest: String,
    pub nonblank_samples: u64,
    pub unique_rgba_values: u64,
}

#[derive(Clone, Debug, PartialEq)]
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
        }
    }

    fn encode(&self, out: &mut Encoder) -> Result<(), ObserverError> {
        match self {
            Self::RoleMetadata(value) => {
                out.u8(value.role as u8);
                out.u32(value.pid);
                out.string(&value.surface_id)?;
                out.string(&value.session_id)?;
                out.u64(value.surface_epoch);
                out.f32(value.logical_width);
                out.f32(value.logical_height);
                out.u32(value.physical_width);
                out.u32(value.physical_height);
                out.f64(value.scale);
                out.string(&value.adapter_name)?;
                out.string(&value.adapter_backend)?;
                out.string(&value.adapter_device_type)?;
                out.bool(value.software_adapter);
                out.string(&value.surface_format)?;
                out.string(&value.present_mode)?;
                out.string(&value.window_backend)?;
            }
            Self::InputAccepted(value) => {
                out.u8(value.role as u8);
                out.u64(value.event_sequence);
                out.bool(value.real_os);
                out.u64(value.callback_to_host_ns);
                out.u64(value.surface_epoch);
                out.u8(value.kind as u8);
                out.u8(match value.pointer_button_pressed {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                });
                out.optional_f32(value.pointer_x);
                out.optional_f32(value.pointer_y);
                out.optional_string(value.target.as_deref())?;
                out.optional_string(value.target_source_path.as_deref())?;
                out.string(&value.event_digest)?;
                out.bool(value.visible_change);
            }
            Self::FramePresented(value) => {
                out.u8(value.role as u8);
                value.key.encode(out)?;
                out.optional_u64(value.event_sequence);
                out.optional_u8(value.input_kind.map(|kind| kind as u8));
                out.u64(value.callback_to_host_ns);
                out.u64(value.input_to_present_us);
                out.u64(value.event_dispatch_us);
                out.u64(value.executor_us);
                out.u64(value.runtime_document_us);
                out.u64(value.document_update_us);
                out.u64(value.render_us);
                out.u64(value.document_scene_convert_us);
                out.u64(value.scene_key_us);
                out.u64(value.rect_vertices_us);
                out.u64(value.asset_prepare_us);
                out.u64(value.quad_batch_key_us);
                out.u64(value.quad_upload_us);
                out.u64(value.draw_pass_us);
                out.u64(value.retained_metrics_us);
                out.u64(value.text_render_us);
                out.u64(value.submit_us);
                out.u64(value.present_us);
                out.u64(value.frame_us);
                out.u64(value.observer_drop_count);
            }
            Self::SourceSwitchAcknowledged {
                revision,
                elapsed_us,
            } => {
                out.u64(*revision);
                out.u64(*elapsed_us);
            }
            Self::SourceSwitchFinal {
                revision,
                elapsed_us,
                compile_us,
                post_compile_us,
                key,
            } => {
                out.u64(*revision);
                out.u64(*elapsed_us);
                out.u64(*compile_us);
                out.u64(*post_compile_us);
                key.encode(out)?;
            }
            Self::TestTarget {
                request_id,
                node,
                source_path,
                x,
                y,
            } => {
                out.u64(*request_id);
                out.string(node)?;
                out.string(source_path)?;
                out.f32(*x);
                out.f32(*y);
            }
            Self::TestCompleted {
                request_id,
                passed,
                semantic_assertions_proven,
                completed_steps,
                message,
            } => {
                out.u64(*request_id);
                out.bool(*passed);
                out.bool(*semantic_assertions_proven);
                out.u32(*completed_steps);
                out.string(message)?;
            }
            Self::TestPointerFrame {
                request_id,
                step_index,
                phase,
                x,
                y,
                target,
                runtime_sequence,
                key,
            } => {
                out.u64(*request_id);
                out.u32(*step_index);
                out.u8(*phase as u8);
                out.f32(*x);
                out.f32(*y);
                out.optional_string(target.as_deref())?;
                out.u64(*runtime_sequence);
                key.encode(out)?;
            }
            Self::ProofRequested {
                key,
                snapshot_prepare_us,
            } => {
                key.encode(out)?;
                out.u64(*snapshot_prepare_us);
            }
            Self::ProofCompleted {
                key,
                completed_after_key,
                elapsed_us,
                replaced_count,
                result_drop_count,
                artifact,
                error,
            } => {
                key.encode(out)?;
                completed_after_key.encode(out)?;
                out.u64(*elapsed_us);
                out.u64(*replaced_count);
                out.u64(*result_drop_count);
                out.bool(artifact.is_some());
                if let Some(artifact) = artifact {
                    out.string(&artifact.path)?;
                    out.string(&artifact.sha256)?;
                    out.u64(artifact.byte_len);
                    out.string(&artifact.capture_method)?;
                    out.string(&artifact.capture_token_digest)?;
                    out.u64(artifact.nonblank_samples);
                    out.u64(artifact.unique_rgba_values);
                }
                out.optional_string(error.as_deref())?;
            }
            Self::RoleTarget { role, node, x, y } => {
                out.u8(*role as u8);
                out.string(node)?;
                out.f32(*x);
                out.f32(*y);
            }
            Self::SourceFailed {
                revision,
                stage,
                message,
            } => {
                out.u64(*revision);
                out.string(stage)?;
                out.string(message)?;
            }
            Self::StateMounted {
                disposition,
                schema_version,
                schema_hash,
                migration,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                state_digest,
                key,
            } => {
                out.u8(*disposition as u8);
                out.u64(*schema_version);
                out.string(schema_hash)?;
                out.bool(migration.is_some());
                if let Some(migration) = migration {
                    out.u64(migration.source_schema_version);
                    out.string(&migration.source_schema_hash)?;
                    out.u64(migration.target_schema_version);
                    out.string(&migration.target_schema_hash)?;
                    out.u32(migration.step_count);
                }
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                out.string(state_digest)?;
                key.encode(out)?;
            }
            Self::ScenarioCheckpoint {
                request_id,
                step_id,
                assertion_count,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                state_digest,
                key,
            } => {
                out.u64(*request_id);
                out.string(step_id)?;
                out.u32(*assertion_count);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                out.string(state_digest)?;
                key.encode(out)?;
            }
            Self::PersistenceEvidence {
                kind,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                before_state_digest,
                after_state_digest,
                key,
            } => {
                out.u8(*kind as u8);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                out.string(before_state_digest)?;
                out.string(after_state_digest)?;
                key.encode(out)?;
            }
            Self::ResponsiveLayoutEvidence {
                resize_sequence,
                logical_width,
                logical_height,
                action_count,
                action_digest,
                state_digest,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                key,
            } => {
                out.u64(*resize_sequence);
                out.u32(*logical_width);
                out.u32(*logical_height);
                out.u32(*action_count);
                out.string(action_digest)?;
                out.string(state_digest)?;
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                key.encode(out)?;
            }
            Self::ProfileSample {
                ordinal,
                input_sequence,
                callback_to_host_ns,
                editor_visible_us,
                preview_visible_us,
                compile_us,
                parent_dispatch_us,
                parent_executor_us,
                parent_runtime_document_us,
                parent_persistence_us,
                completion_us,
                completion_executor_us,
                completion_runtime_document_us,
                completion_persistence_us,
                document_us,
                interaction_us,
                demand_us,
                present_us,
                patch_count,
                full_lowered,
                interaction_frame_block_us,
                pending_child_artifacts,
                pending_program_artifact_stores,
                pending_program_artifact_loads,
                pending_persistence_artifact_stores,
                pending_persistence_artifact_loads,
                pending_durable_batches,
                trusted_parent_rebuilds,
                source_revision,
                runtime_sequence,
                editor_key,
                key,
            } => {
                out.u32(*ordinal);
                out.u64(*input_sequence);
                out.u64(*callback_to_host_ns);
                out.u64(*editor_visible_us);
                out.u64(*preview_visible_us);
                out.u64(*compile_us);
                out.u64(*parent_dispatch_us);
                out.u64(*parent_executor_us);
                out.u64(*parent_runtime_document_us);
                out.u64(*parent_persistence_us);
                out.u64(*completion_us);
                out.u64(*completion_executor_us);
                out.u64(*completion_runtime_document_us);
                out.u64(*completion_persistence_us);
                out.u64(*document_us);
                out.u64(*interaction_us);
                out.u64(*demand_us);
                out.u64(*present_us);
                out.u32(*patch_count);
                out.bool(*full_lowered);
                out.u64(*interaction_frame_block_us);
                out.u32(*pending_child_artifacts);
                out.u32(*pending_program_artifact_stores);
                out.u32(*pending_program_artifact_loads);
                out.u32(*pending_persistence_artifact_stores);
                out.u32(*pending_persistence_artifact_loads);
                out.u32(*pending_durable_batches);
                out.u32(*trusted_parent_rebuilds);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                editor_key.encode(out)?;
                key.encode(out)?;
            }
            Self::StaleProgramRejected {
                session,
                stale_revision,
                latest_revision,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                state_digest,
                key,
            } => {
                out.string(session)?;
                out.u64(*stale_revision);
                out.u64(*latest_revision);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                out.string(state_digest)?;
                key.encode(out)?;
            }
            Self::ProfileInputTarget {
                node,
                source_path,
                x,
                y,
                sample_count,
                key,
            } => {
                out.string(node)?;
                out.string(source_path)?;
                out.f32(*x);
                out.f32(*y);
                out.u32(*sample_count);
                key.encode(out)?;
            }
            Self::ProfileInputSeeded {
                input_sequence,
                callback_to_host_ns,
                compile_us,
                pending_child_artifacts,
                editor_key,
                key,
            } => {
                out.u64(*input_sequence);
                out.u64(*callback_to_host_ns);
                out.u64(*compile_us);
                out.u32(*pending_child_artifacts);
                editor_key.encode(out)?;
                key.encode(out)?;
            }
            Self::ResponsiveResizeReady {
                desired_width,
                desired_height,
                current_width,
                current_height,
                key,
            } => {
                out.u32(*desired_width);
                out.u32(*desired_height);
                out.u32(*current_width);
                out.u32(*current_height);
                key.encode(out)?;
            }
            Self::ResponsiveResizeObserved {
                event_sequence,
                logical_width,
                logical_height,
                previous_surface_epoch,
                key,
            } => {
                out.u64(*event_sequence);
                out.u32(*logical_width);
                out.u32(*logical_height);
                out.u64(*previous_surface_epoch);
                key.encode(out)?;
            }
            Self::ScrollProofFrame { ordinal, key } => {
                out.u32(*ordinal);
                key.encode(out)?;
            }
            Self::NativeWorkflowReady {
                test_request_id,
                step_count,
                source_revision,
                runtime_sequence,
                durable_epoch,
                state_digest,
                key,
            } => {
                out.u64(*test_request_id);
                out.u32(*step_count);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.string(state_digest)?;
                key.encode(out)?;
            }
            Self::NativeWorkflowTarget {
                request_id,
                ordinal,
                step_id,
                source_path,
                action_kind,
                action_digest,
                node,
                x,
                y,
                key,
            } => {
                out.u64(*request_id);
                out.u32(*ordinal);
                out.string(step_id)?;
                out.string(source_path)?;
                out.string(action_kind)?;
                out.string(action_digest)?;
                out.string(node)?;
                out.f32(*x);
                out.f32(*y);
                key.encode(out)?;
            }
            Self::NativeWorkflowStep {
                request_id,
                ordinal,
                step_id,
                source_path,
                action_kind,
                action_digest,
                input_first_sequence,
                input_last_sequence,
                input_event_count,
                input_event_digest,
                assertion_count,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                durable_acked,
                before_state_digest,
                state_digest,
                key,
            } => {
                out.u64(*request_id);
                out.u32(*ordinal);
                out.string(step_id)?;
                out.string(source_path)?;
                out.string(action_kind)?;
                out.string(action_digest)?;
                out.u64(*input_first_sequence);
                out.u64(*input_last_sequence);
                out.u32(*input_event_count);
                out.string(input_event_digest)?;
                out.u32(*assertion_count);
                out.u64(*source_revision);
                out.u64(*runtime_sequence);
                out.u64(*durable_epoch);
                out.u64(*durable_turn_sequence);
                out.bool(*durable_acked);
                out.string(before_state_digest)?;
                out.string(state_digest)?;
                key.encode(out)?;
            }
            Self::NativeWorkflowCompleted {
                test_request_id,
                step_count,
                initial_state_digest,
                final_state_digest,
                key,
            } => {
                out.u64(*test_request_id);
                out.u32(*step_count);
                out.string(initial_state_digest)?;
                out.string(final_state_digest)?;
                key.encode(out)?;
            }
            Self::AsyncLaneCompleted {
                lane,
                request_id,
                revision,
                queue_depth,
                queue_wait_us,
                worker_us,
                apply_us,
                end_to_end_us,
                outcome,
                key,
            } => {
                out.u8(*lane as u8);
                out.string(request_id)?;
                out.u64(*revision);
                out.u32(*queue_depth);
                out.u64(*queue_wait_us);
                out.u64(*worker_us);
                out.u64(*apply_us);
                out.u64(*end_to_end_us);
                out.u8(*outcome as u8);
                key.encode(out)?;
            }
        }
        Ok(())
    }

    fn decode(tag: u8, input: &mut Decoder<'_>) -> Result<Self, ObserverError> {
        let event = match tag {
            1 => Self::RoleMetadata(RoleMetadata {
                role: ObserverRole::decode(input.u8()?)?,
                pid: input.u32()?,
                surface_id: input.string()?,
                session_id: input.string()?,
                surface_epoch: input.u64()?,
                logical_width: input.f32()?,
                logical_height: input.f32()?,
                physical_width: input.u32()?,
                physical_height: input.u32()?,
                scale: input.f64()?,
                adapter_name: input.string()?,
                adapter_backend: input.string()?,
                adapter_device_type: input.string()?,
                software_adapter: input.bool()?,
                surface_format: input.string()?,
                present_mode: input.string()?,
                window_backend: input.string()?,
            }),
            2 => Self::InputAccepted(InputAccepted {
                role: ObserverRole::decode(input.u8()?)?,
                event_sequence: input.u64()?,
                real_os: input.bool()?,
                callback_to_host_ns: input.u64()?,
                surface_epoch: input.u64()?,
                kind: InputKind::decode(input.u8()?)?,
                pointer_button_pressed: match input.u8()? {
                    0 => None,
                    1 => Some(true),
                    2 => Some(false),
                    value => return Err(ObserverError::InvalidEnum("pointer button state", value)),
                },
                pointer_x: input.optional_f32()?,
                pointer_y: input.optional_f32()?,
                target: input.optional_string()?,
                target_source_path: input.optional_string()?,
                event_digest: input.string()?,
                visible_change: input.bool()?,
            }),
            3 => Self::FramePresented(FramePresented {
                role: ObserverRole::decode(input.u8()?)?,
                key: FrameEvidenceKey::decode(input)?,
                event_sequence: input.optional_u64()?,
                input_kind: input.optional_u8()?.map(InputKind::decode).transpose()?,
                callback_to_host_ns: input.u64()?,
                input_to_present_us: input.u64()?,
                event_dispatch_us: input.u64()?,
                executor_us: input.u64()?,
                runtime_document_us: input.u64()?,
                document_update_us: input.u64()?,
                render_us: input.u64()?,
                document_scene_convert_us: input.u64()?,
                scene_key_us: input.u64()?,
                rect_vertices_us: input.u64()?,
                asset_prepare_us: input.u64()?,
                quad_batch_key_us: input.u64()?,
                quad_upload_us: input.u64()?,
                draw_pass_us: input.u64()?,
                retained_metrics_us: input.u64()?,
                text_render_us: input.u64()?,
                submit_us: input.u64()?,
                present_us: input.u64()?,
                frame_us: input.u64()?,
                observer_drop_count: input.u64()?,
            }),
            4 => Self::SourceSwitchAcknowledged {
                revision: input.u64()?,
                elapsed_us: input.u64()?,
            },
            5 => Self::SourceSwitchFinal {
                revision: input.u64()?,
                elapsed_us: input.u64()?,
                compile_us: input.u64()?,
                post_compile_us: input.u64()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            6 => Self::TestTarget {
                request_id: input.u64()?,
                node: input.string()?,
                source_path: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
            },
            7 => Self::TestCompleted {
                request_id: input.u64()?,
                passed: input.bool()?,
                semantic_assertions_proven: input.bool()?,
                completed_steps: input.u32()?,
                message: input.string()?,
            },
            8 => Self::ProofRequested {
                key: FrameEvidenceKey::decode(input)?,
                snapshot_prepare_us: input.u64()?,
            },
            9 => {
                let key = FrameEvidenceKey::decode(input)?;
                let completed_after_key = FrameEvidenceKey::decode(input)?;
                let elapsed_us = input.u64()?;
                let replaced_count = input.u64()?;
                let result_drop_count = input.u64()?;
                let artifact = if input.bool()? {
                    Some(ProofArtifact {
                        path: input.string()?,
                        sha256: input.string()?,
                        byte_len: input.u64()?,
                        capture_method: input.string()?,
                        capture_token_digest: input.string()?,
                        nonblank_samples: input.u64()?,
                        unique_rgba_values: input.u64()?,
                    })
                } else {
                    None
                };
                Self::ProofCompleted {
                    key,
                    completed_after_key,
                    elapsed_us,
                    replaced_count,
                    result_drop_count,
                    artifact,
                    error: input.optional_string()?,
                }
            }
            10 => Self::RoleTarget {
                role: ObserverRole::decode(input.u8()?)?,
                node: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
            },
            11 => Self::SourceFailed {
                revision: input.u64()?,
                stage: input.string()?,
                message: input.string()?,
            },
            12 => Self::TestPointerFrame {
                request_id: input.u64()?,
                step_index: input.u32()?,
                phase: TestPointerPhase::decode(input.u8()?)?,
                x: input.f32()?,
                y: input.f32()?,
                target: input.optional_string()?,
                runtime_sequence: input.u64()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            13 => Self::StateMounted {
                disposition: StartupDisposition::decode(input.u8()?)?,
                schema_version: input.u64()?,
                schema_hash: input.string()?,
                migration: if input.bool()? {
                    Some(StartupMigrationEvidence {
                        source_schema_version: input.u64()?,
                        source_schema_hash: input.string()?,
                        target_schema_version: input.u64()?,
                        target_schema_hash: input.string()?,
                        step_count: input.u32()?,
                    })
                } else {
                    None
                },
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            14 => Self::ScenarioCheckpoint {
                request_id: input.u64()?,
                step_id: input.string()?,
                assertion_count: input.u32()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            15 => Self::PersistenceEvidence {
                kind: PersistenceEvidenceKind::decode(input.u8()?)?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                before_state_digest: input.string()?,
                after_state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            16 => Self::ResponsiveLayoutEvidence {
                resize_sequence: input.u64()?,
                logical_width: input.u32()?,
                logical_height: input.u32()?,
                action_count: input.u32()?,
                action_digest: input.string()?,
                state_digest: input.string()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            17 => Self::ProfileSample {
                ordinal: input.u32()?,
                input_sequence: input.u64()?,
                callback_to_host_ns: input.u64()?,
                editor_visible_us: input.u64()?,
                preview_visible_us: input.u64()?,
                compile_us: input.u64()?,
                parent_dispatch_us: input.u64()?,
                parent_executor_us: input.u64()?,
                parent_runtime_document_us: input.u64()?,
                parent_persistence_us: input.u64()?,
                completion_us: input.u64()?,
                completion_executor_us: input.u64()?,
                completion_runtime_document_us: input.u64()?,
                completion_persistence_us: input.u64()?,
                document_us: input.u64()?,
                interaction_us: input.u64()?,
                demand_us: input.u64()?,
                present_us: input.u64()?,
                patch_count: input.u32()?,
                full_lowered: input.bool()?,
                interaction_frame_block_us: input.u64()?,
                pending_child_artifacts: input.u32()?,
                pending_program_artifact_stores: input.u32()?,
                pending_program_artifact_loads: input.u32()?,
                pending_persistence_artifact_stores: input.u32()?,
                pending_persistence_artifact_loads: input.u32()?,
                pending_durable_batches: input.u32()?,
                trusted_parent_rebuilds: input.u32()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                editor_key: FrameEvidenceKey::decode(input)?,
                key: FrameEvidenceKey::decode(input)?,
            },
            18 => Self::StaleProgramRejected {
                session: input.string()?,
                stale_revision: input.u64()?,
                latest_revision: input.u64()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            19 => Self::ProfileInputTarget {
                node: input.string()?,
                source_path: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
                sample_count: input.u32()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            20 => Self::ProfileInputSeeded {
                input_sequence: input.u64()?,
                callback_to_host_ns: input.u64()?,
                compile_us: input.u64()?,
                pending_child_artifacts: input.u32()?,
                editor_key: FrameEvidenceKey::decode(input)?,
                key: FrameEvidenceKey::decode(input)?,
            },
            21 => Self::ResponsiveResizeReady {
                desired_width: input.u32()?,
                desired_height: input.u32()?,
                current_width: input.u32()?,
                current_height: input.u32()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            22 => Self::ResponsiveResizeObserved {
                event_sequence: input.u64()?,
                logical_width: input.u32()?,
                logical_height: input.u32()?,
                previous_surface_epoch: input.u64()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            23 => Self::ScrollProofFrame {
                ordinal: input.u32()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            24 => Self::NativeWorkflowReady {
                test_request_id: input.u64()?,
                step_count: input.u32()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            25 => Self::NativeWorkflowTarget {
                request_id: input.u64()?,
                ordinal: input.u32()?,
                step_id: input.string()?,
                source_path: input.string()?,
                action_kind: input.string()?,
                action_digest: input.string()?,
                node: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            26 => Self::NativeWorkflowStep {
                request_id: input.u64()?,
                ordinal: input.u32()?,
                step_id: input.string()?,
                source_path: input.string()?,
                action_kind: input.string()?,
                action_digest: input.string()?,
                input_first_sequence: input.u64()?,
                input_last_sequence: input.u64()?,
                input_event_count: input.u32()?,
                input_event_digest: input.string()?,
                assertion_count: input.u32()?,
                source_revision: input.u64()?,
                runtime_sequence: input.u64()?,
                durable_epoch: input.u64()?,
                durable_turn_sequence: input.u64()?,
                durable_acked: input.bool()?,
                before_state_digest: input.string()?,
                state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            27 => Self::NativeWorkflowCompleted {
                test_request_id: input.u64()?,
                step_count: input.u32()?,
                initial_state_digest: input.string()?,
                final_state_digest: input.string()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            28 => Self::AsyncLaneCompleted {
                lane: AsyncLaneKind::decode(input.u8()?)?,
                request_id: input.string()?,
                revision: input.u64()?,
                queue_depth: input.u32()?,
                queue_wait_us: input.u64()?,
                worker_us: input.u64()?,
                apply_us: input.u64()?,
                end_to_end_us: input.u64()?,
                outcome: AsyncLaneOutcome::decode(input.u8()?)?,
                key: FrameEvidenceKey::decode(input)?,
            },
            _ => return Err(ObserverError::UnknownEvent(tag)),
        };
        input.finish()?;
        Ok(event)
    }
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
    let mut encoded = Encoder::default();
    encoded.bytes.extend_from_slice(&MAGIC);
    encoded.u16(VERSION);
    encoded.u8(event.tag());
    event.encode(&mut encoded)?;
    if encoded.bytes.len() > MAX_EVENT_BYTES {
        return Err(ObserverError::FrameTooLarge(encoded.bytes.len()));
    }
    writer.write_all(&(encoded.bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&encoded.bytes)?;
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
    if bytes[..4] != MAGIC {
        return Err(ObserverError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(ObserverError::UnsupportedVersion(version));
    }
    let mut input = Decoder::new(&bytes[HEADER_BYTES..]);
    ObserverEvent::decode(bytes[6], &mut input).map(Some)
}

#[derive(Debug)]
pub enum ObserverError {
    Io(io::Error),
    FrameTooLarge(usize),
    StringTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownEvent(u8),
    InvalidEnum(&'static str, u8),
    InvalidBool(u8),
    InvalidOption(u8),
    InvalidUtf8(std::str::Utf8Error),
    Truncated,
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
            Self::InvalidEnum(name, value) => {
                write!(formatter, "observer {name} value {value} is invalid")
            }
            Self::InvalidBool(value) => write!(formatter, "observer bool {value} is invalid"),
            Self::InvalidOption(value) => write!(formatter, "observer option {value} is invalid"),
            Self::InvalidUtf8(error) => write!(formatter, "observer UTF-8 is invalid: {error}"),
            Self::Truncated => formatter.write_str("observer frame is truncated"),
            Self::TrailingBytes(bytes) => {
                write!(formatter, "observer frame has {bytes} trailing bytes")
            }
        }
    }
}

impl std::error::Error for ObserverError {}

impl From<io::Error> for ObserverError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
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

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    fn optional_u8(&mut self, value: Option<u8>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u8(value);
            }
            None => self.u8(0),
        }
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

    fn optional_f32(&mut self, value: Option<f32>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.f32(value);
            }
            None => self.u8(0),
        }
    }

    fn optional_string(&mut self, value: Option<&str>) -> Result<(), ObserverError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value)?;
            }
            None => self.u8(0),
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), ObserverError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ObserverError::StringTooLarge(value.len()));
        }
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
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

    fn take(&mut self, count: usize) -> Result<&'a [u8], ObserverError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ObserverError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ObserverError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ObserverError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, ObserverError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(ObserverError::InvalidBool(value)),
        }
    }

    fn u32(&mut self) -> Result<u32, ObserverError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ObserverError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn f32(&mut self) -> Result<f32, ObserverError> {
        self.u32().map(f32::from_bits)
    }

    fn f64(&mut self) -> Result<f64, ObserverError> {
        self.u64().map(f64::from_bits)
    }

    fn optional_u8(&mut self) -> Result<Option<u8>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u8().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u64().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_f32(&mut self) -> Result<Option<f32>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.f32().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_string(&mut self) -> Result<Option<String>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn string(&mut self) -> Result<String, ObserverError> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(ObserverError::StringTooLarge(length));
        }
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(ObserverError::InvalidUtf8)
    }

    fn finish(&self) -> Result<(), ObserverError> {
        let trailing = self.bytes.len().saturating_sub(self.offset);
        if trailing == 0 {
            Ok(())
        } else {
            Err(ObserverError::TrailingBytes(trailing))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: u64) -> FrameEvidenceKey {
        FrameEvidenceKey {
            surface_id: "preview-surface".to_owned(),
            process_id: 41,
            session_id: "launch-primary".to_owned(),
            frame_id: frame,
            input_id: frame + 1,
            content_id: frame + 2,
            layout_id: frame + 3,
            render_id: frame + 4,
            surface_epoch: frame + 5,
            present_id: frame + 6,
            proof_id: frame + 7,
        }
    }

    fn roundtrip(event: ObserverEvent) {
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).expect("encode observer event");
        assert_eq!(read_event(&mut bytes.as_slice()).unwrap(), Some(event));
    }

    #[test]
    fn observer_codec_roundtrips_identity_timing_and_artifact_records() {
        roundtrip(ObserverEvent::InputAccepted(InputAccepted {
            role: ObserverRole::Dev,
            event_sequence: 3,
            real_os: true,
            callback_to_host_ns: 12,
            surface_epoch: 2,
            kind: InputKind::PointerButton,
            pointer_button_pressed: Some(true),
            pointer_x: Some(20.0),
            pointer_y: Some(30.0),
            target: Some("dev.test".to_owned()),
            target_source_path: None,
            event_digest: "0".repeat(64),
            visible_change: true,
        }));
        roundtrip(ObserverEvent::RoleMetadata(RoleMetadata {
            role: ObserverRole::Preview,
            pid: 41,
            surface_id: "preview-surface".to_owned(),
            session_id: "launch-primary".to_owned(),
            surface_epoch: 2,
            logical_width: 390.0,
            logical_height: 844.0,
            physical_width: 390,
            physical_height: 844,
            scale: 1.0,
            adapter_name: "adapter".to_owned(),
            adapter_backend: "vulkan".to_owned(),
            adapter_device_type: "discrete-gpu".to_owned(),
            software_adapter: false,
            surface_format: "rgba8".to_owned(),
            present_mode: "fifo".to_owned(),
            window_backend: "wayland".to_owned(),
        }));
        roundtrip(ObserverEvent::FramePresented(FramePresented {
            role: ObserverRole::Preview,
            key: key(10),
            event_sequence: Some(4),
            input_kind: Some(InputKind::Wheel),
            callback_to_host_ns: 123,
            input_to_present_us: 456,
            event_dispatch_us: 11,
            executor_us: 5,
            runtime_document_us: 6,
            document_update_us: 12,
            render_us: 7,
            document_scene_convert_us: 1,
            scene_key_us: 2,
            rect_vertices_us: 3,
            asset_prepare_us: 4,
            quad_batch_key_us: 5,
            quad_upload_us: 6,
            draw_pass_us: 7,
            retained_metrics_us: 8,
            text_render_us: 9,
            submit_us: 8,
            present_us: 9,
            frame_us: 10,
            observer_drop_count: 0,
        }));
        roundtrip(ObserverEvent::SourceSwitchFinal {
            revision: 11,
            elapsed_us: 12_345,
            compile_us: 2_345,
            post_compile_us: 9_500,
            key: key(11),
        });
        roundtrip(ObserverEvent::ProofCompleted {
            key: key(20),
            completed_after_key: key(22),
            elapsed_us: 1_234,
            replaced_count: 2,
            result_drop_count: 0,
            artifact: Some(ProofArtifact {
                path: "target/proof.png".to_owned(),
                sha256: "a".repeat(64),
                byte_len: 42,
                capture_method: "app-owned-render-target-readback".to_owned(),
                capture_token_digest: "b".repeat(64),
                nonblank_samples: 10,
                unique_rgba_values: 3,
            }),
            error: None,
        });
        roundtrip(ObserverEvent::TestPointerFrame {
            request_id: 7,
            step_index: 2,
            phase: TestPointerPhase::Hover,
            x: 120.5,
            y: 88.25,
            target: Some("counter.increment".to_owned()),
            runtime_sequence: 3,
            key: key(30),
        });
        roundtrip(ObserverEvent::TestCompleted {
            request_id: 7,
            passed: true,
            semantic_assertions_proven: true,
            completed_steps: 3,
            message: "host input and semantic assertions passed".to_owned(),
        });
        roundtrip(ObserverEvent::SourceFailed {
            revision: 9,
            stage: "runtime-mount".to_owned(),
            message: "invalid retained document".to_owned(),
        });
        roundtrip(ObserverEvent::StateMounted {
            disposition: StartupDisposition::Migrated,
            schema_version: 2,
            schema_hash: "a".repeat(64),
            migration: Some(StartupMigrationEvidence {
                source_schema_version: 1,
                source_schema_hash: "b".repeat(64),
                target_schema_version: 2,
                target_schema_hash: "a".repeat(64),
                step_count: 1,
            }),
            source_revision: 4,
            runtime_sequence: 8,
            durable_epoch: 7,
            durable_turn_sequence: 6,
            state_digest: "b".repeat(64),
            key: key(40),
        });
        roundtrip(ObserverEvent::ScenarioCheckpoint {
            request_id: 7,
            step_id: "publish-success".to_owned(),
            assertion_count: 4,
            source_revision: 4,
            runtime_sequence: 9,
            durable_epoch: 8,
            durable_turn_sequence: 7,
            state_digest: "c".repeat(64),
            key: key(41),
        });
        roundtrip(ObserverEvent::PersistenceEvidence {
            kind: PersistenceEvidenceKind::MigrationProductRestored,
            source_revision: 4,
            runtime_sequence: 10,
            durable_epoch: 9,
            durable_turn_sequence: 8,
            before_state_digest: "d".repeat(64),
            after_state_digest: "e".repeat(64),
            key: key(42),
        });
        roundtrip(ObserverEvent::ResponsiveLayoutEvidence {
            resize_sequence: 12,
            logical_width: 390,
            logical_height: 844,
            action_count: 12,
            action_digest: "f".repeat(64),
            state_digest: "0".repeat(64),
            source_revision: 4,
            runtime_sequence: 10,
            durable_epoch: 9,
            durable_turn_sequence: 8,
            key: key(43),
        });
        roundtrip(ObserverEvent::ProfileSample {
            ordinal: 17,
            input_sequence: 18,
            callback_to_host_ns: 900,
            editor_visible_us: 1_200,
            preview_visible_us: 4_200,
            compile_us: 1_900,
            parent_dispatch_us: 900,
            parent_executor_us: 500,
            parent_runtime_document_us: 300,
            parent_persistence_us: 20,
            completion_us: 700,
            completion_executor_us: 350,
            completion_runtime_document_us: 220,
            completion_persistence_us: 15,
            document_us: 600,
            interaction_us: 100,
            demand_us: 50,
            present_us: 500,
            patch_count: 4,
            full_lowered: false,
            interaction_frame_block_us: 2_100,
            pending_child_artifacts: 1,
            pending_program_artifact_stores: 1,
            pending_program_artifact_loads: 0,
            pending_persistence_artifact_stores: 1,
            pending_persistence_artifact_loads: 0,
            pending_durable_batches: 1,
            trusted_parent_rebuilds: 0,
            source_revision: 4,
            runtime_sequence: 11,
            editor_key: key(43),
            key: key(44),
        });
        roundtrip(ObserverEvent::ProfileInputTarget {
            node: "source-editor".to_owned(),
            source_path: "store.elements.source_editor".to_owned(),
            x: 120.0,
            y: 80.0,
            sample_count: 120,
            key: key(45),
        });
        roundtrip(ObserverEvent::ProfileInputSeeded {
            input_sequence: 19,
            callback_to_host_ns: 700,
            compile_us: 800,
            pending_child_artifacts: 1,
            editor_key: key(46),
            key: key(47),
        });
        roundtrip(ObserverEvent::ResponsiveResizeReady {
            desired_width: 390,
            desired_height: 844,
            current_width: 960,
            current_height: 844,
            key: key(48),
        });
        roundtrip(ObserverEvent::ResponsiveResizeObserved {
            event_sequence: 20,
            logical_width: 390,
            logical_height: 844,
            previous_surface_epoch: 2,
            key: key(49),
        });
        roundtrip(ObserverEvent::ScrollProofFrame {
            ordinal: 21,
            key: key(50),
        });
        roundtrip(ObserverEvent::NativeWorkflowReady {
            test_request_id: 9,
            step_count: 29,
            source_revision: 4,
            runtime_sequence: 12,
            durable_epoch: 10,
            state_digest: "2".repeat(64),
            key: key(51),
        });
        roundtrip(ObserverEvent::NativeWorkflowTarget {
            request_id: 577,
            ordinal: 1,
            step_id: "valid-edit-preview".to_owned(),
            source_path: "store.elements.source_editor".to_owned(),
            action_kind: "type_text".to_owned(),
            action_digest: "4".repeat(64),
            node: "source-editor".to_owned(),
            x: 120.0,
            y: 80.0,
            key: key(52),
        });
        roundtrip(ObserverEvent::NativeWorkflowStep {
            request_id: 577,
            ordinal: 1,
            step_id: "valid-edit-preview".to_owned(),
            source_path: "store.elements.source_editor".to_owned(),
            action_kind: "type_text".to_owned(),
            action_digest: "4".repeat(64),
            input_first_sequence: 22,
            input_last_sequence: 29,
            input_event_count: 8,
            input_event_digest: "5".repeat(64),
            assertion_count: 4,
            source_revision: 4,
            runtime_sequence: 13,
            durable_epoch: 11,
            durable_turn_sequence: 10,
            durable_acked: true,
            before_state_digest: "2".repeat(64),
            state_digest: "3".repeat(64),
            key: key(53),
        });
        roundtrip(ObserverEvent::NativeWorkflowCompleted {
            test_request_id: 9,
            step_count: 29,
            initial_state_digest: "2".repeat(64),
            final_state_digest: "3".repeat(64),
            key: key(53),
        });
        roundtrip(ObserverEvent::AsyncLaneCompleted {
            lane: AsyncLaneKind::ChildProgramCompile,
            request_id: "public-page:request-7".to_owned(),
            revision: 7,
            queue_depth: 1,
            queue_wait_us: 10,
            worker_us: 20,
            apply_us: 30,
            end_to_end_us: 60,
            outcome: AsyncLaneOutcome::Applied,
            key: key(54),
        });
        roundtrip(ObserverEvent::StaleProgramRejected {
            session: "profile-draft".to_owned(),
            stale_revision: 11,
            latest_revision: 12,
            source_revision: 4,
            runtime_sequence: 12,
            durable_epoch: 10,
            durable_turn_sequence: 9,
            state_digest: "1".repeat(64),
            key: key(45),
        });
    }

    #[test]
    fn evidence_key_rejects_zero_identity_components() {
        assert!(key(1).is_complete());
        assert!(!key(0).is_complete());
        let mut missing_surface = key(1);
        missing_surface.surface_id.clear();
        assert!(!missing_surface.is_complete());
        let mut missing_process = key(1);
        missing_process.process_id = 0;
        assert!(!missing_process.is_complete());
        let mut missing_session = key(1);
        missing_session.session_id.clear();
        assert!(!missing_session.is_complete());
        let mut restart = key(1);
        restart.session_id = "launch-restart".to_owned();
        assert!(!key(1).same_producer_surface(&restart));
    }

    #[test]
    fn decoder_rejects_unbounded_and_trailing_frames() {
        let mut oversized = (MAX_EVENT_BYTES as u32 + 1).to_le_bytes().to_vec();
        oversized.resize(8, 0);
        assert!(matches!(
            read_event(&mut oversized.as_slice()),
            Err(ObserverError::FrameTooLarge(_))
        ));

        let event = ObserverEvent::ProofRequested {
            key: key(3),
            snapshot_prepare_us: 42,
        };
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).unwrap();
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        bytes.extend_from_slice(&[9]);
        bytes[..4].copy_from_slice(&((length + 1) as u32).to_le_bytes());
        assert!(matches!(
            read_event(&mut bytes.as_slice()),
            Err(ObserverError::TrailingBytes(1))
        ));
    }
}
