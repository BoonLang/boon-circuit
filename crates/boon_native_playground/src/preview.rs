use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use boon_host::{
    HostEvent, HostEventEnvelope, HostEventOrigin, KeyEvent, LogicalKey, PointerButton,
    PointerEvent, PointerPhase, TextInputEvent, Viewport,
};
use boon_native_app_window::{NativeRoleResult, NativeSurfaceHost, SensitiveInputTarget};
use boon_persistence::{DecodeLimits, encode_restore_image};
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};
use sha2::{Digest, Sha256};

use boon_runtime::{
    MigrationScenarioRunner, PersistentRuntimeStartupDisposition, ProgramCompletion,
    ProgramHostCompletion, ProgramHostRequest, ProgramRequestId, ProgramSessionId,
    RuntimePhaseTimings, ScenarioStep, compile_program_artifact,
};

use crate::compile::{
    CompileRequest, CompileWorker, CompiledExecutable, ProgramCompileReceipt, ProgramCompileWorker,
    compile_migration_stage, preview_project_key, project_key_for_stage,
};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, host_event_digest,
    input_kind, pointer_button_pressed, role_message_frame,
};
use crate::native_input::{ASCII_BATCH_END_PHYSICAL_KEY, ASCII_TEXT_BATCH_MAX_BYTES};
use crate::observer::{
    AsyncLaneKind, AsyncLaneOutcome, InputAccepted, MIGRATION_EVIDENCE_ENV,
    NATIVE_WORKFLOW_PROOF_STEPS_ENV, NATIVE_WORKFLOW_STEPS_ENV, ObserverClient, ObserverEvent,
    ObserverRole, PERSISTENCE_EVIDENCE_ENV, PRODUCT_PROOF_AFTER_TEST_ENV, PROFILE_BENCHMARK_ENV,
    PROFILE_BENCHMARK_STEPS_ENV, PersistenceEvidenceKind, RESPONSIVE_EVIDENCE_WIDTH_ENV,
    RESPONSIVE_NAVIGATION_SOURCES_ENV, SCROLL_PROOF_ORDINAL_ENV, STALE_PROGRAM_EVIDENCE_ENV,
    STATE_EVIDENCE_STEPS_ENV, STATE_MOUNT_EVIDENCE_ENV, StartupDisposition,
    StartupMigrationEvidence, TestPointerPhase,
};
use crate::proof::{ProofConfig, ProofRequest, ProofResult, ProofWorker};
use crate::protocol::{
    ApplicationIdentity, AssetBlob, CanonicalStateArtifact, Connection, FrameMode,
    MAX_PERSISTENCE_ARTIFACT_BYTES, Message, MigrationBundle, MigrationCommand, MigrationOperation,
    MigrationStatus, PersistenceCommand, PersistenceOperation, PersistenceOperationStatus,
    PreviewIntent, PreviewSource, PreviewStats, ProofMode, Role, StateArtifactFormat,
    StateArtifactPreviewSummary, TestStep,
};
use crate::runtime_view::{
    ProgramCompletionObservation, RuntimeAsyncLaneKind, RuntimeAsyncLaneObservation,
    RuntimeAsyncLaneOutcome, RuntimeSourceDispatch, RuntimeView, STATE_ROOT_ENV, digest_hex,
};
use crate::view::{HitTarget, RetainedView};

pub(crate) const TEST_STEP_LIMIT: usize = 64;
const OUTBOUND_QUEUE_DEPTH: usize = 8;
const STATS_INTERVAL: Duration = Duration::from_millis(100);
const TEST_CURSOR_FRAME: Duration = Duration::from_millis(16);
const TEST_CURSOR_PIXELS_PER_FRAME: f32 = 64.0;
const TEST_CURSOR_MAX_MOVE_FRAMES: usize = 8;
const TEST_SETTLE_PROGRAM_LIMIT: usize = 128;
const TEST_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING_EVIDENCE_PROOFS: usize = 8;
const PROFILE_SAMPLE_TEXT: &str = " ";
const MAX_NATIVE_WORKFLOW_INPUT_EVENTS: usize = ASCII_TEXT_BATCH_MAX_BYTES * 5 + 16;

struct TestRunOutcome {
    completed_steps: usize,
    semantic_assertions_proven: bool,
    proof_requests: Vec<PreparedProofRequest>,
    last_key: crate::observer::FrameEvidenceKey,
}

#[derive(Clone)]
struct SubmittedProgramRequest {
    session: ProgramSessionId,
    request_id: ProgramRequestId,
    revision: u64,
    pending_depth: u32,
}

#[derive(Default)]
struct RuntimeUpdateMeasurement {
    changed: bool,
    document_us: u64,
    interaction_us: u64,
    demand_us: u64,
    patch_count: u32,
    full_lowered: bool,
}

enum ProductProfilePhase {
    Seed,
    Samples,
    Complete,
}

impl ProductProfilePhase {
    fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

struct ProductProfileCandidate {
    batch_text: String,
    input_sequence: u64,
    callback_to_host_ns: u64,
    accepted_at: Instant,
    parent_generation_before: u64,
    parent_dispatch_us: u64,
    parent_phase: RuntimePhaseTimings,
    update: RuntimeUpdateMeasurement,
    requests: Vec<SubmittedProgramRequest>,
    closed: bool,
    editor_frame: Option<PresentedFrame>,
    editor_visible_us: Option<u64>,
    compile_us: u64,
    pending_depth: u32,
    completion_us: u64,
    completion_phase: RuntimePhaseTimings,
    completion_update: RuntimeUpdateMeasurement,
    completed_requests: BTreeSet<(String, String)>,
    invalid_completion: bool,
    child_frame: Option<PresentedFrame>,
    preview_visible_us: Option<u64>,
}

struct ProductProfileBenchmark {
    baseline: Vec<u8>,
    source_path: String,
    target_node: String,
    seed_text: String,
    sample_count: u32,
    completed_samples: u32,
    phase: ProductProfilePhase,
    candidate: Option<ProductProfileCandidate>,
}

struct ResponsiveEvidenceState {
    desired_width: u32,
    desired_height: u32,
    baseline_key: crate::observer::FrameEvidenceKey,
    baseline_state_digest: String,
    expected_actions: boon_document::source_actions::SourceActionCoverage,
    observed_actions: boon_document::source_actions::SourceActionCoverage,
    navigation_sources: Vec<String>,
    navigation_index: usize,
    pending_navigation: Option<(String, String)>,
    baseline_action_count: u32,
    baseline_action_digest: String,
    resize_sequence: Option<u64>,
    last_surface_epoch: u64,
    resize_started: bool,
    complete: bool,
}

struct NativeWorkflowPending {
    request_id: u64,
    action_digest: String,
    target_node: String,
    before_state_digest: String,
    started_at: Option<Instant>,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    event_digests: Vec<String>,
    batch_text: String,
    pointer_up_count: u8,
    last_pointer_up_source_path: Option<String>,
    last_pointer_up_dispatched_source_paths: Vec<String>,
    keyboard_phase: u8,
    action_complete: bool,
}

struct NativeWorkflowPointerPresentation {
    request_id: u64,
    step_index: u32,
    phase: TestPointerPhase,
    x: f32,
    y: f32,
    target: Option<String>,
    runtime_sequence: u64,
}

struct NativeWorkflowState {
    test_request_id: u64,
    steps: Vec<TestStep>,
    proof_steps: BTreeSet<String>,
    prepared: bool,
    host_evidence_complete: bool,
    completed: usize,
    initial_state_digest: Option<String>,
    current_state_digest: Option<String>,
    pending: Option<NativeWorkflowPending>,
    test_completed_emitted: bool,
}

impl NativeWorkflowState {
    fn from_test_steps(
        test_request_id: u64,
        all_steps: &[TestStep],
        required_steps: &[String],
        proof_steps: &BTreeSet<String>,
    ) -> Result<Option<Self>, String> {
        if required_steps.is_empty() {
            return Ok(None);
        }
        let steps = required_steps
            .iter()
            .map(|id| {
                all_steps
                    .iter()
                    .find(|step| step.id == *id)
                    .cloned()
                    .ok_or_else(|| format!("native workflow step `{id}` is absent"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if steps.iter().any(|step| match step.action_kind.as_deref() {
            None => !step.source_path.is_empty() || step.expectations.is_empty(),
            Some(
                "click" | "type_text" | "double_click" | "key" | "focused_key" | "focused_chord"
                | "blur",
            ) => step.source_path.is_empty() || step.expectations.is_empty(),
            Some(_) => true,
        }) {
            return Err(
                "native workflow requires assertion-only or typed native actions with semantic assertions"
                    .to_owned()
            );
        }
        if proof_steps
            .iter()
            .any(|id| !required_steps.iter().any(|required| required == id))
        {
            return Err("native workflow proof steps must be part of the workflow".to_owned());
        }
        Ok(Some(Self {
            test_request_id,
            steps,
            proof_steps: proof_steps.clone(),
            prepared: false,
            host_evidence_complete: false,
            completed: 0,
            initial_state_digest: None,
            current_state_digest: None,
            pending: None,
            test_completed_emitted: false,
        }))
    }

    fn complete(&self) -> bool {
        self.prepared && self.completed == self.steps.len()
    }

    fn current(&self) -> Option<&TestStep> {
        self.prepared
            .then(|| self.steps.get(self.completed))
            .flatten()
    }
}

pub(crate) fn native_workflow_action_digest(step: &TestStep) -> String {
    let canonical = format!(
        "id={};source={};kind={:?};target={:?};text={:?};key={:?};address={:?};occurrence={:?};pointer={:?},{:?},{:?},{:?}",
        step.id,
        step.source_path,
        step.action_kind,
        step.target_text,
        step.text,
        step.key,
        step.address,
        step.target_occurrence,
        step.pointer_x,
        step.pointer_y,
        step.pointer_width,
        step.pointer_height,
    );
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

pub(crate) fn native_workflow_input_digest(event_digests: &[String]) -> String {
    let mut hasher = Sha256::new();
    for digest in event_digests {
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Default)]
struct StateEvidenceConfig {
    mount: bool,
    scenario_steps: BTreeSet<String>,
    persistence_exercise: bool,
    migration_exercise: bool,
    profile_samples: usize,
    profile_steps: Vec<String>,
    responsive_width: Option<u32>,
    responsive_navigation_sources: Vec<String>,
    stale_program: bool,
    native_workflow_steps: Vec<String>,
    native_workflow_proof_steps: BTreeSet<String>,
}

impl StateEvidenceConfig {
    fn from_env() -> Result<Self, String> {
        let mount = std::env::var_os(STATE_MOUNT_EVIDENCE_ENV).is_some();
        let scenario_steps = std::env::var(STATE_EVIDENCE_STEPS_ENV)
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if scenario_steps.len() > TEST_STEP_LIMIT
            || scenario_steps
                .iter()
                .any(|step| step.is_empty() || step.len() > 96)
        {
            return Err(format!(
                "{STATE_EVIDENCE_STEPS_ENV} exceeds the bounded scenario-step contract"
            ));
        }
        let persistence_exercise = std::env::var_os(PERSISTENCE_EVIDENCE_ENV).is_some();
        let migration_exercise = std::env::var_os(MIGRATION_EVIDENCE_ENV).is_some();
        let profile_samples = std::env::var(PROFILE_BENCHMARK_ENV)
            .ok()
            .map(|value| {
                value.parse::<usize>().map_err(|error| {
                    format!("invalid {PROFILE_BENCHMARK_ENV} value `{value}`: {error}")
                })
            })
            .transpose()?
            .unwrap_or(0);
        if profile_samples != 0 && !(70..=256).contains(&profile_samples) {
            return Err(format!(
                "{PROFILE_BENCHMARK_ENV} must be zero or within 70..=256"
            ));
        }
        let profile_steps = std::env::var(PROFILE_BENCHMARK_STEPS_ENV)
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if (profile_samples == 0 && !profile_steps.is_empty())
            || (profile_samples > 0
                && (profile_steps.len() != 2
                    || profile_steps
                        .iter()
                        .any(|step| step.is_empty() || step.len() > 96)))
        {
            return Err(format!(
                "{PROFILE_BENCHMARK_STEPS_ENV} must identify exactly two bounded steps when profiling"
            ));
        }
        let responsive_width = std::env::var(RESPONSIVE_EVIDENCE_WIDTH_ENV)
            .ok()
            .map(|value| {
                let width = value.parse::<u32>().map_err(|error| error.to_string())?;
                if !(240..=1_920).contains(&width) {
                    return Err(format!(
                        "{RESPONSIVE_EVIDENCE_WIDTH_ENV} is outside 240..1920"
                    ));
                }
                Ok(width)
            })
            .transpose()?;
        let responsive_navigation_sources =
            bounded_evidence_ids(RESPONSIVE_NAVIGATION_SOURCES_ENV)?;
        if responsive_width.is_some() != !responsive_navigation_sources.is_empty()
            || responsive_navigation_sources.len() > 8
        {
            return Err(format!(
                "{RESPONSIVE_NAVIGATION_SOURCES_ENV} must declare one to eight routes with responsive evidence"
            ));
        }
        let stale_program = std::env::var_os(STALE_PROGRAM_EVIDENCE_ENV).is_some();
        let native_workflow_steps = bounded_evidence_ids(NATIVE_WORKFLOW_STEPS_ENV)?;
        let native_workflow_proof_steps = bounded_evidence_ids(NATIVE_WORKFLOW_PROOF_STEPS_ENV)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if native_workflow_proof_steps
            .iter()
            .any(|id| !native_workflow_steps.iter().any(|step| step == id))
        {
            return Err(format!(
                "{NATIVE_WORKFLOW_PROOF_STEPS_ENV} must be a subset of {NATIVE_WORKFLOW_STEPS_ENV}"
            ));
        }
        Ok(Self {
            mount,
            scenario_steps,
            persistence_exercise,
            migration_exercise,
            profile_samples,
            profile_steps,
            responsive_width,
            responsive_navigation_sources,
            stale_program,
            native_workflow_steps,
            native_workflow_proof_steps,
        })
    }

    fn enabled(&self) -> bool {
        self.mount
            || !self.scenario_steps.is_empty()
            || self.persistence_exercise
            || self.migration_exercise
            || self.profile_samples != 0
            || self.responsive_width.is_some()
            || self.stale_program
            || !self.native_workflow_steps.is_empty()
    }
}

fn bounded_evidence_ids(name: &str) -> Result<Vec<String>, String> {
    let values = std::env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if values.len() > TEST_STEP_LIMIT
        || values
            .iter()
            .any(|value| value.is_empty() || value.len() > 96)
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(format!("{name} exceeds the bounded unique step contract"));
    }
    Ok(values)
}

struct AuthoritativeStateEvidence {
    artifact: Vec<u8>,
    digest: String,
    durable_epoch: u64,
    durable_turn_sequence: u64,
}

struct PreviewOutput {
    sender: Option<SyncSender<Message>>,
    error: Arc<Mutex<Option<String>>>,
    writer: Option<thread::JoinHandle<()>>,
}

struct CachedStateArtifactPreview {
    artifact: CanonicalStateArtifact,
    summary: StateArtifactPreviewSummary,
}

struct DeadlineScheduler {
    commands: Option<std::sync::mpsc::Sender<Option<Instant>>>,
    ticks: mpsc::UnboundedReceiver<()>,
    worker: Option<thread::JoinHandle<()>>,
    scheduled: Option<Option<Instant>>,
}

impl DeadlineScheduler {
    fn start() -> Result<Self, String> {
        let (commands, receiver) = std::sync::mpsc::channel::<Option<Instant>>();
        let (tick_sender, ticks) = mpsc::unbounded();
        let worker = thread::Builder::new()
            .name("boon-preview-deadline".to_owned())
            .spawn(move || {
                let mut deadline = None::<Instant>;
                loop {
                    let command = match deadline {
                        Some(at) => match receiver
                            .recv_timeout(at.saturating_duration_since(Instant::now()))
                        {
                            Ok(command) => Some(command),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if tick_sender.unbounded_send(()).is_err() {
                                    break;
                                }
                                deadline = None;
                                None
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                        None => match receiver.recv() {
                            Ok(command) => Some(command),
                            Err(_) => break,
                        },
                    };
                    if let Some(command) = command {
                        deadline = receiver.try_iter().last().unwrap_or(command);
                    }
                }
            })
            .map_err(|error| format!("spawn preview deadline scheduler: {error}"))?;
        Ok(Self {
            commands: Some(commands),
            ticks,
            worker: Some(worker),
            scheduled: None,
        })
    }

    fn schedule(&mut self, deadline: Option<Instant>) {
        if self.scheduled == Some(deadline) {
            return;
        }
        self.scheduled = Some(deadline);
        if let Some(commands) = &self.commands {
            let _ = commands.send(deadline);
        }
    }

    fn fired(&mut self) {
        self.scheduled = None;
    }
}

impl Drop for DeadlineScheduler {
    fn drop(&mut self) {
        self.commands.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl PreviewOutput {
    fn start(mut connection: Connection) -> Result<Self, String> {
        let (sender, receiver) = sync_channel::<Message>(OUTBOUND_QUEUE_DEPTH);
        let error = Arc::new(Mutex::new(None));
        let writer_error = Arc::clone(&error);
        let writer = thread::Builder::new()
            .name("boon-preview-output".to_owned())
            .spawn(move || {
                for message in receiver {
                    if let Err(write_error) = connection.send(&message) {
                        *writer_error.lock().expect("preview output error") =
                            Some(write_error.to_string());
                        break;
                    }
                }
            })
            .map_err(|error| format!("spawn preview output writer: {error}"))?;
        Ok(Self {
            sender: Some(sender),
            error,
            writer: Some(writer),
        })
    }

    fn send(&self, message: Message) -> Result<(), String> {
        if let Some(error) = self.error.lock().expect("preview output error").clone() {
            return Err(error);
        }
        self.sender
            .as_ref()
            .ok_or_else(|| "preview output writer is closed".to_owned())?
            .send(message)
            .map_err(|_| "preview output writer stopped".to_owned())
    }

    fn try_send_stats(&self, message: Message) -> Result<(), String> {
        if let Some(error) = self.error.lock().expect("preview output error").clone() {
            return Err(error);
        }
        match self
            .sender
            .as_ref()
            .ok_or_else(|| "preview output writer is closed".to_owned())?
            .try_send(message)
        {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err("preview output writer stopped".to_owned()),
        }
    }
}

impl Drop for PreviewOutput {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

pub fn connect(path: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Connection::connect(path, Role::Preview)?)
}

pub async fn run(mut host: NativeSurfaceHost, writer: Connection) -> NativeRoleResult {
    let observer = ObserverClient::from_env()?;
    let proof_config = ProofConfig::from_env().map_err(|error| format!("proof config: {error}"))?;
    let state_evidence = StateEvidenceConfig::from_env()
        .map_err(|error| format!("state evidence config: {error}"))?;
    let scroll_proof_ordinal = std::env::var(SCROLL_PROOF_ORDINAL_ENV)
        .ok()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|error| format!("invalid {SCROLL_PROOF_ORDINAL_ENV} value: {error}"))
        })
        .transpose()?
        .filter(|ordinal| *ordinal != 0 && *ordinal <= 256);
    if std::env::var_os(SCROLL_PROOF_ORDINAL_ENV).is_some() && scroll_proof_ordinal.is_none() {
        return Err(format!("{SCROLL_PROOF_ORDINAL_ENV} must be within 1..=256").into());
    }
    if proof_config.is_some() && observer.is_none() {
        return Err("verifier proof mode requires the verifier observer channel".into());
    }
    if (state_evidence.enabled() || scroll_proof_ordinal.is_some())
        && (observer.is_none() || proof_config.is_none())
    {
        return Err(
            "state/profile evidence requires both the observer and app-owned WGPU proof mode"
                .into(),
        );
    }
    let mut proof = proof_config
        .as_ref()
        .map(|config| ProofWorker::start(config.artifact_dir.clone()))
        .transpose()
        .map_err(|error| format!("proof worker: {error}"))?;

    let mut product =
        ProductFrame::attach(&mut host, ObserverRole::Preview, proof.is_some()).await?;
    emit(
        &observer,
        ObserverEvent::RoleMetadata(product.role_metadata()),
    );
    let mut columns = boon_native_gpu::GlyphonRenderTextColumnMeasurer::new();
    let mut view = RetainedView::new(
        role_message_frame("Boon Preview", "Waiting for source...", "#eef1f4"),
        viewport(&host),
        &mut columns,
    )?;

    let (incoming_tx, mut incoming) = mpsc::unbounded::<Result<Message, String>>();
    let mut reader = writer.try_clone()?;
    let output = PreviewOutput::start(writer)?;
    thread::Builder::new()
        .name("boon-preview-ipc".to_owned())
        .spawn(move || {
            loop {
                let item = match reader.receive() {
                    Ok(Some(message)) => Ok(message),
                    Ok(None) => Err("desktop IPC closed".to_owned()),
                    Err(error) => Err(error.to_string()),
                };
                let closed = item.is_err();
                if incoming_tx.unbounded_send(item).is_err() || closed {
                    break;
                }
            }
        })?;
    output.send(Message::Ready {
        role: Role::Preview,
    })?;

    let (compiler, mut compiled) = CompileWorker::start();
    let (program_compiler, mut program_compiled) = ProgramCompileWorker::start();
    let mut runtime = None::<RuntimeView>;
    let mut runtime_key = None::<String>;
    let mut package_assets = Vec::<AssetBlob>::new();
    let mut state_mount_captured = false;
    let mut migration = None::<MigrationBundle>;
    let mut active_migration_stage = None::<String>;
    let mut previewed_migration_stage = None::<String>;
    let mut desired_revision = 0u64;
    let mut source_revision = 0u64;
    let mut cursor = (24.0f32, 24.0f32);
    let mut switch_started = None::<(u64, Instant)>;
    let mut proof_eligible_ordinal = 0u64;
    let mut proof_requested = false;
    let mut queued_evidence_proofs = VecDeque::<PreparedProofRequest>::new();
    let mut evidence_proof_in_flight = None::<crate::observer::FrameEvidenceKey>;
    let mut last_stats_sent = None::<Instant>;
    let mut persistence_snapshot_sequence = 0u64;
    let mut last_persistence_operation = None::<PersistenceOperationStatus>;
    let mut import_preview = None::<CachedStateArtifactPreview>;
    let mut next_import_preview_id = 0u64;
    let mut deadline_scheduler = DeadlineScheduler::start()?;
    let mut migration_evidence_completed = false;
    let mut profile_benchmark = None::<ProductProfileBenchmark>;
    let mut responsive_evidence = None::<ResponsiveEvidenceState>;
    let mut native_workflow = None::<NativeWorkflowState>;
    let mut latest_presented_key = None::<crate::observer::FrameEvidenceKey>;
    let mut scroll_frame_ordinal = 0_u32;
    let mut scroll_proof_requested = false;

    loop {
        if let Some(runtime) = runtime.as_mut() {
            runtime.resolve_program_artifact_requests()?;
            submit_program_requests(runtime.take_program_requests(), &program_compiler);
        }
        let runtime_deadline = runtime.as_ref().and_then(|runtime| {
            [
                runtime.caret_blink_deadline(),
                runtime.scheduled_source_deadline(),
                runtime.persistence_poll_deadline(),
                runtime.effect_poll_deadline(),
            ]
            .into_iter()
            .flatten()
            .min()
        });
        let workflow_deadline = native_workflow
            .as_ref()
            .and_then(|workflow| workflow.pending.as_ref())
            .and_then(|pending| pending.started_at)
            .map(|started| started + TEST_SETTLE_TIMEOUT);
        deadline_scheduler.schedule(
            [runtime_deadline, workflow_deadline]
                .into_iter()
                .flatten()
                .min(),
        );
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
            Compiled(Option<crate::compile::CompileOutcome>),
            ProgramCompiled(Option<crate::compile::ProgramCompileOutcome>),
            Proof(Option<Box<ProofResult>>),
            MapTile(Option<()>),
            Scheduled(Option<()>),
        }
        let wake = {
            let native = host.next_event().fuse();
            let command = incoming.next().fuse();
            let result = compiled.next().fuse();
            let program_result = program_compiled.next().fuse();
            let proof_result = async {
                match proof.as_mut() {
                    Some(worker) => worker.next_result().await,
                    None => futures::future::pending::<Option<ProofResult>>().await,
                }
            }
            .fuse();
            let map_tile = product.next_map_tile_wake().fuse();
            let scheduled = deadline_scheduler.ticks.next().fuse();
            pin_mut!(
                native,
                command,
                result,
                program_result,
                proof_result,
                map_tile,
                scheduled
            );
            select! {
                value = native => Wake::Native(value),
                value = command => Wake::Ipc(value),
                value = result => Wake::Compiled(value),
                value = program_result => Wake::ProgramCompiled(value),
                value = proof_result => Wake::Proof(value.map(Box::new)),
                value = map_tile => Wake::MapTile(value),
                value = scheduled => Wake::Scheduled(value),
            }
        };

        match wake {
            Wake::Native(event) => {
                let mut transaction = NativeFrameTransaction::default();
                let mut latest_runtime_sequence = None;
                let mut persistence_turn_changed = false;
                let mut resize_observation = None::<(u64, u32, u32, u64)>;
                let mut workflow_pointer_presentation = None;
                for accepted in drain_native_events(&mut host, event).await? {
                    let envelope = &accepted.envelope;
                    if matches!(envelope.event, HostEvent::CloseRequested { .. }) {
                        observe_input(&observer, envelope, None, None, false);
                        let _ = output.send(Message::Shutdown);
                        return Ok(());
                    }
                    if let (Some(x), Some(y)) = event_position(&envelope.event) {
                        cursor = (x, y);
                        if native_workflow.is_some() {
                            product.set_virtual_cursor(Some(cursor));
                        }
                    }
                    let target = event_target(&view, &envelope.event, &mut columns);
                    let target_name = target.as_ref().map(|target| target.node.clone());
                    let target_source_path = target
                        .as_ref()
                        .and_then(|target| target.source_path.clone());
                    let (map_consumed, map_visible_changed, _map_events) =
                        if matches!(envelope.event, HostEvent::Resize(_)) {
                            (false, false, Vec::new())
                        } else {
                            product.handle_map_input(view.scene(), &envelope.event)?
                        };
                    let mut event_dispatch_us = 0;
                    let mut executor_us = 0;
                    let mut runtime_document_us = 0;
                    let mut document_update_us = 0;
                    let dirty = if let HostEvent::Resize(resize) = &envelope.event {
                        let started = Instant::now();
                        view.resize(viewport(&host), &mut columns)?;
                        emit(
                            &observer,
                            ObserverEvent::RoleMetadata(
                                product.current_role_metadata(&host, resize.epoch),
                            ),
                        );
                        if let Some(model) = runtime.as_mut() {
                            converge_document_demands(model, &mut view, &mut columns)?;
                        }
                        let _map_resize =
                            product.handle_map_input(view.scene(), &envelope.event)?;
                        document_update_us = duration_us(started.elapsed());
                        if let Some(state) = responsive_evidence.as_mut() {
                            state.resize_started = true;
                            let previous_surface_epoch = state.last_surface_epoch;
                            state.last_surface_epoch = resize.epoch;
                            resize_observation = Some((
                                envelope.sequence,
                                resize.logical_size.width.round().max(0.0) as u32,
                                resize.logical_size.height.round().max(0.0) as u32,
                                previous_surface_epoch,
                            ));
                        }
                        true
                    } else if map_consumed {
                        map_visible_changed
                    } else if let Some(model) = runtime.as_mut() {
                        let parent_generation_before = model.parent_runtime_generation();
                        let started = Instant::now();
                        let sequence_before = model.event_sequence();
                        let runtime_turn_before = model.runtime_turn_sequence();
                        let outcome = model.handle_event_observed(&envelope.event, target)?;
                        let changed = outcome.changed;
                        sync_sensitive_input_focus(model, &mut host)?;
                        let sequence_after = model.event_sequence();
                        persistence_turn_changed |=
                            model.runtime_turn_sequence() > runtime_turn_before;
                        if sequence_after > sequence_before {
                            latest_runtime_sequence = Some(sequence_after);
                        }
                        if let Some(url) = model.take_external_url()
                            && let Err(error) = open_external_url(&url)
                        {
                            eprintln!("open external URL: {error}");
                        }
                        event_dispatch_us = duration_us(started.elapsed());
                        let phase = model.last_runtime_phase();
                        executor_us = phase.executor_us;
                        runtime_document_us = phase.document_us;
                        let update = if changed {
                            apply_runtime_update_measured(model, &mut view, &mut columns)?
                        } else {
                            RuntimeUpdateMeasurement::default()
                        };
                        document_update_us = update.total_us();
                        model.resolve_program_artifact_requests()?;
                        let submitted = submit_program_requests(
                            model.take_program_requests(),
                            &program_compiler,
                        );
                        if let HostEvent::TextInput(text) = &envelope.event
                            && profile_benchmark.is_some()
                        {
                            record_profile_text_input(
                                profile_benchmark.as_mut().expect("profile benchmark"),
                                envelope,
                                accepted.accepted_at,
                                text,
                                model,
                                parent_generation_before,
                                event_dispatch_us,
                                phase,
                                update,
                                submitted,
                                &outcome.dispatches,
                            )?;
                        }
                        if let Some(workflow) = native_workflow.as_mut() {
                            observe_native_workflow_input(
                                workflow,
                                envelope,
                                target_source_path.as_deref(),
                                model.focused(),
                                &outcome.dispatches,
                            )?;
                        }
                        if let Some(state) = responsive_evidence.as_mut() {
                            observe_responsive_navigation(
                                state,
                                envelope,
                                target_name.as_deref(),
                                target_source_path.as_deref(),
                                &outcome.dispatches,
                            )?;
                        }
                        changed
                    } else {
                        false
                    };
                    transaction.record_work(
                        event_dispatch_us,
                        executor_us,
                        runtime_document_us,
                        document_update_us,
                    );
                    let pointer_presentation = native_workflow_pointer_presentation(
                        native_workflow.as_ref(),
                        &envelope.event,
                        target_name.as_deref(),
                        runtime.as_ref().map_or(0, RuntimeView::event_sequence),
                    );
                    let visible_change = dirty || pointer_presentation.is_some();
                    observe_input(
                        &observer,
                        envelope,
                        target_name,
                        target_source_path,
                        visible_change,
                    );
                    if visible_change {
                        transaction.visible_change(&accepted);
                    }
                    if pointer_presentation.is_some() {
                        workflow_pointer_presentation = pointer_presentation;
                    }
                    if is_ascii_batch_end(&envelope.event) {
                        if let Some(benchmark) = profile_benchmark.as_mut() {
                            close_profile_input_batch(benchmark)?;
                        }
                    }
                }
                if runtime.is_none() {
                    continue;
                }
                if let Some(presented) = transaction.present(&mut product, &mut host, &view).await?
                {
                    let proof_request = prepare_product_proof_request(
                        profile_benchmark.as_ref(),
                        native_workflow.as_ref(),
                        proof.as_ref(),
                        proof_config.as_ref(),
                        &mut proof_requested,
                        &mut proof_eligible_ordinal,
                        &presented,
                        &mut product,
                    )?;
                    emit_presented(&observer, &presented);
                    latest_presented_key = Some(presented.key.clone());
                    submit_proof_request(&observer, proof.as_ref(), proof_request)?;
                    if let Some(pointer) = workflow_pointer_presentation.take() {
                        emit(
                            &observer,
                            ObserverEvent::TestPointerFrame {
                                request_id: pointer.request_id,
                                step_index: pointer.step_index,
                                phase: pointer.phase,
                                x: pointer.x,
                                y: pointer.y,
                                target: pointer.target.clone(),
                                runtime_sequence: pointer.runtime_sequence,
                                key: presented.key.clone(),
                            },
                        );
                        if pointer.phase == TestPointerPhase::Move {
                            let key = present_test_cursor_frame(
                                &observer,
                                pointer.request_id,
                                pointer.step_index as usize,
                                TestPointerPhase::Hover,
                                pointer.target.as_deref(),
                                pointer.runtime_sequence,
                                &mut product,
                                &mut host,
                                &view,
                                (pointer.x, pointer.y),
                                1,
                            )
                            .await?;
                            latest_presented_key = Some(key);
                        }
                    }
                    if let Some(benchmark) = profile_benchmark.as_mut()
                        && benchmark.candidate.as_ref().is_some_and(|candidate| {
                            presented.event_sequence == Some(candidate.input_sequence)
                        })
                    {
                        let candidate = benchmark.candidate.as_mut().expect("profile candidate");
                        candidate.editor_visible_us =
                            Some(duration_us(candidate.accepted_at.elapsed()));
                        candidate.editor_frame = Some(presented.clone());
                    }
                    if presented.input_kind == Some(crate::observer::InputKind::Wheel) {
                        scroll_frame_ordinal = scroll_frame_ordinal.saturating_add(1);
                        if !scroll_proof_requested
                            && scroll_proof_ordinal == Some(scroll_frame_ordinal)
                        {
                            scroll_proof_requested = true;
                            emit(
                                &observer,
                                ObserverEvent::ScrollProofFrame {
                                    ordinal: scroll_frame_ordinal,
                                    key: presented.key.clone(),
                                },
                            );
                            queue_evidence_proofs(
                                &observer,
                                proof.as_ref(),
                                &mut queued_evidence_proofs,
                                &mut evidence_proof_in_flight,
                                [prepare_evidence_proof(
                                    "warm-scroll",
                                    presented.key.clone(),
                                    &mut product,
                                )?],
                            )?;
                        }
                    }
                    if let Some((sequence, width, height, previous_surface_epoch)) =
                        resize_observation
                        && let Some(state) = responsive_evidence.as_mut()
                    {
                        emit(
                            &observer,
                            ObserverEvent::ResponsiveResizeObserved {
                                event_sequence: sequence,
                                logical_width: width,
                                logical_height: height,
                                previous_surface_epoch,
                                key: presented.key.clone(),
                            },
                        );
                        if !state.complete
                            && width == state.desired_width
                            && height == state.desired_height
                            && presented.key.surface_epoch > previous_surface_epoch
                        {
                            state.resize_sequence = Some(sequence);
                        }
                    }
                    if let Some(state) = responsive_evidence.as_mut()
                        && state.resize_sequence.is_some()
                        && state.pending_navigation.is_none()
                        && !state.complete
                    {
                        let request = advance_responsive_layout_evidence(
                            &observer,
                            source_revision,
                            runtime
                                .as_mut()
                                .ok_or("responsive evidence has no runtime")?,
                            &view,
                            &mut product,
                            state,
                            presented.key.clone(),
                        )?;
                        if let Some(request) = request {
                            queue_evidence_proofs(
                                &observer,
                                proof.as_ref(),
                                &mut queued_evidence_proofs,
                                &mut evidence_proof_in_flight,
                                [request],
                            )?;
                        }
                    }
                    send_stats(
                        &output,
                        &product,
                        runtime.as_ref(),
                        source_revision,
                        FrameMode::Burst,
                        compiler.replaced_count(),
                        &mut last_stats_sent,
                        false,
                    )?;
                }
                if let (Some(benchmark), Some(model)) =
                    (profile_benchmark.as_mut(), runtime.as_mut())
                {
                    finalize_ready_profile_candidate(
                        &observer,
                        source_revision,
                        model,
                        benchmark,
                        &mut view,
                        &mut product,
                        &mut host,
                        &mut columns,
                        proof.as_ref(),
                        &mut queued_evidence_proofs,
                        &mut evidence_proof_in_flight,
                        &program_compiler,
                        &mut latest_presented_key,
                    )
                    .await?;
                }
                if let Some(runtime_sequence) = latest_runtime_sequence {
                    output.send(Message::PreviewRuntimeChanged {
                        revision: source_revision,
                        runtime_sequence,
                    })?;
                }
                if persistence_turn_changed {
                    import_preview = None;
                    push_persistence_snapshot(
                        &output,
                        runtime.as_ref(),
                        source_revision,
                        &mut persistence_snapshot_sequence,
                        last_persistence_operation.as_ref(),
                        import_preview.as_ref(),
                    )?;
                }
            }
            Wake::Ipc(message) => {
                let message = message.ok_or("desktop IPC reader stopped")??;
                match message {
                    Message::PreviewAssets { assets } => {
                        let sources = assets
                            .iter()
                            .cloned()
                            .map(render_asset_source)
                            .collect::<Vec<_>>();
                        product.replace_asset_sources(sources)?;
                        package_assets = assets;
                    }
                    Message::PreviewApply {
                        intent,
                        request_id,
                        revision,
                        source,
                        test_steps,
                        migration: incoming_migration,
                        migration_stage,
                    } => {
                        let accepted_at = Instant::now();
                        desired_revision = desired_revision.max(revision);
                        import_preview = None;
                        if incoming_migration.is_some() != migration_stage.is_some() {
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: false,
                                message: "migration bundle and active stage must travel together"
                                    .to_owned(),
                            })?;
                            continue;
                        }
                        if incoming_migration.is_some()
                            && !matches!(&source, PreviewSource::BuiltInSingleRole { .. })
                        {
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: false,
                                message:
                                    "distributed packages cannot use single-role migration bundles"
                                        .to_owned(),
                            })?;
                            continue;
                        }
                        if let (Some(bundle), Some(stage_id)) =
                            (incoming_migration.as_ref(), migration_stage.as_deref())
                            && bundle.stage(stage_id).is_none()
                        {
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: false,
                                message: format!("active migration stage `{stage_id}` is absent"),
                            })?;
                            continue;
                        }
                        if intent == PreviewIntent::Test
                            && let Some(bundle) = incoming_migration.as_ref()
                            && bundle.test_driver == crate::protocol::MigrationTestDriver::Migration
                        {
                            let request_id = request_id.unwrap_or(0);
                            let result = match &source {
                                PreviewSource::BuiltInSingleRole { application, .. }
                                    if revision == source_revision =>
                                {
                                    run_migration_test(bundle, application, request_id, revision)
                                }
                                PreviewSource::BuiltInSingleRole { .. } => Err(format!(
                                    "migration TEST revision {revision} is stale; preview is at {source_revision}"
                                )),
                                PreviewSource::DistributedPackage { .. } => {
                                    Err("distributed package migration TEST is unavailable"
                                        .to_owned())
                                }
                            };
                            let (passed, semantic_assertions_proven, completed, message) =
                                match result {
                                    Ok(count) => (
                                        true,
                                        true,
                                        count,
                                        format!(
                                            "{count} manifest migration lifecycle steps passed in temporary namespaces"
                                        ),
                                    ),
                                    Err(error) => (false, false, 0, error),
                                };
                            emit(
                                &observer,
                                ObserverEvent::TestCompleted {
                                    request_id,
                                    passed,
                                    semantic_assertions_proven,
                                    completed_steps: completed.try_into().unwrap_or(u32::MAX),
                                    message: message.clone(),
                                },
                            );
                            output.send(Message::PreviewTestResult {
                                request_id,
                                passed,
                                message,
                            })?;
                            continue;
                        }
                        migration = incoming_migration.clone();
                        active_migration_stage.clone_from(&migration_stage);
                        previewed_migration_stage = None;
                        let key = preview_project_key(&source, migration_stage.as_deref());
                        if intent == PreviewIntent::Replace {
                            switch_started = Some((revision, accepted_at));
                            emit(
                                &observer,
                                ObserverEvent::SourceSwitchAcknowledged {
                                    revision,
                                    elapsed_us: duration_us(accepted_at.elapsed()),
                                },
                            );
                        }
                        if intent == PreviewIntent::Replace
                            && runtime_key.as_deref() == Some(key.as_str())
                        {
                            source_revision = revision;
                            let post_compile_started = Instant::now();
                            let presented = product
                                .present(&mut host, &view)
                                .await?
                                .ok_or("active cached preview did not produce a frame")?;
                            emit_presented(&observer, &presented);
                            emit_switch_final(
                                &observer,
                                &mut switch_started,
                                source_revision,
                                &presented,
                                0,
                                duration_us(post_compile_started.elapsed()),
                            );
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: true,
                                message: "active mounted runtime retained".to_owned(),
                            })?;
                            push_persistence_snapshot(
                                &output,
                                runtime.as_ref(),
                                source_revision,
                                &mut persistence_snapshot_sequence,
                                last_persistence_operation.as_ref(),
                                import_preview.as_ref(),
                            )?;
                            send_stats(
                                &output,
                                &product,
                                runtime.as_ref(),
                                source_revision,
                                FrameMode::Idle,
                                compiler.replaced_count(),
                                &mut last_stats_sent,
                                true,
                            )?;
                            continue;
                        }
                        compiler.replace(CompileRequest {
                            intent,
                            request_id,
                            revision,
                            source,
                            test_steps,
                            migration: incoming_migration,
                            migration_stage,
                        });
                        output.send(Message::PreviewStatus {
                            revision,
                            ok: true,
                            message: "source accepted by latest-wins compiler".to_owned(),
                        })?;
                    }
                    Message::PreviewInspect {
                        request_id,
                        revision,
                        path,
                    } => {
                        let runtime_sequence = runtime
                            .as_ref()
                            .map(RuntimeView::event_sequence)
                            .unwrap_or(0);
                        let result = if revision != source_revision {
                            Err(format!(
                                "preview revision {source_revision} is not editor revision {revision}"
                            ))
                        } else {
                            runtime
                                .as_mut()
                                .ok_or_else(|| "preview runtime is not mounted".to_owned())
                                .and_then(|runtime| runtime.inspect_root_current(&path))
                        };
                        let (ok, value) = match result {
                            Ok(value) => (true, value),
                            Err(_) => (
                                false,
                                "No current runtime value for this expression".to_owned(),
                            ),
                        };
                        let authority = ok
                            .then(|| {
                                runtime
                                    .as_ref()
                                    .and_then(|runtime| runtime.authority_selection_for_path(&path))
                            })
                            .flatten();
                        output.send(Message::PreviewInspectResult {
                            request_id,
                            revision,
                            runtime_sequence,
                            path,
                            ok,
                            value,
                            authority,
                        })?;
                    }
                    Message::PreviewMigrationCommand {
                        request_id,
                        revision,
                        command,
                    } => {
                        let Some(bundle) = migration.as_ref() else {
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: false,
                                message: "active project has no migration sequence".to_owned(),
                            })?;
                            continue;
                        };
                        let Some(active_stage) = active_migration_stage.as_mut() else {
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: false,
                                message: "migration sequence has no active stage".to_owned(),
                            })?;
                            continue;
                        };
                        let execution = execute_migration_command(
                            command,
                            request_id,
                            revision,
                            source_revision,
                            bundle,
                            active_stage,
                            &mut previewed_migration_stage,
                            &observer,
                            &mut runtime,
                            &mut runtime_key,
                            &mut view,
                            &mut product,
                            &mut host,
                            &mut columns,
                        )
                        .await?;
                        if execution.runtime_changed {
                            import_preview = None;
                            output.send(Message::PreviewRuntimeChanged {
                                revision: source_revision,
                                runtime_sequence: runtime
                                    .as_ref()
                                    .map(RuntimeView::event_sequence)
                                    .unwrap_or(0),
                            })?;
                        }
                        output.send(Message::PreviewMigrationStatus(execution.status))?;
                        push_persistence_snapshot(
                            &output,
                            runtime.as_ref(),
                            source_revision,
                            &mut persistence_snapshot_sequence,
                            last_persistence_operation.as_ref(),
                            import_preview.as_ref(),
                        )?;
                        send_stats(
                            &output,
                            &product,
                            runtime.as_ref(),
                            source_revision,
                            FrameMode::Idle,
                            compiler.replaced_count(),
                            &mut last_stats_sent,
                            true,
                        )?;
                    }
                    Message::PreviewPersistenceCommand {
                        request_id,
                        revision,
                        command,
                    } => {
                        let execution = execute_persistence_command(
                            command,
                            request_id,
                            revision,
                            source_revision,
                            &observer,
                            &mut runtime,
                            &mut import_preview,
                            &mut next_import_preview_id,
                            &mut view,
                            &mut product,
                            &mut host,
                            &mut columns,
                        )
                        .await?;
                        if execution.runtime_changed {
                            output.send(Message::PreviewRuntimeChanged {
                                revision: source_revision,
                                runtime_sequence: runtime
                                    .as_ref()
                                    .map(RuntimeView::event_sequence)
                                    .unwrap_or(0),
                            })?;
                        }
                        if let Some(artifact) = execution.exported_artifact {
                            output.send(Message::PreviewPersistenceArtifact {
                                request_id,
                                revision: source_revision,
                                artifact,
                            })?;
                        }
                        last_persistence_operation = Some(execution.status);
                        push_persistence_snapshot(
                            &output,
                            runtime.as_ref(),
                            source_revision,
                            &mut persistence_snapshot_sequence,
                            last_persistence_operation.as_ref(),
                            import_preview.as_ref(),
                        )?;
                    }
                    Message::Shutdown => return Ok(()),
                    other => {
                        return Err(format!("invalid desktop-to-preview message: {other:?}").into());
                    }
                }
            }
            Wake::Compiled(outcome) => {
                let outcome = outcome.ok_or("preview compiler stopped")?;
                if outcome.revision < desired_revision {
                    continue;
                }
                match outcome.result {
                    Ok(compiled_preview) => {
                        let compile_us = duration_us(compiled_preview.elapsed);
                        let compile_elapsed_ms = compile_us as f64 / 1_000.0;
                        let test = compiled_preview.intent == PreviewIntent::Test;
                        let request_id = compiled_preview.request_id.unwrap_or(0);
                        let steps = compiled_preview.test_steps.clone();
                        let key = compiled_preview.source_key.clone();
                        let revision = compiled_preview.revision;
                        let post_compile_started = Instant::now();
                        let isolated_test = test && state_evidence.native_workflow_steps.is_empty();
                        let deterministic_runtime =
                            test || !state_evidence.scenario_steps.is_empty();
                        let activation = activate_executable(
                            &mut runtime,
                            compiled_preview.executable,
                            deterministic_runtime,
                            isolated_test,
                            &package_assets,
                        );
                        match activation {
                            Ok(activation) => {
                                let capture_mount = state_evidence.mount && !state_mount_captured;
                                let mut startup_async_lanes = Vec::new();
                                let presented = match activation {
                                    RuntimeActivation::Opened(mut next) => {
                                        if capture_mount {
                                            startup_async_lanes =
                                                next.take_async_lane_observations();
                                            emit_runtime_async_lanes_before_present(
                                                &observer,
                                                &host.ids().surface.0,
                                                &startup_async_lanes,
                                            );
                                        }
                                        install_runtime(
                                            *next,
                                            key,
                                            &mut runtime,
                                            &mut runtime_key,
                                            &mut view,
                                            &mut product,
                                            &mut host,
                                            &mut columns,
                                        )
                                        .await?
                                    }
                                    RuntimeActivation::Updated => {
                                        host.restart_sensitive_inputs()?;
                                        runtime_key = Some(key);
                                        present_runtime(
                                            runtime.as_mut().expect("activated runtime"),
                                            &mut view,
                                            &mut product,
                                            &mut host,
                                            &mut columns,
                                        )
                                        .await?
                                    }
                                };
                                source_revision = revision;
                                if let Some(presented) = &presented {
                                    emit_presented(&observer, presented);
                                    emit_switch_final(
                                        &observer,
                                        &mut switch_started,
                                        source_revision,
                                        presented,
                                        compile_us,
                                        duration_us(post_compile_started.elapsed()),
                                    );
                                    if capture_mount {
                                        let request = capture_state_mounted(
                                            &observer,
                                            runtime.as_mut().expect("mounted runtime"),
                                            source_revision,
                                            presented,
                                            &mut product,
                                        )?;
                                        queue_evidence_proofs(
                                            &observer,
                                            proof.as_ref(),
                                            &mut queued_evidence_proofs,
                                            &mut evidence_proof_in_flight,
                                            [request],
                                        )?;
                                        emit_runtime_async_observations(
                                            &observer,
                                            startup_async_lanes,
                                            presented.key.clone(),
                                        );
                                        emit_runtime_async_lanes(
                                            &observer,
                                            runtime.as_mut().expect("mounted runtime"),
                                            &product,
                                        )?;
                                        state_mount_captured = true;
                                    }
                                    if state_evidence.migration_exercise
                                        && !migration_evidence_completed
                                    {
                                        let requests = run_schema_migration_evidence(
                                            &observer,
                                            source_revision,
                                            runtime.as_mut().expect("mounted runtime"),
                                            &mut view,
                                            &mut product,
                                            &mut host,
                                            &mut columns,
                                            migration.as_ref().ok_or(
                                                "migration evidence requires a source-controlled migration bundle",
                                            )?,
                                        )
                                        .await?;
                                        queue_evidence_proofs(
                                            &observer,
                                            proof.as_ref(),
                                            &mut queued_evidence_proofs,
                                            &mut evidence_proof_in_flight,
                                            requests,
                                        )?;
                                        migration_evidence_completed = true;
                                        runtime_key = None;
                                    }
                                }
                                output.send(Message::PreviewStatus {
                                    revision: source_revision,
                                    ok: true,
                                    message: format!(
                                        "typed runtime and retained document mounted in {compile_elapsed_ms:.2}ms"
                                    ),
                                })?;
                                if let (Some(_), Some(active_stage), Some(runtime)) = (
                                    migration.as_ref(),
                                    active_migration_stage.as_ref(),
                                    runtime.as_ref(),
                                ) {
                                    output.send(Message::PreviewMigrationStatus(
                                        MigrationStatus {
                                            request_id: None,
                                            revision: source_revision,
                                            operation: MigrationOperation::Opened,
                                            ok: true,
                                            active_stage: active_stage.clone(),
                                            previewed_stage: previewed_migration_stage.clone(),
                                            target_stage: None,
                                            target_schema_version: runtime
                                                .persistence_schema_version(),
                                            migration_step_count: 0,
                                            deleted_memory_count: 0,
                                            message: format!(
                                                "Opened migration sequence at {active_stage}"
                                            ),
                                        },
                                    ))?;
                                }
                                push_persistence_snapshot(
                                    &output,
                                    runtime.as_ref(),
                                    source_revision,
                                    &mut persistence_snapshot_sequence,
                                    last_persistence_operation.as_ref(),
                                    import_preview.as_ref(),
                                )?;
                                if test {
                                    if !state_evidence.native_workflow_steps.is_empty() {
                                        let initial_key = presented
                                            .as_ref()
                                            .map(|frame| frame.key.clone())
                                            .ok_or("native workflow TEST did not mount a presented frame")?;
                                        native_workflow = NativeWorkflowState::from_test_steps(
                                            request_id,
                                            &steps,
                                            &state_evidence.native_workflow_steps,
                                            &state_evidence.native_workflow_proof_steps,
                                        )?;
                                        product.set_virtual_cursor(Some(cursor));
                                        if state_evidence.profile_samples > 0 {
                                            profile_benchmark =
                                                Some(arm_product_profile_benchmark(
                                                    &observer,
                                                    runtime.as_mut().expect("mounted runtime"),
                                                    &view,
                                                    &steps,
                                                    state_evidence.profile_samples,
                                                    &state_evidence.profile_steps,
                                                    initial_key.clone(),
                                                )?);
                                        }
                                        if let Some(target) = first_test_target(
                                            runtime.as_mut().expect("mounted runtime"),
                                            &view,
                                            &steps,
                                        )
                                        .or_else(|| view.first_visible_hit_target())
                                        {
                                            emit(
                                                &observer,
                                                ObserverEvent::TestTarget {
                                                    request_id,
                                                    node: target.node,
                                                    source_path: target
                                                        .source_path
                                                        .unwrap_or_else(|| "unbound".to_owned()),
                                                    x: target.center_x,
                                                    y: target.center_y,
                                                },
                                            );
                                        }
                                        push_persistence_snapshot(
                                            &output,
                                            runtime.as_ref(),
                                            source_revision,
                                            &mut persistence_snapshot_sequence,
                                            last_persistence_operation.as_ref(),
                                            import_preview.as_ref(),
                                        )?;
                                    } else {
                                        let result = run_test(
                                            &observer,
                                            &output,
                                            request_id,
                                            source_revision,
                                            runtime.as_mut().expect("mounted runtime"),
                                            &mut view,
                                            &mut product,
                                            &mut host,
                                            &mut columns,
                                            &steps,
                                            &mut cursor,
                                            &state_evidence,
                                        )
                                        .await;
                                        let (
                                            passed,
                                            semantic_assertions_proven,
                                            completed,
                                            message,
                                        ) = match result {
                                            Ok(outcome) => {
                                                let last_key = outcome.last_key.clone();
                                                queue_evidence_proofs(
                                                    &observer,
                                                    proof.as_ref(),
                                                    &mut queued_evidence_proofs,
                                                    &mut evidence_proof_in_flight,
                                                    outcome.proof_requests,
                                                )?;
                                                if state_evidence.profile_samples > 0 {
                                                    profile_benchmark =
                                                        Some(arm_product_profile_benchmark(
                                                            &observer,
                                                            runtime
                                                                .as_mut()
                                                                .expect("mounted runtime"),
                                                            &view,
                                                            &steps,
                                                            state_evidence.profile_samples,
                                                            &state_evidence.profile_steps,
                                                            last_key.clone(),
                                                        )?);
                                                }
                                                if let Some(width) = state_evidence.responsive_width
                                                {
                                                    responsive_evidence =
                                                        Some(arm_responsive_evidence(
                                                            &observer,
                                                            runtime
                                                                .as_mut()
                                                                .expect("mounted runtime"),
                                                            &view,
                                                            &host,
                                                            width,
                                                            &state_evidence
                                                                .responsive_navigation_sources,
                                                            last_key,
                                                        )?);
                                                }
                                                (
                                                    true,
                                                    outcome.semantic_assertions_proven,
                                                    outcome.completed_steps,
                                                    if outcome.semantic_assertions_proven {
                                                        format!(
                                                            "{} public HostEvent steps and their semantic assertions passed",
                                                            outcome.completed_steps
                                                        )
                                                    } else {
                                                        format!(
                                                            "{} public HostEvent steps passed without declared semantic assertions",
                                                            outcome.completed_steps
                                                        )
                                                    },
                                                )
                                            }
                                            Err(error) => (false, false, 0, error.to_string()),
                                        };
                                        if std::env::var_os(PRODUCT_PROOF_AFTER_TEST_ENV).is_some()
                                        {
                                            proof_requested = false;
                                            proof_eligible_ordinal = 0;
                                        }
                                        if let Some(runtime) = runtime.as_ref()
                                            && let Some(target) =
                                                first_test_target(runtime, &view, &steps)
                                                    .or_else(|| view.first_visible_hit_target())
                                        {
                                            emit(
                                                &observer,
                                                ObserverEvent::TestTarget {
                                                    request_id,
                                                    node: target.node,
                                                    source_path: target
                                                        .source_path
                                                        .unwrap_or_else(|| "unbound".to_owned()),
                                                    x: target.center_x,
                                                    y: target.center_y,
                                                },
                                            );
                                        }
                                        emit(
                                            &observer,
                                            ObserverEvent::TestCompleted {
                                                request_id,
                                                passed,
                                                semantic_assertions_proven,
                                                completed_steps: completed
                                                    .try_into()
                                                    .unwrap_or(u32::MAX),
                                                message: message.clone(),
                                            },
                                        );
                                        output.send(Message::PreviewTestResult {
                                            request_id,
                                            passed,
                                            message,
                                        })?;
                                        push_persistence_snapshot(
                                            &output,
                                            runtime.as_ref(),
                                            source_revision,
                                            &mut persistence_snapshot_sequence,
                                            last_persistence_operation.as_ref(),
                                            import_preview.as_ref(),
                                        )?;
                                    }
                                }
                                send_stats(
                                    &output,
                                    &product,
                                    runtime.as_ref(),
                                    source_revision,
                                    FrameMode::Idle,
                                    compiler.replaced_count(),
                                    &mut last_stats_sent,
                                    true,
                                )?;
                            }
                            Err(error) => {
                                emit(
                                    &observer,
                                    ObserverEvent::SourceFailed {
                                        revision: source_revision,
                                        stage: "runtime-mount".to_owned(),
                                        message: error.clone(),
                                    },
                                );
                                if runtime.is_none() {
                                    show_error(
                                        &observer,
                                        &mut view,
                                        &mut product,
                                        &mut host,
                                        &mut columns,
                                        &error,
                                    )
                                    .await?;
                                }
                                output.send(Message::PreviewStatus {
                                    revision: source_revision,
                                    ok: false,
                                    message: error,
                                })?;
                            }
                        }
                    }
                    Err(error) => {
                        emit(
                            &observer,
                            ObserverEvent::SourceFailed {
                                revision: outcome.revision,
                                stage: "compile".to_owned(),
                                message: error.clone(),
                            },
                        );
                        show_error(
                            &observer,
                            &mut view,
                            &mut product,
                            &mut host,
                            &mut columns,
                            &error,
                        )
                        .await?;
                        output.send(Message::PreviewStatus {
                            revision: outcome.revision,
                            ok: false,
                            message: error,
                        })?;
                    }
                }
            }
            Wake::ProgramCompiled(outcome) => {
                let outcome = outcome.ok_or("child program compiler stopped")?;
                let Some(model) = runtime.as_mut() else {
                    continue;
                };
                let async_request_id = format!("{}:{}", outcome.session.0, outcome.request_id.0);
                let async_revision = outcome.revision;
                let async_queue_depth = outcome.pending_depth;
                let async_queue_wait_us = duration_us(outcome.queue_wait);
                let async_worker_us = duration_us(outcome.elapsed);
                let async_queued_at = outcome.queued_at;
                let async_completed_at = outcome.completed_at;
                let compile_failed = outcome.result.is_err();
                let profile_request = profile_benchmark.as_ref().is_some_and(|benchmark| {
                    profile_candidate_has_request(
                        benchmark,
                        &outcome.session,
                        &outcome.request_id,
                        outcome.revision,
                    )
                });
                let profile_result_invalid = profile_request && outcome.result.is_err();
                let completion_started = Instant::now();
                let observed = model.complete_program_observed(
                    &outcome.session,
                    &outcome.request_id,
                    outcome.result,
                )?;
                let async_completion = observed.completion.clone();
                let completion_us = duration_us(completion_started.elapsed());
                let completion_phase = model.last_runtime_phase();
                let artifact_changed = model.resolve_program_artifact_requests()?;
                let changed = observed.changed || artifact_changed;
                let update = if changed {
                    apply_runtime_update_measured(model, &mut view, &mut columns)?
                } else {
                    RuntimeUpdateMeasurement::default()
                };
                submit_program_requests(model.take_program_requests(), &program_compiler);
                let mut child_frame = None;
                if changed && let Some(presented) = product.present(&mut host, &view).await? {
                    let proof_request = prepare_product_proof_request(
                        profile_benchmark.as_ref(),
                        native_workflow.as_ref(),
                        proof.as_ref(),
                        proof_config.as_ref(),
                        &mut proof_requested,
                        &mut proof_eligible_ordinal,
                        &presented,
                        &mut product,
                    )?;
                    emit_presented(&observer, &presented);
                    latest_presented_key = Some(presented.key.clone());
                    submit_proof_request(&observer, proof.as_ref(), proof_request)?;
                    child_frame = Some(presented);
                }
                if profile_request {
                    let benchmark = profile_benchmark.as_mut().expect("profile benchmark");
                    if profile_result_invalid {
                        if record_profile_invalid_completion(benchmark)? {
                            return Err(
                                "profile uinput batch produced an invalid final child program"
                                    .into(),
                            );
                        }
                    } else {
                        let completion = match observed.completion {
                            ProgramCompletionObservation::Host(completion) => completion,
                            ProgramCompletionObservation::ArtifactStorePending { .. } => {
                                return Err(
                                    "profile child compile unexpectedly required artifact persistence"
                                        .into(),
                                );
                            }
                        };
                        record_profile_program_completion(
                            benchmark,
                            &outcome.session,
                            &outcome.request_id,
                            outcome.revision,
                            duration_us(outcome.elapsed),
                            outcome.pending_depth,
                            completion_us,
                            completion_phase,
                            update,
                            completion,
                            child_frame,
                        )?;
                        finalize_ready_profile_candidate(
                            &observer,
                            source_revision,
                            model,
                            benchmark,
                            &mut view,
                            &mut product,
                            &mut host,
                            &mut columns,
                            proof.as_ref(),
                            &mut queued_evidence_proofs,
                            &mut evidence_proof_in_flight,
                            &program_compiler,
                            &mut latest_presented_key,
                        )
                        .await?;
                    }
                }
                let async_key = product
                    .last_presented_key()
                    .cloned()
                    .ok_or("child compile completed before a production frame existed")?;
                let async_apply_us = duration_us(async_completed_at.elapsed());
                let async_end_to_end_us = accounted_end_to_end_us(
                    duration_us(async_queued_at.elapsed()),
                    async_queue_wait_us,
                    async_worker_us,
                    async_apply_us,
                );
                emit(
                    &observer,
                    ObserverEvent::AsyncLaneCompleted {
                        lane: AsyncLaneKind::ChildProgramCompile,
                        request_id: async_request_id,
                        revision: async_revision,
                        queue_depth: async_queue_depth,
                        queue_wait_us: async_queue_wait_us,
                        worker_us: async_worker_us,
                        apply_us: async_apply_us,
                        end_to_end_us: async_end_to_end_us,
                        outcome: program_async_lane_outcome(&async_completion, compile_failed),
                        key: async_key,
                    },
                );
                send_program_status(&output, Some(model), source_revision)?;
            }
            Wake::Proof(result) => {
                let proof_apply_started = Instant::now();
                let result = result.ok_or("proof worker stopped")?;
                let completed_key = result.key.clone();
                let proof_queue_wait_us = duration_us(result.queue_wait);
                let proof_worker_us = duration_us(result.elapsed);
                let proof_end_to_end_us = duration_us(result.end_to_end);
                let proof_queue_depth = result.queue_depth;
                let proof_failed = result.proof.is_err();
                let completed_after_key = product
                    .last_presented_key()
                    .cloned()
                    .ok_or("proof completed before any production frame was presented")?;
                if !completed_key.same_producer_surface(&completed_after_key)
                    || completed_after_key.frame_id < completed_key.frame_id
                    || completed_after_key.present_id < completed_key.present_id
                {
                    return Err("proof completion is not ordered after its production frame".into());
                }
                let worker = proof.as_ref().expect("proof result without worker");
                emit(
                    &observer,
                    result.observer_event(
                        completed_after_key.clone(),
                        worker.replaced_count(),
                        worker.result_drop_count(),
                    ),
                );
                let proof_apply_us = duration_us(proof_apply_started.elapsed());
                let proof_end_to_end_us = accounted_end_to_end_us(
                    proof_end_to_end_us.saturating_add(proof_apply_us),
                    proof_queue_wait_us,
                    proof_worker_us,
                    proof_apply_us,
                );
                emit(
                    &observer,
                    ObserverEvent::AsyncLaneCompleted {
                        lane: AsyncLaneKind::ProofReadback,
                        request_id: format!("proof-{}", completed_key.proof_id),
                        revision: completed_key.frame_id,
                        queue_depth: proof_queue_depth,
                        queue_wait_us: proof_queue_wait_us,
                        worker_us: proof_worker_us,
                        apply_us: proof_apply_us,
                        end_to_end_us: proof_end_to_end_us,
                        outcome: if proof_failed {
                            AsyncLaneOutcome::Failed
                        } else {
                            AsyncLaneOutcome::Applied
                        },
                        key: completed_after_key,
                    },
                );
                if evidence_proof_in_flight.as_ref() == Some(&completed_key) {
                    evidence_proof_in_flight = None;
                    queue_evidence_proofs(
                        &observer,
                        proof.as_ref(),
                        &mut queued_evidence_proofs,
                        &mut evidence_proof_in_flight,
                        [],
                    )?;
                }
            }
            Wake::MapTile(wake) => {
                wake.ok_or("native map tile worker stopped")?;
                let (map_visible_changed, _map_tile_events) = product.service_map_tiles()?;
                if map_visible_changed && runtime.is_some() {
                    if let Some(presented) = product.present(&mut host, &view).await? {
                        emit_presented(&observer, &presented);
                        latest_presented_key = Some(presented.key.clone());
                    }
                    send_stats(
                        &output,
                        &product,
                        runtime.as_ref(),
                        source_revision,
                        FrameMode::Burst,
                        compiler.replaced_count(),
                        &mut last_stats_sent,
                        false,
                    )?;
                }
            }
            Wake::Scheduled(tick) => {
                tick.ok_or("preview deadline scheduler stopped")?;
                deadline_scheduler.fired();
                if let Some(runtime) = runtime.as_mut() {
                    let now = Instant::now();
                    let sequence_before = runtime.event_sequence();
                    let runtime_turn_before = runtime.runtime_turn_sequence();
                    let timer_changed = runtime.advance_scheduled_sources(now)?;
                    let caret_changed = runtime.advance_caret_blink(now);
                    let artifact_changed = runtime.poll_program_artifact_stores()?;
                    let persistence_changed = runtime.poll_persistence_acknowledgement(now);
                    let effect_changed = runtime.poll_host_effects(now)?;
                    if timer_changed || caret_changed || artifact_changed || effect_changed {
                        apply_runtime_update(runtime, &mut view, &mut columns)?;
                    }
                    if timer_changed || caret_changed || artifact_changed || effect_changed {
                        if let Some(presented) = product.present(&mut host, &view).await? {
                            emit_presented(&observer, &presented);
                            latest_presented_key = Some(presented.key.clone());
                        }
                        send_stats(
                            &output,
                            &product,
                            Some(&*runtime),
                            source_revision,
                            FrameMode::Burst,
                            compiler.replaced_count(),
                            &mut last_stats_sent,
                            false,
                        )?;
                    }
                    if runtime.event_sequence() > sequence_before {
                        output.send(Message::PreviewRuntimeChanged {
                            revision: source_revision,
                            runtime_sequence: runtime.event_sequence(),
                        })?;
                    }
                    if persistence_changed
                        || artifact_changed
                        || effect_changed
                        || runtime.runtime_turn_sequence() > runtime_turn_before
                    {
                        if runtime.runtime_turn_sequence() > runtime_turn_before {
                            import_preview = None;
                        }
                        push_persistence_snapshot(
                            &output,
                            Some(&*runtime),
                            source_revision,
                            &mut persistence_snapshot_sequence,
                            last_persistence_operation.as_ref(),
                            import_preview.as_ref(),
                        )?;
                    }
                }
            }
        }
        let profile_complete = profile_benchmark
            .as_ref()
            .is_none_or(|benchmark| benchmark.phase.is_complete());
        if let (Some(workflow), Some(model)) = (native_workflow.as_mut(), runtime.as_mut()) {
            service_native_workflow(
                workflow,
                profile_complete,
                &observer,
                &output,
                &state_evidence,
                source_revision,
                model,
                &mut view,
                &mut product,
                &mut host,
                &mut columns,
                &mut latest_presented_key,
                proof.as_ref(),
                &mut queued_evidence_proofs,
                &mut evidence_proof_in_flight,
                cursor,
            )
            .await?;
            if responsive_evidence.is_none()
                && workflow.complete()
                && let Some(width) = state_evidence.responsive_width
            {
                responsive_evidence = Some(arm_responsive_evidence(
                    &observer,
                    runtime.as_mut().expect("mounted runtime"),
                    &view,
                    &host,
                    width,
                    &state_evidence.responsive_navigation_sources,
                    latest_presented_key
                        .clone()
                        .ok_or("responsive workflow completed without a baseline frame")?,
                )?);
            }
        }
        if let Some(model) = runtime.as_mut() {
            emit_runtime_async_lanes(&observer, model, &product)?;
        }
    }
}

fn observe_native_workflow_input(
    workflow: &mut NativeWorkflowState,
    envelope: &HostEventEnvelope,
    pointer_source_path: Option<&str>,
    focused_node: Option<&str>,
    dispatches: &[RuntimeSourceDispatch],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !workflow.prepared || workflow.complete() || envelope.origin != HostEventOrigin::RealOs {
        return Ok(());
    }
    let step = workflow
        .current()
        .cloned()
        .ok_or("native workflow lost its current step")?;
    let Some(action_kind) = step.action_kind.as_deref() else {
        return Ok(());
    };
    let Some(pending) = workflow.pending.as_mut() else {
        return Ok(());
    };
    if pending.started_at.is_none() {
        let starts_on_target = matches!(
            &envelope.event,
            HostEvent::Pointer(PointerEvent {
                phase: PointerPhase::Down,
                button: Some(PointerButton::Primary),
                ..
            })
        ) && pointer_source_path == Some(step.source_path.as_str());
        let starts_on_blur = action_kind == "blur"
            && matches!(envelope.event, HostEvent::Focus { focused: false, .. });
        let starts_focused = matches!(action_kind, "focused_key" | "focused_chord")
            && matches!(
                envelope.event,
                HostEvent::Keyboard(KeyEvent { pressed: true, .. })
            )
            && focused_node == Some(pending.target_node.as_str());
        if !starts_on_target && !starts_on_blur && !starts_focused {
            return Ok(());
        }
        pending.started_at = Some(Instant::now());
    }
    if pending.action_complete {
        return Err(format!(
            "native workflow step `{}` received input after its action span closed",
            step.id
        )
        .into());
    }
    if pending
        .last_sequence
        .is_some_and(|last| envelope.sequence <= last)
    {
        return Err(format!(
            "native workflow step `{}` received a non-monotonic input sequence",
            step.id
        )
        .into());
    }
    pending.first_sequence.get_or_insert(envelope.sequence);
    pending.last_sequence = Some(envelope.sequence);
    pending.event_digests.push(host_event_digest(envelope));
    if pending.event_digests.len() > MAX_NATIVE_WORKFLOW_INPUT_EVENTS {
        return Err(format!(
            "native workflow step `{}` exceeded its bounded {}-event input span",
            step.id, MAX_NATIVE_WORKFLOW_INPUT_EVENTS
        )
        .into());
    }

    if matches!(
        envelope.event,
        HostEvent::Pointer(PointerEvent {
            phase: PointerPhase::Up,
            button: Some(PointerButton::Primary),
            ..
        })
    ) {
        pending.last_pointer_up_source_path = pointer_source_path.map(str::to_owned);
        pending.last_pointer_up_dispatched_source_paths = dispatches
            .iter()
            .map(|dispatch| dispatch.source_path.clone())
            .collect();
    }

    let dispatched_declared_source = dispatches
        .iter()
        .any(|dispatch| dispatch.source_path == step.source_path);

    match (action_kind, &envelope.event) {
        (
            "click" | "double_click",
            HostEvent::Pointer(PointerEvent {
                phase: PointerPhase::Up,
                button: Some(PointerButton::Primary),
                ..
            }),
        ) if pointer_source_path == Some(step.source_path.as_str())
            && dispatched_declared_source =>
        {
            pending.pointer_up_count = pending.pointer_up_count.saturating_add(1);
            pending.action_complete = action_kind == "click" || pending.pointer_up_count == 2;
        }
        ("type_text", HostEvent::TextInput(text)) if dispatched_declared_source => {
            let expected = step
                .text
                .as_deref()
                .ok_or("native workflow type_text step has no text")?;
            pending.batch_text.push_str(&text.text);
            if !expected.starts_with(&pending.batch_text) {
                return Err(format!(
                    "native workflow step `{}` received text outside its declared batch",
                    step.id
                )
                .into());
            }
        }
        ("type_text", event) if is_ascii_batch_end(event) => {
            pending.action_complete = step.text.as_deref() == Some(pending.batch_text.as_str());
            if !pending.action_complete {
                return Err(format!(
                    "native workflow step `{}` closed an incomplete text batch",
                    step.id
                )
                .into());
            }
        }
        ("key" | "focused_key" | "focused_chord", HostEvent::Keyboard(key)) => {
            let expected = step
                .key
                .as_deref()
                .ok_or("native workflow key step has no declared key")?;
            let (phase, complete_phase) =
                native_workflow_keyboard_phase(action_kind, key, expected)
                    .ok_or("native workflow received an undeclared key")?;
            if focused_node != Some(pending.target_node.as_str())
                || phase != pending.keyboard_phase.saturating_add(1)
            {
                return Err("native workflow key sequence or focus mismatch".into());
            }
            pending.keyboard_phase = phase;
            pending.action_complete = phase == complete_phase;
        }
        ("blur", HostEvent::Focus { focused: false, .. }) => {
            pending.action_complete = true;
        }
        _ => {}
    }
    Ok(())
}

fn native_workflow_keyboard_phase(
    action_kind: &str,
    key: &KeyEvent,
    expected: &str,
) -> Option<(u8, u8)> {
    let actual = match &key.logical_key {
        LogicalKey::Named(actual) | LogicalKey::Character(actual) => actual,
        LogicalKey::Dead(_) | LogicalKey::Unidentified => return None,
    };
    let actual = crate::runtime_view::normalize_key(actual);
    let expected = crate::runtime_view::normalize_key(expected);
    match action_kind {
        "key" | "focused_key" if actual == expected => Some((if key.pressed { 1 } else { 2 }, 2)),
        "focused_chord" => {
            let (modifier, expected_key) = expected.split_once('+')?;
            let modifier_matches = match modifier {
                "ctrl" => actual.starts_with("control") || actual.starts_with("ctrl"),
                "shift" => actual.starts_with("shift"),
                _ => false,
            };
            let phase = if modifier_matches {
                if key.pressed { 1 } else { 4 }
            } else if actual == expected_key {
                if key.pressed { 2 } else { 3 }
            } else {
                return None;
            };
            Some((phase, 4))
        }
        _ => None,
    }
}

fn native_workflow_pointer_presentation(
    workflow: Option<&NativeWorkflowState>,
    event: &HostEvent,
    target: Option<&str>,
    runtime_sequence: u64,
) -> Option<NativeWorkflowPointerPresentation> {
    let workflow = workflow.filter(|workflow| workflow.prepared && !workflow.complete())?;
    let pointer = match event {
        HostEvent::Pointer(pointer) => pointer,
        _ => return None,
    };
    let phase = match pointer.phase {
        PointerPhase::Move => TestPointerPhase::Move,
        PointerPhase::Down if pointer.button == Some(PointerButton::Primary) => {
            TestPointerPhase::Down
        }
        PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => TestPointerPhase::Up,
        _ => return None,
    };
    Some(NativeWorkflowPointerPresentation {
        request_id: workflow.test_request_id,
        step_index: workflow.completed.try_into().unwrap_or(u32::MAX),
        phase,
        x: pointer.x,
        y: pointer.y,
        target: target.map(str::to_owned),
        runtime_sequence,
    })
}

#[allow(clippy::too_many_arguments)]
async fn service_native_workflow(
    workflow: &mut NativeWorkflowState,
    profile_complete: bool,
    observer: &Option<ObserverClient>,
    output: &PreviewOutput,
    state_evidence: &StateEvidenceConfig,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    latest_presented_key: &mut Option<crate::observer::FrameEvidenceKey>,
    proof: Option<&ProofWorker>,
    queued_evidence_proofs: &mut VecDeque<PreparedProofRequest>,
    evidence_proof_in_flight: &mut Option<crate::observer::FrameEvidenceKey>,
    cursor: (f32, f32),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if workflow.complete() && workflow.test_completed_emitted {
        return Ok(());
    }
    if !workflow.prepared {
        if !profile_complete {
            return Ok(());
        }
        if !workflow.host_evidence_complete {
            let key = latest_presented_key
                .clone()
                .ok_or("native workflow host evidence has no presented baseline")?;
            if state_evidence.persistence_exercise {
                let requests = run_persistence_evidence(
                    observer,
                    source_revision,
                    runtime,
                    view,
                    product,
                    host,
                    columns,
                    key,
                )
                .await?;
                queue_evidence_proofs(
                    observer,
                    proof,
                    queued_evidence_proofs,
                    evidence_proof_in_flight,
                    requests,
                )?;
            }
            if state_evidence.stale_program {
                let request = run_stale_program_evidence(
                    observer,
                    source_revision,
                    runtime,
                    view,
                    product,
                    host,
                    columns,
                    &workflow.steps,
                    &state_evidence.profile_steps,
                )
                .await?;
                queue_evidence_proofs(
                    observer,
                    proof,
                    queued_evidence_proofs,
                    evidence_proof_in_flight,
                    [request],
                )?;
            }
            workflow.host_evidence_complete = true;
        }
        runtime.start_over()?;
        view.replace(runtime.frame(), viewport(host), columns)?;
        settle_test_runtime(runtime, view, columns)?;
        let frame = present_runtime(runtime, view, product, host, columns)
            .await?
            .ok_or("native workflow reset did not present")?;
        emit_presented(observer, &frame);
        *latest_presented_key = Some(frame.key.clone());
        let state = authoritative_state_evidence(runtime)?;
        workflow.initial_state_digest = Some(state.digest.clone());
        workflow.current_state_digest = Some(state.digest.clone());
        workflow.prepared = true;
        emit(
            observer,
            ObserverEvent::NativeWorkflowReady {
                test_request_id: workflow.test_request_id,
                step_count: workflow.steps.len().try_into().unwrap_or(u32::MAX),
                source_revision,
                runtime_sequence: runtime.runtime_turn_sequence(),
                durable_epoch: state.durable_epoch,
                state_digest: state.digest,
                key: frame.key,
            },
        );
    }

    loop {
        while workflow
            .current()
            .is_some_and(|step| step.action_kind.is_none())
        {
            let step = workflow
                .current()
                .cloned()
                .ok_or("native workflow lost its assertion-only step")?;
            let assertion_count = assert_test_step_semantics(runtime, &step)?;
            let state = authoritative_state_evidence(runtime)?;
            let durable_acked = state.durable_turn_sequence >= runtime.runtime_turn_sequence();
            if !durable_acked {
                return Err(format!(
                    "native workflow step `{}` was observed before its durable acknowledgement",
                    step.id
                )
                .into());
            }
            let frame = present_runtime(runtime, view, product, host, columns)
                .await?
                .ok_or("native workflow assertion frame did not present")?;
            emit_presented(observer, &frame);
            *latest_presented_key = Some(frame.key.clone());
            let ordinal = workflow.completed.saturating_add(1);
            let request_id = native_workflow_request_id(workflow.test_request_id, ordinal);
            let before_state_digest = workflow
                .current_state_digest
                .clone()
                .ok_or("native workflow assertion has no prior state digest")?;
            emit(
                observer,
                ObserverEvent::NativeWorkflowStep {
                    request_id,
                    ordinal: ordinal.try_into().unwrap_or(u32::MAX),
                    step_id: step.id.clone(),
                    source_path: "assertion-only".to_owned(),
                    action_kind: "assertion_only".to_owned(),
                    action_digest: native_workflow_action_digest(&step),
                    input_first_sequence: 0,
                    input_last_sequence: 0,
                    input_event_count: 0,
                    input_event_digest: native_workflow_input_digest(&[]),
                    assertion_count: assertion_count.try_into().unwrap_or(u32::MAX),
                    source_revision,
                    runtime_sequence: runtime.runtime_turn_sequence(),
                    durable_epoch: state.durable_epoch,
                    durable_turn_sequence: state.durable_turn_sequence,
                    durable_acked,
                    before_state_digest,
                    state_digest: state.digest.clone(),
                    key: frame.key.clone(),
                },
            );
            emit_native_workflow_state_frame(
                observer,
                workflow,
                cursor,
                runtime.runtime_turn_sequence(),
                frame.key.clone(),
            );
            if workflow.proof_steps.contains(&step.id) {
                queue_evidence_proofs(
                    observer,
                    proof,
                    queued_evidence_proofs,
                    evidence_proof_in_flight,
                    [prepare_evidence_proof(
                        &format!("native-workflow-{}", step.id),
                        frame.key.clone(),
                        product,
                    )?],
                )?;
            }
            workflow.current_state_digest = Some(state.digest);
            workflow.completed = workflow.completed.saturating_add(1);
        }

        if !workflow
            .pending
            .as_ref()
            .is_some_and(|pending| pending.action_complete)
        {
            if let Some(pending) = workflow.pending.as_ref()
                && pending
                    .started_at
                    .is_some_and(|started| started.elapsed() >= TEST_SETTLE_TIMEOUT)
            {
                return Err(format!(
                    "native workflow step `{}` did not complete its declared real-input span; events={}, pointer_ups={}, first_sequence={:?}, last_sequence={:?}, pointer_up_source={:?}, pointer_up_dispatch={:?}",
                    workflow
                        .current()
                        .map_or("unknown", |step| step.id.as_str()),
                    pending.event_digests.len(),
                    pending.pointer_up_count,
                    pending.first_sequence,
                    pending.last_sequence,
                    pending.last_pointer_up_source_path,
                    pending.last_pointer_up_dispatched_source_paths,
                )
                .into());
            }
            break;
        }

        let step = workflow
            .current()
            .cloned()
            .ok_or("native workflow accepted input after its final step")?;
        let pending_started_at = workflow
            .pending
            .as_ref()
            .and_then(|pending| pending.started_at);
        match assert_test_step_semantics(runtime, &step) {
            Ok(assertion_count) => {
                let pending = workflow
                    .pending
                    .take()
                    .expect("checked native workflow action span");
                let state = authoritative_state_evidence(runtime)?;
                let durable_acked = state.durable_turn_sequence >= runtime.runtime_turn_sequence();
                if !durable_acked {
                    return Err(format!(
                        "native workflow step `{}` was observed before its durable acknowledgement",
                        step.id
                    )
                    .into());
                }
                let frame = present_runtime(runtime, view, product, host, columns)
                    .await?
                    .ok_or("native workflow evidence frame did not present")?;
                emit_presented(observer, &frame);
                *latest_presented_key = Some(frame.key.clone());
                let ordinal = workflow.completed.saturating_add(1);
                let first_sequence = pending
                    .first_sequence
                    .ok_or("native workflow action has no first input sequence")?;
                let last_sequence = pending
                    .last_sequence
                    .ok_or("native workflow action has no last input sequence")?;
                emit(
                    observer,
                    ObserverEvent::NativeWorkflowStep {
                        request_id: pending.request_id,
                        ordinal: ordinal.try_into().unwrap_or(u32::MAX),
                        step_id: step.id.clone(),
                        source_path: step.source_path.clone(),
                        action_kind: step.action_kind.clone().expect("validated workflow action"),
                        action_digest: pending.action_digest,
                        input_first_sequence: first_sequence,
                        input_last_sequence: last_sequence,
                        input_event_count: pending
                            .event_digests
                            .len()
                            .try_into()
                            .unwrap_or(u32::MAX),
                        input_event_digest: native_workflow_input_digest(&pending.event_digests),
                        assertion_count: assertion_count.try_into().unwrap_or(u32::MAX),
                        source_revision,
                        runtime_sequence: runtime.runtime_turn_sequence(),
                        durable_epoch: state.durable_epoch,
                        durable_turn_sequence: state.durable_turn_sequence,
                        durable_acked,
                        before_state_digest: pending.before_state_digest,
                        state_digest: state.digest.clone(),
                        key: frame.key.clone(),
                    },
                );
                emit_native_workflow_state_frame(
                    observer,
                    workflow,
                    cursor,
                    runtime.runtime_turn_sequence(),
                    frame.key.clone(),
                );
                if workflow.proof_steps.contains(&step.id) {
                    queue_evidence_proofs(
                        observer,
                        proof,
                        queued_evidence_proofs,
                        evidence_proof_in_flight,
                        [prepare_evidence_proof(
                            &format!("native-workflow-{}", step.id),
                            frame.key.clone(),
                            product,
                        )?],
                    )?;
                }
                workflow.current_state_digest = Some(state.digest);
                workflow.completed = workflow.completed.saturating_add(1);
                continue;
            }
            Err(error)
                if pending_started_at
                    .is_some_and(|started| started.elapsed() >= TEST_SETTLE_TIMEOUT) =>
            {
                return Err(format!(
                    "native workflow step `{}` did not reach its declared semantics: {error}",
                    step.id
                )
                .into());
            }
            Err(_) => return Ok(()),
        }
    }

    if workflow.complete() {
        if !workflow.test_completed_emitted {
            let key = latest_presented_key
                .clone()
                .ok_or("native workflow completed without a presented frame")?;
            let initial_state_digest = workflow
                .initial_state_digest
                .clone()
                .ok_or("native workflow has no reset digest")?;
            let final_state_digest = workflow
                .current_state_digest
                .clone()
                .ok_or("native workflow has no final digest")?;
            emit(
                observer,
                ObserverEvent::NativeWorkflowCompleted {
                    test_request_id: workflow.test_request_id,
                    step_count: workflow.completed.try_into().unwrap_or(u32::MAX),
                    initial_state_digest,
                    final_state_digest,
                    key,
                },
            );
            emit(
                observer,
                ObserverEvent::TestCompleted {
                    request_id: workflow.test_request_id,
                    passed: true,
                    semantic_assertions_proven: true,
                    completed_steps: workflow.completed.try_into().unwrap_or(u32::MAX),
                    message: format!(
                        "{} isolated kernel-uinput steps and semantic assertions passed",
                        workflow.completed
                    ),
                },
            );
            output.send(Message::PreviewTestResult {
                request_id: workflow.test_request_id,
                passed: true,
                message: format!(
                    "{} isolated kernel-uinput steps and semantic assertions passed",
                    workflow.completed
                ),
            })?;
            workflow.test_completed_emitted = true;
            product.set_virtual_cursor(None);
        }
        return Ok(());
    }

    emit_current_native_workflow_target(
        workflow,
        observer,
        runtime,
        view,
        latest_presented_key.as_ref(),
    )?;
    Ok(())
}

fn emit_native_workflow_state_frame(
    observer: &Option<ObserverClient>,
    workflow: &NativeWorkflowState,
    cursor: (f32, f32),
    runtime_sequence: u64,
    key: crate::observer::FrameEvidenceKey,
) {
    emit(
        observer,
        ObserverEvent::TestPointerFrame {
            request_id: workflow.test_request_id,
            step_index: workflow.completed.try_into().unwrap_or(u32::MAX),
            phase: TestPointerPhase::State,
            x: cursor.0,
            y: cursor.1,
            target: None,
            runtime_sequence,
            key,
        },
    );
}

fn emit_current_native_workflow_target(
    workflow: &mut NativeWorkflowState,
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    view: &RetainedView,
    key: Option<&crate::observer::FrameEvidenceKey>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if workflow.pending.is_some() || workflow.complete() {
        return Ok(());
    }
    let step = workflow
        .current()
        .cloned()
        .ok_or("native workflow has no target step")?;
    let action_kind = step
        .action_kind
        .clone()
        .ok_or("native workflow target cannot represent an assertion-only step")?;
    let target_row = runtime.scenario_target_row(
        &step.source_path,
        step.target_text.as_deref(),
        step.address.as_deref(),
        step.target_occurrence,
    )?;
    let Some(target) = view.target_for_scenario(
        &step.source_path,
        step.action_kind.as_deref(),
        step.target_text.as_deref(),
        step.address.as_deref(),
        target_row,
    ) else {
        return Ok(());
    };
    let key = key
        .cloned()
        .ok_or("native workflow target has no presented frame")?;
    let point = test_step_pointer_position(view, &target, &step);
    let ordinal = workflow.completed.saturating_add(1);
    let request_id = native_workflow_request_id(workflow.test_request_id, ordinal);
    let action_digest = native_workflow_action_digest(&step);
    workflow.pending = Some(NativeWorkflowPending {
        request_id,
        action_digest: action_digest.clone(),
        target_node: target.node.clone(),
        before_state_digest: workflow
            .current_state_digest
            .clone()
            .ok_or("native workflow target has no prior state digest")?,
        started_at: None,
        first_sequence: None,
        last_sequence: None,
        event_digests: Vec::new(),
        batch_text: String::new(),
        pointer_up_count: 0,
        last_pointer_up_source_path: None,
        last_pointer_up_dispatched_source_paths: Vec::new(),
        keyboard_phase: 0,
        action_complete: false,
    });
    emit(
        observer,
        ObserverEvent::NativeWorkflowTarget {
            request_id,
            ordinal: ordinal.try_into().unwrap_or(u32::MAX),
            step_id: step.id,
            source_path: step.source_path,
            action_kind,
            action_digest,
            node: target.node,
            x: point.0,
            y: point.1,
            key,
        },
    );
    Ok(())
}

fn native_workflow_request_id(test_request_id: u64, ordinal: usize) -> u64 {
    test_request_id
        .saturating_mul(64)
        .saturating_add(ordinal.try_into().unwrap_or(u64::MAX))
        .max(1)
}

fn render_asset_source(asset: AssetBlob) -> boon_native_gpu::RenderAssetSource {
    boon_native_gpu::RenderAssetSource {
        url: asset.url,
        media_type: asset.media_type,
        sha256: asset.sha256,
        bytes: asset.bytes.into(),
    }
}

fn push_persistence_snapshot(
    output: &PreviewOutput,
    runtime: Option<&RuntimeView>,
    revision: u64,
    snapshot_sequence: &mut u64,
    last_operation: Option<&PersistenceOperationStatus>,
    import_preview: Option<&CachedStateArtifactPreview>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(runtime) = runtime else {
        return Ok(());
    };
    *snapshot_sequence = snapshot_sequence.saturating_add(1);
    output.send(Message::PreviewPersistenceSnapshot(Box::new(
        runtime.cached_persistence_snapshot(
            *snapshot_sequence,
            revision,
            last_operation.cloned(),
            import_preview.map(|preview| preview.summary.clone()),
        ),
    )))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_persistence_command(
    command: PersistenceCommand,
    request_id: u64,
    revision: u64,
    source_revision: u64,
    observer: &Option<ObserverClient>,
    runtime: &mut Option<RuntimeView>,
    import_preview: &mut Option<CachedStateArtifactPreview>,
    next_import_preview_id: &mut u64,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<PersistenceCommandExecution, Box<dyn std::error::Error + Send + Sync>> {
    let operation = match &command {
        PersistenceCommand::Flush => PersistenceOperation::Flush,
        PersistenceCommand::Compact => PersistenceOperation::Compact,
        PersistenceCommand::ClearAll { .. } => PersistenceOperation::ClearAll,
        PersistenceCommand::ExportState => PersistenceOperation::ExportState,
        PersistenceCommand::ImportPreview { .. } => PersistenceOperation::ImportPreview,
        PersistenceCommand::ActivateImport { .. } => PersistenceOperation::ActivateImport,
        PersistenceCommand::ClearSelected { .. } => PersistenceOperation::ClearSelected,
    };
    let result: Result<(String, bool, Option<CanonicalStateArtifact>), String> = async {
        if revision != source_revision {
            return Err(format!(
                "persistence command revision {revision} is stale; preview is at {source_revision}"
            ));
        }
        let runtime = runtime
            .as_mut()
            .ok_or_else(|| "preview runtime is not mounted".to_owned())?;
        match command {
            PersistenceCommand::Flush => {
                let (epoch, turn) = runtime.flush_persistence()?;
                *import_preview = None;
                Ok((
                    format!("Flushed durable state through epoch {epoch}, turn {turn}"),
                    false,
                    None,
                ))
            }
            PersistenceCommand::Compact => {
                let epoch = runtime.compact_persistence()?;
                *import_preview = None;
                Ok((
                    format!("Maintenance completed at durable epoch {epoch}"),
                    false,
                    None,
                ))
            }
            PersistenceCommand::ClearAll { confirmed } => {
                if !confirmed {
                    return Err("Clear All requires explicit confirmation".to_owned());
                }
                host.restart_sensitive_inputs()
                    .map_err(|error| error.to_string())?;
                let change = runtime.start_over()?;
                *import_preview = None;
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    format!(
                        "Cleared all authority and outbox state durably at epoch {}",
                        change.durable_epoch
                    ),
                    true,
                    None,
                ))
            }
            PersistenceCommand::ExportState => {
                let bytes = runtime.export_state_artifact()?;
                *import_preview = None;
                if bytes.len() > MAX_PERSISTENCE_ARTIFACT_BYTES {
                    return Err(format!(
                        "canonical state artifact is {} bytes; IPC limit is {} bytes",
                        bytes.len(),
                        MAX_PERSISTENCE_ARTIFACT_BYTES
                    ));
                }
                let artifact = canonical_state_artifact(runtime.persistence_schema_version(), bytes);
                Ok((
                    format!(
                        "Exported {} bytes of canonical CBOR into the bounded dev cache",
                        artifact.bytes.len()
                    ),
                    false,
                    Some(artifact),
                ))
            }
            PersistenceCommand::ImportPreview { artifact } => {
                validate_state_artifact_digest(&artifact)?;
                let preview = runtime.preview_state_artifact(&artifact.bytes)?;
                *next_import_preview_id = next_import_preview_id.saturating_add(1).max(1);
                let (migration_step_count, deleted_memory_count) =
                    migration_counts(preview.migration.as_ref());
                let status = runtime.persistence_status();
                let summary = StateArtifactPreviewSummary {
                    preview_id: *next_import_preview_id,
                    source_schema_version: preview.source_schema_version,
                    target_schema_version: preview.target_schema_version,
                    scalar_count: preview.scalar_count.try_into().unwrap_or(u32::MAX),
                    list_count: preview.list_count.try_into().unwrap_or(u32::MAX),
                    row_count: preview.row_count.try_into().unwrap_or(u64::MAX),
                    migration_step_count,
                    deleted_memory_count,
                    document_node_count: preview
                        .document_node_count
                        .try_into()
                        .unwrap_or(u32::MAX),
                    baseline_runtime_turn_sequence: runtime.runtime_turn_sequence(),
                    baseline_durable_epoch: status.durable_epoch,
                    baseline_durable_turn_sequence: status.durable_through_turn_sequence,
                };
                *import_preview = Some(CachedStateArtifactPreview {
                    artifact,
                    summary: summary.clone(),
                });
                Ok((
                    format!(
                        "Import Preview #{} settled {} scalar, {} list, {} row records without mutating the active namespace",
                        summary.preview_id,
                        summary.scalar_count,
                        summary.list_count,
                        summary.row_count
                    ),
                    false,
                    None,
                ))
            }
            PersistenceCommand::ActivateImport { preview_id } => {
                let cached = import_preview.as_ref().ok_or_else(|| {
                    "Import activation requires a current successful Import Preview".to_owned()
                })?;
                if cached.summary.preview_id != preview_id {
                    return Err(format!(
                        "Import Preview #{preview_id} is stale; current preview is #{}",
                        cached.summary.preview_id
                    ));
                }
                let persistence = runtime.persistence_status();
                if runtime.runtime_turn_sequence()
                    != cached.summary.baseline_runtime_turn_sequence
                    || persistence.durable_epoch != cached.summary.baseline_durable_epoch
                    || persistence.durable_through_turn_sequence
                        != cached.summary.baseline_durable_turn_sequence
                    || persistence.pending.is_some()
                {
                    return Err(
                        "active authority changed after Import Preview; preview the artifact again"
                            .to_owned(),
                    );
                }
                let bytes = cached.artifact.bytes.clone();
                host.restart_sensitive_inputs()
                    .map_err(|error| error.to_string())?;
                let (_, epoch) = runtime.activate_state_artifact(&bytes)?;
                *import_preview = None;
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    format!("Activated imported state durably at epoch {epoch}"),
                    true,
                    None,
                ))
            }
            PersistenceCommand::ClearSelected {
                selection,
                confirmed,
            } => {
                if !confirmed {
                    return Err("Clear Selected requires explicit confirmation".to_owned());
                }
                if runtime.authority_selection_for_path(&selection.semantic_path)
                    != Some(selection.clone())
                {
                    return Err(
                        "selected authority no longer matches the active MachinePlan".to_owned(),
                    );
                }
                let (epoch, turn) = runtime.clear_authority_path(&selection.semantic_path)?;
                *import_preview = None;
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    format!(
                        "Cleared `{}` authority durably at epoch {epoch}, turn {turn}",
                        selection.semantic_path
                    ),
                    true,
                    None,
                ))
            }
        }
    }
    .await;
    Ok(match result {
        Ok((message, runtime_changed, exported_artifact)) => PersistenceCommandExecution {
            status: PersistenceOperationStatus {
                request_id,
                operation,
                ok: true,
                message,
            },
            runtime_changed,
            exported_artifact,
        },
        Err(message) => PersistenceCommandExecution {
            status: PersistenceOperationStatus {
                request_id,
                operation,
                ok: false,
                message,
            },
            runtime_changed: false,
            exported_artifact: None,
        },
    })
}

struct PersistenceCommandExecution {
    status: PersistenceOperationStatus,
    runtime_changed: bool,
    exported_artifact: Option<CanonicalStateArtifact>,
}

fn canonical_state_artifact(schema_version: u64, bytes: Vec<u8>) -> CanonicalStateArtifact {
    let sha256 = Sha256::digest(&bytes).into();
    CanonicalStateArtifact {
        format: StateArtifactFormat::CanonicalCbor,
        schema_version,
        sha256,
        bytes,
    }
}

fn validate_state_artifact_digest(artifact: &CanonicalStateArtifact) -> Result<(), String> {
    let actual: [u8; 32] = Sha256::digest(&artifact.bytes).into();
    if actual != artifact.sha256 {
        Err("canonical state artifact digest does not match its typed envelope".to_owned())
    } else {
        Ok(())
    }
}

struct MigrationCommandExecution {
    status: MigrationStatus,
    runtime_changed: bool,
}

#[allow(clippy::too_many_arguments)]
async fn execute_migration_command(
    command: MigrationCommand,
    request_id: u64,
    revision: u64,
    source_revision: u64,
    migration: &MigrationBundle,
    active_stage: &mut String,
    previewed_stage: &mut Option<String>,
    observer: &Option<ObserverClient>,
    runtime: &mut Option<RuntimeView>,
    runtime_key: &mut Option<String>,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<MigrationCommandExecution, Box<dyn std::error::Error + Send + Sync>> {
    let target_stage = match &command {
        MigrationCommand::Preview { stage_id } | MigrationCommand::Activate { stage_id } => {
            Some(stage_id.clone())
        }
        MigrationCommand::Restart | MigrationCommand::StartOver { .. } => {
            Some(active_stage.clone())
        }
    };
    if revision != source_revision {
        return Ok(failed_migration_command(
            request_id,
            revision,
            active_stage,
            previewed_stage.as_deref(),
            target_stage.as_deref(),
            format!(
                "migration command revision {revision} is stale; preview is at {source_revision}"
            ),
        ));
    }
    let Some(runtime) = runtime.as_mut() else {
        return Ok(failed_migration_command(
            request_id,
            revision,
            active_stage,
            previewed_stage.as_deref(),
            target_stage.as_deref(),
            "preview runtime is not mounted".to_owned(),
        ));
    };

    let result: Result<(MigrationOperation, u64, u32, u32, String, bool), String> = async {
        match command {
            MigrationCommand::Preview { stage_id } => {
                let stage = forward_migration_stage(migration, active_stage, &stage_id)?;
                let plan =
                    compile_migration_stage(runtime.application_identity(), migration, &stage_id)?;
                let preview = runtime.preview_machine_plan(plan)?;
                let (steps, deletions) = migration_counts(preview.migration.as_ref());
                *previewed_stage = Some(stage_id.clone());
                Ok((
                    MigrationOperation::Previewed,
                    preview.target_schema_version,
                    steps,
                    deletions,
                    format!(
                        "Previewed {} as schema v{}; mounted frame and durable state unchanged",
                        stage.label, preview.target_schema_version
                    ),
                    false,
                ))
            }
            MigrationCommand::Activate { stage_id } => {
                let stage = forward_migration_stage(migration, active_stage, &stage_id)?.clone();
                if previewed_stage.as_deref() != Some(stage_id.as_str()) {
                    return Err(format!(
                        "stage `{stage_id}` must be previewed before activation"
                    ));
                }
                let plan =
                    compile_migration_stage(runtime.application_identity(), migration, &stage_id)?;
                host.restart_sensitive_inputs()
                    .map_err(|error| error.to_string())?;
                let change = runtime.activate_machine_plan(plan)?;
                let (steps, deletions) = migration_counts(change.migration.as_ref());
                *active_stage = stage_id.clone();
                *previewed_stage = None;
                *runtime_key = Some(project_key_for_stage(
                    runtime.application_identity(),
                    &stage.units,
                    Some(&stage_id),
                ));
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    MigrationOperation::Activated,
                    change.target_schema_version,
                    steps,
                    deletions,
                    format!(
                        "Activated {} durably at epoch {}",
                        stage.label, change.durable_epoch
                    ),
                    true,
                ))
            }
            MigrationCommand::Restart => {
                host.restart_sensitive_inputs()
                    .map_err(|error| error.to_string())?;
                let change = runtime.restart()?;
                *previewed_stage = None;
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    MigrationOperation::Restarted,
                    change.target_schema_version,
                    0,
                    0,
                    format!(
                        "Restarted {} from durable turn {}",
                        active_stage, change.through_turn_sequence
                    ),
                    true,
                ))
            }
            MigrationCommand::StartOver { confirmed } => {
                if !confirmed {
                    return Err("Start Over requires explicit confirmation".to_owned());
                }
                host.restart_sensitive_inputs()
                    .map_err(|error| error.to_string())?;
                let change = runtime.start_over()?;
                *previewed_stage = None;
                if let Some(presented) = present_runtime(runtime, view, product, host, columns)
                    .await
                    .map_err(|error| error.to_string())?
                {
                    emit_presented(observer, &presented);
                }
                Ok((
                    MigrationOperation::StartedOver,
                    change.target_schema_version,
                    0,
                    0,
                    format!(
                        "Started over {} durably at epoch {}",
                        active_stage, change.durable_epoch
                    ),
                    true,
                ))
            }
        }
    }
    .await;

    Ok(match result {
        Ok((operation, schema, steps, deletions, message, runtime_changed)) => {
            MigrationCommandExecution {
                status: MigrationStatus {
                    request_id: Some(request_id),
                    revision,
                    operation,
                    ok: true,
                    active_stage: active_stage.clone(),
                    previewed_stage: previewed_stage.clone(),
                    target_stage,
                    target_schema_version: schema,
                    migration_step_count: steps,
                    deleted_memory_count: deletions,
                    message,
                },
                runtime_changed,
            }
        }
        Err(message) => failed_migration_command(
            request_id,
            revision,
            active_stage,
            previewed_stage.as_deref(),
            target_stage.as_deref(),
            message,
        ),
    })
}

fn failed_migration_command(
    request_id: u64,
    revision: u64,
    active_stage: &str,
    previewed_stage: Option<&str>,
    target_stage: Option<&str>,
    message: String,
) -> MigrationCommandExecution {
    MigrationCommandExecution {
        status: MigrationStatus {
            request_id: Some(request_id),
            revision,
            operation: MigrationOperation::Failed,
            ok: false,
            active_stage: active_stage.to_owned(),
            previewed_stage: previewed_stage.map(str::to_owned),
            target_stage: target_stage.map(str::to_owned),
            target_schema_version: 0,
            migration_step_count: 0,
            deleted_memory_count: 0,
            message,
        },
        runtime_changed: false,
    }
}

fn forward_migration_stage<'a>(
    migration: &'a MigrationBundle,
    active_stage: &str,
    target_stage: &str,
) -> Result<&'a crate::protocol::MigrationStage, String> {
    let active_index = migration
        .stages
        .iter()
        .position(|stage| stage.id == active_stage)
        .ok_or_else(|| format!("active migration stage `{active_stage}` is absent"))?;
    let target_index = migration
        .stages
        .iter()
        .position(|stage| stage.id == target_stage)
        .ok_or_else(|| format!("target migration stage `{target_stage}` is absent"))?;
    if target_index <= active_index {
        return Err(format!(
            "migration target `{target_stage}` is not forward from `{active_stage}`"
        ));
    }
    Ok(&migration.stages[target_index])
}

fn migration_counts(preview: Option<&boon_persistence::MigrationPreview>) -> (u32, u32) {
    preview.map_or((0, 0), |preview| {
        (
            preview.steps.len().try_into().unwrap_or(u32::MAX),
            preview.deleted_memory.len().try_into().unwrap_or(u32::MAX),
        )
    })
}

fn run_migration_test(
    migration: &MigrationBundle,
    application: &ApplicationIdentity,
    request_id: u64,
    revision: u64,
) -> Result<usize, String> {
    let sequence = migration.manifest_sequence()?;
    let prefix = format!("test:{}:{request_id}:{revision}:", std::process::id());
    let scenario = temporary_migration_scenario(&migration.scenario, &prefix)?;
    let application = ApplicationIdentity::new(
        application.package_id.clone(),
        format!("{prefix}template"),
        application.deployment_domain.clone(),
    );
    MigrationScenarioRunner::new(sequence, scenario, application)
        .map_err(|error| error.to_string())?
        .run()
        .map(|report| report.steps.len())
        .map_err(|error| error.to_string())
}

fn temporary_migration_scenario(
    scenario: &boon_runtime::MigrationScenario,
    prefix: &str,
) -> Result<boon_runtime::MigrationScenario, String> {
    let encoded = toml::to_string(scenario).map_err(|error| error.to_string())?;
    let mut value = toml::from_str::<toml::Value>(&encoded).map_err(|error| error.to_string())?;
    prefix_namespace_fields(&mut value, prefix);
    value.try_into().map_err(|error| error.to_string())
}

fn prefix_namespace_fields(value: &mut toml::Value, prefix: &str) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                if matches!(key.as_str(), "namespace" | "other_namespace")
                    && let Some(namespace) = value.as_str()
                {
                    *value = toml::Value::String(format!("{prefix}{namespace}"));
                } else {
                    prefix_namespace_fields(value, prefix);
                }
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                prefix_namespace_fields(value, prefix);
            }
        }
        _ => {}
    }
}

fn open_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !["https://", "http://", "mailto:"]
        .into_iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return Err(format!("unsupported external URL scheme: {trimmed}"));
    }
    open::that_detached(trimmed).map_err(|error| error.to_string())
}

enum RuntimeActivation {
    Opened(Box<RuntimeView>),
    Updated,
}

fn activate_executable(
    runtime: &mut Option<RuntimeView>,
    executable: CompiledExecutable,
    deterministic_scenario: bool,
    isolated_scenario: bool,
    assets: &[AssetBlob],
) -> Result<RuntimeActivation, String> {
    match executable {
        CompiledExecutable::BuiltInSingleRole(plan) => activate_compatible_single_role(
            runtime,
            plan,
            deterministic_scenario,
            isolated_scenario,
            assets,
        ),
        CompiledExecutable::DistributedPackage(bundle) => {
            RuntimeView::open_distributed_with_assets(
                bundle,
                deterministic_scenario || isolated_scenario,
                assets,
            )
            .map(|runtime| RuntimeActivation::Opened(Box::new(runtime)))
        }
    }
}

fn activate_compatible_single_role(
    runtime: &mut Option<RuntimeView>,
    plan: Arc<boon_plan::MachinePlan>,
    deterministic_scenario: bool,
    isolated_scenario: bool,
    assets: &[AssetBlob],
) -> Result<RuntimeActivation, String> {
    if !isolated_scenario
        && let Some(runtime) = runtime.as_mut()
        && runtime.application_identity() == &plan.application.identity
    {
        if !runtime.plan_schema_matches(&plan) {
            return Err(
                "same-identity schema change requires Migration Preview and Activate".to_owned(),
            );
        }
        runtime.activate_machine_plan(plan)?;
        return Ok(RuntimeActivation::Updated);
    }
    if isolated_scenario {
        RuntimeView::open_for_scenario_with_assets(plan, assets)
    } else {
        RuntimeView::open_with_assets(plan, deterministic_scenario, assets)
    }
    .map(|runtime| RuntimeActivation::Opened(Box::new(runtime)))
}

#[allow(clippy::too_many_arguments)]
async fn install_runtime(
    next: RuntimeView,
    key: String,
    runtime: &mut Option<RuntimeView>,
    runtime_key: &mut Option<String>,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
    host.restart_sensitive_inputs()?;
    *runtime = Some(next);
    *runtime_key = Some(key);
    present_runtime(
        runtime.as_mut().expect("installed runtime"),
        view,
        product,
        host,
        columns,
    )
    .await
}

async fn present_runtime(
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
    sync_sensitive_input_focus(runtime, host)?;
    view.replace(runtime.frame(), viewport(host), columns)?;
    converge_document_demands(runtime, view, columns)?;
    product.present(host, view).await
}

fn sync_sensitive_input_focus(
    runtime: &RuntimeView,
    host: &mut NativeSurfaceHost,
) -> Result<(), boon_native_app_window::NativeHostError> {
    if let Some((node, binding)) = runtime.focused_sensitive_input() {
        host.focus_sensitive_input(SensitiveInputTarget::new(node, binding))?;
    } else {
        host.clear_sensitive_input_focus()?;
    }
    Ok(())
}

fn emit_switch_final(
    observer: &Option<ObserverClient>,
    switch_started: &mut Option<(u64, Instant)>,
    revision: u64,
    presented: &PresentedFrame,
    compile_us: u64,
    post_compile_us: u64,
) {
    if let Some((pending_revision, started)) = *switch_started
        && pending_revision == revision
    {
        emit(
            observer,
            ObserverEvent::SourceSwitchFinal {
                revision,
                elapsed_us: duration_us(started.elapsed()),
                compile_us,
                post_compile_us,
                key: presented.key.clone(),
            },
        );
        *switch_started = None;
    }
}

async fn show_error(
    observer: &Option<ObserverClient>,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    error: &str,
) -> NativeRoleResult {
    host.clear_sensitive_input_focus()?;
    view.replace(
        role_message_frame("Boon compile error", error, "#fff3f2"),
        viewport(host),
        columns,
    )?;
    if let Some(presented) = product.present(host, view).await? {
        emit_presented(observer, &presented);
    }
    Ok(())
}

struct PreparedProofRequest {
    request: ProofRequest,
    snapshot_prepare_us: u64,
}

fn product_proof_is_eligible(
    benchmark: Option<&ProductProfileBenchmark>,
    workflow: Option<&NativeWorkflowState>,
) -> bool {
    benchmark.is_none_or(|benchmark| matches!(benchmark.phase, ProductProfilePhase::Complete))
        && workflow.is_none_or(|workflow| workflow.complete() && workflow.test_completed_emitted)
}

#[allow(clippy::too_many_arguments)]
fn prepare_product_proof_request(
    benchmark: Option<&ProductProfileBenchmark>,
    workflow: Option<&NativeWorkflowState>,
    proof: Option<&ProofWorker>,
    config: Option<&ProofConfig>,
    requested: &mut bool,
    ordinal: &mut u64,
    presented: &PresentedFrame,
    product: &mut ProductFrame,
) -> Result<Option<PreparedProofRequest>, Box<dyn std::error::Error + Send + Sync>> {
    if !product_proof_is_eligible(benchmark, workflow) {
        return Ok(None);
    }
    *ordinal = ordinal.saturating_add(1);
    prepare_proof_request(proof, config, requested, *ordinal, presented, product)
}

fn prepare_proof_request(
    proof: Option<&ProofWorker>,
    config: Option<&ProofConfig>,
    requested: &mut bool,
    ordinal: u64,
    presented: &PresentedFrame,
    product: &mut ProductFrame,
) -> Result<Option<PreparedProofRequest>, Box<dyn std::error::Error + Send + Sync>> {
    let (Some(_proof), Some(config)) = (proof, config) else {
        return Ok(None);
    };
    if *requested || ordinal < config.sample_ordinal {
        return Ok(None);
    }
    let snapshot_started = Instant::now();
    let readback = product.capture_presented(
        &presented.key,
        format!("preview-frame-{}", presented.key.frame_id),
    )?;
    *requested = true;
    Ok(Some(PreparedProofRequest {
        request: ProofRequest {
            key: presented.key.clone(),
            readback,
            queued_at: Instant::now(),
            queue_depth: 1,
        },
        snapshot_prepare_us: duration_us(snapshot_started.elapsed()),
    }))
}

fn submit_proof_request(
    observer: &Option<ObserverClient>,
    proof: Option<&ProofWorker>,
    prepared: Option<PreparedProofRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (Some(proof), Some(prepared)) = (proof, prepared) else {
        return Ok(());
    };
    let key = prepared.request.key.clone();
    proof.request_latest(prepared.request)?;
    emit(
        observer,
        ObserverEvent::ProofRequested {
            key,
            snapshot_prepare_us: prepared.snapshot_prepare_us,
        },
    );
    Ok(())
}

fn authoritative_state_evidence(
    runtime: &mut RuntimeView,
) -> Result<AuthoritativeStateEvidence, Box<dyn std::error::Error + Send + Sync>> {
    let artifact = runtime.export_state_artifact()?;
    let image = boon_persistence::decode_application_transfer(&artifact, DecodeLimits::default())?
        .restore_image;
    let durable_epoch = image.epoch;
    let durable_turn_sequence = image.through_turn_sequence;
    let semantic = runtime.semantic_value_image()?;
    let canonical = encode_restore_image(&semantic)?;
    Ok(AuthoritativeStateEvidence {
        artifact,
        digest: format!("{:x}", Sha256::digest(canonical)),
        durable_epoch,
        durable_turn_sequence,
    })
}

fn importable_authority(
    artifact: &[u8],
) -> Result<boon_persistence::RestoreImage, boon_persistence::CodecError> {
    let mut image =
        boon_persistence::decode_application_transfer(artifact, DecodeLimits::default())?
            .restore_image;
    image.epoch = 0;
    image.through_turn_sequence = 0;
    image.outbox.clear();
    Ok(image)
}

fn prepare_evidence_proof(
    label: &str,
    key: crate::observer::FrameEvidenceKey,
    product: &mut ProductFrame,
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let readback = product.capture_presented(&key, format!("{label}-frame-{}", key.frame_id))?;
    Ok(PreparedProofRequest {
        request: ProofRequest {
            key,
            readback,
            queued_at: Instant::now(),
            queue_depth: 1,
        },
        snapshot_prepare_us: duration_us(started.elapsed()),
    })
}

fn capture_state_mounted(
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    source_revision: u64,
    presented: &PresentedFrame,
    product: &mut ProductFrame,
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let state = authoritative_state_evidence(runtime)?;
    let startup = runtime.startup_evidence();
    let (disposition, migration) = match &startup.disposition {
        PersistentRuntimeStartupDisposition::Fresh => (StartupDisposition::Fresh, None),
        PersistentRuntimeStartupDisposition::Restored => (StartupDisposition::Restored, None),
        PersistentRuntimeStartupDisposition::Migrated(preview) => (
            StartupDisposition::Migrated,
            Some(StartupMigrationEvidence {
                source_schema_version: preview.source_schema_version,
                source_schema_hash: digest_hex(&preview.source_schema_hash),
                target_schema_version: preview.target_schema_version,
                target_schema_hash: digest_hex(&preview.target_schema_hash),
                step_count: preview.steps.len().try_into().unwrap_or(u32::MAX),
            }),
        ),
    };
    emit(
        observer,
        ObserverEvent::StateMounted {
            disposition,
            schema_version: startup.schema_version,
            schema_hash: digest_hex(&startup.schema_hash),
            migration,
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: state.durable_epoch,
            durable_turn_sequence: state.durable_turn_sequence,
            state_digest: state.digest,
            key: presented.key.clone(),
        },
    );
    prepare_evidence_proof("state-mounted", presented.key.clone(), product)
}

#[allow(clippy::too_many_arguments)]
fn capture_scenario_checkpoint(
    observer: &Option<ObserverClient>,
    request_id: u64,
    step: &TestStep,
    assertion_count: usize,
    source_revision: u64,
    runtime: &mut RuntimeView,
    key: crate::observer::FrameEvidenceKey,
    product: &mut ProductFrame,
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    if assertion_count == 0 {
        return Err(format!(
            "state checkpoint `{}` has no proven semantic assertions",
            step.id
        )
        .into());
    }
    let state = authoritative_state_evidence(runtime)?;
    emit(
        observer,
        ObserverEvent::ScenarioCheckpoint {
            request_id,
            step_id: step.id.clone(),
            assertion_count: assertion_count.try_into().unwrap_or(u32::MAX),
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: state.durable_epoch,
            durable_turn_sequence: state.durable_turn_sequence,
            state_digest: state.digest,
            key: key.clone(),
        },
    );
    prepare_evidence_proof(&format!("checkpoint-{}", step.id), key, product)
}

#[allow(clippy::too_many_arguments)]
async fn run_persistence_evidence(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    baseline_key: crate::observer::FrameEvidenceKey,
) -> Result<Vec<PreparedProofRequest>, Box<dyn std::error::Error + Send + Sync>> {
    let baseline = authoritative_state_evidence(runtime)?;
    let mut proofs = vec![prepare_evidence_proof(
        "persistence-baseline",
        baseline_key.clone(),
        product,
    )?];
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::Exported,
        source_revision,
        runtime,
        &baseline,
        &baseline,
        baseline_key.clone(),
    );

    let mut corrupt_envelope = canonical_state_artifact(
        runtime.persistence_schema_version(),
        baseline.artifact.clone(),
    );
    let byte = corrupt_envelope
        .bytes
        .last_mut()
        .ok_or("canonical persistence artifact is unexpectedly empty")?;
    *byte ^= 0x5a;
    if validate_state_artifact_digest(&corrupt_envelope).is_ok() {
        return Err("corrupt canonical state artifact envelope was accepted".into());
    }

    let mut malformed = baseline.artifact.clone();
    malformed.push(0);
    if runtime.preview_state_artifact(&malformed).is_ok() {
        return Err("malformed canonical state artifact was accepted".into());
    }
    let after_corruption = authoritative_state_evidence(runtime)?;
    if after_corruption.digest != baseline.digest {
        return Err("corrupt import preview changed authoritative state".into());
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::CorruptionRejected,
        source_revision,
        runtime,
        &baseline,
        &after_corruption,
        baseline_key,
    );

    runtime.start_over()?;
    view.replace(runtime.frame(), viewport(host), columns)?;
    settle_test_runtime(runtime, view, columns)?;
    let cleared_frame = present_runtime(runtime, view, product, host, columns)
        .await?
        .ok_or("start-over state did not present")?;
    emit_presented(observer, &cleared_frame);
    let cleared = authoritative_state_evidence(runtime)?;
    if cleared.digest == baseline.digest {
        return Err("start-over did not change authoritative state".into());
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::ClearedAndStartedOver,
        source_revision,
        runtime,
        &baseline,
        &cleared,
        cleared_frame.key.clone(),
    );
    proofs.push(prepare_evidence_proof(
        "persistence-cleared",
        cleared_frame.key.clone(),
        product,
    )?);

    runtime.preview_state_artifact(&baseline.artifact)?;
    let after_preview = authoritative_state_evidence(runtime)?;
    if after_preview.digest != cleared.digest {
        return Err("import preview mutated authoritative state before activation".into());
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::ImportPreviewed,
        source_revision,
        runtime,
        &cleared,
        &after_preview,
        cleared_frame.key,
    );

    runtime.activate_state_artifact(&baseline.artifact)?;
    view.replace(runtime.frame(), viewport(host), columns)?;
    settle_test_runtime(runtime, view, columns)?;
    let activated_frame = present_runtime(runtime, view, product, host, columns)
        .await?
        .ok_or("activated import state did not present")?;
    emit_presented(observer, &activated_frame);
    let activated = authoritative_state_evidence(runtime)?;
    if importable_authority(&activated.artifact)? != importable_authority(&baseline.artifact)? {
        return Err("activated import does not match the exported importable authority".into());
    }
    let cleared_image =
        boon_persistence::decode_application_transfer(&cleared.artifact, DecodeLimits::default())?
            .restore_image;
    let activated_image = boon_persistence::decode_application_transfer(
        &activated.artifact,
        DecodeLimits::default(),
    )?
    .restore_image;
    if activated_image.outbox != cleared_image.outbox {
        return Err("activated import replaced destination effect history".into());
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::ImportActivated,
        source_revision,
        runtime,
        &cleared,
        &activated,
        activated_frame.key.clone(),
    );
    proofs.push(prepare_evidence_proof(
        "persistence-imported",
        activated_frame.key,
        product,
    )?);
    Ok(proofs)
}

#[allow(clippy::too_many_arguments)]
async fn run_schema_migration_evidence(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    migration: &MigrationBundle,
) -> Result<Vec<PreparedProofRequest>, Box<dyn std::error::Error + Send + Sync>> {
    let product_before = authoritative_state_evidence(runtime)?;
    let product_frame_before = product
        .last_presented_key()
        .cloned()
        .ok_or("migration evidence has no mounted product frame")?;
    let target_stage = if migration.launch_stage != migration.initial_stage {
        migration.launch_stage.as_str()
    } else {
        migration
            .stages
            .last()
            .map(|stage| stage.id.as_str())
            .ok_or("migration evidence has no stages")?
    };
    if target_stage == migration.initial_stage {
        return Err("migration evidence requires distinct initial and target stages".into());
    }

    let current = runtime.shared_machine_plan();
    let mut evidence_application = current.application.identity.clone();
    evidence_application.state_namespace = format!(
        "{}:migration-evidence",
        evidence_application.state_namespace
    );
    let completed = run_migration_test(
        migration,
        &evidence_application,
        source_revision,
        source_revision,
    )?;
    if completed != migration.scenario.steps.len() {
        return Err(format!(
            "migration scenario completed {completed} of {} source-controlled steps",
            migration.scenario.steps.len()
        )
        .into());
    }

    let state_root = std::env::var_os(STATE_ROOT_ENV)
        .ok_or("migration evidence requires a launch-scoped playground state root")?;
    let evidence_root = std::path::PathBuf::from(state_root)
        .join(format!("migration-evidence-{}", std::process::id()));
    let initial =
        compile_migration_stage(&evidence_application, migration, &migration.initial_stage)?;
    let target = compile_migration_stage(&evidence_application, migration, target_stage)?;
    let target_schema_version = target.persistence.schema_version;
    let mut evidence_runtime =
        RuntimeView::open_with_state_root_deterministic(initial, &evidence_root)?;

    let baseline = authoritative_state_evidence(&mut evidence_runtime)?;
    let preview = evidence_runtime.preview_machine_plan(Arc::clone(&target))?;
    let preview_migration = preview
        .migration
        .as_ref()
        .ok_or("schema migration preview did not produce a migration")?;
    if preview_migration.source_schema_version
        != migration
            .initial()
            .ok_or("migration initial stage is absent")?
            .schema_version
        || preview.target_schema_version != target_schema_version
        || preview_migration.steps.is_empty()
    {
        return Err(
            "schema migration preview has incomplete source, target, or step evidence".into(),
        );
    }

    let activation = evidence_runtime.activate_machine_plan(target)?;
    let activated_migration = activation
        .migration
        .as_ref()
        .ok_or("schema migration activation did not apply a migration")?;
    if activation.target_schema_version != target_schema_version
        || activated_migration.steps.is_empty()
    {
        return Err("schema migration activation has incomplete version or step evidence".into());
    }
    let mut evidence_view = RetainedView::new(evidence_runtime.frame(), viewport(host), columns)?;
    settle_test_runtime(&mut evidence_runtime, &mut evidence_view, columns)?;
    let presented = present_runtime(
        &mut evidence_runtime,
        &mut evidence_view,
        product,
        host,
        columns,
    )
    .await?
    .ok_or("schema migration activation did not present")?;
    emit_presented(observer, &presented);
    let activated = authoritative_state_evidence(&mut evidence_runtime)?;
    if activated.digest == baseline.digest
        || activated.durable_epoch < activation.durable_epoch
        || activated.durable_turn_sequence < activation.through_turn_sequence
    {
        return Err(
            "schema migration activation did not durably change authoritative state".into(),
        );
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::MigrationActivated,
        source_revision,
        &evidence_runtime,
        &baseline,
        &activated,
        presented.key.clone(),
    );
    let migration_proof = prepare_evidence_proof("persistence-migrated", presented.key, product)?;
    let restored = product
        .present(host, view)
        .await?
        .ok_or("schema migration evidence did not restore the product frame")?;
    emit_presented(observer, &restored);
    let product_after = authoritative_state_evidence(runtime)?;
    if product_after.digest != product_before.digest
        || product_after.durable_epoch != product_before.durable_epoch
        || product_after.durable_turn_sequence != product_before.durable_turn_sequence
    {
        return Err("schema migration evidence changed the mounted product authority".into());
    }
    if !product_frame_before.same_producer_surface(&restored.key)
        || restored.key.content_id != product_frame_before.content_id
        || restored.key.layout_id != product_frame_before.layout_id
        || restored.key.render_id != product_frame_before.render_id
        || restored.key.frame_id <= product_frame_before.frame_id
        || restored.key.present_id <= product_frame_before.present_id
    {
        return Err(
            "schema migration evidence did not restore the mounted product revisions".into(),
        );
    }
    emit_persistence_evidence(
        observer,
        PersistenceEvidenceKind::MigrationProductRestored,
        source_revision,
        runtime,
        &product_before,
        &product_after,
        restored.key.clone(),
    );
    let restored_proof = prepare_evidence_proof(
        "persistence-migration-product-restored",
        restored.key,
        product,
    )?;
    Ok(vec![migration_proof, restored_proof])
}

fn emit_persistence_evidence(
    observer: &Option<ObserverClient>,
    kind: PersistenceEvidenceKind,
    source_revision: u64,
    runtime: &RuntimeView,
    before: &AuthoritativeStateEvidence,
    after: &AuthoritativeStateEvidence,
    key: crate::observer::FrameEvidenceKey,
) {
    emit(
        observer,
        ObserverEvent::PersistenceEvidence {
            kind,
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: after.durable_epoch,
            durable_turn_sequence: after.durable_turn_sequence,
            before_state_digest: before.digest.clone(),
            after_state_digest: after.digest.clone(),
            key,
        },
    );
}

fn arm_responsive_evidence(
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    view: &RetainedView,
    host: &NativeSurfaceHost,
    desired_width: u32,
    navigation_sources: &[String],
    key: crate::observer::FrameEvidenceKey,
) -> Result<ResponsiveEvidenceState, Box<dyn std::error::Error + Send + Sync>> {
    let current = host.viewport().logical_size;
    let current_width = current.width.round().max(0.0) as u32;
    let current_height = current.height.round().max(0.0) as u32;
    if !(320..=2_160).contains(&current_height) {
        return Err("responsive evidence has an unsupported tiled height".into());
    }
    let expected_actions = boon_document::source_actions::SourceActionCoverage::collect(
        view.visible_source_action_bounds(),
        current_width,
        current_height,
    )?;
    let baseline_state_digest = authoritative_state_evidence(runtime)?.digest;
    let baseline_action_count = expected_actions.total();
    let baseline_action_digest = expected_actions.digest();
    emit(
        observer,
        ObserverEvent::ResponsiveResizeReady {
            desired_width,
            desired_height: current_height,
            current_width,
            current_height,
            baseline_action_count,
            baseline_action_digest: baseline_action_digest.clone(),
            key: key.clone(),
        },
    );
    Ok(ResponsiveEvidenceState {
        desired_width,
        desired_height: current_height,
        baseline_key: key.clone(),
        baseline_state_digest,
        expected_actions,
        observed_actions: Default::default(),
        navigation_sources: navigation_sources.to_vec(),
        navigation_index: 0,
        pending_navigation: None,
        baseline_action_count,
        baseline_action_digest,
        resize_sequence: None,
        last_surface_epoch: key.surface_epoch,
        resize_started: false,
        complete: false,
    })
}

fn advance_responsive_layout_evidence(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &RetainedView,
    product: &mut ProductFrame,
    evidence: &mut ResponsiveEvidenceState,
    key: crate::observer::FrameEvidenceKey,
) -> Result<Option<PreparedProofRequest>, Box<dyn std::error::Error + Send + Sync>> {
    let observed_actions = boon_document::source_actions::SourceActionCoverage::collect(
        view.visible_source_action_bounds(),
        evidence.desired_width,
        evidence.desired_height,
    )?;
    for (action, count) in observed_actions.counts() {
        if let Some(expected) = evidence.expected_actions.counts().get(action) {
            if count > expected {
                return Err(format!("narrow layout duplicates public action {action:?}").into());
            }
        } else if !evidence.navigation_sources.contains(&action.source_path) {
            return Err(format!("undeclared narrow-only action {action:?}").into());
        }
    }
    evidence.observed_actions.merge_max(&observed_actions);
    let proof = prepare_evidence_proof("responsive-narrow-visit", key.clone(), product)?;
    if let Some(source) = evidence.navigation_sources.get(evidence.navigation_index) {
        let action = observed_actions
            .counts()
            .keys()
            .find(|action| action.source_path == *source && action.intent == "press")
            .ok_or_else(|| format!("responsive navigation source `{source}` is not visible"))?;
        let target = view
            .target_for_source(&action.source_path, None)
            .ok_or_else(|| format!("responsive navigation source `{source}` has no hit target"))?;
        evidence.pending_navigation = Some((source.clone(), target.node.clone()));
        emit(
            observer,
            ObserverEvent::RoleTarget {
                role: ObserverRole::Preview,
                node: target.node,
                x: target.center_x,
                y: target.center_y,
            },
        );
        return Ok(Some(proof));
    }
    runtime.flush_persistence()?;
    let state = authoritative_state_evidence(runtime)?;
    let action_mismatches = evidence
        .expected_actions
        .mismatches(&evidence.observed_actions);
    if !action_mismatches.is_empty() || state.digest != evidence.baseline_state_digest {
        return Err(format!(
            "responsive traversal did not restore equivalent actions and semantic values: {action_mismatches:?}"
        ).into());
    }
    let equivalent_actions = evidence
        .observed_actions
        .restricted_to(&evidence.expected_actions);
    emit(
        observer,
        ObserverEvent::ResponsiveLayoutEvidence {
            resize_sequence: evidence
                .resize_sequence
                .ok_or("responsive resize was not observed")?,
            logical_width: evidence.desired_width,
            logical_height: evidence.desired_height,
            baseline_key: evidence.baseline_key.clone(),
            baseline_action_count: evidence.baseline_action_count,
            baseline_action_digest: evidence.baseline_action_digest.clone(),
            action_count: equivalent_actions.total(),
            action_digest: equivalent_actions.digest(),
            state_digest: state.digest,
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: state.durable_epoch,
            durable_turn_sequence: state.durable_turn_sequence,
            key: key.clone(),
        },
    );
    evidence.complete = true;
    Ok(Some(proof))
}

fn observe_responsive_navigation(
    state: &mut ResponsiveEvidenceState,
    envelope: &HostEventEnvelope,
    target_node: Option<&str>,
    target_source: Option<&str>,
    dispatches: &[RuntimeSourceDispatch],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if envelope.origin == HostEventOrigin::RealOs
        && let Some((source, node)) = state.pending_navigation.as_ref()
        && target_node == Some(node)
        && target_source == Some(source)
        && matches!(
            &envelope.event,
            HostEvent::Pointer(PointerEvent {
                phase: PointerPhase::Up,
                button: Some(PointerButton::Primary),
                ..
            })
        )
    {
        if !dispatches
            .iter()
            .any(|dispatch| dispatch.source_path == *source)
        {
            return Err(format!("responsive navigation `{source}` did not dispatch").into());
        }
        state.navigation_index += 1;
        state.pending_navigation = None;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_stale_program_evidence(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    steps: &[TestStep],
    profile_steps: &[String],
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let edits = profile_steps
        .iter()
        .map(|id| {
            steps
                .iter()
                .find(|step| step.id == *id)
                .ok_or_else(|| format!("stale-program evidence step `{id}` is absent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if edits.len() != 2
        || edits[0].source_path != edits[1].source_path
        || edits.iter().any(|step| {
            step.action_kind.as_deref() != Some("type_text")
                || step.text.as_ref().is_none_or(|text| text.is_empty())
        })
    {
        return Err("stale-program evidence requires two valid text edits for one source".into());
    }

    let baseline = runtime.export_state_artifact()?;
    let target_row = runtime.scenario_target_row(
        &edits[0].source_path,
        edits[0].target_text.as_deref(),
        edits[0].address.as_deref(),
        edits[0].target_occurrence,
    )?;
    let target = view
        .target_for_scenario(
            &edits[0].source_path,
            edits[0].action_kind.as_deref(),
            edits[0].target_text.as_deref(),
            edits[0].address.as_deref(),
            target_row,
        )
        .ok_or("stale-program evidence could not resolve its public text input")?;
    let point = test_step_pointer_position(view, &target, edits[0]);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        if runtime.handle_event(
            &HostEvent::Pointer(PointerEvent {
                surface: host.ids().surface.clone(),
                x: point.0,
                y: point.1,
                phase,
                button: Some(PointerButton::Primary),
            }),
            Some(target.clone()),
        )? {
            apply_runtime_update(runtime, view, columns)?;
        }
    }
    if runtime.focused() != Some(target.node.as_str()) {
        return Err("stale-program evidence text input did not receive focus".into());
    }

    let mut requested = Vec::with_capacity(2);
    for step in &edits {
        for (logical_key, pressed) in [
            (LogicalKey::Named("Control_L".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), true),
            (LogicalKey::Character("a".to_owned()), false),
            (LogicalKey::Named("Control_L".to_owned()), false),
        ] {
            runtime.handle_event(
                &HostEvent::Keyboard(KeyEvent {
                    surface: host.ids().surface.clone(),
                    physical_key: None,
                    logical_key,
                    pressed,
                }),
                None,
            )?;
        }
        let changed = runtime.handle_event(
            &HostEvent::TextInput(TextInputEvent {
                surface: host.ids().surface.clone(),
                text: step.text.clone().expect("validated stale evidence text"),
            }),
            None,
        )?;
        if !changed {
            return Err(format!(
                "stale-program edit `{}` did not change the runtime",
                step.id
            )
            .into());
        }
        apply_runtime_update(runtime, view, columns)?;
        let requests = runtime.take_program_requests();
        if requests.is_empty() {
            return Err(format!(
                "stale-program edit `{}` produced no compile request",
                step.id
            )
            .into());
        }
        requested.push(requests);
    }

    let mut pairs = requested[0]
        .iter()
        .filter_map(|older| {
            requested[1]
                .iter()
                .find(|latest| {
                    latest.session == older.session
                        && latest.compile.revision > older.compile.revision
                })
                .map(|latest| (older, latest))
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.0.session.cmp(&right.0.session));
    let Some((older, latest)) = pairs.first().copied() else {
        return Err("two text edits produced no common increasing child-program session".into());
    };
    let observed = runtime.complete_program_observed(
        &older.session,
        &older.request_id,
        compile_program_artifact(&older.compile),
    )?;
    if observed.changed {
        return Err("stale child completion changed the retained product document".into());
    }
    let (session, stale_revision, latest_revision) = match observed.completion {
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Stale {
                revision,
                latest_requested_revision,
            },
        )) => (older.session.0.clone(), revision, latest_requested_revision),
        ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded {
            session, ..
        }) => (session.0, older.compile.revision, latest.compile.revision),
        completion => {
            return Err(format!(
                "older child completion was not rejected as stale: {completion:?}"
            )
            .into());
        }
    };

    for request in &requested[1] {
        runtime.complete_program(
            &request.session,
            &request.request_id,
            compile_program_artifact(&request.compile),
        )?;
    }
    settle_test_runtime(runtime, view, columns)?;
    let frame = product
        .present(host, view)
        .await?
        .ok_or("stale-program rejection did not present its latest child frame")?;
    emit_presented(observer, &frame);
    let state = authoritative_state_evidence(runtime)?;
    emit(
        observer,
        ObserverEvent::StaleProgramRejected {
            session,
            stale_revision,
            latest_revision,
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: state.durable_epoch,
            durable_turn_sequence: state.durable_turn_sequence,
            state_digest: state.digest,
            key: frame.key.clone(),
        },
    );
    let proof = prepare_evidence_proof("stale-program-rejected", frame.key, product)?;

    runtime.activate_state_artifact(&baseline)?;
    settle_test_runtime(runtime, view, columns)?;
    present_runtime(runtime, view, product, host, columns).await?;
    Ok(proof)
}

fn queue_evidence_proofs(
    observer: &Option<ObserverClient>,
    proof: Option<&ProofWorker>,
    queued: &mut VecDeque<PreparedProofRequest>,
    in_flight: &mut Option<crate::observer::FrameEvidenceKey>,
    requests: impl IntoIterator<Item = PreparedProofRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let requests = requests.into_iter().collect::<Vec<_>>();
    let current_depth = queued.len() + usize::from(in_flight.is_some());
    if current_depth.saturating_add(requests.len()) > MAX_PENDING_EVIDENCE_PROOFS {
        return Err(format!(
            "evidence proof backlog would exceed {MAX_PENDING_EVIDENCE_PROOFS} production snapshots"
        )
        .into());
    }
    for (offset, mut request) in requests.into_iter().enumerate() {
        request.request.queue_depth = current_depth
            .saturating_add(offset)
            .saturating_add(1)
            .try_into()
            .unwrap_or(u32::MAX);
        queued.push_back(request);
    }
    if in_flight.is_some() {
        return Ok(());
    }
    let Some(next) = queued.pop_front() else {
        return Ok(());
    };
    *in_flight = Some(next.request.key.clone());
    submit_proof_request(observer, proof, Some(next))
}

fn first_test_target(
    runtime: &RuntimeView,
    view: &RetainedView,
    steps: &[TestStep],
) -> Option<HitTarget> {
    steps
        .iter()
        .filter(|step| !step.source_path.is_empty())
        .find_map(|step| {
            let target_row = runtime
                .scenario_target_row(
                    &step.source_path,
                    step.target_text.as_deref(),
                    step.address.as_deref(),
                    step.target_occurrence,
                )
                .ok()
                .flatten();
            view.target_for_scenario(
                &step.source_path,
                step.action_kind.as_deref(),
                step.target_text.as_deref(),
                step.address.as_deref(),
                target_row,
            )
        })
}

#[allow(clippy::too_many_arguments)]
async fn run_test(
    observer: &Option<ObserverClient>,
    output: &PreviewOutput,
    request_id: u64,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    steps: &[TestStep],
    cursor: &mut (f32, f32),
    state_evidence: &StateEvidenceConfig,
) -> Result<TestRunOutcome, Box<dyn std::error::Error + Send + Sync>> {
    if steps.is_empty() {
        return Err("example scenario has no source-event steps".into());
    }
    let surface = host.ids().surface.clone();
    let mut completed = 0usize;
    let mut semantic_expectation_count = 0usize;
    let mut proof_requests = Vec::new();
    let mut last_state_key = None;
    for (step_index, step) in steps.iter().take(TEST_STEP_LIMIT).enumerate() {
        if step.source_path.is_empty() && step.action_kind.is_none() {
            settle_test_runtime(runtime, view, columns)?;
            let assertion_count = assert_test_step_semantics(runtime, step)?;
            semantic_expectation_count = semantic_expectation_count.saturating_add(assertion_count);
            let key = present_test_cursor_frame(
                observer,
                request_id,
                step_index,
                TestPointerPhase::State,
                None,
                runtime.event_sequence(),
                product,
                host,
                view,
                *cursor,
                1,
            )
            .await?;
            last_state_key = Some(key.clone());
            if state_evidence.scenario_steps.contains(&step.id) {
                proof_requests.push(capture_scenario_checkpoint(
                    observer,
                    request_id,
                    step,
                    assertion_count,
                    source_revision,
                    runtime,
                    key,
                    product,
                )?);
            }
            completed += 1;
            continue;
        }
        runtime.begin_scenario_step(&step.source_path);
        let target_row = runtime.scenario_target_row(
            &step.source_path,
            step.target_text.as_deref(),
            step.address.as_deref(),
            step.target_occurrence,
        )?;
        let target = view
            .target_for_scenario(
                &step.source_path,
                step.action_kind.as_deref(),
                step.target_text.as_deref(),
                step.address.as_deref(),
                target_row,
            )
            .ok_or_else(|| {
                format!(
                    "TEST could not resolve visible source `{}` target {:?}",
                    step.source_path, step.target_text
                )
            })?;
        let target_point = test_step_pointer_position(view, &target, step);
        for next in test_cursor_path(*cursor, target_point) {
            *cursor = next;
            let hover_target = view.hit_target(cursor.0, cursor.1);
            let changed = runtime.handle_event(
                &HostEvent::Pointer(PointerEvent {
                    surface: surface.clone(),
                    x: cursor.0,
                    y: cursor.1,
                    phase: PointerPhase::Move,
                    button: None,
                }),
                hover_target.clone(),
            )?;
            if changed {
                apply_runtime_update(runtime, view, columns)?;
            }
            present_test_cursor_frame(
                observer,
                request_id,
                step_index,
                TestPointerPhase::Move,
                hover_target.as_ref().map(|target| target.node.as_str()),
                runtime.event_sequence(),
                product,
                host,
                view,
                *cursor,
                1,
            )
            .await?;
        }
        let final_target = view
            .hit_target(target_point.0, target_point.1)
            .ok_or_else(|| format!("TEST cursor ended outside target `{}`", target.node))?;
        let same_source = target
            .source_path
            .as_ref()
            .is_some_and(|source| final_target.source_path.as_ref() == Some(source));
        let same_target = final_target.node == target.node || same_source;
        if !same_target {
            return Err(format!(
                "TEST cursor resolved `{}` instead of `{}` at ({:.1}, {:.1})",
                final_target.node, target.node, target_point.0, target_point.1
            )
            .into());
        }
        if runtime.hovered() != Some(final_target.node.as_str()) {
            return Err(format!(
                "TEST cursor reached `{}` without entering its hover state",
                final_target.node
            )
            .into());
        }
        present_test_cursor_frame(
            observer,
            request_id,
            step_index,
            TestPointerPhase::Hover,
            Some(&final_target.node),
            runtime.event_sequence(),
            product,
            host,
            view,
            *cursor,
            1,
        )
        .await?;

        let pointer_cycles = usize::from(
            step.action_kind.as_deref() == Some("double_click")
                || target.source_intent.as_deref() == Some("double_click"),
        ) + 1;
        let mut declared_source_dispatched = false;
        for _ in 0..pointer_cycles {
            for (phase, playback_phase, dwell_frames) in [
                (PointerPhase::Down, TestPointerPhase::Down, 1),
                (PointerPhase::Up, TestPointerPhase::Up, 1),
            ] {
                let event = HostEvent::Pointer(PointerEvent {
                    surface: surface.clone(),
                    x: target_point.0,
                    y: target_point.1,
                    phase,
                    button: Some(PointerButton::Primary),
                });
                let outcome = runtime.handle_event_observed(&event, Some(final_target.clone()))?;
                let changed = outcome.changed;
                declared_source_dispatched |= outcome.dispatched(&step.source_path);
                sync_sensitive_input_focus(runtime, host)?;
                if changed {
                    apply_runtime_update(runtime, view, columns)?;
                }
                if phase == PointerPhase::Down
                    && runtime.focused() != Some(final_target.node.as_str())
                {
                    return Err(format!("TEST `{}` lost focus", step.id).into());
                }
                present_test_cursor_frame(
                    observer,
                    request_id,
                    step_index,
                    playback_phase,
                    Some(&final_target.node),
                    runtime.event_sequence(),
                    product,
                    host,
                    view,
                    *cursor,
                    dwell_frames,
                )
                .await?;
            }
        }
        let mut dirty = false;
        if let Some(text) = &step.text {
            for (logical_key, pressed) in [
                (LogicalKey::Named("Control_L".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), true),
                (LogicalKey::Character("a".to_owned()), false),
                (LogicalKey::Named("Control_L".to_owned()), false),
            ] {
                dirty |= runtime.handle_event(
                    &HostEvent::Keyboard(KeyEvent {
                        surface: surface.clone(),
                        physical_key: None,
                        logical_key,
                        pressed,
                    }),
                    None,
                )?;
            }
            let event = HostEvent::TextInput(TextInputEvent {
                surface: surface.clone(),
                text: text.clone(),
            });
            let outcome = runtime.handle_event_observed(&event, None)?;
            dirty |= outcome.changed;
            declared_source_dispatched |= outcome.dispatched(&step.source_path);
        }
        if let Some(key) = &step.key {
            let event = HostEvent::Keyboard(KeyEvent {
                surface: surface.clone(),
                physical_key: None,
                logical_key: LogicalKey::Named(key.clone()),
                pressed: true,
            });
            let outcome = runtime.handle_event_observed(&event, None)?;
            dirty |= outcome.changed;
            declared_source_dispatched |= outcome.dispatched(&step.source_path);
        }
        if step.action_kind.as_deref() == Some("blur")
            || target.source_intent.as_deref() == Some("blur")
        {
            let outcome = runtime.handle_event_observed(
                &HostEvent::Focus {
                    surface: surface.clone(),
                    focused: false,
                },
                None,
            )?;
            dirty |= outcome.changed;
            declared_source_dispatched |= outcome.dispatched(&step.source_path);
            sync_sensitive_input_focus(runtime, host)?;
        }
        if !declared_source_dispatched {
            return Err(format!(
                "TEST step `{}` host events did not dispatch declared source `{}` (intent {:?}, focused {:?}, target `{}`) in their event-local outcomes",
                step.id, step.source_path, target.source_intent, runtime.focused(), final_target.node,
            )
            .into());
        }
        if dirty {
            apply_runtime_update(runtime, view, columns)?;
        }
        settle_test_runtime(runtime, view, columns)?;
        let assertion_count = assert_test_step_semantics(runtime, step)?;
        semantic_expectation_count = semantic_expectation_count.saturating_add(assertion_count);
        let key = present_test_cursor_frame(
            observer,
            request_id,
            step_index,
            TestPointerPhase::State,
            Some(&final_target.node),
            runtime.event_sequence(),
            product,
            host,
            view,
            *cursor,
            1,
        )
        .await?;
        last_state_key = Some(key.clone());
        if state_evidence.scenario_steps.contains(&step.id) {
            proof_requests.push(capture_scenario_checkpoint(
                observer,
                request_id,
                step,
                assertion_count,
                source_revision,
                runtime,
                key,
                product,
            )?);
        }
        output.send(Message::PreviewRuntimeChanged {
            revision: source_revision,
            runtime_sequence: runtime.event_sequence(),
        })?;
        completed += 1;
    }
    if state_evidence.persistence_exercise {
        let key = last_state_key
            .clone()
            .ok_or("persistence evidence requires a presented scenario state")?;
        proof_requests.extend(
            run_persistence_evidence(
                observer,
                source_revision,
                runtime,
                view,
                product,
                host,
                columns,
                key,
            )
            .await?,
        );
    }
    if state_evidence.stale_program {
        proof_requests.push(
            run_stale_program_evidence(
                observer,
                source_revision,
                runtime,
                view,
                product,
                host,
                columns,
                steps,
                &state_evidence.profile_steps,
            )
            .await?,
        );
    }
    let last_key = last_state_key.ok_or("TEST completed without a presented state frame")?;
    if std::env::var_os(PRODUCT_PROOF_AFTER_TEST_ENV).is_some()
        && !proof_requests
            .iter()
            .any(|proof| proof.request.key == last_key)
    {
        proof_requests.push(prepare_evidence_proof(
            "test-state",
            last_key.clone(),
            product,
        )?);
    }
    Ok(TestRunOutcome {
        completed_steps: completed,
        semantic_assertions_proven: semantic_expectation_count > 0,
        proof_requests,
        last_key,
    })
}

fn arm_product_profile_benchmark(
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    view: &RetainedView,
    steps: &[TestStep],
    sample_count: usize,
    profile_steps: &[String],
    key: crate::observer::FrameEvidenceKey,
) -> Result<ProductProfileBenchmark, Box<dyn std::error::Error + Send + Sync>> {
    let edits = profile_steps
        .iter()
        .map(|id| {
            steps
                .iter()
                .find(|step| step.id == *id)
                .ok_or_else(|| format!("profile benchmark step `{id}` is absent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if edits.len() != 2 || edits[0].source_path != edits[1].source_path {
        return Err(
            "profile benchmark requires two valid text edits for the same public source path"
                .into(),
        );
    }
    if edits.iter().any(|step| {
        step.action_kind.as_deref() != Some("type_text")
            || step.text.as_ref().is_none_or(|text| {
                text.is_empty()
                    || text.len() > ASCII_TEXT_BATCH_MAX_BYTES
                    || !text.bytes().all(|byte| (b' '..=b'~').contains(&byte))
            })
    }) {
        return Err(
            "profile benchmark steps must be bounded printable-ASCII public text edits".into(),
        );
    }
    let baseline = runtime.export_state_artifact()?;
    let target_row = runtime.scenario_target_row(
        &edits[0].source_path,
        edits[0].target_text.as_deref(),
        edits[0].address.as_deref(),
        edits[0].target_occurrence,
    )?;
    let target = view
        .target_for_scenario(
            &edits[0].source_path,
            edits[0].action_kind.as_deref(),
            edits[0].target_text.as_deref(),
            edits[0].address.as_deref(),
            target_row,
        )
        .ok_or("profile benchmark could not resolve its public text-input target")?;
    let point = test_step_pointer_position(view, &target, edits[0]);
    emit(
        observer,
        ObserverEvent::ProfileInputTarget {
            node: target.node.clone(),
            source_path: edits[0].source_path.clone(),
            x: point.0,
            y: point.1,
            sample_count: sample_count.try_into().unwrap_or(u32::MAX),
            key,
        },
    );
    Ok(ProductProfileBenchmark {
        baseline,
        source_path: edits[0].source_path.clone(),
        target_node: target.node,
        seed_text: edits[0].text.clone().expect("validated profile seed"),
        sample_count: sample_count.try_into().unwrap_or(u32::MAX),
        completed_samples: 0,
        phase: ProductProfilePhase::Seed,
        candidate: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn record_profile_text_input(
    benchmark: &mut ProductProfileBenchmark,
    envelope: &HostEventEnvelope,
    accepted_at: Instant,
    text: &TextInputEvent,
    runtime: &RuntimeView,
    parent_generation_before: u64,
    parent_dispatch_us: u64,
    parent_phase: RuntimePhaseTimings,
    update: RuntimeUpdateMeasurement,
    requests: Vec<SubmittedProgramRequest>,
    dispatches: &[RuntimeSourceDispatch],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(benchmark.phase, ProductProfilePhase::Complete) {
        return Ok(());
    }
    if envelope.origin != HostEventOrigin::RealOs
        || runtime.focused() != Some(benchmark.target_node.as_str())
        || !dispatches
            .iter()
            .any(|dispatch| dispatch.source_path == benchmark.source_path)
    {
        return Err("profile text did not arrive through the focused real native source".into());
    }
    if text.text.is_empty() || !text.text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        return Err("profile text callback was not bounded printable ASCII".into());
    }
    if requests.is_empty() {
        return Err("profile text callback produced no child-program worker request".into());
    }
    let mut batch_text = match benchmark.candidate.take() {
        Some(candidate) if !candidate.closed => candidate.batch_text,
        Some(_) => return Err("profile input batch overlapped an unfinished compilation".into()),
        None => String::new(),
    };
    batch_text.push_str(&text.text);
    let expected_bytes = match benchmark.phase {
        ProductProfilePhase::Seed => benchmark.seed_text.len(),
        ProductProfilePhase::Samples => PROFILE_SAMPLE_TEXT.len(),
        ProductProfilePhase::Complete => 0,
    };
    if batch_text.len() > expected_bytes {
        return Err("profile input batch exceeded its declared bounded text".into());
    }
    let pending_depth = requests
        .iter()
        .map(|request| request.pending_depth)
        .max()
        .unwrap_or(0);
    benchmark.candidate = Some(ProductProfileCandidate {
        batch_text,
        input_sequence: envelope.sequence,
        callback_to_host_ns: envelope.callback_to_host_ns.get(),
        accepted_at,
        parent_generation_before,
        parent_dispatch_us,
        parent_phase,
        update,
        requests,
        closed: false,
        editor_frame: None,
        editor_visible_us: None,
        compile_us: 0,
        pending_depth,
        completion_us: 0,
        completion_phase: RuntimePhaseTimings::default(),
        completion_update: RuntimeUpdateMeasurement::default(),
        completed_requests: BTreeSet::new(),
        invalid_completion: false,
        child_frame: None,
        preview_visible_us: None,
    });
    Ok(())
}

fn is_ascii_batch_end(event: &HostEvent) -> bool {
    matches!(event, HostEvent::Keyboard(key)
        if !key.pressed
            && key.physical_key.as_deref() == Some(ASCII_BATCH_END_PHYSICAL_KEY))
}

fn close_profile_input_batch(
    benchmark: &mut ProductProfileBenchmark,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(benchmark.phase, ProductProfilePhase::Complete) {
        return Ok(());
    }
    let expected = match benchmark.phase {
        ProductProfilePhase::Seed => benchmark.seed_text.as_str(),
        ProductProfilePhase::Samples => PROFILE_SAMPLE_TEXT,
        ProductProfilePhase::Complete => unreachable!(),
    };
    let candidate = benchmark
        .candidate
        .as_mut()
        .ok_or("profile batch marker arrived without an accepted text edit")?;
    if candidate.batch_text != expected {
        return Err("profile batch marker did not close the declared ASCII edit".into());
    }
    candidate.closed = true;
    if candidate.invalid_completion {
        return Err("profile uinput batch closed after an invalid final child program".into());
    }
    Ok(())
}

fn record_profile_invalid_completion(
    benchmark: &mut ProductProfileBenchmark,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let candidate = benchmark
        .candidate
        .as_mut()
        .ok_or("profile invalid completion has no pending input batch")?;
    candidate.invalid_completion = true;
    Ok(candidate.closed)
}

fn profile_candidate_has_request(
    benchmark: &ProductProfileBenchmark,
    session: &ProgramSessionId,
    request_id: &ProgramRequestId,
    revision: u64,
) -> bool {
    benchmark.candidate.as_ref().is_some_and(|candidate| {
        candidate.requests.iter().any(|request| {
            request.session == *session
                && request.request_id == *request_id
                && request.revision == revision
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn record_profile_program_completion(
    benchmark: &mut ProductProfileBenchmark,
    session: &ProgramSessionId,
    request_id: &ProgramRequestId,
    revision: u64,
    compile_us: u64,
    pending_depth: u32,
    completion_us: u64,
    completion_phase: RuntimePhaseTimings,
    update: RuntimeUpdateMeasurement,
    completion: ProgramHostCompletion,
    child_frame: Option<PresentedFrame>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !matches!(
        completion,
        ProgramHostCompletion::Program(ProgramCompletion::Activated {
            revision: activated
        }) if activated == revision
    ) {
        return Err("final profile child-program completion was not activated".into());
    }
    let candidate = benchmark
        .candidate
        .as_mut()
        .ok_or("profile completion has no pending input batch")?;
    let request = candidate.requests.iter().find(|request| {
        request.session == *session
            && request.request_id == *request_id
            && request.revision == revision
    });
    let Some(request) = request else {
        return Err("profile completion did not match its worker receipt".into());
    };
    candidate.compile_us = candidate.compile_us.max(compile_us);
    candidate.pending_depth = candidate
        .pending_depth
        .max(pending_depth)
        .max(request.pending_depth);
    candidate.completion_us = candidate.completion_us.saturating_add(completion_us);
    candidate.completion_phase.executor_us = candidate
        .completion_phase
        .executor_us
        .saturating_add(completion_phase.executor_us);
    candidate.completion_phase.document_us = candidate
        .completion_phase
        .document_us
        .saturating_add(completion_phase.document_us);
    candidate.completion_phase.persistence_enqueue_us = candidate
        .completion_phase
        .persistence_enqueue_us
        .saturating_add(completion_phase.persistence_enqueue_us);
    candidate.completion_update.accumulate(update);
    candidate
        .completed_requests
        .insert((session.0.clone(), request_id.0.clone()));
    let child_frame = child_frame.ok_or("profile child activation did not present a frame")?;
    candidate.preview_visible_us = Some(duration_us(candidate.accepted_at.elapsed()));
    candidate.child_frame = Some(child_frame);
    Ok(())
}

fn profile_candidate_is_ready(benchmark: &ProductProfileBenchmark) -> bool {
    benchmark.candidate.as_ref().is_some_and(|candidate| {
        candidate.closed
            && candidate.editor_frame.is_some()
            && candidate.editor_visible_us.is_some()
            && candidate.child_frame.is_some()
            && candidate.preview_visible_us.is_some()
            && candidate.requests.iter().all(|request| {
                candidate
                    .completed_requests
                    .contains(&(request.session.0.clone(), request.request_id.0.clone()))
            })
    })
}

struct ProfileFinalize {
    proof_request: Option<PreparedProofRequest>,
    restore_baseline: Option<Vec<u8>>,
}

fn finalize_profile_candidate(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &RuntimeView,
    benchmark: &mut ProductProfileBenchmark,
    product: &mut ProductFrame,
) -> Result<ProfileFinalize, Box<dyn std::error::Error + Send + Sync>> {
    let candidate = benchmark
        .candidate
        .take()
        .ok_or("profile candidate disappeared before finalization")?;
    let editor_frame = candidate
        .editor_frame
        .expect("checked profile editor frame");
    let child_frame = candidate.child_frame.expect("checked profile child frame");
    if !editor_frame.key.same_producer_surface(&child_frame.key)
        || child_frame.key.frame_id <= editor_frame.key.frame_id
        || editor_frame.event_sequence != Some(candidate.input_sequence)
        || editor_frame.input_kind != Some(crate::observer::InputKind::Text)
    {
        return Err(
            "profile editor and child frames do not form one exact native frame chain".into(),
        );
    }
    let editor_block_us = candidate
        .parent_dispatch_us
        .saturating_add(candidate.update.total_us())
        .saturating_add(editor_frame.frame_us);
    let child_block_us = candidate
        .completion_us
        .saturating_add(candidate.completion_update.total_us())
        .saturating_add(child_frame.frame_us);
    let trusted_parent_rebuilds = runtime
        .parent_runtime_generation()
        .saturating_sub(candidate.parent_generation_before)
        .try_into()
        .unwrap_or(u32::MAX);
    let (pending_program_artifact_stores, pending_program_artifact_loads) =
        runtime.program_artifact_lane_counts();
    let persistence = runtime.persistence_status();
    match benchmark.phase {
        ProductProfilePhase::Seed => {
            emit(
                observer,
                ObserverEvent::ProfileInputSeeded {
                    input_sequence: candidate.input_sequence,
                    callback_to_host_ns: candidate.callback_to_host_ns,
                    compile_us: candidate.compile_us,
                    pending_child_artifacts: candidate.pending_depth,
                    editor_key: editor_frame.key,
                    key: child_frame.key,
                },
            );
            benchmark.phase = ProductProfilePhase::Samples;
            Ok(ProfileFinalize {
                proof_request: None,
                restore_baseline: None,
            })
        }
        ProductProfilePhase::Samples => {
            benchmark.completed_samples = benchmark.completed_samples.saturating_add(1);
            let ordinal = benchmark.completed_samples;
            let proof_request = if ordinal == 11 {
                Some(prepare_evidence_proof(
                    "profile-benchmark",
                    child_frame.key.clone(),
                    product,
                )?)
            } else {
                None
            };
            emit(
                observer,
                ObserverEvent::ProfileSample {
                    ordinal,
                    input_sequence: candidate.input_sequence,
                    callback_to_host_ns: candidate.callback_to_host_ns,
                    editor_visible_us: candidate.editor_visible_us.expect("checked editor timing"),
                    preview_visible_us: candidate.preview_visible_us.expect("checked child timing"),
                    compile_us: candidate.compile_us,
                    parent_dispatch_us: candidate.parent_dispatch_us,
                    parent_executor_us: candidate.parent_phase.executor_us,
                    parent_runtime_document_us: candidate.parent_phase.document_us,
                    parent_persistence_us: candidate.parent_phase.persistence_enqueue_us,
                    completion_us: candidate.completion_us,
                    completion_executor_us: candidate.completion_phase.executor_us,
                    completion_runtime_document_us: candidate.completion_phase.document_us,
                    completion_persistence_us: candidate.completion_phase.persistence_enqueue_us,
                    document_us: candidate
                        .update
                        .document_us
                        .saturating_add(candidate.completion_update.document_us),
                    interaction_us: candidate
                        .update
                        .interaction_us
                        .saturating_add(candidate.completion_update.interaction_us),
                    demand_us: candidate
                        .update
                        .demand_us
                        .saturating_add(candidate.completion_update.demand_us),
                    present_us: child_frame.present_us,
                    patch_count: candidate
                        .update
                        .patch_count
                        .saturating_add(candidate.completion_update.patch_count),
                    full_lowered: candidate.update.full_lowered
                        || candidate.completion_update.full_lowered,
                    interaction_frame_block_us: editor_block_us.max(child_block_us),
                    pending_child_artifacts: candidate.pending_depth,
                    pending_program_artifact_stores: pending_program_artifact_stores
                        .try_into()
                        .unwrap_or(u32::MAX),
                    pending_program_artifact_loads: pending_program_artifact_loads
                        .try_into()
                        .unwrap_or(u32::MAX),
                    pending_persistence_artifact_stores: persistence
                        .pending_content_artifact_stores
                        .try_into()
                        .unwrap_or(u32::MAX),
                    pending_persistence_artifact_loads: persistence
                        .pending_content_artifact_loads
                        .try_into()
                        .unwrap_or(u32::MAX),
                    pending_durable_batches: persistence
                        .queued_checkpoint_batches
                        .try_into()
                        .unwrap_or(u32::MAX),
                    trusted_parent_rebuilds,
                    source_revision,
                    runtime_sequence: runtime.runtime_turn_sequence(),
                    editor_key: editor_frame.key,
                    key: child_frame.key,
                },
            );
            let restore_baseline = if ordinal == benchmark.sample_count {
                benchmark.phase = ProductProfilePhase::Complete;
                Some(std::mem::take(&mut benchmark.baseline))
            } else {
                None
            };
            Ok(ProfileFinalize {
                proof_request,
                restore_baseline,
            })
        }
        ProductProfilePhase::Complete => Err("completed profile produced another candidate".into()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn finalize_ready_profile_candidate(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    benchmark: &mut ProductProfileBenchmark,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
    proof: Option<&ProofWorker>,
    queued_evidence_proofs: &mut VecDeque<PreparedProofRequest>,
    evidence_proof_in_flight: &mut Option<crate::observer::FrameEvidenceKey>,
    program_compiler: &ProgramCompileWorker,
    latest_presented_key: &mut Option<crate::observer::FrameEvidenceKey>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !profile_candidate_is_ready(benchmark) {
        return Ok(());
    }
    let finalized =
        finalize_profile_candidate(observer, source_revision, runtime, benchmark, product)?;
    if let Some(request) = finalized.proof_request {
        queue_evidence_proofs(
            observer,
            proof,
            queued_evidence_proofs,
            evidence_proof_in_flight,
            [request],
        )?;
    }
    if let Some(baseline) = finalized.restore_baseline {
        runtime.activate_state_artifact(&baseline)?;
        if let Some(restored) = present_runtime(runtime, view, product, host, columns).await? {
            emit_presented(observer, &restored);
            *latest_presented_key = Some(restored.key);
        }
        runtime.resolve_program_artifact_requests()?;
        submit_program_requests(runtime.take_program_requests(), program_compiler);
    }
    Ok(())
}

fn retain_latest_program_request(
    latest: &mut BTreeMap<ProgramSessionId, ProgramHostRequest>,
    request: ProgramHostRequest,
) {
    let replace = latest.get(&request.session).is_none_or(|current| {
        (request.compile.revision, request.request_id.0.as_str())
            > (current.compile.revision, current.request_id.0.as_str())
    });
    if replace {
        latest.insert(request.session.clone(), request);
    }
}

fn submit_program_requests(
    requests: Vec<ProgramHostRequest>,
    worker: &ProgramCompileWorker,
) -> Vec<SubmittedProgramRequest> {
    let mut submitted = BTreeMap::<ProgramSessionId, SubmittedProgramRequest>::new();
    for request in requests {
        assert!(
            !request.is_artifact_load(),
            "artifact-backed program request reached the compile worker"
        );
        let identity = SubmittedProgramRequest {
            session: request.session.clone(),
            request_id: request.request_id.clone(),
            revision: request.compile.revision,
            pending_depth: 0,
        };
        let ProgramCompileReceipt {
            accepted,
            pending_depth,
        } = worker.replace(request);
        if accepted {
            submitted.insert(
                identity.session.clone(),
                SubmittedProgramRequest {
                    pending_depth,
                    ..identity
                },
            );
        }
    }
    submitted.into_values().collect()
}

fn send_program_status(
    output: &PreviewOutput,
    runtime: Option<&RuntimeView>,
    source_revision: u64,
) -> Result<(), String> {
    let runtime = runtime.ok_or_else(|| "program status has no mounted runtime".to_owned())?;
    if let Some(diagnostic) = runtime.program_diagnostics().first() {
        output.send(Message::PreviewStatus {
            revision: source_revision,
            ok: false,
            message: format!(
                "embedded program revision {} failed; last valid preview retained: {}",
                diagnostic.diagnostic.revision, diagnostic.diagnostic.message
            ),
        })
    } else {
        output.send(Message::PreviewStatus {
            revision: source_revision,
            ok: true,
            message: "embedded program compiled and mounted".to_owned(),
        })
    }
}

fn assert_test_step_semantics(runtime: &mut RuntimeView, step: &TestStep) -> Result<usize, String> {
    if step.expectations.is_empty() {
        return Ok(0);
    }
    runtime.assert_scenario_step(&ScenarioStep {
        id: step.id.clone(),
        user_action_kind: step.action_kind.clone(),
        user_action_text: step.text.clone(),
        user_action_key: step.key.clone(),
        source_event: None,
        expectations: step.expectations.clone(),
    })?;
    Ok(step.expectations.len())
}

fn settle_test_runtime(
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    apply_runtime_update(runtime, view, columns)?;
    let started = Instant::now();
    let mut round = 0usize;
    loop {
        if started.elapsed() > TEST_SETTLE_TIMEOUT {
            let (pending_stores, pending_loads) = runtime.program_artifact_lane_counts();
            return Err(format!(
                "TEST runtime did not settle within {}ms after {round} rounds; program stores={pending_stores}, loads={pending_loads}, effect_deadline={}",
                TEST_SETTLE_TIMEOUT.as_millis(),
                runtime.effect_poll_deadline().is_some(),
            )
            .into());
        }
        round = round.saturating_add(1);

        let mut changed = runtime.resolve_program_artifact_requests()?;
        let requests = runtime.take_program_requests();
        if requests.len() > TEST_SETTLE_PROGRAM_LIMIT {
            return Err(format!(
                "TEST runtime produced {} child program requests in one round; limit is {TEST_SETTLE_PROGRAM_LIMIT}",
                requests.len()
            )
            .into());
        }
        let had_program_requests = !requests.is_empty();
        let mut latest = BTreeMap::<_, ProgramHostRequest>::new();
        for request in requests {
            retain_latest_program_request(&mut latest, request);
        }

        for request in latest.into_values() {
            if request.is_artifact_load() {
                return Err("stored artifact request reached the TEST compiler".into());
            }
            let result = compile_program_artifact(&request.compile);
            changed |= runtime.complete_program(&request.session, &request.request_id, result)?;
        }

        changed |= runtime.poll_program_artifact_stores()?;

        if let Some(deadline) = runtime.effect_poll_deadline() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                thread::sleep(remaining.min(Duration::from_millis(2)));
            }
            changed |= runtime.poll_host_effects(Instant::now())?;
        }

        if changed {
            apply_runtime_update(runtime, view, columns)?;
        }
        let (pending_stores, pending_loads) = runtime.program_artifact_lane_counts();
        let artifact_work_pending = pending_stores > 0 || pending_loads > 0;
        if !had_program_requests
            && !artifact_work_pending
            && runtime.effect_poll_deadline().is_none()
        {
            return Ok(());
        }
        if artifact_work_pending {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

pub(crate) fn test_step_pointer_position(
    view: &RetainedView,
    target: &HitTarget,
    step: &TestStep,
) -> (f32, f32) {
    let Some(bounds) = view.node_bounds(&target.node) else {
        return (target.center_x, target.center_y);
    };
    (
        projected_test_coordinate(
            step.pointer_x.as_deref(),
            step.pointer_width.as_deref(),
            bounds.x,
            bounds.width,
            target.center_x,
        ),
        projected_test_coordinate(
            step.pointer_y.as_deref(),
            step.pointer_height.as_deref(),
            bounds.y,
            bounds.height,
            target.center_y,
        ),
    )
}

fn projected_test_coordinate(
    value: Option<&str>,
    source_span: Option<&str>,
    target_start: f32,
    target_span: f32,
    fallback: f32,
) -> f32 {
    let Some(value) = value.and_then(|value| value.parse::<f32>().ok()) else {
        return fallback;
    };
    let Some(source_span) = source_span
        .and_then(|span| span.parse::<f32>().ok())
        .filter(|span| span.is_finite() && *span > 0.0)
    else {
        return fallback;
    };
    if !value.is_finite() || !target_span.is_finite() || target_span <= 0.0 {
        return fallback;
    }
    let inset = (target_span * 0.5).min(0.5);
    let usable = (target_span - inset * 2.0).max(0.0);
    target_start + inset + usable * (value / source_span).clamp(0.0, 1.0)
}

fn test_cursor_path(from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
    let distance = (to.0 - from.0).hypot(to.1 - from.1);
    let frames = ((distance / TEST_CURSOR_PIXELS_PER_FRAME).ceil() as usize)
        .clamp(1, TEST_CURSOR_MAX_MOVE_FRAMES);
    (1..=frames)
        .map(|frame| {
            let linear = frame as f32 / frames as f32;
            let eased = linear * linear * (3.0 - 2.0 * linear);
            (
                from.0 + (to.0 - from.0) * eased,
                from.1 + (to.1 - from.1) * eased,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn present_test_cursor_frame(
    observer: &Option<ObserverClient>,
    request_id: u64,
    step_index: usize,
    phase: TestPointerPhase,
    target: Option<&str>,
    runtime_sequence: u64,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    view: &RetainedView,
    cursor: (f32, f32),
    dwell_frames: u32,
) -> Result<crate::observer::FrameEvidenceKey, Box<dyn std::error::Error + Send + Sync>> {
    for _ in 0..3 {
        if let Some(presented) = product
            .present_cursor(host, view, cursor.0, cursor.1)
            .await?
        {
            emit_presented(observer, &presented);
            emit(
                observer,
                ObserverEvent::TestPointerFrame {
                    request_id,
                    step_index: step_index.try_into().unwrap_or(u32::MAX),
                    phase,
                    x: cursor.0,
                    y: cursor.1,
                    target: target.map(str::to_owned),
                    runtime_sequence,
                    key: presented.key.clone(),
                },
            );
            thread::sleep(TEST_CURSOR_FRAME.saturating_mul(dwell_frames));
            return Ok(presented.key);
        }
        thread::sleep(TEST_CURSOR_FRAME);
    }
    Err("TEST cursor frame could not be presented after three attempts".into())
}

fn apply_runtime_update(
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    Ok(apply_runtime_update_measured(runtime, view, columns)?.changed)
}

fn apply_runtime_update_measured(
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<RuntimeUpdateMeasurement, Box<dyn std::error::Error + Send + Sync>> {
    let patches = runtime.take_patches();
    let patch_count = patches.len().try_into().unwrap_or(u32::MAX);
    let document_started = Instant::now();
    let document = match view.apply_patches(patches, columns) {
        Ok(document) => document,
        Err(error) => {
            if let boon_document::PatchApplyError::MissingParent { id, parent } = &error {
                let authoritative = runtime.frame();
                return Err(format!(
                    "retained patch base diverged: node `{}` requires parent `{}`; retained_has_parent={}, authoritative_has_parent={}, retained_nodes={}, authoritative_nodes={}, patch_count={patch_count}: {error}",
                    id.0,
                    parent.0,
                    view.frame().nodes.contains_key(parent),
                    authoritative.nodes.contains_key(parent),
                    view.frame().nodes.len(),
                    authoritative.nodes.len(),
                )
                .into());
            }
            return Err(error.into());
        }
    };
    let document_us = duration_us(document_started.elapsed());
    let interaction_started = Instant::now();
    let interaction = view.set_interaction_state(runtime.hovered(), runtime.focused(), columns)?;
    let interaction_us = duration_us(interaction_started.elapsed());
    let demand_started = Instant::now();
    let demand_changed = converge_document_demands(runtime, view, columns)?;
    let demand_us = duration_us(demand_started.elapsed());
    Ok(RuntimeUpdateMeasurement {
        changed: document.render_changed
            || document.layout_changed
            || interaction.render_changed
            || interaction.layout_changed
            || demand_changed,
        document_us,
        interaction_us,
        demand_us,
        patch_count,
        full_lowered: document.full_lowered,
    })
}

impl RuntimeUpdateMeasurement {
    fn total_us(&self) -> u64 {
        self.document_us
            .saturating_add(self.interaction_us)
            .saturating_add(self.demand_us)
    }

    fn accumulate(&mut self, other: Self) {
        self.changed |= other.changed;
        self.document_us = self.document_us.saturating_add(other.document_us);
        self.interaction_us = self.interaction_us.saturating_add(other.interaction_us);
        self.demand_us = self.demand_us.saturating_add(other.demand_us);
        self.patch_count = self.patch_count.saturating_add(other.patch_count);
        self.full_lowered |= other.full_lowered;
    }
}

fn converge_document_demands(
    runtime: &mut RuntimeView,
    view: &mut RetainedView,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut visible_changed = false;
    for _ in 0..4 {
        let demands = view.demands().to_vec();
        if !runtime.apply_layout_demands(&demands)? {
            return Ok(visible_changed);
        }
        let update = view.apply_patches(runtime.take_patches(), columns)?;
        visible_changed |= update.render_changed || update.layout_changed;
    }
    Err("document materialization demands did not converge in four passes".into())
}

fn observe_input(
    observer: &Option<ObserverClient>,
    envelope: &HostEventEnvelope,
    target: Option<String>,
    target_source_path: Option<String>,
    visible_change: bool,
) {
    let (pointer_x, pointer_y) = event_position(&envelope.event);
    emit(
        observer,
        ObserverEvent::InputAccepted(InputAccepted {
            role: ObserverRole::Preview,
            event_sequence: envelope.sequence,
            real_os: envelope.origin == HostEventOrigin::RealOs,
            callback_to_host_ns: envelope.callback_to_host_ns.get(),
            surface_epoch: envelope.surface_epoch,
            kind: input_kind(&envelope.event),
            pointer_button_pressed: pointer_button_pressed(&envelope.event),
            pointer_x,
            pointer_y,
            target,
            target_source_path,
            event_digest: host_event_digest(envelope),
            visible_change,
        }),
    );
}

fn event_position(event: &HostEvent) -> (Option<f32>, Option<f32>) {
    match event {
        HostEvent::Pointer(pointer) => (Some(pointer.x), Some(pointer.y)),
        HostEvent::Wheel(wheel) => (Some(wheel.x), Some(wheel.y)),
        _ => (None, None),
    }
}

fn emit_presented(observer: &Option<ObserverClient>, frame: &PresentedFrame) {
    let drops = observer
        .as_ref()
        .map(ObserverClient::dropped_count)
        .unwrap_or(0);
    emit(observer, frame.observer_event(ObserverRole::Preview, drops));
}

fn emit_runtime_async_lanes(
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    product: &ProductFrame,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let observations = runtime.take_async_lane_observations();
    if observations.is_empty() {
        return Ok(());
    }
    let key = product
        .last_presented_key()
        .cloned()
        .ok_or("async runtime lane completed before a production frame existed")?;
    emit_runtime_async_observations(observer, observations, key);
    Ok(())
}

fn emit_runtime_async_observations(
    observer: &Option<ObserverClient>,
    observations: Vec<RuntimeAsyncLaneObservation>,
    key: crate::observer::FrameEvidenceKey,
) {
    for observation in observations {
        let end_to_end_us = observed_async_end_to_end(&observation);
        emit(
            observer,
            ObserverEvent::AsyncLaneCompleted {
                lane: observed_async_lane(observation.lane),
                request_id: observation.request_id,
                revision: observation.revision,
                queue_depth: observation.queue_depth,
                queue_wait_us: observation.queue_wait_us,
                worker_us: observation.worker_us,
                apply_us: observation.apply_us,
                end_to_end_us,
                outcome: observed_async_outcome(observation.outcome),
                key: key.clone(),
            },
        );
    }
}

fn emit_runtime_async_lanes_before_present(
    observer: &Option<ObserverClient>,
    surface_id: &str,
    observations: &[RuntimeAsyncLaneObservation],
) {
    for observation in observations {
        emit(
            observer,
            ObserverEvent::AsyncLaneCompletedBeforePresent {
                surface_id: surface_id.to_owned(),
                process_id: std::process::id(),
                lane: observed_async_lane(observation.lane),
                request_id: observation.request_id.clone(),
                revision: observation.revision,
                queue_depth: observation.queue_depth,
                queue_wait_us: observation.queue_wait_us,
                worker_us: observation.worker_us,
                apply_us: observation.apply_us,
                end_to_end_us: observed_async_end_to_end(observation),
                outcome: observed_async_outcome(observation.outcome),
            },
        );
    }
}

fn observed_async_lane(lane: RuntimeAsyncLaneKind) -> AsyncLaneKind {
    match lane {
        RuntimeAsyncLaneKind::PersistenceTurn => AsyncLaneKind::PersistenceTurn,
        RuntimeAsyncLaneKind::ProgramArtifactStore => AsyncLaneKind::ProgramArtifactStore,
        RuntimeAsyncLaneKind::ProgramArtifactLoad => AsyncLaneKind::ProgramArtifactLoad,
    }
}

fn observed_async_outcome(outcome: RuntimeAsyncLaneOutcome) -> AsyncLaneOutcome {
    match outcome {
        RuntimeAsyncLaneOutcome::Applied => AsyncLaneOutcome::Applied,
        RuntimeAsyncLaneOutcome::StaleRejected => AsyncLaneOutcome::StaleRejected,
        RuntimeAsyncLaneOutcome::Failed => AsyncLaneOutcome::Failed,
    }
}

fn observed_async_end_to_end(observation: &RuntimeAsyncLaneObservation) -> u64 {
    accounted_end_to_end_us(
        observation.end_to_end_us,
        observation.queue_wait_us,
        observation.worker_us,
        observation.apply_us,
    )
}

fn program_async_lane_outcome(
    completion: &ProgramCompletionObservation,
    failed: bool,
) -> AsyncLaneOutcome {
    if failed {
        return AsyncLaneOutcome::Failed;
    }
    match completion {
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Stale { .. },
        ))
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Superseded { .. })
        | ProgramCompletionObservation::Host(ProgramHostCompletion::Removed { .. }) => {
            AsyncLaneOutcome::StaleRejected
        }
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Rejected { .. },
        )) => AsyncLaneOutcome::Failed,
        ProgramCompletionObservation::Host(ProgramHostCompletion::Program(
            ProgramCompletion::Activated { .. },
        ))
        | ProgramCompletionObservation::ArtifactStorePending { .. } => AsyncLaneOutcome::Applied,
    }
}

fn emit(observer: &Option<ObserverClient>, event: ObserverEvent) {
    if let Some(observer) = observer {
        observer.emit(event);
    }
}

fn event_target(
    view: &RetainedView,
    event: &HostEvent,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Option<HitTarget> {
    match event {
        HostEvent::Pointer(pointer) => {
            view.hit_target_with_text_column(pointer.x, pointer.y, columns)
        }
        HostEvent::Wheel(wheel) => {
            view.wheel_target(wheel.x, wheel.y, wheel.delta_x, wheel.delta_y, columns)
        }
        _ => None,
    }
}

fn viewport(host: &NativeSurfaceHost) -> Viewport {
    let native = host.viewport();
    Viewport {
        surface: host.epoch(),
        width: native.logical_size.width,
        height: native.logical_size.height,
        scale: native.scale,
    }
}

#[allow(clippy::too_many_arguments)]
fn send_stats(
    output: &PreviewOutput,
    product: &ProductFrame,
    runtime: Option<&RuntimeView>,
    source_revision: u64,
    frame_mode: FrameMode,
    dropped_snapshots: u64,
    last_sent: &mut Option<Instant>,
    force: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = Instant::now();
    if !force && last_sent.is_some_and(|last| now.saturating_duration_since(last) < STATS_INTERVAL)
    {
        return Ok(());
    }
    let stats = product.stats();
    let (
        persistence_schema_version,
        persistence_durable_epoch,
        persistence_durable_turn,
        persistence_pending_turns,
        persistence_queue_depth,
        persistence_accepting,
        persistence_worker_alive,
        persistence_error,
    ) = runtime.map_or_else(
        || (0, 0, 0, 0, 0, false, false, String::new()),
        |runtime| {
            let status = runtime.persistence_status();
            let pending_turns = status.pending.as_ref().map_or(0, |pending| {
                pending
                    .last_turn_sequence
                    .saturating_sub(pending.first_turn_sequence)
                    .saturating_add(1)
            });
            (
                runtime.persistence_schema_version(),
                status.durable_epoch,
                status.durable_through_turn_sequence,
                pending_turns,
                status.queue_depth,
                status.accepting_turns,
                status.worker_alive,
                status
                    .last_error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            )
        },
    );
    output.try_send_stats(Message::PreviewStats(PreviewStats {
        frame_seq: stats.frame_id,
        source_revision,
        frame_mode,
        proof_mode: if stats.proof_enabled {
            ProofMode::Readback
        } else {
            ProofMode::Off
        },
        frames_per_second_milli: 0,
        input_to_present_micros: saturating_u32(stats.last_input_to_present_us),
        render_micros: saturating_u32(stats.last_render_us),
        present_micros: saturating_u32(stats.last_present_us),
        missed_frames: stats.missed_frame_count,
        dropped_snapshots,
        sample_age_millis: 0,
        persistence_schema_version,
        persistence_durable_epoch,
        persistence_durable_turn,
        persistence_pending_turns: persistence_pending_turns.try_into().unwrap_or(u32::MAX),
        persistence_queue_depth: persistence_queue_depth.try_into().unwrap_or(u32::MAX),
        persistence_accepting,
        persistence_worker_alive,
        persistence_error,
    }))?;
    *last_sent = Some(now);
    Ok(())
}

fn saturating_u32(value: u64) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn accounted_end_to_end_us(measured: u64, queue_wait: u64, worker: u64, apply: u64) -> u64 {
    measured.max(queue_wait.saturating_add(worker).saturating_add(apply))
}
