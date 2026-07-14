use boon_editor::{Buffer, Command, Position};
use boon_host::{HostEvent, ImeInputKind, LogicalKey, PointerButton, PointerPhase};

use crate::ui::{
    DEV_EDITOR, DEV_FILE_NEW, DEV_FILE_REMOVE, DEV_FILE_RENAME, DEV_FORMAT, DEV_INSPECT_OUTBOX,
    DEV_INSPECT_PERSISTENCE, DEV_INSPECT_VALUE, DEV_MIGRATION_ACTIVATE, DEV_MIGRATION_PREVIEW,
    DEV_MIGRATION_RESTART, DEV_MIGRATION_STAGE_PREFIX, DEV_MIGRATION_START_OVER, DEV_NEW, DEV_NEXT,
    DEV_OUTBOX_NEXT, DEV_OUTBOX_PREVIOUS, DEV_PERSISTENCE_ACTIVATE_IMPORT,
    DEV_PERSISTENCE_CLEAR_ALL, DEV_PERSISTENCE_CLEAR_SELECTED, DEV_PERSISTENCE_COMPACT,
    DEV_PERSISTENCE_EXPORT, DEV_PERSISTENCE_FLUSH, DEV_PERSISTENCE_IMPORT_PREVIEW, DEV_PREVIOUS,
    DEV_REMOVE, DEV_RENAME, DEV_RENAME_CANCEL, DEV_RENAME_INPUT, DEV_RENAME_SAVE, DEV_RESET,
    DEV_RUN, DEV_SAVE, DEV_TEST, InspectorMode,
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
    MigrationPreview,
    MigrationActivate,
    MigrationRestart,
    MigrationStartOver,
    SelectInspector(InspectorMode),
    PersistenceFlush,
    PersistenceCompact,
    PersistenceClearAll,
    PersistenceClearSelected,
    PersistenceExport,
    PersistenceImportPreview,
    PersistenceActivateImport,
    OutboxPrevious,
    OutboxNext,
    Save,
    Format,
    NewProject,
    NewFile,
    BeginRename,
    BeginFileRename,
    CommitRename,
    CancelRename,
    RemoveProject,
    RemoveFile,
    SelectExample(String),
    SelectFile(usize),
    SelectMigrationStage(String),
    Clipboard(ClipboardAction),
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameEditTarget {
    Project,
    NewFile,
    File(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevChange {
    None,
    Interaction,
    Scroll,
    EditorText,
    EditorSelection,
    Rename,
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
    rename: Option<Buffer>,
    rename_target: Option<NameEditTarget>,
    rename_focused: bool,
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
            rename: None,
            rename_target: None,
            rename_focused: false,
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
        self.rename = None;
        self.rename_target = None;
        self.rename_focused = false;
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

    pub fn focused_target(&self) -> Option<&'static str> {
        if self.rename_focused {
            Some(DEV_RENAME_INPUT)
        } else if self.editor_focused {
            Some(DEV_EDITOR)
        } else {
            None
        }
    }

    pub fn rename_buffer(&self) -> Option<&Buffer> {
        self.rename.as_ref()
    }

    pub fn begin_rename(&mut self, label: &str) {
        self.begin_name_edit(NameEditTarget::Project, label);
    }

    pub fn begin_new_file(&mut self, name: &str) {
        self.begin_name_edit(NameEditTarget::NewFile, name);
    }

    pub fn begin_file_rename(&mut self, index: usize, name: &str) {
        self.begin_name_edit(NameEditTarget::File(index), name);
    }

    fn begin_name_edit(&mut self, target: NameEditTarget, value: &str) {
        let mut buffer = Buffer::new(value);
        buffer.apply(Command::SelectAll);
        self.rename = Some(buffer);
        self.rename_target = Some(target);
        self.rename_focused = true;
        self.editor_focused = false;
    }

    pub fn name_edit_target(&self) -> Option<NameEditTarget> {
        self.rename_target
    }

    pub fn rename_prompt(&self) -> Option<&'static str> {
        match self.rename_target? {
            NameEditTarget::Project => Some("Example name"),
            NameEditTarget::NewFile => Some("New file"),
            NameEditTarget::File(_) => Some("File name"),
        }
    }

    pub fn rename_text(&self) -> Option<String> {
        self.rename.as_ref().map(Buffer::text)
    }

    pub fn finish_rename(&mut self) {
        self.rename = None;
        self.rename_target = None;
        self.rename_focused = false;
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
        self.inspector_position = None;
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
        if self.rename_focused {
            self.rename
                .as_ref()
                .map(Buffer::selected_text)
                .unwrap_or_default()
        } else {
            self.buffer.selected_text()
        }
    }

    pub fn paste(&mut self, text: &str) -> bool {
        if self.rename_focused {
            self.rename_edit(Command::InsertPlain(single_line_text(text))) == DevChange::Rename
        } else {
            self.edit(Command::InsertPlain(text.to_owned()))
        }
    }

    pub fn cut_selection(&mut self) -> bool {
        if self.rename_focused {
            if self
                .rename
                .as_ref()
                .is_none_or(|buffer| buffer.selection().is_collapsed())
            {
                false
            } else {
                self.rename_edit(Command::DeleteForward) == DevChange::Rename
            }
        } else if self.buffer.selection().is_collapsed() {
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
                    let rename_focused = target.as_deref() == Some(DEV_RENAME_INPUT);
                    let changed =
                        focused != self.editor_focused || rename_focused != self.rename_focused;
                    self.editor_focused = focused;
                    self.rename_focused = rename_focused && self.rename.is_some();
                    result(
                        if self.rename.is_some()
                            && !self.rename_focused
                            && !matches!(
                                target.as_deref(),
                                Some(DEV_RENAME_SAVE | DEV_RENAME_CANCEL)
                            )
                        {
                            DevAction::CommitRename
                        } else {
                            DevAction::None
                        },
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
            HostEvent::TextInput(text) if self.rename_focused => result(
                DevAction::None,
                self.rename_edit(Command::InsertPlain(single_line_text(&text.text))),
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
            HostEvent::Ime(ime) if self.rename_focused => {
                if let ImeInputKind::Commit { text } = &ime.kind {
                    result(
                        DevAction::None,
                        self.rename_edit(Command::InsertPlain(single_line_text(text))),
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
            let name = name.to_ascii_lowercase();
            if matches!(name.as_str(), "control" | "ctrl")
                || name.starts_with("control_")
                || name.starts_with("ctrl_")
            {
                self.control = pressed;
            } else if name == "shift" || name.starts_with("shift_") {
                self.shift = pressed;
            } else if name == "alt" || name.starts_with("alt_") {
                self.alt = pressed;
            }
        }
        if !pressed {
            return result(DevAction::None, DevChange::None);
        }
        if self.rename_focused {
            return self.rename_keyboard(key);
        }
        if !self.editor_focused {
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

    fn rename_keyboard(&mut self, key: &LogicalKey) -> DevEventResult {
        if self.control {
            let action = match key_text(key).as_deref() {
                Some("a") => {
                    if let Some(buffer) = self.rename.as_mut() {
                        buffer.apply(Command::SelectAll);
                    }
                    return result(DevAction::None, DevChange::Rename);
                }
                Some("c") => DevAction::Clipboard(ClipboardAction::Copy),
                Some("x") => DevAction::Clipboard(ClipboardAction::Cut),
                Some("v") => DevAction::Clipboard(ClipboardAction::Paste),
                Some("z") if self.shift => {
                    return result(DevAction::None, self.rename_edit(Command::Redo));
                }
                Some("z") => return result(DevAction::None, self.rename_edit(Command::Undo)),
                Some("y") => return result(DevAction::None, self.rename_edit(Command::Redo)),
                _ => DevAction::None,
            };
            if action != DevAction::None {
                return result(action, DevChange::None);
            }
        }
        let extend = self.shift;
        match key {
            LogicalKey::Named(name) => match normalize_key(name).as_str() {
                "enter" => result(DevAction::CommitRename, DevChange::None),
                "escape" => result(DevAction::CancelRename, DevChange::None),
                "backspace" => {
                    let change = self.rename_edit(Command::DeleteBackward);
                    result(DevAction::None, change)
                }
                "delete" => {
                    let change = self.rename_edit(Command::DeleteForward);
                    result(DevAction::None, change)
                }
                "left" => {
                    let change = self.rename_edit(Command::MoveLeft { extend });
                    result(DevAction::None, change)
                }
                "right" => {
                    let change = self.rename_edit(Command::MoveRight { extend });
                    result(DevAction::None, change)
                }
                "home" => {
                    let change = self.rename_edit(Command::MoveHome { extend });
                    result(DevAction::None, change)
                }
                "end" => {
                    let change = self.rename_edit(Command::MoveEnd { extend });
                    result(DevAction::None, change)
                }
                _ => result(DevAction::None, DevChange::None),
            },
            _ => result(DevAction::None, DevChange::None),
        }
    }

    fn command(&mut self, command: Command) -> DevEventResult {
        result(DevAction::None, self.apply_command(command))
    }

    fn edit(&mut self, command: Command) -> bool {
        self.apply_command(command) == DevChange::EditorText
    }

    fn rename_edit(&mut self, command: Command) -> DevChange {
        let Some(buffer) = self.rename.as_mut() else {
            return DevChange::None;
        };
        if buffer.apply(command) {
            DevChange::Rename
        } else {
            DevChange::None
        }
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

fn normalize_key(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "arrowleft" | "leftarrow" => "left".to_owned(),
        "arrowright" | "rightarrow" => "right".to_owned(),
        "back_space" => "backspace".to_owned(),
        "return" | "kp_enter" => "enter".to_owned(),
        value => value.to_owned(),
    }
}

fn single_line_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
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
        Some(DEV_MIGRATION_PREVIEW) => DevAction::MigrationPreview,
        Some(DEV_MIGRATION_ACTIVATE) => DevAction::MigrationActivate,
        Some(DEV_MIGRATION_RESTART) => DevAction::MigrationRestart,
        Some(DEV_MIGRATION_START_OVER) => DevAction::MigrationStartOver,
        Some(DEV_INSPECT_VALUE) => DevAction::SelectInspector(InspectorMode::Value),
        Some(DEV_INSPECT_PERSISTENCE) => DevAction::SelectInspector(InspectorMode::Persistence),
        Some(DEV_INSPECT_OUTBOX) => DevAction::SelectInspector(InspectorMode::Outbox),
        Some(DEV_PERSISTENCE_FLUSH) => DevAction::PersistenceFlush,
        Some(DEV_PERSISTENCE_COMPACT) => DevAction::PersistenceCompact,
        Some(DEV_PERSISTENCE_CLEAR_ALL) => DevAction::PersistenceClearAll,
        Some(DEV_PERSISTENCE_CLEAR_SELECTED) => DevAction::PersistenceClearSelected,
        Some(DEV_PERSISTENCE_EXPORT) => DevAction::PersistenceExport,
        Some(DEV_PERSISTENCE_IMPORT_PREVIEW) => DevAction::PersistenceImportPreview,
        Some(DEV_PERSISTENCE_ACTIVATE_IMPORT) => DevAction::PersistenceActivateImport,
        Some(DEV_OUTBOX_PREVIOUS) => DevAction::OutboxPrevious,
        Some(DEV_OUTBOX_NEXT) => DevAction::OutboxNext,
        Some(DEV_SAVE) => DevAction::Save,
        Some(DEV_FORMAT) => DevAction::Format,
        Some(DEV_NEW) => DevAction::NewProject,
        Some(DEV_FILE_NEW) => DevAction::NewFile,
        Some(DEV_RENAME) => DevAction::BeginRename,
        Some(DEV_FILE_RENAME) => DevAction::BeginFileRename,
        Some(DEV_RENAME_SAVE) => DevAction::CommitRename,
        Some(DEV_RENAME_CANCEL) => DevAction::CancelRename,
        Some(DEV_REMOVE) => DevAction::RemoveProject,
        Some(DEV_FILE_REMOVE) => DevAction::RemoveFile,
        Some(target) if target.starts_with("dev.example.") => {
            DevAction::SelectExample(target["dev.example.".len()..].to_owned())
        }
        Some(target) if target.starts_with("dev.file.") => target["dev.file.".len()..]
            .parse()
            .map_or(DevAction::None, DevAction::SelectFile),
        Some(target) if target.starts_with(DEV_MIGRATION_STAGE_PREFIX) => {
            DevAction::SelectMigrationStage(target[DEV_MIGRATION_STAGE_PREFIX.len()..].to_owned())
        }
        _ => DevAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_host::{KeyEvent, PointerEvent, SurfaceId, TextInputEvent, WheelEvent};

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
    fn migration_controls_emit_typed_actions() {
        assert_eq!(
            action_for_target(Some(DEV_MIGRATION_PREVIEW)),
            DevAction::MigrationPreview
        );
        assert_eq!(
            action_for_target(Some(DEV_MIGRATION_ACTIVATE)),
            DevAction::MigrationActivate
        );
        assert_eq!(
            action_for_target(Some(DEV_MIGRATION_RESTART)),
            DevAction::MigrationRestart
        );
        assert_eq!(
            action_for_target(Some(DEV_MIGRATION_START_OVER)),
            DevAction::MigrationStartOver
        );
        assert_eq!(
            action_for_target(Some("dev.migration.stage.v3")),
            DevAction::SelectMigrationStage("v3".to_owned())
        );
    }

    #[test]
    fn new_and_rename_controls_emit_commands_and_edit_the_selected_label() {
        let mut state = DevState::new("source".to_owned());
        state.handle_event(&pointer(PointerPhase::Down), |_, _| {
            Some(DEV_NEW.to_owned())
        });
        let result =
            state.handle_event(&pointer(PointerPhase::Up), |_, _| Some(DEV_NEW.to_owned()));
        assert_eq!(result.action, DevAction::NewProject);

        state.begin_rename("Old name");
        assert_eq!(state.focused_target(), Some(DEV_RENAME_INPUT));
        assert_eq!(state.selected_text(), "Old name");
        let result = state.handle_event(
            &HostEvent::TextInput(TextInputEvent {
                surface: SurfaceId("dev".to_owned()),
                text: "New name".to_owned(),
            }),
            |_, _| None,
        );
        assert_eq!(result.change, DevChange::Rename);
        assert_eq!(state.rename_text().as_deref(), Some("New name"));
        let result = state.handle_event(
            &HostEvent::Keyboard(KeyEvent {
                surface: SurfaceId("dev".to_owned()),
                physical_key: None,
                logical_key: LogicalKey::Named("Return".to_owned()),
                pressed: true,
            }),
            |_, _| None,
        );
        assert_eq!(result.action, DevAction::CommitRename);
    }

    #[test]
    fn custom_file_controls_preserve_the_name_edit_target() {
        let mut state = DevState::new("source".to_owned());
        for (target, action) in [
            (DEV_FILE_NEW, DevAction::NewFile),
            (DEV_FILE_RENAME, DevAction::BeginFileRename),
            (DEV_FILE_REMOVE, DevAction::RemoveFile),
        ] {
            state.handle_event(&pointer(PointerPhase::Down), |_, _| Some(target.to_owned()));
            let result =
                state.handle_event(&pointer(PointerPhase::Up), |_, _| Some(target.to_owned()));
            assert_eq!(result.action, action);
        }

        state.begin_new_file("Module.bn");
        assert_eq!(state.name_edit_target(), Some(NameEditTarget::NewFile));
        assert_eq!(state.rename_prompt(), Some("New file"));
        state.finish_rename();
        state.begin_file_rename(3, "Store.bn");
        assert_eq!(state.name_edit_target(), Some(NameEditTarget::File(3)));
        assert_eq!(state.rename_prompt(), Some("File name"));
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

    #[test]
    fn clicking_a_caret_clears_the_hover_inspector_override() {
        let mut state = DevState::new("one two".to_owned());
        state.set_inspector_position(Some(Position { line: 0, column: 1 }));
        state.set_caret(Position { line: 0, column: 6 }, false);
        assert_eq!(state.inspection_position(), Position { line: 0, column: 6 });
    }

    #[test]
    fn row_hits_keep_mouse_drag_selection_active() {
        let mut state = DevState::new("select this".to_owned());
        state.handle_event(&pointer(PointerPhase::Down), |_, _| {
            Some("dev.editor.row.0".to_owned())
        });
        state.set_caret(Position { line: 0, column: 0 }, false);
        state.handle_event(&pointer(PointerPhase::Move), |_, _| {
            Some("dev.editor.row.0".to_owned())
        });
        assert!(state.dragging_editor());
        state.set_caret(Position { line: 0, column: 6 }, true);
        state.handle_event(&pointer(PointerPhase::Up), |_, _| {
            Some("dev.editor.row.0".to_owned())
        });
        assert_eq!(state.selected_text(), "select");
        assert!(!state.dragging_editor());
    }
}
