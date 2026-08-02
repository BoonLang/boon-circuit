//! Low-overhead development profiler for the parser/typechecker core.
//!
//! Acceptance evidence remains owned by `cargo xtask verify-compiler-performance`.
//! This executable deliberately avoids linking the semantic/backend stack so
//! an inference-only optimization does not require a multi-minute release
//! rebuild before its first measurement.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

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
    let parsed = boon_parser::parse_project(entrypoint, units)?;
    let parse_ms = parse_started.elapsed().as_secs_f64() * 1_000.0;
    let typecheck_started = Instant::now();
    let output = boon_typecheck::check_diagnostics_program_profiled(&parsed).0;
    let typecheck_ms = typecheck_started.elapsed().as_secs_f64() * 1_000.0;
    println!(
        "parse_ms={parse_ms:.3} typecheck_ms={typecheck_ms:.3} expressions={} checked={} diagnostics={} errors={} program={}",
        parsed.expressions.len(),
        output.report.checked_expression_count,
        output.report.diagnostics.len(),
        output.report.has_errors(),
        output.program.is_some(),
    );
    Ok(())
}
