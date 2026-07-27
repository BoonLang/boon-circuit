use boon_phase0_baseline::allocator::CountingSystem;
use boon_phase0_baseline::report::{
    DEFAULT_BUDGET, DEFAULT_MANIFEST, DEFAULT_REPORT, SourceIdentity,
};
use boon_phase0_baseline::runner::{ProducerOptions, produce};
use std::path::{Path, PathBuf};

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingSystem = CountingSystem;

fn main() {
    if let Err(error) = run() {
        eprintln!("boon_phase0_baseline: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let workspace = workspace_root();
    let mut manifest = workspace.join(DEFAULT_MANIFEST);
    let mut budget = workspace.join(DEFAULT_BUDGET);
    let mut report = workspace.join(DEFAULT_REPORT);
    let mut source_head = None;
    let mut workspace_digest = None;
    let mut dirty = None;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--manifest" => {
                manifest = option_path(&args, index, "--manifest", &workspace)?;
                index += 2;
            }
            "--report" => {
                report = option_path(&args, index, "--report", &workspace)?;
                index += 2;
            }
            "--budget" => {
                budget = option_path(&args, index, "--budget", &workspace)?;
                index += 2;
            }
            "--source-head" if source_head.is_none() => {
                source_head = Some(option_value(&args, index, "--source-head")?.to_owned());
                index += 2;
            }
            "--workspace-digest" if workspace_digest.is_none() => {
                workspace_digest =
                    Some(option_value(&args, index, "--workspace-digest")?.to_owned());
                index += 2;
            }
            "--dirty" if dirty.is_none() => {
                dirty = Some(match option_value(&args, index, "--dirty")? {
                    "true" => true,
                    "false" => false,
                    value => {
                        return Err(format!("--dirty expects true or false, received `{value}`"));
                    }
                });
                index += 2;
            }
            option => return Err(format!("unsupported or duplicate option `{option}`")),
        }
    }
    let source = SourceIdentity {
        head: source_head.ok_or("--source-head is required")?,
        workspace_digest: workspace_digest.ok_or("--workspace-digest is required")?,
        dirty: dirty.ok_or("--dirty is required")?,
    };
    let report = produce(&ProducerOptions {
        workspace,
        manifest_path: manifest,
        budget_path: budget,
        report_path: report,
        source,
    })?;
    println!(
        "packed baseline: {:?}; {} fixtures",
        report.status,
        report.fixtures.len()
    );
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("baseline crate lives at crates/boon_phase0_baseline")
        .to_path_buf()
}

fn option_value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn option_path(
    args: &[String],
    index: usize,
    flag: &str,
    workspace: &Path,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(option_value(args, index, flag)?);
    Ok(if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    })
}
