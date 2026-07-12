use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub anchor: Position,
    pub head: Position,
}

impl Selection {
    pub fn collapsed(position: Position) -> Self {
        Self {
            anchor: position,
            head: position,
        }
    }

    pub fn is_collapsed(self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Insert(String),
    InsertPlain(String),
    Newline,
    DeleteBackward,
    DeleteForward,
    MoveLeft { extend: bool },
    MoveRight { extend: bool },
    MoveUp { extend: bool },
    MoveDown { extend: bool },
    MoveHome { extend: bool },
    MoveEnd { extend: bool },
    PageUp { extend: bool, lines: usize },
    PageDown { extend: bool, lines: usize },
    SelectAll,
    Indent,
    Unindent,
    Undo,
    Redo,
}

#[derive(Clone, Debug)]
struct Snapshot {
    rope: Rope,
    selection: Selection,
}

#[derive(Clone, Debug)]
pub struct Buffer {
    rope: Rope,
    selection: Selection,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    revision: u64,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            selection: Selection::collapsed(Position::default()),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 1,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    pub fn line(&self, line: usize) -> String {
        let index = line.min(self.line_count().saturating_sub(1));
        self.rope
            .line(index)
            .to_string()
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    }

    pub fn max_line_columns(&self) -> usize {
        (0..self.line_count())
            .map(|line| self.line(line).graphemes(true).count())
            .max()
            .unwrap_or(0)
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn caret(&self) -> Position {
        self.selection.head
    }

    pub fn set_selection(&mut self, anchor: Position, head: Position) -> bool {
        let next = Selection {
            anchor: self.clamp_position(anchor),
            head: self.clamp_position(head),
        };
        let changed = next != self.selection;
        self.selection = next;
        changed
    }

    pub fn set_caret(&mut self, position: Position, extend: bool) -> bool {
        let position = self.clamp_position(position);
        let next = if extend {
            Selection {
                anchor: self.selection.anchor,
                head: position,
            }
        } else {
            Selection::collapsed(position)
        };
        let changed = next != self.selection;
        self.selection = next;
        changed
    }

    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection_char_range();
        self.rope.slice(start..end).to_string()
    }

    pub fn selection_byte_range(&self) -> (usize, usize) {
        let (start, end) = self.selection_char_range();
        (self.rope.char_to_byte(start), self.rope.char_to_byte(end))
    }

    pub fn position_for_byte(&self, byte: usize) -> Position {
        self.position_for_char(self.rope.byte_to_char(byte.min(self.rope.len_bytes())))
    }

    pub fn byte_for_position(&self, position: Position) -> usize {
        self.rope.char_to_byte(self.char_for_position(position))
    }

    pub fn replace_text(&mut self, text: &str) {
        self.push_undo();
        self.rope = Rope::from_str(text);
        self.selection = Selection::collapsed(Position::default());
        self.bump_revision();
    }

    pub fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Insert(text) => self.insert_text(&text, true),
            Command::InsertPlain(text) => self.insert_text(&text, false),
            Command::Newline => {
                let line = self.line(self.caret().line);
                let indent = line
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .collect::<String>();
                self.insert_text(&format!("\n{indent}"), false)
            }
            Command::DeleteBackward => self.delete_backward(),
            Command::DeleteForward => self.delete_forward(),
            Command::MoveLeft { extend } => self.move_horizontal(false, extend),
            Command::MoveRight { extend } => self.move_horizontal(true, extend),
            Command::MoveUp { extend } => self.move_vertical(-1, extend),
            Command::MoveDown { extend } => self.move_vertical(1, extend),
            Command::MoveHome { extend } => self.set_caret(
                Position {
                    line: self.caret().line,
                    column: 0,
                },
                extend,
            ),
            Command::MoveEnd { extend } => {
                let line = self.caret().line;
                self.set_caret(
                    Position {
                        line,
                        column: self.line_columns(line),
                    },
                    extend,
                )
            }
            Command::PageUp { extend, lines } => self.move_lines(-(lines as isize), extend),
            Command::PageDown { extend, lines } => self.move_lines(lines as isize, extend),
            Command::SelectAll => {
                let end_line = self.line_count().saturating_sub(1);
                self.set_selection(
                    Position::default(),
                    Position {
                        line: end_line,
                        column: self.line_columns(end_line),
                    },
                )
            }
            Command::Indent => self.indent(false),
            Command::Unindent => self.indent(true),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
        }
    }

    fn insert_text(&mut self, text: &str, pair: bool) -> bool {
        if pair && self.selection.is_collapsed() && matches!(text, ")" | "]" | "}") {
            let caret = self.byte_for_position(self.caret());
            if self
                .text()
                .get(caret..)
                .is_some_and(|suffix| suffix.starts_with(text))
            {
                return self.move_horizontal(true, false);
            }
        }
        let close = pair.then(|| match text {
            "(" => ")",
            "[" => "]",
            "{" => "}",
            _ => "",
        });
        self.push_undo();
        let (start, end) = self.selection_char_range();
        let selected = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        if let Some(close) = close.filter(|close| !close.is_empty()) {
            self.rope.insert(start, &format!("{text}{selected}{close}"));
            let caret = start + text.chars().count() + selected.chars().count();
            self.selection = Selection::collapsed(self.position_for_char(caret));
        } else {
            self.rope.insert(start, text);
            let caret = start + text.chars().count();
            self.selection = Selection::collapsed(self.position_for_char(caret));
        }
        self.bump_revision();
        true
    }

    fn delete_backward(&mut self) -> bool {
        let (start, end) = self.selection_byte_range();
        if start == end && start == 0 {
            return false;
        }
        let text = self.text();
        let remove_start = if start == end {
            previous_grapheme(&text, start)
        } else {
            start
        };
        self.replace_byte_range(remove_start, end, "")
    }

    fn delete_forward(&mut self) -> bool {
        let (start, end) = self.selection_byte_range();
        if start == end && end == self.rope.len_bytes() {
            return false;
        }
        let text = self.text();
        let remove_end = if start == end {
            next_grapheme(&text, end)
        } else {
            end
        };
        self.replace_byte_range(start, remove_end, "")
    }

    fn replace_byte_range(&mut self, start: usize, end: usize, replacement: &str) -> bool {
        self.push_undo();
        let start_char = self.rope.byte_to_char(start.min(self.rope.len_bytes()));
        let end_char = self.rope.byte_to_char(end.min(self.rope.len_bytes()));
        self.rope.remove(start_char..end_char);
        self.rope.insert(start_char, replacement);
        self.selection =
            Selection::collapsed(self.position_for_char(start_char + replacement.chars().count()));
        self.bump_revision();
        true
    }

    fn move_horizontal(&mut self, right: bool, extend: bool) -> bool {
        let text = self.text();
        let byte = self.byte_for_position(self.caret());
        let next = if right {
            next_grapheme(&text, byte)
        } else {
            previous_grapheme(&text, byte)
        };
        self.set_caret(self.position_for_byte(next), extend)
    }

    fn move_vertical(&mut self, delta: isize, extend: bool) -> bool {
        self.move_lines(delta, extend)
    }

    fn move_lines(&mut self, delta: isize, extend: bool) -> bool {
        let caret = self.caret();
        let last = self.line_count().saturating_sub(1) as isize;
        let line = (caret.line as isize + delta).clamp(0, last) as usize;
        self.set_caret(
            Position {
                line,
                column: caret.column.min(self.line_columns(line)),
            },
            extend,
        )
    }

    fn indent(&mut self, unindent: bool) -> bool {
        let (start, end) = self.ordered_selection();
        self.push_undo();
        for line in (start.line..=end.line).rev() {
            let char_index = self.rope.line_to_char(line);
            if unindent {
                let remove = self
                    .line(line)
                    .chars()
                    .take_while(|ch| *ch == ' ')
                    .take(4)
                    .count();
                if remove > 0 {
                    self.rope.remove(char_index..char_index + remove);
                }
            } else {
                self.rope.insert(char_index, "    ");
            }
        }
        let adjust = |position: Position| Position {
            line: position.line,
            column: if unindent {
                position.column.saturating_sub(4)
            } else {
                position.column.saturating_add(4)
            },
        };
        self.selection = Selection {
            anchor: self.clamp_position(adjust(self.selection.anchor)),
            head: self.clamp_position(adjust(self.selection.head)),
        };
        self.bump_revision();
        true
    }

    fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    fn push_undo(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            rope: self.rope.clone(),
            selection: self.selection,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.rope = snapshot.rope;
        self.selection = snapshot.selection;
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn ordered_selection(&self) -> (Position, Position) {
        if self.selection.anchor <= self.selection.head {
            (self.selection.anchor, self.selection.head)
        } else {
            (self.selection.head, self.selection.anchor)
        }
    }

    fn selection_char_range(&self) -> (usize, usize) {
        let anchor = self.char_for_position(self.selection.anchor);
        let head = self.char_for_position(self.selection.head);
        if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        }
    }

    fn char_for_position(&self, position: Position) -> usize {
        let position = self.clamp_position(position);
        let line_start = self.rope.line_to_char(position.line);
        let line = self.line(position.line);
        let byte = byte_for_grapheme_column(&line, position.column);
        line_start + line[..byte].chars().count()
    }

    fn position_for_char(&self, char_index: usize) -> Position {
        let char_index = char_index.min(self.rope.len_chars());
        let line = self.rope.char_to_line(char_index);
        let line_start = self.rope.line_to_char(line);
        let prefix = self.rope.slice(line_start..char_index).to_string();
        Position {
            line,
            column: prefix.graphemes(true).count(),
        }
    }

    fn clamp_position(&self, position: Position) -> Position {
        let line = position.line.min(self.line_count().saturating_sub(1));
        Position {
            line,
            column: position.column.min(self.line_columns(line)),
        }
    }

    fn line_columns(&self, line: usize) -> usize {
        self.line(line).graphemes(true).count()
    }
}

fn byte_for_grapheme_column(text: &str, column: usize) -> usize {
    text.grapheme_indices(true)
        .nth(column)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn previous_grapheme(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .grapheme_indices(true)
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_grapheme(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text[byte..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(index, _)| byte + index)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_insert_undo_and_redo_preserve_unicode_boundaries() {
        let mut buffer = Buffer::new("a🙂b");
        buffer.set_selection(
            Position { line: 0, column: 1 },
            Position { line: 0, column: 2 },
        );
        assert_eq!(buffer.selected_text(), "🙂");
        assert!(buffer.apply(Command::InsertPlain("x".to_owned())));
        assert_eq!(buffer.text(), "axb");
        assert!(buffer.apply(Command::Undo));
        assert_eq!(buffer.text(), "a🙂b");
        assert!(buffer.apply(Command::Redo));
        assert_eq!(buffer.text(), "axb");
    }

    #[test]
    fn multiline_indent_and_unindent_update_the_selected_rows() {
        let mut buffer = Buffer::new("a\nb\n");
        buffer.set_selection(Position::default(), Position { line: 1, column: 1 });
        buffer.apply(Command::Indent);
        assert_eq!(buffer.text(), "    a\n    b\n");
        buffer.apply(Command::Unindent);
        assert_eq!(buffer.text(), "a\nb\n");
    }

    #[test]
    fn paired_delimiters_wrap_selection_and_skip_existing_close() {
        let mut buffer = Buffer::new("value");
        buffer.apply(Command::SelectAll);
        buffer.apply(Command::Insert("(".to_owned()));
        assert_eq!(buffer.text(), "(value)");
        assert_eq!(buffer.caret().column, 6);
        buffer.apply(Command::Insert(")".to_owned()));
        assert_eq!(buffer.text(), "(value)");
        assert_eq!(buffer.caret().column, 7);
    }
}
