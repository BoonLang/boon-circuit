mod architecture;
mod compiler_interactions;
mod compiler_performance;
mod dependency_classifier;
mod fjordpulse_traceability;
mod gates;
mod language_surface;
mod packed_baseline;
mod packed_site_inventory;
mod report_v2;
mod shaders;
mod verify_phase0;

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
    if let Some(status) = run_standalone(&workspace, &args)? {
        if status == ReportStatus::Fail {
            return Err("verification wrote a valid fail report".into());
        }
        return Ok(());
    }
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

fn run_standalone(workspace: &Path, args: &[String]) -> ToolResult<Option<ReportStatus>> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    match command {
        "shaders" => {
            let check = match &args[1..] {
                [] => false,
                [flag] if flag == "--check" => true,
                _ => return Err("usage: cargo xtask shaders [--check]".into()),
            };
            shaders::run(workspace, check)?;
            Ok(Some(ReportStatus::Pass))
        }
        "fjordpulse-traceability" => {
            let (action, reference) = parse_fjordpulse_traceability_options(&args[1..])?;
            fjordpulse_traceability::run(workspace, action, &reference)?;
            Ok(Some(ReportStatus::Pass))
        }
        "verify-language-surface" => {
            if args.len() != 1 {
                return Err("usage: cargo xtask verify-language-surface".into());
            }
            language_surface::run(workspace)?;
            Ok(Some(ReportStatus::Pass))
        }
        "verify-phase0" => {
            let mut unused_check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], false, &mut unused_check_existing, &mut report)?;
            verify_phase0::run(workspace, report.map(|path| resolve_path(workspace, path)))
                .map(Some)
        }
        "verify-packed-baseline" => {
            let mut check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], true, &mut check_existing, &mut report)?;
            packed_baseline::run(
                workspace,
                check_existing,
                report.map(|path| resolve_path(workspace, path)),
            )
            .map(Some)
        }
        "verify-compiler-performance" => {
            let (check_existing, report, setup_samples, scored_samples) =
                parse_compiler_performance_options(&args[1..])?;
            compiler_performance::run(
                workspace,
                check_existing,
                report.map(|path| resolve_path(workspace, path)),
                setup_samples,
                scored_samples,
            )
            .map(Some)
        }
        "verify-compiler-interactions" => {
            let (check_existing, report, setup_samples, scored_samples) =
                parse_compiler_performance_options(&args[1..])?;
            compiler_interactions::run(
                workspace,
                check_existing,
                report.map(|path| resolve_path(workspace, path)),
                setup_samples,
                scored_samples,
            )
            .map(Some)
        }
        "packed-site-inventory" => {
            packed_site_inventory::run_cli(workspace, &args[1..])
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            Ok(Some(ReportStatus::Pass))
        }
        _ => Ok(None),
    }
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

fn parse_compiler_performance_options(
    args: &[String],
) -> Result<(bool, Option<PathBuf>, Option<usize>, Option<usize>), String> {
    let mut check_existing = false;
    let mut report = None;
    let mut setup_samples = None;
    let mut scored_samples = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check-existing" if !check_existing => {
                check_existing = true;
                index += 1;
            }
            "--report" if report.is_none() => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--report requires a path".to_owned())?;
                report = Some(PathBuf::from(value));
                index += 2;
            }
            "--setup-samples" if setup_samples.is_none() => {
                setup_samples = Some(parse_sample_count(args, index, "--setup-samples")?);
                index += 2;
            }
            "--scored-samples" if scored_samples.is_none() => {
                scored_samples = Some(parse_sample_count(args, index, "--scored-samples")?);
                index += 2;
            }
            option => return Err(format!("unsupported or duplicate option {option}")),
        }
    }
    Ok((check_existing, report, setup_samples, scored_samples))
}

fn parse_sample_count(args: &[String], index: usize, option: &str) -> Result<usize, String> {
    args.get(index + 1)
        .ok_or_else(|| format!("{option} requires a count"))?
        .parse::<usize>()
        .map_err(|error| format!("invalid {option} count: {error}"))
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
    println!("  verify-language-surface");
    println!("  verify-phase0 [--report <path>]");
    println!("  verify-packed-baseline [--check-existing] [--report <path>]");
    println!(
        "  verify-compiler-performance [--check-existing] [--report <path>] [--setup-samples N] [--scored-samples N]"
    );
    println!(
        "  verify-compiler-interactions [--check-existing] [--report <path>] [--setup-samples N] [--scored-samples N]"
    );
    for gate in &manifest.gates {
        println!("  {}", gate.verifier.as_str());
    }
    println!("  {}", manifest.aggregate.as_str());
}
