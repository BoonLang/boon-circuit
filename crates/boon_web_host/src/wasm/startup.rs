use super::{
    BrowserClientEffectHost, BrowserDistributedSessionSocket, BrowserFetchAdapter,
    BrowserInputBindings, CanvasFrameDisposition, SemanticDomEvent, SemanticDomProjector,
    WebGpuCanvasHost, request_animation_frame, window,
};
use crate::sensitive_input::BrowserSensitiveInputVault;
use crate::{
    BrowserAppStartup, BrowserDocumentHostConfig, BrowserDocumentHostCore, BrowserDocumentRuntime,
    BrowserFetchCapabilities, BrowserFetchCapability, BrowserFetchRequest, BrowserHostEvent,
    BrowserLifecycleEvent, BrowserWebSocketCapabilities, BrowserWebSocketCapability,
    BrowserWebSocketRequest, DistributedSessionSocketLimits, FetchMethod, WebGpuFrameIdentity,
    WebHostError, WebHostResult, decode_browser_app_config,
};
use boon_app_package::{BrowserAppConfig, MAX_BROWSER_APP_CONFIG_BYTES};
use boon_document::SemanticWebInputEvent;
use boon_host::{SemanticId, SensitiveInputHandle, SurfaceId, Viewport, WindowId};
use boon_native_gpu::GlyphonRenderTextColumnMeasurer;
use boon_runtime::DistributedClientUpdate;
use boon_wire::DISTRIBUTED_SESSION_TRANSPORT_PATH;
use js_sys::Uint8Array;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlCanvasElement;

const BOOTSTRAP_FETCH_CAPABILITY: &str = "boon-browser-bootstrap";
const DISTRIBUTED_SESSION_SOCKET_CAPABILITY: &str = "boon-distributed-session";
const RECONNECT_INITIAL_DELAY_MS: u32 = 100;
const RECONNECT_MAX_DELAY_MS: u32 = 5_000;
const RECONNECT_WINDOW_MS: f64 = 60_000.0;
const BROWSER_SURFACE_ID: &str = "boon-browser-surface";
const BROWSER_WINDOW_ID: &str = "boon-browser-window";
const MAX_BROWSER_INPUT_TEXT_BYTES: usize = 64 * 1024;
const MAX_PENDING_SEMANTIC_EVENTS: usize = 1_024;
const MAX_EFFECT_COMPLETIONS_PER_PUMP: usize = 1_024;

enum BrowserWasmStartupState {
    Idle,
    Starting,
    Started(Box<ActiveBrowserApp>),
}

enum PendingSemanticEvent {
    Public(SemanticDomEvent),
    SensitiveInput {
        semantic_id: SemanticId,
        handle: SensitiveInputHandle,
    },
}

struct ActiveBrowserApp {
    config: BrowserAppConfig,
    session: BrowserDistributedSessionSocket,
    effects: BrowserClientEffectHost,
    document: BrowserDocumentRuntime,
    columns: GlyphonRenderTextColumnMeasurer,
    host: BrowserDocumentHostCore,
    canvas: WebGpuCanvasHost,
    semantics: SemanticDomProjector,
    sensitive_inputs: BrowserSensitiveInputVault,
    _input: BrowserInputBindings,
    semantic_events: VecDeque<PendingSemanticEvent>,
    host_pump_scheduled: bool,
    animation_callback: Closure<dyn FnMut(f64)>,
    last_input_event_sequence: Option<u64>,
    network_pump_scheduled: bool,
    terminal_error: Option<String>,
    reconnect_timer_id: Option<i32>,
    reconnect_callback: Closure<dyn FnMut()>,
    reconnect_attempt: u32,
    reconnect_deadline_ms: Option<f64>,
}

thread_local! {
    static STARTUP_STATE: RefCell<BrowserWasmStartupState> =
        const { RefCell::new(BrowserWasmStartupState::Idle) };
}

/// Consume the package-generated CBOR bootstrap, verify the bounded public
/// Client artifact, and mount exactly one browser Client session.
#[wasm_bindgen]
pub async fn start_boon_app(config_bytes: Uint8Array) -> Result<(), JsValue> {
    begin_startup().map_err(js_startup_error)?;
    let result = start_boon_app_inner(config_bytes).await;
    match result {
        Ok(active) => {
            STARTUP_STATE.with(|state| {
                *state.borrow_mut() = BrowserWasmStartupState::Started(Box::new(active));
            });
            schedule_browser_network_pump();
            schedule_browser_animation_frame();
            Ok(())
        }
        Err(error) => {
            STARTUP_STATE.with(|state| {
                *state.borrow_mut() = BrowserWasmStartupState::Idle;
            });
            Err(js_startup_error(error))
        }
    }
}

async fn start_boon_app_inner(config_bytes: Uint8Array) -> WebHostResult<ActiveBrowserApp> {
    let config_len = usize::try_from(config_bytes.length()).unwrap_or(usize::MAX);
    if config_len == 0 || config_len > MAX_BROWSER_APP_CONFIG_BYTES {
        return Err(WebHostError::LimitExceeded {
            resource: "browser app config".to_owned(),
            limit: MAX_BROWSER_APP_CONFIG_BYTES,
        });
    }
    let config = decode_browser_app_config(&config_bytes.to_vec())?;
    let canvas = browser_canvas(&config)?;
    let artifact_bytes = fetch_client_artifact(&config).await?;
    let startup = BrowserAppStartup::from_artifact_bytes(config, artifact_bytes)?;
    let (config, identity, runtime, effect_contracts) = startup.into_distributed_parts();
    let authoritative_frame =
        runtime
            .document_frame()
            .cloned()
            .ok_or_else(|| WebHostError::InvalidInput {
                field: "browser client artifact".to_owned(),
                reason: "distributed Client did not mount an authoritative document frame"
                    .to_owned(),
            })?;
    let surface = SurfaceId(BROWSER_SURFACE_ID.to_owned());
    let window_id = WindowId(BROWSER_WINDOW_ID.to_owned());
    let viewport = browser_viewport(&canvas)?;
    let mut columns = GlyphonRenderTextColumnMeasurer::new();
    let mut document = BrowserDocumentRuntime::new(authoritative_frame, viewport, &mut columns)
        .map_err(|error| {
            WebHostError::platform(
                "mount authoritative browser document frame",
                error.to_string(),
            )
        })?;
    let browser_document = window()?
        .document()
        .ok_or_else(|| WebHostError::unsupported("Document", "window has no document"))?;
    document.set_visible(browser_document.visibility_state() == web_sys::VisibilityState::Visible);
    let canvas_host = WebGpuCanvasHost::acquire(canvas.clone()).await?;
    let semantic_parent = canvas
        .parent_element()
        .ok_or_else(|| WebHostError::InvalidInput {
            field: "browser canvas_id".to_owned(),
            reason: "canvas must be mounted before browser startup".to_owned(),
        })?;
    let semantic_sink: Rc<dyn Fn(SemanticDomEvent)> = Rc::new(|event| {
        spawn_local(async move {
            enqueue_browser_semantic_event(event);
        });
    });
    let mut semantics = SemanticDomProjector::mount(
        &semantic_parent,
        MAX_BROWSER_INPUT_TEXT_BYTES,
        semantic_sink,
    )?;
    semantics.apply(document.semantic_bridge())?;
    let host = BrowserDocumentHostCore::new(BrowserDocumentHostConfig::default())?;
    let input_sink: Rc<dyn Fn(BrowserHostEvent)> = Rc::new(|event| {
        spawn_local(async move {
            enqueue_browser_host_event(event);
        });
    });
    let input = BrowserInputBindings::install(
        canvas.clone(),
        surface,
        window_id,
        MAX_BROWSER_INPUT_TEXT_BYTES,
        input_sink,
    )?;
    let limits = DistributedSessionSocketLimits::default();
    let mut capability = BrowserWebSocketCapability::same_origin(
        DISTRIBUTED_SESSION_SOCKET_CAPABILITY,
        DISTRIBUTED_SESSION_TRANSPORT_PATH,
    );
    capability.max_url_bytes = DISTRIBUTED_SESSION_TRANSPORT_PATH.len();
    capability.max_message_bytes = limits.max_frame_bytes;
    capability.max_queue_messages = limits.max_inbound_messages;
    capability.max_queue_bytes = limits.max_inbound_bytes;
    let capabilities = BrowserWebSocketCapabilities::new([capability])?;
    let request = BrowserWebSocketRequest {
        connection_id: 1,
        capability: DISTRIBUTED_SESSION_SOCKET_CAPABILITY.to_owned(),
        path_and_query: DISTRIBUTED_SESSION_TRANSPORT_PATH.to_owned(),
        protocols: Vec::new(),
    };
    let event_wake: Rc<dyn Fn()> = Rc::new(schedule_browser_network_pump);
    let effects = BrowserClientEffectHost::new(
        &config.client_capability_profile,
        &effect_contracts,
        Rc::clone(&event_wake),
    )?;
    let session = BrowserDistributedSessionSocket::connect_with_event_wake(
        capabilities,
        request,
        identity,
        runtime,
        limits,
        event_wake,
    )
    .map_err(|error| {
        WebHostError::platform("start distributed browser Session", error.to_string())
    })?;
    let reconnect_callback = Closure::wrap(Box::new(attempt_browser_reconnect) as Box<dyn FnMut()>);
    let animation_callback =
        Closure::wrap(Box::new(run_browser_animation_frame) as Box<dyn FnMut(f64)>);
    Ok(ActiveBrowserApp {
        config,
        session,
        effects,
        document,
        columns,
        host,
        canvas: canvas_host,
        semantics,
        sensitive_inputs: BrowserSensitiveInputVault::default(),
        _input: input,
        semantic_events: VecDeque::new(),
        host_pump_scheduled: false,
        animation_callback,
        last_input_event_sequence: None,
        network_pump_scheduled: false,
        terminal_error: None,
        reconnect_timer_id: None,
        reconnect_callback,
        reconnect_attempt: 0,
        reconnect_deadline_ms: None,
    })
}

impl ActiveBrowserApp {
    fn consume_runtime_updates(
        &mut self,
        updates: impl IntoIterator<Item = DistributedClientUpdate>,
    ) -> WebHostResult<bool> {
        let mut pending = updates.into_iter().collect::<VecDeque<_>>();
        let mut request_animation_frame = false;
        let mut completion_count = 0;
        loop {
            while let Some(update) = pending.pop_front() {
                let turns = update.turns;
                if turns.is_empty() {
                    continue;
                }
                self.effects.route_turns(&turns)?;
                let authoritative_frame =
                    self.session.document_frame().cloned().ok_or_else(|| {
                        WebHostError::platform(
                            "consume distributed Client update",
                            "Client no longer exposes an authoritative document frame",
                        )
                    })?;
                let update = self
                    .document
                    .consume_turns_and_verify(
                        &turns,
                        &authoritative_frame,
                        browser_now_ms(),
                        &mut self.columns,
                    )
                    .map_err(|error| {
                        WebHostError::platform(
                            "apply distributed browser document turns",
                            error.to_string(),
                        )
                    })?;
                if update.semantic.is_some() {
                    self.semantics.apply(self.document.semantic_bridge())?;
                    self.sensitive_inputs
                        .retain(|id| self.document.semantic_scene().nodes.contains_key(id));
                }
                request_animation_frame |= update.scheduling.request_animation_frame;
            }

            let Some(completion) = self.effects.try_completion()? else {
                break;
            };
            completion_count += 1;
            if completion_count > MAX_EFFECT_COMPLETIONS_PER_PUMP {
                return Err(WebHostError::QueueOverflow {
                    queue: "browser effect completions per pump".to_owned(),
                    capacity: MAX_EFFECT_COMPLETIONS_PER_PUMP,
                });
            }
            let update = self
                .session
                .complete_transient_effect(completion.call_id, completion.outcome)
                .map_err(|error| {
                    WebHostError::platform("complete browser Client effect", error.to_string())
                })?;
            pending.push_back(update);
        }
        Ok(request_animation_frame)
    }

    fn dispatch_source(
        &mut self,
        source_path: String,
        payload: boon_runtime::SourcePayload,
    ) -> WebHostResult<bool> {
        let update = self
            .session
            .dispatch(&source_path, payload)
            .map_err(|error| {
                WebHostError::platform("dispatch browser HostEvent to Client", error.to_string())
            })?;
        self.consume_runtime_updates([update])
    }

    fn dispatch_document_source(
        &mut self,
        dispatch: crate::BrowserDocumentSourceDispatch,
    ) -> WebHostResult<bool> {
        if let Some(handle) = dispatch.sensitive_input {
            let semantic_id = SemanticId::from_document_node_id(&dispatch.node);
            if dispatch.payload != boon_runtime::SourcePayload::default()
                || !self.sensitive_inputs.owns(&semantic_id, handle)
            {
                return Err(WebHostError::InvalidInput {
                    field: "browser sensitive source dispatch".to_owned(),
                    reason: "sensitive source dispatch is not owned by this browser host"
                        .to_owned(),
                });
            }
        }
        self.dispatch_source(dispatch.source_path, dispatch.payload)
    }

    fn process_host_event(&mut self, event: BrowserHostEvent) -> WebHostResult<bool> {
        match event {
            BrowserHostEvent::Input { envelope } => {
                self.last_input_event_sequence = Some(envelope.sequence);
                let output = self
                    .document
                    .handle_host_event(&envelope.event, browser_now_ms(), &mut self.columns)
                    .map_err(|error| {
                        WebHostError::platform("apply browser HostEvent", error.to_string())
                    })?;
                if output.semantic.is_some() {
                    self.semantics.apply(self.document.semantic_bridge())?;
                    self.sensitive_inputs
                        .retain(|id| self.document.semantic_scene().nodes.contains_key(id));
                }
                let mut request_frame = output.scheduling.request_animation_frame;
                if let Some(dispatch) = output.dispatch {
                    request_frame |= self.dispatch_document_source(dispatch)?;
                }
                Ok(request_frame)
            }
            BrowserHostEvent::Lifecycle {
                event: BrowserLifecycleEvent::VisibilityChanged { visible },
            } => Ok(self.document.set_visible(visible)),
            BrowserHostEvent::Rejected { error } => Err(error),
            BrowserHostEvent::Gesture { .. }
            | BrowserHostEvent::Clipboard { .. }
            | BrowserHostEvent::Lifecycle { .. }
            | BrowserHostEvent::UrlChanged { .. } => Ok(false),
        }
    }

    fn process_semantic_event(&mut self, event: PendingSemanticEvent) -> WebHostResult<bool> {
        let event = match event {
            PendingSemanticEvent::SensitiveInput {
                semantic_id,
                handle,
            } => {
                if !self.sensitive_inputs.owns(&semantic_id, handle) {
                    return Err(WebHostError::InvalidInput {
                        field: "browser sensitive semantic input".to_owned(),
                        reason:
                            "sensitive input handle is stale or belongs to another browser host"
                                .to_owned(),
                    });
                }
                let Some(dispatch) = self
                    .document
                    .source_dispatch_for_sensitive_semantic_input(&semantic_id, handle)
                else {
                    return Ok(false);
                };
                return self.dispatch_document_source(dispatch);
            }
            PendingSemanticEvent::Public(event) => event,
        };
        let action = match event {
            SemanticDomEvent::Action(action) => action,
            SemanticDomEvent::Ime {
                semantic_id,
                kind: boon_host::ImeInputKind::Commit { text },
            } => SemanticWebInputEvent::SetText { semantic_id, text },
            SemanticDomEvent::Ime {
                semantic_id,
                kind: boon_host::ImeInputKind::Disabled,
            } => {
                self.sensitive_inputs.clear(&semantic_id);
                return Ok(false);
            }
            SemanticDomEvent::Ime { .. } => return Ok(false),
            SemanticDomEvent::SensitiveTextInput { .. } => {
                return Err(WebHostError::InvalidInput {
                    field: "browser sensitive semantic input".to_owned(),
                    reason: "cleartext sensitive input reached the queued semantic lane".to_owned(),
                });
            }
            SemanticDomEvent::Rejected { error } => return Err(error),
        };
        let Some(dispatch) = self.document.source_dispatch_for_semantic_web_event(action) else {
            return Ok(false);
        };
        self.dispatch_source(dispatch.source_path, dispatch.payload)
    }
}

fn enqueue_browser_host_event(event: BrowserHostEvent) {
    let should_spawn = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return false;
        };
        if active.terminal_error.is_some() {
            return false;
        }
        if let Err(error) = active.host.accept_event(event, browser_now_ms()) {
            active.terminal_error = Some(error.to_string());
            return false;
        }
        if active.host_pump_scheduled {
            return false;
        }
        active.host_pump_scheduled = true;
        true
    });
    if should_spawn {
        spawn_local(async {
            pump_browser_host_once();
        });
    }
}

fn enqueue_browser_semantic_event(event: SemanticDomEvent) {
    let should_spawn = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return false;
        };
        if active.terminal_error.is_some() {
            return false;
        }
        if active.semantic_events.len() >= MAX_PENDING_SEMANTIC_EVENTS {
            active.terminal_error = Some(
                WebHostError::QueueOverflow {
                    queue: "browser semantic events".to_owned(),
                    capacity: MAX_PENDING_SEMANTIC_EVENTS,
                }
                .to_string(),
            );
            return false;
        }
        let event = match event {
            SemanticDomEvent::SensitiveTextInput { semantic_id, text } => {
                let handle = match active.sensitive_inputs.replace(semantic_id.clone(), text) {
                    Ok(handle) => handle,
                    Err(error) => {
                        active.terminal_error = Some(error.to_string());
                        return false;
                    }
                };
                PendingSemanticEvent::SensitiveInput {
                    semantic_id,
                    handle,
                }
            }
            event => PendingSemanticEvent::Public(event),
        };
        active.semantic_events.push_back(event);
        if active.host_pump_scheduled {
            return false;
        }
        active.host_pump_scheduled = true;
        true
    });
    if should_spawn {
        spawn_local(async {
            pump_browser_host_once();
        });
    }
}

fn pump_browser_host_once() {
    let request_frame = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return false;
        };
        active.host_pump_scheduled = false;
        if active.terminal_error.is_some() {
            return false;
        }
        let host_events = active.host.drain_events().collect::<Vec<_>>();
        let semantic_events = active.semantic_events.drain(..).collect::<Vec<_>>();
        let mut request_frame = false;
        for event in host_events {
            match active.process_host_event(event) {
                Ok(request) => request_frame |= request,
                Err(error) => {
                    active.terminal_error = Some(error.to_string());
                    return false;
                }
            }
        }
        for event in semantic_events {
            match active.process_semantic_event(event) {
                Ok(request) => request_frame |= request,
                Err(error) => {
                    active.terminal_error = Some(error.to_string());
                    return false;
                }
            }
        }
        request_frame
    });
    if request_frame {
        schedule_browser_animation_frame();
    }
}

/// Network callbacks only enqueue bounded events and coalesce one microtask.
/// This pump is independent of requestAnimationFrame so hidden-tab throttling
/// cannot stall Session acknowledgements or reconnect state transitions.
fn schedule_browser_network_pump() {
    let should_spawn = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return false;
        };
        if active.network_pump_scheduled || active.terminal_error.is_some() {
            return false;
        }
        active.network_pump_scheduled = true;
        true
    });
    if should_spawn {
        spawn_local(async {
            pump_browser_network_once();
        });
    }
}

fn pump_browser_network_once() {
    let (reconnect, request_frame) = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return (false, false);
        };
        active.network_pump_scheduled = false;
        if active.terminal_error.is_some() {
            return (false, false);
        }
        match active.session.poll() {
            Ok(poll) => {
                let request_frame = match active.consume_runtime_updates(poll.runtime_updates) {
                    Ok(request_frame) => request_frame,
                    Err(error) => {
                        let _ = active.session.close();
                        active.terminal_error = Some(error.to_string());
                        return (false, false);
                    }
                };
                let reconnect = active.session.phase()
                    == crate::DistributedSessionSocketPhase::ReconnectRequired;
                if active.session.phase() == crate::DistributedSessionSocketPhase::Current {
                    active.reconnect_attempt = 0;
                    active.reconnect_deadline_ms = None;
                }
                (reconnect, request_frame)
            }
            Err(error) => {
                let _ = active.session.close();
                active.terminal_error = Some(error.to_string());
                (false, false)
            }
        }
    });
    if request_frame {
        schedule_browser_animation_frame();
    }
    if reconnect {
        schedule_browser_reconnect(RECONNECT_INITIAL_DELAY_MS);
    }
}

fn schedule_browser_animation_frame() {
    STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return;
        };
        if active.terminal_error.is_some() || !active.document.animation_frame_pending() {
            return;
        }
        if let Err(error) =
            request_animation_frame(active.animation_callback.as_ref().unchecked_ref())
        {
            active.terminal_error = Some(error.to_string());
        }
    });
}

fn run_browser_animation_frame(timestamp_ms: f64) {
    let schedule_next = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return false;
        };
        if active.terminal_error.is_some() {
            return false;
        }
        let start = active.document.begin_animation_frame();
        if !start.render {
            return false;
        }
        if let Err(error) = active.canvas.resize_to_display_size() {
            active.terminal_error = Some(error.to_string());
            return false;
        }
        let stats = active.document.stats();
        let identity = WebGpuFrameIdentity {
            surface_id: SurfaceId(BROWSER_SURFACE_ID.to_owned()),
            content_revision: stats.content_revision,
            layout_revision: stats.layout_revision,
            render_scene_revision: stats.render_revision,
            scene_identity: format!("browser-document:{}", active.config.package_id),
            input_event_seq: active.last_input_event_sequence,
            proof_request_id: None,
        };
        let result = active
            .canvas
            .render(active.document.render_scene(), &identity);
        let wants_retry = match result {
            Ok(frame) => match frame.disposition {
                CanvasFrameDisposition::Presented => false,
                CanvasFrameDisposition::PresentedSuboptimal
                | CanvasFrameDisposition::ReconfigureRequired => {
                    active.canvas.reconfigure();
                    true
                }
                CanvasFrameDisposition::SurfaceLost => {
                    if let Err(error) = active.canvas.recover_lost_surface() {
                        active.terminal_error = Some(error.to_string());
                        return false;
                    }
                    true
                }
                CanvasFrameDisposition::SkippedTimeout
                | CanvasFrameDisposition::SkippedOccluded => true,
                CanvasFrameDisposition::DeviceLost => {
                    let reason = active
                        .canvas
                        .device_lost_reason()
                        .unwrap_or_else(|| "browser WebGPU device was lost".to_owned());
                    active.terminal_error = Some(reason);
                    return false;
                }
            },
            Err(error) => {
                active.terminal_error = Some(error.to_string());
                return false;
            }
        };
        active
            .document
            .complete_animation_frame(timestamp_to_ms(timestamp_ms), false, wants_retry)
            .schedule_next_animation_frame
    });
    if schedule_next {
        schedule_browser_animation_frame();
    }
}

fn timestamp_to_ms(timestamp_ms: f64) -> u64 {
    if timestamp_ms.is_finite() && timestamp_ms > 0.0 {
        timestamp_ms.min(u64::MAX as f64) as u64
    } else {
        0
    }
}

fn browser_now_ms() -> u64 {
    window()
        .ok()
        .and_then(|window| window.performance())
        .map(|performance| timestamp_to_ms(performance.now()))
        .unwrap_or_default()
}

fn browser_viewport(canvas: &HtmlCanvasElement) -> WebHostResult<Viewport> {
    let scale = window()?.device_pixel_ratio();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(WebHostError::InvalidInput {
            field: "devicePixelRatio".to_owned(),
            reason: "must be finite and positive".to_owned(),
        });
    }
    Ok(Viewport {
        surface: 1,
        width: canvas.client_width().max(1) as f32,
        height: canvas.client_height().max(1) as f32,
        scale,
    })
}

fn schedule_browser_reconnect(delay_ms: u32) {
    STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return;
        };
        if active.terminal_error.is_some() || active.reconnect_timer_id.is_some() {
            return;
        }
        let now = js_sys::Date::now();
        let deadline = *active
            .reconnect_deadline_ms
            .get_or_insert(now + RECONNECT_WINDOW_MS);
        if now >= deadline {
            active.terminal_error = Some("distributed Session reconnect window expired".to_owned());
            return;
        }
        match window().and_then(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    active.reconnect_callback.as_ref().unchecked_ref(),
                    i32::try_from(delay_ms).unwrap_or(i32::MAX),
                )
                .map_err(|error| super::js_error("schedule distributed Session reconnect", error))
        }) {
            Ok(timer_id) => active.reconnect_timer_id = Some(timer_id),
            Err(error) => active.terminal_error = Some(error.to_string()),
        }
    });
}

fn attempt_browser_reconnect() {
    let retry_delay = STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let BrowserWasmStartupState::Started(active) = &mut *state else {
            return None;
        };
        active.reconnect_timer_id = None;
        if active.terminal_error.is_some()
            || active.session.phase() != crate::DistributedSessionSocketPhase::ReconnectRequired
        {
            return None;
        }
        if active
            .reconnect_deadline_ms
            .is_some_and(|deadline| js_sys::Date::now() >= deadline)
        {
            active.terminal_error = Some("distributed Session reconnect window expired".to_owned());
            return None;
        }
        match active.session.reconnect() {
            Ok(_) => {
                active.reconnect_attempt = active.reconnect_attempt.saturating_add(1);
                None
            }
            Err(_) => {
                active.reconnect_attempt = active.reconnect_attempt.saturating_add(1);
                let shift = active.reconnect_attempt.min(6);
                Some(
                    RECONNECT_INITIAL_DELAY_MS
                        .saturating_mul(1_u32 << shift)
                        .min(RECONNECT_MAX_DELAY_MS),
                )
            }
        }
    });
    if let Some(delay_ms) = retry_delay {
        schedule_browser_reconnect(delay_ms);
    }
}

fn begin_startup() -> WebHostResult<()> {
    STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        match &*state {
            BrowserWasmStartupState::Idle => {
                *state = BrowserWasmStartupState::Starting;
                Ok(())
            }
            BrowserWasmStartupState::Starting => Err(WebHostError::InvalidInput {
                field: "browser app startup".to_owned(),
                reason: "startup is already in progress".to_owned(),
            }),
            BrowserWasmStartupState::Started(active) => Err(WebHostError::InvalidInput {
                field: "browser app startup".to_owned(),
                reason: format!("package `{}` is already started", active.config.package_id),
            }),
        }
    })
}

fn browser_canvas(config: &BrowserAppConfig) -> WebHostResult<HtmlCanvasElement> {
    let document = window()?
        .document()
        .ok_or_else(|| WebHostError::unsupported("Document", "window has no document"))?;
    document
        .get_element_by_id(&config.canvas_id)
        .ok_or_else(|| WebHostError::InvalidInput {
            field: "browser canvas_id".to_owned(),
            reason: format!("element `{}` does not exist", config.canvas_id),
        })?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| WebHostError::InvalidInput {
            field: "browser canvas_id".to_owned(),
            reason: format!("element `{}` is not a canvas", config.canvas_id),
        })
}

async fn fetch_client_artifact(config: &BrowserAppConfig) -> WebHostResult<Vec<u8>> {
    let mut capability = BrowserFetchCapability::same_origin_api(
        BOOTSTRAP_FETCH_CAPABILITY,
        config.client_artifact_path.clone(),
    );
    capability.methods = [FetchMethod::Get].into_iter().collect();
    capability.request_headers.clear();
    capability.max_url_bytes = config.client_artifact_path.len();
    capability.max_request_bytes = 1;
    capability.max_response_bytes = config.client_artifact_bytes_len;
    let adapter = BrowserFetchAdapter::new(BrowserFetchCapabilities::new([capability])?, 1)?;
    let response = adapter
        .execute(BrowserFetchRequest {
            request_id: 1,
            capability: BOOTSTRAP_FETCH_CAPABILITY.to_owned(),
            method: FetchMethod::Get,
            path_and_query: config.client_artifact_path.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await?;
    if response.status != 200 {
        return Err(WebHostError::platform(
            "fetch browser client artifact",
            format!("server returned HTTP {}", response.status),
        ));
    }
    Ok(response.body)
}

fn js_startup_error(error: WebHostError) -> JsValue {
    JsValue::from_str(&error.to_string())
}
