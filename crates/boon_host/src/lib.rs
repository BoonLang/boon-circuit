use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SurfaceId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct WindowId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RoleId(pub String);

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
    Pointer(PointerEvent),
    Wheel(WheelEvent),
    Focus { surface: SurfaceId, focused: bool },
    CloseRequested { window: WindowId },
    Resize(SurfaceResizeEvent),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub surface: SurfaceId,
    pub key: String,
    pub pressed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextInputEvent {
    pub surface: SurfaceId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub surface: SurfaceId,
    pub x: f32,
    pub y: f32,
    pub pressed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WheelEvent {
    pub surface: SurfaceId,
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
