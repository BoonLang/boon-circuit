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
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};
use sha2::{Digest, Sha256};

use boon_runtime::MigrationScenarioRunner;

use crate::compile::{
    CompileRequest, CompileWorker, compile_migration_stage, project_key_for_stage,
};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, input_kind,
    pointer_button_pressed, role_message_frame,
};
use crate::observer::{
    InputAccepted, ObserverClient, ObserverEvent, ObserverRole, TestPointerPhase,
};
use crate::proof::{ProofConfig, ProofRequest, ProofResult, ProofWorker};
use crate::protocol::{
    ApplicationIdentity, AssetBlob, CanonicalStateArtifact, Connection, FrameMode,
    MAX_PERSISTENCE_ARTIFACT_BYTES, Message, MigrationBundle, MigrationCommand, MigrationOperation,
    MigrationStatus, PersistenceCommand, PersistenceOperation, PersistenceOperationStatus,
    PreviewIntent, PreviewStats, ProofMode, Role, StateArtifactFormat, StateArtifactPreviewSummary,
    TestStep,
};
use crate::runtime_view::RuntimeView;
use crate::view::{HitTarget, RetainedView};

pub(crate) const TEST_STEP_LIMIT: usize = 24;
const OUTBOUND_QUEUE_DEPTH: usize = 8;
const STATS_INTERVAL: Duration = Duration::from_millis(100);
const TEST_CURSOR_FRAME: Duration = Duration::from_millis(16);
const TEST_CURSOR_PIXELS_PER_FRAME: f32 = 36.0;
const TEST_CURSOR_MAX_MOVE_FRAMES: usize = 12;

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
    if proof_config.is_some() && observer.is_none() {
        return Err("verifier proof mode requires the verifier observer channel".into());
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
    let mut last_stats_sent = None::<Instant>;
    let mut persistence_snapshot_sequence = 0u64;
    let mut last_persistence_operation = None::<PersistenceOperationStatus>;
    let mut import_preview = None::<CachedStateArtifactPreview>;
    let mut next_import_preview_id = 0u64;
    let mut deadline_scheduler = DeadlineScheduler::start()?;

    loop {
        deadline_scheduler.schedule(runtime.as_ref().and_then(|runtime| {
            [
                runtime.caret_blink_deadline(),
                runtime.scheduled_source_deadline(),
                runtime.persistence_poll_deadline(),
            ]
            .into_iter()
            .flatten()
            .min()
        }));
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
            Compiled(Option<crate::compile::CompileOutcome>),
            Proof(Option<Box<ProofResult>>),
            Scheduled(Option<()>),
        }
        let wake = {
            let native = host.next_event().fuse();
            let command = incoming.next().fuse();
            let result = compiled.next().fuse();
            let proof_result = async {
                match proof.as_mut() {
                    Some(worker) => worker.next_result().await,
                    None => futures::future::pending::<Option<ProofResult>>().await,
                }
            }
            .fuse();
            let scheduled = deadline_scheduler.ticks.next().fuse();
            pin_mut!(native, command, result, proof_result, scheduled);
            select! {
                value = native => Wake::Native(value),
                value = command => Wake::Ipc(value),
                value = result => Wake::Compiled(value),
                value = proof_result => Wake::Proof(value.map(Box::new)),
                value = scheduled => Wake::Scheduled(value),
            }
        };

        match wake {
            Wake::Native(event) => {
                let mut transaction = NativeFrameTransaction::default();
                let mut latest_runtime_sequence = None;
                let mut persistence_turn_changed = false;
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
                    let dirty = if matches!(envelope.event, HostEvent::Resize(_)) {
                        let started = Instant::now();
                        view.resize(viewport(&host), &mut columns)?;
                        document_update_us = duration_us(started.elapsed());
                        true
                    } else if let Some(model) = runtime.as_mut() {
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
                        if changed {
                            let started = Instant::now();
                            let changed = apply_runtime_update(model, &mut view, &mut columns)?;
                            document_update_us = duration_us(started.elapsed());
                            changed
                        } else {
                            false
                        }
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
                }
                if let Some(presented) = transaction.present(&mut product, &mut host, &view).await?
                {
                    proof_eligible_ordinal = proof_eligible_ordinal.saturating_add(1);
                    let proof_request = prepare_proof_request(
                        proof.as_ref(),
                        proof_config.as_ref(),
                        &mut proof_requested,
                        proof_eligible_ordinal,
                        &presented,
                        &view,
                        &host,
                    );
                    emit_presented(&observer, &presented);
                    submit_proof_request(&observer, proof.as_ref(), proof_request)?;
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
                            let (passed, completed, message) = match result {
                                Ok(count) => (
                                    true,
                                    count,
                                    format!(
                                        "{count} manifest migration lifecycle steps passed in temporary namespaces"
                                    ),
                                ),
                                Err(error) => (false, 0, error),
                            };
                            emit(
                                &observer,
                                ObserverEvent::TestCompleted {
                                    request_id,
                                    passed,
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
                        let revision = compiled_preview.revision;
                        let post_compile_started = Instant::now();
                        match activate_compatible(&mut runtime, compiled_preview.plan) {
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
                                    )
                                    .await;
                                    let (passed, completed, message) = match result {
                                        Ok(count) => (
                                            true,
                                            count,
                                            format!("{count} public HostEvent steps passed"),
                                        ),
                                        Err(error) => (false, 0, error.to_string()),
                                    };
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
            Wake::Proof(result) => {
                let result = result.ok_or("proof worker stopped")?;
                let worker = proof.as_ref().expect("proof result without worker");
                emit(
                    &observer,
                    result.observer_event(
                        product.frame_id(),
                        worker.replaced_count(),
                        worker.result_drop_count(),
                    ),
                );
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
                    let persistence_changed = runtime.poll_persistence_acknowledgement(now);
                    if timer_changed || caret_changed {
                        apply_runtime_update(runtime, &mut view, &mut columns)?;
                    }
                    if timer_changed || caret_changed {
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
                    if persistence_changed || runtime.runtime_turn_sequence() > runtime_turn_before
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
    RuntimeView::open(plan).map(|runtime| RuntimeActivation::Opened(Box::new(runtime)))
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

fn first_test_target(
    runtime: &RuntimeView,
    view: &RetainedView,
    steps: &[TestStep],
) -> Option<HitTarget> {
    steps.iter().find_map(|step| {
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
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    if steps.is_empty() {
        return Err("example scenario has no source-event steps".into());
    }
    let surface = host.ids().surface.clone();
    let mut completed = 0usize;
    for (step_index, step) in steps.iter().take(TEST_STEP_LIMIT).enumerate() {
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
        present_test_cursor_frame(
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
        output.send(Message::PreviewRuntimeChanged {
            revision: source_revision,
            runtime_sequence: runtime.event_sequence(),
        })?;
        completed += 1;
    }
    Ok(completed)
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
                    key: presented.key,
                },
            );
            thread::sleep(TEST_CURSOR_FRAME.saturating_mul(dwell_frames));
            return Ok(());
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
    let document = view.apply_patches(runtime.take_patches(), columns)?;
    let interaction = view.set_interaction_state(runtime.hovered(), runtime.focused(), columns)?;
    let demand_changed = converge_document_demands(runtime, view, columns)?;
    Ok(document.render_changed
        || document.layout_changed
        || interaction.render_changed
        || interaction.layout_changed
        || demand_changed)
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

        let error = match activate_compatible(&mut runtime, target) {
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
