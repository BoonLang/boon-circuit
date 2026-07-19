use boon_example_manifest::{ExampleEntry, ExampleManifest};
use boon_ir::{ErasedProgram, verify_hidden_identity, verify_static_schedule};
use boon_parser::{AstExpr, AstExprKind, AstStatement, ParsedProgram, parse_project, parse_source};
pub use boon_plan::{
    ApplicationIdentity, MachinePlan, MigrationPredecessorBinding, PlanError, ProgramRole,
    TargetProfile,
};
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

mod distributed_compiler;
mod document_plan_backend;
mod machine_plan_backend;

pub use distributed_compiler::{
    CompiledDistributedMachinePlans, DistributedCompilerProgram,
    compile_distributed_runtime_source_programs,
};

pub type CompilerResult<T> = Result<T, Box<dyn std::error::Error>>;

pub const COMPILER_ID: &str = concat!("boon-compiler/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDiagnostic {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub message: String,
}

/// Produces structured parser/type diagnostics for a failed runtime compile.
/// Callers use this only on the error path, so successful compilation does not
/// repeat parsing or type checking.
pub fn diagnose_runtime_source_units(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> Vec<CompilerDiagnostic> {
    let parsed = if let [unit] = units {
        parse_source(unit.path.clone(), unit.source.clone())
    } else {
        parse_project(
            source_label.to_owned(),
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
    };
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            return vec![CompilerDiagnostic {
                path: error.path,
                line: error.line,
                column: error.column,
                start: None,
                end: None,
                message: error.message,
            }];
        }
    };
    boon_typecheck::check_runtime_profiled(&parsed)
        .0
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == boon_typecheck::DiagnosticSeverity::Error)
        .map(|diagnostic| {
            let (path, line) = source_file_location(&parsed, diagnostic.line);
            CompilerDiagnostic {
                path,
                line: Some(line),
                column: grapheme_column(&parsed.source, diagnostic.line, diagnostic.start),
                start: Some(diagnostic.start),
                end: Some(diagnostic.end),
                message: diagnostic.message,
            }
        })
        .collect()
}

fn source_file_location(parsed: &ParsedProgram, global_line: usize) -> (String, usize) {
    parsed
        .files
        .iter()
        .filter(|file| file.start_line <= global_line)
        .max_by_key(|file| file.start_line)
        .map_or_else(
            || (parsed.path.clone(), global_line),
            |file| {
                (
                    file.path.clone(),
                    global_line
                        .saturating_sub(file.start_line)
                        .saturating_add(1),
                )
            },
        )
}

fn grapheme_column(source: &str, line: usize, byte: usize) -> Option<usize> {
    let line_start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    (byte >= line_start && byte <= source.len())
        .then(|| source.get(line_start..byte.min(source.len())))
        .flatten()
        .map(|prefix| prefix.graphemes(true).count().saturating_add(1))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompileProfile {
    pub source_unit_count: usize,
    pub expression_count: usize,
    pub graph_node_count: usize,
    pub parse_ms: f64,
    pub lower_ms: f64,
    pub verify_ms: f64,
    pub compile_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Debug)]
pub struct CompiledMachinePlanFromSource {
    pub parsed: ParsedProgram,
    pub ir: ErasedProgram,
    pub plan: MachinePlan,
    pub profile: CompileProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompilerDerivedTextExpression {
    EnterKeyPayloadTextTrimNonEmpty,
    EnterKeyRootTextTrimNonEmpty { path: String },
    SourceRootText { path: String },
    Other,
}

fn compiler_statement_ast_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<AstExpr> {
    let mut ids = BTreeSet::new();
    collect_statement_expr_ids(statement, expressions, &mut ids);
    expressions
        .iter()
        .filter(|expr| {
            ids.contains(&expr.id) || (expr.start >= statement.start && expr.end <= statement.end)
        })
        .cloned()
        .collect()
}

fn collect_statement_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    ids: &mut BTreeSet<usize>,
) {
    if let Some(expr) = statement.expr {
        collect_expr_ids(expr, expressions, ids);
    }
    for child in &statement.children {
        collect_statement_expr_ids(child, expressions, ids);
    }
}

fn collect_expr_ids(id: usize, expressions: &[AstExpr], ids: &mut BTreeSet<usize>) {
    if !ids.insert(id) {
        return;
    }
    let Some(expr) = expressions.iter().find(|expr| expr.id == id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                collect_expr_ids(arg.value, expressions, ids);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            collect_expr_ids(*input, expressions, ids);
            for arg in args {
                collect_expr_ids(arg.value, expressions, ids);
            }
        }
        AstExprKind::Draining { input } => {
            collect_expr_ids(*input, expressions, ids);
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial, .. } => {
            collect_expr_ids(*initial, expressions, ids);
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                collect_expr_ids(binding.value, expressions, ids);
            }
            if let Some(result) = result {
                collect_expr_ids(*result, expressions, ids);
            }
        }
        AstExprKind::Then { input, output } => {
            collect_expr_ids(*input, expressions, ids);
            if let Some(output) = output {
                collect_expr_ids(*output, expressions, ids);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            collect_expr_ids(*left, expressions, ids);
            collect_expr_ids(*right, expressions, ids);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_expr_ids(*output, expressions, ids);
            }
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_expr_ids(field.value, expressions, ids);
            }
        }
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_expr_ids(*item, expressions, ids);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn compiler_source_event_transform_text_expression(
    value: &boon_ir::DerivedValue,
    source: &str,
    expressions: &[AstExpr],
    _functions: &[boon_ir::FunctionDefinition],
) -> CompilerDerivedTextExpression {
    let expressions = compiler_statement_ast_exprs(&value.statement, expressions);
    if let Some(path) = text_trim_input_path(&expressions) {
        if source_payload_suffix(&path, source).as_deref() == Some("text") {
            return CompilerDerivedTextExpression::EnterKeyPayloadTextTrimNonEmpty;
        }
        return CompilerDerivedTextExpression::EnterKeyRootTextTrimNonEmpty {
            path: canonical_sibling_path(&value.path, &path),
        };
    }
    for expr in &expressions {
        let AstExprKind::Then {
            input,
            output: Some(output),
        } = expr.kind
        else {
            continue;
        };
        if !expr_tree_mentions_source(&expressions, input, source) {
            continue;
        }
        if let Some(path) = expr_path(&expressions, output) {
            return CompilerDerivedTextExpression::SourceRootText {
                path: canonical_sibling_path(&value.path, &path),
            };
        }
    }
    CompilerDerivedTextExpression::Other
}

fn text_trim_input_path(expressions: &[AstExpr]) -> Option<String> {
    expressions.iter().find_map(|expr| match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Text/trim" => expr_path(expressions, *input),
        AstExprKind::Call { function, args, .. } if function == "Text/trim" => args
            .iter()
            .find(|arg| arg.is_bare_binding())
            .and_then(|arg| expr_path(expressions, arg.value)),
        _ => None,
    })
}

fn expr_tree_mentions_source(expressions: &[AstExpr], id: usize, source: &str) -> bool {
    let mut ids = BTreeSet::new();
    collect_expr_ids(id, expressions, &mut ids);
    expressions.iter().any(|expr| {
        ids.contains(&expr.id)
            && expr_path(expressions, expr.id)
                .is_some_and(|path| source_suffix(&path, source).is_some())
    })
}

fn expr_path(expressions: &[AstExpr], id: usize) -> Option<String> {
    match &expressions.iter().find(|expr| expr.id == id)?.kind {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) if !parts.is_empty() => {
            Some(boon_parser::canonical_value_path(parts))
        }
        _ => None,
    }
}

fn source_payload_suffix(path: &str, source: &str) -> Option<String> {
    let suffix = source_suffix(path, source)?;
    Some(match suffix {
        "change.text" | "event.change.text" | "events.change.text" => "text".to_owned(),
        "key_down.key" | "event.key_down.key" | "events.key_down.key" => "key".to_owned(),
        other => other.rsplit('.').next().unwrap_or(other).to_owned(),
    })
}

fn source_suffix<'a>(path: &'a str, source: &str) -> Option<&'a str> {
    let mut variants = vec![source.to_owned()];
    if let Some((_, suffix)) = source.split_once('.') {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    variants.into_iter().find_map(|variant| {
        if path == variant {
            return Some("");
        }
        path.strip_prefix(&variant)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .or_else(|| {
                let marker = format!(".{variant}.");
                path.find(&marker)
                    .map(|start| &path[start + marker.len()..])
            })
    })
}

fn canonical_sibling_path(owner: &str, path: &str) -> String {
    if path.contains('.') {
        path.to_owned()
    } else {
        owner
            .rsplit_once('.')
            .map(|(parent, _)| format!("{parent}.{path}"))
            .unwrap_or_else(|| path.to_owned())
    }
}

pub fn compile_typed_program(
    program: &ErasedProgram,
    target_profile: TargetProfile,
) -> Result<MachinePlan, PlanError> {
    compile_typed_program_with_identity(
        program,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

/// Compiles with a host-supplied durable application identity. Callers that
/// may persist state should use this API instead of the compatibility boundary.
pub fn compile_typed_program_with_identity(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> Result<MachinePlan, PlanError> {
    compile_typed_program_with_persistence_identity(
        program,
        target_profile,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
    )
}

pub fn compile_typed_program_with_persistence_identity(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
) -> Result<MachinePlan, PlanError> {
    compile_typed_program_with_persistence_catalog(
        program,
        target_profile,
        application_identity,
        schema_version,
        &[],
    )
}

pub fn compile_typed_program_with_persistence_catalog(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<MachinePlan, PlanError> {
    compile_typed_program_for_role_with_persistence_catalog(
        program,
        target_profile,
        ProgramRole::Client,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_typed_program_for_role_with_persistence_catalog(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<MachinePlan, PlanError> {
    machine_plan_backend::compile_typed_program(
        program,
        target_profile,
        program_role,
        &application_identity,
        schema_version,
        migration_predecessors,
    )
}

/// Uses `ApplicationIdentity::compiler_default()` because this compatibility
/// boundary has no host application identity. Persistent hosts must call the
/// identity-aware variant.
pub fn compile_source_path_to_machine_plan(
    source_path: &Path,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_path_to_machine_plan_with_identity(
        source_path,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_source_path_to_machine_plan_for_role(
    source_path: &Path,
    target_profile: TargetProfile,
    program_role: ProgramRole,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_path_to_machine_plan_for_role_with_identity(
        source_path,
        target_profile,
        program_role,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_source_path_to_machine_plan_with_identity(
    source_path: &Path,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_path_to_machine_plan_for_role_with_identity(
        source_path,
        target_profile,
        ProgramRole::Client,
        application_identity,
    )
}

pub fn compile_source_path_to_machine_plan_for_role_with_identity(
    source_path: &Path,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_path_or_manifest_project(source_path)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_ms,
        total_started,
        target_profile,
        program_role,
        LoweringMode::Full,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

pub fn compile_source_text_to_machine_plan(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_text_to_machine_plan_with_identity(
        source_label,
        source_text,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_source_text_to_machine_plan_for_role(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_text_to_machine_plan_for_role_with_identity(
        source_label,
        source_text,
        target_profile,
        program_role,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_source_text_to_machine_plan_for_role_with_identity(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_ms,
        total_started,
        target_profile,
        program_role,
        LoweringMode::Full,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

pub fn compile_source_text_to_machine_plan_with_identity(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_text_to_machine_plan_for_role_with_identity(
        source_label,
        source_text,
        target_profile,
        ProgramRole::Client,
        application_identity,
    )
}

pub fn compile_runtime_source_text_to_machine_plan(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_with_identity(
        source_label,
        source_text,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_runtime_source_text_to_machine_plan_with_identity(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_with_persistence_identity(
        source_label,
        source_text,
        target_profile,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
    )
}

pub fn compile_runtime_source_text_to_machine_plan_for_role_with_identity(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        source_text,
        target_profile,
        program_role,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

pub fn compile_runtime_source_text_to_machine_plan_with_persistence_identity(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_with_persistence_catalog(
        source_label,
        source_text,
        target_profile,
        application_identity,
        schema_version,
        &[],
    )
}

pub fn compile_runtime_source_text_to_machine_plan_with_persistence_catalog(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        source_text,
        target_profile,
        ProgramRole::Client,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_runtime_source_text_to_machine_plan_for_role_with_persistence_catalog(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_ms,
        total_started,
        target_profile,
        program_role,
        LoweringMode::Runtime,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_source_units_to_machine_plan(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_units_to_machine_plan_with_identity(
        source_label,
        units,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_source_units_to_machine_plan_with_identity(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_units(source_label, units)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_ms,
        total_started,
        target_profile,
        ProgramRole::Client,
        LoweringMode::Full,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

pub fn compile_runtime_source_units_to_machine_plan(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_units_to_machine_plan_with_identity(
        source_label,
        units,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_runtime_source_units_to_machine_plan_with_identity(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_units_to_machine_plan_for_role_with_identity(
        source_label,
        units,
        target_profile,
        ProgramRole::Client,
        application_identity,
    )
}

pub fn compile_runtime_source_units_to_machine_plan_for_role_with_identity(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_units_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        units,
        target_profile,
        program_role,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

pub fn compile_runtime_source_units_to_machine_plan_for_role_with_persistence_catalog(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_units(source_label, units)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_machine_plan(
        parsed,
        parse_ms,
        total_started,
        target_profile,
        program_role,
        LoweringMode::Runtime,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_runtime_source_units_to_machine_plan_with_persistence_identity(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
        source_label,
        units,
        target_profile,
        application_identity,
        schema_version,
        &[],
    )
}

pub fn compile_runtime_source_units_to_machine_plan_with_persistence_catalog(
    source_label: &str,
    units: &[CompilerSourceUnit],
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_units_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        units,
        target_profile,
        ProgramRole::Client,
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

pub fn compile_parsed_program_to_machine_plan(
    parsed: ParsedProgram,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_parsed_program_to_machine_plan_with_identity(
        parsed,
        target_profile,
        ApplicationIdentity::compiler_default(),
    )
}

pub fn compile_parsed_program_to_machine_plan_with_identity(
    parsed: ParsedProgram,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_parsed_to_machine_plan(
        parsed,
        0.0,
        Instant::now(),
        target_profile,
        ProgramRole::Client,
        LoweringMode::Full,
        application_identity,
        boon_plan::DEFAULT_PERSISTENCE_SCHEMA_VERSION,
        &[],
    )
}

#[derive(Clone, Copy)]
enum LoweringMode {
    Full,
    Runtime,
}

#[allow(clippy::too_many_arguments)]
fn compile_parsed_to_machine_plan(
    parsed: ParsedProgram,
    parse_ms: f64,
    total_started: Instant,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    lowering_mode: LoweringMode,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let lower_started = Instant::now();
    let requires_recursive_migration_types = parsed.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            AstExprKind::Drain { .. } | AstExprKind::Draining { .. }
        )
    });
    let external_types = boon_typecheck::ExternalTypeEnvironment::empty(program_role);
    let ir = match lowering_mode {
        LoweringMode::Full => boon_ir::lower_with_external_types(&parsed, &external_types),
        LoweringMode::Runtime if requires_recursive_migration_types => {
            boon_ir::lower_with_external_types(&parsed, &external_types)
        }
        LoweringMode::Runtime => {
            boon_ir::lower_runtime_with_external_types(&parsed, &external_types)
        }
    }?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    let compile_started = Instant::now();
    let plan = compile_typed_program_for_role_with_persistence_catalog(
        &ir,
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
    )?;
    let compile_ms = elapsed_ms(compile_started);
    let profile = CompileProfile {
        source_unit_count: parsed.files.len(),
        expression_count: ir.expression_count,
        graph_node_count: ir.graph_node_count,
        parse_ms,
        lower_ms,
        verify_ms,
        compile_ms,
        total_ms: elapsed_ms(total_started),
    };
    Ok(CompiledMachinePlanFromSource {
        parsed,
        ir,
        plan,
        profile,
    })
}

fn parse_source_units(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> CompilerResult<ParsedProgram> {
    Ok(if let [unit] = units {
        parse_source(unit.path.clone(), unit.source.clone())?
    } else {
        parse_project(
            source_label.to_owned(),
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )?
    })
}

pub fn compiler_source_units_for_path(path: &Path) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_units_for_files(compiler_source_files_for_path(path)?)
}

pub fn compiler_source_units_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_units_for_files(compiler_source_files_for_manifest_source(
        source,
        source_files,
    ))
}

pub fn compiler_source_files_for_path(path: &Path) -> CompilerResult<Vec<PathBuf>> {
    source_files_for_path(path)
}

pub fn compiler_source_files_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> Vec<PathBuf> {
    source_files_for_manifest_source(source, source_files)
}

pub fn compiler_source_text_for_path(path: &Path) -> CompilerResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(path))?)
}

pub fn compiler_source_text_for_manifest_source(source: &str) -> CompilerResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(source))?)
}

pub fn parse_scenario_file<T>(path: &Path) -> CompilerResult<T>
where
    T: DeserializeOwned,
{
    let text = fs::read_to_string(resolve_repo_file(path))?;
    Ok(toml::from_str(&text)?)
}

fn compiler_source_units_for_files(files: Vec<PathBuf>) -> CompilerResult<Vec<CompilerSourceUnit>> {
    files
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)?;
            Ok(CompilerSourceUnit {
                path: path.display().to_string(),
                source,
            })
        })
        .collect()
}

fn parse_source_path_or_manifest_project(source_path: &Path) -> CompilerResult<ParsedProgram> {
    let units = compiler_source_units_for_path(source_path)?;
    parse_source_units(&source_path.display().to_string(), &units)
}

fn source_files_for_path(source_path: &Path) -> CompilerResult<Vec<PathBuf>> {
    let source_path = resolve_repo_file(source_path);
    for entry in example_manifest_entries().unwrap_or_default() {
        if paths_match(&resolve_repo_file(&entry.source), &source_path) {
            return Ok(source_files_for_manifest_source(
                &entry.source,
                &entry.source_files,
            ));
        }
        for program in &entry.programs {
            if paths_match(&resolve_repo_file(&program.source), &source_path) {
                return Ok(source_files_for_manifest_source(
                    &program.source,
                    &program.source_files,
                ));
            }
        }
    }
    Ok(vec![source_path])
}

fn example_manifest_entries() -> CompilerResult<Vec<ExampleEntry>> {
    let path = resolve_repo_file("examples/manifest.toml");
    let manifest = ExampleManifest::from_path(path)?;
    Ok(manifest.example)
}

fn source_files_for_manifest_source(source: &str, source_files: &[String]) -> Vec<PathBuf> {
    let source_path = resolve_repo_file(source);
    let mut files = source_files
        .iter()
        .map(resolve_repo_file)
        .collect::<Vec<_>>();
    files.retain(|path| !paths_match(path, &source_path));
    files.push(source_path);
    files
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn resolve_repo_file(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.exists() {
        return relative.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    relative.to_path_buf()
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests;
