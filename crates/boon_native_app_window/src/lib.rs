#![recursion_limit = "512"]

use app_window::coordinates::{Position, Size};
use app_window::input::keyboard::{Keyboard, key::KeyboardKey};
use app_window::input::mouse::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT, Mouse};
use app_window::window::Window;
use app_window::{WGPU_SURFACE_STRATEGY, WGPUStrategy};
use boon_host::{PhysicalSize, SurfaceId, Viewport, WindowId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wgpu::SurfaceTargetUnsafe;

// The native loop currently owns both DemandDriven frame pacing and app-window
// input discovery. Until `PreviewHotLoop` splits those responsibilities, this
// timeout is the worst-case interval before mouse/keyboard state becomes
// sampleable on paths where the app-window callback cannot interrupt our wait.
// Keep it below a frame budget instead of treating it as an idle-only energy
// knob; otherwise user-visible clicks can sit behind a 100ms sleep before the
// product path even starts.
const PASSIVE_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(2);
const VISIBLE_SURFACE_READBACK_TIMEOUT: Duration = Duration::from_secs(5);
const NATIVE_WINDOW_RENDER_THREAD_STACK_BYTES: usize = 32 * 1024 * 1024;
const INPUT_EVENT_WAKE_TIMELINE_LIMIT: usize = 512;
const MAX_CONSECUTIVE_UNSAMPLED_INPUT_RESAMPLES: u8 = 3;
const PACED_SURFACE_FRAME_LATENCY: u32 = 2;
const MAX_CONFIGURED_SURFACE_FRAME_LATENCY: u32 = 8;
const NATIVE_TARGET_FRAME_INTERVAL_MS: f64 = 1000.0 / 60.0;
pub const REQUESTED_ANIMATION_BURST_MIN_FRAMES: u32 = 2;
pub const REQUESTED_ANIMATION_QUIET_MS: u64 = 100;
pub const REQUESTED_ANIMATION_HARD_CAP_MS: u64 = 1_000;
pub const REQUESTED_ANIMATION_MAX_PENDING_SNAPSHOTS: u32 = 1;
const PREVIEW_PERF_STATS_WINDOW: usize = 120;
const RECENT_PRODUCT_FRAME_COMMIT_LIMIT: usize = 256;
const RECENT_POST_PRESENT_PROOF_QUEUE_LIMIT: usize = 64;
const RECENT_POST_PRESENT_PROOF_ARTIFACT_LIMIT: usize = 64;
const RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT: usize = 16;
const PRE_INPUT_COMPLETED_PROOF_DRAIN_LIMIT: usize = 4;
const POST_PRESENT_PROOF_SUBSCRIBER_PENDING_BATCH_LIMIT: usize = 4;

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
    pub dispatch: boon_host::SemanticSourceDispatch,
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
    scene: &boon_host::SemanticScene,
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

pub fn accesskit_focus_update_from_semantic_node(
    focused: &boon_host::SemanticId,
    node: Option<&boon_host::SemanticNode>,
) -> NativeAccessibilitySnapshot {
    let focus = accesskit_node_id_for_semantic_id(focused);
    let mut id_map = BTreeMap::new();
    id_map.insert(focused.clone(), focus);
    if let Some(node) = node {
        id_map.insert(node.id.clone(), accesskit_node_id_for_semantic_id(&node.id));
        if let Some(parent) = node.relations.parent.as_ref() {
            id_map.insert(parent.clone(), accesskit_node_id_for_semantic_id(parent));
        }
        for child in &node.relations.children {
            id_map.insert(child.clone(), accesskit_node_id_for_semantic_id(child));
        }
    }
    let nodes = node
        .map(|node| {
            (
                accesskit_node_id_for_semantic_id(&node.id),
                accesskit_node_from_semantic_node(node, &id_map),
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    let semantic_node_ids = node
        .map(|node| NativeAccessibilityNodeMapping {
            semantic_id: node.id.0.clone(),
            accesskit_node_id: accesskit_node_id_for_semantic_id(&node.id).0,
        })
        .into_iter()
        .collect::<Vec<_>>();
    let metrics = NativeAccessibilityMetrics {
        semantic_node_count: node.is_some() as usize,
        accesskit_node_count: nodes.len(),
        interactive_node_count: node
            .filter(|node| node.actions.press || node.actions.set_text)
            .is_some() as usize,
        focusable_node_count: node
            .filter(|node| node.actions.focus || node.state.focused)
            .is_some() as usize,
        text_input_node_count: node.filter(|node| node.actions.set_text).is_some() as usize,
        checked_node_count: node
            .filter(|node| node.state.checked == Some(true))
            .is_some() as usize,
        node_id_collision_count: 0,
        root_present: false,
        focus_present: true,
    };
    NativeAccessibilitySnapshot {
        tree_update: accesskit::TreeUpdate {
            nodes,
            tree: None,
            tree_id: accesskit::TreeId::ROOT,
            focus,
        },
        metrics,
        semantic_node_ids,
    }
}

fn accesskit_node_id_map(
    scene: &boon_host::SemanticScene,
) -> BTreeMap<boon_host::SemanticId, accesskit::NodeId> {
    scene
        .nodes
        .keys()
        .map(|id| (id.clone(), accesskit_node_id_for_semantic_id(id)))
        .collect()
}

fn accesskit_node_id_for_semantic_id(id: &boon_host::SemanticId) -> accesskit::NodeId {
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
    scene: &boon_host::SemanticScene,
    requests: &[NativeAccessibilityActionRequest],
) -> Vec<boon_host::SemanticSourceDispatch> {
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
    scene: &boon_host::SemanticScene,
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
    semantic_id: boon_host::SemanticId,
    request: &NativeAccessibilityActionRequest,
) -> Option<boon_host::SemanticInputEvent> {
    match request.action {
        NativeAccessibilityAction::Focus => {
            Some(boon_host::SemanticInputEvent::Focus { semantic_id })
        }
        NativeAccessibilityAction::Click => {
            Some(boon_host::SemanticInputEvent::Press { semantic_id })
        }
        NativeAccessibilityAction::SetValue => Some(boon_host::SemanticInputEvent::SetText {
            semantic_id,
            text: request.value.clone().unwrap_or_default(),
        }),
        NativeAccessibilityAction::ReplaceSelectedText => {
            Some(boon_host::SemanticInputEvent::ReplaceSelectedText {
                semantic_id,
                text: request.value.clone().unwrap_or_default(),
            })
        }
        NativeAccessibilityAction::Increment => {
            Some(boon_host::SemanticInputEvent::Increment { semantic_id })
        }
        NativeAccessibilityAction::Decrement => {
            Some(boon_host::SemanticInputEvent::Decrement { semantic_id })
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
    semantic: &boon_host::SemanticNode,
    id_map: &BTreeMap<boon_host::SemanticId, accesskit::NodeId>,
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

fn accesskit_role_for_semantic_role(role: &boon_host::SemanticRole) -> accesskit::Role {
    match role {
        boon_host::SemanticRole::Application => accesskit::Role::Application,
        boon_host::SemanticRole::Group => accesskit::Role::Group,
        boon_host::SemanticRole::Row => accesskit::Role::Row,
        boon_host::SemanticRole::Text => accesskit::Role::TextRun,
        boon_host::SemanticRole::Button => accesskit::Role::Button,
        boon_host::SemanticRole::Checkbox => accesskit::Role::CheckBox,
        boon_host::SemanticRole::TextInput => accesskit::Role::TextInput,
        boon_host::SemanticRole::Table => accesskit::Role::Table,
        boon_host::SemanticRole::Cell => accesskit::Role::Cell,
        boon_host::SemanticRole::ScrollRegion => accesskit::Role::ScrollView,
    }
}

fn accesskit_value_for_semantic_value(value: Option<&boon_host::SemanticValue>) -> Option<String> {
    match value? {
        boon_host::SemanticValue::Text { text } => Some(text.clone()),
        boon_host::SemanticValue::Bool { value } => Some(value.to_string()),
        boon_host::SemanticValue::Number { value } => Some(value.to_string()),
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
    pub sample_input_after_initial_frames: bool,
    pub warmup_frame_count: u32,
    pub sample_frame_count: u32,
    pub readback_artifact_dir: Option<String>,
    pub render_loop_state_report: Option<String>,
    pub demand_driven_loop: bool,
    pub proof_mode: NativeProofMode,
    pub skip_interactive_surface_readback_when_external_proof: bool,
}

impl NativeWindowOptions {
    fn readback_enabled(&self) -> bool {
        self.proof_mode == NativeProofMode::Readback && self.readback_artifact_dir.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRenderLoopMode {
    ContinuousProbe,
    DemandDriven,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProofMode {
    #[default]
    Counters,
    Readback,
}

impl NativeProofMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Counters => "counters",
            Self::Readback => "readback",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFramePacingState {
    Idle,
    RequestedAnimationBurst,
    Probe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePresentPathMode {
    DirectVisibleSurface,
    AppOwnedOffscreenCopyToPresent,
}

impl NativePresentPathMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectVisibleSurface => "direct_visible_surface",
            Self::AppOwnedOffscreenCopyToPresent => "app_owned_offscreen_copy_to_present",
        }
    }

    fn render_target_kind(self) -> &'static str {
        match self {
            Self::DirectVisibleSurface => "visible-surface-direct",
            Self::AppOwnedOffscreenCopyToPresent => "app-owned-offscreen-copy-to-present",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativePresentPathSelection {
    requested_mode: NativePresentPathMode,
    selected_mode: NativePresentPathMode,
    reason: &'static str,
    hooks_present: bool,
    surface_copy_to_present_supported: bool,
    readback_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeFramePacing {
    pub state: NativeFramePacingState,
    pub target_frame_interval_ms: f64,
    pub last_frame_interval_ms: Option<f64>,
    pub last_frame_lateness_ms: Option<f64>,
    pub timer_due: bool,
    pub requested_animation_burst_frames_remaining: u32,
    pub requested_animation_burst_started_elapsed_ms: Option<f64>,
    pub requested_animation_burst_quiet_until_elapsed_ms: Option<f64>,
    pub requested_animation_burst_hard_stop_elapsed_ms: Option<f64>,
    pub requested_animation_burst_min_frames: u32,
    pub requested_animation_quiet_ms: u64,
    pub requested_animation_hard_cap_ms: u64,
    pub requested_animation_max_pending_snapshots: u32,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFrameLane {
    ProductInteraction,
    AnimationFollowup,
    ProofOrHarness,
    RuntimeOrLayout,
    DevTelemetry,
    SurfaceLifecycle,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NativeAcceptedInputFrameTiming {
    pub timing_scope: String,
    pub frame_lane: NativeFrameLane,
    pub input_event_wake_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_input_event: Option<NativeHostInputEventSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_wake_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_wake_to_input_accept_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_wake_to_present_ms: Option<f64>,
    pub input_to_present_ms: f64,
    pub input_accept_elapsed_ms: Option<f64>,
    pub dirty_poll_elapsed_ms: Option<f64>,
    pub render_started_elapsed_ms: Option<f64>,
    pub surface_acquired_elapsed_ms: Option<f64>,
    pub render_hook_completed_elapsed_ms: Option<f64>,
    pub queue_submitted_elapsed_ms: Option<f64>,
    pub present_completed_elapsed_ms: Option<f64>,
    pub input_accept_to_dirty_poll_ms: Option<f64>,
    pub dirty_poll_to_render_started_ms: Option<f64>,
    pub render_started_to_surface_acquired_ms: Option<f64>,
    pub render_started_to_render_hook_completed_ms: Option<f64>,
    pub surface_acquired_to_render_hook_completed_ms: Option<f64>,
    pub render_hook_completed_to_present_ms: Option<f64>,
    pub render_hook_to_queue_ms: Option<f64>,
    pub queue_to_present_ms: Option<f64>,
    pub surface_acquire_call_ms: Option<f64>,
    pub encoder_finish_ms: Option<f64>,
    pub queue_submit_call_ms: Option<f64>,
    pub present_call_ms: Option<f64>,
    pub present_path_ms: Option<f64>,
    pub scheduler_reason: Option<NativeSchedulerReason>,
    pub render_target_kind: Option<String>,
    pub present_path_mode: Option<NativePresentPathMode>,
}

impl NativeAcceptedInputFrameTiming {
    fn from_state(
        state: &NativeRenderLoopState,
        input_event_wake_count: u64,
        input_to_present_ms: f64,
    ) -> Self {
        Self {
            timing_scope: "accepted_visible_host_input_frame".to_owned(),
            frame_lane: NativeFrameLane::ProductInteraction,
            input_event_wake_count,
            host_input_event: state.last_accepted_host_input_event.clone(),
            input_wake_elapsed_ms: state
                .last_accepted_host_input_event
                .as_ref()
                .and_then(|event| event.wake_elapsed_ms),
            input_wake_to_input_accept_ms: state
                .last_accepted_host_input_event
                .as_ref()
                .and_then(|event| event.wake_to_accept_ms),
            input_wake_to_present_ms: state.last_accepted_host_input_event.as_ref().and_then(
                |event| {
                    elapsed_delta_ms(
                        event.wake_elapsed_ms,
                        state.last_present_completed_elapsed_ms,
                    )
                },
            ),
            input_to_present_ms,
            input_accept_elapsed_ms: state.last_accepted_host_input_elapsed_ms,
            dirty_poll_elapsed_ms: state.last_dirty_poll_elapsed_ms,
            render_started_elapsed_ms: state.last_render_started_elapsed_ms,
            surface_acquired_elapsed_ms: state.last_surface_acquired_elapsed_ms,
            render_hook_completed_elapsed_ms: state.last_render_hook_completed_elapsed_ms,
            queue_submitted_elapsed_ms: state.last_queue_submitted_elapsed_ms,
            present_completed_elapsed_ms: state.last_present_completed_elapsed_ms,
            input_accept_to_dirty_poll_ms: elapsed_delta_ms(
                state.last_accepted_host_input_elapsed_ms,
                state.last_dirty_poll_elapsed_ms,
            ),
            dirty_poll_to_render_started_ms: elapsed_delta_ms(
                state.last_dirty_poll_elapsed_ms,
                state.last_render_started_elapsed_ms,
            ),
            render_started_to_surface_acquired_ms: elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_surface_acquired_elapsed_ms,
            ),
            render_started_to_render_hook_completed_ms: elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            ),
            surface_acquired_to_render_hook_completed_ms: elapsed_delta_ms(
                state.last_surface_acquired_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            ),
            render_hook_completed_to_present_ms: elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            ),
            render_hook_to_queue_ms: elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_queue_submitted_elapsed_ms,
            ),
            queue_to_present_ms: match (
                state.last_queue_submitted_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            ) {
                (Some(queue_ms), Some(present_ms)) if present_ms >= queue_ms => {
                    Some(present_ms - queue_ms)
                }
                _ => None,
            },
            surface_acquire_call_ms: state.last_surface_acquire_call_ms,
            encoder_finish_ms: state.last_encoder_finish_ms,
            queue_submit_call_ms: state.last_queue_submit_call_ms,
            present_call_ms: state.last_present_call_ms,
            present_path_ms: state.last_present_path_ms,
            scheduler_reason: state.current_scheduler_reason,
            render_target_kind: state.last_render_target_kind.clone(),
            present_path_mode: state.last_present_path_mode,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeRenderLoopState {
    pub mode: NativeRenderLoopMode,
    pub dirty_revision: u64,
    pub presented_revision: u64,
    pub last_render_content_revision: u64,
    pub last_render_layout_revision: u64,
    pub last_render_scene_revision: u64,
    pub rendered_frame_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_preissued_frame_evidence_key: Option<FrameEvidenceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_preissued_frame_evidence_elapsed_ms: Option<f64>,
    #[serde(default)]
    pub last_frame_evidence_key_issued_before_present: bool,
    pub skipped_idle_poll_count: u64,
    pub input_poll_count: u64,
    pub input_inline_resample_count: u64,
    pub input_deferred_resample_count: u64,
    pub input_inline_resample_event_gap_count: u64,
    pub input_deferred_resample_event_gap_count: u64,
    pub last_input_resample_event_gap_count: u64,
    pub last_input_resample_kind: Option<String>,
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
    pub last_role_revision: u64,
    pub last_frame_lane: Option<NativeFrameLane>,
    pub current_frame_lane: Option<NativeFrameLane>,
    pub last_render_target_kind: Option<String>,
    pub last_present_path_requested_mode: Option<NativePresentPathMode>,
    pub last_present_path_mode: Option<NativePresentPathMode>,
    pub last_present_path_selection_reason: Option<String>,
    pub last_present_path_hooks_present: bool,
    pub last_present_path_surface_copy_to_present_supported: bool,
    pub last_present_path_readback_enabled: bool,
    pub last_poll_started_elapsed_ms: Option<f64>,
    pub last_dirty_poll_elapsed_ms: Option<f64>,
    pub last_accepted_host_input_event_wake_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accepted_host_input_event: Option<NativeHostInputEventSummary>,
    pub last_accepted_host_input_elapsed_ms: Option<f64>,
    pub last_accepted_host_input_press_only: bool,
    pub last_input_to_present_accounted_event_wake_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accounted_input_frame_timing: Option<NativeAcceptedInputFrameTiming>,
    pub last_external_wake_generation: u64,
    pub last_external_wake_observed_elapsed_ms: Option<f64>,
    pub last_render_started_elapsed_ms: Option<f64>,
    pub last_surface_acquired_elapsed_ms: Option<f64>,
    pub last_render_hook_completed_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_render_frame_metrics: Option<NativeRenderFrameMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_product_frame_commit: Option<NativeProductFrameCommit>,
    pub product_frame_commit_count: u64,
    #[serde(default)]
    pub recent_post_present_proof_queue: VecDeque<NativePostPresentProofQueueEntry>,
    #[serde(default)]
    pub post_present_proof_queue_enqueued_count: u64,
    #[serde(default)]
    pub post_present_proof_queue_deferred_count: u64,
    #[serde(default)]
    pub post_present_proof_queue_legacy_pre_present_count: u64,
    #[serde(default)]
    pub post_present_proof_queue_completed_count: u64,
    #[serde(default)]
    pub post_present_subscriber_drain_deferred_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_present_subscriber_drain_deferred_reason: Option<String>,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_enqueued_batch_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_enqueued_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_dropped_batch_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_dropped_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_completed_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_error_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_worker_pending_batch_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_present_proof_subscriber_worker_enqueue_ms: Option<f64>,
    #[serde(default)]
    pub recent_post_present_proof_artifacts: VecDeque<NativePostPresentProofArtifact>,
    #[serde(default)]
    pub post_present_proof_artifact_count: u64,
    #[serde(default)]
    pub post_present_proof_subscriber_error_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_present_proof_subscriber_error: Option<String>,
    #[serde(default)]
    pub proof_readback_deferred_count: u64,
    #[serde(default)]
    pub proof_readback_deferred_for_product_input_count: u64,
    #[serde(default)]
    pub proof_readback_deferred_for_interaction_burst_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_proof_readback_deferred_reason: Option<String>,
    #[serde(default)]
    pub proof_readback_queued_during_interaction_quiet_count: u64,
    pub last_queue_submitted_elapsed_ms: Option<f64>,
    pub last_present_completed_elapsed_ms: Option<f64>,
    pub last_present_interval_ms: Option<f64>,
    pub last_frame_lateness_ms: Option<f64>,
    pub last_surface_acquire_call_ms: Option<f64>,
    pub last_encoder_finish_ms: Option<f64>,
    pub last_queue_submit_call_ms: Option<f64>,
    pub last_present_call_ms: Option<f64>,
    pub last_present_path_ms: Option<f64>,
    pub missed_frame_count: u64,
    pub telemetry_drop_count: u64,
    pub last_missed_frame_cause: Option<String>,
    pub pre_input_subscriber_drain_skip_count: u64,
    pub last_pre_input_subscriber_drain_skip_reason: Option<String>,
    pub requested_animation_burst_count: u64,
    pub requested_animation_prewarm_count: u64,
    pub requested_animation_burst_present_on_wake: bool,
    pub armed_frame_token: u64,
    pub armed_frame_started_elapsed_ms: Option<f64>,
    #[serde(skip)]
    pub armed_frame_pending: bool,
    pub clean_armed_poll_count: u64,
    pub skipped_clean_burst_present_count: u64,
    pub input_waited_for_already_armed_frame_count: u64,
    pub last_burst_exit_reason: Option<String>,
    pub requested_animation_burst_frames_remaining: u32,
    pub requested_animation_burst_started_elapsed_ms: Option<f64>,
    pub requested_animation_burst_quiet_until_elapsed_ms: Option<f64>,
    pub requested_animation_burst_hard_stop_elapsed_ms: Option<f64>,
    pub last_poll_diagnostics: Option<serde_json::Value>,
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
            last_render_layout_revision: 0,
            last_render_scene_revision: 0,
            rendered_frame_count: 0,
            last_preissued_frame_evidence_key: None,
            last_preissued_frame_evidence_elapsed_ms: None,
            last_frame_evidence_key_issued_before_present: false,
            skipped_idle_poll_count: 0,
            input_poll_count: 0,
            input_inline_resample_count: 0,
            input_deferred_resample_count: 0,
            input_inline_resample_event_gap_count: 0,
            input_deferred_resample_event_gap_count: 0,
            last_input_resample_event_gap_count: 0,
            last_input_resample_kind: None,
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
            last_role_revision: 0,
            last_frame_lane: None,
            current_frame_lane: None,
            last_render_target_kind: None,
            last_present_path_requested_mode: None,
            last_present_path_mode: None,
            last_present_path_selection_reason: None,
            last_present_path_hooks_present: false,
            last_present_path_surface_copy_to_present_supported: false,
            last_present_path_readback_enabled: false,
            last_poll_started_elapsed_ms: None,
            last_dirty_poll_elapsed_ms: None,
            last_accepted_host_input_event_wake_count: 0,
            last_accepted_host_input_event: None,
            last_accepted_host_input_elapsed_ms: None,
            last_accepted_host_input_press_only: false,
            last_input_to_present_accounted_event_wake_count: 0,
            last_accounted_input_frame_timing: None,
            last_external_wake_generation: 0,
            last_external_wake_observed_elapsed_ms: None,
            last_render_started_elapsed_ms: None,
            last_surface_acquired_elapsed_ms: None,
            last_render_hook_completed_elapsed_ms: None,
            last_render_frame_metrics: None,
            last_product_frame_commit: None,
            product_frame_commit_count: 0,
            recent_post_present_proof_queue: VecDeque::new(),
            post_present_proof_queue_enqueued_count: 0,
            post_present_proof_queue_deferred_count: 0,
            post_present_proof_queue_legacy_pre_present_count: 0,
            post_present_proof_queue_completed_count: 0,
            post_present_subscriber_drain_deferred_count: 0,
            last_post_present_subscriber_drain_deferred_reason: None,
            post_present_proof_subscriber_worker_enqueued_batch_count: 0,
            post_present_proof_subscriber_worker_enqueued_count: 0,
            post_present_proof_subscriber_worker_dropped_batch_count: 0,
            post_present_proof_subscriber_worker_dropped_count: 0,
            post_present_proof_subscriber_worker_completed_count: 0,
            post_present_proof_subscriber_worker_error_count: 0,
            post_present_proof_subscriber_worker_pending_batch_count: 0,
            last_post_present_proof_subscriber_worker_enqueue_ms: None,
            recent_post_present_proof_artifacts: VecDeque::new(),
            post_present_proof_artifact_count: 0,
            post_present_proof_subscriber_error_count: 0,
            last_post_present_proof_subscriber_error: None,
            proof_readback_deferred_count: 0,
            proof_readback_deferred_for_product_input_count: 0,
            proof_readback_deferred_for_interaction_burst_count: 0,
            last_proof_readback_deferred_reason: None,
            proof_readback_queued_during_interaction_quiet_count: 0,
            last_queue_submitted_elapsed_ms: None,
            last_present_completed_elapsed_ms: None,
            last_present_interval_ms: None,
            last_frame_lateness_ms: None,
            last_surface_acquire_call_ms: None,
            last_encoder_finish_ms: None,
            last_queue_submit_call_ms: None,
            last_present_call_ms: None,
            last_present_path_ms: None,
            missed_frame_count: 0,
            telemetry_drop_count: 0,
            last_missed_frame_cause: None,
            pre_input_subscriber_drain_skip_count: 0,
            last_pre_input_subscriber_drain_skip_reason: None,
            requested_animation_burst_count: 0,
            requested_animation_prewarm_count: 0,
            requested_animation_burst_present_on_wake: false,
            armed_frame_token: 0,
            armed_frame_started_elapsed_ms: None,
            armed_frame_pending: false,
            clean_armed_poll_count: 0,
            skipped_clean_burst_present_count: 0,
            input_waited_for_already_armed_frame_count: 0,
            last_burst_exit_reason: None,
            requested_animation_burst_frames_remaining: 0,
            requested_animation_burst_started_elapsed_ms: None,
            requested_animation_burst_quiet_until_elapsed_ms: None,
            requested_animation_burst_hard_stop_elapsed_ms: None,
            last_poll_diagnostics: None,
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
        let frame_lane = native_frame_lane_for_scheduler(
            Some(scheduler_reason),
            role_dirty_reason,
            false,
            false,
        );
        self.last_frame_lane = Some(frame_lane);
        self.current_frame_lane = Some(frame_lane);
        self.dirty_revision
    }

    pub fn mark_presented(&mut self, revision: u64) {
        self.presented_revision = self.presented_revision.max(revision);
        self.last_render_content_revision = self.last_render_content_revision.max(revision);
        self.last_render_layout_revision = self.last_render_layout_revision.max(revision);
        self.last_render_scene_revision = self.last_render_scene_revision.max(revision);
        self.rendered_frame_count = self.rendered_frame_count.saturating_add(1);
        if self.presented_revision >= self.dirty_revision {
            self.current_scheduler_reason = None;
            self.current_role_dirty_reason = None;
            self.current_frame_lane = None;
        }
    }

    pub fn mark_presented_with_content(&mut self, revision: u64, content_revision: u64) {
        self.mark_presented_with_revisions(
            revision,
            content_revision,
            content_revision,
            content_revision,
        );
    }

    pub fn mark_presented_with_revisions(
        &mut self,
        revision: u64,
        content_revision: u64,
        layout_revision: u64,
        render_scene_revision: u64,
    ) {
        self.presented_revision = self.presented_revision.max(revision);
        self.last_render_content_revision = content_revision;
        self.last_render_layout_revision = layout_revision;
        self.last_render_scene_revision = render_scene_revision;
        self.rendered_frame_count = self.rendered_frame_count.saturating_add(1);
        if self.presented_revision >= self.dirty_revision {
            self.current_scheduler_reason = None;
            self.current_role_dirty_reason = None;
            self.current_frame_lane = None;
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

    pub fn note_input_inline_resample(&mut self, event_gap_count: u64) {
        self.input_inline_resample_count = self.input_inline_resample_count.saturating_add(1);
        self.input_inline_resample_event_gap_count = self
            .input_inline_resample_event_gap_count
            .saturating_add(event_gap_count);
        self.last_input_resample_event_gap_count = event_gap_count;
        self.last_input_resample_kind = Some("inline_before_hook".to_owned());
    }

    pub fn note_input_deferred_resample(&mut self, event_gap_count: u64) {
        self.input_deferred_resample_count = self.input_deferred_resample_count.saturating_add(1);
        self.input_deferred_resample_event_gap_count = self
            .input_deferred_resample_event_gap_count
            .saturating_add(event_gap_count);
        self.last_input_resample_event_gap_count = event_gap_count;
        self.last_input_resample_kind = Some("deferred_next_loop".to_owned());
    }

    pub fn note_input_pre_present_resample(&mut self, event_gap_count: u64) {
        self.input_deferred_resample_count = self.input_deferred_resample_count.saturating_add(1);
        self.input_deferred_resample_event_gap_count = self
            .input_deferred_resample_event_gap_count
            .saturating_add(event_gap_count);
        self.last_input_resample_event_gap_count = event_gap_count;
        self.last_input_resample_kind = Some("pre_present_drop".to_owned());
    }

    pub fn note_input_post_present_stale_readback_skip(&mut self, event_gap_count: u64) {
        self.input_deferred_resample_count = self.input_deferred_resample_count.saturating_add(1);
        self.input_deferred_resample_event_gap_count = self
            .input_deferred_resample_event_gap_count
            .saturating_add(event_gap_count);
        self.last_input_resample_event_gap_count = event_gap_count;
        self.last_input_resample_kind = Some("post_present_stale_readback_skip".to_owned());
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

    pub fn note_accepted_host_input(
        &mut self,
        input_event_wake_count: u64,
        elapsed_ms: f64,
        press_only: bool,
        host_input_event: Option<NativeHostInputEventSummary>,
    ) {
        self.last_accepted_host_input_event_wake_count = input_event_wake_count;
        self.last_accepted_host_input_event = host_input_event;
        self.last_accepted_host_input_elapsed_ms = Some(elapsed_ms);
        self.last_accepted_host_input_press_only = press_only;
        if self.armed_frame_pending || self.armed_frame_started_elapsed_ms.is_some() {
            self.input_waited_for_already_armed_frame_count = self
                .input_waited_for_already_armed_frame_count
                .saturating_add(1);
        }
        self.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
        self.current_scheduler_reason = Some(NativeSchedulerReason::HostInput);
        self.last_frame_lane = Some(NativeFrameLane::ProductInteraction);
        self.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
    }

    pub fn take_frame_accepted_input_to_present_ms(
        &mut self,
        presented_input_event_wake_count: u64,
    ) -> Option<f64> {
        self.take_frame_accepted_input_timing(presented_input_event_wake_count)
            .map(|timing| timing.input_to_present_ms)
    }

    pub fn take_frame_accepted_input_timing(
        &mut self,
        presented_input_event_wake_count: u64,
    ) -> Option<NativeAcceptedInputFrameTiming> {
        if presented_input_event_wake_count == 0
            || self.last_accepted_host_input_event_wake_count == 0
            || self.last_accepted_host_input_event_wake_count
                <= self.last_input_to_present_accounted_event_wake_count
            || self.last_accepted_host_input_event_wake_count != presented_input_event_wake_count
        {
            return None;
        }
        let latency_ms = elapsed_delta_ms(
            self.last_accepted_host_input_elapsed_ms,
            self.last_present_completed_elapsed_ms,
        )?;
        let timing = NativeAcceptedInputFrameTiming::from_state(
            self,
            self.last_accepted_host_input_event_wake_count,
            latency_ms,
        );
        self.last_input_to_present_accounted_event_wake_count =
            self.last_accepted_host_input_event_wake_count;
        self.last_accounted_input_frame_timing = Some(timing.clone());
        Some(timing)
    }

    pub fn current_frame_carries_unaccounted_host_input(
        &self,
        presented_input_event_wake_count: u64,
    ) -> bool {
        presented_input_event_wake_count != 0
            && self.last_accepted_host_input_event_wake_count == presented_input_event_wake_count
            && self.last_accepted_host_input_event_wake_count
                > self.last_input_to_present_accounted_event_wake_count
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

    pub fn note_surface_acquire_call(&mut self, call_ms: f64) {
        self.last_surface_acquire_call_ms = Some(call_ms);
    }

    pub fn note_render_hook_completed(&mut self, elapsed_ms: f64) {
        self.last_render_hook_completed_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_render_frame_metrics(&mut self, metrics: Option<NativeRenderFrameMetrics>) {
        self.last_render_frame_metrics = metrics;
    }

    pub fn note_product_frame_commit(&mut self, commit: NativeProductFrameCommit) {
        self.enqueue_post_present_proof_requests(
            &commit.frame_evidence_key,
            &commit.post_present_proof_requests,
            commit.present_completed_elapsed_ms,
        );
        self.product_frame_commit_count = self.product_frame_commit_count.saturating_add(1);
        self.last_product_frame_commit = Some(commit);
    }

    pub fn enqueue_post_present_proof_requests(
        &mut self,
        frame_evidence_key: &FrameEvidenceKey,
        requests: &[NativePostPresentProofRequestSummary],
        enqueued_elapsed_ms: Option<f64>,
    ) {
        for request in requests {
            let status = if request.currently_legacy_pre_present {
                self.post_present_proof_queue_legacy_pre_present_count = self
                    .post_present_proof_queue_legacy_pre_present_count
                    .saturating_add(1);
                NativePostPresentProofQueueStatus::LegacyAlreadyBuiltPrePresent
            } else {
                self.post_present_proof_queue_deferred_count = self
                    .post_present_proof_queue_deferred_count
                    .saturating_add(1);
                NativePostPresentProofQueueStatus::Queued
            };
            self.post_present_proof_queue_enqueued_count = self
                .post_present_proof_queue_enqueued_count
                .saturating_add(1);
            self.recent_post_present_proof_queue
                .push_back(NativePostPresentProofQueueEntry {
                    schema_version: 1,
                    frame_evidence_key: frame_evidence_key.clone(),
                    request: request.clone(),
                    status,
                    enqueued_elapsed_ms,
                    completed_elapsed_ms: None,
                });
            while self.recent_post_present_proof_queue.len() > RECENT_POST_PRESENT_PROOF_QUEUE_LIMIT
            {
                self.recent_post_present_proof_queue.pop_front();
            }
        }
    }

    pub fn note_post_present_proof_request_completed(
        &mut self,
        frame_evidence_key: &FrameEvidenceKey,
        kind: NativePostPresentProofRequestKind,
        completed_elapsed_ms: Option<f64>,
    ) -> bool {
        let Some(entry) = self
            .recent_post_present_proof_queue
            .iter_mut()
            .rev()
            .find(|entry| {
                entry.frame_evidence_key == *frame_evidence_key && entry.request.kind == kind
            })
        else {
            return false;
        };
        if entry.status != NativePostPresentProofQueueStatus::Queued {
            return false;
        }
        entry.status = NativePostPresentProofQueueStatus::CompletedPostPresent;
        entry.completed_elapsed_ms = completed_elapsed_ms;
        self.post_present_proof_queue_completed_count = self
            .post_present_proof_queue_completed_count
            .saturating_add(1);
        true
    }

    fn has_queued_post_present_proof_request(
        &self,
        frame_evidence_key: &FrameEvidenceKey,
        kind: NativePostPresentProofRequestKind,
    ) -> bool {
        self.recent_post_present_proof_queue
            .iter()
            .rev()
            .any(|entry| {
                entry.frame_evidence_key == *frame_evidence_key
                    && entry.request.kind == kind
                    && entry.status == NativePostPresentProofQueueStatus::Queued
            })
    }

    pub fn note_post_present_subscriber_drain_deferred(&mut self, reason: &str) {
        self.post_present_subscriber_drain_deferred_count = self
            .post_present_subscriber_drain_deferred_count
            .saturating_add(1);
        self.last_post_present_subscriber_drain_deferred_reason = Some(reason.to_owned());
    }

    fn note_post_present_proof_subscriber_worker_enqueue(
        &mut self,
        report: AsyncPostPresentProofSubscriberEnqueueReport,
    ) {
        self.post_present_proof_subscriber_worker_enqueued_batch_count = self
            .post_present_proof_subscriber_worker_enqueued_batch_count
            .saturating_add(report.enqueued_batches);
        self.post_present_proof_subscriber_worker_enqueued_count = self
            .post_present_proof_subscriber_worker_enqueued_count
            .saturating_add(report.enqueued_subscribers);
        self.post_present_proof_subscriber_worker_dropped_batch_count = self
            .post_present_proof_subscriber_worker_dropped_batch_count
            .saturating_add(report.dropped_batches);
        self.post_present_proof_subscriber_worker_dropped_count = self
            .post_present_proof_subscriber_worker_dropped_count
            .saturating_add(report.dropped_subscribers);
        self.post_present_proof_subscriber_worker_pending_batch_count = report.pending_batches;
        self.last_post_present_proof_subscriber_worker_enqueue_ms = report.enqueue_ms;
    }

    fn note_post_present_proof_subscriber_worker_completed(&mut self) {
        self.post_present_proof_subscriber_worker_completed_count = self
            .post_present_proof_subscriber_worker_completed_count
            .saturating_add(1);
    }

    fn note_post_present_proof_subscriber_worker_error(&mut self) {
        self.post_present_proof_subscriber_worker_error_count = self
            .post_present_proof_subscriber_worker_error_count
            .saturating_add(1);
    }

    pub fn note_post_present_proof_artifact(&mut self, artifact: NativePostPresentProofArtifact) {
        self.note_post_present_proof_request_completed(
            &artifact.frame_evidence_key,
            artifact.kind,
            artifact.completed_elapsed_ms,
        );
        self.record_post_present_proof_artifact(artifact);
    }

    fn note_post_present_proof_artifact_if_request_queued(
        &mut self,
        artifact: NativePostPresentProofArtifact,
    ) -> bool {
        if !self.note_post_present_proof_request_completed(
            &artifact.frame_evidence_key,
            artifact.kind,
            artifact.completed_elapsed_ms,
        ) {
            return false;
        }
        self.record_post_present_proof_artifact(artifact);
        true
    }

    fn record_post_present_proof_artifact(&mut self, artifact: NativePostPresentProofArtifact) {
        self.post_present_proof_artifact_count =
            self.post_present_proof_artifact_count.saturating_add(1);
        self.last_post_present_proof_subscriber_error = None;
        self.recent_post_present_proof_artifacts.push_back(artifact);
        while self.recent_post_present_proof_artifacts.len()
            > RECENT_POST_PRESENT_PROOF_ARTIFACT_LIMIT
        {
            self.recent_post_present_proof_artifacts.pop_front();
        }
    }

    pub fn note_post_present_proof_subscriber_error(&mut self, error: String) {
        self.post_present_proof_subscriber_error_count = self
            .post_present_proof_subscriber_error_count
            .saturating_add(1);
        self.last_post_present_proof_subscriber_error = Some(error);
    }

    pub fn note_proof_readback_deferred(&mut self, reason: &str) {
        self.proof_readback_deferred_count = self.proof_readback_deferred_count.saturating_add(1);
        match reason {
            "product_input_frame" => {
                self.proof_readback_deferred_for_product_input_count = self
                    .proof_readback_deferred_for_product_input_count
                    .saturating_add(1);
            }
            "interaction_burst" => {
                self.proof_readback_deferred_for_interaction_burst_count = self
                    .proof_readback_deferred_for_interaction_burst_count
                    .saturating_add(1);
            }
            _ => {}
        }
        self.last_proof_readback_deferred_reason = Some(reason.to_owned());
    }

    pub fn note_proof_readback_queued_during_interaction_quiet(&mut self) {
        self.proof_readback_queued_during_interaction_quiet_count = self
            .proof_readback_queued_during_interaction_quiet_count
            .saturating_add(1);
    }

    pub fn note_preissued_frame_evidence_key(&mut self, key: FrameEvidenceKey, elapsed_ms: f64) {
        self.last_preissued_frame_evidence_key = Some(key);
        self.last_preissued_frame_evidence_elapsed_ms = Some(elapsed_ms);
        self.last_frame_evidence_key_issued_before_present = true;
    }

    pub fn note_queue_submitted(&mut self, elapsed_ms: f64) {
        self.last_queue_submitted_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_present_completed(&mut self, elapsed_ms: f64) {
        if let Some(previous_ms) = self.last_present_completed_elapsed_ms
            && elapsed_ms >= previous_ms
        {
            let interval_ms = elapsed_ms - previous_ms;
            self.last_present_interval_ms = Some(interval_ms);
            let lateness_ms = (interval_ms - NATIVE_TARGET_FRAME_INTERVAL_MS).max(0.0);
            self.last_frame_lateness_ms = Some(lateness_ms);
            let frame_paced = self.mode == NativeRenderLoopMode::ContinuousProbe
                || self.current_scheduler_reason == Some(NativeSchedulerReason::RequestedAnimation);
            if frame_paced && lateness_ms > NATIVE_TARGET_FRAME_INTERVAL_MS {
                self.missed_frame_count = self.missed_frame_count.saturating_add(1);
                self.last_missed_frame_cause = Some(
                    match self.mode {
                        NativeRenderLoopMode::ContinuousProbe => {
                            "continuous_probe_present_interval_exceeded_two_frames"
                        }
                        NativeRenderLoopMode::DemandDriven => {
                            "requested_animation_present_interval_exceeded_two_frames"
                        }
                    }
                    .to_owned(),
                );
            }
        }
        if self.requested_animation_burst_frames_remaining > 0 {
            self.requested_animation_burst_frames_remaining = self
                .requested_animation_burst_frames_remaining
                .saturating_sub(1);
        }
        self.armed_frame_pending = false;
        self.armed_frame_started_elapsed_ms = None;
        self.last_present_completed_elapsed_ms = Some(elapsed_ms);
    }

    pub fn note_submit_phase_durations(
        &mut self,
        encoder_finish_ms: f64,
        queue_submit_call_ms: f64,
        present_call_ms: f64,
    ) {
        self.last_encoder_finish_ms = Some(encoder_finish_ms);
        self.last_queue_submit_call_ms = Some(queue_submit_call_ms);
        self.last_present_call_ms = Some(present_call_ms);
        self.last_present_path_ms = Some(
            self.last_surface_acquire_call_ms.unwrap_or(0.0)
                + queue_submit_call_ms
                + present_call_ms,
        );
    }

    pub fn note_render_target_kind(&mut self, render_target_kind: &'static str) {
        self.last_render_target_kind = Some(render_target_kind.to_owned());
    }

    fn note_present_path_selection(&mut self, selection: NativePresentPathSelection) {
        self.last_present_path_requested_mode = Some(selection.requested_mode);
        self.last_present_path_mode = Some(selection.selected_mode);
        self.last_present_path_selection_reason = Some(selection.reason.to_owned());
        self.last_present_path_hooks_present = selection.hooks_present;
        self.last_present_path_surface_copy_to_present_supported =
            selection.surface_copy_to_present_supported;
        self.last_present_path_readback_enabled = selection.readback_enabled;
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

    pub fn note_pre_input_subscriber_drain_skipped(&mut self, reason: impl Into<String>) {
        self.pre_input_subscriber_drain_skip_count =
            self.pre_input_subscriber_drain_skip_count.saturating_add(1);
        self.last_pre_input_subscriber_drain_skip_reason = Some(reason.into());
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

    pub fn request_animation_burst(
        &mut self,
        now: Instant,
        elapsed_ms: f64,
        reason: NativeSchedulerReason,
    ) {
        if self.mode != NativeRenderLoopMode::DemandDriven {
            return;
        }
        self.extend_requested_animation_burst(elapsed_ms, true);
        self.last_scheduler_reason = Some(reason);
        self.current_scheduler_reason = Some(reason);
        self.schedule_wake_after(now, native_target_frame_interval_duration());
    }

    pub fn request_interactive_prewarm_burst(&mut self, now: Instant, elapsed_ms: f64) {
        if self.mode != NativeRenderLoopMode::DemandDriven {
            return;
        }
        self.requested_animation_prewarm_count =
            self.requested_animation_prewarm_count.saturating_add(1);
        self.extend_requested_animation_burst(elapsed_ms, false);
        if self.current_scheduler_reason.is_none() {
            self.last_scheduler_reason = Some(NativeSchedulerReason::RequestedAnimation);
        }
        self.schedule_wake_after(now, native_target_frame_interval_duration());
    }

    fn extend_requested_animation_burst(&mut self, elapsed_ms: f64, present_on_wake: bool) {
        let active = self
            .requested_animation_burst_hard_stop_elapsed_ms
            .is_some_and(|hard_stop| elapsed_ms <= hard_stop);
        if !active {
            self.requested_animation_burst_count =
                self.requested_animation_burst_count.saturating_add(1);
            self.requested_animation_burst_started_elapsed_ms = Some(elapsed_ms);
            self.requested_animation_burst_hard_stop_elapsed_ms =
                Some(elapsed_ms + REQUESTED_ANIMATION_HARD_CAP_MS as f64);
            self.requested_animation_burst_present_on_wake = false;
            self.last_burst_exit_reason = None;
        }
        self.requested_animation_burst_present_on_wake |= present_on_wake;
        let hard_stop = self
            .requested_animation_burst_hard_stop_elapsed_ms
            .unwrap_or(elapsed_ms + REQUESTED_ANIMATION_HARD_CAP_MS as f64);
        self.requested_animation_burst_quiet_until_elapsed_ms =
            Some((elapsed_ms + REQUESTED_ANIMATION_QUIET_MS as f64).min(hard_stop));
        self.requested_animation_burst_frames_remaining = self
            .requested_animation_burst_frames_remaining
            .max(REQUESTED_ANIMATION_BURST_MIN_FRAMES);
    }

    pub fn consume_due_wake_after_poll(&mut self, now: Instant) -> bool {
        if self.next_wake_at.is_some_and(|wake_at| now >= wake_at)
            && self.current_scheduler_reason == Some(NativeSchedulerReason::HostInput)
            && self.last_accepted_host_input_event_wake_count > 0
            && self.last_accepted_host_input_event_wake_count
                > self.last_input_to_present_accounted_event_wake_count
        {
            self.next_wake_at = None;
            self.scheduled_wake_count = self.scheduled_wake_count.saturating_add(1);
            return true;
        }
        self.consume_due_wake(now)
    }

    pub fn schedule_requested_animation_followup(&mut self, now: Instant, elapsed_ms: f64) {
        if self.mode != NativeRenderLoopMode::DemandDriven {
            return;
        }
        if self
            .requested_animation_burst_hard_stop_elapsed_ms
            .is_some_and(|hard_stop| elapsed_ms > hard_stop)
        {
            self.clear_requested_animation_burst();
            return;
        }
        let in_quiet_window = self
            .requested_animation_burst_quiet_until_elapsed_ms
            .is_some_and(|quiet_until| elapsed_ms < quiet_until);
        if self.requested_animation_burst_frames_remaining > 0 || in_quiet_window {
            let delay = self
                .last_render_started_elapsed_ms
                .map(|render_started_ms| {
                    let next_frame_deadline_ms =
                        render_started_ms + NATIVE_TARGET_FRAME_INTERVAL_MS;
                    let remaining_ms = (next_frame_deadline_ms - elapsed_ms).max(0.0);
                    Duration::from_secs_f64(remaining_ms / 1000.0)
                })
                .unwrap_or_else(native_target_frame_interval_duration);
            self.schedule_wake_after(now, delay);
        }
    }

    pub fn clear_requested_animation_burst_if_quiet(&mut self, elapsed_ms: f64) {
        if self.requested_animation_burst_frames_remaining == 0
            && self
                .requested_animation_burst_quiet_until_elapsed_ms
                .is_some_and(|quiet_until| elapsed_ms >= quiet_until)
        {
            self.last_burst_exit_reason = Some("quiet_interval_elapsed".to_owned());
            self.clear_requested_animation_burst();
        }
    }

    fn clear_requested_animation_burst(&mut self) {
        self.requested_animation_burst_frames_remaining = 0;
        self.requested_animation_burst_present_on_wake = false;
        self.armed_frame_pending = false;
        self.armed_frame_started_elapsed_ms = None;
        self.requested_animation_burst_started_elapsed_ms = None;
        self.requested_animation_burst_quiet_until_elapsed_ms = None;
        self.requested_animation_burst_hard_stop_elapsed_ms = None;
    }

    pub fn interaction_burst_active(&self, elapsed_ms: f64) -> bool {
        self.requested_animation_burst_frames_remaining > 0
            || self
                .requested_animation_burst_quiet_until_elapsed_ms
                .is_some_and(|quiet_until| elapsed_ms < quiet_until)
    }

    pub fn interaction_quiet_proof_readback_frame(&self, elapsed_ms: f64) -> bool {
        self.current_frame_lane == Some(NativeFrameLane::AnimationFollowup)
            && self.requested_animation_burst_frames_remaining == 0
            && self
                .requested_animation_burst_quiet_until_elapsed_ms
                .is_some_and(|quiet_until| elapsed_ms < quiet_until)
    }

    pub fn defer_proof_readback_for_product_lane(&self, elapsed_ms: f64) -> bool {
        if self.current_frame_lane == Some(NativeFrameLane::ProofOrHarness)
            || self.current_scheduler_reason == Some(NativeSchedulerReason::VerifierFrame)
        {
            return false;
        }
        self.interaction_burst_active(elapsed_ms)
    }

    pub fn consume_due_wake(&mut self, now: Instant) -> bool {
        if self.next_wake_at.is_some_and(|wake_at| now >= wake_at) {
            self.next_wake_at = None;
            let burst_pacing_active = self.requested_animation_burst_frames_remaining > 0
                || self
                    .requested_animation_burst_quiet_until_elapsed_ms
                    .is_some();
            let reason = if burst_pacing_active {
                if self.requested_animation_burst_present_on_wake {
                    self.dirty_revision = self.dirty_revision.saturating_add(1);
                    NativeSchedulerReason::RequestedAnimation
                } else {
                    self.armed_frame_token = self.armed_frame_token.saturating_add(1);
                    self.armed_frame_pending = true;
                    self.armed_frame_started_elapsed_ms = None;
                    self.last_scheduler_reason = Some(NativeSchedulerReason::RequestedAnimation);
                    self.scheduled_wake_count = self.scheduled_wake_count.saturating_add(1);
                    return true;
                }
            } else {
                NativeSchedulerReason::Timer
            };
            self.last_scheduler_reason = Some(reason);
            self.current_scheduler_reason = Some(reason);
            self.current_role_dirty_reason = None;
            let frame_lane = native_frame_lane_for_scheduler(Some(reason), None, false, false);
            self.last_frame_lane = Some(frame_lane);
            self.current_frame_lane = Some(frame_lane);
            self.scheduled_wake_count = self.scheduled_wake_count.saturating_add(1);
            true
        } else {
            false
        }
    }

    pub fn note_armed_frame_input_sampled(&mut self, elapsed_ms: f64) {
        if self.armed_frame_pending && self.armed_frame_started_elapsed_ms.is_none() {
            self.armed_frame_started_elapsed_ms = Some(elapsed_ms);
        }
    }

    pub fn clean_armed_poll_pending(&self) -> bool {
        self.armed_frame_pending
            && self.armed_frame_started_elapsed_ms.is_some()
            && self.dirty_revision == self.presented_revision
            && self.current_scheduler_reason.is_none()
    }

    pub fn skip_clean_armed_burst_present(&mut self, now: Instant, elapsed_ms: f64) {
        if !self.clean_armed_poll_pending() {
            return;
        }
        self.clean_armed_poll_count = self.clean_armed_poll_count.saturating_add(1);
        self.skipped_clean_burst_present_count =
            self.skipped_clean_burst_present_count.saturating_add(1);
        self.armed_frame_pending = false;
        self.armed_frame_started_elapsed_ms = None;
        if self.requested_animation_burst_frames_remaining > 0 {
            self.requested_animation_burst_frames_remaining = self
                .requested_animation_burst_frames_remaining
                .saturating_sub(1);
        }
        if self
            .requested_animation_burst_hard_stop_elapsed_ms
            .is_some_and(|hard_stop| elapsed_ms > hard_stop)
        {
            self.last_burst_exit_reason = Some("hard_cap_elapsed".to_owned());
            self.clear_requested_animation_burst();
            return;
        }
        let in_quiet_window = self
            .requested_animation_burst_quiet_until_elapsed_ms
            .is_some_and(|quiet_until| elapsed_ms < quiet_until);
        if self.requested_animation_burst_frames_remaining > 0 || in_quiet_window {
            self.schedule_wake_after(now, native_target_frame_interval_duration());
        } else {
            self.last_burst_exit_reason = Some("clean_armed_poll_quiet".to_owned());
            self.clear_requested_animation_burst();
        }
    }

    pub fn idle_wait_timeout(&self, now: Instant) -> Duration {
        self.next_wake_at
            .and_then(|wake_at| wake_at.checked_duration_since(now))
            .map(|timeout| timeout.min(PASSIVE_INPUT_POLL_INTERVAL))
            .unwrap_or(PASSIVE_INPUT_POLL_INTERVAL)
    }

    pub fn apply_poll_result(&mut self, poll_result: &NativePollResult, real_os_input: bool) {
        self.last_poll_diagnostics = poll_result.diagnostics.clone();
        if poll_result.dirty {
            self.last_role_revision = poll_result.role_revision;
            self.last_scheduler_reason = poll_result.scheduler_reason.or_else(|| {
                if real_os_input {
                    Some(NativeSchedulerReason::HostInput)
                } else {
                    Some(NativeSchedulerReason::ExternalWake)
                }
            });
            self.current_scheduler_reason = self.last_scheduler_reason;
            self.current_role_dirty_reason = poll_result.role_dirty_reason;
            let frame_lane = poll_result.frame_lane.unwrap_or_else(|| {
                native_frame_lane_for_scheduler(
                    self.current_scheduler_reason,
                    poll_result.role_dirty_reason,
                    real_os_input,
                    poll_result.wants_animation_frame,
                )
            });
            self.last_frame_lane = Some(frame_lane);
            self.current_frame_lane = Some(frame_lane);
            if poll_result.role_dirty_reason.is_some() {
                self.last_role_dirty_reason = poll_result.role_dirty_reason;
            }
            let scheduler_only_repaint = poll_result.role_dirty_reason.is_none()
                && self.last_scheduler_reason == Some(NativeSchedulerReason::HostInput);
            let host_input_visible_repaint = poll_result.role_revision <= self.presented_revision
                && self.last_scheduler_reason == Some(NativeSchedulerReason::HostInput);
            let accepted_real_input_visible_repaint =
                real_os_input && poll_result.role_revision <= self.presented_revision;
            if poll_result.role_revision > self.presented_revision {
                self.dirty_revision = self.dirty_revision.max(poll_result.role_revision);
            } else if self.last_scheduler_reason == Some(NativeSchedulerReason::VerifierFrame) {
                self.dirty_revision = self
                    .dirty_revision
                    .max(self.presented_revision.saturating_add(1));
            } else if scheduler_only_repaint
                || host_input_visible_repaint
                || accepted_real_input_visible_repaint
            {
                self.dirty_revision = self.dirty_revision.saturating_add(1);
            } else {
                self.dirty_revision = self.dirty_revision.max(poll_result.role_revision);
            }
        }
        if poll_result.wants_animation_frame && !poll_result.dirty {
            self.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
            if let Some(frame_lane) = poll_result.frame_lane {
                self.last_frame_lane = Some(frame_lane);
                self.current_frame_lane = Some(frame_lane);
            }
        }
    }

    pub fn content_validation_revision(&self) -> u64 {
        if self.last_role_revision > 0 {
            self.last_role_revision
        } else {
            self.dirty_revision
        }
    }
}

fn native_frame_lane_for_scheduler(
    scheduler_reason: Option<NativeSchedulerReason>,
    role_dirty_reason: Option<NativeRoleDirtyReason>,
    real_os_input: bool,
    wants_animation_frame: bool,
) -> NativeFrameLane {
    if matches!(scheduler_reason, Some(NativeSchedulerReason::VerifierFrame))
        || matches!(
            role_dirty_reason,
            Some(NativeRoleDirtyReason::VerifierFrame)
        )
    {
        return NativeFrameLane::ProofOrHarness;
    }
    if real_os_input || matches!(scheduler_reason, Some(NativeSchedulerReason::HostInput)) {
        return NativeFrameLane::ProductInteraction;
    }
    if matches!(
        scheduler_reason,
        Some(NativeSchedulerReason::RequestedAnimation)
    ) || wants_animation_frame
    {
        return NativeFrameLane::AnimationFollowup;
    }
    if matches!(
        scheduler_reason,
        Some(NativeSchedulerReason::SurfaceChanged | NativeSchedulerReason::SurfaceLifecycle)
    ) {
        return NativeFrameLane::SurfaceLifecycle;
    }
    if matches!(
        role_dirty_reason,
        Some(NativeRoleDirtyReason::TelemetrySummaryChanged)
    ) {
        return NativeFrameLane::DevTelemetry;
    }
    if matches!(
        role_dirty_reason,
        Some(
            NativeRoleDirtyReason::RuntimeTurnApplied
                | NativeRoleDirtyReason::DocumentPatchApplied
                | NativeRoleDirtyReason::LayoutChanged
                | NativeRoleDirtyReason::ScrollChanged
                | NativeRoleDirtyReason::FocusChanged
                | NativeRoleDirtyReason::SourcePayloadAccepted
                | NativeRoleDirtyReason::WorkspaceSelectionChanged
                | NativeRoleDirtyReason::ErrorOverlayChanged
        )
    ) {
        return NativeFrameLane::RuntimeOrLayout;
    }
    NativeFrameLane::RuntimeOrLayout
}

fn native_target_frame_interval_duration() -> Duration {
    Duration::from_micros((NATIVE_TARGET_FRAME_INTERVAL_MS * 1000.0).round() as u64)
}

fn post_present_subscriber_drain_allowed(
    current_input_event_wake_count: u64,
    presented_input_event_wake_count: u64,
) -> bool {
    current_input_event_wake_count <= presented_input_event_wake_count
}

fn pre_submit_proof_poll_allowed(
    product_input_frame: bool,
    interaction_burst_active: bool,
    frame_lane: Option<NativeFrameLane>,
    scheduler_reason: Option<NativeSchedulerReason>,
) -> bool {
    if product_input_frame {
        return false;
    }
    if frame_lane == Some(NativeFrameLane::ProofOrHarness)
        || scheduler_reason == Some(NativeSchedulerReason::VerifierFrame)
    {
        return true;
    }
    !interaction_burst_active
}

#[cfg(test)]
fn run_post_present_proof_subscribers(
    state: &mut NativeRenderLoopState,
    subscribers: Vec<NativePostPresentProofSubscriber>,
    frame_evidence_key: &FrameEvidenceKey,
    completed_elapsed_ms: Option<f64>,
) {
    for subscriber in subscribers {
        match subscriber(NativePostPresentProofContext {
            frame_evidence_key: frame_evidence_key.clone(),
            completed_elapsed_ms,
        }) {
            Ok(artifact) => state.note_post_present_proof_artifact(artifact),
            Err(error) => state.note_post_present_proof_subscriber_error(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AsyncPostPresentProofSubscriberEnqueueReport {
    enqueued_batches: u64,
    enqueued_subscribers: u64,
    dropped_batches: u64,
    dropped_subscribers: u64,
    pending_batches: u64,
    enqueue_ms: Option<f64>,
}

struct AsyncPostPresentProofSubscriberBatch {
    context: NativePostPresentProofContext,
    subscribers: Vec<NativePostPresentProofSubscriber>,
}

#[derive(Default)]
struct AsyncPostPresentProofSubscriberShared {
    pending: VecDeque<AsyncPostPresentProofSubscriberBatch>,
    shutdown: bool,
}

struct AsyncPostPresentProofSubscriberWorker {
    shared: Arc<(Mutex<AsyncPostPresentProofSubscriberShared>, Condvar)>,
    results: mpsc::Receiver<Result<NativePostPresentProofArtifact, String>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl AsyncPostPresentProofSubscriberWorker {
    fn new() -> Self {
        let shared = Arc::new((
            Mutex::new(AsyncPostPresentProofSubscriberShared::default()),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let (sender, results) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("boon-post-present-proof-worker".to_owned())
            .spawn(move || {
                loop {
                    let batch = {
                        let (lock, cvar) = &*worker_shared;
                        let mut shared = lock.lock().expect("post-present proof worker lock");
                        while shared.pending.is_empty() && !shared.shutdown {
                            shared = cvar.wait(shared).expect("post-present proof worker wait");
                        }
                        if shared.pending.is_empty() && shared.shutdown {
                            break;
                        }
                        shared
                            .pending
                            .pop_front()
                            .expect("pending proof batch after wait")
                    };
                    for subscriber in batch.subscribers {
                        let result = subscriber(batch.context.clone());
                        if sender.send(result).is_err() {
                            break;
                        }
                    }
                }
            })
            .expect("spawn post-present proof worker");
        Self {
            shared,
            results,
            worker: Some(worker),
        }
    }

    fn enqueue(
        &self,
        subscribers: Vec<NativePostPresentProofSubscriber>,
        frame_evidence_key: FrameEvidenceKey,
        completed_elapsed_ms: Option<f64>,
        enqueue_started: Instant,
    ) -> Option<AsyncPostPresentProofSubscriberEnqueueReport> {
        if subscribers.is_empty() {
            return None;
        }
        let subscriber_count = subscribers.len() as u64;
        let mut report = AsyncPostPresentProofSubscriberEnqueueReport {
            enqueued_batches: 1,
            enqueued_subscribers: subscriber_count,
            enqueue_ms: Some(elapsed_ms(enqueue_started)),
            ..AsyncPostPresentProofSubscriberEnqueueReport::default()
        };
        let batch = AsyncPostPresentProofSubscriberBatch {
            context: NativePostPresentProofContext {
                frame_evidence_key,
                completed_elapsed_ms,
            },
            subscribers,
        };
        let (lock, cvar) = &*self.shared;
        let mut shared = lock.lock().expect("post-present proof enqueue lock");
        while shared.pending.len() >= POST_PRESENT_PROOF_SUBSCRIBER_PENDING_BATCH_LIMIT {
            if let Some(dropped) = shared.pending.pop_front() {
                report.dropped_batches = report.dropped_batches.saturating_add(1);
                report.dropped_subscribers = report
                    .dropped_subscribers
                    .saturating_add(dropped.subscribers.len() as u64);
            }
        }
        shared.pending.push_back(batch);
        report.pending_batches = shared.pending.len() as u64;
        cvar.notify_one();
        Some(report)
    }

    fn drain_completed(&self, state: &mut NativeRenderLoopState) -> usize {
        self.drain_completed_limit(state, usize::MAX)
    }

    fn drain_completed_limit(
        &self,
        state: &mut NativeRenderLoopState,
        max_completed: usize,
    ) -> usize {
        let mut completed = 0_usize;
        while completed < max_completed {
            match self.results.try_recv() {
                Ok(Ok(artifact)) => {
                    state.note_post_present_proof_subscriber_worker_completed();
                    state.note_post_present_proof_artifact(artifact);
                    completed = completed.saturating_add(1);
                }
                Ok(Err(error)) => {
                    state.note_post_present_proof_subscriber_worker_error();
                    state.note_post_present_proof_subscriber_error(error);
                    completed = completed.saturating_add(1);
                }
                Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        let (lock, _) = &*self.shared;
        if let Ok(shared) = lock.lock() {
            state.post_present_proof_subscriber_worker_pending_batch_count =
                shared.pending.len() as u64;
        }
        completed
    }

    fn shutdown(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let (lock, cvar) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.shutdown = true;
            cvar.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    fn shutdown_and_drain(&mut self, state: &mut NativeRenderLoopState) {
        self.shutdown();
        let _ = self.drain_completed(state);
    }
}

impl Drop for AsyncPostPresentProofSubscriberWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePollResult {
    pub dirty: bool,
    pub role_revision: u64,
    pub scheduler_reason: Option<NativeSchedulerReason>,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_lane: Option<NativeFrameLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_host_input_event_hint: Option<NativeHostInputEventHint>,
    pub next_wake_after_ms: Option<u64>,
    pub wants_animation_frame: bool,
    pub cursor_icon: NativeCursorIcon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<serde_json::Value>,
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
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            wants_animation_frame: false,
            cursor_icon: NativeCursorIcon::Default,
            diagnostics: None,
            accessibility_update: None,
        }
    }
}

fn effective_poll_result_for_host_input(
    mut poll_result: NativePollResult,
    real_os_input: bool,
) -> NativePollResult {
    if !poll_result.dirty
        && real_os_input
        && poll_result.scheduler_reason == Some(NativeSchedulerReason::HostInput)
        && poll_result.role_dirty_reason.is_some()
    {
        poll_result.dirty = true;
    }
    if poll_result.dirty && real_os_input {
        poll_result.scheduler_reason = Some(NativeSchedulerReason::HostInput);
        poll_result.frame_lane = Some(NativeFrameLane::ProductInteraction);
    }
    poll_result
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

pub struct NativeRenderHookResult {
    pub proof: Option<serde_json::Value>,
    pub content_revision: u64,
    pub layout_revision: Option<u64>,
    pub render_scene_revision: Option<u64>,
    pub render_frame_metrics: Option<NativeRenderFrameMetrics>,
    pub post_present_proof_subscribers: Vec<NativePostPresentProofSubscriber>,
    pub rendered: bool,
    pub content_changed: bool,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
}

impl std::fmt::Debug for NativeRenderHookResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRenderHookResult")
            .field("proof", &self.proof)
            .field("content_revision", &self.content_revision)
            .field("layout_revision", &self.layout_revision)
            .field("render_scene_revision", &self.render_scene_revision)
            .field("render_frame_metrics", &self.render_frame_metrics)
            .field(
                "post_present_proof_subscriber_count",
                &self.post_present_proof_subscribers.len(),
            )
            .field("rendered", &self.rendered)
            .field("content_changed", &self.content_changed)
            .field("role_dirty_reason", &self.role_dirty_reason)
            .finish()
    }
}

impl NativeRenderHookResult {
    pub fn rendered_with_proof(proof: serde_json::Value) -> Self {
        Self {
            proof: Some(proof),
            content_revision: 0,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: true,
            role_dirty_reason: None,
        }
    }

    pub fn rendered_without_proof() -> Self {
        Self {
            proof: None,
            content_revision: 0,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
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
        if self.layout_revision == Some(0) {
            return Err(
                "render hook result layout_revision must be nonzero when provided".to_owned(),
            );
        }
        if self.render_scene_revision == Some(0) {
            return Err(
                "render hook result render_scene_revision must be nonzero when provided".to_owned(),
            );
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
        let host_input_repaint_can_present_existing_content = matches!(
            scheduler_reason,
            Some(
                NativeSchedulerReason::HostInput
                    | NativeSchedulerReason::Timer
                    | NativeSchedulerReason::RequestedAnimation
            )
        );
        let scheduler_idle_same_content_repaint =
            scheduler_reason.is_none() && role_dirty_reason.is_none() && !self.content_changed;
        let external_retained_frame_repaint =
            matches!(scheduler_reason, Some(NativeSchedulerReason::ExternalWake))
                && matches!(
                    role_dirty_reason,
                    Some(
                        NativeRoleDirtyReason::LayoutChanged
                            | NativeRoleDirtyReason::ScrollChanged
                            | NativeRoleDirtyReason::FocusChanged
                    )
                );
        if self.content_revision < dirty_revision
            && !same_content_surface_render
            && !scheduler_only_input_repaint
            && !host_input_repaint_can_present_existing_content
            && !scheduler_idle_same_content_repaint
            && !external_retained_frame_repaint
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
        let _ = dirty_revision;
        let _ = scheduler_reason;
        let _ = role_dirty_reason;
        self.content_revision
    }

    pub fn presented_revisions(
        &self,
        dirty_revision: u64,
        scheduler_reason: Option<NativeSchedulerReason>,
        role_dirty_reason: Option<NativeRoleDirtyReason>,
    ) -> (u64, u64, u64) {
        let content_revision =
            self.presented_content_revision(dirty_revision, scheduler_reason, role_dirty_reason);
        let layout_revision = self.layout_revision.unwrap_or(content_revision);
        let render_scene_revision = self.render_scene_revision.unwrap_or(layout_revision);
        (content_revision, layout_revision, render_scene_revision)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_evidence_key: Option<FrameEvidenceKey>,
    pub wgpu_strategy: String,
    pub wgpu_surface_strategy: String,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub adapter_device: u32,
    pub adapter_vendor: u32,
    pub adapter_device_type: String,
    pub adapter_is_software: bool,
    pub surface_format: String,
    pub present_mode: String,
    pub supported_present_modes: Vec<String>,
    pub non_vsync_present_mode_available: bool,
    pub desired_maximum_frame_latency: u32,
    pub desired_maximum_frame_latency_source: String,
    pub alpha_mode: String,
    pub supported_alpha_modes: Vec<String>,
    pub supported_usages: String,
    pub surface_copy_to_present_supported: bool,
    pub surface_copy_src_readback_supported: bool,
    pub present_path_mode: Option<NativePresentPathMode>,
    pub present_path_requested_mode: Option<NativePresentPathMode>,
    pub present_path_selection_reason: Option<String>,
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
    pub input_event_wake_count: u64,
    pub app_window_surface_content_report: Option<serde_json::Value>,
    pub input_sample_delay_ms: u64,
    pub sample_input_after_initial_frames: bool,
    pub frame_timing: NativeFrameTimingProof,
    pub post_input_frame_timing: Option<NativeFrameTimingProof>,
    pub input_adapter: NativeInputAdapterProof,
    pub external_render_proof: Option<serde_json::Value>,
    pub readback_artifact: Option<AppWindowReadbackArtifact>,
}

fn low_latency_present_mode(capabilities: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    if capabilities
        .present_modes
        .contains(&wgpu::PresentMode::Immediate)
    {
        wgpu::PresentMode::Immediate
    } else if capabilities
        .present_modes
        .contains(&wgpu::PresentMode::AutoNoVsync)
    {
        wgpu::PresentMode::AutoNoVsync
    } else if capabilities
        .present_modes
        .contains(&wgpu::PresentMode::Mailbox)
    {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    }
}

fn present_mode_from_name(name: &str) -> Option<wgpu::PresentMode> {
    let normalized = name
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "fifo" => Some(wgpu::PresentMode::Fifo),
        "fiforelaxed" => Some(wgpu::PresentMode::FifoRelaxed),
        "immediate" => Some(wgpu::PresentMode::Immediate),
        "mailbox" => Some(wgpu::PresentMode::Mailbox),
        "autovsync" => Some(wgpu::PresentMode::AutoVsync),
        "autonovsync" => Some(wgpu::PresentMode::AutoNoVsync),
        _ => None,
    }
}

fn configured_low_latency_present_mode(
    capabilities: &wgpu::SurfaceCapabilities,
    requested: Option<&str>,
) -> wgpu::PresentMode {
    if let Some(mode) = requested
        .and_then(present_mode_from_name)
        .filter(|mode| capabilities.present_modes.contains(mode))
    {
        return mode;
    }
    low_latency_present_mode(capabilities)
}

fn interactive_desired_maximum_frame_latency(present_mode: wgpu::PresentMode) -> u32 {
    match present_mode {
        wgpu::PresentMode::Mailbox
        | wgpu::PresentMode::Immediate
        | wgpu::PresentMode::AutoNoVsync => PACED_SURFACE_FRAME_LATENCY,
        _ => PACED_SURFACE_FRAME_LATENCY,
    }
}

fn configured_desired_maximum_frame_latency(
    present_mode: wgpu::PresentMode,
    requested: Option<&str>,
) -> (u32, &'static str) {
    let default = interactive_desired_maximum_frame_latency(present_mode);
    let Some(requested) = requested else {
        return (default, "present_mode_default");
    };
    let Ok(parsed) = requested.trim().parse::<u32>() else {
        return (default, "invalid_env_default");
    };
    (
        parsed.clamp(1, MAX_CONFIGURED_SURFACE_FRAME_LATENCY),
        "env_override",
    )
}

fn surface_present_mode_names(capabilities: &wgpu::SurfaceCapabilities) -> Vec<String> {
    capabilities
        .present_modes
        .iter()
        .map(|mode| format!("{mode:?}"))
        .collect()
}

fn surface_alpha_mode_names(capabilities: &wgpu::SurfaceCapabilities) -> Vec<String> {
    capabilities
        .alpha_modes
        .iter()
        .map(|mode| format!("{mode:?}"))
        .collect()
}

fn surface_has_non_vsync_present_mode(capabilities: &wgpu::SurfaceCapabilities) -> bool {
    capabilities
        .present_modes
        .iter()
        .any(|mode| !matches!(mode, wgpu::PresentMode::Fifo | wgpu::PresentMode::AutoVsync))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeFrameTimingProof {
    pub warmup_frame_count: u32,
    pub sample_frame_count: u32,
    pub measured_frame_count: u32,
    pub first_presented_frame_ms: f64,
    pub surface_acquire_ms_p50: f64,
    pub surface_acquire_ms_p95: f64,
    pub surface_acquire_ms_max: f64,
    pub present_submit_ms_p50: f64,
    pub present_submit_ms_p95: f64,
    pub present_submit_ms_max: f64,
    pub command_record_ms_p50: f64,
    pub command_record_ms_p95: f64,
    pub command_record_ms_max: f64,
    pub encoder_finish_ms_p50: f64,
    pub encoder_finish_ms_p95: f64,
    pub encoder_finish_ms_max: f64,
    pub queue_submit_ms_p50: f64,
    pub queue_submit_ms_p95: f64,
    pub queue_submit_ms_max: f64,
    pub frame_present_ms_p50: f64,
    pub frame_present_ms_p95: f64,
    pub frame_present_ms_max: f64,
    pub sample_pacing_wait_ms_p50: f64,
    pub sample_pacing_wait_ms_p95: f64,
    pub sample_pacing_wait_ms_max: f64,
    pub post_present_bookkeeping_ms_p50: f64,
    pub post_present_bookkeeping_ms_p95: f64,
    pub post_present_bookkeeping_ms_max: f64,
    pub presented_frame_ms_p50: f64,
    pub presented_frame_ms_p95: f64,
    pub presented_frame_ms_p99: f64,
    pub presented_frame_ms_max: f64,
    pub presented_frame_ms_over_16_7_count: u32,
    pub presented_frame_ms_over_16_7_indices: Vec<u32>,
    pub presented_frame_ms_over_16_7_max: f64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_elapsed_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeHostInputEventSummary {
    pub scope: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_intent: Option<String>,
    pub sequence: Option<u64>,
    pub input_event_wake_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_wake_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_to_accept_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_to_accept_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_protocol_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub mouse_button_delta_count: usize,
    pub keyboard_delta_count: usize,
    pub has_scroll_delta: bool,
    pub has_motion_delta: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeHostInputEventHint {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_protocol_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_elapsed_ms: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeMouseWindowPosition {
    pub x: f64,
    pub y: f64,
    pub window_width: f64,
    pub window_height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameEvidenceKey {
    pub frame_seq: u64,
    pub content_revision: u64,
    pub layout_revision: u64,
    pub render_scene_revision: u64,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub input_event_seq: Option<u64>,
    pub present_id: u64,
    pub proof_request_id: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NativePerfMetricSummary {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
    pub sample_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePostPresentProofRequestKind {
    VisibleBoundText,
    RetainedBoundSync,
    RenderHookReportJson,
    ExternalAppOwnedReadback,
    VisibleSurfaceReadback,
    ProofHistory,
    ArtifactHash,
    DevRenderReportJson,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePostPresentProofRequestSummary {
    pub kind: NativePostPresentProofRequestKind,
    pub currently_legacy_pre_present: bool,
    pub frame_local_snapshot_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePostPresentProofQueueStatus {
    Queued,
    LegacyAlreadyBuiltPrePresent,
    CompletedPostPresent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativePostPresentProofQueueEntry {
    pub schema_version: u32,
    pub frame_evidence_key: FrameEvidenceKey,
    pub request: NativePostPresentProofRequestSummary,
    pub status: NativePostPresentProofQueueStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enqueued_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_elapsed_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativePostPresentProofArtifact {
    pub schema_version: u32,
    pub frame_evidence_key: FrameEvidenceKey,
    pub kind: NativePostPresentProofRequestKind,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_elapsed_ms: Option<f64>,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct NativePostPresentProofContext {
    pub frame_evidence_key: FrameEvidenceKey,
    pub completed_elapsed_ms: Option<f64>,
}

pub type NativePostPresentProofSubscriber = Box<
    dyn FnOnce(NativePostPresentProofContext) -> Result<NativePostPresentProofArtifact, String>
        + Send,
>;

pub fn native_post_present_json_proof_artifact(
    kind: NativePostPresentProofRequestKind,
    frame_evidence_key: FrameEvidenceKey,
    completed_elapsed_ms: Option<f64>,
    payload: serde_json::Value,
) -> NativePostPresentProofArtifact {
    NativePostPresentProofArtifact {
        schema_version: 1,
        frame_evidence_key,
        kind,
        status: "pass".to_owned(),
        completed_elapsed_ms,
        payload,
    }
}

pub fn native_post_present_json_proof_subscriber<F>(
    kind: NativePostPresentProofRequestKind,
    build_payload: F,
) -> NativePostPresentProofSubscriber
where
    F: FnOnce(&NativePostPresentProofContext) -> serde_json::Value + Send + 'static,
{
    Box::new(move |context| {
        let payload = build_payload(&context);
        Ok(native_post_present_json_proof_artifact(
            kind,
            context.frame_evidence_key.clone(),
            context.completed_elapsed_ms,
            payload,
        ))
    })
}

fn proof_history_post_present_subscriber(
    recent_frame_evidence_count: usize,
) -> NativePostPresentProofSubscriber {
    native_post_present_json_proof_subscriber(
        NativePostPresentProofRequestKind::ProofHistory,
        move |context| {
            serde_json::json!({
                "status": "pass",
                "proof_history_updated": true,
                "recent_frame_evidence_count": recent_frame_evidence_count,
                "frame_seq": context.frame_evidence_key.frame_seq,
                "input_event_seq": context.frame_evidence_key.input_event_seq
            })
        },
    )
}

fn render_hook_report_json_post_present_subscriber(
    report_path: String,
    enqueue_ms: f64,
) -> NativePostPresentProofSubscriber {
    native_post_present_json_proof_subscriber(
        NativePostPresentProofRequestKind::RenderHookReportJson,
        move |context| {
            serde_json::json!({
                "status": "pass",
                "report_snapshot_enqueued": true,
                "report_path": report_path,
                "enqueue_ms": enqueue_ms,
                "frame_seq": context.frame_evidence_key.frame_seq,
                "input_event_seq": context.frame_evidence_key.input_event_seq
            })
        },
    )
}

pub fn native_post_present_artifact_hash_subscriber(
    artifact_paths: Vec<String>,
) -> NativePostPresentProofSubscriber {
    Box::new(move |context| {
        let mut artifact_sha256s = Vec::with_capacity(artifact_paths.len());
        for artifact_path in &artifact_paths {
            let sha256 = sha256_file(Path::new(artifact_path)).map_err(|error| {
                format!("post-present artifact hash failed for {artifact_path}: {error}")
            })?;
            artifact_sha256s.push(serde_json::json!({
                "path": artifact_path,
                "sha256": sha256,
            }));
        }
        let hashed_artifact_count = artifact_sha256s.len();
        Ok(native_post_present_json_proof_artifact(
            NativePostPresentProofRequestKind::ArtifactHash,
            context.frame_evidence_key.clone(),
            context.completed_elapsed_ms,
            serde_json::json!({
                "status": "pass",
                "artifact_hash_status": if artifact_paths.is_empty() {
                    "no_registered_artifacts"
                } else {
                    "hashed_registered_artifacts"
                },
                "registered_artifact_count": artifact_paths.len(),
                "hashed_artifact_count": hashed_artifact_count,
                "artifact_sha256s": artifact_sha256s,
                "frame_seq": context.frame_evidence_key.frame_seq,
                "input_event_seq": context.frame_evidence_key.input_event_seq,
            }),
        ))
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeRenderedProductFrame {
    pub schema_version: u32,
    pub render_target_kind: String,
    pub visible_surface_rendered: bool,
    pub visible_present_path: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_scene_identity: Option<String>,
    pub legacy_proof_json_built_pre_present: bool,
    pub legacy_render_hook_proof_built_pre_present: bool,
    pub post_present_proof_request_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeProductFrameCommit {
    pub schema_version: u32,
    pub commit_source: String,
    pub frame_evidence_key: FrameEvidenceKey,
    pub frame_lane: NativeFrameLane,
    pub scheduler_reason: Option<NativeSchedulerReason>,
    pub role_dirty_reason: Option<NativeRoleDirtyReason>,
    pub input_event_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_input_event: Option<NativeHostInputEventSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_timing: Option<NativeAcceptedInputFrameTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_to_present_ms: Option<f64>,
    pub content_revision: u64,
    pub layout_revision: u64,
    pub render_scene_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_target_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present_path_mode: Option<NativePresentPathMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_started_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_acquired_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_completed_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_submitted_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present_completed_elapsed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_acquire_call_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_finish_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_submit_call_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present_call_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present_path_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_state_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_input_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_world_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_core_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_revision_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_total_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_phase_timings: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_frame: Option<NativeRenderedProductFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_present_proof_requests: Vec<NativePostPresentProofRequestSummary>,
    pub post_present_proof_request_count: u32,
    pub legacy_pre_present_proof_request_count: u32,
    pub legacy_product_proof_built_pre_present: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NativeRenderFrameMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_call_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glyph_cache_hit_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_item_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_display_item_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_write_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_blocked_on_ipc_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_state_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_input_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_world_snapshot_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_core_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_revision_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_outer_total_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_hook_phase_timings: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_frame: Option<NativeRenderedProductFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_present_proof_requests: Vec<NativePostPresentProofRequestSummary>,
}

#[derive(Clone, Debug)]
struct NativePreviewPerfAccumulator {
    render_hook_ms: VecDeque<f64>,
    layout_ms: VecDeque<f64>,
    present_call_ms: VecDeque<f64>,
    surface_acquire_call_ms: VecDeque<f64>,
    queue_submit_call_ms: VecDeque<f64>,
    present_path_ms: VecDeque<f64>,
    input_to_present_ms: VecDeque<f64>,
    product_input_to_present_ms: VecDeque<f64>,
    upload_bytes: VecDeque<f64>,
    draw_call_count: VecDeque<f64>,
    glyph_cache_hit_rate: VecDeque<f64>,
    materialized_item_count: VecDeque<f64>,
    proof_overhead_ms: VecDeque<f64>,
    product_frame_count: u64,
    product_missed_frame_count: u64,
}

impl Default for NativePreviewPerfAccumulator {
    fn default() -> Self {
        Self {
            render_hook_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            layout_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            present_call_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            surface_acquire_call_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            queue_submit_call_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            present_path_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            input_to_present_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            product_input_to_present_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            upload_bytes: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            draw_call_count: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            glyph_cache_hit_rate: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            materialized_item_count: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            proof_overhead_ms: VecDeque::with_capacity(PREVIEW_PERF_STATS_WINDOW),
            product_frame_count: 0,
            product_missed_frame_count: 0,
        }
    }
}

impl NativePreviewPerfAccumulator {
    fn record(
        &mut self,
        render_hook_ms: Option<f64>,
        render_frame_metrics: Option<&NativeRenderFrameMetrics>,
        present_call_ms: Option<f64>,
        surface_acquire_call_ms: Option<f64>,
        queue_submit_call_ms: Option<f64>,
        present_path_ms: Option<f64>,
        input_to_present_ms: Option<f64>,
        frame_lane: Option<NativeFrameLane>,
        proof_overhead_ms: Option<f64>,
    ) {
        push_perf_sample(&mut self.render_hook_ms, render_hook_ms);
        push_perf_sample(
            &mut self.layout_ms,
            render_frame_metrics.and_then(|metrics| metrics.layout_ms),
        );
        push_perf_sample(&mut self.present_call_ms, present_call_ms);
        push_perf_sample(&mut self.surface_acquire_call_ms, surface_acquire_call_ms);
        push_perf_sample(&mut self.queue_submit_call_ms, queue_submit_call_ms);
        push_perf_sample(&mut self.present_path_ms, present_path_ms);
        push_perf_sample(&mut self.input_to_present_ms, input_to_present_ms);
        if frame_lane == Some(NativeFrameLane::ProductInteraction)
            && let Some(input_to_present_ms) = input_to_present_ms.filter(|value| value.is_finite())
        {
            self.product_frame_count = self.product_frame_count.saturating_add(1);
            if input_to_present_ms > NATIVE_TARGET_FRAME_INTERVAL_MS {
                self.product_missed_frame_count = self.product_missed_frame_count.saturating_add(1);
            }
            push_perf_sample(
                &mut self.product_input_to_present_ms,
                Some(input_to_present_ms),
            );
        }
        push_perf_sample(
            &mut self.upload_bytes,
            render_frame_metrics
                .and_then(|metrics| metrics.upload_bytes)
                .map(|value| value as f64),
        );
        push_perf_sample(
            &mut self.draw_call_count,
            render_frame_metrics
                .and_then(|metrics| metrics.draw_call_count)
                .map(|value| value as f64),
        );
        push_perf_sample(
            &mut self.glyph_cache_hit_rate,
            render_frame_metrics.and_then(|metrics| metrics.glyph_cache_hit_rate),
        );
        push_perf_sample(
            &mut self.materialized_item_count,
            render_frame_metrics
                .and_then(|metrics| metrics.materialized_item_count)
                .map(|value| value as f64),
        );
        push_perf_sample(&mut self.proof_overhead_ms, proof_overhead_ms);
    }

    fn render_hook_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.render_hook_ms)
    }

    fn layout_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.layout_ms)
    }

    fn present_call_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.present_call_ms)
    }

    fn surface_acquire_call_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.surface_acquire_call_ms)
    }

    fn queue_submit_call_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.queue_submit_call_ms)
    }

    fn present_path_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.present_path_ms)
    }

    fn input_to_present_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.input_to_present_ms)
    }

    fn product_input_to_present_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.product_input_to_present_ms)
    }

    fn upload_bytes_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.upload_bytes)
    }

    fn draw_call_count_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.draw_call_count)
    }

    fn glyph_cache_hit_rate_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.glyph_cache_hit_rate)
    }

    fn materialized_item_count_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.materialized_item_count)
    }

    fn proof_overhead_summary(&self) -> NativePerfMetricSummary {
        metric_summary_from_samples(&self.proof_overhead_ms)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativePreviewPerfStats {
    pub kind: String,
    pub status: String,
    pub role: NativeWindowRole,
    pub frame_seq: u64,
    pub sample_elapsed_ms: f64,
    pub render_loop_mode: NativeRenderLoopMode,
    pub frame_pacing: NativeFramePacing,
    pub renders_per_second: f64,
    pub render_hook_ms: Option<f64>,
    pub present_call_ms: Option<f64>,
    pub frame_present_call_ms: Option<f64>,
    pub surface_acquire_call_ms: Option<f64>,
    pub queue_submit_call_ms: Option<f64>,
    pub present_path_ms: Option<f64>,
    pub input_to_present_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_lane: Option<NativeFrameLane>,
    pub render_hook_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub layout_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub present_call_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub frame_present_call_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub surface_acquire_call_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub queue_submit_call_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub present_path_ms_p50_p95_p99_max: NativePerfMetricSummary,
    pub input_to_present_ms_p50_p95_p99_max: NativePerfMetricSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_input_to_present_ms: Option<f64>,
    #[serde(default)]
    pub product_input_to_present_ms_p50_p95_p99_max: NativePerfMetricSummary,
    #[serde(default)]
    pub product_frame_count: u64,
    #[serde(default)]
    pub product_missed_frame_count: u64,
    pub upload_bytes_p50_p95_max: NativePerfMetricSummary,
    pub draw_call_count_p50_p95_max: NativePerfMetricSummary,
    pub glyph_cache_hit_rate: Option<f64>,
    pub glyph_cache_hit_rate_p50_p95_max: NativePerfMetricSummary,
    pub materialized_item_count: Option<u64>,
    pub materialized_item_count_p50_p95_max: NativePerfMetricSummary,
    pub missed_frame_count: u64,
    pub proof_mode: String,
    pub proof_overhead_ms: Option<f64>,
    pub proof_overhead_ms_p50_p95_max: NativePerfMetricSummary,
    pub telemetry_drop_count: u64,
    pub last_missed_frame_cause: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_evidence_key: Option<FrameEvidenceKey>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_evidence_key: Option<FrameEvidenceKey>,
    pub capture_method: String,
    pub texture_format: String,
    pub nonblank_samples: usize,
    pub unique_rgba_values: usize,
    pub readback_deadline_ms: u64,
    pub readback_poll_status: String,
}

fn artifact_hash_requested_post_present(commit: &NativeProductFrameCommit) -> bool {
    commit.post_present_proof_requests.iter().any(|request| {
        request.kind == NativePostPresentProofRequestKind::ArtifactHash
            && !request.currently_legacy_pre_present
    })
}

fn post_present_artifact_hash_paths_for_frame(
    key: &FrameEvidenceKey,
    last_interactive_readback_artifact: Option<&AppWindowReadbackArtifact>,
    recent_interactive_readback_artifacts: &VecDeque<AppWindowReadbackArtifact>,
    external_render_proof: Option<&serde_json::Value>,
) -> Vec<String> {
    let mut artifact_paths = Vec::new();
    for artifact in recent_interactive_readback_artifacts.iter().rev() {
        if artifact.frame_evidence_key.as_ref() == Some(key) {
            push_existing_artifact_path(&mut artifact_paths, Some(&artifact.path));
        }
    }
    if let Some(artifact) = last_interactive_readback_artifact
        && artifact.frame_evidence_key.as_ref() == Some(key)
    {
        push_existing_artifact_path(&mut artifact_paths, Some(&artifact.path));
    }
    if let Some(proof) =
        external_render_proof_with_frame_evidence_key(external_render_proof, Some(key)).as_ref()
    {
        collect_external_render_proof_artifact_paths_for_frame(proof, key, &mut artifact_paths);
    }
    artifact_paths
}

fn should_defer_empty_artifact_hash_for_pending_readback(
    artifact_paths: &[String],
    state: &NativeRenderLoopState,
    key: &FrameEvidenceKey,
    readback_pending: bool,
) -> bool {
    artifact_paths.is_empty()
        && readback_pending
        && state.has_queued_post_present_proof_request(
            key,
            NativePostPresentProofRequestKind::VisibleSurfaceReadback,
        )
}

fn collect_external_render_proof_artifact_paths_for_frame(
    value: &serde_json::Value,
    key: &FrameEvidenceKey,
    artifact_paths: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            let frame_key_matches = object
                .get("frame_evidence_key")
                .and_then(|value| serde_json::from_value::<FrameEvidenceKey>(value.clone()).ok())
                .as_ref()
                == Some(key);
            let app_owned_readback = object
                .get("capture_method")
                .and_then(serde_json::Value::as_str)
                == Some("wgpu-visible-surface-copy-src-readback")
                || object.get("kind").and_then(serde_json::Value::as_str)
                    == Some("app_owned_pixels");
            if frame_key_matches && app_owned_readback {
                push_existing_artifact_path(
                    artifact_paths,
                    object
                        .get("artifact_path")
                        .or_else(|| object.get("path"))
                        .and_then(serde_json::Value::as_str),
                );
            }
            for child in object.values() {
                collect_external_render_proof_artifact_paths_for_frame(child, key, artifact_paths);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_external_render_proof_artifact_paths_for_frame(child, key, artifact_paths);
            }
        }
        _ => {}
    }
}

fn push_existing_artifact_path(artifact_paths: &mut Vec<String>, path: Option<&str>) {
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return;
    };
    if Path::new(path).is_file() && !artifact_paths.iter().any(|existing| existing == path) {
        artifact_paths.push(path.to_owned());
    }
}

struct PendingSurfaceReadback {
    buffer: wgpu::Buffer,
    role: NativeWindowRole,
    title: String,
    surface_id: SurfaceId,
    surface_epoch: u64,
    frame_evidence_key: Option<FrameEvidenceKey>,
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    format: wgpu::TextureFormat,
    capture_method: &'static str,
}

struct AsyncInteractiveReadbackResult {
    artifact: AppWindowReadbackArtifact,
    finish_ms: f64,
    completed_elapsed_ms: f64,
}

struct AsyncInteractiveReadbackJob {
    receiver: mpsc::Receiver<Result<AsyncInteractiveReadbackResult, String>>,
}

struct NativeOffscreenPresentTarget {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
}

fn cached_offscreen_present_target<'a>(
    target: &'a mut Option<NativeOffscreenPresentTarget>,
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> &'a wgpu::Texture {
    let stale = target.as_ref().is_none_or(|target| {
        target.width != width || target.height != height || target.format != format
    });
    if stale {
        *target = Some(NativeOffscreenPresentTarget {
            width,
            height,
            format,
            texture: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("boon-native-offscreen-present-target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
        });
    }
    &target
        .as_ref()
        .expect("offscreen present target should be initialized")
        .texture
}

pub struct NativeRenderFrameContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub surface_view: &'a wgpu::TextureView,
    pub surface_texture_format: wgpu::TextureFormat,
    pub render_target_kind: &'static str,
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
pub type NativePerfStatsHook = Box<dyn FnMut(NativePreviewPerfStats) + Send>;

pub struct NativeWindowHooks {
    pub input: Option<NativePollHook>,
    pub poll: Option<NativePollHook>,
    pub should_exit: Option<NativeExitHook>,
    pub render: NativeRenderHook,
    pub perf_stats: Option<NativePerfStatsHook>,
}

impl NativeWindowHooks {
    pub fn from_render_hook(render: NativeRenderHook) -> Self {
        Self {
            input: None,
            poll: None,
            should_exit: None,
            render,
            perf_stats: None,
        }
    }
}

fn native_window_exit_reason(hooks: &mut Option<NativeWindowHooks>) -> Option<String> {
    hooks
        .as_mut()
        .and_then(|hooks| hooks.should_exit.as_mut())
        .and_then(|should_exit| should_exit())
}

fn notify_native_perf_stats(hooks: &mut Option<NativeWindowHooks>, stats: NativePreviewPerfStats) {
    if let Some(callback) = hooks.as_mut().and_then(|hooks| hooks.perf_stats.as_mut()) {
        callback(stats);
    }
}

fn poll_native_window_input_hooks(
    hooks: &mut Option<NativeWindowHooks>,
    context: NativePollContext,
) -> Result<Option<NativePollResult>, NativeWindowError> {
    hooks
        .as_mut()
        .and_then(|hooks| hooks.input.as_mut())
        .map(|poll| {
            poll(context)
                .map_err(|error| NativeWindowError::Failed(format!("role input hook: {error}")))
        })
        .transpose()
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
    let latest_surface_size = Arc::new(Mutex::new(Size::ZERO));
    {
        let latest_surface_size = Arc::clone(&latest_surface_size);
        let resize_wake_count = Arc::clone(&resize_wake_count);
        let resize_wake_handle = wake_handle.clone();
        app_surface.size_update(move |size| {
            if let Ok(mut latest) = latest_surface_size.lock() {
                *latest = size;
            }
            resize_wake_count.fetch_add(1, Ordering::Relaxed);
            resize_wake_handle.wake();
        });
    }
    let (size, scale) = app_surface.size_scale().await;
    if let Ok(mut latest) = latest_surface_size.lock() {
        *latest = size;
    }
    let cached_surface_scale = scale;
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
    let input_event_wake_count = Arc::new(AtomicU64::new(0));
    let input_event_last_wake_at = Arc::new(Mutex::new(None::<Instant>));
    let input_event_wake_timeline = Arc::new(Mutex::new(VecDeque::<(u64, Instant)>::new()));
    {
        let input_event_wake_count = Arc::clone(&input_event_wake_count);
        let input_event_last_wake_at = Arc::clone(&input_event_last_wake_at);
        let input_event_wake_timeline = Arc::clone(&input_event_wake_timeline);
        let input_wake_handle = wake_handle.clone();
        mouse.on_input_event(move || {
            record_input_event_wake(
                &input_event_wake_count,
                &input_event_last_wake_at,
                &input_event_wake_timeline,
            );
            input_wake_handle.wake();
        });
    }
    {
        let input_event_wake_count = Arc::clone(&input_event_wake_count);
        let input_event_last_wake_at = Arc::clone(&input_event_last_wake_at);
        let input_event_wake_timeline = Arc::clone(&input_event_wake_timeline);
        let input_wake_handle = wake_handle.clone();
        keyboard.on_input_event(move || {
            record_input_event_wake(
                &input_event_wake_count,
                &input_event_last_wake_at,
                &input_event_wake_timeline,
            );
            input_wake_handle.wake();
        });
    }
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
    let supported_present_modes = surface_present_mode_names(&capabilities);
    let non_vsync_present_mode_available = surface_has_non_vsync_present_mode(&capabilities);
    let supported_alpha_modes = surface_alpha_mode_names(&capabilities);
    let supported_usages = format!("{:?}", capabilities.usages);
    let mut config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| NativeWindowError::Failed("surface default config unavailable".into()))?;
    if capabilities
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::Opaque)
    {
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }
    let requested_present_mode = std::env::var("BOON_NATIVE_PRESENT_MODE").ok();
    config.present_mode =
        configured_low_latency_present_mode(&capabilities, requested_present_mode.as_deref());
    let requested_desired_maximum_frame_latency =
        std::env::var("BOON_NATIVE_DESIRED_MAXIMUM_FRAME_LATENCY").ok();
    let (configured_desired_maximum_frame_latency, desired_maximum_frame_latency_source) =
        configured_desired_maximum_frame_latency(
            config.present_mode,
            requested_desired_maximum_frame_latency.as_deref(),
        );
    config.desired_maximum_frame_latency = configured_desired_maximum_frame_latency;
    let surface_copy_to_present_supported =
        capabilities.usages.contains(wgpu::TextureUsages::COPY_DST);
    let surface_copy_src_readback_supported =
        capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC);
    if surface_copy_to_present_supported {
        config.usage |= wgpu::TextureUsages::COPY_DST;
    }
    if options.readback_enabled() {
        if !surface_copy_src_readback_supported {
            return Err(NativeWindowError::Failed(format!(
                "visible surface readback requires COPY_SRC usage, but supported usages are {:?}",
                capabilities.usages
            )));
        }
        config.usage |= wgpu::TextureUsages::COPY_SRC;
    }
    let surface_format = format!("{:?}", config.format);
    let present_mode = format!("{:?}", config.present_mode);
    let desired_maximum_frame_latency = config.desired_maximum_frame_latency;
    let desired_maximum_frame_latency_source = desired_maximum_frame_latency_source.to_owned();
    let alpha_mode = format!("{:?}", config.alpha_mode);
    surface.configure(&device, &config);
    let warmup_frame_count = options.warmup_frame_count;
    let sample_frame_count = options.sample_frame_count.max(1);
    let total_frame_count = warmup_frame_count.saturating_add(sample_frame_count).max(1);
    let mut external_render_proof = None;
    let mut surface_acquire_ms = 0.0;
    let mut present_submit_ms = 0.0;
    let mut first_presented_frame_ms = 0.0;
    let mut surface_acquire_samples = Vec::new();
    let mut present_submit_samples = Vec::new();
    let mut command_record_samples = Vec::new();
    let mut encoder_finish_samples = Vec::new();
    let mut queue_submit_samples = Vec::new();
    let mut frame_present_samples = Vec::new();
    let mut sample_pacing_wait_samples = Vec::new();
    let mut post_present_bookkeeping_samples = Vec::new();
    let mut presented_frame_samples = Vec::new();
    let mut presented_frame_over_16_7_indices = Vec::new();
    let mut presented_frame_over_16_7_max = 0.0_f64;
    let mut render_hook_samples = Vec::new();
    let mut pending_readback = None;
    let loop_mode = if options.hold_ms == 0 || options.demand_driven_loop {
        NativeRenderLoopMode::DemandDriven
    } else {
        NativeRenderLoopMode::ContinuousProbe
    };
    let mut render_loop_state = NativeRenderLoopState::new(loop_mode);
    let mut surface_lifecycle = NativeSurfaceLifecycleState::new(width, height);
    let mut next_forced_sample_frame_start = Instant::now();

    for frame_index in 0..total_frame_count {
        let current_sample_pacing_wait_ms = wait_for_forced_sample_frame_pacing(
            &mut next_forced_sample_frame_start,
            frame_index > 0,
        );
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
        let content_validation_revision = render_loop_state.content_validation_revision();
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
        let mut rendered_layout_revision = rendered_revision;
        let mut rendered_render_scene_revision = rendered_revision;
        let render_hook_ms = match hooks.as_mut() {
            Some(hooks) => {
                let render_start = Instant::now();
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    render_target_kind: "visible-surface-direct",
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
                    content_validation_revision,
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
                            &NativePreviewPerfAccumulator::default(),
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                input_event_wake_count.load(Ordering::Relaxed),
                                present_mode.as_str(),
                                surface_format.as_str(),
                                desired_maximum_frame_latency,
                                desired_maximum_frame_latency_source.as_str(),
                                None,
                                &app_surface,
                                None,
                                options.proof_mode,
                            ),
                            Some(error.as_str()),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook: {error}"
                    )));
                }
                let presented_revisions = render_result.presented_revisions(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
                rendered_content_revision = presented_revisions.0;
                rendered_layout_revision = presented_revisions.1;
                rendered_render_scene_revision = presented_revisions.2;
                external_render_proof = render_result.proof;
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
        let pending_frame_evidence_key = frame_evidence_key_for_next_presented_frame_with_revisions(
            &render_loop_state,
            &surface_id,
            surface_lifecycle.epoch(),
            None,
            None,
            rendered_content_revision,
            rendered_layout_revision,
            rendered_render_scene_revision,
        );
        render_loop_state.note_preissued_frame_evidence_key(
            pending_frame_evidence_key.clone(),
            elapsed_ms(present_start),
        );
        let readback_sample_frame =
            frame_index + 1 == total_frame_count && options.readback_enabled();
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
                Some(pending_frame_evidence_key.clone()),
            )?);
        }
        let current_command_record_ms = elapsed_ms(present_start);
        let encoder_finish_start = Instant::now();
        let command_buffer = encoder.finish();
        let current_encoder_finish_ms = elapsed_ms(encoder_finish_start);
        let queue_submit_start = Instant::now();
        queue.submit(Some(command_buffer));
        let current_queue_submit_ms = elapsed_ms(queue_submit_start);
        let frame_present_start = Instant::now();
        frame.present();
        let current_frame_present_ms = elapsed_ms(frame_present_start);
        let post_present_bookkeeping_start = Instant::now();
        render_loop_state.mark_presented_with_revisions(
            rendered_revision,
            rendered_content_revision,
            rendered_layout_revision,
            rendered_render_scene_revision,
        );
        debug_assert_eq!(
            frame_evidence_key_for_presented_frame(
                &render_loop_state,
                &surface_id,
                surface_lifecycle.epoch(),
                None,
                None,
            ),
            pending_frame_evidence_key
        );
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
                &NativePreviewPerfAccumulator::default(),
                render_loop_report_extras(
                    resize_wake_count.load(Ordering::Relaxed),
                    input_event_wake_count.load(Ordering::Relaxed),
                    present_mode.as_str(),
                    surface_format.as_str(),
                    desired_maximum_frame_latency,
                    desired_maximum_frame_latency_source.as_str(),
                    None,
                    &app_surface,
                    None,
                    options.proof_mode,
                ),
                None,
            )?;
        }
        let current_post_present_bookkeeping_ms = elapsed_ms(post_present_bookkeeping_start);
        let current_present_submit_ms = elapsed_ms(present_start);
        let frame_ms = current_surface_acquire_ms + current_present_submit_ms;
        if frame_index == 0 {
            surface_acquire_ms = current_surface_acquire_ms;
            present_submit_ms = current_present_submit_ms;
            first_presented_frame_ms = frame_ms;
        }
        let include_timing_sample =
            frame_index >= warmup_frame_count && !(readback_sample_frame && sample_frame_count > 1);
        if include_timing_sample {
            surface_acquire_samples.push(current_surface_acquire_ms);
            present_submit_samples.push(current_present_submit_ms);
            command_record_samples.push(current_command_record_ms);
            encoder_finish_samples.push(current_encoder_finish_ms);
            queue_submit_samples.push(current_queue_submit_ms);
            frame_present_samples.push(current_frame_present_ms);
            sample_pacing_wait_samples.push(current_sample_pacing_wait_ms);
            post_present_bookkeeping_samples.push(current_post_present_bookkeeping_ms);
            let sample_index = presented_frame_samples.len() as u32;
            presented_frame_samples.push(frame_ms);
            if frame_ms > 16.7 {
                presented_frame_over_16_7_indices.push(sample_index);
                presented_frame_over_16_7_max = presented_frame_over_16_7_max.max(frame_ms);
            }
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
        surface_acquire_ms_p50: percentile(&surface_acquire_samples, 0.50),
        surface_acquire_ms_p95: percentile(&surface_acquire_samples, 0.95),
        surface_acquire_ms_max: surface_acquire_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        present_submit_ms_p50: percentile(&present_submit_samples, 0.50),
        present_submit_ms_p95: percentile(&present_submit_samples, 0.95),
        present_submit_ms_max: present_submit_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        command_record_ms_p50: percentile(&command_record_samples, 0.50),
        command_record_ms_p95: percentile(&command_record_samples, 0.95),
        command_record_ms_max: command_record_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        encoder_finish_ms_p50: percentile(&encoder_finish_samples, 0.50),
        encoder_finish_ms_p95: percentile(&encoder_finish_samples, 0.95),
        encoder_finish_ms_max: encoder_finish_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        queue_submit_ms_p50: percentile(&queue_submit_samples, 0.50),
        queue_submit_ms_p95: percentile(&queue_submit_samples, 0.95),
        queue_submit_ms_max: queue_submit_samples.iter().copied().fold(0.0_f64, f64::max),
        frame_present_ms_p50: percentile(&frame_present_samples, 0.50),
        frame_present_ms_p95: percentile(&frame_present_samples, 0.95),
        frame_present_ms_max: frame_present_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        sample_pacing_wait_ms_p50: percentile(&sample_pacing_wait_samples, 0.50),
        sample_pacing_wait_ms_p95: percentile(&sample_pacing_wait_samples, 0.95),
        sample_pacing_wait_ms_max: sample_pacing_wait_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        post_present_bookkeeping_ms_p50: percentile(&post_present_bookkeeping_samples, 0.50),
        post_present_bookkeeping_ms_p95: percentile(&post_present_bookkeeping_samples, 0.95),
        post_present_bookkeeping_ms_max: post_present_bookkeeping_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        presented_frame_ms_p50: percentile(&presented_frame_samples, 0.50),
        presented_frame_ms_p95: percentile(&presented_frame_samples, 0.95),
        presented_frame_ms_p99: percentile(&presented_frame_samples, 0.99),
        presented_frame_ms_max: presented_frame_samples
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        presented_frame_ms_over_16_7_count: presented_frame_over_16_7_indices.len() as u32,
        presented_frame_ms_over_16_7_indices: presented_frame_over_16_7_indices,
        presented_frame_ms_over_16_7_max: presented_frame_over_16_7_max,
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
    let mut input_adapter = if options.sample_input_after_initial_frames {
        if options.input_sample_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(options.input_sample_delay_ms));
        }
        if options.synthetic_input_probe {
            inject_synthetic_input_probe(&mut mouse, &keyboard, &window_id, width, height);
        }
        sample_input_adapter(&mut mouse, &keyboard, options.synthetic_input_probe)
    } else {
        let mut input_adapter = empty_input_adapter_proof(options.synthetic_input_probe);
        input_adapter.real_os_events_observed = input_event_wake_count.load(Ordering::Relaxed) > 0;
        input_adapter.sampled_after_visible_window = false;
        input_adapter.input_injection_method = "none-no-input-floor-observation".to_owned();
        input_adapter
    };
    let mut post_input_frame_timing = None;
    if input_adapter.real_os_events_observed && hooks.is_some() {
        let post_input_warmup_frame_count = warmup_frame_count;
        let post_input_sample_count = sample_frame_count.max(1);
        let post_input_total_frame_count = post_input_warmup_frame_count
            .saturating_add(post_input_sample_count)
            .max(1);
        let mut post_input_surface_acquire_samples = Vec::new();
        let mut post_input_present_submit_samples = Vec::new();
        let mut post_input_command_record_samples = Vec::new();
        let mut post_input_encoder_finish_samples = Vec::new();
        let mut post_input_queue_submit_samples = Vec::new();
        let mut post_input_frame_present_samples = Vec::new();
        let mut post_input_sample_pacing_wait_samples = Vec::new();
        let mut post_input_post_present_bookkeeping_samples = Vec::new();
        let mut post_input_presented_frame_samples = Vec::new();
        let mut post_input_presented_frame_over_16_7_indices = Vec::new();
        let mut post_input_presented_frame_over_16_7_max = 0.0_f64;
        let mut post_input_render_hook_samples = Vec::new();
        let mut post_input_first_frame_ms = 0.0;
        let mut post_input_readback = None;
        let mut post_input_next_forced_sample_frame_start = Instant::now();
        for frame_index in 0..post_input_total_frame_count {
            let current_sample_pacing_wait_ms = wait_for_forced_sample_frame_pacing(
                &mut post_input_next_forced_sample_frame_start,
                frame_index > 0,
            );
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
            let content_validation_revision = render_loop_state.content_validation_revision();
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
            let mut rendered_layout_revision = rendered_revision;
            let mut rendered_render_scene_revision = rendered_revision;
            let mut post_input_render_hook_ms = None;
            if let Some(hooks) = hooks.as_mut() {
                let render_start = Instant::now();
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &view,
                    surface_texture_format: config.format,
                    render_target_kind: "visible-surface-direct",
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
                    content_validation_revision,
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
                            &NativePreviewPerfAccumulator::default(),
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                input_event_wake_count.load(Ordering::Relaxed),
                                present_mode.as_str(),
                                surface_format.as_str(),
                                desired_maximum_frame_latency,
                                desired_maximum_frame_latency_source.as_str(),
                                None,
                                &app_surface,
                                Some(&input_adapter),
                                options.proof_mode,
                            ),
                            Some(error.as_str()),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook after input: {error}"
                    )));
                }
                let presented_revisions = render_result.presented_revisions(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
                rendered_content_revision = presented_revisions.0;
                rendered_layout_revision = presented_revisions.1;
                rendered_render_scene_revision = presented_revisions.2;
                external_render_proof = render_result.proof;
                post_input_render_hook_ms = Some(elapsed_ms(render_start));
            }
            let pending_frame_evidence_key =
                frame_evidence_key_for_next_presented_frame_with_revisions(
                    &render_loop_state,
                    &surface_id,
                    surface_lifecycle.epoch(),
                    (render_loop_state.last_accepted_host_input_event_wake_count > 0)
                        .then_some(render_loop_state.last_accepted_host_input_event_wake_count),
                    None,
                    rendered_content_revision,
                    rendered_layout_revision,
                    rendered_render_scene_revision,
                );
            render_loop_state.note_preissued_frame_evidence_key(
                pending_frame_evidence_key.clone(),
                elapsed_ms(present_start),
            );
            let readback_sample_frame =
                frame_index + 1 == post_input_total_frame_count && options.readback_enabled();
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
                    Some(pending_frame_evidence_key.clone()),
                )?);
            }
            let current_command_record_ms = elapsed_ms(present_start);
            let encoder_finish_start = Instant::now();
            let command_buffer = encoder.finish();
            let current_encoder_finish_ms = elapsed_ms(encoder_finish_start);
            let queue_submit_start = Instant::now();
            queue.submit(Some(command_buffer));
            let current_queue_submit_ms = elapsed_ms(queue_submit_start);
            let frame_present_start = Instant::now();
            frame.present();
            let current_frame_present_ms = elapsed_ms(frame_present_start);
            let post_present_bookkeeping_start = Instant::now();
            render_loop_state.mark_presented_with_revisions(
                rendered_revision,
                rendered_content_revision,
                rendered_layout_revision,
                rendered_render_scene_revision,
            );
            debug_assert_eq!(
                frame_evidence_key_for_presented_frame(
                    &render_loop_state,
                    &surface_id,
                    surface_lifecycle.epoch(),
                    (render_loop_state.last_accepted_host_input_event_wake_count > 0)
                        .then_some(render_loop_state.last_accepted_host_input_event_wake_count),
                    None,
                ),
                pending_frame_evidence_key
            );
            let current_post_present_bookkeeping_ms = elapsed_ms(post_present_bookkeeping_start);
            let current_present_submit_ms = elapsed_ms(present_start);
            let frame_ms = current_surface_acquire_ms + current_present_submit_ms;
            if frame_index == 0 {
                post_input_first_frame_ms = frame_ms;
            }
            let include_timing_sample = frame_index >= post_input_warmup_frame_count
                && !(readback_sample_frame && post_input_sample_count > 1);
            if include_timing_sample {
                post_input_surface_acquire_samples.push(current_surface_acquire_ms);
                post_input_present_submit_samples.push(current_present_submit_ms);
                post_input_command_record_samples.push(current_command_record_ms);
                post_input_encoder_finish_samples.push(current_encoder_finish_ms);
                post_input_queue_submit_samples.push(current_queue_submit_ms);
                post_input_frame_present_samples.push(current_frame_present_ms);
                post_input_sample_pacing_wait_samples.push(current_sample_pacing_wait_ms);
                post_input_post_present_bookkeeping_samples
                    .push(current_post_present_bookkeeping_ms);
                let sample_index = post_input_presented_frame_samples.len() as u32;
                post_input_presented_frame_samples.push(frame_ms);
                if frame_ms > 16.7 {
                    post_input_presented_frame_over_16_7_indices.push(sample_index);
                    post_input_presented_frame_over_16_7_max =
                        post_input_presented_frame_over_16_7_max.max(frame_ms);
                }
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
            surface_acquire_ms_p50: percentile(&post_input_surface_acquire_samples, 0.50),
            surface_acquire_ms_p95: percentile(&post_input_surface_acquire_samples, 0.95),
            surface_acquire_ms_max: post_input_surface_acquire_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            present_submit_ms_p50: percentile(&post_input_present_submit_samples, 0.50),
            present_submit_ms_p95: percentile(&post_input_present_submit_samples, 0.95),
            present_submit_ms_max: post_input_present_submit_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            command_record_ms_p50: percentile(&post_input_command_record_samples, 0.50),
            command_record_ms_p95: percentile(&post_input_command_record_samples, 0.95),
            command_record_ms_max: post_input_command_record_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            encoder_finish_ms_p50: percentile(&post_input_encoder_finish_samples, 0.50),
            encoder_finish_ms_p95: percentile(&post_input_encoder_finish_samples, 0.95),
            encoder_finish_ms_max: post_input_encoder_finish_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            queue_submit_ms_p50: percentile(&post_input_queue_submit_samples, 0.50),
            queue_submit_ms_p95: percentile(&post_input_queue_submit_samples, 0.95),
            queue_submit_ms_max: post_input_queue_submit_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            frame_present_ms_p50: percentile(&post_input_frame_present_samples, 0.50),
            frame_present_ms_p95: percentile(&post_input_frame_present_samples, 0.95),
            frame_present_ms_max: post_input_frame_present_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            sample_pacing_wait_ms_p50: percentile(&post_input_sample_pacing_wait_samples, 0.50),
            sample_pacing_wait_ms_p95: percentile(&post_input_sample_pacing_wait_samples, 0.95),
            sample_pacing_wait_ms_max: post_input_sample_pacing_wait_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            post_present_bookkeeping_ms_p50: percentile(
                &post_input_post_present_bookkeeping_samples,
                0.50,
            ),
            post_present_bookkeeping_ms_p95: percentile(
                &post_input_post_present_bookkeeping_samples,
                0.95,
            ),
            post_present_bookkeeping_ms_max: post_input_post_present_bookkeeping_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            presented_frame_ms_p50: percentile(&post_input_presented_frame_samples, 0.50),
            presented_frame_ms_p95: percentile(&post_input_presented_frame_samples, 0.95),
            presented_frame_ms_p99: percentile(&post_input_presented_frame_samples, 0.99),
            presented_frame_ms_max: post_input_presented_frame_samples
                .iter()
                .copied()
                .fold(0.0_f64, f64::max),
            presented_frame_ms_over_16_7_count: post_input_presented_frame_over_16_7_indices.len()
                as u32,
            presented_frame_ms_over_16_7_indices: post_input_presented_frame_over_16_7_indices,
            presented_frame_ms_over_16_7_max: post_input_presented_frame_over_16_7_max,
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

    let proof_frame_evidence_key = (render_loop_state.rendered_frame_count > 0).then(|| {
        frame_evidence_key_for_presented_frame(
            &render_loop_state,
            &surface_id,
            surface_lifecycle.epoch(),
            (render_loop_state.last_accepted_host_input_event_wake_count > 0)
                .then_some(render_loop_state.last_accepted_host_input_event_wake_count),
            None,
        )
    });
    if let Some(artifact) = readback_artifact.as_mut() {
        artifact.presented_revision = Some(render_loop_state.presented_revision);
        artifact.content_revision = Some(render_loop_state.last_render_content_revision);
        artifact.rendered_frame_count = Some(render_loop_state.rendered_frame_count);
        artifact.frame_evidence_key = proof_frame_evidence_key.clone();
    }
    let surface_external_render_proof = external_render_proof_with_frame_evidence_key(
        external_render_proof.as_ref(),
        proof_frame_evidence_key.as_ref(),
    );

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
        frame_evidence_key: proof_frame_evidence_key,
        wgpu_strategy: format!("{:?}", app_window::WGPU_STRATEGY),
        wgpu_surface_strategy: format!("{:?}", app_window::WGPU_SURFACE_STRATEGY),
        adapter_name: adapter_info.name,
        adapter_backend: format!("{:?}", adapter_info.backend),
        adapter_device: adapter_info.device,
        adapter_vendor: adapter_info.vendor,
        adapter_device_type: format!("{:?}", adapter_info.device_type),
        adapter_is_software: matches!(adapter_info.device_type, wgpu::DeviceType::Cpu),
        surface_format: surface_format.clone(),
        present_mode: present_mode.clone(),
        supported_present_modes,
        non_vsync_present_mode_available,
        desired_maximum_frame_latency,
        desired_maximum_frame_latency_source: desired_maximum_frame_latency_source.clone(),
        alpha_mode,
        supported_alpha_modes,
        supported_usages,
        surface_copy_to_present_supported,
        surface_copy_src_readback_supported,
        present_path_mode: render_loop_state.last_present_path_mode,
        present_path_requested_mode: render_loop_state.last_present_path_requested_mode,
        present_path_selection_reason: render_loop_state.last_present_path_selection_reason.clone(),
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
        input_event_wake_count: input_event_wake_count.load(Ordering::Relaxed),
        app_window_surface_content_report: app_window_surface_content_report(&app_surface),
        input_sample_delay_ms: options.input_sample_delay_ms,
        sample_input_after_initial_frames: options.sample_input_after_initial_frames,
        frame_timing,
        post_input_frame_timing,
        input_adapter,
        external_render_proof: surface_external_render_proof,
        readback_artifact,
    };
    let _ = ready_sender.send(Ok(proof));
    let mut external_render_proof = external_render_proof.map(Arc::new);
    let hold_started = Instant::now();
    let mut input_cursor = NativeInputCursor::default();
    let mut last_wake_generation = 0;
    let mut last_interactive_readback_artifact: Option<AppWindowReadbackArtifact> = None;
    let mut last_interactive_readback_finish_ms: Option<f64> = None;
    let mut last_interactive_readback_completed_elapsed_ms: Option<f64> = None;
    let mut last_interactive_readback_error: Option<String> = None;
    let mut recent_interactive_readback_artifacts: VecDeque<AppWindowReadbackArtifact> =
        VecDeque::new();
    let mut last_interactive_surface_readback_queued = false;
    let mut last_interactive_surface_readback_skipped_for_external_proof = false;
    let mut last_interactive_surface_readback_skipped_for_stale_input = false;
    let mut last_interactive_surface_readback_skipped_for_backpressure = false;
    let mut last_interactive_surface_readback_pending = false;
    let mut interactive_readback_job: Option<AsyncInteractiveReadbackJob> = None;
    let mut last_render_loop_report_write_ms: Option<f64> = None;
    let mut last_render_loop_report_enqueue_ms: Option<f64> = None;
    let mut last_frame_evidence_key: Option<FrameEvidenceKey> = None;
    let mut recent_frame_evidence: VecDeque<serde_json::Value> = VecDeque::new();
    let mut recent_product_frame_commits: VecDeque<NativeProductFrameCommit> = VecDeque::new();
    let mut deferred_artifact_hash_readback_keys: VecDeque<FrameEvidenceKey> = VecDeque::new();
    let mut preview_perf_accumulator = NativePreviewPerfAccumulator::default();
    let mut offscreen_present_target: Option<NativeOffscreenPresentTarget> = None;
    let requested_present_path_mode = requested_present_path_mode_from_env();
    let async_render_loop_report_writer = options
        .render_loop_state_report
        .as_ref()
        .map(|_| AsyncRenderLoopReportWriter::new());
    let mut async_post_present_proof_worker = AsyncPostPresentProofSubscriberWorker::new();
    let mut last_sampled_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
    let mut last_presented_input_event_wake_count = last_sampled_input_event_wake_count;
    let mut consecutive_unsampled_input_resamples = 0_u8;
    loop {
        let pre_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
        let loop_start_elapsed_ms = hold_started.elapsed().as_secs_f64() * 1000.0;
        let pre_input_subscriber_drain_allowed =
            pre_input_event_wake_count <= last_sampled_input_event_wake_count;
        if pre_input_subscriber_drain_allowed {
            let pre_input_proof_drain_limit =
                if render_loop_state.interaction_burst_active(loop_start_elapsed_ms) {
                    PRE_INPUT_COMPLETED_PROOF_DRAIN_LIMIT
                } else {
                    usize::MAX
                };
            let _ = async_post_present_proof_worker
                .drain_completed_limit(&mut render_loop_state, pre_input_proof_drain_limit);
            if let Some(result) = poll_interactive_readback_job(&mut interactive_readback_job) {
                match result {
                    Ok(result) => {
                        last_interactive_readback_finish_ms = Some(result.finish_ms);
                        last_interactive_readback_completed_elapsed_ms =
                            Some(result.completed_elapsed_ms);
                        note_completed_interactive_readback_artifact(
                            &mut render_loop_state,
                            &mut recent_interactive_readback_artifacts,
                            &mut last_interactive_readback_artifact,
                            &mut deferred_artifact_hash_readback_keys,
                            result.artifact,
                            Some(result.completed_elapsed_ms),
                        );
                        last_interactive_surface_readback_pending = false;
                        last_interactive_readback_error = None;
                    }
                    Err(error) => {
                        last_interactive_readback_finish_ms = None;
                        last_interactive_readback_completed_elapsed_ms =
                            Some(hold_started.elapsed().as_secs_f64() * 1000.0);
                        last_interactive_surface_readback_pending = false;
                        last_interactive_readback_error = Some(error);
                        render_loop_state.telemetry_drop_count =
                            render_loop_state.telemetry_drop_count.saturating_add(1);
                        render_loop_state.last_missed_frame_cause =
                            Some("interactive_readback_error".to_owned());
                    }
                }
                if let (Some(report), Some(report_writer)) = (
                    options.render_loop_state_report.as_deref(),
                    async_render_loop_report_writer.as_ref(),
                ) {
                    if input_event_wake_count.load(Ordering::Relaxed) > pre_input_event_wake_count {
                        render_loop_state
                            .note_pre_input_subscriber_drain_skipped("pending_host_input");
                        render_loop_state.last_scheduler_reason =
                            Some(NativeSchedulerReason::HostInput);
                        render_loop_state.scheduled_wake_count =
                            render_loop_state.scheduled_wake_count.saturating_add(1);
                        continue;
                    }
                    let report_enqueue_started = Instant::now();
                    let report_writer_stats = report_writer.stats();
                    last_render_loop_report_write_ms = report_writer_stats.last_write_ms;
                    let snapshot = render_loop_report_snapshot(
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
                        &preview_perf_accumulator,
                        render_loop_report_extras(
                            resize_wake_count.load(Ordering::Relaxed),
                            input_event_wake_count.load(Ordering::Relaxed),
                            present_mode.as_str(),
                            surface_format.as_str(),
                            desired_maximum_frame_latency,
                            desired_maximum_frame_latency_source.as_str(),
                            input_event_last_wake_elapsed_ms(
                                &input_event_last_wake_at,
                                hold_started,
                            ),
                            &app_surface,
                            Some(&observed_input_adapter),
                            options.proof_mode,
                        )
                        .with_input_generation(
                            last_sampled_input_event_wake_count,
                            last_presented_input_event_wake_count,
                            input_event_wake_elapsed_ms_for_generation(
                                &input_event_wake_timeline,
                                last_sampled_input_event_wake_count,
                                hold_started,
                            ),
                            input_event_wake_elapsed_ms_for_generation(
                                &input_event_wake_timeline,
                                last_presented_input_event_wake_count,
                                hold_started,
                            ),
                        )
                        .with_interactive_timing(
                            report_writer_stats.last_write_ms,
                            last_interactive_readback_finish_ms,
                            last_interactive_readback_completed_elapsed_ms,
                            last_interactive_surface_readback_queued,
                            last_interactive_surface_readback_skipped_for_external_proof,
                            last_interactive_surface_readback_skipped_for_stale_input,
                            last_interactive_surface_readback_skipped_for_backpressure,
                            last_interactive_surface_readback_pending,
                            last_interactive_readback_error.clone(),
                        )
                        .with_external_render_proof(external_render_proof.as_ref())
                        .with_frame_evidence_key(last_frame_evidence_key.as_ref())
                        .with_recent_frame_evidence(&recent_frame_evidence)
                        .with_recent_product_frame_commits(&recent_product_frame_commits)
                        .with_recent_interactive_readback_artifacts(
                            &recent_interactive_readback_artifacts,
                        )
                        .with_report_writer_stats(Some(report_writer_stats)),
                        None,
                    );
                    last_render_loop_report_enqueue_ms =
                        Some(report_writer.enqueue(snapshot, report_enqueue_started));
                }
            }
        } else if interactive_readback_job.is_some() {
            let skip_reason = if render_loop_state.interaction_burst_active(loop_start_elapsed_ms) {
                "interaction_burst"
            } else {
                "pending_host_input"
            };
            render_loop_state.note_pre_input_subscriber_drain_skipped(skip_reason);
        }
        if options.hold_ms > 0 && hold_started.elapsed() >= Duration::from_millis(options.hold_ms) {
            render_loop_state.note_loop_exit("hold_timeout_elapsed");
            break;
        }
        if let Some(reason) = native_window_exit_reason(&mut hooks) {
            render_loop_state.note_loop_exit(reason);
            break;
        }
        let size_scale_started = Instant::now();
        let current_size = latest_surface_size
            .lock()
            .map(|latest| *latest)
            .unwrap_or(size);
        let current_scale = cached_surface_scale;
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
        let poll_started_elapsed_ms = hold_started.elapsed().as_secs_f64() * 1000.0;
        render_loop_state.note_poll_started(poll_started_elapsed_ms);
        render_loop_state.consume_due_wake(poll_started_at);
        render_loop_state.clear_requested_animation_burst_if_quiet(
            hold_started.elapsed().as_secs_f64() * 1000.0,
        );
        let input_sample_started = Instant::now();
        let mut sampled_input_event_wake_count_before =
            input_event_wake_count.load(Ordering::Relaxed);
        let mut input =
            sample_input_adapter_delta(&mut mouse, &keyboard, &input_cursor, false, hold_started);
        let mut sampled_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
        for _ in 0..2 {
            if sampled_input_event_wake_count <= sampled_input_event_wake_count_before {
                break;
            }
            let resample_gap =
                sampled_input_event_wake_count - sampled_input_event_wake_count_before;
            render_loop_state.note_input_inline_resample(resample_gap);
            sampled_input_event_wake_count_before = sampled_input_event_wake_count;
            input = sample_input_adapter_delta(
                &mut mouse,
                &keyboard,
                &input_cursor,
                false,
                hold_started,
            );
            sampled_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
        }
        let input_sample_elapsed = input_sample_started.elapsed();
        render_loop_state
            .note_armed_frame_input_sampled(hold_started.elapsed().as_secs_f64() * 1000.0);
        last_sampled_input_event_wake_count = sampled_input_event_wake_count;
        let sampled_delta_has_real_os_events = input.real_os_events_observed;
        let raw_host_input_wake_pending =
            sampled_input_event_wake_count > last_presented_input_event_wake_count;
        let host_input_observed_this_turn =
            input.real_os_events_observed || raw_host_input_wake_pending;
        let reportable_host_input_this_turn =
            host_input_observed_this_turn && native_input_delta_has_reportable_host_event(&input);
        let accessibility_started = Instant::now();
        let accessibility_actions = native_accessibility_action_requests_from_accesskit(
            app_surface.take_accessibility_action_requests(),
        );
        let accessibility_elapsed = accessibility_started.elapsed();
        merge_input_adapter_proof(&mut observed_input_adapter, &input);
        render_loop_state.note_input_poll();
        let hook_started = Instant::now();
        let mut poll_result = None;
        let mut dirty_poll_result_this_turn = false;
        if host_input_observed_this_turn
            && let Some(input_poll_result) = poll_native_window_input_hooks(
                &mut hooks,
                NativePollContext {
                    window_id: window_id.clone(),
                    surface_id: surface_id.clone(),
                    surface_epoch: surface_lifecycle.epoch(),
                    width,
                    height,
                    scale: current_scale as f32,
                    input_delta: input.clone(),
                    accessibility_actions: accessibility_actions.clone(),
                    now: poll_started_at,
                    forced_frame: false,
                },
            )?
        {
            let effective_input_poll_result = effective_poll_result_for_host_input(
                input_poll_result,
                reportable_host_input_this_turn,
            );
            if effective_input_poll_result.dirty || sampled_delta_has_real_os_events {
                dirty_poll_result_this_turn = effective_input_poll_result.dirty;
                poll_result = Some(effective_input_poll_result);
            }
        }
        if poll_result.is_none() {
            poll_result = poll_native_window_hooks(
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
        }
        let hook_elapsed = hook_started.elapsed();
        let role_poll_completed_elapsed_ms = hold_started.elapsed().as_secs_f64() * 1000.0;
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
            let effective_poll_result = effective_poll_result_for_host_input(
                poll_result.clone(),
                reportable_host_input_this_turn,
            );
            render_loop_state
                .apply_poll_result(&effective_poll_result, host_input_observed_this_turn);
            dirty_poll_result_this_turn |= effective_poll_result.dirty;
            if reportable_host_input_this_turn
                && effective_poll_result.dirty
                && native_input_delta_is_pointer_motion_only(&input)
            {
                render_loop_state.request_interactive_prewarm_burst(
                    poll_started_at,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                );
            } else if reportable_host_input_this_turn && effective_poll_result.dirty {
                render_loop_state.request_animation_burst(
                    poll_started_at,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                    NativeSchedulerReason::HostInput,
                );
            } else if native_input_delta_is_pointer_motion_only(&input) {
                render_loop_state.request_interactive_prewarm_burst(
                    poll_started_at,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                );
            } else if effective_poll_result.wants_animation_frame {
                render_loop_state.request_animation_burst(
                    poll_started_at,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                    NativeSchedulerReason::RequestedAnimation,
                );
            }
            if effective_poll_result.dirty {
                render_loop_state.note_dirty_poll(role_poll_completed_elapsed_ms);
            }
            if reportable_host_input_this_turn && effective_poll_result.dirty {
                let sampled_input_wake_elapsed_ms = input_event_wake_elapsed_ms_for_generation(
                    &input_event_wake_timeline,
                    sampled_input_event_wake_count,
                    hold_started,
                );
                render_loop_state.note_accepted_host_input(
                    sampled_input_event_wake_count,
                    role_poll_completed_elapsed_ms,
                    native_input_delta_is_button_press_only(&input),
                    Some(accepted_host_input_event_summary(
                        &input,
                        sampled_input_event_wake_count,
                        sampled_input_wake_elapsed_ms,
                        role_poll_completed_elapsed_ms,
                        effective_poll_result
                            .accepted_host_input_event_hint
                            .as_ref(),
                    )),
                );
            }
            if should_accept_input_cursor_after_poll(
                host_input_observed_this_turn,
                &input,
                &effective_poll_result,
            ) {
                accept_input_cursor(&mut mouse, &mut input_cursor, &input);
            }
        } else if native_input_delta_is_pointer_motion_only(&input) {
            render_loop_state.request_interactive_prewarm_burst(
                poll_started_at,
                hold_started.elapsed().as_secs_f64() * 1000.0,
            );
            accept_input_cursor(&mut mouse, &mut input_cursor, &input);
        } else if reportable_host_input_this_turn
            && !native_input_delta_is_button_press_only(&input)
        {
            let app_window_accept_elapsed_ms = hold_started.elapsed().as_secs_f64() * 1000.0;
            dirty_poll_result_this_turn = true;
            render_loop_state.mark_dirty(NativeSchedulerReason::HostInput, None);
            render_loop_state.request_animation_burst(
                poll_started_at,
                app_window_accept_elapsed_ms,
                NativeSchedulerReason::HostInput,
            );
            render_loop_state.note_dirty_poll(app_window_accept_elapsed_ms);
            let sampled_input_wake_elapsed_ms = input_event_wake_elapsed_ms_for_generation(
                &input_event_wake_timeline,
                sampled_input_event_wake_count,
                hold_started,
            );
            render_loop_state.note_accepted_host_input(
                sampled_input_event_wake_count,
                app_window_accept_elapsed_ms,
                false,
                Some(accepted_host_input_event_summary(
                    &input,
                    sampled_input_event_wake_count,
                    sampled_input_wake_elapsed_ms,
                    app_window_accept_elapsed_ms,
                    None,
                )),
            );
        }
        if !dirty_poll_result_this_turn {
            render_loop_state.consume_due_wake_after_poll(Instant::now());
        }
        let wake_generation = wake_handle.generation();
        let wake_generation_changed = wake_generation != last_wake_generation;
        if wake_generation_changed {
            last_wake_generation = wake_generation;
            render_loop_state.note_external_wake_observed(
                wake_generation,
                hold_started.elapsed().as_secs_f64() * 1000.0,
            );
            if !render_loop_state.should_render(Instant::now(), false) {
                render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::ExternalWake);
                render_loop_state.scheduled_wake_count =
                    render_loop_state.scheduled_wake_count.saturating_add(1);
                continue;
            }
        }
        let current_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
        let unsampled_input_wake_count =
            current_input_event_wake_count > sampled_input_event_wake_count;
        let render_due_after_poll = render_loop_state.should_render(Instant::now(), false);
        if unsampled_input_wake_count
            && render_due_after_poll
            && native_input_delta_is_pointer_motion_only(&input)
            && consecutive_unsampled_input_resamples < MAX_CONSECUTIVE_UNSAMPLED_INPUT_RESAMPLES
        {
            render_loop_state.note_input_deferred_resample(
                current_input_event_wake_count.saturating_sub(sampled_input_event_wake_count),
            );
            consecutive_unsampled_input_resamples =
                consecutive_unsampled_input_resamples.saturating_add(1);
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            continue;
        }
        if unsampled_input_wake_count
            && !render_due_after_poll
            && consecutive_unsampled_input_resamples < MAX_CONSECUTIVE_UNSAMPLED_INPUT_RESAMPLES
        {
            render_loop_state.note_input_deferred_resample(
                current_input_event_wake_count.saturating_sub(sampled_input_event_wake_count),
            );
            consecutive_unsampled_input_resamples =
                consecutive_unsampled_input_resamples.saturating_add(1);
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            if hooks.as_ref().is_none_or(|hooks| hooks.poll.is_none()) {
                accept_input_cursor(&mut mouse, &mut input_cursor, &input);
            }
            continue;
        }
        if !unsampled_input_wake_count {
            consecutive_unsampled_input_resamples = 0;
        }
        if render_loop_state.clean_armed_poll_pending()
            && !render_loop_state.should_render(Instant::now(), false)
        {
            render_loop_state.skip_clean_armed_burst_present(
                Instant::now(),
                hold_started.elapsed().as_secs_f64() * 1000.0,
            );
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
        if should_defer_render_for_interactive_readback(
            options.readback_enabled(),
            interactive_readback_job.is_some(),
            host_input_observed_this_turn,
            render_loop_state.current_scheduler_reason,
        ) {
            let bookkeeping_elapsed = bookkeeping_started.elapsed();
            render_loop_state.note_idle_poll_substeps(
                size_scale_elapsed,
                input_sample_elapsed,
                accessibility_elapsed,
                hook_elapsed,
                bookkeeping_elapsed,
            );
            render_loop_state.note_idle_poll();
            let wait_started = Instant::now();
            let completed_generation =
                wake_handle.wait_for_wake_after(last_wake_generation, PASSIVE_INPUT_POLL_INTERVAL);
            render_loop_state.note_idle_wait(
                PASSIVE_INPUT_POLL_INTERVAL,
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
        let content_validation_revision = render_loop_state.content_validation_revision();
        render_loop_state.note_render_started(hold_started.elapsed().as_secs_f64() * 1000.0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-app-window-interactive-encoder"),
        });
        let mut rendered_content_revision = rendered_revision;
        let mut rendered_layout_revision = rendered_revision;
        let mut rendered_render_scene_revision = rendered_revision;
        let readback_enabled = options.readback_enabled();
        let present_path_selection = select_native_present_path_mode(
            hooks.is_some(),
            surface_copy_to_present_supported,
            requested_present_path_mode,
            readback_enabled,
        );
        let use_offscreen_copy_to_present = present_path_selection.selected_mode
            == NativePresentPathMode::AppOwnedOffscreenCopyToPresent;
        let render_target_kind = present_path_selection.selected_mode.render_target_kind();
        render_loop_state.note_present_path_selection(present_path_selection);
        render_loop_state.note_render_target_kind(render_target_kind);
        let mut deferred_app_owned_readback_texture: Option<wgpu::Texture> = None;
        let mut post_present_proof_subscribers: Vec<NativePostPresentProofSubscriber> = Vec::new();

        let frame = if use_offscreen_copy_to_present {
            let offscreen_texture = cached_offscreen_present_target(
                &mut offscreen_present_target,
                &device,
                width,
                height,
                config.format,
            );
            deferred_app_owned_readback_texture = Some(offscreen_texture.clone());
            let offscreen_view =
                offscreen_texture.create_view(&wgpu::TextureViewDescriptor::default());
            if let Some(hooks) = hooks.as_mut() {
                let render_result = (hooks.render)(NativeRenderFrameContext {
                    device: &device,
                    queue: &queue,
                    encoder: &mut encoder,
                    surface_view: &offscreen_view,
                    surface_texture_format: config.format,
                    render_target_kind,
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
                    content_validation_revision,
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
                            &preview_perf_accumulator,
                            render_loop_report_extras(
                                resize_wake_count.load(Ordering::Relaxed),
                                input_event_wake_count.load(Ordering::Relaxed),
                                present_mode.as_str(),
                                surface_format.as_str(),
                                desired_maximum_frame_latency,
                                desired_maximum_frame_latency_source.as_str(),
                                input_event_last_wake_elapsed_ms(
                                    &input_event_last_wake_at,
                                    hold_started,
                                ),
                                &app_surface,
                                Some(&observed_input_adapter),
                                options.proof_mode,
                            ),
                            Some(error.as_str()),
                        );
                    }
                    return Err(NativeWindowError::Failed(format!(
                        "external render hook: {error}"
                    )));
                }
                let presented_revisions = render_result.presented_revisions(
                    rendered_revision,
                    render_loop_state.current_scheduler_reason,
                    render_loop_state.current_role_dirty_reason,
                );
                rendered_content_revision = presented_revisions.0;
                rendered_layout_revision = presented_revisions.1;
                rendered_render_scene_revision = presented_revisions.2;
                render_loop_state
                    .note_render_frame_metrics(render_result.render_frame_metrics.clone());
                post_present_proof_subscribers = render_result.post_present_proof_subscribers;
                external_render_proof = render_result.proof.map(Arc::new);
                render_loop_state
                    .note_render_hook_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
            }
            let pre_present_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
            if pre_present_input_event_wake_count > sampled_input_event_wake_count
                && !render_loop_state
                    .current_frame_carries_unaccounted_host_input(sampled_input_event_wake_count)
                && consecutive_unsampled_input_resamples < MAX_CONSECUTIVE_UNSAMPLED_INPUT_RESAMPLES
            {
                render_loop_state.note_input_pre_present_resample(
                    pre_present_input_event_wake_count
                        .saturating_sub(sampled_input_event_wake_count),
                );
                consecutive_unsampled_input_resamples =
                    consecutive_unsampled_input_resamples.saturating_add(1);
                render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
                render_loop_state.scheduled_wake_count =
                    render_loop_state.scheduled_wake_count.saturating_add(1);
                continue;
            }
            if pre_present_input_event_wake_count <= sampled_input_event_wake_count {
                consecutive_unsampled_input_resamples = 0;
            }
            let surface_acquire_started = Instant::now();
            let Some(frame) = acquire_surface_texture_for_present(
                &surface,
                &device,
                &config,
                &mut surface_lifecycle,
                &mut render_loop_state,
                "interactive offscreen copy-to-present",
            )?
            else {
                continue;
            };
            render_loop_state.note_surface_acquire_call(elapsed_ms(surface_acquire_started));
            render_loop_state.note_surface_acquired(hold_started.elapsed().as_secs_f64() * 1000.0);
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: offscreen_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            frame
        } else {
            let surface_acquire_started = Instant::now();
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
            render_loop_state.note_surface_acquire_call(elapsed_ms(surface_acquire_started));
            render_loop_state.note_surface_acquired(hold_started.elapsed().as_secs_f64() * 1000.0);
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            match hooks.as_mut() {
                Some(hooks) => {
                    let render_result = (hooks.render)(NativeRenderFrameContext {
                        device: &device,
                        queue: &queue,
                        encoder: &mut encoder,
                        surface_view: &view,
                        surface_texture_format: config.format,
                        render_target_kind,
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
                    if let Err(error) = render_result
                        .validate_for_presented_revision_with_scheduler(
                            content_validation_revision,
                            render_loop_state.current_scheduler_reason,
                            render_loop_state.current_role_dirty_reason,
                        )
                    {
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
                                &preview_perf_accumulator,
                                render_loop_report_extras(
                                    resize_wake_count.load(Ordering::Relaxed),
                                    input_event_wake_count.load(Ordering::Relaxed),
                                    present_mode.as_str(),
                                    surface_format.as_str(),
                                    desired_maximum_frame_latency,
                                    desired_maximum_frame_latency_source.as_str(),
                                    input_event_last_wake_elapsed_ms(
                                        &input_event_last_wake_at,
                                        hold_started,
                                    ),
                                    &app_surface,
                                    Some(&observed_input_adapter),
                                    options.proof_mode,
                                ),
                                Some(error.as_str()),
                            );
                        }
                        return Err(NativeWindowError::Failed(format!(
                            "external render hook: {error}"
                        )));
                    }
                    let presented_revisions = render_result.presented_revisions(
                        rendered_revision,
                        render_loop_state.current_scheduler_reason,
                        render_loop_state.current_role_dirty_reason,
                    );
                    rendered_content_revision = presented_revisions.0;
                    rendered_layout_revision = presented_revisions.1;
                    rendered_render_scene_revision = presented_revisions.2;
                    render_loop_state
                        .note_render_frame_metrics(render_result.render_frame_metrics.clone());
                    post_present_proof_subscribers = render_result.post_present_proof_subscribers;
                    external_render_proof = render_result.proof.map(Arc::new);
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
                    render_loop_state.note_render_frame_metrics(None);
                    render_loop_state
                        .note_render_hook_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
                }
            }
            frame
        };
        let pending_frame_evidence_key = frame_evidence_key_for_next_presented_frame_with_revisions(
            &render_loop_state,
            &surface_id,
            surface_lifecycle.epoch(),
            (sampled_input_event_wake_count > 0).then_some(sampled_input_event_wake_count),
            None,
            rendered_content_revision,
            rendered_layout_revision,
            rendered_render_scene_revision,
        );
        render_loop_state.note_preissued_frame_evidence_key(
            pending_frame_evidence_key.clone(),
            hold_started.elapsed().as_secs_f64() * 1000.0,
        );
        let skip_interactive_surface_readback = options
            .skip_interactive_surface_readback_when_external_proof
            && options.role == NativeWindowRole::Preview
            && options.readback_enabled()
            && external_render_proof_replaces_interactive_readback(
                external_render_proof.as_deref(),
            );
        let product_input_frame = host_input_observed_this_turn
            || render_loop_state.current_frame_lane == Some(NativeFrameLane::ProductInteraction)
            || render_loop_state.current_scheduler_reason == Some(NativeSchedulerReason::HostInput)
            || render_loop_state
                .current_frame_carries_unaccounted_host_input(sampled_input_event_wake_count);
        let loop_elapsed_ms = hold_started.elapsed().as_secs_f64() * 1000.0;
        let allow_pre_submit_proof_poll = pre_submit_proof_poll_allowed(
            product_input_frame,
            render_loop_state.interaction_burst_active(loop_elapsed_ms),
            render_loop_state.current_frame_lane,
            render_loop_state.current_scheduler_reason,
        );
        if allow_pre_submit_proof_poll
            && let Some(result) = poll_interactive_readback_job(&mut interactive_readback_job)
        {
            match result {
                Ok(result) => {
                    last_interactive_readback_finish_ms = Some(result.finish_ms);
                    last_interactive_readback_completed_elapsed_ms =
                        Some(result.completed_elapsed_ms);
                    note_completed_interactive_readback_artifact(
                        &mut render_loop_state,
                        &mut recent_interactive_readback_artifacts,
                        &mut last_interactive_readback_artifact,
                        &mut deferred_artifact_hash_readback_keys,
                        result.artifact,
                        Some(result.completed_elapsed_ms),
                    );
                    last_interactive_surface_readback_pending = false;
                    last_interactive_readback_error = None;
                }
                Err(error) => {
                    last_interactive_readback_finish_ms = None;
                    last_interactive_readback_completed_elapsed_ms =
                        Some(hold_started.elapsed().as_secs_f64() * 1000.0);
                    last_interactive_surface_readback_pending = false;
                    last_interactive_readback_error = Some(error);
                    render_loop_state.telemetry_drop_count =
                        render_loop_state.telemetry_drop_count.saturating_add(1);
                    render_loop_state.last_missed_frame_cause =
                        Some("interactive_readback_error".to_owned());
                }
            }
        }
        let readback_job_in_flight = interactive_readback_job.is_some();
        let interaction_quiet_readback_frame =
            render_loop_state.interaction_quiet_proof_readback_frame(loop_elapsed_ms);
        let product_lane_proof_deferred =
            render_loop_state.defer_proof_readback_for_product_lane(loop_elapsed_ms);
        let interactive_readback_decision = interactive_surface_readback_decision(
            options.role,
            readback_enabled,
            skip_interactive_surface_readback,
            readback_job_in_flight,
            product_input_frame,
            product_lane_proof_deferred,
        );
        if interactive_readback_decision == InteractiveSurfaceReadbackDecision::SkipBackpressure {
            render_loop_state.telemetry_drop_count =
                render_loop_state.telemetry_drop_count.saturating_add(1);
            render_loop_state.last_missed_frame_cause =
                Some("interactive_readback_backpressure".to_owned());
        } else if interactive_readback_decision
            == InteractiveSurfaceReadbackDecision::DeferProductInput
        {
            render_loop_state.note_post_present_subscriber_drain_deferred("product_input_frame");
            render_loop_state.note_proof_readback_deferred("product_input_frame");
        } else if interactive_readback_decision
            == InteractiveSurfaceReadbackDecision::DeferInteractionBurst
        {
            render_loop_state.note_post_present_subscriber_drain_deferred("interaction_burst");
            render_loop_state.note_proof_readback_deferred("interaction_burst");
        }
        let interactive_readback_requested =
            interactive_readback_decision == InteractiveSurfaceReadbackDecision::Queue;
        if interactive_readback_requested && interaction_quiet_readback_frame {
            render_loop_state.note_proof_readback_queued_during_interaction_quiet();
        }
        if !use_offscreen_copy_to_present {
            let pre_present_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
            if pre_present_input_event_wake_count > sampled_input_event_wake_count
                && !render_loop_state
                    .current_frame_carries_unaccounted_host_input(sampled_input_event_wake_count)
                && consecutive_unsampled_input_resamples < MAX_CONSECUTIVE_UNSAMPLED_INPUT_RESAMPLES
            {
                render_loop_state.note_input_pre_present_resample(
                    pre_present_input_event_wake_count
                        .saturating_sub(sampled_input_event_wake_count),
                );
                consecutive_unsampled_input_resamples =
                    consecutive_unsampled_input_resamples.saturating_add(1);
                render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
                render_loop_state.scheduled_wake_count =
                    render_loop_state.scheduled_wake_count.saturating_add(1);
                continue;
            }
            if pre_present_input_event_wake_count <= sampled_input_event_wake_count {
                consecutive_unsampled_input_resamples = 0;
            }
        }
        let interactive_readback =
            if interactive_readback_requested && !use_offscreen_copy_to_present {
                let artifact_dir = options
                    .readback_artifact_dir
                    .as_deref()
                    .expect("queue readback decision requires artifact dir");
                Some((
                    artifact_dir.to_owned(),
                    queue_visible_surface_readback(
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
                        Some(pending_frame_evidence_key.clone()),
                    )?,
                ))
            } else {
                None
            };
        last_interactive_surface_readback_queued = interactive_readback_requested;
        last_interactive_surface_readback_skipped_for_external_proof =
            interactive_readback_decision == InteractiveSurfaceReadbackDecision::SkipExternalProof;
        last_interactive_surface_readback_skipped_for_backpressure = interactive_readback_decision
            == InteractiveSurfaceReadbackDecision::SkipBackpressure
            || interactive_readback_decision
                == InteractiveSurfaceReadbackDecision::DeferProductInput;
        let encoder_finish_started = Instant::now();
        let command_buffer = encoder.finish();
        let encoder_finish_ms = elapsed_ms(encoder_finish_started);
        let queue_submit_started = Instant::now();
        queue.submit(Some(command_buffer));
        let queue_submit_call_ms = elapsed_ms(queue_submit_started);
        render_loop_state.note_queue_submitted(hold_started.elapsed().as_secs_f64() * 1000.0);
        let present_call_started = Instant::now();
        frame.present();
        let present_call_ms = elapsed_ms(present_call_started);
        last_presented_input_event_wake_count = sampled_input_event_wake_count;
        render_loop_state.note_present_completed(hold_started.elapsed().as_secs_f64() * 1000.0);
        render_loop_state.note_submit_phase_durations(
            encoder_finish_ms,
            queue_submit_call_ms,
            present_call_ms,
        );
        let stats_input_timing = render_loop_state
            .take_frame_accepted_input_timing(last_presented_input_event_wake_count);
        let stats_input_to_present_ms = stats_input_timing
            .as_ref()
            .map(|timing| timing.input_to_present_ms);
        let stats_frame_lane = stats_input_timing
            .as_ref()
            .map(|timing| timing.frame_lane)
            .or(render_loop_state.current_frame_lane);
        let product_commit_scheduler_reason = render_loop_state.current_scheduler_reason;
        let product_commit_role_dirty_reason = render_loop_state.current_role_dirty_reason;
        let product_commit_frame_lane = stats_frame_lane.unwrap_or_else(|| {
            native_frame_lane_for_scheduler(
                product_commit_scheduler_reason,
                product_commit_role_dirty_reason,
                host_input_observed_this_turn,
                false,
            )
        });
        render_loop_state.mark_presented_with_revisions(
            rendered_revision,
            rendered_content_revision,
            rendered_layout_revision,
            rendered_render_scene_revision,
        );
        debug_assert_eq!(
            frame_evidence_key_for_presented_frame(
                &render_loop_state,
                &surface_id,
                surface_lifecycle.epoch(),
                (last_presented_input_event_wake_count > 0)
                    .then_some(last_presented_input_event_wake_count),
                None,
            ),
            pending_frame_evidence_key
        );
        let mut product_frame_commit = product_frame_commit_for_presented_frame(
            &render_loop_state,
            pending_frame_evidence_key.clone(),
            product_commit_frame_lane,
            product_commit_scheduler_reason,
            product_commit_role_dirty_reason,
            stats_input_timing.clone(),
        );
        let post_present_input_event_wake_count = input_event_wake_count.load(Ordering::Relaxed);
        let skip_interactive_surface_readback_for_stale_input = interactive_readback_requested
            && post_present_input_event_wake_count > sampled_input_event_wake_count;
        if skip_interactive_surface_readback_for_stale_input {
            render_loop_state.note_input_post_present_stale_readback_skip(
                post_present_input_event_wake_count.saturating_sub(sampled_input_event_wake_count),
            );
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
        }
        last_interactive_surface_readback_skipped_for_stale_input =
            skip_interactive_surface_readback_for_stale_input;
        last_interactive_surface_readback_pending = (interactive_readback_requested
            && !skip_interactive_surface_readback_for_stale_input)
            || (last_interactive_surface_readback_skipped_for_backpressure
                && interactive_readback_job.is_some());
        let current_frame_evidence_key = pending_frame_evidence_key;
        last_frame_evidence_key = Some(current_frame_evidence_key.clone());
        if interactive_readback_requested && !skip_interactive_surface_readback_for_stale_input {
            add_post_present_proof_request_to_commit(
                &mut product_frame_commit,
                visible_surface_readback_post_present_request(),
            );
        }
        render_loop_state.note_product_frame_commit(product_frame_commit.clone());
        push_recent_product_frame_commit(&mut recent_product_frame_commits, &product_frame_commit);
        render_loop_state.schedule_requested_animation_followup(
            Instant::now(),
            hold_started.elapsed().as_secs_f64() * 1000.0,
        );
        if input_event_wake_count.load(Ordering::Relaxed) > last_presented_input_event_wake_count {
            render_loop_state.note_post_present_subscriber_drain_deferred("pending_host_input");
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            continue;
        }
        let deferred_interactive_readback = if !skip_interactive_surface_readback_for_stale_input
            && interactive_readback_requested
            && use_offscreen_copy_to_present
        {
            let artifact_dir = options
                .readback_artifact_dir
                .as_deref()
                .expect("queue readback decision requires artifact dir");
            let Some(readback_texture) = deferred_app_owned_readback_texture.as_ref() else {
                return Err(NativeWindowError::Failed(
                    "offscreen readback requested without an app-owned present target".to_owned(),
                ));
            };
            let mut proof_encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("boon-native-app-window-post-present-proof-encoder"),
                });
            let pending = queue_app_owned_present_target_readback(
                &device,
                &mut proof_encoder,
                readback_texture,
                options.role,
                width,
                height,
                config.format,
                &options.title,
                surface_id.clone(),
                surface_lifecycle.epoch(),
                Some(current_frame_evidence_key.clone()),
            )?;
            queue.submit(Some(proof_encoder.finish()));
            Some((artifact_dir.to_owned(), pending))
        } else {
            None
        };
        if !skip_interactive_surface_readback_for_stale_input
            && let Some((artifact_dir, pending)) =
                interactive_readback.or(deferred_interactive_readback)
        {
            if let Some(result) = poll_interactive_readback_job(&mut interactive_readback_job) {
                match result {
                    Ok(result) => {
                        last_interactive_readback_finish_ms = Some(result.finish_ms);
                        last_interactive_readback_completed_elapsed_ms =
                            Some(result.completed_elapsed_ms);
                        note_completed_interactive_readback_artifact(
                            &mut render_loop_state,
                            &mut recent_interactive_readback_artifacts,
                            &mut last_interactive_readback_artifact,
                            &mut deferred_artifact_hash_readback_keys,
                            result.artifact,
                            Some(result.completed_elapsed_ms),
                        );
                        last_interactive_readback_error = None;
                    }
                    Err(error) => {
                        last_interactive_readback_finish_ms = None;
                        last_interactive_readback_completed_elapsed_ms =
                            Some(hold_started.elapsed().as_secs_f64() * 1000.0);
                        last_interactive_readback_error = Some(error);
                        render_loop_state.telemetry_drop_count =
                            render_loop_state.telemetry_drop_count.saturating_add(1);
                        render_loop_state.last_missed_frame_cause =
                            Some("interactive_readback_error".to_owned());
                    }
                }
            }
            if interactive_readback_job.is_some() {
                render_loop_state.telemetry_drop_count =
                    render_loop_state.telemetry_drop_count.saturating_add(1);
                render_loop_state.last_missed_frame_cause =
                    Some("interactive_readback_backpressure".to_owned());
                last_interactive_surface_readback_pending = true;
                last_interactive_surface_readback_queued = true;
            } else {
                last_interactive_readback_finish_ms = None;
                last_interactive_readback_completed_elapsed_ms = None;
                last_interactive_readback_error = None;
                last_interactive_surface_readback_pending = true;
                interactive_readback_job = Some(spawn_interactive_visible_surface_readback(
                    device.clone(),
                    pending,
                    artifact_dir,
                    current_frame_evidence_key.clone(),
                    render_loop_state.presented_revision,
                    render_loop_state.last_render_content_revision,
                    render_loop_state.rendered_frame_count,
                    wake_handle.clone(),
                    hold_started,
                )?);
            }
        }
        let stats_elapsed = hold_started.elapsed();
        let stats_elapsed_seconds = stats_elapsed.as_secs_f64().max(0.001);
        let stats_render_hook_ms = elapsed_delta_ms(
            render_loop_state.last_surface_acquired_elapsed_ms,
            render_loop_state.last_render_hook_completed_elapsed_ms,
        );
        let stats_present_call_ms = render_loop_state.last_present_call_ms;
        let stats_surface_acquire_call_ms = render_loop_state.last_surface_acquire_call_ms;
        let stats_queue_submit_call_ms = render_loop_state.last_queue_submit_call_ms;
        let stats_present_path_ms = render_loop_state.last_present_path_ms;
        let stats_proof_mode = if last_interactive_readback_error.is_some() {
            "readback_error"
        } else if last_interactive_surface_readback_queued {
            if last_interactive_surface_readback_pending {
                "readback_pending"
            } else {
                "readback"
            }
        } else if last_interactive_surface_readback_skipped_for_external_proof {
            "external_app_owned_readback"
        } else if last_interactive_surface_readback_skipped_for_stale_input {
            "skipped_stale_input"
        } else if last_interactive_surface_readback_skipped_for_backpressure {
            "readback_pending_backpressure"
        } else if options.proof_mode == NativeProofMode::Counters {
            "counters"
        } else {
            "off"
        };
        let recent_presented_input_event_wake_elapsed_ms =
            input_event_wake_elapsed_ms_for_generation(
                &input_event_wake_timeline,
                last_presented_input_event_wake_count,
                hold_started,
            );
        let recent_input_wake_to_present_ms = elapsed_delta_ms(
            recent_presented_input_event_wake_elapsed_ms,
            render_loop_state.last_present_completed_elapsed_ms,
        );
        let post_present_subscriber_allowed = post_present_subscriber_drain_allowed(
            input_event_wake_count.load(Ordering::Relaxed),
            last_presented_input_event_wake_count,
        );
        if post_present_subscriber_allowed {
            let proof_history_entry_count = recent_frame_evidence.len().saturating_add(1);
            push_recent_frame_evidence(
                &mut recent_frame_evidence,
                recent_frame_evidence_entry(
                    &render_loop_state,
                    &current_frame_evidence_key,
                    hold_started.elapsed().as_secs_f64() * 1000.0,
                    last_sampled_input_event_wake_count,
                    last_presented_input_event_wake_count,
                    recent_presented_input_event_wake_elapsed_ms,
                    recent_input_wake_to_present_ms,
                    stats_input_timing.as_ref(),
                    stats_input_to_present_ms,
                    stats_proof_mode,
                    external_render_proof.as_deref(),
                ),
            );
            post_present_proof_subscribers.push(proof_history_post_present_subscriber(
                proof_history_entry_count,
            ));
        }
        preview_perf_accumulator.record(
            stats_render_hook_ms,
            render_loop_state.last_render_frame_metrics.as_ref(),
            stats_present_call_ms,
            stats_surface_acquire_call_ms,
            stats_queue_submit_call_ms,
            stats_present_path_ms,
            stats_input_to_present_ms,
            stats_frame_lane,
            None,
        );
        notify_native_perf_stats(
            &mut hooks,
            native_preview_perf_stats_snapshot(
                options.role,
                &render_loop_state,
                stats_elapsed,
                render_loop_state.rendered_frame_count as f64 / stats_elapsed_seconds,
                &preview_perf_accumulator,
                stats_input_to_present_ms,
                stats_frame_lane,
                stats_proof_mode,
                None,
                Some(current_frame_evidence_key.clone()),
            ),
        );
        if input_event_wake_count.load(Ordering::Relaxed) > last_presented_input_event_wake_count {
            render_loop_state.note_post_present_subscriber_drain_deferred("pending_host_input");
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
            continue;
        }
        if !post_present_subscriber_allowed {
            render_loop_state.note_post_present_subscriber_drain_deferred("pending_host_input");
            render_loop_state.last_scheduler_reason = Some(NativeSchedulerReason::HostInput);
            render_loop_state.scheduled_wake_count =
                render_loop_state.scheduled_wake_count.saturating_add(1);
        } else if let (Some(report), Some(report_writer)) = (
            options.render_loop_state_report.as_deref(),
            async_render_loop_report_writer.as_ref(),
        ) {
            let report_enqueue_started = Instant::now();
            let report_writer_stats = report_writer.stats();
            last_render_loop_report_write_ms = report_writer_stats.last_write_ms;
            let snapshot = render_loop_report_snapshot(
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
                &preview_perf_accumulator,
                render_loop_report_extras(
                    resize_wake_count.load(Ordering::Relaxed),
                    input_event_wake_count.load(Ordering::Relaxed),
                    present_mode.as_str(),
                    surface_format.as_str(),
                    desired_maximum_frame_latency,
                    desired_maximum_frame_latency_source.as_str(),
                    input_event_last_wake_elapsed_ms(&input_event_last_wake_at, hold_started),
                    &app_surface,
                    Some(&observed_input_adapter),
                    options.proof_mode,
                )
                .with_input_generation(
                    last_sampled_input_event_wake_count,
                    last_presented_input_event_wake_count,
                    input_event_wake_elapsed_ms_for_generation(
                        &input_event_wake_timeline,
                        last_sampled_input_event_wake_count,
                        hold_started,
                    ),
                    input_event_wake_elapsed_ms_for_generation(
                        &input_event_wake_timeline,
                        last_presented_input_event_wake_count,
                        hold_started,
                    ),
                )
                .with_frame_input_to_present_ms(stats_input_to_present_ms)
                .with_interactive_timing(
                    report_writer_stats.last_write_ms,
                    last_interactive_readback_finish_ms,
                    last_interactive_readback_completed_elapsed_ms,
                    last_interactive_surface_readback_queued,
                    last_interactive_surface_readback_skipped_for_external_proof,
                    last_interactive_surface_readback_skipped_for_stale_input,
                    last_interactive_surface_readback_skipped_for_backpressure,
                    last_interactive_surface_readback_pending,
                    last_interactive_readback_error.clone(),
                )
                .with_external_render_proof(external_render_proof.as_ref())
                .with_frame_evidence_key(last_frame_evidence_key.as_ref())
                .with_recent_frame_evidence(&recent_frame_evidence)
                .with_recent_product_frame_commits(&recent_product_frame_commits)
                .with_recent_interactive_readback_artifacts(&recent_interactive_readback_artifacts)
                .with_report_writer_stats(Some(report_writer_stats)),
                None,
            );
            let report_enqueue_ms = report_writer.enqueue(snapshot, report_enqueue_started);
            last_render_loop_report_enqueue_ms = Some(report_enqueue_ms);
            post_present_proof_subscribers.push(render_hook_report_json_post_present_subscriber(
                report.to_owned(),
                report_enqueue_ms,
            ));
        }
        if post_present_subscriber_allowed
            && artifact_hash_requested_post_present(&product_frame_commit)
        {
            let artifact_paths = post_present_artifact_hash_paths_for_frame(
                &current_frame_evidence_key,
                last_interactive_readback_artifact.as_ref(),
                &recent_interactive_readback_artifacts,
                external_render_proof.as_deref(),
            );
            if !should_defer_empty_artifact_hash_for_pending_readback(
                &artifact_paths,
                &render_loop_state,
                &current_frame_evidence_key,
                last_interactive_surface_readback_pending,
            ) {
                post_present_proof_subscribers
                    .push(native_post_present_artifact_hash_subscriber(artifact_paths));
            } else {
                push_deferred_artifact_hash_readback_key(
                    &mut deferred_artifact_hash_readback_keys,
                    &current_frame_evidence_key,
                );
            }
        }
        if post_present_subscriber_allowed {
            let post_present_completed_elapsed_ms =
                Some(hold_started.elapsed().as_secs_f64() * 1000.0);
            if let Some(enqueue_report) = async_post_present_proof_worker.enqueue(
                post_present_proof_subscribers,
                current_frame_evidence_key.clone(),
                post_present_completed_elapsed_ms,
                Instant::now(),
            ) {
                render_loop_state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);
            }
        }
        if skip_interactive_surface_readback_for_stale_input {
            continue;
        }
        if loop_mode == NativeRenderLoopMode::ContinuousProbe {
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    async_post_present_proof_worker.shutdown_and_drain(&mut render_loop_state);
    if let Some(result) = finish_interactive_readback_job_before_report(
        &mut interactive_readback_job,
        VISIBLE_SURFACE_READBACK_TIMEOUT,
    ) {
        match result {
            Ok(result) => {
                last_interactive_readback_finish_ms = Some(result.finish_ms);
                last_interactive_readback_completed_elapsed_ms = Some(result.completed_elapsed_ms);
                note_completed_interactive_readback_artifact(
                    &mut render_loop_state,
                    &mut recent_interactive_readback_artifacts,
                    &mut last_interactive_readback_artifact,
                    &mut deferred_artifact_hash_readback_keys,
                    result.artifact,
                    Some(result.completed_elapsed_ms),
                );
                last_interactive_surface_readback_pending = false;
                last_interactive_readback_error = None;
            }
            Err(error) => {
                last_interactive_readback_finish_ms = None;
                last_interactive_readback_completed_elapsed_ms =
                    Some(hold_started.elapsed().as_secs_f64() * 1000.0);
                last_interactive_surface_readback_pending = false;
                last_interactive_readback_error = Some(error);
                render_loop_state.telemetry_drop_count =
                    render_loop_state.telemetry_drop_count.saturating_add(1);
                render_loop_state.last_missed_frame_cause =
                    Some("interactive_readback_error".to_owned());
            }
        }
    }
    let final_report_writer_stats = async_render_loop_report_writer.map(|writer| {
        let mut stats = writer.shutdown();
        if stats.last_enqueue_ms.is_none() {
            stats.last_enqueue_ms = last_render_loop_report_enqueue_ms;
        }
        stats
    });
    if let Some(stats) = final_report_writer_stats.as_ref() {
        last_render_loop_report_write_ms = stats.last_write_ms;
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
            &preview_perf_accumulator,
            render_loop_report_extras(
                resize_wake_count.load(Ordering::Relaxed),
                input_event_wake_count.load(Ordering::Relaxed),
                present_mode.as_str(),
                surface_format.as_str(),
                desired_maximum_frame_latency,
                desired_maximum_frame_latency_source.as_str(),
                input_event_last_wake_elapsed_ms(&input_event_last_wake_at, hold_started),
                &app_surface,
                Some(&observed_input_adapter),
                options.proof_mode,
            )
            .with_input_generation(
                last_sampled_input_event_wake_count,
                last_presented_input_event_wake_count,
                input_event_wake_elapsed_ms_for_generation(
                    &input_event_wake_timeline,
                    last_sampled_input_event_wake_count,
                    hold_started,
                ),
                input_event_wake_elapsed_ms_for_generation(
                    &input_event_wake_timeline,
                    last_presented_input_event_wake_count,
                    hold_started,
                ),
            )
            .with_interactive_timing(
                final_report_writer_stats
                    .as_ref()
                    .and_then(|stats| stats.last_write_ms)
                    .or(last_render_loop_report_write_ms),
                last_interactive_readback_finish_ms,
                last_interactive_readback_completed_elapsed_ms,
                last_interactive_surface_readback_queued,
                last_interactive_surface_readback_skipped_for_external_proof,
                last_interactive_surface_readback_skipped_for_stale_input,
                last_interactive_surface_readback_skipped_for_backpressure,
                last_interactive_surface_readback_pending,
                last_interactive_readback_error.clone(),
            )
            .with_external_render_proof(external_render_proof.as_ref())
            .with_frame_evidence_key(last_frame_evidence_key.as_ref())
            .with_recent_frame_evidence(&recent_frame_evidence)
            .with_recent_product_frame_commits(&recent_product_frame_commits)
            .with_recent_interactive_readback_artifacts(&recent_interactive_readback_artifacts)
            .with_report_writer_stats(final_report_writer_stats),
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

fn queue_texture_readback(
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
    frame_evidence_key: Option<FrameEvidenceKey>,
    capture_method: &'static str,
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
        frame_evidence_key,
        width,
        height,
        unpadded_bytes_per_row,
        padded_bytes_per_row,
        format,
        capture_method,
    })
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
    frame_evidence_key: Option<FrameEvidenceKey>,
) -> Result<PendingSurfaceReadback, NativeWindowError> {
    queue_texture_readback(
        device,
        encoder,
        texture,
        role,
        width,
        height,
        format,
        title,
        surface_id,
        surface_epoch,
        frame_evidence_key,
        "wgpu-visible-surface-copy-src-readback",
    )
}

fn queue_app_owned_present_target_readback(
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
    frame_evidence_key: Option<FrameEvidenceKey>,
) -> Result<PendingSurfaceReadback, NativeWindowError> {
    queue_texture_readback(
        device,
        encoder,
        texture,
        role,
        width,
        height,
        format,
        title,
        surface_id,
        surface_epoch,
        frame_evidence_key,
        "wgpu-app-owned-present-target-copy-to-visible-surface-readback",
    )
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
    input_event_wake_count: u64,
    present_mode: String,
    surface_format: String,
    desired_maximum_frame_latency: u32,
    desired_maximum_frame_latency_source: String,
    last_input_event_wake_elapsed_ms: Option<f64>,
    sampled_input_event_wake_count: Option<u64>,
    presented_input_event_wake_count: Option<u64>,
    sampled_input_event_wake_elapsed_ms: Option<f64>,
    presented_input_event_wake_elapsed_ms: Option<f64>,
    frame_input_to_present_ms: Option<f64>,
    last_render_loop_report_write_ms: Option<f64>,
    last_interactive_readback_finish_ms: Option<f64>,
    last_interactive_readback_completed_elapsed_ms: Option<f64>,
    last_interactive_surface_readback_queued: bool,
    last_interactive_surface_readback_skipped_for_external_proof: bool,
    last_interactive_surface_readback_skipped_for_stale_input: bool,
    last_interactive_surface_readback_skipped_for_backpressure: bool,
    last_interactive_surface_readback_pending: bool,
    last_interactive_readback_error: Option<String>,
    app_window_surface_content_report: Option<serde_json::Value>,
    observed_input_adapter: Option<NativeInputAdapterProof>,
    configured_proof_mode: NativeProofMode,
    external_render_proof: Option<Arc<serde_json::Value>>,
    frame_evidence_key: Option<FrameEvidenceKey>,
    recent_frame_evidence: Vec<serde_json::Value>,
    recent_product_frame_commits: Vec<NativeProductFrameCommit>,
    recent_interactive_readback_artifacts: Vec<AppWindowReadbackArtifact>,
    report_writer: Option<AsyncRenderLoopReportStats>,
}

impl NativeRenderLoopReportExtras {
    fn with_input_generation(
        mut self,
        sampled: u64,
        presented: u64,
        sampled_elapsed_ms: Option<f64>,
        presented_elapsed_ms: Option<f64>,
    ) -> Self {
        self.sampled_input_event_wake_count = Some(sampled);
        self.presented_input_event_wake_count = Some(presented);
        self.sampled_input_event_wake_elapsed_ms = sampled_elapsed_ms;
        self.presented_input_event_wake_elapsed_ms = presented_elapsed_ms;
        self
    }

    fn with_frame_input_to_present_ms(mut self, input_to_present_ms: Option<f64>) -> Self {
        self.frame_input_to_present_ms = input_to_present_ms;
        self
    }

    fn with_interactive_timing(
        mut self,
        report_write_ms: Option<f64>,
        readback_finish_ms: Option<f64>,
        readback_completed_elapsed_ms: Option<f64>,
        surface_readback_queued: bool,
        surface_readback_skipped_for_external_proof: bool,
        surface_readback_skipped_for_stale_input: bool,
        surface_readback_skipped_for_backpressure: bool,
        surface_readback_pending: bool,
        readback_error: Option<String>,
    ) -> Self {
        self.last_render_loop_report_write_ms = report_write_ms;
        self.last_interactive_readback_finish_ms = readback_finish_ms;
        self.last_interactive_readback_completed_elapsed_ms = readback_completed_elapsed_ms;
        self.last_interactive_surface_readback_queued = surface_readback_queued;
        self.last_interactive_surface_readback_skipped_for_external_proof =
            surface_readback_skipped_for_external_proof;
        self.last_interactive_surface_readback_skipped_for_stale_input =
            surface_readback_skipped_for_stale_input;
        self.last_interactive_surface_readback_skipped_for_backpressure =
            surface_readback_skipped_for_backpressure;
        self.last_interactive_surface_readback_pending = surface_readback_pending;
        self.last_interactive_readback_error = readback_error;
        self
    }

    fn with_external_render_proof(mut self, proof: Option<&Arc<serde_json::Value>>) -> Self {
        self.external_render_proof = proof.cloned();
        self
    }

    fn with_frame_evidence_key(mut self, key: Option<&FrameEvidenceKey>) -> Self {
        self.frame_evidence_key = key.cloned();
        self
    }

    fn with_recent_frame_evidence(mut self, recent: &VecDeque<serde_json::Value>) -> Self {
        self.recent_frame_evidence = recent.iter().cloned().collect();
        self
    }

    fn with_recent_product_frame_commits(
        mut self,
        recent: &VecDeque<NativeProductFrameCommit>,
    ) -> Self {
        self.recent_product_frame_commits = recent.iter().cloned().collect();
        self
    }

    fn with_recent_interactive_readback_artifacts(
        mut self,
        recent: &VecDeque<AppWindowReadbackArtifact>,
    ) -> Self {
        self.recent_interactive_readback_artifacts = recent.iter().cloned().collect();
        self
    }

    fn with_report_writer_stats(mut self, stats: Option<AsyncRenderLoopReportStats>) -> Self {
        if let Some(stats) = stats.as_ref() {
            self.last_render_loop_report_write_ms = stats.last_write_ms;
        }
        self.report_writer = stats;
        self
    }
}

fn render_loop_report_extras(
    resize_wake_count: u64,
    input_event_wake_count: u64,
    present_mode: &str,
    surface_format: &str,
    desired_maximum_frame_latency: u32,
    desired_maximum_frame_latency_source: &str,
    last_input_event_wake_elapsed_ms: Option<f64>,
    app_surface: &app_window::surface::Surface,
    observed_input_adapter: Option<&NativeInputAdapterProof>,
    configured_proof_mode: NativeProofMode,
) -> NativeRenderLoopReportExtras {
    NativeRenderLoopReportExtras {
        resize_wake_count,
        input_event_wake_count,
        present_mode: present_mode.to_owned(),
        surface_format: surface_format.to_owned(),
        desired_maximum_frame_latency,
        desired_maximum_frame_latency_source: desired_maximum_frame_latency_source.to_owned(),
        last_input_event_wake_elapsed_ms,
        sampled_input_event_wake_count: None,
        presented_input_event_wake_count: None,
        sampled_input_event_wake_elapsed_ms: None,
        presented_input_event_wake_elapsed_ms: None,
        frame_input_to_present_ms: None,
        last_render_loop_report_write_ms: None,
        last_interactive_readback_finish_ms: None,
        last_interactive_readback_completed_elapsed_ms: None,
        last_interactive_surface_readback_queued: false,
        last_interactive_surface_readback_skipped_for_external_proof: false,
        last_interactive_surface_readback_skipped_for_stale_input: false,
        last_interactive_surface_readback_skipped_for_backpressure: false,
        last_interactive_surface_readback_pending: false,
        last_interactive_readback_error: None,
        app_window_surface_content_report: app_window_surface_content_report(app_surface),
        observed_input_adapter: observed_input_adapter.cloned(),
        configured_proof_mode,
        external_render_proof: None,
        frame_evidence_key: None,
        recent_frame_evidence: Vec::new(),
        recent_product_frame_commits: Vec::new(),
        recent_interactive_readback_artifacts: Vec::new(),
        report_writer: None,
    }
}

fn record_input_event_wake(
    input_event_wake_count: &AtomicU64,
    input_event_last_wake_at: &Mutex<Option<Instant>>,
    input_event_wake_timeline: &Mutex<VecDeque<(u64, Instant)>>,
) -> u64 {
    let wake_at = Instant::now();
    let generation = input_event_wake_count
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if let Ok(mut last_wake_at) = input_event_last_wake_at.lock() {
        *last_wake_at = Some(wake_at);
    }
    if let Ok(mut timeline) = input_event_wake_timeline.lock() {
        timeline.push_back((generation, wake_at));
        while timeline.len() > INPUT_EVENT_WAKE_TIMELINE_LIMIT {
            timeline.pop_front();
        }
    }
    generation
}

fn input_event_last_wake_elapsed_ms(
    input_event_last_wake_at: &Arc<Mutex<Option<Instant>>>,
    hold_started: Instant,
) -> Option<f64> {
    let last_wake_at = input_event_last_wake_at
        .lock()
        .ok()
        .and_then(|guard| *guard)?;
    last_wake_at
        .checked_duration_since(hold_started)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
}

fn input_event_wake_elapsed_ms_for_generation(
    input_event_wake_timeline: &Arc<Mutex<VecDeque<(u64, Instant)>>>,
    generation: u64,
    hold_started: Instant,
) -> Option<f64> {
    if generation == 0 {
        return None;
    }
    let wake_at = input_event_wake_timeline.lock().ok().and_then(|timeline| {
        timeline
            .iter()
            .rev()
            .find_map(|(recorded_generation, wake_at)| {
                (*recorded_generation == generation).then_some(*wake_at)
            })
    })?;
    wake_at
        .checked_duration_since(hold_started)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
}

fn external_render_proof_has_app_owned_readback(proof: Option<&serde_json::Value>) -> bool {
    let Some(proof) = proof else {
        return false;
    };
    let app_owned_reused = proof
        .get("app_owned_readback_reused")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let artifact_hash = proof
        .pointer("/proof/artifact/artifact_sha256")
        .and_then(serde_json::Value::as_str)
        .is_some_and(is_sha256_hex_string);
    (app_owned_reused && artifact_hash)
        || contains_visible_surface_wgpu_readback_proof(proof, false)
}

fn contains_visible_surface_wgpu_readback_proof(
    value: &serde_json::Value,
    ancestor_failed: bool,
) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            let current_failed = ancestor_failed
                || object
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|status| status != "pass");
            let visible_surface_readback = object
                .get("capture_method")
                .and_then(serde_json::Value::as_str)
                == Some("wgpu-visible-surface-copy-src-readback");
            if !current_failed && visible_surface_readback {
                return true;
            }
            object
                .values()
                .any(|child| contains_visible_surface_wgpu_readback_proof(child, current_failed))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|child| contains_visible_surface_wgpu_readback_proof(child, ancestor_failed)),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InteractiveSurfaceReadbackDecision {
    Queue,
    SkipExternalProof,
    SkipBackpressure,
    DeferProductInput,
    DeferInteractionBurst,
    Off,
}

fn interactive_surface_readback_decision(
    role: NativeWindowRole,
    readback_enabled: bool,
    external_proof_replaces_readback: bool,
    readback_job_in_flight: bool,
    product_input_frame: bool,
    interaction_burst_active: bool,
) -> InteractiveSurfaceReadbackDecision {
    if !matches!(role, NativeWindowRole::Preview | NativeWindowRole::Dev) || !readback_enabled {
        return InteractiveSurfaceReadbackDecision::Off;
    }
    if external_proof_replaces_readback {
        return InteractiveSurfaceReadbackDecision::SkipExternalProof;
    }
    if product_input_frame {
        return InteractiveSurfaceReadbackDecision::DeferProductInput;
    }
    if interaction_burst_active {
        return InteractiveSurfaceReadbackDecision::DeferInteractionBurst;
    }
    if readback_job_in_flight {
        return InteractiveSurfaceReadbackDecision::SkipBackpressure;
    }
    InteractiveSurfaceReadbackDecision::Queue
}

fn should_defer_render_for_interactive_readback(
    readback_enabled: bool,
    readback_job_in_flight: bool,
    real_os_input_observed: bool,
    scheduler_reason: Option<NativeSchedulerReason>,
) -> bool {
    readback_enabled
        && readback_job_in_flight
        && !real_os_input_observed
        && scheduler_reason == Some(NativeSchedulerReason::VerifierFrame)
}

fn requested_present_path_mode_from_env() -> NativePresentPathMode {
    if std::env::var_os("BOON_NATIVE_OFFSCREEN_COPY_TO_PRESENT").is_some() {
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent
    } else {
        NativePresentPathMode::DirectVisibleSurface
    }
}

fn select_native_present_path_mode(
    hooks_present: bool,
    surface_copy_to_present_supported: bool,
    requested_mode: NativePresentPathMode,
    readback_enabled: bool,
) -> NativePresentPathSelection {
    let (selected_mode, reason) = match requested_mode {
        NativePresentPathMode::DirectVisibleSurface => (
            NativePresentPathMode::DirectVisibleSurface,
            if readback_enabled {
                "default_direct_visible_surface_with_separate_readback"
            } else {
                "default_direct_visible_surface"
            },
        ),
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent if !hooks_present => (
            NativePresentPathMode::DirectVisibleSurface,
            "offscreen_copy_requested_without_render_hook",
        ),
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent
            if !surface_copy_to_present_supported =>
        {
            (
                NativePresentPathMode::DirectVisibleSurface,
                "offscreen_copy_requested_without_surface_copy_dst",
            )
        }
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent => (
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            "explicit_offscreen_copy_to_present",
        ),
    };
    NativePresentPathSelection {
        requested_mode,
        selected_mode,
        reason,
        hooks_present,
        surface_copy_to_present_supported,
        readback_enabled,
    }
}

fn external_render_proof_replaces_interactive_readback(proof: Option<&serde_json::Value>) -> bool {
    external_render_proof_has_app_owned_readback(proof)
}

fn external_render_proof_with_frame_evidence_key(
    proof: Option<&serde_json::Value>,
    key: Option<&FrameEvidenceKey>,
) -> Option<serde_json::Value> {
    let mut proof = proof?.clone();
    let Some(key) = key else {
        return Some(proof);
    };
    let Ok(key_value) = serde_json::to_value(key) else {
        return Some(proof);
    };
    attach_frame_evidence_key_to_visible_readback_values(&mut proof, &key_value);
    Some(proof)
}

fn attach_frame_evidence_key_to_visible_readback_values(
    value: &mut serde_json::Value,
    key_value: &serde_json::Value,
) {
    match value {
        serde_json::Value::Object(object) => {
            let visible_surface_readback = object
                .get("capture_method")
                .and_then(serde_json::Value::as_str)
                == Some("wgpu-visible-surface-copy-src-readback");
            if visible_surface_readback && !object.contains_key("frame_evidence_key") {
                object.insert("frame_evidence_key".to_owned(), key_value.clone());
            }
            for child in object.values_mut() {
                attach_frame_evidence_key_to_visible_readback_values(child, key_value);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                attach_frame_evidence_key_to_visible_readback_values(child, key_value);
            }
        }
        _ => {}
    }
}

fn is_sha256_hex_string(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn elapsed_delta_ms(start_ms: Option<f64>, end_ms: Option<f64>) -> Option<f64> {
    match (start_ms, end_ms) {
        (Some(start), Some(end)) if end >= start => Some(end - start),
        _ => None,
    }
}

fn native_frame_pacing_snapshot(state: &NativeRenderLoopState) -> NativeFramePacing {
    let state_name = if state.mode == NativeRenderLoopMode::ContinuousProbe {
        NativeFramePacingState::Probe
    } else if state.requested_animation_burst_frames_remaining > 0
        || state
            .requested_animation_burst_quiet_until_elapsed_ms
            .is_some()
    {
        NativeFramePacingState::RequestedAnimationBurst
    } else {
        NativeFramePacingState::Idle
    };
    NativeFramePacing {
        state: state_name,
        target_frame_interval_ms: NATIVE_TARGET_FRAME_INTERVAL_MS,
        last_frame_interval_ms: state.last_present_interval_ms,
        last_frame_lateness_ms: state.last_frame_lateness_ms,
        timer_due: state.next_wake_at.is_some(),
        requested_animation_burst_frames_remaining: state
            .requested_animation_burst_frames_remaining,
        requested_animation_burst_started_elapsed_ms: state
            .requested_animation_burst_started_elapsed_ms,
        requested_animation_burst_quiet_until_elapsed_ms: state
            .requested_animation_burst_quiet_until_elapsed_ms,
        requested_animation_burst_hard_stop_elapsed_ms: state
            .requested_animation_burst_hard_stop_elapsed_ms,
        requested_animation_burst_min_frames: REQUESTED_ANIMATION_BURST_MIN_FRAMES,
        requested_animation_quiet_ms: REQUESTED_ANIMATION_QUIET_MS,
        requested_animation_hard_cap_ms: REQUESTED_ANIMATION_HARD_CAP_MS,
        requested_animation_max_pending_snapshots: REQUESTED_ANIMATION_MAX_PENDING_SNAPSHOTS,
    }
}

fn frame_evidence_key_for_presented_frame(
    state: &NativeRenderLoopState,
    surface_id: &SurfaceId,
    surface_epoch: u64,
    input_event_seq: Option<u64>,
    proof_request_id: Option<u64>,
) -> FrameEvidenceKey {
    FrameEvidenceKey {
        frame_seq: state.rendered_frame_count,
        content_revision: state.last_render_content_revision,
        layout_revision: state.last_render_layout_revision,
        render_scene_revision: state.last_render_scene_revision,
        surface_id: surface_id.clone(),
        surface_epoch,
        input_event_seq,
        present_id: state.rendered_frame_count,
        proof_request_id,
    }
}

fn frame_evidence_key_for_next_presented_frame_with_revisions(
    state: &NativeRenderLoopState,
    surface_id: &SurfaceId,
    surface_epoch: u64,
    input_event_seq: Option<u64>,
    proof_request_id: Option<u64>,
    content_revision: u64,
    layout_revision: u64,
    render_scene_revision: u64,
) -> FrameEvidenceKey {
    let next_frame_seq = state.rendered_frame_count.saturating_add(1);
    FrameEvidenceKey {
        frame_seq: next_frame_seq,
        content_revision,
        layout_revision,
        render_scene_revision,
        surface_id: surface_id.clone(),
        surface_epoch,
        input_event_seq,
        present_id: next_frame_seq,
        proof_request_id,
    }
}

fn product_frame_commit_for_presented_frame(
    state: &NativeRenderLoopState,
    frame_evidence_key: FrameEvidenceKey,
    frame_lane: NativeFrameLane,
    scheduler_reason: Option<NativeSchedulerReason>,
    role_dirty_reason: Option<NativeRoleDirtyReason>,
    input_timing: Option<NativeAcceptedInputFrameTiming>,
) -> NativeProductFrameCommit {
    let render_frame_metrics = state.last_render_frame_metrics.as_ref();
    let product_frame = render_frame_metrics.and_then(|metrics| metrics.product_frame.clone());
    let post_present_proof_requests = render_frame_metrics
        .map(|metrics| metrics.post_present_proof_requests.clone())
        .unwrap_or_default();
    let legacy_pre_present_proof_request_count = post_present_proof_requests
        .iter()
        .filter(|request| request.currently_legacy_pre_present)
        .count() as u32;
    let legacy_product_proof_built_pre_present = product_frame.as_ref().is_some_and(|frame| {
        frame.legacy_proof_json_built_pre_present
            || frame.legacy_render_hook_proof_built_pre_present
    });
    NativeProductFrameCommit {
        schema_version: 1,
        commit_source: "app_window_product_frame_commit".to_owned(),
        input_event_seq: frame_evidence_key.input_event_seq,
        content_revision: frame_evidence_key.content_revision,
        layout_revision: frame_evidence_key.layout_revision,
        render_scene_revision: frame_evidence_key.render_scene_revision,
        render_target_kind: state.last_render_target_kind.clone(),
        present_path_mode: state.last_present_path_mode,
        render_started_elapsed_ms: state.last_render_started_elapsed_ms,
        surface_acquired_elapsed_ms: state.last_surface_acquired_elapsed_ms,
        render_hook_completed_elapsed_ms: state.last_render_hook_completed_elapsed_ms,
        queue_submitted_elapsed_ms: state.last_queue_submitted_elapsed_ms,
        present_completed_elapsed_ms: state.last_present_completed_elapsed_ms,
        surface_acquire_call_ms: state.last_surface_acquire_call_ms,
        encoder_finish_ms: state.last_encoder_finish_ms,
        queue_submit_call_ms: state.last_queue_submit_call_ms,
        present_call_ms: state.last_present_call_ms,
        present_path_ms: state.last_present_path_ms,
        render_hook_outer_state_snapshot_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_state_snapshot_ms),
        render_hook_outer_input_snapshot_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_input_snapshot_ms),
        render_hook_outer_world_snapshot_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_world_snapshot_ms),
        render_hook_outer_core_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_core_ms),
        render_hook_outer_revision_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_revision_ms),
        render_hook_outer_total_ms: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_outer_total_ms),
        render_hook_phase_timings: render_frame_metrics
            .and_then(|metrics| metrics.render_hook_phase_timings.clone()),
        input_to_present_ms: input_timing
            .as_ref()
            .map(|timing| timing.input_to_present_ms),
        host_input_event: input_timing
            .as_ref()
            .and_then(|timing| timing.host_input_event.clone()),
        input_timing,
        frame_evidence_key,
        frame_lane,
        scheduler_reason,
        role_dirty_reason,
        post_present_proof_request_count: post_present_proof_requests.len() as u32,
        post_present_proof_requests,
        legacy_pre_present_proof_request_count,
        legacy_product_proof_built_pre_present,
        product_frame,
    }
}

fn visible_surface_readback_post_present_request() -> NativePostPresentProofRequestSummary {
    NativePostPresentProofRequestSummary {
        kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
        currently_legacy_pre_present: false,
        frame_local_snapshot_required: true,
    }
}

fn add_post_present_proof_request_to_commit(
    commit: &mut NativeProductFrameCommit,
    request: NativePostPresentProofRequestSummary,
) {
    if commit
        .post_present_proof_requests
        .iter()
        .any(|existing| existing.kind == request.kind)
    {
        return;
    }
    if request.currently_legacy_pre_present {
        commit.legacy_pre_present_proof_request_count = commit
            .legacy_pre_present_proof_request_count
            .saturating_add(1);
    }
    commit.post_present_proof_requests.push(request);
    commit.post_present_proof_request_count = commit.post_present_proof_requests.len() as u32;
    if let Some(product_frame) = commit.product_frame.as_mut() {
        product_frame.post_present_proof_request_count = commit.post_present_proof_request_count;
    }
}

fn push_recent_frame_evidence(recent: &mut VecDeque<serde_json::Value>, entry: serde_json::Value) {
    const RECENT_FRAME_EVIDENCE_LIMIT: usize = 32;
    recent.push_back(entry);
    while recent.len() > RECENT_FRAME_EVIDENCE_LIMIT {
        recent.pop_front();
    }
}

fn push_recent_product_frame_commit(
    recent: &mut VecDeque<NativeProductFrameCommit>,
    commit: &NativeProductFrameCommit,
) {
    if commit.input_timing.is_none() {
        return;
    }
    recent.push_back(commit.clone());
    while recent.len() > RECENT_PRODUCT_FRAME_COMMIT_LIMIT {
        recent.pop_front();
    }
}

fn push_recent_interactive_readback_artifact(
    recent: &mut VecDeque<AppWindowReadbackArtifact>,
    artifact: &AppWindowReadbackArtifact,
) {
    recent.push_back(artifact.clone());
    while recent.len() > RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT {
        recent.pop_front();
    }
}

fn push_deferred_artifact_hash_readback_key(
    deferred: &mut VecDeque<FrameEvidenceKey>,
    key: &FrameEvidenceKey,
) {
    if !deferred.iter().any(|existing| existing == key) {
        deferred.push_back(key.clone());
    }
    while deferred.len() > RECENT_POST_PRESENT_PROOF_QUEUE_LIMIT {
        deferred.pop_front();
    }
}

fn take_deferred_artifact_hash_readback_key(
    deferred: &mut VecDeque<FrameEvidenceKey>,
    key: &FrameEvidenceKey,
) -> bool {
    let Some(position) = deferred.iter().position(|existing| existing == key) else {
        return false;
    };
    deferred.remove(position);
    true
}

fn post_present_artifact_for_interactive_readback(
    artifact: &AppWindowReadbackArtifact,
    completed_elapsed_ms: Option<f64>,
) -> Option<NativePostPresentProofArtifact> {
    let frame_evidence_key = artifact.frame_evidence_key.clone()?;
    Some(native_post_present_json_proof_artifact(
        NativePostPresentProofRequestKind::VisibleSurfaceReadback,
        frame_evidence_key.clone(),
        completed_elapsed_ms,
        serde_json::json!({
            "status": "pass",
            "artifact_path": artifact.path,
            "artifact_sha256": artifact.sha256,
            "width": artifact.width,
            "height": artifact.height,
            "presented_revision": artifact.presented_revision,
            "content_revision": artifact.content_revision,
            "rendered_frame_count": artifact.rendered_frame_count,
            "capture_method": artifact.capture_method,
            "texture_format": artifact.texture_format,
            "nonblank_samples": artifact.nonblank_samples,
            "unique_rgba_values": artifact.unique_rgba_values,
            "readback_deadline_ms": artifact.readback_deadline_ms,
            "readback_poll_status": artifact.readback_poll_status,
            "frame_seq": frame_evidence_key.frame_seq,
            "input_event_seq": frame_evidence_key.input_event_seq,
            "present_id": frame_evidence_key.present_id,
        }),
    ))
}

fn post_present_artifact_hash_for_interactive_readback(
    artifact: &AppWindowReadbackArtifact,
    completed_elapsed_ms: Option<f64>,
) -> Option<NativePostPresentProofArtifact> {
    let frame_evidence_key = artifact.frame_evidence_key.clone()?;
    Some(native_post_present_json_proof_artifact(
        NativePostPresentProofRequestKind::ArtifactHash,
        frame_evidence_key.clone(),
        completed_elapsed_ms,
        serde_json::json!({
            "status": "pass",
            "artifact_hash_status": "hashed_registered_artifacts",
            "registered_artifact_count": 1,
            "hashed_artifact_count": 1,
            "artifact_sha256s": [{
                "path": artifact.path,
                "sha256": artifact.sha256,
                "source": "visible_surface_readback_completion"
            }],
            "frame_seq": frame_evidence_key.frame_seq,
            "input_event_seq": frame_evidence_key.input_event_seq,
        }),
    ))
}

fn note_completed_interactive_readback_artifact(
    state: &mut NativeRenderLoopState,
    recent: &mut VecDeque<AppWindowReadbackArtifact>,
    last: &mut Option<AppWindowReadbackArtifact>,
    deferred_artifact_hash_readback_keys: &mut VecDeque<FrameEvidenceKey>,
    artifact: AppWindowReadbackArtifact,
    completed_elapsed_ms: Option<f64>,
) {
    let should_complete_artifact_hash = artifact.frame_evidence_key.as_ref().is_some_and(|key| {
        take_deferred_artifact_hash_readback_key(deferred_artifact_hash_readback_keys, key)
            && state.has_queued_post_present_proof_request(
                key,
                NativePostPresentProofRequestKind::ArtifactHash,
            )
    });
    if let Some(post_present_artifact) =
        post_present_artifact_for_interactive_readback(&artifact, completed_elapsed_ms)
    {
        state.note_post_present_proof_artifact(post_present_artifact);
    }
    if should_complete_artifact_hash
        && let Some(post_present_artifact_hash) =
            post_present_artifact_hash_for_interactive_readback(&artifact, completed_elapsed_ms)
    {
        state.note_post_present_proof_artifact_if_request_queued(post_present_artifact_hash);
    }
    push_recent_interactive_readback_artifact(recent, &artifact);
    *last = Some(artifact);
}

fn recent_interactive_readback_artifact_for_frame<'a>(
    recent: &'a [AppWindowReadbackArtifact],
    key: Option<&FrameEvidenceKey>,
) -> Option<&'a AppWindowReadbackArtifact> {
    let key = key?;
    recent
        .iter()
        .rev()
        .find(|artifact| artifact.frame_evidence_key.as_ref() == Some(key))
}

fn compact_external_render_proof_for_recent_history(
    proof: Option<&serde_json::Value>,
    key: &FrameEvidenceKey,
) -> serde_json::Value {
    let Some(proof) = proof else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "status": proof.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "visible_surface_rendered": proof.get("visible_surface_rendered").cloned().unwrap_or(serde_json::Value::Null),
        "visible_present_path": proof.get("visible_present_path").cloned().unwrap_or(serde_json::Value::Null),
        "render_target_kind": proof.get("render_target_kind").cloned().unwrap_or(serde_json::Value::Null),
        "layout_artifact": proof.get("layout_artifact").cloned().unwrap_or(serde_json::Value::Null),
        "render_hook_phase_timings_ms": proof.get("render_hook_phase_timings_ms").cloned().unwrap_or(serde_json::Value::Null),
        "visible_bound_text": compact_visible_bound_text_for_recent_history(proof),
        "retained_bound_sync": proof.get("retained_bound_sync").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_focus_state": proof.get("input_overlay_focus_state").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_focused_node_probe": proof.get("input_overlay_focused_node_probe").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_render_scene_patch_applied": proof.get("input_overlay_render_scene_patch_applied").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_render_scene_patch_built_this_frame": proof.get("input_overlay_render_scene_patch_built_this_frame").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_render_scene_patch_direct_encode": proof.get("input_overlay_render_scene_patch_direct_encode").cloned().unwrap_or(serde_json::Value::Null),
        "input_overlay_render_scene_patch_touched_node_count": proof.get("input_overlay_render_scene_patch_touched_node_count").cloned().unwrap_or(serde_json::Value::Null),
        "proof": {
            "status": proof.pointer("/proof/status").cloned().unwrap_or(serde_json::Value::Null),
            "capture_method": proof.pointer("/proof/capture_method").cloned().unwrap_or(serde_json::Value::Null),
            "frame_evidence_key": proof.pointer("/proof/frame_evidence_key").cloned().unwrap_or_else(|| serde_json::json!(key)),
            "artifact": {
                "path": proof
                    .pointer("/proof/artifact/artifact_path")
                    .or_else(|| proof.pointer("/proof/artifact/path"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "artifact_sha256": proof.pointer("/proof/artifact/artifact_sha256").cloned().unwrap_or(serde_json::Value::Null),
                "frame_evidence_key": proof.pointer("/proof/artifact/frame_evidence_key").cloned().unwrap_or_else(|| serde_json::json!(key))
            },
            "metrics": {
                "preview_blocked_on_ipc_count": proof.pointer("/proof/metrics/preview_blocked_on_ipc_count").cloned().unwrap_or_else(|| serde_json::json!(0)),
                "frame_seq": proof.pointer("/proof/metrics/frame_seq").cloned().unwrap_or_else(|| serde_json::json!(key.frame_seq)),
                "render_scene_source": proof.pointer("/proof/metrics/render_scene_source").cloned().unwrap_or(serde_json::Value::Null),
                "dirty_chunk_count": proof.pointer("/proof/metrics/dirty_chunk_count").cloned().unwrap_or(serde_json::Value::Null),
                "retained_chunk_hit_count": proof.pointer("/proof/metrics/retained_chunk_hit_count").cloned().unwrap_or(serde_json::Value::Null),
                "retained_chunk_miss_count": proof.pointer("/proof/metrics/retained_chunk_miss_count").cloned().unwrap_or(serde_json::Value::Null),
                "queue_write_count": proof.pointer("/proof/metrics/queue_write_count").cloned().unwrap_or(serde_json::Value::Null),
                "draw_calls": proof.pointer("/proof/metrics/draw_calls").cloned().unwrap_or(serde_json::Value::Null)
            }
        }
    })
}

fn compact_visible_bound_text_for_recent_history(proof: &serde_json::Value) -> serde_json::Value {
    const RECENT_VISIBLE_BOUND_TEXT_ENTRY_LIMIT: usize = 64;
    const RECENT_VISIBLE_BOUND_TEXT_FALLBACK_LIMIT: usize = 8;
    let Some(visible_bound_text) = proof.get("visible_bound_text") else {
        return serde_json::Value::Null;
    };
    let Some(entries) = visible_bound_text
        .get("entries")
        .and_then(serde_json::Value::as_array)
    else {
        return visible_bound_text.clone();
    };

    let mut compact_entries = Vec::new();
    let mut selected_or_focused_count = 0usize;
    for entry in entries {
        let selected = entry.get("selected").and_then(serde_json::Value::as_bool) == Some(true);
        let focused = entry.get("focused").and_then(serde_json::Value::as_bool) == Some(true);
        let selection_bound_path = entry
            .get("paths")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|paths| {
                paths
                    .iter()
                    .any(|path| path.as_str().is_some_and(|path| path.contains("selected")))
            });
        if selected || focused || selection_bound_path {
            selected_or_focused_count = selected_or_focused_count.saturating_add(1);
            if compact_entries.len() < RECENT_VISIBLE_BOUND_TEXT_ENTRY_LIMIT {
                compact_entries.push(compact_visible_bound_text_entry(entry));
            }
        }
    }

    if compact_entries.is_empty() {
        compact_entries.extend(
            entries
                .iter()
                .take(RECENT_VISIBLE_BOUND_TEXT_FALLBACK_LIMIT)
                .map(compact_visible_bound_text_entry),
        );
    }

    serde_json::json!({
        "status": visible_bound_text.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "source": visible_bound_text.get("source").cloned().unwrap_or(serde_json::Value::Null),
        "layout_frame_hash": visible_bound_text.get("layout_frame_hash").cloned().unwrap_or(serde_json::Value::Null),
        "entry_count": visible_bound_text
            .get("entry_count")
            .cloned()
            .unwrap_or_else(|| serde_json::json!(entries.len())),
        "entry_limit": RECENT_VISIBLE_BOUND_TEXT_ENTRY_LIMIT,
        "original_entry_limit": visible_bound_text.get("entry_limit").cloned().unwrap_or(serde_json::Value::Null),
        "truncated": visible_bound_text.get("truncated").cloned().unwrap_or(serde_json::Value::Null),
        "recent_history_compacted": true,
        "selected_or_focused_entry_count": selected_or_focused_count,
        "compact_entry_count": compact_entries.len(),
        "entries": compact_entries
    })
}

fn compact_visible_bound_text_entry(entry: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "node": entry.get("node").cloned().unwrap_or(serde_json::Value::Null),
        "kind": entry.get("kind").cloned().unwrap_or(serde_json::Value::Null),
        "text": entry.get("text").cloned().unwrap_or(serde_json::Value::Null),
        "text_truncated": entry.get("text_truncated").cloned().unwrap_or(serde_json::Value::Null),
        "paths": entry.get("paths").cloned().unwrap_or_else(|| serde_json::json!([])),
        "source_intents": entry.get("source_intents").cloned().unwrap_or_else(|| serde_json::json!([])),
        "source_intent_count": entry.get("source_intent_count").cloned().unwrap_or(serde_json::Value::Null),
        "bounds": entry.get("bounds").cloned().unwrap_or(serde_json::Value::Null),
        "focused": entry.get("focused").cloned().unwrap_or(serde_json::Value::Null),
        "selected": entry.get("selected").cloned().unwrap_or(serde_json::Value::Null)
    })
}

fn compact_poll_diagnostics_for_recent_history(
    diagnostics: Option<&serde_json::Value>,
) -> serde_json::Value {
    let Some(diagnostics) = diagnostics else {
        return serde_json::Value::Null;
    };
    if !diagnostics.is_object() {
        return diagnostics.clone();
    }
    serde_json::json!({
        "kind": diagnostics.get("kind").cloned().unwrap_or(serde_json::Value::Null),
        "dirty": diagnostics.get("dirty").cloned().unwrap_or(serde_json::Value::Null),
        "forced_frame": diagnostics.get("forced_frame").cloned().unwrap_or(serde_json::Value::Null),
        "focus_changed": diagnostics.get("focus_changed").cloned().unwrap_or(serde_json::Value::Null),
        "real_os_events_observed": diagnostics.get("real_os_events_observed").cloned().unwrap_or(serde_json::Value::Null),
        "input_delta_real_os_events_observed": diagnostics
            .get("input_delta_real_os_events_observed")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "before_update_count": diagnostics.get("before_update_count").cloned().unwrap_or(serde_json::Value::Null),
        "after_update_count": diagnostics.get("after_update_count").cloned().unwrap_or(serde_json::Value::Null),
        "after_content_revision": diagnostics.get("after_content_revision").cloned().unwrap_or(serde_json::Value::Null),
        "previous_poll_revision": diagnostics.get("previous_poll_revision").cloned().unwrap_or(serde_json::Value::Null),
        "scheduler_reason": diagnostics.get("scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
        "role_dirty_reason": diagnostics.get("role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null),
        "text_input_caret_active": diagnostics.get("text_input_caret_active").cloned().unwrap_or(serde_json::Value::Null),
        "preview_has_world_scene": diagnostics.get("preview_has_world_scene").cloned().unwrap_or(serde_json::Value::Null),
        "accessibility_snapshot_status": diagnostics.get("accessibility_snapshot_status").cloned().unwrap_or(serde_json::Value::Null),
        "phase_timings_ms": diagnostics.get("phase_timings_ms").cloned().unwrap_or(serde_json::Value::Null),
        "recent_native_input_timing_samples": diagnostics.get("recent_native_input_timing_samples").cloned().unwrap_or_else(|| serde_json::json!([])),
        "native_input_reject_counts": diagnostics.get("native_input_reject_counts").cloned().unwrap_or(serde_json::Value::Null),
        "recent_history_compacted": true
    })
}

fn recent_frame_evidence_entry(
    state: &NativeRenderLoopState,
    key: &FrameEvidenceKey,
    elapsed_ms: f64,
    sampled_input_event_wake_count: u64,
    presented_input_event_wake_count: u64,
    presented_input_event_wake_elapsed_ms: Option<f64>,
    input_wake_to_present_ms: Option<f64>,
    accepted_input_frame_timing: Option<&NativeAcceptedInputFrameTiming>,
    input_accept_to_present_ms: Option<f64>,
    proof_mode: &str,
    external_render_proof: Option<&serde_json::Value>,
) -> serde_json::Value {
    let input_wake_to_input_accept_ms = elapsed_delta_ms(
        presented_input_event_wake_elapsed_ms,
        state.last_accepted_host_input_elapsed_ms,
    );
    let poll_started_to_input_accept_ms = elapsed_delta_ms(
        state.last_poll_started_elapsed_ms,
        state.last_accepted_host_input_elapsed_ms,
    );
    let input_accept_to_dirty_poll_ms = accepted_input_frame_timing
        .and_then(|timing| timing.input_accept_to_dirty_poll_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_accepted_host_input_elapsed_ms,
                state.last_dirty_poll_elapsed_ms,
            )
        });
    let dirty_poll_to_render_started_ms = accepted_input_frame_timing
        .and_then(|timing| timing.dirty_poll_to_render_started_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_dirty_poll_elapsed_ms,
                state.last_render_started_elapsed_ms,
            )
        });
    let render_started_to_surface_acquired_ms = accepted_input_frame_timing
        .and_then(|timing| timing.render_started_to_surface_acquired_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_surface_acquired_elapsed_ms,
            )
        });
    let render_started_to_render_hook_completed_ms = accepted_input_frame_timing
        .and_then(|timing| timing.render_started_to_render_hook_completed_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            )
        });
    let surface_acquired_to_render_hook_completed_ms = accepted_input_frame_timing
        .and_then(|timing| timing.surface_acquired_to_render_hook_completed_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_surface_acquired_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            )
        });
    let render_hook_completed_to_present_ms = accepted_input_frame_timing
        .and_then(|timing| timing.render_hook_completed_to_present_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            )
        });
    let render_hook_to_queue_ms = accepted_input_frame_timing
        .and_then(|timing| timing.render_hook_to_queue_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_queue_submitted_elapsed_ms,
            )
        });
    let queue_to_present_ms = accepted_input_frame_timing
        .and_then(|timing| timing.queue_to_present_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_queue_submitted_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            )
        });
    let surface_acquire_call_ms = accepted_input_frame_timing
        .and_then(|timing| timing.surface_acquire_call_ms)
        .or(state.last_surface_acquire_call_ms);
    let encoder_finish_ms = accepted_input_frame_timing
        .and_then(|timing| timing.encoder_finish_ms)
        .or(state.last_encoder_finish_ms);
    let queue_submit_call_ms = accepted_input_frame_timing
        .and_then(|timing| timing.queue_submit_call_ms)
        .or(state.last_queue_submit_call_ms);
    let present_call_ms = accepted_input_frame_timing
        .and_then(|timing| timing.present_call_ms)
        .or(state.last_present_call_ms);
    let present_path_ms = accepted_input_frame_timing
        .and_then(|timing| timing.present_path_ms)
        .or(state.last_present_path_ms);
    serde_json::json!({
        "status": "pass",
        "recorded_elapsed_ms": elapsed_ms,
        "frame_evidence_key": key,
        "frame_evidence_key_issued_before_present": state.last_frame_evidence_key_issued_before_present,
        "preissued_frame_evidence_key": state.last_preissued_frame_evidence_key.clone(),
        "preissued_frame_evidence_elapsed_ms": state.last_preissued_frame_evidence_elapsed_ms,
        "preissued_frame_evidence_matches_presented_frame": state
            .last_preissued_frame_evidence_key
            .as_ref()
            == Some(key),
        "frame_seq": key.frame_seq,
        "present_id": key.present_id,
        "content_revision": key.content_revision,
        "layout_revision": key.layout_revision,
        "render_scene_revision": key.render_scene_revision,
        "surface_id": &key.surface_id,
        "surface_epoch": key.surface_epoch,
        "input_event_wake_count": presented_input_event_wake_count,
        "input_event_seq": key.input_event_seq,
        "accepted_host_input_event": accepted_input_frame_timing
            .and_then(|timing| timing.host_input_event.clone())
            .or_else(|| state.last_accepted_host_input_event.clone()),
        "sampled_input_event_wake_count": sampled_input_event_wake_count,
        "presented_input_event_wake_count": presented_input_event_wake_count,
        "presented_input_event_wake_elapsed_ms": presented_input_event_wake_elapsed_ms,
        "presented_revision": state.presented_revision,
        "rendered_frame_count": state.rendered_frame_count,
        "input_wake_to_present_ms": input_wake_to_present_ms,
        "input_wake_to_input_accept_ms": input_wake_to_input_accept_ms,
        "input_accept_to_present_ms": input_accept_to_present_ms,
        "frame_input_to_present_ms": input_accept_to_present_ms,
        "frame_lane": accepted_input_frame_timing
            .map(|timing| timing.frame_lane)
            .or(state.last_frame_lane),
        "accepted_input_frame_timing": accepted_input_frame_timing,
        "input_accept_timing_source": input_accept_to_present_ms.map(|_| "role_poll_hook_accepted_visible_host_input"),
        "input_wake_timing_source": input_wake_to_present_ms.map(|_| "recent_presented_input_generation"),
        "last_poll_started_elapsed_ms": state.last_poll_started_elapsed_ms,
        "poll_started_to_input_accept_ms": poll_started_to_input_accept_ms,
        "last_dirty_poll_elapsed_ms": state.last_dirty_poll_elapsed_ms,
        "last_render_started_elapsed_ms": state.last_render_started_elapsed_ms,
        "last_surface_acquired_elapsed_ms": state.last_surface_acquired_elapsed_ms,
        "last_render_hook_completed_elapsed_ms": state.last_render_hook_completed_elapsed_ms,
        "last_queue_submitted_elapsed_ms": state.last_queue_submitted_elapsed_ms,
        "last_present_completed_elapsed_ms": state.last_present_completed_elapsed_ms,
        "last_present_call_ms": state.last_present_call_ms,
        "last_queue_submit_call_ms": state.last_queue_submit_call_ms,
        "last_surface_acquire_call_ms": state.last_surface_acquire_call_ms,
        "input_accept_to_dirty_poll_ms": input_accept_to_dirty_poll_ms,
        "dirty_poll_to_render_started_ms": dirty_poll_to_render_started_ms,
        "render_started_to_surface_acquired_ms": render_started_to_surface_acquired_ms,
        "render_started_to_render_hook_completed_ms": render_started_to_render_hook_completed_ms,
        "surface_acquired_to_render_hook_completed_ms": surface_acquired_to_render_hook_completed_ms,
        "render_hook_completed_to_surface_acquired_ms": elapsed_delta_ms(
            state.last_render_hook_completed_elapsed_ms,
            state.last_surface_acquired_elapsed_ms,
        ),
        "render_hook_completed_to_present_ms": render_hook_completed_to_present_ms,
        "render_hook_to_queue_ms": render_hook_to_queue_ms,
        "queue_to_present_ms": queue_to_present_ms,
        "surface_acquire_call_ms": surface_acquire_call_ms,
        "encoder_finish_ms": encoder_finish_ms,
        "queue_submit_call_ms": queue_submit_call_ms,
        "present_call_ms": present_call_ms,
        "present_path_ms": present_path_ms,
        "last_poll_diagnostics": compact_poll_diagnostics_for_recent_history(
            state.last_poll_diagnostics.as_ref(),
        ),
        "last_render_target_kind": state.last_render_target_kind,
        "configured_proof_mode": proof_mode,
        "proof_mode": proof_mode,
        "last_external_render_proof": compact_external_render_proof_for_recent_history(
            external_render_proof,
            key,
        )
    })
}

fn native_preview_perf_stats_snapshot(
    role: NativeWindowRole,
    state: &NativeRenderLoopState,
    elapsed: Duration,
    renders_per_second: f64,
    accumulator: &NativePreviewPerfAccumulator,
    input_to_present_ms: Option<f64>,
    frame_lane: Option<NativeFrameLane>,
    proof_mode: impl Into<String>,
    proof_overhead_ms: Option<f64>,
    frame_evidence_key: Option<FrameEvidenceKey>,
) -> NativePreviewPerfStats {
    NativePreviewPerfStats {
        kind: "preview-perf-stats".to_owned(),
        status: "pass".to_owned(),
        role,
        frame_seq: state.rendered_frame_count,
        sample_elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        render_loop_mode: state.mode,
        frame_pacing: native_frame_pacing_snapshot(state),
        renders_per_second,
        render_hook_ms: elapsed_delta_ms(
            state.last_surface_acquired_elapsed_ms,
            state.last_render_hook_completed_elapsed_ms,
        ),
        present_call_ms: state.last_present_call_ms,
        frame_present_call_ms: state.last_present_call_ms,
        surface_acquire_call_ms: state.last_surface_acquire_call_ms,
        queue_submit_call_ms: state.last_queue_submit_call_ms,
        present_path_ms: state.last_present_path_ms,
        input_to_present_ms,
        frame_lane,
        render_hook_ms_p50_p95_p99_max: accumulator.render_hook_summary(),
        layout_ms_p50_p95_p99_max: accumulator.layout_summary(),
        present_call_ms_p50_p95_p99_max: accumulator.present_call_summary(),
        frame_present_call_ms_p50_p95_p99_max: accumulator.present_call_summary(),
        surface_acquire_call_ms_p50_p95_p99_max: accumulator.surface_acquire_call_summary(),
        queue_submit_call_ms_p50_p95_p99_max: accumulator.queue_submit_call_summary(),
        present_path_ms_p50_p95_p99_max: accumulator.present_path_summary(),
        input_to_present_ms_p50_p95_p99_max: accumulator.input_to_present_summary(),
        product_input_to_present_ms: if frame_lane == Some(NativeFrameLane::ProductInteraction) {
            input_to_present_ms
        } else {
            None
        },
        product_input_to_present_ms_p50_p95_p99_max: accumulator.product_input_to_present_summary(),
        product_frame_count: accumulator.product_frame_count,
        product_missed_frame_count: accumulator.product_missed_frame_count,
        upload_bytes_p50_p95_max: accumulator.upload_bytes_summary(),
        draw_call_count_p50_p95_max: accumulator.draw_call_count_summary(),
        glyph_cache_hit_rate: state
            .last_render_frame_metrics
            .as_ref()
            .and_then(|metrics| metrics.glyph_cache_hit_rate),
        glyph_cache_hit_rate_p50_p95_max: accumulator.glyph_cache_hit_rate_summary(),
        materialized_item_count: state
            .last_render_frame_metrics
            .as_ref()
            .and_then(|metrics| metrics.materialized_item_count),
        materialized_item_count_p50_p95_max: accumulator.materialized_item_count_summary(),
        missed_frame_count: state.missed_frame_count,
        proof_mode: proof_mode.into(),
        proof_overhead_ms,
        proof_overhead_ms_p50_p95_max: accumulator.proof_overhead_summary(),
        telemetry_drop_count: state.telemetry_drop_count,
        last_missed_frame_cause: state.last_missed_frame_cause.clone(),
        frame_evidence_key,
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

#[derive(Clone, Debug, Default)]
struct AsyncRenderLoopReportStats {
    enqueued_count: u64,
    coalesced_count: u64,
    completed_count: u64,
    error_count: u64,
    pending_count: u64,
    last_enqueue_ms: Option<f64>,
    last_write_ms: Option<f64>,
    last_error: Option<String>,
}

#[derive(Debug)]
struct NativeRenderLoopReportSnapshot {
    path: PathBuf,
    role: NativeWindowRole,
    pid: u32,
    window_id: WindowId,
    surface_id: SurfaceId,
    surface_lifecycle: NativeSurfaceLifecycleReport,
    state: NativeRenderLoopState,
    elapsed: Duration,
    wake_generation: u64,
    last_interactive_readback_artifact: Option<AppWindowReadbackArtifact>,
    perf_accumulator: NativePreviewPerfAccumulator,
    extras: NativeRenderLoopReportExtras,
    loop_error: Option<String>,
}

#[derive(Debug, Default)]
struct AsyncRenderLoopReportShared {
    pending: Option<NativeRenderLoopReportSnapshot>,
    shutdown: bool,
    stats: AsyncRenderLoopReportStats,
}

struct AsyncRenderLoopReportWriter {
    shared: Arc<(Mutex<AsyncRenderLoopReportShared>, Condvar)>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl AsyncRenderLoopReportWriter {
    fn new() -> Self {
        let shared = Arc::new((
            Mutex::new(AsyncRenderLoopReportShared::default()),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("boon-render-loop-report-writer".to_owned())
            .spawn(move || async_render_loop_report_writer(worker_shared))
            .expect("spawn render-loop report writer");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    fn enqueue(&self, snapshot: NativeRenderLoopReportSnapshot, enqueue_started: Instant) -> f64 {
        let (lock, condvar) = &*self.shared;
        let mut enqueue_ms = elapsed_ms(enqueue_started);
        if let Ok(mut shared) = lock.lock() {
            enqueue_ms = elapsed_ms(enqueue_started);
            if shared.pending.is_some() {
                shared.stats.coalesced_count = shared.stats.coalesced_count.saturating_add(1);
            }
            shared.stats.enqueued_count = shared.stats.enqueued_count.saturating_add(1);
            shared.stats.pending_count = 1;
            shared.stats.last_enqueue_ms = Some(enqueue_ms);
            shared.pending = Some(snapshot);
            condvar.notify_one();
        }
        enqueue_ms
    }

    fn stats(&self) -> AsyncRenderLoopReportStats {
        let (lock, _) = &*self.shared;
        lock.lock()
            .map(|shared| shared.stats.clone())
            .unwrap_or_default()
    }

    fn shutdown(mut self) -> AsyncRenderLoopReportStats {
        self.shutdown_worker();
        self.stats()
    }

    fn shutdown_worker(&mut self) {
        let (lock, condvar) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.shutdown = true;
            condvar.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AsyncRenderLoopReportWriter {
    fn drop(&mut self) {
        self.shutdown_worker();
    }
}

fn async_render_loop_report_writer(shared: Arc<(Mutex<AsyncRenderLoopReportShared>, Condvar)>) {
    loop {
        let snapshot = {
            let (lock, condvar) = &*shared;
            let Ok(state) = lock.lock() else {
                return;
            };
            let Ok(mut state) =
                condvar.wait_while(state, |state| state.pending.is_none() && !state.shutdown)
            else {
                return;
            };
            if let Some(snapshot) = state.pending.take() {
                state.stats.pending_count = 0;
                snapshot
            } else if state.shutdown {
                return;
            } else {
                continue;
            }
        };

        let write_started = Instant::now();
        let result = write_render_loop_state_report_snapshot(snapshot);
        let write_ms = elapsed_ms(write_started);
        let (lock, _) = &*shared;
        if let Ok(mut state) = lock.lock() {
            state.stats.last_write_ms = Some(write_ms);
            match result {
                Ok(()) => {
                    state.stats.completed_count = state.stats.completed_count.saturating_add(1);
                    state.stats.last_error = None;
                }
                Err(error) => {
                    state.stats.error_count = state.stats.error_count.saturating_add(1);
                    state.stats.last_error = Some(error.to_string());
                }
            }
        }
    }
}

fn render_loop_report_snapshot(
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
    perf_accumulator: &NativePreviewPerfAccumulator,
    extras: NativeRenderLoopReportExtras,
    loop_error: Option<&str>,
) -> NativeRenderLoopReportSnapshot {
    NativeRenderLoopReportSnapshot {
        path: path.to_path_buf(),
        role,
        pid,
        window_id: window_id.clone(),
        surface_id: surface_id.clone(),
        surface_lifecycle: surface_lifecycle.clone(),
        state: state.clone(),
        elapsed,
        wake_generation,
        last_interactive_readback_artifact: last_interactive_readback_artifact.cloned(),
        perf_accumulator: perf_accumulator.clone(),
        extras,
        loop_error: loop_error.map(str::to_owned),
    }
}

fn write_render_loop_state_report_snapshot(
    snapshot: NativeRenderLoopReportSnapshot,
) -> Result<(), NativeWindowError> {
    write_render_loop_state_report(
        &snapshot.path,
        snapshot.role,
        snapshot.pid,
        &snapshot.window_id,
        &snapshot.surface_id,
        &snapshot.surface_lifecycle,
        &snapshot.state,
        snapshot.elapsed,
        snapshot.wake_generation,
        snapshot.last_interactive_readback_artifact.as_ref(),
        &snapshot.perf_accumulator,
        snapshot.extras,
        snapshot.loop_error.as_deref(),
    )
}

fn post_present_proof_isolation_report(
    state: &NativeRenderLoopState,
    report_write_mode: &str,
) -> serde_json::Value {
    let mut queued_request_count = 0_u64;
    let mut completed_request_count = 0_u64;
    let mut legacy_pre_present_request_count = 0_u64;
    for entry in &state.recent_post_present_proof_queue {
        match entry.status {
            NativePostPresentProofQueueStatus::Queued => {
                queued_request_count = queued_request_count.saturating_add(1);
            }
            NativePostPresentProofQueueStatus::CompletedPostPresent => {
                completed_request_count = completed_request_count.saturating_add(1);
            }
            NativePostPresentProofQueueStatus::LegacyAlreadyBuiltPrePresent => {
                legacy_pre_present_request_count =
                    legacy_pre_present_request_count.saturating_add(1);
            }
        }
    }
    let worker_status = if state.post_present_proof_subscriber_worker_error_count > 0 {
        "error"
    } else if state.post_present_proof_subscriber_worker_dropped_count > 0 {
        "dropped"
    } else if queued_request_count > 0
        || state.post_present_proof_subscriber_worker_pending_batch_count > 0
    {
        "lagging"
    } else if state.post_present_proof_subscriber_worker_enqueued_count > 0 {
        "settled"
    } else {
        "unused"
    };
    serde_json::json!({
        "schema_version": 1,
        "status": "pass",
        "product_path_status": "pass",
        "proof_worker_status": worker_status,
        "mode": "post_present_subscriber_queue",
        "report_write_mode": report_write_mode,
        "product_latency_includes_proof_completion": false,
        "product_blocks_on_proof_subscribers": false,
        "proof_latency_reported_separately": true,
        "proof_lag_allowed_when_reported": true,
        "proof_completion_required_for_product_present": false,
        "report_write_in_hot_path": false,
        "report_serialization_in_hot_path": false,
        "hot_path_report_write_count": 0,
        "hot_path_report_serialization_count": 0,
        "queued_request_count": queued_request_count,
        "completed_request_count": completed_request_count,
        "legacy_pre_present_request_count": legacy_pre_present_request_count,
        "worker_enqueued_batch_count": state.post_present_proof_subscriber_worker_enqueued_batch_count,
        "worker_enqueued_count": state.post_present_proof_subscriber_worker_enqueued_count,
        "worker_pending_batch_count": state.post_present_proof_subscriber_worker_pending_batch_count,
        "worker_completed_count": state.post_present_proof_subscriber_worker_completed_count,
        "worker_dropped_batch_count": state.post_present_proof_subscriber_worker_dropped_batch_count,
        "worker_dropped_count": state.post_present_proof_subscriber_worker_dropped_count,
        "worker_error_count": state.post_present_proof_subscriber_worker_error_count,
        "subscriber_error_count": state.post_present_proof_subscriber_error_count,
        "proof_readback_deferred_count": state.proof_readback_deferred_count,
        "proof_readback_deferred_for_product_input_count": state.proof_readback_deferred_for_product_input_count,
        "proof_readback_deferred_for_interaction_burst_count": state.proof_readback_deferred_for_interaction_burst_count,
        "last_proof_readback_deferred_reason": state.last_proof_readback_deferred_reason.clone(),
        "proof_readback_queued_during_interaction_quiet_count": state.proof_readback_queued_during_interaction_quiet_count,
        "artifact_count": state.post_present_proof_artifact_count,
        "recent_queue_count": state.recent_post_present_proof_queue.len(),
        "recent_artifact_count": state.recent_post_present_proof_artifacts.len()
    })
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
    perf_accumulator: &NativePreviewPerfAccumulator,
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
    let presented_input_wake_elapsed_ms = extras
        .presented_input_event_wake_elapsed_ms
        .or(extras.sampled_input_event_wake_elapsed_ms);
    let input_wake_to_dirty_poll_ms = elapsed_delta_ms(
        presented_input_wake_elapsed_ms,
        state.last_dirty_poll_elapsed_ms,
    );
    let input_wake_to_present_ms = elapsed_delta_ms(
        presented_input_wake_elapsed_ms,
        state.last_present_completed_elapsed_ms,
    );
    let input_wake_to_poll_started_ms = elapsed_delta_ms(
        presented_input_wake_elapsed_ms,
        state.last_poll_started_elapsed_ms,
    );
    let input_wake_to_input_accept_ms = elapsed_delta_ms(
        presented_input_wake_elapsed_ms,
        state.last_accepted_host_input_elapsed_ms,
    );
    let poll_started_to_input_accept_ms = elapsed_delta_ms(
        state.last_poll_started_elapsed_ms,
        state.last_accepted_host_input_elapsed_ms,
    );
    let accepted_input_frame_timing = state.last_accounted_input_frame_timing.clone();
    let input_accept_to_dirty_poll_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.input_accept_to_dirty_poll_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_accepted_host_input_elapsed_ms,
                state.last_dirty_poll_elapsed_ms,
            )
        });
    let input_accept_to_present_ms = extras
        .frame_input_to_present_ms
        .or_else(|| {
            accepted_input_frame_timing
                .as_ref()
                .map(|timing| timing.input_to_present_ms)
        })
        .or_else(|| {
            if extras.presented_input_event_wake_count
                == Some(state.last_accepted_host_input_event_wake_count)
                && state.last_accepted_host_input_event_wake_count
                    > state.last_input_to_present_accounted_event_wake_count
            {
                elapsed_delta_ms(
                    state.last_accepted_host_input_elapsed_ms,
                    state.last_present_completed_elapsed_ms,
                )
            } else {
                None
            }
        });
    let poll_started_to_dirty_poll_ms = elapsed_delta_ms(
        state.last_poll_started_elapsed_ms,
        state.last_dirty_poll_elapsed_ms,
    );
    let dirty_poll_to_render_started_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.dirty_poll_to_render_started_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_dirty_poll_elapsed_ms,
                state.last_render_started_elapsed_ms,
            )
        });
    let render_started_to_surface_acquired_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.render_started_to_surface_acquired_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_surface_acquired_elapsed_ms,
            )
        });
    let render_started_to_render_hook_completed_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.render_started_to_render_hook_completed_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_started_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            )
        });
    let surface_acquired_to_render_hook_completed_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.surface_acquired_to_render_hook_completed_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_surface_acquired_elapsed_ms,
                state.last_render_hook_completed_elapsed_ms,
            )
        });
    let render_hook_completed_to_surface_acquired_ms = if accepted_input_frame_timing.is_some() {
        None
    } else {
        elapsed_delta_ms(
            state.last_render_hook_completed_elapsed_ms,
            state.last_surface_acquired_elapsed_ms,
        )
    };
    let render_hook_completed_to_present_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.render_hook_completed_to_present_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            )
        });
    let render_hook_to_queue_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.render_hook_to_queue_ms)
        .or_else(|| {
            elapsed_delta_ms(
                state.last_render_hook_completed_elapsed_ms,
                state.last_queue_submitted_elapsed_ms,
            )
        });
    let poll_started_to_queue_ms = elapsed_delta_ms(
        state.last_poll_started_elapsed_ms,
        state.last_queue_submitted_elapsed_ms,
    );
    let render_started_to_queue_ms = elapsed_delta_ms(
        state.last_render_started_elapsed_ms,
        state.last_queue_submitted_elapsed_ms,
    );
    let wake_to_queue_ms = elapsed_delta_ms(
        presented_input_wake_elapsed_ms,
        state.last_queue_submitted_elapsed_ms,
    );
    let queue_to_present_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.queue_to_present_ms)
        .or_else(|| {
            match (
                state.last_queue_submitted_elapsed_ms,
                state.last_present_completed_elapsed_ms,
            ) {
                (Some(queue_ms), Some(present_ms)) if present_ms >= queue_ms => {
                    Some(present_ms - queue_ms)
                }
                _ => None,
            }
        });
    let surface_acquire_call_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.surface_acquire_call_ms)
        .or(state.last_surface_acquire_call_ms);
    let encoder_finish_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.encoder_finish_ms)
        .or(state.last_encoder_finish_ms);
    let queue_submit_call_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.queue_submit_call_ms)
        .or(state.last_queue_submit_call_ms);
    let present_call_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.present_call_ms)
        .or(state.last_present_call_ms);
    let present_path_ms = accepted_input_frame_timing
        .as_ref()
        .and_then(|timing| timing.present_path_ms)
        .or(state.last_present_path_ms);
    let present_to_readback_report_ms = match (
        state.last_present_completed_elapsed_ms,
        extras.last_interactive_readback_completed_elapsed_ms,
    ) {
        (Some(present_ms), Some(readback_ms)) if readback_ms >= present_ms => {
            Some(readback_ms - present_ms)
        }
        _ => None,
    };
    let frame_matched_interactive_readback_artifact =
        recent_interactive_readback_artifact_for_frame(
            &extras.recent_interactive_readback_artifacts,
            extras.frame_evidence_key.as_ref(),
        );
    let proof_lag_artifact =
        frame_matched_interactive_readback_artifact.or(last_interactive_readback_artifact);
    let proof_lag_frames = proof_lag_artifact.and_then(|artifact| {
        artifact
            .frame_evidence_key
            .as_ref()
            .and_then(|artifact_key| {
                extras
                    .frame_evidence_key
                    .as_ref()
                    .map(|current_key| current_key.frame_seq.saturating_sub(artifact_key.frame_seq))
            })
    });
    let proof_mode = if extras.last_interactive_readback_error.is_some() {
        "readback_error"
    } else if extras.last_interactive_surface_readback_queued {
        if extras.last_interactive_surface_readback_pending {
            "readback_pending"
        } else {
            "readback"
        }
    } else if extras.last_interactive_surface_readback_skipped_for_external_proof {
        "external_app_owned_readback"
    } else if extras.last_interactive_surface_readback_skipped_for_stale_input {
        "skipped_stale_input"
    } else if extras.last_interactive_surface_readback_skipped_for_backpressure {
        "readback_pending_backpressure"
    } else if extras.configured_proof_mode == NativeProofMode::Counters {
        "counters"
    } else {
        "off"
    };
    let report_write_mode = extras
        .report_writer
        .as_ref()
        .map(|_| "async_latest_wins_atomic_replace")
        .unwrap_or("atomic_replace");
    let post_present_proof_isolation =
        post_present_proof_isolation_report(state, report_write_mode);
    let mut report_perf_accumulator = perf_accumulator.clone();
    report_perf_accumulator.record(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        present_to_readback_report_ms,
    );
    let input_accept_frame_lane = accepted_input_frame_timing
        .as_ref()
        .map(|timing| timing.frame_lane)
        .or(state.last_frame_lane);
    let preview_perf_stats = native_preview_perf_stats_snapshot(
        role,
        state,
        elapsed,
        renders_per_second,
        &report_perf_accumulator,
        input_accept_to_present_ms,
        input_accept_frame_lane,
        proof_mode,
        present_to_readback_report_ms,
        extras.frame_evidence_key.clone(),
    );
    let stale_for_latest_input = extras
        .presented_input_event_wake_count
        .is_some_and(|presented| presented < extras.input_event_wake_count);
    let active_timer_reason = state.next_wake_at.map(|_| {
        if state.current_scheduler_reason == Some(NativeSchedulerReason::Timer) {
            "timer_due"
        } else {
            "scheduled_wake"
        }
    });
    let external_render_proof = external_render_proof_with_frame_evidence_key(
        extras.external_render_proof.as_deref(),
        extras.frame_evidence_key.as_ref(),
    );
    let last_product_render_frame = state
        .last_render_frame_metrics
        .as_ref()
        .and_then(|metrics| metrics.product_frame.clone());
    let last_post_present_proof_requests = state
        .last_render_frame_metrics
        .as_ref()
        .map(|metrics| metrics.post_present_proof_requests.clone())
        .unwrap_or_default();
    let post_present_proof_request_count = last_post_present_proof_requests.len();
    let legacy_pre_present_proof_request_count = last_post_present_proof_requests
        .iter()
        .filter(|request| request.currently_legacy_pre_present)
        .count();
    let legacy_product_proof_built_pre_present =
        last_product_render_frame.as_ref().is_some_and(|frame| {
            frame.legacy_proof_json_built_pre_present
                || frame.legacy_render_hook_proof_built_pre_present
        });
    let last_product_frame_commit = state.last_product_frame_commit.clone();
    let product_frame_commit_matches_frame_evidence_key =
        last_product_frame_commit.as_ref().is_some_and(|commit| {
            extras.frame_evidence_key.as_ref() == Some(&commit.frame_evidence_key)
        });
    let mut report = serde_json::json!({
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
        "last_render_layout_revision": state.last_render_layout_revision,
        "last_render_scene_revision": state.last_render_scene_revision,
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
        "last_render_target_kind": state.last_render_target_kind,
        "present_path_mode": state
            .last_present_path_mode
            .map(NativePresentPathMode::as_str),
        "present_path_requested_mode": state
            .last_present_path_requested_mode
            .map(NativePresentPathMode::as_str),
        "present_path_selection_reason": state.last_present_path_selection_reason,
        "present_path_hooks_present": state.last_present_path_hooks_present,
        "present_path_surface_copy_to_present_supported": state
            .last_present_path_surface_copy_to_present_supported,
        "present_path_readback_enabled": state.last_present_path_readback_enabled,
        "last_poll_diagnostics": state.last_poll_diagnostics,
        "frame_pacing": preview_perf_stats.frame_pacing.clone(),
        "preview_perf_stats": preview_perf_stats.clone(),
        "loop_exit_reason": state.loop_exit_reason,
        "forced_frame_count": state.forced_frame_count,
        "renders_per_second": renders_per_second,
        "scheduled_wake_count": state.scheduled_wake_count,
        "active_timer_reason": active_timer_reason,
        "passive_input_poll_interval_ms": PASSIVE_INPUT_POLL_INTERVAL.as_millis() as u64,
        "resize_wake_count": extras.resize_wake_count,
        "input_event_wake_count": extras.input_event_wake_count,
        "sampled_input_event_wake_count": extras.sampled_input_event_wake_count,
        "presented_input_event_wake_count": extras.presented_input_event_wake_count,
        "stale_for_latest_input": stale_for_latest_input,
        "last_input_event_wake_elapsed_ms": extras.last_input_event_wake_elapsed_ms,
        "sampled_input_event_wake_elapsed_ms": extras.sampled_input_event_wake_elapsed_ms,
        "presented_input_event_wake_elapsed_ms": extras.presented_input_event_wake_elapsed_ms,
        "frame_input_wake_elapsed_ms": presented_input_wake_elapsed_ms,
        "input_wake_timing_source": if extras.presented_input_event_wake_elapsed_ms.is_some() {
            "presented_input_generation"
        } else if extras.sampled_input_event_wake_elapsed_ms.is_some() {
            "sampled_input_generation"
        } else {
            "missing_generation_timestamp"
        },
        "accepted_host_input_event_wake_count": state.last_accepted_host_input_event_wake_count,
        "accepted_host_input_elapsed_ms": state.last_accepted_host_input_elapsed_ms,
        "accepted_host_input_press_only": state.last_accepted_host_input_press_only,
        "input_to_present_accounted_event_wake_count": state.last_input_to_present_accounted_event_wake_count,
        "accepted_input_frame_timing": accepted_input_frame_timing,
        "frame_lane": input_accept_frame_lane,
        "top_level_phase_timing_scope": if accepted_input_frame_timing.is_some() {
            "accepted_visible_host_input_frame"
        } else {
            "latest_presented_frame"
        },
        "latest_frame_timing_scope": "latest_presented_frame_raw_last_fields",
        "frame_input_to_present_ms": input_accept_to_present_ms,
        "input_accept_timing_source": if state.last_accepted_host_input_elapsed_ms.is_some() {
            "role_poll_hook_accepted_visible_host_input"
        } else {
            "missing_accepted_host_input"
        },
        "input_wake_to_input_accept_ms": input_wake_to_input_accept_ms,
        "poll_started_to_input_accept_ms": poll_started_to_input_accept_ms,
        "input_accept_to_dirty_poll_ms": input_accept_to_dirty_poll_ms,
        "input_accept_to_present_ms": input_accept_to_present_ms,
        "input_wake_to_dirty_poll_ms": input_wake_to_dirty_poll_ms,
        "input_wake_to_poll_started_ms": input_wake_to_poll_started_ms,
        "poll_started_to_dirty_poll_ms": poll_started_to_dirty_poll_ms,
        "dirty_poll_to_render_started_ms": dirty_poll_to_render_started_ms,
        "render_started_to_surface_acquired_ms": render_started_to_surface_acquired_ms,
        "render_started_to_render_hook_completed_ms": render_started_to_render_hook_completed_ms,
        "surface_acquired_to_render_hook_completed_ms": surface_acquired_to_render_hook_completed_ms,
        "render_hook_completed_to_surface_acquired_ms": render_hook_completed_to_surface_acquired_ms,
        "render_hook_completed_to_present_ms": render_hook_completed_to_present_ms,
        "render_hook_to_queue_ms": render_hook_to_queue_ms,
        "poll_started_to_queue_ms": poll_started_to_queue_ms,
        "render_started_to_queue_ms": render_started_to_queue_ms,
        "input_wake_to_present_ms": input_wake_to_present_ms,
        "wake_to_queue_ms": wake_to_queue_ms,
        "queue_to_present_ms": queue_to_present_ms,
        "present_to_readback_report_ms": present_to_readback_report_ms,
        "proof_lag_frames": proof_lag_frames,
        "render_loop_report_write_mode": report_write_mode,
        "report_write_in_hot_path": false,
        "report_serialization_in_hot_path": false,
        "hot_path_report_write_count": 0,
        "hot_path_report_serialization_count": 0,
        "post_present_proof_isolation": post_present_proof_isolation,
        "post_present_subscriber_drain_deferred_count": state.post_present_subscriber_drain_deferred_count,
        "last_post_present_subscriber_drain_deferred_reason": state.last_post_present_subscriber_drain_deferred_reason.clone(),
        "post_present_proof_subscriber_worker_enqueued_batch_count": state.post_present_proof_subscriber_worker_enqueued_batch_count,
        "post_present_proof_subscriber_worker_enqueued_count": state.post_present_proof_subscriber_worker_enqueued_count,
        "post_present_proof_subscriber_worker_dropped_batch_count": state.post_present_proof_subscriber_worker_dropped_batch_count,
        "post_present_proof_subscriber_worker_dropped_count": state.post_present_proof_subscriber_worker_dropped_count,
        "post_present_proof_subscriber_worker_completed_count": state.post_present_proof_subscriber_worker_completed_count,
        "post_present_proof_subscriber_worker_error_count": state.post_present_proof_subscriber_worker_error_count,
        "post_present_proof_subscriber_worker_pending_batch_count": state.post_present_proof_subscriber_worker_pending_batch_count,
        "last_post_present_proof_subscriber_worker_enqueue_ms": state.last_post_present_proof_subscriber_worker_enqueue_ms,
        "last_render_loop_report_write_ms": extras.last_render_loop_report_write_ms,
        "last_render_loop_report_enqueue_ms": extras
            .report_writer
            .as_ref()
            .and_then(|stats| stats.last_enqueue_ms),
        "render_loop_report_async_enqueued_count": extras
            .report_writer
            .as_ref()
            .map(|stats| stats.enqueued_count)
            .unwrap_or(0),
        "render_loop_report_async_coalesced_count": extras
            .report_writer
            .as_ref()
            .map(|stats| stats.coalesced_count)
            .unwrap_or(0),
        "render_loop_report_async_completed_count": extras
            .report_writer
            .as_ref()
            .map(|stats| stats.completed_count)
            .unwrap_or(0),
        "render_loop_report_async_error_count": extras
            .report_writer
            .as_ref()
            .map(|stats| stats.error_count)
            .unwrap_or(0),
        "render_loop_report_async_pending_count": extras
            .report_writer
            .as_ref()
            .map(|stats| stats.pending_count)
            .unwrap_or(0),
        "render_loop_report_async_last_error": extras
            .report_writer
            .as_ref()
            .and_then(|stats| stats.last_error.clone()),
        "last_interactive_readback_finish_ms": extras.last_interactive_readback_finish_ms,
        "last_interactive_readback_completed_elapsed_ms": extras.last_interactive_readback_completed_elapsed_ms,
        "last_interactive_surface_readback_queued": extras.last_interactive_surface_readback_queued,
        "last_interactive_surface_readback_skipped_for_external_proof": extras.last_interactive_surface_readback_skipped_for_external_proof,
        "last_interactive_surface_readback_skipped_for_stale_input": extras.last_interactive_surface_readback_skipped_for_stale_input,
        "last_interactive_surface_readback_skipped_for_backpressure": extras.last_interactive_surface_readback_skipped_for_backpressure,
        "last_interactive_surface_readback_pending": extras.last_interactive_surface_readback_pending,
        "last_interactive_readback_error": extras.last_interactive_readback_error,
        "interactive_readback_registry_limit": RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT,
        "recent_interactive_readback_artifact_count": extras.recent_interactive_readback_artifacts.len(),
        "recent_interactive_readback_artifacts": extras.recent_interactive_readback_artifacts.clone(),
        "matching_interactive_readback_artifact_for_frame": frame_matched_interactive_readback_artifact,
        "matching_interactive_readback_artifact_for_frame_status": if frame_matched_interactive_readback_artifact.is_some() {
            "matched"
        } else if extras.frame_evidence_key.is_some() {
            "missing_for_frame"
        } else {
            "missing_frame_evidence_key"
        },
        "app_window_surface_content_report": extras.app_window_surface_content_report,
        "observed_input_adapter": extras.observed_input_adapter,
        "last_external_render_proof": external_render_proof,
        "last_product_render_frame": last_product_render_frame,
        "last_product_frame_commit": last_product_frame_commit,
        "product_frame_commit_count": state.product_frame_commit_count,
        "recent_product_frame_commit_count": extras.recent_product_frame_commits.len(),
        "recent_product_frame_commits": extras.recent_product_frame_commits,
        "product_frame_commit_matches_frame_evidence_key": product_frame_commit_matches_frame_evidence_key,
        "post_present_proof_queue_limit": RECENT_POST_PRESENT_PROOF_QUEUE_LIMIT,
        "post_present_proof_queue_enqueued_count": state.post_present_proof_queue_enqueued_count,
        "post_present_proof_queue_deferred_count": state.post_present_proof_queue_deferred_count,
        "post_present_proof_queue_legacy_pre_present_count": state.post_present_proof_queue_legacy_pre_present_count,
        "post_present_proof_queue_completed_count": state.post_present_proof_queue_completed_count,
        "recent_post_present_proof_queue_count": state.recent_post_present_proof_queue.len(),
        "recent_post_present_proof_queue": state.recent_post_present_proof_queue.clone(),
        "post_present_proof_artifact_limit": RECENT_POST_PRESENT_PROOF_ARTIFACT_LIMIT,
        "post_present_proof_artifact_count": state.post_present_proof_artifact_count,
        "post_present_proof_subscriber_error_count": state.post_present_proof_subscriber_error_count,
        "last_post_present_proof_subscriber_error": state.last_post_present_proof_subscriber_error.clone(),
        "recent_post_present_proof_artifact_count": state.recent_post_present_proof_artifacts.len(),
        "recent_post_present_proof_artifacts": state.recent_post_present_proof_artifacts.clone(),
        "post_present_proof_requests": last_post_present_proof_requests,
        "post_present_proof_request_count": post_present_proof_request_count,
        "legacy_pre_present_proof_request_count": legacy_pre_present_proof_request_count,
        "legacy_product_proof_built_pre_present": legacy_product_proof_built_pre_present,
        "frame_evidence_key_issued_before_present": state.last_frame_evidence_key_issued_before_present,
        "preissued_frame_evidence_key": state.last_preissued_frame_evidence_key.clone(),
        "preissued_frame_evidence_elapsed_ms": state.last_preissued_frame_evidence_elapsed_ms,
        "preissued_frame_evidence_matches_presented_frame": state
            .last_preissued_frame_evidence_key
            .as_ref()
            == extras.frame_evidence_key.as_ref(),
        "frame_evidence_key": extras.frame_evidence_key,
        "recent_frame_evidence": extras.recent_frame_evidence,
        "last_scheduler_reason": state.last_scheduler_reason,
        "last_role_dirty_reason": state.last_role_dirty_reason,
        "current_scheduler_reason": state.current_scheduler_reason,
        "current_role_dirty_reason": state.current_role_dirty_reason,
        "loop_error": loop_error,
        "last_interactive_readback_artifact": last_interactive_readback_artifact
    });
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "requested_animation_burst_count".to_owned(),
            serde_json::json!(state.requested_animation_burst_count),
        );
        object.insert(
            "requested_animation_prewarm_count".to_owned(),
            serde_json::json!(state.requested_animation_prewarm_count),
        );
        object.insert(
            "requested_animation_burst_present_on_wake".to_owned(),
            serde_json::json!(state.requested_animation_burst_present_on_wake),
        );
        object.insert(
            "armed_frame_token".to_owned(),
            serde_json::json!(state.armed_frame_token),
        );
        object.insert(
            "armed_frame_started_elapsed_ms".to_owned(),
            serde_json::json!(state.armed_frame_started_elapsed_ms),
        );
        object.insert(
            "clean_armed_poll_count".to_owned(),
            serde_json::json!(state.clean_armed_poll_count),
        );
        object.insert(
            "skipped_clean_burst_present_count".to_owned(),
            serde_json::json!(state.skipped_clean_burst_present_count),
        );
        object.insert(
            "input_waited_for_already_armed_frame_count".to_owned(),
            serde_json::json!(state.input_waited_for_already_armed_frame_count),
        );
        object.insert(
            "last_burst_exit_reason".to_owned(),
            serde_json::json!(state.last_burst_exit_reason),
        );
        object.insert(
            "proof_readback_deferred_count".to_owned(),
            serde_json::json!(state.proof_readback_deferred_count),
        );
        object.insert(
            "proof_readback_deferred_for_product_input_count".to_owned(),
            serde_json::json!(state.proof_readback_deferred_for_product_input_count),
        );
        object.insert(
            "proof_readback_deferred_for_interaction_burst_count".to_owned(),
            serde_json::json!(state.proof_readback_deferred_for_interaction_burst_count),
        );
        object.insert(
            "last_proof_readback_deferred_reason".to_owned(),
            serde_json::json!(state.last_proof_readback_deferred_reason),
        );
        object.insert(
            "proof_readback_queued_during_interaction_quiet_count".to_owned(),
            serde_json::json!(state.proof_readback_queued_during_interaction_quiet_count),
        );
        object.insert(
            "accepted_host_input_event".to_owned(),
            serde_json::json!(state.last_accepted_host_input_event),
        );
        object.insert(
            "present_mode".to_owned(),
            serde_json::json!(extras.present_mode),
        );
        object.insert(
            "surface_format".to_owned(),
            serde_json::json!(extras.surface_format),
        );
        object.insert(
            "configured_proof_mode".to_owned(),
            serde_json::json!(extras.configured_proof_mode.as_str()),
        );
        object.insert(
            "desired_maximum_frame_latency".to_owned(),
            serde_json::json!(extras.desired_maximum_frame_latency),
        );
        object.insert(
            "desired_maximum_frame_latency_source".to_owned(),
            serde_json::json!(extras.desired_maximum_frame_latency_source),
        );
        object.insert(
            "last_encoder_finish_ms".to_owned(),
            serde_json::json!(state.last_encoder_finish_ms),
        );
        object.insert(
            "last_surface_acquire_call_ms".to_owned(),
            serde_json::json!(state.last_surface_acquire_call_ms),
        );
        object.insert(
            "last_queue_submit_call_ms".to_owned(),
            serde_json::json!(state.last_queue_submit_call_ms),
        );
        object.insert(
            "last_present_call_ms".to_owned(),
            serde_json::json!(state.last_present_call_ms),
        );
        object.insert(
            "last_present_path_ms".to_owned(),
            serde_json::json!(state.last_present_path_ms),
        );
        object.insert(
            "encoder_finish_ms".to_owned(),
            serde_json::json!(encoder_finish_ms),
        );
        object.insert(
            "surface_acquire_call_ms".to_owned(),
            serde_json::json!(surface_acquire_call_ms),
        );
        object.insert(
            "queue_submit_call_ms".to_owned(),
            serde_json::json!(queue_submit_call_ms),
        );
        object.insert(
            "present_call_ms".to_owned(),
            serde_json::json!(present_call_ms),
        );
        object.insert(
            "frame_present_call_ms".to_owned(),
            serde_json::json!(present_call_ms),
        );
        object.insert(
            "present_path_ms".to_owned(),
            serde_json::json!(present_path_ms),
        );
        object.insert(
            "input_inline_resample_count".to_owned(),
            serde_json::json!(state.input_inline_resample_count),
        );
        object.insert(
            "input_deferred_resample_count".to_owned(),
            serde_json::json!(state.input_deferred_resample_count),
        );
        object.insert(
            "input_inline_resample_event_gap_count".to_owned(),
            serde_json::json!(state.input_inline_resample_event_gap_count),
        );
        object.insert(
            "input_deferred_resample_event_gap_count".to_owned(),
            serde_json::json!(state.input_deferred_resample_event_gap_count),
        );
        object.insert(
            "last_input_resample_event_gap_count".to_owned(),
            serde_json::json!(state.last_input_resample_event_gap_count),
        );
        object.insert(
            "last_input_resample_kind".to_owned(),
            serde_json::json!(state.last_input_resample_kind),
        );
    }
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| NativeWindowError::Failed(format!("serialize loop state: {error}")))?;
    write_atomic_report_bytes(path, &bytes).map_err(|error| {
        NativeWindowError::Failed(format!(
            "write render-loop report {}: {error}",
            path.display()
        ))
    })?;
    Ok(())
}

fn write_atomic_report_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("render-loop-report.json");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temp_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        unique
    ));
    std::fs::write(&temp_path, bytes)?;
    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
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
        frame_evidence_key: pending.frame_evidence_key,
        capture_method: pending.capture_method.to_owned(),
        texture_format: format!("{:?}", pending.format),
        nonblank_samples,
        unique_rgba_values,
        readback_deadline_ms: VISIBLE_SURFACE_READBACK_TIMEOUT.as_millis() as u64,
        readback_poll_status: "completed_before_deadline".to_owned(),
    })
}

fn spawn_interactive_visible_surface_readback(
    device: wgpu::Device,
    pending: PendingSurfaceReadback,
    artifact_dir: String,
    frame_evidence_key: FrameEvidenceKey,
    presented_revision: u64,
    content_revision: u64,
    rendered_frame_count: u64,
    wake_handle: NativeWakeHandle,
    hold_started: Instant,
) -> Result<AsyncInteractiveReadbackJob, NativeWindowError> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("boon-interactive-readback".to_owned())
        .spawn(move || {
            let finish_started = Instant::now();
            let result = finish_visible_surface_readback(&device, pending, &artifact_dir)
                .map(|mut artifact| {
                    artifact.presented_revision = Some(presented_revision);
                    artifact.content_revision = Some(content_revision);
                    artifact.rendered_frame_count = Some(rendered_frame_count);
                    artifact.frame_evidence_key = Some(frame_evidence_key);
                    AsyncInteractiveReadbackResult {
                        artifact,
                        finish_ms: elapsed_ms(finish_started),
                        completed_elapsed_ms: hold_started.elapsed().as_secs_f64() * 1000.0,
                    }
                })
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
            wake_handle.wake();
        })
        .map_err(|error| {
            NativeWindowError::Failed(format!("spawn interactive readback worker: {error}"))
        })?;
    Ok(AsyncInteractiveReadbackJob { receiver })
}

fn poll_interactive_readback_job(
    job: &mut Option<AsyncInteractiveReadbackJob>,
) -> Option<Result<AsyncInteractiveReadbackResult, String>> {
    let pending = job.take()?;
    match pending.receiver.try_recv() {
        Ok(result) => Some(result),
        Err(mpsc::TryRecvError::Empty) => {
            *job = Some(pending);
            None
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            Some(Err("interactive readback worker disconnected".to_owned()))
        }
    }
}

fn finish_interactive_readback_job_before_report(
    job: &mut Option<AsyncInteractiveReadbackJob>,
    timeout: Duration,
) -> Option<Result<AsyncInteractiveReadbackResult, String>> {
    let pending = job.take()?;
    match pending.receiver.recv_timeout(timeout) {
        Ok(result) => Some(result),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            *job = Some(pending);
            None
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Some(Err("interactive readback worker disconnected".to_owned()))
        }
    }
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

fn native_target_frame_interval() -> Duration {
    Duration::from_micros((NATIVE_TARGET_FRAME_INTERVAL_MS * 1000.0).round() as u64)
}

fn wait_for_forced_sample_frame_pacing(next_frame_start: &mut Instant, enabled: bool) -> f64 {
    let mut wait_ms = 0.0;
    let now = Instant::now();
    if enabled
        && let Some(wait_duration) = next_frame_start.checked_duration_since(now)
        && !wait_duration.is_zero()
    {
        let wait_started = Instant::now();
        std::thread::sleep(wait_duration);
        wait_ms = elapsed_ms(wait_started);
    }
    let frame_started = Instant::now();
    *next_frame_start = frame_started + native_target_frame_interval();
    wait_ms
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

fn push_perf_sample(samples: &mut VecDeque<f64>, sample: Option<f64>) {
    let Some(sample) = sample else {
        return;
    };
    if !sample.is_finite() {
        return;
    }
    if samples.len() == PREVIEW_PERF_STATS_WINDOW {
        samples.pop_front();
    }
    samples.push_back(sample.max(0.0));
}

fn metric_summary_from_samples(samples: &VecDeque<f64>) -> NativePerfMetricSummary {
    if samples.is_empty() {
        return NativePerfMetricSummary {
            p50: None,
            p95: None,
            p99: None,
            max: None,
            sample_count: 0,
        };
    }
    let values = samples.iter().copied().collect::<Vec<_>>();
    NativePerfMetricSummary {
        p50: Some(percentile(&values, 0.50)),
        p95: Some(percentile(&values, 0.95)),
        p99: Some(percentile(&values, 0.99)),
        max: values.iter().copied().reduce(f64::max),
        sample_count: values.len(),
    }
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
            event_elapsed_ms: None,
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
    hold_started: Instant,
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
            event_elapsed_ms: event
                .occurred_at
                .checked_duration_since(hold_started)
                .map(|elapsed| elapsed.as_secs_f64() * 1000.0),
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
    base.mouse_buttons_down = sample.mouse_buttons_down.clone();
    base.pressed_keys = sample.pressed_keys.clone();
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

fn should_accept_input_cursor_after_poll(
    host_input_observed_this_turn: bool,
    input: &NativeInputAdapterProof,
    poll_result: &NativePollResult,
) -> bool {
    !(host_input_observed_this_turn
        && native_input_delta_is_button_press_only(input)
        && !poll_result.dirty)
}

fn accepted_host_input_event_summary(
    input: &NativeInputAdapterProof,
    input_event_wake_count: u64,
    wake_elapsed_ms: Option<f64>,
    accepted_elapsed_ms: f64,
    semantic_hint: Option<&NativeHostInputEventHint>,
) -> NativeHostInputEventSummary {
    let hinted_button = semantic_hint.and_then(|hint| {
        if hint.kind != "mouse_button" {
            return None;
        }
        hint.sequence
            .and_then(|sequence| {
                input
                    .mouse_button_events
                    .iter()
                    .find(|event| event.sequence == sequence)
            })
            .or_else(|| {
                input.mouse_button_events.iter().find(|event| {
                    hint.button
                        .as_deref()
                        .is_none_or(|button| event.button == button)
                        && hint.pressed.is_none_or(|pressed| event.pressed == pressed)
                })
            })
    });
    let hinted_key = semantic_hint.and_then(|hint| {
        if hint.kind != "keyboard_key" {
            return None;
        }
        hint.sequence
            .and_then(|sequence| {
                input
                    .keyboard_events
                    .iter()
                    .find(|event| event.sequence == sequence)
            })
            .or_else(|| {
                input.keyboard_events.iter().find(|event| {
                    hint.key.as_deref().is_none_or(|key| event.key == key)
                        && hint.pressed.is_none_or(|pressed| event.pressed == pressed)
                })
            })
    });
    let latest_button = input
        .mouse_button_events
        .iter()
        .max_by_key(|event| event.sequence);
    let latest_key = input
        .keyboard_events
        .iter()
        .max_by_key(|event| event.sequence);
    let has_scroll_delta = input.scroll_delta_x != 0.0 || input.scroll_delta_y != 0.0;
    let has_motion_delta = input.real_os_events_observed
        && latest_button.is_none()
        && latest_key.is_none()
        && !has_scroll_delta
        && input.mouse_window_pos.is_some();

    let (kind, sequence, window_protocol_id, button, pressed, key, event_elapsed_ms) =
        if let Some(event) = hinted_button {
            (
                "mouse_button",
                Some(event.sequence),
                event.window_protocol_id,
                Some(event.button.clone()),
                Some(event.pressed),
                None,
                event.event_elapsed_ms,
            )
        } else if let Some(event) = hinted_key {
            (
                "keyboard_key",
                Some(event.sequence),
                event.window_protocol_id,
                None,
                Some(event.pressed),
                Some(event.key.clone()),
                None,
            )
        } else if let Some(hint) = semantic_hint {
            (
                hint.kind.as_str(),
                hint.sequence,
                hint.window_protocol_id,
                hint.button.clone(),
                hint.pressed,
                hint.key.clone(),
                hint.event_elapsed_ms,
            )
        } else if let Some(event) = latest_button {
            (
                "mouse_button",
                Some(event.sequence),
                event.window_protocol_id,
                Some(event.button.clone()),
                Some(event.pressed),
                None,
                event.event_elapsed_ms,
            )
        } else if let Some(event) = latest_key {
            (
                "keyboard_key",
                Some(event.sequence),
                event.window_protocol_id,
                None,
                Some(event.pressed),
                Some(event.key.clone()),
                None,
            )
        } else if has_scroll_delta {
            (
                "mouse_scroll",
                Some(input.mouse_scroll_event_count),
                input.mouse_last_window_protocol_id,
                None,
                None,
                None,
                None,
            )
        } else if has_motion_delta {
            (
                "mouse_motion",
                Some(input.mouse_motion_event_count),
                input.mouse_last_window_protocol_id,
                None,
                None,
                None,
                None,
            )
        } else {
            (
                "raw_host_input_wake",
                Some(input_event_wake_count),
                input
                    .mouse_last_window_protocol_id
                    .or(input.keyboard_last_window_protocol_id),
                None,
                None,
                None,
                None,
            )
        };
    let accepted_event_elapsed_ms = event_elapsed_ms.or(wake_elapsed_ms);

    NativeHostInputEventSummary {
        scope: "accepted_host_input_delta".to_owned(),
        kind: kind.to_owned(),
        source_intent: semantic_hint.and_then(|hint| hint.source_intent.clone()),
        sequence,
        input_event_wake_count,
        wake_elapsed_ms: accepted_event_elapsed_ms,
        raw_wake_elapsed_ms: wake_elapsed_ms,
        event_elapsed_ms,
        accepted_elapsed_ms: Some(accepted_elapsed_ms),
        wake_to_accept_ms: elapsed_delta_ms(accepted_event_elapsed_ms, Some(accepted_elapsed_ms)),
        event_to_accept_ms: elapsed_delta_ms(event_elapsed_ms, Some(accepted_elapsed_ms)),
        window_protocol_id,
        button,
        pressed,
        key,
        mouse_button_delta_count: input.mouse_button_events.len(),
        keyboard_delta_count: input.keyboard_events.len(),
        has_scroll_delta,
        has_motion_delta,
    }
}

fn native_input_delta_is_button_press_only(input: &NativeInputAdapterProof) -> bool {
    let has_press = input.mouse_button_events.iter().any(|event| event.pressed);
    let has_release = input.mouse_button_events.iter().any(|event| !event.pressed);
    has_press
        && !has_release
        && input.keyboard_events.is_empty()
        && input.scroll_delta_x == 0.0
        && input.scroll_delta_y == 0.0
}

fn native_input_delta_has_reportable_host_event(input: &NativeInputAdapterProof) -> bool {
    !input.mouse_button_events.is_empty()
        || !input.keyboard_events.is_empty()
        || input.scroll_delta_x != 0.0
        || input.scroll_delta_y != 0.0
        || native_input_delta_is_pointer_motion_only(input)
}

fn native_input_delta_is_pointer_motion_only(input: &NativeInputAdapterProof) -> bool {
    input.real_os_events_observed
        && input.mouse_button_events.is_empty()
        && input.keyboard_events.is_empty()
        && input.mouse_buttons_down.is_empty()
        && input.pressed_keys.is_empty()
        && input.scroll_delta_x == 0.0
        && input.scroll_delta_y == 0.0
        && input.mouse_window_pos.is_some()
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

    fn test_readback_artifact(key: FrameEvidenceKey, sequence: u64) -> AppWindowReadbackArtifact {
        AppWindowReadbackArtifact {
            path: format!("artifact-{sequence}.png"),
            sha256: format!("sha-{sequence}"),
            width: 8,
            height: 8,
            presented_revision: Some(key.frame_seq),
            content_revision: Some(key.content_revision),
            rendered_frame_count: Some(key.frame_seq),
            frame_evidence_key: Some(key),
            capture_method: "wgpu-visible-surface-copy-src-readback".to_owned(),
            texture_format: "Rgba8Unorm".to_owned(),
            nonblank_samples: 1,
            unique_rgba_values: 1,
            readback_deadline_ms: 5_000,
            readback_poll_status: "completed_before_deadline".to_owned(),
        }
    }

    #[test]
    fn merge_input_adapter_proof_keeps_current_button_and_key_state() {
        let mut base = empty_input_adapter_proof(false);

        let mut pressed = empty_input_adapter_proof(false);
        pressed.real_os_events_observed = true;
        pressed.mouse_button_event_count = 2;
        pressed.keyboard_key_event_count = 3;
        pressed.mouse_buttons_down = vec!["left".to_owned()];
        pressed.pressed_keys = vec!["KeyA".to_owned()];
        merge_input_adapter_proof(&mut base, &pressed);

        assert_eq!(base.mouse_button_event_count, 2);
        assert_eq!(base.keyboard_key_event_count, 3);
        assert_eq!(base.mouse_buttons_down, vec!["left".to_owned()]);
        assert_eq!(base.pressed_keys, vec!["KeyA".to_owned()]);

        let mut released = empty_input_adapter_proof(false);
        released.mouse_button_event_count = 4;
        released.keyboard_key_event_count = 5;
        merge_input_adapter_proof(&mut base, &released);

        assert_eq!(base.mouse_button_event_count, 4);
        assert_eq!(base.keyboard_key_event_count, 5);
        assert!(
            base.mouse_buttons_down.is_empty(),
            "current mouse button state must clear after a release/no-buttons sample"
        );
        assert!(
            base.pressed_keys.is_empty(),
            "current keyboard state must clear after a no-keys sample"
        );
    }

    #[test]
    fn recent_history_compacts_visible_bound_text_without_losing_selection_evidence() {
        let mut entries = (0..96)
            .map(|index| {
                serde_json::json!({
                    "node": format!("unrelated-{index}"),
                    "kind": "Text",
                    "text": format!("bulk-{index}"),
                    "text_truncated": false,
                    "paths": [format!("bulk.path.{index}")],
                    "focused": false,
                    "selected": false
                })
            })
            .collect::<Vec<_>>();
        entries.push(serde_json::json!({
            "node": "formula-bar",
            "kind": "TextInput",
            "text": "selected formula",
            "text_truncated": false,
            "paths": ["store.selected_input.editing_text"],
            "focused": false,
            "selected": false
        }));
        entries.push(serde_json::json!({
            "node": "selected-cell",
            "kind": "Text",
            "text": "selected row",
            "text_truncated": false,
            "paths": ["record.selected_label"],
            "focused": true,
            "selected": true
        }));
        let proof = serde_json::json!({
            "visible_bound_text": {
                "status": "pass",
                "source": "layout-frame-current-bound-text",
                "entry_count": entries.len(),
                "entry_limit": 512,
                "truncated": false,
                "entries": entries
            }
        });

        let compact = compact_visible_bound_text_for_recent_history(&proof);
        let compact_entries = compact
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .expect("compact visible text entries");

        assert_eq!(compact["recent_history_compacted"], true);
        assert_eq!(compact["entry_count"], 98);
        assert!(compact_entries.len() <= 64);
        assert!(compact_entries.iter().any(|entry| {
            entry.get("node").and_then(serde_json::Value::as_str) == Some("formula-bar")
                && entry.get("text").and_then(serde_json::Value::as_str) == Some("selected formula")
        }));
        assert!(compact_entries.iter().any(|entry| {
            entry.get("node").and_then(serde_json::Value::as_str) == Some("selected-cell")
                && entry.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
        }));
        assert!(
            !compact_entries.iter().any(|entry| {
                entry.get("node").and_then(serde_json::Value::as_str) == Some("unrelated-95")
            }),
            "bulk unrelated entries should not dominate recent frame history"
        );
    }

    #[test]
    fn recent_history_preserves_render_hook_phase_timings() {
        let key = FrameEvidenceKey {
            frame_seq: 7,
            content_revision: 11,
            layout_revision: 3,
            render_scene_revision: 5,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(13),
            present_id: 17,
            proof_request_id: None,
        };
        let proof = serde_json::json!({
            "status": "pass",
            "render_hook_phase_timings_ms": {
                "encode_scene_ms": 0.42,
                "report_json_ms": 0.11,
                "total_with_report_json_ms": 0.57
            },
            "proof": {
                "status": "pass",
                "capture_method": "wgpu-visible-surface-copy-src-readback"
            }
        });

        let compact = compact_external_render_proof_for_recent_history(Some(&proof), &key);

        assert_eq!(
            compact
                .pointer("/render_hook_phase_timings_ms/encode_scene_ms")
                .and_then(serde_json::Value::as_f64),
            Some(0.42)
        );
        assert_eq!(
            compact
                .pointer("/render_hook_phase_timings_ms/total_with_report_json_ms")
                .and_then(serde_json::Value::as_f64),
            Some(0.57)
        );
        assert_eq!(
            compact.pointer("/proof/frame_evidence_key"),
            Some(&serde_json::to_value(key).expect("frame evidence key serializes"))
        );
    }

    #[test]
    fn post_present_proof_queue_tracks_deferred_and_legacy_requests_by_frame_key() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 7,
            content_revision: 11,
            layout_revision: 3,
            render_scene_revision: 5,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(13),
            present_id: 17,
            proof_request_id: None,
        };
        let requests = vec![
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleBoundText,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            },
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ExternalAppOwnedReadback,
                currently_legacy_pre_present: true,
                frame_local_snapshot_required: true,
            },
        ];

        state.enqueue_post_present_proof_requests(&key, &requests, Some(42.0));

        assert_eq!(state.post_present_proof_queue_enqueued_count, 2);
        assert_eq!(state.post_present_proof_queue_deferred_count, 1);
        assert_eq!(state.post_present_proof_queue_legacy_pre_present_count, 1);
        assert_eq!(state.recent_post_present_proof_queue.len(), 2);
        assert_eq!(
            state.recent_post_present_proof_queue[0].frame_evidence_key,
            key
        );
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::Queued
        );
        assert_eq!(
            state.recent_post_present_proof_queue[1].status,
            NativePostPresentProofQueueStatus::LegacyAlreadyBuiltPrePresent
        );
        assert_eq!(
            state.recent_post_present_proof_queue[0].enqueued_elapsed_ms,
            Some(42.0)
        );
        assert!(state.note_post_present_proof_request_completed(
            &key,
            NativePostPresentProofRequestKind::VisibleBoundText,
            Some(48.0),
        ));
        assert!(!state.note_post_present_proof_request_completed(
            &key,
            NativePostPresentProofRequestKind::ExternalAppOwnedReadback,
            Some(49.0),
        ));
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::CompletedPostPresent
        );
        assert_eq!(
            state.recent_post_present_proof_queue[0].completed_elapsed_ms,
            Some(48.0)
        );
        assert_eq!(
            state.recent_post_present_proof_queue[1].status,
            NativePostPresentProofQueueStatus::LegacyAlreadyBuiltPrePresent
        );
    }

    #[test]
    fn product_frame_commit_adds_visible_surface_readback_request_once() {
        let key = FrameEvidenceKey {
            frame_seq: 7,
            content_revision: 11,
            layout_revision: 3,
            render_scene_revision: 5,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(13),
            present_id: 17,
            proof_request_id: None,
        };
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
            product_frame: Some(NativeRenderedProductFrame {
                schema_version: 1,
                render_target_kind: "visible-surface-direct".to_owned(),
                visible_surface_rendered: true,
                visible_present_path: true,
                layout_identity: Some("layout:1".to_owned()),
                render_scene_identity: Some("scene:1".to_owned()),
                legacy_proof_json_built_pre_present: false,
                legacy_render_hook_proof_built_pre_present: false,
                post_present_proof_request_count: 1,
            }),
            post_present_proof_requests: vec![NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleBoundText,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            ..NativeRenderFrameMetrics::default()
        }));

        let mut commit = product_frame_commit_for_presented_frame(
            &state,
            key.clone(),
            NativeFrameLane::ProductInteraction,
            Some(NativeSchedulerReason::HostInput),
            None,
            None,
        );

        assert_eq!(commit.post_present_proof_request_count, 1);
        add_post_present_proof_request_to_commit(
            &mut commit,
            visible_surface_readback_post_present_request(),
        );
        add_post_present_proof_request_to_commit(
            &mut commit,
            visible_surface_readback_post_present_request(),
        );

        assert_eq!(commit.post_present_proof_request_count, 2);
        assert_eq!(commit.post_present_proof_requests.len(), 2);
        assert_eq!(commit.legacy_pre_present_proof_request_count, 0);
        assert!(commit.post_present_proof_requests.iter().any(|request| {
            request.kind == NativePostPresentProofRequestKind::VisibleSurfaceReadback
                && !request.currently_legacy_pre_present
                && request.frame_local_snapshot_required
        }));
        assert_eq!(
            commit
                .product_frame
                .as_ref()
                .map(|frame| frame.post_present_proof_request_count),
            Some(2)
        );

        let mut queue_state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        queue_state.note_product_frame_commit(commit);
        assert_eq!(queue_state.post_present_proof_queue_enqueued_count, 2);
        assert_eq!(queue_state.post_present_proof_queue_deferred_count, 2);
        assert_eq!(
            queue_state
                .recent_post_present_proof_queue
                .iter()
                .filter(|entry| {
                    entry.request.kind == NativePostPresentProofRequestKind::VisibleSurfaceReadback
                        && entry.frame_evidence_key == key
                })
                .count(),
            1
        );
    }

    #[test]
    fn post_present_proof_subscriber_artifact_completes_matching_queue_request() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 9,
            content_revision: 12,
            layout_revision: 4,
            render_scene_revision: 6,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(15),
            present_id: 19,
            proof_request_id: None,
        };
        state.enqueue_post_present_proof_requests(
            &key,
            &[NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::RetainedBoundSync,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            Some(50.0),
        );

        let subscriber = native_post_present_json_proof_subscriber(
            NativePostPresentProofRequestKind::RetainedBoundSync,
            |context| {
                serde_json::json!({
                    "frame_seq": context.frame_evidence_key.frame_seq,
                    "completed_elapsed_ms": context.completed_elapsed_ms
                })
            },
        );

        run_post_present_proof_subscribers(&mut state, vec![subscriber], &key, Some(51.0));

        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert_eq!(state.post_present_proof_subscriber_error_count, 0);
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::CompletedPostPresent
        );
        assert_eq!(
            state.recent_post_present_proof_queue[0].completed_elapsed_ms,
            Some(51.0)
        );
        let artifact = state
            .recent_post_present_proof_artifacts
            .back()
            .expect("subscriber artifact");
        assert_eq!(
            artifact.kind,
            NativePostPresentProofRequestKind::RetainedBoundSync
        );
        assert_eq!(artifact.frame_evidence_key, key);
        assert_eq!(
            artifact
                .payload
                .pointer("/frame_seq")
                .and_then(|value| value.as_u64()),
            Some(9)
        );
    }

    #[test]
    fn async_post_present_proof_worker_records_keyed_artifact() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 10,
            content_revision: 12,
            layout_revision: 4,
            render_scene_revision: 6,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(16),
            present_id: 20,
            proof_request_id: None,
        };
        state.enqueue_post_present_proof_requests(
            &key,
            &[NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleBoundText,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            Some(50.0),
        );
        let subscriber = native_post_present_json_proof_subscriber(
            NativePostPresentProofRequestKind::VisibleBoundText,
            |context| {
                serde_json::json!({
                    "frame_seq": context.frame_evidence_key.frame_seq,
                    "completed_elapsed_ms": context.completed_elapsed_ms
                })
            },
        );
        let mut worker = AsyncPostPresentProofSubscriberWorker::new();
        let enqueue_report = worker
            .enqueue(vec![subscriber], key.clone(), Some(61.0), Instant::now())
            .expect("non-empty subscriber batch should enqueue");
        state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

        assert_eq!(
            state.post_present_proof_subscriber_worker_enqueued_batch_count,
            1
        );
        assert_eq!(state.post_present_proof_subscriber_worker_enqueued_count, 1);
        assert_eq!(
            state.post_present_proof_subscriber_worker_pending_batch_count,
            1
        );

        worker.shutdown_and_drain(&mut state);

        assert_eq!(
            state.post_present_proof_subscriber_worker_completed_count,
            1
        );
        assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
        assert_eq!(
            state.post_present_proof_subscriber_worker_pending_batch_count,
            0
        );
        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::CompletedPostPresent
        );
        let artifact = state
            .recent_post_present_proof_artifacts
            .back()
            .expect("worker should record artifact");
        assert_eq!(
            artifact.kind,
            NativePostPresentProofRequestKind::VisibleBoundText
        );
        assert_eq!(artifact.frame_evidence_key, key);
        assert_eq!(artifact.completed_elapsed_ms, Some(61.0));
    }

    #[test]
    fn async_post_present_proof_worker_drain_can_be_bounded() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 11,
            content_revision: 12,
            layout_revision: 4,
            render_scene_revision: 6,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(16),
            present_id: 21,
            proof_request_id: None,
        };
        let shared = Arc::new((
            Mutex::new(AsyncPostPresentProofSubscriberShared::default()),
            Condvar::new(),
        ));
        let (sender, results) = mpsc::channel();
        let worker = AsyncPostPresentProofSubscriberWorker {
            shared,
            results,
            worker: None,
        };
        for index in 0..3_u64 {
            sender
                .send(Ok(native_post_present_json_proof_artifact(
                    NativePostPresentProofRequestKind::VisibleBoundText,
                    key.clone(),
                    Some(index as f64),
                    serde_json::json!({ "index": index }),
                )))
                .expect("send proof artifact");
        }

        assert_eq!(worker.drain_completed_limit(&mut state, 2), 2);
        assert_eq!(state.post_present_proof_artifact_count, 2);
        assert_eq!(worker.drain_completed(&mut state), 1);
        assert_eq!(state.post_present_proof_artifact_count, 3);
    }

    #[test]
    fn lagging_post_present_proof_worker_is_reported_without_blocking_product_frame() {
        let dir = std::env::temp_dir().join(format!(
            "boon-post-present-proof-isolation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.json");

        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_poll_started(1.0);
        state.note_accepted_host_input(1, 2.0, false, None);
        state.note_dirty_poll(2.5);
        state.note_render_started(3.0);
        state.note_surface_acquired(3.5);
        state.note_surface_acquire_call(0.5);
        state.note_render_hook_completed(5.0);
        state.note_queue_submitted(5.5);
        state.note_submit_phase_durations(0.2, 0.3, 1.0);
        state.note_present_completed(6.5);
        state.mark_presented_with_revisions(1, 2, 3, 4);
        state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
        state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
            product_frame: Some(NativeRenderedProductFrame {
                schema_version: 1,
                render_target_kind: "visible-surface-direct".to_owned(),
                visible_surface_rendered: true,
                visible_present_path: true,
                layout_identity: Some("layout-proof-isolation".to_owned()),
                render_scene_identity: Some("scene-proof-isolation".to_owned()),
                legacy_proof_json_built_pre_present: false,
                legacy_render_hook_proof_built_pre_present: false,
                post_present_proof_request_count: 1,
            }),
            post_present_proof_requests: vec![NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleBoundText,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            ..NativeRenderFrameMetrics::default()
        }));
        let input_timing = state.take_frame_accepted_input_timing(1);
        let key = frame_evidence_key_for_presented_frame(
            &state,
            &SurfaceId("preview:proof-isolation".to_owned()),
            1,
            Some(1),
            None,
        );
        state.note_product_frame_commit(product_frame_commit_for_presented_frame(
            &state,
            key.clone(),
            NativeFrameLane::ProductInteraction,
            Some(NativeSchedulerReason::HostInput),
            None,
            input_timing,
        ));

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let subscriber: NativePostPresentProofSubscriber = Box::new(move |context| {
            started_tx
                .send(())
                .expect("test should observe blocked subscriber start");
            release_rx
                .recv()
                .expect("test should release blocked proof subscriber");
            Ok(native_post_present_json_proof_artifact(
                NativePostPresentProofRequestKind::VisibleBoundText,
                context.frame_evidence_key.clone(),
                context.completed_elapsed_ms,
                serde_json::json!({
                    "status": "pass",
                    "blocked_worker_test": true,
                    "frame_seq": context.frame_evidence_key.frame_seq
                }),
            ))
        });
        let mut worker = AsyncPostPresentProofSubscriberWorker::new();
        let enqueue_report = worker
            .enqueue(vec![subscriber], key.clone(), Some(7.0), Instant::now())
            .expect("proof subscriber batch should enqueue");
        state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("proof subscriber should be running but blocked");
        worker.drain_completed(&mut state);

        let mut recent_product_frame_commits = VecDeque::new();
        if let Some(commit) = state.last_product_frame_commit.as_ref() {
            push_recent_product_frame_commit(&mut recent_product_frame_commits, commit);
        }
        write_render_loop_state_report(
            &path,
            NativeWindowRole::Preview,
            std::process::id(),
            &WindowId("window-proof-isolation".to_owned()),
            &SurfaceId("preview:proof-isolation".to_owned()),
            &NativeSurfaceLifecycleReport {
                surface_epoch: 1,
                final_width: 1,
                final_height: 1,
                ..NativeSurfaceLifecycleReport::default()
            },
            &state,
            Duration::from_millis(16),
            1,
            None,
            &NativePreviewPerfAccumulator::default(),
            NativeRenderLoopReportExtras {
                present_mode: "Immediate".to_owned(),
                surface_format: "Bgra8Unorm".to_owned(),
                desired_maximum_frame_latency: 1,
                desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
                ..NativeRenderLoopReportExtras::default()
            }
            .with_input_generation(1, 1, Some(0.0), Some(0.0))
            .with_frame_evidence_key(Some(&key))
            .with_recent_product_frame_commits(&recent_product_frame_commits),
            None,
        )
        .unwrap();

        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            report["post_present_proof_isolation"]["status"],
            serde_json::json!("pass")
        );
        assert_eq!(
            report["post_present_proof_isolation"]["product_latency_includes_proof_completion"],
            serde_json::json!(false)
        );
        assert_eq!(
            report["post_present_proof_isolation"]["product_blocks_on_proof_subscribers"],
            serde_json::json!(false)
        );
        assert_eq!(
            report["post_present_proof_isolation"]["proof_worker_status"],
            serde_json::json!("lagging")
        );
        assert_eq!(
            report["post_present_proof_isolation"]["queued_request_count"],
            serde_json::json!(1)
        );
        assert_eq!(report["product_frame_commit_count"], serde_json::json!(1));
        assert_eq!(
            report["post_present_proof_artifact_count"],
            serde_json::json!(0)
        );

        release_tx.send(()).unwrap();
        worker.shutdown_and_drain(&mut state);
        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn async_post_present_proof_worker_completes_history_and_report_requests() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 11,
            content_revision: 13,
            layout_revision: 5,
            render_scene_revision: 7,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(17),
            present_id: 21,
            proof_request_id: None,
        };
        state.enqueue_post_present_proof_requests(
            &key,
            &[
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::ProofHistory,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::RenderHookReportJson,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
            ],
            Some(70.0),
        );
        let mut worker = AsyncPostPresentProofSubscriberWorker::new();
        let enqueue_report = worker
            .enqueue(
                vec![
                    proof_history_post_present_subscriber(3),
                    render_hook_report_json_post_present_subscriber(
                        "target/reports/native-gpu/loop.json".to_owned(),
                        0.42,
                    ),
                ],
                key.clone(),
                Some(72.0),
                Instant::now(),
            )
            .expect("history and report subscriber batch should enqueue");
        state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

        worker.shutdown_and_drain(&mut state);

        assert_eq!(
            state.post_present_proof_subscriber_worker_completed_count,
            2
        );
        assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
        assert_eq!(state.post_present_proof_artifact_count, 2);
        assert_eq!(state.post_present_proof_queue_completed_count, 2);
        assert!(
            state.recent_post_present_proof_queue.iter().all(
                |entry| entry.status == NativePostPresentProofQueueStatus::CompletedPostPresent
            )
        );
        let artifacts: Vec<_> = state.recent_post_present_proof_artifacts.iter().collect();
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == NativePostPresentProofRequestKind::ProofHistory
                && artifact.frame_evidence_key == key
                && artifact.payload["recent_frame_evidence_count"] == serde_json::json!(3)
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == NativePostPresentProofRequestKind::RenderHookReportJson
                && artifact.frame_evidence_key == key
                && artifact.payload["report_snapshot_enqueued"] == serde_json::json!(true)
                && artifact.payload["report_path"]
                    == serde_json::json!("target/reports/native-gpu/loop.json")
        }));
    }

    #[test]
    fn async_post_present_proof_worker_completes_artifact_hash_request() {
        let dir = std::env::temp_dir().join(format!(
            "boon-post-present-artifact-hash-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let artifact_path = dir.join("proof-artifact.bin");
        std::fs::write(&artifact_path, b"post-present artifact hash").unwrap();
        let expected_sha256 = sha256_file(&artifact_path).unwrap();

        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let key = FrameEvidenceKey {
            frame_seq: 12,
            content_revision: 14,
            layout_revision: 6,
            render_scene_revision: 8,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 2,
            input_event_seq: Some(18),
            present_id: 22,
            proof_request_id: None,
        };
        state.enqueue_post_present_proof_requests(
            &key,
            &[NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ArtifactHash,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            Some(80.0),
        );
        let mut worker = AsyncPostPresentProofSubscriberWorker::new();
        let enqueue_report = worker
            .enqueue(
                vec![native_post_present_artifact_hash_subscriber(vec![
                    artifact_path.display().to_string(),
                ])],
                key.clone(),
                Some(82.0),
                Instant::now(),
            )
            .expect("artifact hash subscriber should enqueue");
        state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

        worker.shutdown_and_drain(&mut state);

        assert_eq!(
            state.post_present_proof_subscriber_worker_completed_count,
            1
        );
        assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::CompletedPostPresent
        );
        let artifact = state
            .recent_post_present_proof_artifacts
            .back()
            .expect("artifact hash worker should record artifact");
        assert_eq!(
            artifact.kind,
            NativePostPresentProofRequestKind::ArtifactHash
        );
        assert_eq!(artifact.frame_evidence_key, key);
        assert_eq!(
            artifact.payload["artifact_hash_status"],
            serde_json::json!("hashed_registered_artifacts")
        );
        assert_eq!(artifact.payload["registered_artifact_count"], 1);
        assert_eq!(artifact.payload["hashed_artifact_count"], 1);
        assert_eq!(
            artifact.payload["artifact_sha256s"][0]["sha256"],
            serde_json::json!(expected_sha256)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn artifact_hash_paths_are_collected_for_exact_frame_artifacts() {
        let dir = std::env::temp_dir().join(format!(
            "boon-post-present-artifact-paths-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let readback_path = dir.join("readback.bin");
        let external_path = dir.join("external.bin");
        let stale_path = dir.join("stale.bin");
        std::fs::write(&readback_path, b"readback artifact").unwrap();
        std::fs::write(&external_path, b"external artifact").unwrap();
        std::fs::write(&stale_path, b"stale artifact").unwrap();

        let key = FrameEvidenceKey {
            frame_seq: 21,
            content_revision: 23,
            layout_revision: 25,
            render_scene_revision: 27,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 3,
            input_event_seq: Some(29),
            present_id: 31,
            proof_request_id: None,
        };
        let stale_key = FrameEvidenceKey {
            frame_seq: 20,
            content_revision: 22,
            layout_revision: 24,
            render_scene_revision: 26,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 3,
            input_event_seq: Some(28),
            present_id: 30,
            proof_request_id: None,
        };
        let mut current_artifact = test_readback_artifact(key.clone(), 1);
        current_artifact.path = readback_path.display().to_string();
        let mut stale_artifact = test_readback_artifact(stale_key.clone(), 2);
        stale_artifact.path = stale_path.display().to_string();
        let mut recent_artifacts = VecDeque::new();
        recent_artifacts.push_back(stale_artifact.clone());
        recent_artifacts.push_back(current_artifact.clone());
        let external_proof = serde_json::json!({
            "status": "pass",
            "proof": {
                "status": "pass",
                "artifact": {
                    "kind": "app_owned_pixels",
                    "artifact_path": external_path.display().to_string(),
                    "artifact_sha256": "0".repeat(64),
                    "capture_method": "wgpu-visible-surface-copy-src-readback",
                    "surface_id": key.surface_id.clone(),
                    "surface_epoch": key.surface_epoch,
                    "frame_seq": key.frame_seq
                },
                "stale_artifact": {
                    "kind": "app_owned_pixels",
                    "artifact_path": stale_path.display().to_string(),
                    "artifact_sha256": "1".repeat(64),
                    "capture_method": "wgpu-visible-surface-copy-src-readback",
                    "frame_evidence_key": stale_key.clone()
                }
            }
        });

        let artifact_paths = post_present_artifact_hash_paths_for_frame(
            &key,
            Some(&current_artifact),
            &recent_artifacts,
            Some(&external_proof),
        );

        assert_eq!(artifact_paths.len(), 2);
        assert!(artifact_paths.contains(&readback_path.display().to_string()));
        assert!(artifact_paths.contains(&external_path.display().to_string()));
        assert!(!artifact_paths.contains(&stale_path.display().to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_artifact_hash_is_deferred_while_exact_readback_is_pending() {
        let key = FrameEvidenceKey {
            frame_seq: 30,
            content_revision: 31,
            layout_revision: 32,
            render_scene_revision: 33,
            surface_id: SurfaceId("preview:test".to_owned()),
            surface_epoch: 4,
            input_event_seq: Some(34),
            present_id: 35,
            proof_request_id: None,
        };
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.enqueue_post_present_proof_requests(
            &key,
            &[
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::ArtifactHash,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
            ],
            Some(90.0),
        );

        assert!(should_defer_empty_artifact_hash_for_pending_readback(
            &[],
            &state,
            &key,
            true
        ));
        assert!(!should_defer_empty_artifact_hash_for_pending_readback(
            &["target/artifacts/native-gpu/frames/already-ready.png".to_owned()],
            &state,
            &key,
            true
        ));
        assert!(!should_defer_empty_artifact_hash_for_pending_readback(
            &[],
            &state,
            &key,
            false
        ));

        let mut no_readback_state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        no_readback_state.enqueue_post_present_proof_requests(
            &key,
            &[NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ArtifactHash,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            Some(90.0),
        );
        assert!(!should_defer_empty_artifact_hash_for_pending_readback(
            &[],
            &no_readback_state,
            &key,
            true
        ));
    }

    #[test]
    fn post_present_subscriber_drain_yields_to_pending_host_input() {
        assert!(post_present_subscriber_drain_allowed(3, 3));
        assert!(post_present_subscriber_drain_allowed(2, 3));
        assert!(!post_present_subscriber_drain_allowed(4, 3));
    }

    #[test]
    fn recent_history_preserves_poll_input_timing_samples() {
        let diagnostics = serde_json::json!({
            "kind": "preview_role_poll",
            "dirty": true,
            "accessibility_snapshot_status": "deferred_product_input",
            "phase_timings_ms": {
                "source_input_ms": 5.0,
                "passive_hover_ms": 0.25
            },
            "recent_native_input_timing_samples": [
                {
                    "fast_path": "simple_source_click",
                    "total_ms": 1.25,
                    "resolve_source": "cached_click_candidate"
                }
            ],
            "native_input_reject_counts": {}
        });

        let compact = compact_poll_diagnostics_for_recent_history(Some(&diagnostics));

        assert_eq!(
            compact
                .pointer("/accessibility_snapshot_status")
                .and_then(serde_json::Value::as_str),
            Some("deferred_product_input")
        );
        assert_eq!(
            compact
                .pointer("/recent_native_input_timing_samples/0/fast_path")
                .and_then(serde_json::Value::as_str),
            Some("simple_source_click")
        );
        assert_eq!(
            compact
                .pointer("/phase_timings_ms/source_input_ms")
                .and_then(serde_json::Value::as_f64),
            Some(5.0)
        );
    }

    #[test]
    fn render_loop_report_bytes_replace_existing_file_atomically() {
        let dir = std::env::temp_dir().join(format!(
            "boon-native-report-atomic-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.json");

        write_atomic_report_bytes(&path, br#"{"old":true}"#).unwrap();
        write_atomic_report_bytes(&path, br#"{"new":true}"#).unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"new":true}"#);
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "atomic report writes must not leave temp files on success: {leftovers:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn async_render_loop_report_writer_flushes_latest_report_on_shutdown() {
        let dir = std::env::temp_dir().join(format!(
            "boon-native-report-async-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.json");
        let writer = AsyncRenderLoopReportWriter::new();

        writer.enqueue(
            test_render_loop_report_snapshot(&path, 1, None),
            Instant::now(),
        );
        writer.enqueue(
            test_render_loop_report_snapshot(
                &path,
                7,
                Some(AsyncRenderLoopReportStats {
                    enqueued_count: 2,
                    ..AsyncRenderLoopReportStats::default()
                }),
            ),
            Instant::now(),
        );
        let stats = writer.shutdown();

        assert!(
            stats.completed_count >= 1,
            "async report writer should flush at least the latest pending report: {stats:?}"
        );
        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(report["status"], "pass");
        assert_eq!(report["rendered_frame_count"], 7);
        assert_eq!(
            report["render_loop_report_write_mode"],
            "async_latest_wins_atomic_replace"
        );
        assert_eq!(report["render_loop_report_async_enqueued_count"], 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats() {
        let dir = std::env::temp_dir().join(format!(
            "boon-native-report-frame-input-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.json");

        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_poll_started(18.0);
        state.note_accepted_host_input(3, 20.0, false, None);
        state.note_dirty_poll(21.0);
        state.note_render_started(22.0);
        state.note_surface_acquired(22.5);
        state.note_surface_acquire_call(0.5);
        state.note_render_hook_completed(24.0);
        state.note_queue_submitted(26.0);
        state.note_submit_phase_durations(0.2, 0.3, 1.5);
        state.note_present_completed(27.5);
        state.mark_presented_with_revisions(1, 2, 3, 4);
        state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
        let frame_input_to_present_ms = state.take_frame_accepted_input_to_present_ms(3);
        state.note_dirty_poll(140.0);
        state.note_render_started(150.0);
        state.note_surface_acquired(151.0);
        state.note_surface_acquire_call(5.0);
        state.note_render_hook_completed(152.0);
        state.note_queue_submitted(153.0);
        state.note_submit_phase_durations(6.0, 7.0, 8.0);
        state.note_present_completed(160.0);
        state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
            layout_ms: Some(0.25),
            product_frame: Some(NativeRenderedProductFrame {
                schema_version: 1,
                render_target_kind: "visible-surface-direct".to_owned(),
                visible_surface_rendered: true,
                visible_present_path: true,
                layout_identity: Some("layout-test".to_owned()),
                render_scene_identity: Some("scene-test".to_owned()),
                legacy_proof_json_built_pre_present: true,
                legacy_render_hook_proof_built_pre_present: true,
                post_present_proof_request_count: 2,
            }),
            post_present_proof_requests: vec![
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::VisibleBoundText,
                    currently_legacy_pre_present: true,
                    frame_local_snapshot_required: true,
                },
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::RenderHookReportJson,
                    currently_legacy_pre_present: true,
                    frame_local_snapshot_required: true,
                },
            ],
            ..NativeRenderFrameMetrics::default()
        }));
        let product_commit_key = frame_evidence_key_for_presented_frame(
            &state,
            &SurfaceId("surface-test".to_owned()),
            1,
            Some(3),
            None,
        );
        state.note_product_frame_commit(product_frame_commit_for_presented_frame(
            &state,
            product_commit_key.clone(),
            NativeFrameLane::ProductInteraction,
            Some(NativeSchedulerReason::HostInput),
            None,
            state.last_accounted_input_frame_timing.clone(),
        ));
        let mut recent_product_frame_commits = VecDeque::new();
        if let Some(commit) = state.last_product_frame_commit.as_ref() {
            push_recent_product_frame_commit(&mut recent_product_frame_commits, commit);
        }
        let mut perf_accumulator = NativePreviewPerfAccumulator::default();
        perf_accumulator.record(
            None,
            None,
            state.last_present_call_ms,
            state.last_surface_acquire_call_ms,
            state.last_queue_submit_call_ms,
            state.last_present_path_ms,
            frame_input_to_present_ms,
            Some(NativeFrameLane::ProductInteraction),
            None,
        );

        write_render_loop_state_report(
            &path,
            NativeWindowRole::Preview,
            std::process::id(),
            &WindowId("window-test".to_owned()),
            &SurfaceId("surface-test".to_owned()),
            &NativeSurfaceLifecycleReport {
                surface_epoch: 1,
                final_width: 1,
                final_height: 1,
                ..NativeSurfaceLifecycleReport::default()
            },
            &state,
            Duration::from_millis(16),
            0,
            None,
            &perf_accumulator,
            NativeRenderLoopReportExtras {
                present_mode: "Immediate".to_owned(),
                surface_format: "Bgra8Unorm".to_owned(),
                desired_maximum_frame_latency: 1,
                desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
                ..NativeRenderLoopReportExtras::default()
            }
            .with_input_generation(3, 3, None, None)
            .with_frame_evidence_key(Some(&product_commit_key))
            .with_recent_product_frame_commits(&recent_product_frame_commits),
            None,
        )
        .unwrap();

        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(report["frame_input_to_present_ms"], serde_json::json!(7.5));
        assert_eq!(report["input_accept_to_present_ms"], serde_json::json!(7.5));
        assert_eq!(
            report["top_level_phase_timing_scope"],
            serde_json::json!("accepted_visible_host_input_frame")
        );
        assert_eq!(
            report["latest_frame_timing_scope"],
            serde_json::json!("latest_presented_frame_raw_last_fields")
        );
        assert_eq!(
            report["input_to_present_accounted_event_wake_count"],
            serde_json::json!(3)
        );
        assert_eq!(
            report["accepted_input_frame_timing"]["timing_scope"],
            serde_json::json!("accepted_visible_host_input_frame")
        );
        assert_eq!(
            report["poll_started_to_input_accept_ms"],
            serde_json::json!(2.0),
            "pre-accept poll/hook work must remain visible separately from accepted product latency"
        );
        assert_eq!(
            report["accepted_input_frame_timing"]["dirty_poll_to_render_started_ms"],
            serde_json::json!(1.0)
        );
        assert_eq!(
            report["dirty_poll_to_render_started_ms"],
            serde_json::json!(1.0),
            "verifier-facing phase fields must describe the accepted input frame, not a later requested-animation frame"
        );
        assert_eq!(
            report["render_hook_completed_to_present_ms"],
            serde_json::json!(3.5)
        );
        assert_eq!(report["queue_to_present_ms"], serde_json::json!(1.5));
        assert_eq!(report["surface_acquire_call_ms"], serde_json::json!(0.5));
        assert_eq!(report["queue_submit_call_ms"], serde_json::json!(0.3));
        assert_eq!(report["present_call_ms"], serde_json::json!(1.5));
        assert_eq!(report["frame_present_call_ms"], serde_json::json!(1.5));
        assert_eq!(report["present_path_ms"], serde_json::json!(2.3));
        assert_eq!(
            report["last_present_call_ms"],
            serde_json::json!(8.0),
            "latest-frame debug fields should still expose the most recent follow-up frame"
        );
        assert_eq!(
            report["preview_perf_stats"]["input_to_present_ms"],
            serde_json::json!(7.5)
        );
        assert_eq!(
            report["preview_perf_stats"]["input_to_present_ms_p50_p95_p99_max"]["p95"],
            serde_json::json!(7.5)
        );
        assert_eq!(
            report["preview_perf_stats"]["frame_lane"],
            serde_json::json!("product_interaction")
        );
        assert_eq!(
            report["preview_perf_stats"]["product_input_to_present_ms_p50_p95_p99_max"]["p95"],
            serde_json::json!(7.5)
        );
        assert_eq!(
            report["preview_perf_stats"]["product_missed_frame_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["last_product_render_frame"]["render_target_kind"],
            serde_json::json!("visible-surface-direct")
        );
        assert_eq!(
            report["last_product_render_frame"]["legacy_proof_json_built_pre_present"],
            serde_json::json!(true)
        );
        assert_eq!(
            report["post_present_proof_request_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["legacy_pre_present_proof_request_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["legacy_product_proof_built_pre_present"],
            serde_json::json!(true)
        );
        assert_eq!(report["product_frame_commit_count"], serde_json::json!(1));
        assert_eq!(
            report["recent_product_frame_commit_count"],
            serde_json::json!(1)
        );
        assert_eq!(
            report["product_frame_commit_matches_frame_evidence_key"],
            serde_json::json!(true)
        );
        assert_eq!(
            report["last_product_frame_commit"]["commit_source"],
            serde_json::json!("app_window_product_frame_commit")
        );
        assert_eq!(
            report["last_product_frame_commit"]["frame_lane"],
            serde_json::json!("product_interaction")
        );
        assert_eq!(
            report["last_product_frame_commit"]["input_to_present_ms"],
            serde_json::json!(7.5)
        );
        assert_eq!(
            report["last_product_frame_commit"]["post_present_proof_request_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["last_product_frame_commit"]["legacy_pre_present_proof_request_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["post_present_proof_queue_enqueued_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["post_present_proof_queue_deferred_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["post_present_proof_queue_legacy_pre_present_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["post_present_proof_queue_completed_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["post_present_subscriber_drain_deferred_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["last_post_present_subscriber_drain_deferred_reason"],
            serde_json::Value::Null
        );
        assert_eq!(
            report["post_present_proof_artifact_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["post_present_proof_subscriber_error_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["recent_post_present_proof_artifact_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            report["recent_post_present_proof_queue_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            report["recent_post_present_proof_queue"][0]["frame_evidence_key"],
            report["frame_evidence_key"]
        );
        assert_eq!(
            report["recent_post_present_proof_queue"][0]["status"],
            serde_json::json!("legacy_already_built_pre_present")
        );
        assert_eq!(
            report["recent_product_frame_commits"][0]["frame_evidence_key"],
            report["frame_evidence_key"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn test_render_loop_report_snapshot(
        path: &Path,
        rendered_frame_count: u64,
        writer_stats: Option<AsyncRenderLoopReportStats>,
    ) -> NativeRenderLoopReportSnapshot {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.dirty_revision = rendered_frame_count;
        state.presented_revision = rendered_frame_count;
        state.rendered_frame_count = rendered_frame_count;
        state.last_render_content_revision = rendered_frame_count;
        state.last_render_layout_revision = rendered_frame_count;
        state.last_render_scene_revision = rendered_frame_count;
        state.last_present_call_ms = Some(1.0);
        let mut extras = NativeRenderLoopReportExtras {
            present_mode: "Immediate".to_owned(),
            surface_format: "Bgra8Unorm".to_owned(),
            desired_maximum_frame_latency: 1,
            desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
            ..NativeRenderLoopReportExtras::default()
        };
        extras = extras.with_report_writer_stats(writer_stats);
        let mut perf_accumulator = NativePreviewPerfAccumulator::default();
        perf_accumulator.record(
            None,
            None,
            state.last_present_call_ms,
            state.last_surface_acquire_call_ms,
            state.last_queue_submit_call_ms,
            state.last_present_path_ms,
            None,
            None,
            None,
        );
        render_loop_report_snapshot(
            path,
            NativeWindowRole::Preview,
            std::process::id(),
            &WindowId("window-test".to_owned()),
            &SurfaceId("surface-test".to_owned()),
            &NativeSurfaceLifecycleReport {
                surface_epoch: 1,
                final_width: 1,
                final_height: 1,
                ..NativeSurfaceLifecycleReport::default()
            },
            &state,
            Duration::from_millis(16),
            0,
            None,
            &perf_accumulator,
            extras,
            None,
        )
    }

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
    fn demand_driven_idle_wait_uses_frame_class_passive_input_poll() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        assert_eq!(state.idle_wait_timeout(now), PASSIVE_INPUT_POLL_INTERVAL);

        state.schedule_wake_after(now, Duration::from_millis(30));
        assert_eq!(state.idle_wait_timeout(now), Duration::from_millis(30));

        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);
        state.schedule_wake_after(now, Duration::from_millis(4));
        assert_eq!(state.idle_wait_timeout(now), Duration::from_millis(4));
    }

    #[test]
    fn demand_driven_idle_gap_before_host_input_is_not_a_missed_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(10.0);

        state.mark_dirty(NativeSchedulerReason::HostInput, None);
        state.note_present_completed(160.0);

        assert_eq!(
            state.last_present_interval_ms,
            Some(150.0),
            "DemandDriven still reports the idle gap for diagnostics"
        );
        assert!(
            state.last_frame_lateness_ms.unwrap_or_default() > NATIVE_TARGET_FRAME_INTERVAL_MS,
            "the interval is late relative to continuous pacing, but DemandDriven idle is allowed"
        );
        assert_eq!(
            state.missed_frame_count, 0,
            "a long healthy DemandDriven idle gap before new host input is not a dropped frame"
        );
        assert_eq!(state.last_missed_frame_cause, None);
    }

    #[test]
    fn requested_animation_followup_gap_counts_as_missed_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(10.0);

        state.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
        state.note_present_completed(60.0);

        assert_eq!(state.missed_frame_count, 1);
        assert_eq!(
            state.last_missed_frame_cause.as_deref(),
            Some("requested_animation_present_interval_exceeded_two_frames")
        );
    }

    #[test]
    fn continuous_probe_long_present_gap_counts_as_missed_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::ContinuousProbe);
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(10.0);
        state.note_present_completed(60.0);

        assert_eq!(state.missed_frame_count, 1);
        assert_eq!(
            state.last_missed_frame_cause.as_deref(),
            Some("continuous_probe_present_interval_exceeded_two_frames")
        );
    }

    #[test]
    fn input_resample_counters_distinguish_inline_and_deferred_turns() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

        state.note_input_inline_resample(2);
        assert_eq!(state.input_inline_resample_count, 1);
        assert_eq!(state.input_deferred_resample_count, 0);
        assert_eq!(state.input_inline_resample_event_gap_count, 2);
        assert_eq!(state.input_deferred_resample_event_gap_count, 0);
        assert_eq!(state.last_input_resample_event_gap_count, 2);
        assert_eq!(
            state.last_input_resample_kind.as_deref(),
            Some("inline_before_hook")
        );

        state.note_input_deferred_resample(3);
        assert_eq!(state.input_inline_resample_count, 1);
        assert_eq!(state.input_deferred_resample_count, 1);
        assert_eq!(state.input_inline_resample_event_gap_count, 2);
        assert_eq!(state.input_deferred_resample_event_gap_count, 3);
        assert_eq!(state.last_input_resample_event_gap_count, 3);
        assert_eq!(
            state.last_input_resample_kind.as_deref(),
            Some("deferred_next_loop")
        );

        state.note_input_pre_present_resample(4);
        assert_eq!(state.input_inline_resample_count, 1);
        assert_eq!(state.input_deferred_resample_count, 2);
        assert_eq!(state.input_inline_resample_event_gap_count, 2);
        assert_eq!(state.input_deferred_resample_event_gap_count, 7);
        assert_eq!(state.last_input_resample_event_gap_count, 4);
        assert_eq!(
            state.last_input_resample_kind.as_deref(),
            Some("pre_present_drop")
        );

        state.note_input_post_present_stale_readback_skip(5);
        assert_eq!(state.input_inline_resample_count, 1);
        assert_eq!(state.input_deferred_resample_count, 3);
        assert_eq!(state.input_inline_resample_event_gap_count, 2);
        assert_eq!(state.input_deferred_resample_event_gap_count, 12);
        assert_eq!(state.last_input_resample_event_gap_count, 5);
        assert_eq!(
            state.last_input_resample_kind.as_deref(),
            Some("post_present_stale_readback_skip")
        );
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
    fn elapsed_delta_ms_only_reports_forward_time() {
        assert_eq!(elapsed_delta_ms(Some(10.0), Some(14.5)), Some(4.5));
        assert_eq!(elapsed_delta_ms(Some(14.5), Some(10.0)), None);
        assert_eq!(elapsed_delta_ms(None, Some(10.0)), None);
        assert_eq!(elapsed_delta_ms(Some(10.0), None), None);
    }

    #[test]
    fn input_event_wake_elapsed_ms_uses_generation_timeline() {
        let hold_started = Instant::now();
        let timeline = Arc::new(Mutex::new(VecDeque::from([
            (1, hold_started + Duration::from_millis(3)),
            (2, hold_started + Duration::from_millis(7)),
        ])));

        assert_eq!(
            input_event_wake_elapsed_ms_for_generation(&timeline, 2, hold_started),
            Some(7.0)
        );
        assert_eq!(
            input_event_wake_elapsed_ms_for_generation(&timeline, 3, hold_started),
            None
        );
    }

    #[test]
    fn frame_evidence_key_tracks_presented_frame_identity() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented_with_revisions(7, 42, 43, 44);
        let surface_id = SurfaceId("surface-test".to_owned());

        let key = frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(3), Some(11));

        assert_eq!(key.frame_seq, 1);
        assert_eq!(key.present_id, 1);
        assert_eq!(key.content_revision, 42);
        assert_eq!(key.layout_revision, 43);
        assert_eq!(key.render_scene_revision, 44);
        assert_eq!(key.surface_id, surface_id);
        assert_eq!(key.surface_epoch, 9);
        assert_eq!(key.input_event_seq, Some(3));
        assert_eq!(key.proof_request_id, Some(11));
    }

    #[test]
    fn frame_evidence_key_can_be_preissued_before_present() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented_with_revisions(7, 42, 43, 44);
        let surface_id = SurfaceId("surface-test".to_owned());

        let preissued = frame_evidence_key_for_next_presented_frame_with_revisions(
            &state,
            &surface_id,
            9,
            Some(5),
            Some(12),
            52,
            53,
            54,
        );
        state.note_preissued_frame_evidence_key(preissued.clone(), 3.5);

        assert_eq!(preissued.frame_seq, 2);
        assert_eq!(preissued.present_id, 2);
        assert_eq!(preissued.content_revision, 52);
        assert_eq!(preissued.layout_revision, 53);
        assert_eq!(preissued.render_scene_revision, 54);
        assert_eq!(
            state.last_preissued_frame_evidence_key.as_ref(),
            Some(&preissued)
        );
        assert_eq!(state.last_preissued_frame_evidence_elapsed_ms, Some(3.5));
        assert!(state.last_frame_evidence_key_issued_before_present);

        state.mark_presented_with_revisions(8, 52, 53, 54);

        let presented =
            frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(5), Some(12));
        assert_eq!(presented, preissued);
    }

    #[test]
    fn recent_interactive_readback_registry_matches_exact_frame_key() {
        let surface_id = SurfaceId("surface-test".to_owned());
        let mut recent = VecDeque::new();
        let mut expected_key = None;

        for index in 0..(RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT + 2) {
            let key = FrameEvidenceKey {
                frame_seq: index as u64,
                content_revision: 100 + index as u64,
                layout_revision: 200 + index as u64,
                render_scene_revision: 300 + index as u64,
                surface_id: surface_id.clone(),
                surface_epoch: 4,
                input_event_seq: Some(index as u64),
                present_id: index as u64,
                proof_request_id: None,
            };
            if index == RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT {
                expected_key = Some(key.clone());
            }
            let artifact = test_readback_artifact(key, index as u64);
            push_recent_interactive_readback_artifact(&mut recent, &artifact);
        }

        assert_eq!(recent.len(), RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT);
        assert_eq!(
            recent
                .front()
                .unwrap()
                .frame_evidence_key
                .as_ref()
                .unwrap()
                .frame_seq,
            2
        );
        let expected_key = expected_key.expect("test should capture a retained key");
        let matched = recent_interactive_readback_artifact_for_frame(
            recent.make_contiguous(),
            Some(&expected_key),
        )
        .expect("registry should find exact frame evidence key");
        assert_eq!(matched.frame_evidence_key.as_ref(), Some(&expected_key));

        let missing_key = FrameEvidenceKey {
            frame_seq: 999,
            content_revision: 999,
            layout_revision: 999,
            render_scene_revision: 999,
            surface_id,
            surface_epoch: 4,
            input_event_seq: Some(999),
            present_id: 999,
            proof_request_id: None,
        };
        assert!(
            recent_interactive_readback_artifact_for_frame(
                recent.make_contiguous(),
                Some(&missing_key),
            )
            .is_none()
        );
    }

    #[test]
    fn completed_interactive_readback_records_visible_surface_post_present_artifact() {
        let key = FrameEvidenceKey {
            frame_seq: 42,
            content_revision: 100,
            layout_revision: 200,
            render_scene_revision: 300,
            surface_id: SurfaceId("surface-test".to_owned()),
            surface_epoch: 4,
            input_event_seq: Some(7),
            present_id: 42,
            proof_request_id: Some(99),
        };
        let artifact = test_readback_artifact(key.clone(), 1);
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.enqueue_post_present_proof_requests(
            &key,
            &[NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                currently_legacy_pre_present: false,
                frame_local_snapshot_required: true,
            }],
            Some(1.0),
        );
        let mut recent = VecDeque::new();
        let mut last = None;
        let mut deferred_artifact_hash_readback_keys = VecDeque::new();

        note_completed_interactive_readback_artifact(
            &mut state,
            &mut recent,
            &mut last,
            &mut deferred_artifact_hash_readback_keys,
            artifact,
            Some(12.5),
        );

        assert_eq!(recent.len(), 1);
        assert_eq!(
            last.as_ref()
                .and_then(|artifact| artifact.frame_evidence_key.as_ref()),
            Some(&key)
        );
        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert_eq!(
            state.recent_post_present_proof_queue[0].status,
            NativePostPresentProofQueueStatus::CompletedPostPresent
        );

        let post_present_artifact = state
            .recent_post_present_proof_artifacts
            .back()
            .expect("completed readback should record a post-present artifact");
        assert_eq!(
            post_present_artifact.kind,
            NativePostPresentProofRequestKind::VisibleSurfaceReadback
        );
        assert_eq!(post_present_artifact.frame_evidence_key, key);
        assert_eq!(post_present_artifact.completed_elapsed_ms, Some(12.5));
        assert_eq!(post_present_artifact.payload["status"], "pass");
        assert_eq!(
            post_present_artifact.payload["capture_method"],
            "wgpu-visible-surface-copy-src-readback"
        );
        assert_eq!(
            post_present_artifact.payload["artifact_path"],
            "artifact-1.png"
        );
        assert_eq!(post_present_artifact.payload["artifact_sha256"], "sha-1");
        assert_eq!(post_present_artifact.payload["frame_seq"], 42);
        assert_eq!(post_present_artifact.payload["input_event_seq"], 7);
        assert_eq!(post_present_artifact.payload["present_id"], 42);
    }

    #[test]
    fn completed_interactive_readback_completes_matching_artifact_hash_request() {
        let key = FrameEvidenceKey {
            frame_seq: 43,
            content_revision: 101,
            layout_revision: 201,
            render_scene_revision: 301,
            surface_id: SurfaceId("surface-test".to_owned()),
            surface_epoch: 4,
            input_event_seq: Some(8),
            present_id: 43,
            proof_request_id: Some(100),
        };
        let artifact = test_readback_artifact(key.clone(), 2);
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.enqueue_post_present_proof_requests(
            &key,
            &[
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::ArtifactHash,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
            ],
            Some(2.0),
        );
        let mut recent = VecDeque::new();
        let mut last = None;
        let mut deferred_artifact_hash_readback_keys = VecDeque::new();
        push_deferred_artifact_hash_readback_key(&mut deferred_artifact_hash_readback_keys, &key);

        note_completed_interactive_readback_artifact(
            &mut state,
            &mut recent,
            &mut last,
            &mut deferred_artifact_hash_readback_keys,
            artifact,
            Some(13.5),
        );

        assert_eq!(recent.len(), 1);
        assert_eq!(state.post_present_proof_artifact_count, 2);
        assert_eq!(state.post_present_proof_queue_completed_count, 2);
        assert!(state.recent_post_present_proof_queue.iter().all(|entry| {
            entry.status == NativePostPresentProofQueueStatus::CompletedPostPresent
                && entry.frame_evidence_key == key
                && entry.completed_elapsed_ms == Some(13.5)
        }));
        let artifact_hash = state
            .recent_post_present_proof_artifacts
            .iter()
            .find(|artifact| artifact.kind == NativePostPresentProofRequestKind::ArtifactHash)
            .expect("completed readback should also record artifact hash proof");
        assert_eq!(artifact_hash.frame_evidence_key, key);
        assert_eq!(
            artifact_hash.payload["artifact_hash_status"],
            serde_json::json!("hashed_registered_artifacts")
        );
        assert_eq!(artifact_hash.payload["registered_artifact_count"], 1);
        assert_eq!(artifact_hash.payload["hashed_artifact_count"], 1);
        assert_eq!(
            artifact_hash.payload["artifact_sha256s"][0]["path"],
            serde_json::json!("artifact-2.png")
        );
        assert_eq!(
            artifact_hash.payload["artifact_sha256s"][0]["sha256"],
            serde_json::json!("sha-2")
        );
        assert_eq!(
            artifact_hash.payload["artifact_sha256s"][0]["source"],
            serde_json::json!("visible_surface_readback_completion")
        );
        assert!(deferred_artifact_hash_readback_keys.is_empty());
    }

    #[test]
    fn completed_interactive_readback_does_not_race_non_deferred_artifact_hash() {
        let key = FrameEvidenceKey {
            frame_seq: 44,
            content_revision: 102,
            layout_revision: 202,
            render_scene_revision: 302,
            surface_id: SurfaceId("surface-test".to_owned()),
            surface_epoch: 4,
            input_event_seq: Some(9),
            present_id: 44,
            proof_request_id: Some(101),
        };
        let artifact = test_readback_artifact(key.clone(), 3);
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.enqueue_post_present_proof_requests(
            &key,
            &[
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
                NativePostPresentProofRequestSummary {
                    kind: NativePostPresentProofRequestKind::ArtifactHash,
                    currently_legacy_pre_present: false,
                    frame_local_snapshot_required: true,
                },
            ],
            Some(3.0),
        );
        let mut recent = VecDeque::new();
        let mut last = None;
        let mut deferred_artifact_hash_readback_keys = VecDeque::new();

        note_completed_interactive_readback_artifact(
            &mut state,
            &mut recent,
            &mut last,
            &mut deferred_artifact_hash_readback_keys,
            artifact,
            Some(14.5),
        );

        assert_eq!(state.post_present_proof_artifact_count, 1);
        assert_eq!(state.post_present_proof_queue_completed_count, 1);
        assert!(
            state
                .recent_post_present_proof_artifacts
                .iter()
                .all(|artifact| artifact.kind != NativePostPresentProofRequestKind::ArtifactHash)
        );
        let artifact_hash_entry = state
            .recent_post_present_proof_queue
            .iter()
            .find(|entry| entry.request.kind == NativePostPresentProofRequestKind::ArtifactHash)
            .expect("artifact hash request remains queued for the real subscriber");
        assert_eq!(
            artifact_hash_entry.status,
            NativePostPresentProofQueueStatus::Queued
        );
    }

    #[test]
    fn external_visible_readback_proof_gets_frame_evidence_key() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented_with_revisions(7, 42, 43, 44);
        let surface_id = SurfaceId("surface-test".to_owned());
        let key = frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(3), None);
        let proof = serde_json::json!({
            "status": "pass",
            "proof": {
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "artifact": {
                    "capture_method": "metadata-only"
                }
            },
            "nested": [
                {
                    "capture_method": "wgpu-visible-surface-copy-src-readback"
                }
            ]
        });

        let enriched =
            external_render_proof_with_frame_evidence_key(Some(&proof), Some(&key)).unwrap();

        assert_eq!(
            enriched.pointer("/proof/frame_evidence_key/frame_seq"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            enriched.pointer("/nested/0/frame_evidence_key/surface_id"),
            Some(&serde_json::json!("surface-test"))
        );
        assert!(
            enriched
                .pointer("/proof/artifact/frame_evidence_key")
                .is_none()
        );
    }

    #[test]
    fn external_visible_readback_proof_replaces_duplicate_interactive_readback() {
        let proof = serde_json::json!({
            "status": "pass",
            "renderer": "boon_native_gpu",
            "proof": {
                "status": "pass",
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "replacement_proof": "render-loop visible surface readback artifact"
            }
        });

        assert!(external_render_proof_replaces_interactive_readback(Some(
            &proof
        )));

        let failing_proof = serde_json::json!({
            "status": "fail",
            "proof": {
                "status": "fail",
                "capture_method": "wgpu-visible-surface-copy-src-readback"
            }
        });
        assert!(!external_render_proof_replaces_interactive_readback(Some(
            &failing_proof
        )));

        let desktop_capture = serde_json::json!({
            "status": "pass",
            "proof": {
                "capture_method": "desktop-screenshot"
            }
        });
        assert!(!external_render_proof_replaces_interactive_readback(Some(
            &desktop_capture
        )));
    }

    #[test]
    fn interactive_surface_readback_is_coalesced_while_previous_proof_is_pending() {
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                true,
                false,
                false,
                false,
                false,
            ),
            InteractiveSurfaceReadbackDecision::Queue
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                true,
                true,
                false,
                false,
                false,
            ),
            InteractiveSurfaceReadbackDecision::SkipExternalProof
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                true,
                false,
                true,
                false,
                false,
            ),
            InteractiveSurfaceReadbackDecision::SkipBackpressure
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Dev,
                true,
                false,
                false,
                false,
                false
            ),
            InteractiveSurfaceReadbackDecision::Queue
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Dev,
                true,
                false,
                true,
                false,
                false
            ),
            InteractiveSurfaceReadbackDecision::SkipBackpressure
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                true,
                false,
                true,
                false,
                true,
            ),
            InteractiveSurfaceReadbackDecision::DeferInteractionBurst
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                false,
                false,
                false,
                false,
                false,
            ),
            InteractiveSurfaceReadbackDecision::Off
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Preview,
                true,
                false,
                false,
                true,
                true,
            ),
            InteractiveSurfaceReadbackDecision::DeferProductInput
        );
        assert_eq!(
            interactive_surface_readback_decision(
                NativeWindowRole::Dev,
                true,
                false,
                true,
                true,
                true
            ),
            InteractiveSurfaceReadbackDecision::DeferProductInput
        );
    }

    #[test]
    fn final_report_drain_completes_pending_interactive_readback() {
        let frame_evidence_key = FrameEvidenceKey {
            frame_seq: 42,
            content_revision: 7,
            layout_revision: 5,
            render_scene_revision: 6,
            surface_id: SurfaceId("surface-test".to_owned()),
            surface_epoch: 1,
            input_event_seq: Some(3),
            present_id: 42,
            proof_request_id: None,
        };
        let artifact = AppWindowReadbackArtifact {
            path: "target/artifacts/native-gpu/frames/test.png".to_owned(),
            sha256: "0".repeat(64),
            width: 4,
            height: 4,
            presented_revision: Some(7),
            content_revision: Some(7),
            rendered_frame_count: Some(42),
            frame_evidence_key: Some(frame_evidence_key.clone()),
            capture_method: "wgpu-visible-surface-copy-src-readback".to_owned(),
            texture_format: "Bgra8UnormSrgb".to_owned(),
            nonblank_samples: 16,
            unique_rgba_values: 2,
            readback_deadline_ms: 5_000,
            readback_poll_status: "completed_before_deadline".to_owned(),
        };
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(AsyncInteractiveReadbackResult {
                artifact,
                finish_ms: 1.0,
                completed_elapsed_ms: 2.0,
            }))
            .unwrap();
        let mut job = Some(AsyncInteractiveReadbackJob { receiver });

        let result =
            finish_interactive_readback_job_before_report(&mut job, Duration::from_millis(1))
                .expect("pending readback should complete before final report")
                .expect("readback result should be ok");

        assert!(job.is_none());
        assert_eq!(
            result.artifact.frame_evidence_key.as_ref(),
            Some(&frame_evidence_key)
        );
        assert_eq!(result.completed_elapsed_ms, 2.0);
    }

    #[test]
    fn final_report_drain_preserves_pending_interactive_readback_on_timeout() {
        let (_sender, receiver) = mpsc::channel();
        let mut job = Some(AsyncInteractiveReadbackJob { receiver });

        let result =
            finish_interactive_readback_job_before_report(&mut job, Duration::from_millis(0));

        assert!(result.is_none());
        assert!(job.is_some());
    }

    #[test]
    fn verifier_readback_backpressure_never_defers_product_rendering() {
        assert!(should_defer_render_for_interactive_readback(
            true,
            true,
            false,
            Some(NativeSchedulerReason::VerifierFrame)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            false,
            false,
            Some(NativeSchedulerReason::VerifierFrame)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            true,
            false,
            Some(NativeSchedulerReason::Timer)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            true,
            false,
            Some(NativeSchedulerReason::RequestedAnimation)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            true,
            true,
            Some(NativeSchedulerReason::HostInput)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            true,
            false,
            Some(NativeSchedulerReason::HostInput)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            false,
            true,
            false,
            Some(NativeSchedulerReason::Timer)
        ));
        assert!(!should_defer_render_for_interactive_readback(
            true,
            false,
            false,
            Some(NativeSchedulerReason::Timer)
        ));
    }

    #[test]
    fn present_path_mode_selection_is_explicit_and_generic() {
        let direct = select_native_present_path_mode(
            true,
            true,
            NativePresentPathMode::DirectVisibleSurface,
            false,
        );
        assert_eq!(
            direct.selected_mode,
            NativePresentPathMode::DirectVisibleSurface
        );
        assert_eq!(direct.reason, "default_direct_visible_surface");

        let direct_with_readback = select_native_present_path_mode(
            true,
            true,
            NativePresentPathMode::DirectVisibleSurface,
            true,
        );
        assert_eq!(
            direct_with_readback.selected_mode,
            NativePresentPathMode::DirectVisibleSurface,
            "offscreen copy-to-present remains explicit because it can regress compositor present latency"
        );
        assert_eq!(
            direct_with_readback.reason,
            "default_direct_visible_surface_with_separate_readback"
        );

        let direct_readback_without_copy_dst = select_native_present_path_mode(
            true,
            false,
            NativePresentPathMode::DirectVisibleSurface,
            true,
        );
        assert_eq!(
            direct_readback_without_copy_dst.selected_mode,
            NativePresentPathMode::DirectVisibleSurface
        );
        assert_eq!(
            direct_readback_without_copy_dst.reason,
            "default_direct_visible_surface_with_separate_readback"
        );

        let direct_readback_without_hook = select_native_present_path_mode(
            false,
            true,
            NativePresentPathMode::DirectVisibleSurface,
            true,
        );
        assert_eq!(
            direct_readback_without_hook.selected_mode,
            NativePresentPathMode::DirectVisibleSurface
        );
        assert_eq!(
            direct_readback_without_hook.reason,
            "default_direct_visible_surface_with_separate_readback"
        );

        let no_hook = select_native_present_path_mode(
            false,
            true,
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            false,
        );
        assert_eq!(
            no_hook.selected_mode,
            NativePresentPathMode::DirectVisibleSurface
        );
        assert_eq!(
            no_hook.reason,
            "offscreen_copy_requested_without_render_hook"
        );

        let no_copy_support = select_native_present_path_mode(
            true,
            false,
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            false,
        );
        assert_eq!(
            no_copy_support.selected_mode,
            NativePresentPathMode::DirectVisibleSurface
        );
        assert_eq!(
            no_copy_support.reason,
            "offscreen_copy_requested_without_surface_copy_dst"
        );

        let offscreen = select_native_present_path_mode(
            true,
            true,
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            false,
        );
        assert_eq!(
            offscreen.selected_mode,
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent
        );
        assert_eq!(offscreen.reason, "explicit_offscreen_copy_to_present");
        assert_eq!(
            offscreen.selected_mode.render_target_kind(),
            "app-owned-offscreen-copy-to-present"
        );
    }

    #[test]
    fn present_path_selection_is_recorded_in_render_loop_state() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let selection = select_native_present_path_mode(
            true,
            true,
            NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            true,
        );

        state.note_present_path_selection(selection);

        assert_eq!(
            state.last_present_path_requested_mode,
            Some(NativePresentPathMode::AppOwnedOffscreenCopyToPresent)
        );
        assert_eq!(
            state.last_present_path_mode,
            Some(NativePresentPathMode::AppOwnedOffscreenCopyToPresent)
        );
        assert_eq!(
            state.last_present_path_selection_reason.as_deref(),
            Some("explicit_offscreen_copy_to_present")
        );
        assert!(state.last_present_path_hooks_present);
        assert!(state.last_present_path_surface_copy_to_present_supported);
        assert!(state.last_present_path_readback_enabled);
    }

    #[test]
    fn preview_perf_stats_keep_proof_overhead_separate_from_ux_latency() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_surface_acquired(4.0);
        state.note_surface_acquire_call(0.25);
        state.note_render_hook_completed(6.5);
        state.note_present_completed(12.0);
        state.note_submit_phase_durations(0.25, 0.125, 1.5);
        state.mark_presented_with_content(2, 5);
        let render_metrics = NativeRenderFrameMetrics {
            layout_ms: Some(0.7),
            upload_bytes: Some(4096),
            draw_call_count: Some(12),
            glyph_cache_hit_rate: Some(0.75),
            materialized_item_count: Some(42),
            visible_display_item_count: Some(40),
            queue_write_count: Some(3),
            preview_blocked_on_ipc_count: Some(0),
            render_hook_outer_state_snapshot_ms: None,
            render_hook_outer_input_snapshot_ms: None,
            render_hook_outer_world_snapshot_ms: None,
            render_hook_outer_core_ms: None,
            render_hook_outer_revision_ms: None,
            render_hook_outer_total_ms: None,
            render_hook_phase_timings: None,
            product_frame: None,
            post_present_proof_requests: Vec::new(),
        };
        state.note_render_frame_metrics(Some(render_metrics.clone()));
        let key = frame_evidence_key_for_presented_frame(
            &state,
            &SurfaceId("surface-test".to_owned()),
            1,
            Some(8),
            None,
        );
        let mut accumulator = NativePreviewPerfAccumulator::default();
        accumulator.record(
            Some(2.5),
            Some(&render_metrics),
            Some(1.5),
            state.last_surface_acquire_call_ms,
            state.last_queue_submit_call_ms,
            state.last_present_path_ms,
            Some(8.0),
            Some(NativeFrameLane::ProductInteraction),
            Some(24.0),
        );

        let stats = native_preview_perf_stats_snapshot(
            NativeWindowRole::Preview,
            &state,
            Duration::from_millis(120),
            60.0,
            &accumulator,
            Some(8.0),
            Some(NativeFrameLane::ProductInteraction),
            "readback",
            Some(24.0),
            Some(key.clone()),
        );

        assert_eq!(stats.render_loop_mode, NativeRenderLoopMode::DemandDriven);
        assert_eq!(stats.input_to_present_ms, Some(8.0));
        assert_eq!(stats.frame_lane, Some(NativeFrameLane::ProductInteraction));
        assert_eq!(stats.product_input_to_present_ms, Some(8.0));
        assert_eq!(stats.proof_overhead_ms, Some(24.0));
        assert_eq!(stats.render_hook_ms, Some(2.5));
        assert_eq!(stats.present_call_ms, Some(1.5));
        assert_eq!(stats.frame_present_call_ms, Some(1.5));
        assert_eq!(stats.surface_acquire_call_ms, Some(0.25));
        assert_eq!(stats.queue_submit_call_ms, Some(0.125));
        assert_eq!(stats.present_path_ms, Some(1.875));
        assert_eq!(stats.input_to_present_ms_p50_p95_p99_max.p95, Some(8.0));
        assert_eq!(
            stats.product_input_to_present_ms_p50_p95_p99_max.p95,
            Some(8.0)
        );
        assert_eq!(stats.product_frame_count, 1);
        assert_eq!(stats.product_missed_frame_count, 0);
        assert_eq!(stats.render_hook_ms_p50_p95_p99_max.sample_count, 1);
        assert_eq!(
            stats.surface_acquire_call_ms_p50_p95_p99_max.p95,
            Some(0.25)
        );
        assert_eq!(stats.queue_submit_call_ms_p50_p95_p99_max.p95, Some(0.125));
        assert_eq!(stats.present_path_ms_p50_p95_p99_max.p95, Some(1.875));
        assert_eq!(stats.layout_ms_p50_p95_p99_max.p95, Some(0.7));
        assert_eq!(stats.upload_bytes_p50_p95_max.max, Some(4096.0));
        assert_eq!(stats.draw_call_count_p50_p95_max.max, Some(12.0));
        assert_eq!(stats.glyph_cache_hit_rate, Some(0.75));
        assert_eq!(stats.materialized_item_count, Some(42));
        assert_eq!(stats.materialized_item_count_p50_p95_max.p95, Some(42.0));
        assert_eq!(stats.proof_overhead_ms_p50_p95_max.max, Some(24.0));
        assert_eq!(stats.frame_evidence_key, Some(key));
    }

    #[test]
    fn accepted_host_input_timing_defines_product_input_to_present_latency() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let host_input_event = NativeHostInputEventSummary {
            scope: "accepted_host_input_delta".to_owned(),
            kind: "mouse_button".to_owned(),
            source_intent: None,
            sequence: Some(17),
            input_event_wake_count: 3,
            wake_elapsed_ms: Some(8.0),
            raw_wake_elapsed_ms: Some(8.0),
            event_elapsed_ms: Some(8.0),
            accepted_elapsed_ms: Some(20.0),
            wake_to_accept_ms: Some(12.0),
            event_to_accept_ms: Some(12.0),
            window_protocol_id: Some(99),
            button: Some("left".to_owned()),
            pressed: Some(false),
            key: None,
            mouse_button_delta_count: 1,
            keyboard_delta_count: 0,
            has_scroll_delta: false,
            has_motion_delta: false,
        };
        state.note_accepted_host_input(3, 20.0, false, Some(host_input_event.clone()));
        state.note_dirty_poll(21.0);
        state.note_render_started(22.0);
        state.note_surface_acquire_call(0.1);
        state.note_surface_acquired(23.0);
        state.note_render_hook_completed(24.0);
        state.note_queue_submitted(26.0);
        state.note_present_completed(28.0);
        state.note_submit_phase_durations(0.2, 0.3, 1.0);

        let raw_wake_elapsed_ms = Some(8.0);
        let raw_wake_to_present_ms =
            elapsed_delta_ms(raw_wake_elapsed_ms, state.last_present_completed_elapsed_ms);
        state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
        let accepted_input_to_present_ms = state.take_frame_accepted_input_to_present_ms(3);
        let mut accumulator = NativePreviewPerfAccumulator::default();
        accumulator.record(
            None,
            None,
            None,
            None,
            None,
            None,
            accepted_input_to_present_ms,
            Some(NativeFrameLane::ProductInteraction),
            None,
        );
        let stats = native_preview_perf_stats_snapshot(
            NativeWindowRole::Preview,
            &state,
            Duration::from_millis(64),
            60.0,
            &accumulator,
            accepted_input_to_present_ms,
            Some(NativeFrameLane::ProductInteraction),
            "off",
            None,
            None,
        );

        assert_eq!(raw_wake_to_present_ms, Some(20.0));
        assert_eq!(accepted_input_to_present_ms, Some(8.0));
        assert_eq!(
            stats.input_to_present_ms,
            Some(8.0),
            "product UX latency starts when the role poll hook accepts visible-changing host input, not at an earlier raw input wake"
        );
        assert_eq!(stats.input_to_present_ms_p50_p95_p99_max.p95, Some(8.0));
        assert_eq!(state.last_accepted_host_input_event_wake_count, 3);
        assert_eq!(state.last_input_to_present_accounted_event_wake_count, 3);
        let accepted_timing = state
            .last_accounted_input_frame_timing
            .as_ref()
            .expect("accepted input timing should be captured once");
        assert_eq!(accepted_timing.input_event_wake_count, 3);
        assert_eq!(
            accepted_timing.host_input_event.as_ref(),
            Some(&host_input_event)
        );
        assert_eq!(accepted_timing.input_wake_elapsed_ms, Some(8.0));
        assert_eq!(accepted_timing.input_wake_to_input_accept_ms, Some(12.0));
        assert_eq!(accepted_timing.input_wake_to_present_ms, Some(20.0));
        assert_eq!(accepted_timing.input_to_present_ms, 8.0);
        assert_eq!(accepted_timing.input_accept_to_dirty_poll_ms, Some(1.0));
        assert_eq!(accepted_timing.dirty_poll_to_render_started_ms, Some(1.0));
        assert_eq!(
            accepted_timing.render_started_to_render_hook_completed_ms,
            Some(2.0)
        );
        assert_eq!(
            accepted_timing.render_hook_completed_to_present_ms,
            Some(4.0)
        );
        assert_eq!(accepted_timing.queue_to_present_ms, Some(2.0));
        assert_eq!(accepted_timing.present_path_ms, Some(1.4));
        assert!(!state.last_accepted_host_input_press_only);

        let key = frame_evidence_key_for_presented_frame(
            &state,
            &SurfaceId("surface-test".to_owned()),
            1,
            Some(3),
            None,
        );
        let commit = product_frame_commit_for_presented_frame(
            &state,
            key,
            NativeFrameLane::ProductInteraction,
            Some(NativeSchedulerReason::HostInput),
            None,
            Some(accepted_timing.clone()),
        );
        assert_eq!(commit.host_input_event.as_ref(), Some(&host_input_event));
        assert_eq!(
            commit
                .input_timing
                .as_ref()
                .and_then(|timing| timing.input_wake_to_present_ms),
            Some(20.0)
        );
    }

    #[test]
    fn accepted_host_input_summary_honors_semantic_press_hint_in_coalesced_batch() {
        let mut input = empty_input_adapter_proof(false);
        input.real_os_events_observed = true;
        input.mouse_button_event_count = 2;
        input.mouse_button_events = vec![
            NativeMouseButtonEventProof {
                sequence: 1,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(9),
                event_elapsed_ms: Some(10.0),
            },
            NativeMouseButtonEventProof {
                sequence: 2,
                button: "left".to_owned(),
                pressed: false,
                window_protocol_id: Some(9),
                event_elapsed_ms: Some(18.0),
            },
        ];
        let hint = NativeHostInputEventHint {
            kind: "mouse_button".to_owned(),
            source_intent: Some("press".to_owned()),
            sequence: Some(1),
            window_protocol_id: Some(9),
            button: Some("left".to_owned()),
            pressed: Some(true),
            key: None,
            event_elapsed_ms: Some(10.0),
        };

        let summary = accepted_host_input_event_summary(&input, 2, Some(18.0), 20.0, Some(&hint));

        assert_eq!(summary.source_intent.as_deref(), Some("press"));
        assert_eq!(summary.sequence, Some(1));
        assert_eq!(summary.pressed, Some(true));
        assert_eq!(summary.event_elapsed_ms, Some(10.0));
        assert_eq!(summary.wake_elapsed_ms, Some(10.0));
        assert_eq!(summary.wake_to_accept_ms, Some(10.0));
        assert_eq!(summary.raw_wake_elapsed_ms, Some(18.0));
        assert_eq!(summary.mouse_button_delta_count, 2);
    }

    #[test]
    fn accepted_host_input_latency_is_frame_scoped_and_single_use() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_accepted_host_input(3, 20.0, false, None);
        state.note_present_completed(27.5);

        assert_eq!(state.take_frame_accepted_input_to_present_ms(2), None);
        assert_eq!(state.take_frame_accepted_input_to_present_ms(3), Some(7.5));
        assert_eq!(state.take_frame_accepted_input_to_present_ms(3), None);

        state.note_present_completed(44.0);
        assert_eq!(
            state.take_frame_accepted_input_to_present_ms(3),
            None,
            "later frames must not keep reusing the old accepted input timestamp"
        );

        state.note_accepted_host_input(4, 45.0, false, None);
        state.note_present_completed(50.0);
        assert_eq!(state.take_frame_accepted_input_to_present_ms(4), Some(5.0));
        assert_eq!(state.last_input_to_present_accounted_event_wake_count, 4);
    }

    #[test]
    fn accepted_host_input_timing_owns_lane_during_requested_animation_burst() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.current_scheduler_reason = Some(NativeSchedulerReason::RequestedAnimation);
        state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);
        state.note_accepted_host_input(7, 10.0, false, None);
        state.note_dirty_poll(10.5);
        state.note_render_started(11.0);
        state.note_surface_acquired(11.5);
        state.note_render_hook_completed(12.0);
        state.note_queue_submitted(13.0);
        state.note_present_completed(14.0);

        let timing = state
            .take_frame_accepted_input_timing(7)
            .expect("accepted host input should define product timing");

        assert_eq!(timing.frame_lane, NativeFrameLane::ProductInteraction);
        assert_eq!(
            timing.scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(timing.input_to_present_ms, 4.0);
    }

    #[test]
    fn accepted_input_frame_timing_is_not_rewritten_by_followup_burst_frames() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_accepted_host_input(7, 100.0, false, None);
        state.note_dirty_poll(101.0);
        state.note_render_started(102.0);
        state.note_surface_acquired(103.0);
        state.note_surface_acquire_call(0.25);
        state.note_render_hook_completed(104.0);
        state.note_queue_submitted(104.0);
        state.note_submit_phase_durations(0.125, 0.5, 2.0);
        state.note_render_target_kind("surface");
        state.note_present_path_selection(NativePresentPathSelection {
            requested_mode: NativePresentPathMode::DirectVisibleSurface,
            selected_mode: NativePresentPathMode::DirectVisibleSurface,
            reason: "test-direct",
            hooks_present: true,
            surface_copy_to_present_supported: false,
            readback_enabled: true,
        });
        state.note_present_completed(106.0);

        let timing = state
            .take_frame_accepted_input_timing(7)
            .expect("input frame should be accounted");
        assert_eq!(timing.input_to_present_ms, 6.0);
        assert_eq!(timing.input_accept_to_dirty_poll_ms, Some(1.0));
        assert_eq!(timing.dirty_poll_to_render_started_ms, Some(1.0));
        assert_eq!(timing.render_started_to_surface_acquired_ms, Some(1.0));
        assert_eq!(timing.render_started_to_render_hook_completed_ms, Some(2.0));
        assert_eq!(
            timing.surface_acquired_to_render_hook_completed_ms,
            Some(1.0)
        );
        assert_eq!(timing.render_hook_completed_to_present_ms, Some(2.0));
        assert_eq!(timing.render_hook_to_queue_ms, Some(0.0));
        assert_eq!(timing.queue_to_present_ms, Some(2.0));
        assert_eq!(timing.surface_acquire_call_ms, Some(0.25));
        assert_eq!(timing.encoder_finish_ms, Some(0.125));
        assert_eq!(timing.queue_submit_call_ms, Some(0.5));
        assert_eq!(timing.present_call_ms, Some(2.0));
        assert_eq!(timing.present_path_ms, Some(2.75));
        assert_eq!(timing.render_target_kind.as_deref(), Some("surface"));
        assert_eq!(
            timing.present_path_mode,
            Some(NativePresentPathMode::DirectVisibleSurface)
        );

        state.note_render_started(140.0);
        state.note_surface_acquired(140.5);
        state.note_surface_acquire_call(3.25);
        state.note_render_hook_completed(141.0);
        state.note_queue_submitted(143.0);
        state.note_submit_phase_durations(3.125, 3.5, 4.0);
        state.note_render_target_kind("offscreen");
        state.note_present_path_selection(NativePresentPathSelection {
            requested_mode: NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            selected_mode: NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
            reason: "test-offscreen",
            hooks_present: true,
            surface_copy_to_present_supported: true,
            readback_enabled: true,
        });
        state.note_present_completed(144.0);

        assert_eq!(state.take_frame_accepted_input_timing(7), None);
        let stored = state
            .last_accounted_input_frame_timing
            .as_ref()
            .expect("accepted input timing should remain available for later reports");
        assert_eq!(stored.input_to_present_ms, 6.0);
        assert_eq!(
            stored.dirty_poll_to_render_started_ms,
            Some(1.0),
            "later requested-animation frames must not make the product UX phase breakdown compare stale dirty-poll time with a newer render start"
        );
        assert_eq!(stored.surface_acquire_call_ms, Some(0.25));
        assert_eq!(stored.encoder_finish_ms, Some(0.125));
        assert_eq!(stored.queue_submit_call_ms, Some(0.5));
        assert_eq!(stored.present_call_ms, Some(2.0));
        assert_eq!(stored.present_path_ms, Some(2.75));
        assert_eq!(stored.render_target_kind.as_deref(), Some("surface"));
        assert_eq!(
            stored.present_path_mode,
            Some(NativePresentPathMode::DirectVisibleSurface)
        );
    }

    #[test]
    fn pre_input_subscriber_drain_skip_is_counted() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

        state.note_pre_input_subscriber_drain_skipped("pending_host_input");
        state.note_pre_input_subscriber_drain_skipped("pending_host_input");

        assert_eq!(state.pre_input_subscriber_drain_skip_count, 2);
        assert_eq!(
            state.last_pre_input_subscriber_drain_skip_reason.as_deref(),
            Some("pending_host_input")
        );
    }

    #[test]
    fn preview_perf_accumulator_keeps_bounded_rolling_summaries() {
        let mut accumulator = NativePreviewPerfAccumulator::default();
        for value in 0..(PREVIEW_PERF_STATS_WINDOW + 10) {
            let render_metrics = NativeRenderFrameMetrics {
                layout_ms: Some((value * 4) as f64),
                upload_bytes: Some((value * 5) as u64),
                draw_call_count: Some((value * 6) as u64),
                glyph_cache_hit_rate: Some(0.5),
                materialized_item_count: Some((value * 7) as u64),
                visible_display_item_count: None,
                queue_write_count: None,
                preview_blocked_on_ipc_count: None,
                render_hook_outer_state_snapshot_ms: None,
                render_hook_outer_input_snapshot_ms: None,
                render_hook_outer_world_snapshot_ms: None,
                render_hook_outer_core_ms: None,
                render_hook_outer_revision_ms: None,
                render_hook_outer_total_ms: None,
                render_hook_phase_timings: None,
                product_frame: None,
                post_present_proof_requests: Vec::new(),
            };
            accumulator.record(
                Some(value as f64),
                Some(&render_metrics),
                Some((value * 2) as f64),
                Some((value * 8) as f64),
                Some((value * 9) as f64),
                Some((value * 19) as f64),
                Some((value * 3) as f64),
                None,
                None,
            );
        }

        let input = accumulator.input_to_present_summary();
        let render = accumulator.render_hook_summary();

        assert_eq!(input.sample_count, PREVIEW_PERF_STATS_WINDOW);
        assert_eq!(render.sample_count, PREVIEW_PERF_STATS_WINDOW);
        assert_eq!(render.p50, Some(70.0));
        assert_eq!(render.max, Some(129.0));
        assert_eq!(input.max, Some(387.0));
        assert_eq!(accumulator.surface_acquire_call_summary().max, Some(1032.0));
        assert_eq!(accumulator.queue_submit_call_summary().max, Some(1161.0));
        assert_eq!(accumulator.present_path_summary().max, Some(2451.0));
        assert_eq!(accumulator.layout_summary().max, Some(516.0));
        assert_eq!(accumulator.upload_bytes_summary().max, Some(645.0));
        assert_eq!(
            accumulator.materialized_item_count_summary().max,
            Some(903.0)
        );
        assert_eq!(accumulator.proof_overhead_summary().sample_count, 0);
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
    fn requested_animation_burst_is_bounded_inside_demand_driven_mode() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);

        assert_eq!(
            native_frame_pacing_snapshot(&state).state,
            NativeFramePacingState::RequestedAnimationBurst
        );
        assert_eq!(
            state.requested_animation_burst_frames_remaining,
            REQUESTED_ANIMATION_BURST_MIN_FRAMES
        );
        assert!(!state.should_render(now, false));

        let due = now + Duration::from_millis(17);
        assert!(state.consume_due_wake(due));
        assert_eq!(
            state.last_scheduler_reason,
            Some(NativeSchedulerReason::RequestedAnimation)
        );
        assert!(state.should_render(due, false));

        let dirty = state.dirty_revision;
        state.mark_presented(dirty);
        state.note_present_completed(27.0);
        state.schedule_requested_animation_followup(due, 27.0);
        assert_eq!(state.requested_animation_burst_frames_remaining, 1);
        assert!(state.next_wake_at.is_some());

        let second_due = due + Duration::from_millis(17);
        assert!(state.consume_due_wake(second_due));
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(44.0);
        state.clear_requested_animation_burst_if_quiet(200.0);
        assert_eq!(
            native_frame_pacing_snapshot(&state).state,
            NativeFramePacingState::Idle
        );
    }

    #[test]
    fn proof_readback_defers_during_product_interaction_burst() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);
        state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);

        assert!(state.interaction_burst_active(11.0));
        assert!(state.defer_proof_readback_for_product_lane(11.0));

        state.note_proof_readback_deferred("interaction_burst");
        assert_eq!(state.proof_readback_deferred_count, 1);
        assert_eq!(state.proof_readback_deferred_for_interaction_burst_count, 1);
        assert_eq!(state.proof_readback_deferred_for_product_input_count, 0);
        assert_eq!(
            state.last_proof_readback_deferred_reason.as_deref(),
            Some("interaction_burst")
        );

        state.current_frame_lane = Some(NativeFrameLane::ProofOrHarness);
        assert!(!state.defer_proof_readback_for_product_lane(11.0));

        state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);
        state.requested_animation_burst_frames_remaining = 0;
        assert!(state.interaction_burst_active(11.0));
        assert!(state.interaction_quiet_proof_readback_frame(11.0));
        assert!(state.defer_proof_readback_for_product_lane(11.0));

        state.current_frame_lane = Some(NativeFrameLane::ProofOrHarness);
        assert!(!state.defer_proof_readback_for_product_lane(11.0));
    }

    #[test]
    fn proof_frames_poll_completed_readback_during_interaction_burst() {
        assert!(!pre_submit_proof_poll_allowed(
            true,
            true,
            Some(NativeFrameLane::ProductInteraction),
            Some(NativeSchedulerReason::HostInput),
        ));
        assert!(!pre_submit_proof_poll_allowed(
            false,
            true,
            Some(NativeFrameLane::AnimationFollowup),
            Some(NativeSchedulerReason::RequestedAnimation),
        ));
        assert!(pre_submit_proof_poll_allowed(
            false,
            true,
            Some(NativeFrameLane::ProofOrHarness),
            Some(NativeSchedulerReason::VerifierFrame),
        ));
        assert!(pre_submit_proof_poll_allowed(
            false,
            false,
            Some(NativeFrameLane::RuntimeOrLayout),
            Some(NativeSchedulerReason::ExternalWake),
        ));
    }

    #[test]
    fn requested_animation_burst_paces_until_quiet_interval_expires() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
        assert!(!state.consume_due_wake_after_poll(now));
        assert!(!state.should_render(now, false));
        let due = now + native_target_frame_interval_duration();
        assert!(state.consume_due_wake(due));
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(12.0);
        state.schedule_requested_animation_followup(due, 12.0);
        assert!(state.next_wake_at.is_some());

        let next_due = due + native_target_frame_interval_duration();
        assert!(state.consume_due_wake(next_due));
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(29.0);
        state.schedule_requested_animation_followup(next_due, 29.0);
        assert!(
            state.next_wake_at.is_some(),
            "the burst should keep pacing frames during the quiet window after min frames are consumed"
        );

        state.clear_requested_animation_burst_if_quiet(200.0);
        assert_eq!(
            native_frame_pacing_snapshot(&state).state,
            NativeFramePacingState::Idle
        );
    }

    #[test]
    fn requested_animation_followup_uses_frame_start_deadline_after_blocking_present() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
        state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
        assert!(!state.consume_due_wake_after_poll(now));
        state.note_render_started(10.0);
        state.mark_presented(state.dirty_revision);
        state.note_present_completed(25.0);

        let present_return = now + Duration::from_millis(25);
        state.schedule_requested_animation_followup(present_return, 25.0);
        let next_wake = state
            .next_wake_at
            .expect("burst follow-up should schedule a wake");

        assert!(
            next_wake <= present_return + Duration::from_millis(2),
            "follow-up should target the next frame deadline from render start, not present return plus a full interval"
        );
    }

    #[test]
    fn host_input_product_frame_starts_bounded_followup_burst() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.mark_dirty(NativeSchedulerReason::HostInput, None);
        state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);

        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(
            state.requested_animation_burst_frames_remaining,
            REQUESTED_ANIMATION_BURST_MIN_FRAMES
        );
        assert!(
            state.next_wake_at.is_some_and(|wake_at| wake_at > now),
            "visible-changing host input should keep a short paced burst hot after the current product frame"
        );
    }

    #[test]
    fn pointer_motion_prewarm_schedules_hot_burst_without_dirtying_current_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);
        let presented = state.presented_revision;

        state.request_interactive_prewarm_burst(now, 10.0);

        assert_eq!(state.dirty_revision, presented);
        assert_eq!(state.current_scheduler_reason, None);
        assert_eq!(state.current_frame_lane, None);
        assert!(!state.should_render(now, false));
        assert_eq!(state.requested_animation_prewarm_count, 1);
        assert_eq!(
            state.requested_animation_burst_frames_remaining,
            REQUESTED_ANIMATION_BURST_MIN_FRAMES
        );

        let due = now + native_target_frame_interval_duration();
        assert!(state.consume_due_wake(due));
        assert_eq!(
            state.last_scheduler_reason,
            Some(NativeSchedulerReason::RequestedAnimation)
        );
        assert_eq!(state.current_scheduler_reason, None);
        assert_eq!(state.current_frame_lane, None);
        assert!(!state.should_render(due, false));

        state.note_armed_frame_input_sampled(27.0);
        assert!(state.clean_armed_poll_pending());
        state.skip_clean_armed_burst_present(due, 27.0);

        assert_eq!(state.clean_armed_poll_count, 1);
        assert_eq!(state.skipped_clean_burst_present_count, 1);
        assert_eq!(
            state.requested_animation_burst_frames_remaining,
            REQUESTED_ANIMATION_BURST_MIN_FRAMES - 1
        );

        state.note_armed_frame_input_sampled(28.0);
        assert!(
            !state.clean_armed_poll_pending(),
            "an armed sampling turn must be consumed after one clean poll"
        );
    }

    #[test]
    fn host_input_after_pointer_prewarm_preserves_product_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_interactive_prewarm_burst(now, 10.0);
        let prewarm_wake = state
            .next_wake_at
            .expect("pointer prewarm should schedule a delayed frame");

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: Some(NativeRoleDirtyReason::FocusChanged),
                frame_lane: Some(NativeFrameLane::ProductInteraction),
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            true,
        );
        state.note_accepted_host_input(11, 12.0, false, None);

        assert!(state.consume_due_wake_after_poll(prewarm_wake));
        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(
            state.current_frame_lane,
            Some(NativeFrameLane::ProductInteraction)
        );
        assert!(state.should_render(prewarm_wake, false));
    }

    #[test]
    fn host_input_burst_keeps_followup_wake_off_current_input_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
        let animation_wake = state
            .next_wake_at
            .expect("requested animation should schedule a delayed wake");
        assert!(animation_wake > now);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);

        assert_eq!(
            state.next_wake_at,
            Some(animation_wake),
            "host input should preserve the current frame for HostInput and use the burst wake for the follow-up frame"
        );
        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
    }

    #[test]
    fn due_burst_wake_after_host_input_poll_does_not_steal_product_frame() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
        let due = state
            .next_wake_at
            .expect("requested animation should schedule a wake");
        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                frame_lane: Some(NativeFrameLane::ProductInteraction),
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            true,
        );
        state.note_accepted_host_input(9, 12.0, false, None);

        assert!(state.consume_due_wake_after_poll(due));
        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput),
            "a due burst wake must not relabel an accepted host-input product frame"
        );
        assert_eq!(
            state.current_frame_lane,
            Some(NativeFrameLane::ProductInteraction)
        );
        assert!(state.should_render(due, false));
    }

    #[test]
    fn host_input_animation_burst_can_repaint_without_waiting_a_frame_interval() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            true,
        );
        assert!(state.should_render(now, false));

        state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);
        assert!(!state.consume_due_wake_after_poll(now));

        assert!(state.should_render(now, false));
        assert_eq!(
            state.current_scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(
            state.current_role_dirty_reason,
            Some(NativeRoleDirtyReason::ScrollChanged)
        );
        assert_eq!(state.dirty_revision, state.presented_revision + 1);
    }

    #[test]
    fn accepted_real_input_repaint_is_presentable_even_with_runtime_dirty_reason() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        let now = Instant::now();
        state.mark_presented(state.dirty_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
                role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            true,
        );

        assert!(state.should_render(now, false));
        assert_eq!(state.dirty_revision, state.presented_revision + 1);
        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::RuntimeTurnApplied)
        );
    }

    #[test]
    fn unaccounted_host_input_frame_is_not_pre_present_drop_eligible() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.note_accepted_host_input(5, 20.0, false, None);
        state.note_present_completed(28.0);

        assert!(state.current_frame_carries_unaccounted_host_input(5));
        assert!(!state.current_frame_carries_unaccounted_host_input(4));

        assert!(state.take_frame_accepted_input_timing(5).is_some());
        assert!(!state.current_frame_carries_unaccounted_host_input(5));
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
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
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
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            false,
        );

        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::SourcePayloadAccepted)
        );
        assert!(state.should_render(Instant::now(), false));
        assert_eq!(state.dirty_revision, state.presented_revision + 1);
        assert_eq!(
            state.current_frame_lane,
            Some(NativeFrameLane::ProofOrHarness)
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
        let root_id = boon_host::SemanticId("semantic:world-editor:root".to_owned());
        let export_id =
            boon_host::SemanticId("semantic:world-editor:manufacturing:export-3mf".to_owned());
        let mut scene = boon_host::SemanticScene {
            root: Some(root_id.clone()),
            focused: Some(export_id.clone()),
            ..boon_host::SemanticScene::default()
        };
        scene.nodes.insert(
            root_id.clone(),
            boon_host::SemanticNode {
                id: root_id.clone(),
                node: boon_host::DocumentNodeId("world:world-editor:root".to_owned()),
                role: boon_host::SemanticRole::Application,
                name: Some("Car editor".to_owned()),
                description: None,
                value: None,
                state: boon_host::SemanticState::default(),
                actions: boon_host::SemanticActions::default(),
                relations: boon_host::SemanticRelations {
                    children: vec![export_id.clone()],
                    ..boon_host::SemanticRelations::default()
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
            boon_host::SemanticNode {
                id: export_id.clone(),
                node: boon_host::DocumentNodeId(
                    "world:world-editor:manufacturing:export-3mf".to_owned(),
                ),
                role: boon_host::SemanticRole::Button,
                name: Some("Export 3MF".to_owned()),
                description: None,
                value: None,
                state: boon_host::SemanticState {
                    focused: true,
                    ..boon_host::SemanticState::default()
                },
                actions: boon_host::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_host::SemanticRelations {
                    parent: Some(root_id),
                    ..boon_host::SemanticRelations::default()
                },
                bounds: None,
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(boon_host::SourceBindingId(
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
            boon_host::SemanticScene::from_world_editor_tree(&tree)
        };
        let node_id_for_name =
            |scene: &boon_host::SemanticScene, name: &str| -> accesskit::NodeId {
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
    fn low_latency_present_mode_prefers_nonblocking_present_modes() {
        let mut capabilities = wgpu::SurfaceCapabilities {
            formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
            present_modes: vec![
                wgpu::PresentMode::Fifo,
                wgpu::PresentMode::Immediate,
                wgpu::PresentMode::Mailbox,
            ],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        };

        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::Immediate
        );

        capabilities.present_modes = vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::AutoNoVsync,
            wgpu::PresentMode::Mailbox,
        ];
        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::AutoNoVsync
        );

        capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox];
        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::Mailbox
        );

        capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::AutoNoVsync];
        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::AutoNoVsync
        );

        capabilities.present_modes = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::Immediate
        );

        capabilities.present_modes = vec![wgpu::PresentMode::Fifo];
        assert_eq!(
            low_latency_present_mode(&capabilities),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn configured_present_mode_honors_supported_override() {
        let capabilities = wgpu::SurfaceCapabilities {
            formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
            present_modes: vec![
                wgpu::PresentMode::Fifo,
                wgpu::PresentMode::Immediate,
                wgpu::PresentMode::Mailbox,
            ],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        };

        assert_eq!(
            configured_low_latency_present_mode(&capabilities, Some("mailbox")),
            wgpu::PresentMode::Mailbox
        );
        assert_eq!(
            configured_low_latency_present_mode(&capabilities, Some("fifo-relaxed")),
            wgpu::PresentMode::Immediate,
            "unsupported overrides fall back to the normal low-latency policy"
        );
        assert_eq!(
            configured_low_latency_present_mode(&capabilities, Some("not-a-mode")),
            wgpu::PresentMode::Immediate
        );
        assert_eq!(
            configured_low_latency_present_mode(&capabilities, None),
            wgpu::PresentMode::Immediate
        );
        assert_eq!(
            configured_low_latency_present_mode(&capabilities, Some("immediate")),
            wgpu::PresentMode::Immediate,
            "supported overrides remain available for diagnostics"
        );
    }

    #[test]
    fn interactive_surface_latency_uses_bounded_multiple_frames_in_flight() {
        assert_eq!(
            interactive_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox),
            PACED_SURFACE_FRAME_LATENCY
        );
        assert_eq!(
            interactive_desired_maximum_frame_latency(wgpu::PresentMode::Fifo),
            PACED_SURFACE_FRAME_LATENCY
        );
        assert_eq!(
            interactive_desired_maximum_frame_latency(wgpu::PresentMode::Immediate),
            PACED_SURFACE_FRAME_LATENCY
        );
        assert_eq!(
            interactive_desired_maximum_frame_latency(wgpu::PresentMode::AutoNoVsync),
            PACED_SURFACE_FRAME_LATENCY
        );
    }

    #[test]
    fn configured_surface_frame_latency_honors_bounded_override() {
        assert_eq!(
            configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, None),
            (PACED_SURFACE_FRAME_LATENCY, "present_mode_default")
        );
        assert_eq!(
            configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("2")),
            (2, "env_override")
        );
        assert_eq!(
            configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("0")),
            (1, "env_override")
        );
        assert_eq!(
            configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("99")),
            (MAX_CONFIGURED_SURFACE_FRAME_LATENCY, "env_override")
        );
        assert_eq!(
            configured_desired_maximum_frame_latency(wgpu::PresentMode::Mailbox, Some("nope")),
            (PACED_SURFACE_FRAME_LATENCY, "invalid_env_default")
        );
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
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
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
    fn host_input_dirty_reason_is_presentable_even_if_dirty_flag_was_false() {
        let poll = NativePollResult {
            dirty: false,
            role_revision: 7,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        };

        let effective = effective_poll_result_for_host_input(poll.clone(), true);
        assert!(effective.dirty);
        assert_eq!(
            effective.role_dirty_reason,
            Some(NativeRoleDirtyReason::ScrollChanged)
        );
        assert_eq!(
            effective.scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(
            effective.frame_lane,
            Some(NativeFrameLane::ProductInteraction)
        );

        let without_real_input = effective_poll_result_for_host_input(poll, false);
        assert!(!without_real_input.dirty);
    }

    #[test]
    fn host_input_dominates_requested_animation_burst_accounting() {
        let poll = NativePollResult {
            dirty: true,
            role_revision: 7,
            scheduler_reason: Some(NativeSchedulerReason::RequestedAnimation),
            role_dirty_reason: Some(NativeRoleDirtyReason::FocusChanged),
            frame_lane: Some(NativeFrameLane::AnimationFollowup),
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: true,
            diagnostics: None,
            accessibility_update: None,
        };

        let effective = effective_poll_result_for_host_input(poll, true);

        assert_eq!(
            effective.scheduler_reason,
            Some(NativeSchedulerReason::HostInput)
        );
        assert_eq!(
            effective.frame_lane,
            Some(NativeFrameLane::ProductInteraction)
        );
        assert_eq!(
            native_frame_lane_for_scheduler(
                Some(NativeSchedulerReason::RequestedAnimation),
                None,
                true,
                true,
            ),
            NativeFrameLane::ProductInteraction
        );
    }

    #[test]
    fn same_content_host_input_repaint_can_use_existing_content_revision() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
        state.mark_presented(state.dirty_revision);

        state.apply_poll_result(
            &NativePollResult {
                dirty: true,
                role_revision: state.presented_revision,
                scheduler_reason: Some(NativeSchedulerReason::HostInput),
                role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
                accessibility_update: None,
            },
            true,
        );

        assert_eq!(state.dirty_revision, state.presented_revision + 1);
        assert!(state.should_render(Instant::now(), false));
        assert_eq!(
            state.last_role_dirty_reason,
            Some(NativeRoleDirtyReason::ScrollChanged)
        );
        let same_content = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: state.presented_revision,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: false,
            role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
        };
        assert!(
            same_content
                .validate_for_presented_revision_with_scheduler(
                    state.dirty_revision,
                    state.current_scheduler_reason,
                    state.current_role_dirty_reason,
                )
                .is_ok(),
            "same-content host input repaint should not require a new document content revision"
        );
        let changed_content_with_stale_revision = NativeRenderHookResult {
            content_changed: true,
            ..same_content
        };
        assert!(
            changed_content_with_stale_revision
                .validate_for_presented_revision_with_scheduler(
                    state.dirty_revision,
                    state.current_scheduler_reason,
                    state.current_role_dirty_reason,
                )
                .is_ok(),
            "host-input retained/runtime repaint may present an existing content revision until frame and content revisions are split"
        );
        assert!(
            changed_content_with_stale_revision
                .validate_for_presented_revision_with_scheduler(
                    state.dirty_revision,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::LayoutChanged),
                )
                .is_ok(),
            "external retained layout repaint may present an existing content revision"
        );
        assert!(
            changed_content_with_stale_revision
                .validate_for_presented_revision_with_scheduler(
                    state.dirty_revision,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_err(),
            "external runtime changes must still provide a current content revision"
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
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        };

        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 1);
        assert!(
            NativeRenderHookResult {
                proof: Some(serde_json::json!({})),
                content_revision: 1,
                layout_revision: None,
                render_scene_revision: None,
                render_frame_metrics: None,
                post_present_proof_subscribers: Vec::new(),
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
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: Some(16),
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: true,
            diagnostics: None,
            accessibility_update: None,
        };

        state.apply_poll_result(&poll, false);

        assert_eq!(state.dirty_revision, 2);
        assert!(
            (NativeRenderHookResult {
                proof: Some(serde_json::json!({})),
                content_revision: 2,
                layout_revision: None,
                render_scene_revision: None,
                render_frame_metrics: None,
                post_present_proof_subscribers: Vec::new(),
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
            proof: Some(serde_json::json!({})),
            content_revision: 2,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
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
            2
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

        zero.layout_revision = Some(0);
        assert!(zero.validate_for_presented_revision(2).is_err());

        zero.layout_revision = Some(3);
        zero.render_scene_revision = Some(0);
        assert!(zero.validate_for_presented_revision(2).is_err());
    }

    #[test]
    fn render_hook_result_can_present_without_external_proof_payload() {
        let mut render = NativeRenderHookResult::rendered_without_proof();
        render.content_revision = 3;
        render.layout_revision = Some(3);
        render.render_scene_revision = Some(3);

        assert_eq!(render.proof, None);
        assert!(
            render.validate_for_presented_revision(3).is_ok(),
            "product counters frames must not need pre-present proof JSON"
        );
    }

    #[test]
    fn render_hook_result_can_carry_independent_layer_revisions() {
        let render = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 10,
            layout_revision: Some(4),
            render_scene_revision: Some(7),
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: true,
            role_dirty_reason: None,
        };

        assert_eq!(
            render.presented_revisions(
                10,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            ),
            (10, 4, 7)
        );
    }

    #[test]
    fn surface_dirty_revision_can_present_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 1,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
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
            1
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
    fn external_runtime_cleanup_can_repaint_existing_content_revision() {
        let same_content_runtime_cleanup = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 13,
            layout_revision: Some(7),
            render_scene_revision: Some(9),
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: false,
            role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        };

        assert!(
            same_content_runtime_cleanup
                .validate_for_presented_revision_with_scheduler(
                    14,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_ok(),
            "queued runtime cleanup may repaint the current content frame without inventing a semantic content revision"
        );
        assert_eq!(
            same_content_runtime_cleanup.presented_revisions(
                14,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            ),
            (13, 7, 9),
            "frame revision may advance while content/layout/render-scene revisions stay keyed to the actual rendered state"
        );

        let changed_runtime_cleanup = NativeRenderHookResult {
            content_changed: true,
            ..same_content_runtime_cleanup
        };
        assert!(
            changed_runtime_cleanup
                .validate_for_presented_revision_with_scheduler(
                    14,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_ok(),
            "changed runtime content has its own content revision and must not be compared to a frame repaint revision"
        );
    }

    #[test]
    fn scheduler_only_host_input_can_repaint_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 2,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
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
            2
        );
        let changed_host_input_content = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 2,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: true,
            role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        };
        assert!(
            changed_host_input_content
                .validate_for_presented_revision_with_scheduler(
                    3,
                    Some(NativeSchedulerReason::HostInput),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_ok(),
            "host-input retained/runtime repaint may present an existing content revision until frame and content revisions are split"
        );
        let changed_external_content = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 2,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: true,
            role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        };
        assert!(
            changed_external_content
                .validate_for_presented_revision_with_scheduler(
                    3,
                    Some(NativeSchedulerReason::ExternalWake),
                    Some(NativeRoleDirtyReason::RuntimeTurnApplied),
                )
                .is_err(),
            "external runtime input must not present stale semantic content"
        );
    }

    #[test]
    fn idle_same_content_frame_can_repaint_existing_content_revision() {
        let render = NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 4,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
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
        assert_eq!(render.presented_content_revision(5, None, None), 4);

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
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
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
                frame_lane: None,
                accepted_host_input_event_hint: None,
                next_wake_after_ms: None,
                cursor_icon: NativeCursorIcon::Default,
                wants_animation_frame: false,
                diagnostics: None,
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
                proof: Some(serde_json::json!({})),
                content_revision: semantic_revision,
                layout_revision: None,
                render_scene_revision: None,
                render_frame_metrics: None,
                post_present_proof_subscribers: Vec::new(),
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
    fn presented_state_records_render_layer_revisions() {
        let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

        state.mark_presented_with_revisions(1, 3, 4, 5);

        assert_eq!(state.presented_revision, 1);
        assert_eq!(state.last_render_content_revision, 3);
        assert_eq!(state.last_render_layout_revision, 4);
        assert_eq!(state.last_render_scene_revision, 5);
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
                event_elapsed_ms: None,
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
    fn button_press_only_input_delta_is_coalescible() {
        let press_only = NativeInputAdapterProof {
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            }],
            ..empty_input_adapter_proof(false)
        };
        let click_pair = NativeInputAdapterProof {
            mouse_button_events: vec![
                NativeMouseButtonEventProof {
                    sequence: 7,
                    button: "left".to_owned(),
                    pressed: true,
                    window_protocol_id: Some(42),
                    event_elapsed_ms: None,
                },
                NativeMouseButtonEventProof {
                    sequence: 8,
                    button: "left".to_owned(),
                    pressed: false,
                    window_protocol_id: Some(42),
                    event_elapsed_ms: None,
                },
            ],
            ..empty_input_adapter_proof(false)
        };

        assert!(native_input_delta_is_button_press_only(&press_only));
        assert!(!native_input_delta_is_button_press_only(&click_pair));
    }

    #[test]
    fn clean_press_only_poll_does_not_accept_input_cursor() {
        let press_only = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            }],
            ..empty_input_adapter_proof(false)
        };
        let clean_poll = NativePollResult::clean(0);

        assert!(
            !should_accept_input_cursor_after_poll(true, &press_only, &clean_poll),
            "a clean role poll must not consume the only press edge before source input accepts it"
        );
    }

    #[test]
    fn dirty_press_only_poll_accepts_input_cursor() {
        let press_only = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            }],
            ..empty_input_adapter_proof(false)
        };
        let dirty_poll = NativePollResult {
            dirty: true,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            ..NativePollResult::clean(1)
        };

        assert!(
            should_accept_input_cursor_after_poll(true, &press_only, &dirty_poll),
            "once a role accepts a press edge, the native cursor can advance"
        );
    }

    #[test]
    fn raw_wake_without_input_delta_is_not_reportable_host_input() {
        let raw_wake = NativeInputAdapterProof {
            real_os_events_observed: false,
            ..empty_input_adapter_proof(false)
        };
        let press = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            }],
            ..empty_input_adapter_proof(false)
        };
        let motion = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_motion_event_count: 3,
            mouse_window_pos: Some(NativeMouseWindowPosition {
                x: 10.0,
                y: 20.0,
                window_width: 640.0,
                window_height: 480.0,
            }),
            ..empty_input_adapter_proof(false)
        };

        assert!(!native_input_delta_has_reportable_host_event(&raw_wake));
        assert!(native_input_delta_has_reportable_host_event(&press));
        assert!(native_input_delta_has_reportable_host_event(&motion));
    }

    #[test]
    fn pointer_motion_only_input_delta_can_yield_to_newer_input() {
        let motion_only = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_motion_event_count: 3,
            mouse_window_pos: Some(NativeMouseWindowPosition {
                x: 10.0,
                y: 20.0,
                window_width: 640.0,
                window_height: 480.0,
            }),
            ..empty_input_adapter_proof(false)
        };
        let button_delta = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_button_events: vec![NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            }],
            mouse_window_pos: motion_only.mouse_window_pos.clone(),
            ..empty_input_adapter_proof(false)
        };
        let scroll_delta = NativeInputAdapterProof {
            real_os_events_observed: true,
            mouse_window_pos: motion_only.mouse_window_pos.clone(),
            scroll_delta_y: 120.0,
            ..empty_input_adapter_proof(false)
        };

        assert!(native_input_delta_is_pointer_motion_only(&motion_only));
        assert!(!native_input_delta_is_pointer_motion_only(&button_delta));
        assert!(!native_input_delta_is_pointer_motion_only(&scroll_delta));
    }

    #[test]
    fn semantic_scene_lowers_to_accesskit_tree_update_with_stable_ids() {
        let root_id = boon_host::SemanticId("semantic:root".to_owned());
        let button_id = boon_host::SemanticId("semantic:save".to_owned());
        let checkbox_id = boon_host::SemanticId("semantic:done".to_owned());
        let input_id = boon_host::SemanticId("semantic:filter".to_owned());
        let mut scene = boon_host::SemanticScene {
            root: Some(root_id.clone()),
            focused: Some(input_id.clone()),
            ..boon_host::SemanticScene::default()
        };
        scene.nodes.insert(
            root_id.clone(),
            boon_host::SemanticNode {
                id: root_id.clone(),
                node: boon_host::DocumentNodeId("root".to_owned()),
                role: boon_host::SemanticRole::Application,
                name: Some("Boon app".to_owned()),
                description: None,
                value: None,
                state: boon_host::SemanticState::default(),
                actions: boon_host::SemanticActions::default(),
                relations: boon_host::SemanticRelations {
                    children: vec![button_id.clone(), checkbox_id.clone(), input_id.clone()],
                    ..boon_host::SemanticRelations::default()
                },
                bounds: Some(boon_host::Rect {
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
            boon_host::SemanticNode {
                id: button_id.clone(),
                node: boon_host::DocumentNodeId("save".to_owned()),
                role: boon_host::SemanticRole::Button,
                name: Some("Save".to_owned()),
                description: None,
                value: None,
                state: boon_host::SemanticState::default(),
                actions: boon_host::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_host::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_host::SemanticRelations::default()
                },
                bounds: Some(boon_host::Rect {
                    x: 8.0,
                    y: 8.0,
                    width: 80.0,
                    height: 28.0,
                }),
                language: None,
                heading_level: None,
                href: None,
                source_binding_id: Some(boon_host::SourceBindingId("source:save".to_owned())),
                source_path: Some("toolbar.save".to_owned()),
                source_intent: Some("press".to_owned()),
            },
        );
        scene.nodes.insert(
            checkbox_id.clone(),
            boon_host::SemanticNode {
                id: checkbox_id.clone(),
                node: boon_host::DocumentNodeId("done".to_owned()),
                role: boon_host::SemanticRole::Checkbox,
                name: Some("Done".to_owned()),
                description: None,
                value: Some(boon_host::SemanticValue::Bool { value: true }),
                state: boon_host::SemanticState {
                    checked: Some(true),
                    ..boon_host::SemanticState::default()
                },
                actions: boon_host::SemanticActions {
                    focus: true,
                    press: true,
                    set_text: false,
                    increment: false,
                    decrement: false,
                },
                relations: boon_host::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_host::SemanticRelations::default()
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
            boon_host::SemanticNode {
                id: input_id.clone(),
                node: boon_host::DocumentNodeId("filter".to_owned()),
                role: boon_host::SemanticRole::TextInput,
                name: Some("Filter".to_owned()),
                description: None,
                value: Some(boon_host::SemanticValue::Text {
                    text: "abc".to_owned(),
                }),
                state: boon_host::SemanticState {
                    focused: true,
                    ..boon_host::SemanticState::default()
                },
                actions: boon_host::SemanticActions {
                    focus: true,
                    press: false,
                    set_text: true,
                    increment: false,
                    decrement: false,
                },
                relations: boon_host::SemanticRelations {
                    parent: Some(root_id.clone()),
                    ..boon_host::SemanticRelations::default()
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
        let focus_update =
            accesskit_focus_update_from_semantic_node(&input_id, scene.nodes.get(&input_id));
        assert_eq!(focus_update.tree_update.focus, snapshot.tree_update.focus);
        assert!(
            focus_update.tree_update.tree.is_none(),
            "focus-only updates must not republish unchanged tree metadata"
        );
        assert_eq!(
            focus_update.tree_update.nodes.len(),
            1,
            "focused-node patch should upsert only the changed semantic node"
        );
        assert_eq!(
            focus_update.tree_update.nodes[0].0,
            snapshot.tree_update.focus
        );
        assert_eq!(focus_update.metrics.accesskit_node_count, 1);
        assert!(focus_update.metrics.focus_present);

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
