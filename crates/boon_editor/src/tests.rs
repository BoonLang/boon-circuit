use super::*;

#[test]
fn inserts_auto_pair_and_skips_closing_bracket() {
    let mut buffer = EditorBuffer::new("");
    buffer.insert_text_at_caret("(");
    assert_eq!(buffer.source_text(), "()");
    assert_eq!(*buffer.caret(), EditorPosition { line: 1, column: 2 });
    buffer.insert_text_at_caret(")");
    assert_eq!(buffer.source_text(), "()");
    assert_eq!(*buffer.caret(), EditorPosition { line: 1, column: 3 });
}

#[test]
fn edits_selection_and_undoes() {
    let mut buffer = EditorBuffer::new("abc");
    buffer.set_selection(
        EditorPosition { line: 1, column: 2 },
        EditorPosition { line: 1, column: 3 },
    );
    buffer.insert_text_at_caret("Z");
    assert_eq!(buffer.source_text(), "aZc");
    assert!(buffer.undo());
    assert_eq!(buffer.source_text(), "abc");
}

#[test]
fn deletes_grapheme_without_splitting_bytes() {
    let mut buffer = EditorBuffer::new("aé");
    buffer.move_end(false);
    buffer.delete_backward();
    assert_eq!(buffer.source_text(), "a");
}

#[test]
fn finds_innermost_bracket_pair_around_caret() {
    let source = "outer: [value: { count + 1 }]\n-- [ignored]\n";
    let caret = source.find("count").unwrap();
    let ignored = vec![(source.find("--").unwrap(), source.len())];
    let pair = bracket_match_for_source(source, caret, &ignored).unwrap();
    assert_eq!(&source[pair.open_byte..pair.open_byte + 1], "{");
    assert_eq!(&source[pair.close_byte..pair.close_byte + 1], "}");
}

#[test]
fn bracket_matching_prefers_closest_pair_without_crossing_invalid_nesting() {
    let source = "outer: ([bad)]";
    let caret = source.find(')').unwrap();
    let pair = bracket_match_for_source(source, caret, &[]).unwrap();
    assert!(!pair.matched);
    assert_eq!(pair.open_byte, caret);
    assert_eq!(pair.close_byte, caret);
}

#[test]
fn bracket_matching_is_empty_when_caret_is_outside_any_pair() {
    let source = "left()  right[]";
    let caret = source.find("right").unwrap();
    assert!(bracket_match_for_source(source, caret, &[]).is_none());

    let caret = source.len();
    assert!(bracket_match_for_source(source, caret, &[]).is_some());
}

#[test]
fn bracket_matching_does_not_highlight_first_pair_from_root_text() {
    let source = "root\n  first: []\n  second: {}\n";
    let caret = source.find("root").unwrap() + "root".len();
    assert!(bracket_match_for_source(source, caret, &[]).is_none());
}
