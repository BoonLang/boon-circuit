use boon_host::{HostEvent, ImeInputKind, LogicalKey, PointerButton, PointerPhase};

use crate::ui::{DEV_EDITOR, DEV_NEXT, DEV_PREVIOUS, DEV_RESET, DEV_RUN, DEV_TEST};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevAction {
    None,
    Previous,
    Next,
    Run,
    Reset,
    Test,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevChange {
    None,
    Interaction,
    Scroll,
    SourceAndStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevEventResult {
    pub action: DevAction,
    pub change: DevChange,
}

impl DevEventResult {
    pub fn visible_change(self) -> bool {
        self.action != DevAction::None || self.change != DevChange::None
    }
}

pub struct DevState {
    source: String,
    original_source: String,
    hovered: Option<String>,
    pressed: Option<String>,
    editor_focused: bool,
    editor_scroll: f32,
    status: String,
}

impl DevState {
    pub fn new(source: String) -> Self {
        Self {
            original_source: source.clone(),
            source,
            hovered: None,
            pressed: None,
            editor_focused: false,
            editor_scroll: 0.0,
            status: "Ready".to_owned(),
        }
    }

    pub fn replace_source(&mut self, source: String) {
        self.original_source = source.clone();
        self.source = source;
        self.editor_scroll = 0.0;
        self.editor_focused = false;
        self.status = "Ready".to_owned();
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    pub fn editor_focused(&self) -> bool {
        self.editor_focused
    }

    pub fn editor_scroll(&self) -> f32 {
        self.editor_scroll
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn handle_event(
        &mut self,
        event: &HostEvent,
        mut hit: impl FnMut(f32, f32) -> Option<String>,
    ) -> DevEventResult {
        match event {
            HostEvent::CloseRequested { .. } => result(DevAction::Close, DevChange::None),
            HostEvent::Pointer(pointer) => match pointer.phase {
                PointerPhase::Move => {
                    let next = hit(pointer.x, pointer.y);
                    let changed = next != self.hovered;
                    self.hovered = next;
                    result(
                        DevAction::None,
                        if changed {
                            DevChange::Interaction
                        } else {
                            DevChange::None
                        },
                    )
                }
                PointerPhase::Leave => {
                    let changed = self.hovered.take().is_some();
                    self.pressed = None;
                    result(
                        DevAction::None,
                        if changed {
                            DevChange::Interaction
                        } else {
                            DevChange::None
                        },
                    )
                }
                PointerPhase::Down if pointer.button == Some(PointerButton::Primary) => {
                    let target = hit(pointer.x, pointer.y);
                    self.pressed.clone_from(&target);
                    let focused = target.as_deref() == Some(DEV_EDITOR);
                    let changed = focused != self.editor_focused;
                    self.editor_focused = focused;
                    result(
                        DevAction::None,
                        if changed {
                            DevChange::Interaction
                        } else {
                            DevChange::None
                        },
                    )
                }
                PointerPhase::Up if pointer.button == Some(PointerButton::Primary) => {
                    let target = hit(pointer.x, pointer.y);
                    let action = if self.pressed.take() == target {
                        action_for_target(target.as_deref())
                    } else {
                        DevAction::None
                    };
                    result(action, DevChange::None)
                }
                _ => result(DevAction::None, DevChange::None),
            },
            HostEvent::Wheel(wheel) if self.editor_focused => {
                let next = (self.editor_scroll + wheel.delta_y).max(0.0);
                let changed = next != self.editor_scroll;
                self.editor_scroll = next;
                result(
                    DevAction::None,
                    if changed {
                        DevChange::Scroll
                    } else {
                        DevChange::None
                    },
                )
            }
            HostEvent::TextInput(text) if self.editor_focused => {
                self.source.push_str(&text.text);
                self.status = "Edited".to_owned();
                result(DevAction::None, DevChange::SourceAndStatus)
            }
            HostEvent::Ime(ime) if self.editor_focused => {
                if let ImeInputKind::Commit { text } = &ime.kind {
                    self.source.push_str(text);
                    self.status = "Edited".to_owned();
                    result(DevAction::None, DevChange::SourceAndStatus)
                } else {
                    result(DevAction::None, DevChange::None)
                }
            }
            HostEvent::Keyboard(key) if self.editor_focused && key.pressed => {
                match &key.logical_key {
                    LogicalKey::Named(name) if name.eq_ignore_ascii_case("backspace") => {
                        let changed = self.source.pop().is_some();
                        if changed {
                            self.status = "Edited".to_owned();
                        }
                        result(
                            DevAction::None,
                            if changed {
                                DevChange::SourceAndStatus
                            } else {
                                DevChange::None
                            },
                        )
                    }
                    LogicalKey::Named(name) if name.eq_ignore_ascii_case("escape") => {
                        self.editor_focused = false;
                        result(DevAction::None, DevChange::Interaction)
                    }
                    _ => result(DevAction::None, DevChange::None),
                }
            }
            _ => result(DevAction::None, DevChange::None),
        }
    }
}

fn result(action: DevAction, change: DevChange) -> DevEventResult {
    DevEventResult { action, change }
}

fn action_for_target(target: Option<&str>) -> DevAction {
    match target {
        Some(DEV_PREVIOUS) => DevAction::Previous,
        Some(DEV_NEXT) => DevAction::Next,
        Some(DEV_RUN) => DevAction::Run,
        Some(DEV_RESET) => DevAction::Reset,
        Some(DEV_TEST) => DevAction::Test,
        _ => DevAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_host::{PointerEvent, SurfaceId, TextInputEvent};

    fn pointer(phase: PointerPhase) -> HostEvent {
        HostEvent::Pointer(PointerEvent {
            surface: SurfaceId("dev".to_owned()),
            x: 10.0,
            y: 10.0,
            phase,
            button: Some(PointerButton::Primary),
        })
    }

    #[test]
    fn test_requires_matching_real_down_and_up_target() {
        let mut state = DevState::new("source".to_owned());
        state.handle_event(&pointer(PointerPhase::Down), |_, _| {
            Some(DEV_TEST.to_owned())
        });
        let result =
            state.handle_event(&pointer(PointerPhase::Up), |_, _| Some(DEV_TEST.to_owned()));
        assert_eq!(result.action, DevAction::Test);
    }

    #[test]
    fn text_changes_only_after_editor_focus() {
        let mut state = DevState::new("a".to_owned());
        let input = HostEvent::TextInput(TextInputEvent {
            surface: SurfaceId("dev".to_owned()),
            text: "b".to_owned(),
        });
        state.handle_event(&input, |_, _| None);
        assert_eq!(state.source(), "a");
        state.handle_event(&pointer(PointerPhase::Down), |_, _| {
            Some(DEV_EDITOR.to_owned())
        });
        state.handle_event(&input, |_, _| None);
        assert_eq!(state.source(), "ab");
    }
}
