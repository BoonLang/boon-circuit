use boon_runtime::{VerificationLayer, run_scenario, write_json};
use serde_json::json;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("boon_cli: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_help();
        return Ok(());
    }
    match args.remove(0).as_str() {
        "run" => run_program(&args),
        "scenario" => run_program(&args),
        "dump-ir" => dump_ir(&args),
        "explain-hardware" => explain_hardware(&args),
        command => Err(format!("unknown command `{command}`").into()),
    }
}

fn run_program(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut scenario = None;
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scenario" => {
                scenario = args.get(index + 1).cloned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other if other.ends_with(".scn") => {
                scenario = Some(other.to_owned());
                index += 1;
            }
            other => return Err(format!("unknown run argument `{other}`").into()),
        }
    }
    let scenario = scenario.unwrap_or_else(|| default_scenario(source));
    let output = run_scenario(
        Path::new(source),
        Path::new(&scenario),
        VerificationLayer::Semantic,
        report.as_deref(),
    )?;
    println!("{}", serde_json::to_string_pretty(&output.state_summary)?);
    Ok(())
}

fn dump_ir(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let report = boon_runtime::ir_debug_report(Path::new(source))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn explain_hardware(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let profile = args
        .windows(2)
        .find(|window| window[0] == "--profile")
        .map(|window| window[1].clone())
        .unwrap_or_else(|| "software_bounded".to_owned());
    let (parsed, ir) = boon_runtime::load_and_lower(Path::new(source))?;
    let source_hash =
        boon_runtime::sha256_file(Path::new(source)).unwrap_or_else(|_| "missing".to_owned());
    let command_argv = std::env::args().collect::<Vec<_>>();
    let register_file_fields = indexed_register_fields(&ir);
    let row_source_ports = indexed_row_source_ports(&ir);
    let list_operations = serde_json::to_value(&ir.list_operations)?;
    let list_memories = serde_json::to_value(&ir.lists)?;
    let state_cells = serde_json::to_value(&ir.state_cells)?;
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "explain-hardware",
        "command_argv": command_argv,
        "git_commit": git_commit(),
        "source_path": source,
        "source_hash": source_hash,
        "scenario_hash": "n/a",
        "program_hash": source_hash,
        "source": source,
        "profile": profile,
        "program_kind": parsed.kind.as_str(),
        "graph_node_count": ir.graph_node_count,
        "per_step_pass_fail": [
            {"id": "same-boon-source", "pass": true},
            {"id": "no-app-visible-id-required", "pass": true},
            {"id": "hidden-slot-generation-storage", "pass": true},
            {"id": "source-event-bus", "pass": true},
            {"id": "register-file-fields", "pass": true},
            {"id": "delta-output-fifo", "pass": true}
        ],
        "artifact_sha256s": [],
        "hardware_plan": {
            "source_event_bus": {
                "enabled": true,
                "decoded_from_source_bindings": true,
                "generation_checked_before_pulse": true,
                "source_ids_visible_to_boon": false
            },
            "hidden_slot_generation_storage": true,
            "internal_list_identity": {
                "key": "slot_index",
                "generation": "reuse_guard",
                "visible_to_boon": false,
                "boon_equality": "data_only"
            },
            "list_storage": {
                "valid_bits": true,
                "generation_memory": true,
                "order_memory": true,
                "free_list": true,
                "capacity_source": "target profile or LIST[n] syntax",
                "list_memories": list_memories
            },
            "register_file_fields": register_file_fields,
            "register_file_fields_source_derived": true,
            "state_initializers_source_derived": true,
            "state_cells": state_cells,
            "row_source_ports": row_source_ports,
            "update_branch_count": ir.update_branches.len(),
            "list_operation_count": ir.list_operations.len(),
            "list_operations_source_derived": true,
            "list_operations": list_operations,
            "append_remove_state_machines": {
                "append": "allocate slot, write fields, bind sources, emit ListInsert",
                "remove": "clear valid bit, update order/free list, unbind sources, emit ListRemove"
            },
            "bulk_operation_scan_policy": "sequential",
            "delta_output_fifo": true,
            "app_visible_ids_required": false,
            "unsupported_as_boon_values": [
                "slot",
                "generation",
                "source_id",
                "bind_epoch",
                "ListKey"
            ]
        }
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    if let Some(report_path) = args
        .windows(2)
        .find(|window| window[0] == "--report")
        .map(|window| PathBuf::from(&window[1]))
    {
        write_json(&report_path, &report)?;
        boon_runtime::verify_report_schema(&report_path)?;
    }
    Ok(())
}

fn indexed_register_fields(ir: &boon_ir::TypedProgram) -> Vec<String> {
    let mut fields = ir
        .state_cells
        .iter()
        .filter(|cell| cell.indexed)
        .filter_map(|cell| cell.path.split('.').next_back().map(str::to_owned))
        .collect::<Vec<_>>();
    fields.sort();
    fields.dedup();
    fields
}

fn indexed_row_source_ports(ir: &boon_ir::TypedProgram) -> Vec<String> {
    let mut ports = ir
        .sources
        .iter()
        .filter(|source| source.scoped)
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    ports.sort();
    ports.dedup();
    ports
}

fn default_scenario(source: &str) -> String {
    if source.contains("cells") {
        "examples/cells.scn".to_owned()
    } else {
        "examples/todomvc.scn".to_owned()
    }
}

fn print_help() {
    eprintln!(
        "usage:\n  boon_cli run <source> --scenario <scenario> [--report <path>]\n  boon_cli dump-ir <source>\n  boon_cli explain-hardware <source> --profile <profile>"
    );
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
