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
use boon_persistence::{DecodeLimits, decode_restore_image, encode_restore_image};
use boon_plan::MigrationPredecessorBinding;
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};
use sha2::{Digest, Sha256};

use boon_runtime::{
    MigrationScenarioRunner, ProgramCompletion, ProgramHostCompletion, ProgramHostRequest,
    ProgramRequestId, ProgramSessionId, RuntimePhaseTimings, ScenarioStep,
    compile_program_artifact,
};

use crate::compile::{
    CompileRequest, CompileWorker, ProgramCompileReceipt, ProgramCompileWorker,
    compile_migration_stage, project_key_for_stage,
};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, input_kind,
    pointer_button_pressed, role_message_frame,
};
use crate::native_input::{ASCII_BATCH_END_PHYSICAL_KEY, ASCII_TEXT_BATCH_MAX_BYTES};
use crate::observer::{
    InputAccepted, MIGRATION_EVIDENCE_ENV, ObserverClient, ObserverEvent, ObserverRole,
    PERSISTENCE_EVIDENCE_ENV, PRODUCT_PROOF_AFTER_TEST_ENV, PROFILE_BENCHMARK_ENV,
    PROFILE_BENCHMARK_STEPS_ENV, PersistenceEvidenceKind, RESPONSIVE_EVIDENCE_WIDTH_ENV,
    SCROLL_PROOF_ORDINAL_ENV, STALE_PROGRAM_EVIDENCE_ENV, STATE_EVIDENCE_STEPS_ENV,
    STATE_MOUNT_EVIDENCE_ENV, StartupDisposition, StartupMigrationEvidence, TestPointerPhase,
};
use crate::proof::{ProofConfig, ProofRequest, ProofResult, ProofWorker};
use crate::protocol::{
    ApplicationIdentity, AssetBlob, CanonicalStateArtifact, Connection, FrameMode,
    MAX_PERSISTENCE_ARTIFACT_BYTES, Message, MigrationBundle, MigrationCommand, MigrationOperation,
    MigrationStatus, PersistenceCommand, PersistenceOperation, PersistenceOperationStatus,
    PreviewIntent, PreviewStats, ProofMode, Role, SourceUnit, StateArtifactFormat,
    StateArtifactPreviewSummary, TestStep,
};
use crate::runtime_view::{
    ProgramCompletionObservation, RuntimeStartupDisposition, RuntimeView, digest_hex,
};
use crate::view::{HitTarget, RetainedView};

pub(crate) const TEST_STEP_LIMIT: usize = 32;
const OUTBOUND_QUEUE_DEPTH: usize = 8;
const STATS_INTERVAL: Duration = Duration::from_millis(100);
const TEST_CURSOR_FRAME: Duration = Duration::from_millis(16);
const TEST_CURSOR_PIXELS_PER_FRAME: f32 = 36.0;
const TEST_CURSOR_MAX_MOVE_FRAMES: usize = 12;
const TEST_SETTLE_ROUND_LIMIT: usize = 64;
const TEST_SETTLE_PROGRAM_LIMIT: usize = 128;
const TEST_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
const PROFILE_SAMPLE_TEXT: &str = " ";

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
    expected_actions: BTreeSet<String>,
    last_surface_epoch: u64,
    complete: bool,
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
    stale_program: bool,
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
        let stale_program = std::env::var_os(STALE_PROGRAM_EVIDENCE_ENV).is_some();
        Ok(Self {
            mount,
            scenario_steps,
            persistence_exercise,
            migration_exercise,
            profile_samples,
            profile_steps,
            responsive_width,
            stale_program,
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
    }
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

    let mut product = ProductFrame::attach(&mut host, ObserverRole::Preview).await?;
    product.set_proof_enabled(proof.is_some());
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
    if let Some(presented) = product.present(&mut host, &view).await? {
        emit_presented(&observer, &presented);
    }

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
    let mut scroll_frame_ordinal = 0_u32;
    let mut scroll_proof_requested = false;

    loop {
        if let Some(runtime) = runtime.as_mut() {
            runtime.resolve_program_artifact_requests()?;
            submit_program_requests(runtime.take_program_requests(), &program_compiler);
        }
        deadline_scheduler.schedule(runtime.as_ref().and_then(|runtime| {
            [
                runtime.caret_blink_deadline(),
                runtime.scheduled_source_deadline(),
                runtime.persistence_poll_deadline(),
                runtime.effect_poll_deadline(),
            ]
            .into_iter()
            .flatten()
            .min()
        }));
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
            Compiled(Option<crate::compile::CompileOutcome>),
            ProgramCompiled(Option<crate::compile::ProgramCompileOutcome>),
            Proof(Option<Box<ProofResult>>),
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
            let scheduled = deadline_scheduler.ticks.next().fuse();
            pin_mut!(
                native,
                command,
                result,
                program_result,
                proof_result,
                scheduled
            );
            select! {
                value = native => Wake::Native(value),
                value = command => Wake::Ipc(value),
                value = result => Wake::Compiled(value),
                value = program_result => Wake::ProgramCompiled(value),
                value = proof_result => Wake::Proof(value.map(Box::new)),
                value = scheduled => Wake::Scheduled(value),
            }
        };

        match wake {
            Wake::Native(event) => {
                let mut transaction = NativeFrameTransaction::default();
                let mut latest_runtime_sequence = None;
                let mut persistence_turn_changed = false;
                let mut resize_observation = None::<(u64, u32, u32, u64)>;
                for accepted in drain_native_events(&mut host, event).await? {
                    let envelope = &accepted.envelope;
                    if matches!(envelope.event, HostEvent::CloseRequested { .. }) {
                        observe_input(&observer, envelope, None, None, false);
                        let _ = output.send(Message::Shutdown);
                        return Ok(());
                    }
                    let target = event_target(&view, &envelope.event, &mut columns);
                    let target_name = target.as_ref().map(|target| target.node.clone());
                    let target_source_path = target
                        .as_ref()
                        .and_then(|target| target.source_path.clone());
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
                        document_update_us = duration_us(started.elapsed());
                        if let Some(state) = responsive_evidence.as_mut() {
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
                    } else if let Some(model) = runtime.as_mut() {
                        let parent_generation_before = model.parent_runtime_generation();
                        let started = Instant::now();
                        let sequence_before = model.event_sequence();
                        let runtime_turn_before = model.runtime_turn_sequence();
                        let changed = model.handle_event(&envelope.event, target)?;
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
                    observe_input(&observer, envelope, target_name, target_source_path, dirty);
                    if dirty {
                        transaction.visible_change(&accepted);
                    }
                    if is_ascii_batch_end(&envelope.event) {
                        if let Some(benchmark) = profile_benchmark.as_mut() {
                            close_profile_input_batch(benchmark)?;
                        }
                    }
                }
                if let Some(presented) = transaction.present(&mut product, &mut host, &view).await?
                {
                    let proof_request = prepare_product_proof_request(
                        profile_benchmark.as_ref(),
                        proof.as_ref(),
                        proof_config.as_ref(),
                        &mut proof_requested,
                        &mut proof_eligible_ordinal,
                        &presented,
                        &view,
                        &host,
                    );
                    emit_presented(&observer, &presented);
                    submit_proof_request(&observer, proof.as_ref(), proof_request)?;
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
                                    &view,
                                    &host,
                                )],
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
                            let request = capture_responsive_layout_evidence(
                                &observer,
                                source_revision,
                                runtime
                                    .as_mut()
                                    .ok_or("responsive evidence has no runtime")?,
                                &view,
                                &host,
                                state,
                                sequence,
                                presented.key.clone(),
                            )?;
                            state.complete = true;
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
                            .into_iter()
                            .map(render_asset_source)
                            .collect::<Vec<_>>();
                        product.replace_asset_sources(sources.clone())?;
                        if let Some(proof) = proof.as_ref() {
                            proof.replace_asset_sources(sources)?;
                        }
                    }
                    Message::PreviewApply {
                        intent,
                        request_id,
                        application,
                        revision,
                        units,
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
                        {
                            let request_id = request_id.unwrap_or(0);
                            let result = if revision == source_revision {
                                run_migration_test(bundle, &application, request_id, revision)
                            } else {
                                Err(format!(
                                    "migration TEST revision {revision} is stale; preview is at {source_revision}"
                                ))
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
                        let key =
                            project_key_for_stage(&application, &units, migration_stage.as_deref());
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
                            application,
                            revision,
                            units,
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
                        let compiled_units = compiled_preview.units.clone();
                        let revision = compiled_preview.revision;
                        let post_compile_started = Instant::now();
                        let deterministic_runtime =
                            test || !state_evidence.scenario_steps.is_empty();
                        match activate_compatible(
                            &mut runtime,
                            compiled_preview.plan,
                            deterministic_runtime,
                        ) {
                            Ok(activation) => {
                                let presented = match activation {
                                    RuntimeActivation::Opened(next) => {
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
                                    if state_evidence.enabled() {
                                        let request = capture_state_mounted(
                                            &observer,
                                            runtime.as_mut().expect("mounted runtime"),
                                            source_revision,
                                            presented,
                                            &view,
                                            &host,
                                        )?;
                                        queue_evidence_proofs(
                                            &observer,
                                            proof.as_ref(),
                                            &mut queued_evidence_proofs,
                                            &mut evidence_proof_in_flight,
                                            [request],
                                        )?;
                                    }
                                    if state_evidence.migration_exercise
                                        && !migration_evidence_completed
                                    {
                                        let request = run_schema_migration_evidence(
                                            &observer,
                                            source_revision,
                                            runtime.as_mut().expect("mounted runtime"),
                                            &mut view,
                                            &mut product,
                                            &mut host,
                                            &mut columns,
                                            &compiled_units,
                                        )
                                        .await?;
                                        queue_evidence_proofs(
                                            &observer,
                                            proof.as_ref(),
                                            &mut queued_evidence_proofs,
                                            &mut evidence_proof_in_flight,
                                            [request],
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
                                    let (passed, semantic_assertions_proven, completed, message) =
                                        match result {
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
                                                            &observer, &view, &host, width,
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
                                    if std::env::var_os(PRODUCT_PROOF_AFTER_TEST_ENV).is_some() {
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
                if changed {
                    if let Some(presented) = product.present(&mut host, &view).await? {
                        let proof_request = prepare_product_proof_request(
                            profile_benchmark.as_ref(),
                            proof.as_ref(),
                            proof_config.as_ref(),
                            &mut proof_requested,
                            &mut proof_eligible_ordinal,
                            &presented,
                            &view,
                            &host,
                        );
                        emit_presented(&observer, &presented);
                        submit_proof_request(&observer, proof.as_ref(), proof_request)?;
                        child_frame = Some(presented);
                    }
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
                        )
                        .await?;
                    }
                }
                send_program_status(&output, Some(model), source_revision)?;
            }
            Wake::Proof(result) => {
                let result = result.ok_or("proof worker stopped")?;
                let completed_key = result.key.clone();
                let worker = proof.as_ref().expect("proof result without worker");
                emit(
                    &observer,
                    result.observer_event(
                        product.frame_id(),
                        worker.replaced_count(),
                        worker.result_drop_count(),
                    ),
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
    }
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

fn activate_compatible(
    runtime: &mut Option<RuntimeView>,
    plan: Arc<boon_plan::MachinePlan>,
    deterministic_scenario: bool,
) -> Result<RuntimeActivation, String> {
    if let Some(runtime) = runtime.as_mut()
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
    if deterministic_scenario {
        RuntimeView::open_for_scenario(plan)
    } else {
        RuntimeView::open(plan)
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

fn product_proof_is_eligible(benchmark: Option<&ProductProfileBenchmark>) -> bool {
    benchmark.is_none_or(|benchmark| matches!(benchmark.phase, ProductProfilePhase::Complete))
}

#[allow(clippy::too_many_arguments)]
fn prepare_product_proof_request(
    benchmark: Option<&ProductProfileBenchmark>,
    proof: Option<&ProofWorker>,
    config: Option<&ProofConfig>,
    requested: &mut bool,
    ordinal: &mut u64,
    presented: &PresentedFrame,
    view: &RetainedView,
    host: &NativeSurfaceHost,
) -> Option<PreparedProofRequest> {
    if !product_proof_is_eligible(benchmark) {
        return None;
    }
    *ordinal = ordinal.saturating_add(1);
    prepare_proof_request(proof, config, requested, *ordinal, presented, view, host)
}

fn prepare_proof_request(
    proof: Option<&ProofWorker>,
    config: Option<&ProofConfig>,
    requested: &mut bool,
    ordinal: u64,
    presented: &PresentedFrame,
    view: &RetainedView,
    host: &NativeSurfaceHost,
) -> Option<PreparedProofRequest> {
    let (Some(_proof), Some(config)) = (proof, config) else {
        return None;
    };
    if *requested || ordinal < config.sample_ordinal {
        return None;
    }
    let snapshot_started = Instant::now();
    let native = host.viewport();
    *requested = true;
    Some(PreparedProofRequest {
        request: ProofRequest {
            key: presented.key.clone(),
            scene: view.scene().clone(),
            width: native.physical_size.width,
            height: native.physical_size.height,
            surface_id: host.ids().surface.clone(),
            artifact_label: format!("preview-frame-{}", presented.key.frame_id),
        },
        snapshot_prepare_us: duration_us(snapshot_started.elapsed()),
    })
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
    let image = decode_restore_image(&artifact, DecodeLimits::default())?;
    let durable_epoch = image.epoch;
    let durable_turn_sequence = image.through_turn_sequence;
    let semantic = semantic_authority_image(image);
    let canonical = encode_restore_image(&semantic)?;
    Ok(AuthoritativeStateEvidence {
        artifact,
        digest: format!("{:x}", Sha256::digest(canonical)),
        durable_epoch,
        durable_turn_sequence,
    })
}

fn semantic_authority_image(
    mut image: boon_persistence::RestoreImage,
) -> boon_persistence::RestoreImage {
    image.epoch = 0;
    image.through_turn_sequence = 0;
    image.outbox.retain(|_, item| {
        !matches!(
            &item.state,
            boon_persistence::DurableOutboxState::Completed { .. }
        )
    });
    image
}

fn importable_authority(
    artifact: &[u8],
) -> Result<boon_persistence::RestoreImage, boon_persistence::CodecError> {
    let mut image = decode_restore_image(artifact, DecodeLimits::default())?;
    image.epoch = 0;
    image.through_turn_sequence = 0;
    image.outbox.clear();
    Ok(image)
}

fn prepare_evidence_proof(
    label: &str,
    key: crate::observer::FrameEvidenceKey,
    view: &RetainedView,
    host: &NativeSurfaceHost,
) -> PreparedProofRequest {
    let started = Instant::now();
    let native = host.viewport();
    PreparedProofRequest {
        request: ProofRequest {
            artifact_label: format!("{label}-frame-{}", key.frame_id),
            key,
            scene: view.scene().clone(),
            width: native.physical_size.width,
            height: native.physical_size.height,
            surface_id: host.ids().surface.clone(),
        },
        snapshot_prepare_us: duration_us(started.elapsed()),
    }
}

fn capture_state_mounted(
    observer: &Option<ObserverClient>,
    runtime: &mut RuntimeView,
    source_revision: u64,
    presented: &PresentedFrame,
    view: &RetainedView,
    host: &NativeSurfaceHost,
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let state = authoritative_state_evidence(runtime)?;
    let startup = runtime.startup_evidence();
    let (disposition, migration) = match &startup.disposition {
        RuntimeStartupDisposition::Fresh => (StartupDisposition::Fresh, None),
        RuntimeStartupDisposition::Restored => (StartupDisposition::Restored, None),
        RuntimeStartupDisposition::Migrated(preview) => (
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
    Ok(prepare_evidence_proof(
        "state-mounted",
        presented.key.clone(),
        view,
        host,
    ))
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
    view: &RetainedView,
    host: &NativeSurfaceHost,
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
    Ok(prepare_evidence_proof(
        &format!("checkpoint-{}", step.id),
        key,
        view,
        host,
    ))
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
        view,
        host,
    )];
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
        view,
        host,
    ));

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
    let cleared_image = decode_restore_image(&cleared.artifact, DecodeLimits::default())?;
    let activated_image = decode_restore_image(&activated.artifact, DecodeLimits::default())?;
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
        view,
        host,
    ));
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
    source_units: &[SourceUnit],
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let current = runtime.shared_machine_plan();
    let target_schema_version = current
        .persistence
        .schema_version
        .checked_add(1)
        .ok_or("persistence schema version overflow")?;
    let root_source = source_units
        .last()
        .ok_or("schema migration evidence has no active source units")?
        .path
        .clone();
    let probe_name = format!("native_migration_probe_{target_schema_version}");
    if source_units
        .iter()
        .any(|unit| unit.source.contains(&format!("{probe_name}:")))
    {
        return Err("schema migration evidence probe collides with product source".into());
    }
    let mut target_units = source_units
        .iter()
        .map(|unit| boon_compiler::CompilerSourceUnit {
            path: unit.path.clone(),
            source: unit.source.clone(),
        })
        .collect::<Vec<_>>();
    target_units.push(boon_compiler::CompilerSourceUnit {
        path: format!("migration-evidence-v{target_schema_version}.bn"),
        source: format!(
            "{probe_name}: TEXT {{ schema-{target_schema_version} }} |> HOLD {probe_name} {{ LATEST {{}} }}\n"
        ),
    });
    let predecessor = MigrationPredecessorBinding::from_machine_plan(&current);
    let target = Arc::new(
        boon_compiler::compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
            &root_source,
            &target_units,
            current.target_profile,
            current.application.identity.clone(),
            target_schema_version,
            &[predecessor],
        )
        .map_err(|error| error.to_string())?
        .plan,
    );
    if target.persistence.schema_hash == current.persistence.schema_hash {
        return Err("schema migration evidence compiled an unchanged persistence schema".into());
    }

    let baseline = authoritative_state_evidence(runtime)?;
    let preview = runtime.preview_machine_plan(Arc::clone(&target))?;
    let preview_migration = preview
        .migration
        .as_ref()
        .ok_or("schema migration preview did not produce a migration")?;
    if preview_migration.source_schema_version != current.persistence.schema_version
        || preview.target_schema_version != target_schema_version
        || preview_migration.steps.is_empty()
    {
        return Err(
            "schema migration preview has incomplete source, target, or step evidence".into(),
        );
    }

    let activation = runtime.activate_machine_plan(target)?;
    let activated_migration = activation
        .migration
        .as_ref()
        .ok_or("schema migration activation did not apply a migration")?;
    if activation.target_schema_version != target_schema_version
        || activated_migration.steps.is_empty()
    {
        return Err("schema migration activation has incomplete version or step evidence".into());
    }
    let presented = present_runtime(runtime, view, product, host, columns)
        .await?
        .ok_or("schema migration activation did not present")?;
    emit_presented(observer, &presented);
    let activated = authoritative_state_evidence(runtime)?;
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
        runtime,
        &baseline,
        &activated,
        presented.key.clone(),
    );
    Ok(prepare_evidence_proof(
        "persistence-migrated",
        presented.key,
        view,
        host,
    ))
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
    view: &RetainedView,
    host: &NativeSurfaceHost,
    desired_width: u32,
    key: crate::observer::FrameEvidenceKey,
) -> Result<ResponsiveEvidenceState, Box<dyn std::error::Error + Send + Sync>> {
    let expected_actions = document_action_paths(view.frame());
    if expected_actions.is_empty() {
        return Err("responsive evidence found no public document actions".into());
    }
    let current = host.viewport().logical_size;
    let current_width = current.width.round().max(0.0) as u32;
    let current_height = current.height.round().max(0.0) as u32;
    if !(320..=2_160).contains(&current_height) {
        return Err("responsive evidence has an unsupported tiled height".into());
    }
    emit(
        observer,
        ObserverEvent::ResponsiveResizeReady {
            desired_width,
            desired_height: current_height,
            current_width,
            current_height,
            key: key.clone(),
        },
    );
    Ok(ResponsiveEvidenceState {
        desired_width,
        desired_height: current_height,
        expected_actions,
        last_surface_epoch: key.surface_epoch,
        complete: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn capture_responsive_layout_evidence(
    observer: &Option<ObserverClient>,
    source_revision: u64,
    runtime: &mut RuntimeView,
    view: &RetainedView,
    host: &NativeSurfaceHost,
    evidence: &ResponsiveEvidenceState,
    resize_sequence: u64,
    key: crate::observer::FrameEvidenceKey,
) -> Result<PreparedProofRequest, Box<dyn std::error::Error + Send + Sync>> {
    let observed_actions = document_action_paths(view.frame());
    if observed_actions != evidence.expected_actions {
        return Err("narrow layout does not expose the same public actions".into());
    }
    let mut bounded_actions = BTreeSet::new();
    for node in view
        .frame()
        .nodes
        .values()
        .filter(|node| !node.source_bindings.is_empty())
    {
        let Some(bounds) = view.node_bounds(&node.id.0) else {
            continue;
        };
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || bounds.width <= 0.0
            || bounds.height <= 0.0
            || bounds.x < -0.5
            || bounds.y < -0.5
            || bounds.x + bounds.width > evidence.desired_width as f32 + 0.5
            || bounds.y + bounds.height > evidence.desired_height as f32 + 0.5
        {
            return Err(format!(
                "narrow action `{}` has invalid bounds ({}, {}, {}, {})",
                node.id.0, bounds.x, bounds.y, bounds.width, bounds.height
            )
            .into());
        }
        bounded_actions.extend(
            node.source_bindings
                .iter()
                .map(|binding| binding.source_path.clone()),
        );
    }
    if bounded_actions.is_empty() {
        return Err("narrow layout rendered no bound controls".into());
    }
    let state = authoritative_state_evidence(runtime)?;
    let mut action_hasher = Sha256::new();
    for action in &observed_actions {
        action_hasher.update((action.len() as u64).to_le_bytes());
        action_hasher.update(action.as_bytes());
    }
    emit(
        observer,
        ObserverEvent::ResponsiveLayoutEvidence {
            resize_sequence,
            logical_width: evidence.desired_width,
            logical_height: evidence.desired_height,
            action_count: observed_actions.len().try_into().unwrap_or(u32::MAX),
            action_digest: format!("{:x}", action_hasher.finalize()),
            state_digest: state.digest,
            source_revision,
            runtime_sequence: runtime.runtime_turn_sequence(),
            durable_epoch: state.durable_epoch,
            key: key.clone(),
        },
    );
    Ok(prepare_evidence_proof("responsive-narrow", key, view, host))
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
    let proof = prepare_evidence_proof("stale-program-rejected", frame.key, view, host);

    runtime.activate_state_artifact(&baseline)?;
    settle_test_runtime(runtime, view, columns)?;
    present_runtime(runtime, view, product, host, columns).await?;
    Ok(proof)
}

fn document_action_paths(frame: &boon_document::DocumentFrame) -> BTreeSet<String> {
    frame
        .nodes
        .values()
        .flat_map(|node| node.source_bindings.iter())
        .map(|binding| binding.source_path.clone())
        .collect()
}

fn queue_evidence_proofs(
    observer: &Option<ObserverClient>,
    proof: Option<&ProofWorker>,
    queued: &mut VecDeque<PreparedProofRequest>,
    in_flight: &mut Option<crate::observer::FrameEvidenceKey>,
    requests: impl IntoIterator<Item = PreparedProofRequest>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    queued.extend(requests);
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
                2,
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
                    view,
                    host,
                )?);
            }
            completed += 1;
            continue;
        }
        runtime.begin_scenario_step(&step.source_path);
        let sequence_before = runtime.event_sequence();
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
            3,
        )
        .await?;

        let pointer_cycles = usize::from(
            step.action_kind.as_deref() == Some("double_click")
                || target.source_intent.as_deref() == Some("double_click"),
        ) + 1;
        for _ in 0..pointer_cycles {
            for (phase, playback_phase, dwell_frames) in [
                (PointerPhase::Down, TestPointerPhase::Down, 2),
                (PointerPhase::Up, TestPointerPhase::Up, 3),
            ] {
                let event = HostEvent::Pointer(PointerEvent {
                    surface: surface.clone(),
                    x: target_point.0,
                    y: target_point.1,
                    phase,
                    button: Some(PointerButton::Primary),
                });
                let changed = runtime.handle_event(&event, Some(final_target.clone()))?;
                sync_sensitive_input_focus(runtime, host)?;
                if phase == PointerPhase::Down
                    && runtime.focused() != Some(final_target.node.as_str())
                {
                    return Err(
                        format!("TEST pointer down did not focus `{}`", final_target.node).into(),
                    );
                }
                if changed {
                    apply_runtime_update(runtime, view, columns)?;
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
            dirty |= runtime.handle_event(&event, None)?;
        }
        if let Some(key) = &step.key {
            let event = HostEvent::Keyboard(KeyEvent {
                surface: surface.clone(),
                physical_key: None,
                logical_key: LogicalKey::Named(key.clone()),
                pressed: true,
            });
            dirty |= runtime.handle_event(&event, None)?;
        }
        if step.action_kind.as_deref() == Some("blur")
            || target.source_intent.as_deref() == Some("blur")
        {
            dirty |= runtime.handle_event(
                &HostEvent::Focus {
                    surface: surface.clone(),
                    focused: false,
                },
                None,
            )?;
            sync_sensitive_input_focus(runtime, host)?;
        }
        if runtime.event_sequence() == sequence_before
            || runtime.last_dispatched_source() != Some(step.source_path.as_str())
        {
            return Err(format!(
                "TEST host events did not dispatch declared source `{}` (intent {:?}); last dispatch was {:?}",
                step.source_path,
                target.source_intent,
                runtime.last_dispatched_source()
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
            4,
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
                view,
                host,
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
    Ok(TestRunOutcome {
        completed_steps: completed,
        semantic_assertions_proven: semantic_expectation_count > 0,
        proof_requests,
        last_key: last_state_key.ok_or("TEST completed without a presented state frame")?,
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(benchmark.phase, ProductProfilePhase::Complete) {
        return Ok(());
    }
    if envelope.origin != HostEventOrigin::RealOs
        || runtime.focused() != Some(benchmark.target_node.as_str())
        || runtime.last_dispatched_source() != Some(benchmark.source_path.as_str())
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
        if key.pressed
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
    view: &RetainedView,
    host: &NativeSurfaceHost,
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
            let proof_request = (ordinal == 11).then(|| {
                prepare_evidence_proof("profile-benchmark", child_frame.key.clone(), view, host)
            });
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !profile_candidate_is_ready(benchmark) {
        return Ok(());
    }
    let finalized =
        finalize_profile_candidate(observer, source_revision, runtime, benchmark, view, host)?;
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
    for round in 0..TEST_SETTLE_ROUND_LIMIT {
        if started.elapsed() > TEST_SETTLE_TIMEOUT {
            return Err(format!(
                "TEST runtime did not settle within {}ms after {round} rounds",
                TEST_SETTLE_TIMEOUT.as_millis()
            )
            .into());
        }

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
        let artifact_store_pending = runtime.has_pending_program_artifact_store();
        if !had_program_requests
            && !artifact_store_pending
            && runtime.effect_poll_deadline().is_none()
        {
            return Ok(());
        }
        if artifact_store_pending {
            thread::sleep(Duration::from_millis(1));
        }
    }
    Err(format!(
        "TEST runtime exceeded its {TEST_SETTLE_ROUND_LIMIT}-round child/effect settle limit"
    )
    .into())
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
    let document = view.apply_patches(patches, columns)?;
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
            view.wheel_target(wheel.x, wheel.y, wheel.delta_x, wheel.delta_y)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_state_artifact_envelope_rejects_payload_corruption() {
        let mut artifact = canonical_state_artifact(7, vec![0x81, 0x01]);
        artifact.bytes[1] ^= 0x5a;
        assert!(validate_state_artifact_digest(&artifact).is_err());
    }

    fn counter_migration() -> (crate::catalog::LoadedExample, MigrationBundle) {
        let mut example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter_migration")
            .unwrap();
        let migration = example.migration.take().expect("migration bundle");
        (example, migration)
    }

    #[test]
    fn test_cursor_path_moves_smoothly_and_finishes_on_the_hit_target() {
        let path = test_cursor_path((24.0, 24.0), (384.0, 264.0));
        assert!(path.len() > 2);
        assert!(path.len() <= TEST_CURSOR_MAX_MOVE_FRAMES);
        assert_eq!(path.last().copied(), Some((384.0, 264.0)));
        assert!(path.windows(2).all(|pair| {
            pair[0].0 <= pair[1].0 && pair[0].1 <= pair[1].1 && pair[0] != pair[1]
        }));
        assert_eq!(test_cursor_path((80.0, 40.0), (80.0, 40.0)), [(80.0, 40.0)]);
    }

    fn profile_test_frame(
        frame_id: u64,
        event_sequence: Option<u64>,
        input_kind: Option<crate::observer::InputKind>,
    ) -> PresentedFrame {
        PresentedFrame {
            key: crate::observer::FrameEvidenceKey {
                surface_id: "test-preview".to_owned(),
                process_id: 1,
                session_id: "test-session".to_owned(),
                frame_id,
                input_id: frame_id,
                content_id: frame_id,
                layout_id: frame_id,
                render_id: frame_id,
                surface_epoch: 1,
                present_id: frame_id,
                proof_id: frame_id,
            },
            event_sequence,
            input_kind,
            callback_to_host_ns: 1,
            input_to_present_us: 1,
            event_dispatch_us: 1,
            executor_us: 1,
            runtime_document_us: 1,
            document_update_us: 1,
            render_us: 1,
            document_scene_convert_us: 1,
            scene_key_us: 1,
            rect_vertices_us: 1,
            asset_prepare_us: 1,
            quad_batch_key_us: 1,
            quad_upload_us: 1,
            draw_pass_us: 1,
            retained_metrics_us: 1,
            text_render_us: 1,
            submit_us: 1,
            present_us: 1,
            frame_us: 1,
        }
    }

    fn profile_test_benchmark(batch_text: &str, completed: bool) -> ProductProfileBenchmark {
        let request = SubmittedProgramRequest {
            session: ProgramSessionId("profile-child".to_owned()),
            request_id: ProgramRequestId("request-7".to_owned()),
            revision: 7,
            pending_depth: 1,
        };
        let mut completed_requests = BTreeSet::new();
        if completed {
            completed_requests.insert((request.session.0.clone(), request.request_id.0.clone()));
        }
        ProductProfileBenchmark {
            baseline: Vec::new(),
            source_path: "store.source".to_owned(),
            target_node: "editor".to_owned(),
            seed_text: "seed".to_owned(),
            sample_count: 1,
            completed_samples: 0,
            phase: ProductProfilePhase::Seed,
            candidate: Some(ProductProfileCandidate {
                batch_text: batch_text.to_owned(),
                input_sequence: 11,
                callback_to_host_ns: 1,
                accepted_at: Instant::now(),
                parent_generation_before: 1,
                parent_dispatch_us: 1,
                parent_phase: RuntimePhaseTimings::default(),
                update: RuntimeUpdateMeasurement::default(),
                requests: vec![request],
                closed: false,
                editor_frame: completed.then(|| {
                    profile_test_frame(1, Some(11), Some(crate::observer::InputKind::Text))
                }),
                editor_visible_us: completed.then_some(1),
                compile_us: 1,
                pending_depth: 1,
                completion_us: 1,
                completion_phase: RuntimePhaseTimings::default(),
                completion_update: RuntimeUpdateMeasurement::default(),
                completed_requests,
                invalid_completion: false,
                child_frame: completed.then(|| profile_test_frame(2, None, None)),
                preview_visible_us: completed.then_some(1),
            }),
        }
    }

    #[test]
    fn profile_batch_becomes_ready_when_marker_follows_completion() {
        let mut benchmark = profile_test_benchmark("seed", true);

        assert!(!product_proof_is_eligible(Some(&benchmark)));
        assert!(!profile_candidate_is_ready(&benchmark));
        close_profile_input_batch(&mut benchmark).unwrap();
        assert!(profile_candidate_is_ready(&benchmark));
        benchmark.phase = ProductProfilePhase::Complete;
        assert!(product_proof_is_eligible(Some(&benchmark)));
    }

    #[test]
    fn profile_open_batch_tolerates_an_invalid_intermediate_compile() {
        let mut intermediate = profile_test_benchmark("see", false);

        assert!(!record_profile_invalid_completion(&mut intermediate).unwrap());
        assert!(
            intermediate
                .candidate
                .as_ref()
                .is_some_and(|candidate| candidate.invalid_completion)
        );

        let mut final_candidate = profile_test_benchmark("seed", false);
        assert!(!record_profile_invalid_completion(&mut final_candidate).unwrap());
        assert!(
            close_profile_input_batch(&mut final_candidate)
                .unwrap_err()
                .to_string()
                .contains("invalid final child program")
        );
    }

    #[test]
    fn test_settle_completes_pending_child_program_requests() {
        let source = r#"
store: [
    child_program: [
        compiled: SOURCE
        rejected: SOURCE
    ]
]

child_source: "scene: Missing/constructor("

scene: Scene/Element/program(
    element: [event: store.child_program]
    style: [width: Fill, height: Fill]
    source: child_source
    revision: 1
    capability_profile: PublicDocument
    session_key: TEXT { test-child }
    mount: True
)
"#;
        let runtime = boon_runtime::LiveRuntime::from_source("test-child.bn", source).unwrap();
        let mut runtime = RuntimeView::open_in_memory(runtime).unwrap();
        let mut columns = boon_native_gpu::GlyphonRenderTextColumnMeasurer::new();
        let mut view = RetainedView::new(
            runtime.frame(),
            Viewport {
                surface: 1,
                width: 1_280.0,
                height: 800.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();

        settle_test_runtime(&mut runtime, &mut view, &mut columns).unwrap();

        assert!(runtime.take_program_requests().is_empty());
        let diagnostics = runtime.program_diagnostics();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].session.0, "test-child");
    }

    #[test]
    fn child_program_compiles_through_the_depth_one_worker() {
        let source = r#"
store: [
    child_source: "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT { Child })\n"
    child_program: [
        compiled: SOURCE
        rejected: SOURCE
    ]
]

scene: Scene/Element/program(
    element: [event: store.child_program]
    style: [width: Fill, height: Fill]
    source: store.child_source
    revision: 1
    capability_profile: PublicDocument
    session_key: TEXT { test-child }
    mount: True
)
"#;
        let runtime = boon_runtime::LiveRuntime::from_source("worker-child.bn", source).unwrap();
        let mut runtime = RuntimeView::open_in_memory(runtime).unwrap();
        let (worker, mut results) = ProgramCompileWorker::start();

        let submitted = submit_program_requests(runtime.take_program_requests(), &worker);
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].pending_depth, 1);
        let outcome = futures::executor::block_on(results.next()).expect("worker outcome");
        assert_eq!(outcome.revision, submitted[0].revision);
        assert_eq!(outcome.pending_depth, 1);
        let changed = runtime
            .complete_program(&outcome.session, &outcome.request_id, outcome.result)
            .unwrap();

        let texts = runtime
            .frame()
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.clone()))
            .collect::<Vec<_>>();
        assert!(changed, "texts={texts:?}");
        assert!(texts.iter().any(|text| text == "Child"), "texts={texts:?}");
        assert!(runtime.take_program_requests().is_empty());
        assert!(runtime.program_diagnostics().is_empty());
    }

    #[test]
    fn test_semantic_assertions_use_current_runtime_state_and_fail_closed_without_a_turn() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("counter")
            .unwrap();
        let units = example
            .units
            .into_iter()
            .map(|unit| boon_runtime::RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect::<Vec<_>>();
        let runtime =
            boon_runtime::LiveRuntime::from_project("examples/counter.bn", &units).unwrap();
        let mut runtime = RuntimeView::open_in_memory(runtime).unwrap();
        let mut step = TestStep {
            id: "initial".to_owned(),
            source_path: "unused".to_owned(),
            action_kind: None,
            target_text: None,
            text: None,
            key: None,
            address: None,
            target_occurrence: None,
            pointer_x: None,
            pointer_y: None,
            pointer_width: None,
            pointer_height: None,
            expectations: vec![boon_runtime::ScenarioExpectation::RootText {
                name: "store.count".to_owned(),
                value: "0".to_owned(),
            }],
        };

        assert_eq!(assert_test_step_semantics(&mut runtime, &step).unwrap(), 1);

        step.expectations = vec![boon_runtime::ScenarioExpectation::RootText {
            name: "store.count".to_owned(),
            value: "1".to_owned(),
        }];
        assert!(assert_test_step_semantics(&mut runtime, &step).is_err());

        step.expectations = vec![boon_runtime::ScenarioExpectation::DocumentChanged];
        let error = assert_test_step_semantics(&mut runtime, &step).unwrap_err();
        assert!(error.contains("requires a source event"), "{error}");
    }

    #[test]
    fn same_identity_schema_change_requires_explicit_migration_activation() {
        let (example, migration) = counter_migration();
        let initial =
            compile_migration_stage(&example.application, &migration, &migration.initial_stage)
                .unwrap();
        let runtime = boon_runtime::LiveRuntime::from_shared_machine_plan(
            initial,
            boon_runtime::SessionOptions::default(),
        )
        .unwrap();
        let mut runtime = Some(RuntimeView::open_in_memory(runtime).unwrap());
        let before = runtime.as_ref().unwrap().persistence_schema_version();
        let target = compile_migration_stage(&example.application, &migration, "v2").unwrap();

        let error = match activate_compatible(&mut runtime, target, false) {
            Err(error) => error,
            Ok(_) => panic!("schema-changing source reload activated implicitly"),
        };
        assert!(error.contains("requires Migration Preview and Activate"));
        assert_eq!(
            runtime.as_ref().unwrap().persistence_schema_version(),
            before
        );
    }

    #[test]
    fn migration_test_uses_the_generic_runner_with_temporary_namespaces() {
        let (example, migration) = counter_migration();
        let completed = run_migration_test(&migration, &example.application, 41, 3).unwrap();
        assert_eq!(completed, migration.scenario.steps.len());
    }

    #[test]
    fn migration_targets_are_forward_only() {
        let (_, migration) = counter_migration();
        assert_eq!(
            forward_migration_stage(&migration, "v1", "v3")
                .unwrap()
                .schema_version,
            3
        );
        assert!(forward_migration_stage(&migration, "v2", "v1").is_err());
        assert!(forward_migration_stage(&migration, "v2", "v2").is_err());
    }
}
