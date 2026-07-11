use app_window::coordinates::{Position, Size};
use app_window::event::{
    ButtonState, ImeEvent, LogicalKey, PointerButton as NativePointerButton, WheelDelta,
    WindowEvent, WindowEventCapabilities, WindowEventCapability, WindowEventError,
};
use boon_host::{
    AccessibilityInputAction, AccessibilityInputEvent, CallbackToHostNs, HostEvent,
    HostEventEnvelope, HostEventOrigin, ImeInputEvent, ImeInputKind, KeyEvent,
    LogicalKey as HostLogicalKey, LogicalSize, PhysicalSize, PointerButton, PointerEvent,
    PointerPhase, SurfaceResizeEvent, TextInputEvent, WheelEvent,
};

use crate::error::NativeHostError;
use crate::surface::{NativeHostIds, NativeViewport};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeEventCapabilities {
    pub pointer: bool,
    pub physical_key: bool,
    pub logical_key: bool,
    pub text: bool,
    pub ime: bool,
    pub focus: bool,
    pub resize_scale: bool,
    pub close: bool,
    pub accessibility_action: bool,
}

impl NativeEventCapabilities {
    pub(crate) fn from_app_window(capabilities: WindowEventCapabilities) -> Self {
        let supported = |value| value == WindowEventCapability::Supported;
        Self {
            pointer: supported(capabilities.pointer),
            physical_key: supported(capabilities.physical_key),
            logical_key: supported(capabilities.logical_key),
            text: supported(capabilities.text),
            ime: supported(capabilities.ime),
            focus: supported(capabilities.focus),
            resize_scale: supported(capabilities.resize_scale),
            close: supported(capabilities.close),
            accessibility_action: supported(capabilities.accessibility_action),
        }
    }
}

pub(crate) enum AdaptedWindowEvent {
    Host(HostEvent),
    Resize(NativeViewport),
    Omitted,
}

pub(crate) struct EventAdapter {
    ids: NativeHostIds,
    sequence: u64,
    pointer_position: Option<(f32, f32)>,
    ime_commit_echo: Option<String>,
}

impl EventAdapter {
    pub(crate) fn new(ids: NativeHostIds) -> Self {
        Self {
            ids,
            sequence: 0,
            pointer_position: None,
            ime_commit_echo: None,
        }
    }

    pub(crate) fn adapt(
        &mut self,
        event: WindowEvent,
    ) -> Result<AdaptedWindowEvent, NativeHostError> {
        if let WindowEvent::TextInput(text) = &event {
            if self.ime_commit_echo.as_deref() == Some(text) {
                self.ime_commit_echo = None;
                return Ok(AdaptedWindowEvent::Omitted);
            }
            self.ime_commit_echo = None;
        } else if !matches!(event, WindowEvent::Ime(ImeEvent::Commit(_))) {
            self.ime_commit_echo = None;
        }
        let event = match event {
            WindowEvent::PointerEntered { position } | WindowEvent::PointerMoved { position } => {
                let position = pointer_position(position)?;
                self.pointer_position = Some(position);
                HostEvent::Pointer(PointerEvent {
                    surface: self.ids.surface.clone(),
                    x: position.0,
                    y: position.1,
                    phase: PointerPhase::Move,
                    button: None,
                })
            }
            WindowEvent::PointerLeft => {
                let (x, y) = self
                    .pointer_position
                    .take()
                    .ok_or(NativeHostError::MissingPointerPosition("pointer leave"))?;
                HostEvent::Pointer(PointerEvent {
                    surface: self.ids.surface.clone(),
                    x,
                    y,
                    phase: PointerPhase::Leave,
                    button: None,
                })
            }
            WindowEvent::PointerButton { button, state } => {
                let (x, y) = self
                    .pointer_position
                    .ok_or(NativeHostError::MissingPointerPosition("pointer button"))?;
                HostEvent::Pointer(PointerEvent {
                    surface: self.ids.surface.clone(),
                    x,
                    y,
                    phase: match state {
                        ButtonState::Pressed => PointerPhase::Down,
                        ButtonState::Released => PointerPhase::Up,
                    },
                    button: Some(pointer_button(button)?),
                })
            }
            WindowEvent::Wheel { delta } => {
                let (x, y) = self
                    .pointer_position
                    .ok_or(NativeHostError::MissingPointerPosition("wheel"))?;
                let (delta_x, delta_y) = wheel_delta(delta)?;
                HostEvent::Wheel(WheelEvent {
                    surface: self.ids.surface.clone(),
                    x,
                    y,
                    delta_x,
                    delta_y,
                })
            }
            WindowEvent::KeyboardInput {
                physical_key,
                logical_key,
                state,
            } => HostEvent::Keyboard(KeyEvent {
                surface: self.ids.surface.clone(),
                physical_key: physical_key.map(|key| format!("{key:?}")),
                logical_key: logical_key_value(logical_key),
                pressed: state == ButtonState::Pressed,
            }),
            WindowEvent::TextInput(text) => HostEvent::TextInput(TextInputEvent {
                surface: self.ids.surface.clone(),
                text,
            }),
            WindowEvent::Ime(ImeEvent::Commit(text)) => {
                self.ime_commit_echo = Some(text.clone());
                HostEvent::Ime(ImeInputEvent {
                    surface: self.ids.surface.clone(),
                    kind: ImeInputKind::Commit { text },
                })
            }
            WindowEvent::Ime(event) => HostEvent::Ime(ImeInputEvent {
                surface: self.ids.surface.clone(),
                kind: match event {
                    ImeEvent::Enabled => ImeInputKind::Enabled,
                    ImeEvent::Disabled => ImeInputKind::Disabled,
                    ImeEvent::Preedit { text, cursor } => ImeInputKind::Preedit { text, cursor },
                    ImeEvent::DeleteSurrounding {
                        before_bytes,
                        after_bytes,
                    } => ImeInputKind::DeleteSurrounding {
                        before_bytes,
                        after_bytes,
                    },
                    ImeEvent::Commit(_) => unreachable!("commit handled above"),
                },
            }),
            WindowEvent::AccessibilityAction(request) => {
                HostEvent::Accessibility(AccessibilityInputEvent {
                    surface: self.ids.surface.clone(),
                    target: request.target,
                    action: match request.action {
                        app_window::event::AccessibilityAction::Click => {
                            AccessibilityInputAction::Click
                        }
                        app_window::event::AccessibilityAction::Focus => {
                            AccessibilityInputAction::Focus
                        }
                        app_window::event::AccessibilityAction::Increment => {
                            AccessibilityInputAction::Increment
                        }
                        app_window::event::AccessibilityAction::Decrement => {
                            AccessibilityInputAction::Decrement
                        }
                        app_window::event::AccessibilityAction::Other(value) => {
                            AccessibilityInputAction::Other(value)
                        }
                    },
                })
            }
            WindowEvent::Focused(focused) => HostEvent::Focus {
                surface: self.ids.surface.clone(),
                focused,
            },
            WindowEvent::Resized { size, scale_factor } => {
                return Ok(AdaptedWindowEvent::Resize(viewport(size, scale_factor)?));
            }
            WindowEvent::CloseRequested => HostEvent::CloseRequested {
                window: self.ids.window.clone(),
            },
            _ => return Err(NativeHostError::UnknownWindowEvent),
        };
        Ok(AdaptedWindowEvent::Host(event))
    }

    pub(crate) fn envelope(
        &mut self,
        event: HostEvent,
        surface_epoch: u64,
        callback_to_host_ns: CallbackToHostNs,
    ) -> Result<HostEventEnvelope, NativeHostError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(NativeHostError::CounterOverflow("host event sequence"))?;
        Ok(HostEventEnvelope {
            sequence: self.sequence,
            origin: HostEventOrigin::RealOs,
            callback_to_host_ns,
            window: self.ids.window.clone(),
            surface: self.ids.surface.clone(),
            surface_epoch,
            event,
        })
    }

    pub(crate) fn resize_envelope(
        &mut self,
        viewport: NativeViewport,
        surface_epoch: u64,
        callback_to_host_ns: CallbackToHostNs,
    ) -> Result<HostEventEnvelope, NativeHostError> {
        let event = HostEvent::Resize(SurfaceResizeEvent {
            surface: self.ids.surface.clone(),
            logical_size: viewport.logical_size,
            scale: viewport.scale,
            physical_size: viewport.physical_size,
            epoch: surface_epoch,
        });
        self.envelope(event, surface_epoch, callback_to_host_ns)
    }
}

pub(crate) fn map_event_error(error: WindowEventError) -> NativeHostError {
    match error {
        WindowEventError::Overflow => NativeHostError::EventQueueOverflow,
        WindowEventError::Closed => NativeHostError::EventSourceClosed,
    }
}

pub(crate) fn viewport(size: Size, scale: f64) -> Result<NativeViewport, NativeHostError> {
    let width = finite_f32("logical width", size.width())?;
    let height = finite_f32("logical height", size.height())?;
    if width < 0.0 || height < 0.0 || !scale.is_finite() || scale <= 0.0 {
        return Err(NativeHostError::InvalidNumber {
            field: "surface size or scale",
            value: scale,
        });
    }
    Ok(NativeViewport {
        logical_size: LogicalSize { width, height },
        scale,
        physical_size: PhysicalSize {
            width: physical_dimension("physical width", f64::from(width), scale)?,
            height: physical_dimension("physical height", f64::from(height), scale)?,
        },
    })
}

fn pointer_position(position: Position) -> Result<(f32, f32), NativeHostError> {
    Ok((
        finite_f32("pointer x", position.x())?,
        finite_f32("pointer y", position.y())?,
    ))
}

fn pointer_button(button: NativePointerButton) -> Result<PointerButton, NativeHostError> {
    Ok(match button {
        NativePointerButton::Primary => PointerButton::Primary,
        NativePointerButton::Secondary => PointerButton::Secondary,
        NativePointerButton::Middle => PointerButton::Middle,
        NativePointerButton::Other(button) => PointerButton::Other(
            button
                .try_into()
                .map_err(|_| NativeHostError::PointerButtonOutOfRange(button))?,
        ),
    })
}

fn wheel_delta(delta: WheelDelta) -> Result<(f32, f32), NativeHostError> {
    let (x, y) = match delta {
        WheelDelta::Lines { x, y } | WheelDelta::Pixels { x, y } | WheelDelta::Pages { x, y } => {
            (x, y)
        }
    };
    Ok((finite_f32("wheel x", x)?, finite_f32("wheel y", y)?))
}

fn logical_key_value(logical_key: LogicalKey) -> HostLogicalKey {
    match logical_key {
        LogicalKey::Character(value) => HostLogicalKey::Character(value),
        LogicalKey::Named(value) => HostLogicalKey::Named(value),
        LogicalKey::Dead(value) => HostLogicalKey::Dead(value),
        LogicalKey::Unidentified => HostLogicalKey::Unidentified,
    }
}

fn finite_f32(field: &'static str, value: f64) -> Result<f32, NativeHostError> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(NativeHostError::InvalidNumber { field, value });
    }
    Ok(value as f32)
}

fn physical_dimension(
    field: &'static str,
    logical: f64,
    scale: f64,
) -> Result<u32, NativeHostError> {
    let physical = (logical * scale).round();
    if !physical.is_finite() || physical < 0.0 || physical > f64::from(u32::MAX) {
        return Err(NativeHostError::InvalidNumber {
            field,
            value: physical,
        });
    }
    Ok(physical as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_window::input::keyboard::key::KeyboardKey;
    use boon_host::{RoleId, SurfaceId, WindowId};

    fn adapter() -> EventAdapter {
        EventAdapter::new(NativeHostIds {
            role: RoleId("preview".to_owned()),
            window: WindowId("window-1".to_owned()),
            surface: SurfaceId("surface-1".to_owned()),
        })
    }

    #[test]
    fn pointer_button_keeps_position_and_sequence() {
        let mut adapter = adapter();
        let moved = adapter
            .adapt(WindowEvent::PointerMoved {
                position: Position::new(12.5, 24.0),
            })
            .unwrap();
        let AdaptedWindowEvent::Host(moved) = moved else {
            panic!("expected host event");
        };
        assert_eq!(
            adapter
                .envelope(moved, 3, CallbackToHostNs::ZERO)
                .unwrap()
                .sequence,
            1
        );

        let pressed = adapter
            .adapt(WindowEvent::PointerButton {
                button: NativePointerButton::Primary,
                state: ButtonState::Pressed,
            })
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::Pointer(pressed)) = pressed else {
            panic!("expected pointer event");
        };
        assert_eq!((pressed.x, pressed.y), (12.5, 24.0));
        assert_eq!(pressed.phase, PointerPhase::Down);
    }

    #[test]
    fn keyboard_event_keeps_physical_and_logical_values_separate() {
        let mut adapter = adapter();
        let event = adapter
            .adapt(WindowEvent::KeyboardInput {
                physical_key: Some(KeyboardKey::A),
                logical_key: LogicalKey::Character("a".to_owned()),
                state: ButtonState::Pressed,
            })
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::Keyboard(event)) = event else {
            panic!("expected keyboard event");
        };
        assert_eq!(event.physical_key.as_deref(), Some("A"));
        assert_eq!(event.logical_key, HostLogicalKey::Character("a".to_owned()));
        assert!(event.pressed);
    }

    #[test]
    fn resize_keeps_logical_and_physical_sizes() {
        let viewport = viewport(Size::new(800.0, 600.0), 1.5).unwrap();
        assert_eq!(viewport.logical_size.width, 800.0);
        assert_eq!(viewport.physical_size.width, 1200);
        assert_eq!(viewport.physical_size.height, 900);
    }

    #[test]
    fn composition_and_accessibility_remain_typed_host_input() {
        let mut adapter = adapter();
        assert!(matches!(
            adapter.adapt(WindowEvent::Ime(ImeEvent::Enabled)).unwrap(),
            AdaptedWindowEvent::Host(HostEvent::Ime(ImeInputEvent {
                kind: ImeInputKind::Enabled,
                ..
            }))
        ));
        assert!(matches!(
            adapter
                .adapt(WindowEvent::AccessibilityAction(
                    app_window::event::AccessibilityActionRequest {
                        target: 7,
                        action: app_window::event::AccessibilityAction::Focus,
                    }
                ))
                .unwrap(),
            AdaptedWindowEvent::Host(HostEvent::Accessibility(AccessibilityInputEvent {
                target: 7,
                action: AccessibilityInputAction::Focus,
                ..
            }))
        ));
    }

    #[test]
    fn ime_commit_is_typed_and_its_adjacent_text_echo_is_suppressed() {
        let mut adapter = adapter();
        let commit = adapter
            .adapt(WindowEvent::Ime(ImeEvent::Commit("hello".to_owned())))
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::Ime(ImeInputEvent {
            kind: ImeInputKind::Commit { text },
            ..
        })) = commit
        else {
            panic!("expected committed IME text");
        };
        assert_eq!(text, "hello");
        assert!(matches!(
            adapter
                .adapt(WindowEvent::TextInput("hello".to_owned()))
                .unwrap(),
            AdaptedWindowEvent::Omitted
        ));
    }
}
