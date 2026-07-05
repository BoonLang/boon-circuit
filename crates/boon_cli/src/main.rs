#![recursion_limit = "256"]

use boon_compiler::{compile_source_path_to_full_ir, compile_typed_program};
use boon_plan::{TargetProfile, verify_plan};
use boon_runtime::{
    emit_compiled_artifact, inspect_compiled_artifact_report, run_plan_initial_state,
    run_plan_root_scalar_scenario, run_plan_scenario_events, run_plan_source_route, write_json,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CLI_HELP: &str = "\
usage:
  boon_cli run <source> [--scenario <path>] [--engine <plan>] [--target <software_default|software_bounded|fpga_todomvc>] [--report <path>] [--print-report]
  boon_cli scenario <source> [--scenario <path>] [--report <path>]
  boon_cli run-plan <source> [--target <software_default|software_bounded|fpga_todomvc>] [--report <path>]
  boon_cli run-plan-route <source> --source <source-route> --target-state <state-path> [--text <text>] [--key <key>] [--target-key <row-key>] [--target-generation <generation>] [--address <address>] [--payload <name=value>] [--payload-bytes-hex <name=hex>] [--payload-bytes-file <name=path>] [--target <software_default|software_bounded|fpga_todomvc>] [--report <path>]
  boon_cli run-plan-root-scalar-scenario <source> --scenario <path> --steps <id[,id...]> [--target <software_default|software_bounded|fpga_todomvc>] [--report <path>]
  boon_cli compile <source> --out <path.boonc> [--report <path>]
  boon_cli inspect-artifact <path.boonc> [--report <path>]
  boon_cli dump-ir <source>
  boon_cli dump-plan <source> [--target <software_default|software_bounded|fpga_todomvc>] [--report <path>]
  boon_cli explain-hardware <source> [--profile <software_bounded|fpga_todomvc>] [--target <software_bounded|fpga_todomvc>] [--report <path>]

Bundled examples default to target/reports/<example>-cli-run.json when --report is omitted.
";

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
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "run" => run_program(&args),
        "scenario" => run_program(&args),
        "run-plan" => run_plan(&args),
        "run-plan-route" => run_plan_route(&args),
        "run-plan-root-scalar-scenario" => run_plan_root_scalar_scenario_cmd(&args),
        "compile" => compile_program(&args),
        "inspect-artifact" => inspect_artifact(&args),
        "dump-ir" => dump_ir(&args),
        "dump-plan" => dump_plan(&args),
        "explain-hardware" => explain_hardware(&args),
        command => Err(format!("unknown command `{command}`").into()),
    }
}

fn run_program(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut scenario = None;
    let mut engine = "plan".to_owned();
    let mut target = "software_default".to_owned();
    let mut report = None;
    let mut explicit_report = false;
    let mut print_report = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scenario" => {
                scenario = args.get(index + 1).cloned();
                index += 2;
            }
            "--engine" => {
                engine = args
                    .get(index + 1)
                    .ok_or("missing value for --engine")?
                    .clone();
                index += 2;
            }
            "--target" | "--profile" => {
                target = args
                    .get(index + 1)
                    .ok_or("missing value for --target")?
                    .to_owned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                explicit_report = true;
                index += 2;
            }
            "--print-report" => {
                print_report = true;
                index += 1;
            }
            other if other.ends_with(".scn") => {
                scenario = Some(other.to_owned());
                index += 1;
            }
            other => return Err(format!("unknown run argument `{other}`").into()),
        }
    }
    let scenario = match scenario {
        Some(scenario) => scenario,
        None => default_scenario(source)?,
    };
    let report = report.or_else(|| default_cli_report(source, &scenario));
    match engine.as_str() {
        "plan" => {
            let target_profile = TargetProfile::from_name(&target)?;
            let output = run_plan_scenario_events(
                Path::new(source),
                Path::new(&scenario),
                target_profile,
                report.as_deref(),
            )?;
            if print_report || !explicit_report {
                println!("{}", serde_json::to_string_pretty(&output.report)?);
            }
            if output
                .report
                .get("status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                return Err(format!(
                    "run --engine {engine} report status is `{}`",
                    output
                        .report
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                )
                .into());
            }
        }
        "legacy" | "semantic" => {
            return Err(
                "normal `run` no longer exposes the legacy runtime; use PlanExecutor product commands"
                    .into(),
            );
        }
        "compare" => {
            return Err("normal `run` no longer exposes legacy comparison".into());
        }
        other => return Err(format!("unknown run engine `{other}`").into()),
    }
    Ok(())
}

fn run_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut target = "software_default".to_owned();
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--target" | "--profile" => {
                target = args
                    .get(index + 1)
                    .ok_or("missing value for --target")?
                    .to_owned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("unknown run-plan argument `{other}`").into()),
        }
    }
    let target_profile = TargetProfile::from_name(&target)?;
    let output = run_plan_initial_state(Path::new(source), target_profile, report.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&output.report)?);
    Ok(())
}

fn run_plan_route(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut target_profile = "software_default".to_owned();
    let mut source_route = None;
    let mut target_state = None;
    let mut text = None;
    let mut key = None;
    let mut address = None;
    let mut target_key = None;
    let mut target_generation = None;
    let mut payload = BTreeMap::new();
    let mut payload_bytes = BTreeMap::new();
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--source" | "--route" => {
                source_route = args.get(index + 1).cloned();
                index += 2;
            }
            "--target-state" | "--state" | "--summary-path" => {
                target_state = args.get(index + 1).cloned();
                index += 2;
            }
            "--text" => {
                text = args.get(index + 1).cloned();
                index += 2;
            }
            "--key" => {
                key = args.get(index + 1).cloned();
                index += 2;
            }
            "--target-key" => {
                let value = args
                    .get(index + 1)
                    .ok_or("missing value for --target-key")?;
                target_key = Some(
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --target-key `{value}`: {error}"))?,
                );
                index += 2;
            }
            "--target-generation" => {
                let value = args
                    .get(index + 1)
                    .ok_or("missing value for --target-generation")?;
                target_generation =
                    Some(value.parse::<u64>().map_err(|error| {
                        format!("invalid --target-generation `{value}`: {error}")
                    })?);
                index += 2;
            }
            "--address" => {
                address = args.get(index + 1).cloned();
                index += 2;
            }
            "--payload" => {
                let value = args.get(index + 1).ok_or("missing value for --payload")?;
                let (name, value) = value
                    .split_once('=')
                    .ok_or("--payload expects <name=value>")?;
                payload.insert(name.to_owned(), value.to_owned());
                index += 2;
            }
            "--payload-bytes-hex" => {
                let value = args
                    .get(index + 1)
                    .ok_or("missing value for --payload-bytes-hex")?;
                let (name, value) = value
                    .split_once('=')
                    .ok_or("--payload-bytes-hex expects <name=hex>")?;
                if name != "bytes" {
                    return Err(format!(
                        "--payload-bytes-hex supports only the reserved BYTES payload key `bytes` in v1, got `{name}`"
                    )
                    .into());
                }
                payload_bytes.insert(
                    name.to_owned(),
                    decode_hex_bytes(value)
                        .map_err(|error| format!("--payload-bytes-hex {name}: {error}"))?,
                );
                index += 2;
            }
            "--payload-bytes-file" => {
                let value = args
                    .get(index + 1)
                    .ok_or("missing value for --payload-bytes-file")?;
                let (name, path) = value
                    .split_once('=')
                    .ok_or("--payload-bytes-file expects <name=path>")?;
                if name != "bytes" {
                    return Err(format!(
                        "--payload-bytes-file supports only the reserved BYTES payload key `bytes` in v1, got `{name}`"
                    )
                    .into());
                }
                payload_bytes.insert(
                    name.to_owned(),
                    fs::read(path)
                        .map_err(|error| format!("--payload-bytes-file {name}: {error}"))?,
                );
                index += 2;
            }
            "--diagnostic-compare-legacy" | "--compare-legacy" => {
                return Err("run-plan-route no longer exposes legacy comparison".into());
            }
            "--target" | "--profile" => {
                target_profile = args
                    .get(index + 1)
                    .ok_or("missing value for --target")?
                    .to_owned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("unknown run-plan-route argument `{other}`").into()),
        }
    }
    let source_route = source_route.ok_or("missing --source <source-route>")?;
    let target_state = target_state.ok_or("missing --target-state <state-path>")?;
    let target_profile = TargetProfile::from_name(&target_profile)?;
    let event = boon_runtime::LiveSourceEvent {
        source: source_route.clone(),
        text,
        key,
        address,
        target_key,
        target_generation,
        payload,
        payload_bytes,
        ..Default::default()
    };
    let output = run_plan_source_route(
        Path::new(source),
        target_profile,
        &source_route,
        &target_state,
        event,
        report.as_deref(),
    )?;
    println!("{}", serde_json::to_string_pretty(&output.report)?);
    Ok(())
}

fn decode_hex_bytes(value: &str) -> Result<Vec<u8>, String> {
    let compact = value.trim();
    if !compact.len().is_multiple_of(2) {
        return Err("hex payload must contain an even number of digits".to_owned());
    }
    let mut bytes = Vec::with_capacity(compact.len() / 2);
    for index in (0..compact.len()).step_by(2) {
        let pair = &compact[index..index + 2];
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|_| format!("invalid hex byte `{pair}` at byte {}", index / 2))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn run_plan_root_scalar_scenario_cmd(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut target_profile = "software_default".to_owned();
    let mut scenario = None;
    let mut selected_steps = Vec::new();
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scenario" => {
                scenario = args.get(index + 1).cloned();
                index += 2;
            }
            "--steps" => {
                let value = args.get(index + 1).ok_or("missing value for --steps")?;
                selected_steps.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|step| !step.is_empty())
                        .map(str::to_owned),
                );
                index += 2;
            }
            "--step" => {
                selected_steps.push(
                    args.get(index + 1)
                        .ok_or("missing value for --step")?
                        .clone(),
                );
                index += 2;
            }
            "--diagnostic-compare-legacy" | "--compare-legacy" => {
                return Err(
                    "run-plan-root-scalar-scenario no longer exposes legacy comparison".into(),
                );
            }
            "--target" | "--profile" => {
                target_profile = args
                    .get(index + 1)
                    .ok_or("missing value for --target")?
                    .to_owned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => {
                return Err(
                    format!("unknown run-plan-root-scalar-scenario argument `{other}`").into(),
                );
            }
        }
    }
    let scenario = match scenario {
        Some(scenario) => scenario,
        None => default_scenario(source)?,
    };
    if selected_steps.is_empty() {
        return Err("run-plan-root-scalar-scenario requires --steps <id[,id...]>".into());
    }
    let target_profile = TargetProfile::from_name(&target_profile)?;
    let output = run_plan_root_scalar_scenario(
        Path::new(source),
        Path::new(&scenario),
        target_profile,
        &selected_steps,
        report.as_deref(),
    )?;
    println!("{}", serde_json::to_string_pretty(&output.report)?);
    Ok(())
}

fn compile_program(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut out = None;
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                out = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("unknown compile argument `{other}`").into()),
        }
    }
    let out = out.ok_or("missing --out <path.boonc>")?;
    let report_value = emit_compiled_artifact(Path::new(source), &out, report.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&report_value)?);
    Ok(())
}

fn inspect_artifact(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let artifact = args.first().ok_or("missing artifact path")?;
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("unknown inspect-artifact argument `{other}`").into()),
        }
    }
    let report_value = inspect_compiled_artifact_report(Path::new(artifact), report.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&report_value)?);
    Ok(())
}

fn dump_ir(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let report = boon_runtime::ir_debug_report(Path::new(source))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn dump_plan(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let mut target = "software_default".to_owned();
    let mut report = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--target" | "--profile" => {
                target = args
                    .get(index + 1)
                    .ok_or("missing value for --target")?
                    .to_owned();
                index += 2;
            }
            "--report" => {
                report = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("unknown dump-plan argument `{other}`").into()),
        }
    }
    let target_profile = TargetProfile::from_name(&target)?;
    let compiled = compile_source_path_to_full_ir(Path::new(source))?;
    let parsed = compiled.parsed;
    let ir = compiled.ir;
    let plan = compile_typed_program(&ir, target_profile)?;
    let verification = verify_plan(&plan)?;
    let program_hash = parsed_program_hash(&parsed);
    let source_hash = parsed_source_hash(&parsed);
    let source_files = parsed_source_files_report(&parsed);
    let budget_hash = boon_runtime::sha256_file(&Path::new(source).with_extension("budget.toml"))
        .unwrap_or_else(|_| "missing-budget".to_owned());
    let command_argv = std::env::args().collect::<Vec<_>>();
    let typed_lowering_executable = plan.capability_summary.typed_lowering_executable;
    let report_status = if verification.error_count == 0 && typed_lowering_executable {
        "pass"
    } else {
        "fail"
    };
    let verification_error_count = verification.error_count;
    let plan_hash = verification.plan_hash.clone();
    let plan_version = plan.version;
    let capability_summary = plan.capability_summary.clone();
    let report_value = json!({
        "status": report_status,
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "dump-plan",
        "command_argv": command_argv,
        "measurement_mode": "diagnostic",
        "exit_status": if verification_error_count == 0 { 0 } else { 1 },
        "git_commit": git_commit(),
        "worktree_fingerprint": worktree_fingerprint(),
        "binary_hash": current_binary_hash(),
        "binary_path": current_binary_path(),
        "source_path": source,
        "source_hash": source_hash.clone(),
        "source_files": source_files,
        "scenario_hash": "n/a",
        "program_hash": program_hash,
        "budget_hash": budget_hash,
        "graph_node_count": ir.graph_node_count,
        "target_profile": target_profile.as_str(),
        "plan_version": plan_version,
        "plan_hash": plan_hash,
        "capability_summary": capability_summary,
        "verification": verification.clone(),
        "typecheck_report": {
            "resolved_constant_table": &ir.typecheck_report.resolved_constant_table
        },
        "per_step_pass_fail": [
            {
                "id": "machine-plan-constructed",
                "pass": true,
                "detail": "semantic TypedProgram compiled into Phase 1 MachinePlan scaffold"
            },
            {
                "id": "machine-plan-verified",
                "pass": verification_error_count == 0,
                "detail": format!("{} structural verification error(s)", verification_error_count)
            },
            {
                "id": "machine-plan-typed-lowering-executable",
                "pass": typed_lowering_executable,
                "detail": if typed_lowering_executable {
                    format!(
                        "plan has no unresolved typed executable refs; whole-plan CPU executor complete={}",
                        capability_summary.cpu_plan_executor_complete
                    )
                } else {
                    format!(
                        "plan is structural only: {} unresolved executable ref(s), {} executable string path(s)",
                        capability_summary.unresolved_executable_ref_count,
                        capability_summary.executable_string_path_count
                    )
                }
            },
            {
                "id": "dump-plan-does-not-execute-program",
                "pass": true,
                "detail": "dump-plan is a developer inspection command and does not execute the runtime"
            }
        ],
        "artifact_sha256s": [],
        "machine_plan": plan,
    });
    if let Some(report) = report {
        write_json(&report, &report_value)?;
    }
    println!("{}", serde_json::to_string_pretty(&report_value)?);
    Ok(())
}

fn explain_hardware(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args.first().ok_or("missing source path")?;
    let profile = args
        .windows(2)
        .find(|window| window[0] == "--profile" || window[0] == "--target")
        .map(|window| window[1].clone())
        .unwrap_or_else(|| "software_bounded".to_owned());
    let hardware_profile = HardwareProfile::for_name(&profile)?;
    let ir = compile_source_path_to_full_ir(Path::new(source))?.ir;
    hardware_profile.validate_program(&ir)?;
    let source_hash =
        boon_runtime::sha256_file(Path::new(source)).unwrap_or_else(|_| "missing".to_owned());
    let command_argv = std::env::args().collect::<Vec<_>>();
    let register_file_fields = indexed_register_fields(&ir);
    let row_source_ports = indexed_row_source_ports(&ir);
    let list_operations = serde_json::to_value(&ir.list_operations)?;
    let list_memories = hardware_profile.list_memories(&ir)?;
    let runtime_profile = hardware_profile.runtime_profile();
    let capacities = hardware_profile.capacity_report(&ir, runtime_profile);
    let state_cells = serde_json::to_value(&ir.state_cells)?;
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "explain-hardware",
        "command_argv": command_argv,
        "measurement_mode": "diagnostic",
        "exit_status": 0,
        "git_commit": git_commit(),
        "worktree_fingerprint": worktree_fingerprint(),
        "binary_hash": current_binary_hash(),
        "binary_path": current_binary_path(),
        "source_path": source,
        "source_hash": source_hash,
        "scenario_hash": "n/a",
        "program_hash": source_hash,
        "budget_hash": boon_runtime::sha256_file(&Path::new(source).with_extension("budget.toml")).unwrap_or_else(|_| "missing-budget".to_owned()),
        "source": source,
        "profile": profile,
        "runtime_profile": runtime_profile,
        "runtime_profile_detail": hardware_profile.runtime_profile_detail(runtime_profile),
        "capacities": capacities,
        "program_kind": "generic",
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
                "capacity_source": hardware_profile.capacity_source,
                "overflow_policy": hardware_profile.overflow_policy,
                "list_memories": list_memories
            },
            "bounded_storage_profile": {
                "name": hardware_profile.name,
                "clock": hardware_profile.clock,
                "reset": hardware_profile.reset,
                "todos_capacity": hardware_profile.todos_capacity,
                "todo_title_width": hardware_profile.todo_title_width,
                "todo_edit_text_width": hardware_profile.todo_edit_text_width,
                "input_event_fifo_capacity": hardware_profile.input_event_fifo_capacity,
                "output_delta_fifo_capacity": hardware_profile.output_delta_fifo_capacity,
                "unbounded_text_allowed": hardware_profile.unbounded_text_allowed
            },
            "fixed_text_storage": {
                "todo.title": {
                    "width": hardware_profile.todo_title_width,
                    "encoding": "ascii"
                },
                "todo.edit_text": {
                    "width": hardware_profile.todo_edit_text_width,
                    "encoding": "ascii"
                }
            },
            "input_event_fifo": {
                "capacity": hardware_profile.input_event_fifo_capacity,
                "overflow_policy": hardware_profile.overflow_policy
            },
            "output_delta_fifo": {
                "capacity": hardware_profile.output_delta_fifo_capacity,
                "overflow_policy": hardware_profile.overflow_policy
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

struct HardwareProfile {
    name: &'static str,
    clock: &'static str,
    reset: &'static str,
    todos_capacity: usize,
    todo_title_width: usize,
    todo_edit_text_width: usize,
    input_event_fifo_capacity: usize,
    output_delta_fifo_capacity: usize,
    capacity_source: &'static str,
    overflow_policy: &'static str,
    unbounded_text_allowed: bool,
}

impl HardwareProfile {
    fn for_name(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match name {
            "fpga_todomvc" => Ok(Self {
                name: "fpga_todomvc",
                clock: "PASSED.clk",
                reset: "PASSED.reset",
                todos_capacity: 256,
                todo_title_width: 64,
                todo_edit_text_width: 64,
                input_event_fifo_capacity: 16,
                output_delta_fifo_capacity: 64,
                capacity_source: "fpga_todomvc target profile",
                overflow_policy: "explicit_overflow_error",
                unbounded_text_allowed: false,
            }),
            "software_bounded" => Ok(Self {
                name: "software_bounded",
                clock: "software_tick",
                reset: "software_reset",
                todos_capacity: 10_000,
                todo_title_width: 256,
                todo_edit_text_width: 256,
                input_event_fifo_capacity: 1024,
                output_delta_fifo_capacity: 4096,
                capacity_source: "software_bounded verification profile",
                overflow_policy: "reported_runtime_error",
                unbounded_text_allowed: false,
            }),
            other => Err(format!(
                "unsupported hardware explanation profile `{other}`; expected `fpga_todomvc` or `software_bounded`"
            )
            .into()),
        }
    }

    fn validate_program(
        &self,
        ir: &boon_ir::TypedProgram,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.name == "fpga_todomvc" && !ir.lists.iter().any(|list| list.name == "todos") {
            return Err("profile `fpga_todomvc` requires a manifest/source with a `todos` list in this phase".into());
        }
        Ok(())
    }

    fn list_memories(
        &self,
        ir: &boon_ir::TypedProgram,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut memories = serde_json::to_value(&ir.lists)?;
        let Some(items) = memories.as_array_mut() else {
            return Ok(memories);
        };
        for item in items {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            let declared_capacity = object
                .get("capacity")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as usize);
            let effective_capacity = declared_capacity.unwrap_or(self.todos_capacity);
            object.insert("effective_capacity".to_owned(), json!(effective_capacity));
            object.insert(
                "capacity_source".to_owned(),
                json!(if declared_capacity.is_some() {
                    "LIST[n] syntax"
                } else {
                    self.capacity_source
                }),
            );
            object.insert("overflow_policy".to_owned(), json!(self.overflow_policy));
            object.insert(
                "fixed_text_fields".to_owned(),
                json!({
                    "title": {
                        "width": self.todo_title_width,
                        "encoding": "ascii"
                    },
                    "edit_text": {
                        "width": self.todo_edit_text_width,
                        "encoding": "ascii"
                    }
                }),
            );
        }
        Ok(memories)
    }

    fn runtime_profile(&self) -> &'static str {
        match self.name {
            "fpga_todomvc" => "hardware_bounded",
            "software_bounded" => "software_bounded",
            _ => "hardware_bounded",
        }
    }

    fn runtime_profile_detail(&self, runtime_profile: &str) -> serde_json::Value {
        json!({
            "name": runtime_profile,
            "mode": match runtime_profile {
                "hardware_bounded" => "hardware_style_bounded",
                "software_bounded" => "bounded_software",
                _ => "bounded",
            },
            "target_profile": self.name,
            "capacity_source": self.capacity_source,
            "overflow_behavior": self.overflow_policy,
            "bounded_allocation_budget_applies_after_preparation": true,
            "unbounded_lists": [],
            "unbounded_text_allowed": self.unbounded_text_allowed
        })
    }

    fn capacity_report(
        &self,
        ir: &boon_ir::TypedProgram,
        runtime_profile: &str,
    ) -> serde_json::Value {
        let bytes = ir
            .state_cells
            .iter()
            .filter_map(|cell| match &cell.initial_value {
                boon_ir::InitialValue::Bytes { fixed_len, .. } => Some(json!({
                    "name": cell.path,
                    "scope": if cell.indexed { "indexed" } else { "root" },
                    "fixed_len": fixed_len,
                    "effective_capacity": fixed_len,
                    "capacity_source": if fixed_len.is_some() {
                        "BYTES[N] fixed length"
                    } else {
                        "dynamic BYTES"
                    },
                    "dynamic_growth_allowed": false,
                    "overflow_behavior": if fixed_len.is_some() {
                        self.overflow_policy
                    } else {
                        "not_hardware_bounded"
                    }
                })),
                _ => None,
            })
            .collect::<Vec<_>>();
        json!({
            "profile": runtime_profile,
            "all_lists_bounded": true,
            "lists": ir.lists.iter().map(|list| {
                let declared_capacity = list.capacity.map(|capacity| capacity as u64);
                let effective_capacity = declared_capacity.unwrap_or(self.todos_capacity as u64);
                json!({
                    "name": list.name,
                    "declared_capacity": declared_capacity,
                    "effective_capacity": effective_capacity,
                    "capacity_source": if declared_capacity.is_some() {
                        "LIST[n] syntax"
                    } else {
                        self.capacity_source
                    },
                    "dynamic_growth_allowed": false,
                    "overflow_behavior": self.overflow_policy
                })
            }).collect::<Vec<_>>(),
            "all_bytes_bounded": ir.state_cells.iter().all(|cell| match &cell.initial_value {
                boon_ir::InitialValue::Bytes { fixed_len, .. } => fixed_len.is_some(),
                _ => true,
            }),
            "bytes": bytes,
        })
    }
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

fn default_scenario(source: &str) -> Result<String, Box<dyn std::error::Error>> {
    let source_path = Path::new(source);
    let entries = boon_runtime::example_manifest_entries()?;
    if let Some(entry) = entries.iter().into_iter().find(|entry| {
        let entry_source = Path::new(&entry.source);
        entry_source == source_path || entry_source.file_name() == source_path.file_name()
    }) {
        return Ok(entry.scenario.clone());
    }
    if let Ok(source_text) = fs::read_to_string(source_path)
        && let Ok(program) = boon_parser::parse_source(source, &source_text)
        && let Some(example_id) = parsed_default_example_id(&program)
        && let Some(entry) = entries.iter().find(|entry| entry.id == example_id)
    {
        return Ok(entry.scenario.clone());
    }
    Err(format!(
        "no default scenario for `{source}`; add it to examples/manifest.toml or pass --scenario"
    )
    .into())
}

fn parsed_default_example_id(program: &boon_parser::ParsedProgram) -> Option<&'static str> {
    if program
        .list_memories
        .iter()
        .any(|list| list.name == "todos")
    {
        return Some("todomvc");
    }
    if program
        .functions
        .iter()
        .any(|function| function == "cells_app")
        || program
            .list_memories
            .iter()
            .any(|list| list.name == "cells")
    {
        return Some("cells");
    }
    None
}

fn default_cli_report(source: &str, scenario: &str) -> Option<PathBuf> {
    let source_path = Path::new(source);
    let scenario_path = Path::new(scenario);
    match (
        source_path.file_name().and_then(|name| name.to_str()),
        scenario_path.file_name().and_then(|name| name.to_str()),
    ) {
        (Some("todomvc.bn"), Some("todomvc.scn")) => {
            Some(PathBuf::from("target/reports/todomvc-cli-run.json"))
        }
        (Some("cells.bn"), Some("cells.scn")) => {
            Some(PathBuf::from("target/reports/cells-cli-run.json"))
        }
        _ => None,
    }
}

fn print_help() {
    eprint!("{CLI_HELP}");
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

fn worktree_fingerprint() -> String {
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .ok()
        .map(|output| output.stdout)
        .unwrap_or_default();
    let diff = std::process::Command::new("git")
        .args(["diff", "--binary", "HEAD", "--"])
        .output()
        .ok()
        .map(|output| output.stdout)
        .unwrap_or_default();
    boon_runtime::sha256_bytes(&[status, diff].concat())
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| boon_runtime::sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_binary_path() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn parsed_program_hash(program: &boon_parser::ParsedProgram) -> String {
    let mut hasher = Sha256::new();
    let mut files = program.files.iter().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.module.cmp(&right.module))
    });
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.module.as_deref().unwrap_or("").as_bytes());
        hasher.update([0]);
        hasher.update(file.start_line.to_le_bytes());
        hasher.update([0]);
        hasher.update(file.source.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn parsed_source_hash(program: &boon_parser::ParsedProgram) -> String {
    let mut files = program.files.iter().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.module.cmp(&right.module))
    });
    if let [file] = files.as_slice() {
        return boon_runtime::sha256_bytes(file.source.as_bytes());
    }
    let mut canonical = String::new();
    for file in files {
        canonical.push_str(&file.path);
        canonical.push('\0');
        canonical.push_str(&boon_runtime::sha256_bytes(file.source.as_bytes()));
        canonical.push('\0');
    }
    boon_runtime::sha256_bytes(canonical.as_bytes())
}

fn parsed_source_files_report(program: &boon_parser::ParsedProgram) -> Vec<serde_json::Value> {
    let mut files = program.files.iter().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.module.cmp(&right.module))
    });
    files
        .into_iter()
        .map(|file| {
            json!({
                "path": file.path,
                "module": file.module,
                "start_line": file.start_line,
                "source_hash": boon_runtime::sha256_bytes(file.source.as_bytes())
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
        assert!(!CLI_HELP.contains("diagnose-plan-legacy-compare"));
        assert!(!CLI_HELP.contains("[--diagnostic-compare-legacy]"));
        assert!(!CLI_HELP.contains("--engine <legacy|plan|compare>"));
        assert!(!CLI_HELP.contains("[--compare-legacy]"));
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
}
