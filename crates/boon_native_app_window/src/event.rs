use app_window::coordinates::{Position, Size};
use app_window::event::{
    ButtonState, ImeEvent, LogicalKey, PointerButton as NativePointerButton, WheelDelta,
    WindowEvent, WindowEventCapabilities, WindowEventCapability, WindowEventError,
};
use app_window::input::keyboard::key::KeyboardKey;
use boon_host::{
    AccessibilityInputAction, AccessibilityInputEvent, CallbackToHostNs, HostEvent,
    HostEventEnvelope, HostEventOrigin, ImeInputEvent, ImeInputKind, KeyEvent,
    LogicalKey as HostLogicalKey, LogicalSize, PhysicalSize, PointerButton, PointerEvent,
    PointerPhase, SensitiveInputHandle, SurfaceResizeEvent, TextInputEvent, WheelEvent,
};

use crate::error::NativeHostError;
use crate::sensitive_input::{SensitiveEdit, SensitiveInputTarget, SensitiveInputVault};
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
    sensitive_ime_commit_echo: bool,
    shift_pressed: bool,
    control_pressed: bool,
    sensitive_inputs: SensitiveInputVault,
}

impl EventAdapter {
    pub(crate) fn new(ids: NativeHostIds) -> Self {
        Self {
            ids,
            sequence: 0,
            pointer_position: None,
            ime_commit_echo: None,
            sensitive_ime_commit_echo: false,
            shift_pressed: false,
            control_pressed: false,
            sensitive_inputs: SensitiveInputVault::default(),
        }
    }

    pub(crate) fn focus_sensitive_input(
        &mut self,
        target: SensitiveInputTarget,
    ) -> Result<SensitiveInputHandle, NativeHostError> {
        self.sensitive_inputs.focus(target).map_err(Into::into)
    }

    pub(crate) fn clear_sensitive_input_focus(&mut self) {
        self.sensitive_inputs.clear_focus();
        self.sensitive_ime_commit_echo = false;
    }

    pub(crate) fn restart_sensitive_inputs(&mut self) {
        self.sensitive_inputs.restart();
        self.sensitive_ime_commit_echo = false;
    }

    pub(crate) fn with_sensitive_input<R>(
        &self,
        handle: SensitiveInputHandle,
        use_bytes: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, crate::SensitiveInputError> {
        self.sensitive_inputs.with_bytes(handle, use_bytes)
    }

    pub(crate) fn adapt(
        &mut self,
        event: WindowEvent,
    ) -> Result<AdaptedWindowEvent, NativeHostError> {
        if matches!(event, WindowEvent::TextInput(_)) && self.sensitive_ime_commit_echo {
            self.sensitive_ime_commit_echo = false;
            return Ok(AdaptedWindowEvent::Omitted);
        }
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
            } => {
                self.update_modifiers(physical_key, state);
                let logical_is_text =
                    matches!(&logical_key, LogicalKey::Character(_) | LogicalKey::Dead(_));
                if let Some(handle) =
                    self.capture_sensitive_key(physical_key, state, logical_is_text)
                {
                    HostEvent::SensitiveInput(
                        self.sensitive_inputs.event(&self.ids.surface, handle),
                    )
                } else {
                    HostEvent::Keyboard(KeyEvent {
                        surface: self.ids.surface.clone(),
                        physical_key: physical_key.map(|key| format!("{key:?}")),
                        logical_key: logical_key_value(logical_key),
                        pressed: state == ButtonState::Pressed,
                    })
                }
            }
            WindowEvent::TextInput(text) => {
                if let Some(handle) = self.sensitive_inputs.insert_text(&text)? {
                    HostEvent::SensitiveInput(
                        self.sensitive_inputs.event(&self.ids.surface, handle),
                    )
                } else {
                    HostEvent::TextInput(TextInputEvent {
                        surface: self.ids.surface.clone(),
                        text,
                    })
                }
            }
            WindowEvent::Ime(ImeEvent::Commit(text)) => {
                if let Some(handle) = self.sensitive_inputs.insert_text(&text)? {
                    self.sensitive_ime_commit_echo = true;
                    HostEvent::SensitiveInput(
                        self.sensitive_inputs.event(&self.ids.surface, handle),
                    )
                } else {
                    self.ime_commit_echo = Some(text.clone());
                    HostEvent::Ime(ImeInputEvent {
                        surface: self.ids.surface.clone(),
                        kind: ImeInputKind::Commit { text },
                    })
                }
            }
            WindowEvent::Ime(event) => {
                if let Some(handle) = self.capture_sensitive_ime(&event)? {
                    HostEvent::SensitiveInput(
                        self.sensitive_inputs.event(&self.ids.surface, handle),
                    )
                } else {
                    HostEvent::Ime(ImeInputEvent {
                        surface: self.ids.surface.clone(),
                        kind: match event {
                            ImeEvent::Enabled => ImeInputKind::Enabled,
                            ImeEvent::Disabled => ImeInputKind::Disabled,
                            ImeEvent::Preedit { text, cursor } => {
                                ImeInputKind::Preedit { text, cursor }
                            }
                            ImeEvent::DeleteSurrounding {
                                before_bytes,
                                after_bytes,
                            } => ImeInputKind::DeleteSurrounding {
                                before_bytes,
                                after_bytes,
                            },
                            ImeEvent::Commit(_) => unreachable!("commit handled above"),
                        },
                    })
                }
            }
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
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.clear_sensitive_input_focus();
                    self.shift_pressed = false;
                    self.control_pressed = false;
                }
                HostEvent::Focus {
                    surface: self.ids.surface.clone(),
                    focused,
                }
            }
            WindowEvent::Resized { size, scale_factor } => {
                return Ok(AdaptedWindowEvent::Resize(viewport(size, scale_factor)?));
            }
            WindowEvent::CloseRequested => {
                self.restart_sensitive_inputs();
                HostEvent::CloseRequested {
                    window: self.ids.window.clone(),
                }
            }
            _ => return Err(NativeHostError::UnknownWindowEvent),
        };
        Ok(AdaptedWindowEvent::Host(event))
    }

    fn update_modifiers(&mut self, key: Option<KeyboardKey>, state: ButtonState) {
        let pressed = state == ButtonState::Pressed;
        match key {
            Some(KeyboardKey::Shift | KeyboardKey::RightShift) => self.shift_pressed = pressed,
            Some(
                KeyboardKey::Control
                | KeyboardKey::RightControl
                | KeyboardKey::Command
                | KeyboardKey::RightCommand,
            ) => self.control_pressed = pressed,
            _ => {}
        }
    }

    fn capture_sensitive_key(
        &mut self,
        key: Option<KeyboardKey>,
        state: ButtonState,
        logical_is_text: bool,
    ) -> Option<SensitiveInputHandle> {
        let active = self.sensitive_inputs.active_handle()?;
        if logical_is_text {
            return Some(active);
        }
        if state != ButtonState::Pressed {
            return None;
        }
        let command = match key {
            Some(KeyboardKey::Delete) => Some(SensitiveEdit::Backspace),
            Some(KeyboardKey::ForwardDelete) => Some(SensitiveEdit::DeleteForward),
            Some(KeyboardKey::LeftArrow) => Some(SensitiveEdit::MoveLeft {
                extend: self.shift_pressed,
            }),
            Some(KeyboardKey::RightArrow) => Some(SensitiveEdit::MoveRight {
                extend: self.shift_pressed,
            }),
            Some(KeyboardKey::Home) => Some(SensitiveEdit::MoveHome {
                extend: self.shift_pressed,
            }),
            Some(KeyboardKey::End) => Some(SensitiveEdit::MoveEnd {
                extend: self.shift_pressed,
            }),
            Some(KeyboardKey::A) if self.control_pressed => Some(SensitiveEdit::SelectAll),
            Some(KeyboardKey::X) if self.control_pressed => Some(SensitiveEdit::CutSelection),
            _ => None,
        };
        if let Some(command) = command {
            return self.sensitive_inputs.edit(command);
        }
        if self.control_pressed
            && matches!(
                key,
                Some(KeyboardKey::C | KeyboardKey::V | KeyboardKey::Z | KeyboardKey::Y)
            )
        {
            // Clipboard/history shortcuts stay host-owned. Paste is disabled
            // until a sensitive clipboard provider exists; forwarding it would
            // let the ordinary text editor copy the credential into Boon state.
            return Some(active);
        }
        None
    }

    fn capture_sensitive_ime(
        &mut self,
        event: &ImeEvent,
    ) -> Result<Option<SensitiveInputHandle>, NativeHostError> {
        if self.sensitive_inputs.active_handle().is_none() {
            return Ok(None);
        }
        let handle = match event {
            ImeEvent::Enabled => self.sensitive_inputs.active_handle(),
            ImeEvent::Disabled => self.sensitive_inputs.clear_preedit(),
            ImeEvent::Preedit { text, .. } => self.sensitive_inputs.set_preedit(text)?,
            ImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => self
                .sensitive_inputs
                .delete_surrounding(*before_bytes, *after_bytes),
            ImeEvent::Commit(_) => unreachable!("commit handled before sensitive IME routing"),
        };
        Ok(handle)
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
    use boon_host::{DocumentNodeId, RoleId, SourceBindingId, SurfaceId, WindowId};

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
    fn pointer_release_keeps_the_last_position_after_leave() {
        let mut adapter = adapter();
        adapter
            .adapt(WindowEvent::PointerEntered {
                position: Position::new(18.0, 32.0),
            })
            .unwrap();
        adapter
            .adapt(WindowEvent::PointerButton {
                button: NativePointerButton::Primary,
                state: ButtonState::Pressed,
            })
            .unwrap();
        let left = adapter.adapt(WindowEvent::PointerLeft).unwrap();
        let AdaptedWindowEvent::Host(HostEvent::Pointer(left)) = left else {
            panic!("expected pointer leave");
        };
        assert_eq!((left.x, left.y), (18.0, 32.0));
        assert_eq!(left.phase, PointerPhase::Leave);

        let released = adapter
            .adapt(WindowEvent::PointerButton {
                button: NativePointerButton::Primary,
                state: ButtonState::Released,
            })
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::Pointer(released)) = released else {
            panic!("expected pointer release");
        };
        assert_eq!((released.x, released.y), (18.0, 32.0));
        assert_eq!(released.phase, PointerPhase::Up);
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

    #[test]
    fn sensitive_text_is_captured_as_a_handle_only_event_and_cleared_on_focus_loss() {
        const SENTINEL: &str = "sensitive-SENTINEL-91c52f";
        let mut adapter = adapter();
        let handle = adapter
            .focus_sensitive_input(SensitiveInputTarget::new(
                DocumentNodeId("login-password".to_owned()),
                Some(SourceBindingId("login.password".to_owned())),
            ))
            .unwrap();

        let AdaptedWindowEvent::Host(key_event) = adapter
            .adapt(WindowEvent::KeyboardInput {
                physical_key: Some(KeyboardKey::A),
                logical_key: LogicalKey::Character(SENTINEL.to_owned()),
                state: ButtonState::Pressed,
            })
            .unwrap()
        else {
            panic!("sensitive logical text must produce a host event");
        };
        assert!(matches!(key_event, HostEvent::SensitiveInput(_)));
        assert!(!toml::to_string(&key_event).unwrap().contains(SENTINEL));

        let adapted = adapter
            .adapt(WindowEvent::TextInput(SENTINEL.to_owned()))
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::SensitiveInput(event)) = adapted else {
            panic!("sensitive input must not enter an ordinary text event");
        };
        assert_eq!(event.handle, handle);
        assert_eq!(
            adapter.with_sensitive_input(handle, |bytes| bytes == SENTINEL.as_bytes()),
            Ok(true)
        );
        let artifact = toml::to_string(&HostEvent::SensitiveInput(event)).unwrap();
        assert!(!artifact.contains(SENTINEL));
        assert!(!artifact.contains("91c52f"));
        assert!(!artifact.contains("text"));

        adapter.adapt(WindowEvent::Focused(false)).unwrap();
        assert_eq!(
            adapter.with_sensitive_input(handle, |_| ()),
            Err(crate::SensitiveInputError::UnknownHandle)
        );
    }

    #[test]
    fn sensitive_ime_commit_never_keeps_a_plaintext_echo() {
        const SENTINEL: &str = "ime-SENTINEL-d41c33";
        let mut adapter = adapter();
        let handle = adapter
            .focus_sensitive_input(SensitiveInputTarget::new(
                DocumentNodeId("login-password".to_owned()),
                None,
            ))
            .unwrap();
        let AdaptedWindowEvent::Host(HostEvent::SensitiveInput(event)) = adapter
            .adapt(WindowEvent::Ime(ImeEvent::Commit(SENTINEL.to_owned())))
            .unwrap()
        else {
            panic!("sensitive IME commit must be handle-only");
        };
        assert_eq!(event.handle, handle);
        assert!(matches!(
            adapter
                .adapt(WindowEvent::TextInput(SENTINEL.to_owned()))
                .unwrap(),
            AdaptedWindowEvent::Omitted
        ));
        assert_eq!(
            adapter.with_sensitive_input(handle, |bytes| bytes == SENTINEL.as_bytes()),
            Ok(true)
        );
    }
}
