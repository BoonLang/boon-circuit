use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorPosition {
    pub line: usize,
    pub column: usize,
}

impl EditorPosition {
    pub fn start() -> Self {
        Self { line: 1, column: 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSelection {
    pub anchor: EditorPosition,
    pub head: EditorPosition,
}

impl EditorSelection {
    pub fn collapsed(position: EditorPosition) -> Self {
        Self {
            anchor: position.clone(),
            head: position,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Debug)]
pub struct EditorSnapshot {
    source_text: String,
    selection: EditorSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveUnit {
    Grapheme,
    Line,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDirection {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    InsertPlainText(String),
    DeleteBackward,
    DeleteForward,
    Move {
        direction: MoveDirection,
        unit: MoveUnit,
        extend: bool,
    },
    SelectAll,
    Indent,
    Unindent,
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BracketMatch {
    pub open_byte: usize,
    pub close_byte: usize,
    pub open: char,
    pub close: char,
    pub matched: bool,
    pub contains_caret: bool,
}

#[derive(Clone, Debug)]
pub struct EditorBuffer {
    rope: Rope,
    selection: EditorSelection,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    pub last_command: Option<&'static str>,
}

impl EditorBuffer {
    pub fn new(source_text: &str) -> Self {
        Self {
            rope: Rope::from_str(source_text),
            selection: EditorSelection::collapsed(EditorPosition::start()),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_command: None,
        }
    }

    pub fn source_text(&self) -> String {
        self.rope.to_string()
    }

    pub fn line_count(&self) -> usize {
        let lines = self.rope.len_lines().max(1);
        if self.rope.len_chars() > 0
            && self.rope.char(self.rope.len_chars().saturating_sub(1)) == '\n'
        {
            lines.saturating_sub(1).max(1)
        } else {
            lines
        }
    }

    pub fn selection(&self) -> &EditorSelection {
        &self.selection
    }

    pub fn caret(&self) -> &EditorPosition {
        &self.selection.head
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    pub fn byte_offset(&self, position: &EditorPosition) -> usize {
        let char_idx = self.position_to_char(position);
        self.rope.char_to_byte(char_idx.min(self.rope.len_chars()))
    }

    pub fn position_for_byte_offset(&self, byte_offset: usize) -> EditorPosition {
        let byte_offset = byte_offset.min(self.rope.len_bytes());
        let char_idx = self.rope.byte_to_char(byte_offset);
        self.position_for_char(char_idx)
    }

    pub fn selection_byte_offsets(&self) -> (usize, usize) {
        let anchor = self.byte_offset(&self.selection.anchor);
        let head = self.byte_offset(&self.selection.head);
        if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        }
    }

    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection_char_offsets();
        self.rope.slice(start..end).to_string()
    }

    pub fn set_selection(&mut self, anchor: EditorPosition, head: EditorPosition) {
        let anchor = self.clamp_position(anchor);
        let head = self.clamp_position(head);
        self.selection = EditorSelection { anchor, head };
        self.last_command = Some("selection");
    }

    pub fn insert_text_at_caret(&mut self, text: &str) {
        match text {
            "(" => self.insert_auto_pair("(", ")"),
            "[" => self.insert_auto_pair("[", "]"),
            "{" => self.insert_auto_pair("{", "}"),
            ")" | "]" | "}" if self.selection.is_collapsed() && self.next_text_is(text) => {
                self.move_right(false);
                self.last_command = Some("keyboard-skip-close-bracket");
            }
            _ => self.insert_plain_text_at_caret(text, "keyboard-insert-text"),
        }
    }

    pub fn insert_plain_text_at_caret(&mut self, text: &str, command: &'static str) {
        self.push_undo();
        let (start, end) = self.selection_char_offsets();
        self.rope.remove(start..end);
        self.rope.insert(start, text);
        let new_position = self.position_for_char(start + text.chars().count());
        self.selection = EditorSelection::collapsed(new_position);
        self.last_command = Some(command);
    }

    pub fn delete_backward(&mut self) {
        self.push_undo();
        let (start, end) = self.selection_byte_offsets();
        if start != end {
            self.replace_byte_range(start, end, "");
            self.selection = EditorSelection::collapsed(self.position_for_byte_offset(start));
        } else if start > 0 {
            let previous = previous_grapheme_boundary(&self.source_text(), start);
            self.replace_byte_range(previous, start, "");
            self.selection = EditorSelection::collapsed(self.position_for_byte_offset(previous));
        }
        self.last_command = Some("keyboard-delete-backward");
    }

    pub fn delete_forward(&mut self) {
        self.push_undo();
        let (start, end) = self.selection_byte_offsets();
        if start != end {
            self.replace_byte_range(start, end, "");
        } else {
            let text = self.source_text();
            let next = next_grapheme_boundary(&text, start);
            self.replace_byte_range(start, next, "");
        }
        self.selection = EditorSelection::collapsed(self.position_for_byte_offset(start));
        self.last_command = Some("keyboard-delete-forward");
    }

    pub fn insert_newline_with_indent(&mut self) {
        let line = self.line_text(self.caret().line);
        let indent = line
            .chars()
            .take_while(|character| character.is_whitespace())
            .collect::<String>();
        self.insert_plain_text_at_caret(&format!("\n{indent}"), "keyboard-enter-indent");
    }

    pub fn indent_selection(&mut self) {
        self.push_undo();
        let (start_line, end_line) = self.selected_line_range();
        for line in (start_line..=end_line).rev() {
            let char_idx = self.rope.line_to_char(line.saturating_sub(1));
            self.rope.insert(char_idx, "    ");
        }
        self.selection = EditorSelection {
            anchor: EditorPosition {
                line: self.selection.anchor.line,
                column: self.selection.anchor.column + 4,
            },
            head: EditorPosition {
                line: self.selection.head.line,
                column: self.selection.head.column + 4,
            },
        };
        self.last_command = Some("keyboard-tab-indent");
    }

    pub fn unindent_selection(&mut self) {
        self.push_undo();
        let (start_line, end_line) = self.selected_line_range();
        for line in (start_line..=end_line).rev() {
            let text = self.line_text(line);
            let remove = text.chars().take_while(|ch| *ch == ' ').take(4).count();
            if remove > 0 {
                let start = self.rope.line_to_char(line.saturating_sub(1));
                self.rope.remove(start..start + remove);
            }
        }
        self.selection = EditorSelection {
            anchor: EditorPosition {
                line: self.selection.anchor.line,
                column: self.selection.anchor.column.saturating_sub(4).max(1),
            },
            head: EditorPosition {
                line: self.selection.head.line,
                column: self.selection.head.column.saturating_sub(4).max(1),
            },
        };
        self.last_command = Some("keyboard-shift-tab-unindent");
    }

    pub fn move_home(&mut self, extend: bool) {
        let next = EditorPosition {
            line: self.caret().line,
            column: 1,
        };
        self.set_caret(next, extend);
        self.last_command = Some("keyboard-home");
    }

    pub fn move_end(&mut self, extend: bool) {
        let line = self.caret().line;
        let column = self.line_text(line).chars().count() + 1;
        self.set_caret(EditorPosition { line, column }, extend);
        self.last_command = Some("keyboard-end");
    }

    pub fn move_left(&mut self, extend: bool) {
        let offset = self.byte_offset(self.caret());
        let previous = previous_grapheme_boundary(&self.source_text(), offset);
        self.set_caret(self.position_for_byte_offset(previous), extend);
        self.last_command = Some("keyboard-arrow-left");
    }

    pub fn move_right(&mut self, extend: bool) {
        let offset = self.byte_offset(self.caret());
        let next = next_grapheme_boundary(&self.source_text(), offset);
        self.set_caret(self.position_for_byte_offset(next), extend);
        self.last_command = Some("keyboard-arrow-right");
    }

    pub fn move_up(&mut self, extend: bool) {
        let caret = self.caret().clone();
        if caret.line > 1 {
            let line = caret.line - 1;
            let column = caret.column.min(self.line_text(line).chars().count() + 1);
            self.set_caret(EditorPosition { line, column }, extend);
        }
        self.last_command = Some("keyboard-arrow-up");
    }

    pub fn move_down(&mut self, extend: bool) {
        let caret = self.caret().clone();
        let line = (caret.line + 1).min(self.line_count());
        let column = caret.column.min(self.line_text(line).chars().count() + 1);
        self.set_caret(EditorPosition { line, column }, extend);
        self.last_command = Some("keyboard-arrow-down");
    }

    pub fn page_down(&mut self, extend: bool) {
        let caret = self.caret().clone();
        let line = (caret.line + 24).min(self.line_count());
        let column = caret.column.min(self.line_text(line).chars().count() + 1);
        self.set_caret(EditorPosition { line, column }, extend);
        self.last_command = Some("keyboard-page-down");
    }

    pub fn page_up(&mut self, extend: bool) {
        let caret = self.caret().clone();
        let line = caret.line.saturating_sub(24).max(1);
        let column = caret.column.min(self.line_text(line).chars().count() + 1);
        self.set_caret(EditorPosition { line, column }, extend);
        self.last_command = Some("keyboard-page-up");
    }

    pub fn select_all(&mut self) {
        let end = self.position_for_byte_offset(self.rope.len_bytes());
        self.selection = EditorSelection {
            anchor: EditorPosition::start(),
            head: end,
        };
        self.last_command = Some("keyboard-select-all");
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
        self.last_command = Some("undo");
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
        self.last_command = Some("redo");
        true
    }

    pub fn bracket_match(&self, ignored_ranges: &[(usize, usize)]) -> Option<BracketMatch> {
        bracket_match_for_source(
            &self.source_text(),
            self.byte_offset(self.caret()),
            ignored_ranges,
        )
    }

    fn insert_auto_pair(&mut self, open: &str, close: &str) {
        self.push_undo();
        let (start, end) = self.selection_char_offsets();
        let selected = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        self.rope.insert(start, &format!("{open}{selected}{close}"));
        let caret_char = start + open.chars().count() + selected.chars().count();
        self.selection = EditorSelection::collapsed(self.position_for_char(caret_char));
        self.last_command = Some("keyboard-auto-close-bracket");
    }

    fn next_text_is(&self, text: &str) -> bool {
        let start = self.position_to_char(self.caret());
        let end = (start + text.chars().count()).min(self.rope.len_chars());
        self.rope.slice(start..end).to_string() == text
    }

    fn position_to_char(&self, position: &EditorPosition) -> usize {
        let line_count = self.rope.len_lines().max(1);
        let line_index = position.line.saturating_sub(1).min(line_count - 1);
        let line_start = self.rope.line_to_char(line_index);
        let line_len = self.line_text(line_index + 1).chars().count();
        line_start + position.column.saturating_sub(1).min(line_len)
    }

    fn position_for_char(&self, char_idx: usize) -> EditorPosition {
        let char_idx = char_idx.min(self.rope.len_chars());
        let line_index = self.rope.char_to_line(char_idx);
        let line_start = self.rope.line_to_char(line_index);
        EditorPosition {
            line: line_index + 1,
            column: char_idx.saturating_sub(line_start) + 1,
        }
    }

    fn line_text(&self, line: usize) -> String {
        let line_index = line
            .saturating_sub(1)
            .min(self.rope.len_lines().saturating_sub(1));
        self.rope
            .line(line_index)
            .to_string()
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    }

    fn selection_char_offsets(&self) -> (usize, usize) {
        let anchor = self.position_to_char(&self.selection.anchor);
        let head = self.position_to_char(&self.selection.head);
        if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        }
    }

    fn selected_line_range(&self) -> (usize, usize) {
        let start = self
            .selection
            .anchor
            .line
            .min(self.selection.head.line)
            .max(1);
        let end = self
            .selection
            .anchor
            .line
            .max(self.selection.head.line)
            .min(self.line_count());
        (start, end)
    }

    fn set_caret(&mut self, position: EditorPosition, extend: bool) {
        let position = self.clamp_position(position);
        if extend {
            self.selection.head = position;
        } else {
            self.selection = EditorSelection::collapsed(position);
        }
    }

    fn clamp_position(&self, position: EditorPosition) -> EditorPosition {
        let line = position.line.max(1).min(self.line_count());
        let column = position
            .column
            .max(1)
            .min(self.line_text(line).chars().count() + 1);
        EditorPosition { line, column }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            source_text: self.source_text(),
            selection: self.selection.clone(),
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.rope = Rope::from_str(&snapshot.source_text);
        self.selection = snapshot.selection;
    }

    fn replace_byte_range(&mut self, start: usize, end: usize, replacement: &str) {
        let start_char = self.rope.byte_to_char(start.min(self.rope.len_bytes()));
        let end_char = self.rope.byte_to_char(end.min(self.rope.len_bytes()));
        self.rope.remove(start_char..end_char);
        self.rope.insert(start_char, replacement);
    }
}

pub trait ClipboardAdapter {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

pub fn bracket_match_for_source(
    source: &str,
    caret_byte: usize,
    ignored_ranges: &[(usize, usize)],
) -> Option<BracketMatch> {
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut pairs: Vec<BracketMatch> = Vec::new();
    let mut unmatched: Vec<BracketMatch> = Vec::new();
    for (byte, ch) in source.char_indices() {
        if ignored_ranges
            .iter()
            .any(|(start, end)| byte >= *start && byte < *end)
        {
            continue;
        }
        if is_open(ch) {
            stack.push((ch, byte));
        } else if let Some(open) = matching_open(ch) {
            if stack
                .last()
                .map(|(candidate, _)| *candidate == open)
                .unwrap_or(false)
            {
                let (open, open_byte) = stack.pop().expect("stack top checked above");
                pairs.push(BracketMatch {
                    open_byte,
                    close_byte: byte,
                    open,
                    close: ch,
                    matched: true,
                    contains_caret: false,
                });
            } else {
                unmatched.push(BracketMatch {
                    open_byte: byte,
                    close_byte: byte,
                    open: ch,
                    close: ch,
                    matched: false,
                    contains_caret: false,
                });
            }
        }
    }
    for (open, open_byte) in stack {
        unmatched.push(BracketMatch {
            open_byte,
            close_byte: open_byte,
            open,
            close: matching_close(open).unwrap_or(open),
            matched: false,
            contains_caret: false,
        });
    }

    let adjacent = source
        .char_indices()
        .find(|(byte, _)| *byte == caret_byte)
        .map(|(byte, _)| byte)
        .or_else(|| previous_grapheme_start(source, caret_byte));
    if let Some(adjacent) = adjacent {
        if let Some(pair) = pairs
            .iter()
            .find(|pair| pair.open_byte == adjacent || pair.close_byte == adjacent)
        {
            let mut pair = pair.clone();
            pair.contains_caret = true;
            return Some(pair);
        }
        if let Some(pair) = unmatched.iter().find(|pair| pair.open_byte == adjacent) {
            return Some(pair.clone());
        }
    }

    if let Some(pair) = pairs
        .iter()
        .filter(|pair| pair.open_byte <= caret_byte && caret_byte <= pair.close_byte)
        .min_by_key(|pair| pair.close_byte.saturating_sub(pair.open_byte))
        .cloned()
        .map(|mut pair| {
            pair.contains_caret = true;
            pair
        })
    {
        return Some(pair);
    }

    None
}

fn previous_grapheme_boundary(text: &str, byte: usize) -> usize {
    previous_grapheme_start(text, byte).unwrap_or(0)
}

fn previous_grapheme_start(text: &str, byte: usize) -> Option<usize> {
    let byte = byte.min(text.len());
    UnicodeSegmentation::grapheme_indices(&text[..byte], true)
        .last()
        .map(|(index, _)| index)
}

fn next_grapheme_boundary(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    UnicodeSegmentation::grapheme_indices(&text[byte..], true)
        .nth(1)
        .map(|(index, _)| byte + index)
        .unwrap_or(text.len())
}

fn is_open(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{')
}

fn matching_open(ch: char) -> Option<char> {
    match ch {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

fn matching_close(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
