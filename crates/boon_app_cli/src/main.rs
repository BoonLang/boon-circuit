use boon_app_package::{BuildRequest, NamespaceProfile, RunMode, build_app_package};
use std::path::PathBuf;

const HELP: &str = "\
usage:
  boon-app build <app.toml> --out <dir> --mode <deterministic|live> \\
    --namespace <deterministic|staging|production> --browser-wasm <path> \\
    --source-revision <identity> [--force]
";

fn main() {
    if let Err(error) = run() {
        eprintln!("boon-app: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || matches!(args[0].as_str(), "help" | "-h" | "--help") {
        print!("{HELP}");
        return Ok(());
    }
    if args[0] != "build" {
        return Err(format!("unknown command `{}`", args[0]).into());
    }
    let manifest = args
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .ok_or("build requires an app.toml path")?;
    let output = required_option(&args, "--out")?;
    let mode = required_option(&args, "--mode")?.parse::<RunMode>()?;
    let namespace = required_option(&args, "--namespace")?.parse::<NamespaceProfile>()?;
    let browser_wasm = required_option(&args, "--browser-wasm")?;
    let source_revision = required_option(&args, "--source-revision")?;
    reject_unknown_options(&args)?;
    let result = build_app_package(BuildRequest {
        manifest_path: &PathBuf::from(manifest),
        output_dir: &PathBuf::from(output),
        run_mode: mode,
        namespace_profile: namespace,
        browser_wasm: &PathBuf::from(browser_wasm),
        source_revision,
        force: args.iter().any(|arg| arg == "--force"),
    })?;
    let document = result
        .manifest
        .artifact(boon_plan::ProgramRole::Document)
        .ok_or("built bundle lost its document artifact")?;
    let server = result
        .manifest
        .artifact(boon_plan::ProgramRole::Server)
        .ok_or("built bundle lost its server artifact")?;
    println!(
        "built {} {} at {}\n  document {}\n  server   {}",
        result.manifest.package_id,
        result.manifest.source_revision,
        result.output_dir.display(),
        document.content_artifact_id,
        server.content_artifact_id,
    );
    Ok(())
}

fn required_option<'a>(args: &'a [String], option: &str) -> Result<&'a str, String> {
    let index = args
        .iter()
        .position(|arg| arg == option)
        .ok_or_else(|| format!("build requires {option} <value>"))?;
    args.get(index + 1)
        .filter(|value| !value.starts_with('-'))
        .map(String::as_str)
        .ok_or_else(|| format!("{option} requires a value"))
}

fn reject_unknown_options(args: &[String]) -> Result<(), String> {
    let options = [
        "--out",
        "--mode",
        "--namespace",
        "--browser-wasm",
        "--source-revision",
    ];
    let mut index = 2;
    while index < args.len() {
        let argument = args[index].as_str();
        if options.contains(&argument) {
            index += 2;
        } else if argument == "--force" {
            index += 1;
        } else {
            return Err(format!("unknown build argument `{argument}`"));
        }
    }
    Ok(())
}
