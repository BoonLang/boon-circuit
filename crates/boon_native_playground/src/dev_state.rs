use boon_editor::{Buffer, Command, Position};
use boon_host::{HostEvent, ImeInputKind, LogicalKey, PointerButton, PointerPhase};

use crate::ui::{
    DEV_EDITOR, DEV_FORMAT, DEV_NEW, DEV_NEXT, DEV_PREVIOUS, DEV_REMOVE, DEV_RESET, DEV_RUN,
    DEV_SAVE, DEV_TEST,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardAction {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DevAction {
    None,
    Previous,
    Next,
    Run,
    Reset,
    Test,
    Save,
    Format,
    NewProject,
    RemoveProject,
    SelectExample(String),
    SelectFile(usize),
    Clipboard(ClipboardAction),
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevChange {
    None,
    Interaction,
    Scroll,
    EditorText,
    EditorSelection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevEventResult {
    pub action: DevAction,
    pub change: DevChange,
}

impl DevEventResult {
    pub fn visible_change(&self) -> bool {
        self.action != DevAction::None || self.change != DevChange::None
    }
}

pub struct DevState {
    buffer: Buffer,
    hovered: Option<String>,
    pressed: Option<String>,
    editor_focused: bool,
    editor_scroll: f32,
    inspector_position: Option<Position>,
    status: String,
    control: bool,
    shift: bool,
    alt: bool,
}

impl DevState {
    pub fn new(source: String) -> Self {
        Self {
            buffer: Buffer::new(&source),
            hovered: None,
            pressed: None,
            editor_focused: false,
            editor_scroll: 0.0,
            inspector_position: None,
            status: "Ready".to_owned(),
            control: false,
            shift: false,
            alt: false,
        }
    }

    pub fn replace_source(&mut self, source: String) {
        self.buffer = Buffer::new(&source);
        self.editor_scroll = 0.0;
        self.inspector_position = None;
        self.editor_focused = false;
        self.status = "Ready".to_owned();
    }

    pub fn source(&self) -> String {
        self.buffer.text()
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    pub fn editor_focused(&self) -> bool {
        self.editor_focused
    }

    pub fn dragging_editor(&self) -> bool {
        self.pressed.as_deref().is_some_and(is_editor_target)
    }

    pub fn shift_held(&self) -> bool {
        self.shift
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

    pub fn set_caret(&mut self, position: Position, extend: bool) -> bool {
        self.editor_focused = true;
        let changed = self.buffer.set_caret(position, extend);
        if changed {
            self.reveal_caret();
        }
        changed
    }

    pub fn set_inspector_position(&mut self, position: Option<Position>) -> bool {
        let position = position.map(|position| Position {
            line: position
                .line
                .min(self.buffer.line_count().saturating_sub(1)),
            column: position.column,
        });
        let changed = position != self.inspector_position;
        self.inspector_position = position;
        changed
    }

    pub fn inspection_position(&self) -> Position {
        self.inspector_position
            .unwrap_or_else(|| self.buffer.caret())
    }

    pub fn selected_text(&self) -> String {
        self.buffer.selected_text()
    }

    pub fn paste(&mut self, text: &str) -> bool {
        self.edit(Command::InsertPlain(text.to_owned()))
    }

    pub fn cut_selection(&mut self) -> bool {
        if self.buffer.selection().is_collapsed() {
            false
        } else {
            self.edit(Command::DeleteForward)
        }
    }

    pub fn format(&mut self, source: String) {
        self.buffer = Buffer::new(&source);
        self.status = "Formatted".to_owned();
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
                    let focused = target.as_deref().is_some_and(is_editor_target);
                    let changed = focused != self.editor_focused;
                    self.editor_focused = focused;
                    result(
                        DevAction::None,
                        if changed || focused {
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
            HostEvent::Wheel(wheel)
                if hit(wheel.x, wheel.y)
                    .as_deref()
                    .is_some_and(is_editor_target) =>
            {
                let max_scroll = self.buffer.line_count().saturating_sub(24) as f32 * 23.0;
                let next = (self.editor_scroll + wheel.delta_y).clamp(0.0, max_scroll);
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
            HostEvent::TextInput(text) if self.editor_focused => result(
                DevAction::None,
                self.apply_command(Command::Insert(text.text.clone())),
            ),
            HostEvent::Ime(ime) if self.editor_focused => {
                if let ImeInputKind::Commit { text } = &ime.kind {
                    result(
                        DevAction::None,
                        self.apply_command(Command::InsertPlain(text.clone())),
                    )
                } else {
                    result(DevAction::None, DevChange::None)
                }
            }
            HostEvent::Keyboard(key) => self.keyboard(&key.logical_key, key.pressed),
            _ => result(DevAction::None, DevChange::None),
        }
    }

    fn keyboard(&mut self, key: &LogicalKey, pressed: bool) -> DevEventResult {
        if let LogicalKey::Named(name) = key {
            match name.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => self.control = pressed,
                "shift" => self.shift = pressed,
                "alt" => self.alt = pressed,
                _ => {}
            }
        }
        if !pressed || !self.editor_focused {
            return result(DevAction::None, DevChange::None);
        }
        if self.control {
            let action = match key_text(key).as_deref() {
                Some("a") => {
                    self.buffer.apply(Command::SelectAll);
                    return result(DevAction::None, DevChange::EditorSelection);
                }
                Some("c") => DevAction::Clipboard(ClipboardAction::Copy),
                Some("x") => DevAction::Clipboard(ClipboardAction::Cut),
                Some("v") => DevAction::Clipboard(ClipboardAction::Paste),
                Some("s") => DevAction::Save,
                Some("z") if self.shift => {
                    return self.command(Command::Redo);
                }
                Some("z") => return self.command(Command::Undo),
                Some("y") => return self.command(Command::Redo),
                _ => DevAction::None,
            };
            if action != DevAction::None {
                return result(action, DevChange::None);
            }
        }
        let extend = self.shift;
        let command = match key {
            LogicalKey::Named(name) => match name.to_ascii_lowercase().as_str() {
                "backspace" => Some(Command::DeleteBackward),
                "delete" => Some(Command::DeleteForward),
                "enter" => Some(Command::Newline),
                "arrowleft" | "left" => Some(Command::MoveLeft { extend }),
                "arrowright" | "right" => Some(Command::MoveRight { extend }),
                "arrowup" | "up" => Some(Command::MoveUp { extend }),
                "arrowdown" | "down" => Some(Command::MoveDown { extend }),
                "home" => Some(Command::MoveHome { extend }),
                "end" => Some(Command::MoveEnd { extend }),
                "pageup" => Some(Command::PageUp { extend, lines: 24 }),
                "pagedown" => Some(Command::PageDown { extend, lines: 24 }),
                "tab" if self.shift => Some(Command::Unindent),
                "tab" => Some(Command::Indent),
                "escape" => {
                    self.editor_focused = false;
                    return result(DevAction::None, DevChange::Interaction);
                }
                _ => None,
            },
            _ => None,
        };
        command.map_or_else(
            || result(DevAction::None, DevChange::None),
            |command| self.command(command),
        )
    }

    fn command(&mut self, command: Command) -> DevEventResult {
        result(DevAction::None, self.apply_command(command))
    }

    fn edit(&mut self, command: Command) -> bool {
        self.apply_command(command) == DevChange::EditorText
    }

    fn apply_command(&mut self, command: Command) -> DevChange {
        let revision = self.buffer.revision();
        if !self.buffer.apply(command) {
            return DevChange::None;
        }
        self.reveal_caret();
        if self.buffer.revision() != revision {
            self.status = "Edited".to_owned();
            DevChange::EditorText
        } else {
            DevChange::EditorSelection
        }
    }

    fn reveal_caret(&mut self) {
        let caret_line = self.buffer.caret().line;
        let first = (self.editor_scroll / 23.0).floor() as usize;
        if caret_line < first {
            self.editor_scroll = caret_line as f32 * 23.0;
        } else if caret_line >= first + 24 {
            self.editor_scroll = caret_line.saturating_sub(23) as f32 * 23.0;
        }
    }
}

fn result(action: DevAction, change: DevChange) -> DevEventResult {
    DevEventResult { action, change }
}

fn key_text(key: &LogicalKey) -> Option<String> {
    match key {
        LogicalKey::Character(value) => Some(value.to_ascii_lowercase()),
        LogicalKey::Named(value) if value.len() == 1 => Some(value.to_ascii_lowercase()),
        _ => None,
    }
}

fn is_editor_target(target: &str) -> bool {
    target == DEV_EDITOR || target.starts_with("dev.editor.")
}

fn action_for_target(target: Option<&str>) -> DevAction {
    match target {
        Some(DEV_PREVIOUS) => DevAction::Previous,
        Some(DEV_NEXT) => DevAction::Next,
        Some(DEV_RUN) => DevAction::Run,
        Some(DEV_RESET) => DevAction::Reset,
        Some(DEV_TEST) => DevAction::Test,
        Some(DEV_SAVE) => DevAction::Save,
        Some(DEV_FORMAT) => DevAction::Format,
        Some(DEV_NEW) => DevAction::NewProject,
        Some(DEV_REMOVE) => DevAction::RemoveProject,
        Some(target) if target.starts_with("dev.example.") => {
            DevAction::SelectExample(target["dev.example.".len()..].to_owned())
        }
        Some(target) if target.starts_with("dev.file.") => target["dev.file.".len()..]
            .parse()
            .map_or(DevAction::None, DevAction::SelectFile),
        _ => DevAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_host::{PointerEvent, SurfaceId, TextInputEvent, WheelEvent};

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
    fn text_changes_at_the_caret_only_after_editor_focus() {
        let mut state = DevState::new("ac".to_owned());
        let input = HostEvent::TextInput(TextInputEvent {
            surface: SurfaceId("dev".to_owned()),
            text: "b".to_owned(),
        });
        state.handle_event(&input, |_, _| None);
        assert_eq!(state.source(), "ac");
        state.handle_event(&pointer(PointerPhase::Down), |_, _| {
            Some("dev.editor.code.0".to_owned())
        });
        state.set_caret(Position { line: 0, column: 1 }, false);
        state.handle_event(&input, |_, _| None);
        assert_eq!(state.source(), "abc");
    }

    #[test]
    fn wheel_scrolls_when_pointer_is_over_editor_without_focus() {
        let mut state = DevState::new("a\n".repeat(30));
        let result = state.handle_event(
            &HostEvent::Wheel(WheelEvent {
                surface: SurfaceId("dev".to_owned()),
                x: 10.0,
                y: 10.0,
                delta_x: 0.0,
                delta_y: 40.0,
            }),
            |_, _| Some("dev.editor.code.1".to_owned()),
        );
        assert_eq!(result.change, DevChange::Scroll);
        assert_eq!(state.editor_scroll(), 40.0);
    }
}
