use boon_compiler::{
    CancellationToken, CompileIntent, CompileRequest, CompilerProject, CompilerSession,
    compile_machine_plan, compiler_source_project_for_path,
};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile, verify_plan};
use boon_runtime::{LiveRuntime, parse_scenario, source_units_for_path};
use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod compiler_sample;

struct CompilerCountingAllocator;

static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CompilerCountingAllocator = CompilerCountingAllocator;

// SAFETY: every operation is forwarded unchanged to the process System
// allocator. The atomics are observation-only and do not influence allocation
// addresses, sizes, alignment, or lifetimes.
unsafe impl GlobalAlloc for CompilerCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: this method has exactly the `GlobalAlloc::alloc` contract and
        // forwards the same valid layout to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: this method forwards the same valid layout to `System`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        DEALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: this method forwards the allocation pointer and its original
        // layout unchanged to `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        DEALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        DEALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        // SAFETY: this method forwards the allocation pointer, original
        // layout, and requested size unchanged to `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CompilerAllocationCounters {
    pub allocation_calls: u64,
    pub allocated_bytes: u64,
    pub deallocation_calls: u64,
    pub deallocated_bytes: u64,
}

pub(crate) fn reset_compiler_allocation_counters() {
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    DEALLOCATION_CALLS.store(0, Ordering::Relaxed);
    DEALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

pub(crate) fn compiler_allocation_counters() -> CompilerAllocationCounters {
    CompilerAllocationCounters {
        allocation_calls: ALLOCATION_CALLS.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        deallocation_calls: DEALLOCATION_CALLS.load(Ordering::Relaxed),
        deallocated_bytes: DEALLOCATED_BYTES.load(Ordering::Relaxed),
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
        "compiler-sample" => compiler_sample::run(&args),
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
    let verification = verify_plan(&compiled.plan)?;
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
