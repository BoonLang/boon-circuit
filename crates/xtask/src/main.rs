mod architecture;
mod gates;
mod report_v2;
mod shaders;

use report_v2::{GateName, ReportStatus, ToolResult};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicCommand {
    Shaders,
    VerifyArchitecture,
    VerifyCounterDev,
    VerifyTodomvcPhysical,
    VerifyCells,
    VerifyNovywave,
    VerifyNegative,
    VerifyAll,
}

impl PublicCommand {
    const ALL: [Self; 8] = [
        Self::Shaders,
        Self::VerifyArchitecture,
        Self::VerifyCounterDev,
        Self::VerifyTodomvcPhysical,
        Self::VerifyCells,
        Self::VerifyNovywave,
        Self::VerifyNegative,
        Self::VerifyAll,
    ];

    fn parse(value: &str) -> Option<Self> {
        match value {
            "shaders" => Some(Self::Shaders),
            "verify-architecture" => Some(Self::VerifyArchitecture),
            "verify-counter-dev" => Some(Self::VerifyCounterDev),
            "verify-todomvc-physical" => Some(Self::VerifyTodomvcPhysical),
            "verify-cells" => Some(Self::VerifyCells),
            "verify-novywave" => Some(Self::VerifyNovywave),
            "verify-negative" => Some(Self::VerifyNegative),
            "verify-all" => Some(Self::VerifyAll),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Shaders => "shaders",
            Self::VerifyArchitecture => "verify-architecture",
            Self::VerifyCounterDev => "verify-counter-dev",
            Self::VerifyTodomvcPhysical => "verify-todomvc-physical",
            Self::VerifyCells => "verify-cells",
            Self::VerifyNovywave => "verify-novywave",
            Self::VerifyNegative => "verify-negative",
            Self::VerifyAll => "verify-all",
        }
    }

    fn gate(self) -> Option<GateName> {
        match self {
            Self::VerifyArchitecture => Some(GateName::Architecture),
            Self::VerifyCounterDev => Some(GateName::CounterDev),
            Self::VerifyTodomvcPhysical => Some(GateName::TodomvcPhysical),
            Self::VerifyCells => Some(GateName::Cells),
            Self::VerifyNovywave => Some(GateName::Novywave),
            Self::VerifyNegative => Some(GateName::Negative),
            Self::Shaders | Self::VerifyAll => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ParsedCommand {
    Shaders {
        check: bool,
    },
    Gate {
        command: PublicCommand,
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
    let parsed = parse_command(&args)?;
    let workspace = workspace_root();
    let status = match parsed {
        ParsedCommand::Help => {
            print_help();
            return Ok(());
        }
        ParsedCommand::Shaders { check } => {
            shaders::run(&workspace, check)?;
            return Ok(());
        }
        ParsedCommand::Gate { command, report } => gates::run_gate(
            &workspace,
            command.gate().expect("gate command has a gate"),
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

fn parse_command(args: &[String]) -> Result<ParsedCommand, String> {
    if args.is_empty() || matches!(args, [flag] if flag == "-h" || flag == "--help") {
        return Ok(ParsedCommand::Help);
    }
    let command_name = &args[0];
    let command = PublicCommand::parse(command_name)
        .ok_or_else(|| format!("unknown xtask command {command_name}"))?;
    match command {
        PublicCommand::Shaders => {
            let check = match &args[1..] {
                [] => false,
                [flag] if flag == "--check" => true,
                _ => return Err("usage: cargo xtask shaders [--check]".to_owned()),
            };
            Ok(ParsedCommand::Shaders { check })
        }
        PublicCommand::VerifyAll => {
            let mut check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], true, &mut check_existing, &mut report)?;
            Ok(ParsedCommand::VerifyAll {
                check_existing,
                report,
            })
        }
        gate => {
            let mut unused_check_existing = false;
            let mut report = None;
            parse_verify_options(&args[1..], false, &mut unused_check_existing, &mut report)?;
            Ok(ParsedCommand::Gate {
                command: gate,
                report,
            })
        }
    }
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

fn print_help() {
    println!("Boon Circuit tooling");
    for command in PublicCommand::ALL {
        println!("  {}", command.as_str());
    }
}

#[cfg(test)]
mod tests;
