use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use boon_document_model::{StyleEditorTypeHint, StyleRichTextSpan};
use boon_parser::{AstToken, AstTokenKind, lex_source, parse_project};
use boon_typecheck::{
    CheckedCallEntry, CheckedDeclarationKind, CheckedProgram, CheckedSpan, DeclId,
    DiagnosticSeverity, SemanticOccurrenceKind as CheckedSemanticOccurrenceKind, TypeDisplayNode,
};
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SemanticKind {
    Declaration,
    Reference,
    Call,
    FreshOut,
    ForwardOut,
    Pass,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub file_index: usize,
    pub path: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticItem {
    pub target: DeclId,
    pub kind: SemanticKind,
    pub location: SourceLocation,
    pub name: String,
    pub label: String,
    pub detail: String,
    pub out_related: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDiagnostic {
    pub severity: DiagnosticSeverity,
    pub location: SourceLocation,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageSnapshot {
    pub revision: u64,
    pub file_index: usize,
    pub path: String,
    pub lines: Vec<LineDecorations>,
    pub inspector_hints: Vec<InspectorHint>,
    pub semantics: Vec<SemanticItem>,
    pub diagnostics: Vec<SemanticDiagnostic>,
    pub inline_out_hints: bool,
}

impl LanguageSnapshot {
    pub fn hint_at(&self, byte: usize) -> Option<&InspectorHint> {
        self.inspector_hints
            .iter()
            .filter(|hint| hint.start <= byte && byte <= hint.end)
            .min_by_key(|hint| hint.end.saturating_sub(hint.start))
    }

    pub fn semantic_at(&self, byte: usize) -> Option<&SemanticItem> {
        self.semantics
            .iter()
            .filter(|item| {
                item.location.file_index == self.file_index
                    && item.location.start <= byte
                    && byte <= item.location.end
            })
            .min_by_key(|item| {
                (
                    item.location.end.saturating_sub(item.location.start),
                    semantic_priority(item.kind),
                )
            })
    }

    pub fn definition_at(&self, byte: usize) -> Option<&SemanticItem> {
        let target = self.semantic_at(byte)?.target;
        self.semantics
            .iter()
            .filter(|item| {
                item.target == target
                    && matches!(
                        item.kind,
                        SemanticKind::Declaration | SemanticKind::FreshOut
                    )
            })
            .min_by_key(|item| {
                (
                    item.location.file_index,
                    item.location.start,
                    semantic_priority(item.kind),
                )
            })
    }

    pub fn references_at(&self, byte: usize) -> Vec<&SemanticItem> {
        let Some(target) = self.semantic_at(byte).map(|item| item.target) else {
            return Vec::new();
        };
        let mut references = self
            .semantics
            .iter()
            .filter(|item| {
                item.target == target
                    && !matches!(
                        item.kind,
                        SemanticKind::Declaration | SemanticKind::FreshOut
                    )
            })
            .collect::<Vec<_>>();
        references.sort_by_key(|item| {
            (
                item.location.file_index,
                item.location.start,
                item.location.end,
                semantic_priority(item.kind),
            )
        });
        references
    }

    pub fn next_reference_at(&self, byte: usize) -> Option<&SemanticItem> {
        let references = self.references_at(byte);
        references
            .iter()
            .copied()
            .find(|item| (item.location.file_index, item.location.start) > (self.file_index, byte))
            .or_else(|| references.first().copied())
    }

    pub fn diagnostics_text(&self) -> String {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.location.file_index == self.file_index)
            .map(|diagnostic| {
                let severity = match diagnostic.severity {
                    DiagnosticSeverity::Error => "error",
                    DiagnosticSeverity::Warning => "warning",
                };
                format!(
                    "{}:{}: {severity}: {}",
                    diagnostic.location.path,
                    diagnostic.location.line + 1,
                    diagnostic.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
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
    let project_tokens = job
        .units
        .iter()
        .map(|unit| lex_source(&unit.path, &unit.source).unwrap_or_default())
        .collect::<Vec<_>>();
    let mut inspector_hints = Vec::new();
    let mut line_type_hints = Vec::new();
    let mut semantics = Vec::new();
    let mut diagnostics = Vec::new();

    match parse_project(
        "playground",
        job.units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    ) {
        Ok(program) => {
            let output = boon_typecheck::check_program(&program);
            if let Some(checked) = output.program.as_ref() {
                semantics = semantic_items(&program, checked, &project_tokens);
            }
            if let Some(file) = program.files.get(job.file_index) {
                let file_start = byte_offset_for_line(&program.source, file.start_line);
                let file_end = file_start.saturating_add(file.source.len());
                for hint in output.report.type_hint_table.entries {
                    if hint.start < file_start || hint.start > file_end {
                        continue;
                    }
                    let local_line = hint.line.saturating_sub(file.start_line);
                    let local_start = hint.start.saturating_sub(file_start);
                    let local_end = hint.end.saturating_sub(file_start).min(file.source.len());
                    line_type_hints.push((
                        local_line,
                        StyleEditorTypeHint {
                            line: local_line,
                            start: local_start,
                            end: local_end,
                            anchor_column: hint.anchor_column,
                            category: hint.category.clone(),
                            compact_label: hint.compact_label.clone(),
                            detail_label: hint.detail_label.clone(),
                        },
                    ));
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
            }
            diagnostics.extend(
                output
                    .report
                    .diagnostics
                    .into_iter()
                    .filter_map(|diagnostic| {
                        source_location_for_span(
                            &program,
                            CheckedSpan {
                                line: diagnostic.line,
                                start: diagnostic.start,
                                end: diagnostic.end,
                            },
                        )
                        .map(|location| SemanticDiagnostic {
                            severity: diagnostic.severity,
                            location,
                            message: diagnostic.message,
                        })
                    }),
            );
        }
        Err(error) => diagnostics.push(parse_diagnostic(&job.units, job.file_index, error)),
    }

    let active_semantics = semantics
        .iter()
        .filter(|item| item.location.file_index == job.file_index)
        .collect::<Vec<_>>();
    let mut lines = syntax_lines(source, &tokens, &active_semantics);
    for (line, hint) in line_type_hints {
        if let Some(decorations) = lines.get_mut(line) {
            decorations.type_hints.push(hint);
        }
    }

    LanguageSnapshot {
        revision: job.revision,
        file_index: job.file_index,
        path,
        lines,
        inspector_hints,
        semantics,
        diagnostics,
        inline_out_hints: false,
    }
}

fn semantic_items(
    program: &boon_parser::ParsedProgram,
    checked: &CheckedProgram,
    tokens_by_file: &[Vec<AstToken>],
) -> Vec<SemanticItem> {
    let declarations = checked
        .declarations
        .iter()
        .map(|declaration| (declaration.id, declaration))
        .collect::<BTreeMap<_, _>>();
    let callable_names = checked
        .callables
        .iter()
        .map(|callable| (callable.decl_id, callable.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut items = checked
        .occurrences
        .iter()
        .filter_map(|occurrence| {
            let kind = semantic_kind(occurrence.kind);
            let declaration = declarations.get(&occurrence.target).copied();
            let name = declaration
                .map(|declaration| declaration.name.clone())
                .or_else(|| {
                    callable_names
                        .get(&occurrence.target)
                        .map(|name| (*name).to_owned())
                })
                .unwrap_or_else(|| format!("declaration {}", occurrence.target.0));
            let mut location = source_location_for_span(program, occurrence.span)?;
            if let Some(tokens) = tokens_by_file.get(location.file_index) {
                refine_semantic_location(&mut location, tokens, kind, &name);
            }
            let declaration_kind = declaration.map(|declaration| declaration.kind);
            let out_related = matches!(
                declaration_kind,
                Some(CheckedDeclarationKind::OutParameter | CheckedDeclarationKind::FreshOut)
            );
            let (label, detail) = semantic_description(
                checked,
                occurrence.target,
                occurrence.span,
                kind,
                &name,
                declaration_kind,
            );
            Some(SemanticItem {
                target: occurrence.target,
                kind,
                location,
                name,
                label,
                detail,
                out_related,
            })
        })
        .collect::<Vec<_>>();

    items.sort_by_key(|item| {
        (
            item.location.file_index,
            item.location.start,
            item.location.end,
            semantic_priority(item.kind),
            item.target,
        )
    });
    let definitions = items
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                SemanticKind::Declaration | SemanticKind::FreshOut
            )
        })
        .map(|item| (item.target, item.location.clone()))
        .collect::<BTreeMap<_, _>>();
    for item in &mut items {
        if matches!(
            item.kind,
            SemanticKind::Declaration | SemanticKind::FreshOut
        ) {
            continue;
        }
        if let Some(definition) = definitions.get(&item.target) {
            item.detail.push_str(&format!(
                "\nDefined at {}:{}",
                definition.path,
                definition.line + 1
            ));
        }
    }
    items
}

fn semantic_kind(kind: CheckedSemanticOccurrenceKind) -> SemanticKind {
    match kind {
        CheckedSemanticOccurrenceKind::Declaration => SemanticKind::Declaration,
        CheckedSemanticOccurrenceKind::Read => SemanticKind::Reference,
        CheckedSemanticOccurrenceKind::Call => SemanticKind::Call,
        CheckedSemanticOccurrenceKind::FreshOut => SemanticKind::FreshOut,
        CheckedSemanticOccurrenceKind::ForwardOut => SemanticKind::ForwardOut,
        CheckedSemanticOccurrenceKind::Pass => SemanticKind::Pass,
    }
}

fn semantic_priority(kind: SemanticKind) -> u8 {
    match kind {
        SemanticKind::FreshOut => 0,
        SemanticKind::ForwardOut => 1,
        SemanticKind::Pass => 2,
        SemanticKind::Call => 3,
        SemanticKind::Reference => 4,
        SemanticKind::Declaration => 5,
    }
}

fn semantic_description(
    checked: &CheckedProgram,
    target: DeclId,
    span: CheckedSpan,
    kind: SemanticKind,
    name: &str,
    declaration_kind: Option<CheckedDeclarationKind>,
) -> (String, String) {
    match kind {
        SemanticKind::FreshOut => {
            let provider = checked.calls.iter().find_map(|call| {
                call.entries.iter().find_map(|entry| match entry {
                    CheckedCallEntry::FreshOut {
                        name: formal,
                        output,
                        ..
                    } if *output == target => Some((call.function.as_str(), formal.as_str())),
                    _ => None,
                })
            });
            provider.map_or_else(
                || {
                    (
                        format!("OUT {name}"),
                        "Fresh output binding supplied by this call".to_owned(),
                    )
                },
                |(function, formal)| {
                    (
                        format!("OUT {name}, supplied by {function}"),
                        format!("{function}.{formal} creates this scoped output"),
                    )
                },
            )
        }
        SemanticKind::ForwardOut => {
            let provider = checked
                .calls
                .iter()
                .filter(|call| call.span.start <= span.start && span.end <= call.span.end)
                .filter_map(|call| {
                    call.entries.iter().find_map(|entry| match entry {
                        CheckedCallEntry::ForwardOut {
                            name: formal,
                            target: forwarded,
                            target_name,
                            ..
                        } if *forwarded == target => Some((
                            call.span.end.saturating_sub(call.span.start),
                            call.function.as_str(),
                            formal.as_str(),
                            target_name.as_str(),
                        )),
                        _ => None,
                    })
                })
                .min_by_key(|(width, ..)| *width);
            provider.map_or_else(
                || {
                    (
                        format!("OUT {name}, forwarded"),
                        "Forwards an enclosing output into this call".to_owned(),
                    )
                },
                |(_, function, formal, target_name)| {
                    (
                        format!("OUT {target_name}, forwarded to {function}.{formal}"),
                        format!("{function}.{formal} receives enclosing OUT {target_name}"),
                    )
                },
            )
        }
        SemanticKind::Reference => {
            let target_kind = match declaration_kind {
                Some(CheckedDeclarationKind::OutParameter | CheckedDeclarationKind::FreshOut) => {
                    "OUT"
                }
                Some(CheckedDeclarationKind::Function) => "function",
                Some(CheckedDeclarationKind::Source) => "SOURCE",
                Some(CheckedDeclarationKind::Hold) => "HOLD",
                Some(CheckedDeclarationKind::List) => "LIST",
                _ => "value",
            };
            (
                format!("Reference {name}"),
                format!("Reads {target_kind} {name}"),
            )
        }
        SemanticKind::Call => (
            format!("Call {name}"),
            format!("Calls {name}; F12 opens its declaration"),
        ),
        SemanticKind::Pass => (
            format!("PASS context for {name}"),
            format!("Supplies lexical context to {name}; F12 opens its declaration"),
        ),
        SemanticKind::Declaration => {
            let category = match declaration_kind {
                Some(CheckedDeclarationKind::OutParameter) => "OUT parameter",
                Some(CheckedDeclarationKind::ValueParameter) => "parameter",
                Some(CheckedDeclarationKind::Function) => "function",
                Some(CheckedDeclarationKind::PatternBinding) => "pattern binding",
                Some(CheckedDeclarationKind::Source) => "SOURCE",
                Some(CheckedDeclarationKind::Hold) => "HOLD",
                Some(CheckedDeclarationKind::List) => "LIST",
                Some(CheckedDeclarationKind::ElementState) => "element state",
                _ => "declaration",
            };
            (
                format!("{category} {name}"),
                format!("Declares {category} {name}"),
            )
        }
    }
}

fn source_location_for_span(
    program: &boon_parser::ParsedProgram,
    span: CheckedSpan,
) -> Option<SourceLocation> {
    let by_line = program
        .files
        .iter()
        .enumerate()
        .find_map(|(file_index, file)| {
            let lines = file.source.lines().count().max(1);
            (span.line >= file.start_line && span.line < file.start_line.saturating_add(lines))
                .then(|| {
                    (
                        file_index,
                        file,
                        byte_offset_for_line(&program.source, file.start_line),
                    )
                })
        });
    let (file_index, file, file_start) = by_line.or_else(|| {
        program
            .files
            .iter()
            .enumerate()
            .find_map(|(file_index, file)| {
                let file_start = byte_offset_for_line(&program.source, file.start_line);
                let file_end = file_start.saturating_add(file.source.len());
                (file_start <= span.start && span.start <= file_end)
                    .then_some((file_index, file, file_start))
            })
    })?;
    let local_start = span.start.saturating_sub(file_start).min(file.source.len());
    let line = if span.line >= file.start_line {
        span.line.saturating_sub(file.start_line)
    } else {
        file.source[..local_start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
    };
    Some(SourceLocation {
        file_index,
        path: file.path.clone(),
        line,
        start: local_start,
        end: span.end.saturating_sub(file_start).min(file.source.len()),
    })
}

fn refine_semantic_location(
    location: &mut SourceLocation,
    tokens: &[AstToken],
    kind: SemanticKind,
    name: &str,
) {
    let expected = match kind {
        SemanticKind::Pass => "PASS",
        SemanticKind::Declaration
        | SemanticKind::Call
        | SemanticKind::FreshOut
        | SemanticKind::ForwardOut => name,
        SemanticKind::Reference => return,
    };
    let short_expected = expected.rsplit('/').next().unwrap_or(expected);
    // The checked occurrence owns identity and meaning. Lexing only narrows its
    // compiler-provided range to the token painted by the editor.
    if let Some(token) = tokens.iter().find(|token| {
        token.start >= location.start
            && token.end <= location.end
            && (token.lexeme == expected || token.lexeme == short_expected)
    }) {
        location.start = token.start;
        location.end = token.end;
        location.line = token.line.saturating_sub(1);
    }
}

fn parse_diagnostic(
    units: &[SourceUnit],
    active_file: usize,
    error: boon_parser::ParseError,
) -> SemanticDiagnostic {
    let file_index = units
        .iter()
        .position(|unit| unit.path == error.path)
        .unwrap_or_else(|| active_file.min(units.len().saturating_sub(1)));
    let unit = units.get(file_index);
    let path = unit.map_or_else(|| error.path.clone(), |unit| unit.path.clone());
    let source = unit.map_or("", |unit| unit.source.as_str());
    let line = error.line.unwrap_or(1).saturating_sub(1);
    let line_start = byte_offset_for_line(source, line + 1);
    let start = line_start
        .saturating_add(error.column.unwrap_or(1).saturating_sub(1))
        .min(source.len());
    SemanticDiagnostic {
        severity: DiagnosticSeverity::Error,
        location: SourceLocation {
            file_index,
            path,
            line,
            start,
            end: start,
        },
        message: error.message,
    }
}

fn syntax_lines(
    source: &str,
    tokens: &[AstToken],
    semantics: &[&SemanticItem],
) -> Vec<LineDecorations> {
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
                spans: spans_for_range(source, tokens, semantics, start, end),
                type_hints: Vec::new(),
            }
        })
        .collect()
}

fn spans_for_range(
    source: &str,
    tokens: &[AstToken],
    semantics: &[&SemanticItem],
    start: usize,
    end: usize,
) -> Vec<StyleRichTextSpan> {
    let line_tokens = tokens
        .iter()
        .filter(|token| token.end > start && token.start < end)
        .collect::<Vec<_>>();
    let line_semantics = semantics
        .iter()
        .copied()
        .filter(|item| item.location.end > start && item.location.start < end)
        .collect::<Vec<_>>();
    let mut boundaries = vec![start, end];
    for token in &line_tokens {
        boundaries.push(token.start.max(start));
        boundaries.push(token.end.min(end));
    }
    for item in &line_semantics {
        boundaries.push(item.location.start.max(start));
        boundaries.push(item.location.end.min(end));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|range| {
            let segment_start = range[0];
            let segment_end = range[1];
            (segment_start < segment_end).then(|| {
                let semantic = line_semantics
                    .iter()
                    .copied()
                    .filter(|item| {
                        item.location.start <= segment_start && segment_end <= item.location.end
                    })
                    .min_by_key(|item| {
                        (
                            item.location.end.saturating_sub(item.location.start),
                            semantic_priority(item.kind),
                        )
                    });
                let lexical = line_tokens
                    .iter()
                    .copied()
                    .find(|token| token.start <= segment_start && segment_end <= token.end);
                let (color, weight, style) = semantic.map_or_else(
                    || {
                        lexical.map_or((None, None, None), |token| {
                            let (color, weight, style) = token_style(token, tokens);
                            (Some(color), weight, style)
                        })
                    },
                    |item| {
                        let (color, weight, style) = semantic_style(item);
                        (Some(color), weight, style)
                    },
                );
                span(&source[segment_start..segment_end], color, weight, style)
            })
        })
        .collect()
}

fn semantic_style(
    item: &SemanticItem,
) -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match item.kind {
        SemanticKind::FreshOut => ("#53e0c1", Some("800"), Some("italic")),
        SemanticKind::ForwardOut => ("#e995ff", Some("750"), Some("italic")),
        SemanticKind::Pass => ("#ffad66", Some("750"), Some("italic")),
        SemanticKind::Call => ("#fcbf49", Some("650"), None),
        SemanticKind::Reference if item.out_related => ("#8ee8d4", Some("600"), None),
        SemanticKind::Reference => ("#d9e1f2", None, None),
        SemanticKind::Declaration if item.out_related => ("#53e0c1", Some("750"), Some("italic")),
        SemanticKind::Declaration => ("#ff6ec7", Some("650"), Some("italic")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(source: &str) -> LanguageSnapshot {
        analyze(AnalysisJob {
            revision: 7,
            file_index: 0,
            units: vec![SourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
        })
    }

    #[test]
    fn checked_semantics_drive_styles_hover_definitions_and_references() {
        let source = r#"FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new
    )
}

FUNCTION render(value) {
    value
}

rows: LIST { [value: 2] }
mapped:
    rows
    |> doubled(
        entry
        new: entry.value * 2
    )
shown: render(value: mapped, PASS: [store: [count: 1]])
"#;
        let snapshot = snapshot(source);
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:#?}",
            snapshot.diagnostics
        );
        assert!(!snapshot.inline_out_hints);
        for kind in [
            SemanticKind::Declaration,
            SemanticKind::Reference,
            SemanticKind::Call,
            SemanticKind::FreshOut,
            SemanticKind::ForwardOut,
            SemanticKind::Pass,
        ] {
            assert!(
                snapshot.semantics.iter().any(|item| item.kind == kind),
                "missing {kind:?} in {:#?}",
                snapshot.semantics
            );
        }

        let fresh = snapshot
            .semantics
            .iter()
            .find(|item| item.kind == SemanticKind::FreshOut && item.name == "entry")
            .expect("outer call creates entry");
        assert!(
            fresh.label.contains("supplied by RUN/doubled"),
            "{}",
            fresh.label
        );
        assert_eq!(
            snapshot
                .semantic_at(fresh.location.start)
                .map(|item| item.target),
            Some(fresh.target)
        );
        assert_eq!(
            snapshot
                .definition_at(fresh.location.start)
                .map(|item| item.kind),
            Some(SemanticKind::FreshOut)
        );
        let references = snapshot.references_at(fresh.location.start);
        assert!(
            references
                .iter()
                .any(|item| item.kind == SemanticKind::Reference)
        );

        let painted = snapshot.lines[fresh.location.line]
            .spans
            .iter()
            .find(|span| {
                span.source_text.as_deref() == Some("entry")
                    && span.color.as_deref() == Some("#53e0c1")
            });
        assert!(
            painted.is_some(),
            "fresh OUT token was not semantically painted"
        );

        let forwarded = snapshot
            .semantics
            .iter()
            .find(|item| item.kind == SemanticKind::ForwardOut)
            .expect("wrapper forwards entry to List/map.item");
        assert!(
            forwarded.label.contains("forwarded to List/map.item"),
            "{}",
            forwarded.label
        );
        assert_eq!(
            snapshot
                .definition_at(forwarded.location.start)
                .map(|item| item.name.as_str()),
            Some("entry")
        );

        let pass = snapshot
            .semantics
            .iter()
            .find(|item| item.kind == SemanticKind::Pass)
            .expect("PASS is a checked occurrence");
        assert_eq!(&source[pass.location.start..pass.location.end], "PASS");
        assert_eq!(
            snapshot
                .definition_at(pass.location.start)
                .map(|item| item.name.as_str()),
            Some("RUN/render")
        );
    }

    #[test]
    fn checked_declaration_identity_navigates_across_project_files() {
        let units = vec![
            SourceUnit {
                path: "Math.bn".to_owned(),
                source: "FUNCTION double(value) {\n    value * 2\n}\n".to_owned(),
            },
            SourceUnit {
                path: "RUN.bn".to_owned(),
                source: "result: Math/double(value: 21)\n".to_owned(),
            },
        ];
        let snapshot = analyze(AnalysisJob {
            revision: 9,
            file_index: 1,
            units,
        });
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:#?}",
            snapshot.diagnostics
        );
        let call = snapshot
            .semantics
            .iter()
            .find(|item| item.kind == SemanticKind::Call && item.location.file_index == 1)
            .expect("cross-file call occurrence");
        let definition = snapshot
            .definition_at(call.location.start)
            .expect("cross-file source definition");
        assert_eq!(definition.location.file_index, 0);
        assert_eq!(definition.location.path, "Math.bn");
        assert_eq!(definition.name, "Math/double");
    }

    #[test]
    fn typechecker_cycle_errors_remain_structured_and_source_bound() {
        let snapshot = snapshot(
            r#"FUNCTION first(list, entry: OUT, new) {
    list |> second(entry: entry, new: new)
}

FUNCTION second(list, entry: OUT, new) {
    list |> first(entry: entry, new: new)
}

result: LIST { 1 } |> first(entry, new: entry)
"#,
        );
        let cycle = snapshot
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("OUT forwarding cycle"))
            .expect("typed OUT cycle diagnostic");
        assert_eq!(cycle.severity, DiagnosticSeverity::Error);
        assert_eq!(cycle.location.path, "RUN.bn");
        assert!(cycle.location.end >= cycle.location.start);
        assert!(snapshot.diagnostics_text().contains("OUT forwarding cycle"));
    }
}
