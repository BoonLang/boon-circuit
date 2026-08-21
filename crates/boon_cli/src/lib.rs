use boon_compiler::{
    CancellationToken, CompileIntent, CompileRequest, CompilerProject, CompilerSession,
    compile_machine_plan, compiler_source_project_for_path,
};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile};
use boon_runtime::{LiveRuntime, parse_scenario, source_units_for_path};
use std::fs;
use std::path::{Path, PathBuf};

pub mod allocator;
mod compiler_sample;

use allocator::{
    AllocationInstrumentation, AllocatorConfiguration, compiler_allocation_counters,
    reset_compiler_allocation_counters,
};

#[derive(Clone, Copy, Debug)]
pub struct ProducerConfiguration {
    pub product_kind: &'static str,
    pub allocator: AllocatorConfiguration,
    pub allocation_instrumentation: AllocationInstrumentation,
}

pub const fn system_product_configuration() -> ProducerConfiguration {
    ProducerConfiguration {
        product_kind: "linux-system-product",
        allocator: AllocatorConfiguration::system(),
        allocation_instrumentation: AllocationInstrumentation::None,
    }
}

pub const fn system_evidence_configuration() -> ProducerConfiguration {
    ProducerConfiguration {
        product_kind: "rust-allocation-evidence",
        allocator: AllocatorConfiguration::system(),
        allocation_instrumentation: AllocationInstrumentation::ThreadLocalRustGlobal,
    }
}

#[cfg(target_os = "linux")]
pub const fn mimalloc_product_configuration() -> ProducerConfiguration {
    ProducerConfiguration {
        product_kind: "linux-mimalloc-product",
        allocator: AllocatorConfiguration::mimalloc_3_5(),
        allocation_instrumentation: AllocationInstrumentation::None,
    }
}

#[cfg(target_os = "linux")]
pub const fn mimalloc_evidence_configuration() -> ProducerConfiguration {
    ProducerConfiguration {
        product_kind: "rust-allocation-evidence",
        allocator: AllocatorConfiguration::mimalloc_3_5(),
        allocation_instrumentation: AllocationInstrumentation::ThreadLocalRustGlobal,
    }
}

const HELP: &str = "\
usage:
  boon_cli run <source> --scenario <path>
  boon_cli check <source> [--target <profile>]
  boon_cli dump-plan <source> [--target <profile>] [--out <path>]
  boon_cli dump-ir <source> [--out <path>]
  boon_cli compiler-sample <source> --intent <diagnostics|verified> --mode <fresh-process|empty-session> [--samples <count>]
";

pub fn main_entry(producer: ProducerConfiguration) {
    if let Err(error) = run(producer) {
        eprintln!("boon_cli: {error}");
        std::process::exit(1);
    }
}

fn run(producer: ProducerConfiguration) -> Result<(), Box<dyn std::error::Error>> {
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
        "compiler-sample" => compiler_sample::run(&args, producer),
        other => Err(format!("unknown command `{other}`").into()),
    }
}

fn run_scenario(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("run requires a source path")?;
    let scenario = option_value(args, "--scenario")?.ok_or("run requires --scenario <path>")?;
    reject_unknown_options(args, &["--scenario"])?;

    let units = source_units_for_path(Path::new(source))?;
    let activation = LiveRuntime::from_project(source, &units)?;
    let (mut runtime, initial_turn, _) = activation.into_parts();
    let scenario = parse_scenario(Path::new(&scenario))?;
    let mut turns = vec![initial_turn];
    turns.extend(runtime.run_scenario(&scenario)?);
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
    let (entrypoint, units) = compiler_source_project_for_path(Path::new(source))?;
    let mut compiler = CompilerSession::new();
    let project = compiler.open_project(CompilerProject::new(
        entrypoint,
        units,
        target,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let revision = compiler.revision(project)?;
    let result = compiler.request(
        project,
        revision,
        CompileIntent::VerifiedCheck,
        &CancellationToken::new(),
    )?;
    let compiled = result
        .compiled()
        .ok_or("verified check produced no compiled artifact")?;
    let verification = compiled.plan.verification();
    if verification.status != "pass" {
        let failed = verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .map(|check| format!("{}: {}", check.id, check.detail))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "MachinePlan verification failed with {} error(s): {failed}",
            verification.error_count,
        )
        .into());
    }
    let plan = compiled.plan.plan();
    println!(
        "pass: MachinePlan {}.{}, {} operation(s)",
        plan.version.major, plan.version.minor, plan.capability_summary.operation_count
    );
    Ok(())
}

fn dump_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("dump-plan requires a source path")?;
    let target = target_profile(args)?;
    let out = option_value(args, "--out")?.map(PathBuf::from);
    reject_unknown_options(args, &["--target", "--out"])?;
    let compiled = compile_machine_plan(CompileRequest::source_path(
        Path::new(source),
        target,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let bytes = serde_json::to_vec_pretty(&compiled.plan)?;
    write_or_print(out.as_deref(), &bytes)
}

fn dump_ir(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("dump-ir requires a source path")?;
    let out = option_value(args, "--out")?.map(PathBuf::from);
    reject_unknown_options(args, &["--out"])?;
    let compiled = compile_machine_plan(CompileRequest::source_path(
        Path::new(source),
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
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
