#![recursion_limit = "256"]

use app_window::coordinates::{Position, Size};
use app_window::input::keyboard::{Keyboard, key::KeyboardKey};
use app_window::input::mouse::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT, Mouse};
use app_window::window::Window;
use app_window::{WGPU_SURFACE_STRATEGY, WGPUStrategy};
use boon_host::{PhysicalSize, SurfaceId, Viewport, WindowId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};
use wgpu::SurfaceTargetUnsafe;

const PASSIVE_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const VISIBLE_SURFACE_READBACK_TIMEOUT: Duration = Duration::from_secs(5);
const NATIVE_WINDOW_RENDER_THREAD_STACK_BYTES: usize = 32 * 1024 * 1024;

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

#[derive(Debug)]
pub struct NativeAccessibilitySnapshot {
    pub tree_update: accesskit::TreeUpdate,
    pub metrics: NativeAccessibilityMetrics,
    pub semantic_node_ids: Vec<NativeAccessibilityNodeMapping>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeAccessibilityNodeMapping {
    pub semantic_id: String,
    pub accesskit_node_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeAccessibilityActionRequest {
    pub target_node_id: u64,
    pub action: NativeAccessibilityAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeWorldEditorSessionActionReport {
    pub dispatch: boon_document::SemanticSourceDispatch,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_report: Option<boon_scene_model::WorldEditorSessionActionReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeAccessibilityAction {
    Focus,
    Blur,
    Click,
    SetValue,
    ReplaceSelectedText,
    Increment,
    Decrement,
    ScrollIntoView,
    Other(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeAccessibilityMetrics {
    pub semantic_node_count: usize,
    pub accesskit_node_count: usize,
    pub interactive_node_count: usize,
    pub focusable_node_count: usize,
    pub text_input_node_count: usize,
    pub checked_node_count: usize,
    pub node_id_collision_count: usize,
    pub root_present: bool,
    pub focus_present: bool,
}

pub fn accesskit_tree_update_from_semantic_scene(
    scene: &boon_document::SemanticScene,
    toolkit_name: impl Into<String>,
    toolkit_version: impl Into<String>,
) -> NativeAccessibilitySnapshot {
    let id_map = accesskit_node_id_map(scene);
    let root = scene
        .root
        .as_ref()
        .and_then(|id| id_map.get(id))
        .copied()
        .or_else(|| {
            scene
                .nodes
                .keys()
                .next()
                .and_then(|id| id_map.get(id).copied())
        })
        .unwrap_or(accesskit::NodeId(1));
    let focus = scene
        .focused
        .as_ref()
        .and_then(|id| id_map.get(id))
        .copied()
        .unwrap_or(root);
    let mut nodes = Vec::with_capacity(scene.nodes.len());
    for semantic in scene.nodes.values() {
        let node_id = id_map
            .get(&semantic.id)
            .copied()
            .unwrap_or_else(|| accesskit_node_id_for_semantic_id(&semantic.id));
        let mut node = accesskit_node_from_semantic_node(semantic, &id_map);
        let child_ids = semantic
            .relations
            .children
            .iter()
            .filter_map(|child| id_map.get(child).copied())
            .collect::<Vec<_>>();
        if !child_ids.is_empty() {
            node.set_children(child_ids);
        }
        nodes.push((node_id, node));
    }
    let tree_update = accesskit::TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree {
            root,
            toolkit_name: Some(toolkit_name.into()),
            toolkit_version: Some(toolkit_version.into()),
        }),
        tree_id: accesskit::TreeId::ROOT,
        focus,
    };
    let semantic_node_ids = scene
        .nodes
        .keys()
        .filter_map(|id| {
            id_map
                .get(id)
                .map(|node_id| NativeAccessibilityNodeMapping {
                    semantic_id: id.0.clone(),
                    accesskit_node_id: node_id.0,
                })
        })
        .collect::<Vec<_>>();
    let unique_ids = id_map.values().copied().collect::<BTreeSet<_>>().len();
    let metrics = NativeAccessibilityMetrics {
        semantic_node_count: scene.nodes.len(),
        accesskit_node_count: tree_update.nodes.len(),
        interactive_node_count: scene
            .nodes
            .values()
            .filter(|node| node.actions.press || node.actions.set_text)
            .count(),
        focusable_node_count: scene
            .nodes
            .values()
            .filter(|node| node.actions.focus || node.state.focused)
            .count(),
        text_input_node_count: scene
            .nodes
            .values()
            .filter(|node| node.actions.set_text)
            .count(),
        checked_node_count: scene
            .nodes
            .values()
            .filter(|node| node.state.checked == Some(true))
            .count(),
        node_id_collision_count: scene.nodes.len().saturating_sub(unique_ids),
        root_present: scene
            .root
            .as_ref()
            .is_some_and(|id| id_map.contains_key(id)),
        focus_present: scene
            .focused
            .as_ref()
            .is_some_and(|id| id_map.contains_key(id)),
    };
    NativeAccessibilitySnapshot {
        tree_update,
        metrics,
        semantic_node_ids,
    }
}

fn accesskit_node_id_map(
    scene: &boon_document::SemanticScene,
) -> BTreeMap<boon_document::SemanticId, accesskit::NodeId> {
    scene
        .nodes
        .keys()
        .map(|id| (id.clone(), accesskit_node_id_for_semantic_id(id)))
        .collect()
}

fn accesskit_node_id_for_semantic_id(id: &boon_document::SemanticId) -> accesskit::NodeId {
    let digest = Sha256::digest(id.0.as_bytes());
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let mut value = u64::from_le_bytes(bytes);
    if value == 0 {
        value = 1;
    }
    accesskit::NodeId(value)
}

pub fn native_accessibility_action_requests_from_accesskit(
    requests: Vec<accesskit::ActionRequest>,
) -> Vec<NativeAccessibilityActionRequest> {
    requests
        .into_iter()
        .map(|request| NativeAccessibilityActionRequest {
            target_node_id: request.target_node.0,
            action: native_accessibility_action_from_accesskit(request.action),
            value: native_accessibility_action_value_from_accesskit(request.data),
        })
        .collect()
}

pub fn native_accessibility_source_dispatches_from_requests(
    scene: &boon_document::SemanticScene,
    requests: &[NativeAccessibilityActionRequest],
) -> Vec<boon_document::SemanticSourceDispatch> {
    let native_to_semantic = accesskit_node_id_map(scene)
        .into_iter()
        .map(|(semantic_id, node_id)| (node_id.0, semantic_id))
        .collect::<BTreeMap<_, _>>();
    requests
        .iter()
        .filter_map(|request| {
            let semantic_id = native_to_semantic.get(&request.target_node_id)?.clone();
            let event = native_accessibility_semantic_input_event(semantic_id, request)?;
            scene.source_dispatch_for_event(event)
        })
        .collect()
}

pub fn native_accessibility_world_editor_session_reports_from_requests(
    scene: &boon_document::SemanticScene,
    requests: &[NativeAccessibilityActionRequest],
    session: &mut boon_scene_model::WorldEditorSession,
    bundle: &boon_solid_model::SolidModelBundle,
) -> Vec<NativeWorldEditorSessionActionReport> {
    native_accessibility_source_dispatches_from_requests(scene, requests)
        .into_iter()
        .map(|dispatch| {
            let action = boon_scene_model::WorldEditorSourceAction {
                source_path: dispatch.source_path.clone(),
                source_intent: dispatch.source_intent.clone(),
            };
            match session.handle_source_action(bundle, &action) {
                Ok(session_report) => NativeWorldEditorSessionActionReport {
                    dispatch,
                    session_report: Some(session_report),
                    error: None,
                },
                Err(error) => NativeWorldEditorSessionActionReport {
                    dispatch,
                    session_report: None,
                    error: Some(error),
                },
            }
        })
        .collect()
}

fn native_accessibility_semantic_input_event(
    semantic_id: boon_document::SemanticId,
    request: &NativeAccessibilityActionRequest,
) -> Option<boon_document::SemanticInputEvent> {
    match request.action {
        NativeAccessibilityAction::Focus => {
            Some(boon_document::SemanticInputEvent::Focus { semantic_id })
        }
        NativeAccessibilityAction::Click => {
            Some(boon_document::SemanticInputEvent::Press { semantic_id })
        }
        NativeAccessibilityAction::SetValue => Some(boon_document::SemanticInputEvent::SetText {
            semantic_id,
            text: request.value.clone().unwrap_or_default(),
        }),
        NativeAccessibilityAction::ReplaceSelectedText => {
            Some(boon_document::SemanticInputEvent::ReplaceSelectedText {
                semantic_id,
                text: request.value.clone().unwrap_or_default(),
            })
        }
        NativeAccessibilityAction::Increment => {
            Some(boon_document::SemanticInputEvent::Increment { semantic_id })
        }
        NativeAccessibilityAction::Decrement => {
            Some(boon_document::SemanticInputEvent::Decrement { semantic_id })
        }
        NativeAccessibilityAction::Blur
        | NativeAccessibilityAction::ScrollIntoView
        | NativeAccessibilityAction::Other(_) => None,
    }
}

fn native_accessibility_action_from_accesskit(
    action: accesskit::Action,
) -> NativeAccessibilityAction {
    match action {
        accesskit::Action::Focus => NativeAccessibilityAction::Focus,
        accesskit::Action::Blur => NativeAccessibilityAction::Blur,
        accesskit::Action::Click => NativeAccessibilityAction::Click,
        accesskit::Action::SetValue => NativeAccessibilityAction::SetValue,
        accesskit::Action::ReplaceSelectedText => NativeAccessibilityAction::ReplaceSelectedText,
        accesskit::Action::Increment => NativeAccessibilityAction::Increment,
        accesskit::Action::Decrement => NativeAccessibilityAction::Decrement,
        accesskit::Action::ScrollIntoView => NativeAccessibilityAction::ScrollIntoView,
        other => NativeAccessibilityAction::Other(format!("{other:?}")),
    }
}

fn native_accessibility_action_value_from_accesskit(
    data: Option<accesskit::ActionData>,
) -> Option<String> {
    match data? {
        accesskit::ActionData::Value(value) => Some(value.into_string()),
        accesskit::ActionData::NumericValue(value) => Some(value.to_string()),
        accesskit::ActionData::CustomAction(value) => Some(value.to_string()),
        accesskit::ActionData::ScrollUnit(value) => Some(format!("{value:?}")),
        accesskit::ActionData::ScrollHint(value) => Some(format!("{value:?}")),
        accesskit::ActionData::ScrollToPoint(value) => Some(format!("{value:?}")),
        accesskit::ActionData::SetScrollOffset(value) => Some(format!("{value:?}")),
        accesskit::ActionData::SetTextSelection(value) => Some(format!("{value:?}")),
    }
}

fn accesskit_node_from_semantic_node(
    semantic: &boon_document::SemanticNode,
    id_map: &BTreeMap<boon_document::SemanticId, accesskit::NodeId>,
) -> accesskit::Node {
    let mut node = accesskit::Node::new(accesskit_role_for_semantic_role(&semantic.role));
    if let Some(name) = &semantic.name {
        node.set_label(name.clone());
    }
    if let Some(description) = &semantic.description {
        node.set_description(description.clone());
    }
    if let Some(value) = accesskit_value_for_semantic_value(semantic.value.as_ref()) {
        node.set_value(value);
    }
    if let Some(bounds) = semantic.bounds {
        node.set_bounds(accesskit::Rect::new(
            bounds.x as f64,
            bounds.y as f64,
            (bounds.x + bounds.width) as f64,
            (bounds.y + bounds.height) as f64,
        ));
    }
    if semantic.state.disabled {
        node.set_disabled();
    }
    if semantic.state.selected {
        node.set_selected(true);
    }
    if let Some(checked) = semantic.state.checked {
        node.set_toggled(accesskit::Toggled::from(checked));
    }
    if let Some(language) = &semantic.language {
        node.set_language(language.clone());
    }
    if let Some(level) = semantic.heading_level {
        node.set_level(level as usize);
    }
    if let Some(href) = &semantic.href {
        node.set_url(href.clone());
    }
    if semantic.actions.focus {
        node.add_action(accesskit::Action::Focus);
    }
    if semantic.actions.press {
        node.add_action(accesskit::Action::Click);
    }
    if semantic.actions.set_text {
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::ReplaceSelectedText);
    }
    if semantic.actions.increment {
        node.add_action(accesskit::Action::Increment);
    }
    if semantic.actions.decrement {
        node.add_action(accesskit::Action::Decrement);
    }
    let labelled_by = semantic
        .relations
        .labelled_by
        .iter()
        .filter_map(|id| id_map.get(id).copied())
        .collect::<Vec<_>>();
    if !labelled_by.is_empty() {
        node.set_labelled_by(labelled_by);
    }
    let described_by = semantic
        .relations
        .described_by
        .iter()
        .filter_map(|id| id_map.get(id).copied())
        .collect::<Vec<_>>();
    if !described_by.is_empty() {
        node.set_described_by(described_by);
    }
    let controls = semantic
        .relations
        .controls
        .iter()
        .filter_map(|id| id_map.get(id).copied())
        .collect::<Vec<_>>();
    if !controls.is_empty() {
        node.set_controls(controls);
    }
    node
}

fn accesskit_role_for_semantic_role(role: &boon_document::SemanticRole) -> accesskit::Role {
    match role {
        boon_document::SemanticRole::Application => accesskit::Role::Application,
        boon_document::SemanticRole::Group => accesskit::Role::Group,
        boon_document::SemanticRole::Row => accesskit::Role::Row,
        boon_document::SemanticRole::Text => accesskit::Role::TextRun,
        boon_document::SemanticRole::Button => accesskit::Role::Button,
        boon_document::SemanticRole::Checkbox => accesskit::Role::CheckBox,
        boon_document::SemanticRole::TextInput => accesskit::Role::TextInput,
        boon_document::SemanticRole::Table => accesskit::Role::Table,
        boon_document::SemanticRole::Cell => accesskit::Role::Cell,
        boon_document::SemanticRole::ScrollRegion => accesskit::Role::ScrollView,
    }
}

fn accesskit_value_for_semantic_value(
    value: Option<&boon_document::SemanticValue>,
) -> Option<String> {
    match value? {
        boon_document::SemanticValue::Text { text } => Some(text.clone()),
        boon_document::SemanticValue::Bool { value } => Some(value.to_string()),
        boon_document::SemanticValue::Number { value } => Some(value.to_string()),
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
    pub idle_poll_size_scale_total_us: u64,
    pub idle_poll_input_sample_total_us: u64,
    pub idle_poll_accessibility_total_us: u64,
    pub idle_poll_hook_total_us: u64,
    pub idle_poll_bookkeeping_total_us: u64,
    pub last_idle_poll_size_scale_us: u64,
    pub last_idle_poll_input_sample_us: u64,
    pub last_idle_poll_accessibility_us: u64,
    pub last_idle_poll_hook_us: u64,
    pub last_idle_poll_bookkeeping_us: u64,
    pub idle_wait_count: u64,
    pub idle_wait_total_ms: u64,
    pub last_idle_wait_timeout_ms: u64,
    pub last_idle_wait_actual_ms: u64,
    pub last_idle_wait_wake_reason: Option<String>,
    pub loop_exit_reason: Option<String>,
    pub forced_frame_count: u64,
    pub scheduled_wake_count: u64,
    pub last_scheduler_reason: Option<NativeSchedulerReason>,
    pub last_role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub current_scheduler_reason: Option<NativeSchedulerReason>,
    pub current_role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub last_poll_started_elapsed_ms: Option<f64>,
    pub last_dirty_poll_elapsed_ms: Option<f64>,
    pub last_external_wake_generation: u64,
    pub last_external_wake_observed_elapsed_ms: Option<f64>,
    pub last_render_started_elapsed_ms: Option<f64>,
    pub last_surface_acquired_elapsed_ms: Option<f64>,
    pub last_render_hook_completed_elapsed_ms: Option<f64>,
    pub last_queue_submitted_elapsed_ms: Option<f64>,
    pub last_present_completed_elapsed_ms: Option<f64>,
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
            idle_poll_size_scale_total_us: 0,
            idle_poll_input_sample_total_us: 0,
            idle_poll_accessibility_total_us: 0,
            idle_poll_hook_total_us: 0,
            idle_poll_bookkeeping_total_us: 0,
            last_idle_poll_size_scale_us: 0,
            last_idle_poll_input_sample_us: 0,
            last_idle_poll_accessibility_us: 0,
            last_idle_poll_hook_us: 0,
            last_idle_poll_bookkeeping_us: 0,
            idle_wait_count: 0,
            idle_wait_total_ms: 0,
            last_idle_wait_timeout_ms: 0,
            last_idle_wait_actual_ms: 0,
            last_idle_wait_wake_reason: None,
            loop_exit_reason: None,
            forced_frame_count: 0,
            scheduled_wake_count: 0,
            last_scheduler_reason: Some(NativeSchedulerReason::FirstFrame),
            last_role_dirty_reason: None,
            current_scheduler_reason: Some(NativeSchedulerReason::FirstFrame),
            current_role_dirty_reason: None,
            last_poll_started_elapsed_ms: None,
            last_dirty_poll_elapsed_ms: None,
            last_external_wake_generation: 0,
            last_external_wake_observed_elapsed_ms: None,
            last_render_started_elapsed_ms: None,
            last_surface_acquired_elapsed_ms: None,
            last_render_hook_completed_elapsed_ms: None,
            last_queue_submitted_elapsed_ms: None,
            last_present_completed_elapsed_ms: None,
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

    pub fn note_idle_poll_substeps(
        &mut self,
        size_scale: Duration,
        input_sample: Duration,
        accessibility: Duration,
        hook: Duration,
        bookkeeping: Duration,
    ) {
        let size_scale_us = duration_micros_u64(size_scale);
        let input_sample_us = duration_micros_u64(input_sample);
        let accessibility_us = duration_micros_u64(accessibility);
        let hook_us = duration_micros_u64(hook);
        let bookkeeping_us = duration_micros_u64(bookkeeping);
        self.idle_poll_size_scale_total_us = self
            .idle_poll_size_scale_total_us
            .saturating_add(size_scale_us);
        self.idle_poll_input_sample_total_us = self
            .idle_poll_input_sample_total_us
            .saturating_add(input_sample_us);
        self.idle_poll_accessibility_total_us = self
            .idle_poll_accessibility_total_us
            .saturating_add(accessibility_us);
        self.idle_poll_hook_total_us = self.idle_poll_hook_total_us.saturating_add(hook_us);
        self.idle_poll_bookkeeping_total_us = self
            .idle_poll_bookkeeping_total_us
            .saturating_add(bookkeeping_us);
        self.last_idle_poll_size_scale_us = size_scale_us;
        self.last_idle_poll_input_sample_us = input_sample_us;
        self.last_idle_poll_accessibility_us = accessibility_us;
        self.last_idle_poll_hook_us = hook_us;
        self.last_idle_poll_bookkeeping_us = bookkeeping_us;
    }

    pub fn note_poll_started(&mut self, elapsed_ms: f64) {
        self.last_poll_started_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_dirty_poll(&mut self, elapsed_ms: f64) {
        self.last_dirty_poll_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_external_wake_observed(&mut self, generation: u64, elapsed_ms: f64) {
        self.last_external_wake_generation = generation;
        self.last_external_wake_observed_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_render_started(&mut self, elapsed_ms: f64) {
        self.last_render_started_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_surface_acquired(&mut self, elapsed_ms: f64) {
        self.last_surface_acquired_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_render_hook_completed(&mut self, elapsed_ms: f64) {
        self.last_render_hook_completed_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_queue_submitted(&mut self, elapsed_ms: f64) {
        self.last_queue_submitted_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_present_completed(&mut self, elapsed_ms: f64) {
        self.last_present_completed_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_idle_wait(
        &mut self,
        timeout: Duration,
        actual: Duration,
        observed_generation: u64,
        completed_generation: u64,
    ) {
        self.idle_wait_count = self.idle_wait_count.saturating_add(1);
        let actual_ms = actual.as_millis() as u64;
        self.idle_wait_total_ms = self.idle_wait_total_ms.saturating_add(actual_ms);
        self.last_idle_wait_timeout_ms = timeout.as_millis() as u64;
        self.last_idle_wait_actual_ms = actual_ms;
        self.last_idle_wait_wake_reason = Some(if timeout.is_zero() {
            "zero_timeout".to_owned()
        } else if completed_generation != observed_generation {
            "external_wake".to_owned()
        } else {
            "timeout".to_owned()
        });
    }

    pub fn note_loop_exit(&mut self, reason: impl Into<String>) {
        self.loop_exit_reason = Some(reason.into());
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
        if poll_result.wants_animation_frame && !poll_result.dirty {
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
    #[serde(skip)]
    pub accessibility_update: Option<accesskit::TreeUpdate>,
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
            accessibility_update: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCursorIcon {
    Default,
    ColumnResize,
    RowResize,
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
                Some(
                    NativeSchedulerReason::HostInput
                        | NativeSchedulerReason::Timer
                        | NativeSchedulerReason::RequestedAnimation
                )
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
                    Some(
                        NativeSchedulerReason::HostInput
                            | NativeSchedulerReason::Timer
                            | NativeSchedulerReason::RequestedAnimation
                    )
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
    pub last_mouse_motion_event_count: u64,
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
        self.last_mouse_motion_event_count = self
            .last_mouse_motion_event_count
            .max(input.mouse_motion_event_count);
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
    pub readback_deadline_ms: u64,
    pub readback_poll_status: String,
}

struct PendingSurfaceReadback {
    buffer: wgpu::Buffer,
    role: NativeWindowRole,
    title: String,
    surface_id: SurfaceId,
    surface_epoch: u64,
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
    pub accessibility_actions: Vec<NativeAccessibilityActionRequest>,
    pub now: Instant,
    pub forced_frame: bool,
}

pub type NativeRenderHook = Box<
    dyn for<'a> FnMut(NativeRenderFrameContext<'a>) -> Result<NativeRenderHookResult, String>
        + Send,
>;

pub type NativePollHook =
    Box<dyn FnMut(NativePollContext) -> Result<NativePollResult, String> + Send>;
pub type NativeExitHook = Box<dyn FnMut() -> Option<String> + Send>;

pub struct NativeWindowHooks {
    pub poll: Option<NativePollHook>,
    pub should_exit: Option<NativeExitHook>,
    pub render: NativeRenderHook,
}

impl NativeWindowHooks {
    pub fn from_render_hook(render: NativeRenderHook) -> Self {
        Self {
            poll: None,
            should_exit: None,
            render,
        }
    }
}

fn native_window_exit_reason(hooks: &mut Option<NativeWindowHooks>) -> Option<String> {
    hooks
        .as_mut()
        .and_then(|hooks| hooks.should_exit.as_mut())
        .and_then(|should_exit| should_exit())
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

pub fn run_visible_surface_probe<F>(options: NativeWindowOptions, on_ready: F)
where
    F: FnOnce(Result<AppWindowSurfaceProof, NativeWindowError>) + Send + 'static,
{
    run_visible_surface_probe_with_render_hook(options, None, on_ready)
}

pub fn run_visible_surface_probe_with_render_hook<F>(
    options: NativeWindowOptions,
    render_hook: Option<NativeRenderHook>,
    on_ready: F,
) where
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
) where
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
) where
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
            .stack_size(NATIVE_WINDOW_RENDER_THREAD_STACK_BYTES)
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
        app_window::application::stop();
    });
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
        let accessibility_actions = native_accessibility_action_requests_from_accesskit(
            app_surface.take_accessibility_action_requests(),
        );
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
                accessibility_actions,
                now: Instant::now(),
                forced_frame: true,
            },
        )? {
            apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
            if let Some(update) = poll_result.accessibility_update.clone() {
                app_surface.update_accessibility_if_active(update);
            }
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
        let readback_sample_frame =
            frame_index + 1 == total_frame_count && options.readback_artifact_dir.is_some();
        if readback_sample_frame {
            pending_readback = Some(queue_visible_surface_readback(
                &device,
                &mut encoder,
                &frame.texture,
                options.role,
                width,
                height,
                config.format,
                &options.title,
                surface_id.clone(),
                surface_lifecycle.epoch(),
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
        let include_timing_sample =
            frame_index >= warmup_frame_count && !(readback_sample_frame && sample_frame_count > 1);
        if include_timing_sample {
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
        let post_input_warmup_frame_count = warmup_frame_count;
        let post_input_sample_count = sample_frame_count.max(1);
        let post_input_total_frame_count = post_input_warmup_frame_count
            .saturating_add(post_input_sample_count)
            .max(1);
        let mut post_input_presented_frame_samples = Vec::new();
        let mut post_input_render_hook_samples = Vec::new();
        let mut post_input_first_frame_ms = 0.0;
        let mut post_input_readback = None;
        for frame_index in 0..post_input_total_frame_count {
            let frame_input = if frame_index == 0 {
                input_adapter.clone()
            } else {
                sample_input_adapter(&mut mouse, &keyboard, false)
            };
            let accessibility_actions = native_accessibility_action_requests_from_accesskit(
                app_surface.take_accessibility_action_requests(),
            );
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
                    accessibility_actions,
                    now: Instant::now(),
                    forced_frame: true,
                },
            )? {
                apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
                if let Some(update) = poll_result.accessibility_update.clone() {
                    app_surface.update_accessibility_if_active(update);
                }
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
            let mut post_input_render_hook_ms = None;
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
                post_input_render_hook_ms = Some(elapsed_ms(render_start));
            }
            let readback_sample_frame = frame_index + 1 == post_input_total_frame_count
                && options.readback_artifact_dir.is_some();
            if readback_sample_frame {
                post_input_readback = Some(queue_visible_surface_readback(
                    &device,
                    &mut encoder,
                    &frame.texture,
                    options.role,
                    width,
                    height,
                    config.format,
                    &options.title,
                    surface_id.clone(),
                    surface_lifecycle.epoch(),
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
            let include_timing_sample = frame_index >= post_input_warmup_frame_count
                && !(readback_sample_frame && post_input_sample_count > 1);
            if include_timing_sample {
                post_input_presented_frame_samples.push(frame_ms);
                if let Some(render_hook_ms) = post_input_render_hook_ms {
                    post_input_render_hook_samples.push(render_hook_ms);
                }
            }
        }
        post_input_frame_timing = Some(NativeFrameTimingProof {
            warmup_frame_count: post_input_warmup_frame_count,
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
        external_render_proof: external_render_proof.clone(),
        readback_artifact,
    };
    let _ = ready_sender.send(Ok(proof));
    let hold_started = Instant::now();
    let mut input_cursor = NativeInputCursor::default();
    let mut last_wake_generation = 0;
    let mut last_interactive_readback_artifact: Option<AppWindowReadbackArtifact> = None;
    loop {
        if options.hold_ms > 0 && hold_started.elapsed() >= Duration::from_millis(options.hold_ms) {
            render_loop_state.note_loop_exit("hold_timeout_elapsed");
            break;
        }
        if let Some(reason) = native_window_exit_reason(&mut hooks) {
            render_loop_state.note_loop_exit(reason);
            break;
        }
        let size_scale_started = Instant::now();
        let (current_size, current_scale) = app_surface.size_scale().await;
        let size_scale_elapsed = size_scale_started.elapsed();
        let raw_width = (current_size.width() * current_scale).round();
        let raw_height = (current_size.height() * current_scale).round();
        if raw_width <= 0.0 || raw_height <= 0.0 {
            surface_lifecycle.note_zero_size_skip();
            render_loop_state.note_idle_poll();
            let idle_timeout = render_loop_state.idle_wait_timeout(Instant::now());
            let wait_started = Instant::now();
            let completed_generation =
                wake_handle.wait_for_wake_after(last_wake_generation, idle_timeout);
            render_loop_state.note_idle_wait(
                idle_timeout,
                wait_started.elapsed(),
                last_wake_generation,
                completed_generation,
            );
            if completed_generation != last_wake_generation {
                render_loop_state.note_external_wake_observed(
                    completed_generation,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            last_wake_generation = completed_generation;
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
        render_loop_state.note_poll_started(hold_started.elapsed().as_secs_f64() * 1000.0);
        render_loop_state.consume_due_wake(poll_started_at);
        let input_sample_started = Instant::now();
        let input = sample_input_adapter_delta(&mut mouse, &keyboard, &input_cursor, false);
        let input_sample_elapsed = input_sample_started.elapsed();
        let accessibility_started = Instant::now();
        let accessibility_actions = native_accessibility_action_requests_from_accesskit(
            app_surface.take_accessibility_action_requests(),
        );
        let accessibility_elapsed = accessibility_started.elapsed();
        merge_input_adapter_proof(&mut observed_input_adapter, &input);
        render_loop_state.note_input_poll();
        let hook_started = Instant::now();
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
                accessibility_actions,
                now: poll_started_at,
                forced_frame: false,
            },
        )?;
        let hook_elapsed = hook_started.elapsed();
        let bookkeeping_started = Instant::now();
        if let Some(poll_result) = poll_result {
            apply_native_cursor_icon(&app_surface, poll_result.cursor_icon);
            if let Some(update) = poll_result.accessibility_update.clone() {
                app_surface.update_accessibility_if_active(update);
            }
            if let Some(next_wake_after_ms) = poll_result.next_wake_after_ms {
                render_loop_state.schedule_wake_after(
                    poll_started_at,
                    Duration::from_millis(next_wake_after_ms),
                );
            }
            render_loop_state.apply_poll_result(&poll_result, input.real_os_events_observed);
            if poll_result.dirty {
                render_loop_state.note_dirty_poll(hold_started.elapsed().as_secs_f64() * 1000.0);
            }
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        } else if input.real_os_events_observed {
            render_loop_state.mark_dirty(NativeSchedulerReason::HostInput, None);
            render_loop_state.note_dirty_poll(hold_started.elapsed().as_secs_f64() * 1000.0);
        }
        let wake_generation = wake_handle.generation();
        let wake_generation_changed = wake_generation != last_wake_generation;
        if wake_generation_changed {
            last_wake_generation = wake_generation;
            render_loop_state.note_external_wake_observed(
                wake_generation,
                hold_started.elapsed().as_secs_f64() * 1000.0,
            );
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::ExternalWake);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            continue;
        }
        if !render_loop_state.should_render(Instant::now(), false) {
            let bookkeeping_elapsed = bookkeeping_started.elapsed();
            render_loop_state.note_idle_poll_substeps(
                size_scale_elapsed,
                input_sample_elapsed,
                accessibility_elapsed,
                hook_elapsed,
                bookkeeping_elapsed,
            );
            render_loop_state.note_idle_poll();
            let idle_timeout = render_loop_state.idle_wait_timeout(Instant::now());
            let wait_started = Instant::now();
            let completed_generation =
                wake_handle.wait_for_wake_after(last_wake_generation, idle_timeout);
            render_loop_state.note_idle_wait(
                idle_timeout,
                wait_started.elapsed(),
                last_wake_generation,
                completed_generation,
            );
            if completed_generation != last_wake_generation {
                render_loop_state.note_external_wake_observed(
                    completed_generation,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            last_wake_generation = completed_generation;
            continue;
        }
        if hooks.as_ref().is_none_or(|hooks| hooks.poll.is_none()) {
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        }
        let rendered_revision = render_loop_state.dirty_revision;
        render_loop_state.note_render_started(hold_started.elapsed().as_secs_f64() * 1000.0);
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
        render_loop_state.note_surface_acquired(hold_started.elapsed().as_secs_f64() * 1000.0);
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
                external_render_proof = Some(render_result.proof);
                render_loop_state
                    .note_render_hook_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
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
                render_loop_state
                    .note_render_hook_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
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
                        surface_id.clone(),
                        surface_lifecycle.epoch(),
                    )?,
                ))
            } else {
                None
            }
        } else {
            None
        };
        queue.submit(Some(encoder.finish()));
        render_loop_state.note_queue_submitted(hold_started.elapsed().as_secs_f64() * 1000.0);
        frame.present();
        render_loop_state.mark_presented_with_content(rendered_revision, rendered_content_revision);
        render_loop_state.note_present_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
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
                )
                .with_external_render_proof(external_render_proof.as_ref()),
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
                    )
                    .with_external_render_proof(external_render_proof.as_ref()),
                    None,
                )?;
            }
        }
        if loop_mode == NativeRenderLoopMode::ContinuousProbe {
            std::thread::sleep(Duration::from_millis(16));
        }
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
            )
            .with_external_render_proof(external_render_proof.as_ref()),
            None,
        )?;
    }
    let callback_done_timeout =
        Duration::from_millis(options.hold_ms.max(2_000)).saturating_add(Duration::from_secs(240));
    let _ = callback_done_receiver.recv_timeout(callback_done_timeout);
    drop(surface);
    drop(app_surface);
    drop(window);
    Ok(())
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
    surface_id: SurfaceId,
    surface_epoch: u64,
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
        surface_id,
        surface_epoch,
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
    external_render_proof: Option<serde_json::Value>,
}

impl NativeRenderLoopReportExtras {
    fn with_external_render_proof(mut self, proof: Option<&serde_json::Value>) -> Self {
        self.external_render_proof = proof.cloned();
        self
    }
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
        external_render_proof: None,
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
            NativeCursorIcon::RowResize => app_window::surface::SurfaceCursorIcon::RowResize,
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
    let idle_poll_count = state.skipped_idle_poll_count.max(1) as f64;
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
        "idle_poll_substep_total_us": {
            "size_scale": state.idle_poll_size_scale_total_us,
            "input_sample": state.idle_poll_input_sample_total_us,
            "accessibility": state.idle_poll_accessibility_total_us,
            "hook_poll": state.idle_poll_hook_total_us,
            "bookkeeping": state.idle_poll_bookkeeping_total_us
        },
        "idle_poll_substep_avg_us": {
            "size_scale": state.idle_poll_size_scale_total_us as f64 / idle_poll_count,
            "input_sample": state.idle_poll_input_sample_total_us as f64 / idle_poll_count,
            "accessibility": state.idle_poll_accessibility_total_us as f64 / idle_poll_count,
            "hook_poll": state.idle_poll_hook_total_us as f64 / idle_poll_count,
            "bookkeeping": state.idle_poll_bookkeeping_total_us as f64 / idle_poll_count
        },
        "last_idle_poll_substep_us": {
            "size_scale": state.last_idle_poll_size_scale_us,
            "input_sample": state.last_idle_poll_input_sample_us,
            "accessibility": state.last_idle_poll_accessibility_us,
            "hook_poll": state.last_idle_poll_hook_us,
            "bookkeeping": state.last_idle_poll_bookkeeping_us
        },
        "idle_wait_count": state.idle_wait_count,
        "idle_wait_total_ms": state.idle_wait_total_ms,
        "last_idle_wait_timeout_ms": state.last_idle_wait_timeout_ms,
        "last_idle_wait_actual_ms": state.last_idle_wait_actual_ms,
        "last_idle_wait_wake_reason": state.last_idle_wait_wake_reason,
        "last_poll_started_elapsed_ms": state.last_poll_started_elapsed_ms,
        "last_dirty_poll_elapsed_ms": state.last_dirty_poll_elapsed_ms,
        "last_external_wake_generation": state.last_external_wake_generation,
        "last_external_wake_observed_elapsed_ms": state.last_external_wake_observed_elapsed_ms,
        "last_render_started_elapsed_ms": state.last_render_started_elapsed_ms,
        "last_surface_acquired_elapsed_ms": state.last_surface_acquired_elapsed_ms,
        "last_render_hook_completed_elapsed_ms": state.last_render_hook_completed_elapsed_ms,
        "last_queue_submitted_elapsed_ms": state.last_queue_submitted_elapsed_ms,
        "last_present_completed_elapsed_ms": state.last_present_completed_elapsed_ms,
        "loop_exit_reason": state.loop_exit_reason,
        "forced_frame_count": state.forced_frame_count,
        "renders_per_second": renders_per_second,
        "scheduled_wake_count": state.scheduled_wake_count,
        "active_timer_reason": active_timer_reason,
        "passive_input_poll_interval_ms": PASSIVE_INPUT_POLL_INTERVAL.as_millis() as u64,
        "resize_wake_count": extras.resize_wake_count,
        "app_window_surface_content_report": extras.app_window_surface_content_report,
        "observed_input_adapter": extras.observed_input_adapter,
        "last_external_render_proof": extras.external_render_proof,
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
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(VISIBLE_SURFACE_READBACK_TIMEOUT),
        })
        .map_err(|error| {
            NativeWindowError::Failed(visible_readback_failure_message(
                "poll",
                &pending,
                &error.to_string(),
            ))
        })?;
    receiver
        .recv_timeout(VISIBLE_SURFACE_READBACK_TIMEOUT)
        .map_err(|error| {
            NativeWindowError::Failed(visible_readback_failure_message(
                "callback",
                &pending,
                &error.to_string(),
            ))
        })?
        .map_err(|error| {
            NativeWindowError::Failed(visible_readback_failure_message(
                "map",
                &pending,
                &error.to_string(),
            ))
        })?;

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
        readback_deadline_ms: VISIBLE_SURFACE_READBACK_TIMEOUT.as_millis() as u64,
        readback_poll_status: "completed_before_deadline".to_owned(),
    })
}

fn visible_readback_failure_message(
    phase: &str,
    pending: &PendingSurfaceReadback,
    reason: &str,
) -> String {
    format!(
        "visible surface readback {phase} failed before deadline: backend=wgpu adapter=unavailable frame_id={} surface={} epoch={} requested_rect=0,0,{},{} submission=latest; report_context=app_window_visible_surface_readback role={} deadline_ms={} reason={reason}",
        stable_debug_hash(&pending.title),
        pending.surface_id.0,
        pending.surface_epoch,
        pending.width,
        pending.height,
        pending.role.as_str(),
        VISIBLE_SURFACE_READBACK_TIMEOUT.as_millis(),
    )
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn duration_micros_u64(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
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
    let new_motion_observed =
        mouse_provenance.motion_event_count > cursor.last_mouse_motion_event_count;
    let real_os_events_observed = !mouse_button_events.is_empty()
        || !keyboard_events.is_empty()
        || new_motion_observed
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
    fn demand_driven_idle_wait_reports_timeout_and_wake_reason() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

        state.note_idle_wait(Duration::from_millis(30), Duration::from_millis(12), 4, 4);
        assert_eq!(state.idle_wait_count, 1);
        assert_eq!(state.idle_wait_total_ms, 12);
        assert_eq!(state.last_idle_wait_timeout_ms, 30);
        assert_eq!(state.last_idle_wait_actual_ms, 12);
        assert_eq!(state.last_idle_wait_wake_reason.as_deref(), Some("timeout"));

        state.note_idle_wait(Duration::from_millis(100), Duration::from_millis(3), 4, 5);
        assert_eq!(state.idle_wait_count, 2);
        assert_eq!(state.idle_wait_total_ms, 15);
        assert_eq!(
            state.last_idle_wait_wake_reason.as_deref(),
            Some("external_wake")
        );
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
                accessibility_update: None,
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
                accessibility_update: None,
            },
            false,
        );

        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::SourcePayloadAccepted)
        );
    }

    #[test]
    fn accessibility_action_requests_lower_without_leaking_accesskit_types() {
        let requests = native_accessibility_action_requests_from_accesskit(vec![
            accesskit::ActionRequest {
                action: accesskit::Action::Focus,
                target_tree: accesskit::TreeId::ROOT,
                target_node: accesskit::NodeId(41),
                data: None,
            },
            accesskit::ActionRequest {
                action: accesskit::Action::SetValue,
                target_tree: accesskit::TreeId::ROOT,
                target_node: accesskit::NodeId(42),
                data: Some(accesskit::ActionData::Value("hello".into())),
            },
            accesskit::ActionRequest {
                action: accesskit::Action::ScrollLeft,
                target_tree: accesskit::TreeId::ROOT,
                target_node: accesskit::NodeId(43),
                data: None,
            },
        ]);

        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].target_node_id, 41);
        assert_eq!(requests[0].action, NativeAccessibilityAction::Focus);
        assert_eq!(requests[1].action, NativeAccessibilityAction::SetValue);
        assert_eq!(requests[1].value.as_deref(), Some("hello"));
        assert_eq!(
            requests[2].action,
            NativeAccessibilityAction::Other("ScrollLeft".to_owned())
        );
    }

    #[test]
    fn accessibility_action_requests_route_to_semantic_source_dispatch() {
        let root_id = boon_document::SemanticId("semantic:world-editor:root".to_owned());
        let export_id =
            boon_document::SemanticId("semantic:world-editor:manufacturing:export-3mf".to_owned());
        let mut scene = boon_document::SemanticScene {
            root: Some(root_id.clone()),
            focused: Some(export_id.clone()),
            ..boon_document::SemanticScene::default()
        };
        scene.nodes.insert(
            root_id.clone(),
            boon_document::SemanticNode {
                id: root_id.clone(),
                node: boon_document::DocumentNodeId("world:world-editor:root".to_owned()),
                role: boon_document::SemanticRole::Application,
                name: Some("Car editor".to_owned()),
                description: None,
                value: None,
                state: boon_document::SemanticState::default(),
                actions: boon_document::SemanticActions::default(),
                relations: boon_document::SemanticRelations {
                    children: vec![export_id.clone()],
                    ..boon_document::SemanticRelations::default()
                },
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );
        scene.nodes.insert(
            export_id.clone(),
            boon_document::SemanticNode {
                id: export_id.clone(),
                node: boon_document::DocumentNodeId(
                    "world:world-editor:manufacturing:export-3mf".to_owned(),
                ),
                role: boon_document::SemanticRole::Button,
                name: Some("Export 3MF".to_owned()),
                description: None,
                value: None,
                state: boon_document::SemanticState {
                    focused: true,
                    ..boon_document::SemanticState::default()
                },
                actions: boon_document::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_document::SemanticRelations {
                    parent: Some(root_id),
                    ..boon_document::SemanticRelations::default()
                },
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(boon_document::SourceBindingId(
                    "source:world.manufacturing.export_3mf".to_owned(),
                )),
                source_path: Some("world.manufacturing.export_3mf".to_owned()),
                source_intent: Some("press".to_owned()),
            },
        );
        let snapshot =
            accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");
        let export_node_id = snapshot
            .semantic_node_ids
            .iter()
            .find(|mapping| mapping.semantic_id == export_id.0)
            .expect("export semantic node should map to AccessKit")
            .accesskit_node_id;
        let requests =
            native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: accesskit::NodeId(export_node_id),
                data: None,
            }]);

        let dispatches = native_accessibility_source_dispatches_from_requests(&scene, &requests);

        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].semantic_id, export_id);
        assert_eq!(dispatches[0].source_path, "world.manufacturing.export_3mf");
        assert_eq!(dispatches[0].source_intent.as_deref(), Some("press"));
        assert_eq!(dispatches[0].text, None);
    }

    #[test]
    fn accessibility_action_requests_drive_world_editor_session_actions() {
        let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
        let visual =
            boon_scene_model::WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
                .expect("car fixture should compile to visual proxy scene");
        let mut session = boon_scene_model::WorldEditorSession::new(visual.scene);
        let scene_for_session = |session: &boon_scene_model::WorldEditorSession| {
            let tree = session
                .semantic_editor_tree(&bundle, "Car editor")
                .expect("world editor semantic tree");
            boon_document::SemanticScene::from_world_editor_tree(&tree)
        };
        let node_id_for_name =
            |scene: &boon_document::SemanticScene, name: &str| -> accesskit::NodeId {
                let semantic_id = scene
                    .nodes
                    .values()
                    .find(|node| node.name.as_deref() == Some(name))
                    .expect("semantic node by name")
                    .id
                    .clone();
                let node_id =
                    accesskit_tree_update_from_semantic_scene(scene, "boon-native", "test-version")
                        .semantic_node_ids
                        .iter()
                        .find(|mapping| mapping.semantic_id == semantic_id.0)
                        .expect("semantic node should map to AccessKit")
                        .accesskit_node_id;
                accesskit::NodeId(node_id)
            };

        let scene = scene_for_session(&session);
        let wheel_node_id = node_id_for_name(&scene, "Front-left wheel");
        let select_requests =
            native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: wheel_node_id,
                data: None,
            }]);
        let select_reports = native_accessibility_world_editor_session_reports_from_requests(
            &scene,
            &select_requests,
            &mut session,
            &bundle,
        );

        assert_eq!(select_reports.len(), 1);
        assert_eq!(select_reports[0].error, None);
        let select_report = select_reports[0]
            .session_report
            .as_ref()
            .expect("selection session report");
        assert!(matches!(
            select_report.outcome.action,
            boon_scene_model::WorldEditorActionKind::SelectInstance { .. }
        ));
        assert_eq!(
            select_report
                .patch_report
                .as_ref()
                .map(|report| report.selection_update_count),
            Some(1)
        );
        assert_eq!(select_report.selected_instance_count, 1);

        let selected_scene = scene_for_session(&session);
        assert_eq!(
            selected_scene
                .nodes
                .values()
                .filter(|node| node.state.selected)
                .count(),
            1
        );
        let export_node_id = node_id_for_name(&selected_scene, "Export 3MF");
        let export_requests =
            native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: export_node_id,
                data: None,
            }]);
        let export_reports = native_accessibility_world_editor_session_reports_from_requests(
            &selected_scene,
            &export_requests,
            &mut session,
            &bundle,
        );

        assert_eq!(export_reports.len(), 1);
        assert_eq!(export_reports[0].error, None);
        let export_report = export_reports[0]
            .session_report
            .as_ref()
            .expect("export session report");
        let preparation = export_report
            .outcome
            .export_preparation
            .as_ref()
            .expect("export preparation");
        assert_eq!(
            export_report.outcome.action,
            boon_scene_model::WorldEditorActionKind::Export3Mf
        );
        assert_eq!(
            preparation.status,
            boon_scene_model::WorldManufacturingExportStatus::ReadySelectedPrintable
        );
        assert!(preparation.selected_part_exportable);
        assert_eq!(preparation.excluded_visual_only_instance_count, 1);
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
            accessibility_update: None,
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
                accessibility_update: None,
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
            accessibility_update: None,
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
    fn animation_request_on_dirty_role_revision_does_not_invent_unrenderable_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let poll = NativePollResult {
            dirty: true,
            role_revision: 2,
            scheduler_reason: Some(NativeSchedulerReason::Timer),
            role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
            next_wake_after_ms: Some(16),
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: true,
            accessibility_update: None,
        };

        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 2);
        assert!(
            (NativeRenderHookResult {
                proof: serde_json::json!({}),
                content_revision: 2,
                rendered: true,
                content_changed: true,
                role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
            })
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                state.current_scheduler_reason,
                state.current_role_dirty_reason,
            )
            .is_ok(),
            "animation scheduling must not demand a content revision the role never produced"
        );
    }

    #[test]
    fn requested_animation_can_repaint_existing_scheduler_only_content() {
        let render = NativeRenderHookResult {
            proof: serde_json::json!({}),
            content_revision: 2,
            rendered: true,
            content_changed: true,
            role_dirty_reason: None,
        };

        assert!(
            render
                .validate_for_presented_revision_with_scheduler(
                    3,
                    Some(NativeSchedulerReason::RequestedAnimation),
                    None,
                )
                .is_ok(),
            "requested animation frames are scheduler-owned repaints"
        );
        assert_eq!(
            render.presented_content_revision(
                3,
                Some(NativeSchedulerReason::RequestedAnimation),
                None
            ),
            3
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
                accessibility_update: None,
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
                accessibility_update: None,
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

    #[test]
    fn semantic_scene_lowers_to_accesskit_tree_update_with_stable_ids() {
        let root_id = boon_document::SemanticId("semantic:root".to_owned());
        let button_id = boon_document::SemanticId("semantic:save".to_owned());
        let checkbox_id = boon_document::SemanticId("semantic:done".to_owned());
        let input_id = boon_document::SemanticId("semantic:filter".to_owned());
        let mut scene = boon_document::SemanticScene {
            root: Some(root_id.clone()),
            focused: Some(input_id.clone()),
            ..boon_document::SemanticScene::default()
        };
        scene.nodes.insert(
            root_id.clone(),
            boon_document::SemanticNode {
                id: root_id.clone(),
                node: boon_document::DocumentNodeId("root".to_owned()),
                role: boon_document::SemanticRole::Application,
                name: Some("Boon app".to_owned()),
                description: None,
                value: None,
                state: boon_document::SemanticState::default(),
                actions: boon_document::SemanticActions::default(),
                relations: boon_document::SemanticRelations {
                    children: vec![button_id.clone(), checkbox_id.clone(), input_id.clone()],
                    ..boon_document::SemanticRelations::default()
                },
                bounds: Some(boon_document::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 320.0,
                    height: 180.0,
                }),
                language: Some("en".to_owned()),
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );
        scene.nodes.insert(
            button_id.clone(),
            boon_document::SemanticNode {
                id: button_id.clone(),
                node: boon_document::DocumentNodeId("save".to_owned()),
                role: boon_document::SemanticRole::Button,
                name: Some("Save".to_owned()),
                description: None,
                value: None,
                state: boon_document::SemanticState::default(),
                actions: boon_document::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_document::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_document::SemanticRelations::default()
                },
                bounds: Some(boon_document::Rect {
                    x: 8.0,
                    y: 8.0,
                    width: 80.0,
                    height: 28.0,
                }),
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(boon_document::SourceBindingId("source:save".to_owned())),
                source_path: Some("toolbar.save".to_owned()),
                source_intent: Some("press".to_owned()),
            },
        );
        scene.nodes.insert(
            checkbox_id.clone(),
            boon_document::SemanticNode {
                id: checkbox_id.clone(),
                node: boon_document::DocumentNodeId("done".to_owned()),
                role: boon_document::SemanticRole::Checkbox,
                name: Some("Done".to_owned()),
                description: None,
                value: Some(boon_document::SemanticValue::Bool { value: true }),
                state: boon_document::SemanticState {
                    checked: Some(true),
                    ..boon_document::SemanticState::default()
                },
                actions: boon_document::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_document::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_document::SemanticRelations::default()
                },
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );
        scene.nodes.insert(
            input_id.clone(),
            boon_document::SemanticNode {
                id: input_id.clone(),
                node: boon_document::DocumentNodeId("filter".to_owned()),
                role: boon_document::SemanticRole::TextInput,
                name: Some("Filter".to_owned()),
                description: None,
                value: Some(boon_document::SemanticValue::Text {
                    text: "abc".to_owned(),
                }),
                state: boon_document::SemanticState {
                    focused: true,
                    ..boon_document::SemanticState::default()
                },
                actions: boon_document::SemanticActions {
                    focus: true,
                    press: false,
                    set_text: true,
                    increment: false,
                    decrement: false,
                },
                relations: boon_document::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_document::SemanticRelations::default()
                },
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: None,
                source_path: None,
                source_intent: None,
            },
        );

        let snapshot =
            accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");
        let repeat =
            accesskit_tree_update_from_semantic_scene(&scene, "boon-native", "test-version");

        assert_eq!(snapshot.metrics.semantic_node_count, 4);
        assert_eq!(snapshot.metrics.accesskit_node_count, 4);
        assert_eq!(snapshot.metrics.interactive_node_count, 3);
        assert_eq!(snapshot.metrics.focusable_node_count, 3);
        assert_eq!(snapshot.metrics.text_input_node_count, 1);
        assert_eq!(snapshot.metrics.checked_node_count, 1);
        assert_eq!(snapshot.metrics.node_id_collision_count, 0);
        assert!(snapshot.metrics.root_present);
        assert!(snapshot.metrics.focus_present);
        assert_eq!(snapshot.semantic_node_ids, repeat.semantic_node_ids);
        assert_eq!(
            snapshot.tree_update.tree.as_ref().unwrap().root,
            snapshot
                .semantic_node_ids
                .iter()
                .find(|mapping| mapping.semantic_id == "semantic:root")
                .map(|mapping| accesskit::NodeId(mapping.accesskit_node_id))
                .unwrap()
        );
        assert_eq!(
            snapshot.tree_update.focus,
            snapshot
                .semantic_node_ids
                .iter()
                .find(|mapping| mapping.semantic_id == "semantic:filter")
                .map(|mapping| accesskit::NodeId(mapping.accesskit_node_id))
                .unwrap()
        );

        let root = snapshot
            .tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::Application)
            .expect("root application node should exist");
        assert_eq!(root.1.children().len(), 3);

        let button = snapshot
            .tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::Button)
            .expect("button node should exist");
        assert!(button.1.supports_action(accesskit::Action::Click));
        assert!(button.1.supports_action(accesskit::Action::Focus));

        let text_input = snapshot
            .tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::TextInput)
            .expect("text input node should exist");
        assert!(text_input.1.supports_action(accesskit::Action::SetValue));
        assert!(
            text_input
                .1
                .supports_action(accesskit::Action::ReplaceSelectedText)
        );

        let checkbox = snapshot
            .tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::CheckBox)
            .expect("checkbox node should exist");
        assert!(checkbox.1.supports_action(accesskit::Action::Click));
        assert_eq!(checkbox.1.toggled(), Some(accesskit::Toggled::True));
    }
}
