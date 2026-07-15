use boon_document::{
    DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, ScrollRootId, StyleValue,
    TextValue,
};
use boon_document_model::{ScrollState, SourceBinding, SourceBindingId};
use boon_editor::{Buffer, Selection};

use crate::language::LanguageSnapshot;
use crate::protocol::{
    AuthoritySelection, CatalogItem, MigrationStage, OutboxSampleState, PersistenceSnapshot,
};
use crate::workspace::ProjectOrigin;

pub const DEV_PREVIOUS: &str = "dev.previous";
pub const DEV_NEXT: &str = "dev.next";
pub const DEV_RUN: &str = "dev.run";
pub const DEV_RESET: &str = "dev.reset";
pub const DEV_TEST: &str = "dev.test";
pub const DEV_SAVE: &str = "dev.save";
pub const DEV_FORMAT: &str = "dev.format";
pub const DEV_NEW: &str = "dev.new";
pub const DEV_RENAME: &str = "dev.rename";
pub const DEV_RENAME_INPUT: &str = "dev.rename.input";
pub const DEV_RENAME_SAVE: &str = "dev.rename.save";
pub const DEV_RENAME_CANCEL: &str = "dev.rename.cancel";
pub const DEV_REMOVE: &str = "dev.remove";
pub const DEV_FILE_NEW: &str = "dev.file.new";
pub const DEV_FILE_RENAME: &str = "dev.file.rename";
pub const DEV_FILE_REMOVE: &str = "dev.file.remove";
pub const DEV_EDITOR: &str = "dev.editor";
pub const DEV_EDITOR_INPUT_TARGET: &str = "dev.editor.code.0";
pub const DEV_MIGRATION_PREVIEW: &str = "dev.migration.preview";
pub const DEV_MIGRATION_ACTIVATE: &str = "dev.migration.activate";
pub const DEV_MIGRATION_RESTART: &str = "dev.migration.restart";
pub const DEV_MIGRATION_START_OVER: &str = "dev.migration.start_over";
pub const DEV_MIGRATION_STAGE_PREFIX: &str = "dev.migration.stage.";
pub const DEV_INSPECT_VALUE: &str = "dev.inspect.value";
pub const DEV_INSPECT_PERSISTENCE: &str = "dev.inspect.persistence";
pub const DEV_INSPECT_OUTBOX: &str = "dev.inspect.outbox";
pub const DEV_PERSISTENCE_FLUSH: &str = "dev.persistence.flush";
pub const DEV_PERSISTENCE_COMPACT: &str = "dev.persistence.compact";
pub const DEV_PERSISTENCE_CLEAR_ALL: &str = "dev.persistence.clear_all";
pub const DEV_PERSISTENCE_CLEAR_SELECTED: &str = "dev.persistence.clear_selected";
pub const DEV_PERSISTENCE_EXPORT: &str = "dev.persistence.export";
pub const DEV_PERSISTENCE_IMPORT_PREVIEW: &str = "dev.persistence.import_preview";
pub const DEV_PERSISTENCE_ACTIVATE_IMPORT: &str = "dev.persistence.activate_import";
pub const DEV_OUTBOX_PREVIOUS: &str = "dev.outbox.previous";
pub const DEV_OUTBOX_NEXT: &str = "dev.outbox.next";

const EDITOR_LINE_HEIGHT: f64 = 23.0;
const EDITOR_WINDOW_LINES: usize = 36;
pub const OUTBOX_WINDOW_ROWS: usize = 6;
const DEV_BG: &str = "#0f1724";
const DEV_PANEL: &str = "#141b2a";
const DEV_PANEL_RAISED: &str = "#1a2435";
const DEV_PANEL_ACTIVE: &str = "#26354d";
const DEV_BORDER: &str = "#334155";
const DEV_BORDER_MUTED: &str = "#243244";
const DEV_TEXT: &str = "#eef2ff";
const DEV_TEXT_MUTED: &str = "#9aa8bd";
const DEV_ACCENT: &str = "#6ca2ff";
const EDITOR_BG: &str = "#282c34";
const EDITOR_BG_DARK: &str = "#21252b";
const EDITOR_BG_ACTIVE: &str = "#2c313a";
const EDITOR_TEXT: &str = "#d9e1f2";
const EDITOR_GUTTER: &str = "#5c6773";
const EDITOR_SELECTION: &str = "#3E4451";
const EDITOR_CARET: &str = "#528bff";
const EDITOR_BRACKET: &str = "#528bff40";

pub struct InspectorState<'a> {
    pub symbol: &'a str,
    pub static_type: &'a str,
    pub detail: &'a str,
    pub current_value: &'a str,
}

pub struct MigrationUiState<'a> {
    pub stages: &'a [MigrationStage],
    pub active_stage: &'a str,
    pub selected_stage: &'a str,
    pub previewed_stage: Option<&'a str>,
    pub status: &'a str,
    pub start_over_armed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorMode {
    Value,
    Persistence,
    Outbox,
}

pub struct PersistenceUiState<'a> {
    pub snapshot: Option<&'a PersistenceSnapshot>,
    pub mode: InspectorMode,
    pub outbox_offset: usize,
    pub clear_all_armed: bool,
    pub clear_selected_armed: bool,
    pub selected_authority: Option<&'a AuthoritySelection>,
    pub has_state_artifact: bool,
}

pub struct DevFrameState<'a> {
    pub catalog: &'a [CatalogItem],
    pub active_id: &'a str,
    pub example_label: &'a str,
    pub origin: ProjectOrigin,
    pub source_paths: &'a [String],
    pub active_file: usize,
    pub buffer: &'a Buffer,
    pub rename_buffer: Option<&'a Buffer>,
    pub rename_prompt: Option<&'a str>,
    pub editor_scroll: f32,
    pub language: Option<&'a LanguageSnapshot>,
    pub migration: Option<MigrationUiState<'a>>,
    pub inspector: InspectorState<'a>,
    pub persistence: PersistenceUiState<'a>,
    pub status: &'a str,
    pub perf: &'a str,
}

pub fn dev_frame(state: DevFrameState<'_>) -> DocumentFrame {
    let mut frame = DocumentFrame::empty("dev.root");
    style_root(frame.nodes.get_mut(&frame.root).expect("dev root"), DEV_BG);
    add_header(&mut frame);
    add_example_tabs(&mut frame, &state);
    add_file_tabs(&mut frame, &state);
    if let Some(migration) = state.migration.as_ref() {
        add_migration_bar(&mut frame, migration);
    }
    add_workspace(&mut frame, &state);
    add_footer(&mut frame, &state);
    frame
}

fn add_migration_bar(frame: &mut DocumentFrame, state: &MigrationUiState<'_>) {
    let mut strip = node("dev.migration", DocumentNodeKind::Row, Some("dev.root"));
    strip.style.insert("height".into(), number(42.0));
    strip.style.insert("width".into(), text("Fill"));
    strip.style.insert("gap".into(), number(4.0));
    strip.style.insert("padding".into(), number(5.0));
    strip.style.insert("background".into(), text("#172033"));
    strip.style.insert("border_bottom".into(), text(DEV_BORDER));
    strip
        .style
        .insert("border_bottom_width".into(), number(1.0));
    add(frame, "dev.root", strip);

    let mut title = label("dev.migration.title", "MIGRATION", "dev.migration");
    title.style.insert("width".into(), number(78.0));
    title.style.insert("height".into(), number(32.0));
    title.style.insert("font_size".into(), number(11.0));
    title.style.insert("font_weight".into(), text("700"));
    title.style.insert("color".into(), text("#72d6a5"));
    title.style.insert("vertical_align".into(), text("Center"));
    add(frame, "dev.migration", title);

    let active_index = state
        .stages
        .iter()
        .position(|stage| stage.id == state.active_stage)
        .unwrap_or(0);
    for (index, stage) in state.stages.iter().enumerate() {
        let id = format!("{DEV_MIGRATION_STAGE_PREFIX}{}", stage.id);
        if index > active_index {
            let mut stage_button = button(&id, &stage.id, 40.0, "dev.migration", false);
            stage_button
                .style
                .insert("accessibility_label".into(), text(&stage.label));
            if stage.id == state.selected_stage {
                stage_button
                    .style
                    .insert("background".into(), text(DEV_PANEL_ACTIVE));
                stage_button.style.insert("border".into(), text("#72d6a5"));
                stage_button.style.insert("color".into(), text(DEV_TEXT));
            }
            add(frame, "dev.migration", stage_button);
        } else {
            let mut stage_label = label(&id, &stage.id, "dev.migration");
            stage_label.style.insert("width".into(), number(40.0));
            stage_label.style.insert("height".into(), number(32.0));
            stage_label.style.insert(
                "background".into(),
                text(if stage.id == state.active_stage {
                    "#20543d"
                } else {
                    DEV_PANEL
                }),
            );
            stage_label.style.insert(
                "color".into(),
                text(if stage.id == state.active_stage {
                    "#b9f6d5"
                } else {
                    DEV_TEXT_MUTED
                }),
            );
            stage_label
                .style
                .insert("text_align".into(), text("Center"));
            stage_label
                .style
                .insert("vertical_align".into(), text("Center"));
            add(frame, "dev.migration", stage_label);
        }
    }

    let mut status = label("dev.migration.status", state.status, "dev.migration");
    status.style.insert("width".into(), text("Fill"));
    status.style.insert("height".into(), number(32.0));
    status.style.insert("font_size".into(), number(11.0));
    status.style.insert("color".into(), text(DEV_TEXT_MUTED));
    status.style.insert("vertical_align".into(), text("Center"));
    add(frame, "dev.migration", status);

    if state.selected_stage != state.active_stage {
        add(
            frame,
            "dev.migration",
            button(
                DEV_MIGRATION_PREVIEW,
                "Preview",
                62.0,
                "dev.migration",
                true,
            ),
        );
        if state.previewed_stage == Some(state.selected_stage) {
            add(
                frame,
                "dev.migration",
                button(
                    DEV_MIGRATION_ACTIVATE,
                    "Activate",
                    66.0,
                    "dev.migration",
                    true,
                ),
            );
        }
    }
    add(
        frame,
        "dev.migration",
        button(
            DEV_MIGRATION_RESTART,
            "Restart",
            60.0,
            "dev.migration",
            false,
        ),
    );
    add(
        frame,
        "dev.migration",
        button(
            DEV_MIGRATION_START_OVER,
            if state.start_over_armed {
                "Confirm Start Over"
            } else {
                "Start Over"
            },
            if state.start_over_armed { 118.0 } else { 76.0 },
            "dev.migration",
            false,
        ),
    );
}

fn add_header(frame: &mut DocumentFrame) {
    let mut header = node("dev.header", DocumentNodeKind::Row, Some("dev.root"));
    header.style.insert("height".into(), number(40.0));
    header.style.insert("width".into(), text("Fill"));
    header.style.insert("gap".into(), number(4.0));
    header.style.insert("padding".into(), number(4.0));
    header
        .style
        .insert("background".into(), text(DEV_PANEL_RAISED));
    header
        .style
        .insert("border_bottom".into(), text(DEV_BORDER));
    header
        .style
        .insert("border_bottom_width".into(), number(1.0));
    add(frame, "dev.root", header);

    add(
        frame,
        "dev.header",
        button(DEV_TEST, "TEST", 52.0, "dev.header", true),
    );
    add_brand(frame);

    for (id, label, width, accent) in [
        (DEV_RUN, "Run", 44.0, true),
        (DEV_SAVE, "Save", 48.0, false),
        (DEV_FORMAT, "Fmt", 44.0, false),
        (DEV_RESET, "Reset", 50.0, false),
    ] {
        add(
            frame,
            "dev.header",
            button(id, label, width, "dev.header", accent),
        );
    }

    let mut spacer = label("dev.header.spacer", "", "dev.header");
    spacer.style.insert("width".into(), text("Fill"));
    add(frame, "dev.header", spacer);
}

fn add_brand(frame: &mut DocumentFrame) {
    let mut brand = node("dev.brand", DocumentNodeKind::Row, Some("dev.header"));
    brand.style.insert("width".into(), number(96.0));
    brand.style.insert("height".into(), number(32.0));
    brand.style.insert("background".into(), text("#00000000"));
    add(frame, "dev.header", brand);

    for (id, value, color, underline) in [
        ("dev.brand.boon", "Boon", "#6cb6ff", true),
        ("dev.brand.slash", "/", "#d2691e", false),
        ("dev.brand.play", "play", "#fcbf49", false),
    ] {
        let mut part = label(id, value, "dev.brand");
        part.style.insert("width".into(), text("Auto"));
        part.style.insert("height".into(), text("Fill"));
        part.style.insert("auto_padding".into(), number(0.0));
        part.style.insert("background".into(), text("#00000000"));
        part.style.insert("font".into(), text("JetBrains Mono"));
        part.style.insert("font_size".into(), number(16.0));
        part.style.insert("font_weight".into(), text("700"));
        part.style.insert("color".into(), text(color));
        part.style.insert("vertical_align".into(), text("Center"));
        if underline {
            part.style
                .insert("underline_if".into(), StyleValue::Bool(true));
            part.style.insert("underline_color".into(), text(color));
        }
        add(frame, "dev.brand", part);
    }
}

fn add_example_tabs(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    let mut strip = node("dev.examples", DocumentNodeKind::Row, Some("dev.root"));
    strip.style.insert("height".into(), number(40.0));
    strip.style.insert("width".into(), text("Fill"));
    strip.style.insert("gap".into(), number(2.0));
    strip.style.insert("padding".into(), number(4.0));
    strip.style.insert("background".into(), text(DEV_PANEL));
    add(frame, "dev.root", strip);

    add(
        frame,
        "dev.examples",
        button(DEV_PREVIOUS, "<", 30.0, "dev.examples", false),
    );
    add(
        frame,
        "dev.examples",
        button(DEV_NEXT, ">", 30.0, "dev.examples", false),
    );
    let mut new_example = button(DEV_NEW, "+ New", 64.0, "dev.examples", true);
    new_example
        .style
        .insert("accessibility_label".into(), text("Create new example"));
    add(frame, "dev.examples", new_example);
    if state.origin == ProjectOrigin::Custom {
        if let Some(rename) = state
            .rename_buffer
            .filter(|_| state.rename_prompt == Some("Example name"))
        {
            add_rename_controls(frame, "dev.examples", rename, "Example name");
        } else if state.rename_buffer.is_none() {
            let mut rename = button(DEV_RENAME, "Rename", 62.0, "dev.examples", false);
            rename
                .style
                .insert("accessibility_label".into(), text("Rename custom example"));
            add(frame, "dev.examples", rename);
            let mut remove = button(DEV_REMOVE, "Remove", 58.0, "dev.examples", false);
            remove
                .style
                .insert("accessibility_label".into(), text("Remove custom example"));
            add(frame, "dev.examples", remove);
        }
    }

    for entry in state.catalog {
        let id = format!("dev.example.{}", entry.id);
        let mut tab = button(&id, &entry.label, 116.0, "dev.examples", false);
        tab.style.insert("width".into(), text("Auto"));
        tab.style.insert("min_width".into(), number(84.0));
        tab.style.insert("auto_padding".into(), number(12.0));
        tab.style
            .insert("accessibility_label".into(), text(&entry.label));
        tab.style.insert("height".into(), number(32.0));
        if entry.id == state.active_id {
            tab.style
                .insert("background".into(), text(DEV_PANEL_ACTIVE));
            tab.style.insert("border".into(), text(DEV_ACCENT));
            tab.style.insert("color".into(), text(DEV_TEXT));
        } else if entry.custom {
            tab.style.insert("color".into(), text("#d6b6ff"));
        }
        add(frame, "dev.examples", tab);
    }
}

fn add_file_tabs(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    let mut strip = node("dev.files", DocumentNodeKind::Row, Some("dev.root"));
    strip.style.insert("height".into(), number(38.0));
    strip.style.insert("width".into(), text("Fill"));
    strip.style.insert("gap".into(), number(2.0));
    strip.style.insert("padding".into(), number(4.0));
    strip
        .style
        .insert("background".into(), text(DEV_PANEL_RAISED));
    strip.style.insert("border_bottom".into(), text(DEV_BORDER));
    strip
        .style
        .insert("border_bottom_width".into(), number(1.0));
    add(frame, "dev.root", strip);

    for (index, path) in state.source_paths.iter().enumerate() {
        let id = format!("dev.file.{index}");
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path);
        let mut tab = button(&id, name, 126.0, "dev.files", false);
        tab.style.insert("width".into(), text("Auto"));
        tab.style.insert("min_width".into(), number(64.0));
        tab.style.insert("auto_padding".into(), number(10.0));
        tab.style.insert("height".into(), number(30.0));
        if index == state.active_file {
            tab.style.insert("background".into(), text(DEV_BG));
            tab.style.insert("border".into(), text(DEV_BORDER));
            tab.style.insert("color".into(), text(DEV_TEXT));
        }
        add(frame, "dev.files", tab);
    }

    if state.origin == ProjectOrigin::Custom {
        if let Some(rename) = state
            .rename_buffer
            .filter(|_| state.rename_prompt != Some("Example name"))
        {
            add_rename_controls(
                frame,
                "dev.files",
                rename,
                state.rename_prompt.unwrap_or("Name"),
            );
        } else {
            add(
                frame,
                "dev.files",
                button(DEV_FILE_NEW, "+ File", 58.0, "dev.files", true),
            );
            add(
                frame,
                "dev.files",
                button(DEV_FILE_RENAME, "Rename", 62.0, "dev.files", false),
            );
            if state.source_paths.len() > 1 {
                add(
                    frame,
                    "dev.files",
                    button(DEV_FILE_REMOVE, "Remove", 58.0, "dev.files", false),
                );
            }
        }
    }
}

fn add_rename_controls(frame: &mut DocumentFrame, parent: &str, buffer: &Buffer, prompt: &str) {
    let mut prompt_node = label("dev.rename.prompt", prompt, parent);
    prompt_node.style.insert("width".into(), text("Auto"));
    prompt_node.style.insert("height".into(), number(30.0));
    prompt_node.style.insert("auto_padding".into(), number(6.0));
    prompt_node
        .style
        .insert("vertical_align".into(), text("Center"));
    prompt_node.style.insert("font_size".into(), number(11.0));
    prompt_node
        .style
        .insert("color".into(), text(DEV_TEXT_MUTED));
    add(frame, parent, prompt_node);
    let mut input = node(DEV_RENAME_INPUT, DocumentNodeKind::TextInput, Some(parent));
    input.text = Some(TextValue {
        text: buffer.text(),
    });
    input.style.insert("width".into(), number(160.0));
    input.style.insert("height".into(), number(30.0));
    input.style.insert("background".into(), text(EDITOR_BG));
    input.style.insert("border".into(), text(DEV_ACCENT));
    input.style.insert("border_width".into(), number(1.0));
    input.style.insert("color".into(), text(DEV_TEXT));
    input.style.insert("font_size".into(), number(12.0));
    input.style.insert("text_inset".into(), number(6.0));
    input.style.insert("vertical_align".into(), text("Center"));
    input
        .style
        .insert("caret_visible".into(), StyleValue::Bool(true));
    input
        .style
        .insert("caret_column".into(), number(buffer.caret().column as f64));
    let selection = buffer.selection();
    if !selection.is_collapsed() {
        let (start, end) = if selection.anchor <= selection.head {
            (selection.anchor.column, selection.head.column)
        } else {
            (selection.head.column, selection.anchor.column)
        };
        input
            .style
            .insert("selection_start".into(), number(start as f64));
        input
            .style
            .insert("selection_end".into(), number(end as f64));
        input
            .style
            .insert("selection_color".into(), text("#528bff55"));
    }
    input
        .source_bindings
        .push(binding(DEV_RENAME_INPUT, "edit"));
    add(frame, parent, input);
    add(
        frame,
        parent,
        button(DEV_RENAME_SAVE, "OK", 38.0, parent, true),
    );
    add(
        frame,
        parent,
        button(DEV_RENAME_CANCEL, "Cancel", 54.0, parent, false),
    );
}

fn add_workspace(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    let mut workspace = node("dev.workspace", DocumentNodeKind::Row, Some("dev.root"));
    workspace.style.insert("width".into(), text("Fill"));
    workspace.style.insert("height".into(), text("Fill"));
    workspace.style.insert("background".into(), text(DEV_BG));
    add(frame, "dev.root", workspace);

    let mut editor = node(
        DEV_EDITOR,
        DocumentNodeKind::ScrollRoot,
        Some("dev.workspace"),
    );
    editor.style.insert("width".into(), text("Fill"));
    editor.style.insert("height".into(), text("Fill"));
    editor.style.insert("background".into(), text(EDITOR_BG));
    editor.style.insert("border".into(), text(DEV_BORDER));
    editor
        .style
        .insert("border_right_width".into(), number(1.0));
    editor.scroll = Some(ScrollState { x: 0.0, y: 0.0 });
    editor.source_bindings.push(binding(DEV_EDITOR, "edit"));
    frame.scroll_roots.insert(
        ScrollRootId(DEV_EDITOR.to_owned()),
        ScrollState { x: 0.0, y: 0.0 },
    );
    add(frame, "dev.workspace", editor);

    let mut lines = node(
        "dev.editor.lines",
        DocumentNodeKind::Stack,
        Some(DEV_EDITOR),
    );
    lines.style.insert("width".into(), text("Fill"));
    lines.style.insert("height".into(), text("Fit"));
    lines.style.insert("background".into(), text(EDITOR_BG));
    lines.style.insert("padding_top".into(), number(8.0));
    lines.style.insert("padding_bottom".into(), number(18.0));
    add(frame, DEV_EDITOR, lines);

    let selection = state.buffer.selection();
    let first_line = editor_first_line(state.editor_scroll);
    let visible_count = state
        .buffer
        .line_count()
        .saturating_sub(first_line)
        .min(EDITOR_WINDOW_LINES);
    for slot in 0..visible_count {
        add_editor_line(frame, state, selection, slot, first_line + slot);
    }

    add_inspector(frame, state);
}

fn add_editor_line(
    frame: &mut DocumentFrame,
    state: &DevFrameState<'_>,
    selection: Selection,
    slot: usize,
    line_index: usize,
) {
    let row_id = format!("dev.editor.row.{slot}");
    let mut row = node(&row_id, DocumentNodeKind::Row, Some("dev.editor.lines"));
    row.style.insert("width".into(), text("Fill"));
    row.style
        .insert("height".into(), number(EDITOR_LINE_HEIGHT));
    row.style.insert("background".into(), text(EDITOR_BG));
    row.style
        .insert("__hover_background".into(), text(EDITOR_BG_ACTIVE));
    row.source_bindings.push(binding(&row_id, "edit"));
    add(frame, "dev.editor.lines", row);

    let mut gutter = label(
        &format!("dev.editor.gutter.{slot}"),
        &(line_index + 1).to_string(),
        &row_id,
    );
    gutter.style.insert("width".into(), number(48.0));
    gutter
        .style
        .insert("height".into(), number(EDITOR_LINE_HEIGHT));
    gutter.style.insert("font".into(), text("JetBrains Mono"));
    gutter.style.insert("font_size".into(), number(13.0));
    gutter.style.insert("color".into(), text(EDITOR_GUTTER));
    gutter.style.insert("text_align".into(), text("Right"));
    gutter.style.insert("vertical_align".into(), text("Center"));
    gutter.style.insert("padding_right".into(), number(10.0));
    gutter
        .source_bindings
        .push(binding(&format!("dev.editor.gutter.{slot}"), "edit"));
    add(frame, &row_id, gutter);

    let line_text = state.buffer.line(line_index);
    let code_id = format!("dev.editor.code.{slot}");
    let mut code = label(&code_id, &line_text, &row_id);
    code.style.insert("width".into(), text("Fill"));
    code.style
        .insert("height".into(), number(EDITOR_LINE_HEIGHT));
    code.style.insert("font".into(), text("JetBrains Mono"));
    code.style.insert("font_size".into(), number(14.0));
    code.style
        .insert("line_height".into(), number(EDITOR_LINE_HEIGHT));
    code.style.insert("vertical_align".into(), text("Center"));
    code.style.insert("color".into(), text(EDITOR_TEXT));
    code.style.insert("text_inset".into(), number(5.0));
    code.style
        .insert("editor_selection_color".into(), text(EDITOR_SELECTION));
    code.style
        .insert("editor_caret_color".into(), text(EDITOR_CARET));
    code.style
        .insert("editor_bracket_color".into(), text(EDITOR_BRACKET));
    if let Some(language) = state
        .language
        .filter(|language| language.file_index == state.active_file)
        && let Some(decorations) = language.lines.get(line_index)
    {
        code.style.insert(
            "syntax_spans".into(),
            StyleValue::RichTextSpans(decorations.spans.clone()),
        );
        code.style.insert(
            "editor_type_hints".into(),
            StyleValue::EditorTypeHints(decorations.type_hints.clone()),
        );
        code.style
            .insert("editor_type_hint_color".into(), text("#8aa0b8"));
    }
    add_selection_style(&mut code, selection, state.buffer, line_index);
    if selection.is_collapsed() && selection.head.line == line_index {
        code.style
            .insert("editor_caret_visible".into(), StyleValue::Bool(true));
        code.style.insert(
            "editor_caret_column".into(),
            number(selection.head.column as f64),
        );
        if let Some(columns) = bracket_columns(&line_text, selection.head.column) {
            code.style
                .insert("editor_bracket_columns".into(), text(&columns));
        }
    }
    code.source_bindings.push(binding(&code_id, "edit"));
    add(frame, &row_id, code);
}

fn add_selection_style(
    code: &mut DocumentNode,
    selection: Selection,
    buffer: &Buffer,
    line: usize,
) {
    if selection.is_collapsed() {
        return;
    }
    let (start, end) = if selection.anchor <= selection.head {
        (selection.anchor, selection.head)
    } else {
        (selection.head, selection.anchor)
    };
    if line < start.line || line > end.line {
        return;
    }
    let start_column = if line == start.line { start.column } else { 0 };
    let end_column = if line == end.line {
        end.column
    } else {
        buffer.line(line).chars().count().saturating_add(1)
    };
    code.style
        .insert("editor_selection_start".into(), number(start_column as f64));
    code.style
        .insert("editor_selection_end".into(), number(end_column as f64));
}

fn add_inspector(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    let mut inspector = node(
        "dev.inspector",
        DocumentNodeKind::Stack,
        Some("dev.workspace"),
    );
    inspector.style.insert("width".into(), number(340.0));
    inspector.style.insert("height".into(), text("Fill"));
    inspector.style.insert("padding".into(), number(10.0));
    inspector.style.insert("gap".into(), number(6.0));
    inspector.style.insert("background".into(), text(DEV_PANEL));
    add(frame, "dev.workspace", inspector);

    let mut title = label("dev.inspector.title", "STATE", "dev.inspector");
    title.style.insert("height".into(), number(20.0));
    title.style.insert("font_size".into(), number(12.0));
    title.style.insert("font_weight".into(), text("700"));
    title.style.insert("color".into(), text(DEV_ACCENT));
    title.style.insert("vertical_align".into(), text("Center"));
    add(frame, "dev.inspector", title);
    add_inspector_tabs(frame, state.persistence.mode);
    match state.persistence.mode {
        InspectorMode::Value => add_value_inspector(frame, state),
        InspectorMode::Persistence => add_persistence_inspector(frame, &state.persistence),
        InspectorMode::Outbox => add_outbox_inspector(frame, &state.persistence),
    }
}

fn add_inspector_tabs(frame: &mut DocumentFrame, active: InspectorMode) {
    let mut tabs = node(
        "dev.inspector.tabs",
        DocumentNodeKind::Row,
        Some("dev.inspector"),
    );
    tabs.style.insert("width".into(), text("Fill"));
    tabs.style.insert("height".into(), number(30.0));
    tabs.style.insert("gap".into(), number(3.0));
    add(frame, "dev.inspector", tabs);
    for (id, caption, width, mode) in [
        (DEV_INSPECT_VALUE, "Value", 65.0, InspectorMode::Value),
        (
            DEV_INSPECT_PERSISTENCE,
            "Persistence",
            94.0,
            InspectorMode::Persistence,
        ),
        (DEV_INSPECT_OUTBOX, "Outbox", 70.0, InspectorMode::Outbox),
    ] {
        let mut tab = button(id, caption, width, "dev.inspector.tabs", false);
        if mode == active {
            tab.style.insert("background".into(), text("#315f9d"));
            tab.style.insert("border".into(), text(DEV_ACCENT));
        }
        add(frame, "dev.inspector.tabs", tab);
    }
}

fn add_value_inspector(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    inspector_field(frame, "Symbol", state.inspector.symbol, 38.0, "symbol");
    inspector_field(
        frame,
        "Static type",
        state.inspector.static_type,
        52.0,
        "type",
    );
    inspector_field(
        frame,
        "Current value",
        state.inspector.current_value,
        82.0,
        "value",
    );
    inspector_field(frame, "Details", state.inspector.detail, 116.0, "detail");
    if let Some(language) = state.language
        && !language.diagnostics.is_empty()
    {
        inspector_field(
            frame,
            "Diagnostics",
            &language.diagnostics.join("\n"),
            98.0,
            "diagnostics",
        );
    }
}

fn add_persistence_inspector(frame: &mut DocumentFrame, state: &PersistenceUiState<'_>) {
    let summary = state.snapshot.map_or_else(
        || "Waiting for preview snapshot".to_owned(),
        |snapshot| {
            let stored = snapshot.stored.as_ref().map_or_else(
                || "unavailable".to_owned(),
                |stored| {
                    let bytes = stored.encoded_value_bytes.map_or_else(
                        || "size unknown".to_owned(),
                        |bytes| format!("{bytes} bytes"),
                    );
                    format!(
                        "e{} t{} | {} scalar, {} list, {} rows, {} artifacts / {} bytes, {bytes}",
                        stored.epoch,
                        stored.through_turn_sequence,
                        stored.scalar_count,
                        stored.list_count,
                        stored.row_count,
                        stored.content_artifact_count,
                        stored.content_artifact_bytes
                    )
                },
            );
            let pending = snapshot.pending.first_turn_sequence.zip(
                snapshot.pending.last_turn_sequence,
            ).map_or_else(
                || "none".to_owned(),
                |(first, last)| {
                    format!(
                        "{first}..{last} ({}), {}ms",
                        snapshot.pending.turn_count, snapshot.pending.oldest_age_millis
                    )
                },
            );
            format!(
                "Authority  turn {} / source {}\nDeclared   {} scalar, {} indexed, {} list\nStored     {stored}\nPending    {pending}; queue {} + {} reserved\nDurable    epoch {}, turn {}\nTimings    enqueue {}us, encode {}us, commit {}us\n            barrier {}us, restore {}us, migrate {}us, rebuild {}us\nSchema     v{} {}\nWorker     {}{}",
                snapshot.authority.runtime_turn_sequence,
                snapshot.authority.source_event_sequence,
                snapshot.authority.scalar_count,
                snapshot.authority.indexed_field_count,
                snapshot.authority.list_count,
                snapshot.pending.queue_depth,
                snapshot.pending.reserved_slots,
                snapshot.durable.epoch,
                snapshot.durable.through_turn_sequence,
                snapshot.timings.authority_enqueue_us,
                snapshot.timings.encode_us,
                snapshot.timings.checkpoint_us,
                snapshot.timings.barrier_us,
                snapshot.timings.restore_us,
                snapshot.timings.migration_us,
                snapshot.timings.rebuild_derived_us,
                snapshot.schema_version,
                short_digest(&snapshot.schema_hash),
                if snapshot.worker_alive { "online" } else { "offline" },
                if snapshot.pending.accepting_turns { "" } else { ", paused" },
            )
        },
    );
    inspector_field(frame, "Persistence", &summary, 212.0, "persistence.summary");

    let mut controls = node(
        "dev.persistence.controls",
        DocumentNodeKind::Row,
        Some("dev.inspector"),
    );
    controls.style.insert("width".into(), text("Fill"));
    controls.style.insert("height".into(), number(32.0));
    controls.style.insert("gap".into(), number(4.0));
    add(frame, "dev.inspector", controls);
    add(
        frame,
        "dev.persistence.controls",
        button(
            DEV_PERSISTENCE_FLUSH,
            "Flush",
            58.0,
            "dev.persistence.controls",
            true,
        ),
    );
    add(
        frame,
        "dev.persistence.controls",
        button(
            DEV_PERSISTENCE_COMPACT,
            "Maintain",
            74.0,
            "dev.persistence.controls",
            false,
        ),
    );
    add(
        frame,
        "dev.persistence.controls",
        button(
            DEV_PERSISTENCE_CLEAR_ALL,
            if state.clear_all_armed {
                "Confirm Clear All"
            } else {
                "Clear All"
            },
            if state.clear_all_armed { 126.0 } else { 72.0 },
            "dev.persistence.controls",
            false,
        ),
    );

    let mut artifact_controls = node(
        "dev.persistence.artifact_controls",
        DocumentNodeKind::Row,
        Some("dev.inspector"),
    );
    artifact_controls.style.insert("width".into(), text("Fill"));
    artifact_controls
        .style
        .insert("height".into(), number(32.0));
    artifact_controls.style.insert("gap".into(), number(4.0));
    add(frame, "dev.inspector", artifact_controls);
    let clear_selected = state.snapshot.map(|snapshot| {
        (
            snapshot.capabilities.clear_selected.available && state.selected_authority.is_some(),
            snapshot.capabilities.clear_selected.reason.as_str(),
        )
    });
    let export = state.snapshot.map(|snapshot| {
        (
            snapshot.capabilities.export_state.available,
            snapshot.capabilities.export_state.reason.as_str(),
        )
    });
    let import = state.snapshot.map(|snapshot| {
        (
            snapshot.capabilities.import_preview.available && state.has_state_artifact,
            snapshot.capabilities.import_preview.reason.as_str(),
        )
    });
    add(
        frame,
        "dev.persistence.artifact_controls",
        capability_button(
            DEV_PERSISTENCE_CLEAR_SELECTED,
            if state.clear_selected_armed {
                "Confirm Clear"
            } else {
                "Clear Selected"
            },
            104.0,
            "dev.persistence.artifact_controls",
            clear_selected,
        ),
    );
    add(
        frame,
        "dev.persistence.artifact_controls",
        capability_button(
            DEV_PERSISTENCE_EXPORT,
            "Export",
            60.0,
            "dev.persistence.artifact_controls",
            export,
        ),
    );
    add(
        frame,
        "dev.persistence.artifact_controls",
        capability_button(
            DEV_PERSISTENCE_IMPORT_PREVIEW,
            "Import Preview",
            108.0,
            "dev.persistence.artifact_controls",
            import,
        ),
    );

    let activate = state.snapshot.map(|snapshot| {
        (
            snapshot.capabilities.activate_import.available && snapshot.import_preview.is_some(),
            snapshot.capabilities.activate_import.reason.as_str(),
        )
    });
    add(
        frame,
        "dev.inspector",
        capability_button(
            DEV_PERSISTENCE_ACTIVATE_IMPORT,
            "Activate Import",
            112.0,
            "dev.inspector",
            activate,
        ),
    );

    if let Some(snapshot) = state.snapshot {
        let operation = snapshot
            .last_operation
            .as_ref()
            .map_or("No persistence command in this session", |operation| {
                operation.message.as_str()
            });
        inspector_field(
            frame,
            "Last operation",
            operation,
            62.0,
            "persistence.operation",
        );
        inspector_field(
            frame,
            "Last actionable error",
            snapshot.last_actionable_error.as_deref().unwrap_or("None"),
            82.0,
            "persistence.error",
        );
        if let Some(preview) = snapshot.import_preview.as_ref() {
            let detail = format!(
                "Preview #{}: schema v{} -> v{}\n{} scalar, {} list, {} rows; {} document nodes\nActive runtime and durable namespace unchanged",
                preview.preview_id,
                preview.source_schema_version,
                preview.target_schema_version,
                preview.scalar_count,
                preview.list_count,
                preview.row_count,
                preview.document_node_count,
            );
            inspector_field(
                frame,
                "Import Preview",
                &detail,
                72.0,
                "persistence.import_preview",
            );
        }
        let unavailable = [
            &snapshot.capabilities.clear_selected,
            &snapshot.capabilities.export_state,
            &snapshot.capabilities.import_preview,
            &snapshot.capabilities.activate_import,
        ]
        .into_iter()
        .filter(|capability| !capability.available)
        .map(|capability| capability.reason.as_str())
        .collect::<Vec<_>>()
        .join("\n");
        if !unavailable.is_empty() {
            inspector_field(
                frame,
                "Unavailable core operations",
                &unavailable,
                86.0,
                "persistence.capabilities",
            );
        }
    }
}

fn add_outbox_inspector(frame: &mut DocumentFrame, state: &PersistenceUiState<'_>) {
    let Some(snapshot) = state.snapshot else {
        inspector_field(
            frame,
            "Outbox",
            "Waiting for preview snapshot",
            72.0,
            "outbox.summary",
        );
        return;
    };
    let samples = &snapshot.outbox.samples;
    let offset = state
        .outbox_offset
        .min(samples.len().saturating_sub(OUTBOX_WINDOW_ROWS));
    let end = offset.saturating_add(OUTBOX_WINDOW_ROWS).min(samples.len());
    let summary = format!(
        "Pending {} | Dispatching {}\nReconcile {} | Completed {}\nShowing {}..{} of {} bounded samples",
        snapshot.outbox.pending_count,
        snapshot.outbox.dispatching_count,
        snapshot.outbox.reconciliation_count,
        snapshot.outbox.completed_count,
        if samples.is_empty() { 0 } else { offset + 1 },
        end,
        samples.len(),
    );
    inspector_field(frame, "Durable outbox", &summary, 66.0, "outbox.summary");

    if samples.len() > OUTBOX_WINDOW_ROWS {
        let mut navigation = node(
            "dev.outbox.navigation",
            DocumentNodeKind::Row,
            Some("dev.inspector"),
        );
        navigation.style.insert("width".into(), text("Fill"));
        navigation.style.insert("height".into(), number(30.0));
        navigation.style.insert("gap".into(), number(4.0));
        add(frame, "dev.inspector", navigation);
        let mut previous = button(
            DEV_OUTBOX_PREVIOUS,
            "<",
            32.0,
            "dev.outbox.navigation",
            false,
        );
        previous.style.insert(
            "accessibility_label".into(),
            text("Previous outbox samples"),
        );
        add(frame, "dev.outbox.navigation", previous);
        let mut next = button(DEV_OUTBOX_NEXT, ">", 32.0, "dev.outbox.navigation", false);
        next.style
            .insert("accessibility_label".into(), text("Next outbox samples"));
        add(frame, "dev.outbox.navigation", next);
    }

    for (slot, sample) in samples[offset..end].iter().enumerate() {
        let status = match sample.state {
            OutboxSampleState::Pending => "Pending",
            OutboxSampleState::Dispatching => "Dispatching",
            OutboxSampleState::ReconciliationRequired => "Reconcile",
            OutboxSampleState::Completed => "Completed",
        };
        let value = format!(
            "{}  attempt {}\nitem {}  effect {}\nturn {} -> {}",
            status,
            sample.attempt,
            short_digest(&sample.item_id),
            short_digest(&sample.effect_id),
            sample.created_turn_sequence,
            sample.updated_turn_sequence,
        );
        inspector_field(
            frame,
            &format!("Sample {}", offset + slot + 1),
            &value,
            48.0,
            &format!("outbox.sample.{slot}"),
        );
    }
}

fn short_digest(value: &[u8; 32]) -> String {
    value[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn inspector_field(frame: &mut DocumentFrame, name: &str, value: &str, height: f64, key: &str) {
    let mut heading = label(
        &format!("dev.inspector.{key}.heading"),
        name,
        "dev.inspector",
    );
    heading.style.insert("height".into(), number(19.0));
    heading.style.insert("font_size".into(), number(11.0));
    heading.style.insert("font_weight".into(), text("600"));
    heading.style.insert("color".into(), text(DEV_TEXT_MUTED));
    heading
        .style
        .insert("vertical_align".into(), text("Center"));
    add(frame, "dev.inspector", heading);
    let max_lines = ((height - 14.0) / 16.0).floor().max(1.0) as usize;
    let bounded = bounded_panel_text(if value.is_empty() { "-" } else { value }, 31, max_lines);
    let mut body = label(
        &format!("dev.inspector.{key}.body"),
        &bounded,
        "dev.inspector",
    );
    body.style.insert("width".into(), text("Fill"));
    body.style.insert("height".into(), number(height));
    body.style.insert("font".into(), text("JetBrains Mono"));
    body.style.insert("font_size".into(), number(11.0));
    body.style.insert("color".into(), text(EDITOR_TEXT));
    body.style.insert("background".into(), text(EDITOR_BG_DARK));
    body.style.insert("padding".into(), number(8.0));
    body.style.insert("border".into(), text(DEV_BORDER_MUTED));
    body.style.insert("border_width".into(), number(1.0));
    add(frame, "dev.inspector", body);
}

fn bounded_panel_text(value: &str, columns: usize, max_lines: usize) -> String {
    let mut lines = Vec::new();
    for raw_line in value.lines().chain(value.is_empty().then_some("")) {
        let mut remaining = raw_line.trim_end();
        if remaining.is_empty() {
            lines.push(String::new());
            continue;
        }
        while !remaining.is_empty() {
            let end = remaining
                .char_indices()
                .nth(columns)
                .map_or(remaining.len(), |(index, _)| index);
            let candidate = &remaining[..end];
            let split = if end < remaining.len() {
                candidate
                    .rfind(char::is_whitespace)
                    .filter(|index| *index > columns / 2)
                    .unwrap_or(end)
            } else {
                end
            };
            lines.push(remaining[..split].trim().to_owned());
            remaining = remaining[split..].trim_start();
        }
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let keep = columns.saturating_sub(3);
            *last = format!("{}...", last.chars().take(keep).collect::<String>());
        }
    }
    lines.join("\n")
}

fn add_footer(frame: &mut DocumentFrame, state: &DevFrameState<'_>) {
    let mut footer = node("dev.footer", DocumentNodeKind::Row, Some("dev.root"));
    footer.style.insert("height".into(), number(30.0));
    footer.style.insert("width".into(), text("Fill"));
    footer.style.insert("gap".into(), number(16.0));
    footer.style.insert("padding".into(), number(6.0));
    footer
        .style
        .insert("background".into(), text(DEV_PANEL_RAISED));
    footer.style.insert("border_top".into(), text(DEV_BORDER));
    footer.style.insert("border_top_width".into(), number(1.0));
    add(frame, "dev.root", footer);
    let identity_text = format!("{}  {}", state.example_label, state.origin.badge());
    let mut identity = footer_label("dev.identity", &identity_text);
    identity.style.insert("width".into(), text("Auto"));
    identity.style.insert("auto_padding".into(), number(4.0));
    identity.style.insert(
        "color".into(),
        text(if state.origin == ProjectOrigin::BuiltIn {
            DEV_ACCENT
        } else {
            "#d1a6ff"
        }),
    );
    add(frame, "dev.footer", identity);
    add(
        frame,
        "dev.footer",
        footer_label("dev.status", state.status),
    );
    add(frame, "dev.footer", footer_label("dev.perf", state.perf));
}

pub fn message_frame(title: &str, message: &str, background: &str) -> DocumentFrame {
    let mut frame = DocumentFrame::empty("message.root");
    style_root(
        frame.nodes.get_mut(&frame.root).expect("message root"),
        background,
    );
    let mut stack = node(
        "message.stack",
        DocumentNodeKind::Stack,
        Some("message.root"),
    );
    stack.style.insert("width".into(), text("Fill"));
    stack.style.insert("height".into(), text("Fill"));
    stack.style.insert("padding".into(), number(32.0));
    stack.style.insert("gap".into(), number(14.0));
    add(&mut frame, "message.root", stack);
    let mut title_node = label("message.title", title, "message.stack");
    title_node.style.insert("font_size".into(), number(28.0));
    title_node.style.insert("height".into(), number(44.0));
    add(&mut frame, "message.stack", title_node);
    let mut body = label("message.body", message, "message.stack");
    body.style.insert("font_size".into(), number(15.0));
    body.style.insert("height".into(), text("Fill"));
    add(&mut frame, "message.stack", body);
    frame
}

fn bracket_columns(line: &str, caret: usize) -> Option<String> {
    let chars = line.chars().collect::<Vec<_>>();
    let candidate = [caret, caret.saturating_sub(1)]
        .into_iter()
        .find(|column| {
            chars
                .get(*column)
                .is_some_and(|value| "[]{}()".contains(*value))
        })?;
    let bracket = chars[candidate];
    let (other, direction) = match bracket {
        '(' => (')', 1_isize),
        '[' => (']', 1),
        '{' => ('}', 1),
        ')' => ('(', -1),
        ']' => ('[', -1),
        '}' => ('{', -1),
        _ => return None,
    };
    let mut depth = 0_isize;
    let mut index = candidate as isize;
    loop {
        index += direction;
        let value = *chars.get(index as usize)?;
        if value == bracket {
            depth += 1;
        } else if value == other {
            if depth == 0 {
                return Some(format!("{candidate},{}", index as usize));
            }
            depth -= 1;
        }
    }
}

pub fn editor_line_from_target(target: &str) -> Option<usize> {
    ["dev.editor.code.", "dev.editor.row.", "dev.editor.gutter."]
        .into_iter()
        .find_map(|prefix| {
            target.strip_prefix(prefix).and_then(|suffix| {
                let digits = suffix
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>();
                (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
            })
        })
}

pub fn editor_first_line(scroll: f32) -> usize {
    (scroll.max(0.0) / EDITOR_LINE_HEIGHT as f32).floor() as usize
}

fn style_root(root: &mut DocumentNode, background: &str) {
    root.style.insert("width".into(), text("Fill"));
    root.style.insert("height".into(), text("Fill"));
    root.style.insert("background".into(), text(background));
    root.style.insert("color".into(), text(DEV_TEXT));
    root.style.insert("font_size".into(), number(14.0));
}

fn button(id: &str, label: &str, width: f64, parent: &str, accent: bool) -> DocumentNode {
    let mut button = node(id, DocumentNodeKind::Button, Some(parent));
    button.text = Some(TextValue {
        text: label.to_owned(),
    });
    button.style.insert("width".into(), number(width));
    button.style.insert("height".into(), number(30.0));
    button.style.insert("padding".into(), number(5.0));
    button.style.insert(
        "background".into(),
        text(if accent { "#315f9d" } else { DEV_PANEL_ACTIVE }),
    );
    button.style.insert(
        "__hover_background".into(),
        text(if accent { "#417ac2" } else { DEV_BORDER }),
    );
    button.style.insert(
        "__focus_background".into(),
        text(if accent { "#417ac2" } else { DEV_BORDER }),
    );
    button.style.insert(
        "border".into(),
        text(if accent { DEV_ACCENT } else { DEV_BORDER }),
    );
    button.style.insert("border_width".into(), number(1.0));
    button.style.insert("color".into(), text(DEV_TEXT));
    button.style.insert("font_size".into(), number(12.0));
    button.style.insert("font_weight".into(), text("600"));
    button.style.insert("vertical_align".into(), text("Center"));
    button.style.insert("text_align".into(), text("Center"));
    button.source_bindings.push(binding(id, "press"));
    button
}

fn capability_button(
    id: &str,
    label: &str,
    width: f64,
    parent: &str,
    capability: Option<(bool, &str)>,
) -> DocumentNode {
    let (available, reason) = capability.unwrap_or((false, "Waiting for persistence snapshot"));
    let mut control = button(id, label, width, parent, false);
    if !available {
        control.source_bindings.clear();
        control.style.insert("background".into(), text(DEV_PANEL));
        control.style.insert("color".into(), text("#66758b"));
        control
            .style
            .insert("border".into(), text(DEV_BORDER_MUTED));
        control.style.insert(
            "accessibility_label".into(),
            text(if reason.is_empty() {
                "Unavailable for the current selection or artifact cache"
            } else {
                reason
            }),
        );
    }
    control
}

fn footer_label(id: &str, value: &str) -> DocumentNode {
    let mut node = label(id, value, "dev.footer");
    node.style.insert("width".into(), text("Fill"));
    node.style.insert("height".into(), number(18.0));
    node.style.insert("font_size".into(), number(11.0));
    node.style.insert("color".into(), text(DEV_TEXT_MUTED));
    node.style.insert("vertical_align".into(), text("Center"));
    node
}

fn label(id: &str, value: &str, parent: &str) -> DocumentNode {
    let mut node = node(id, DocumentNodeKind::Text, Some(parent));
    node.text = Some(TextValue {
        text: value.to_owned(),
    });
    node
}

fn node(id: &str, kind: DocumentNodeKind, parent: Option<&str>) -> DocumentNode {
    let mut node = DocumentNode::new(id, kind);
    node.parent = parent.map(|value| DocumentNodeId(value.to_owned()));
    node
}

fn add(frame: &mut DocumentFrame, parent: &str, node: DocumentNode) {
    let id = node.id.clone();
    frame
        .nodes
        .get_mut(&DocumentNodeId(parent.to_owned()))
        .unwrap_or_else(|| panic!("missing parent {parent}"))
        .children
        .push(id.clone());
    frame.nodes.insert(id, node);
}

fn binding(id: &str, intent: &str) -> SourceBinding {
    SourceBinding {
        id: SourceBindingId(format!("binding:{id}")),
        source_path: id.to_owned(),
        intent: intent.to_owned(),
    }
}

fn text(value: &str) -> StyleValue {
    StyleValue::Text(value.to_owned())
}

fn number(value: f64) -> StyleValue {
    StyleValue::Number(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document::render_scene::ApproximateTextColumnMeasurer;
    use boon_host::Viewport;

    use crate::view::RetainedView;

    fn empty_persistence() -> PersistenceUiState<'static> {
        PersistenceUiState {
            snapshot: None,
            mode: InspectorMode::Value,
            outbox_offset: 0,
            clear_all_armed: false,
            clear_selected_armed: false,
            selected_authority: None,
            has_state_artifact: false,
        }
    }

    #[test]
    fn dev_frame_exposes_versioned_and_custom_identity_without_fixture_branches() {
        let buffer = Buffer::new("value: 1\n");
        let catalog = vec![CatalogItem {
            id: "example".to_owned(),
            label: "Example".to_owned(),
            custom: false,
        }];
        let paths = vec!["examples/example.bn".to_owned()];
        let frame = dev_frame(DevFrameState {
            catalog: &catalog,
            active_id: "example",
            example_label: "Example",
            origin: ProjectOrigin::BuiltIn,
            source_paths: &paths,
            active_file: 0,
            buffer: &buffer,
            rename_buffer: None,
            rename_prompt: None,
            editor_scroll: 0.0,
            language: None,
            migration: None,
            inspector: InspectorState {
                symbol: "value",
                static_type: "Number",
                detail: "Number",
                current_value: "1",
            },
            persistence: empty_persistence(),
            status: "Ready",
            perf: "Preview idle",
        });
        assert_eq!(
            frame.nodes[&DocumentNodeId("dev.identity".to_owned())]
                .text
                .as_ref()
                .unwrap()
                .text,
            "Example  BUILT-IN  VERSIONED"
        );
        assert!(
            frame
                .nodes
                .contains_key(&DocumentNodeId("dev.editor.code.0".to_owned()))
        );
        let brand = &frame.nodes[&DocumentNodeId("dev.brand".to_owned())];
        assert_eq!(brand.kind, DocumentNodeKind::Row);
        assert_eq!(
            brand
                .children
                .iter()
                .map(|id| frame.nodes[id].text.as_ref().unwrap().text.as_str())
                .collect::<String>(),
            "Boon/play"
        );
        assert_eq!(
            frame.nodes[&DocumentNodeId("dev.brand.boon".to_owned())]
                .style
                .get("underline_if"),
            Some(&StyleValue::Bool(true))
        );
    }

    #[test]
    fn editor_target_line_is_stable() {
        assert_eq!(editor_line_from_target("dev.editor.code.17"), Some(17));
        assert_eq!(
            editor_line_from_target("dev.editor.code.17:type-hint:0"),
            Some(17)
        );
        assert_eq!(editor_line_from_target("dev.editor.row.17"), Some(17));
        assert_eq!(editor_line_from_target("dev.editor.gutter.17"), Some(17));
        assert_eq!(editor_line_from_target("dev.editor"), None);
    }

    #[test]
    fn inspector_text_is_bounded_to_its_panel() {
        let text = "invalid MachinePlan detail that must not cross the inspector panel boundary";
        let bounded = bounded_panel_text(text, 20, 2);
        assert!(bounded.lines().count() <= 2);
        assert!(bounded.lines().all(|line| line.chars().count() <= 20));
        assert!(bounded.ends_with("..."));
    }

    #[test]
    fn navigation_targets_stay_inside_the_bounded_dev_surface() {
        let catalog = vec![
            CatalogItem {
                id: "one".to_owned(),
                label: "One".to_owned(),
                custom: false,
            },
            CatalogItem {
                id: "two".to_owned(),
                label: "Two".to_owned(),
                custom: false,
            },
        ];
        let paths = vec!["examples/one.bn".to_owned()];
        let buffer = Buffer::new("document: value\n");
        let frame = dev_frame(DevFrameState {
            catalog: &catalog,
            active_id: "one",
            example_label: "One",
            origin: ProjectOrigin::BuiltIn,
            source_paths: &paths,
            active_file: 0,
            buffer: &buffer,
            rename_buffer: None,
            rename_prompt: None,
            editor_scroll: 0.0,
            language: None,
            migration: None,
            inspector: InspectorState {
                symbol: "",
                static_type: "",
                detail: "",
                current_value: "",
            },
            persistence: empty_persistence(),
            status: "Ready",
            perf: "Preview idle",
        });
        let mut columns = ApproximateTextColumnMeasurer;
        let view = RetainedView::new(
            frame,
            Viewport {
                surface: 1,
                width: 508.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        for id in [DEV_PREVIOUS, DEV_NEXT, DEV_NEW] {
            let target = view.target_for_source(id, None).expect("navigation target");
            assert!(target.center_x > 0.0 && target.center_x < 508.0);
            assert!(target.center_y > 0.0 && target.center_y < 540.0);
            assert_eq!(view.hit(target.center_x, target.center_y), Some(id));
        }
        let test = view.target_for_source(DEV_TEST, None).expect("TEST target");
        assert!(
            test.center_x < 75.0,
            "primary action must remain at the safe left edge: {test:?}"
        );
        assert_eq!(view.hit(test.center_x, test.center_y), Some(DEV_TEST));
        let new_example = view
            .target_for_source(DEV_NEW, None)
            .expect("new-example target");
        assert!(
            new_example.center_y > 40.0,
            "new-example action must stay below native titlebar controls"
        );
        let editor = view
            .target_for_source(DEV_EDITOR, None)
            .expect("editor target");
        assert!(
            editor.bounds_width > 40.0,
            "editor target must retain useful width: {editor:?}"
        );
        let editor_input = view
            .target_for_source(DEV_EDITOR_INPUT_TARGET, None)
            .expect("visible editor input target");
        assert!(editor_input.bounds_width > 40.0);
        assert_eq!(
            view.hit(editor_input.center_x, editor_input.center_y),
            Some(DEV_EDITOR_INPUT_TARGET)
        );

        let clipped = RetainedView::new(
            view.frame().clone(),
            Viewport {
                surface: 1,
                width: 508.0,
                height: 136.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let visible_editor_input = clipped
            .target_for_source(DEV_EDITOR_INPUT_TARGET, None)
            .expect("partially visible editor input target");
        assert!(visible_editor_input.center_y < 136.0);
        assert_eq!(
            clipped.hit(visible_editor_input.center_x, visible_editor_input.center_y),
            Some(DEV_EDITOR_INPUT_TARGET)
        );
    }

    #[test]
    fn custom_examples_expose_rename_controls_and_a_real_text_input() {
        let catalog = vec![CatalogItem {
            id: "custom-1".to_owned(),
            label: "My example".to_owned(),
            custom: true,
        }];
        let paths = vec![
            "playground/custom_examples/custom-1/RUN.bn".to_owned(),
            "playground/custom_examples/custom-1/Model.bn".to_owned(),
        ];
        let source = Buffer::new("document: value\n");
        let mut rename = Buffer::new("My example");
        rename.apply(boon_editor::Command::SelectAll);
        let state = |rename_buffer, rename_prompt| DevFrameState {
            catalog: &catalog,
            active_id: "custom-1",
            example_label: "My example",
            origin: ProjectOrigin::Custom,
            source_paths: &paths,
            active_file: 0,
            buffer: &source,
            rename_buffer,
            rename_prompt,
            editor_scroll: 0.0,
            language: None,
            migration: None,
            inspector: InspectorState {
                symbol: "",
                static_type: "",
                detail: "",
                current_value: "",
            },
            persistence: empty_persistence(),
            status: "Ready",
            perf: "Preview idle",
        };
        let normal = dev_frame(state(None, None));
        assert!(
            normal
                .nodes
                .contains_key(&DocumentNodeId(DEV_RENAME.into()))
        );
        assert!(
            normal
                .nodes
                .contains_key(&DocumentNodeId(DEV_REMOVE.into()))
        );
        for action in [DEV_FILE_NEW, DEV_FILE_RENAME, DEV_FILE_REMOVE] {
            assert!(normal.nodes.contains_key(&DocumentNodeId(action.into())));
        }

        let editing = dev_frame(state(Some(&rename), Some("Example name")));
        let input = &editing.nodes[&DocumentNodeId(DEV_RENAME_INPUT.into())];
        assert_eq!(input.kind, DocumentNodeKind::TextInput);
        assert_eq!(input.text.as_ref().unwrap().text, "My example");
        assert_eq!(input.parent.as_ref().unwrap().0, "dev.examples");
        assert!(
            editing
                .nodes
                .contains_key(&DocumentNodeId(DEV_RENAME_SAVE.into()))
        );
        assert!(
            editing
                .nodes
                .contains_key(&DocumentNodeId(DEV_RENAME_CANCEL.into()))
        );

        let file_editing = dev_frame(state(Some(&rename), Some("File name")));
        assert_eq!(
            file_editing.nodes[&DocumentNodeId(DEV_RENAME_INPUT.into())]
                .parent
                .as_ref()
                .unwrap()
                .0,
            "dev.files"
        );
    }

    #[test]
    fn example_tabs_use_measured_widths_for_complete_labels() {
        let catalog = vec![CatalogItem {
            id: "todo_mvc_physical".to_owned(),
            label: "TodoMVC Physical".to_owned(),
            custom: false,
        }];
        let paths = vec!["examples/todo_mvc_physical/RUN.bn".to_owned()];
        let source = Buffer::new("document: value\n");
        let frame = dev_frame(DevFrameState {
            catalog: &catalog,
            active_id: "todo_mvc_physical",
            example_label: "TodoMVC Physical",
            origin: ProjectOrigin::BuiltIn,
            source_paths: &paths,
            active_file: 0,
            buffer: &source,
            rename_buffer: None,
            rename_prompt: None,
            editor_scroll: 0.0,
            language: None,
            migration: None,
            inspector: InspectorState {
                symbol: "",
                static_type: "",
                detail: "",
                current_value: "",
            },
            persistence: empty_persistence(),
            status: "Ready",
            perf: "Preview idle",
        });
        let id = DocumentNodeId("dev.example.todo_mvc_physical".to_owned());
        let tab = frame.nodes.get(&id).expect("physical TodoMVC tab");
        assert_eq!(tab.style.get("width"), Some(&text("Auto")));
        assert_eq!(
            tab.style.get("accessibility_label"),
            Some(&text("TodoMVC Physical"))
        );

        let mut columns = ApproximateTextColumnMeasurer;
        let view = RetainedView::new(
            frame,
            Viewport {
                surface: 1,
                width: 508.0,
                height: 540.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let bounds = view
            .node_bounds("dev.example.todo_mvc_physical")
            .expect("physical TodoMVC tab bounds");
        let run = view
            .scene()
            .text_runs
            .iter()
            .find(|run| run.node == id)
            .expect("physical TodoMVC text run");
        let editor_lines_fill = view
            .scene()
            .visual_primitives
            .iter()
            .find(|primitive| {
                primitive.node == DocumentNodeId("dev.editor.lines".to_owned())
                    && primitive.primitive
                        == boon_document::render_scene::RenderVisualPrimitiveKind::Fill
            })
            .expect("editor lines fill");
        let brand_underline = view
            .scene()
            .visual_primitives
            .iter()
            .find(|primitive| {
                primitive.node == DocumentNodeId("dev.brand.boon".to_owned())
                    && primitive.primitive
                        == boon_document::render_scene::RenderVisualPrimitiveKind::Underline
            })
            .expect("Boon brand underline");
        let measured_text_width = "TodoMVC Physical".chars().count() as f32 * 12.0 * 0.62;
        assert_eq!(run.text, "TodoMVC Physical");
        assert_eq!(editor_lines_fill.color, [40, 44, 52, 255]);
        assert_eq!(brand_underline.color, [108, 182, 255, 255]);
        assert!(
            bounds.width >= measured_text_width + 20.0,
            "tab width {} must contain measured label width {measured_text_width}",
            bounds.width
        );
    }
}
