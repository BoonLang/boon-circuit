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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostic_messages: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    function_types: Vec<&'a boon_checked::FunctionTypeEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    expression_debug: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    call_debug: Vec<String>,
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
        boon_checked::ExternalTypeEnvironment::empty(boon_document_model::ProgramRole::Client);
    let (output, typecheck_profile) = if env::var_os("BOON_PROFILE_EAGER_BINDINGS").is_some() {
        boon_typecheck::check_program_profiled_with_external_types(&parsed, &external_types)
    } else {
        boon_typecheck::check_diagnostics_program_profiled_with_external_types(
            &parsed,
            &external_types,
        )
    };
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    serde_json::to_writer(
        std::io::stdout().lock(),
        &FrontendProfile {
            parse_ms,
            typecheck_ms,
            expressions: parsed.expressions.len(),
            checked_expressions: output.report.checked_expression_count,
            diagnostics: output.report.diagnostics.len(),
            diagnostic_messages: output
                .report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    let file = parsed
                        .files
                        .iter()
                        .rev()
                        .find(|file| file.start_line <= diagnostic.line);
                    let location = file.map_or_else(
                        || diagnostic.line.to_string(),
                        |file| {
                            format!(
                                "{}:{}",
                                file.path,
                                diagnostic.line.saturating_sub(file.start_line) + 1,
                            )
                        },
                    );
                    let expression = parsed.expressions.iter().find(|expression| {
                        expression.line == diagnostic.line
                            && expression.start == diagnostic.start
                            && expression.end == diagnostic.end
                    });
                    let expression_detail = expression
                        .and_then(|expression| {
                            output
                                .report
                                .expr_type_table
                                .entries
                                .iter()
                                .find(|entry| entry.expr_id == expression.id)
                                .map(|entry| {
                                    format!(
                                        " expr={} kind={:?} flow={:?}",
                                        expression.id, expression.kind, entry.flow_type,
                                    )
                                })
                        })
                        .unwrap_or_default();
                    format!(
                        "{}:{}-{}: {}{}",
                        location,
                        diagnostic.start,
                        diagnostic.end,
                        diagnostic.message,
                        expression_detail,
                    )
                })
                .collect(),
            function_types: env::var_os("BOON_PROFILE_TYPES")
                .is_some()
                .then(|| output.report.function_type_table.entries.iter().collect())
                .unwrap_or_default(),
            expression_debug: env::var("BOON_PROFILE_EXPRESSIONS")
                .ok()
                .into_iter()
                .flat_map(|ids| {
                    ids.split(',')
                        .filter_map(|id| id.parse::<usize>().ok())
                        .collect::<Vec<_>>()
                })
                .filter_map(|id| {
                    parsed.expressions.get(id).map(|expression| {
                        let flow = output
                            .report
                            .expr_type_table
                            .entries
                            .iter()
                            .find(|entry| entry.expr_id == id)
                            .map(|entry| format!("{:?}", entry.flow_type))
                            .unwrap_or_else(|| "<missing>".to_owned());
                        format!(
                            "{}: {:?} linked_input={:?} => {}",
                            id, expression.kind, expression.linked_input, flow,
                        )
                    })
                })
                .collect(),
            call_debug: env::var("BOON_PROFILE_CALLS")
                .ok()
                .into_iter()
                .flat_map(|ids| {
                    ids.split(',')
                        .filter_map(|id| id.parse::<u32>().ok())
                        .collect::<Vec<_>>()
                })
                .filter_map(|id| {
                    output.program.as_ref().and_then(|program| {
                        program
                            .calls
                            .iter()
                            .find(|call| call.id.0 == id)
                            .map(|call| format!("{id}: {call:#?}"))
                    })
                })
                .collect(),
            errors: output.report.has_errors(),
            program_available: output.program.is_some(),
            parse_work: parse_profile.work_counters,
            typecheck_work: &typecheck_profile.work_counters,
        },
    )?;
    println!();
    Ok(())
}
