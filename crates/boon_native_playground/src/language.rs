use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use boon_document_model::{StyleEditorTypeHint, StyleRichTextSpan};
use boon_parser::{AstToken, AstTokenKind, lex_source, parse_project};
use boon_typecheck::TypeDisplayNode;
use futures::channel::mpsc;

use crate::protocol::SourceUnit;

const ANALYSIS_QUIET: Duration = Duration::from_millis(90);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorHint {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub category: String,
    pub compact_label: String,
    pub detail_label: String,
    pub display_tree: TypeDisplayNode,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineDecorations {
    pub spans: Vec<StyleRichTextSpan>,
    pub type_hints: Vec<StyleEditorTypeHint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageSnapshot {
    pub revision: u64,
    pub file_index: usize,
    pub path: String,
    pub lines: Vec<LineDecorations>,
    pub inspector_hints: Vec<InspectorHint>,
    pub diagnostics: Vec<String>,
}

impl LanguageSnapshot {
    pub fn hint_at(&self, byte: usize) -> Option<&InspectorHint> {
        self.inspector_hints
            .iter()
            .filter(|hint| hint.start <= byte && byte <= hint.end)
            .min_by_key(|hint| hint.end.saturating_sub(hint.start))
    }
}

#[derive(Clone)]
struct AnalysisJob {
    revision: u64,
    file_index: usize,
    units: Vec<SourceUnit>,
}

#[derive(Default)]
struct WorkerState {
    generation: u64,
    pending: Option<AnalysisJob>,
    stop: bool,
}

pub struct LanguageWorker {
    state: Arc<(Mutex<WorkerState>, Condvar)>,
    output: Option<mpsc::UnboundedReceiver<LanguageSnapshot>>,
    thread: Option<JoinHandle<()>>,
}

impl LanguageWorker {
    pub fn new() -> Self {
        let state = Arc::new((Mutex::new(WorkerState::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let (output_tx, output) = mpsc::unbounded();
        let thread = thread::Builder::new()
            .name("boon-dev-language".to_owned())
            .spawn(move || worker_loop(&worker_state, &output_tx))
            .expect("spawn Boon language worker");
        Self {
            state,
            output: Some(output),
            thread: Some(thread),
        }
    }

    pub fn submit(&self, revision: u64, file_index: usize, units: Vec<SourceUnit>) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("language worker state");
        state.generation = state.generation.saturating_add(1);
        state.pending = Some(AnalysisJob {
            revision,
            file_index,
            units,
        });
        wake.notify_one();
    }

    pub fn take_output(&mut self) -> mpsc::UnboundedReceiver<LanguageSnapshot> {
        self.output.take().expect("language output already taken")
    }
}

impl Drop for LanguageWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().expect("language worker state").stop = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn worker_loop(
    shared: &Arc<(Mutex<WorkerState>, Condvar)>,
    output: &mpsc::UnboundedSender<LanguageSnapshot>,
) {
    let (lock, wake) = &**shared;
    loop {
        let mut state = lock.lock().expect("language worker state");
        while state.pending.is_none() && !state.stop {
            state = wake.wait(state).expect("language worker wait");
        }
        if state.stop {
            return;
        }
        let mut generation = state.generation;
        loop {
            let (next, timeout) = wake
                .wait_timeout(state, ANALYSIS_QUIET)
                .expect("language worker debounce");
            state = next;
            if state.stop {
                return;
            }
            if state.generation == generation && timeout.timed_out() {
                break;
            }
            generation = state.generation;
        }
        let Some(job) = state.pending.take() else {
            continue;
        };
        drop(state);
        if output.unbounded_send(analyze(job)).is_err() {
            return;
        }
    }
}

fn analyze(job: AnalysisJob) -> LanguageSnapshot {
    let active = job.units.get(job.file_index);
    let path = active.map_or_else(|| "RUN.bn".to_owned(), |unit| unit.path.clone());
    let source = active.map_or("", |unit| unit.source.as_str());
    let tokens = lex_source(&path, source).unwrap_or_default();
    let mut lines = syntax_lines(source, &tokens);
    let mut inspector_hints = Vec::new();
    let mut diagnostics = Vec::new();

    match parse_project(
        "playground",
        job.units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    ) {
        Ok(program) => {
            let report = boon_typecheck::check(&program);
            if let Some(file) = program.files.get(job.file_index) {
                let file_start = byte_offset_for_line(&program.source, file.start_line);
                let file_end = file_start.saturating_add(file.source.len());
                for hint in report.type_hint_table.entries {
                    if hint.start < file_start || hint.start > file_end {
                        continue;
                    }
                    let local_line = hint.line.saturating_sub(file.start_line);
                    let local_start = hint.start.saturating_sub(file_start);
                    let local_end = hint.end.saturating_sub(file_start).min(file.source.len());
                    if let Some(line) = lines.get_mut(local_line) {
                        line.type_hints.push(StyleEditorTypeHint {
                            line: local_line,
                            start: local_start,
                            end: local_end,
                            anchor_column: hint.anchor_column,
                            category: hint.category.clone(),
                            compact_label: hint.compact_label.clone(),
                            detail_label: hint.detail_label.clone(),
                        });
                    }
                    inspector_hints.push(InspectorHint {
                        line: local_line,
                        start: local_start,
                        end: local_end,
                        category: hint.category,
                        compact_label: hint.compact_label,
                        detail_label: hint.detail_label,
                        display_tree: hint.display_tree,
                    });
                }
                diagnostics.extend(report.diagnostics.into_iter().filter_map(|diagnostic| {
                    (diagnostic.line >= file.start_line
                        && diagnostic.line < file.start_line + file.source.lines().count().max(1))
                    .then(|| {
                        format!(
                            "{}:{}: {}",
                            file.path,
                            diagnostic.line.saturating_sub(file.start_line) + 1,
                            diagnostic.message
                        )
                    })
                }));
            }
        }
        Err(error) => diagnostics.push(error.to_string()),
    }

    LanguageSnapshot {
        revision: job.revision,
        file_index: job.file_index,
        path,
        lines,
        inspector_hints,
        diagnostics,
    }
}

fn syntax_lines(source: &str, tokens: &[AstToken]) -> Vec<LineDecorations> {
    let offsets = line_offsets(source);
    let line_count = offsets.len().max(1);
    (0..line_count)
        .map(|line| {
            let start = offsets.get(line).copied().unwrap_or(0);
            let raw_end = offsets.get(line + 1).copied().unwrap_or(source.len());
            let end = if raw_end > start && source.as_bytes().get(raw_end - 1) == Some(&b'\n') {
                raw_end - 1
            } else {
                raw_end
            };
            LineDecorations {
                spans: spans_for_range(source, tokens, start, end),
                type_hints: Vec::new(),
            }
        })
        .collect()
}

fn spans_for_range(
    source: &str,
    tokens: &[AstToken],
    start: usize,
    end: usize,
) -> Vec<StyleRichTextSpan> {
    let mut spans = Vec::new();
    let mut cursor = start;
    for token in tokens
        .iter()
        .filter(|token| token.end > start && token.start < end)
    {
        let token_start = token.start.max(start).max(cursor);
        let token_end = token.end.min(end);
        if cursor < token_start {
            spans.push(span(&source[cursor..token_start], None, None, None));
        }
        if token_start < token_end {
            let (color, weight, style) = token_style(token, tokens);
            spans.push(span(
                &source[token_start..token_end],
                Some(color),
                weight,
                style,
            ));
            cursor = token_end;
        }
    }
    if cursor < end {
        spans.push(span(&source[cursor..end], None, None, None));
    }
    spans
}

fn span(
    text: &str,
    color: Option<&str>,
    font_weight: Option<&str>,
    font_style: Option<&str>,
) -> StyleRichTextSpan {
    StyleRichTextSpan {
        text: text.to_owned(),
        source_text: Some(text.to_owned()),
        color: color.map(str::to_owned),
        font_style: font_style.map(str::to_owned),
        font_weight: font_weight.map(str::to_owned),
    }
}

fn token_style(
    token: &AstToken,
    tokens: &[AstToken],
) -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match token.kind {
        AstTokenKind::Comment => ("#778899", None, Some("italic")),
        AstTokenKind::String => ("#fff59e", None, None),
        AstTokenKind::Number => ("#7ad1ff", None, None),
        AstTokenKind::Operator if token.lexeme == "|>" => ("#D2691E", Some("700"), None),
        AstTokenKind::Operator => ("#ff9f43", Some("600"), None),
        AstTokenKind::Symbol => ("#D2691E", Some("700"), None),
        AstTokenKind::Unknown => ("#ffffff", None, None),
        AstTokenKind::Newline => ("#d9e1f2", None, None),
        AstTokenKind::Identifier if is_keyword(&token.lexeme) => {
            ("#D2691E", Some("800"), Some("italic"))
        }
        AstTokenKind::Identifier if is_definition(token, tokens) => {
            ("#ff6ec7", Some("600"), Some("italic"))
        }
        AstTokenKind::Identifier if is_function(token, tokens) => ("#fcbf49", Some("600"), None),
        AstTokenKind::Identifier if token.lexeme.contains('/') => ("#6cb6ff", None, None),
        AstTokenKind::Identifier if is_tag(token, tokens) => ("#6df59a", None, None),
        AstTokenKind::Identifier if is_type(&token.lexeme) => ("#6f9cff", None, None),
        AstTokenKind::Identifier => ("#eeeeee", None, None),
    }
}

fn is_definition(token: &AstToken, tokens: &[AstToken]) -> bool {
    next_token(token, tokens)
        .is_some_and(|candidate| candidate.kind == AstTokenKind::Symbol && candidate.lexeme == ":")
}

fn is_keyword(value: &str) -> bool {
    value.chars().count() >= 2
        && value
            .chars()
            .any(|character| character.is_ascii_uppercase())
        && value
            .chars()
            .all(|character| character.is_ascii_uppercase() || character == '_')
}

fn is_function(token: &AstToken, tokens: &[AstToken]) -> bool {
    next_token(token, tokens)
        .is_some_and(|candidate| candidate.kind == AstTokenKind::Symbol && candidate.lexeme == "(")
        || previous_token(token, tokens).is_some_and(|candidate| candidate.lexeme == "FUNCTION")
}

fn is_tag(token: &AstToken, tokens: &[AstToken]) -> bool {
    matches!(token.lexeme.as_str(), "True" | "False" | "Null")
        || (is_type(&token.lexeme)
            && next_token(token, tokens).is_some_and(|candidate| {
                candidate.kind == AstTokenKind::Symbol && candidate.lexeme == "["
            }))
}

fn is_type(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
}

fn next_token<'a>(token: &AstToken, tokens: &'a [AstToken]) -> Option<&'a AstToken> {
    tokens.iter().find(|candidate| {
        candidate.start >= token.end
            && candidate.line == token.line
            && candidate.kind != AstTokenKind::Newline
    })
}

fn previous_token<'a>(token: &AstToken, tokens: &'a [AstToken]) -> Option<&'a AstToken> {
    tokens
        .iter()
        .rev()
        .find(|candidate| candidate.end <= token.start && candidate.kind != AstTokenKind::Newline)
}

fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    offsets.extend(source.match_indices('\n').map(|(offset, _)| offset + 1));
    offsets
}

fn byte_offset_for_line(source: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    source
        .match_indices('\n')
        .nth(line - 2)
        .map_or(source.len(), |(offset, _)| offset + 1)
}
