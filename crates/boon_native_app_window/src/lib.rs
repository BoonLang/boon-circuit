use app_window::coordinates::{Position, Size};
use app_window::input::keyboard::{Keyboard, key::KeyboardKey};
use app_window::input::mouse::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT, Mouse};
use app_window::window::Window;
use app_window::{WGPU_SURFACE_STRATEGY, WGPUStrategy};
use boon_host::{PhysicalSize, SurfaceId, Viewport, WindowId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};
use wgpu::SurfaceTargetUnsafe;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SurfaceDeviceBinding {
    pub adapter_id: String,
    pub device_id: String,
    pub queue_id: String,
    pub surface_id: SurfaceId,
    pub format: String,
    pub present_mode: String,
    pub alpha_mode: String,
    pub usage: String,
    pub epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycle {
    Created,
    Configured,
    Lost,
    Closing,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSlotMetadata {
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub role: String,
    pub viewport: Viewport,
    pub epoch: u64,
    pub binding: SurfaceDeviceBinding,
    pub lifecycle: SurfaceLifecycle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppWindowContract {
    pub wgpu_strategy: String,
    pub wgpu_surface_strategy: String,
    pub render_thread_required: bool,
    pub required_surface_usage: String,
}

pub fn app_window_contract() -> AppWindowContract {
    AppWindowContract {
        wgpu_strategy: format!("{:?}", app_window::WGPU_STRATEGY),
        wgpu_surface_strategy: format!("{:?}", app_window::WGPU_SURFACE_STRATEGY),
        render_thread_required: true,
        required_surface_usage: format!(
            "{:?}",
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        ),
    }
}

pub fn reject_stale_epoch(slot: &SurfaceSlotMetadata, frame_epoch: u64) -> Result<(), String> {
    if slot.epoch == frame_epoch && slot.binding.epoch == frame_epoch {
        Ok(())
    } else {
        Err(format!(
            "stale surface epoch: slot={}, binding={}, frame={frame_epoch}",
            slot.epoch, slot.binding.epoch
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeWindowRole {
    Preview,
    Dev,
}

impl NativeWindowRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Dev => "dev",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeWindowOptions {
    pub role: NativeWindowRole,
    pub title: String,
    pub initial_width: f32,
    pub initial_height: f32,
    pub hold_ms: u64,
    pub input_sample_delay_ms: u64,
    pub synthetic_input_probe: bool,
    pub warmup_frame_count: u32,
    pub sample_frame_count: u32,
    pub readback_artifact_dir: Option<String>,
    pub render_loop_state_report: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRenderLoopMode {
    ContinuousProbe,
    DemandDriven,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSchedulerReason {
    FirstFrame,
    SurfaceChanged,
    HostInput,
    Timer,
    ExternalWake,
    VerifierFrame,
    RequestedAnimation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRoleDirtyReason {
    SourcePayloadAccepted,
    WorkspaceSelectionChanged,
    RuntimeTurnApplied,
    DocumentPatchApplied,
    LayoutChanged,
    ScrollChanged,
    FocusChanged,
    TelemetrySummaryChanged,
    CaretBlink,
    VerifierFrame,
    ErrorOverlayChanged,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeRenderLoopState {
    pub mode: NativeRenderLoopMode,
    pub dirty_revision: u64,
    pub presented_revision: u64,
    pub rendered_frame_count: u64,
    pub skipped_idle_poll_count: u64,
    pub input_poll_count: u64,
    pub forced_frame_count: u64,
    pub scheduled_wake_count: u64,
    pub last_scheduler_reason: Option<NativeSchedulerReason>,
    pub last_role_dirty_reason: Option<NativeRoleDirtyReason>,
    #[serde(skip)]
    pub next_wake_at: Option<Instant>,
}

impl NativeRenderLoopState {
    pub fn new(mode: NativeRenderLoopMode) -> Self {
        Self {
            mode,
            dirty_revision: 1,
            presented_revision: 0,
            rendered_frame_count: 0,
            skipped_idle_poll_count: 0,
            input_poll_count: 0,
            forced_frame_count: 0,
            scheduled_wake_count: 0,
            last_scheduler_reason: Some(NativeSchedulerReason::FirstFrame),
            last_role_dirty_reason: None,
            next_wake_at: None,
        }
    }

    pub fn mark_dirty(
        &mut self,
        scheduler_reason: NativeSchedulerReason,
        role_dirty_reason: Option<NativeRoleDirtyReason>,
    ) -> u64 {
        self.dirty_revision = self.dirty_revision.saturating_add(1);
        self.last_scheduler_reason = Some(scheduler_reason);
        self.last_role_dirty_reason = role_dirty_reason;
        self.dirty_revision
    }

    pub fn mark_presented(&mut self, revision: u64) {
        self.presented_revision = self.presented_revision.max(revision);
        self.rendered_frame_count = self.rendered_frame_count.saturating_add(1);
    }

    pub fn should_render(&self, now: Instant, wake_generation_changed: bool) -> bool {
        if self.mode == NativeRenderLoopMode::ContinuousProbe {
            return true;
        }
        self.dirty_revision != self.presented_revision
            || wake_generation_changed
            || self.next_wake_at.is_some_and(|wake_at| now >= wake_at)
    }

    pub fn note_idle_poll(&mut self) {
        self.skipped_idle_poll_count = self.skipped_idle_poll_count.saturating_add(1);
    }

    pub fn note_input_poll(&mut self) {
        self.input_poll_count = self.input_poll_count.saturating_add(1);
    }

    pub fn schedule_wake_after(&mut self, now: Instant, delay: Duration) -> Instant {
        let candidate = now + delay;
        if self
            .next_wake_at
            .is_none_or(|existing| candidate < existing)
        {
            self.next_wake_at = Some(candidate);
        }
        self.next_wake_at.unwrap_or(candidate)
    }

    pub fn consume_due_wake(&mut self, now: Instant) -> bool {
        if self.next_wake_at.is_some_and(|wake_at| now >= wake_at) {
            self.next_wake_at = None;
            self.mark_dirty(NativeSchedulerReason::Timer, None);
            self.scheduled_wake_count = self.scheduled_wake_count.saturating_add(1);
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePollResult {
    pub dirty: bool,
    pub role_revision: u64,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub next_wake_after_ms: Option<u64>,
    pub wants_animation_frame: bool,
}

impl NativePollResult {
    pub fn clean(role_revision: u64) -> Self {
        Self {
            dirty: false,
            role_revision,
            role_dirty_reason: None,
            next_wake_after_ms: None,
            wants_animation_frame: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeRenderHookResult {
    pub proof: serde_json::Value,
    pub content_revision: u64,
    pub rendered: bool,
    pub content_changed: bool,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
}

impl NativeRenderHookResult {
    pub fn rendered_with_proof(proof: serde_json::Value) -> Self {
        Self {
            proof,
            content_revision: 0,
            rendered: true,
            content_changed: true,
            role_dirty_reason: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NativeWakeHandle {
    generation: Arc<AtomicU64>,
    signal: Arc<(Mutex<u64>, Condvar)>,
}

impl NativeWakeHandle {
    pub fn new() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            signal: Arc::new((Mutex::new(0), Condvar::new())),
        }
    }

    pub fn wake(&self) -> u64 {
        let generation = self
            .generation
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        let (lock, condvar) = &*self.signal;
        if let Ok(mut signaled_generation) = lock.lock() {
            *signaled_generation = generation;
            condvar.notify_all();
        }
        generation
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn wait_for_wake_after(&self, observed_generation: u64, timeout: Duration) -> u64 {
        if self.generation() != observed_generation || timeout.is_zero() {
            return self.generation();
        }
        let (lock, condvar) = &*self.signal;
        let Ok(guard) = lock.lock() else {
            return self.generation();
        };
        let _ = condvar.wait_timeout_while(guard, timeout, |signaled_generation| {
            *signaled_generation <= observed_generation && self.generation() == observed_generation
        });
        self.generation()
    }
}

impl Default for NativeWakeHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppWindowSurfaceProof {
    pub role: String,
    pub pid: u32,
    pub main_thread_id: String,
    pub render_thread_id: String,
    pub display_server: String,
    pub display_connection: String,
    pub window_backend: String,
    pub window_title: String,
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub wgpu_strategy: String,
    pub wgpu_surface_strategy: String,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub adapter_device: u32,
    pub adapter_vendor: u32,
    pub adapter_is_software: bool,
    pub surface_format: String,
    pub present_mode: String,
    pub alpha_mode: String,
    pub logical_size: Viewport,
    pub physical_size: PhysicalSize,
    pub acquired_surface_texture: bool,
    pub presented_frame: bool,
    pub clear_color_hash: String,
    pub surface_acquire_ms: f64,
    pub present_submit_ms: f64,
    pub presented_frame_ms: f64,
    pub readback_ms: Option<f64>,
    pub first_frame_ms: f64,
    pub interactive_frame_loop: bool,
    pub render_loop_mode: NativeRenderLoopMode,
    pub render_loop_state_at_ready: NativeRenderLoopState,
    pub input_sample_delay_ms: u64,
    pub frame_timing: NativeFrameTimingProof,
    pub post_input_frame_timing: Option<NativeFrameTimingProof>,
    pub input_adapter: NativeInputAdapterProof,
    pub external_render_proof: Option<serde_json::Value>,
    pub readback_artifact: Option<AppWindowReadbackArtifact>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeFrameTimingProof {
    pub warmup_frame_count: u32,
    pub sample_frame_count: u32,
    pub measured_frame_count: u32,
    pub first_presented_frame_ms: f64,
    pub presented_frame_ms_p50: f64,
    pub presented_frame_ms_p95: f64,
    pub presented_frame_ms_p99: f64,
    pub presented_frame_ms_max: f64,
    pub render_hook_ms_p95: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeInputAdapterProof {
    pub installed: bool,
    pub capture_scope: String,
    pub keyboard_api: String,
    pub mouse_api: String,
    pub wheel_api: String,
    pub per_window_event_provenance_api: String,
    pub sampled_after_visible_window: bool,
    pub real_os_events_observed: bool,
    pub input_injection_method: String,
    pub synthetic_input_probe: bool,
    pub mouse_last_window_protocol_id: Option<u64>,
    pub keyboard_last_window_protocol_id: Option<u64>,
    pub mouse_motion_event_count: u64,
    pub mouse_button_event_count: u64,
    pub mouse_scroll_event_count: u64,
    pub mouse_total_event_count: u64,
    pub keyboard_key_event_count: u64,
    pub mouse_button_events: Vec<NativeMouseButtonEventProof>,
    pub keyboard_events: Vec<NativeKeyboardEventProof>,
    pub mouse_window_pos: Option<NativeMouseWindowPosition>,
    pub mouse_buttons_down: Vec<String>,
    pub pressed_keys: Vec<String>,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NativeInputCursor {
    pub last_mouse_button_sequence: u64,
    pub last_keyboard_sequence: u64,
    pub last_mouse_scroll_event_count: u64,
}

impl NativeInputCursor {
    pub fn accept(&mut self, input: &NativeInputAdapterProof) {
        self.last_mouse_button_sequence = self.last_mouse_button_sequence.max(
            input
                .mouse_button_events
                .iter()
                .map(|event| event.sequence)
                .max()
                .unwrap_or(0),
        );
        self.last_keyboard_sequence = self.last_keyboard_sequence.max(
            input
                .keyboard_events
                .iter()
                .map(|event| event.sequence)
                .max()
                .unwrap_or(0),
        );
        self.last_mouse_scroll_event_count = self
            .last_mouse_scroll_event_count
            .max(input.mouse_scroll_event_count);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeKeyboardEventProof {
    pub sequence: u64,
    pub key: String,
    pub pressed: bool,
    pub window_protocol_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeMouseButtonEventProof {
    pub sequence: u64,
    pub button: String,
    pub pressed: bool,
    pub window_protocol_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeMouseWindowPosition {
    pub x: f64,
    pub y: f64,
    pub window_width: f64,
    pub window_height: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppWindowReadbackArtifact {
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub capture_method: String,
    pub texture_format: String,
    pub nonblank_samples: usize,
    pub unique_rgba_values: usize,
}

struct PendingSurfaceReadback {
    buffer: wgpu::Buffer,
    role: NativeWindowRole,
    title: String,
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    format: wgpu::TextureFormat,
}

pub struct NativeRenderFrameContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub surface_view: &'a wgpu::TextureView,
    pub surface_texture_format: wgpu::TextureFormat,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub surface_format: String,
    pub width: u32,
    pub height: u32,
    pub input: NativeInputAdapterProof,
}

#[derive(Clone, Debug)]
pub struct NativePollContext {
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub input_delta: NativeInputAdapterProof,
    pub now: Instant,
    pub forced_frame: bool,
}

pub type NativeRenderHook = Box<
    dyn for<'a> FnMut(NativeRenderFrameContext<'a>) -> Result<serde_json::Value, String> + Send,
>;

pub type NativePollHook =
    Box<dyn FnMut(NativePollContext) -> Result<NativePollResult, String> + Send>;

pub struct NativeWindowHooks {
    pub poll: Option<NativePollHook>,
    pub render: NativeRenderHook,
}

impl NativeWindowHooks {
    pub fn from_render_hook(render: NativeRenderHook) -> Self {
        Self { poll: None, render }
    }
}

fn poll_native_window_hooks(
    hooks: &mut Option<NativeWindowHooks>,
    context: NativePollContext,
) -> Result<Option<NativePollResult>, NativeWindowError> {
    hooks
        .as_mut()
        .and_then(|hooks| hooks.poll.as_mut())
        .map(|poll| {
            poll(context)
                .map_err(|error| NativeWindowError::Failed(format!("role poll hook: {error}")))
        })
        .transpose()
}

#[derive(Debug)]
pub enum NativeWindowError {
    MissingProof,
    Failed(String),
}

impl std::fmt::Display for NativeWindowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProof => {
                formatter.write_str("app_window role thread did not produce a proof before exiting")
            }
            Self::Failed(message) => write!(formatter, "app_window role failed: {message}"),
        }
    }
}

impl std::error::Error for NativeWindowError {}

pub fn run_visible_surface_probe<F>(options: NativeWindowOptions, on_ready: F) -> !
where
    F: FnOnce(Result<AppWindowSurfaceProof, NativeWindowError>) + Send + 'static,
{
    run_visible_surface_probe_with_render_hook(options, None, on_ready)
}

pub fn run_visible_surface_probe_with_render_hook<F>(
    options: NativeWindowOptions,
    render_hook: Option<NativeRenderHook>,
    on_ready: F,
) -> !
where
    F: FnOnce(Result<AppWindowSurfaceProof, NativeWindowError>) + Send + 'static,
{
    run_visible_surface_probe_with_render_hook_and_wake(
        options,
        render_hook,
        NativeWakeHandle::new(),
        on_ready,
    )
}

pub fn run_visible_surface_probe_with_render_hook_and_wake<F>(
    options: NativeWindowOptions,
    render_hook: Option<NativeRenderHook>,
    wake_handle: NativeWakeHandle,
    on_ready: F,
) -> !
where
    F: FnOnce(Result<AppWindowSurfaceProof, NativeWindowError>) + Send + 'static,
{
    let hooks = render_hook.map(NativeWindowHooks::from_render_hook);
    run_visible_surface_probe_with_hooks_and_wake(options, hooks, wake_handle, on_ready)
}

pub fn run_visible_surface_probe_with_hooks_and_wake<F>(
    options: NativeWindowOptions,
    hooks: Option<NativeWindowHooks>,
    wake_handle: NativeWakeHandle,
    on_ready: F,
) -> !
where
    F: FnOnce(Result<AppWindowSurfaceProof, NativeWindowError>) + Send + 'static,
{
    let main_thread_id = thread_id_string();
    app_window::application::main(move || {
        let (sender, receiver) =
            mpsc::sync_channel::<Result<AppWindowSurfaceProof, NativeWindowError>>(0);
        let (callback_done_sender, callback_done_receiver) = mpsc::sync_channel::<()>(0);
        let (render_done_sender, render_done_receiver) = mpsc::sync_channel::<()>(0);
        std::thread::Builder::new()
            .name(format!("boon-native-{}-render", options.role.as_str()))
            .spawn({
                let main_thread_id = main_thread_id.clone();
                move || {
                    futures::executor::block_on(run_surface_probe_async(
                        options,
                        hooks,
                        wake_handle,
                        main_thread_id,
                        sender,
                        callback_done_receiver,
                    ));
                    let _ = render_done_sender.send(());
                }
            })
            .expect("failed to spawn app_window render thread");

        match receiver.recv() {
            Ok(result) => on_ready(result),
            Err(_) => on_ready(Err(NativeWindowError::MissingProof)),
        }
        let _ = callback_done_sender.send(());
        let _ = render_done_receiver.recv();
    });
    std::process::exit(0);
}

async fn run_surface_probe_async(
    options: NativeWindowOptions,
    hooks: Option<NativeWindowHooks>,
    wake_handle: NativeWakeHandle,
    main_thread_id: String,
    ready_sender: mpsc::SyncSender<Result<AppWindowSurfaceProof, NativeWindowError>>,
    callback_done_receiver: mpsc::Receiver<()>,
) {
    if let Err(error) = run_surface_probe_inner(
        options,
        hooks,
        wake_handle,
        main_thread_id,
        ready_sender.clone(),
        callback_done_receiver,
    )
    .await
    {
        let _ = ready_sender.send(Err(error));
    }
}

async fn run_surface_probe_inner(
    options: NativeWindowOptions,
    mut hooks: Option<NativeWindowHooks>,
    wake_handle: NativeWakeHandle,
    main_thread_id: String,
    ready_sender: mpsc::SyncSender<Result<AppWindowSurfaceProof, NativeWindowError>>,
    callback_done_receiver: mpsc::Receiver<()>,
) -> Result<(), NativeWindowError> {
    let mut window = Window::new(
        Position::new(120.0, 120.0),
        Size::new(options.initial_width as f64, options.initial_height as f64),
        options.title.clone(),
    )
    .await;
    let app_surface = window.surface().await;
    let (size, scale) = app_surface.size_scale().await;
    let raw_display_handle = app_surface.raw_display_handle();
    let raw_window_handle = app_surface.raw_window_handle();
    let window_hash = stable_debug_hash(&raw_window_handle);
    let display_hash = stable_debug_hash(&raw_display_handle);
    let surface_id = SurfaceId(format!(
        "{}:{display_hash}:{window_hash}",
        options.role.as_str()
    ));
    let window_id = WindowId(format!("{}:{window_hash}", options.role.as_str()));
    let mut mouse = Mouse::coalesced().await;
    let keyboard = Keyboard::coalesced().await;
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    if WGPU_SURFACE_STRATEGY == WGPUStrategy::NotMainThread
        && app_window::application::is_main_thread()
    {
        return Err(NativeWindowError::Failed(
            "WGPU surface creation must not run on the app_window main thread".to_owned(),
        ));
    }
    if WGPU_SURFACE_STRATEGY == WGPUStrategy::MainThread
        && !app_window::application::is_main_thread()
    {
        return Err(NativeWindowError::Failed(
            "main-thread WGPU surface creation is not implemented in this native Wayland probe"
                .to_owned(),
        ));
    }
    let surface = unsafe {
        instance
            .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display_handle),
                raw_window_handle,
            })
            .map_err(|error| NativeWindowError::Failed(format!("create_surface: {error}")))?
    };
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        })
        .await
        .map_err(|error| NativeWindowError::Failed(format!("request_adapter: {error}")))?;
    let adapter_info = adapter.get_info();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("boon-native-app-window-probe-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
        })
        .await
        .map_err(|error| NativeWindowError::Failed(format!("request_device: {error}")))?;
    let mut width = ((size.width() * scale).round() as u32).max(1);
    let mut height = ((size.height() * scale).round() as u32).max(1);
    let capabilities = surface.get_capabilities(&adapter);
    let mut config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| NativeWindowError::Failed("surface default config unavailable".into()))?;
    if capabilities
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::Opaque)
    {
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }
    if options.readback_artifact_dir.is_some() {
        if !capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            return Err(NativeWindowError::Failed(format!(
                "visible surface readback requires COPY_SRC usage, but supported usages are {:?}",
                capabilities.usages
            )));
        }
        config.usage |= wgpu::TextureUsages::COPY_SRC;
    }
    let surface_format = format!("{:?}", config.format);
    let present_mode = format!("{:?}", config.present_mode);
    let alpha_mode = format!("{:?}", config.alpha_mode);
    surface.configure(&device, &config);
    let warmup_frame_count = options.warmup_frame_count;
    let sample_frame_count = options.sample_frame_count.max(1);
    let total_frame_count = warmup_frame_count.saturating_add(sample_frame_count).max(1);
    let mut external_render_proof = None;
    let mut surface_acquire_ms = 0.0;
    let mut present_submit_ms = 0.0;
    let mut first_presented_frame_ms = 0.0;
    let mut presented_frame_samples = Vec::new();
    let mut render_hook_samples = Vec::new();
    let mut pending_readback = None;

    for frame_index in 0..total_frame_count {
        let input = empty_input_adapter_proof(false);
        let _ = poll_native_window_hooks(
            &mut hooks,
            NativePollContext {
                window_id: window_id.clone(),
                surface_id: surface_id.clone(),
                surface_epoch: 1,
                width,
                height,
                scale: scale as f32,
                input_delta: input.clone(),
                now: Instant::now(),
                forced_frame: true,
            },
        )?;
        let acquire_start = Instant::now();
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            other => {
                return Err(NativeWindowError::Failed(format!(
                    "get_current_texture: {other:?}"
                )));
            }
        };
        let current_surface_acquire_ms = elapsed_ms(acquire_start);
        let present_start = Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-app-window-probe-encoder"),
        });
        let render_hook_ms = match hooks.as_mut() {
            Some(hooks) => {
                let render_start = Instant::now();
                let proof = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    surface_id: surface_id.clone(),
                    surface_epoch: 1,
                    surface_format: surface_format.clone(),
                    width,
                    height,
                    input,
                })
                .map_err(|error| {
                    NativeWindowError::Failed(format!("external render hook: {error}"))
                })?;
                external_render_proof = Some(proof);
                Some(elapsed_ms(render_start))
            }
            None => {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("boon-native-app-window-probe-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color(options.role)),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                None
            }
        };
        if frame_index + 1 == total_frame_count && options.readback_artifact_dir.is_some() {
            pending_readback = Some(queue_visible_surface_readback(
                &device,
                &mut encoder,
                &frame.texture,
                options.role,
                width,
                height,
                config.format,
                &options.title,
            )?);
        }
        queue.submit(Some(encoder.finish()));
        frame.present();
        let current_present_submit_ms = elapsed_ms(present_start);
        if frame_index == 0 {
            surface_acquire_ms = current_surface_acquire_ms;
            present_submit_ms = current_present_submit_ms;
            first_presented_frame_ms = current_surface_acquire_ms + current_present_submit_ms;
        }
        if frame_index >= warmup_frame_count {
            presented_frame_samples.push(current_surface_acquire_ms + current_present_submit_ms);
            if let Some(render_hook_ms) = render_hook_ms {
                render_hook_samples.push(render_hook_ms);
            }
        }
    }
    let frame_timing = NativeFrameTimingProof {
        warmup_frame_count,
        sample_frame_count,
        measured_frame_count: presented_frame_samples.len() as u32,
        first_presented_frame_ms,
        presented_frame_ms_p50: percentile(&presented_frame_samples, 0.50),
        presented_frame_ms_p95: percentile(&presented_frame_samples, 0.95),
        presented_frame_ms_p99: percentile(&presented_frame_samples, 0.99),
        presented_frame_ms_max: presented_frame_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        render_hook_ms_p95: (!render_hook_samples.is_empty())
            .then(|| percentile(&render_hook_samples, 0.95)),
    };
    let readback_start = Instant::now();
    let mut readback_artifact = if let (Some(pending), Some(artifact_dir)) =
        (pending_readback, options.readback_artifact_dir.as_deref())
    {
        Some(finish_visible_surface_readback(
            &device,
            pending,
            artifact_dir,
        )?)
    } else {
        None
    };
    let readback_ms = readback_artifact
        .as_ref()
        .map(|_| elapsed_ms(readback_start));
    if options.input_sample_delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(options.input_sample_delay_ms));
    }
    if options.synthetic_input_probe {
        inject_synthetic_input_probe(&mut mouse, &keyboard, &window_id, width, height);
    }
    let input_adapter = sample_input_adapter(&mut mouse, &keyboard, options.synthetic_input_probe);
    let mut post_input_frame_timing = None;
    if input_adapter.real_os_events_observed && hooks.is_some() {
        let post_input_sample_count = sample_frame_count.max(1);
        let mut post_input_presented_frame_samples = Vec::new();
        let mut post_input_render_hook_samples = Vec::new();
        let mut post_input_first_frame_ms = 0.0;
        let mut post_input_readback = None;
        for frame_index in 0..post_input_sample_count {
            let frame_input = if frame_index == 0 {
                input_adapter.clone()
            } else {
                sample_input_adapter(&mut mouse, &keyboard, false)
            };
            let _ = poll_native_window_hooks(
                &mut hooks,
                NativePollContext {
                    window_id: window_id.clone(),
                    surface_id: surface_id.clone(),
                    surface_epoch: 1,
                    width,
                    height,
                    scale: scale as f32,
                    input_delta: frame_input.clone(),
                    now: Instant::now(),
                    forced_frame: true,
                },
            )?;
            let acquire_start = Instant::now();
            let frame = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                other => {
                    return Err(NativeWindowError::Failed(format!(
                        "get_current_texture after input sample: {other:?}"
                    )));
                }
            };
            let current_surface_acquire_ms = elapsed_ms(acquire_start);
            let present_start = Instant::now();
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-native-app-window-input-sample-encoder"),
            });
            if let Some(hooks) = hooks.as_mut() {
                let render_start = Instant::now();
                external_render_proof = Some(
                    (hooks.render)(NativeRenderFrameContext {
                        device: &device,
                        queue: &queue,
                        encoder: &mut encoder,
                        surface_view: &view,
                        surface_texture_format: config.format,
                        surface_id: surface_id.clone(),
                        surface_epoch: 1,
                        surface_format: surface_format.clone(),
                        width,
                        height,
                        input: frame_input,
                    })
                    .map_err(|error| {
                        NativeWindowError::Failed(format!(
                            "external render hook after input: {error}"
                        ))
                    })?,
                );
                post_input_render_hook_samples.push(elapsed_ms(render_start));
            }
            if frame_index + 1 == post_input_sample_count && options.readback_artifact_dir.is_some()
            {
                post_input_readback = Some(queue_visible_surface_readback(
                    &device,
                    &mut encoder,
                    &frame.texture,
                    options.role,
                    width,
                    height,
                    config.format,
                    &options.title,
                )?);
            }
            queue.submit(Some(encoder.finish()));
            frame.present();
            let current_present_submit_ms = elapsed_ms(present_start);
            let frame_ms = current_surface_acquire_ms + current_present_submit_ms;
            if frame_index == 0 {
                post_input_first_frame_ms = frame_ms;
            }
            post_input_presented_frame_samples.push(frame_ms);
        }
        post_input_frame_timing = Some(NativeFrameTimingProof {
            warmup_frame_count: 0,
            sample_frame_count: post_input_sample_count,
            measured_frame_count: post_input_presented_frame_samples.len() as u32,
            first_presented_frame_ms: post_input_first_frame_ms,
            presented_frame_ms_p50: percentile(&post_input_presented_frame_samples, 0.50),
            presented_frame_ms_p95: percentile(&post_input_presented_frame_samples, 0.95),
            presented_frame_ms_p99: percentile(&post_input_presented_frame_samples, 0.99),
            presented_frame_ms_max: post_input_presented_frame_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            render_hook_ms_p95: (!post_input_render_hook_samples.is_empty())
                .then(|| percentile(&post_input_render_hook_samples, 0.95)),
        });
        if let (Some(pending), Some(artifact_dir)) = (
            post_input_readback,
            options.readback_artifact_dir.as_deref(),
        ) {
            readback_artifact = Some(finish_visible_surface_readback(
                &device,
                pending,
                artifact_dir,
            )?);
        }
    }

    let loop_mode = if options.hold_ms == 0 {
        NativeRenderLoopMode::DemandDriven
    } else {
        NativeRenderLoopMode::ContinuousProbe
    };
    let mut render_loop_state = NativeRenderLoopState::new(loop_mode);
    render_loop_state.mark_presented(1);
    let proof = AppWindowSurfaceProof {
        role: options.role.as_str().to_owned(),
        pid: std::process::id(),
        main_thread_id,
        render_thread_id: thread_id_string(),
        display_server: display_server(),
        display_connection: display_connection(),
        window_backend: "app_window-wayland".to_owned(),
        window_title: options.title,
        window_id: window_id.clone(),
        surface_id: surface_id.clone(),
        surface_epoch: 1,
        wgpu_strategy: format!("{:?}", app_window::WGPU_STRATEGY),
        wgpu_surface_strategy: format!("{:?}", app_window::WGPU_SURFACE_STRATEGY),
        adapter_name: adapter_info.name,
        adapter_backend: format!("{:?}", adapter_info.backend),
        adapter_device: adapter_info.device,
        adapter_vendor: adapter_info.vendor,
        adapter_is_software: matches!(adapter_info.device_type, wgpu::DeviceType::Cpu),
        surface_format: surface_format.clone(),
        present_mode,
        alpha_mode,
        logical_size: Viewport {
            surface: 1,
            width: size.width() as f32,
            height: size.height() as f32,
            scale,
        },
        physical_size: PhysicalSize { width, height },
        acquired_surface_texture: true,
        presented_frame: true,
        clear_color_hash: clear_color_hash(options.role),
        surface_acquire_ms,
        present_submit_ms,
        presented_frame_ms: surface_acquire_ms + present_submit_ms,
        readback_ms,
        first_frame_ms: surface_acquire_ms + present_submit_ms + readback_ms.unwrap_or(0.0),
        interactive_frame_loop: true,
        render_loop_mode: loop_mode,
        render_loop_state_at_ready: render_loop_state.clone(),
        input_sample_delay_ms: options.input_sample_delay_ms,
        frame_timing,
        post_input_frame_timing,
        input_adapter,
        external_render_proof,
        readback_artifact,
    };
    let _ = ready_sender.send(Ok(proof));
    let hold_started = Instant::now();
    let mut input_cursor = NativeInputCursor::default();
    let mut last_wake_generation = 0;
    loop {
        if options.hold_ms > 0 && hold_started.elapsed() >= Duration::from_millis(options.hold_ms) {
            break;
        }
        let (current_size, current_scale) = app_surface.size_scale().await;
        let current_width = ((current_size.width() * current_scale).round() as u32).max(1);
        let current_height = ((current_size.height() * current_scale).round() as u32).max(1);
        if current_width != width || current_height != height {
            width = current_width;
            height = current_height;
            config.width = width;
            config.height = height;
            surface.configure(&device, &config);
            render_loop_state.mark_dirty(NativeSchedulerReason::SurfaceChanged, None);
        }
        let poll_started_at = Instant::now();
        render_loop_state.consume_due_wake(poll_started_at);
        let input = sample_input_adapter_delta(&mut mouse, &keyboard, &input_cursor, false);
        render_loop_state.note_input_poll();
        let poll_result = poll_native_window_hooks(
            &mut hooks,
            NativePollContext {
                window_id: window_id.clone(),
                surface_id: surface_id.clone(),
                surface_epoch: 1,
                width,
                height,
                scale: current_scale as f32,
                input_delta: input.clone(),
                now: poll_started_at,
                forced_frame: false,
            },
        )?;
        if let Some(poll_result) = poll_result {
            if let Some(next_wake_after_ms) = poll_result.next_wake_after_ms {
                render_loop_state.schedule_wake_after(
                    poll_started_at,
                    Duration::from_millis(next_wake_after_ms),
                );
            }
            if poll_result.dirty {
                render_loop_state.mark_dirty(
                    NativeSchedulerReason::HostInput,
                    poll_result.role_dirty_reason,
                );
                render_loop_state.dirty_revision = render_loop_state
                    .dirty_revision
                    .max(poll_result.role_revision);
            }
            if poll_result.wants_animation_frame {
                render_loop_state.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
            }
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        } else if input.real_os_events_observed {
            render_loop_state.mark_dirty(NativeSchedulerReason::HostInput, None);
        }
        let wake_generation = wake_handle.generation();
        let wake_generation_changed = wake_generation != last_wake_generation;
        if wake_generation_changed {
            last_wake_generation = wake_generation;
            render_loop_state.mark_dirty(NativeSchedulerReason::ExternalWake, None);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
        }
        if !render_loop_state.should_render(Instant::now(), false) {
            render_loop_state.note_idle_poll();
            let idle_timeout = render_loop_state
                .next_wake_at
                .and_then(|wake_at| wake_at.checked_duration_since(Instant::now()))
                .unwrap_or_else(|| Duration::from_millis(50))
                .min(Duration::from_millis(50));
            let _ = wake_handle.wait_for_wake_after(last_wake_generation, idle_timeout);
            continue;
        }
        if hooks.as_ref().is_none_or(|hooks| hooks.poll.is_none()) {
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        }
        let rendered_revision = render_loop_state.dirty_revision;
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            other => {
                return Err(NativeWindowError::Failed(format!(
                    "get_current_texture during interactive loop: {other:?}"
                )));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-app-window-interactive-encoder"),
        });
        match hooks.as_mut() {
            Some(hooks) => {
                (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    surface_id: surface_id.clone(),
                    surface_epoch: 1,
                    surface_format: surface_format.clone(),
                    width,
                    height,
                    input: input.clone(),
                })
                .map_err(|error| {
                    NativeWindowError::Failed(format!("external render hook: {error}"))
                })?;
            }
            None => {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("boon-native-app-window-interactive-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color(options.role)),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
        }
        queue.submit(Some(encoder.finish()));
        frame.present();
        render_loop_state.mark_presented(rendered_revision);
        let frame_sleep_ms = match loop_mode {
            NativeRenderLoopMode::ContinuousProbe => 16,
            NativeRenderLoopMode::DemandDriven => 5,
        };
        std::thread::sleep(Duration::from_millis(frame_sleep_ms));
    }
    if let Some(report) = options.render_loop_state_report.as_deref() {
        write_render_loop_state_report(
            Path::new(report),
            options.role,
            std::process::id(),
            &window_id,
            &surface_id,
            1,
            &render_loop_state,
            hold_started.elapsed(),
            wake_handle.generation(),
        )?;
    }
    let _ = callback_done_receiver.recv_timeout(Duration::from_secs(30));
    drop(surface);
    drop(app_surface);
    drop(window);
    std::process::exit(0);
}

fn queue_visible_surface_readback(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    role: NativeWindowRole,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    title: &str,
) -> Result<PendingSurfaceReadback, NativeWindowError> {
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 1080);
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer_size = padded_bytes_per_row as u64 * height as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-readback-buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(PendingSurfaceReadback {
        buffer,
        role,
        title: title.to_owned(),
        width,
        height,
        unpadded_bytes_per_row,
        padded_bytes_per_row,
        format,
    })
}

fn write_render_loop_state_report(
    path: &Path,
    role: NativeWindowRole,
    pid: u32,
    window_id: &WindowId,
    surface_id: &SurfaceId,
    surface_epoch: u64,
    state: &NativeRenderLoopState,
    elapsed: Duration,
    wake_generation: u64,
) -> Result<(), NativeWindowError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            NativeWindowError::Failed(format!(
                "create render-loop report dir {}: {error}",
                parent.display()
            ))
        })?;
    }
    let report = serde_json::json!({
        "status": "pass",
        "role": role.as_str(),
        "pid": pid,
        "window_id": window_id,
        "surface_id": surface_id,
        "surface_epoch": surface_epoch,
        "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
        "wake_generation": wake_generation,
        "render_loop_state": state,
        "render_loop_mode": state.mode,
        "dirty_revision": state.dirty_revision,
        "presented_revision": state.presented_revision,
        "rendered_frame_count": state.rendered_frame_count,
        "skipped_idle_poll_count": state.skipped_idle_poll_count,
        "input_poll_count": state.input_poll_count,
        "forced_frame_count": state.forced_frame_count,
        "scheduled_wake_count": state.scheduled_wake_count,
        "last_scheduler_reason": state.last_scheduler_reason,
        "last_role_dirty_reason": state.last_role_dirty_reason
    });
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| NativeWindowError::Failed(format!("serialize loop state: {error}")))?;
    std::fs::write(path, bytes).map_err(|error| {
        NativeWindowError::Failed(format!(
            "write render-loop report {}: {error}",
            path.display()
        ))
    })?;
    Ok(())
}

fn finish_visible_surface_readback(
    device: &wgpu::Device,
    pending: PendingSurfaceReadback,
    artifact_dir: &str,
) -> Result<AppWindowReadbackArtifact, NativeWindowError> {
    let artifact_dir = PathBuf::from(artifact_dir);
    std::fs::create_dir_all(&artifact_dir).map_err(|error| {
        NativeWindowError::Failed(format!(
            "create readback artifact directory `{}`: {error}",
            artifact_dir.display()
        ))
    })?;
    let slice = pending.buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| NativeWindowError::Failed(format!("readback poll: {error}")))?;
    receiver
        .recv()
        .map_err(|error| NativeWindowError::Failed(format!("readback map callback: {error}")))?
        .map_err(|error| NativeWindowError::Failed(format!("readback map: {error}")))?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((pending.width * pending.height * 4) as usize);
    for row in 0..pending.height as usize {
        let start = row * pending.padded_bytes_per_row as usize;
        let end = start + pending.unpadded_bytes_per_row as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    pending.buffer.unmap();

    if matches!(
        pending.format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }

    let nonblank_samples = pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[0] != 0 || rgba[1] != 0 || rgba[2] != 0 || rgba[3] != 0)
        .count();
    let unique_rgba_values = pixels
        .chunks_exact(4)
        .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let path = artifact_dir.join(format!(
        "{}-{}-{}.png",
        std::process::id(),
        pending.role.as_str(),
        stable_debug_hash(&pending.title)
    ));
    image::save_buffer(
        &path,
        &pixels,
        pending.width,
        pending.height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| {
        NativeWindowError::Failed(format!("save readback `{}`: {error}", path.display()))
    })?;
    let sha256 = sha256_file(&path)?;
    Ok(AppWindowReadbackArtifact {
        path: path.display().to_string(),
        sha256,
        width: pending.width,
        height: pending.height,
        capture_method: "wgpu-visible-surface-copy-src-readback".to_owned(),
        texture_format: format!("{:?}", pending.format),
        nonblank_samples,
        unique_rgba_values,
    })
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let rank = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn sha256_file(path: &Path) -> Result<String, NativeWindowError> {
    let bytes = std::fs::read(path).map_err(|error| {
        NativeWindowError::Failed(format!("read artifact `{}`: {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn inject_synthetic_input_probe(
    mouse: &mut Mouse,
    keyboard: &Keyboard,
    window_id: &WindowId,
    width: u32,
    height: u32,
) {
    let protocol_id = u64::from_str_radix(&stable_debug_hash(window_id), 16)
        .unwrap_or(1)
        .max(1);
    mouse.inject_test_motion(
        f64::from(width) / 2.0,
        f64::from(height) / 2.0,
        f64::from(width),
        f64::from(height),
        protocol_id,
    );
    mouse.inject_test_button(MOUSE_BUTTON_LEFT, true, protocol_id);
    mouse.inject_test_button(MOUSE_BUTTON_LEFT, false, protocol_id);
    mouse.inject_test_scroll(320.0, 640.0, protocol_id);
    keyboard.inject_test_key(KeyboardKey::A, true, protocol_id);
    keyboard.inject_test_key(KeyboardKey::A, false, protocol_id);
}

fn sample_input_adapter(
    mouse: &mut Mouse,
    keyboard: &Keyboard,
    synthetic_input_probe: bool,
) -> NativeInputAdapterProof {
    let mouse_window_pos = mouse
        .window_pos()
        .map(|position| NativeMouseWindowPosition {
            x: position.pos_x(),
            y: position.pos_y(),
            window_width: position.window_width(),
            window_height: position.window_height(),
        });
    let mouse_buttons_down = [
        (MOUSE_BUTTON_LEFT, "left"),
        (MOUSE_BUTTON_RIGHT, "right"),
        (MOUSE_BUTTON_MIDDLE, "middle"),
    ]
    .into_iter()
    .filter_map(|(button, label)| mouse.button_state(button).then(|| label.to_owned()))
    .collect::<Vec<_>>();
    let pressed_keys = [
        KeyboardKey::A,
        KeyboardKey::B,
        KeyboardKey::C,
        KeyboardKey::D,
        KeyboardKey::E,
        KeyboardKey::F,
        KeyboardKey::G,
        KeyboardKey::H,
        KeyboardKey::I,
        KeyboardKey::J,
        KeyboardKey::K,
        KeyboardKey::L,
        KeyboardKey::M,
        KeyboardKey::N,
        KeyboardKey::O,
        KeyboardKey::P,
        KeyboardKey::Q,
        KeyboardKey::R,
        KeyboardKey::S,
        KeyboardKey::T,
        KeyboardKey::U,
        KeyboardKey::V,
        KeyboardKey::W,
        KeyboardKey::X,
        KeyboardKey::Y,
        KeyboardKey::Z,
        KeyboardKey::Space,
        KeyboardKey::Delete,
        KeyboardKey::ForwardDelete,
        KeyboardKey::Tab,
        KeyboardKey::Home,
        KeyboardKey::End,
        KeyboardKey::PageUp,
        KeyboardKey::PageDown,
        KeyboardKey::LeftArrow,
        KeyboardKey::RightArrow,
        KeyboardKey::UpArrow,
        KeyboardKey::DownArrow,
        KeyboardKey::Return,
        KeyboardKey::Escape,
        KeyboardKey::Shift,
        KeyboardKey::RightShift,
        KeyboardKey::Control,
        KeyboardKey::RightControl,
        KeyboardKey::Option,
        KeyboardKey::RightOption,
        KeyboardKey::Command,
        KeyboardKey::RightCommand,
    ]
    .into_iter()
    .filter_map(|key| keyboard.is_pressed(key).then(|| format!("{key:?}")))
    .collect::<Vec<_>>();
    let (scroll_delta_x, scroll_delta_y) = mouse.load_clear_scroll_delta();
    let mouse_provenance = mouse.event_provenance();
    let keyboard_provenance = keyboard.event_provenance();
    let keyboard_events = keyboard_provenance
        .recent_events
        .iter()
        .map(|event| NativeKeyboardEventProof {
            sequence: event.sequence,
            key: format!("{:?}", event.key),
            pressed: event.pressed,
            window_protocol_id: event.window_protocol_id,
        })
        .collect::<Vec<_>>();
    let mouse_button_events = mouse_provenance
        .recent_button_events
        .iter()
        .map(|event| NativeMouseButtonEventProof {
            sequence: event.sequence,
            button: mouse_button_label(event.button).to_owned(),
            pressed: event.pressed,
            window_protocol_id: event.window_protocol_id,
        })
        .collect::<Vec<_>>();
    let real_os_events_observed = mouse_window_pos.is_some()
        || !mouse_buttons_down.is_empty()
        || !pressed_keys.is_empty()
        || scroll_delta_x != 0.0
        || scroll_delta_y != 0.0
        || mouse_provenance.total_event_count > 0
        || keyboard_provenance.key_event_count > 0;

    NativeInputAdapterProof {
        installed: true,
        capture_scope: "app_window_coalesced_input_with_per_window_event_provenance".to_owned(),
        keyboard_api: "app_window::input::keyboard::Keyboard::coalesced".to_owned(),
        mouse_api: "app_window::input::mouse::Mouse::coalesced".to_owned(),
        wheel_api: "app_window::input::mouse::Mouse::load_clear_scroll_delta".to_owned(),
        per_window_event_provenance_api: "app_window::input::{mouse,keyboard}::event_provenance"
            .to_owned(),
        sampled_after_visible_window: true,
        real_os_events_observed,
        input_injection_method: if synthetic_input_probe {
            "app_window_per_window_synthetic_input_harness".to_owned()
        } else {
            "none-observation-only".to_owned()
        },
        synthetic_input_probe,
        mouse_last_window_protocol_id: mouse_provenance.last_window_protocol_id,
        keyboard_last_window_protocol_id: keyboard_provenance.last_window_protocol_id,
        mouse_motion_event_count: mouse_provenance.motion_event_count,
        mouse_button_event_count: mouse_provenance.button_event_count,
        mouse_scroll_event_count: mouse_provenance.scroll_event_count,
        mouse_total_event_count: mouse_provenance.total_event_count,
        keyboard_key_event_count: keyboard_provenance.key_event_count,
        mouse_button_events,
        keyboard_events,
        mouse_window_pos,
        mouse_buttons_down,
        pressed_keys,
        scroll_delta_x,
        scroll_delta_y,
    }
}

fn sample_input_adapter_delta(
    mouse: &Mouse,
    keyboard: &Keyboard,
    cursor: &NativeInputCursor,
    synthetic_input_probe: bool,
) -> NativeInputAdapterProof {
    let mouse_window_pos = mouse
        .window_pos()
        .map(|position| NativeMouseWindowPosition {
            x: position.pos_x(),
            y: position.pos_y(),
            window_width: position.window_width(),
            window_height: position.window_height(),
        });
    let mouse_buttons_down = [
        (MOUSE_BUTTON_LEFT, "left"),
        (MOUSE_BUTTON_RIGHT, "right"),
        (MOUSE_BUTTON_MIDDLE, "middle"),
    ]
    .into_iter()
    .filter_map(|(button, label)| mouse.button_state(button).then(|| label.to_owned()))
    .collect::<Vec<_>>();
    let pressed_keys = [
        KeyboardKey::A,
        KeyboardKey::B,
        KeyboardKey::C,
        KeyboardKey::D,
        KeyboardKey::E,
        KeyboardKey::F,
        KeyboardKey::G,
        KeyboardKey::H,
        KeyboardKey::I,
        KeyboardKey::J,
        KeyboardKey::K,
        KeyboardKey::L,
        KeyboardKey::M,
        KeyboardKey::N,
        KeyboardKey::O,
        KeyboardKey::P,
        KeyboardKey::Q,
        KeyboardKey::R,
        KeyboardKey::S,
        KeyboardKey::T,
        KeyboardKey::U,
        KeyboardKey::V,
        KeyboardKey::W,
        KeyboardKey::X,
        KeyboardKey::Y,
        KeyboardKey::Z,
        KeyboardKey::Space,
        KeyboardKey::Delete,
        KeyboardKey::ForwardDelete,
        KeyboardKey::Tab,
        KeyboardKey::Home,
        KeyboardKey::End,
        KeyboardKey::PageUp,
        KeyboardKey::PageDown,
        KeyboardKey::LeftArrow,
        KeyboardKey::RightArrow,
        KeyboardKey::UpArrow,
        KeyboardKey::DownArrow,
        KeyboardKey::Return,
        KeyboardKey::Escape,
        KeyboardKey::Shift,
        KeyboardKey::RightShift,
        KeyboardKey::Control,
        KeyboardKey::RightControl,
        KeyboardKey::Option,
        KeyboardKey::RightOption,
        KeyboardKey::Command,
        KeyboardKey::RightCommand,
    ]
    .into_iter()
    .filter_map(|key| keyboard.is_pressed(key).then(|| format!("{key:?}")))
    .collect::<Vec<_>>();
    let (scroll_delta_x, scroll_delta_y) = mouse.scroll_delta();
    let mouse_provenance = mouse.event_provenance();
    let keyboard_provenance = keyboard.event_provenance();
    let keyboard_events = keyboard_provenance
        .recent_events
        .iter()
        .filter(|event| event.sequence > cursor.last_keyboard_sequence)
        .map(|event| NativeKeyboardEventProof {
            sequence: event.sequence,
            key: format!("{:?}", event.key),
            pressed: event.pressed,
            window_protocol_id: event.window_protocol_id,
        })
        .collect::<Vec<_>>();
    let mouse_button_events = mouse_provenance
        .recent_button_events
        .iter()
        .filter(|event| event.sequence > cursor.last_mouse_button_sequence)
        .map(|event| NativeMouseButtonEventProof {
            sequence: event.sequence,
            button: mouse_button_label(event.button).to_owned(),
            pressed: event.pressed,
            window_protocol_id: event.window_protocol_id,
        })
        .collect::<Vec<_>>();
    let new_scroll_observed = mouse_provenance.scroll_event_count
        > cursor.last_mouse_scroll_event_count
        && (scroll_delta_x != 0.0 || scroll_delta_y != 0.0);
    let real_os_events_observed = !mouse_button_events.is_empty()
        || !keyboard_events.is_empty()
        || new_scroll_observed
        || !mouse_buttons_down.is_empty()
        || !pressed_keys.is_empty();

    NativeInputAdapterProof {
        installed: true,
        capture_scope: "app_window_coalesced_input_delta_with_per_window_event_provenance"
            .to_owned(),
        keyboard_api: "app_window::input::keyboard::Keyboard::coalesced".to_owned(),
        mouse_api: "app_window::input::mouse::Mouse::coalesced".to_owned(),
        wheel_api: "app_window::input::mouse::Mouse::{scroll_delta,load_clear_scroll_delta}"
            .to_owned(),
        per_window_event_provenance_api: "app_window::input::{mouse,keyboard}::event_provenance"
            .to_owned(),
        sampled_after_visible_window: true,
        real_os_events_observed,
        input_injection_method: if synthetic_input_probe {
            "app_window_per_window_synthetic_input_harness".to_owned()
        } else {
            "none-observation-only".to_owned()
        },
        synthetic_input_probe,
        mouse_last_window_protocol_id: mouse_provenance.last_window_protocol_id,
        keyboard_last_window_protocol_id: keyboard_provenance.last_window_protocol_id,
        mouse_motion_event_count: mouse_provenance.motion_event_count,
        mouse_button_event_count: mouse_provenance.button_event_count,
        mouse_scroll_event_count: mouse_provenance.scroll_event_count,
        mouse_total_event_count: mouse_provenance.total_event_count,
        keyboard_key_event_count: keyboard_provenance.key_event_count,
        mouse_button_events,
        keyboard_events,
        mouse_window_pos,
        mouse_buttons_down,
        pressed_keys,
        scroll_delta_x: if new_scroll_observed {
            scroll_delta_x
        } else {
            0.0
        },
        scroll_delta_y: if new_scroll_observed {
            scroll_delta_y
        } else {
            0.0
        },
    }
}

fn accept_input_cursor(
    mouse: &mut Mouse,
    cursor: &mut NativeInputCursor,
    input: &NativeInputAdapterProof,
) {
    if input.scroll_delta_x != 0.0 || input.scroll_delta_y != 0.0 {
        let _ = mouse.load_clear_scroll_delta();
    }
    cursor.accept(input);
}

fn mouse_button_label(button: u8) -> &'static str {
    match button {
        MOUSE_BUTTON_LEFT => "left",
        MOUSE_BUTTON_RIGHT => "right",
        MOUSE_BUTTON_MIDDLE => "middle",
        _ => "other",
    }
}

fn empty_input_adapter_proof(synthetic_input_probe: bool) -> NativeInputAdapterProof {
    NativeInputAdapterProof {
        installed: true,
        capture_scope: "app_window_coalesced_input_with_per_window_event_provenance".to_owned(),
        keyboard_api: "app_window::input::keyboard::Keyboard::coalesced".to_owned(),
        mouse_api: "app_window::input::mouse::Mouse::coalesced".to_owned(),
        wheel_api: "app_window::input::mouse::Mouse::load_clear_scroll_delta".to_owned(),
        per_window_event_provenance_api: "app_window::input::{mouse,keyboard}::event_provenance"
            .to_owned(),
        sampled_after_visible_window: true,
        real_os_events_observed: false,
        input_injection_method: if synthetic_input_probe {
            "app_window_per_window_synthetic_input_harness".to_owned()
        } else {
            "none-observation-only".to_owned()
        },
        synthetic_input_probe,
        mouse_last_window_protocol_id: None,
        keyboard_last_window_protocol_id: None,
        mouse_motion_event_count: 0,
        mouse_button_event_count: 0,
        mouse_scroll_event_count: 0,
        mouse_total_event_count: 0,
        keyboard_key_event_count: 0,
        mouse_button_events: Vec::new(),
        keyboard_events: Vec::new(),
        mouse_window_pos: None,
        mouse_buttons_down: Vec::new(),
        pressed_keys: Vec::new(),
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
    }
}

fn clear_color(role: NativeWindowRole) -> wgpu::Color {
    match role {
        NativeWindowRole::Preview => wgpu::Color {
            r: 0.06,
            g: 0.44,
            b: 0.30,
            a: 1.0,
        },
        NativeWindowRole::Dev => wgpu::Color {
            r: 0.16,
            g: 0.24,
            b: 0.64,
            a: 1.0,
        },
    }
}

fn clear_color_hash(role: NativeWindowRole) -> String {
    let color = clear_color(role);
    format!(
        "clear:{:.3}:{:.3}:{:.3}:{:.3}",
        color.r, color.g, color.b, color.a
    )
}

fn thread_id_string() -> String {
    format!("{:?}", std::thread::current().id())
}

fn display_server() -> String {
    match std::env::var("XDG_SESSION_TYPE") {
        Ok(value) if value == "wayland" => value,
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => "wayland".to_owned(),
        _ if std::env::var_os("DISPLAY").is_some() => "x11".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn display_connection() -> String {
    std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn stable_debug_hash<T: std::fmt::Debug>(value: &T) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{value:?}").hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demand_driven_scheduler_renders_first_dirty_revision_once() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();

        assert!(state.should_render(now, false));
        state.mark_presented(state.dirty_revision);

        assert!(!state.should_render(now, false));
        state.note_idle_poll();
        assert_eq!(state.rendered_frame_count, 1);
        assert_eq!(state.skipped_idle_poll_count, 1);
    }

    #[test]
    fn demand_driven_scheduler_wakes_for_surface_change() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);

        let dirty = state.mark_dirty(NativeSchedulerReason::SurfaceChanged, None);

        assert_eq!(dirty, 2);
        assert!(state.should_render(Instant::now(), false));
        state.mark_presented(dirty);
        assert_eq!(state.presented_revision, dirty);
        assert_eq!(
            state.last_scheduler_reason,
            Some(NativeSchedulerReason::SurfaceChanged)
        );
    }

    #[test]
    fn demand_driven_scheduler_wakes_for_role_dirty_reason_without_branching_on_it() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);

        let dirty = state.mark_dirty(
            NativeSchedulerReason::ExternalWake,
            Some(NativeRoleDirtyReason::SourcePayloadAccepted),
        );

        assert!(state.should_render(Instant::now(), false));
        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::SourcePayloadAccepted)
        );
        state.mark_presented(dirty);
        assert!(!state.should_render(Instant::now(), false));
    }

    #[test]
    fn continuous_probe_scheduler_always_renders() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::ContinuousProbe);
        state.mark_presented(state.dirty_revision);

        assert!(state.should_render(Instant::now(), false));
    }

    #[test]
    fn wake_handle_changes_generation() {
        let wake_handle = NativeWakeHandle::new();
        assert_eq!(wake_handle.generation(), 0);
        assert_eq!(wake_handle.wake(), 1);
        assert_eq!(wake_handle.generation(), 1);
    }

    #[test]
    fn wake_handle_interrupts_idle_wait() {
        let wake_handle = NativeWakeHandle::new();
        let worker_wake = wake_handle.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            worker_wake.wake();
        });

        let observed = wake_handle.wait_for_wake_after(0, Duration::from_secs(5));

        assert_eq!(observed, 1);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn scheduled_wake_is_not_pushed_later_by_repeated_poll_results() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);
        let now = Instant::now();
        let first = state.schedule_wake_after(now, Duration::from_millis(500));
        let second =
            state.schedule_wake_after(now + Duration::from_millis(100), Duration::from_millis(500));

        assert_eq!(first, second);
        assert!(!state.consume_due_wake(now + Duration::from_millis(499)));
        assert!(state.consume_due_wake(now + Duration::from_millis(500)));
        assert_eq!(
            state.last_scheduler_reason,
            Some(NativeSchedulerReason::Timer)
        );
        assert!(state.should_render(now + Duration::from_millis(500), false));
    }

    #[test]
    fn input_cursor_accepts_events_only_after_role_update() {
        let mut cursor = NativeInputCursor::default();
        let input = NativeInputAdapterProof {
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
            }],
            keyboard_events: vec![NativeKeyboardEventProof {
                sequence: 11,
                key: "A".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
            }],
            mouse_scroll_event_count: 3,
            scroll_delta_x: 4.0,
            scroll_delta_y: 8.0,
            ..empty_input_adapter_proof(false)
        };

        assert_eq!(cursor.last_mouse_button_sequence, 0);
        cursor.accept(&input);

        assert_eq!(cursor.last_mouse_button_sequence, 7);
        assert_eq!(cursor.last_keyboard_sequence, 11);
        assert_eq!(cursor.last_mouse_scroll_event_count, 3);
    }
}
