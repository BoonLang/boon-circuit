use boon_document::{
    DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, ScrollRootId, StyleValue,
    TextValue,
};
use boon_document_model::{ScrollState, SourceBinding, SourceBindingId};
use boon_editor::{Buffer, Selection};

use crate::language::LanguageSnapshot;
use crate::protocol::CatalogItem;
use crate::workspace::ProjectOrigin;

pub const DEV_PREVIOUS: &str = "dev.previous";
pub const DEV_NEXT: &str = "dev.next";
pub const DEV_RUN: &str = "dev.run";
pub const DEV_RESET: &str = "dev.reset";
pub const DEV_TEST: &str = "dev.test";
pub const DEV_SAVE: &str = "dev.save";
pub const DEV_FORMAT: &str = "dev.format";
pub const DEV_NEW: &str = "dev.new";
pub const DEV_REMOVE: &str = "dev.remove";
pub const DEV_EDITOR: &str = "dev.editor";

const EDITOR_LINE_HEIGHT: f64 = 23.0;
const EDITOR_WINDOW_LINES: usize = 36;
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

pub struct DevFrameState<'a> {
    pub catalog: &'a [CatalogItem],
    pub active_id: &'a str,
    pub example_label: &'a str,
    pub origin: ProjectOrigin,
    pub source_paths: &'a [String],
    pub active_file: usize,
    pub buffer: &'a Buffer,
    pub editor_scroll: f32,
    pub language: Option<&'a LanguageSnapshot>,
    pub inspector: InspectorState<'a>,
    pub status: &'a str,
    pub perf: &'a str,
}

pub fn dev_frame(state: DevFrameState<'_>) -> DocumentFrame {
    let mut frame = DocumentFrame::empty("dev.root");
    style_root(frame.nodes.get_mut(&frame.root).expect("dev root"), DEV_BG);
    add_header(&mut frame);
    add_example_tabs(&mut frame, &state);
    add_file_tabs(&mut frame, &state);
    add_workspace(&mut frame, &state);
    add_footer(&mut frame, &state);
    frame
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

    let mut brand = label("dev.brand", "BOON", "dev.header");
    brand.style.insert("width".into(), number(70.0));
    brand.style.insert("font_size".into(), number(12.0));
    brand.style.insert("font_weight".into(), text("700"));
    brand.style.insert("color".into(), text(DEV_ACCENT));
    add(frame, "dev.header", brand);

    for (id, label, width, accent) in [
        (DEV_TEST, "TEST", 52.0, true),
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
    add(
        frame,
        "dev.header",
        button(DEV_NEW, "+", 32.0, "dev.header", true),
    );
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

    for entry in state.catalog {
        let id = format!("dev.example.{}", entry.id);
        let mut tab = button(&id, &entry.label, 116.0, "dev.examples", false);
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
        tab.style.insert("height".into(), number(30.0));
        if index == state.active_file {
            tab.style.insert("background".into(), text(DEV_BG));
            tab.style.insert("border".into(), text(DEV_BORDER));
            tab.style.insert("color".into(), text(DEV_TEXT));
        }
        add(frame, "dev.files", tab);
    }

    let identity_text = format!("{}  {}", state.example_label, state.origin.badge());
    let mut identity = label("dev.identity", &identity_text, "dev.files");
    identity.style.insert("width".into(), text("Fill"));
    identity.style.insert("font_size".into(), number(11.0));
    identity.style.insert(
        "color".into(),
        text(if state.origin == ProjectOrigin::BuiltIn {
            DEV_ACCENT
        } else {
            "#d1a6ff"
        }),
    );
    add(frame, "dev.files", identity);
    if state.origin == ProjectOrigin::Custom {
        add(
            frame,
            "dev.files",
            button(DEV_REMOVE, "Remove", 58.0, "dev.files", false),
        );
    }
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
    inspector.style.insert("width".into(), number(292.0));
    inspector.style.insert("height".into(), text("Fill"));
    inspector.style.insert("padding".into(), number(14.0));
    inspector.style.insert("gap".into(), number(8.0));
    inspector.style.insert("background".into(), text(DEV_PANEL));
    add(frame, "dev.workspace", inspector);

    let mut title = label("dev.inspector.title", "INSPECT", "dev.inspector");
    title.style.insert("height".into(), number(24.0));
    title.style.insert("font_size".into(), number(12.0));
    title.style.insert("font_weight".into(), text("700"));
    title.style.insert("color".into(), text(DEV_ACCENT));
    add(frame, "dev.inspector", title);
    inspector_field(frame, "Symbol", state.inspector.symbol, 44.0, "symbol");
    inspector_field(
        frame,
        "Static type",
        state.inspector.static_type,
        62.0,
        "type",
    );
    inspector_field(
        frame,
        "Current value",
        state.inspector.current_value,
        92.0,
        "value",
    );
    inspector_field(frame, "Details", state.inspector.detail, 138.0, "detail");

    if let Some(language) = state.language
        && !language.diagnostics.is_empty()
    {
        inspector_field(
            frame,
            "Diagnostics",
            &language.diagnostics.join("\n"),
            120.0,
            "diagnostics",
        );
    }
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
    button.source_bindings.push(binding(id, "press"));
    button
}

fn footer_label(id: &str, value: &str) -> DocumentNode {
    let mut node = label(id, value, "dev.footer");
    node.style.insert("width".into(), text("Fill"));
    node.style.insert("height".into(), number(18.0));
    node.style.insert("font_size".into(), number(11.0));
    node.style.insert("color".into(), text(DEV_TEXT_MUTED));
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
            editor_scroll: 0.0,
            language: None,
            inspector: InspectorState {
                symbol: "value",
                static_type: "Number",
                detail: "Number",
                current_value: "1",
            },
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
            editor_scroll: 0.0,
            language: None,
            inspector: InspectorState {
                symbol: "",
                static_type: "",
                detail: "",
                current_value: "",
            },
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
        for id in [DEV_PREVIOUS, DEV_NEXT] {
            let target = view.target_for_source(id, None).expect("navigation target");
            assert!(target.center_x > 0.0 && target.center_x < 508.0);
            assert!(target.center_y > 0.0 && target.center_y < 540.0);
            assert_eq!(view.hit(target.center_x, target.center_y), Some(id));
        }
    }
}
