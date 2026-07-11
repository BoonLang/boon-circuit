use boon_compiler::compile_source_path_to_machine_plan;
use boon_plan::{TargetProfile, verify_plan};
use boon_runtime::{LiveRuntime, parse_scenario, source_units_for_path};
use std::fs;
use std::path::{Path, PathBuf};

const HELP: &str = "\
usage:
  boon_cli run <source> --scenario <path>
  boon_cli check <source> [--target <profile>]
  boon_cli dump-plan <source> [--target <profile>] [--out <path>]
  boon_cli dump-ir <source> [--out <path>]
";

fn main() {
    if let Err(error) = run() {
        eprintln!("boon_cli: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print!("{HELP}");
        return Ok(());
    };
    let args = args.collect::<Vec<_>>();
    match command.as_str() {
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        "run" => run_scenario(&args),
        "check" => check_source(&args),
        "dump-plan" => dump_plan(&args),
        "dump-ir" => dump_ir(&args),
        other => Err(format!("unknown command `{other}`").into()),
    }
}

fn run_scenario(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("run requires a source path")?;
    let scenario = option_value(args, "--scenario")?.ok_or("run requires --scenario <path>")?;
    reject_unknown_options(args, &["--scenario"])?;

    let units = source_units_for_path(Path::new(source))?;
    let mut runtime = LiveRuntime::from_project(source, &units)?;
    let scenario = parse_scenario(Path::new(&scenario))?;
    let turns = runtime.run_scenario(&scenario)?;
    let snapshot = runtime.snapshot()?;
    println!(
        "pass: {} turn(s), {} state value(s), {} derived field value(s), {} list(s)",
        turns.len(),
        snapshot.states.len(),
        snapshot.fields.len(),
        snapshot.lists.len()
    );
    Ok(())
}

fn check_source(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("check requires a source path")?;
    let target = target_profile(args)?;
    reject_unknown_options(args, &["--target"])?;
    let compiled = compile_source_path_to_machine_plan(Path::new(source), target)?;
    let verification = verify_plan(&compiled.plan)?;
    if verification.status != "pass" {
        return Err(format!(
            "MachinePlan verification failed with {} error(s)",
            verification.error_count
        )
        .into());
    }
    println!(
        "pass: MachinePlan {}.{}, {} operation(s)",
        compiled.plan.version.major,
        compiled.plan.version.minor,
        compiled.plan.capability_summary.operation_count
    );
    Ok(())
}

fn dump_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("dump-plan requires a source path")?;
    let target = target_profile(args)?;
    let out = option_value(args, "--out")?.map(PathBuf::from);
    reject_unknown_options(args, &["--target", "--out"])?;
    let compiled = compile_source_path_to_machine_plan(Path::new(source), target)?;
    let bytes = serde_json::to_vec_pretty(&compiled.plan)?;
    write_or_print(out.as_deref(), &bytes)
}

fn dump_ir(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("dump-ir requires a source path")?;
    let out = option_value(args, "--out")?.map(PathBuf::from);
    reject_unknown_options(args, &["--out"])?;
    let compiled =
        compile_source_path_to_machine_plan(Path::new(source), TargetProfile::SoftwareDefault)?;
    let bytes = serde_json::to_vec_pretty(&compiled.ir)?;
    write_or_print(out.as_deref(), &bytes)
}

fn target_profile(args: &[String]) -> Result<TargetProfile, Box<dyn std::error::Error>> {
    option_value(args, "--target")?
        .as_deref()
        .map(TargetProfile::from_name)
        .transpose()
        .map(|target| target.unwrap_or(TargetProfile::SoftwareDefault))
        .map_err(Into::into)
}

fn option_value(
    args: &[String],
    option: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(index) = args.iter().position(|arg| arg == option) else {
        return Ok(None);
    };
    Ok(Some(
        args.get(index + 1)
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone(),
    ))
}

fn reject_unknown_options(
    args: &[String],
    options_with_values: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut index = 1usize;
    while index < args.len() {
        let option = args[index].as_str();
        if options_with_values.contains(&option) {
            index += 2;
        } else {
            return Err(format!("unknown argument `{option}`").into());
        }
    }
    Ok(())
}

fn write_or_print(path: Option<&Path>, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
    } else {
        println!("{}", String::from_utf8_lossy(bytes));
    }
    Ok(())
}
