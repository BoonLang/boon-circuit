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

const PASSIVE_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);

static READBACK_ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
    pub demand_driven_loop: bool,
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
    SurfaceLifecycle,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeSurfaceLifecycleReport {
    pub surface_epoch: u64,
    pub reconfigure_count: u64,
    pub resize_reconfigure_count: u64,
    pub lost_reconfigure_count: u64,
    pub outdated_reconfigure_count: u64,
    pub suboptimal_frame_count: u64,
    pub timeout_skip_count: u64,
    pub occluded_skip_count: u64,
    pub zero_size_skip_count: u64,
    pub validation_error_count: u64,
    pub final_width: u32,
    pub final_height: u32,
    pub last_lifecycle_event: Option<String>,
}

#[derive(Clone, Debug)]
struct NativeSurfaceLifecycleState {
    report: NativeSurfaceLifecycleReport,
    needs_suboptimal_reconfigure: bool,
}

impl NativeSurfaceLifecycleState {
    fn new(width: u32, height: u32) -> Self {
        Self {
            report: NativeSurfaceLifecycleReport {
                surface_epoch: 1,
                final_width: width,
                final_height: height,
                ..NativeSurfaceLifecycleReport::default()
            },
            needs_suboptimal_reconfigure: false,
        }
    }

    fn epoch(&self) -> u64 {
        self.report.surface_epoch
    }

    fn report(&self) -> &NativeSurfaceLifecycleReport {
        &self.report
    }

    fn reconfigured(&mut self, reason: &str, width: u32, height: u32) {
        self.report.surface_epoch = self.report.surface_epoch.saturating_add(1);
        self.report.reconfigure_count = self.report.reconfigure_count.saturating_add(1);
        self.report.final_width = width;
        self.report.final_height = height;
        self.report.last_lifecycle_event = Some(reason.to_owned());
        match reason {
            "resize" => {
                self.report.resize_reconfigure_count =
                    self.report.resize_reconfigure_count.saturating_add(1);
            }
            "lost" => {
                self.report.lost_reconfigure_count =
                    self.report.lost_reconfigure_count.saturating_add(1);
            }
            "outdated" => {
                self.report.outdated_reconfigure_count =
                    self.report.outdated_reconfigure_count.saturating_add(1);
            }
            "suboptimal" => {
                self.needs_suboptimal_reconfigure = false;
            }
            _ => {}
        }
    }

    fn note_suboptimal_frame(&mut self) {
        self.report.suboptimal_frame_count = self.report.suboptimal_frame_count.saturating_add(1);
        self.report.last_lifecycle_event = Some("suboptimal".to_owned());
        self.needs_suboptimal_reconfigure = true;
    }

    fn note_timeout_skip(&mut self) {
        self.report.timeout_skip_count = self.report.timeout_skip_count.saturating_add(1);
        self.report.last_lifecycle_event = Some("timeout_skip".to_owned());
    }

    fn note_occluded_skip(&mut self) {
        self.report.occluded_skip_count = self.report.occluded_skip_count.saturating_add(1);
        self.report.last_lifecycle_event = Some("occluded_skip".to_owned());
    }

    fn note_zero_size_skip(&mut self) {
        self.report.zero_size_skip_count = self.report.zero_size_skip_count.saturating_add(1);
        self.report.last_lifecycle_event = Some("zero_size_skip".to_owned());
    }

    fn note_validation_error(&mut self) {
        self.report.validation_error_count = self.report.validation_error_count.saturating_add(1);
        self.report.last_lifecycle_event = Some("validation_error".to_owned());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeRenderLoopState {
    pub mode: NativeRenderLoopMode,
    pub dirty_revision: u64,
    pub presented_revision: u64,
    pub last_render_content_revision: u64,
    pub rendered_frame_count: u64,
    pub skipped_idle_poll_count: u64,
    pub input_poll_count: u64,
    pub forced_frame_count: u64,
    pub scheduled_wake_count: u64,
    pub last_scheduler_reason: Option<NativeSchedulerReason>,
    pub last_role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub current_scheduler_reason: Option<NativeSchedulerReason>,
    pub current_role_dirty_reason: Option<NativeRoleDirtyReason>,
    #[serde(skip)]
    pub next_wake_at: Option<Instant>,
}

impl NativeRenderLoopState {
    pub fn new(mode: NativeRenderLoopMode) -> Self {
        Self {
            mode,
            dirty_revision: 1,
            presented_revision: 0,
            last_render_content_revision: 0,
            rendered_frame_count: 0,
            skipped_idle_poll_count: 0,
            input_poll_count: 0,
            forced_frame_count: 0,
            scheduled_wake_count: 0,
            last_scheduler_reason: Some(NativeSchedulerReason::FirstFrame),
            last_role_dirty_reason: None,
            current_scheduler_reason: Some(NativeSchedulerReason::FirstFrame),
            current_role_dirty_reason: None,
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
        if role_dirty_reason.is_some() {
            self.last_role_dirty_reason = role_dirty_reason;
        }
        self.current_scheduler_reason = Some(scheduler_reason);
        self.current_role_dirty_reason = role_dirty_reason;
        self.dirty_revision
    }

    pub fn mark_presented(&mut self, revision: u64) {
        self.presented_revision = self.presented_revision.max(revision);
        self.last_render_content_revision = self.last_render_content_revision.max(revision);
        self.rendered_frame_count = self.rendered_frame_count.saturating_add(1);
        if self.presented_revision >= self.dirty_revision {
            self.current_scheduler_reason = None;
            self.current_role_dirty_reason = None;
        }
    }

    pub fn mark_presented_with_content(&mut self, revision: u64, content_revision: u64) {
        self.presented_revision = self.presented_revision.max(revision);
        self.last_render_content_revision = content_revision;
        self.rendered_frame_count = self.rendered_frame_count.saturating_add(1);
        if self.presented_revision >= self.dirty_revision {
            self.current_scheduler_reason = None;
            self.current_role_dirty_reason = None;
        }
    }

    pub fn should_render(&self, now: Instant, _wake_generation_changed: bool) -> bool {
        if self.mode == NativeRenderLoopMode::ContinuousProbe {
            return true;
        }
        self.dirty_revision != self.presented_revision
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
            self.last_scheduler_reason = Some(NativeSchedulerReason::Timer);
            self.current_scheduler_reason = Some(NativeSchedulerReason::Timer);
            self.current_role_dirty_reason = None;
            self.scheduled_wake_count = self.scheduled_wake_count.saturating_add(1);
            true
        } else {
            false
        }
    }

    pub fn idle_wait_timeout(&self, now: Instant) -> Duration {
        self.next_wake_at
            .and_then(|wake_at| wake_at.checked_duration_since(now))
            .map(|timeout| timeout.min(PASSIVE_INPUT_POLL_INTERVAL))
            .unwrap_or(PASSIVE_INPUT_POLL_INTERVAL)
    }

    pub fn apply_poll_result(&mut self, poll_result: &NativePollResult, real_os_input: bool) {
        if poll_result.dirty {
            self.last_scheduler_reason = poll_result.scheduler_reason.or_else(|| {
                if real_os_input {
                    Some(NativeSchedulerReason::HostInput)
                } else {
                    Some(NativeSchedulerReason::ExternalWake)
                }
            });
            self.current_scheduler_reason = self.last_scheduler_reason;
            self.current_role_dirty_reason = poll_result.role_dirty_reason;
            if poll_result.role_dirty_reason.is_some() {
                self.last_role_dirty_reason = poll_result.role_dirty_reason;
            }
            let scheduler_only_repaint = poll_result.role_dirty_reason.is_none()
                && self.last_scheduler_reason == Some(NativeSchedulerReason::HostInput);
            if poll_result.role_revision > self.presented_revision {
                self.dirty_revision = self.dirty_revision.max(poll_result.role_revision);
            } else if self.last_scheduler_reason == Some(NativeSchedulerReason::VerifierFrame) {
                self.dirty_revision = self.dirty_revision.max(self.presented_revision);
            } else if scheduler_only_repaint {
                self.dirty_revision = self.dirty_revision.saturating_add(1);
            } else {
                self.dirty_revision = self.dirty_revision.max(poll_result.role_revision);
            }
        }
        if poll_result.wants_animation_frame {
            self.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePollResult {
    pub dirty: bool,
    pub role_revision: u64,
    pub scheduler_reason: Option<NativeSchedulerReason>,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub next_wake_after_ms: Option<u64>,
    pub wants_animation_frame: bool,
    pub cursor_icon: NativeCursorIcon,
}

impl NativePollResult {
    pub fn clean(role_revision: u64) -> Self {
        Self {
            dirty: false,
            role_revision,
            scheduler_reason: None,
            role_dirty_reason: None,
            next_wake_after_ms: None,
            wants_animation_frame: false,
            cursor_icon: NativeCursorIcon::Default,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCursorIcon {
    Default,
    ColumnResize,
    Pointer,
    Text,
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

    pub fn validate_for_presented_revision(&self, dirty_revision: u64) -> Result<(), String> {
        self.validate_for_presented_revision_with_scheduler(dirty_revision, None, None)
    }

    pub fn validate_for_presented_revision_with_scheduler(
        &self,
        dirty_revision: u64,
        scheduler_reason: Option<NativeSchedulerReason>,
        role_dirty_reason: Option<NativeRoleDirtyReason>,
    ) -> Result<(), String> {
        if !self.rendered {
            return Err("render hook result did not render a frame".to_owned());
        }
        if self.content_revision == 0 {
            return Err("render hook result content_revision must be nonzero".to_owned());
        }
        let same_content_surface_render = matches!(
            scheduler_reason,
            Some(NativeSchedulerReason::SurfaceChanged | NativeSchedulerReason::SurfaceLifecycle)
        );
        let scheduler_only_input_repaint = role_dirty_reason.is_none()
            && matches!(
                scheduler_reason,
                Some(NativeSchedulerReason::HostInput | NativeSchedulerReason::Timer)
            );
        let scheduler_idle_same_content_repaint =
            scheduler_reason.is_none() && role_dirty_reason.is_none() && !self.content_changed;
        if self.content_revision < dirty_revision
            && !same_content_surface_render
            && !scheduler_only_input_repaint
            && !scheduler_idle_same_content_repaint
        {
            return Err(format!(
                "render hook result content_revision {} is older than dirty_revision {}",
                self.content_revision, dirty_revision
            ));
        }
        Ok(())
    }

    pub fn presented_content_revision(
        &self,
        dirty_revision: u64,
        scheduler_reason: Option<NativeSchedulerReason>,
        role_dirty_reason: Option<NativeRoleDirtyReason>,
    ) -> u64 {
        if self.content_revision < dirty_revision
            && (matches!(
                scheduler_reason,
                Some(
                    NativeSchedulerReason::SurfaceChanged | NativeSchedulerReason::SurfaceLifecycle
                )
            ) || (role_dirty_reason.is_none()
                && matches!(
                    scheduler_reason,
                    Some(NativeSchedulerReason::HostInput | NativeSchedulerReason::Timer)
                ))
                || (scheduler_reason.is_none()
                    && role_dirty_reason.is_none()
                    && !self.content_changed))
        {
            dirty_revision
        } else {
            self.content_revision
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
    pub surface_lifecycle: NativeSurfaceLifecycleReport,
    pub resize_wake_count: u64,
    pub app_window_surface_content_report: Option<serde_json::Value>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presented_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_frame_count: Option<u64>,
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
    dyn for<'a> FnMut(NativeRenderFrameContext<'a>) -> Result<NativeRenderHookResult, String>
        + Send,
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
                    let _ = render_done_sender.try_send(());
                }
            })
            .expect("failed to spawn app_window render thread");

        match receiver.recv() {
            Ok(result) => on_ready(result),
            Err(_) => on_ready(Err(NativeWindowError::MissingProof)),
        }
        let _ = callback_done_sender.send(());
        let _ = render_done_receiver.recv();
        std::process::exit(0);
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
        eprintln!("boon_native_app_window: surface loop failed after ready: {error}");
        let _ = ready_sender.try_send(Err(error));
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
    let mut app_surface = window.surface().await;
    let resize_wake_count = Arc::new(AtomicU64::new(0));
    {
        let resize_wake_count = Arc::clone(&resize_wake_count);
        let resize_wake_handle = wake_handle.clone();
        app_surface.size_update(move |_| {
            resize_wake_count.fetch_add(1, Ordering::Relaxed);
            resize_wake_handle.wake();
        });
    }
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
    let loop_mode = if options.hold_ms == 0 || options.demand_driven_loop {
        NativeRenderLoopMode::DemandDriven
    } else {
        NativeRenderLoopMode::ContinuousProbe
    };
    let mut render_loop_state = NativeRenderLoopState::new(loop_mode);
    let mut surface_lifecycle = NativeSurfaceLifecycleState::new(width, height);

    for frame_index in 0..total_frame_count {
        let input = empty_input_adapter_proof(false);
        if let Some(poll_result) = poll_native_window_hooks(
            &mut hooks,
            NativePollContext {
                window_id: window_id.clone(),
                surface_id: surface_id.clone(),
                surface_epoch: surface_lifecycle.epoch(),
                width,
                height,
                scale: scale as f32,
                input_delta: input.clone(),
                now: Instant::now(),
                forced_frame: true,
            },
        )? {
            apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
            if let Some(next_wake_after_ms) = poll_result.next_wake_after_ms {
                render_loop_state
                    .schedule_wake_after(Instant::now(), Duration::from_millis(next_wake_after_ms));
            }
            render_loop_state.apply_poll_result(&poll_result, false);
        }
        let rendered_revision = render_loop_state.dirty_revision;
        let acquire_start = Instant::now();
        let Some(frame) = acquire_surface_texture_for_present(
            &surface,
            &device,
            &config,
            &mut surface_lifecycle,
            &mut render_loop_state,
            "probe loop",
        )?
        else {
            continue;
        };
        let current_surface_acquire_ms = elapsed_ms(acquire_start);
        let present_start = Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-app-window-probe-encoder"),
        });
        let mut rendered_content_revision = rendered_revision;
        let render_hook_ms = match hooks.as_mut() {
            Some(hooks) => {
                let render_start = Instant::now();
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    surface_id: surface_id.clone(),
                    surface_epoch: surface_lifecycle.epoch(),
                    surface_format: surface_format.clone(),
                    width,
                    height,
                    input,
                })
                .map_err(|error| {
                    NativeWindowError::Failed(format!("external render hook: {error}"))
                })?;
                if let Err(error) = render_result.validate_for_presented_revision_with_scheduler(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                ) {
                    surface_lifecycle.note_validation_error();
                    if let Some(report) = options.render_loop_state_report.as_deref() {
                        let _ = write_render_loop_state_report(
                            Path::new(report),
                            options.role,
                            std::process::id(),
                            &window_id,
                            &surface_id,
                            surface_lifecycle.report(),
                            &render_loop_state,
                            Duration::ZERO,
                            wake_handle.generation(),
                            None,
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                &app_surface,
                                None,
                            ),
                            Some(&error),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook: {error}"
                    )));
                }
                rendered_content_revision = render_result.presented_content_revision(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
                external_render_proof = Some(render_result.proof);
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
        render_loop_state.mark_presented_with_content(rendered_revision, rendered_content_revision);
        if let Some(report) = options.render_loop_state_report.as_deref() {
            write_render_loop_state_report(
                Path::new(report),
                options.role,
                std::process::id(),
                &window_id,
                &surface_id,
                surface_lifecycle.report(),
                &render_loop_state,
                Duration::ZERO,
                wake_handle.generation(),
                None,
                render_loop_report_extras(
                    resize_wake_count.load(Ordering::Relaxed),
                    &app_surface,
                    None,
                ),
                None,
            )?;
        }
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
    let mut input_adapter =
        sample_input_adapter(&mut mouse, &keyboard, options.synthetic_input_probe);
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
            merge_input_adapter_proof(&mut input_adapter, &frame_input);
            if let Some(poll_result) = poll_native_window_hooks(
                &mut hooks,
                NativePollContext {
                    window_id: window_id.clone(),
                    surface_id: surface_id.clone(),
                    surface_epoch: surface_lifecycle.epoch(),
                    width,
                    height,
                    scale: scale as f32,
                    input_delta: frame_input.clone(),
                    now: Instant::now(),
                    forced_frame: true,
                },
            )? {
                apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
                if let Some(next_wake_after_ms) = poll_result.next_wake_after_ms {
                    render_loop_state.schedule_wake_after(
                        Instant::now(),
                        Duration::from_millis(next_wake_after_ms),
                    );
                }
                render_loop_state
                    .apply_poll_result(&poll_result, frame_input.real_os_events_observed);
            }
            let rendered_revision = render_loop_state.dirty_revision;
            let acquire_start = Instant::now();
            let Some(frame) = acquire_surface_texture_for_present(
                &surface,
                &device,
                &config,
                &mut surface_lifecycle,
                &mut render_loop_state,
                "post-input sample",
            )?
            else {
                continue;
            };
            let current_surface_acquire_ms = elapsed_ms(acquire_start);
            let present_start = Instant::now();
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-native-app-window-input-sample-encoder"),
            });
            let mut rendered_content_revision = rendered_revision;
            if let Some(hooks) = hooks.as_mut() {
                let render_start = Instant::now();
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    surface_id: surface_id.clone(),
                    surface_epoch: surface_lifecycle.epoch(),
                    surface_format: surface_format.clone(),
                    width,
                    height,
                    input: frame_input,
                })
                .map_err(|error| {
                    NativeWindowError::Failed(format!("external render hook after input: {error}"))
                })?;
                if let Err(error) = render_result.validate_for_presented_revision_with_scheduler(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                ) {
                    surface_lifecycle.note_validation_error();
                    if let Some(report) = options.render_loop_state_report.as_deref() {
                        let _ = write_render_loop_state_report(
                            Path::new(report),
                            options.role,
                            std::process::id(),
                            &window_id,
                            &surface_id,
                            surface_lifecycle.report(),
                            &render_loop_state,
                            Duration::ZERO,
                            wake_handle.generation(),
                            None,
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                &app_surface,
                                Some(&input_adapter),
                            ),
                            Some(&error),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook after input: {error}"
                    )));
                }
                rendered_content_revision = render_result.presented_content_revision(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
                external_render_proof = Some(render_result.proof);
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
            render_loop_state
                .mark_presented_with_content(rendered_revision, rendered_content_revision);
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

    let mut observed_input_adapter = input_adapter.clone();
    let proof = AppWindowSurfaceProof {
        role: options.role.as_str().to_owned(),
        pid: std::process::id(),
        main_thread_id,
        render_thread_id: thread_id_string(),
        display_server: display_server(),
        display_connection: display_connection(),
        window_backend: "app_window-wayland".to_owned(),
        window_title: options.title.clone(),
        window_id: window_id.clone(),
        surface_id: surface_id.clone(),
        surface_epoch: surface_lifecycle.epoch(),
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
        surface_lifecycle: surface_lifecycle.report().clone(),
        resize_wake_count: resize_wake_count.load(Ordering::Relaxed),
        app_window_surface_content_report: app_window_surface_content_report(&app_surface),
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
    let mut last_interactive_readback_artifact: Option<AppWindowReadbackArtifact> = None;
    loop {
        if options.hold_ms > 0 && hold_started.elapsed() >= Duration::from_millis(options.hold_ms) {
            break;
        }
        let (current_size, current_scale) = app_surface.size_scale().await;
        let raw_width = (current_size.width() * current_scale).round();
        let raw_height = (current_size.height() * current_scale).round();
        if raw_width <= 0.0 || raw_height <= 0.0 {
            surface_lifecycle.note_zero_size_skip();
            render_loop_state.note_idle_poll();
            let _ = wake_handle.wait_for_wake_after(
                last_wake_generation,
                render_loop_state.idle_wait_timeout(Instant::now()),
            );
            continue;
        }
        let current_width = raw_width as u32;
        let current_height = raw_height as u32;
        if current_width != width || current_height != height {
            width = current_width;
            height = current_height;
            config.width = width;
            config.height = height;
            surface.configure(&device, &config);
            surface_lifecycle.reconfigured("resize", width, height);
            render_loop_state.mark_dirty(NativeSchedulerReason::SurfaceChanged, None);
        } else if surface_lifecycle.needs_suboptimal_reconfigure {
            surface.configure(&device, &config);
            surface_lifecycle.reconfigured("suboptimal", width, height);
            render_loop_state.mark_dirty(NativeSchedulerReason::SurfaceLifecycle, None);
        }
        let poll_started_at = Instant::now();
        render_loop_state.consume_due_wake(poll_started_at);
        let input = sample_input_adapter_delta(&mut mouse, &keyboard, &input_cursor, false);
        merge_input_adapter_proof(&mut observed_input_adapter, &input);
        render_loop_state.note_input_poll();
        let poll_result = poll_native_window_hooks(
            &mut hooks,
            NativePollContext {
                window_id: window_id.clone(),
                surface_id: surface_id.clone(),
                surface_epoch: surface_lifecycle.epoch(),
                width,
                height,
                scale: current_scale as f32,
                input_delta: input.clone(),
                now: poll_started_at,
                forced_frame: false,
            },
        )?;
        if let Some(poll_result) = poll_result {
            apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
            if let Some(next_wake_after_ms) = poll_result.next_wake_after_ms {
                render_loop_state.schedule_wake_after(
                    poll_started_at,
                    Duration::from_millis(next_wake_after_ms),
                );
            }
            render_loop_state.apply_poll_result(&poll_result, input.real_os_events_observed);
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        } else if input.real_os_events_observed {
            render_loop_state.mark_dirty(NativeSchedulerReason::HostInput, None);
        }
        let wake_generation = wake_handle.generation();
        let wake_generation_changed = wake_generation != last_wake_generation;
        if wake_generation_changed {
            last_wake_generation = wake_generation;
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::ExternalWake);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            continue;
        }
        if !render_loop_state.should_render(Instant::now(), false) {
            render_loop_state.note_idle_poll();
            let idle_timeout = render_loop_state.idle_wait_timeout(Instant::now());
            let _ = wake_handle.wait_for_wake_after(last_wake_generation, idle_timeout);
            continue;
        }
        if hooks.as_ref().is_none_or(|hooks| hooks.poll.is_none()) {
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        }
        let rendered_revision = render_loop_state.dirty_revision;
        let Some(frame) = acquire_surface_texture_for_present(
            &surface,
            &device,
            &config,
            &mut surface_lifecycle,
            &mut render_loop_state,
            "interactive loop",
        )?
        else {
            continue;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-app-window-interactive-encoder"),
        });
        let mut rendered_content_revision = rendered_revision;
        match hooks.as_mut() {
            Some(hooks) => {
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    surface_id: surface_id.clone(),
                    surface_epoch: surface_lifecycle.epoch(),
                    surface_format: surface_format.clone(),
                    width,
                    height,
                    input: input.clone(),
                })
                .map_err(|error| {
                    NativeWindowError::Failed(format!("external render hook: {error}"))
                })?;
                if let Err(error) = render_result.validate_for_presented_revision_with_scheduler(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                ) {
                    surface_lifecycle.note_validation_error();
                    if let Some(report) = options.render_loop_state_report.as_deref() {
                        let _ = write_render_loop_state_report(
                            Path::new(report),
                            options.role,
                            std::process::id(),
                            &window_id,
                            &surface_id,
                            surface_lifecycle.report(),
                            &render_loop_state,
                            hold_started.elapsed(),
                            wake_handle.generation(),
                            last_interactive_readback_artifact.as_ref(),
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                &app_surface,
                                Some(&observed_input_adapter),
                            ),
                            Some(&error),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook: {error}"
                    )));
                }
                rendered_content_revision = render_result.presented_content_revision(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
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
        let interactive_readback = if options.role == NativeWindowRole::Preview {
            if let Some(artifact_dir) = options.readback_artifact_dir.as_deref() {
                Some((
                    artifact_dir.to_owned(),
                    queue_visible_surface_readback(
                        &device,
                        &mut encoder,
                        &frame.texture,
                        options.role,
                        width.min(480),
                        height.min(260),
                        config.format,
                        &options.title,
                    )?,
                ))
            } else {
                None
            }
        } else {
            None
        };
        queue.submit(Some(encoder.finish()));
        frame.present();
        render_loop_state.mark_presented_with_content(rendered_revision, rendered_content_revision);
        if let Some(report) = options.render_loop_state_report.as_deref() {
            write_render_loop_state_report(
                Path::new(report),
                options.role,
                std::process::id(),
                &window_id,
                &surface_id,
                surface_lifecycle.report(),
                &render_loop_state,
                hold_started.elapsed(),
                wake_handle.generation(),
                last_interactive_readback_artifact.as_ref(),
                render_loop_report_extras(
                    resize_wake_count.load(Ordering::Relaxed),
                    &app_surface,
                    Some(&observed_input_adapter),
                ),
                None,
            )?;
        }
        if let Some((artifact_dir, pending)) = interactive_readback {
            let mut artifact = finish_visible_surface_readback(&device, pending, &artifact_dir)?;
            artifact.presented_revision = Some(render_loop_state.presented_revision);
            artifact.content_revision = Some(render_loop_state.last_render_content_revision);
            artifact.rendered_frame_count = Some(render_loop_state.rendered_frame_count);
            last_interactive_readback_artifact = Some(artifact);
            if let Some(report) = options.render_loop_state_report.as_deref() {
                write_render_loop_state_report(
                    Path::new(report),
                    options.role,
                    std::process::id(),
                    &window_id,
                    &surface_id,
                    surface_lifecycle.report(),
                    &render_loop_state,
                    hold_started.elapsed(),
                    wake_handle.generation(),
                    last_interactive_readback_artifact.as_ref(),
                    render_loop_report_extras(
                        resize_wake_count.load(Ordering::Relaxed),
                        &app_surface,
                        Some(&observed_input_adapter),
                    ),
                    None,
                )?;
            }
        }
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
            surface_lifecycle.report(),
            &render_loop_state,
            hold_started.elapsed(),
            wake_handle.generation(),
            last_interactive_readback_artifact.as_ref(),
            render_loop_report_extras(
                resize_wake_count.load(Ordering::Relaxed),
                &app_surface,
                Some(&observed_input_adapter),
            ),
            None,
        )?;
    }
    let callback_done_timeout =
        Duration::from_millis(options.hold_ms.max(2_000)).saturating_add(Duration::from_secs(240));
    let _ = callback_done_receiver.recv_timeout(callback_done_timeout);
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

fn acquire_surface_texture_for_present(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    lifecycle: &mut NativeSurfaceLifecycleState,
    render_loop_state: &mut NativeRenderLoopState,
    context: &str,
) -> Result<Option<wgpu::SurfaceTexture>, NativeWindowError> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Some(frame)),
        wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
            lifecycle.note_suboptimal_frame();
            Ok(Some(frame))
        }
        wgpu::CurrentSurfaceTexture::Timeout => {
            lifecycle.note_timeout_skip();
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Occluded => {
            lifecycle.note_occluded_skip();
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            surface.configure(device, config);
            lifecycle.reconfigured("outdated", config.width, config.height);
            render_loop_state.mark_dirty(NativeSchedulerReason::SurfaceLifecycle, None);
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            surface.configure(device, config);
            lifecycle.reconfigured("lost", config.width, config.height);
            render_loop_state.mark_dirty(NativeSchedulerReason::SurfaceLifecycle, None);
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Validation => {
            lifecycle.note_validation_error();
            Err(NativeWindowError::Failed(format!(
                "get_current_texture validation error during {context}"
            )))
        }
    }
}

#[derive(Clone, Debug, Default)]
struct NativeRenderLoopReportExtras {
    resize_wake_count: u64,
    app_window_surface_content_report: Option<serde_json::Value>,
    observed_input_adapter: Option<NativeInputAdapterProof>,
}

fn render_loop_report_extras(
    resize_wake_count: u64,
    app_surface: &app_window::surface::Surface,
    observed_input_adapter: Option<&NativeInputAdapterProof>,
) -> NativeRenderLoopReportExtras {
    NativeRenderLoopReportExtras {
        resize_wake_count,
        app_window_surface_content_report: app_window_surface_content_report(app_surface),
        observed_input_adapter: observed_input_adapter.cloned(),
    }
}

fn app_window_surface_content_report(
    surface: &app_window::surface::Surface,
) -> Option<serde_json::Value> {
    #[cfg(target_os = "linux")]
    {
        let report = surface.content_report();
        Some(serde_json::json!({
            "external_surface_created": report.external_surface_created,
            "shm_content_attach_count": report.shm_content_attach_count,
            "shm_content_attach_after_external_surface_count": report
                .shm_content_attach_after_external_surface_count,
            "external_surface_configure_skip_count": report.external_surface_configure_skip_count
        }))
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = surface;
        None
    }
}

fn apply_native_cursor_icon(surface: &app_window::surface::Surface, icon: NativeCursorIcon) {
    #[cfg(target_os = "linux")]
    {
        let surface_icon = match icon {
            NativeCursorIcon::Default => app_window::surface::SurfaceCursorIcon::Default,
            NativeCursorIcon::ColumnResize => app_window::surface::SurfaceCursorIcon::ColumnResize,
            NativeCursorIcon::Pointer => app_window::surface::SurfaceCursorIcon::Pointer,
            NativeCursorIcon::Text => app_window::surface::SurfaceCursorIcon::Text,
        };
        surface.set_cursor_icon(surface_icon);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = surface;
        let _ = icon;
    }
}

fn write_render_loop_state_report(
    path: &Path,
    role: NativeWindowRole,
    pid: u32,
    window_id: &WindowId,
    surface_id: &SurfaceId,
    surface_lifecycle: &NativeSurfaceLifecycleReport,
    state: &NativeRenderLoopState,
    elapsed: Duration,
    wake_generation: u64,
    last_interactive_readback_artifact: Option<&AppWindowReadbackArtifact>,
    extras: NativeRenderLoopReportExtras,
    loop_error: Option<&str>,
) -> Result<(), NativeWindowError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            NativeWindowError::Failed(format!(
                "create render-loop report dir {}: {error}",
                parent.display()
            ))
        })?;
    }
    let status = if loop_error.is_some() { "fail" } else { "pass" };
    let elapsed_seconds = elapsed.as_secs_f64().max(0.001);
    let input_polls_per_second = state.input_poll_count as f64 / elapsed_seconds;
    let renders_per_second = state.rendered_frame_count as f64 / elapsed_seconds;
    let active_timer_reason = state.next_wake_at.map(|_| {
        if state.current_scheduler_reason == Some(NativeSchedulerReason::Timer) {
            "timer_due"
        } else {
            "scheduled_wake"
        }
    });
    let report = serde_json::json!({
        "status": status,
        "role": role.as_str(),
        "pid": pid,
        "window_id": window_id,
        "surface_id": surface_id,
        "surface_epoch": surface_lifecycle.surface_epoch,
        "surface_lifecycle": surface_lifecycle,
        "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
        "wake_generation": wake_generation,
        "render_loop_state": state,
        "render_loop_mode": state.mode,
        "dirty_revision": state.dirty_revision,
        "presented_revision": state.presented_revision,
        "last_render_content_revision": state.last_render_content_revision,
        "rendered_frame_count": state.rendered_frame_count,
        "skipped_idle_poll_count": state.skipped_idle_poll_count,
        "input_poll_count": state.input_poll_count,
        "input_polls_per_second": input_polls_per_second,
        "forced_frame_count": state.forced_frame_count,
        "renders_per_second": renders_per_second,
        "scheduled_wake_count": state.scheduled_wake_count,
        "active_timer_reason": active_timer_reason,
        "passive_input_poll_interval_ms": PASSIVE_INPUT_POLL_INTERVAL.as_millis() as u64,
        "resize_wake_count": extras.resize_wake_count,
        "app_window_surface_content_report": extras.app_window_surface_content_report,
        "observed_input_adapter": extras.observed_input_adapter,
        "last_scheduler_reason": state.last_scheduler_reason,
        "last_role_dirty_reason": state.last_role_dirty_reason,
        "current_scheduler_reason": state.current_scheduler_reason,
        "current_role_dirty_reason": state.current_role_dirty_reason,
        "loop_error": loop_error,
        "last_interactive_readback_artifact": last_interactive_readback_artifact
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
        "{}-{}-{}-{}.png",
        std::process::id(),
        pending.role.as_str(),
        stable_debug_hash(&pending.title),
        READBACK_ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
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
        presented_revision: None,
        content_revision: None,
        rendered_frame_count: None,
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
    let pressed_keys = sample_pressed_keyboard_keys(keyboard);
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
    let pressed_keys = sample_pressed_keyboard_keys(keyboard);
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

fn sample_pressed_keyboard_keys(keyboard: &Keyboard) -> Vec<String> {
    [
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
        KeyboardKey::Num0,
        KeyboardKey::Num1,
        KeyboardKey::Num2,
        KeyboardKey::Num3,
        KeyboardKey::Num4,
        KeyboardKey::Num5,
        KeyboardKey::Num6,
        KeyboardKey::Num7,
        KeyboardKey::Num8,
        KeyboardKey::Num9,
        KeyboardKey::Keypad0,
        KeyboardKey::Keypad1,
        KeyboardKey::Keypad2,
        KeyboardKey::Keypad3,
        KeyboardKey::Keypad4,
        KeyboardKey::Keypad5,
        KeyboardKey::Keypad6,
        KeyboardKey::Keypad7,
        KeyboardKey::Keypad8,
        KeyboardKey::Keypad9,
        KeyboardKey::Space,
        KeyboardKey::Minus,
        KeyboardKey::Equal,
        KeyboardKey::Comma,
        KeyboardKey::Period,
        KeyboardKey::Slash,
        KeyboardKey::Semicolon,
        KeyboardKey::Quote,
        KeyboardKey::LeftBracket,
        KeyboardKey::RightBracket,
        KeyboardKey::Backslash,
        KeyboardKey::InternationalBackslash,
        KeyboardKey::Grave,
        KeyboardKey::KeypadDecimal,
        KeyboardKey::KeypadMinus,
        KeyboardKey::KeypadPlus,
        KeyboardKey::KeypadDivide,
        KeyboardKey::KeypadMultiply,
        KeyboardKey::KeypadEquals,
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
    .collect()
}

fn merge_input_adapter_proof(base: &mut NativeInputAdapterProof, sample: &NativeInputAdapterProof) {
    base.sampled_after_visible_window |= sample.sampled_after_visible_window;
    base.real_os_events_observed |= sample.real_os_events_observed;
    base.mouse_motion_event_count = base
        .mouse_motion_event_count
        .max(sample.mouse_motion_event_count);
    base.mouse_button_event_count = base
        .mouse_button_event_count
        .max(sample.mouse_button_event_count);
    base.mouse_scroll_event_count = base
        .mouse_scroll_event_count
        .max(sample.mouse_scroll_event_count);
    base.mouse_total_event_count = base
        .mouse_total_event_count
        .max(sample.mouse_total_event_count);
    base.keyboard_key_event_count = base
        .keyboard_key_event_count
        .max(sample.keyboard_key_event_count);
    if sample.mouse_last_window_protocol_id.is_some() {
        base.mouse_last_window_protocol_id = sample.mouse_last_window_protocol_id;
    }
    if sample.keyboard_last_window_protocol_id.is_some() {
        base.keyboard_last_window_protocol_id = sample.keyboard_last_window_protocol_id;
    }
    if sample.mouse_window_pos.is_some() {
        base.mouse_window_pos = sample.mouse_window_pos.clone();
    }
    base.scroll_delta_x += sample.scroll_delta_x;
    base.scroll_delta_y += sample.scroll_delta_y;
    for button in &sample.mouse_buttons_down {
        if !base.mouse_buttons_down.contains(button) {
            base.mouse_buttons_down.push(button.clone());
        }
    }
    for key in &sample.pressed_keys {
        if !base.pressed_keys.contains(key) {
            base.pressed_keys.push(key.clone());
        }
    }
    for event in &sample.mouse_button_events {
        if !base
            .mouse_button_events
            .iter()
            .any(|existing| existing.sequence == event.sequence)
        {
            base.mouse_button_events.push(event.clone());
        }
    }
    for event in &sample.keyboard_events {
        if !base
            .keyboard_events
            .iter()
            .any(|existing| existing.sequence == event.sequence)
        {
            base.keyboard_events.push(event.clone());
        }
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
    fn demand_driven_idle_wait_uses_slow_passive_input_poll() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        assert_eq!(state.idle_wait_timeout(now), PASSIVE_INPUT_POLL_INTERVAL);

        state.schedule_wake_after(now, Duration::from_millis(30));
        assert_eq!(state.idle_wait_timeout(now), Duration::from_millis(30));
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
    fn surface_lifecycle_reconfigure_increments_epoch_and_records_reason() {
        let mut lifecycle = NativeSurfaceLifecycleState::new(800, 600);
        assert_eq!(lifecycle.epoch(), 1);

        lifecycle.reconfigured("resize", 1024, 768);

        assert_eq!(lifecycle.epoch(), 2);
        assert_eq!(lifecycle.report().resize_reconfigure_count, 1);
        assert_eq!(lifecycle.report().final_width, 1024);
        assert_eq!(lifecycle.report().final_height, 768);
        assert_eq!(
            lifecycle.report().last_lifecycle_event.as_deref(),
            Some("resize")
        );
    }

    #[test]
    fn surface_lifecycle_skips_nonpresentable_frames_without_epoch_commit() {
        let mut lifecycle = NativeSurfaceLifecycleState::new(800, 600);

        lifecycle.note_timeout_skip();
        lifecycle.note_occluded_skip();
        lifecycle.note_zero_size_skip();

        assert_eq!(lifecycle.epoch(), 1);
        assert_eq!(lifecycle.report().timeout_skip_count, 1);
        assert_eq!(lifecycle.report().occluded_skip_count, 1);
        assert_eq!(lifecycle.report().zero_size_skip_count, 1);
        assert_eq!(
            lifecycle.report().last_lifecycle_event.as_deref(),
            Some("zero_size_skip")
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
    fn scheduler_preserves_previous_role_dirty_reason_when_later_poll_has_no_role_reason() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision.saturating_add(1),
                scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
                role_dirty_reason: Some(NativeRoleDirtyReason::SourcePayloadAccepted),
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
            },
            false,
        );
        state.mark_presented(state.dirty_revision);
        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::VerifierFrame),
                role_dirty_reason: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
            },
            false,
        );

        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::SourcePayloadAccepted)
        );
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
        assert!(!state.should_render(now + Duration::from_millis(500), false));
    }

    #[test]
    fn poll_result_uses_role_revision_as_presentable_dirty_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let poll = NativePollResult {
            dirty: true,
            role_revision: 1,
            scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
            role_dirty_reason: Some(NativeRoleDirtyReason::DocumentPatchApplied),
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
        };

        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 1);
        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::DocumentPatchApplied)
        );

        state.mark_presented(1);
        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 1);
        assert!(!state.should_render(Instant::now(), false));
    }

    #[test]
    fn stale_role_dirty_poll_does_not_invent_unrenderable_content_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
            },
            true,
        );

        assert_eq!(state.dirty_revision, state.presented_revision);
        assert!(!state.should_render(Instant::now(), false));
        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::ScrollChanged)
        );
    }

    #[test]
    fn verifier_frame_does_not_invent_new_content_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let poll = NativePollResult {
            dirty: true,
            role_revision: 0,
            scheduler_reason: Some(NativeSchedulerReason::VerifierFrame),
            role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
        };

        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 1);
        assert!(
            NativeRenderHookResult {
                proof: serde_json::json!({}),
                content_revision: 1,
                rendered: true,
                content_changed: false,
                role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
            }
            .validate_for_presented_revision(state.dirty_revision)
            .is_ok()
        );
    }

    #[test]
    fn structured_render_result_rejects_stale_or_missing_revisions() {
        let mut zero = NativeRenderHookResult::rendered_with_proof(serde_json::json!({}));
        assert!(zero.validate_for_presented_revision(1).is_err());

        zero.content_revision = 1;
        assert!(zero.validate_for_presented_revision(2).is_err());

        zero.content_revision = 2;
        zero.rendered = false;
        assert!(zero.validate_for_presented_revision(2).is_err());

        zero.rendered = true;
        assert!(zero.validate_for_presented_revision(2).is_ok());
    }

    #[test]
    fn surface_dirty_revision_can_present_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: serde_json::json!({}),
            content_revision: 1,
            rendered: true,
            content_changed: false,
            role_dirty_reason: None,
        };

        assert!(
            render
                .validate_for_presented_revision_with_scheduler(
                    2,
                    Some(NativeSchedulerReason::SurfaceChanged),
                    None,
                )
                .is_ok(),
            "surface resize should be allowed to repaint unchanged document content"
        );
        assert_eq!(
            render.presented_content_revision(2, Some(NativeSchedulerReason::SurfaceChanged), None),
            2
        );
        assert!(
            render
                .validate_for_presented_revision_with_scheduler(
                    2,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::SourcePayloadAccepted),
                )
                .is_err(),
            "runtime/source wakes must still reject stale content revisions"
        );
    }

    #[test]
    fn scheduler_only_host_input_can_repaint_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: serde_json::json!({}),
            content_revision: 2,
            rendered: true,
            content_changed: false,
            role_dirty_reason: None,
        };

        assert!(
            render
                .validate_for_presented_revision_with_scheduler(
                    3,
                    Some(NativeSchedulerReason::HostInput),
                    None,
                )
                .is_ok(),
            "focus/activation/mouse movement can repaint without semantic content changes"
        );
        assert_eq!(
            render.presented_content_revision(3, Some(NativeSchedulerReason::HostInput), None),
            3
        );
        assert!(
            render
                .validate_for_presented_revision_with_scheduler(
                    3,
                    Some(NativeSchedulerReason::HostInput),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_err(),
            "real runtime input must still advance the content revision"
        );
    }

    #[test]
    fn idle_same_content_frame_can_repaint_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: serde_json::json!({}),
            content_revision: 4,
            rendered: true,
            content_changed: false,
            role_dirty_reason: None,
        };

        assert!(
            render
                .validate_for_presented_revision_with_scheduler(5, None, None)
                .is_ok(),
            "continuous verifier frames may repaint unchanged already-presented content"
        );
        assert_eq!(render.presented_content_revision(5, None, None), 5);

        let changed = NativeRenderHookResult {
            content_changed: true,
            ..render
        };
        assert!(
            changed
                .validate_for_presented_revision_with_scheduler(5, None, None)
                .is_err(),
            "new content without a scheduler/role reason must not be backdated"
        );
    }

    #[test]
    fn scheduler_only_repaint_ignores_sticky_previous_role_dirty_reason() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);
        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision.saturating_add(1),
                scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
                role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
            },
            false,
        );
        let semantic_revision = state.dirty_revision;
        state.mark_presented(semantic_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: semantic_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
            },
            true,
        );

        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            "reporting should preserve the last semantic role dirty reason"
        );
        assert_eq!(state.current_role_dirty_reason, None);
        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert!(
            (NativeRenderHookResult {
                proof: serde_json::json!({}),
                content_revision: semantic_revision,
                rendered: true,
                content_changed: false,
                role_dirty_reason: None,
            })
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                state.current_scheduler_reason,
                state.current_role_dirty_reason,
            )
            .is_ok(),
            "host focus/mouse repaint must not be rejected because of a previous runtime dirty reason"
        );
    }

    #[test]
    fn presented_state_records_render_content_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

        state.mark_presented_with_content(1, 3);

        assert_eq!(state.presented_revision, 1);
        assert_eq!(state.last_render_content_revision, 3);
        assert_eq!(state.rendered_frame_count, 1);
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
