use std::collections::BTreeMap;

use boon_checked::{
    CheckOutput, CheckedCallEntry, CheckedContextBinding, CheckedDeclarationKind,
    CheckedProgramFields, CheckedSpan, DeclId, DiagnosticSeverity,
    SemanticOccurrenceKind as CheckedSemanticOccurrenceKind, TypeDisplayNode,
};
use boon_contract::{CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleUnit};
use boon_document_model::{StyleEditorTypeHint, StyleRichTextSpan};
use boon_parser::{ParseError, ParsedProgram, ProjectSyntaxSnapshot, lex_source};
use boon_syntax::{AstToken, AstTokenKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectorHint {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub anchor_column: usize,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SemanticKind {
    Declaration,
    Reference,
    Call,
    FreshOut,
    ForwardOut,
    Pass,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceLocation {
    pub file_index: usize,
    pub path: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemanticItem {
    pub target: DeclId,
    pub kind: SemanticKind,
    pub location: SourceLocation,
    pub name: String,
    pub label: String,
    pub detail: String,
    pub out_related: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// Compact, compiler-produced language data for one source file.
///
/// Source text and painted lines deliberately stay in the dev process. The
/// compiler service sends only checked hints and project-wide semantic data;
/// [`LanguageProjectSnapshot::materialize_file`] lexes and paints the selected
/// local source file without parsing or typechecking the project again.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageFileIndex {
    /// Canonical UTF-8 project-relative path at this dev-facing file index.
    pub path: String,
    pub inspector_hints: Vec<InspectorHint>,
}

/// Serializable editor projection of one exact compiler source revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageProjectSnapshot {
    pub revision: u64,
    pub entrypoint: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    /// Files are ordered exactly like the dev-owned [`SourceUnit`] slice.
    pub files: Vec<LanguageFileIndex>,
    /// All project occurrences are retained so navigation can cross files.
    pub semantics: Vec<SemanticItem>,
    pub diagnostics: Vec<SemanticDiagnostic>,
    pub inline_out_hints: bool,
}

impl LanguageProjectSnapshot {
    /// Proves that the dev-owned bytes are the exact compiler snapshot from
    /// which this projection was produced.
    pub fn matches_source_units(&self, units: &[SourceUnit]) -> bool {
        canonical_source_bundle(&self.entrypoint, units).is_ok_and(|bundle| {
            bundle.digest() == self.source_bundle_digest_v1
                && bundle.units().len() == self.files.len()
                && self.files.iter().enumerate().all(|(dev_index, file)| {
                    units
                        .get(dev_index)
                        .and_then(|unit| boon_contract::normalize_source_path(&unit.path).ok())
                        .is_some_and(|path| path == file.path)
                })
        })
    }

    /// Materializes the existing active-file `LanguageSnapshot` API using only
    /// a local lexical pass over that file. Callers should first use
    /// [`Self::matches_source_units`] when accepting a new IPC snapshot; file
    /// switches on the already accepted revision need only this path check.
    pub fn materialize_file(
        &self,
        file_index: usize,
        unit: &SourceUnit,
    ) -> Option<LanguageSnapshot> {
        let file = self.files.get(file_index)?;
        let unit_path = boon_contract::normalize_source_path(&unit.path).ok()?;
        if unit_path != file.path {
            return None;
        }
        let tokens = lex_source(&file.path, &unit.source).unwrap_or_default();
        let active_semantics = self
            .semantics
            .iter()
            .filter(|item| item.location.file_index == file_index)
            .collect::<Vec<_>>();
        let mut lines = syntax_lines(&unit.source, &tokens, &active_semantics);
        for hint in &file.inspector_hints {
            if let Some(decorations) = lines.get_mut(hint.line) {
                decorations.type_hints.push(style_type_hint(hint));
            }
        }
        Some(LanguageSnapshot {
            revision: self.revision,
            file_index,
            path: file.path.clone(),
            lines,
            inspector_hints: file.inspector_hints.clone(),
            semantics: self.semantics.clone(),
            diagnostics: self.diagnostics.clone(),
            inline_out_hints: self.inline_out_hints,
        })
    }
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

/// Projects editor data from the exact parsed/typechecked artifact owned by the
/// compiler service. This function never parses, lexes, or typechecks source.
pub fn project_checked_language(
    revision: u64,
    units: &[SourceUnit],
    program: &ParsedProgram,
    output: &CheckOutput,
) -> Result<LanguageProjectSnapshot, String> {
    project_checked_language_syntax(
        revision,
        units,
        CheckedLanguageSyntax::Assembled(program),
        output,
    )
}

pub fn project_checked_unit_native_language(
    revision: u64,
    units: &[SourceUnit],
    program: &ProjectSyntaxSnapshot,
    output: &CheckOutput,
) -> Result<LanguageProjectSnapshot, String> {
    project_checked_language_syntax(
        revision,
        units,
        CheckedLanguageSyntax::UnitNative(program),
        output,
    )
}

#[derive(Clone, Copy)]
enum CheckedLanguageSyntax<'a> {
    Assembled(&'a ParsedProgram),
    UnitNative(&'a ProjectSyntaxSnapshot),
}

impl<'a> CheckedLanguageSyntax<'a> {
    fn path(self) -> &'a str {
        match self {
            Self::Assembled(program) => &program.path,
            Self::UnitNative(program) => program.path(),
        }
    }

    fn digest(self) -> SourceBundleDigestV1 {
        match self {
            Self::Assembled(program) => program.source_bundle_digest_v1,
            Self::UnitNative(program) => program.source_bundle_digest_v1(),
        }
    }
}

fn project_checked_language_syntax(
    revision: u64,
    units: &[SourceUnit],
    program: CheckedLanguageSyntax<'_>,
    output: &CheckOutput,
) -> Result<LanguageProjectSnapshot, String> {
    let bundle = canonical_source_bundle(program.path(), units)?;
    if bundle.digest() != program.digest() {
        return Err(format!(
            "language projection source digest {} differs from parsed digest {}",
            bundle.digest(),
            program.digest()
        ));
    }
    let canonical_to_dev_file = canonical_to_dev_file_mapping(program, units)?;
    let mut files = units
        .iter()
        .map(|unit| {
            Ok(LanguageFileIndex {
                path: boon_contract::normalize_source_path(&unit.path)
                    .map_err(|error| error.to_string())?,
                inspector_hints: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut semantics = Vec::new();
    let mut diagnostics = Vec::new();

    if let Some(checked) = output.checked_program_fields() {
        semantics = semantic_items(program, checked);
        for item in &mut semantics {
            remap_source_location(&mut item.location, &canonical_to_dev_file);
        }
    }
    // Diagnostics requests intentionally omit this global presentation
    // sidecar. Materialize it here from the already-checked tables, without a
    // second parse or type solve, only when an editor projection is requested.
    let type_hints = match program {
        CheckedLanguageSyntax::Assembled(program) => {
            boon_typecheck::project_type_hints(program, output)
        }
        CheckedLanguageSyntax::UnitNative(program) => {
            boon_typecheck::project_type_hints_for_project(program, output)
        }
    };
    for hint in &type_hints.entries {
        let Some(mut location) = source_location_for_span(
            program,
            CheckedSpan {
                line: hint.line,
                start: hint.start,
                end: hint.end,
            },
        ) else {
            continue;
        };
        remap_source_location(&mut location, &canonical_to_dev_file);
        let Some(file) = files.get_mut(location.file_index) else {
            continue;
        };
        file.inspector_hints.push(InspectorHint {
            line: location.line,
            start: location.start,
            end: location.end,
            anchor_column: hint.anchor_column,
            category: hint.category.clone(),
            compact_label: hint.compact_label.clone(),
            detail_label: hint.detail_label.clone(),
            display_tree: hint.display_tree.clone(),
        });
    }
    diagnostics.extend(output.report.diagnostics.iter().filter_map(|diagnostic| {
        source_location_for_span(
            program,
            CheckedSpan {
                line: diagnostic.line,
                start: diagnostic.start,
                end: diagnostic.end,
            },
        )
        .map(|mut location| {
            remap_source_location(&mut location, &canonical_to_dev_file);
            SemanticDiagnostic {
                severity: diagnostic.severity,
                location,
                message: diagnostic.message.clone(),
            }
        })
    }));

    Ok(LanguageProjectSnapshot {
        revision,
        entrypoint: bundle.entrypoint().to_owned(),
        source_bundle_digest_v1: bundle.digest(),
        files,
        semantics,
        diagnostics,
        inline_out_hints: false,
    })
}

/// Produces lexical editor data and the exact parser failure without invoking a
/// parser or typechecker. The active file is lexed later by
/// [`LanguageProjectSnapshot::materialize_file`].
pub fn project_parse_error_language(
    revision: u64,
    entrypoint: &str,
    units: &[SourceUnit],
    error: &ParseError,
) -> Result<LanguageProjectSnapshot, String> {
    let bundle = canonical_source_bundle(entrypoint, units)?;
    let files = units
        .iter()
        .map(|unit| {
            Ok(LanguageFileIndex {
                path: boon_contract::normalize_source_path(&unit.path)
                    .map_err(|error| error.to_string())?,
                inspector_hints: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let fallback_file = units
        .iter()
        .position(|unit| {
            boon_contract::normalize_source_path(&unit.path)
                .is_ok_and(|path| path == bundle.entrypoint())
        })
        .unwrap_or(0);
    Ok(LanguageProjectSnapshot {
        revision,
        entrypoint: bundle.entrypoint().to_owned(),
        source_bundle_digest_v1: bundle.digest(),
        files,
        semantics: Vec::new(),
        diagnostics: vec![parse_diagnostic(units, fallback_file, error)],
        inline_out_hints: false,
    })
}

fn canonical_source_bundle<'a>(
    entrypoint: &str,
    units: &'a [SourceUnit],
) -> Result<CanonicalSourceBundleV1<'a>, String> {
    CanonicalSourceBundleV1::new(
        entrypoint,
        units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .map_err(|error| format!("invalid language source bundle: {error}"))
}

fn canonical_to_dev_file_mapping(
    program: CheckedLanguageSyntax<'_>,
    units: &[SourceUnit],
) -> Result<Vec<usize>, String> {
    let canonical_paths = match program {
        CheckedLanguageSyntax::Assembled(program) => program
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        CheckedLanguageSyntax::UnitNative(program) => program
            .source_layouts()
            .iter()
            .map(|layout| layout.path.as_str())
            .collect::<Vec<_>>(),
    };
    if canonical_paths.len() != units.len() {
        return Err(format!(
            "parsed language file count {} differs from dev source count {}",
            canonical_paths.len(),
            units.len()
        ));
    }
    let mut dev_by_path = BTreeMap::new();
    for (dev_index, unit) in units.iter().enumerate() {
        let path =
            boon_contract::normalize_source_path(&unit.path).map_err(|error| error.to_string())?;
        if dev_by_path.insert(path.clone(), dev_index).is_some() {
            return Err(format!("duplicate normalized dev source path `{path}`"));
        }
    }
    canonical_paths
        .into_iter()
        .map(|path| {
            dev_by_path.get(path).copied().ok_or_else(|| {
                format!("parsed source path `{path}` is absent from the dev source snapshot")
            })
        })
        .collect()
}

fn remap_source_location(location: &mut SourceLocation, canonical_to_dev_file: &[usize]) {
    if let Some(file_index) = canonical_to_dev_file.get(location.file_index).copied() {
        location.file_index = file_index;
    }
}

fn semantic_items(
    program: CheckedLanguageSyntax<'_>,
    checked: &CheckedProgramFields,
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
            let span = refine_semantic_span_for_syntax(program, occurrence.span, kind, &name);
            let location = source_location_for_span(program, span)?;
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
    checked: &CheckedProgramFields,
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
        SemanticKind::Call => {
            let pass_detail = checked
                .calls
                .iter()
                .find(|call| call.callable == target && call.span == span)
                .and_then(|call| match call.context_binding {
                    CheckedContextBinding::Explicit { .. } => Some(" with explicit PASS"),
                    CheckedContextBinding::Inherited { .. } => Some(" with inherited PASS"),
                    CheckedContextBinding::None => None,
                })
                .unwrap_or_default();
            (
                format!("Call {name}"),
                format!("Calls {name}{pass_detail}; F12 opens its declaration"),
            )
        }
        SemanticKind::Pass => {
            let explicitly_bound = checked.calls.iter().any(|call| {
                call.callable == target
                    && matches!(
                        call.context_binding,
                        CheckedContextBinding::Explicit {
                            span: explicit_span,
                            ..
                        } if explicit_span.start == span.start && explicit_span.end == span.end
                    )
            });
            (
                format!("PASS context for {name}"),
                if explicitly_bound {
                    format!(
                        "Supplies explicit lexical context to {name}, replacing inherited context when present; F12 opens its declaration"
                    )
                } else {
                    format!("Supplies lexical context to {name}; F12 opens its declaration")
                },
            )
        }
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
    program: CheckedLanguageSyntax<'_>,
    span: CheckedSpan,
) -> Option<SourceLocation> {
    match program {
        CheckedLanguageSyntax::Assembled(program) => {
            source_location_for_assembled_span(program, span)
        }
        CheckedLanguageSyntax::UnitNative(program) => {
            let layouts = program.source_layouts();
            let by_line = layouts.iter().enumerate().find(|(_, layout)| {
                span.line >= layout.start_line
                    && span.line < layout.start_line.saturating_add(layout.line_count)
            });
            let (file_index, layout) = by_line.or_else(|| {
                layouts.iter().enumerate().find(|(_, layout)| {
                    let file_end = layout.start_byte.saturating_add(layout.source_len);
                    (layout.start_byte <= span.start && span.start < file_end)
                        || (layout.source_len == 0 && span.start == layout.start_byte)
                })
            })?;
            let unit = program.units().get(file_index)?;
            let local_start = span
                .start
                .saturating_sub(layout.start_byte)
                .min(unit.source.len());
            let line = if span.line >= layout.start_line {
                span.line.saturating_sub(layout.start_line)
            } else {
                unit.source[..local_start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
            };
            Some(SourceLocation {
                file_index,
                path: layout.path.clone(),
                line,
                start: local_start,
                end: span
                    .end
                    .saturating_sub(layout.start_byte)
                    .min(unit.source.len()),
            })
        }
    }
}

fn source_location_for_assembled_span(
    program: &ParsedProgram,
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

fn refine_semantic_span_for_syntax(
    program: CheckedLanguageSyntax<'_>,
    span: CheckedSpan,
    kind: SemanticKind,
    name: &str,
) -> CheckedSpan {
    match program {
        CheckedLanguageSyntax::Assembled(program) => {
            refine_semantic_span(span, &program.ast.tokens, kind, name)
        }
        CheckedLanguageSyntax::UnitNative(program) => {
            let Some((unit_index, layout)) =
                program
                    .source_layouts()
                    .iter()
                    .enumerate()
                    .find(|(_, layout)| {
                        let end = layout.start_byte.saturating_add(layout.source_len);
                        (layout.start_byte <= span.start && span.start < end)
                            || (layout.source_len == 0 && span.start == layout.start_byte)
                    })
            else {
                return span;
            };
            let Some(unit) = program.units().get(unit_index) else {
                return span;
            };
            let local = CheckedSpan {
                line: span
                    .line
                    .saturating_sub(layout.start_line)
                    .saturating_add(1),
                start: span.start.saturating_sub(layout.start_byte),
                end: span.end.saturating_sub(layout.start_byte),
            };
            let refined = refine_semantic_span(local, &unit.ast.tokens, kind, name);
            CheckedSpan {
                line: layout
                    .start_line
                    .saturating_add(refined.line.saturating_sub(1)),
                start: layout.start_byte.saturating_add(refined.start),
                end: layout.start_byte.saturating_add(refined.end),
            }
        }
    }
}

fn refine_semantic_span(
    mut span: CheckedSpan,
    tokens: &[AstToken],
    kind: SemanticKind,
    name: &str,
) -> CheckedSpan {
    let expected = match kind {
        SemanticKind::Pass => "PASS",
        SemanticKind::Declaration
        | SemanticKind::Call
        | SemanticKind::FreshOut
        | SemanticKind::ForwardOut => name,
        SemanticKind::Reference => return span,
    };
    let short_expected = expected.rsplit('/').next().unwrap_or(expected);
    // The checked occurrence owns identity and meaning. Parser-owned tokens
    // only narrow its compiler-provided global range to the painted token; no
    // project-wide editor re-lex is necessary.
    if let Some(token) = tokens.iter().find(|token| {
        token.start >= span.start
            && token.end <= span.end
            && (token.lexeme == expected || token.lexeme == short_expected)
    }) {
        span.start = token.start;
        span.end = token.end;
        span.line = token.line;
    }
    span
}

fn parse_diagnostic(
    units: &[SourceUnit],
    active_file: usize,
    error: &ParseError,
) -> SemanticDiagnostic {
    let file_index = units
        .iter()
        .position(|unit| {
            boon_contract::normalize_source_path(&unit.path).is_ok_and(|path| path == error.path)
        })
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
        message: error.message.clone(),
    }
}

fn style_type_hint(hint: &InspectorHint) -> StyleEditorTypeHint {
    StyleEditorTypeHint {
        line: hint.line,
        start: hint.start,
        end: hint.end,
        anchor_column: hint.anchor_column,
        category: hint.category.clone(),
        compact_label: hint.compact_label.clone(),
        detail_label: hint.detail_label.clone(),
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

    fn project(revision: u64, entrypoint: &str, units: &[SourceUnit]) -> LanguageProjectSnapshot {
        let parsed = boon_parser::parse_project(
            entrypoint,
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        project_checked_language(revision, units, &parsed, &output).unwrap()
    }

    fn snapshot(source: &str) -> LanguageSnapshot {
        let units = vec![SourceUnit {
            path: "RUN.bn".to_owned(),
            source: source.to_owned(),
        }];
        let project = project(7, "RUN.bn", &units);
        assert!(project.matches_source_units(&units));
        project.materialize_file(0, &units[0]).unwrap()
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
            fresh.label.contains("supplied by doubled"),
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
        assert!(pass.detail.contains("Supplies explicit lexical context"));
        assert_eq!(
            snapshot
                .definition_at(pass.location.start)
                .map(|item| item.name.as_str()),
            Some("render")
        );
        assert!(snapshot.semantics.iter().any(|item| {
            item.kind == SemanticKind::Call
                && item.name == "render"
                && item.detail.contains("with explicit PASS")
        }));
    }

    #[test]
    fn checked_call_hover_reports_implicit_pass_inheritance() {
        let snapshot = snapshot(
            r#"FUNCTION wrapper() {
    leaf()
}

FUNCTION leaf() {
    PASSED.store.count
}

result: wrapper(PASS: [store: [count: 1]])
"#,
        );
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:#?}",
            snapshot.diagnostics
        );
        let inherited = snapshot
            .semantics
            .iter()
            .find(|item| {
                item.kind == SemanticKind::Call
                    && item.name == "leaf"
                    && item.detail.contains("with inherited PASS")
            })
            .expect("nested requiring call exposes inherited PASS in hover");
        assert!(!inherited.detail.contains("explicit PASS"));
    }

    #[test]
    fn checked_declaration_identity_navigates_across_project_files() {
        let units = vec![
            SourceUnit {
                path: "RUN.bn".to_owned(),
                source: "result: Math/double(value: 21)\n".to_owned(),
            },
            SourceUnit {
                path: "Math.bn".to_owned(),
                source: "FUNCTION double(value) {\n    value * 2\n}\n".to_owned(),
            },
        ];
        let project = project(9, "RUN.bn", &units);
        let snapshot = project.materialize_file(0, &units[0]).unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:#?}",
            snapshot.diagnostics
        );
        let call = snapshot
            .semantics
            .iter()
            .find(|item| item.kind == SemanticKind::Call && item.location.file_index == 0)
            .expect("cross-file call occurrence");
        let definition = snapshot
            .definition_at(call.location.start)
            .expect("cross-file source definition");
        assert_eq!(definition.location.file_index, 1);
        assert_eq!(definition.location.path, "Math.bn");
        assert_eq!(definition.name, "Math/double");
    }

    #[test]
    fn compact_project_snapshot_preserves_dev_order_and_materializes_each_file() {
        // Canonical source order is Math.bn, RUN.bn, deliberately unlike the
        // dev-owned tab order below.
        let units = vec![
            SourceUnit {
                path: "RUN.bn".to_owned(),
                source: "result: Math/double(value: 21)\n".to_owned(),
            },
            SourceUnit {
                path: "Math.bn".to_owned(),
                source: "FUNCTION double(value) {\n    value * 2\n}\n".to_owned(),
            },
        ];
        let project = project(11, "RUN.bn", &units);
        assert_eq!(
            project
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["RUN.bn", "Math.bn"]
        );
        assert!(project.matches_source_units(&units));
        assert!(
            project
                .files
                .iter()
                .any(|file| !file.inspector_hints.is_empty()),
            "report-owned checked projection should retain editor type hints"
        );

        let run = project.materialize_file(0, &units[0]).unwrap();
        let math = project.materialize_file(1, &units[1]).unwrap();
        assert_eq!(run.path, "RUN.bn");
        assert_eq!(math.path, "Math.bn");
        assert!(
            run.semantics
                .iter()
                .any(|item| { item.kind == SemanticKind::Call && item.location.file_index == 0 })
        );
        assert!(math.semantics.iter().any(|item| {
            item.kind == SemanticKind::Declaration && item.location.file_index == 1
        }));
        for snapshot in [&run, &math] {
            for hint in &snapshot.inspector_hints {
                let painted_hint = snapshot.lines[hint.line]
                    .type_hints
                    .iter()
                    .find(|candidate| {
                        candidate.start == hint.start
                            && candidate.end == hint.end
                            && candidate.anchor_column == hint.anchor_column
                    });
                assert!(
                    painted_hint.is_some(),
                    "materialized hint missing: {hint:?}"
                );
            }
        }
    }

    #[test]
    fn unit_native_language_projection_matches_assembled_project_exactly() {
        let units = vec![
            SourceUnit {
                path: "RUN.bn".to_owned(),
                source: "result: Math/double(value: 21)\n".to_owned(),
            },
            SourceUnit {
                path: "Math.bn".to_owned(),
                source: "FUNCTION double(value) {\n    value * 2\n}\n".to_owned(),
            },
        ];
        let files = units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone()))
            .collect::<Vec<_>>();
        let assembled = boon_parser::parse_project("RUN.bn", files.clone()).unwrap();
        let unit_native = boon_parser::parse_project_syntax("RUN.bn", files).unwrap();
        let assembled_output = boon_typecheck::check_program(&assembled);
        let unit_native_output =
            boon_typecheck::check_project_program_profiled_with_external_types(
                &unit_native,
                &boon_checked::ExternalTypeEnvironment::default(),
            )
            .0;

        let assembled_projection =
            project_checked_language(23, &units, &assembled, &assembled_output).unwrap();
        let unit_native_projection =
            project_checked_unit_native_language(23, &units, &unit_native, &unit_native_output)
                .unwrap();
        assert_eq!(unit_native_projection, assembled_projection);
    }

    #[test]
    fn source_digest_rejects_changed_bytes_and_dev_order() {
        let units = vec![
            SourceUnit {
                path: "RUN.bn".to_owned(),
                source: "result: value\n".to_owned(),
            },
            SourceUnit {
                path: "Value.bn".to_owned(),
                source: "value: 1\n".to_owned(),
            },
        ];
        let project = project(12, "RUN.bn", &units);
        assert!(project.matches_source_units(&units));

        let mut changed = units.clone();
        changed[1].source = "value: 2\n".to_owned();
        assert!(!project.matches_source_units(&changed));

        let parsed = boon_parser::parse_project(
            "RUN.bn",
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
        .unwrap();
        let output = boon_typecheck::check_program(&parsed);
        let error = project_checked_language(12, &changed, &parsed, &output).unwrap_err();
        assert!(error.contains("source digest"), "{error}");

        let mut reordered = units.clone();
        reordered.swap(0, 1);
        assert!(!project.matches_source_units(&reordered));
        assert!(project.materialize_file(0, &units[1]).is_none());
    }

    #[test]
    fn parse_error_projection_materializes_lexical_lines_without_rechecking() {
        let units = vec![SourceUnit {
            path: "RUN.bn".to_owned(),
            source: "value: [\n".to_owned(),
        }];
        let error = boon_parser::parse_project(
            "RUN.bn",
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
        .unwrap_err();
        let project = project_parse_error_language(13, "RUN.bn", &units, &error).unwrap();
        assert!(project.matches_source_units(&units));
        assert!(project.semantics.is_empty());
        assert!(project.files[0].inspector_hints.is_empty());

        let snapshot = project.materialize_file(0, &units[0]).unwrap();
        assert_eq!(snapshot.revision, 13);
        assert_eq!(snapshot.path, "RUN.bn");
        assert!(!snapshot.lines.is_empty());
        assert!(snapshot.lines.iter().any(|line| !line.spans.is_empty()));
        assert_eq!(snapshot.diagnostics.len(), 1);
        assert_eq!(snapshot.diagnostics[0].location.path, "RUN.bn");
        assert_eq!(snapshot.diagnostics[0].message, error.message);
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
