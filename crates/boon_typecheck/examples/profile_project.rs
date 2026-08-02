//! Low-overhead development profiler for the parser/typechecker core.
//!
//! Acceptance evidence remains owned by `cargo xtask verify-compiler-performance`.
//! This executable deliberately avoids linking the semantic/backend stack so
//! an inference-only optimization does not require a multi-minute release
//! rebuild before its first measurement.

use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Serialize)]
struct FrontendProfile<'a> {
    parse_ms: f64,
    typecheck_ms: f64,
    expressions: usize,
    checked_expressions: usize,
    diagnostics: usize,
    errors: bool,
    program_available: bool,
    parse_work: boon_parser::ParseWorkCounters,
    typecheck_work: &'a boon_typecheck::TypeCheckWorkCounters,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: profile_project <root> <entrypoint> <unit> [<unit> ...]")?,
    );
    let entrypoint = args
        .next()
        .ok_or("usage: profile_project <root> <entrypoint> <unit> [<unit> ...]")?;
    let units = args
        .map(|path| {
            let source = fs::read_to_string(root.join(&path))?;
            Ok((path, source))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    if units.is_empty() {
        return Err("profile_project requires at least one source unit".into());
    }

    let parse_started = Instant::now();
    let (parsed, parse_profile) = boon_parser::parse_project_profiled(entrypoint, units);
    let parsed = parsed?;
    let parse_ms = parse_started.elapsed().as_secs_f64() * 1_000.0;
    let typecheck_started = Instant::now();
    let external_types =
        boon_typecheck::ExternalTypeEnvironment::empty(boon_document_model::ProgramRole::Client);
    let (output, typecheck_profile) =
        boon_typecheck::check_diagnostics_program_profiled_with_external_types(
            &parsed,
            &external_types,
        );
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    serde_json::to_writer(
        std::io::stdout().lock(),
        &FrontendProfile {
            parse_ms,
            typecheck_ms,
            expressions: parsed.expressions.len(),
            checked_expressions: output.report.checked_expression_count,
            diagnostics: output.report.diagnostics.len(),
            errors: output.report.has_errors(),
            program_available: output.program.is_some(),
            parse_work: parse_profile.work_counters,
            typecheck_work: &typecheck_profile.work_counters,
        },
    )?;
    println!();
    Ok(())
}
