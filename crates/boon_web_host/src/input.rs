use crate::{WebHostError, WebHostResult};
use boon_host::{
    CallbackToHostNs, HostEvent, HostEventEnvelope, HostEventOrigin, ImeInputEvent, ImeInputKind,
    KeyEvent, LogicalKey, LogicalSize, PhysicalSize, PointerButton, PointerEvent, PointerPhase,
    SurfaceId, SurfaceResizeEvent, TextInputEvent, WheelEvent, WindowId,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserHostEvent {
    Input { envelope: HostEventEnvelope },
    Gesture { event: BrowserGestureEvent },
    Clipboard { event: BrowserClipboardEvent },
    Lifecycle { event: BrowserLifecycleEvent },
    UrlChanged { path_query_fragment: String },
    Rejected { error: WebHostError },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserGestureEvent {
    TouchStart {
        pointer_id: i32,
        x: f32,
        y: f32,
    },
    TouchMove {
        pointer_id: i32,
        x: f32,
        y: f32,
    },
    TouchEnd {
        pointer_id: i32,
        x: f32,
        y: f32,
    },
    Pinch {
        center_x: f32,
        center_y: f32,
        scale_delta: f32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserClipboardEvent {
    PasteText { text: String },
    CopyRequested,
    CutRequested,
    ReadDenied { reason: String },
    WriteDenied { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserLifecycleEvent {
    VisibilityChanged { visible: bool },
    OnlineChanged { online: bool },
    ReducedMotionChanged { reduced: bool },
    PageHide { persisted: bool },
    PageShow { persisted: bool },
    BeforeUnload,
}

#[derive(Clone, Debug)]
pub struct BrowserInputNormalizer {
    surface: SurfaceId,
    window: WindowId,
    surface_epoch: u64,
    next_sequence: u64,
    max_text_bytes: usize,
}

impl BrowserInputNormalizer {
    pub fn new(
        surface: SurfaceId,
        window: WindowId,
        surface_epoch: u64,
        max_text_bytes: usize,
    ) -> WebHostResult<Self> {
        if max_text_bytes == 0 {
            return Err(WebHostError::InvalidInput {
                field: "max_text_bytes".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self {
            surface,
            window,
            surface_epoch,
            next_sequence: 1,
            max_text_bytes,
        })
    }

    pub fn surface_epoch(&self) -> u64 {
        self.surface_epoch
    }

    pub fn set_surface_epoch(&mut self, epoch: u64) {
        self.surface_epoch = epoch;
    }

    pub fn pointer(
        &mut self,
        x: f32,
        y: f32,
        phase: PointerPhase,
        button: Option<PointerButton>,
    ) -> WebHostResult<BrowserHostEvent> {
        validate_finite("pointer x", x)?;
        validate_finite("pointer y", y)?;
        Ok(self.wrap(HostEvent::Pointer(PointerEvent {
            surface: self.surface.clone(),
            x,
            y,
            phase,
            button,
        })))
    }

    pub fn wheel(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> WebHostResult<BrowserHostEvent> {
        for (field, value) in [
            ("wheel x", x),
            ("wheel y", y),
            ("wheel delta_x", delta_x),
            ("wheel delta_y", delta_y),
        ] {
            validate_finite(field, value)?;
        }
        Ok(self.wrap(HostEvent::Wheel(WheelEvent {
            surface: self.surface.clone(),
            x,
            y,
            delta_x,
            delta_y,
        })))
    }

    pub fn key(
        &mut self,
        physical_code: Option<String>,
        logical_key: String,
        pressed: bool,
    ) -> WebHostResult<BrowserHostEvent> {
        validate_bounded_text(
            "physical key",
            physical_code.as_deref().unwrap_or_default(),
            128,
        )?;
        validate_bounded_text("logical key", &logical_key, 256)?;
        let logical_key = match logical_key.as_str() {
            "Dead" => LogicalKey::Dead(None),
            "Unidentified" => LogicalKey::Unidentified,
            value if value.chars().count() == 1 => LogicalKey::Character(value.to_owned()),
            value => LogicalKey::Named(value.to_owned()),
        };
        Ok(self.wrap(HostEvent::Keyboard(KeyEvent {
            surface: self.surface.clone(),
            physical_key: physical_code,
            logical_key,
            pressed,
        })))
    }

    pub fn text(&mut self, text: String) -> WebHostResult<BrowserHostEvent> {
        self.validate_text(&text)?;
        Ok(self.wrap(HostEvent::TextInput(TextInputEvent {
            surface: self.surface.clone(),
            text,
        })))
    }

    pub fn ime(&mut self, kind: ImeInputKind) -> WebHostResult<BrowserHostEvent> {
        match &kind {
            ImeInputKind::Preedit { text, .. } | ImeInputKind::Commit { text } => {
                self.validate_text(text)?;
            }
            ImeInputKind::Enabled
            | ImeInputKind::Disabled
            | ImeInputKind::DeleteSurrounding { .. } => {}
        }
        Ok(self.wrap(HostEvent::Ime(ImeInputEvent {
            surface: self.surface.clone(),
            kind,
        })))
    }

    pub fn focus(&mut self, focused: bool) -> BrowserHostEvent {
        self.wrap(HostEvent::Focus {
            surface: self.surface.clone(),
            focused,
        })
    }

    pub fn resize(
        &mut self,
        logical_width: f32,
        logical_height: f32,
        scale: f64,
        physical_width: u32,
        physical_height: u32,
    ) -> WebHostResult<BrowserHostEvent> {
        validate_finite("logical width", logical_width)?;
        validate_finite("logical height", logical_height)?;
        if logical_width <= 0.0
            || logical_height <= 0.0
            || !scale.is_finite()
            || scale <= 0.0
            || physical_width == 0
            || physical_height == 0
        {
            return Err(WebHostError::InvalidInput {
                field: "browser viewport".to_owned(),
                reason: "dimensions and scale must be finite and positive".to_owned(),
            });
        }
        self.surface_epoch = self.surface_epoch.saturating_add(1);
        Ok(self.wrap(HostEvent::Resize(SurfaceResizeEvent {
            surface: self.surface.clone(),
            logical_size: LogicalSize {
                width: logical_width,
                height: logical_height,
            },
            scale,
            physical_size: PhysicalSize {
                width: physical_width,
                height: physical_height,
            },
            epoch: self.surface_epoch,
        })))
    }

    fn validate_text(&self, text: &str) -> WebHostResult<()> {
        validate_bounded_text("browser text input", text, self.max_text_bytes)
    }

    fn wrap(&mut self, event: HostEvent) -> BrowserHostEvent {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        BrowserHostEvent::Input {
            envelope: HostEventEnvelope {
                sequence,
                origin: HostEventOrigin::RealOs,
                callback_to_host_ns: CallbackToHostNs::ZERO,
                window: self.window.clone(),
                surface: self.surface.clone(),
                surface_epoch: self.surface_epoch,
                event,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct BrowserEventQueue {
    events: VecDeque<BrowserHostEvent>,
    capacity: usize,
}

impl BrowserEventQueue {
    pub fn new(capacity: usize) -> WebHostResult<Self> {
        if capacity == 0 {
            return Err(WebHostError::InvalidInput {
                field: "browser event queue capacity".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self {
            events: VecDeque::new(),
            capacity,
        })
    }

    pub fn push(&mut self, event: BrowserHostEvent) -> WebHostResult<()> {
        if let Some(previous) = self.events.back_mut()
            && coalesce_adjacent(previous, &event)
        {
            return Ok(());
        }
        if self.events.len() >= self.capacity {
            return Err(WebHostError::QueueOverflow {
                queue: "browser host events".to_owned(),
                capacity: self.capacity,
            });
        }
        self.events.push_back(event);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<BrowserHostEvent> {
        self.events.pop_front()
    }

    pub fn drain(&mut self) -> impl Iterator<Item = BrowserHostEvent> + '_ {
        self.events.drain(..)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

fn coalesce_adjacent(previous: &mut BrowserHostEvent, next: &BrowserHostEvent) -> bool {
    let (
        BrowserHostEvent::Input { envelope: previous },
        BrowserHostEvent::Input { envelope: next },
    ) = (previous, next)
    else {
        return false;
    };
    if previous.origin != next.origin
        || previous.window != next.window
        || previous.surface != next.surface
        || previous.surface_epoch != next.surface_epoch
    {
        return false;
    }
    match (&mut previous.event, &next.event) {
        (
            HostEvent::Pointer(PointerEvent {
                phase: PointerPhase::Move,
                x: previous_x,
                y: previous_y,
                ..
            }),
            HostEvent::Pointer(PointerEvent {
                phase: PointerPhase::Move,
                x: next_x,
                y: next_y,
                ..
            }),
        ) => {
            *previous_x = *next_x;
            *previous_y = *next_y;
            previous.sequence = next.sequence;
            previous.callback_to_host_ns = next.callback_to_host_ns;
            true
        }
        (HostEvent::Wheel(previous_wheel), HostEvent::Wheel(next_wheel)) => {
            previous_wheel.x = next_wheel.x;
            previous_wheel.y = next_wheel.y;
            previous_wheel.delta_x += next_wheel.delta_x;
            previous_wheel.delta_y += next_wheel.delta_y;
            previous.sequence = next.sequence;
            previous.callback_to_host_ns = next.callback_to_host_ns;
            true
        }
        _ => false,
    }
}

fn validate_finite(field: &str, value: f32) -> WebHostResult<()> {
    if !value.is_finite() {
        return Err(WebHostError::InvalidInput {
            field: field.to_owned(),
            reason: "must be finite".to_owned(),
        });
    }
    Ok(())
}

fn validate_bounded_text(field: &str, value: &str, limit: usize) -> WebHostResult<()> {
    if value.len() > limit {
        return Err(WebHostError::LimitExceeded {
            resource: field.to_owned(),
            limit,
        });
    }
    if value.contains('\0') {
        return Err(WebHostError::InvalidInput {
            field: field.to_owned(),
            reason: "contains NUL".to_owned(),
        });
    }
    Ok(())
}
