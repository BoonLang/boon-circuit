mod architecture;
mod fjordpulse_traceability;
mod gates;
mod report_v2;
mod shaders;

use fjordpulse_traceability::TraceabilityAction;
use report_v2::{GateName, HandoffManifest, ReportStatus, ToolResult, load_manifest};
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq)]
enum ParsedCommand {
    Shaders {
        check: bool,
    },
    FjordpulseTraceability {
        action: TraceabilityAction,
        reference: PathBuf,
    },
    Gate {
        gate: GateName,
        report: Option<PathBuf>,
    },
    VerifyAll {
        check_existing: bool,
        report: Option<PathBuf>,
    },
    Help,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> ToolResult<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let workspace = workspace_root();
    let (manifest, _) = load_manifest(&workspace)?;
    let parsed = parse_command(&args, &manifest)?;
    let status = match parsed {
        ParsedCommand::Help => {
            print_help(&manifest);
            return Ok(());
        }
        ParsedCommand::Shaders { check } => {
            shaders::run(&workspace, check)?;
            return Ok(());
        }
        ParsedCommand::FjordpulseTraceability { action, reference } => {
            fjordpulse_traceability::run(&workspace, action, &reference)?;
            return Ok(());
        }
        ParsedCommand::Gate { gate, report } => gates::run_gate(
            &workspace,
            gate,
            report.map(|path| resolve_path(&workspace, path)),
        )?,
        ParsedCommand::VerifyAll {
            check_existing,
            report,
        } => gates::run_verify_all(
            &workspace,
            check_existing,
            report.map(|path| resolve_path(&workspace, path)),
        )?,
    };
    if status == ReportStatus::Fail {
        return Err("verification wrote a valid fail report".into());
    }
    Ok(())
}

fn parse_command(args: &[String], manifest: &HandoffManifest) -> Result<ParsedCommand, String> {
    if args.is_empty() || matches!(args, [flag] if flag == "-h" || flag == "--help") {
        return Ok(ParsedCommand::Help);
    }
    let command_name = &args[0];
    match command_name.as_str() {
        "shaders" => {
            let check = match &args[1..] {
                [] => false,
                [flag] if flag == "--check" => true,
                _ => return Err("usage: cargo xtask shaders [--check]".to_owned()),
            };
            Ok(ParsedCommand::Shaders { check })
        }
        "fjordpulse-traceability" => {
            let (action, reference) = parse_fjordpulse_traceability_options(&args[1..])?;
            Ok(ParsedCommand::FjordpulseTraceability { action, reference })
        }
        command if command == manifest.aggregate.as_str() => {
            let mut check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], true, &mut check_existing, &mut report)?;
            Ok(ParsedCommand::VerifyAll {
                check_existing,
                report,
            })
        }
        command => {
            let entry = manifest
                .gate_for_verifier(command)
                .ok_or_else(|| format!("unknown xtask command {command_name}"))?;
            let mut unused_check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], false, &mut unused_check_existing, &mut report)?;
            Ok(ParsedCommand::Gate {
                gate: entry.gate.clone(),
                report,
            })
        }
    }
}

fn parse_fjordpulse_traceability_options(
    args: &[String],
) -> Result<(TraceabilityAction, PathBuf), String> {
    let (action, options) = match args {
        [action, options @ ..] if action == "import" => (TraceabilityAction::Import, options),
        [action, options @ ..] if action == "verify" => (TraceabilityAction::Verify, options),
        _ => {
            return Err(
                "usage: cargo xtask fjordpulse-traceability <import|verify> --reference <FjordPulse-repo>"
                    .to_owned(),
            );
        }
    };
    let reference = match options {
        [flag, path] if flag == "--reference" => PathBuf::from(path),
        _ => {
            return Err(
                "usage: cargo xtask fjordpulse-traceability <import|verify> --reference <FjordPulse-repo>"
                    .to_owned(),
            );
        }
    };
    Ok((action, reference))
}

fn parse_verify_options(
    args: &[String],
    allow_check_existing: bool,
    check_existing: &mut bool,
    report: &mut Option<PathBuf>,
) -> Result<(), String> {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check-existing" if allow_check_existing && !*check_existing => {
                *check_existing = true;
                index += 1;
            }
            "--report" if report.is_none() => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--report requires a path".to_owned())?;
                *report = Some(PathBuf::from(value));
                index += 2;
            }
            option => return Err(format!("unsupported or duplicate option {option}")),
        }
    }
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at crates/xtask")
        .to_path_buf()
}

fn resolve_path(workspace: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

fn print_help(manifest: &HandoffManifest) {
    println!("Boon Circuit tooling");
    println!("  shaders");
    println!("  fjordpulse-traceability <import|verify> --reference <FjordPulse-repo>");
    for gate in &manifest.gates {
        println!("  {}", gate.verifier.as_str());
    }
    println!("  {}", manifest.aggregate.as_str());
}

#[cfg(test)]
mod tests;
