use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use boon_host::{
    HostEvent, HostEventEnvelope, HostEventOrigin, KeyEvent, LogicalKey, PointerButton,
    PointerEvent, PointerPhase, TextInputEvent, Viewport,
};
use boon_native_app_window::{NativeRoleResult, NativeSurfaceHost};
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};

use crate::compile::{CompileRequest, CompileWorker, CompiledPreview, source_key};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, input_kind,
    pointer_button_pressed, role_message_frame,
};
use crate::observer::{InputAccepted, ObserverClient, ObserverEvent, ObserverRole};
use crate::proof::{ProofConfig, ProofRequest, ProofResult, ProofWorker};
use crate::protocol::{
    Connection, FrameMode, Message, PreviewIntent, PreviewStats, ProofMode, Role, TestStep,
};
use crate::runtime_view::RuntimeView;
use crate::view::{HitTarget, RetainedView};

pub(crate) const TEST_STEP_LIMIT: usize = 24;
const RUNTIME_VIEW_CACHE_LIMIT: usize = 8;
const OUTBOUND_QUEUE_DEPTH: usize = 8;
const STATS_INTERVAL: Duration = Duration::from_millis(100);

struct PreviewOutput {
    sender: Option<SyncSender<Message>>,
    error: Arc<Mutex<Option<String>>>,
    writer: Option<thread::JoinHandle<()>>,
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
    let mut runtime_cache = BTreeMap::<String, RuntimeView>::new();
    let mut desired_revision = 0u64;
    let mut source_revision = 0u64;
    let mut cursor = (24.0f32, 24.0f32);
    let mut switch_started = None::<(u64, Instant)>;
    let mut proof_eligible_ordinal = 0u64;
    let mut proof_requested = false;
    let mut last_stats_sent = None::<Instant>;

    loop {
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
            Compiled(Option<crate::compile::CompileOutcome>),
            Proof(Option<ProofResult>),
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
            pin_mut!(native, command, result, proof_result);
            select! {
                value = native => Wake::Native(value),
                value = command => Wake::Ipc(value),
                value = result => Wake::Compiled(value),
                value = proof_result => Wake::Proof(value),
            }
        };

        match wake {
            Wake::Native(event) => {
                let mut transaction = NativeFrameTransaction::default();
                for accepted in drain_native_events(&mut host, event).await? {
                    let envelope = &accepted.envelope;
                    if matches!(envelope.event, HostEvent::CloseRequested { .. }) {
                        observe_input(&observer, envelope, None, None, false);
                        let _ = output.send(Message::Shutdown);
                        return Ok(());
                    }
                    let target = event_target(&view, &envelope.event);
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
                        let changed = model.handle_event(&envelope.event, target)?;
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
                        source_revision,
                        FrameMode::Burst,
                        compiler.replaced_count(),
                        &mut last_stats_sent,
                        false,
                    )?;
                }
            }
            Wake::Ipc(message) => {
                let message = message.ok_or("desktop IPC reader stopped")??;
                match message {
                    Message::PreviewApply {
                        intent,
                        request_id,
                        revision,
                        units,
                        test_steps,
                    } => {
                        let accepted_at = Instant::now();
                        desired_revision = desired_revision.max(revision);
                        let key = source_key(&units);
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
                            );
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: true,
                                message: "active mounted runtime retained".to_owned(),
                            })?;
                            send_stats(
                                &output,
                                &product,
                                source_revision,
                                FrameMode::Idle,
                                compiler.replaced_count(),
                                &mut last_stats_sent,
                                true,
                            )?;
                            continue;
                        }
                        if intent == PreviewIntent::Replace
                            && let Some(cached) = runtime_cache.remove(&key)
                        {
                            source_revision = revision;
                            let presented = install_runtime(
                                cached,
                                key,
                                &mut runtime,
                                &mut runtime_key,
                                &mut runtime_cache,
                                &mut view,
                                &mut product,
                                &mut host,
                                &mut columns,
                            )
                            .await?
                            .ok_or("cached preview runtime did not produce a frame")?;
                            emit_presented(&observer, &presented);
                            emit_switch_final(
                                &observer,
                                &mut switch_started,
                                source_revision,
                                &presented,
                            );
                            output.send(Message::PreviewStatus {
                                revision,
                                ok: true,
                                message: "mounted runtime restored from bounded cache".to_owned(),
                            })?;
                            send_stats(
                                &output,
                                &product,
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
                            units,
                            test_steps,
                        });
                        output.send(Message::PreviewStatus {
                            revision,
                            ok: true,
                            message: "source accepted by latest-wins compiler".to_owned(),
                        })?;
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
                        source_revision = compiled_preview.revision;
                        let compile_elapsed_ms = compiled_preview.elapsed.as_secs_f64() * 1_000.0;
                        let test = compiled_preview.intent == PreviewIntent::Test;
                        let request_id = compiled_preview.request_id.unwrap_or(0);
                        let steps = compiled_preview.test_steps.clone();
                        let key = compiled_preview.source_key.clone();
                        match activate(compiled_preview) {
                            Ok(model) => {
                                let presented = install_runtime(
                                    model,
                                    key,
                                    &mut runtime,
                                    &mut runtime_key,
                                    &mut runtime_cache,
                                    &mut view,
                                    &mut product,
                                    &mut host,
                                    &mut columns,
                                )
                                .await?;
                                if let Some(presented) = &presented {
                                    emit_presented(&observer, presented);
                                    emit_switch_final(
                                        &observer,
                                        &mut switch_started,
                                        source_revision,
                                        presented,
                                    );
                                }
                                output.send(Message::PreviewStatus {
                                    revision: source_revision,
                                    ok: true,
                                    message: format!(
                                        "typed runtime and retained document mounted in {compile_elapsed_ms:.2}ms"
                                    ),
                                })?;
                                if test {
                                    let result = run_test(
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
                                }
                                send_stats(
                                    &output,
                                    &product,
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
        }
    }
}

fn activate(compiled: CompiledPreview) -> Result<RuntimeView, String> {
    RuntimeView::mount(compiled.runtime, compiled.mount).map_err(|error| error.to_string())
}

async fn install_runtime(
    next: RuntimeView,
    key: String,
    runtime: &mut Option<RuntimeView>,
    runtime_key: &mut Option<String>,
    cache: &mut BTreeMap<String, RuntimeView>,
    view: &mut RetainedView,
    product: &mut ProductFrame,
    host: &mut NativeSurfaceHost,
    columns: &mut boon_native_gpu::GlyphonRenderTextColumnMeasurer,
) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
    if let (Some(active_key), Some(active)) = (runtime_key.take(), runtime.take()) {
        if cache.len() >= RUNTIME_VIEW_CACHE_LIMIT && !cache.contains_key(&active_key) {
            let evicted = cache.keys().next().cloned();
            if let Some(evicted) = evicted {
                cache.remove(&evicted);
            }
        }
        cache.insert(active_key, active);
    }
    *runtime = Some(next);
    *runtime_key = Some(key);
    view.replace(
        runtime.as_ref().expect("installed runtime").frame(),
        viewport(host),
        columns,
    )?;
    converge_document_demands(runtime.as_mut().expect("installed runtime"), view, columns)?;
    product.present(host, view).await
}

fn emit_switch_final(
    observer: &Option<ObserverClient>,
    switch_started: &mut Option<(u64, Instant)>,
    revision: u64,
    presented: &PresentedFrame,
) {
    if let Some((pending_revision, started)) = *switch_started
        && pending_revision == revision
    {
        emit(
            observer,
            ObserverEvent::SourceSwitchFinal {
                revision,
                elapsed_us: duration_us(started.elapsed()),
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

async fn run_test(
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
    for step in steps.iter().take(TEST_STEP_LIMIT) {
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
        cursor.0 = target.center_x;
        cursor.1 = target.center_y;
        let mut dirty = false;
        let pointer_cycles = usize::from(
            step.action_kind.as_deref() == Some("double_click")
                || target.source_intent.as_deref() == Some("double_click"),
        ) + 1;
        for _ in 0..pointer_cycles {
            for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                let event = HostEvent::Pointer(PointerEvent {
                    surface: surface.clone(),
                    x: target.center_x,
                    y: target.center_y,
                    phase,
                    button: if phase == PointerPhase::Move {
                        None
                    } else {
                        Some(PointerButton::Primary)
                    },
                });
                dirty |= runtime.handle_event(&event, Some(target.clone()))?;
            }
        }
        if let Some(text) = &step.text {
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
        let _ = product
            .present_cursor(host, view, cursor.0, cursor.1)
            .await?;
        thread::sleep(Duration::from_millis(16));
        completed += 1;
    }
    Ok(completed)
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

fn event_target(view: &RetainedView, event: &HostEvent) -> Option<HitTarget> {
    match event {
        HostEvent::Pointer(pointer) => view.hit_target(pointer.x, pointer.y),
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

fn send_stats(
    output: &PreviewOutput,
    product: &ProductFrame,
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
