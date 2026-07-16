use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

pub use boon_document_model::{DocumentNodeId, Rect, SourceBindingId};

macro_rules! string_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            pub struct $name(pub String);
        )+
    };
}

string_ids!(SurfaceId, WindowId, RoleId);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicalSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TiledWindowRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn tiled_divider_drag_points(
    preview: TiledWindowRect,
    other: TiledWindowRect,
    desired_width: u32,
    desired_height: u32,
) -> Result<((i32, i32), (i32, i32)), String> {
    let desired_width = i32::try_from(desired_width).map_err(|_| "width is out of range")?;
    let desired_height = i32::try_from(desired_height).map_err(|_| "height is out of range")?;
    let overlap = |a: i32, al: i32, b: i32, bl: i32| {
        let (start, end) = (a.max(b), a.saturating_add(al).min(b.saturating_add(bl)));
        (end > start).then_some(start.saturating_add((end - start) / 2))
    };
    let (preview_right, other_right) = (
        preview.x.saturating_add(preview.width),
        other.x.saturating_add(other.width),
    );
    if desired_height == preview.height
        && (preview_right - other.x).abs() <= 48
        && let Some(y) = overlap(preview.y, preview.height, other.y, other.height)
    {
        let from = ((preview_right + other.x) / 2, y);
        return Ok((from, (from.0 + desired_width - preview.width, y)));
    }
    if desired_height == preview.height
        && (other_right - preview.x).abs() <= 48
        && let Some(y) = overlap(preview.y, preview.height, other.y, other.height)
    {
        let from = ((other_right + preview.x) / 2, y);
        return Ok((from, (from.0 + preview.width - desired_width, y)));
    }
    let (preview_bottom, other_bottom) = (
        preview.y.saturating_add(preview.height),
        other.y.saturating_add(other.height),
    );
    if desired_width == preview.width
        && (preview_bottom - other.y).abs() <= 48
        && let Some(x) = overlap(preview.x, preview.width, other.x, other.width)
    {
        let from = (x, (preview_bottom + other.y) / 2);
        return Ok((from, (x, from.1 + desired_height - preview.height)));
    }
    if desired_width == preview.width
        && (other_bottom - preview.y).abs() <= 48
        && let Some(x) = overlap(preview.x, preview.width, other.x, other.width)
    {
        let from = (x, (other_bottom + preview.y) / 2);
        return Ok((from, (x, from.1 + preview.height - desired_height)));
    }
    Err(format!(
        "size {desired_width}x{desired_height} is unreachable through tiled divider {preview:?}/{other:?}"
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub surface: u64,
    pub width: f32,
    pub height: f32,
    pub scale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceResizeEvent {
    pub surface: SurfaceId,
    pub logical_size: LogicalSize,
    pub scale: f64,
    pub physical_size: PhysicalSize,
    pub epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostEvent {
    Keyboard(KeyEvent),
    TextInput(TextInputEvent),
    Ime(ImeInputEvent),
    SensitiveInput(SensitiveInputEvent),
    Pointer(PointerEvent),
    Wheel(WheelEvent),
    Accessibility(AccessibilityInputEvent),
    Focus { surface: SurfaceId, focused: bool },
    CloseRequested { window: WindowId },
    Resize(SurfaceResizeEvent),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostEventOrigin {
    RealOs,
    Operator,
}

/// Saturating callback-to-host latency in nanoseconds.
///
/// This remains an eight-byte scalar in memory and serializes as a number, while
/// preventing timing values with an unspecified unit from entering metrics.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct CallbackToHostNs(u64);

impl CallbackToHostNs {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    pub fn saturating_from_duration(duration: Duration) -> Self {
        Self(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<CallbackToHostNs> for u64 {
    fn from(value: CallbackToHostNs) -> Self {
        value.get()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostEventEnvelope {
    pub sequence: u64,
    pub origin: HostEventOrigin,
    pub callback_to_host_ns: CallbackToHostNs,
    pub window: WindowId,
    pub surface: SurfaceId,
    pub surface_epoch: u64,
    pub event: HostEvent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub surface: SurfaceId,
    pub physical_key: Option<String>,
    pub logical_key: LogicalKey,
    pub pressed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LogicalKey {
    Character(String),
    Named(String),
    Dead(Option<char>),
    Unidentified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextInputEvent {
    pub surface: SurfaceId,
    pub text: String,
}

/// A process-local reference to host-owned sensitive input.
///
/// The numeric token is deliberately not a credential, digest, prefix, or
/// length. It is valid only in the native host instance that issued it.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SensitiveInputHandle(u64);

impl SensitiveInputHandle {
    pub fn from_host_sequence(sequence: u64) -> Option<Self> {
        (sequence != 0).then_some(Self(sequence))
    }
}

impl Debug for SensitiveInputHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveInputHandle(<opaque>)")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SensitiveInputEvent {
    pub surface: SurfaceId,
    pub handle: SensitiveInputHandle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImeInputEvent {
    pub surface: SurfaceId,
    pub kind: ImeInputKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImeInputKind {
    Enabled,
    Disabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit {
        text: String,
    },
    DeleteSurrounding {
        before_bytes: u32,
        after_bytes: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityInputEvent {
    pub surface: SurfaceId,
    pub target: u64,
    pub action: AccessibilityInputAction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AccessibilityInputAction {
    Click,
    Focus,
    Increment,
    Decrement,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub surface: SurfaceId,
    pub x: f32,
    pub y: f32,
    pub phase: PointerPhase,
    pub button: Option<PointerButton>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerPhase {
    Move,
    Down,
    Up,
    Leave,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Other(u8),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WheelEvent {
    pub surface: SurfaceId,
    pub x: f32,
    pub y: f32,
    pub delta_x: f32,
    pub delta_y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewportIntent {
    Scroll { delta_x: f32, delta_y: f32 },
    Resize { size: LogicalSize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceIntent {
    pub binding: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitResolution {
    pub surface: SurfaceId,
    pub target: Option<String>,
    pub focused: Option<String>,
    pub viewport_intent: Option<ViewportIntent>,
    pub source_intents: Vec<SourceIntent>,
}

string_ids!(SemanticId);

impl SemanticId {
    pub fn from_document_node_id(node: &DocumentNodeId) -> Self {
        Self(format!("semantic:{}", node.0))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    Application,
    Group,
    Row,
    Text,
    Link,
    Button,
    Checkbox,
    TextInput,
    EmbeddedMedia,
    Table,
    Cell,
    ScrollRegion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticValue {
    Text { text: String },
    Bool { value: bool },
    Number { value: f64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticState {
    pub focused: bool,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sensitive: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticActions {
    pub focus: bool,
    pub press: bool,
    pub set_text: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sensitive_input: bool,
    pub increment: bool,
    pub decrement: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticRelations {
    pub parent: Option<SemanticId>,
    pub children: Vec<SemanticId>,
    pub controls: Vec<SemanticId>,
    pub labelled_by: Vec<SemanticId>,
    pub described_by: Vec<SemanticId>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: SemanticId,
    pub node: DocumentNodeId,
    pub role: SemanticRole,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<SemanticValue>,
    pub state: SemanticState,
    pub actions: SemanticActions,
    pub relations: SemanticRelations,
    pub bounds: Option<Rect>,
    pub language: Option<String>,
    pub heading_level: Option<u8>,
    pub href: Option<String>,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticScene {
    pub root: Option<SemanticId>,
    pub nodes: BTreeMap<SemanticId, SemanticNode>,
    pub focused: Option<SemanticId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticPatch {
    pub operations: Vec<SemanticPatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticPatchOperation {
    UpsertNode { node: Box<SemanticNode> },
    RemoveNode { id: SemanticId },
    SetFocus { focused: Option<SemanticId> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticAction {
    Focus,
    Press,
    SetText,
    Increment,
    Decrement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticInputEvent {
    Focus {
        semantic_id: SemanticId,
    },
    Press {
        semantic_id: SemanticId,
    },
    SetText {
        semantic_id: SemanticId,
        text: String,
    },
    ReplaceSelectedText {
        semantic_id: SemanticId,
        text: String,
    },
    Increment {
        semantic_id: SemanticId,
    },
    Decrement {
        semantic_id: SemanticId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceDispatch {
    pub semantic_id: SemanticId,
    pub node: DocumentNodeId,
    pub source_path: String,
    pub source_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl SemanticScene {
    pub fn diff(&self, next: &SemanticScene) -> SemanticPatch {
        let mut operations = Vec::new();
        for id in self.nodes.keys() {
            if !next.nodes.contains_key(id) {
                operations.push(SemanticPatchOperation::RemoveNode { id: id.clone() });
            }
        }
        for (id, node) in &next.nodes {
            if self.nodes.get(id) != Some(node) {
                operations.push(SemanticPatchOperation::UpsertNode {
                    node: Box::new(node.clone()),
                });
            }
        }
        if self.focused != next.focused {
            operations.push(SemanticPatchOperation::SetFocus {
                focused: next.focused.clone(),
            });
        }
        SemanticPatch { operations }
    }

    pub fn source_dispatch_for_event(
        &self,
        event: SemanticInputEvent,
    ) -> Option<SemanticSourceDispatch> {
        let (semantic_id, action, text) = match event {
            SemanticInputEvent::Focus { semantic_id } => (semantic_id, SemanticAction::Focus, None),
            SemanticInputEvent::Press { semantic_id } => (semantic_id, SemanticAction::Press, None),
            SemanticInputEvent::SetText { semantic_id, text }
            | SemanticInputEvent::ReplaceSelectedText { semantic_id, text } => {
                (semantic_id, SemanticAction::SetText, Some(text))
            }
            SemanticInputEvent::Increment { semantic_id } => {
                (semantic_id, SemanticAction::Increment, None)
            }
            SemanticInputEvent::Decrement { semantic_id } => {
                (semantic_id, SemanticAction::Decrement, None)
            }
        };
        let node = self.nodes.get(&semantic_id)?;
        Some(SemanticSourceDispatch {
            semantic_id,
            node: node.node.clone(),
            source_path: semantic_source_for_action(node, &action)?,
            source_intent: node.source_intent.clone(),
            text,
        })
    }
}

fn semantic_source_for_action(node: &SemanticNode, action: &SemanticAction) -> Option<String> {
    let intent = node.source_intent.as_deref()?;
    let matches_action = match action {
        SemanticAction::Focus => intent == "focus",
        SemanticAction::Press => matches!(
            intent,
            "press" | "click" | "source" | "activate" | "toggle" | "submit" | "open" | "select"
        ),
        SemanticAction::SetText => matches!(intent, "change" | "text" | "input"),
        SemanticAction::Increment => intent == "increment",
        SemanticAction::Decrement => intent == "decrement",
    };
    matches_action.then(|| node.source_path.clone()).flatten()
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn callback_latency_is_compact_and_saturating() {
        assert_eq!(std::mem::size_of::<CallbackToHostNs>(), 8);
        assert_eq!(
            CallbackToHostNs::saturating_from_duration(Duration::from_nanos(27)).get(),
            27
        );
        assert_eq!(
            CallbackToHostNs::saturating_from_duration(Duration::from_secs(u64::MAX)),
            CallbackToHostNs::MAX
        );
    }
}
