use super::{CLI_HELP, default_scenario};
use std::fs;
use std::path::PathBuf;

#[test]
fn help_advertises_supported_commands() {
    for command in [
        "run",
        "run-plan",
        "run-plan-route",
        "run-plan-root-scalar-scenario",
        "scenario",
        "dump-ir",
        "dump-plan",
        "explain-hardware",
    ] {
        assert!(
            CLI_HELP.contains(&format!("boon_cli {command}")),
            "help should advertise {command}"
        );
    }
}

#[test]
fn help_advertises_default_cli_report_contract() {
    assert!(CLI_HELP.contains("target/reports/<example>-cli-run.json"));
    assert!(CLI_HELP.contains("--scenario <path>"));
    assert!(CLI_HELP.contains("--report <path>"));
    assert!(!CLI_HELP.contains("--engine <"));
}

#[test]
fn default_scenario_uses_manifest_and_parser_not_text_substrings() {
    let path = temp_file_path("boon-cli-todomvc-looking-cells-path.bn");
    let source = include_str!("../../../examples/todomvc.bn");
    fs::write(&path, source).unwrap();

    let scenario = default_scenario(path.to_str().unwrap()).unwrap();

    let _ = fs::remove_file(&path);
    assert_eq!(scenario, "examples/todomvc.scn");
}

fn temp_file_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("{}-{name}", std::process::id()));
    path
}
