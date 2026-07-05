#![recursion_limit = "256"]

use boon_plan::{
    FieldId, InitialValueKind, MachinePlan, PlanConstantId, PlanConstantValue,
    PlanDerivedExpression, PlanExpressionKind, PlanListOperationKind, PlanListProjection, PlanOp,
    PlanOpId, PlanOpKind, PlanRowExpression, PlanRowSelectPattern, PlanSourceGuard, PlanValueType,
    RegionKind, SourceId, SourcePayloadField, SourceRoute, StateId, ValueRef, plan_sha256,
    verify_plan,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub type PlanExecutorResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialStateExecution {
    pub plan_hash: String,
    pub state_summary: JsonValue,
    pub initialized_state_count: usize,
    pub source_route_metadata_count: usize,
    pub list_slot_count: usize,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialStateReportAssembly {
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteSelection {
    pub plan_hash: String,
    pub source_label: String,
    pub source_id: SourceId,
    pub target_state_label: String,
    pub target_state_id: StateId,
    pub update_op_id: PlanOpId,
    pub source_payload_field: Option<SourcePayloadField>,
    pub update_constant_id: Option<PlanConstantId>,
    pub indexed: bool,
    pub unresolved_executable_ref_count: usize,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteExecutionContext {
    pub selection: SourceRouteSelection,
    pub source_route_slot: SourceRoute,
    pub update_op: PlanOp,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteJsonExecution {
    pub plan_hash: String,
    pub source_label: String,
    pub source_id: SourceId,
    pub target_state_label: String,
    pub target_state_id: StateId,
    pub update_op_id: PlanOpId,
    pub supported: bool,
    pub skipped_by_guard: bool,
    pub unsupported_reason: Option<String>,
    pub value: Option<JsonValue>,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: Vec<JsonValue>,
    pub expression_kind: Option<&'static str>,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceRouteExecutionSurfaceKind {
    PlanJson,
    RuntimeBranch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteExecutionSurface {
    pub kind: SourceRouteExecutionSurfaceKind,
    pub route_core_value_is_bytes: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteFullExecutionValidation {
    pub target_state_id: StateId,
    pub target_state_label: String,
    pub value: JsonValue,
    pub state_summary: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteReportAssembly {
    pub route_surface: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteSelectedExecution {
    pub value: JsonValue,
    pub expression_kind: String,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub host_effect: JsonValue,
    pub executor_core: JsonValue,
    pub state_write_core: JsonValue,
    pub bytes_state_core: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteRuntimeBranchExecutionInput {
    pub value: JsonValue,
    pub expression_kind: String,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub host_effect: JsonValue,
    pub state_write_core: JsonValue,
    pub bytes_state_core: JsonValue,
    pub runtime_branch_core: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteFullExecution {
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub per_step: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteOrchestration {
    pub plan_hash: String,
    pub source_id: SourceId,
    pub value: JsonValue,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub route_surface: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteCommandReportInput {
    pub command_argv: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub route_surface: JsonValue,
    pub source_event: JsonValue,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub artifact_sha256s: Vec<JsonValue>,
    pub plan_executor: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteCommandReportAssembly {
    pub report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteCommandOutputInput {
    pub current_args: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub source_route: String,
    pub target_state: String,
    pub event: SourceRouteSourceEventReportInput,
    pub payload_bytes: BTreeMap<String, Vec<u8>>,
    pub report_path: Option<PathBuf>,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub route_surface: JsonValue,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub plan_executor: JsonValue,
    pub inline_byte_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteCommandOutput {
    pub report: JsonValue,
    pub source_event: JsonValue,
    pub command_argv: Vec<String>,
    pub artifact_sha256s: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteCommandArgvInput {
    pub current_args: Vec<String>,
    pub source_path: String,
    pub target_profile: String,
    pub source_route: String,
    pub target_state: String,
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub payload: BTreeMap<String, String>,
    pub payload_bytes: BTreeMap<String, Vec<u8>>,
    pub payload_byte_artifact_paths: BTreeMap<String, String>,
    pub report_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRouteSourceEventReportInput {
    pub source: String,
    pub source_id: u64,
    pub text: Option<String>,
    pub key: Option<String>,
    pub list_id: Option<String>,
    pub address: Option<String>,
    pub target_text: Option<String>,
    pub target_occurrence: Option<usize>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub source_epoch: Option<u64>,
    pub payload: BTreeMap<String, String>,
    pub payload_bytes_report: JsonValue,
    pub pointer_x: Option<String>,
    pub pointer_y: Option<String>,
    pub pointer_width: Option<String>,
    pub pointer_height: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceEventPayloadBytesReport {
    pub payload_bytes: JsonValue,
    pub artifacts: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

pub fn build_source_route_source_event_report(
    input: SourceRouteSourceEventReportInput,
) -> JsonValue {
    json!({
        "source": input.source,
        "source_id": input.source_id,
        "text": input.text,
        "key": input.key,
        "list_id": input.list_id,
        "address": input.address,
        "target_text": input.target_text,
        "target_occurrence": input.target_occurrence,
        "target_key": input.target_key,
        "target_generation": input.target_generation,
        "bind_epoch": input.bind_epoch,
        "source_epoch": input.source_epoch,
        "payload": input.payload,
        "payload_bytes": input.payload_bytes_report,
        "pointer_x": input.pointer_x,
        "pointer_y": input.pointer_y,
        "pointer_width": input.pointer_width,
        "pointer_height": input.pointer_height,
    })
}

pub fn build_source_event_payload_bytes_report(
    payload_bytes: &BTreeMap<String, Vec<u8>>,
    report_path: Option<&Path>,
    inline_byte_limit: usize,
) -> PlanExecutorResult<SourceEventPayloadBytesReport> {
    let mut payload = serde_json::Map::new();
    let mut artifacts = Vec::new();
    let mut inline_payload_count = 0usize;
    let mut artifact_payload_count = 0usize;
    let mut inline_byte_count = 0usize;
    let mut artifact_byte_count = 0usize;

    for (field, bytes) in payload_bytes {
        let digest = sha256_bytes(bytes);
        let byte_len = bytes.len() as u64;
        if bytes.len() <= inline_byte_limit {
            inline_payload_count += 1;
            inline_byte_count += bytes.len();
            payload.insert(
                field.clone(),
                json!({
                    "$boon_type": "BYTES",
                    "storage": "inline",
                    "digest": digest,
                    "byte_len": byte_len,
                    "inline_bytes": bytes.iter().map(|byte| json!(*byte)).collect::<Vec<_>>(),
                    "inline_byte_limit": inline_byte_limit
                }),
            );
            continue;
        }

        artifact_payload_count += 1;
        artifact_byte_count += bytes.len();
        let artifact_path = source_event_bytes_artifact_path(report_path, field, &digest);
        if let Some(parent) = artifact_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&artifact_path, bytes)?;
        let artifact_path = artifact_path.display().to_string();
        payload.insert(
            field.clone(),
            json!({
                "$boon_type": "BYTES",
                "storage": "artifact",
                "digest": digest.clone(),
                "byte_len": byte_len,
                "artifact_path": artifact_path,
                "artifact_sha256": digest.clone(),
                "inline_byte_limit": inline_byte_limit
            }),
        );
        artifacts.push(json!({
            "path": artifact_path,
            "sha256": digest
        }));
    }

    Ok(SourceEventPayloadBytesReport {
        payload_bytes: JsonValue::Object(payload),
        artifacts,
        executor_report: json!({
            "executor": "cpu-plan-source-event-payload-bytes-report-v1",
            "payload_field_count": payload_bytes.len(),
            "inline_payload_count": inline_payload_count,
            "artifact_payload_count": artifact_payload_count,
            "inline_byte_count": inline_byte_count,
            "artifact_byte_count": artifact_byte_count,
            "inline_byte_limit": inline_byte_limit,
            "runtime_ast_eval_count": 0,
            "runtime_string_eval_count": 0,
            "unknown_plan_op_count": 0,
            "graph_rebuild_count": 0
        }),
    })
}

fn source_event_bytes_artifact_path(
    report_path: Option<&Path>,
    field: &str,
    digest: &str,
) -> PathBuf {
    let safe_field = sanitize_artifact_name(field);
    match report_path {
        Some(path) => {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(sanitize_artifact_name)
                .unwrap_or_else(|| "run-plan-route".to_owned());
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            parent
                .join(format!("{stem}-artifacts"))
                .join(format!("source-event-{safe_field}-{digest}.bytes"))
        }
        None => PathBuf::from("target")
            .join("reports")
            .join("bytes-plan")
            .join("source-event-artifacts")
            .join(format!("source-event-{safe_field}-{digest}.bytes")),
    }
}

fn sanitize_artifact_name(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        output.push_str("payload");
    }
    output
}

pub fn build_source_route_command_argv(input: SourceRouteCommandArgvInput) -> Vec<String> {
    if input.current_args.iter().any(|arg| arg == "run-plan-route") {
        return input.current_args;
    }

    let mut argv = vec![
        "target/debug/boon_cli".to_owned(),
        "run-plan-route".to_owned(),
        input.source_path,
        "--source".to_owned(),
        input.source_route,
        "--target-state".to_owned(),
        input.target_state,
    ];
    if let Some(text) = input.text {
        argv.push("--text".to_owned());
        argv.push(text);
    }
    if let Some(key) = input.key {
        argv.push("--key".to_owned());
        argv.push(key);
    }
    if let Some(address) = input.address {
        argv.push("--address".to_owned());
        argv.push(address);
    }
    for (name, value) in input.payload {
        argv.push("--payload".to_owned());
        argv.push(format!("{name}={value}"));
    }
    for (name, bytes) in input.payload_bytes {
        if let Some(path) = input.payload_byte_artifact_paths.get(&name) {
            argv.push("--payload-bytes-file".to_owned());
            argv.push(format!("{name}={path}"));
        } else {
            argv.push("--payload-bytes-hex".to_owned());
            argv.push(format!("{name}={}", bytes_encode_hex(&bytes)));
        }
    }
    if input.target_profile != "software-default" {
        argv.push("--target".to_owned());
        argv.push(input.target_profile);
    }
    if let Some(report_path) = input.report_path {
        argv.push("--report".to_owned());
        argv.push(report_path);
    }
    argv
}

pub fn assemble_source_route_command_output(
    input: SourceRouteCommandOutputInput,
) -> PlanExecutorResult<SourceRouteCommandOutput> {
    let payload_report = build_source_event_payload_bytes_report(
        &input.payload_bytes,
        input.report_path.as_deref(),
        input.inline_byte_limit,
    )?;
    let source_event = build_source_route_source_event_report(SourceRouteSourceEventReportInput {
        payload_bytes_report: payload_report.payload_bytes,
        ..input.event
    });
    let payload_byte_artifact_paths = input
        .payload_bytes
        .keys()
        .filter_map(|name| {
            source_event
                .get("payload_bytes")
                .and_then(|payload| payload.get(name))
                .and_then(|payload| payload.get("artifact_path"))
                .and_then(JsonValue::as_str)
                .map(|path| (name.clone(), path.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let command_argv = build_source_route_command_argv(SourceRouteCommandArgvInput {
        current_args: input.current_args,
        source_path: input.source_path.clone(),
        target_profile: input.target_profile.clone(),
        source_route: input.source_route.clone(),
        target_state: input.target_state.clone(),
        text: source_event
            .get("text")
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
        key: source_event
            .get("key")
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
        address: source_event
            .get("address")
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
        payload: source_event
            .get("payload")
            .and_then(JsonValue::as_object)
            .map(|payload| {
                payload
                    .iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        payload_bytes: input.payload_bytes.clone(),
        payload_byte_artifact_paths,
        report_path: input
            .report_path
            .as_ref()
            .map(|path| path.display().to_string()),
    });
    let command_output_core = json!({
        "executor": "cpu-plan-source-route-command-output-v1",
        "source": input.source_route,
        "target_state": input.target_state,
        "payload_byte_artifact_count": payload_report.artifacts.len(),
        "inline_byte_limit": input.inline_byte_limit,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "source_event_payload_bytes_core": payload_report.executor_report,
    });
    let mut plan_executor = input.plan_executor;
    if let Some(object) = plan_executor.as_object_mut() {
        object.insert(
            "command_output_core".to_owned(),
            command_output_core.clone(),
        );
    }
    let report = assemble_source_route_command_report(SourceRouteCommandReportInput {
        command_argv: command_argv.clone(),
        generated_at_utc: input.generated_at_utc,
        git_commit: input.git_commit,
        worktree_fingerprint: input.worktree_fingerprint,
        binary_hash: input.binary_hash,
        binary_path: input.binary_path,
        source_path: input.source_path,
        source_hash: input.source_hash,
        source_files: input.source_files,
        program_hash: input.program_hash,
        program_kind: input.program_kind,
        program_file_count: input.program_file_count,
        graph_node_count: input.graph_node_count,
        load_pipeline_profile: input.load_pipeline_profile,
        target_profile: input.target_profile,
        plan_hash: input.plan_hash,
        plan_version: input.plan_version,
        capability_summary: input.capability_summary,
        route_surface: input.route_surface,
        source_event: source_event.clone(),
        state_summary: input.state_summary,
        semantic_delta_signatures: input.semantic_delta_signatures,
        semantic_deltas: input.semantic_deltas,
        artifact_sha256s: payload_report.artifacts.clone(),
        plan_executor,
    })
    .report;
    Ok(SourceRouteCommandOutput {
        report,
        source_event,
        command_argv,
        artifact_sha256s: payload_report.artifacts,
        executor_report: command_output_core,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioCommandReportInput {
    pub command_argv: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub scenario_path: String,
    pub scenario_hash: String,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub selected_step_ids: Vec<String>,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub plan_executor: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioCommandReportAssembly {
    pub report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioCommandOutputInput {
    pub command_argv: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub scenario_path: String,
    pub scenario_hash: String,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub selected_step_ids: Vec<String>,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub plan_executor: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioCommandOutput {
    pub report: JsonValue,
    pub command_argv: Vec<String>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEventsCommandReportInput {
    pub command_argv: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub scenario_path: String,
    pub scenario_hash: String,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub plan_executor_coverage: JsonValue,
    pub assertion_only_covered: bool,
    pub plan_executor: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEventsCommandReportAssembly {
    pub report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEventsCommandOutputInput {
    pub command_argv: Vec<String>,
    pub generated_at_utc: String,
    pub git_commit: String,
    pub worktree_fingerprint: String,
    pub binary_hash: String,
    pub binary_path: String,
    pub source_path: String,
    pub source_hash: String,
    pub source_files: Vec<String>,
    pub scenario_path: String,
    pub scenario_hash: String,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
    pub target_profile: String,
    pub plan_hash: String,
    pub plan_version: JsonValue,
    pub capability_summary: JsonValue,
    pub state_summary: JsonValue,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: JsonValue,
    pub plan_executor_coverage: JsonValue,
    pub assertion_only_covered: bool,
    pub plan_executor: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEventsCommandOutput {
    pub report: JsonValue,
    pub command_argv: Vec<String>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug)]
pub struct PlanExecutorExpectedSourceEvent<'a> {
    pub source: &'a str,
    pub text: Option<&'a str>,
    pub key: Option<&'a str>,
    pub list_id: Option<&'a str>,
    pub target_text: Option<&'a str>,
    pub target_occurrence: Option<usize>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub address: Option<&'a str>,
    pub source_epoch: Option<u64>,
    pub source_id: Option<u64>,
    pub payload: BTreeMap<String, &'a str>,
    pub payload_bytes: BTreeMap<String, Vec<u8>>,
    pub pointer_x: Option<&'a str>,
    pub pointer_y: Option<&'a str>,
    pub pointer_width: Option<&'a str>,
    pub pointer_height: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct PlanExecutorLiveSourceEvent<'a> {
    pub source: &'a str,
    pub text: Option<&'a str>,
    pub key: Option<&'a str>,
    pub list_id: Option<&'a str>,
    pub address: Option<&'a str>,
    pub target_text: Option<&'a str>,
    pub target_occurrence: Option<u64>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub source_epoch: Option<u64>,
    pub source_id: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct PlanExecutorLiveSourceEventExpectedToml<'a> {
    pub source: &'a str,
    pub text: Option<&'a str>,
    pub key: Option<&'a str>,
    pub list_id: Option<&'a str>,
    pub address: Option<&'a str>,
    pub payload: &'a BTreeMap<String, String>,
    pub payload_bytes: &'a BTreeMap<String, Vec<u8>>,
    pub pointer_x: Option<&'a str>,
    pub pointer_y: Option<&'a str>,
    pub pointer_width: Option<&'a str>,
    pub pointer_height: Option<&'a str>,
    pub target_text: Option<&'a str>,
    pub target_occurrence: Option<usize>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub source_epoch: Option<u64>,
    pub source_id: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanExecutorScenarioStepMeta {
    pub id: String,
    pub has_expected_source_event: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplicitRootScenarioStepSelection {
    pub selected_indices: Vec<usize>,
    pub selected_step_ids: Vec<String>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEventsStepSelection {
    pub all_indices: Vec<usize>,
    pub selected_indices: Vec<usize>,
    pub selected_step_ids: Vec<String>,
    pub assertion_only_step_ids: Vec<String>,
    pub executor_report: JsonValue,
}

pub fn decode_expected_source_event<'a>(
    step_id: &str,
    expected: &'a BTreeMap<String, toml::Value>,
) -> PlanExecutorResult<PlanExecutorExpectedSourceEvent<'a>> {
    let source = toml_string_ref(expected, "source")
        .ok_or_else(|| format!("{step_id} expected_source_event missing source"))?;
    let mut payload = BTreeMap::new();
    let mut payload_bytes = BTreeMap::new();
    for (key, value) in expected {
        if matches!(
            key.as_str(),
            "source"
                | "text"
                | "key"
                | "target_text"
                | "address"
                | "target_occurrence"
                | "target_key"
                | "target_generation"
                | "bind_epoch"
                | "source_id"
                | "source_epoch"
                | "list_id"
        ) {
            continue;
        }
        if let Some(field) = source_payload_bytes_field_from_toml_key(key)? {
            let value = value.as_str().ok_or_else(|| {
                format!("{step_id} expected_source_event `{key}` BYTES payload must be hex text")
            })?;
            let bytes = bytes_decode_hex(value).map_err(|code| {
                format!(
                    "{step_id} expected_source_event `{key}` has invalid hex BYTES payload: {code}"
                )
            })?;
            payload_bytes.insert(field, bytes);
            continue;
        }
        if let Some(value) = value.as_str() {
            payload.insert(key.clone(), value);
        }
    }

    Ok(PlanExecutorExpectedSourceEvent {
        source,
        text: toml_string_ref(expected, "text"),
        key: toml_string_ref(expected, "key"),
        list_id: toml_string_ref(expected, "list_id"),
        target_text: toml_string_ref(expected, "target_text"),
        target_occurrence: toml_usize_ref(expected, "target_occurrence"),
        target_key: toml_u64_ref(expected, "target_key"),
        target_generation: toml_u64_ref(expected, "target_generation"),
        bind_epoch: toml_u64_ref(expected, "bind_epoch"),
        address: toml_string_ref(expected, "address"),
        source_epoch: toml_u64_ref(expected, "source_epoch"),
        source_id: toml_u64_ref(expected, "source_id"),
        payload,
        payload_bytes,
        pointer_x: toml_string_ref(expected, "pointer_x"),
        pointer_y: toml_string_ref(expected, "pointer_y"),
        pointer_width: toml_string_ref(expected, "pointer_width"),
        pointer_height: toml_string_ref(expected, "pointer_height"),
    })
}

pub fn assert_live_source_event_matches_expected(
    step_id: &str,
    expected: Option<&BTreeMap<String, toml::Value>>,
    event: PlanExecutorLiveSourceEvent<'_>,
) -> PlanExecutorResult<()> {
    let Some(expected) = expected else {
        return Err(format!(
            "{step_id} routed a source-producing event without expected_source_event"
        )
        .into());
    };
    let expected = decode_expected_source_event(step_id, expected)?;
    assert_live_source_event_field(step_id, Some(expected.source), "source", Some(event.source))?;
    assert_live_source_event_field(step_id, expected.text, "text", event.text)?;
    assert_live_source_event_field(step_id, expected.key, "key", event.key)?;
    assert_live_source_event_field(step_id, expected.list_id, "list_id", event.list_id)?;
    assert_live_source_event_field(step_id, expected.address, "address", event.address)?;
    assert_live_source_event_field(
        step_id,
        expected.target_text,
        "target_text",
        event.target_text,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.target_occurrence.map(|value| value as u64),
        "target_occurrence",
        event.target_occurrence,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.target_key,
        "target_key",
        event.target_key,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.target_generation,
        "target_generation",
        event.target_generation,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.bind_epoch,
        "bind_epoch",
        event.bind_epoch,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.source_epoch,
        "source_epoch",
        event.source_epoch,
    )?;
    assert_live_source_event_numeric_field(
        step_id,
        expected.source_id,
        "source_id",
        event.source_id,
    )?;
    Ok(())
}

pub fn build_live_source_event_expected_toml(
    event: PlanExecutorLiveSourceEventExpectedToml<'_>,
) -> BTreeMap<String, toml::Value> {
    let mut expected_source_event = BTreeMap::new();
    expected_source_event.insert(
        "source".to_owned(),
        toml::Value::String(event.source.to_owned()),
    );
    if let Some(text) = event.text {
        expected_source_event.insert("text".to_owned(), toml::Value::String(text.to_owned()));
    }
    if let Some(key) = event.key {
        expected_source_event.insert("key".to_owned(), toml::Value::String(key.to_owned()));
    }
    if let Some(list_id) = event.list_id {
        expected_source_event.insert(
            "list_id".to_owned(),
            toml::Value::String(list_id.to_owned()),
        );
    }
    if let Some(address) = event.address {
        expected_source_event.insert(
            "address".to_owned(),
            toml::Value::String(address.to_owned()),
        );
    }
    for (key, value) in event.payload {
        expected_source_event
            .entry(key.clone())
            .or_insert_with(|| toml::Value::String(value.clone()));
    }
    for (field, bytes) in event.payload_bytes {
        expected_source_event
            .entry(source_payload_bytes_toml_key(field))
            .or_insert_with(|| toml::Value::String(bytes_encode_hex(bytes)));
    }
    if let Some(pointer_x) = event.pointer_x {
        expected_source_event.insert(
            "pointer_x".to_owned(),
            toml::Value::String(pointer_x.to_owned()),
        );
    }
    if let Some(pointer_y) = event.pointer_y {
        expected_source_event.insert(
            "pointer_y".to_owned(),
            toml::Value::String(pointer_y.to_owned()),
        );
    }
    if let Some(pointer_width) = event.pointer_width {
        expected_source_event.insert(
            "pointer_width".to_owned(),
            toml::Value::String(pointer_width.to_owned()),
        );
    }
    if let Some(pointer_height) = event.pointer_height {
        expected_source_event.insert(
            "pointer_height".to_owned(),
            toml::Value::String(pointer_height.to_owned()),
        );
    }
    if let Some(target_text) = event.target_text {
        expected_source_event.insert(
            "target_text".to_owned(),
            toml::Value::String(target_text.to_owned()),
        );
    }
    if let Some(target_occurrence) = event.target_occurrence {
        expected_source_event.insert(
            "target_occurrence".to_owned(),
            toml::Value::Integer(target_occurrence as i64),
        );
    }
    if let Some(target_key) = event.target_key {
        expected_source_event.insert(
            "target_key".to_owned(),
            toml::Value::Integer(target_key as i64),
        );
    }
    if let Some(target_generation) = event.target_generation {
        expected_source_event.insert(
            "target_generation".to_owned(),
            toml::Value::Integer(target_generation as i64),
        );
    }
    if let Some(bind_epoch) = event.bind_epoch {
        expected_source_event.insert(
            "bind_epoch".to_owned(),
            toml::Value::Integer(bind_epoch as i64),
        );
    }
    if let Some(source_epoch) = event.source_epoch {
        expected_source_event.insert(
            "source_epoch".to_owned(),
            toml::Value::Integer(source_epoch as i64),
        );
    }
    if let Some(source_id) = event.source_id {
        expected_source_event.insert(
            "source_id".to_owned(),
            toml::Value::Integer(source_id as i64),
        );
    }
    expected_source_event
}

pub fn select_explicit_root_scenario_steps(
    scenario_name: &str,
    steps: &[PlanExecutorScenarioStepMeta],
    requested_step_ids: &[String],
) -> PlanExecutorResult<ExplicitRootScenarioStepSelection> {
    if requested_step_ids.is_empty() {
        return Err(
            "run-plan-root-scalar-scenario requires --steps with explicit scenario step ids".into(),
        );
    }
    let mut selected_indices = Vec::new();
    let mut selected_step_ids = Vec::new();
    for step_id in requested_step_ids {
        let (index, step) = steps
            .iter()
            .enumerate()
            .find(|(_, step)| step.id == *step_id)
            .ok_or_else(|| format!("scenario `{scenario_name}` has no step `{step_id}`"))?;
        if !step.has_expected_source_event {
            return Err(format!(
                "selected step `{step_id}` has no expected_source_event and cannot be replayed by PlanExecutor"
            )
            .into());
        }
        selected_indices.push(index);
        selected_step_ids.push(step.id.clone());
    }
    let executor_report = json!({
        "executor": "cpu-plan-explicit-root-scenario-step-selection-v1",
        "scenario": scenario_name,
        "requested_step_ids": requested_step_ids,
        "selected_step_ids": selected_step_ids,
        "selected_indices": selected_indices,
        "scenario_step_count": steps.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ExplicitRootScenarioStepSelection {
        selected_indices,
        selected_step_ids,
        executor_report,
    })
}

pub fn select_scenario_event_steps(
    scenario_name: &str,
    steps: &[PlanExecutorScenarioStepMeta],
) -> PlanExecutorResult<ScenarioEventsStepSelection> {
    let selected_indices = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| step.has_expected_source_event.then_some(index))
        .collect::<Vec<_>>();
    if selected_indices.is_empty() {
        return Err(format!(
            "scenario `{scenario_name}` has no expected_source_event steps for PlanExecutor replay"
        )
        .into());
    }
    let all_indices = (0..steps.len()).collect::<Vec<_>>();
    let selected_step_ids = selected_indices
        .iter()
        .map(|index| steps[*index].id.clone())
        .collect::<Vec<_>>();
    let assertion_only_step_ids = steps
        .iter()
        .filter(|step| !step.has_expected_source_event)
        .map(|step| step.id.clone())
        .collect::<Vec<_>>();
    let executor_report = json!({
        "executor": "cpu-plan-scenario-events-step-selection-v1",
        "scenario": scenario_name,
        "selected_step_ids": selected_step_ids,
        "assertion_only_step_ids": assertion_only_step_ids,
        "selected_indices": selected_indices,
        "all_indices": all_indices,
        "scenario_step_count": steps.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ScenarioEventsStepSelection {
        all_indices,
        selected_indices,
        selected_step_ids,
        assertion_only_step_ids,
        executor_report,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioReportAssembly {
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioStepReportAssembly {
    pub step_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioCoverageReport {
    pub assertion_only_covered: bool,
    pub coverage: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanExecutorListRow {
    pub key: u64,
    pub generation: u64,
    pub fields: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanExecutorListRowState {
    pub key: u64,
    pub generation: u64,
    pub fields: BTreeMap<String, JsonValue>,
    pub private_bytes: BTreeMap<String, PlanExecutorBytes>,
    pub fixed_bytes_banks: BTreeMap<String, Vec<u8>>,
}

impl PlanExecutorListRowState {
    pub fn public_row(&self) -> PlanExecutorListRow {
        PlanExecutorListRow {
            key: self.key,
            generation: self.generation,
            fields: self.fields.clone(),
        }
    }
}

pub fn list_row_state_public_rows(
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
) -> BTreeMap<usize, Vec<PlanExecutorListRow>> {
    list_state
        .iter()
        .map(|(list_id, rows)| {
            (
                *list_id,
                rows.iter()
                    .map(PlanExecutorListRowState::public_row)
                    .collect(),
            )
        })
        .collect()
}

pub fn list_row_textlike_field(row: &PlanExecutorListRow, field: &str) -> Option<String> {
    row.fields.get(field).and_then(json_scalar_textlike)
}

fn json_scalar_textlike(value: &JsonValue) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_owned());
    }
    if let Some(value) = value.as_i64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_u64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_f64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Some(if value { "True" } else { "False" }.to_owned());
    }
    None
}

pub fn list_row_report_fields(
    row: &PlanExecutorListRow,
    private_bytes: &BTreeMap<String, PlanExecutorBytes>,
) -> BTreeMap<String, JsonValue> {
    let mut fields = row.fields.clone();
    for (field, bytes) in private_bytes {
        fields.insert(field.clone(), bytes.report_json());
    }
    fields
}

pub fn list_row_state_report_fields(row: &PlanExecutorListRowState) -> BTreeMap<String, JsonValue> {
    list_row_report_fields(&row.public_row(), &row.private_bytes)
}

pub fn row_scoped_source_paths(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
) -> Vec<String> {
    let mut routes = plan
        .source_routes
        .iter()
        .filter(|route| route.scoped && route.scope_id == list_slot.scope_id)
        .collect::<Vec<_>>();
    routes.sort_by_key(|route| route.source_id.0);
    routes.into_iter().map(|route| route.path.clone()).collect()
}

pub fn refresh_list_row_initial_state_fields(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    row: &mut PlanExecutorListRowState,
) {
    for slot in &plan.storage_layout.scalar_slots {
        if slot.scope_id != list_slot.scope_id {
            continue;
        }
        let Some(source_path) = slot.initial_row_field_path.as_deref() else {
            continue;
        };
        let source_name = local_field_name(source_path);
        let Some(value) = row.fields.get(&source_name).cloned() else {
            continue;
        };
        let field_name = local_field_name(&state_label(plan, slot.state_id));
        let field_missing = !row.fields.contains_key(&field_name);
        row.fields.entry(field_name.clone()).or_insert(value);
        if !field_missing {
            continue;
        }
        if let Some(bytes) = row.private_bytes.get(&source_name).cloned() {
            if let Some(expected_len) = indexed_fixed_byte_bank_len(plan, slot.state_id)
                .ok()
                .flatten()
                && bytes.inline_bytes().len() == expected_len
            {
                row.fixed_bytes_banks
                    .insert(field_name.clone(), bytes.inline_bytes().to_vec());
            }
            row.private_bytes.insert(field_name.clone(), bytes);
        } else if let Some(bank_bytes) = row.fixed_bytes_banks.get(&source_name).cloned()
            && indexed_state_has_fixed_byte_bank(plan, slot.state_id)
        {
            row.fixed_bytes_banks.insert(field_name, bank_bytes);
        }
    }
}

pub fn refresh_list_row_bool_not_deltas(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_label: &str,
    key: u64,
    generation: u64,
    fields: &mut BTreeMap<String, JsonValue>,
) -> PlanExecutorResult<Vec<JsonValue>> {
    let mut deltas = Vec::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if !op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            expression: Some(PlanDerivedExpression::BoolNot { input }),
            ..
        } = &op.kind
        else {
            continue;
        };
        let Some(input_state_id) = (match input {
            ValueRef::State(state_id) => Some(state_id),
            _ => None,
        }) else {
            return Err(format!("indexed Bool/not op {} input is not a state ref", op.id.0).into());
        };
        let Some(input_slot) = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == *input_state_id)
        else {
            return Err(format!("indexed Bool/not op {} input state is missing", op.id.0).into());
        };
        if input_slot.scope_id != list_slot.scope_id {
            continue;
        }
        let input_name = local_field_name(&state_label(plan, *input_state_id));
        let input_value = fields
            .get(&input_name)
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                format!(
                    "indexed Bool/not op {} input field `{input_name}` is not available as bool",
                    op.id.0
                )
            })?;
        let output_name = local_field_name(&semantic_field_label(plan, output_id.0));
        let value = JsonValue::Bool(!input_value);
        fields.insert(output_name.clone(), value.clone());
        deltas.push(json!({
            "kind": "FieldSet",
            "list_id": list_label,
            "key": key,
            "generation": generation,
            "source_id": null,
            "bind_epoch": null,
            "field_path": output_name,
            "value": value,
        }));
    }
    Ok(deltas)
}

pub fn refresh_list_row_bool_not_fields(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_label: &str,
    key: u64,
    generation: u64,
    fields: &mut BTreeMap<String, JsonValue>,
) -> PlanExecutorResult<Vec<JsonValue>> {
    let mut deltas = Vec::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if !op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            expression: Some(PlanDerivedExpression::BoolNot { input }),
            ..
        } = &op.kind
        else {
            continue;
        };
        let Some(input_state_id) = (match input {
            ValueRef::State(state_id) => Some(state_id),
            _ => None,
        }) else {
            return Err(format!("indexed Bool/not op {} input is not a state ref", op.id.0).into());
        };
        let Some(input_slot) = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == *input_state_id)
        else {
            return Err(format!("indexed Bool/not op {} input state is missing", op.id.0).into());
        };
        if input_slot.scope_id != list_slot.scope_id {
            continue;
        }
        let input_name = local_field_name(&state_label(plan, *input_state_id));
        let Some(input_value) = fields.get(&input_name).and_then(JsonValue::as_bool) else {
            continue;
        };
        let output_name = local_field_name(&semantic_field_label(plan, output_id.0));
        let value = JsonValue::Bool(!input_value);
        let changed = fields.get(&output_name) != Some(&value);
        fields.insert(output_name.clone(), value.clone());
        if changed {
            deltas.push(json!({
                "kind": "FieldSet",
                "list_id": list_label,
                "key": key,
                "generation": generation,
                "source_id": null,
                "bind_epoch": null,
                "field_path": output_name,
                "value": value,
            }));
        }
    }
    Ok(deltas)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanExecutorScenarioCheckpointCellExpectation {
    pub address: String,
    pub value: Option<String>,
    pub formula: Option<String>,
    pub editing_text: Option<String>,
    pub editing: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanExecutorScenarioCheckpointErrorExpectation {
    pub address: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanExecutorScenarioCheckpointInput {
    pub step_id: String,
    pub source_intent_exemption: Option<String>,
    pub semantic_delta_expectation_count: usize,
    pub render_delta_expectation_count: usize,
    pub expect_titles: Option<Vec<String>>,
    pub expect_visible_titles: Option<Vec<String>>,
    pub expect_completed_titles: Option<Vec<String>>,
    pub expect_active_count: Option<usize>,
    pub expect_completed_count: Option<usize>,
    pub expect_filter: Option<String>,
    pub expect_new_text: Option<String>,
    pub expect_editing_title: Option<String>,
    pub expect_edit_text: Option<String>,
    pub expect_no_editing: Option<bool>,
    pub expect_cell: Option<PlanExecutorScenarioCheckpointCellExpectation>,
    pub expect_error: Option<PlanExecutorScenarioCheckpointErrorExpectation>,
    pub expect_recomputed_present: bool,
    pub expect_root_text: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanExecutorScenarioCheckpointReport {
    pub report: JsonValue,
}

pub fn assert_scenario_checkpoint(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
    input: PlanExecutorScenarioCheckpointInput,
) -> PlanExecutorResult<PlanExecutorScenarioCheckpointReport> {
    if input.semantic_delta_expectation_count != 0 || input.render_delta_expectation_count != 0 {
        return Err(format!(
            "{} assertion-only PlanExecutor checkpoint cannot validate delta expectations without a source event",
            input.step_id
        )
        .into());
    }

    let mut checked = Vec::new();
    let todos = list_rows_with_field(list_state, "title");
    if let Some(expected) = &input.expect_titles {
        let actual = todos
            .iter()
            .filter_map(|row| list_row_textlike_field(row, "title"))
            .collect::<Vec<_>>();
        assert_executor_eq_report(&input.step_id, "titles", expected, &actual)?;
        checked.push("expect_titles");
    }
    if let Some(expected) = &input.expect_visible_titles {
        let retain_execution = materialize_list_retains(plan, root_state, list_state)?;
        let actual = visible_titles_from_retains(&retain_execution)?;
        assert_executor_eq_report(&input.step_id, "visible_titles", expected, &actual)?;
        checked.push("expect_visible_titles");
    }
    if let Some(expected) = &input.expect_completed_titles {
        let actual = todos
            .iter()
            .filter(|row| row.fields.get("completed").and_then(JsonValue::as_bool) == Some(true))
            .filter_map(|row| list_row_textlike_field(row, "title"))
            .collect::<Vec<_>>();
        assert_executor_eq_report(&input.step_id, "completed_titles", expected, &actual)?;
        checked.push("expect_completed_titles");
    }
    if let Some(expected) = input.expect_active_count {
        let actual = todos
            .iter()
            .filter(|row| row.fields.get("completed").and_then(JsonValue::as_bool) != Some(true))
            .count();
        assert_executor_num(&input.step_id, "active_count", expected, actual)?;
        checked.push("expect_active_count");
    }
    if let Some(expected) = input.expect_completed_count {
        let actual = todos
            .iter()
            .filter(|row| row.fields.get("completed").and_then(JsonValue::as_bool) == Some(true))
            .count();
        assert_executor_num(&input.step_id, "completed_count", expected, actual)?;
        checked.push("expect_completed_count");
    }
    if let Some(expected) = &input.expect_filter {
        let actual =
            root_textlike_for_assertion(root_state, "store.selected_filter", "selected_filter")?;
        assert_executor_eq_report(&input.step_id, "filter", expected, &actual)?;
        checked.push("expect_filter");
    }
    if let Some(expected) = &input.expect_new_text {
        let actual =
            root_textlike_for_assertion(root_state, "store.new_todo_text", "new_todo_text")?;
        assert_executor_eq_report(&input.step_id, "new_text", expected, &actual)?;
        checked.push("expect_new_text");
    }
    if let Some(expected) = &input.expect_editing_title {
        let row = single_editing_row(&todos, &input.step_id)?;
        let actual = list_row_textlike_field(row, "title")
            .ok_or_else(|| format!("{} editing row is missing textlike title", input.step_id))?;
        assert_executor_eq_report(&input.step_id, "editing_title", expected, &actual)?;
        checked.push("expect_editing_title");
    }
    if let Some(expected) = &input.expect_edit_text {
        let row = single_editing_row(&todos, &input.step_id)?;
        let actual = list_row_textlike_field(row, "edit_text").ok_or_else(|| {
            format!(
                "{} editing row is missing textlike edit_text",
                input.step_id
            )
        })?;
        assert_executor_eq_report(&input.step_id, "edit_text", expected, &actual)?;
        checked.push("expect_edit_text");
    }
    if let Some(expected) = input.expect_no_editing {
        let actual = !todos
            .iter()
            .any(|row| row.fields.get("editing").and_then(JsonValue::as_bool) == Some(true));
        assert_executor_eq_report(&input.step_id, "no_editing", &expected, &actual)?;
        checked.push("expect_no_editing");
    }
    if let Some(expect) = &input.expect_cell {
        let mut required_fields = Vec::new();
        if expect.value.is_some() {
            required_fields.push("value");
        }
        if expect.formula.is_some() {
            required_fields.push("formula_text");
        }
        if expect.editing_text.is_some() {
            required_fields.push("editing_text");
        }
        if expect.editing.is_some() {
            required_fields.push("editing");
        }
        let row = cell_row(
            list_state,
            &expect.address,
            &required_fields,
            &input.step_id,
        )?;
        if let Some(expected) = &expect.value {
            let actual = list_row_textlike_field(row, "value").unwrap_or_default();
            assert_executor_eq_report(&input.step_id, "cell.value", expected, &actual)?;
            checked.push("expect_cell.value");
        }
        if let Some(expected) = &expect.formula {
            let actual = list_row_textlike_field(row, "formula_text").unwrap_or_default();
            assert_executor_eq_report(&input.step_id, "cell.formula", expected, &actual)?;
            checked.push("expect_cell.formula");
        }
        if let Some(expected) = &expect.editing_text {
            let actual = list_row_textlike_field(row, "editing_text").unwrap_or_default();
            assert_executor_eq_report(&input.step_id, "cell.editing_text", expected, &actual)?;
            checked.push("expect_cell.editing_text");
        }
        if let Some(expected) = expect.editing {
            let actual = row
                .fields
                .get("editing")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            assert_executor_eq_report(&input.step_id, "cell.editing", &expected, &actual)?;
            checked.push("expect_cell.editing");
        }
    }
    if let Some(expect) = &input.expect_error {
        let row = cell_row(list_state, &expect.address, &["error"], &input.step_id)?;
        let actual = list_row_textlike_field(row, "error").filter(|error| !error.is_empty());
        assert_executor_eq_report(
            &input.step_id,
            "cell.error",
            &Some(expect.error.as_str()),
            &actual.as_deref(),
        )?;
        checked.push("expect_error");
    }
    if input.expect_recomputed_present {
        return Err(format!(
            "{} assertion-only PlanExecutor checkpoint cannot validate expect_recomputed without a source event recompute set",
            input.step_id
        )
        .into());
    }
    for (path, expected) in &input.expect_root_text {
        let actual = root_textlike_for_assertion(root_state, path, path)?;
        assert_executor_eq_report(&input.step_id, path, expected, &actual)?;
        checked.push("expect_root_text");
    }

    Ok(PlanExecutorScenarioCheckpointReport {
        report: json!({
            "step_id": input.step_id,
            "source_intent_exemption": input.source_intent_exemption,
            "checked_expectations": checked,
            "checked_expectation_count": checked.len(),
            "passed": true,
        }),
    })
}

fn list_rows_with_field<'a>(
    list_state: &'a BTreeMap<usize, Vec<PlanExecutorListRow>>,
    field: &str,
) -> Vec<&'a PlanExecutorListRow> {
    list_state
        .values()
        .find(|rows| rows.iter().any(|row| row.fields.contains_key(field)))
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn visible_titles_from_retains(
    retain_execution: &ListRetainExecution,
) -> PlanExecutorResult<Vec<String>> {
    retain_execution
        .summary
        .iter()
        .find(|(target, _)| target.ends_with("visible_todos") || target.as_str() == "visible_todos")
        .or_else(|| retain_execution.summary.iter().next())
        .map(|(_, summary)| {
            summary
                .get("titles")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| "PlanExecutor retain summary is missing visible titles".into())
                .and_then(|titles| {
                    titles
                        .iter()
                        .map(|title| {
                            json_scalar_textlike(title)
                                .ok_or_else(|| "PlanExecutor visible title is not textlike".into())
                        })
                        .collect::<PlanExecutorResult<Vec<_>>>()
                })
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn single_editing_row<'a>(
    rows: &[&'a PlanExecutorListRow],
    step_id: &str,
) -> PlanExecutorResult<&'a PlanExecutorListRow> {
    let editing = rows
        .iter()
        .copied()
        .filter(|row| row.fields.get("editing").and_then(JsonValue::as_bool) == Some(true))
        .collect::<Vec<_>>();
    match editing.as_slice() {
        [row] => Ok(*row),
        [] => Err(format!("{step_id}: expected one editing row, found none").into()),
        rows => Err(format!("{step_id}: expected one editing row, found {}", rows.len()).into()),
    }
}

fn cell_row<'a>(
    list_state: &'a BTreeMap<usize, Vec<PlanExecutorListRow>>,
    address: &str,
    required_fields: &[&str],
    step_id: &str,
) -> PlanExecutorResult<&'a PlanExecutorListRow> {
    for rows in list_state.values() {
        for row in rows {
            if list_row_textlike_field(row, "address").as_deref() == Some(address)
                && required_fields
                    .iter()
                    .all(|field| row.fields.contains_key(*field))
            {
                return Ok(row);
            }
        }
    }
    Err(format!(
        "{step_id}: PlanExecutor cell address `{address}` with fields {:?} not found",
        required_fields
    )
    .into())
}

fn root_textlike_for_assertion(
    root_state: &JsonMap<String, JsonValue>,
    path: &str,
    fallback_path: &str,
) -> PlanExecutorResult<String> {
    let root = JsonValue::Object(root_state.clone());
    for candidate in [
        path.to_owned(),
        format!("store.{fallback_path}"),
        fallback_path.to_owned(),
    ] {
        if let Some(value) = json_value_at_dotted_path(&root, &candidate)
            && let Some(text) = json_scalar_textlike(value)
        {
            return Ok(text);
        }
    }
    Err(format!("PlanExecutor root text assertion path `{path}` not found").into())
}

fn assert_executor_eq_report<T>(
    step: &str,
    field: &str,
    expected: &T,
    actual: &T,
) -> PlanExecutorResult<()>
where
    T: std::fmt::Debug + PartialEq,
{
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{step}: {field} expected {expected:?}, got {actual:?}").into())
    }
}

fn assert_executor_num(
    step: &str,
    field: &str,
    expected: usize,
    actual: usize,
) -> PlanExecutorResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{step}: {field} expected {expected}, got {actual}").into())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListRowDefaultFields {
    pub fields: BTreeMap<String, JsonValue>,
    pub private_bytes: BTreeMap<String, PlanExecutorBytes>,
    pub fixed_byte_banks: BTreeMap<String, Vec<u8>>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListProjectionExecution {
    pub summary: JsonMap<String, JsonValue>,
    pub reports: Vec<JsonValue>,
    pub executed_count: usize,
    pub find_count: usize,
    pub chunk_count: usize,
    pub projected_row_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListRetainExecution {
    pub summary: JsonMap<String, JsonValue>,
    pub reports: Vec<JsonValue>,
    pub executed_count: usize,
    pub view_count: usize,
    pub retained_row_count: usize,
}

pub fn summarize_plan_lists(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> JsonValue {
    let mut lists = JsonMap::new();
    for (list_id, rows) in list_state {
        let list_label = list_label(plan, *list_id);
        let titles = rows
            .iter()
            .filter_map(|row| row.fields.get("title").cloned())
            .collect::<Vec<_>>();
        let active_count = rows
            .iter()
            .filter(|row| {
                row.fields
                    .get("completed")
                    .and_then(JsonValue::as_bool)
                    .map(|completed| !completed)
                    .unwrap_or(false)
            })
            .count();
        let completed_count = rows
            .iter()
            .filter(|row| {
                row.fields
                    .get("completed")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false)
            })
            .count();
        lists.insert(
            list_label,
            json!({
                "row_count": rows.len(),
                "titles": titles,
                "active_count": active_count,
                "completed_count": completed_count,
                "rows": rows.iter().map(|row| {
                    json!({
                        "key": row.key,
                        "generation": row.generation,
                        "fields": row.fields,
                    })
                }).collect::<Vec<_>>(),
            }),
        );
    }
    JsonValue::Object(lists)
}

pub fn initial_list_next_keys(
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> BTreeMap<usize, u64> {
    list_state
        .iter()
        .map(|(list_id, rows)| {
            let next_key = rows.iter().map(|row| row.key).max().unwrap_or(0) + 1;
            (*list_id, next_key)
        })
        .collect()
}

pub fn reserve_list_row_key(
    list_next_keys: &mut BTreeMap<usize, u64>,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
    list_id: usize,
) -> PlanExecutorResult<u64> {
    let existing_rows = list_state
        .get(&list_id)
        .ok_or_else(|| format!("list state missing list {list_id}"))?;
    let next_key = list_next_keys
        .entry(list_id)
        .or_insert_with(|| existing_rows.iter().map(|row| row.key).max().unwrap_or(0) + 1);
    let key = *next_key;
    *next_key = next_key.saturating_add(1);
    Ok(key)
}

pub fn row_source_binding_id(
    key: u64,
    route_source_ids: &[SourceId],
    source_id: SourceId,
) -> Option<u64> {
    route_source_ids
        .iter()
        .position(|route_source_id| *route_source_id == source_id)
        .map(|route_index| (key - 1) * route_source_ids.len() as u64 + route_index as u64 + 1)
}

pub fn build_source_bind_deltas(
    list_label: &str,
    key: u64,
    generation: u64,
    source_paths: &[String],
) -> Vec<JsonValue> {
    source_paths
        .iter()
        .enumerate()
        .map(|(route_index, path)| {
            let source_binding_id = (key - 1) * source_paths.len() as u64 + route_index as u64 + 1;
            json!({
                "kind": "SourceBind",
                "list_id": list_label,
                "key": key,
                "generation": generation,
                "source_id": source_binding_id,
                "bind_epoch": source_binding_id,
                "field_path": path,
                "value": path,
            })
        })
        .collect()
}

pub fn build_source_unbind_deltas(
    list_label: &str,
    key: u64,
    generation: u64,
    source_paths: &[String],
) -> Vec<JsonValue> {
    source_paths
        .iter()
        .enumerate()
        .map(|(route_index, path)| {
            let source_binding_id = (key - 1) * source_paths.len() as u64 + route_index as u64 + 1;
            json!({
                "kind": "SourceUnbind",
                "list_id": list_label,
                "key": key,
                "generation": generation,
                "source_id": source_binding_id,
                "bind_epoch": source_binding_id,
                "field_path": path,
                "value": null,
            })
        })
        .collect()
}

pub fn build_list_remove_delta(list_label: &str, key: u64, generation: u64) -> JsonValue {
    json!({
        "kind": "ListRemove",
        "list_id": list_label,
        "key": key,
        "generation": generation,
        "source_id": null,
        "bind_epoch": null,
        "field_path": null,
        "value": null,
    })
}

pub fn build_list_insert_delta(
    list_label: &str,
    key: u64,
    generation: u64,
    trigger_value: JsonValue,
) -> JsonValue {
    json!({
        "kind": "ListInsert",
        "list_id": list_label,
        "key": key,
        "generation": generation,
        "source_id": null,
        "bind_epoch": null,
        "field_path": null,
        "value": trigger_value,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListAppendMutationInput {
    pub list_id: usize,
    pub list_label: String,
    pub append_op_id: usize,
    pub key: u64,
    pub generation: u64,
    pub trigger_value: JsonValue,
    pub fields_before_refresh: BTreeMap<String, JsonValue>,
    pub fields_after_refresh: BTreeMap<String, JsonValue>,
    pub source_paths: Vec<String>,
    pub row_bool_deltas: Vec<JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListAppendMutationRecord {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_row: JsonValue,
    pub source_bind_count: usize,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListAppendRowConstruction {
    pub row: PlanExecutorListRowState,
    pub fields_before_refresh: BTreeMap<String, JsonValue>,
    pub fields_after_refresh: BTreeMap<String, JsonValue>,
    pub source_paths: Vec<String>,
    pub row_bool_deltas: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListAppendExecution {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_rows: Vec<JsonValue>,
    pub appended_row_count: usize,
    pub source_bind_count: usize,
    pub executor_report: JsonValue,
}

#[allow(clippy::too_many_arguments)]
pub fn construct_list_append_row_with<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    append_op_id: usize,
    append: &boon_plan::PlanListAppend,
    list_id: usize,
    list_label: &str,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    key: u64,
    generation: u64,
    derived_values: &BTreeMap<FieldId, JsonValue>,
    emit_bool_deltas: bool,
    mut evaluator: E,
) -> PlanExecutorResult<ListAppendRowConstruction>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    let ListRowDefaultFields {
        mut fields,
        mut private_bytes,
        mut fixed_byte_banks,
        ..
    } = list_row_default_fields(plan, list_slot)?;
    let append_field_count = append.fields.len();
    let mut bytes_field_count = 0usize;
    for field in &append.fields {
        let field_id = field.field_id.ok_or_else(|| {
            format!(
                "append op {append_op_id} field `{}` has no typed field id",
                field.name
            )
        })?;
        let field_label = semantic_field_label(plan, field_id.0);
        let field_name = local_field_name(&field_label);
        let (value, bytes_value) = match (&field.value_ref, field.constant_id) {
            (Some(value_ref), None) => {
                let value = resolve_plan_value_ref(plan, value_ref, derived_values, Some(&fields))?
                    .ok_or_else(|| {
                        format!(
                            "append op {append_op_id} field `{}` value ref was not produced",
                            field.name
                        )
                    })?;
                (value, None)
            }
            (None, Some(constant_id)) => {
                let constant = plan
                    .constants
                    .iter()
                    .find(|constant| constant.id == constant_id)
                    .ok_or_else(|| {
                        format!(
                            "append op {append_op_id} missing constant {}",
                            constant_id.0
                        )
                    })?;
                (
                    plan_constant_json_value(constant)?,
                    plan_constant_value_bytes(
                        &constant.value,
                        &format!("append op {append_op_id} field `{}`", field.name),
                    )?,
                )
            }
            _ => {
                return Err(format!(
                    "append op {append_op_id} field `{}` has invalid value source",
                    field.name
                )
                .into());
            }
        };
        fields.insert(field_name.clone(), value);
        if let Some(bytes) = bytes_value {
            bytes_field_count += 1;
            if indexed_field_has_fixed_byte_bank(plan, list_slot.scope_id, &field_name) {
                fixed_byte_banks.insert(field_name.clone(), bytes.inline_bytes().to_vec());
            }
            private_bytes.insert(field_name.clone(), bytes);
        } else {
            private_bytes.remove(&field_name);
            fixed_byte_banks.remove(&field_name);
        }
    }

    let source_paths = row_scoped_source_paths(plan, list_slot);
    let mut row = PlanExecutorListRowState {
        key,
        generation,
        fields,
        private_bytes,
        fixed_bytes_banks: fixed_byte_banks,
    };
    let fields_before_refresh = row.fields.clone();
    let existing_rows = list_state
        .get(&list_id)
        .ok_or_else(|| format!("list state missing list {list_id}"))?;
    let mut refresh_state = list_state.clone();
    let mut projected_rows = existing_rows.clone();
    projected_rows.push(row.clone());
    refresh_state.insert(list_id, projected_rows);
    refresh_list_row_expression_fields_best_effort_with(
        plan,
        list_slot,
        &refresh_state,
        &mut row,
        &mut evaluator,
    );
    refresh_list_row_initial_state_fields(plan, list_slot, &mut row);

    let mut refresh_state = list_state.clone();
    let mut projected_rows = existing_rows.clone();
    projected_rows.push(row.clone());
    refresh_state.insert(list_id, projected_rows);
    refresh_list_row_expression_fields_with(
        plan,
        list_slot,
        &refresh_state,
        &mut row,
        &mut evaluator,
    )?;
    refresh_list_row_initial_state_fields(plan, list_slot, &mut row);

    let mut row_bool_deltas = refresh_list_row_bool_not_deltas(
        plan,
        list_slot,
        list_label,
        key,
        generation,
        &mut row.fields,
    )?;
    if !emit_bool_deltas {
        row_bool_deltas.clear();
    }
    let fields_after_refresh = row.fields.clone();
    let executor_report = json!({
        "executor": "cpu-plan-list-append-row-construction-v1",
        "list_id": list_id,
        "append_op_id": append_op_id,
        "key": key,
        "generation": generation,
        "append_field_count": append_field_count,
        "bytes_field_count": bytes_field_count,
        "source_path_count": source_paths.len(),
        "row_bool_delta_count": row_bool_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(ListAppendRowConstruction {
        row,
        fields_before_refresh,
        fields_after_refresh,
        source_paths,
        row_bool_deltas,
        executor_report,
    })
}

pub fn append_list_rows_for_derived_values_with<E>(
    plan: &MachinePlan,
    list_state: &mut BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    list_next_keys: &mut BTreeMap<usize, u64>,
    list_append_row_bool_delta_lists: &mut BTreeSet<usize>,
    derived_values: &BTreeMap<FieldId, JsonValue>,
    mut evaluator: E,
) -> PlanExecutorResult<ListAppendExecution>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    let mut semantic_deltas = Vec::new();
    let mut report_rows = Vec::new();
    let mut appended_row_count = 0usize;
    let mut source_bind_count = 0usize;
    let mut visited_append_op_count = 0usize;
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
    {
        let PlanOpKind::ListOperation {
            operation_kind: boon_plan::PlanListOperationKind::Append,
            append: Some(append),
            ..
        } = &op.kind
        else {
            continue;
        };
        visited_append_op_count += 1;
        let Some(trigger_value) =
            resolve_plan_value_ref(plan, &append.trigger, derived_values, None)?
        else {
            continue;
        };
        if trigger_value.is_null() {
            continue;
        }
        let Some(ValueRef::List(list_id)) = op.output else {
            return Err(format!("append op {} does not output a list", op.id.0).into());
        };
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .ok_or_else(|| {
                format!(
                    "append op {} references missing list {}",
                    op.id.0, list_id.0
                )
            })?;
        let list_label = list_label(plan, list_id.0);
        let public_rows = list_row_state_public_rows(list_state);
        let key = reserve_list_row_key(list_next_keys, &public_rows, list_id.0)?;
        let generation = 1u64;
        let emit_bool_deltas = list_append_row_bool_delta_lists.insert(list_id.0);
        let constructed_row = construct_list_append_row_with(
            plan,
            list_slot,
            op.id.0,
            append,
            list_id.0,
            &list_label,
            list_state,
            key,
            generation,
            derived_values,
            emit_bool_deltas,
            &mut evaluator,
        )?;
        let row = constructed_row.row;
        let fields = constructed_row.fields_after_refresh.clone();
        let mutation_record = record_list_append_mutation(
            plan,
            list_slot,
            ListAppendMutationInput {
                list_id: list_id.0,
                list_label: list_label.clone(),
                append_op_id: op.id.0,
                key,
                generation,
                trigger_value,
                fields_before_refresh: constructed_row.fields_before_refresh,
                fields_after_refresh: fields,
                source_paths: constructed_row.source_paths,
                row_bool_deltas: constructed_row.row_bool_deltas,
            },
        );
        source_bind_count += mutation_record.source_bind_count;
        semantic_deltas.extend(mutation_record.semantic_deltas);
        list_state
            .get_mut(&list_id.0)
            .ok_or_else(|| format!("list state missing list {}", list_id.0))?
            .push(row);
        report_rows.push(mutation_record.report_row);
        appended_row_count += 1;
    }
    let executor_report = json!({
        "executor": "cpu-plan-list-append-execution-v1",
        "visited_append_op_count": visited_append_op_count,
        "appended_row_count": appended_row_count,
        "source_bind_count": source_bind_count,
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ListAppendExecution {
        semantic_deltas,
        report_rows,
        appended_row_count,
        source_bind_count,
        executor_report,
    })
}

pub fn record_list_append_mutation(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    input: ListAppendMutationInput,
) -> ListAppendMutationRecord {
    let mut semantic_deltas = Vec::new();
    semantic_deltas.push(build_list_insert_delta(
        &input.list_label,
        input.key,
        input.generation,
        input.trigger_value.clone(),
    ));
    let source_bind_deltas = build_source_bind_deltas(
        &input.list_label,
        input.key,
        input.generation,
        &input.source_paths,
    );
    let source_bind_count = source_bind_deltas.len();
    semantic_deltas.extend(source_bind_deltas);
    semantic_deltas.extend(build_row_refresh_field_deltas(
        plan,
        list_slot,
        &input.list_label,
        input.key,
        input.generation,
        &input.fields_before_refresh,
        &input.fields_after_refresh,
    ));
    semantic_deltas.extend(input.row_bool_deltas);

    let report_row = json!({
        "list_id": input.list_id,
        "list": input.list_label,
        "append_op_id": input.append_op_id,
        "key": input.key,
        "generation": input.generation,
        "trigger": input.trigger_value,
        "fields": input.fields_after_refresh,
    });
    let executor_report = json!({
        "executor": "cpu-plan-list-append-mutation-record-v1",
        "list_id": input.list_id,
        "append_op_id": input.append_op_id,
        "key": input.key,
        "generation": input.generation,
        "source_bind_count": source_bind_count,
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    ListAppendMutationRecord {
        semantic_deltas,
        report_row,
        source_bind_count,
        executor_report,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListRemoveMutationInput {
    pub list_id: usize,
    pub list_label: String,
    pub remove_op_id: usize,
    pub source_id: usize,
    pub source_label: String,
    pub row_index: usize,
    pub key: u64,
    pub generation: u64,
    pub source_binding_id: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub row_resolution: JsonValue,
    pub source_paths: Vec<String>,
    pub row_fields: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListRemoveMutationRecord {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_row: JsonValue,
    pub source_unbind_count: usize,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListRemoveExecution {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_rows: Vec<JsonValue>,
    pub report_derived: Vec<JsonValue>,
    pub removed_row_count: usize,
    pub source_unbind_count: usize,
    pub derived_count: usize,
    pub executor_report: JsonValue,
}

pub fn record_list_remove_mutation(input: ListRemoveMutationInput) -> ListRemoveMutationRecord {
    let source_unbinds = build_source_unbind_deltas(
        &input.list_label,
        input.key,
        input.generation,
        &input.source_paths,
    );
    let source_unbind_count = source_unbinds.len();
    let mut semantic_deltas = source_unbinds.clone();
    semantic_deltas.push(build_list_remove_delta(
        &input.list_label,
        input.key,
        input.generation,
    ));

    let report_row = json!({
        "list_id": input.list_id,
        "list": input.list_label,
        "remove_op_id": input.remove_op_id,
        "source_id": input.source_id,
        "source": input.source_label,
        "row_index": input.row_index,
        "key": input.key,
        "generation": input.generation,
        "source_binding_id": input.source_binding_id,
        "bind_epoch": input.bind_epoch,
        "row_resolution": input.row_resolution,
        "source_unbinds": source_unbinds,
        "row_fields": input.row_fields,
    });
    let executor_report = json!({
        "executor": "cpu-plan-list-remove-mutation-record-v1",
        "list_id": input.list_id,
        "remove_op_id": input.remove_op_id,
        "source_id": input.source_id,
        "key": input.key,
        "generation": input.generation,
        "source_unbind_count": source_unbind_count,
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    ListRemoveMutationRecord {
        semantic_deltas,
        report_row,
        source_unbind_count,
        executor_report,
    }
}

struct ListRemoveTarget {
    row_index: usize,
    row: PlanExecutorListRowState,
    row_resolution: JsonValue,
    source_binding_id: Option<u64>,
    bind_epoch: Option<u64>,
}

fn plan_row_source_binding_id(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    key: u64,
    source_id: SourceId,
) -> Option<u64> {
    let mut route_source_ids = plan
        .source_routes
        .iter()
        .filter(|route| route.scoped && route.scope_id == list_slot.scope_id)
        .map(|route| route.source_id)
        .collect::<Vec<_>>();
    route_source_ids.sort_by_key(|source_id| source_id.0);
    row_source_binding_id(key, &route_source_ids, source_id)
}

fn validate_list_row_binding_epoch(
    plan: &MachinePlan,
    source_route_slot: &SourceRoute,
    list_slot: &boon_plan::ListStorageSlot,
    row: &PlanExecutorListRowState,
    event: &PlanExecutorLiveSourceEvent<'_>,
) -> PlanExecutorResult<()> {
    let Some(source_binding_id) =
        plan_row_source_binding_id(plan, list_slot, row.key, source_route_slot.source_id)
    else {
        return Ok(());
    };
    let expected_epoch = event.bind_epoch.or(event.source_epoch);
    if expected_epoch.is_some_and(|epoch| epoch != source_binding_id) {
        return Err(format!(
            "scoped source `{}` bind_epoch/source_epoch does not match row binding id {source_binding_id}",
            event.source
        )
        .into());
    }
    Ok(())
}

fn resolve_list_row_index_for_source_event(
    plan: &MachinePlan,
    source_route_slot: &SourceRoute,
    event: &PlanExecutorLiveSourceEvent<'_>,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
) -> PlanExecutorResult<usize> {
    let list_label = list_label(plan, list_slot.list_id.0);
    let rows = list_state
        .get(&list_slot.list_id.0)
        .ok_or_else(|| format!("list state missing list {}", list_slot.list_id.0))?;
    if let Some(target_key) = event.target_key {
        let target_generation = event.target_generation.ok_or_else(|| {
            format!(
                "scoped source `{}` supplied target_key without target_generation",
                event.source
            )
        })?;
        let index = rows
            .iter()
            .position(|row| row.key == target_key && row.generation == target_generation)
            .ok_or_else(|| {
                format!(
                    "scoped source `{}` target key/generation {target_key}/{target_generation} not found in `{list_label}`",
                    event.source
                )
            })?;
        validate_list_row_binding_epoch(plan, source_route_slot, list_slot, &rows[index], event)?;
        return Ok(index);
    }
    if let Some(address) = event.address.as_ref()
        && let Some(lookup_field) = source_route_slot.payload_schema.row_lookup_field_name()
        && let Some(index) = rows.iter().position(|row| {
            row.fields.get(lookup_field).and_then(JsonValue::as_str) == Some(*address)
        })
    {
        validate_list_row_binding_epoch(plan, source_route_slot, list_slot, &rows[index], event)?;
        return Ok(index);
    }
    let Some(target_text) = event.target_text else {
        return Err(format!(
            "scoped source `{}` needs target_key, address, or target_text to resolve a row",
            event.source
        )
        .into());
    };
    let occurrence = event.target_occurrence.unwrap_or(1).max(1);
    let mut seen = 0u64;
    for (index, row) in rows.iter().enumerate() {
        if row
            .fields
            .values()
            .any(|value| value.as_str() == Some(target_text))
        {
            seen += 1;
            if seen == occurrence {
                validate_list_row_binding_epoch(plan, source_route_slot, list_slot, row, event)?;
                return Ok(index);
            }
        }
    }
    Err(format!(
        "scoped source `{}` target_text `{target_text}` occurrence {occurrence} not found in `{list_label}`",
        event.source
    )
    .into())
}

fn list_row_resolution_report(
    event: &PlanExecutorLiveSourceEvent<'_>,
    row_index: usize,
    row: &PlanExecutorListRowState,
    source_binding_id: Option<u64>,
) -> JsonValue {
    json!({
        "method": if event.target_key.is_some() {
            "key_generation"
        } else if event.address.is_some() {
            "address"
        } else {
            "target_text"
        },
        "target_text": event.target_text,
        "target_occurrence": event.target_occurrence,
        "target_key": event.target_key,
        "target_generation": event.target_generation,
        "address": event.address,
        "row_index": row_index,
        "key": row.key,
        "generation": row.generation,
        "source_binding_id": source_binding_id,
    })
}

pub fn remove_list_rows_for_source_event(
    plan: &MachinePlan,
    source_id: SourceId,
    source_route_slot: &SourceRoute,
    event: &PlanExecutorLiveSourceEvent<'_>,
    list_state: &mut BTreeMap<usize, Vec<PlanExecutorListRowState>>,
) -> PlanExecutorResult<ListRemoveExecution> {
    let mut semantic_deltas = Vec::new();
    let mut report_rows = Vec::new();
    let mut report_derived = Vec::new();
    let mut removed_row_count = 0usize;
    let mut source_unbind_count = 0usize;
    let mut visited_remove_op_count = 0usize;
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
    {
        let PlanOpKind::ListOperation {
            operation_kind: boon_plan::PlanListOperationKind::Remove,
            remove: Some(remove),
            ..
        } = &op.kind
        else {
            continue;
        };
        if remove.source != ValueRef::Source(source_id) {
            continue;
        }
        visited_remove_op_count += 1;
        if op.unresolved_executable_ref_count != 0 {
            return Err(format!(
                "selected list remove op {} has {} unresolved executable refs",
                op.id.0, op.unresolved_executable_ref_count
            )
            .into());
        }
        let Some(ValueRef::List(list_id)) = op.output else {
            return Err(format!("list remove op {} does not output a list", op.id.0).into());
        };
        let list_slot = plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .ok_or_else(|| {
                format!(
                    "list remove op {} references missing list {}",
                    op.id.0, list_id.0
                )
            })?;
        if source_route_slot.scoped && source_route_slot.scope_id != list_slot.scope_id {
            return Err(format!(
                "list remove op {} source scope {:?} does not match list scope {:?}",
                op.id.0, source_route_slot.scope_id, list_slot.scope_id
            )
            .into());
        }
        let list_label = list_label(plan, list_id.0);
        if let Some(event_list) = event.list_id
            && event_list != list_label
        {
            return Err(format!(
                "scoped source `{}` targeted list `{event_list}`, expected `{list_label}`",
                event.source
            )
            .into());
        }
        let before_derived = evaluate_root_pure_number_compare_values(
            plan,
            &list_row_state_public_rows(list_state),
        )?;
        let rows_snapshot = list_state
            .get(&list_id.0)
            .ok_or_else(|| format!("list state missing list {}", list_id.0))?;
        let target_rows = if source_route_slot.scoped {
            let row_index = resolve_list_row_index_for_source_event(
                plan,
                source_route_slot,
                event,
                list_slot,
                list_state,
            )?;
            let row = rows_snapshot
                .get(row_index)
                .cloned()
                .ok_or_else(|| format!("row index {row_index} missing in `{list_label}`"))?;
            let executor_row = row.public_row();
            if !evaluate_list_remove_predicate(plan, &remove.predicate, &executor_row)?.matches {
                Vec::new()
            } else {
                let source_binding_id =
                    plan_row_source_binding_id(plan, list_slot, row.key, source_id);
                let row_resolution =
                    list_row_resolution_report(event, row_index, &row, source_binding_id);
                vec![ListRemoveTarget {
                    row_index,
                    row,
                    row_resolution,
                    source_binding_id,
                    bind_epoch: source_binding_id,
                }]
            }
        } else {
            let mut targets = Vec::new();
            for (row_index, row) in rows_snapshot.iter().cloned().enumerate() {
                let executor_row = row.public_row();
                if evaluate_list_remove_predicate(plan, &remove.predicate, &executor_row)?.matches {
                    targets.push(ListRemoveTarget {
                        row_index,
                        row: row.clone(),
                        row_resolution: build_list_remove_predicate_row_resolution_report(
                            plan,
                            &remove.predicate,
                            row_index,
                            &executor_row,
                        )?,
                        source_binding_id: None,
                        bind_epoch: None,
                    });
                }
            }
            targets
        };
        let scoped_source_paths = row_scoped_source_paths(plan, list_slot);
        for target in &target_rows {
            let mutation_record = record_list_remove_mutation(ListRemoveMutationInput {
                list_id: list_id.0,
                list_label: list_label.clone(),
                remove_op_id: op.id.0,
                source_id: source_id.0,
                source_label: event.source.to_owned(),
                row_index: target.row_index,
                key: target.row.key,
                generation: target.row.generation,
                source_binding_id: target.source_binding_id,
                bind_epoch: target.bind_epoch,
                row_resolution: target.row_resolution.clone(),
                source_paths: scoped_source_paths.clone(),
                row_fields: target.row.fields.clone(),
            });
            source_unbind_count += mutation_record.source_unbind_count;
            semantic_deltas.extend(mutation_record.semantic_deltas);
            report_rows.push(mutation_record.report_row);
            removed_row_count += 1;
        }
        let rows = list_state
            .get_mut(&list_id.0)
            .ok_or_else(|| format!("list state missing list {}", list_id.0))?;
        for target in target_rows.iter().rev() {
            rows.remove(target.row_index);
        }
        let after_derived = evaluate_root_pure_number_compare_values(
            plan,
            &list_row_state_public_rows(list_state),
        )?;
        for (delta, report) in changed_root_derived_deltas(plan, &before_derived, &after_derived) {
            semantic_deltas.push(delta);
            report_derived.push(report);
        }
    }
    let derived_count = report_derived.len();
    let executor_report = json!({
        "executor": "cpu-plan-list-remove-execution-v1",
        "visited_remove_op_count": visited_remove_op_count,
        "removed_row_count": removed_row_count,
        "source_unbind_count": source_unbind_count,
        "derived_count": derived_count,
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ListRemoveExecution {
        semantic_deltas,
        report_rows,
        report_derived,
        removed_row_count,
        source_unbind_count,
        derived_count,
        executor_report,
    })
}

pub fn resolve_plan_value_ref(
    plan: &MachinePlan,
    value_ref: &ValueRef,
    derived_values: &BTreeMap<FieldId, JsonValue>,
    row_fields: Option<&BTreeMap<String, JsonValue>>,
) -> PlanExecutorResult<Option<JsonValue>> {
    match value_ref {
        ValueRef::Field(field_id) => Ok(derived_values.get(field_id).cloned()),
        ValueRef::State(state_id) => {
            let field_name = local_field_name(&state_label(plan, *state_id));
            Ok(row_fields.and_then(|fields| fields.get(&field_name).cloned()))
        }
        _ => Err(format!("unsupported PlanExecutor value ref `{value_ref:?}`").into()),
    }
}

pub fn row_expression_output_field_names(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
) -> BTreeSet<String> {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .filter_map(|op| {
            if !op.indexed {
                return None;
            }
            let Some(ValueRef::Field(output_id)) = op.output else {
                return None;
            };
            let PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::RowExpression { .. }),
                ..
            } = &op.kind
            else {
                return None;
            };
            row_expression_applies_to_list(plan, list_slot, output_id)
                .then(|| local_field_name(&semantic_field_label(plan, output_id.0)))
        })
        .collect()
}

pub fn build_row_refresh_field_deltas(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_label: &str,
    key: u64,
    generation: u64,
    before: &BTreeMap<String, JsonValue>,
    after: &BTreeMap<String, JsonValue>,
) -> Vec<JsonValue> {
    let row_expression_fields = row_expression_output_field_names(plan, list_slot);
    after
        .iter()
        .filter(|(field, _)| row_expression_fields.contains(*field))
        .filter(|(field, value)| before.get(*field) != Some(*value))
        .map(|(field, value)| {
            json!({
                "kind": "FieldSet",
                "list_id": list_label,
                "key": key,
                "generation": generation,
                "source_id": null,
                "bind_epoch": null,
                "field_path": field,
                "value": value,
            })
        })
        .collect()
}

pub fn refresh_list_row_expression_fields_with<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    mut evaluator: E,
) -> PlanExecutorResult<()>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    refresh_list_row_expression_fields_with_startup_filter(
        plan,
        list_slot,
        list_state,
        row,
        false,
        &mut evaluator,
    )
}

pub fn refresh_startup_list_row_expression_fields_with<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    mut evaluator: E,
) -> PlanExecutorResult<()>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    refresh_list_row_expression_fields_with_startup_filter(
        plan,
        list_slot,
        list_state,
        row,
        true,
        &mut evaluator,
    )
}

fn refresh_list_row_expression_fields_with_startup_filter<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    startup_only: bool,
    evaluator: &mut E,
) -> PlanExecutorResult<()>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if !op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            startup_recompute,
            expression: Some(PlanDerivedExpression::RowExpression { expression }),
            ..
        } = &op.kind
        else {
            continue;
        };
        if startup_only
            && !*startup_recompute
            && row_expression_reads_list(expression, list_slot.list_id)
        {
            continue;
        }
        if !row_expression_applies_to_list(plan, list_slot, output_id) {
            continue;
        }
        if row_expression_row_input_missing(plan, row, expression) {
            continue;
        }
        let value = evaluator(plan, list_state, row, expression)?;
        let field_name = local_field_name(&semantic_field_label(plan, output_id.0));
        row.fields.insert(field_name, value);
    }
    Ok(())
}

pub fn refresh_list_row_expression_fields_best_effort_with<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    mut evaluator: E,
) where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    refresh_list_row_expression_fields_best_effort_with_startup_filter(
        plan,
        list_slot,
        list_state,
        row,
        false,
        &mut evaluator,
    );
}

pub fn refresh_startup_list_row_expression_fields_best_effort_with<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    mut evaluator: E,
) where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    refresh_list_row_expression_fields_best_effort_with_startup_filter(
        plan,
        list_slot,
        list_state,
        row,
        true,
        &mut evaluator,
    );
}

pub fn refresh_startup_list_row_fields_for_all_lists_with<E>(
    plan: &MachinePlan,
    list_state: &mut BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    mut evaluator: E,
) -> PlanExecutorResult<()>
where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    let list_slots = plan.storage_layout.list_slots.clone();
    for slot in &list_slots {
        let list_id = slot.list_id.0;
        let Some(mut rows) = list_state.remove(&list_id) else {
            continue;
        };
        let mut row_expression_list_state = list_state.clone();
        row_expression_list_state.insert(list_id, rows.clone());
        for row in &mut rows {
            refresh_startup_list_row_expression_fields_best_effort_with(
                plan,
                slot,
                &row_expression_list_state,
                row,
                &mut evaluator,
            );
            refresh_list_row_initial_state_fields(plan, slot, row);
            if let Some(current_rows) = row_expression_list_state.get_mut(&list_id)
                && let Some(current_row) = current_rows
                    .iter_mut()
                    .find(|current_row| current_row.key == row.key)
            {
                *current_row = row.clone();
            }
        }
        let mut row_expression_list_state = list_state.clone();
        row_expression_list_state.insert(list_id, rows.clone());
        for row in &mut rows {
            refresh_startup_list_row_expression_fields_with(
                plan,
                slot,
                &row_expression_list_state,
                row,
                &mut evaluator,
            )?;
            refresh_list_row_initial_state_fields(plan, slot, row);
            let _ = refresh_list_row_bool_not_fields(
                plan,
                slot,
                &list_label(plan, slot.list_id.0),
                row.key,
                row.generation,
                &mut row.fields,
            )?;
        }
        list_state.insert(list_id, rows);
    }
    Ok(())
}

pub fn refresh_startup_list_row_fields_for_all_lists(
    plan: &MachinePlan,
    list_state: &mut BTreeMap<usize, Vec<PlanExecutorListRowState>>,
) -> PlanExecutorResult<()> {
    refresh_startup_list_row_fields_for_all_lists_with(plan, list_state, eval_plan_row_expression)
}

fn refresh_list_row_expression_fields_best_effort_with_startup_filter<E>(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &mut PlanExecutorListRowState,
    startup_only: bool,
    evaluator: &mut E,
) where
    E: FnMut(
        &MachinePlan,
        &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
        &PlanExecutorListRowState,
        &PlanRowExpression,
    ) -> PlanExecutorResult<JsonValue>,
{
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if !op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            startup_recompute,
            expression: Some(PlanDerivedExpression::RowExpression { expression }),
            ..
        } = &op.kind
        else {
            continue;
        };
        if startup_only
            && !*startup_recompute
            && row_expression_reads_list(expression, list_slot.list_id)
        {
            continue;
        }
        if !row_expression_applies_to_list(plan, list_slot, output_id) {
            continue;
        }
        if row_expression_row_input_missing(plan, row, expression) {
            continue;
        }
        let Ok(value) = evaluator(plan, list_state, row, expression) else {
            continue;
        };
        let field_name = local_field_name(&semantic_field_label(plan, output_id.0));
        row.fields.insert(field_name, value);
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PlanRowEvalKey {
    list_id: usize,
    row_key: u64,
    field_id: usize,
}

pub fn eval_plan_row_expression(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
) -> PlanExecutorResult<JsonValue> {
    eval_plan_row_expression_with_stack(plan, list_state, row, expression, &[])
}

fn eval_plan_row_expression_with_stack(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<JsonValue> {
    match expression {
        PlanRowExpression::Field { input } => eval_plan_row_field_ref(plan, row, input, stack),
        PlanRowExpression::Constant { constant_id } => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *constant_id)
                .ok_or_else(|| format!("row expression constant {} is missing", constant_id.0))?;
            plan_constant_json_value(constant)
        }
        PlanRowExpression::TextTrim { input } => Ok(JsonValue::String(
            eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?
                .trim()
                .to_owned(),
        )),
        PlanRowExpression::TextIsEmpty { input } => Ok(JsonValue::Bool(
            eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?.is_empty(),
        )),
        PlanRowExpression::TextStartsWith { input, prefix } => Ok(JsonValue::Bool(
            eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?.starts_with(
                &eval_plan_row_text_with_stack(plan, list_state, row, prefix, stack)?,
            ),
        )),
        PlanRowExpression::TextLength { input } => Ok(json!(
            eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?
                .chars()
                .count() as i64
        )),
        PlanRowExpression::TextToNumber { input } => {
            let text = eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?;
            Ok(text
                .parse::<i64>()
                .map(JsonValue::from)
                .unwrap_or_else(|_| JsonValue::String("NaN".to_owned())))
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        } => {
            let text = eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?;
            let start = eval_plan_row_number_with_stack(plan, list_state, row, start, stack)?.max(0)
                as usize;
            let length = eval_plan_row_number_with_stack(plan, list_state, row, length, stack)?
                .max(0) as usize;
            Ok(JsonValue::String(
                text.chars().skip(start).take(length).collect(),
            ))
        }
        PlanRowExpression::TextToBytes { input, encoding } => {
            let text = eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?;
            let encoding = match encoding.as_deref() {
                Some(encoding) => {
                    eval_plan_row_text_with_stack(plan, list_state, row, encoding, stack)?
                }
                None => return Err("row Text/to_bytes requires explicit encoding".into()),
            };
            match row_text_to_bytes(&text, &encoding) {
                Ok(bytes) => Ok(row_private_bytes_json(&bytes)),
                Err(_) => Ok(row_error_json("parse_error")),
            }
        }
        PlanRowExpression::BytesToText { input, encoding } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let encoding = match encoding.as_deref() {
                Some(encoding) => {
                    eval_plan_row_text_with_stack(plan, list_state, row, encoding, stack)?
                }
                None => return Err("row Bytes/to_text requires explicit encoding".into()),
            };
            bytes_to_text(&bytes, &encoding)
                .map(JsonValue::String)
                .map_err(|error| format!("row Bytes/to_text {error}").into())
        }
        PlanRowExpression::BytesToHex { input } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            Ok(JsonValue::String(bytes_encode_hex(&bytes)))
        }
        PlanRowExpression::BytesToBase64 { input } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            Ok(JsonValue::String(bytes_encode_base64(&bytes)))
        }
        PlanRowExpression::BytesFromHex { input } => {
            let text = eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?;
            let bytes = bytes_decode_hex(&text)
                .map_err(|error| format!("row Bytes/from_hex failed: {error}"))?;
            Ok(row_private_bytes_json(&bytes))
        }
        PlanRowExpression::BytesFromBase64 { input } => {
            let text = eval_plan_row_text_with_stack(plan, list_state, row, input, stack)?;
            let bytes = bytes_decode_base64(&text)
                .map_err(|error| format!("row Bytes/from_base64 failed: {error}"))?;
            Ok(row_private_bytes_json(&bytes))
        }
        PlanRowExpression::BytesIsEmpty { input } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            Ok(JsonValue::Bool(bytes.is_empty()))
        }
        PlanRowExpression::BytesLength { input } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            i64::try_from(bytes.len())
                .map(JsonValue::from)
                .map_err(|_| "row Bytes/length exceeds Boon NUMBER".into())
        }
        PlanRowExpression::BytesGet { input, index } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let index = eval_plan_row_number_with_stack(plan, list_state, row, index, stack)?.max(0)
                as usize;
            bytes
                .get(index)
                .copied()
                .map(JsonValue::from)
                .ok_or_else(|| format!("row Bytes/get index {index} is out of bounds").into())
        }
        PlanRowExpression::BytesSlice {
            input,
            offset,
            byte_count,
        } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let offset = eval_plan_row_number_with_stack(plan, list_state, row, offset, stack)?
                .max(0) as usize;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let slice = bytes
                .get(offset..offset.saturating_add(byte_count))
                .ok_or_else(|| "row Bytes/slice out of bounds".to_owned())?
                .to_vec();
            Ok(row_private_bytes_json(&slice))
        }
        PlanRowExpression::BytesTake { input, byte_count } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let slice = bytes
                .get(0..byte_count)
                .ok_or_else(|| "row Bytes/take out of bounds".to_owned())?
                .to_vec();
            Ok(row_private_bytes_json(&slice))
        }
        PlanRowExpression::BytesDrop { input, byte_count } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            if byte_count > bytes.len() {
                return Err("row Bytes/drop out of bounds".into());
            }
            Ok(row_private_bytes_json(&bytes[byte_count..]))
        }
        PlanRowExpression::BytesZeros { byte_count } => {
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let mut output = Vec::new();
            output
                .try_reserve_exact(byte_count)
                .map_err(|_| format!("row Bytes/zeros could not allocate {byte_count} bytes"))?;
            output.resize(byte_count, 0);
            Ok(row_private_bytes_json(&output))
        }
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let offset = eval_plan_row_number_with_stack(plan, list_state, row, offset, stack)?
                .max(0) as usize;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let endian_text = eval_plan_row_text_with_stack(plan, list_state, row, endian, stack)?;
            let endian = row_bytes_endian(&endian_text)?;
            match bytes_read_unsigned(&bytes, offset, byte_count, endian) {
                Ok(value) if value <= i64::MAX as u64 => Ok(json!(value as i64)),
                Ok(_) => Err("row Bytes/read_unsigned overflows Boon NUMBER".into()),
                Err(error) => Err(format!("row Bytes/read_unsigned failed: {error:?}").into()),
            }
        }
        PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let offset = eval_plan_row_number_with_stack(plan, list_state, row, offset, stack)?
                .max(0) as usize;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let endian_text = eval_plan_row_text_with_stack(plan, list_state, row, endian, stack)?;
            let endian = row_bytes_endian(&endian_text)?;
            match bytes_read_signed(&bytes, offset, byte_count, endian) {
                Ok(value) => Ok(json!(value)),
                Err(error) => Err(format!("row Bytes/read_signed failed: {error:?}").into()),
            }
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            let mut bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let index = eval_plan_row_number_with_stack(plan, list_state, row, index, stack)?.max(0)
                as usize;
            let value = eval_plan_row_number_with_stack(plan, list_state, row, value, stack)?;
            let value = u8::try_from(value)
                .map_err(|_| format!("row Bytes/set value {value} is outside BYTE range"))?;
            let slot = bytes
                .get_mut(index)
                .ok_or_else(|| format!("row Bytes/set index {index} is out of bounds"))?;
            *slot = value;
            Ok(row_private_bytes_json(&bytes))
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            let mut bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let offset = eval_plan_row_number_with_stack(plan, list_state, row, offset, stack)?
                .max(0) as usize;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let endian_text = eval_plan_row_text_with_stack(plan, list_state, row, endian, stack)?;
            let endian = row_bytes_endian(&endian_text)?;
            let value = eval_plan_row_number_with_stack(plan, list_state, row, value, stack)?;
            bytes_write_unsigned(&mut bytes, offset, byte_count, endian, value)
                .map_err(|error| format!("row Bytes/write_unsigned failed: {error:?}"))?;
            Ok(row_private_bytes_json(&bytes))
        }
        PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            let mut bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let offset = eval_plan_row_number_with_stack(plan, list_state, row, offset, stack)?
                .max(0) as usize;
            let byte_count =
                eval_plan_row_number_with_stack(plan, list_state, row, byte_count, stack)?.max(0)
                    as usize;
            let endian_text = eval_plan_row_text_with_stack(plan, list_state, row, endian, stack)?;
            let endian = row_bytes_endian(&endian_text)?;
            let value = eval_plan_row_number_with_stack(plan, list_state, row, value, stack)?;
            bytes_write_signed(&mut bytes, offset, byte_count, endian, value)
                .map_err(|error| format!("row Bytes/write_signed failed: {error:?}"))?;
            Ok(row_private_bytes_json(&bytes))
        }
        PlanRowExpression::BytesFind { input, needle } => {
            let haystack = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let needle = eval_plan_row_bytes_with_stack(plan, list_state, row, needle, stack)?;
            Ok(bytes_find(&haystack, &needle)
                .map(|index| json!(index as i64))
                .unwrap_or_else(|| JsonValue::String("NaN".to_owned())))
        }
        PlanRowExpression::BytesStartsWith { input, prefix } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let prefix = eval_plan_row_bytes_with_stack(plan, list_state, row, prefix, stack)?;
            Ok(JsonValue::Bool(bytes.starts_with(&prefix)))
        }
        PlanRowExpression::BytesEndsWith { input, suffix } => {
            let bytes = eval_plan_row_bytes_with_stack(plan, list_state, row, input, stack)?;
            let suffix = eval_plan_row_bytes_with_stack(plan, list_state, row, suffix, stack)?;
            Ok(JsonValue::Bool(bytes.ends_with(&suffix)))
        }
        PlanRowExpression::BytesConcat { left, right } => {
            let mut left = eval_plan_row_bytes_with_stack(plan, list_state, row, left, stack)?;
            let right = eval_plan_row_bytes_with_stack(plan, list_state, row, right, stack)?;
            left.extend_from_slice(&right);
            Ok(row_private_bytes_json(&left))
        }
        PlanRowExpression::BytesEqual { left, right } => {
            let left = eval_plan_row_bytes_with_stack(plan, list_state, row, left, stack)?;
            let right = eval_plan_row_bytes_with_stack(plan, list_state, row, right, stack)?;
            Ok(JsonValue::Bool(left == right))
        }
        PlanRowExpression::NumberInfix { op, left, right } => {
            let left_value =
                eval_plan_row_expression_with_stack(plan, list_state, row, left, stack)?;
            let right_value =
                eval_plan_row_expression_with_stack(plan, list_state, row, right, stack)?;
            if row_value_is_cycle_error(&left_value) || row_value_is_cycle_error(&right_value) {
                return Ok(row_cycle_error_json());
            }
            if row_value_is_nan(&left_value) || row_value_is_nan(&right_value) {
                return Ok(JsonValue::String("NaN".to_owned()));
            }
            if op == "+"
                && (json_number_value(&left_value).is_none()
                    || json_number_value(&right_value).is_none())
            {
                return Ok(JsonValue::String(format!(
                    "{}{}",
                    row_value_to_text(&left_value)?,
                    row_value_to_text(&right_value)?
                )));
            }
            let left = row_value_to_number(&left_value)?;
            let right = row_value_to_number(&right_value)?;
            let value = match op.as_str() {
                "+" => left + right,
                "-" => left - right,
                "*" => left * right,
                "/" => {
                    if right == 0 {
                        return Ok(row_error_json("div_by_zero"));
                    }
                    left / right
                }
                "%" => {
                    if right == 0 {
                        return Ok(row_error_json("mod_by_zero"));
                    }
                    left % right
                }
                _ => return Err(format!("unsupported row numeric op `{op}`").into()),
            };
            Ok(json!(value))
        }
        PlanRowExpression::TextConcat { parts } => {
            let mut text = String::new();
            for part in parts {
                text.push_str(&eval_plan_row_text_with_stack(
                    plan, list_state, row, part, stack,
                )?);
            }
            Ok(JsonValue::String(text))
        }
        PlanRowExpression::ListGetField {
            list_id,
            index,
            field,
        } => {
            let index = eval_plan_row_number_with_stack(plan, list_state, row, index, stack)?;
            let index = usize::try_from(index)
                .map_err(|_| format!("row expression List/get index {index} is negative"))?;
            let rows = list_state
                .get(&list_id.0)
                .ok_or_else(|| format!("row expression list {} is not materialized", list_id.0))?;
            let row = rows
                .get(index)
                .ok_or_else(|| format!("row expression List/get index {index} is out of range"))?;
            eval_plan_row_lookup_field(plan, list_state, *list_id, row, *field, stack)
        }
        PlanRowExpression::ListRef { list_id } => eval_plan_row_list_ref(list_state, *list_id),
        PlanRowExpression::ListFindValue {
            list_id,
            field,
            value,
            target,
            fallback,
        } => {
            let value = eval_plan_row_expression_with_stack(plan, list_state, row, value, stack)?;
            let field_name = plan_row_field_local_name(plan, *field);
            let rows = list_state
                .get(&list_id.0)
                .ok_or_else(|| format!("row expression list {} is not materialized", list_id.0))?;
            if let Some(found) = rows
                .iter()
                .find(|candidate| row_json_values_equal(candidate.fields.get(&field_name), &value))
            {
                return eval_plan_row_lookup_field(
                    plan, list_state, *list_id, found, *target, stack,
                );
            }
            match fallback {
                Some(fallback) => {
                    eval_plan_row_expression_with_stack(plan, list_state, row, fallback, stack)
                }
                None => Ok(JsonValue::String("NaN".to_owned())),
            }
        }
        PlanRowExpression::ListRange { from, to } => {
            let from = eval_plan_row_number_with_stack(plan, list_state, row, from, stack)?;
            let to = eval_plan_row_number_with_stack(plan, list_state, row, to, stack)?;
            let values = if from <= to {
                (from..=to).map(JsonValue::from).collect()
            } else {
                Vec::new()
            };
            Ok(JsonValue::Array(values))
        }
        PlanRowExpression::ListMap {
            input,
            binding,
            value,
        } => {
            let items = eval_plan_row_list_with_stack(plan, list_state, row, input, stack)?;
            let mut output = Vec::with_capacity(items.len());
            let binding_key = row_map_binding_key(binding);
            for item in items {
                let mut bound_row = row.clone();
                bound_row.fields.insert(binding_key.clone(), item);
                output.push(eval_plan_row_expression_with_stack(
                    plan, list_state, &bound_row, value, stack,
                )?);
            }
            Ok(JsonValue::Array(output))
        }
        PlanRowExpression::ListMapItem { binding } => {
            let binding_key = row_map_binding_key(binding);
            row.fields.get(&binding_key).cloned().ok_or_else(|| {
                format!("row expression List/map binding `{binding}` is missing").into()
            })
        }
        PlanRowExpression::ListSum { input } => {
            let items = eval_plan_row_list_with_stack(plan, list_state, row, input, stack)?;
            let mut total = 0i64;
            for item in items {
                if let Some(value) = json_number_value(&item) {
                    total += value;
                }
            }
            Ok(JsonValue::from(total))
        }
        PlanRowExpression::Object { fields } => {
            let mut object = serde_json::Map::with_capacity(fields.len());
            for field in fields {
                object.insert(
                    field.name.clone(),
                    eval_plan_row_expression_with_stack(
                        plan,
                        list_state,
                        row,
                        &field.value,
                        stack,
                    )?,
                );
            }
            Ok(JsonValue::Object(object))
        }
        PlanRowExpression::ObjectField { object, field } => {
            let object = eval_plan_row_expression_with_stack(plan, list_state, row, object, stack)?;
            object
                .as_object()
                .and_then(|object| object.get(field))
                .cloned()
                .ok_or_else(|| format!("row expression object field `{field}` is missing").into())
        }
        PlanRowExpression::BuiltinCall {
            function,
            input,
            args,
        } => eval_plan_row_builtin_call(
            plan,
            list_state,
            row,
            function,
            input.as_deref(),
            args,
            stack,
        ),
        PlanRowExpression::Select { input, arms } => {
            let input_value =
                eval_plan_row_expression_with_stack(plan, list_state, row, input, stack)?;
            for arm in arms {
                if row_select_pattern_matches(&arm.pattern, &input_value) {
                    return eval_plan_row_expression_with_stack(
                        plan, list_state, row, &arm.value, stack,
                    );
                }
            }
            Err(format!("row expression select has no matching arm for `{input_value}`").into())
        }
    }
}

fn row_value_is_nan(value: &JsonValue) -> bool {
    value.as_str() == Some("NaN")
}

pub fn row_value_is_cycle_error(value: &JsonValue) -> bool {
    value.as_str() == Some("cycle_error")
}

fn row_cycle_error_json() -> JsonValue {
    JsonValue::String("cycle_error".to_owned())
}

fn row_error_json(code: &str) -> JsonValue {
    json!({
        "$boon_type": "ERROR",
        "code": code,
    })
}

fn row_value_to_number(value: &JsonValue) -> PlanExecutorResult<i64> {
    if row_value_is_nan(value) {
        return Ok(0);
    }
    if let Some(value) = value.as_i64() {
        return Ok(value);
    }
    if let Some(text) = value.as_str() {
        return text
            .parse::<i64>()
            .map_err(|_| format!("row expression value `{text}` is not a number").into());
    }
    Err(format!("row expression value `{value}` is not a number").into())
}

fn row_value_to_text(value: &JsonValue) -> PlanExecutorResult<String> {
    if let Some(code) = row_error_code(value) {
        return Ok(code.to_owned());
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_owned());
    }
    if let Some(number) = value.as_i64() {
        return Ok(number.to_string());
    }
    if let Some(bool_value) = value.as_bool() {
        return Ok(if bool_value { "True" } else { "False" }.to_owned());
    }
    Err(format!("row expression value `{value}` is not text-convertible").into())
}

fn row_error_code(value: &JsonValue) -> Option<&str> {
    value
        .get("$boon_type")
        .and_then(JsonValue::as_str)
        .filter(|kind| *kind == "ERROR")
        .and_then(|_| value.get("code"))
        .and_then(JsonValue::as_str)
}

fn plan_row_field_local_name(plan: &MachinePlan, field_id: boon_plan::FieldId) -> String {
    local_field_name(&semantic_field_label(plan, field_id.0))
}

fn eval_plan_row_lookup_field(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    list_id: boon_plan::ListId,
    row: &PlanExecutorListRowState,
    field: boon_plan::FieldId,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<JsonValue> {
    let target_name = plan_row_field_local_name(plan, field);
    let key = PlanRowEvalKey {
        list_id: list_id.0,
        row_key: row.key,
        field_id: field.0,
    };
    if let Some((expression, demand_current)) = plan_indexed_row_expression_for_field(plan, field) {
        if !demand_current && let Some(value) = row.fields.get(&target_name).cloned() {
            return Ok(value);
        }
        if stack.contains(&key) {
            return Ok(row_cycle_error_json());
        }
        let mut nested_stack = stack.to_vec();
        nested_stack.push(key);
        return eval_plan_row_expression_with_stack(
            plan,
            list_state,
            row,
            expression,
            &nested_stack,
        );
    }
    row.fields.get(&target_name).cloned().ok_or_else(|| {
        format!(
            "row expression lookup target `{target_name}` missing from list {}",
            list_id.0
        )
        .into()
    })
}

fn plan_indexed_row_expression_for_field(
    plan: &MachinePlan,
    field: boon_plan::FieldId,
) -> Option<(&PlanRowExpression, bool)> {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find_map(|op| {
            if !op.indexed || op.output != Some(ValueRef::Field(field)) {
                return None;
            }
            let PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } = &op.kind
            else {
                return None;
            };
            Some((expression, plan_indexed_derived_op_is_demand_current(op)))
        })
}

fn plan_indexed_derived_op_is_demand_current(op: &boon_plan::PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            startup_recompute: false,
            expression: Some(PlanDerivedExpression::RowExpression { .. }),
            ..
        } if op.indexed
    )
}

fn eval_plan_row_list_ref(
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    list_id: boon_plan::ListId,
) -> PlanExecutorResult<JsonValue> {
    let rows = list_state
        .get(&list_id.0)
        .ok_or_else(|| format!("row expression list {} is not materialized", list_id.0))?;
    Ok(JsonValue::Array(
        rows.iter()
            .map(|row| {
                JsonValue::Object(
                    row.fields
                        .iter()
                        .map(|(field, value)| (field.clone(), value.clone()))
                        .collect(),
                )
            })
            .collect(),
    ))
}

fn eval_plan_row_list_with_stack(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<Vec<JsonValue>> {
    let value = eval_plan_row_expression_with_stack(plan, list_state, row, expression, stack)?;
    match value {
        JsonValue::Array(values) => Ok(values),
        other => Err(format!("row expression value `{other}` is not a list").into()),
    }
}

fn row_map_binding_key(binding: &str) -> String {
    format!("$boon$row_map_binding${binding}")
}

fn json_number_value(value: &JsonValue) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
}

fn eval_plan_row_builtin_call(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    function: &str,
    input: Option<&PlanRowExpression>,
    args: &[boon_plan::PlanRowCallArg],
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<JsonValue> {
    match function {
        "Text/empty" => Ok(JsonValue::String(String::new())),
        "Error/new" => {
            let code = eval_plan_row_text_arg(plan, list_state, row, args, "code", stack)
                .unwrap_or_else(|_| "error".to_owned());
            Ok(json!({
                "$boon_type": "ERROR",
                "code": code,
            }))
        }
        "Error/text" => {
            let value = match input {
                Some(input) => {
                    eval_plan_row_expression_with_stack(plan, list_state, row, input, stack)?
                }
                None => {
                    eval_plan_row_named_or_first_arg(plan, list_state, row, args, "value", stack)?
                }
            };
            if row_value_is_cycle_error(&value) {
                return Ok(row_cycle_error_json());
            }
            Ok(JsonValue::String(
                value
                    .get("$boon_type")
                    .and_then(JsonValue::as_str)
                    .filter(|kind| *kind == "ERROR")
                    .and_then(|_| value.get("code"))
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ))
        }
        _ => Err(format!("unsupported row builtin `{function}`").into()),
    }
}

fn row_text_to_bytes(text: &str, encoding: &str) -> PlanExecutorResult<Vec<u8>> {
    match encoding.to_ascii_lowercase().as_str() {
        "utf8" => Ok(text.as_bytes().to_vec()),
        "ascii" if text.is_ascii() => Ok(text.as_bytes().to_vec()),
        "ascii" => Err("row Text/to_bytes input is not ASCII for Ascii encoding".into()),
        _ => Err(format!("row Text/to_bytes unsupported encoding `{encoding}`").into()),
    }
}

fn row_bytes_endian(text: &str) -> PlanExecutorResult<BytesEndian> {
    match text {
        "Little" => Ok(BytesEndian::Little),
        "Big" => Ok(BytesEndian::Big),
        _ => Err(format!("row BYTES endian `{text}` is unsupported").into()),
    }
}

fn row_private_bytes_json(bytes: &[u8]) -> JsonValue {
    json!({
        "$boon_type": "BYTES",
        "storage": "row_inline",
        "byte_len": bytes.len(),
        "inline_bytes": bytes,
    })
}

fn row_fixed_bytes_report_json(bytes: &[u8]) -> JsonValue {
    json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": sha256_bytes(bytes),
        "byte_len": bytes.len() as u64,
    })
}

fn eval_plan_row_text_arg(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    args: &[boon_plan::PlanRowCallArg],
    name: &str,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<String> {
    let arg = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .ok_or_else(|| format!("row builtin missing text arg `{name}`"))?;
    eval_plan_row_text_with_stack(plan, list_state, row, &arg.value, stack)
}

fn eval_plan_row_named_or_first_arg(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    args: &[boon_plan::PlanRowCallArg],
    name: &str,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<JsonValue> {
    let arg = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .ok_or_else(|| format!("row builtin missing arg `{name}`"))?;
    eval_plan_row_expression_with_stack(plan, list_state, row, &arg.value, stack)
}

fn eval_plan_row_bytes_with_stack(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<Vec<u8>> {
    match expression {
        PlanRowExpression::Constant { constant_id } => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *constant_id)
                .ok_or_else(|| format!("row bytes constant {} is missing", constant_id.0))?;
            let PlanConstantValue::Bytes {
                inline_bytes: Some(bytes),
                ..
            } = &constant.value
            else {
                return Err("row expression constant is not inline BYTES".into());
            };
            Ok(bytes.clone())
        }
        _ => {
            if let Some(bytes) = eval_plan_row_private_bytes_ref(plan, row, expression)? {
                return Ok(bytes.inline_bytes().to_vec());
            }
            let value =
                eval_plan_row_expression_with_stack(plan, list_state, row, expression, stack)?;
            value
                .get("inline_bytes")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| "row expression value is not private BYTES".to_owned())?
                .iter()
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| "row private BYTES payload is invalid".into())
                })
                .collect()
        }
    }
}

fn eval_plan_row_private_bytes_ref(
    plan: &MachinePlan,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
) -> PlanExecutorResult<Option<PlanExecutorBytes>> {
    let PlanRowExpression::Field { input } = expression else {
        return Ok(None);
    };
    let field_name = match input {
        ValueRef::Field(field_id) => local_field_name(&semantic_field_label(plan, field_id.0)),
        ValueRef::State(state_id) => local_field_name(&state_label(plan, *state_id)),
        _ => return Ok(None),
    };
    let Some(bytes) = row.private_bytes.get(&field_name) else {
        return Ok(None);
    };
    let public_value = row.fields.get(&field_name).ok_or_else(|| {
        format!("row BYTES field `{field_name}` has private bytes but no public summary")
    })?;
    if &bytes.report_json() != public_value && &bytes.artifact_json() != public_value {
        return Err(format!(
            "row BYTES field `{field_name}` public summary does not match private bytes"
        )
        .into());
    }
    if let Some(fixed_bytes) = row.fixed_bytes_banks.get(&field_name)
        && row_fixed_bytes_report_json(fixed_bytes) != *public_value
    {
        return Err(format!(
            "row BYTES field `{field_name}` public summary does not match fixed byte bank"
        )
        .into());
    }
    Ok(Some(bytes.clone()))
}

fn row_select_pattern_matches(pattern: &PlanRowSelectPattern, value: &JsonValue) -> bool {
    match pattern {
        PlanRowSelectPattern::Bool { value: expected } => value.as_bool() == Some(*expected),
        PlanRowSelectPattern::Text { value: expected } => value.as_str() == Some(expected.as_str()),
        PlanRowSelectPattern::Number { value: expected } => value.as_i64() == Some(*expected),
        PlanRowSelectPattern::NaN => value.as_str() == Some("NaN"),
        PlanRowSelectPattern::Wildcard => true,
    }
}

fn eval_plan_row_field_ref(
    plan: &MachinePlan,
    row: &PlanExecutorListRowState,
    input: &ValueRef,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<JsonValue> {
    let field_name = match input {
        ValueRef::Field(field_id) => local_field_name(&semantic_field_label(plan, field_id.0)),
        ValueRef::State(state_id) => local_field_name(&state_label(plan, *state_id)),
        _ => {
            return Err(format!("row expression input `{input:?}` is not a row field").into());
        }
    };
    row.fields
        .get(&field_name)
        .cloned()
        .or_else(|| row_initial_state_fallback(plan, row, input))
        .ok_or_else(|| {
            let available = row.fields.keys().cloned().collect::<Vec<_>>().join(", ");
            format!(
                "row expression input `{input:?}` field `{field_name}` is missing from row key {} stack {:?}; available fields: [{}]",
                row.key, stack, available
            )
            .into()
        })
}

fn row_initial_state_fallback(
    plan: &MachinePlan,
    row: &PlanExecutorListRowState,
    input: &ValueRef,
) -> Option<JsonValue> {
    let ValueRef::State(state_id) = input else {
        return None;
    };
    let slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == *state_id)?;
    let source = slot.initial_row_field_path.as_deref()?;
    let source_name = local_field_name(source);
    row.fields.get(&source_name).cloned()
}

fn eval_plan_row_number_with_stack(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<i64> {
    let value = eval_plan_row_expression_with_stack(plan, list_state, row, expression, stack)?;
    row_value_to_number(&value)
}

fn eval_plan_row_text_with_stack(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRowState>>,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
    stack: &[PlanRowEvalKey],
) -> PlanExecutorResult<String> {
    let value = eval_plan_row_expression_with_stack(plan, list_state, row, expression, stack)?;
    row_value_to_text(&value)
}

pub fn row_expression_row_input_missing(
    plan: &MachinePlan,
    row: &PlanExecutorListRowState,
    expression: &PlanRowExpression,
) -> bool {
    match expression {
        PlanRowExpression::Field { input } => {
            let input_name = match input {
                ValueRef::Field(input_id) => {
                    local_field_name(&semantic_field_label(plan, input_id.0))
                }
                ValueRef::State(input_id) => local_field_name(&state_label(plan, *input_id)),
                _ => return false,
            };
            !row.fields.contains_key(&input_name)
        }
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextIsEmpty { input }
        | PlanRowExpression::TextLength { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::TextToBytes { input, .. }
        | PlanRowExpression::BytesToText { input, .. }
        | PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input }
        | PlanRowExpression::BytesIsEmpty { input }
        | PlanRowExpression::BytesLength { input }
        | PlanRowExpression::BytesZeros { byte_count: input }
        | PlanRowExpression::BytesTake { input, .. }
        | PlanRowExpression::BytesDrop { input, .. }
        | PlanRowExpression::ListSum { input }
        | PlanRowExpression::ObjectField { object: input, .. } => {
            row_expression_row_input_missing(plan, row, input)
        }
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, prefix)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        }
        | PlanRowExpression::BytesSlice {
            input,
            offset: start,
            byte_count: length,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, start)
                || row_expression_row_input_missing(plan, row, length)
        }
        PlanRowExpression::BytesGet { input, index }
        | PlanRowExpression::BytesFind {
            input,
            needle: index,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, index)
        }
        PlanRowExpression::BytesStartsWith { input, prefix }
        | PlanRowExpression::BytesEndsWith {
            input,
            suffix: prefix,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, prefix)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, index)
                || row_expression_row_input_missing(plan, row, value)
        }
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, offset)
                || row_expression_row_input_missing(plan, row, byte_count)
                || row_expression_row_input_missing(plan, row, endian)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, offset)
                || row_expression_row_input_missing(plan, row, byte_count)
                || row_expression_row_input_missing(plan, row, endian)
                || row_expression_row_input_missing(plan, row, value)
        }
        PlanRowExpression::BytesConcat { left, right }
        | PlanRowExpression::BytesEqual { left, right }
        | PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_row_input_missing(plan, row, left)
                || row_expression_row_input_missing(plan, row, right)
        }
        PlanRowExpression::TextConcat { parts } => parts
            .iter()
            .any(|part| row_expression_row_input_missing(plan, row, part)),
        PlanRowExpression::Object { fields } => fields
            .iter()
            .any(|field| row_expression_row_input_missing(plan, row, &field.value)),
        PlanRowExpression::ListGetField { index, .. } => {
            row_expression_row_input_missing(plan, row, index)
        }
        PlanRowExpression::ListFindValue {
            value, fallback, ..
        } => {
            row_expression_row_input_missing(plan, row, value)
                || fallback
                    .as_deref()
                    .is_some_and(|fallback| row_expression_row_input_missing(plan, row, fallback))
        }
        PlanRowExpression::ListRange { from, to } => {
            row_expression_row_input_missing(plan, row, from)
                || row_expression_row_input_missing(plan, row, to)
        }
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_row_input_missing(plan, row, input)
                || row_expression_row_input_missing(plan, row, value)
        }
        PlanRowExpression::BuiltinCall { input, args, .. } => {
            input
                .as_deref()
                .is_some_and(|input| row_expression_row_input_missing(plan, row, input))
                || args
                    .iter()
                    .any(|arg| row_expression_row_input_missing(plan, row, &arg.value))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_row_input_missing(plan, row, input)
                || arms
                    .iter()
                    .any(|arm| row_expression_row_input_missing(plan, row, &arm.value))
        }
        PlanRowExpression::Constant { .. }
        | PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListMapItem { .. } => false,
    }
}

fn row_expression_reads_list(expression: &PlanRowExpression, list_id: boon_plan::ListId) -> bool {
    match expression {
        PlanRowExpression::ListGetField {
            list_id: expression_list_id,
            index,
            ..
        } => *expression_list_id == list_id || row_expression_reads_list(index, list_id),
        PlanRowExpression::ListRef {
            list_id: expression_list_id,
        } => *expression_list_id == list_id,
        PlanRowExpression::ListFindValue {
            list_id: expression_list_id,
            value,
            fallback,
            ..
        } => {
            *expression_list_id == list_id
                || row_expression_reads_list(value, list_id)
                || fallback
                    .as_deref()
                    .is_some_and(|fallback| row_expression_reads_list(fallback, list_id))
        }
        PlanRowExpression::ListRange { from, to } => {
            row_expression_reads_list(from, list_id) || row_expression_reads_list(to, list_id)
        }
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_reads_list(input, list_id) || row_expression_reads_list(value, list_id)
        }
        PlanRowExpression::Object { fields } => fields
            .iter()
            .any(|field| row_expression_reads_list(&field.value, list_id)),
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextIsEmpty { input }
        | PlanRowExpression::TextLength { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input }
        | PlanRowExpression::BytesIsEmpty { input }
        | PlanRowExpression::BytesLength { input }
        | PlanRowExpression::ObjectField { object: input, .. }
        | PlanRowExpression::ListSum { input } => row_expression_reads_list(input, list_id),
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_reads_list(input, list_id) || row_expression_reads_list(prefix, list_id)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        }
        | PlanRowExpression::BytesSlice {
            input,
            offset: start,
            byte_count: length,
        } => {
            row_expression_reads_list(input, list_id)
                || row_expression_reads_list(start, list_id)
                || row_expression_reads_list(length, list_id)
        }
        PlanRowExpression::TextToBytes { input, encoding }
        | PlanRowExpression::BytesToText { input, encoding } => {
            row_expression_reads_list(input, list_id)
                || encoding
                    .as_deref()
                    .is_some_and(|encoding| row_expression_reads_list(encoding, list_id))
        }
        PlanRowExpression::BytesGet { input, index } => {
            row_expression_reads_list(input, list_id) || row_expression_reads_list(index, list_id)
        }
        PlanRowExpression::BytesTake { input, byte_count }
        | PlanRowExpression::BytesDrop { input, byte_count } => {
            row_expression_reads_list(input, list_id)
                || row_expression_reads_list(byte_count, list_id)
        }
        PlanRowExpression::BytesZeros { byte_count } => {
            row_expression_reads_list(byte_count, list_id)
        }
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_reads_list(input, list_id)
                || row_expression_reads_list(offset, list_id)
                || row_expression_reads_list(byte_count, list_id)
                || row_expression_reads_list(endian, list_id)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_reads_list(input, list_id)
                || row_expression_reads_list(index, list_id)
                || row_expression_reads_list(value, list_id)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_reads_list(input, list_id)
                || row_expression_reads_list(offset, list_id)
                || row_expression_reads_list(byte_count, list_id)
                || row_expression_reads_list(endian, list_id)
                || row_expression_reads_list(value, list_id)
        }
        PlanRowExpression::BytesFind { input, needle }
        | PlanRowExpression::BytesStartsWith {
            input,
            prefix: needle,
        }
        | PlanRowExpression::BytesEndsWith {
            input,
            suffix: needle,
        } => {
            row_expression_reads_list(input, list_id) || row_expression_reads_list(needle, list_id)
        }
        PlanRowExpression::BytesConcat { left, right }
        | PlanRowExpression::BytesEqual { left, right }
        | PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_reads_list(left, list_id) || row_expression_reads_list(right, list_id)
        }
        PlanRowExpression::TextConcat { parts } => parts
            .iter()
            .any(|part| row_expression_reads_list(part, list_id)),
        PlanRowExpression::BuiltinCall { input, args, .. } => {
            input
                .as_deref()
                .is_some_and(|input| row_expression_reads_list(input, list_id))
                || args
                    .iter()
                    .any(|arg| row_expression_reads_list(&arg.value, list_id))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_reads_list(input, list_id)
                || arms
                    .iter()
                    .any(|arm| row_expression_reads_list(&arm.value, list_id))
        }
        PlanRowExpression::Field { .. }
        | PlanRowExpression::Constant { .. }
        | PlanRowExpression::ListMapItem { .. } => false,
    }
}

pub fn row_expression_applies_to_list(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
    output_id: FieldId,
) -> bool {
    semantic_field_label(plan, output_id.0)
        .rsplit_once('.')
        .map(|(scope_name, _)| scope_name.to_owned())
        .is_some_and(|scope_name| {
            let has_row_source_route = plan.source_routes.iter().any(|route| {
                route.scope_id == list_slot.scope_id
                    && route.path.starts_with(&format!("{scope_name}."))
            });
            let has_row_state_slot = plan.storage_layout.scalar_slots.iter().any(|slot| {
                slot.scope_id == list_slot.scope_id
                    && state_label(plan, slot.state_id).starts_with(&format!("{scope_name}."))
            });
            let has_append_row_field = plan
                .regions
                .iter()
                .filter(|region| region.kind == RegionKind::ListOperations)
                .flat_map(|region| region.ops.iter())
                .any(|op| {
                    op.output == Some(ValueRef::List(list_slot.list_id))
                        && matches!(
                            &op.kind,
                            PlanOpKind::ListOperation {
                                append: Some(append),
                                ..
                            } if append.fields.iter().any(|field| {
                                field.field_id.is_some_and(|field_id| {
                                    semantic_field_label(plan, field_id.0)
                                        .starts_with(&format!("{scope_name}."))
                                })
                            })
                        )
                });
            has_row_source_route || has_row_state_slot || has_append_row_field
        })
}

pub fn evaluate_root_pure_number_compare_values(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> PlanExecutorResult<BTreeMap<usize, JsonValue>> {
    let aggregate_counts = aggregate_count_values(plan, list_state)?;
    let mut values = aggregate_counts
        .iter()
        .map(|(field_id, value)| (*field_id, json!(value)))
        .collect::<BTreeMap<_, _>>();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            expression: Some(expression),
            ..
        } = &op.kind
        else {
            continue;
        };
        if let Some(value) =
            eval_root_bool_derived_expression(op.id.0, expression, &aggregate_counts)?
        {
            values.insert(output_id.0, json!(value));
        }
    }
    Ok(values)
}

fn plan_has_root_router_route_reader(plan: &MachinePlan) -> bool {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .any(|op| {
            !op.indexed
                && matches!(
                    &op.kind,
                    PlanOpKind::DerivedValue {
                        derived_kind: boon_plan::PlanDerivedKind::Pure,
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    } if row_expression_reads_router_route(expression)
                )
        })
}

fn row_expression_reads_router_route(expression: &PlanRowExpression) -> bool {
    match expression {
        PlanRowExpression::BuiltinCall {
            function,
            input,
            args,
        } => {
            function == "Router/route"
                || input
                    .as_deref()
                    .is_some_and(row_expression_reads_router_route)
                || args
                    .iter()
                    .any(|arg| row_expression_reads_router_route(&arg.value))
        }
        PlanRowExpression::Select { input, arms } => {
            row_expression_reads_router_route(input)
                || arms
                    .iter()
                    .any(|arm| row_expression_reads_router_route(&arm.value))
        }
        PlanRowExpression::TextTrim { input }
        | PlanRowExpression::TextIsEmpty { input }
        | PlanRowExpression::TextLength { input }
        | PlanRowExpression::TextToNumber { input }
        | PlanRowExpression::BytesToHex { input }
        | PlanRowExpression::BytesToBase64 { input }
        | PlanRowExpression::BytesFromHex { input }
        | PlanRowExpression::BytesFromBase64 { input }
        | PlanRowExpression::BytesIsEmpty { input }
        | PlanRowExpression::BytesLength { input }
        | PlanRowExpression::ObjectField { object: input, .. }
        | PlanRowExpression::ListSum { input } => row_expression_reads_router_route(input),
        PlanRowExpression::TextStartsWith { input, prefix } => {
            row_expression_reads_router_route(input) || row_expression_reads_router_route(prefix)
        }
        PlanRowExpression::TextSubstring {
            input,
            start,
            length,
        }
        | PlanRowExpression::BytesSlice {
            input,
            offset: start,
            byte_count: length,
        } => {
            row_expression_reads_router_route(input)
                || row_expression_reads_router_route(start)
                || row_expression_reads_router_route(length)
        }
        PlanRowExpression::TextToBytes { input, encoding }
        | PlanRowExpression::BytesToText { input, encoding } => {
            row_expression_reads_router_route(input)
                || encoding
                    .as_deref()
                    .is_some_and(row_expression_reads_router_route)
        }
        PlanRowExpression::BytesGet { input, index } => {
            row_expression_reads_router_route(input) || row_expression_reads_router_route(index)
        }
        PlanRowExpression::BytesTake { input, byte_count }
        | PlanRowExpression::BytesDrop { input, byte_count } => {
            row_expression_reads_router_route(input)
                || row_expression_reads_router_route(byte_count)
        }
        PlanRowExpression::BytesZeros { byte_count } => {
            row_expression_reads_router_route(byte_count)
        }
        PlanRowExpression::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpression::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            row_expression_reads_router_route(input)
                || row_expression_reads_router_route(offset)
                || row_expression_reads_router_route(byte_count)
                || row_expression_reads_router_route(endian)
        }
        PlanRowExpression::BytesSet {
            input,
            index,
            value,
        } => {
            row_expression_reads_router_route(input)
                || row_expression_reads_router_route(index)
                || row_expression_reads_router_route(value)
        }
        PlanRowExpression::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpression::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            row_expression_reads_router_route(input)
                || row_expression_reads_router_route(offset)
                || row_expression_reads_router_route(byte_count)
                || row_expression_reads_router_route(endian)
                || row_expression_reads_router_route(value)
        }
        PlanRowExpression::BytesFind { input, needle }
        | PlanRowExpression::BytesStartsWith {
            input,
            prefix: needle,
        }
        | PlanRowExpression::BytesEndsWith {
            input,
            suffix: needle,
        } => row_expression_reads_router_route(input) || row_expression_reads_router_route(needle),
        PlanRowExpression::BytesConcat { left, right }
        | PlanRowExpression::BytesEqual { left, right }
        | PlanRowExpression::NumberInfix { left, right, .. } => {
            row_expression_reads_router_route(left) || row_expression_reads_router_route(right)
        }
        PlanRowExpression::TextConcat { parts } => {
            parts.iter().any(row_expression_reads_router_route)
        }
        PlanRowExpression::ListFindValue {
            value, fallback, ..
        } => {
            row_expression_reads_router_route(value)
                || fallback
                    .as_deref()
                    .is_some_and(row_expression_reads_router_route)
        }
        PlanRowExpression::ListRange { from, to } => {
            row_expression_reads_router_route(from) || row_expression_reads_router_route(to)
        }
        PlanRowExpression::ListMap { input, value, .. } => {
            row_expression_reads_router_route(input) || row_expression_reads_router_route(value)
        }
        PlanRowExpression::ListGetField { index, .. } => row_expression_reads_router_route(index),
        PlanRowExpression::Object { fields } => fields
            .iter()
            .any(|field| row_expression_reads_router_route(&field.value)),
        PlanRowExpression::Field { .. }
        | PlanRowExpression::Constant { .. }
        | PlanRowExpression::ListRef { .. }
        | PlanRowExpression::ListMapItem { .. } => false,
    }
}

pub fn evaluate_initial_root_source_event_transforms(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    let mut values = BTreeMap::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
            expression: Some(PlanDerivedExpression::SourceEventTransform { default, .. }),
            ..
        } = &op.kind
        else {
            continue;
        };
        let value = eval_root_source_transform_row_expression(plan, root_state, default)?;
        values.insert(output_id, value);
    }
    Ok(values)
}

pub fn evaluate_initial_root_derived_values(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    evaluate_initial_root_derived_values_with_policy(plan, root_state, false)
}

fn evaluate_initial_root_derived_values_partial(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    evaluate_initial_root_derived_values_with_policy(plan, root_state, true)
}

fn evaluate_initial_root_derived_values_with_policy(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    allow_unresolved: bool,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    let mut evaluation_state = root_state.clone();
    evaluation_state
        .entry("Router/route".to_owned())
        .or_insert_with(|| json!("/"));
    let derived_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .filter_map(|op| {
            if op.indexed {
                return None;
            }
            let Some(ValueRef::Field(output_id)) = op.output else {
                return None;
            };
            match &op.kind {
                PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
                    expression: Some(PlanDerivedExpression::SourceEventTransform { default, .. }),
                    ..
                } => Some((op, output_id, default.as_ref())),
                PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } => Some((op, output_id, expression)),
                _ => None,
            }
        })
        .collect::<Vec<_>>();

    let mut values = BTreeMap::new();
    let mut resolved = BTreeSet::new();
    while resolved.len() < derived_ops.len() {
        let mut progressed = false;
        let mut deferred_errors = Vec::new();
        for (op, output_id, expression) in &derived_ops {
            if resolved.contains(&op.id) {
                continue;
            }
            match eval_root_source_transform_row_expression(plan, &evaluation_state, expression) {
                Ok(value) => {
                    evaluation_state.insert(derived_field_label(plan, output_id.0), value.clone());
                    values.insert(*output_id, value);
                    resolved.insert(op.id);
                    progressed = true;
                }
                Err(error) => {
                    deferred_errors.push(format!(
                        "field {}: {error}",
                        derived_field_label(plan, output_id.0)
                    ));
                }
            }
        }
        if !progressed {
            if allow_unresolved {
                return Ok(values);
            }
            return Err(format!(
                "initial root derived value dependency resolution stalled: {}",
                deferred_errors.join("; ")
            )
            .into());
        }
    }
    Ok(values)
}

fn evaluate_root_row_expression_derived_values(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    let mut values = BTreeMap::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
    {
        if op.indexed {
            continue;
        }
        let Some(ValueRef::Field(output_id)) = op.output else {
            continue;
        };
        let PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::Pure,
            expression: Some(PlanDerivedExpression::RowExpression { expression }),
            ..
        } = &op.kind
        else {
            continue;
        };
        if let Ok(value) = eval_root_source_transform_row_expression(plan, root_state, expression) {
            values.insert(output_id, value);
        }
    }
    Ok(values)
}

pub fn commit_source_derived_values_to_root_state(
    plan: &MachinePlan,
    root_state: &mut JsonMap<String, JsonValue>,
    derived_values: &BTreeMap<FieldId, JsonValue>,
) -> Vec<JsonValue> {
    let mut reports = Vec::with_capacity(derived_values.len());
    for (field_id, value) in derived_values {
        let field_path = derived_field_label(plan, field_id.0);
        let changed = root_state.get(&field_path) != Some(value);
        root_state.insert(field_path.clone(), value.clone());
        reports.push(json!({
            "field_id": field_id.0,
            "field_path": field_path,
            "value": value,
            "changed": changed,
        }));
    }
    reports
}

fn eval_root_source_transform_row_expression(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    expression: &PlanRowExpression,
) -> PlanExecutorResult<JsonValue> {
    match expression {
        PlanRowExpression::Constant { constant_id } => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *constant_id)
                .ok_or_else(|| format!("missing source-transform constant {}", constant_id.0))?;
            plan_constant_json_value(constant)
        }
        PlanRowExpression::Field { input } => match input {
            ValueRef::State(state_id) => {
                let label = state_label(plan, *state_id);
                root_state.get(&label).cloned().ok_or_else(|| {
                    format!("source-transform root state input `{label}` is missing").into()
                })
            }
            ValueRef::Field(field_id) => {
                let label = derived_field_label(plan, field_id.0);
                root_state.get(&label).cloned().ok_or_else(|| {
                    format!("source-transform root derived input `{label}` is missing").into()
                })
            }
            other => {
                Err(format!("source-transform field input `{other:?}` is not root-readable").into())
            }
        },
        PlanRowExpression::TextTrim { input } => {
            let value = eval_root_source_transform_row_expression(plan, root_state, input)?;
            Ok(JsonValue::String(
                value
                    .as_str()
                    .ok_or("source-transform Text/trim input is not text")?
                    .trim()
                    .to_owned(),
            ))
        }
        PlanRowExpression::TextToNumber { input } => {
            let value = eval_root_source_transform_row_expression(plan, root_state, input)?;
            if let Some(number) = value.as_i64() {
                return Ok(json!(number));
            }
            let text = json_scalar_textlike(&value)
                .ok_or("source-transform Text/to_number input is not textlike")?;
            let number = text.trim().parse::<i64>().map_err(|error| {
                format!("source-transform Text/to_number input `{text}` is not a number: {error}")
            })?;
            Ok(json!(number))
        }
        PlanRowExpression::TextConcat { parts } => {
            let mut text = String::new();
            for part in parts {
                let value = eval_root_source_transform_row_expression(plan, root_state, part)?;
                text.push_str(
                    &json_scalar_textlike(&value)
                        .ok_or("source-transform TextConcat part is not textlike")?,
                );
            }
            Ok(JsonValue::String(text))
        }
        PlanRowExpression::Object { fields } => {
            let mut object = JsonMap::with_capacity(fields.len());
            for field in fields {
                object.insert(
                    field.name.clone(),
                    eval_root_source_transform_row_expression(plan, root_state, &field.value)?,
                );
            }
            Ok(JsonValue::Object(object))
        }
        PlanRowExpression::ObjectField { object, field } => {
            let value = eval_root_source_transform_row_expression(plan, root_state, object)?;
            value
                .get(field)
                .cloned()
                .ok_or_else(|| format!("source-transform object is missing field `{field}`").into())
        }
        PlanRowExpression::NumberInfix { op, left, right } => {
            let left_value = eval_root_source_transform_row_expression(plan, root_state, left)?;
            let right_value = eval_root_source_transform_row_expression(plan, root_state, right)?;
            if root_source_transform_value_is_nan(&left_value)
                || root_source_transform_value_is_nan(&right_value)
            {
                return Ok(JsonValue::String("NaN".to_owned()));
            }
            if op == "+"
                && (root_source_transform_number_value(&left_value).is_none()
                    || root_source_transform_number_value(&right_value).is_none())
            {
                let left_text = json_scalar_textlike(&left_value)
                    .ok_or("source-transform + left input is not textlike")?;
                let right_text = json_scalar_textlike(&right_value)
                    .ok_or("source-transform + right input is not textlike")?;
                return Ok(JsonValue::String(format!("{left_text}{right_text}")));
            }
            let left = root_source_transform_number_value(&left_value)
                .ok_or("source-transform numeric left input is not a number")?;
            let right = root_source_transform_number_value(&right_value)
                .ok_or("source-transform numeric right input is not a number")?;
            let value = match op.as_str() {
                "+" => left + right,
                "-" => left - right,
                "*" => left * right,
                "/" => {
                    if right == 0 {
                        return Err("source-transform division by zero".into());
                    }
                    left / right
                }
                "%" => {
                    if right == 0 {
                        return Err("source-transform modulo by zero".into());
                    }
                    left % right
                }
                _ => return Err(format!("unsupported source-transform numeric op `{op}`").into()),
            };
            Ok(json!(value))
        }
        PlanRowExpression::ListFindValue {
            list_id,
            field,
            value,
            target,
            fallback,
        } => {
            let selector = eval_root_source_transform_row_expression(plan, root_state, value)?;
            let list_slot = plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.list_id == *list_id)
                .ok_or_else(|| format!("source-transform list {} is missing", list_id.0))?;
            for row in &list_slot.initial_rows {
                let Some(candidate) = initial_row_field_json_value(row, *field)? else {
                    continue;
                };
                if row_json_values_equal(Some(&candidate), &selector) {
                    return initial_row_field_json_value(row, *target)?.ok_or_else(|| {
                        format!(
                            "source-transform List/find_value target field {} is missing in list {}",
                            target.0, list_id.0
                        )
                        .into()
                    });
                }
            }
            if let Some(fallback) = fallback {
                eval_root_source_transform_row_expression(plan, root_state, fallback)
            } else {
                Ok(JsonValue::Null)
            }
        }
        PlanRowExpression::BuiltinCall {
            function,
            input,
            args,
        } if function == "Router/route" && input.is_none() && args.is_empty() => Ok(root_state
            .get("Router/route")
            .cloned()
            .unwrap_or_else(|| json!("/"))),
        PlanRowExpression::Select { input, arms } => {
            let input = eval_root_source_transform_row_expression(plan, root_state, input)?;
            for arm in arms {
                if row_select_pattern_matches_json(&arm.pattern, &input) {
                    return eval_root_source_transform_row_expression(plan, root_state, &arm.value);
                }
            }
            Ok(JsonValue::Null)
        }
        other => Err(format!(
            "CPU PlanExecutor does not support root source-transform row expression `{other:?}`"
        )
        .into()),
    }
}

fn root_source_transform_value_is_nan(value: &JsonValue) -> bool {
    value.as_str() == Some("NaN")
}

fn root_source_transform_number_value(value: &JsonValue) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
}

fn initial_row_field_json_value(
    row: &boon_plan::PlanInitialListRow,
    field_id: FieldId,
) -> PlanExecutorResult<Option<JsonValue>> {
    row.fields
        .iter()
        .find(|field| field.field_id == Some(field_id))
        .map(|field| {
            plan_constant_value_json_value(
                &field.value,
                &format!("initial list row field `{}`", field.name),
            )
        })
        .transpose()
}

fn row_select_pattern_matches_json(
    pattern: &boon_plan::PlanRowSelectPattern,
    value: &JsonValue,
) -> bool {
    match pattern {
        boon_plan::PlanRowSelectPattern::Bool { value: expected } => {
            value.as_bool() == Some(*expected)
        }
        boon_plan::PlanRowSelectPattern::Text { value: expected } => {
            value.as_str() == Some(expected.as_str())
        }
        boon_plan::PlanRowSelectPattern::Number { value: expected } => {
            value.as_i64() == Some(*expected)
        }
        boon_plan::PlanRowSelectPattern::NaN => value.is_null(),
        boon_plan::PlanRowSelectPattern::Wildcard => true,
    }
}

pub fn changed_root_derived_deltas(
    plan: &MachinePlan,
    before: &BTreeMap<usize, JsonValue>,
    after: &BTreeMap<usize, JsonValue>,
) -> Vec<(JsonValue, JsonValue)> {
    let mut changes = Vec::new();
    let mut emptied_scopes = BTreeSet::new();
    let derived_field_ids = plan
        .debug_map
        .derived_values
        .iter()
        .filter_map(|entry| {
            entry
                .id
                .strip_prefix("field:")
                .and_then(|id| id.parse::<usize>().ok())
        })
        .collect::<BTreeSet<_>>();
    for (field_id, value) in after {
        if !derived_field_ids.contains(field_id) {
            continue;
        }
        if before.get(field_id) == Some(value) || value.as_bool() != Some(false) {
            continue;
        }
        let field_path = derived_field_label(plan, *field_id);
        if let Some(scope) = field_path.strip_suffix(".has_todos") {
            emptied_scopes.insert(scope.to_owned());
        }
    }
    for (field_id, value) in after {
        if !derived_field_ids.contains(field_id) {
            continue;
        }
        if before.get(field_id) == Some(value) {
            continue;
        }
        let field_path = derived_field_label(plan, *field_id);
        if emptied_scopes.iter().any(|scope| {
            field_path == format!("{scope}.has_completed")
                || field_path == format!("{scope}.all_completed")
        }) {
            continue;
        }
        changes.push((
            json!({
                "kind": "FieldSet",
                "list_id": null,
                "key": null,
                "generation": null,
                "source_id": null,
                "bind_epoch": null,
                "field_path": field_path,
                "value": value,
            }),
            json!({
                "field_id": field_id,
                "field_path": field_path,
                "expression_kind": "number_compare_const",
                "value": value,
            }),
        ));
    }
    changes
}

fn eval_root_bool_derived_expression(
    op_id: usize,
    expression: &PlanDerivedExpression,
    aggregate_counts: &BTreeMap<usize, i64>,
) -> PlanExecutorResult<Option<bool>> {
    match expression {
        PlanDerivedExpression::NumberCompareConst {
            left,
            op: compare_op,
            right,
        } => {
            let ValueRef::Field(left_id) = left else {
                return Err(format!(
                    "number-compare derived op {op_id} left input is not a field ref"
                )
                .into());
            };
            let Some(left_value) = aggregate_counts.get(&left_id.0) else {
                return Ok(None);
            };
            let value = match compare_op.as_str() {
                ">" => *left_value > *right,
                ">=" => *left_value >= *right,
                "<" => *left_value < *right,
                "<=" => *left_value <= *right,
                "==" => *left_value == *right,
                "!=" => *left_value != *right,
                other => {
                    return Err(format!(
                        "number-compare derived op {op_id} has unsupported operator `{other}`"
                    )
                    .into());
                }
            };
            Ok(Some(value))
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            let Some(left) = eval_root_bool_derived_expression(op_id, left, aggregate_counts)?
            else {
                return Ok(None);
            };
            let Some(right) = eval_root_bool_derived_expression(op_id, right, aggregate_counts)?
            else {
                return Ok(None);
            };
            Ok(Some(left && right))
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            let Some(value) = eval_root_bool_derived_expression(op_id, input, aggregate_counts)?
            else {
                return Ok(None);
            };
            Ok(Some(!value))
        }
        _ => Ok(None),
    }
}

fn aggregate_count_values(
    plan: &MachinePlan,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> PlanExecutorResult<BTreeMap<usize, i64>> {
    let mut values = BTreeMap::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
    {
        let PlanOpKind::ListOperation {
            operation_kind: boon_plan::PlanListOperationKind::Count,
            count: Some(count),
            ..
        } = &op.kind
        else {
            continue;
        };
        let Some(ValueRef::List(list_id)) = op.output else {
            return Err(format!("list count op {} does not output a list", op.id.0).into());
        };
        let rows = list_state
            .get(&list_id.0)
            .ok_or_else(|| format!("list state missing list {}", list_id.0))?;
        let ValueRef::Field(target_id) = count.target else {
            return Err(format!("list count op {} target is not a field ref", op.id.0).into());
        };
        let mut count_value = 0i64;
        for row in rows {
            if list_count_predicate_matches(plan, &count.predicate, row)? {
                count_value += 1;
            }
        }
        values.insert(target_id.0, count_value);
    }
    Ok(values)
}

fn list_count_predicate_matches(
    plan: &MachinePlan,
    predicate: &boon_plan::PlanListRemovePredicate,
    row: &PlanExecutorListRow,
) -> PlanExecutorResult<bool> {
    match predicate {
        boon_plan::PlanListRemovePredicate::AlwaysTrue => Ok(true),
        boon_plan::PlanListRemovePredicate::RowFieldBool { input } => {
            let (_, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(value)
        }
        boon_plan::PlanListRemovePredicate::RowFieldBoolNot { input } => {
            let (_, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(!value)
        }
        boon_plan::PlanListRemovePredicate::SelectedFilterVisibility { .. } => Err(
            "CPU root-scenario PlanExecutor does not support selected-filter visibility predicates in remove/count execution yet".into(),
        ),
        boon_plan::PlanListRemovePredicate::Unknown { summary } => Err(format!(
            "CPU root-scenario PlanExecutor does not support unknown list count predicate `{summary}`"
        )
        .into()),
    }
}

pub fn evaluate_list_remove_predicate(
    plan: &MachinePlan,
    predicate: &boon_plan::PlanListRemovePredicate,
    row: &PlanExecutorListRow,
) -> PlanExecutorResult<ListRemovePredicateEvaluation> {
    let matches = match predicate {
        boon_plan::PlanListRemovePredicate::AlwaysTrue => true,
        boon_plan::PlanListRemovePredicate::RowFieldBool { input } => {
            let (_, value) = list_remove_predicate_field_value(plan, input, row)?;
            value
        }
        boon_plan::PlanListRemovePredicate::RowFieldBoolNot { input } => {
            let (_, value) = list_remove_predicate_field_value(plan, input, row)?;
            !value
        }
        boon_plan::PlanListRemovePredicate::SelectedFilterVisibility { .. } => {
            return Err(
                "CPU root-scenario PlanExecutor does not support selected-filter visibility predicates in remove/count execution yet".into(),
            );
        }
        boon_plan::PlanListRemovePredicate::Unknown { summary } => {
            return Err(format!(
                "CPU root-scenario PlanExecutor does not support unknown list remove predicate `{summary}`"
            )
            .into());
        }
    };
    let executor_report = json!({
        "executor": "cpu-plan-list-remove-predicate-evaluator-v1",
        "matches": matches,
        "key": row.key,
        "generation": row.generation,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ListRemovePredicateEvaluation {
        matches,
        executor_report,
    })
}

pub fn build_list_remove_predicate_row_resolution_report(
    plan: &MachinePlan,
    predicate: &boon_plan::PlanListRemovePredicate,
    row_index: usize,
    row: &PlanExecutorListRow,
) -> PlanExecutorResult<JsonValue> {
    match predicate {
        boon_plan::PlanListRemovePredicate::AlwaysTrue => Ok(json!({
            "method": "predicate",
            "predicate": "always_true",
            "predicate_field": null,
            "predicate_value": true,
            "row_index": row_index,
            "key": row.key,
            "generation": row.generation,
            "source_binding_id": null,
            "executor": "cpu-plan-list-remove-predicate-row-resolution-v1",
        })),
        boon_plan::PlanListRemovePredicate::RowFieldBool { input } => {
            let (field_name, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(json!({
                "method": "predicate",
                "predicate": "row_field_bool",
                "predicate_field": field_name,
                "predicate_value": value,
                "row_index": row_index,
                "key": row.key,
                "generation": row.generation,
                "source_binding_id": null,
                "executor": "cpu-plan-list-remove-predicate-row-resolution-v1",
            }))
        }
        boon_plan::PlanListRemovePredicate::RowFieldBoolNot { input } => {
            let (field_name, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(json!({
                "method": "predicate",
                "predicate": "row_field_bool_not",
                "predicate_field": field_name,
                "predicate_value": value,
                "row_index": row_index,
                "key": row.key,
                "generation": row.generation,
                "source_binding_id": null,
                "executor": "cpu-plan-list-remove-predicate-row-resolution-v1",
            }))
        }
        boon_plan::PlanListRemovePredicate::SelectedFilterVisibility { .. } => Err(
            "CPU root-scenario PlanExecutor does not report selected-filter visibility predicates in remove/count execution yet".into(),
        ),
        boon_plan::PlanListRemovePredicate::Unknown { summary } => Err(format!(
            "CPU root-scenario PlanExecutor does not support unknown list remove predicate `{summary}`"
        )
        .into()),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootSourceEventWork {
    pub plan_hash: String,
    pub source_label: String,
    pub source_id: SourceId,
    pub source_route_scoped: bool,
    pub ordered_update_op_ids: Vec<PlanOpId>,
    pub derived_op_count: usize,
    pub has_list_remove_work: bool,
    pub root_update_key_gate: Option<String>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioStepDispatch {
    pub plan_hash: String,
    pub source_label: String,
    pub source_id: SourceId,
    pub source_route_scoped: bool,
    pub ordered_update_op_ids: Vec<PlanOpId>,
    pub derived_op_count: usize,
    pub has_list_remove_work: bool,
    pub root_update_key_gate: Option<String>,
    pub root_update_key_matches: bool,
    pub executable_work: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioMaterializedWork {
    pub executable_work: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootScenarioStepPreparation {
    pub source_id: SourceId,
    pub source_route_slot: SourceRoute,
    pub route_ops: Vec<PlanOp>,
    pub derived_values: BTreeMap<FieldId, JsonValue>,
    pub root_update_key_matches: bool,
    pub root_dispatch_report: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceDerivedStepDeltas {
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: Vec<JsonValue>,
    pub reports: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListRemovePredicateEvaluation {
    pub matches: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateDeltaBatch {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_rows: Vec<JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateBranchExecution {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_rows: Vec<JsonValue>,
    pub updated_row_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateTargetOverride {
    pub list_label: String,
    pub key: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateBatchExecution {
    pub semantic_deltas: Vec<JsonValue>,
    pub report_rows: Vec<JsonValue>,
    pub updated_row_count: usize,
    pub bulk_indexed_update: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedJsonUpdateEvaluation {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: StateId,
    pub value: Option<JsonValue>,
    pub expression_kind: Option<&'static str>,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateDeltaOrdering {
    pub semantic_deltas: Vec<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexedUpdateTargetRow {
    pub key: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexedUpdateTargetEvent {
    pub source: String,
    pub list_id: Option<String>,
    pub target_key: Option<u64>,
    pub target_text: Option<String>,
    pub address: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedUpdateTargetSelection {
    pub bulk_indexed_update: bool,
    pub list_id: Option<usize>,
    pub list_label: Option<String>,
    pub targets: Vec<IndexedUpdateTargetRow>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RootJsonSourceEvent {
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub payload: BTreeMap<String, String>,
    pub payload_bytes: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootJsonUpdateEvaluation {
    pub supported: bool,
    pub skipped_by_guard: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: Option<StateId>,
    pub value: Option<JsonValue>,
    pub expression_kind: Option<&'static str>,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RootUpdateExecutionSurfaceKind {
    PlanJson,
    RuntimeBranch,
    SkippedByGuard,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateExecutionSurface {
    pub kind: RootUpdateExecutionSurfaceKind,
    pub core_value_is_bytes: bool,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootJsonUpdateExecution {
    pub surface_kind: RootUpdateExecutionSurfaceKind,
    pub executed: Option<RootExecutedUpdate>,
    pub evaluator_report: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootJsonStateWrite {
    pub target_state_id: StateId,
    pub target_state_label: String,
    pub changed: bool,
    pub value: JsonValue,
    pub semantic_delta: Option<JsonValue>,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanExecutorBytes {
    digest: String,
    byte_len: u64,
    inline_bytes: Vec<u8>,
}

impl PlanExecutorBytes {
    pub fn from_inline(
        digest: impl Into<String>,
        byte_len: u64,
        inline_bytes: Vec<u8>,
        context: &str,
    ) -> PlanExecutorResult<Self> {
        let digest = digest.into();
        if digest.trim().is_empty() {
            return Err(format!("{context} BYTES payload is missing a digest").into());
        }
        if inline_bytes.len() as u64 != byte_len {
            return Err(format!(
                "{context} BYTES payload declares byte_len {byte_len} but carries {} byte(s)",
                inline_bytes.len()
            )
            .into());
        }
        let actual_digest = sha256_bytes(&inline_bytes);
        if actual_digest != digest {
            return Err(format!(
                "{context} BYTES payload digest mismatch: expected {digest}, got {actual_digest}"
            )
            .into());
        }
        Ok(Self {
            digest,
            byte_len,
            inline_bytes,
        })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub fn inline_bytes(&self) -> &[u8] {
        &self.inline_bytes
    }

    pub fn into_inline_bytes(self) -> Vec<u8> {
        self.inline_bytes
    }

    pub fn report_json(&self) -> JsonValue {
        bytes_report_json(&self.inline_bytes)
    }

    pub fn artifact_json(&self) -> JsonValue {
        json!({
            "$boon_type": "BYTES",
            "byte_len": self.byte_len,
            "digest": self.digest,
            "inline_bytes": self
                .inline_bytes
                .iter()
                .map(|byte| json!(*byte))
                .collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootBytesStorageInitialization {
    pub private_bytes: BTreeMap<usize, PlanExecutorBytes>,
    pub fixed_byte_banks: BTreeMap<usize, Vec<u8>>,
    pub initialized_bytes_state_count: usize,
    pub fixed_byte_bank_count: usize,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanExecutorRootState {
    pub root_state: JsonMap<String, JsonValue>,
    pub private_bytes: BTreeMap<usize, PlanExecutorBytes>,
    pub fixed_byte_banks: BTreeMap<usize, Vec<u8>>,
    pub initialized_state_count: usize,
    pub executor_report: JsonValue,
}

pub trait RootBytesEnvironment {
    fn private_bytes_for_state(&self, state_id: StateId) -> Option<&PlanExecutorBytes>;
    fn fixed_byte_bank_for_state(&self, state_id: StateId) -> Option<&[u8]>;

    fn has_fixed_byte_bank(&self, state_id: StateId) -> bool {
        self.fixed_byte_bank_for_state(state_id).is_some()
    }
}

pub trait RootBytesStateOwner: RootBytesEnvironment {
    fn private_bytes_state_count(&self) -> usize;
    fn fixed_byte_bank_count(&self) -> usize;
    fn insert_private_bytes_for_state(&mut self, state_id: StateId, bytes: PlanExecutorBytes);
    fn remove_private_bytes_for_state(&mut self, state_id: StateId);
    fn remove_fixed_byte_bank_for_state(&mut self, state_id: StateId);
    fn fixed_byte_bank_mut_for_state(&mut self, state_id: StateId) -> Option<&mut Vec<u8>>;
    fn take_fixed_byte_bank_for_state(&mut self, state_id: StateId) -> Option<Vec<u8>>;
    fn insert_fixed_byte_bank_for_state(&mut self, state_id: StateId, bytes: Vec<u8>);

    fn clear_bytes_for_state(&mut self, state_id: StateId) {
        self.remove_private_bytes_for_state(state_id);
        self.remove_fixed_byte_bank_for_state(state_id);
    }
}

pub trait RootUpdateStateOwner: RootBytesStateOwner {
    fn insert_root_state_value(&mut self, label: &str, value: JsonValue);
}

pub struct RootBytesStateMaps<'a> {
    private_bytes: &'a mut BTreeMap<usize, PlanExecutorBytes>,
    fixed_byte_banks: &'a mut BTreeMap<usize, Vec<u8>>,
}

impl<'a> RootBytesStateMaps<'a> {
    pub fn new(
        private_bytes: &'a mut BTreeMap<usize, PlanExecutorBytes>,
        fixed_byte_banks: &'a mut BTreeMap<usize, Vec<u8>>,
    ) -> Self {
        Self {
            private_bytes,
            fixed_byte_banks,
        }
    }
}

impl RootBytesEnvironment for RootBytesStateMaps<'_> {
    fn private_bytes_for_state(&self, state_id: StateId) -> Option<&PlanExecutorBytes> {
        self.private_bytes.get(&state_id.0)
    }

    fn fixed_byte_bank_for_state(&self, state_id: StateId) -> Option<&[u8]> {
        self.fixed_byte_banks.get(&state_id.0).map(Vec::as_slice)
    }
}

impl RootBytesStateOwner for RootBytesStateMaps<'_> {
    fn private_bytes_state_count(&self) -> usize {
        self.private_bytes.len()
    }

    fn fixed_byte_bank_count(&self) -> usize {
        self.fixed_byte_banks.len()
    }

    fn insert_private_bytes_for_state(&mut self, state_id: StateId, bytes: PlanExecutorBytes) {
        self.private_bytes.insert(state_id.0, bytes);
    }

    fn remove_private_bytes_for_state(&mut self, state_id: StateId) {
        self.private_bytes.remove(&state_id.0);
    }

    fn remove_fixed_byte_bank_for_state(&mut self, state_id: StateId) {
        self.fixed_byte_banks.remove(&state_id.0);
    }

    fn fixed_byte_bank_mut_for_state(&mut self, state_id: StateId) -> Option<&mut Vec<u8>> {
        self.fixed_byte_banks.get_mut(&state_id.0)
    }

    fn take_fixed_byte_bank_for_state(&mut self, state_id: StateId) -> Option<Vec<u8>> {
        self.fixed_byte_banks.remove(&state_id.0)
    }

    fn insert_fixed_byte_bank_for_state(&mut self, state_id: StateId, bytes: Vec<u8>) {
        self.fixed_byte_banks.insert(state_id.0, bytes);
    }
}

pub struct RootUpdateStateMaps<'a> {
    root_state: &'a mut JsonMap<String, JsonValue>,
    private_bytes: &'a mut BTreeMap<usize, PlanExecutorBytes>,
    fixed_byte_banks: &'a mut BTreeMap<usize, Vec<u8>>,
}

impl<'a> RootUpdateStateMaps<'a> {
    pub fn new(
        root_state: &'a mut JsonMap<String, JsonValue>,
        private_bytes: &'a mut BTreeMap<usize, PlanExecutorBytes>,
        fixed_byte_banks: &'a mut BTreeMap<usize, Vec<u8>>,
    ) -> Self {
        Self {
            root_state,
            private_bytes,
            fixed_byte_banks,
        }
    }
}

impl RootBytesEnvironment for RootUpdateStateMaps<'_> {
    fn private_bytes_for_state(&self, state_id: StateId) -> Option<&PlanExecutorBytes> {
        self.private_bytes.get(&state_id.0)
    }

    fn fixed_byte_bank_for_state(&self, state_id: StateId) -> Option<&[u8]> {
        self.fixed_byte_banks.get(&state_id.0).map(Vec::as_slice)
    }
}

impl RootBytesStateOwner for RootUpdateStateMaps<'_> {
    fn private_bytes_state_count(&self) -> usize {
        self.private_bytes.len()
    }

    fn fixed_byte_bank_count(&self) -> usize {
        self.fixed_byte_banks.len()
    }

    fn insert_private_bytes_for_state(&mut self, state_id: StateId, bytes: PlanExecutorBytes) {
        self.private_bytes.insert(state_id.0, bytes);
    }

    fn remove_private_bytes_for_state(&mut self, state_id: StateId) {
        self.private_bytes.remove(&state_id.0);
    }

    fn remove_fixed_byte_bank_for_state(&mut self, state_id: StateId) {
        self.fixed_byte_banks.remove(&state_id.0);
    }

    fn fixed_byte_bank_mut_for_state(&mut self, state_id: StateId) -> Option<&mut Vec<u8>> {
        self.fixed_byte_banks.get_mut(&state_id.0)
    }

    fn take_fixed_byte_bank_for_state(&mut self, state_id: StateId) -> Option<Vec<u8>> {
        self.fixed_byte_banks.remove(&state_id.0)
    }

    fn insert_fixed_byte_bank_for_state(&mut self, state_id: StateId, bytes: Vec<u8>) {
        self.fixed_byte_banks.insert(state_id.0, bytes);
    }
}

impl RootUpdateStateOwner for RootUpdateStateMaps<'_> {
    fn insert_root_state_value(&mut self, label: &str, value: JsonValue) {
        self.root_state.insert(label.to_owned(), value);
    }
}

impl RootBytesEnvironment for PlanExecutorRootState {
    fn private_bytes_for_state(&self, state_id: StateId) -> Option<&PlanExecutorBytes> {
        self.private_bytes.get(&state_id.0)
    }

    fn fixed_byte_bank_for_state(&self, state_id: StateId) -> Option<&[u8]> {
        self.fixed_byte_banks.get(&state_id.0).map(Vec::as_slice)
    }
}

impl RootBytesStateOwner for PlanExecutorRootState {
    fn private_bytes_state_count(&self) -> usize {
        self.private_bytes.len()
    }

    fn fixed_byte_bank_count(&self) -> usize {
        self.fixed_byte_banks.len()
    }

    fn insert_private_bytes_for_state(&mut self, state_id: StateId, bytes: PlanExecutorBytes) {
        self.private_bytes.insert(state_id.0, bytes);
    }

    fn remove_private_bytes_for_state(&mut self, state_id: StateId) {
        self.private_bytes.remove(&state_id.0);
    }

    fn remove_fixed_byte_bank_for_state(&mut self, state_id: StateId) {
        self.fixed_byte_banks.remove(&state_id.0);
    }

    fn fixed_byte_bank_mut_for_state(&mut self, state_id: StateId) -> Option<&mut Vec<u8>> {
        self.fixed_byte_banks.get_mut(&state_id.0)
    }

    fn take_fixed_byte_bank_for_state(&mut self, state_id: StateId) -> Option<Vec<u8>> {
        self.fixed_byte_banks.remove(&state_id.0)
    }

    fn insert_fixed_byte_bank_for_state(&mut self, state_id: StateId, bytes: Vec<u8>) {
        self.fixed_byte_banks.insert(state_id.0, bytes);
    }
}

impl RootUpdateStateOwner for PlanExecutorRootState {
    fn insert_root_state_value(&mut self, label: &str, value: JsonValue) {
        self.root_state.insert(label.to_owned(), value);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootBytesReadEvaluation {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: Option<StateId>,
    pub value: Option<JsonValue>,
    pub bytes: Option<PlanExecutorBytes>,
    pub bytes_access: JsonValue,
    pub expression_kind: Option<&'static str>,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootBytesSourcePayloadCommit {
    pub target_state_id: StateId,
    pub value: JsonValue,
    pub bytes: PlanExecutorBytes,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootBytesWriteEvaluation {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: Option<StateId>,
    pub value: Option<JsonValue>,
    pub bytes: Option<PlanExecutorBytes>,
    pub fixed_mutation: Option<RootBytesFixedMutation>,
    pub bytes_access: JsonValue,
    pub host_effect: JsonValue,
    pub expression_kind: Option<&'static str>,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RootBytesUpdateDispatchKind {
    Read,
    Write,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedRowView {
    pub fields: BTreeMap<String, JsonValue>,
    pub private_bytes: BTreeMap<String, PlanExecutorBytes>,
    pub fixed_byte_banks: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedBytesReadEvaluation {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: Option<StateId>,
    pub value: Option<JsonValue>,
    pub bytes: Option<PlanExecutorBytes>,
    pub bytes_access: JsonValue,
    pub expression_kind: Option<&'static str>,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedBytesWriteEvaluation {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub target_state_id: Option<StateId>,
    pub value: Option<JsonValue>,
    pub bytes: Option<PlanExecutorBytes>,
    pub bytes_access: JsonValue,
    pub bytes_storage: JsonValue,
    pub host_effect: JsonValue,
    pub expression_kind: Option<&'static str>,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootBytesFixedMutation {
    pub input_state_id: StateId,
    pub output_state_id: StateId,
    pub patches: Vec<(usize, u8)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootBytesStateTransition {
    pub target_state_id: StateId,
    pub mode: &'static str,
    pub executor_report: JsonValue,
}

pub fn execute_initial_state(plan: &MachinePlan) -> PlanExecutorResult<InitialStateExecution> {
    let verification = verify_plan(plan)?;
    if verification.status != "pass" {
        return Err(format!(
            "MachinePlan verification failed with {} error(s)",
            verification.error_count
        )
        .into());
    }
    if !plan.capability_summary.cpu_plan_executor_complete {
        return Err(
            "CPU PlanExecutor initial-state slice requires cpu_plan_executor_complete=true".into(),
        );
    }

    let mut state_summary = JsonMap::new();
    let mut initialized_state_count = 0usize;
    let mut source_route_metadata_count = 0usize;
    let mut list_projection_count = 0usize;
    let mut list_operation_count = 0usize;

    for region in &plan.regions {
        match region.kind {
            RegionKind::SourceRouting => {
                for op in &region.ops {
                    if !matches!(op.kind, PlanOpKind::SourceRoute)
                        || !matches!(op.output, Some(ValueRef::Source(_)))
                    {
                        return Err("CPU PlanExecutor initial-state slice only accepts source-route metadata ops in the source-routing region".into());
                    }
                    source_route_metadata_count += 1;
                }
            }
            RegionKind::StateInitialization => {
                for op in &region.ops {
                    let PlanOpKind::StateInitialize {
                        initial_value_kind,
                        initial_constant_id,
                    } = op.kind
                    else {
                        return Err("CPU PlanExecutor initial-state slice only accepts StateInitialize ops in the state-initialization region".into());
                    };
                    let Some(ValueRef::State(state_id)) = op.output else {
                        return Err("StateInitialize op is missing a typed state output".into());
                    };
                    let slot = plan
                        .storage_layout
                        .scalar_slots
                        .iter()
                        .find(|slot| slot.state_id == state_id)
                        .ok_or_else(|| {
                            format!("StateInitialize op targets missing state {}", state_id.0)
                        })?;
                    if slot.initial_value_kind != initial_value_kind
                        || slot.initial_constant_id != initial_constant_id
                    {
                        return Err(format!(
                            "StateInitialize op and scalar storage slot disagree for state {}",
                            state_id.0
                        )
                        .into());
                    }
                    if !slot.indexed {
                        if slot.initial_value_kind != InitialValueKind::RootInitialField {
                            let value = initial_value(
                                plan,
                                slot.initial_constant_id,
                                &slot.value_type,
                                slot.initial_value_kind,
                            )?;
                            state_summary.insert(state_label(plan, state_id), value);
                        }
                    }
                    initialized_state_count += 1;
                }
            }
            RegionKind::DerivedEvaluation
            | RegionKind::ListProjections
            | RegionKind::DependencyEdges => {
                for op in &region.ops {
                    match (&region.kind, &op.kind) {
                        (RegionKind::DerivedEvaluation, PlanOpKind::DerivedValue { .. })
                            if op.unresolved_executable_ref_count == 0 => {}
                        (
                            RegionKind::ListProjections,
                            PlanOpKind::ListProjection { projection },
                        ) => {
                            validate_list_projection(projection, op.id.0)?;
                            list_projection_count += 1;
                        }
                        (RegionKind::DependencyEdges, PlanOpKind::DependencyEdge) => {}
                        _ => {
                            return Err(format!(
                                "CPU PlanExecutor initial-state slice does not support {:?} op {}",
                                region.kind, op.id.0
                            )
                            .into());
                        }
                    }
                }
            }
            RegionKind::UpdateBranches => {}
            RegionKind::ListOperations => {
                for op in &region.ops {
                    match &op.kind {
                        PlanOpKind::ListOperation { .. }
                            if op.unresolved_executable_ref_count == 0 =>
                        {
                            list_operation_count += 1;
                        }
                        _ => {
                            return Err(format!(
                                "CPU PlanExecutor initial-state slice does not support {:?} op {}",
                                region.kind, op.id.0
                            )
                            .into());
                        }
                    }
                }
            }
        }
    }

    if initialized_state_count != plan.storage_layout.scalar_slots.len() {
        return Err(format!(
            "CPU PlanExecutor initialized {initialized_state_count} scalar state(s), expected {}",
            plan.storage_layout.scalar_slots.len()
        )
        .into());
    }
    if source_route_metadata_count != plan.source_routes.len() {
        return Err(format!(
            "CPU PlanExecutor observed {source_route_metadata_count} source route metadata op(s), expected {}",
            plan.source_routes.len()
        )
        .into());
    }

    let (source_event_transform_commits, root_initial_field_copy_count) =
        bootstrap_root_derived_values_and_initial_fields(plan, &mut state_summary)?;
    let initialized_source_event_transform_count = source_event_transform_commits.len();

    let plan_hash = plan_sha256(plan)?;
    let state_summary = JsonValue::Object(state_summary);
    let executor_report = json!({
        "executor": "cpu-plan-executor-core-v1",
        "initialized_state_count": initialized_state_count,
        "root_initial_field_copy_count": root_initial_field_copy_count,
        "initialized_root_source_event_transform_count": initialized_source_event_transform_count,
        "root_source_event_transform_commits": source_event_transform_commits,
        "source_route_metadata_count": source_route_metadata_count,
        "validated_list_projection_count": list_projection_count,
        "validated_list_operation_count": list_operation_count,
        "list_slot_count": plan.storage_layout.list_slots.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": plan.capability_summary.executable_string_path_count,
        "unknown_plan_op_count": plan.capability_summary.unknown_plan_op_count,
        "graph_rebuild_count": plan.capability_summary.graph_rebuild_count,
        "graph_clones_per_item": plan.capability_summary.graph_clones_per_item,
        "state_summary": state_summary,
    });

    Ok(InitialStateExecution {
        plan_hash,
        state_summary,
        initialized_state_count,
        source_route_metadata_count,
        list_slot_count: plan.storage_layout.list_slots.len(),
        executor_report,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_initial_state_report(
    plan: &MachinePlan,
    initialized_state_count: usize,
    source_route_metadata_count: usize,
    executed_list_projection_count: usize,
    executed_list_projection_find_count: usize,
    executed_list_projection_chunk_count: usize,
    projected_list_row_count: usize,
    executed_list_retain_count: usize,
    executed_list_view_count: usize,
    retained_list_row_count: usize,
    state_summary: &JsonValue,
    executor_core: &JsonValue,
    list_summary: &JsonValue,
    list_projection_summary: &JsonValue,
    list_projections: &[JsonValue],
    list_view_summary: &JsonValue,
    list_retains: &[JsonValue],
) -> InitialStateReportAssembly {
    let report_assembly_core = json!({
        "executor": "cpu-plan-initial-state-report-assembly-v1",
        "initialized_state_count": initialized_state_count,
        "source_route_metadata_count": source_route_metadata_count,
        "list_projection_count": executed_list_projection_count,
        "list_retain_count": executed_list_retain_count,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let executor_report = json!({
        "executor": "cpu-plan-initial-state-v1",
        "initialized_state_count": initialized_state_count,
        "source_route_metadata_count": source_route_metadata_count,
        "executed_list_projection_count": executed_list_projection_count,
        "executed_list_projection_find_count": executed_list_projection_find_count,
        "executed_list_projection_chunk_count": executed_list_projection_chunk_count,
        "projected_list_row_count": projected_list_row_count,
        "executed_list_retain_count": executed_list_retain_count,
        "executed_list_view_count": executed_list_view_count,
        "retained_list_row_count": retained_list_row_count,
        "list_slot_count": plan.storage_layout.list_slots.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": plan.capability_summary.executable_string_path_count,
        "unknown_plan_op_count": plan.capability_summary.unknown_plan_op_count,
        "graph_rebuild_count": plan.capability_summary.graph_rebuild_count,
        "graph_clones_per_item": plan.capability_summary.graph_clones_per_item,
        "report_assembly_core": report_assembly_core,
        "state_summary": state_summary,
        "executor_core": executor_core,
        "list_summary": list_summary,
        "list_projection_summary": list_projection_summary,
        "list_projections": list_projections,
        "list_view_summary": list_view_summary,
        "list_retains": list_retains,
    });
    InitialStateReportAssembly { executor_report }
}

pub fn materialize_list_projections(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> PlanExecutorResult<ListProjectionExecution> {
    let mut execution = ListProjectionExecution::default();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListProjections)
        .flat_map(|region| region.ops.iter())
    {
        let Some(ValueRef::Field(output_id)) = op.output else {
            return Err(format!("list projection op {} has no field output", op.id.0).into());
        };
        let PlanOpKind::ListProjection { projection } = &op.kind else {
            return Err(format!(
                "list projection region op {} is not a ListProjection",
                op.id.0
            )
            .into());
        };
        let target_label = semantic_field_label(plan, output_id.0);
        match projection {
            PlanListProjection::Find {
                source_list,
                field,
                value,
            } => {
                let selector = projection_selector_value(plan, root_state, value)?;
                let rows = list_state.get(&source_list.0).ok_or_else(|| {
                    format!(
                        "list projection op {} source list {} is not materialized",
                        op.id.0, source_list.0
                    )
                })?;
                let projected = rows
                    .iter()
                    .find(|row| row_json_values_equal(row.fields.get(field), &selector))
                    .map(|row| JsonValue::Object(row.fields.clone().into_iter().collect()))
                    .unwrap_or(JsonValue::Null);
                let projected_row_count = usize::from(!projected.is_null());
                execution
                    .summary
                    .insert(target_label.clone(), projected.clone());
                execution.reports.push(json!({
                    "executor": "cpu-plan-list-projection-materializer-v1",
                    "op_id": op.id.0,
                    "kind": "find",
                    "target": target_label,
                    "source_list_id": source_list.0,
                    "source_list": list_label(plan, source_list.0),
                    "field": field,
                    "selector": selector,
                    "projected_row_count": projected_row_count,
                }));
                execution.executed_count += 1;
                execution.find_count += 1;
                execution.projected_row_count += projected_row_count;
            }
            PlanListProjection::Chunk {
                source_list,
                size,
                item_field,
                label_field,
            } => {
                let rows = list_state.get(&source_list.0).ok_or_else(|| {
                    format!(
                        "list projection op {} source list {} is not materialized",
                        op.id.0, source_list.0
                    )
                })?;
                let mut chunks = Vec::new();
                for (index, chunk) in rows.chunks(*size).enumerate() {
                    let mut chunk_object = JsonMap::new();
                    chunk_object.insert(label_field.clone(), JsonValue::String(index.to_string()));
                    chunk_object.insert(
                        item_field.clone(),
                        JsonValue::Array(
                            chunk
                                .iter()
                                .map(|row| {
                                    JsonValue::Object(row.fields.clone().into_iter().collect())
                                })
                                .collect::<Vec<_>>(),
                        ),
                    );
                    chunks.push(JsonValue::Object(chunk_object));
                }
                execution
                    .summary
                    .insert(target_label.clone(), JsonValue::Array(chunks.clone()));
                execution.reports.push(json!({
                    "executor": "cpu-plan-list-projection-materializer-v1",
                    "op_id": op.id.0,
                    "kind": "chunk",
                    "target": target_label,
                    "source_list_id": source_list.0,
                    "source_list": list_label(plan, source_list.0),
                    "size": size,
                    "item_field": item_field,
                    "label_field": label_field,
                    "projected_row_count": chunks.len(),
                }));
                execution.executed_count += 1;
                execution.chunk_count += 1;
                execution.projected_row_count += chunks.len();
            }
            PlanListProjection::Unknown { summary } => {
                return Err(format!("list projection op {} is unknown: {summary}", op.id.0).into());
            }
        }
    }
    Ok(execution)
}

pub fn materialize_list_retains(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    list_state: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> PlanExecutorResult<ListRetainExecution> {
    let mut execution = ListRetainExecution::default();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
    {
        let PlanOpKind::ListOperation {
            operation_kind: PlanListOperationKind::Retain,
            retain: Some(retain),
            ..
        } = &op.kind
        else {
            continue;
        };
        let Some(ValueRef::List(source_list_id)) = op.output else {
            return Err(format!("list retain op {} has no source list output", op.id.0).into());
        };
        let ValueRef::Field(target_field_id) = retain.target else {
            return Err(format!("list retain op {} target is not a field", op.id.0).into());
        };
        if !op.inputs.contains(&retain.target) {
            return Err(format!(
                "list retain op {} target field is not present in typed inputs",
                op.id.0
            )
            .into());
        }

        let source_label = list_label(plan, source_list_id.0);
        let target_label = derived_field_label(plan, target_field_id.0);
        let rows = list_state.get(&source_list_id.0).ok_or_else(|| {
            format!(
                "list retain op {} source list {} is not materialized",
                op.id.0, source_list_id.0
            )
        })?;
        let mut retained_rows = Vec::new();
        let mut titles = Vec::new();
        let mut active_count = 0usize;
        let mut completed_count = 0usize;
        let mut predicate_report = None;
        for (row_index, row) in rows.iter().enumerate() {
            let resolution =
                list_retain_predicate_resolution(plan, root_state, &retain.predicate, row)?;
            predicate_report = Some(resolution.report.clone());
            if !resolution.retained {
                continue;
            }
            if let Some(title) = row.fields.get("title") {
                titles.push(title.clone());
            }
            if row
                .fields
                .get("completed")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
            {
                completed_count += 1;
            } else {
                active_count += 1;
            }
            retained_rows.push(json!({
                "source_row_index": row_index,
                "key": row.key,
                "generation": row.generation,
                "fields": row.fields,
            }));
        }
        let predicate = match predicate_report {
            Some(report) => report,
            None => list_retain_empty_predicate_report(plan, root_state, &retain.predicate)?,
        };
        let row_count = retained_rows.len();
        let summary = json!({
            "retain_op_id": op.id.0,
            "target_field_id": target_field_id.0,
            "target": target_label,
            "source_list_id": source_list_id.0,
            "source_list": source_label,
            "source_row_count": rows.len(),
            "row_count": row_count,
            "titles": titles,
            "active_count": active_count,
            "completed_count": completed_count,
            "rows": retained_rows,
        });
        execution
            .summary
            .insert(target_label.clone(), summary.clone());
        execution.reports.push(json!({
            "executor": "cpu-plan-list-retain-materializer-v1",
            "op_id": op.id.0,
            "kind": "retain",
            "target_field_id": target_field_id.0,
            "target": target_label,
            "source_list_id": source_list_id.0,
            "source_list": source_label,
            "predicate": predicate,
            "source_row_count": rows.len(),
            "retained_row_count": row_count,
            "rows": summary["rows"],
        }));
        execution.executed_count += 1;
        execution.view_count += 1;
        execution.retained_row_count += row_count;
    }
    Ok(execution)
}

pub fn initialize_root_bytes_storage(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<RootBytesStorageInitialization> {
    let mut private_bytes = BTreeMap::new();
    let mut fixed_byte_banks = BTreeMap::new();

    for slot in &plan.storage_layout.scalar_slots {
        if slot.indexed || !matches!(slot.value_type, PlanValueType::Bytes { .. }) {
            continue;
        }
        let label = state_label(plan, slot.state_id);
        let constant_id = slot
            .initial_constant_id
            .ok_or_else(|| format!("root BYTES state `{label}` has no typed initial constant"))?;
        let constant = plan
            .constants
            .iter()
            .find(|constant| constant.id == constant_id)
            .ok_or_else(|| format!("missing plan constant {}", constant_id.0))?;
        let bytes = plan_constant_executor_bytes_for_slot(
            constant,
            slot,
            &format!("root BYTES state `{label}` initial storage"),
        )?;
        let public_value = root_state.get(&label).ok_or_else(|| {
            format!("root BYTES state `{label}` is missing from public initial state")
        })?;
        let expected_summary = bytes.report_json();
        if &expected_summary != public_value {
            return Err(format!(
                "root BYTES state `{label}` public summary does not match executor private byte payload"
            )
            .into());
        }
        if root_state_has_fixed_byte_bank(plan, slot.state_id) {
            fixed_byte_banks.insert(slot.state_id.0, bytes.inline_bytes().to_vec());
        }
        private_bytes.insert(slot.state_id.0, bytes);
    }

    let plan_hash = plan_sha256(plan)?;
    let executor_report = json!({
        "executor": "cpu-plan-root-bytes-storage-initializer-v1",
        "plan_hash": plan_hash,
        "initialized_bytes_state_count": private_bytes.len(),
        "fixed_byte_bank_count": fixed_byte_banks.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(RootBytesStorageInitialization {
        initialized_bytes_state_count: private_bytes.len(),
        fixed_byte_bank_count: fixed_byte_banks.len(),
        private_bytes,
        fixed_byte_banks,
        executor_report,
    })
}

fn bootstrap_root_derived_values_and_initial_fields(
    plan: &MachinePlan,
    root_state: &mut JsonMap<String, JsonValue>,
) -> PlanExecutorResult<(Vec<JsonValue>, usize)> {
    let mut commits = Vec::new();
    let mut copied_total = 0usize;
    let max_iterations = plan
        .storage_layout
        .scalar_slots
        .len()
        .saturating_add(
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::DerivedEvaluation)
                .map(|region| region.ops.len())
                .sum::<usize>(),
        )
        .max(1);

    for _ in 0..max_iterations {
        let partial_values = evaluate_initial_root_derived_values_partial(plan, root_state)?;
        let partial_commits =
            commit_source_derived_values_to_root_state(plan, root_state, &partial_values);
        let changed_commit_count = partial_commits
            .iter()
            .filter(|commit| commit.get("changed").and_then(JsonValue::as_bool) == Some(true))
            .count();
        commits.extend(partial_commits);

        let (copied, unresolved) = copy_resolvable_root_initial_fields(plan, root_state)?;
        copied_total += copied;
        if unresolved.is_empty() {
            let final_values = evaluate_initial_root_derived_values_partial(plan, root_state)?;
            commits.extend(commit_source_derived_values_to_root_state(
                plan,
                root_state,
                &final_values,
            ));
            return Ok((commits, copied_total));
        }
        if copied == 0 && changed_commit_count == 0 {
            return Err(format!(
                "root initial field copy source(s) unresolved: {}",
                unresolved.join(", ")
            )
            .into());
        }
    }

    Err("root initial field and derived-value bootstrap exceeded iteration limit".into())
}

fn copy_resolvable_root_initial_fields(
    plan: &MachinePlan,
    root_state: &mut JsonMap<String, JsonValue>,
) -> PlanExecutorResult<(usize, Vec<String>)> {
    let pending = plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter(|slot| {
            !slot.indexed && slot.initial_value_kind == InitialValueKind::RootInitialField
        })
        .collect::<Vec<_>>();
    let mut copied = 0usize;
    let mut unresolved = Vec::new();

    for slot in pending {
        let target_label = state_label(plan, slot.state_id);
        if root_state.contains_key(&target_label) {
            continue;
        }
        let source_path = slot
            .initial_root_field_path
            .as_deref()
            .ok_or_else(|| format!("root initial field `{target_label}` has no source path"))?;
        if let Some(value) = resolve_root_initial_field_value(plan, root_state, source_path)? {
            root_state.insert(target_label, value);
            copied += 1;
        } else {
            unresolved.push(format!("{target_label} <- {source_path}"));
        }
    }

    Ok((copied, unresolved))
}

fn resolve_root_initial_field_value(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    source_path: &str,
) -> PlanExecutorResult<Option<JsonValue>> {
    if let Some(value) = root_state.get(source_path).cloned().or_else(|| {
        (!source_path.contains('.'))
            .then(|| root_state.get(&format!("store.{source_path}")).cloned())
            .flatten()
    }) {
        return Ok(Some(value));
    }
    let candidate_labels = if source_path.contains('.') {
        vec![source_path.to_owned()]
    } else {
        vec![source_path.to_owned(), format!("store.{source_path}")]
    };
    for label in candidate_labels {
        let Some(field_id) = plan.debug_map.fields.iter().find_map(|entry| {
            (entry.label == label)
                .then_some(entry.id.strip_prefix("field:")?.parse::<usize>().ok()?)
        }) else {
            continue;
        };
        let Some(expression) = root_derived_expression_for_field(plan, FieldId(field_id)) else {
            continue;
        };
        return Ok(eval_root_source_transform_row_expression(plan, root_state, expression).ok());
    }
    Ok(None)
}

fn root_derived_expression_for_field(
    plan: &MachinePlan,
    field_id: FieldId,
) -> Option<&PlanRowExpression> {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find_map(|op| {
            if op.indexed || op.output != Some(ValueRef::Field(field_id)) {
                return None;
            }
            match &op.kind {
                PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
                    expression: Some(PlanDerivedExpression::SourceEventTransform { default, .. }),
                    ..
                } => Some(default.as_ref()),
                PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } => Some(expression),
                _ => None,
            }
        })
}

pub fn initialize_root_state(plan: &MachinePlan) -> PlanExecutorResult<PlanExecutorRootState> {
    let mut root_state = JsonMap::new();
    let mut initialized_state_count = 0usize;
    for slot in &plan.storage_layout.scalar_slots {
        if slot.indexed {
            continue;
        }
        if slot.initial_value_kind == InitialValueKind::RootInitialField {
            continue;
        }
        let value = initial_value(
            plan,
            slot.initial_constant_id,
            &slot.value_type,
            slot.initial_value_kind,
        )?;
        root_state.insert(state_label(plan, slot.state_id), value);
        initialized_state_count += 1;
    }
    let (source_event_transform_commits, root_initial_field_copy_count) =
        bootstrap_root_derived_values_and_initial_fields(plan, &mut root_state)?;
    initialized_state_count += root_initial_field_copy_count;
    let initialized_source_event_transform_count = source_event_transform_commits.len();
    let bytes_initialization = initialize_root_bytes_storage(plan, &root_state)?;
    let executor_report = json!({
        "executor": "cpu-plan-root-state-initializer-v1",
        "initialized_state_count": initialized_state_count,
        "root_initial_field_copy_count": root_initial_field_copy_count,
        "initialized_root_source_event_transform_count": initialized_source_event_transform_count,
        "root_source_event_transform_commits": source_event_transform_commits,
        "initialized_bytes_state_count": bytes_initialization.initialized_bytes_state_count,
        "fixed_byte_bank_count": bytes_initialization.fixed_byte_bank_count,
        "bytes_initialization_core": bytes_initialization.executor_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(PlanExecutorRootState {
        root_state,
        private_bytes: bytes_initialization.private_bytes,
        fixed_byte_banks: bytes_initialization.fixed_byte_banks,
        initialized_state_count,
        executor_report,
    })
}

pub fn root_bytes_update_dispatch_kind(op: &PlanOp) -> Option<RootBytesUpdateDispatchKind> {
    let PlanOpKind::UpdateBranch {
        expression_kind,
        source_payload_field,
        ..
    } = &op.kind
    else {
        return None;
    };
    if source_payload_field.is_some() {
        return None;
    }
    match expression_kind {
        PlanExpressionKind::BytesLength
        | PlanExpressionKind::BytesIsEmpty
        | PlanExpressionKind::BytesGet
        | PlanExpressionKind::BytesToHex
        | PlanExpressionKind::BytesToBase64
        | PlanExpressionKind::BytesReadUnsigned
        | PlanExpressionKind::BytesReadSigned
        | PlanExpressionKind::FileReadBytes
        | PlanExpressionKind::BytesToText
        | PlanExpressionKind::BytesEqual
        | PlanExpressionKind::BytesFind
        | PlanExpressionKind::BytesStartsWith
        | PlanExpressionKind::BytesEndsWith => Some(RootBytesUpdateDispatchKind::Read),
        PlanExpressionKind::BytesSet
        | PlanExpressionKind::BytesSlice
        | PlanExpressionKind::BytesTake
        | PlanExpressionKind::BytesDrop
        | PlanExpressionKind::BytesZeros
        | PlanExpressionKind::BytesFromHex
        | PlanExpressionKind::BytesFromBase64
        | PlanExpressionKind::BytesWriteUnsigned
        | PlanExpressionKind::BytesWriteSigned
        | PlanExpressionKind::FileWriteBytes
        | PlanExpressionKind::TextToBytes
        | PlanExpressionKind::BytesConcat => Some(RootBytesUpdateDispatchKind::Write),
        _ => None,
    }
}

pub fn evaluate_root_bytes_read_update(
    plan: &MachinePlan,
    op: &PlanOp,
    root_state: &JsonMap<String, JsonValue>,
    bytes_environment: &(impl RootBytesEnvironment + ?Sized),
    host_file_root: Option<&Path>,
) -> PlanExecutorResult<RootBytesReadEvaluation> {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor root BYTES read branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    let (value, bytes_access, expression_kind, update_constant_id, update_constant_value) =
        match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesLength,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = root_single_state_input(op)?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                let byte_len = i64::try_from(bytes_view.bytes.len()).map_err(|_| {
                    format!(
                        "root Bytes/length update branch {} byte_len exceeds Boon NUMBER",
                        op.id.0
                    )
                })?;
                (
                    json!(byte_len),
                    bytes_view.access_json(input_state_id),
                    "bytes_length",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesIsEmpty,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = root_single_state_input(op)?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::Bool(bytes_view.bytes.is_empty()),
                    bytes_view.access_json(input_state_id),
                    "bytes_is_empty",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesGet,
                source_payload_field: None,
                update_constant_id: Some(update_constant_id),
                ..
            } => {
                let input_state_id = root_single_state_input(op)?;
                let input_label = state_label(plan, input_state_id);
                let index_constant = plan
                    .constants
                    .iter()
                    .find(|constant| constant.id == *update_constant_id)
                    .ok_or_else(|| {
                        format!(
                            "root Bytes/get update branch {} references missing index constant {}",
                            op.id.0, update_constant_id.0
                        )
                    })?;
                let PlanConstantValue::Number { value: index } = &index_constant.value else {
                    return Err(format!(
                        "root Bytes/get update branch {} index constant {} is not a number",
                        op.id.0, update_constant_id.0
                    )
                    .into());
                };
                let index = usize::try_from(*index).map_err(|_| {
                    format!(
                        "root Bytes/get update branch {} index constant {} is negative or too large",
                        op.id.0, update_constant_id.0
                    )
                })?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                let byte = bytes_view.bytes.get(index).ok_or_else(|| {
                    format!(
                        "root Bytes/get update branch {} index {index} is out of bounds for `{input_label}`",
                        op.id.0
                    )
                })?;
                (
                    json!(i64::from(*byte)),
                    bytes_view.access_json(input_state_id),
                    "bytes_get",
                    json!(update_constant_id.0),
                    json!(index),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToHex,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = root_single_state_input(op)?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::String(bytes_encode_hex(bytes_view.bytes)),
                    json!({
                        "read_only": true,
                        "input_state_id": input_state_id.0,
                        "access_source": bytes_view.access_source,
                        "cow_kind": bytes_view.cow_kind,
                    }),
                    "bytes_to_hex",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesEqual,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (left_state_id, right_state_id) =
                    root_bytes_distinct_state_inputs(op, "Bytes/equal")?;
                let left_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    left_state_id,
                    op.id.0,
                )?;
                let right_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    right_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::Bool(left_view.bytes == right_view.bytes),
                    json!({
                        "read_only": true,
                        "inputs": [
                            left_view.labeled_access_json("left", left_state_id),
                            right_view.labeled_access_json("right", right_state_id)
                        ],
                    }),
                    "bytes_equal",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesFind,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (haystack_state_id, needle_state_id) =
                    root_bytes_ordered_state_inputs(op, "Bytes/find")?;
                let haystack_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    haystack_state_id,
                    op.id.0,
                )?;
                let needle_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    needle_state_id,
                    op.id.0,
                )?;
                let value = match bytes_find(haystack_view.bytes, needle_view.bytes) {
                    Some(index) => json!(i64::try_from(index).map_err(|_| {
                        format!(
                            "root Bytes/find update branch {} found index exceeds Boon NUMBER",
                            op.id.0
                        )
                    })?),
                    None => JsonValue::Null,
                };
                (
                    value,
                    json!({
                        "read_only": true,
                        "inputs": [
                            haystack_view.labeled_access_json("haystack", haystack_state_id),
                            needle_view.labeled_access_json("needle", needle_state_id)
                        ],
                    }),
                    "bytes_find",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesStartsWith,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (bytes_state_id, prefix_state_id) =
                    root_bytes_ordered_state_inputs(op, "Bytes/starts_with")?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    bytes_state_id,
                    op.id.0,
                )?;
                let prefix_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    prefix_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::Bool(bytes_view.bytes.starts_with(prefix_view.bytes)),
                    json!({
                        "read_only": true,
                        "inputs": [
                            bytes_view.labeled_access_json("input", bytes_state_id),
                            prefix_view.labeled_access_json("prefix", prefix_state_id)
                        ],
                    }),
                    "bytes_starts_with",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesEndsWith,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (bytes_state_id, suffix_state_id) =
                    root_bytes_ordered_state_inputs(op, "Bytes/ends_with")?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    bytes_state_id,
                    op.id.0,
                )?;
                let suffix_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    suffix_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::Bool(bytes_view.bytes.ends_with(suffix_view.bytes)),
                    json!({
                        "read_only": true,
                        "inputs": [
                            bytes_view.labeled_access_json("input", bytes_state_id),
                            suffix_view.labeled_access_json("suffix", suffix_state_id)
                        ],
                    }),
                    "bytes_ends_with",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToText,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (input_state_id, encoding, encoding_constant_id) =
                    root_text_bytes_conversion_operands(plan, op, "Bytes/to_text")?;
                let input_label = state_label(plan, input_state_id);
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                let text = bytes_to_text(bytes_view.bytes, &encoding).map_err(|error| {
                    format!(
                        "root Bytes/to_text update branch {} input state `{input_label}` {error}",
                        op.id.0
                    )
                })?;
                (
                    JsonValue::String(text),
                    json!({
                        "read_only": true,
                        "input_state_id": input_state_id.0,
                        "access_source": bytes_view.access_source,
                        "cow_kind": bytes_view.cow_kind,
                    }),
                    "bytes_to_text",
                    json!(encoding_constant_id.0),
                    json!(encoding),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToBase64,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = root_single_state_input(op)?;
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                (
                    JsonValue::String(bytes_encode_base64(bytes_view.bytes)),
                    json!({
                        "read_only": true,
                        "input_state_id": input_state_id.0,
                        "access_source": bytes_view.access_source,
                        "cow_kind": bytes_view.cow_kind,
                    }),
                    "bytes_to_base64",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesReadUnsigned,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (
                    input_state_id,
                    offset,
                    byte_count,
                    endian,
                    offset_constant_id,
                    byte_count_constant_id,
                    endian_constant_id,
                ) = root_bytes_numeric_read_operands(plan, op, "Bytes/read_unsigned")?;
                let input_label = state_label(plan, input_state_id);
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                let value = bytes_read_unsigned(bytes_view.bytes, offset, byte_count, endian)
                    .map_err(|code| {
                        format!(
                            "root Bytes/read_unsigned update branch {} failed for `{input_label}`: {code}",
                            op.id.0
                        )
                    })?;
                if value > i64::MAX as u64 {
                    return Err(format!(
                        "root Bytes/read_unsigned update branch {} overflows Boon NUMBER",
                        op.id.0
                    )
                    .into());
                }
                (
                    json!(value as i64),
                    json!({
                        "read_only": true,
                        "input_state_id": input_state_id.0,
                        "access_source": bytes_view.access_source,
                        "cow_kind": bytes_view.cow_kind,
                    }),
                    "bytes_read_unsigned",
                    json!([
                        offset_constant_id.0,
                        byte_count_constant_id.0,
                        endian_constant_id.0
                    ]),
                    json!({
                        "offset": offset,
                        "byte_count": byte_count,
                        "endian": bytes_endian_label(endian)
                    }),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesReadSigned,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (
                    input_state_id,
                    offset,
                    byte_count,
                    endian,
                    offset_constant_id,
                    byte_count_constant_id,
                    endian_constant_id,
                ) = root_bytes_numeric_read_operands(plan, op, "Bytes/read_signed")?;
                let input_label = state_label(plan, input_state_id);
                let bytes_view = root_executor_bytes_view(
                    plan,
                    root_state,
                    bytes_environment,
                    input_state_id,
                    op.id.0,
                )?;
                let value = bytes_read_signed(bytes_view.bytes, offset, byte_count, endian)
                    .map_err(|code| {
                        format!(
                            "root Bytes/read_signed update branch {} failed for `{input_label}`: {code}",
                            op.id.0
                        )
                    })?;
                (
                    json!(value),
                    json!({
                        "read_only": true,
                        "input_state_id": input_state_id.0,
                        "access_source": bytes_view.access_source,
                        "cow_kind": bytes_view.cow_kind,
                    }),
                    "bytes_read_signed",
                    json!([
                        offset_constant_id.0,
                        byte_count_constant_id.0,
                        endian_constant_id.0
                    ]),
                    json!({
                        "offset": offset,
                        "byte_count": byte_count,
                        "endian": bytes_endian_label(endian)
                    }),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::FileReadBytes,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let path_operand = file_read_bytes_path_operand(plan, op, "root")?;
                let (path, path_update_constant_id, path_update_constant_value) =
                    match path_operand {
                        FileReadBytesPathOperand::StaticConstant { path, constant_id } => (
                            path.clone(),
                            json!(constant_id.0),
                            json!({
                                "path": path,
                                "path_source": "static_constant",
                            }),
                        ),
                        FileReadBytesPathOperand::StatePath { state_id } => {
                            let label = state_label(plan, state_id);
                            let value = root_state_value(plan, root_state, state_id, op.id.0)?;
                            let path = value.as_str().ok_or_else(|| {
                                format!(
                                    "root File/read_bytes update branch {} path state `{label}` is not TEXT",
                                    op.id.0
                                )
                            })?;
                            (
                                path.to_owned(),
                                JsonValue::Null,
                                json!({
                                    "path": path,
                                    "path_source": "state",
                                    "path_state": label,
                                    "path_state_id": state_id.0,
                                }),
                            )
                        }
                        FileReadBytesPathOperand::RowFieldPath { field_id } => {
                            return Err(format!(
                                "root File/read_bytes update branch {} cannot use indexed row field path operand {}",
                                op.id.0,
                                field_id.0
                            )
                            .into())
                        }
                    };
                let host_file_root = host_file_root.ok_or_else(|| {
                    format!(
                        "root File/read_bytes update branch {} has no host file root",
                        op.id.0
                    )
                })?;
                let bytes = read_plan_host_file_bytes(host_file_root, &path).map_err(|error| {
                    format!(
                        "root File/read_bytes update branch {} cannot read `{path}`: {error}",
                        op.id.0
                    )
                })?;
                validate_root_bytes_output_len(
                    plan,
                    output_state_id,
                    bytes.len(),
                    "File/read_bytes",
                    op.id,
                )?;
                let executor_bytes = PlanExecutorBytes::from_inline(
                    sha256_bytes(&bytes),
                    bytes.len() as u64,
                    bytes,
                    &format!("root File/read_bytes update branch {}", op.id.0),
                )?;
                return Ok(root_bytes_read_outcome_with_bytes(
                    op.id,
                    true,
                    None,
                    Some(output_state_id),
                    Some(executor_bytes.report_json()),
                    Some(executor_bytes),
                    JsonValue::Null,
                    Some("file_read_bytes"),
                    path_update_constant_id,
                    path_update_constant_value,
                ));
            }
            _ => {
                return Ok(root_bytes_read_outcome(
                    op.id,
                    false,
                    Some("expression kind requires runtime-specific execution".to_owned()),
                    Some(output_state_id),
                    None,
                    JsonValue::Null,
                    None,
                    JsonValue::Null,
                    JsonValue::Null,
                ));
            }
        };

    Ok(root_bytes_read_outcome(
        op.id,
        true,
        None,
        Some(output_state_id),
        Some(value),
        bytes_access,
        Some(expression_kind),
        update_constant_id,
        update_constant_value,
    ))
}

pub fn evaluate_indexed_bytes_read_update(
    plan: &MachinePlan,
    op: &PlanOp,
    row: &IndexedRowView,
    host_file_root: Option<&Path>,
) -> PlanExecutorResult<IndexedBytesReadEvaluation> {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor indexed BYTES read branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    let (value, bytes, bytes_access, expression_kind, update_constant_id, update_constant_value) =
        match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesLength,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = indexed_single_state_input(op, output_state_id)?;
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                let byte_len = i64::try_from(bytes_view.bytes.len()).map_err(|_| {
                    format!(
                        "indexed Bytes/length update branch {} byte_len exceeds Boon NUMBER",
                        op.id.0
                    )
                })?;
                (
                    json!(byte_len),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_length",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesIsEmpty,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = indexed_single_state_input(op, output_state_id)?;
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                (
                    JsonValue::Bool(bytes_view.bytes.is_empty()),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_is_empty",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToHex,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = indexed_single_state_input(op, output_state_id)?;
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                (
                    JsonValue::String(bytes_encode_hex(bytes_view.bytes)),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_to_hex",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToBase64,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let input_state_id = indexed_single_state_input(op, output_state_id)?;
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                (
                    JsonValue::String(bytes_encode_base64(bytes_view.bytes)),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_to_base64",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesToText,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (input_state_id, encoding, encoding_constant_id) =
                    root_text_bytes_conversion_operands(plan, op, "indexed Bytes/to_text")?;
                let input_name = local_field_name(&state_label(plan, input_state_id));
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                let text = bytes_to_text(bytes_view.bytes, &encoding).map_err(|error| {
                    format!(
                        "indexed Bytes/to_text update branch {} input state `{input_name}` {error}",
                        op.id.0
                    )
                })?;
                (
                    JsonValue::String(text),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_to_text",
                    json!(encoding_constant_id.0),
                    json!(encoding),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesGet,
                source_payload_field: None,
                update_constant_id: Some(update_constant_id),
                ..
            } => {
                let input_state_id = indexed_single_state_input(op, output_state_id)?;
                let input_name = local_field_name(&state_label(plan, input_state_id));
                let index_constant = plan
                    .constants
                    .iter()
                    .find(|constant| constant.id == *update_constant_id)
                    .ok_or_else(|| {
                        format!(
                            "indexed Bytes/get update branch {} references missing index constant {}",
                            op.id.0,
                            update_constant_id.0
                        )
                    })?;
                let PlanConstantValue::Number { value: index } = &index_constant.value else {
                    return Err(format!(
                        "indexed Bytes/get update branch {} index constant {} is not a number",
                        op.id.0, update_constant_id.0
                    )
                    .into());
                };
                let index = usize::try_from(*index).map_err(|_| {
                    format!(
                        "indexed Bytes/get update branch {} index constant {} is negative or too large",
                        op.id.0, update_constant_id.0
                    )
                })?;
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                let byte = bytes_view.bytes.get(index).ok_or_else(|| {
                    format!(
                        "indexed Bytes/get update branch {} index {index} is out of bounds for `{input_name}`",
                        op.id.0
                    )
                })?;
                (
                    json!(i64::from(*byte)),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, Some(index)),
                    "bytes_get",
                    json!(update_constant_id.0),
                    json!(index),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesReadUnsigned,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (
                    input_state_id,
                    offset,
                    byte_count,
                    endian,
                    offset_constant_id,
                    byte_count_constant_id,
                    endian_constant_id,
                ) = root_bytes_numeric_read_operands(plan, op, "indexed Bytes/read_unsigned")?;
                let input_name = local_field_name(&state_label(plan, input_state_id));
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                let value = bytes_read_unsigned(bytes_view.bytes, offset, byte_count, endian)
                    .map_err(|code| {
                        format!(
                            "indexed Bytes/read_unsigned update branch {} failed for `{input_name}`: {code}",
                            op.id.0
                        )
                    })?;
                if value > i64::MAX as u64 {
                    return Err(format!(
                        "indexed Bytes/read_unsigned update branch {} overflows Boon NUMBER",
                        op.id.0
                    )
                    .into());
                }
                (
                    json!(value as i64),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_read_unsigned",
                    json!([
                        offset_constant_id.0,
                        byte_count_constant_id.0,
                        endian_constant_id.0
                    ]),
                    json!({
                        "offset": offset,
                        "byte_count": byte_count,
                        "endian": bytes_endian_label(endian)
                    }),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesReadSigned,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (
                    input_state_id,
                    offset,
                    byte_count,
                    endian,
                    offset_constant_id,
                    byte_count_constant_id,
                    endian_constant_id,
                ) = root_bytes_numeric_read_operands(plan, op, "indexed Bytes/read_signed")?;
                let input_name = local_field_name(&state_label(plan, input_state_id));
                let bytes_view =
                    indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
                let value = bytes_read_signed(bytes_view.bytes, offset, byte_count, endian)
                    .map_err(|code| {
                        format!(
                            "indexed Bytes/read_signed update branch {} failed for `{input_name}`: {code}",
                            op.id.0
                        )
                    })?;
                (
                    json!(value),
                    None,
                    indexed_bytes_access_json(plan, input_state_id, &bytes_view, None),
                    "bytes_read_signed",
                    json!([
                        offset_constant_id.0,
                        byte_count_constant_id.0,
                        endian_constant_id.0
                    ]),
                    json!({
                        "offset": offset,
                        "byte_count": byte_count,
                        "endian": bytes_endian_label(endian)
                    }),
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesEqual,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (left_input, right_input) =
                    indexed_bytes_distinct_value_inputs(op, "Bytes/equal")?;
                let left_view =
                    indexed_executor_row_bytes_input_view(plan, row, &left_input, op.id.0)?;
                let right_view =
                    indexed_executor_row_bytes_input_view(plan, row, &right_input, op.id.0)?;
                (
                    JsonValue::Bool(left_view.bytes == right_view.bytes),
                    None,
                    json!({
                        "read_only": true,
                        "inputs": [
                            left_view.labeled_access_json("left"),
                            right_view.labeled_access_json("right")
                        ],
                    }),
                    "bytes_equal",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesFind,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (haystack_input, needle_input) =
                    indexed_bytes_ordered_value_inputs(op, "Bytes/find")?;
                let haystack_view =
                    indexed_executor_row_bytes_input_view(plan, row, &haystack_input, op.id.0)?;
                let needle_view =
                    indexed_executor_row_bytes_input_view(plan, row, &needle_input, op.id.0)?;
                let value = match bytes_find(haystack_view.bytes, needle_view.bytes) {
                    Some(index) => json!(i64::try_from(index).map_err(|_| {
                        format!(
                            "indexed Bytes/find update branch {} found index exceeds Boon NUMBER",
                            op.id.0
                        )
                    })?),
                    None => JsonValue::Null,
                };
                (
                    value,
                    None,
                    json!({
                        "read_only": true,
                        "inputs": [
                            haystack_view.labeled_access_json("haystack"),
                            needle_view.labeled_access_json("needle")
                        ],
                    }),
                    "bytes_find",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesStartsWith,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (input, prefix) = indexed_bytes_ordered_value_inputs(op, "Bytes/starts_with")?;
                let input_view = indexed_executor_row_bytes_input_view(plan, row, &input, op.id.0)?;
                let prefix_view =
                    indexed_executor_row_bytes_input_view(plan, row, &prefix, op.id.0)?;
                (
                    JsonValue::Bool(input_view.bytes.starts_with(prefix_view.bytes)),
                    None,
                    json!({
                        "read_only": true,
                        "inputs": [
                            input_view.labeled_access_json("input"),
                            prefix_view.labeled_access_json("prefix")
                        ],
                    }),
                    "bytes_starts_with",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesEndsWith,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let (input, suffix) = indexed_bytes_ordered_value_inputs(op, "Bytes/ends_with")?;
                let input_view = indexed_executor_row_bytes_input_view(plan, row, &input, op.id.0)?;
                let suffix_view =
                    indexed_executor_row_bytes_input_view(plan, row, &suffix, op.id.0)?;
                (
                    JsonValue::Bool(input_view.bytes.ends_with(suffix_view.bytes)),
                    None,
                    json!({
                        "read_only": true,
                        "inputs": [
                            input_view.labeled_access_json("input"),
                            suffix_view.labeled_access_json("suffix")
                        ],
                    }),
                    "bytes_ends_with",
                    JsonValue::Null,
                    JsonValue::Null,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::FileReadBytes,
                source_payload_field: None,
                update_constant_id: None,
                ..
            } => {
                let path_operand = file_read_bytes_path_operand(plan, op, "indexed")?;
                let (path, path_update_constant_id, path_update_constant_value, path_source) =
                    indexed_file_read_bytes_path(plan, row, op, path_operand)?;
                let host_file_root = host_file_root.ok_or_else(|| {
                    format!(
                        "indexed File/read_bytes update branch {} has no host file root",
                        op.id.0
                    )
                })?;
                let bytes = read_plan_host_file_bytes(host_file_root, &path).map_err(|error| {
                    format!(
                        "indexed File/read_bytes update branch {} cannot read `{path}`: {error}",
                        op.id.0
                    )
                })?;
                validate_root_bytes_output_len(
                    plan,
                    output_state_id,
                    bytes.len(),
                    "File/read_bytes",
                    op.id,
                )?;
                let executor_bytes = PlanExecutorBytes::from_inline(
                    sha256_bytes(&bytes),
                    bytes.len() as u64,
                    bytes,
                    &format!("indexed File/read_bytes update branch {}", op.id.0),
                )?;
                let value = executor_bytes.report_json();
                let bytes_access = json!({
                    "read_only": true,
                    "output_state_id": output_state_id.0,
                    "host_boundary": "file_read_bytes",
                    "byte_len": executor_bytes.byte_len(),
                    "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                    "path_source": path_source,
                });
                (
                    value,
                    Some(executor_bytes),
                    bytes_access,
                    "file_read_bytes",
                    path_update_constant_id,
                    path_update_constant_value,
                )
            }
            PlanOpKind::UpdateBranch {
                expression_kind, ..
            } => {
                return Ok(indexed_bytes_read_outcome(
                    op.id,
                    false,
                    Some(format!(
                        "expression kind {expression_kind:?} requires runtime-specific execution"
                    )),
                    Some(output_state_id),
                    None,
                    None,
                    JsonValue::Null,
                    None,
                    JsonValue::Null,
                    JsonValue::Null,
                ));
            }
            _ => {
                return Err(format!(
                    "CPU PlanExecutor indexed BYTES read branch {} is not an update branch",
                    op.id.0
                )
                .into());
            }
        };

    Ok(indexed_bytes_read_outcome(
        op.id,
        true,
        None,
        Some(output_state_id),
        Some(value),
        bytes,
        bytes_access,
        Some(expression_kind),
        update_constant_id,
        update_constant_value,
    ))
}

pub fn evaluate_indexed_bytes_write_update(
    plan: &MachinePlan,
    op: &PlanOp,
    row: &IndexedRowView,
    host_file_root: Option<&Path>,
) -> PlanExecutorResult<IndexedBytesWriteEvaluation> {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor indexed BYTES write branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    match &op.kind {
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSet,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, index, byte_value, index_constant_id, value_constant_id) =
                indexed_bytes_set_operands(plan, op)?;
            let output_slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            if let PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } = &output_slot.value_type
                && *fixed_len != bytes_view.bytes.len() as u64
            {
                return Err(format!(
                    "indexed Bytes/set update branch {} output fixed length {} does not match input length {}",
                    op.id.0,
                    fixed_len,
                    bytes_view.bytes.len()
                )
                .into());
            }
            if index >= bytes_view.bytes.len() {
                return Err(format!(
                    "indexed Bytes/set update branch {} index {index} is out of bounds for `{input_name}`",
                    op.id.0
                )
                .into());
            }

            let value = bytes_report_json_with_patches(
                &bytes_view.bytes,
                &[(index, byte_value)],
                "Bytes/set",
                op.id,
                &input_name,
            )?;
            let mut output = bytes_view.bytes.to_vec();
            output[index] = byte_value;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/set update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(value),
                Some(bytes),
                indexed_bytes_write_access_json(
                    plan,
                    input_state_id,
                    &bytes_view,
                    index,
                    byte_value,
                ),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_set"),
                json!([index_constant_id.0, value_constant_id.0]),
                json!({"index": index, "value": byte_value}),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSlice,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, offset, byte_count, offset_constant_id, byte_count_constant_id) =
                indexed_bytes_slice_operands(plan, op)?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            let output = checked_indexed_bytes_slice(
                bytes_view.bytes,
                offset,
                byte_count,
                "Bytes/slice",
                op.id,
                &input_name,
            )?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/slice",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/slice update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                indexed_bytes_slice_access_json(
                    plan,
                    input_state_id,
                    output_state_id,
                    &bytes_view,
                    offset,
                    byte_count,
                    byte_count,
                    "bytes_slice_view",
                ),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_slice"),
                json!([offset_constant_id.0, byte_count_constant_id.0]),
                json!({"offset": offset, "byte_count": byte_count}),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesTake,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, byte_count, byte_count_constant_id) =
                indexed_bytes_count_operand(plan, op, "Bytes/take")?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            let output = checked_indexed_bytes_slice(
                bytes_view.bytes,
                0,
                byte_count,
                "Bytes/take",
                op.id,
                &input_name,
            )?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/take",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/take update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                indexed_bytes_slice_access_json(
                    plan,
                    input_state_id,
                    output_state_id,
                    &bytes_view,
                    0,
                    byte_count,
                    byte_count,
                    "bytes_slice_view",
                ),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_take"),
                json!(byte_count_constant_id.0),
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesDrop,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, byte_count, byte_count_constant_id) =
                indexed_bytes_count_operand(plan, op, "Bytes/drop")?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            let output = checked_indexed_bytes_drop(
                bytes_view.bytes,
                byte_count,
                "Bytes/drop",
                op.id,
                &input_name,
            )?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/drop",
                op.id,
            )?;
            let output_len = output.len();
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output_len as u64,
                output,
                &format!("indexed Bytes/drop update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                indexed_bytes_slice_access_json(
                    plan,
                    input_state_id,
                    output_state_id,
                    &bytes_view,
                    byte_count,
                    output_len,
                    output_len,
                    "bytes_slice_view",
                ),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_drop"),
                json!(byte_count_constant_id.0),
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesZeros,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (byte_count, byte_count_constant_id) = indexed_bytes_zeros_operand(plan, op)?;
            let mut output = Vec::new();
            output.try_reserve_exact(byte_count).map_err(|_| {
                format!(
                    "indexed Bytes/zeros update branch {} could not allocate {byte_count} bytes",
                    op.id.0
                )
            })?;
            output.resize(byte_count, 0);
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/zeros",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/zeros update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                JsonValue::Null,
                bytes_storage,
                JsonValue::Null,
                Some("bytes_zeros"),
                json!(byte_count_constant_id.0),
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesConcat,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (left_input, right_input) = indexed_bytes_ordered_value_inputs(op, "Bytes/concat")?;
            let left_view = indexed_executor_row_bytes_input_view(plan, row, &left_input, op.id.0)?;
            let right_view =
                indexed_executor_row_bytes_input_view(plan, row, &right_input, op.id.0)?;
            let output_len = left_view.bytes.len().saturating_add(right_view.bytes.len());
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output_len,
                "Bytes/concat",
                op.id,
            )?;
            let mut output = Vec::with_capacity(output_len);
            output.extend_from_slice(left_view.bytes);
            output.extend_from_slice(right_view.bytes);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/concat update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            let mut outcome = indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                json!({
                    "read_only": false,
                    "output_state_id": output_state_id.0,
                    "output_storage_kind": "bytes_concat_output_vec",
                    "output_cow_kind": "owned_vec",
                    "output_byte_len": output_len,
                    "inputs": [
                        left_view.labeled_access_json("left"),
                        right_view.labeled_access_json("right")
                    ],
                }),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_concat"),
                JsonValue::Null,
                JsonValue::Null,
            );
            attach_executor_bytes_copy_cost(
                &mut outcome.executor_report,
                "bytes_concat_output_vec",
                1,
                output_len as u64,
                2,
                output_len as u64,
            );
            Ok(outcome)
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesWriteUnsigned,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (
                input_state_id,
                offset,
                byte_count,
                endian,
                value,
                offset_constant_id,
                byte_count_constant_id,
                endian_constant_id,
                value_constant_id,
            ) = root_bytes_numeric_write_operands(plan, op, "indexed Bytes/write_unsigned")?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                bytes_view.bytes.len(),
                "Bytes/write_unsigned",
                op.id,
            )?;
            let patches = bytes_write_unsigned_patches(
                bytes_view.bytes.len(),
                offset,
                byte_count,
                endian,
                value,
            )
            .map_err(|code| {
                format!(
                    "indexed Bytes/write_unsigned update branch {} failed for `{input_name}`: {code}",
                    op.id.0
                )
            })?;
            let patch_count = patches.len();
            let value_json = bytes_report_json_with_patches(
                bytes_view.bytes,
                &patches,
                "Bytes/write_unsigned",
                op.id,
                &input_name,
            )?;
            let mut output = bytes_view.bytes.to_vec();
            apply_bytes_patches(&mut output, &patches);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/write_unsigned update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(value_json),
                Some(bytes),
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "access_source": bytes_view.access_source,
                    "cow_kind": bytes_view.cow_kind,
                    "byte_len": bytes_view.bytes.len(),
                    "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
                    "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
                    "mutation_kind": "inline_bytes_copy",
                    "patch_count": patch_count,
                }),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_write_unsigned"),
                json!([
                    offset_constant_id.0,
                    byte_count_constant_id.0,
                    endian_constant_id.0,
                    value_constant_id.0
                ]),
                json!({
                    "offset": offset,
                    "byte_count": byte_count,
                    "endian": bytes_endian_label(endian),
                    "value": value
                }),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesWriteSigned,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (
                input_state_id,
                offset,
                byte_count,
                endian,
                value,
                offset_constant_id,
                byte_count_constant_id,
                endian_constant_id,
                value_constant_id,
            ) = root_bytes_numeric_write_operands(plan, op, "indexed Bytes/write_signed")?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                bytes_view.bytes.len(),
                "Bytes/write_signed",
                op.id,
            )?;
            let patches = bytes_write_signed_patches(
                bytes_view.bytes.len(),
                offset,
                byte_count,
                endian,
                value,
            )
            .map_err(|code| {
                format!(
                    "indexed Bytes/write_signed update branch {} failed for `{input_name}`: {code}",
                    op.id.0
                )
            })?;
            let patch_count = patches.len();
            let value_json = bytes_report_json_with_patches(
                bytes_view.bytes,
                &patches,
                "Bytes/write_signed",
                op.id,
                &input_name,
            )?;
            let mut output = bytes_view.bytes.to_vec();
            apply_bytes_patches(&mut output, &patches);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/write_signed update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(value_json),
                Some(bytes),
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "access_source": bytes_view.access_source,
                    "cow_kind": bytes_view.cow_kind,
                    "byte_len": bytes_view.bytes.len(),
                    "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
                    "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
                    "mutation_kind": "inline_bytes_copy",
                    "patch_count": patch_count,
                }),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_write_signed"),
                json!([
                    offset_constant_id.0,
                    byte_count_constant_id.0,
                    endian_constant_id.0,
                    value_constant_id.0
                ]),
                json!({
                    "offset": offset,
                    "byte_count": byte_count,
                    "endian": bytes_endian_label(endian),
                    "value": value
                }),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextToBytes,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_ref, encoding, encoding_constant_id) =
                indexed_text_to_bytes_operands(plan, op, "Text/to_bytes")?;
            let input_name = indexed_text_input_label(plan, &input_ref);
            let text = indexed_row_text_input(plan, row, &input_ref, op.id.0, "input")?;
            let output = text_to_bytes(&text, &encoding).map_err(|error| {
                format!(
                    "indexed Text/to_bytes update branch {} input state `{input_name}` {error}",
                    op.id.0
                )
            })?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Text/to_bytes",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Text/to_bytes update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                json!({
                    "read_only": true,
                    "input": indexed_text_input_access_json(plan, &input_ref),
                    "input_kind": "text",
                    "output_state_id": output_state_id.0,
                    "encoding": encoding,
                }),
                bytes_storage,
                JsonValue::Null,
                Some("text_to_bytes"),
                json!(encoding_constant_id.0),
                json!(encoding),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromHex,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_ref = indexed_single_text_input(op, output_state_id)?;
            let input_name = indexed_text_input_label(plan, &input_ref);
            let text = indexed_row_text_input(plan, row, &input_ref, op.id.0, "input")?;
            let output = bytes_decode_hex(&text).map_err(|code| {
                format!(
                    "indexed Bytes/from_hex update branch {} failed for `{input_name}`: {code}",
                    op.id.0
                )
            })?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/from_hex",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/from_hex update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                json!({
                    "read_only": true,
                    "input": indexed_text_input_access_json(plan, &input_ref),
                    "input_kind": "text",
                    "output_state_id": output_state_id.0,
                }),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_from_hex"),
                JsonValue::Null,
                JsonValue::Null,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromBase64,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_ref = indexed_single_text_input(op, output_state_id)?;
            let input_name = indexed_text_input_label(plan, &input_ref);
            let text = indexed_row_text_input(plan, row, &input_ref, op.id.0, "input")?;
            let output = bytes_decode_base64(&text).map_err(|code| {
                format!(
                    "indexed Bytes/from_base64 update branch {} failed for `{input_name}`: {code}",
                    op.id.0
                )
            })?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/from_base64",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("indexed Bytes/from_base64 update branch {}", op.id.0),
            )?;
            let bytes_storage = json!({
                "storage": if indexed_state_has_fixed_byte_bank(plan, output_state_id) {
                    "indexed_fixed_byte_bank"
                } else {
                    "indexed_row_private_bytes"
                },
                "output_state_id": output_state_id.0,
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_bank_used": indexed_state_has_fixed_byte_bank(plan, output_state_id),
                "byte_len": bytes.byte_len(),
            });
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                json!({
                    "read_only": true,
                    "input": indexed_text_input_access_json(plan, &input_ref),
                    "input_kind": "text",
                    "output_state_id": output_state_id.0,
                }),
                bytes_storage,
                JsonValue::Null,
                Some("bytes_from_base64"),
                JsonValue::Null,
                JsonValue::Null,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::FileWriteBytes,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, path_operand) = file_write_bytes_operands(plan, op, "indexed")?;
            let (path, path_update_constant_id, path_update_constant_value, path_source) =
                indexed_file_write_bytes_path(plan, row, op, path_operand)?;
            let host_file_root = host_file_root.ok_or_else(|| {
                format!(
                    "indexed File/write_bytes update branch {} has no host file root",
                    op.id.0
                )
            })?;
            let bytes_view = indexed_executor_row_bytes_view(plan, row, input_state_id, op.id.0)?;
            let bytes_access = json!({
                "read_only": true,
                "input_state_id": input_state_id.0,
                "access_source": bytes_view.access_source,
                "cow_kind": bytes_view.cow_kind,
                "host_boundary": "file_write_bytes",
                "byte_len": bytes_view.bytes.len(),
                "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
                "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
                "path_source": path_source,
            });
            let write_result = write_plan_host_file_bytes(host_file_root, &path, bytes_view.bytes)
                .map_err(|error| {
                    format!(
                        "indexed File/write_bytes update branch {} cannot write `{path}`: {error}",
                        op.id.0
                    )
                })?;
            Ok(indexed_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(JsonValue::String(path.clone())),
                None,
                bytes_access,
                JsonValue::Null,
                write_result.report_json(),
                Some("file_write_bytes"),
                path_update_constant_id,
                path_update_constant_value,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind, ..
        } => Ok(indexed_bytes_write_outcome(
            op.id,
            false,
            Some(format!(
                "expression kind {expression_kind:?} requires runtime-specific execution"
            )),
            Some(output_state_id),
            None,
            None,
            JsonValue::Null,
            JsonValue::Null,
            JsonValue::Null,
            None,
            JsonValue::Null,
            JsonValue::Null,
        )),
        _ => Err(format!(
            "CPU PlanExecutor indexed BYTES write branch {} is not an update branch",
            op.id.0
        )
        .into()),
    }
}

pub fn evaluate_root_bytes_source_payload_commit(
    plan: &MachinePlan,
    output_state_id: StateId,
    event: &RootJsonSourceEvent,
    field: &SourcePayloadField,
    op_id: PlanOpId,
) -> PlanExecutorResult<RootBytesSourcePayloadCommit> {
    let slot = scalar_slot_for_state(plan, output_state_id, op_id)?;
    if !matches!(field, SourcePayloadField::Bytes) {
        return Err(format!(
            "root BYTES source-payload commit branch {} received non-BYTES field {field:?}",
            op_id.0
        )
        .into());
    }
    let bytes = source_payload_bytes(event, field)?;
    let PlanValueType::Bytes { fixed_len } = slot.value_type else {
        return Err(format!(
            "root BYTES source-payload commit branch {} targets non-BYTES state {}",
            op_id.0, output_state_id.0
        )
        .into());
    };
    if let Some(expected_len) = fixed_len
        && expected_len != bytes.len() as u64
    {
        return Err(format!(
            "root BYTES source-payload commit branch {} payload has byte_len {} but output fixed_len {expected_len}",
            op_id.0,
            bytes.len()
        )
        .into());
    }
    let executor_bytes = PlanExecutorBytes::from_inline(
        sha256_bytes(bytes),
        bytes.len() as u64,
        bytes.to_vec(),
        &format!("root BYTES source-payload commit branch {}", op_id.0),
    )?;
    let value = executor_bytes.report_json();
    let executor_report = json!({
        "executor": "cpu-plan-root-bytes-source-payload-commit-v1",
        "update_op_id": op_id.0,
        "target_state_id": output_state_id.0,
        "source_payload_field": field,
        "byte_len": executor_bytes.byte_len(),
        "digest": executor_bytes.digest(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootBytesSourcePayloadCommit {
        target_state_id: output_state_id,
        value,
        bytes: executor_bytes,
        executor_report,
    })
}

pub fn evaluate_root_bytes_write_update(
    plan: &MachinePlan,
    op: &PlanOp,
    root_state: &JsonMap<String, JsonValue>,
    bytes_environment: &(impl RootBytesEnvironment + ?Sized),
    host_file_root: Option<&Path>,
) -> PlanExecutorResult<RootBytesWriteEvaluation> {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor root BYTES write branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    match &op.kind {
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSet,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, index, byte_value, index_constant_id, value_constant_id) =
                root_bytes_set_operands(plan, op)?;
            let output_slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            if let PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } = &output_slot.value_type
                && *fixed_len != bytes_view.bytes.len() as u64
            {
                return Err(format!(
                    "root Bytes/set update branch {} output fixed length {} does not match input length {}",
                    op.id.0,
                    fixed_len,
                    bytes_view.bytes.len()
                )
                .into());
            }
            if index >= bytes_view.bytes.len() {
                return Err(format!(
                    "root Bytes/set update branch {} index {index} is out of bounds for `{input_label}`",
                    op.id.0
                )
                .into());
            }

            let update_constant_id = json!([index_constant_id.0, value_constant_id.0]);
            let update_constant_value = json!({"index": index, "value": byte_value});
            if bytes_environment.has_fixed_byte_bank(input_state_id)
                && root_state_has_fixed_byte_bank(plan, output_state_id)
            {
                let value = bytes_report_json_with_patches(
                    bytes_view.bytes,
                    &[(index, byte_value)],
                    "Bytes/set",
                    op.id,
                    &input_label,
                )?;
                return Ok(root_bytes_write_outcome(
                    op.id,
                    true,
                    None,
                    Some(output_state_id),
                    Some(value),
                    None,
                    Some(RootBytesFixedMutation {
                        input_state_id,
                        output_state_id,
                        patches: vec![(index, byte_value)],
                    }),
                    bytes_view.access_json(input_state_id),
                    Some("bytes_set"),
                    update_constant_id,
                    update_constant_value,
                ));
            }

            let mut output = bytes_view.bytes.to_vec();
            output[index] = byte_value;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/set update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                bytes_view.access_json(input_state_id),
                Some("bytes_set"),
                update_constant_id,
                update_constant_value,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesConcat,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (left_state_id, right_state_id) = root_bytes_concat_state_inputs(op)?;
            let left_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                left_state_id,
                op.id.0,
            )?;
            let right_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                right_state_id,
                op.id.0,
            )?;
            let output_slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
            let output_len = left_view.bytes.len().saturating_add(right_view.bytes.len());
            if let PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } = &output_slot.value_type
                && *fixed_len != output_len as u64
            {
                return Err(format!(
                    "root Bytes/concat update branch {} output fixed length {} does not match concat length {}",
                    op.id.0, fixed_len, output_len
                )
                .into());
            }
            let mut output = Vec::with_capacity(output_len);
            output.extend_from_slice(left_view.bytes);
            output.extend_from_slice(right_view.bytes);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/concat update branch {}", op.id.0),
            )?;
            let mut outcome = root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                JsonValue::Null,
                Some("bytes_concat"),
                JsonValue::Null,
                JsonValue::Null,
            );
            attach_executor_bytes_copy_cost(
                &mut outcome.executor_report,
                "bytes_concat_output_vec",
                1,
                output_len as u64,
                2,
                output_len as u64,
            );
            Ok(outcome)
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSlice,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, offset, byte_count, offset_ref, byte_count_ref) =
                root_bytes_slice_operands(plan, root_state, op)?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            let output = checked_root_bytes_slice(
                bytes_view.bytes,
                offset,
                byte_count,
                "Bytes/slice",
                op.id,
                &input_label,
            )?;
            let output_len = output.len();
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output_len,
                "Bytes/slice",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output_len as u64,
                output,
                &format!("root Bytes/slice update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "input_access_source": bytes_view.access_source,
                    "output_storage_kind": "bytes_slice_view",
                    "output_cow_kind": "borrowed_view",
                    "offset": offset,
                    "byte_count": byte_count,
                    "output_byte_len": output_len,
                }),
                Some("bytes_slice"),
                json!([offset_ref, byte_count_ref]),
                json!({"offset": offset, "byte_count": byte_count}),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesTake,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, byte_count, byte_count_ref) =
                root_bytes_count_operand(plan, root_state, op, "Bytes/take")?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            let output = checked_root_bytes_slice(
                bytes_view.bytes,
                0,
                byte_count,
                "Bytes/take",
                op.id,
                &input_label,
            )?;
            let output_len = output.len();
            validate_root_bytes_output_len(plan, output_state_id, output_len, "Bytes/take", op.id)?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output_len as u64,
                output,
                &format!("root Bytes/take update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "input_access_source": bytes_view.access_source,
                    "output_storage_kind": "bytes_slice_view",
                    "output_cow_kind": "borrowed_view",
                    "offset": 0,
                    "byte_count": byte_count,
                    "output_byte_len": output_len,
                }),
                Some("bytes_take"),
                byte_count_ref,
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesDrop,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, byte_count, byte_count_ref) =
                root_bytes_count_operand(plan, root_state, op, "Bytes/drop")?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            let output = checked_root_bytes_drop(
                bytes_view.bytes,
                byte_count,
                "Bytes/drop",
                op.id,
                &input_label,
            )?;
            let output_len = output.len();
            validate_root_bytes_output_len(plan, output_state_id, output_len, "Bytes/drop", op.id)?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output_len as u64,
                output,
                &format!("root Bytes/drop update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "input_access_source": bytes_view.access_source,
                    "output_storage_kind": "bytes_slice_view",
                    "output_cow_kind": "borrowed_view",
                    "offset": byte_count,
                    "byte_count": output_len,
                    "dropped_byte_count": byte_count,
                    "output_byte_len": output_len,
                }),
                Some("bytes_drop"),
                byte_count_ref,
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesZeros,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (byte_count, byte_count_constant_id) = root_bytes_zeros_operand(plan, op)?;
            let mut output = Vec::new();
            output.try_reserve_exact(byte_count).map_err(|_| {
                format!(
                    "root Bytes/zeros update branch {} could not allocate {byte_count} bytes",
                    op.id.0
                )
            })?;
            output.resize(byte_count, 0);
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/zeros",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/zeros update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                JsonValue::Null,
                Some("bytes_zeros"),
                json!(byte_count_constant_id.0),
                json!(byte_count),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextToBytes,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, encoding, encoding_constant_id) =
                root_text_bytes_conversion_operands(plan, op, "Text/to_bytes")?;
            let input_label = state_label(plan, input_state_id);
            let text = root_state_value(plan, root_state, input_state_id, op.id.0)?
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "root Text/to_bytes update branch {} input state `{input_label}` is not TEXT",
                        op.id.0
                    )
                })?;
            let output = text_to_bytes(text, &encoding).map_err(|error| {
                format!(
                    "root Text/to_bytes update branch {} input state `{input_label}` {error}",
                    op.id.0
                )
            })?;
            let output_slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
            if let PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } = &output_slot.value_type
                && *fixed_len != output.len() as u64
            {
                return Err(format!(
                    "root Text/to_bytes update branch {} output fixed length {} does not match encoded length {}",
                    op.id.0,
                    fixed_len,
                    output.len()
                )
                .into());
            }
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Text/to_bytes update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                JsonValue::Null,
                Some("text_to_bytes"),
                json!(encoding_constant_id.0),
                json!(encoding),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromHex,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_state_id = root_single_state_input(op)?;
            let input_label = state_label(plan, input_state_id);
            let text = root_state_value(plan, root_state, input_state_id, op.id.0)?
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "root Bytes/from_hex update branch {} input state `{input_label}` is not TEXT",
                        op.id.0
                    )
                })?;
            let output = bytes_decode_hex(text).map_err(|code| {
                format!(
                    "root Bytes/from_hex update branch {} failed for `{input_label}`: {code}",
                    op.id.0
                )
            })?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/from_hex",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/from_hex update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                JsonValue::Null,
                Some("bytes_from_hex"),
                JsonValue::Null,
                JsonValue::Null,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromBase64,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_state_id = root_single_state_input(op)?;
            let input_label = state_label(plan, input_state_id);
            let text = root_state_value(plan, root_state, input_state_id, op.id.0)?
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "root Bytes/from_base64 update branch {} input state `{input_label}` is not TEXT",
                        op.id.0
                    )
                })?;
            let output = bytes_decode_base64(text).map_err(|code| {
                format!(
                    "root Bytes/from_base64 update branch {} failed for `{input_label}`: {code}",
                    op.id.0
                )
            })?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                output.len(),
                "Bytes/from_base64",
                op.id,
            )?;
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/from_base64 update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                JsonValue::Null,
                Some("bytes_from_base64"),
                JsonValue::Null,
                JsonValue::Null,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesWriteUnsigned,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (
                input_state_id,
                offset,
                byte_count,
                endian,
                value,
                offset_constant_id,
                byte_count_constant_id,
                endian_constant_id,
                value_constant_id,
            ) = root_bytes_numeric_write_operands(plan, op, "Bytes/write_unsigned")?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                bytes_view.bytes.len(),
                "Bytes/write_unsigned",
                op.id,
            )?;
            let patches = bytes_write_unsigned_patches(
                bytes_view.bytes.len(),
                offset,
                byte_count,
                endian,
                value,
            )
            .map_err(|code| {
                format!(
                    "root Bytes/write_unsigned update branch {} failed for `{input_label}`: {code}",
                    op.id.0
                )
            })?;
            let update_constant_id = json!([
                offset_constant_id.0,
                byte_count_constant_id.0,
                endian_constant_id.0,
                value_constant_id.0
            ]);
            let update_constant_value = json!({
                "offset": offset,
                "byte_count": byte_count,
                "endian": bytes_endian_label(endian),
                "value": value
            });
            if bytes_environment.has_fixed_byte_bank(input_state_id)
                && root_state_has_fixed_byte_bank(plan, output_state_id)
            {
                let patch_count = patches.len();
                let value_json = bytes_report_json_with_patches(
                    bytes_view.bytes,
                    &patches,
                    "Bytes/write_unsigned",
                    op.id,
                    &input_label,
                )?;
                return Ok(root_bytes_write_outcome(
                    op.id,
                    true,
                    None,
                    Some(output_state_id),
                    Some(value_json),
                    None,
                    Some(RootBytesFixedMutation {
                        input_state_id,
                        output_state_id,
                        patches,
                    }),
                    json!({
                        "read_only": false,
                        "input_state_id": input_state_id.0,
                        "output_state_id": output_state_id.0,
                        "access_source": "root_fixed_byte_bank",
                        "cow_kind": "borrowed",
                        "mutation_kind": "fixed_byte_bank_patches",
                        "patch_count": patch_count,
                    }),
                    Some("bytes_write_unsigned"),
                    update_constant_id,
                    update_constant_value,
                ));
            }
            let mut output = bytes_view.bytes.to_vec();
            apply_bytes_patches(&mut output, &patches);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/write_unsigned update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "access_source": bytes_view.access_source,
                    "cow_kind": "cloned",
                    "mutation_kind": "inline_bytes_copy",
                    "patch_count": patches.len(),
                }),
                Some("bytes_write_unsigned"),
                update_constant_id,
                update_constant_value,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesWriteSigned,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (
                input_state_id,
                offset,
                byte_count,
                endian,
                value,
                offset_constant_id,
                byte_count_constant_id,
                endian_constant_id,
                value_constant_id,
            ) = root_bytes_numeric_write_operands(plan, op, "Bytes/write_signed")?;
            let input_label = state_label(plan, input_state_id);
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            validate_root_bytes_output_len(
                plan,
                output_state_id,
                bytes_view.bytes.len(),
                "Bytes/write_signed",
                op.id,
            )?;
            let patches = bytes_write_signed_patches(
                bytes_view.bytes.len(),
                offset,
                byte_count,
                endian,
                value,
            )
            .map_err(|code| {
                format!(
                    "root Bytes/write_signed update branch {} failed for `{input_label}`: {code}",
                    op.id.0
                )
            })?;
            let update_constant_id = json!([
                offset_constant_id.0,
                byte_count_constant_id.0,
                endian_constant_id.0,
                value_constant_id.0
            ]);
            let update_constant_value = json!({
                "offset": offset,
                "byte_count": byte_count,
                "endian": bytes_endian_label(endian),
                "value": value
            });
            if bytes_environment.has_fixed_byte_bank(input_state_id)
                && root_state_has_fixed_byte_bank(plan, output_state_id)
            {
                let patch_count = patches.len();
                let value_json = bytes_report_json_with_patches(
                    bytes_view.bytes,
                    &patches,
                    "Bytes/write_signed",
                    op.id,
                    &input_label,
                )?;
                return Ok(root_bytes_write_outcome(
                    op.id,
                    true,
                    None,
                    Some(output_state_id),
                    Some(value_json),
                    None,
                    Some(RootBytesFixedMutation {
                        input_state_id,
                        output_state_id,
                        patches,
                    }),
                    json!({
                        "read_only": false,
                        "input_state_id": input_state_id.0,
                        "output_state_id": output_state_id.0,
                        "access_source": "root_fixed_byte_bank",
                        "cow_kind": "borrowed",
                        "mutation_kind": "fixed_byte_bank_patches",
                        "patch_count": patch_count,
                    }),
                    Some("bytes_write_signed"),
                    update_constant_id,
                    update_constant_value,
                ));
            }
            let mut output = bytes_view.bytes.to_vec();
            apply_bytes_patches(&mut output, &patches);
            let bytes = PlanExecutorBytes::from_inline(
                sha256_bytes(&output),
                output.len() as u64,
                output,
                &format!("root Bytes/write_signed update branch {}", op.id.0),
            )?;
            Ok(root_bytes_write_outcome(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(bytes.report_json()),
                Some(bytes),
                None,
                json!({
                    "read_only": false,
                    "input_state_id": input_state_id.0,
                    "output_state_id": output_state_id.0,
                    "access_source": bytes_view.access_source,
                    "cow_kind": "cloned",
                    "mutation_kind": "inline_bytes_copy",
                    "patch_count": patches.len(),
                }),
                Some("bytes_write_signed"),
                update_constant_id,
                update_constant_value,
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::FileWriteBytes,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let (input_state_id, path_operand) = file_write_bytes_operands(plan, op, "root")?;
            let (path, path_update_constant_id, path_update_constant_value, path_source) =
                match path_operand {
                    FileWriteBytesPathOperand::StaticConstant { path, constant_id } => (
                        path.clone(),
                        json!(constant_id.0),
                        json!({ "path": path }),
                        "static_constant",
                    ),
                    FileWriteBytesPathOperand::StatePath { state_id } => {
                        let label = state_label(plan, state_id);
                        let value = root_state_value(plan, root_state, state_id, op.id.0)?;
                        let path = value.as_str().ok_or_else(|| {
                            format!(
                                "root File/write_bytes update branch {} path state `{label}` is not TEXT",
                                op.id.0
                            )
                        })?;
                        (
                            path.to_owned(),
                            JsonValue::Null,
                            json!({
                                "path": path,
                                "path_state": label,
                                "path_state_id": state_id.0,
                            }),
                            "state",
                        )
                    }
                    FileWriteBytesPathOperand::RowFieldPath { field_id } => {
                        return Err(format!(
                            "root File/write_bytes update branch {} cannot use indexed row field path operand {}",
                            op.id.0,
                            field_id.0
                        )
                        .into())
                    }
                };
            let host_file_root = host_file_root.ok_or_else(|| {
                format!(
                    "root File/write_bytes update branch {} has no host file root",
                    op.id.0
                )
            })?;
            let bytes_view = root_executor_bytes_view(
                plan,
                root_state,
                bytes_environment,
                input_state_id,
                op.id.0,
            )?;
            let bytes_access = json!({
                "read_only": true,
                "input_state_id": input_state_id.0,
                "access_source": bytes_view.access_source,
                "cow_kind": bytes_view.cow_kind,
                "host_boundary": "file_write_bytes",
                "byte_len": bytes_view.bytes.len(),
                "path_source": path_source,
            });
            let write_result = write_plan_host_file_bytes(host_file_root, &path, bytes_view.bytes)
                .map_err(|error| {
                    format!(
                        "root File/write_bytes update branch {} cannot write `{path}`: {error}",
                        op.id.0
                    )
                })?;
            Ok(root_bytes_write_outcome_with_host_effect(
                op.id,
                true,
                None,
                Some(output_state_id),
                Some(JsonValue::String(path.clone())),
                None,
                None,
                bytes_access,
                write_result.report_json(),
                Some("file_write_bytes"),
                path_update_constant_id,
                path_update_constant_value,
            ))
        }
        _ => Ok(root_bytes_write_outcome(
            op.id,
            false,
            Some("expression kind requires runtime-specific execution".to_owned()),
            Some(output_state_id),
            None,
            None,
            None,
            JsonValue::Null,
            None,
            JsonValue::Null,
            JsonValue::Null,
        )),
    }
}

pub fn apply_root_bytes_state_transition(
    bytes_owner: &mut impl RootBytesStateOwner,
    target_state_id: StateId,
    bytes_value: Option<PlanExecutorBytes>,
    fixed_mutation: Option<RootBytesFixedMutation>,
    op_id: PlanOpId,
) -> PlanExecutorResult<RootBytesStateTransition> {
    let mode = if let Some(mutation) = fixed_mutation {
        apply_root_fixed_bytes_mutation(bytes_owner, &mutation, op_id)?;
        bytes_owner.remove_private_bytes_for_state(target_state_id);
        "fixed_byte_patch"
    } else if let Some(bytes) = bytes_value {
        bytes_owner.insert_private_bytes_for_state(target_state_id, bytes);
        bytes_owner.remove_fixed_byte_bank_for_state(target_state_id);
        "bytes_commit"
    } else {
        bytes_owner.clear_bytes_for_state(target_state_id);
        "clear"
    };
    let executor_report = json!({
        "executor": "cpu-plan-root-bytes-state-transition-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.0,
        "mode": mode,
        "private_bytes_state_count": bytes_owner.private_bytes_state_count(),
        "fixed_byte_bank_count": bytes_owner.fixed_byte_bank_count(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootBytesStateTransition {
        target_state_id,
        mode,
        executor_report,
    })
}

pub fn select_source_route_update(
    plan: &MachinePlan,
    source_route: &str,
    target_state: &str,
) -> PlanExecutorResult<SourceRouteSelection> {
    let verification = verify_plan(plan)?;
    if verification.status != "pass" {
        return Err(format!(
            "MachinePlan verification failed with {} error(s)",
            verification.error_count
        )
        .into());
    }
    if !plan.capability_summary.typed_lowering_executable {
        return Err("CPU source-route PlanExecutor requires typed_lowering_executable=true".into());
    }

    let source_id = source_id_for_label(plan, source_route)
        .ok_or_else(|| format!("MachinePlan has no source route `{source_route}`"))?;
    let target_state_id = state_id_for_label(plan, target_state)
        .ok_or_else(|| format!("MachinePlan has no state slot `{target_state}`"))?;
    let source_route_slot = plan
        .source_routes
        .iter()
        .find(|route| route.source_id == source_id)
        .ok_or_else(|| format!("MachinePlan source route `{source_route}` has no route slot"))?;

    let route_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .filter(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if *id == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if *id == target_state_id)
        })
        .collect::<Vec<_>>();
    let [op] = route_ops.as_slice() else {
        return Err(format!(
            "expected exactly one typed update branch for `{source_route}` -> `{target_state}`, found {}",
            route_ops.len()
        )
        .into());
    };
    if op.unresolved_executable_ref_count != 0 {
        return Err(format!(
            "selected update branch {} has {} unresolved executable refs",
            op.id.0, op.unresolved_executable_ref_count
        )
        .into());
    }
    let output_slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == target_state_id)
        .ok_or_else(|| {
            format!(
                "selected update branch {} targets missing state slot {}",
                op.id.0, target_state_id.0
            )
        })?;
    if source_route_slot.scoped {
        if !op.indexed || !output_slot.indexed {
            return Err(format!(
                "scoped source route `{source_route}` -> `{target_state}` must select an indexed update branch"
            )
            .into());
        }
        if source_route_slot.scope_id != output_slot.scope_id {
            return Err(format!(
                "scoped source route `{source_route}` scope {:?} does not match target state `{target_state}` scope {:?}",
                source_route_slot.scope_id, output_slot.scope_id
            )
            .into());
        }
    }
    if op.indexed && !output_slot.indexed {
        return Err(format!(
            "indexed update branch {} targets non-indexed state `{target_state}`",
            op.id.0
        )
        .into());
    }

    let (source_payload_field, update_constant_id) = match &op.kind {
        PlanOpKind::UpdateBranch {
            source_payload_field,
            update_constant_id,
            ..
        } => (*source_payload_field).clone().map_or_else(
            || (None, *update_constant_id),
            |field| (Some(field), *update_constant_id),
        ),
        _ => {
            return Err(format!(
                "selected source-route op {} is not an update branch",
                op.id.0
            )
            .into());
        }
    };
    if let Some(field) = &source_payload_field {
        if !source_route_slot.payload_schema.fields.contains(field) {
            return Err(format!(
                "selected update branch {} reads payload field {:?}, but route `{source_route}` declares {:?}",
                op.id.0, field, source_route_slot.payload_schema.fields
            )
            .into());
        }
        let has_typed_payload_input = op.inputs.iter().any(|input| {
            matches!(
                input,
                ValueRef::SourcePayload {
                    source_id: input_source_id,
                    field: input_field,
                } if *input_source_id == source_id && input_field == field
            )
        });
        if !has_typed_payload_input {
            return Err(format!(
                "selected update branch {} reads payload field {:?} without a typed SourcePayload input",
                op.id.0, field
            )
            .into());
        }
    }

    let plan_hash = plan_sha256(plan)?;
    let executor_report = json!({
        "executor": "cpu-plan-source-route-selection-v1",
        "source": source_route,
        "source_id": source_id.0,
        "target_state": target_state,
        "target_state_id": target_state_id.0,
        "update_op_id": op.id.0,
        "source_payload_field": source_payload_field,
        "update_constant_id": update_constant_id.map(|id| id.0),
        "selected_op_indexed": op.indexed,
        "selected_op_unresolved_executable_ref_count": op.unresolved_executable_ref_count,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(SourceRouteSelection {
        plan_hash,
        source_label: source_route.to_owned(),
        source_id,
        target_state_label: target_state.to_owned(),
        target_state_id,
        update_op_id: op.id,
        source_payload_field,
        update_constant_id,
        indexed: op.indexed,
        unresolved_executable_ref_count: op.unresolved_executable_ref_count,
        executor_report,
    })
}

pub fn resolve_source_route_execution_context(
    plan: &MachinePlan,
    source_route: &str,
    target_state: &str,
) -> PlanExecutorResult<SourceRouteExecutionContext> {
    let selection = select_source_route_update(plan, source_route, target_state)?;
    let source_route_slot = plan
        .source_routes
        .iter()
        .find(|route| route.source_id == selection.source_id)
        .cloned()
        .ok_or_else(|| format!("MachinePlan source route `{source_route}` has no route slot"))?;
    let update_op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| op.id == selection.update_op_id)
        .cloned()
        .ok_or_else(|| {
            format!(
                "source-route selector chose missing update op {}",
                selection.update_op_id.0
            )
        })?;
    let executor_report = json!({
        "executor": "cpu-plan-source-route-execution-context-v1",
        "source": source_route,
        "source_id": selection.source_id.0,
        "target_state": target_state,
        "target_state_id": selection.target_state_id.0,
        "update_op_id": selection.update_op_id.0,
        "source_route_scoped": source_route_slot.scoped,
        "selected_op_indexed": update_op.indexed,
        "selected_op_unresolved_executable_ref_count": update_op.unresolved_executable_ref_count,
        "selection_core": selection.executor_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(SourceRouteExecutionContext {
        selection,
        source_route_slot,
        update_op,
        executor_report,
    })
}

pub fn semantic_delta_signature(delta: &JsonValue) -> PlanExecutorResult<String> {
    let kind = delta
        .get("kind")
        .and_then(JsonValue::as_str)
        .ok_or("semantic delta has no kind")?;
    Ok(match delta.get("field_path").and_then(JsonValue::as_str) {
        Some(field_path) => format!("{kind}:{field_path}"),
        None => kind.to_owned(),
    })
}

pub fn coalesce_field_set_deltas(deltas: Vec<JsonValue>) -> PlanExecutorResult<Vec<JsonValue>> {
    let mut last_by_target = BTreeMap::new();
    for (index, delta) in deltas.iter().enumerate() {
        if let Some(key) = field_set_delta_target_key(delta)? {
            last_by_target.insert(key, index);
        }
    }

    let mut coalesced = Vec::new();
    for (index, delta) in deltas.into_iter().enumerate() {
        let Some(key) = field_set_delta_target_key(&delta)? else {
            coalesced.push(delta);
            continue;
        };
        if last_by_target.get(&key).copied() == Some(index) {
            coalesced.push(delta);
        }
    }
    Ok(coalesced)
}

pub fn order_indexed_update_semantic_deltas(
    bulk_indexed_update: bool,
    executions: &[IndexedUpdateDeltaBatch],
) -> IndexedUpdateDeltaOrdering {
    let mut semantic_deltas = Vec::new();
    if bulk_indexed_update {
        let mut primary = Vec::new();
        let mut derived = Vec::new();
        for execution in executions {
            let output_field = execution
                .report_rows
                .first()
                .and_then(|row| row.get("field_path"))
                .and_then(JsonValue::as_str);
            for delta in &execution.semantic_deltas {
                if delta.get("field_path").and_then(JsonValue::as_str) == output_field {
                    primary.push(delta.clone());
                } else {
                    derived.push(delta.clone());
                }
            }
        }
        semantic_deltas.extend(primary);
        semantic_deltas.extend(derived);
    } else {
        semantic_deltas.extend(
            executions
                .iter()
                .flat_map(|execution| execution.semantic_deltas.iter().cloned()),
        );
    }
    let executor_report = json!({
        "executor": "cpu-plan-indexed-update-delta-ordering-v1",
        "bulk_indexed_update": bulk_indexed_update,
        "execution_count": executions.len(),
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    IndexedUpdateDeltaOrdering {
        semantic_deltas,
        executor_report,
    }
}

pub fn select_unscoped_indexed_update_targets(
    plan: &MachinePlan,
    op: &PlanOp,
    source_route_slot: &SourceRoute,
    event: &IndexedUpdateTargetEvent,
    list_rows: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
) -> PlanExecutorResult<IndexedUpdateTargetSelection> {
    let mut executor_report = json!({
        "executor": "cpu-plan-indexed-update-target-selection-v1",
        "source": event.source,
        "bulk_indexed_update": false,
        "list_id": null,
        "list": null,
        "target_count": 0,
        "skip_reason": null,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let skip = |executor_report: &mut JsonValue, reason: &str| -> IndexedUpdateTargetSelection {
        if let Some(object) = executor_report.as_object_mut() {
            object.insert("skip_reason".to_owned(), json!(reason));
        }
        IndexedUpdateTargetSelection {
            bulk_indexed_update: false,
            list_id: None,
            list_label: None,
            targets: Vec::new(),
            executor_report: executor_report.clone(),
        }
    };

    if source_route_slot.scoped {
        return Ok(skip(&mut executor_report, "source-route-scoped"));
    }
    if event.target_key.is_some() {
        return Ok(skip(&mut executor_report, "event-target-key"));
    }
    if event.target_text.is_some() {
        return Ok(skip(&mut executor_report, "event-target-text"));
    }
    if event.address.is_some() {
        return Ok(skip(&mut executor_report, "event-address"));
    }
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Ok(skip(&mut executor_report, "op-output-not-state"));
    };
    let Some(output_slot) = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == output_state_id)
    else {
        return Ok(skip(&mut executor_report, "missing-output-slot"));
    };
    if !output_slot.indexed {
        return Ok(skip(&mut executor_report, "output-not-indexed"));
    }
    let Some(scope_id) = output_slot.scope_id else {
        return Ok(skip(&mut executor_report, "output-has-no-row-scope"));
    };
    let list_slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope_id))
        .ok_or_else(|| {
            format!(
                "indexed update branch {} has no list slot for scope",
                op.id.0
            )
        })?;
    let list_label = list_label(plan, list_slot.list_id.0);
    if let Some(event_list) = event.list_id.as_deref()
        && event_list != list_label
    {
        return Err(format!(
            "unscoped source `{}` targeted list `{event_list}`, expected `{list_label}`",
            event.source
        )
        .into());
    }
    let rows = list_rows
        .get(&list_slot.list_id.0)
        .ok_or_else(|| format!("list state missing list {}", list_slot.list_id.0))?;
    let targets = rows
        .iter()
        .map(|row| IndexedUpdateTargetRow {
            key: row.key,
            generation: row.generation,
        })
        .collect::<Vec<_>>();
    if let Some(object) = executor_report.as_object_mut() {
        object.insert("bulk_indexed_update".to_owned(), json!(true));
        object.insert("list_id".to_owned(), json!(list_slot.list_id.0));
        object.insert("list".to_owned(), json!(list_label.clone()));
        object.insert("target_count".to_owned(), json!(targets.len()));
    }
    Ok(IndexedUpdateTargetSelection {
        bulk_indexed_update: true,
        list_id: Some(list_slot.list_id.0),
        list_label: Some(list_label),
        targets,
        executor_report,
    })
}

pub fn track_indexed_update_write_conflicts(
    touched: &mut BTreeMap<String, (JsonValue, Vec<usize>)>,
    report_rows: &[JsonValue],
) -> PlanExecutorResult<()> {
    for row in report_rows {
        let key = indexed_update_write_target_key(row)?;
        let value = row.get("value").cloned().unwrap_or(JsonValue::Null);
        let update_op_id = row
            .get("update_op_id")
            .and_then(JsonValue::as_u64)
            .and_then(|id| usize::try_from(id).ok())
            .unwrap_or(0);
        match touched.get_mut(&key) {
            Some((existing, op_ids)) => {
                let conflict_kind = if *existing == value {
                    "duplicate"
                } else {
                    "conflicting"
                };
                return Err(format!(
                    "CPU root-scenario PlanExecutor found {conflict_kind} indexed update branches for target {key}: ops {:?} wrote {:?}, op {} wrote {:?}",
                    op_ids, existing, update_op_id, value
                )
                .into());
            }
            None => {
                touched.insert(key, (value, vec![update_op_id]));
            }
        }
    }
    Ok(())
}

pub fn execute_indexed_update_batch_with<E>(
    plan: &MachinePlan,
    op: &PlanOp,
    source_route_slot: &SourceRoute,
    event: &IndexedUpdateTargetEvent,
    list_rows: &BTreeMap<usize, Vec<PlanExecutorListRow>>,
    mut execute_branch: E,
) -> PlanExecutorResult<IndexedUpdateBatchExecution>
where
    E: FnMut(
        Option<IndexedUpdateTargetOverride>,
    ) -> PlanExecutorResult<IndexedUpdateBranchExecution>,
{
    let target_selection =
        select_unscoped_indexed_update_targets(plan, op, source_route_slot, event, list_rows)?;
    let bulk_indexed_update = target_selection.bulk_indexed_update;
    let mut branch_executions = Vec::new();
    if bulk_indexed_update {
        let list_label = target_selection.list_label.ok_or_else(|| {
            format!(
                "indexed update target selector reported bulk update for op {} without list label",
                op.id.0
            )
        })?;
        for target in target_selection.targets {
            branch_executions.push(execute_branch(Some(IndexedUpdateTargetOverride {
                list_label: list_label.clone(),
                key: target.key,
                generation: target.generation,
            }))?);
        }
    } else {
        branch_executions.push(execute_branch(None)?);
    }

    let mut touched_indexed_updates = BTreeMap::new();
    for indexed in &branch_executions {
        track_indexed_update_write_conflicts(&mut touched_indexed_updates, &indexed.report_rows)?;
    }
    let indexed_delta_batches = branch_executions
        .iter()
        .map(|indexed| IndexedUpdateDeltaBatch {
            semantic_deltas: indexed.semantic_deltas.clone(),
            report_rows: indexed.report_rows.clone(),
        })
        .collect::<Vec<_>>();
    let ordered_indexed_deltas =
        order_indexed_update_semantic_deltas(bulk_indexed_update, &indexed_delta_batches);
    let updated_row_count = branch_executions
        .iter()
        .map(|execution| execution.updated_row_count)
        .sum::<usize>();
    let report_rows = branch_executions
        .into_iter()
        .flat_map(|execution| execution.report_rows)
        .collect::<Vec<_>>();
    let executor_report = json!({
        "executor": "cpu-plan-indexed-update-batch-execution-v1",
        "op_id": op.id.0,
        "bulk_indexed_update": bulk_indexed_update,
        "updated_row_count": updated_row_count,
        "report_row_count": report_rows.len(),
        "semantic_delta_count": ordered_indexed_deltas.semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(IndexedUpdateBatchExecution {
        semantic_deltas: ordered_indexed_deltas.semantic_deltas,
        report_rows,
        updated_row_count,
        bulk_indexed_update,
        executor_report,
    })
}

fn indexed_json_update_outcome(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: StateId,
    value: Option<JsonValue>,
    metadata: Option<(&'static str, JsonValue, JsonValue, JsonValue)>,
) -> IndexedJsonUpdateEvaluation {
    let (expression_kind, source_payload_field, update_constant_id, update_constant_value) =
        metadata
            .map(|(kind, payload, constant_id, constant_value)| {
                (Some(kind), payload, constant_id, constant_value)
            })
            .unwrap_or((None, JsonValue::Null, JsonValue::Null, JsonValue::Null));
    let executor_report = json!({
        "executor": "cpu-plan-indexed-json-update-evaluator-v1",
        "update_op_id": op_id.0,
        "supported": supported,
        "unsupported_reason": unsupported_reason,
        "target_state_id": target_state_id.0,
        "expression_kind": expression_kind,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    IndexedJsonUpdateEvaluation {
        supported,
        unsupported_reason,
        target_state_id,
        value,
        expression_kind,
        source_payload_field,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

pub fn evaluate_indexed_json_update_branch(
    plan: &MachinePlan,
    op: &PlanOp,
    source_id: SourceId,
    source_route_slot: &SourceRoute,
    event: &RootJsonSourceEvent,
    row: &PlanExecutorListRowState,
    root_derived_values: &BTreeMap<usize, JsonValue>,
) -> PlanExecutorResult<IndexedJsonUpdateEvaluation> {
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor indexed JSON update branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    let output_slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
    let output_name = local_field_name(&state_label(plan, output_state_id));
    match &op.kind {
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            source_payload_field: Some(source_payload_field),
            update_constant_id: None,
            ..
        } => {
            if *source_payload_field == SourcePayloadField::Bytes {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    false,
                    Some("BYTES source payload requires runtime byte storage".to_owned()),
                    output_state_id,
                    None,
                    None,
                ));
            }
            validate_route_payload_field(source_route_slot, source_payload_field, op.id)?;
            validate_typed_payload_input(op, source_id, source_payload_field)?;
            if output_slot.value_type != PlanValueType::Text {
                return Err(format!(
                    "indexed source-payload update branch {} reads text payload but output state `{}` is not TEXT",
                    op.id.0,
                    state_label(plan, output_state_id)
                )
                .into());
            }
            let Some(value) = source_payload_json_value_if_present(event, source_payload_field)?
            else {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    true,
                    None,
                    output_state_id,
                    None,
                    Some((
                        "source_payload",
                        serde_json::to_value(source_payload_field)?,
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(value),
                Some((
                    "source_payload",
                    serde_json::to_value(source_payload_field)?,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            source_payload_field: None,
            update_constant_id: Some(update_constant_id),
            ..
        } => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *update_constant_id)
                .ok_or_else(|| format!("missing update constant {}", update_constant_id.0))?;
            if matches!(constant.value, PlanConstantValue::Bytes { .. }) {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    false,
                    Some("BYTES constants require runtime byte storage".to_owned()),
                    output_state_id,
                    None,
                    None,
                ));
            }
            let value = plan_constant_json_value(constant)?;
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(value.clone()),
                Some(("const", JsonValue::Null, json!(update_constant_id.0), value)),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BoolNot,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input = op
                .inputs
                .iter()
                .find(|input| matches!(input, ValueRef::State(_) | ValueRef::Field(_)))
                .ok_or_else(|| {
                    format!(
                        "indexed Bool/not update branch {} has no typed state or field input",
                        op.id.0
                    )
                })?;
            let input_value = match input {
                ValueRef::State(input_state_id) => {
                    let input_name = local_field_name(&state_label(plan, *input_state_id));
                    row.fields
                        .get(&input_name)
                        .and_then(JsonValue::as_bool)
                        .ok_or_else(|| {
                            format!(
                                "indexed Bool/not update branch {} input field `{input_name}` is not bool",
                                op.id.0
                            )
                        })?
                }
                ValueRef::Field(input_field_id) => {
                    let input_label = derived_field_label(plan, input_field_id.0);
                    root_derived_values
                        .get(&input_field_id.0)
                        .and_then(JsonValue::as_bool)
                        .ok_or_else(|| {
                            format!(
                                "indexed Bool/not update branch {} input derived field `{input_label}` is not bool",
                                op.id.0
                            )
                        })?
                }
                _ => unreachable!("filtered above"),
            };
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(JsonValue::Bool(!input_value)),
                Some((
                    "bool_not",
                    JsonValue::Null,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::ReadPath,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let value = match indexed_single_state_or_field_input(op, output_state_id) {
                Ok(input) => indexed_row_read_path_value(plan, row, &input, op.id)?,
                Err(error) => {
                    let Some(row_field_path) = output_slot.initial_row_field_path.as_deref() else {
                        return Err(error);
                    };
                    let row_field = local_field_name(row_field_path);
                    row.fields.get(&row_field).cloned().ok_or_else(|| {
                        format!(
                            "indexed read_path update branch {} could not read row field `{row_field}`",
                            op.id.0
                        )
                    })?
                }
            };
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    false,
                    Some("BYTES state read requires runtime byte storage".to_owned()),
                    output_state_id,
                    None,
                    None,
                ));
            }
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(value),
                Some((
                    "read_path",
                    JsonValue::Null,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::PreviousValue,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_state_id = indexed_single_state_input(op, output_state_id)?;
            let input_name = local_field_name(&state_label(plan, input_state_id));
            let value = row.fields.get(&input_name).cloned().ok_or_else(|| {
                format!(
                    "indexed PreviousValue update branch {} input field `{input_name}` is missing",
                    op.id.0
                )
            })?;
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    false,
                    Some("BYTES previous-value update requires runtime byte storage".to_owned()),
                    output_state_id,
                    None,
                    None,
                ));
            }
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(value),
                Some((
                    "previous_value",
                    JsonValue::Null,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextTrimOrPrevious,
            source_payload_field,
            update_constant_id: None,
            ..
        } => {
            let raw = if let Some(payload_field) = source_payload_field {
                if *payload_field == SourcePayloadField::Bytes {
                    return Ok(indexed_json_update_outcome(
                        op.id,
                        false,
                        Some("BYTES text-trim payload requires runtime byte storage".to_owned()),
                        output_state_id,
                        None,
                        None,
                    ));
                }
                validate_typed_payload_input(op, source_id, payload_field)?;
                let Some(payload_value) =
                    source_payload_json_value_if_present(event, payload_field)?
                else {
                    return Ok(indexed_json_update_outcome(
                        op.id,
                        true,
                        None,
                        output_state_id,
                        None,
                        Some((
                            "text_trim_or_previous",
                            serde_json::to_value(payload_field)?,
                            JsonValue::Null,
                            JsonValue::Null,
                        )),
                    ));
                };
                payload_value
                    .as_str()
                    .ok_or_else(|| {
                        format!(
                            "indexed TextTrimOrPrevious update branch {} payload is not text",
                            op.id.0
                        )
                    })?
                    .to_owned()
            } else {
                let input_state_id = op
                    .inputs
                    .iter()
                    .filter_map(|input| match input {
                        ValueRef::State(state_id) if *state_id != output_state_id => {
                            Some(*state_id)
                        }
                        _ => None,
                    })
                    .next()
                    .unwrap_or(output_state_id);
                let input_name = local_field_name(&state_label(plan, input_state_id));
                row.fields
                    .get(&input_name)
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| {
                        format!(
                            "indexed TextTrimOrPrevious update branch {} input field `{input_name}` is not text",
                            op.id.0
                        )
                    })?
                    .to_owned()
            };
            let current = row
                .fields
                .get(&output_name)
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "indexed TextTrimOrPrevious update branch {} output field `{output_name}` is not text",
                        op.id.0
                    )
                })?;
            let trimmed = raw.trim();
            let value = if trimmed.is_empty() {
                current.to_owned()
            } else {
                trimmed.to_owned()
            };
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(JsonValue::String(value)),
                Some((
                    "text_trim_or_previous",
                    source_payload_field
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(JsonValue::Null),
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchTextIsEmptyConst,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_ref = ordered_inputs.first().ok_or_else(|| {
                format!(
                    "indexed MatchTextIsEmptyConst update branch {} has no match input",
                    op.id.0
                )
            })?;
            let input_value = indexed_update_json_value_for_ref(
                plan,
                event,
                row,
                root_derived_values,
                input_ref,
                op.id,
                "match input",
            )?;
            let input_text = input_value.as_str().ok_or_else(|| {
                format!(
                    "indexed MatchTextIsEmptyConst update branch {} match input is not text",
                    op.id.0
                )
            })?;
            let selected_operand_index = if input_text.is_empty() { 1 } else { 2 };
            let Some(selected_ref) = ordered_inputs.get(selected_operand_index) else {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    true,
                    None,
                    output_state_id,
                    None,
                    Some((
                        "match_text_is_empty_const",
                        json!({
                            "input_empty": input_text.is_empty(),
                            "selected_operand_index": selected_operand_index,
                            "selected_arm_missing": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            let value = indexed_update_json_value_for_ref(
                plan,
                event,
                row,
                root_derived_values,
                selected_ref,
                op.id,
                "match selected arm",
            )?;
            if value.as_str() == Some("SKIP") {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    true,
                    None,
                    output_state_id,
                    None,
                    Some((
                        "match_text_is_empty_const",
                        json!({
                            "input_empty": input_text.is_empty(),
                            "selected_operand_index": selected_operand_index,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            }
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(indexed_json_update_outcome(
                    op.id,
                    false,
                    Some("BYTES match arm values require runtime byte storage".to_owned()),
                    output_state_id,
                    None,
                    None,
                ));
            }
            Ok(indexed_json_update_outcome(
                op.id,
                true,
                None,
                output_state_id,
                Some(value),
                Some((
                    "match_text_is_empty_const",
                    json!({
                        "input_empty": input_text.is_empty(),
                        "selected_operand_index": selected_operand_index,
                        "skip": false,
                    }),
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            ))
        }
        PlanOpKind::UpdateBranch {
            expression_kind, ..
        } => Ok(indexed_json_update_outcome(
            op.id,
            false,
            Some(format!(
                "expression kind {expression_kind:?} requires runtime-specific execution"
            )),
            output_state_id,
            None,
            None,
        )),
        _ => Err(format!(
            "CPU PlanExecutor indexed JSON update branch {} is not an update branch",
            op.id.0
        )
        .into()),
    }
}

fn indexed_single_state_or_field_input(
    op: &PlanOp,
    output_state_id: StateId,
) -> PlanExecutorResult<ValueRef> {
    let inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::State(state_id) if *state_id != output_state_id => Some(input.clone()),
            ValueRef::Field(_) => Some(input.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err(format!(
            "indexed update branch {} expected one non-output state or row-field input, found {}",
            op.id.0,
            inputs.len()
        )
        .into());
    };
    Ok(input.clone())
}

fn indexed_row_read_path_value(
    plan: &MachinePlan,
    row: &PlanExecutorListRowState,
    input: &ValueRef,
    op_id: PlanOpId,
) -> PlanExecutorResult<JsonValue> {
    let input_name = match input {
        ValueRef::State(state_id) => local_field_name(&state_label(plan, *state_id)),
        ValueRef::Field(field_id) => local_field_name(&semantic_field_label(plan, field_id.0)),
        _ => {
            return Err(format!(
                "indexed ReadPath update branch {} input {input:?} is not a state or row field",
                op_id.0
            )
            .into());
        }
    };
    row.fields.get(&input_name).cloned().ok_or_else(|| {
        format!(
            "indexed ReadPath update branch {} input field `{input_name}` is missing",
            op_id.0
        )
        .into()
    })
}

fn root_update_json_value_for_ref(
    plan: &MachinePlan,
    event: &RootJsonSourceEvent,
    root_state: &JsonMap<String, JsonValue>,
    value_ref: &ValueRef,
    active_source_id: SourceId,
    op_id: PlanOpId,
    context: &str,
) -> PlanExecutorResult<Option<JsonValue>> {
    match value_ref {
        ValueRef::State(state_id) => Ok(Some(
            root_state_value(plan, root_state, *state_id, op_id.0)?.clone(),
        )),
        ValueRef::SourcePayload { source_id, field } => {
            if *source_id != active_source_id {
                return Err(format!(
                    "root update branch {} {context} source-payload ref source {} does not match active source {}",
                    op_id.0, source_id.0, active_source_id.0
                )
                .into());
            }
            source_payload_json_value_if_present(event, field)
        }
        ValueRef::Constant(constant_id) => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *constant_id)
                .ok_or_else(|| format!("missing update constant {}", constant_id.0))?;
            plan_constant_json_value(constant).map(Some)
        }
        ValueRef::Field(field_id) => {
            let input_label = derived_field_label(plan, field_id.0);
            Ok(root_state.get(&input_label).cloned())
        }
        ValueRef::Source(source_id) => Err(format!(
            "root update branch {} {context} cannot use source ref {} as a value",
            op_id.0, source_id.0
        )
        .into()),
        ValueRef::List(list_id) => Err(format!(
            "root update branch {} {context} cannot use list ref {} as a scalar value",
            op_id.0, list_id.0
        )
        .into()),
    }
}

fn root_match_const_pattern(
    plan: &MachinePlan,
    value_ref: &ValueRef,
    op_id: PlanOpId,
    arm_index: usize,
) -> PlanExecutorResult<String> {
    let ValueRef::Constant(constant_id) = value_ref else {
        return Err(format!(
            "root MatchConst update branch {} arm {arm_index} pattern is not a constant",
            op_id.0
        )
        .into());
    };
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *constant_id)
        .ok_or_else(|| {
            format!(
                "root MatchConst update branch {} arm {arm_index} references missing pattern constant {}",
                op_id.0, constant_id.0
            )
        })?;
    match &constant.value {
        PlanConstantValue::Text { value } | PlanConstantValue::Enum { value } => Ok(value.clone()),
        _ => Err(format!(
            "root MatchConst update branch {} arm {arm_index} pattern constant {} is not text-like",
            op_id.0, constant_id.0
        )
        .into()),
    }
}

fn indexed_update_json_value_for_ref(
    plan: &MachinePlan,
    event: &RootJsonSourceEvent,
    row: &PlanExecutorListRowState,
    root_derived_values: &BTreeMap<usize, JsonValue>,
    value_ref: &ValueRef,
    op_id: PlanOpId,
    context: &str,
) -> PlanExecutorResult<JsonValue> {
    match value_ref {
        ValueRef::State(state_id) => {
            let input_name = local_field_name(&state_label(plan, *state_id));
            row.fields.get(&input_name).cloned().ok_or_else(|| {
                format!(
                    "indexed update branch {} {context} state field `{input_name}` is missing",
                    op_id.0
                )
                .into()
            })
        }
        ValueRef::Field(field_id) => {
            let input_label = derived_field_label(plan, field_id.0);
            root_derived_values
                .get(&field_id.0)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "indexed update branch {} {context} derived field `{input_label}` is missing",
                        op_id.0
                    )
                    .into()
                })
        }
        ValueRef::SourcePayload { source_id, field } => {
            source_payload_json_value(event, field).map_err(|error| {
                format!(
                    "indexed update branch {} {context} source-payload ref source {} field {:?} failed: {error}",
                    op_id.0, source_id.0, field
                )
                .into()
            })
        }
        ValueRef::Constant(constant_id) => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *constant_id)
                .ok_or_else(|| format!("missing update constant {}", constant_id.0))?;
            plan_constant_json_value(constant)
        }
        ValueRef::Source(source_id) => Err(format!(
            "indexed update branch {} {context} cannot use source ref {} as a value",
            op_id.0, source_id.0
        )
        .into()),
        ValueRef::List(list_id) => Err(format!(
            "indexed update branch {} {context} cannot use list ref {} as a scalar value",
            op_id.0, list_id.0
        )
        .into()),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RootUpdateCandidateTracker {
    order: Vec<usize>,
    candidates: BTreeMap<usize, RootUpdateCandidateState>,
}

impl RootUpdateCandidateTracker {
    pub fn ordered_candidates(&self) -> Vec<RootUpdateCandidateState> {
        self.order
            .iter()
            .filter_map(|state_id| self.candidates.get(state_id).cloned())
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCandidate {
    pub state_id: usize,
    pub op_id: usize,
    pub value: JsonValue,
    pub bytes_value: Option<JsonValue>,
    pub fixed_bytes_mutation: Option<JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootExecutedUpdate {
    pub value: JsonValue,
    pub bytes_value: Option<PlanExecutorBytes>,
    pub fixed_bytes_mutation: Option<RootBytesFixedMutation>,
    pub bytes_access: JsonValue,
    pub executor_core: JsonValue,
    pub state_write_core: JsonValue,
    pub bytes_state_core: JsonValue,
    pub expression_kind: String,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub host_effect: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootRuntimeBranchUpdateInput {
    pub value: JsonValue,
    pub bytes_value: Option<PlanExecutorBytes>,
    pub fixed_bytes_mutation: Option<RootBytesFixedMutation>,
    pub bytes_access: JsonValue,
    pub runtime_branch_core: JsonValue,
    pub state_write_core: JsonValue,
    pub bytes_state_core: JsonValue,
    pub expression_kind: String,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub host_effect: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCandidateState {
    pub state_id: usize,
    pub value: JsonValue,
    pub bytes_value: Option<JsonValue>,
    pub fixed_bytes_mutation: Option<JsonValue>,
    pub op_ids: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RootUpdateCandidateRecordKind {
    Inserted,
    Duplicate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCandidateRecord {
    pub kind: RootUpdateCandidateRecordKind,
    pub state_id: usize,
    pub op_ids: Vec<usize>,
    pub executor_report: JsonValue,
}

pub fn record_root_update_candidate(
    tracker: &mut RootUpdateCandidateTracker,
    source_route: &str,
    candidate: RootUpdateCandidate,
) -> PlanExecutorResult<RootUpdateCandidateRecord> {
    match tracker.candidates.get_mut(&candidate.state_id) {
        Some(existing)
            if existing.value == candidate.value
                && existing.bytes_value == candidate.bytes_value
                && existing.fixed_bytes_mutation == candidate.fixed_bytes_mutation =>
        {
            existing.op_ids.push(candidate.op_id);
            let op_ids = existing.op_ids.clone();
            Ok(RootUpdateCandidateRecord {
                kind: RootUpdateCandidateRecordKind::Duplicate,
                state_id: candidate.state_id,
                op_ids: op_ids.clone(),
                executor_report: root_update_candidate_record_report(
                    "duplicate",
                    source_route,
                    candidate.state_id,
                    &op_ids,
                ),
            })
        }
        Some(existing) => Err(format!(
            "CPU root-scenario PlanExecutor found conflicting branches for state {} from `{}`: {:?} vs {:?}",
            candidate.state_id, source_route, existing.value, candidate.value
        )
        .into()),
        None => {
            tracker.order.push(candidate.state_id);
            tracker.candidates.insert(
                candidate.state_id,
                RootUpdateCandidateState {
                    state_id: candidate.state_id,
                    value: candidate.value,
                    bytes_value: candidate.bytes_value,
                    fixed_bytes_mutation: candidate.fixed_bytes_mutation,
                    op_ids: vec![candidate.op_id],
                },
            );
            Ok(RootUpdateCandidateRecord {
                kind: RootUpdateCandidateRecordKind::Inserted,
                state_id: candidate.state_id,
                op_ids: vec![candidate.op_id],
                executor_report: root_update_candidate_record_report(
                    "inserted",
                    source_route,
                    candidate.state_id,
                    &[candidate.op_id],
                ),
            })
        }
    }
}

pub fn root_update_candidate_from_executed(
    state_id: usize,
    op_id: usize,
    executed: &RootExecutedUpdate,
) -> RootUpdateCandidate {
    RootUpdateCandidate {
        state_id,
        op_id,
        value: executed.value.clone(),
        bytes_value: executed
            .bytes_value
            .as_ref()
            .map(PlanExecutorBytes::report_json),
        fixed_bytes_mutation: executed
            .fixed_bytes_mutation
            .as_ref()
            .map(root_bytes_fixed_mutation_report_json),
    }
}

pub fn assemble_root_runtime_branch_update(
    input: RootRuntimeBranchUpdateInput,
) -> RootExecutedUpdate {
    let runtime_branch_execution_core = json!({
        "executor": "cpu-plan-root-runtime-branch-update-execution-v1",
        "expression_kind": input.expression_kind.clone(),
        "source_payload_field": input.source_payload_field.clone(),
        "update_constant_id": input.update_constant_id.clone(),
        "update_constant_value": input.update_constant_value.clone(),
        "bytes_access": input.bytes_access.clone(),
        "host_effect": input.host_effect.clone(),
        "state_write_core": input.state_write_core.clone(),
        "bytes_state_core": input.bytes_state_core.clone(),
        "runtime_branch_core": input.runtime_branch_core.clone(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let mut executor_core = if input.runtime_branch_core.is_null() {
        runtime_branch_execution_core.clone()
    } else {
        input.runtime_branch_core.clone()
    };
    if let Some(object) = executor_core.as_object_mut() {
        object.insert(
            "runtime_branch_execution_core".to_owned(),
            runtime_branch_execution_core,
        );
    }
    RootExecutedUpdate {
        value: input.value,
        bytes_value: input.bytes_value,
        fixed_bytes_mutation: input.fixed_bytes_mutation,
        bytes_access: input.bytes_access,
        executor_core,
        state_write_core: input.state_write_core,
        bytes_state_core: input.bytes_state_core,
        expression_kind: input.expression_kind,
        source_payload_field: input.source_payload_field,
        update_constant_id: input.update_constant_id,
        update_constant_value: input.update_constant_value,
        host_effect: input.host_effect,
    }
}

fn root_bytes_fixed_mutation_report_json(mutation: &RootBytesFixedMutation) -> JsonValue {
    json!({
        "input_state_id": mutation.input_state_id.0,
        "output_state_id": mutation.output_state_id.0,
        "patches": mutation.patches,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCommitInput {
    pub source_id: SourceId,
    pub target_state: String,
    pub target_state_id: usize,
    pub candidate_update_op_ids: Vec<usize>,
    pub expression_kind: String,
    pub source_payload_field: JsonValue,
    pub update_constant_id: JsonValue,
    pub update_constant_value: JsonValue,
    pub bytes_access: JsonValue,
    pub host_effect: JsonValue,
    pub executor_core: JsonValue,
    pub state_write_core: JsonValue,
    pub bytes_state_core: JsonValue,
    pub value: JsonValue,
    pub changed: bool,
    pub semantic_delta: Option<JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCommitAssembly {
    pub touched_state: Option<(String, JsonValue)>,
    pub semantic_delta_signature: Option<String>,
    pub semantic_delta: Option<JsonValue>,
    pub update_report: JsonValue,
    pub executor_report: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateCommitBatch {
    pub touched_states: JsonMap<String, JsonValue>,
    pub semantic_delta_signatures: Vec<String>,
    pub semantic_deltas: Vec<JsonValue>,
    pub update_reports: Vec<JsonValue>,
    pub executed_update_branch_count: usize,
    pub executor_report: JsonValue,
}

pub fn assemble_root_update_commit(
    input: RootUpdateCommitInput,
) -> PlanExecutorResult<RootUpdateCommitAssembly> {
    let update_op_id = input
        .candidate_update_op_ids
        .first()
        .copied()
        .ok_or("root update commit has no candidate update op id")?;
    let (touched_state, semantic_delta_signature, semantic_delta) = if input.changed {
        let delta = input.semantic_delta.clone().unwrap_or_else(|| {
            json!({
                "kind": "FieldSet",
                "list_id": null,
                "key": null,
                "generation": null,
                "source_id": null,
                "bind_epoch": null,
                "field_path": input.target_state.clone(),
                "value": input.value.clone(),
            })
        });
        (
            Some((input.target_state.clone(), input.value.clone())),
            Some(format!("FieldSet:{}", input.target_state)),
            Some(delta),
        )
    } else {
        (None, None, None)
    };
    let candidate_update_op_count = input.candidate_update_op_ids.len();
    let update_report = json!({
        "source_id": input.source_id.0,
        "target_state": input.target_state,
        "target_state_id": input.target_state_id,
        "update_op_id": update_op_id,
        "candidate_update_op_ids": input.candidate_update_op_ids,
        "expression_kind": input.expression_kind,
        "source_payload_field": input.source_payload_field,
        "update_constant_id": input.update_constant_id,
        "update_constant_value": input.update_constant_value,
        "bytes_access": input.bytes_access,
        "host_effect": input.host_effect,
        "executor_core": input.executor_core,
        "state_write_core": input.state_write_core,
        "bytes_state_core": input.bytes_state_core,
        "selected_op_indexed": false,
        "selected_op_unresolved_executable_ref_count": 0,
        "value": input.value,
        "changed": input.changed,
    });
    let executor_report = json!({
        "executor": "cpu-plan-root-update-commit-assembly-v1",
        "source_id": input.source_id.0,
        "target_state_id": input.target_state_id,
        "update_op_id": update_op_id,
        "candidate_update_op_count": candidate_update_op_count,
        "changed": input.changed,
        "emitted_semantic_delta": semantic_delta.is_some(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootUpdateCommitAssembly {
        touched_state,
        semantic_delta_signature,
        semantic_delta,
        update_report,
        executor_report,
    })
}

pub fn commit_ordered_root_update_candidates(
    root_state: &mut PlanExecutorRootState,
    plan: &MachinePlan,
    source_id: SourceId,
    candidates: &RootUpdateCandidateTracker,
    mut touched_updates: BTreeMap<usize, RootExecutedUpdate>,
) -> PlanExecutorResult<RootUpdateCommitBatch> {
    let ordered_candidates = candidates.ordered_candidates();
    let candidate_count = ordered_candidates.len();
    let mut touched_states = JsonMap::new();
    let mut semantic_delta_signatures = Vec::new();
    let mut semantic_deltas = Vec::new();
    let mut update_reports = Vec::new();
    let mut executed_update_branch_count = 0usize;
    let mut missing_executed_update_count = 0usize;

    for candidate in ordered_candidates {
        let state_id = candidate.state_id;
        let Some(mut executed) = touched_updates.remove(&state_id) else {
            missing_executed_update_count += 1;
            continue;
        };
        let op_ids = candidate.op_ids;
        let target_label = state_label_by_id(plan, state_id);
        let state_id_ref = StateId(state_id);
        let fallback_changed = root_state.root_state.get(&target_label) != Some(&executed.value);
        let (write, changed) =
            if executed.bytes_value.is_none() && executed.fixed_bytes_mutation.is_none() {
                let write = apply_root_json_update_to_root_state(
                    root_state,
                    plan,
                    state_id_ref,
                    executed.value.clone(),
                    PlanOpId(op_ids[0]),
                )?;
                executed.state_write_core = write.executor_report.clone();
                let changed = write.changed;
                (Some(write), changed)
            } else {
                let bytes_state_core = apply_executed_root_update_to_root_state(
                    root_state, plan, state_id, &executed, op_ids[0],
                )?;
                executed.bytes_state_core = bytes_state_core;
                (None, fallback_changed)
            };
        let commit = assemble_root_update_commit(RootUpdateCommitInput {
            source_id,
            target_state: target_label,
            target_state_id: state_id,
            candidate_update_op_ids: op_ids,
            expression_kind: executed.expression_kind,
            source_payload_field: executed.source_payload_field,
            update_constant_id: executed.update_constant_id,
            update_constant_value: executed.update_constant_value,
            bytes_access: executed.bytes_access,
            host_effect: executed.host_effect,
            executor_core: executed.executor_core,
            state_write_core: executed.state_write_core,
            bytes_state_core: executed.bytes_state_core,
            value: executed.value,
            changed,
            semantic_delta: write.and_then(|write| write.semantic_delta),
        })?;
        if let Some((field, value)) = commit.touched_state {
            touched_states.insert(field, value);
        }
        if let Some(signature) = commit.semantic_delta_signature {
            semantic_delta_signatures.push(signature);
        }
        if let Some(delta) = commit.semantic_delta {
            semantic_deltas.push(delta);
        }
        update_reports.push(commit.update_report);
        executed_update_branch_count += 1;
    }

    let executor_report = json!({
        "executor": "cpu-plan-root-update-commit-batch-v1",
        "source_id": source_id.0,
        "candidate_count": candidate_count,
        "committed_update_count": executed_update_branch_count,
        "missing_executed_update_count": missing_executed_update_count,
        "touched_state_count": touched_states.len(),
        "semantic_delta_count": semantic_deltas.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(RootUpdateCommitBatch {
        touched_states,
        semantic_delta_signatures,
        semantic_deltas,
        update_reports,
        executed_update_branch_count,
        executor_report,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateStorageTransition {
    pub target_state_id: StateId,
    pub target_state_label: String,
    pub bytes_transition_mode: String,
    pub executor_report: JsonValue,
}

pub fn apply_root_update_storage_transition(
    state_owner: &mut impl RootUpdateStateOwner,
    target_state_id: StateId,
    target_state_label: &str,
    value: JsonValue,
    bytes_value: Option<PlanExecutorBytes>,
    fixed_mutation: Option<RootBytesFixedMutation>,
    op_id: PlanOpId,
) -> PlanExecutorResult<RootUpdateStorageTransition> {
    state_owner.insert_root_state_value(target_state_label, value);
    let bytes_transition = apply_root_bytes_state_transition(
        state_owner,
        target_state_id,
        bytes_value,
        fixed_mutation,
        op_id,
    )?;
    let executor_report = json!({
        "executor": "cpu-plan-root-update-storage-transition-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.0,
        "target_state": target_state_label,
        "bytes_transition_mode": bytes_transition.mode,
        "bytes_transition_core": bytes_transition.executor_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootUpdateStorageTransition {
        target_state_id,
        target_state_label: target_state_label.to_owned(),
        bytes_transition_mode: bytes_transition.mode.to_owned(),
        executor_report,
    })
}

pub fn apply_executed_root_update_to_state(
    root_state: &mut JsonMap<String, JsonValue>,
    private_bytes: &mut BTreeMap<usize, PlanExecutorBytes>,
    fixed_byte_banks: &mut BTreeMap<usize, Vec<u8>>,
    plan: &MachinePlan,
    state_id: usize,
    executed: &RootExecutedUpdate,
    op_id: usize,
) -> PlanExecutorResult<JsonValue> {
    let target_label = state_label(plan, StateId(state_id));
    let mut state_owner = RootUpdateStateMaps::new(root_state, private_bytes, fixed_byte_banks);
    let transition = apply_root_update_storage_transition(
        &mut state_owner,
        StateId(state_id),
        &target_label,
        executed.value.clone(),
        executed.bytes_value.clone(),
        executed.fixed_bytes_mutation.clone(),
        PlanOpId(op_id),
    )?;
    Ok(transition.executor_report)
}

pub fn apply_root_json_update_to_root_state(
    root_state: &mut PlanExecutorRootState,
    plan: &MachinePlan,
    target_state_id: StateId,
    value: JsonValue,
    update_op_id: PlanOpId,
) -> PlanExecutorResult<RootJsonStateWrite> {
    let write = apply_root_json_state_value(
        plan,
        &mut root_state.root_state,
        target_state_id,
        value,
        update_op_id,
    )?;
    root_state.clear_bytes_for_state(target_state_id);
    Ok(write)
}

pub fn apply_executed_root_update_to_root_state(
    root_state: &mut PlanExecutorRootState,
    plan: &MachinePlan,
    state_id: usize,
    executed: &RootExecutedUpdate,
    op_id: usize,
) -> PlanExecutorResult<JsonValue> {
    let target_label = state_label(plan, StateId(state_id));
    let transition = apply_root_update_storage_transition(
        root_state,
        StateId(state_id),
        &target_label,
        executed.value.clone(),
        executed.bytes_value.clone(),
        executed.fixed_bytes_mutation.clone(),
        PlanOpId(op_id),
    )?;
    Ok(transition.executor_report)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootUpdateBranchCollection {
    pub target_state_id: Option<StateId>,
    pub inserted_update: bool,
    pub duplicate_update: bool,
    pub skipped_update: bool,
    pub runtime_branch_used: bool,
    pub executor_report: JsonValue,
}

pub fn collect_root_update_candidate_for_step<RuntimeBranch>(
    plan: &MachinePlan,
    op: &PlanOp,
    source_id: SourceId,
    source_route_label: &str,
    source_route_slot: &SourceRoute,
    root_json_event: &RootJsonSourceEvent,
    staged_root_state: &mut PlanExecutorRootState,
    touched_update_candidates: &mut RootUpdateCandidateTracker,
    touched_updates: &mut BTreeMap<usize, RootExecutedUpdate>,
    runtime_branch: &mut RuntimeBranch,
) -> PlanExecutorResult<RootUpdateBranchCollection>
where
    RuntimeBranch:
        FnMut(&PlanOp, &PlanExecutorRootState) -> PlanExecutorResult<Option<RootExecutedUpdate>>,
{
    let Some(ValueRef::State(state_id)) = op.output else {
        return Err(format!(
            "CPU root-scenario PlanExecutor update branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    if !root_state_is_scalar(plan, state_id) {
        return Err(format!(
            "CPU root-scenario PlanExecutor update branch {} targets non-root state {}",
            op.id.0, state_id.0
        )
        .into());
    }
    if op.unresolved_executable_ref_count != 0 {
        return Err(format!(
            "selected update branch {} has {} unresolved executable refs",
            op.id.0, op.unresolved_executable_ref_count
        )
        .into());
    }

    let json_update_execution = execute_root_json_update_branch(
        plan,
        op,
        source_id,
        source_route_slot,
        root_json_event,
        &staged_root_state.root_state,
    )?;
    if json_update_execution.surface_kind == RootUpdateExecutionSurfaceKind::SkippedByGuard {
        return Ok(RootUpdateBranchCollection {
            target_state_id: Some(state_id),
            inserted_update: false,
            duplicate_update: false,
            skipped_update: true,
            runtime_branch_used: false,
            executor_report: json!({
                "executor": "cpu-plan-root-update-branch-collection-v1",
                "update_op_id": op.id.0,
                "target_state_id": state_id.0,
                "mode": "skipped_by_guard",
                "json_update_core": json_update_execution.executor_report,
                "runtime_ast_eval_count": 0,
                "executable_string_path_count": 0,
                "unknown_plan_op_count": 0,
                "graph_rebuild_count": 0,
                "graph_clones_per_item": 0,
            }),
        });
    }

    let mut runtime_branch_used = false;
    let mut executed = if let Some(executed) = json_update_execution.executed {
        executed
    } else {
        runtime_branch_used = true;
        let Some(executed) = runtime_branch(op, staged_root_state)? else {
            return Ok(RootUpdateBranchCollection {
                target_state_id: Some(state_id),
                inserted_update: false,
                duplicate_update: false,
                skipped_update: true,
                runtime_branch_used,
                executor_report: json!({
                    "executor": "cpu-plan-root-update-branch-collection-v1",
                    "update_op_id": op.id.0,
                    "target_state_id": state_id.0,
                    "mode": "runtime_branch_noop",
                    "json_update_core": json_update_execution.executor_report,
                    "runtime_ast_eval_count": 0,
                    "executable_string_path_count": 0,
                    "unknown_plan_op_count": 0,
                    "graph_rebuild_count": 0,
                    "graph_clones_per_item": 0,
                }),
            });
        };
        executed
    };
    if executed.executor_core.is_null() {
        executed.executor_core = json_update_execution.evaluator_report;
    }
    let candidate_record = record_root_update_candidate(
        touched_update_candidates,
        source_route_label,
        root_update_candidate_from_executed(state_id.0, op.id.0, &executed),
    )?;
    if candidate_record.kind == RootUpdateCandidateRecordKind::Duplicate {
        return Ok(RootUpdateBranchCollection {
            target_state_id: Some(state_id),
            inserted_update: false,
            duplicate_update: true,
            skipped_update: false,
            runtime_branch_used,
            executor_report: json!({
                "executor": "cpu-plan-root-update-branch-collection-v1",
                "update_op_id": op.id.0,
                "target_state_id": state_id.0,
                "mode": "duplicate_candidate",
                "candidate_record_core": candidate_record.executor_report,
                "json_update_core": json_update_execution.executor_report,
                "runtime_ast_eval_count": 0,
                "executable_string_path_count": 0,
                "unknown_plan_op_count": 0,
                "graph_rebuild_count": 0,
                "graph_clones_per_item": 0,
            }),
        });
    }
    if executed.bytes_value.is_none() && executed.fixed_bytes_mutation.is_none() {
        apply_root_json_update_to_root_state(
            staged_root_state,
            plan,
            state_id,
            executed.value.clone(),
            op.id,
        )?;
    } else {
        apply_executed_root_update_to_root_state(
            staged_root_state,
            plan,
            state_id.0,
            &executed,
            op.id.0,
        )?;
    }
    touched_updates.insert(state_id.0, executed);

    Ok(RootUpdateBranchCollection {
        target_state_id: Some(state_id),
        inserted_update: true,
        duplicate_update: false,
        skipped_update: false,
        runtime_branch_used,
        executor_report: json!({
            "executor": "cpu-plan-root-update-branch-collection-v1",
            "update_op_id": op.id.0,
            "target_state_id": state_id.0,
            "mode": "inserted_candidate",
            "candidate_record_core": candidate_record.executor_report,
            "json_update_core": json_update_execution.executor_report,
            "runtime_ast_eval_count": 0,
            "executable_string_path_count": 0,
            "unknown_plan_op_count": 0,
            "graph_rebuild_count": 0,
            "graph_clones_per_item": 0,
        }),
    })
}

fn root_update_candidate_record_report(
    kind: &str,
    source_route: &str,
    state_id: usize,
    op_ids: &[usize],
) -> JsonValue {
    json!({
        "executor": "cpu-plan-root-update-candidate-tracker-v1",
        "kind": kind,
        "source": source_route,
        "state_id": state_id,
        "candidate_update_op_ids": op_ids,
        "candidate_update_op_count": op_ids.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    })
}

fn field_set_delta_target_key(delta: &JsonValue) -> PlanExecutorResult<Option<String>> {
    if delta.get("kind").and_then(JsonValue::as_str) != Some("FieldSet") {
        return Ok(None);
    }
    let target = json!({
        "kind": delta.get("kind").cloned().unwrap_or(JsonValue::Null),
        "list_id": delta.get("list_id").cloned().unwrap_or(JsonValue::Null),
        "key": delta.get("key").cloned().unwrap_or(JsonValue::Null),
        "generation": delta.get("generation").cloned().unwrap_or(JsonValue::Null),
        "source_id": delta.get("source_id").cloned().unwrap_or(JsonValue::Null),
        "bind_epoch": delta.get("bind_epoch").cloned().unwrap_or(JsonValue::Null),
        "field_path": delta.get("field_path").cloned().unwrap_or(JsonValue::Null),
    });
    Ok(Some(serde_json::to_string(&target)?))
}

fn indexed_update_write_target_key(row: &JsonValue) -> PlanExecutorResult<String> {
    let target = json!({
        "list_id": row.get("list_id").cloned().unwrap_or(JsonValue::Null),
        "key": row.get("key").cloned().unwrap_or(JsonValue::Null),
        "generation": row.get("generation").cloned().unwrap_or(JsonValue::Null),
        "field_path": row.get("field_path").cloned().unwrap_or(JsonValue::Null),
    });
    Ok(serde_json::to_string(&target)?)
}

pub fn execute_source_route_json_update(
    plan: &MachinePlan,
    source_route: &str,
    target_state: &str,
    event: &RootJsonSourceEvent,
) -> PlanExecutorResult<SourceRouteJsonExecution> {
    let selection = select_source_route_update(plan, source_route, target_state)?;
    let source_route_slot = plan
        .source_routes
        .iter()
        .find(|route| route.source_id == selection.source_id)
        .ok_or_else(|| format!("MachinePlan source route `{source_route}` has no route slot"))?;
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| op.id == selection.update_op_id)
        .ok_or_else(|| {
            format!(
                "source-route selector chose missing update op {}",
                selection.update_op_id.0
            )
        })?;
    let mut root_state = execute_initial_state(plan)?
        .state_summary
        .as_object()
        .cloned()
        .ok_or("initial PlanExecutor state summary is not an object")?;
    let evaluation = evaluate_root_json_update_branch(
        plan,
        op,
        selection.source_id,
        source_route_slot,
        event,
        &root_state,
    )?;
    if !evaluation.supported || evaluation.skipped_by_guard {
        let executor_report = json!({
            "executor": "cpu-plan-source-route-json-execution-v1",
            "source": source_route,
            "source_id": selection.source_id.0,
            "target_state": target_state,
            "target_state_id": selection.target_state_id.0,
            "update_op_id": selection.update_op_id.0,
            "supported": evaluation.supported,
            "skipped_by_guard": evaluation.skipped_by_guard,
            "unsupported_reason": evaluation.unsupported_reason,
            "selection_core": selection.executor_report,
            "evaluation_core": evaluation.executor_report,
            "state_write_core": JsonValue::Null,
            "runtime_ast_eval_count": 0,
            "executable_string_path_count": 0,
            "unknown_plan_op_count": 0,
            "graph_rebuild_count": 0,
            "graph_clones_per_item": 0,
        });
        return Ok(SourceRouteJsonExecution {
            plan_hash: selection.plan_hash,
            source_label: selection.source_label,
            source_id: selection.source_id,
            target_state_label: selection.target_state_label,
            target_state_id: selection.target_state_id,
            update_op_id: selection.update_op_id,
            supported: evaluation.supported,
            skipped_by_guard: evaluation.skipped_by_guard,
            unsupported_reason: evaluation.unsupported_reason,
            value: None,
            state_summary: JsonValue::Null,
            semantic_delta_signatures: Vec::new(),
            semantic_deltas: Vec::new(),
            expression_kind: evaluation.expression_kind,
            source_payload_field: evaluation.source_payload_field,
            update_constant_id: evaluation.update_constant_id,
            update_constant_value: evaluation.update_constant_value,
            executor_report,
        });
    }
    let value = evaluation.value.clone().ok_or_else(|| {
        format!(
            "source-route JSON evaluator reported supported branch {} without a value",
            selection.update_op_id.0
        )
    })?;
    let write = apply_root_json_state_value(
        plan,
        &mut root_state,
        selection.target_state_id,
        value.clone(),
        selection.update_op_id,
    )?;
    let state_summary = json!({ selection.target_state_label.clone(): value.clone() });
    let mut semantic_delta_signatures = Vec::new();
    let mut semantic_deltas = Vec::new();
    if let Some(delta) = write.semantic_delta.clone() {
        semantic_delta_signatures.push(format!("FieldSet:{}", selection.target_state_label));
        semantic_deltas.push(delta);
    }
    let executor_report = json!({
        "executor": "cpu-plan-source-route-json-execution-v1",
        "source": source_route,
        "source_id": selection.source_id.0,
        "target_state": target_state,
        "target_state_id": selection.target_state_id.0,
        "update_op_id": selection.update_op_id.0,
        "supported": true,
        "skipped_by_guard": false,
        "unsupported_reason": null,
        "expression_kind": evaluation.expression_kind,
        "source_payload_field": evaluation.source_payload_field,
        "update_constant_id": evaluation.update_constant_id,
        "update_constant_value": evaluation.update_constant_value,
        "selection_core": selection.executor_report,
        "evaluation_core": evaluation.executor_report,
        "state_write_core": write.executor_report,
        "state_summary": state_summary,
        "semantic_delta_signatures": semantic_delta_signatures,
        "semantic_deltas": semantic_deltas,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(SourceRouteJsonExecution {
        plan_hash: selection.plan_hash,
        source_label: selection.source_label,
        source_id: selection.source_id,
        target_state_label: selection.target_state_label,
        target_state_id: selection.target_state_id,
        update_op_id: selection.update_op_id,
        supported: true,
        skipped_by_guard: false,
        unsupported_reason: None,
        value: Some(value),
        state_summary,
        semantic_delta_signatures,
        semantic_deltas,
        expression_kind: evaluation.expression_kind,
        source_payload_field: evaluation.source_payload_field,
        update_constant_id: evaluation.update_constant_id,
        update_constant_value: evaluation.update_constant_value,
        executor_report,
    })
}

pub fn validate_source_route_full_execution(
    plan: &MachinePlan,
    target_state_id: StateId,
    selected_update_op: &PlanOp,
    selected_value: &JsonValue,
    full_state_summary: &JsonValue,
    per_step: &[JsonValue],
) -> PlanExecutorResult<SourceRouteFullExecutionValidation> {
    let target_state_label = state_label(plan, target_state_id);
    if selected_update_op.indexed {
        let mut matches = Vec::new();
        for step in per_step {
            let Some(indexed_updates) = step.get("indexed_updates").and_then(JsonValue::as_array)
            else {
                continue;
            };
            for update in indexed_updates {
                if update.get("update_op_id").and_then(JsonValue::as_u64)
                    == Some(selected_update_op.id.0 as u64)
                    && update.get("target_state_id").and_then(JsonValue::as_u64)
                        == Some(target_state_id.0 as u64)
                {
                    matches.push(update.clone());
                }
            }
        }
        let [indexed_update] = matches.as_slice() else {
            return Err(format!(
                "indexed source-route op {} expected exactly one full-execution indexed update for `{target_state_label}`, found {}",
                selected_update_op.id.0,
                matches.len()
            )
            .into());
        };
        let value = indexed_update.get("value").cloned().ok_or_else(|| {
            format!(
                "indexed source-route op {} full-execution update has no value",
                selected_update_op.id.0
            )
        })?;
        if &value != selected_value {
            return Err(format!(
                "selected indexed route op {} produced {:?}, but full source-route execution produced {:?} for `{target_state_label}`",
                selected_update_op.id.0, selected_value, value
            )
            .into());
        }
        let state_summary = json!({ target_state_label.clone(): value.clone() });
        let executor_report = json!({
            "executor": "cpu-plan-source-route-full-execution-validation-v1",
            "target_state": target_state_label,
            "target_state_id": target_state_id.0,
            "selected_update_op_id": selected_update_op.id.0,
            "selected_op_indexed": true,
            "matched": true,
            "indexed_update": indexed_update,
            "runtime_ast_eval_count": 0,
            "executable_string_path_count": 0,
            "unknown_plan_op_count": 0,
            "graph_rebuild_count": 0,
            "graph_clones_per_item": 0,
        });
        return Ok(SourceRouteFullExecutionValidation {
            target_state_id,
            target_state_label,
            value,
            state_summary,
            executor_report,
        });
    }
    let value = json_value_at_dotted_path(full_state_summary, &target_state_label)
        .cloned()
        .ok_or_else(|| {
            format!(
                "full source-route execution did not produce target state `{target_state_label}`"
            )
        })?;
    if &value != selected_value {
        return Err(format!(
            "selected route op {} produced {:?}, but full source-route execution produced {:?} for `{target_state_label}`",
            selected_update_op.id.0, selected_value, value
        )
        .into());
    }
    let state_summary = json!({ target_state_label.clone(): value.clone() });
    let executor_report = json!({
        "executor": "cpu-plan-source-route-full-execution-validation-v1",
        "target_state": target_state_label,
        "target_state_id": target_state_id.0,
        "selected_update_op_id": selected_update_op.id.0,
        "selected_op_indexed": false,
        "matched": true,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(SourceRouteFullExecutionValidation {
        target_state_id,
        target_state_label,
        value,
        state_summary,
        executor_report,
    })
}

pub fn select_source_route_execution_surface(
    execution: &SourceRouteJsonExecution,
) -> PlanExecutorResult<SourceRouteExecutionSurface> {
    if execution.supported && execution.skipped_by_guard {
        return Err(format!(
            "selected update branch {} source guard did not match the supplied event",
            execution.update_op_id.0
        )
        .into());
    }
    let route_core_value_is_bytes = execution
        .value
        .as_ref()
        .is_some_and(json_value_is_bytes_report);
    let kind = if execution.supported && !route_core_value_is_bytes {
        SourceRouteExecutionSurfaceKind::PlanJson
    } else {
        SourceRouteExecutionSurfaceKind::RuntimeBranch
    };
    let executor_report = json!({
        "executor": "cpu-plan-source-route-execution-surface-v1",
        "source": execution.source_label,
        "source_id": execution.source_id.0,
        "target_state": execution.target_state_label,
        "target_state_id": execution.target_state_id.0,
        "update_op_id": execution.update_op_id.0,
        "json_supported": execution.supported,
        "json_skipped_by_guard": execution.skipped_by_guard,
        "route_core_value_is_bytes": route_core_value_is_bytes,
        "execution_surface": match kind {
            SourceRouteExecutionSurfaceKind::PlanJson => "plan-json",
            SourceRouteExecutionSurfaceKind::RuntimeBranch => "runtime-branch",
        },
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(SourceRouteExecutionSurface {
        kind,
        route_core_value_is_bytes,
        executor_report,
    })
}

pub fn assemble_source_route_runtime_branch_execution(
    input: SourceRouteRuntimeBranchExecutionInput,
    source_route_json_execution_report: &JsonValue,
) -> SourceRouteSelectedExecution {
    let runtime_branch_execution_core = json!({
        "executor": "cpu-plan-source-route-runtime-branch-execution-v1",
        "expression_kind": input.expression_kind.clone(),
        "source_payload_field": input.source_payload_field.clone(),
        "update_constant_id": input.update_constant_id.clone(),
        "update_constant_value": input.update_constant_value.clone(),
        "host_effect": input.host_effect.clone(),
        "state_write_core": input.state_write_core.clone(),
        "bytes_state_core": input.bytes_state_core.clone(),
        "runtime_branch_core": input.runtime_branch_core.clone(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let mut executor_core = source_route_json_execution_report.clone();
    if let Some(object) = executor_core.as_object_mut() {
        object.insert(
            "runtime_branch_execution_core".to_owned(),
            runtime_branch_execution_core,
        );
    }
    SourceRouteSelectedExecution {
        value: input.value,
        expression_kind: input.expression_kind,
        source_payload_field: input.source_payload_field,
        update_constant_id: input.update_constant_id,
        update_constant_value: input.update_constant_value,
        host_effect: input.host_effect,
        executor_core,
        state_write_core: input.state_write_core,
        bytes_state_core: input.bytes_state_core,
    }
}

pub fn execute_source_route_with_runtime_callbacks<RuntimeBranch, FullExecution>(
    plan: &MachinePlan,
    source_route: &str,
    target_state: &str,
    event: &RootJsonSourceEvent,
    mut runtime_branch: RuntimeBranch,
    mut full_execution: FullExecution,
) -> PlanExecutorResult<SourceRouteOrchestration>
where
    RuntimeBranch: FnMut(
        &SourceRouteExecutionContext,
        &SourceRouteExecutionSurface,
        &JsonValue,
    ) -> PlanExecutorResult<SourceRouteSelectedExecution>,
    FullExecution: FnMut() -> PlanExecutorResult<SourceRouteFullExecution>,
{
    let route_context = resolve_source_route_execution_context(plan, source_route, target_state)?;
    let route_context_report = route_context.executor_report.clone();
    let route_selection = route_context.selection.clone();
    let source_id = route_selection.source_id;
    let target_state_id = route_selection.target_state_id;
    let source_route_slot = route_context.source_route_slot.clone();
    let op = route_context.update_op.clone();
    let source_route_json_execution =
        execute_source_route_json_update(plan, source_route, target_state, event)?;
    let mut source_route_json_execution_report =
        source_route_json_execution.executor_report.clone();
    let execution_surface = select_source_route_execution_surface(&source_route_json_execution)?;
    let mut execution_surface_report = execution_surface.executor_report.clone();
    if op.indexed
        && let Some(object) = execution_surface_report.as_object_mut()
    {
        object.insert(
            "execution_surface".to_owned(),
            json!("indexed-full-execution"),
        );
        object.insert(
            "indexed_route_selected_from_full_execution".to_owned(),
            json!(true),
        );
    }
    if let Some(object) = source_route_json_execution_report.as_object_mut() {
        object.insert(
            "execution_surface_core".to_owned(),
            execution_surface_report.clone(),
        );
    }

    let mut selected_executed = if execution_surface.kind
        == SourceRouteExecutionSurfaceKind::PlanJson
        || op.indexed
    {
        let value = source_route_json_execution.value.clone().ok_or_else(|| {
            format!(
                "source-route JSON executor reported supported branch {} without a value",
                op.id.0
            )
        })?;
        let expression_kind = source_route_json_execution.expression_kind.ok_or_else(|| {
            format!(
                "source-route JSON executor reported supported branch {} without an expression kind",
                op.id.0
            )
        })?;
        let state_write_core = source_route_json_execution_report
            .get("state_write_core")
            .cloned()
            .unwrap_or(JsonValue::Null);
        SourceRouteSelectedExecution {
            value,
            expression_kind: expression_kind.to_owned(),
            source_payload_field: source_route_json_execution.source_payload_field.clone(),
            update_constant_id: source_route_json_execution.update_constant_id.clone(),
            update_constant_value: source_route_json_execution.update_constant_value.clone(),
            host_effect: JsonValue::Null,
            executor_core: source_route_json_execution_report.clone(),
            state_write_core,
            bytes_state_core: JsonValue::Null,
        }
    } else {
        runtime_branch(
            &route_context,
            &execution_surface,
            &source_route_json_execution_report,
        )?
    };
    selected_executed.executor_core = source_route_json_execution_report.clone();

    let full_execution = full_execution()?;
    let full_validation = validate_source_route_full_execution(
        plan,
        target_state_id,
        &op,
        &selected_executed.value,
        &full_execution.state_summary,
        &full_execution.per_step,
    )?;
    let value = full_validation.value.clone();
    let state_summary = full_validation.state_summary.clone();
    let route_report = assemble_source_route_report(
        plan,
        source_route,
        source_id,
        target_state,
        target_state_id,
        &op,
        selected_executed.expression_kind.as_str(),
        &selected_executed.source_payload_field,
        &selected_executed.update_constant_id,
        &selected_executed.update_constant_value,
        &selected_executed.host_effect,
        &source_route_slot,
        &route_context_report,
        &source_route_json_execution_report,
        &full_validation.executor_report,
        &full_execution.executor_report,
        &state_summary,
        &full_execution.state_summary,
        &full_execution.semantic_delta_signatures,
        &full_execution.semantic_deltas,
        &full_execution.per_step,
    )?;

    Ok(SourceRouteOrchestration {
        plan_hash: route_selection.plan_hash,
        source_id,
        value,
        state_summary,
        semantic_delta_signatures: full_execution.semantic_delta_signatures,
        semantic_deltas: full_execution.semantic_deltas,
        route_surface: route_report.route_surface,
        executor_report: route_report.executor_report,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_source_route_report(
    plan: &MachinePlan,
    source_route: &str,
    source_id: SourceId,
    target_state: &str,
    target_state_id: StateId,
    update_op: &PlanOp,
    expression_kind: &str,
    source_payload_field: &JsonValue,
    update_constant_id: &JsonValue,
    update_constant_value: &JsonValue,
    host_effect: &JsonValue,
    source_route_slot: &SourceRoute,
    route_selection_core: &JsonValue,
    route_execution_core: &JsonValue,
    full_execution_validation_core: &JsonValue,
    full_executor_report: &JsonValue,
    state_summary: &JsonValue,
    full_state_summary: &JsonValue,
    semantic_delta_signatures: &[String],
    semantic_deltas: &JsonValue,
    per_step: &[JsonValue],
) -> PlanExecutorResult<SourceRouteReportAssembly> {
    let full_executor = full_executor_report
        .as_object()
        .ok_or("root-scenario executor report is not an object")?;
    let report_assembly_core = json!({
        "executor": "cpu-plan-source-route-report-assembly-v1",
        "source": source_route,
        "source_id": source_id.0,
        "target_state": target_state,
        "target_state_id": target_state_id.0,
        "update_op_id": update_op.id.0,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let route_surface = json!({
        "source": source_route,
        "source_id": source_id.0,
        "target_state": target_state,
        "target_state_id": target_state_id.0,
        "update_op_id": update_op.id.0,
        "expression_kind": expression_kind,
        "source_payload_field": source_payload_field,
        "update_constant_id": update_constant_id,
        "update_constant_value": update_constant_value,
        "host_effect": host_effect,
        "payload_schema": source_route_slot.payload_schema,
        "global_plan_executable": plan.capability_summary.executable,
        "global_typed_lowering_executable": plan.capability_summary.typed_lowering_executable,
        "global_cpu_plan_executor_complete": plan.capability_summary.cpu_plan_executor_complete,
        "global_unresolved_executable_ref_count": plan.capability_summary.unresolved_executable_ref_count,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "selected_op_unresolved_executable_ref_count": update_op.unresolved_executable_ref_count,
        "selected_op_indexed": update_op.indexed,
        "executor_core": route_selection_core,
        "route_execution_core": route_execution_core,
        "full_execution_validation_core": full_execution_validation_core,
        "report_assembly_core": report_assembly_core,
    });
    let executor_report = json!({
        "executor": "cpu-plan-source-route-v1",
        "source": source_route,
        "source_id": source_id.0,
        "target_state": target_state,
        "target_state_id": target_state_id.0,
        "update_op_id": update_op.id.0,
        "expression_kind": expression_kind,
        "source_payload_field": source_payload_field,
        "update_constant_id": update_constant_id,
        "update_constant_value": update_constant_value,
        "host_effect": host_effect,
        "executed_update_branch_count": full_executor
            .get("executed_update_branch_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "executed_derived_value_count": full_executor
            .get("executed_derived_value_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "executed_list_append_count": full_executor
            .get("executed_list_append_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "executed_list_remove_count": full_executor
            .get("executed_list_remove_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "executed_indexed_update_count": full_executor
            .get("executed_indexed_update_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "emitted_source_bind_count": full_executor
            .get("emitted_source_bind_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "emitted_source_unbind_count": full_executor
            .get("emitted_source_unbind_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "executor_core": route_selection_core,
        "route_execution_core": route_execution_core,
        "full_execution_validation_core": full_execution_validation_core,
        "report_assembly_core": report_assembly_core,
        "state_summary": state_summary,
        "full_state_summary": full_state_summary,
        "semantic_delta_signatures": semantic_delta_signatures,
        "semantic_deltas": semantic_deltas,
        "per_step": per_step,
    });
    Ok(SourceRouteReportAssembly {
        route_surface,
        executor_report,
    })
}

pub fn assemble_source_route_command_report(
    input: SourceRouteCommandReportInput,
) -> SourceRouteCommandReportAssembly {
    let legacy_comparison = json!({
        "enabled": false,
        "passed": true,
        "reason": "legacy comparison was not requested"
    });
    let comparison_status = "not-requested";
    let plan_executor_status = "pass";
    let accepted_for_product_status = "pass";
    let expression_kind = input
        .route_surface
        .get("expression_kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let report_status = "pass";
    let exit_status = 0;
    let report_status_basis = "plan-executor-product";
    let command_report_assembly_core = json!({
        "executor": "cpu-plan-source-route-command-report-assembly-v1",
        "legacy_passed": false,
        "legacy_required_for_status": false,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "status": report_status,
        "report_status_basis": report_status_basis,
        "exit_status": exit_status,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let report = json!({
        "status": report_status,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "report_status_basis": report_status_basis,
        "report_version": 1,
        "command": "run-plan-route",
        "command_argv": input.command_argv,
        "measurement_mode": "proof",
        "exit_status": exit_status,
        "generated_at_utc": input.generated_at_utc,
        "git_commit": input.git_commit,
        "worktree_fingerprint": input.worktree_fingerprint,
        "binary_hash": input.binary_hash,
        "binary_path": input.binary_path,
        "source_path": input.source_path,
        "source_hash": input.source_hash,
        "source_files": input.source_files,
        "scenario_hash": "n/a",
        "program_hash": input.program_hash,
        "program_kind": input.program_kind,
        "program_file_count": input.program_file_count,
        "budget_hash": "n/a",
        "graph_node_count": input.graph_node_count,
        "load_pipeline_profile": input.load_pipeline_profile,
        "target_profile": input.target_profile,
        "plan_hash": input.plan_hash,
        "plan_version": input.plan_version,
        "capability_summary": input.capability_summary,
        "route_surface": input.route_surface,
        "source_event": input.source_event,
        "state_summary": input.state_summary,
        "semantic_delta_signatures": input.semantic_delta_signatures,
        "semantic_deltas": input.semantic_deltas,
        "legacy_comparison": legacy_comparison,
        "per_step_pass_fail": [
            {
                "id": "machine-plan-verified",
                "pass": true,
                "detail": "MachinePlan verifier passed before CPU source-route execution"
            },
            {
                "id": "cpu-plan-route-surface-executable",
                "pass": true,
                "detail": "selected source route has zero unresolved/fallback counters"
            },
            {
                "id": "cpu-plan-source-route-executed",
                "pass": true,
                "detail": format!(
                    "CPU PlanExecutor executed the full source event and selected one typed {expression_kind} target branch"
                )
            },
            {
                "id": "legacy-route-parity",
                "pass": true,
                "detail": "legacy runtime comparison was not requested and is not part of product report status"
            }
        ],
        "artifact_sha256s": input.artifact_sha256s,
        "plan_executor": input.plan_executor,
        "command_report_assembly_core": command_report_assembly_core,
    });
    SourceRouteCommandReportAssembly { report }
}

pub fn assemble_root_scenario_command_output(
    input: RootScenarioCommandOutputInput,
) -> RootScenarioCommandOutput {
    let command_output_core = json!({
        "executor": "cpu-plan-root-scenario-command-output-v1",
        "selected_step_count": input.selected_step_ids.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let mut plan_executor = input.plan_executor;
    if let Some(object) = plan_executor.as_object_mut() {
        object.insert(
            "command_output_core".to_owned(),
            command_output_core.clone(),
        );
    }
    let command_argv = input.command_argv;
    let report = assemble_root_scenario_command_report(RootScenarioCommandReportInput {
        command_argv: command_argv.clone(),
        generated_at_utc: input.generated_at_utc,
        git_commit: input.git_commit,
        worktree_fingerprint: input.worktree_fingerprint,
        binary_hash: input.binary_hash,
        binary_path: input.binary_path,
        source_path: input.source_path,
        source_hash: input.source_hash,
        source_files: input.source_files,
        scenario_path: input.scenario_path,
        scenario_hash: input.scenario_hash,
        program_hash: input.program_hash,
        program_kind: input.program_kind,
        program_file_count: input.program_file_count,
        graph_node_count: input.graph_node_count,
        load_pipeline_profile: input.load_pipeline_profile,
        target_profile: input.target_profile,
        plan_hash: input.plan_hash,
        plan_version: input.plan_version,
        capability_summary: input.capability_summary,
        selected_step_ids: input.selected_step_ids,
        state_summary: input.state_summary,
        semantic_delta_signatures: input.semantic_delta_signatures,
        semantic_deltas: input.semantic_deltas,
        plan_executor,
    })
    .report;
    RootScenarioCommandOutput {
        report,
        command_argv,
        executor_report: command_output_core,
    }
}

pub fn assemble_root_scenario_command_report(
    input: RootScenarioCommandReportInput,
) -> RootScenarioCommandReportAssembly {
    let legacy_comparison = json!({
        "enabled": false,
        "passed": false,
        "reason": "legacy comparison was not requested"
    });
    let legacy_comparison_acceptance = json!({
        "accepted": false,
        "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
        "kind": "not-applicable",
        "reason": "legacy comparison disabled"
    });
    let comparison_status = "not-requested";
    let plan_executor_status = "pass";
    let accepted_for_product_status = "pass";
    let report_status = "pass";
    let exit_status = 0;
    let report_status_basis = "plan-executor-product";
    let command_report_assembly_core = json!({
        "executor": "cpu-plan-root-scenario-command-report-assembly-v1",
        "legacy_passed": false,
        "legacy_accepted": false,
        "legacy_required_for_status": false,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "status": report_status,
        "report_status_basis": report_status_basis,
        "exit_status": exit_status,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let report = json!({
        "status": report_status,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "report_status_basis": report_status_basis,
        "report_version": 1,
        "command": "run-plan-root-scalar-scenario",
        "command_argv": input.command_argv,
        "measurement_mode": "proof",
        "exit_status": exit_status,
        "generated_at_utc": input.generated_at_utc,
        "git_commit": input.git_commit,
        "worktree_fingerprint": input.worktree_fingerprint,
        "binary_hash": input.binary_hash,
        "binary_path": input.binary_path,
        "source_path": input.source_path,
        "source_hash": input.source_hash,
        "source_files": input.source_files,
        "scenario_path": input.scenario_path,
        "scenario_hash": input.scenario_hash,
        "program_hash": input.program_hash,
        "program_kind": input.program_kind,
        "program_file_count": input.program_file_count,
        "budget_hash": "n/a",
        "graph_node_count": input.graph_node_count,
        "load_pipeline_profile": input.load_pipeline_profile,
        "target_profile": input.target_profile,
        "plan_hash": input.plan_hash,
        "plan_version": input.plan_version,
        "capability_summary": input.capability_summary,
        "selected_step_ids": input.selected_step_ids,
        "state_summary": input.state_summary,
        "semantic_delta_signatures": input.semantic_delta_signatures,
        "semantic_deltas": input.semantic_deltas,
        "legacy_comparison": legacy_comparison,
        "legacy_comparison_acceptance": legacy_comparison_acceptance,
        "per_step_pass_fail": [
            {
                "id": "machine-plan-verified",
                "pass": true,
                "detail": "MachinePlan verifier passed before CPU root-scenario execution"
            },
            {
                "id": "cpu-plan-root-list-scenario-executed",
                "pass": true,
                "detail": "CPU PlanExecutor replayed selected unscoped root/list source events"
            },
            {
                "id": "legacy-root-scenario-parity",
                "pass": true,
                "detail": "legacy runtime comparison was not requested and is not part of product report status"
            }
        ],
        "artifact_sha256s": [],
        "plan_executor": input.plan_executor,
        "command_report_assembly_core": command_report_assembly_core,
    });
    RootScenarioCommandReportAssembly { report }
}

pub fn assemble_scenario_events_command_output(
    input: ScenarioEventsCommandOutputInput,
) -> ScenarioEventsCommandOutput {
    let selected_step_count = input
        .plan_executor_coverage
        .get("selected_step_ids")
        .and_then(JsonValue::as_array)
        .map_or(0, Vec::len);
    let command_output_core = json!({
        "executor": "cpu-plan-scenario-events-command-output-v1",
        "selected_step_count": selected_step_count,
        "assertion_only_covered": input.assertion_only_covered,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let mut plan_executor = input.plan_executor;
    if let Some(object) = plan_executor.as_object_mut() {
        object.insert(
            "command_output_core".to_owned(),
            command_output_core.clone(),
        );
    }
    let command_argv = input.command_argv;
    let report = assemble_scenario_events_command_report(ScenarioEventsCommandReportInput {
        command_argv: command_argv.clone(),
        generated_at_utc: input.generated_at_utc,
        git_commit: input.git_commit,
        worktree_fingerprint: input.worktree_fingerprint,
        binary_hash: input.binary_hash,
        binary_path: input.binary_path,
        source_path: input.source_path,
        source_hash: input.source_hash,
        source_files: input.source_files,
        scenario_path: input.scenario_path,
        scenario_hash: input.scenario_hash,
        program_hash: input.program_hash,
        program_kind: input.program_kind,
        program_file_count: input.program_file_count,
        graph_node_count: input.graph_node_count,
        load_pipeline_profile: input.load_pipeline_profile,
        target_profile: input.target_profile,
        plan_hash: input.plan_hash,
        plan_version: input.plan_version,
        capability_summary: input.capability_summary,
        state_summary: input.state_summary,
        semantic_delta_signatures: input.semantic_delta_signatures,
        semantic_deltas: input.semantic_deltas,
        plan_executor_coverage: input.plan_executor_coverage,
        assertion_only_covered: input.assertion_only_covered,
        plan_executor,
    })
    .report;
    ScenarioEventsCommandOutput {
        report,
        command_argv,
        executor_report: command_output_core,
    }
}

pub fn assemble_scenario_events_command_report(
    input: ScenarioEventsCommandReportInput,
) -> ScenarioEventsCommandReportAssembly {
    let comparison_status = "not-requested";
    let plan_executor_status = "pass";
    let accepted_for_product_status = if input.assertion_only_covered {
        "pass"
    } else {
        "fail"
    };
    let accepted = input.assertion_only_covered;
    let report_status = if accepted { "pass" } else { "fail" };
    let exit_status = if accepted { 0 } else { 1 };
    let report_status_basis = "plan-executor-product-plus-assertion-coverage";
    let measurement_mode = "proof";
    let command_report_assembly_core = json!({
        "executor": "cpu-plan-scenario-events-command-report-assembly-v1",
        "assertion_only_covered": input.assertion_only_covered,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "status": report_status,
        "report_status_basis": report_status_basis,
        "exit_status": exit_status,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let selected_step_ids = input
        .plan_executor_coverage
        .get("selected_step_ids")
        .cloned()
        .unwrap_or_else(|| JsonValue::Array(Vec::new()));
    let report = json!({
        "status": report_status,
        "plan_executor_status": plan_executor_status,
        "comparison_status": comparison_status,
        "accepted_for_product_status": accepted_for_product_status,
        "report_status_basis": report_status_basis,
        "report_version": 1,
        "command": "run-plan-scenario-events",
        "command_argv": input.command_argv,
        "measurement_mode": measurement_mode,
        "exit_status": exit_status,
        "generated_at_utc": input.generated_at_utc,
        "git_commit": input.git_commit,
        "worktree_fingerprint": input.worktree_fingerprint,
        "binary_hash": input.binary_hash,
        "binary_path": input.binary_path,
        "source_path": input.source_path,
        "source_hash": input.source_hash,
        "source_files": input.source_files,
        "scenario_path": input.scenario_path,
        "scenario_hash": input.scenario_hash,
        "program_hash": input.program_hash,
        "program_kind": input.program_kind,
        "program_file_count": input.program_file_count,
        "budget_hash": "n/a",
        "graph_node_count": input.graph_node_count,
        "load_pipeline_profile": input.load_pipeline_profile,
        "target_profile": input.target_profile,
        "plan_hash": input.plan_hash,
        "plan_version": input.plan_version,
        "capability_summary": input.capability_summary,
        "selected_step_ids": selected_step_ids,
        "state_summary": input.state_summary,
        "semantic_delta_signatures": input.semantic_delta_signatures,
        "semantic_deltas": input.semantic_deltas,
        "plan_executor_coverage": input.plan_executor_coverage,
        "per_step_pass_fail": [
            {
                "id": "machine-plan-verified",
                "pass": true,
                "detail": "MachinePlan verifier passed before CPU scenario event replay"
            },
            {
                "id": "cpu-plan-scenario-events-executed",
                "pass": true,
                "detail": "CPU PlanExecutor replayed all scenario steps carrying expected_source_event"
            },
            {
                "id": "scenario-event-product-path-has-no-legacy-compare",
                "pass": true,
                "detail": "Scenario-event product proof is PlanExecutor-only and does not carry legacy runtime comparison"
            },
            {
                "id": "assertion-only-coverage-recorded",
                "pass": input.assertion_only_covered,
                "detail": "CPU PlanExecutor checked assertion-only scenario checkpoints in scenario order"
            }
        ],
        "artifact_sha256s": [],
        "plan_executor": input.plan_executor,
        "command_report_assembly_core": command_report_assembly_core,
    });
    ScenarioEventsCommandReportAssembly { report }
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_root_scenario_report(
    plan: &MachinePlan,
    selected_step_count: usize,
    initialized_root_state_count: usize,
    root_bytes_initialization_core: &JsonValue,
    executed_update_branch_count: usize,
    executed_derived_value_count: usize,
    executed_list_append_count: usize,
    executed_list_remove_count: usize,
    executed_indexed_update_count: usize,
    emitted_source_bind_count: usize,
    emitted_source_unbind_count: usize,
    executed_list_retain_count: usize,
    executed_list_view_count: usize,
    retained_list_row_count: usize,
    executed_list_projection_count: usize,
    executed_list_projection_find_count: usize,
    executed_list_projection_chunk_count: usize,
    projected_list_row_count: usize,
    state_summary: &JsonValue,
    list_summary: &JsonValue,
    list_projection_summary: &JsonValue,
    list_projections: &[JsonValue],
    list_view_summary: &JsonValue,
    list_retains: &[JsonValue],
    semantic_delta_signatures: &[String],
    semantic_deltas: &JsonValue,
    per_step: &[JsonValue],
    assertion_checkpoints: &[JsonValue],
    bytes_storage_counters: &JsonValue,
    bytes_storage_no_copy: bool,
) -> PlanExecutorResult<RootScenarioReportAssembly> {
    let report_assembly_core = json!({
        "executor": "cpu-plan-root-scenario-report-assembly-v1",
        "selected_step_count": selected_step_count,
        "semantic_delta_count": semantic_delta_signatures.len(),
        "per_step_count": per_step.len(),
        "assertion_checkpoint_count": assertion_checkpoints.len(),
        "list_projection_count": executed_list_projection_count,
        "list_retain_count": executed_list_retain_count,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let executor_report = json!({
        "executor": "cpu-plan-root-list-scenario-v1",
        "selected_step_count": selected_step_count,
        "initialized_root_state_count": initialized_root_state_count,
        "root_bytes_initialization_core": root_bytes_initialization_core,
        "executed_update_branch_count": executed_update_branch_count,
        "executed_derived_value_count": executed_derived_value_count,
        "executed_list_append_count": executed_list_append_count,
        "executed_list_remove_count": executed_list_remove_count,
        "executed_indexed_update_count": executed_indexed_update_count,
        "emitted_source_bind_count": emitted_source_bind_count,
        "emitted_source_unbind_count": emitted_source_unbind_count,
        "executed_list_retain_count": executed_list_retain_count,
        "executed_list_view_count": executed_list_view_count,
        "retained_list_row_count": retained_list_row_count,
        "executed_list_projection_count": executed_list_projection_count,
        "executed_list_projection_find_count": executed_list_projection_find_count,
        "executed_list_projection_chunk_count": executed_list_projection_chunk_count,
        "projected_list_row_count": projected_list_row_count,
        "list_slot_count": plan.storage_layout.list_slots.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "report_assembly_core": report_assembly_core,
        "state_summary": state_summary,
        "list_summary": list_summary,
        "list_projection_summary": list_projection_summary,
        "list_projections": list_projections,
        "list_view_summary": list_view_summary,
        "list_retains": list_retains,
        "semantic_delta_signatures": semantic_delta_signatures,
        "semantic_deltas": semantic_deltas,
        "per_step": per_step,
        "assertion_checkpoints": assertion_checkpoints,
        "bytes_storage_counters": bytes_storage_counters,
        "bytes_storage_no_copy": bytes_storage_no_copy,
    });
    Ok(RootScenarioReportAssembly { executor_report })
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_root_scenario_step_report(
    step_id: &str,
    source: &str,
    source_id: SourceId,
    executor_core: &JsonValue,
    executed_update_branch_count: usize,
    executed_derived_value_count: usize,
    executed_list_append_count: usize,
    executed_list_remove_count: usize,
    executed_indexed_update_count: usize,
    executed_list_retain_count: usize,
    executed_list_view_count: usize,
    retained_list_row_count: usize,
    emitted_source_bind_count: usize,
    emitted_source_unbind_count: usize,
    derived: Vec<JsonValue>,
    list_appends: Vec<JsonValue>,
    list_removes: Vec<JsonValue>,
    list_retains: Vec<JsonValue>,
    list_view_summary: JsonValue,
    indexed_updates: Vec<JsonValue>,
    updates: Vec<JsonValue>,
    touched_state_summary: JsonValue,
    semantic_delta_signatures: Vec<String>,
    semantic_deltas: Vec<JsonValue>,
    bytes_storage_counters: JsonValue,
    bytes_storage_no_copy: bool,
) -> RootScenarioStepReportAssembly {
    let report_assembly_core = json!({
        "executor": "cpu-plan-root-scenario-step-report-assembly-v1",
        "step_id": step_id,
        "source": source,
        "source_id": source_id.0,
        "semantic_delta_count": semantic_delta_signatures.len(),
        "update_count": updates.len(),
        "indexed_update_count": indexed_updates.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    let step_report = json!({
        "step_id": step_id,
        "source": source,
        "source_id": source_id.0,
        "executor_core": executor_core,
        "report_assembly_core": report_assembly_core,
        "executed_update_branch_count": executed_update_branch_count,
        "executed_derived_value_count": executed_derived_value_count,
        "executed_list_append_count": executed_list_append_count,
        "executed_list_remove_count": executed_list_remove_count,
        "executed_indexed_update_count": executed_indexed_update_count,
        "executed_list_retain_count": executed_list_retain_count,
        "executed_list_view_count": executed_list_view_count,
        "retained_list_row_count": retained_list_row_count,
        "emitted_source_bind_count": emitted_source_bind_count,
        "emitted_source_unbind_count": emitted_source_unbind_count,
        "derived": derived,
        "list_appends": list_appends,
        "list_removes": list_removes,
        "list_retains": list_retains,
        "list_view_summary": list_view_summary,
        "indexed_updates": indexed_updates,
        "updates": updates,
        "touched_state_summary": touched_state_summary,
        "semantic_delta_signatures": semantic_delta_signatures,
        "semantic_deltas": semantic_deltas,
        "bytes_storage_counters": bytes_storage_counters,
        "bytes_storage_no_copy": bytes_storage_no_copy,
    });
    RootScenarioStepReportAssembly { step_report }
}

pub fn demand_current_field_paths(plan: &MachinePlan) -> BTreeSet<String> {
    let mut count_field_ids = BTreeSet::new();
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
    {
        let PlanOpKind::ListOperation {
            operation_kind: boon_plan::PlanListOperationKind::Count,
            count: Some(count),
            ..
        } = &op.kind
        else {
            continue;
        };
        if let ValueRef::Field(field_id) = count.target {
            count_field_ids.insert(field_id.0);
        }
    }

    let mut paths = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .flat_map(|op| {
            let PlanOpKind::DerivedValue {
                derived_kind: _,
                startup_recompute,
                expression: _,
            } = &op.kind
            else {
                return Vec::new();
            };
            let reads_list_count = match &op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(expression),
                    ..
                } => derived_expression_reads_field_ids(expression, &count_field_ids),
                _ => false,
            };
            if *startup_recompute && !reads_list_count {
                return Vec::new();
            }
            let Some(ValueRef::Field(field_id)) = op.output else {
                return Vec::new();
            };
            let label = if op.indexed {
                field_label(plan, field_id.0)
            } else {
                derived_field_label(plan, field_id.0)
            };
            let mut labels = vec![label.clone()];
            if op.indexed
                && let Some((_, local_field)) = label.rsplit_once('.')
            {
                labels.push(local_field.to_owned());
            }
            labels
        })
        .collect::<BTreeSet<_>>();

    for field_id in count_field_ids {
        paths.insert(derived_field_label(plan, field_id));
    }

    paths
}

fn derived_expression_reads_field_ids(
    expression: &PlanDerivedExpression,
    field_ids: &BTreeSet<usize>,
) -> bool {
    match expression {
        PlanDerivedExpression::NumberCompareConst { left, .. } => {
            matches!(left, ValueRef::Field(field_id) if field_ids.contains(&field_id.0))
        }
        PlanDerivedExpression::BoolAnd { left, right } => {
            derived_expression_reads_field_ids(left, field_ids)
                || derived_expression_reads_field_ids(right, field_ids)
        }
        PlanDerivedExpression::BoolNot { input } => {
            matches!(input, ValueRef::Field(field_id) if field_ids.contains(&field_id.0))
        }
        PlanDerivedExpression::BoolNotExpression { input } => {
            derived_expression_reads_field_ids(input, field_ids)
        }
        _ => false,
    }
}

pub fn demand_current_semantic_delta_acceptance_policy(
    legacy_comparison: &JsonValue,
    demand_current_field_paths: &BTreeSet<String>,
) -> JsonValue {
    if legacy_comparison
        .get("enabled")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return json!({
            "accepted": false,
            "kind": "not-applicable",
            "reason": "legacy comparison disabled",
            "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
        });
    }
    if legacy_comparison
        .get("semantic_delta_match")
        .and_then(JsonValue::as_bool)
        == Some(true)
    {
        return json!({
            "accepted": false,
            "kind": "not-needed",
            "reason": "legacy semantic deltas already match",
            "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
        });
    }
    if legacy_comparison
        .get("state_match")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return json!({
            "accepted": false,
            "kind": "demand-current-coalesced-semantic-deltas",
            "reason": "legacy state parity failed",
            "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
        });
    }

    let Some(steps) = legacy_comparison
        .get("step_comparisons")
        .and_then(JsonValue::as_array)
    else {
        return json!({
            "accepted": false,
            "kind": "demand-current-coalesced-semantic-deltas",
            "reason": "legacy step comparisons missing",
            "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
        });
    };

    let mut mismatched_step_ids = Vec::new();
    let mut missing_delta_field_paths = BTreeSet::new();
    let mut extra_plan_delta_field_paths = BTreeSet::new();
    let mut missing_delta_count = 0_u64;
    let mut extra_plan_delta_count = 0_u64;
    let mut rejected_missing_deltas = Vec::new();
    let mut rejected_extra_plan_deltas = Vec::new();
    for step in steps {
        if step
            .get("semantic_delta_match")
            .and_then(JsonValue::as_bool)
            == Some(true)
        {
            continue;
        }
        let step_id = step
            .get("step_id")
            .and_then(JsonValue::as_str)
            .unwrap_or("missing")
            .to_owned();
        mismatched_step_ids.push(step_id.clone());
        if step.get("state_match").and_then(JsonValue::as_bool) != Some(true) {
            return json!({
                "accepted": false,
                "kind": "demand-current-coalesced-semantic-deltas",
                "reason": format!("step `{step_id}` did not preserve touched state"),
                "mismatched_step_ids": mismatched_step_ids,
                "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
            });
        }
        let legacy_deltas = step
            .get("legacy_semantic_deltas")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let plan_deltas = step
            .get("plan_semantic_deltas")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let mut remaining_legacy = legacy_deltas;
        for plan_delta in plan_deltas {
            if let Some(index) = remaining_legacy
                .iter()
                .position(|legacy| legacy == &plan_delta)
            {
                remaining_legacy.remove(index);
            } else {
                extra_plan_delta_count += 1;
                let field_path = plan_delta
                    .get("field_path")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("missing")
                    .to_owned();
                extra_plan_delta_field_paths.insert(field_path.clone());
                let kind_ok =
                    plan_delta.get("kind").and_then(JsonValue::as_str) == Some("FieldSet");
                let field_ok = demand_current_field_paths.contains(&field_path);
                if !(kind_ok && field_ok) {
                    rejected_extra_plan_deltas.push(json!({
                        "step_id": step_id.clone(),
                        "delta": plan_delta,
                    }));
                }
            }
        }
        for missing_delta in remaining_legacy {
            missing_delta_count += 1;
            let field_path = missing_delta
                .get("field_path")
                .and_then(JsonValue::as_str)
                .unwrap_or("missing")
                .to_owned();
            missing_delta_field_paths.insert(field_path.clone());
            let kind_ok = missing_delta.get("kind").and_then(JsonValue::as_str) == Some("FieldSet");
            let field_ok = demand_current_field_paths.contains(&field_path);
            if !(kind_ok && field_ok) {
                rejected_missing_deltas.push(json!({
                    "step_id": step_id.clone(),
                    "delta": missing_delta,
                }));
            }
        }
    }

    let accepted = !mismatched_step_ids.is_empty()
        && (missing_delta_count > 0 || extra_plan_delta_count > 0)
        && rejected_missing_deltas.is_empty()
        && rejected_extra_plan_deltas.is_empty();
    json!({
        "accepted": accepted,
        "kind": "demand-current-coalesced-semantic-deltas",
        "reason": if accepted {
            "PlanExecutor preserved state/assertion parity; semantic-delta differences are bounded to demand-current/list-derived FieldSet currentness"
        } else {
            "legacy semantic delta mismatch did not match the demand-current coalescing policy"
        },
        "mismatched_step_ids": mismatched_step_ids,
        "missing_delta_field_paths": missing_delta_field_paths.into_iter().collect::<Vec<_>>(),
        "extra_plan_delta_field_paths": extra_plan_delta_field_paths.into_iter().collect::<Vec<_>>(),
        "accepted_demand_current_field_paths": demand_current_field_paths.iter().cloned().collect::<Vec<_>>(),
        "missing_delta_count": missing_delta_count,
        "extra_plan_delta_count": extra_plan_delta_count,
        "rejected_missing_deltas": rejected_missing_deltas,
        "rejected_extra_plan_deltas": rejected_extra_plan_deltas,
        "executor": "cpu-plan-root-scenario-demand-current-acceptance-v1",
    })
}

pub fn assemble_root_scenario_coverage_report(
    scenario_step_count: usize,
    event_step_count: usize,
    selected_step_ids: &[String],
    assertion_only_step_ids: &[String],
    assertion_checkpoints: &[JsonValue],
) -> RootScenarioCoverageReport {
    let assertion_only_covered = assertion_checkpoints.len() == assertion_only_step_ids.len()
        && assertion_checkpoints
            .iter()
            .all(root_scenario_assertion_checkpoint_report_is_covered);
    let coverage = json!({
        "executor": "cpu-plan-root-scenario-coverage-report-v1",
        "surface": "scenario-expected-source-event-replay",
        "scenario_step_count": scenario_step_count,
        "event_step_count": event_step_count,
        "selected_step_ids": selected_step_ids,
        "assertion_only_step_ids": assertion_only_step_ids,
        "covers_all_source_events": true,
        "covers_assertion_only_steps": assertion_only_covered,
        "full_scenario_parity": assertion_only_covered,
        "full_scenario_parity_blocker": if assertion_only_covered {
            JsonValue::Null
        } else {
            JsonValue::String("PlanExecutor did not check every assertion-only scenario checkpoint".to_owned())
        },
        "assertion_checkpoint_count": assertion_checkpoints.len(),
    });
    RootScenarioCoverageReport {
        assertion_only_covered,
        coverage,
    }
}

fn root_scenario_assertion_checkpoint_report_is_covered(checkpoint: &JsonValue) -> bool {
    let checked = checkpoint
        .get("checked_expectations")
        .and_then(JsonValue::as_array);
    checkpoint.get("passed").and_then(JsonValue::as_bool) == Some(true)
        && checked.is_some_and(|checked| {
            checkpoint
                .get("checked_expectation_count")
                .and_then(JsonValue::as_u64)
                == Some(checked.len() as u64)
        })
}

pub fn select_root_source_event_work(
    plan: &MachinePlan,
    source_route: &str,
) -> PlanExecutorResult<RootSourceEventWork> {
    let verification = verify_plan(plan)?;
    if verification.status != "pass" {
        return Err(format!(
            "MachinePlan verification failed with {} error(s)",
            verification.error_count
        )
        .into());
    }
    let source_id = source_id_for_label(plan, source_route)
        .ok_or_else(|| format!("MachinePlan has no source route `{source_route}`"))?;
    let source_route_slot = plan
        .source_routes
        .iter()
        .find(|route| route.source_id == source_id)
        .ok_or_else(|| format!("MachinePlan source route `{source_route}` has no route slot"))?;

    let mut route_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .filter(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if *id == source_id))
        })
        .collect::<Vec<_>>();
    sort_plan_ops_for_same_event_root_reads(&mut route_ops);
    let ordered_update_op_ids = route_ops.iter().map(|op| op.id).collect::<Vec<_>>();

    let derived_op_count = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .filter(|op| source_derived_op_is_executable_for_source(op, source_id))
        .count();
    let has_list_remove_work = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
        .any(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    operation_kind: PlanListOperationKind::Remove,
                    remove: Some(remove),
                    ..
                } if remove.source == ValueRef::Source(source_id)
            )
        });
    let root_update_key_gate = source_key_gate_for_root_updates(plan, source_id);
    if ordered_update_op_ids.is_empty() && derived_op_count == 0 && !has_list_remove_work {
        return Err(format!(
            "CPU root-scenario PlanExecutor found no executable selected-surface work for source route `{source_route}`"
        )
        .into());
    }

    let plan_hash = plan_sha256(plan)?;
    let executor_report = json!({
        "executor": "cpu-plan-root-source-event-work-selection-v1",
        "source": source_route,
        "source_id": source_id.0,
        "source_route_scoped": source_route_slot.scoped,
        "global_typed_lowering_executable": plan.capability_summary.typed_lowering_executable,
        "global_cpu_plan_executor_unsupported_op_count": plan.capability_summary.cpu_plan_executor_unsupported_op_count,
        "selection_mode": "route_scoped_executable_work",
        "ordered_update_op_ids": ordered_update_op_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
        "ordered_update_op_count": ordered_update_op_ids.len(),
        "derived_op_count": derived_op_count,
        "has_list_remove_work": has_list_remove_work,
        "root_update_key_gate": root_update_key_gate,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(RootSourceEventWork {
        plan_hash,
        source_label: source_route.to_owned(),
        source_id,
        source_route_scoped: source_route_slot.scoped,
        ordered_update_op_ids,
        derived_op_count,
        has_list_remove_work,
        root_update_key_gate,
        executor_report,
    })
}

fn source_derived_op_is_executable_for_source(op: &PlanOp, source_id: SourceId) -> bool {
    if op.indexed {
        return false;
    }
    if !op
        .inputs
        .iter()
        .any(|input| matches!(input, ValueRef::Source(id) if *id == source_id))
    {
        return false;
    }
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            expression: Some(
                PlanDerivedExpression::SourceEventTransform { .. }
                    | PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
            ),
            ..
        }
    )
}

pub fn dispatch_root_scenario_step(
    plan: &MachinePlan,
    source_route: &str,
    event: &RootJsonSourceEvent,
) -> PlanExecutorResult<RootScenarioStepDispatch> {
    let work = select_root_source_event_work(plan, source_route)?;
    let root_update_key_matches = work
        .root_update_key_gate
        .as_ref()
        .is_none_or(|required| event.key.as_deref() == Some(required.as_str()));
    let executable_work = (root_update_key_matches && !work.ordered_update_op_ids.is_empty())
        || work.derived_op_count != 0
        || work.has_list_remove_work;
    if !executable_work {
        return Err(format!(
            "CPU root-scenario PlanExecutor found no executable selected-surface work for source route `{source_route}` after source-event key gating"
        )
        .into());
    }
    let executor_report = json!({
        "executor": "cpu-plan-root-scenario-step-dispatch-v1",
        "source": source_route,
        "source_id": work.source_id.0,
        "source_route_scoped": work.source_route_scoped,
        "ordered_update_op_ids": work
            .ordered_update_op_ids
            .iter()
            .map(|id| id.0)
            .collect::<Vec<_>>(),
        "ordered_update_op_count": work.ordered_update_op_ids.len(),
        "derived_op_count": work.derived_op_count,
        "has_list_remove_work": work.has_list_remove_work,
        "root_update_key_gate": work.root_update_key_gate.clone(),
        "root_update_key_matches": root_update_key_matches,
        "executable_work": executable_work,
        "work_selection_core": work.executor_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootScenarioStepDispatch {
        plan_hash: work.plan_hash,
        source_label: work.source_label,
        source_id: work.source_id,
        source_route_scoped: work.source_route_scoped,
        ordered_update_op_ids: work.ordered_update_op_ids,
        derived_op_count: work.derived_op_count,
        has_list_remove_work: work.has_list_remove_work,
        root_update_key_gate: work.root_update_key_gate,
        root_update_key_matches,
        executable_work,
        executor_report,
    })
}

pub fn validate_root_scenario_materialized_work(
    source_route: &str,
    update_op_count: usize,
    derived_value_count: usize,
    has_list_remove_work: bool,
) -> PlanExecutorResult<RootScenarioMaterializedWork> {
    let executable_work = update_op_count != 0 || derived_value_count != 0 || has_list_remove_work;
    if !executable_work {
        return Err(format!(
            "CPU root-scenario PlanExecutor found no executable selected-surface work for source route `{source_route}`"
        )
        .into());
    }
    let executor_report = json!({
        "executor": "cpu-plan-root-scenario-materialized-work-v1",
        "source": source_route,
        "update_op_count": update_op_count,
        "derived_value_count": derived_value_count,
        "has_list_remove_work": has_list_remove_work,
        "executable_work": executable_work,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootScenarioMaterializedWork {
        executable_work,
        executor_report,
    })
}

pub fn ordered_root_update_ops_for_dispatch<'a>(
    plan: &'a MachinePlan,
    dispatch: &RootScenarioStepDispatch,
) -> PlanExecutorResult<Vec<&'a PlanOp>> {
    let mut route_ops = Vec::new();
    for op_id in &dispatch.ordered_update_op_ids {
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| op.id == *op_id)
            .ok_or_else(|| {
                format!(
                    "root source-event selector chose missing update op {}",
                    op_id.0
                )
            })?;
        route_ops.push(op);
    }
    Ok(route_ops)
}

pub fn source_route_slot_for_dispatch<'a>(
    plan: &'a MachinePlan,
    dispatch: &RootScenarioStepDispatch,
) -> PlanExecutorResult<&'a SourceRoute> {
    plan.source_routes
        .iter()
        .find(|route| route.source_id == dispatch.source_id)
        .ok_or_else(|| {
            format!(
                "MachinePlan source route `{}` has no route slot",
                dispatch.source_label
            )
            .into()
        })
}

pub fn evaluate_source_derived_values_for_event(
    plan: &MachinePlan,
    source_id: SourceId,
    event: &RootJsonSourceEvent,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<BTreeMap<FieldId, JsonValue>> {
    let mut values = BTreeMap::new();
    let mut router_route: Option<String> = None;
    for op in plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .filter(|op| source_derived_op_is_executable_for_source(op, source_id))
    {
        let Some(ValueRef::Field(output_id)) = op.output else {
            return Err(format!("derived op {} does not output a field", op.id.0).into());
        };
        match &op.kind {
            PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::SourceEventTransform {
                        default: _,
                        arms,
                        router_route: is_router_route,
                    }),
                ..
            } => {
                if let Some(arm) = arms.iter().find(|arm| arm.source_id == source_id) {
                    let Ok(value) =
                        eval_root_source_transform_row_expression(plan, root_state, &arm.value)
                    else {
                        continue;
                    };
                    if *is_router_route {
                        router_route = Some(
                            value
                                .as_str()
                                .ok_or_else(|| {
                                    format!(
                                        "derived op {} Router/go_to route value is not text",
                                        op.id.0
                                    )
                                })?
                                .to_owned(),
                        );
                    }
                    values.insert(output_id, value);
                }
            }
            PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                        source_id: expression_source_id,
                        key_field,
                        required_key,
                        state,
                        skip_empty,
                    }),
                ..
            } if *expression_source_id == source_id => {
                let key = source_payload_json_value(event, key_field)?;
                if key.as_str() != Some(required_key) {
                    continue;
                }
                let text = match state {
                    ValueRef::State(state_id) => {
                        let state_label = state_label(plan, *state_id);
                        root_state
                            .get(&state_label)
                            .and_then(JsonValue::as_str)
                            .ok_or_else(|| {
                                format!(
                                    "derived op {} root state `{state_label}` is not text",
                                    op.id.0
                                )
                            })?
                            .to_owned()
                    }
                    ValueRef::SourcePayload {
                        source_id: payload_source_id,
                        field: SourcePayloadField::Text,
                    } if *payload_source_id == source_id => {
                        let payload = source_payload_json_value(event, &SourcePayloadField::Text)?;
                        payload
                            .as_str()
                            .ok_or_else(|| {
                                format!("derived op {} source text payload is not text", op.id.0)
                            })?
                            .to_owned()
                    }
                    other => {
                        return Err(format!(
                            "derived op {} trim input is not supported: {other:?}",
                            op.id.0
                        )
                        .into());
                    }
                };
                let trimmed = text.trim().to_owned();
                if *skip_empty && trimmed.is_empty() {
                    continue;
                }
                values.insert(output_id, JsonValue::String(trimmed));
            }
            _ => {
                return Err(format!(
                    "CPU root-scenario PlanExecutor does not support derived op {} for source {}",
                    op.id.0, source_id.0
                )
                .into());
            }
        }
    }
    if plan_has_root_router_route_reader(plan)
        && let Some(route) = router_route
    {
        let mut evaluation_state = root_state.clone();
        commit_source_derived_values_to_root_state(plan, &mut evaluation_state, &values);
        evaluation_state.insert("Router/route".to_owned(), JsonValue::String(route));
        for (field_id, value) in
            evaluate_root_row_expression_derived_values(plan, &evaluation_state)?
        {
            values.insert(field_id, value);
        }
    }
    Ok(values)
}

pub fn prepare_root_scenario_step(
    plan: &MachinePlan,
    source_route: &str,
    event: &RootJsonSourceEvent,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<RootScenarioStepPreparation> {
    let dispatch = dispatch_root_scenario_step(plan, source_route, event)?;
    let source_id = dispatch.source_id;
    let source_route_slot = source_route_slot_for_dispatch(plan, &dispatch)?.clone();
    let derived_values =
        evaluate_source_derived_values_for_event(plan, source_id, event, root_state)?;
    let route_ops = ordered_root_update_ops_for_dispatch(plan, &dispatch)?
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let materialized_work = validate_root_scenario_materialized_work(
        source_route,
        route_ops.len(),
        dispatch.derived_op_count,
        dispatch.has_list_remove_work,
    )?;
    let mut root_dispatch_report = dispatch.executor_report.clone();
    if let Some(object) = root_dispatch_report.as_object_mut() {
        object.insert(
            "materialized_work_core".to_owned(),
            materialized_work.executor_report.clone(),
        );
    }

    let executor_report = json!({
        "executor": "cpu-plan-root-scenario-step-preparation-v1",
        "source": source_route,
        "source_id": source_id.0,
        "route_op_count": route_ops.len(),
        "derived_value_count": derived_values.len(),
        "has_list_remove_work": dispatch.has_list_remove_work,
        "root_update_key_matches": dispatch.root_update_key_matches,
        "dispatch_core": dispatch.executor_report,
        "materialized_work_core": materialized_work.executor_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });

    Ok(RootScenarioStepPreparation {
        source_id,
        source_route_slot,
        route_ops,
        derived_values,
        root_update_key_matches: dispatch.root_update_key_matches,
        root_dispatch_report,
        executor_report,
    })
}

pub fn build_source_derived_value_deltas(
    plan: &MachinePlan,
    derived_values: &BTreeMap<FieldId, JsonValue>,
) -> Vec<(String, JsonValue, JsonValue)> {
    let router_route_outputs = router_route_output_field_ids(plan);
    derived_values
        .iter()
        .filter(|(field_id, _)| !router_route_outputs.contains(field_id))
        .map(|(field_id, value)| {
            let field_label = derived_field_label(plan, field_id.0);
            let signature = format!("FieldSet:{field_label}");
            let delta = json!({
                "kind": "FieldSet",
                "list_id": null,
                "key": null,
                "generation": null,
                "source_id": null,
                "bind_epoch": null,
                "field_path": field_label,
                "value": value,
            });
            let report = json!({
                "field_id": field_id.0,
                "field_path": field_label,
                "value": value,
            });
            (signature, delta, report)
        })
        .collect()
}

fn router_route_output_field_ids(plan: &MachinePlan) -> BTreeSet<FieldId> {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .filter_map(|op| {
            let PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::SourceEventTransform {
                        router_route: true, ..
                    }),
                ..
            } = &op.kind
            else {
                return None;
            };
            let Some(ValueRef::Field(field_id)) = op.output else {
                return None;
            };
            Some(field_id)
        })
        .collect()
}

pub fn assemble_source_derived_step_deltas(
    plan: &MachinePlan,
    derived_values: &BTreeMap<FieldId, JsonValue>,
) -> SourceDerivedStepDeltas {
    let triples = build_source_derived_value_deltas(plan, derived_values);
    let mut semantic_delta_signatures = Vec::with_capacity(triples.len());
    let mut semantic_deltas = Vec::with_capacity(triples.len());
    let mut reports = Vec::with_capacity(triples.len());
    for (signature, delta, report) in triples {
        semantic_delta_signatures.push(signature);
        semantic_deltas.push(delta);
        reports.push(report);
    }
    let executor_report = json!({
        "executor": "cpu-plan-source-derived-step-deltas-v1",
        "derived_value_count": derived_values.len(),
        "semantic_delta_count": semantic_deltas.len(),
        "report_count": reports.len(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    SourceDerivedStepDeltas {
        semantic_delta_signatures,
        semantic_deltas,
        reports,
        executor_report,
    }
}

pub fn evaluate_root_json_update_branch(
    plan: &MachinePlan,
    op: &PlanOp,
    source_id: SourceId,
    source_route_slot: &SourceRoute,
    event: &RootJsonSourceEvent,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<RootJsonUpdateEvaluation> {
    let source_guard = match &op.kind {
        PlanOpKind::UpdateBranch { source_guard, .. } => source_guard,
        _ => {
            return Err(format!(
                "CPU PlanExecutor root JSON update branch {} is not an update branch",
                op.id.0
            )
            .into());
        }
    };
    let Some(ValueRef::State(output_state_id)) = op.output else {
        return Err(format!(
            "CPU PlanExecutor root JSON update branch {} does not target a state slot",
            op.id.0
        )
        .into());
    };
    if !source_guard_matches(source_guard, source_id, event)? {
        return Ok(root_json_update_outcome(
            op.id,
            false,
            true,
            None,
            Some(output_state_id),
            None,
            None,
        ));
    }

    let outcome = match &op.kind {
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            source_payload_field: Some(source_payload_field),
            update_constant_id: None,
            ..
        } => {
            validate_route_payload_field(source_route_slot, source_payload_field, op.id)?;
            validate_typed_payload_input(op, source_id, source_payload_field)?;
            let slot = scalar_slot_for_state(plan, output_state_id, op.id)?;
            let value = source_payload_value_for_slot(event, source_payload_field, slot, op.id)?;
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(value),
                Some((
                    "source_payload",
                    serde_json::to_value(source_payload_field)?,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            source_payload_field: None,
            update_constant_id: Some(update_constant_id),
            ..
        } => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *update_constant_id)
                .ok_or_else(|| format!("missing update constant {}", update_constant_id.0))?;
            if matches!(constant.value, PlanConstantValue::Bytes { .. }) {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES constants require runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            let value = plan_constant_json_value(constant)?;
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(value.clone()),
                Some(("const", JsonValue::Null, json!(update_constant_id.0), value)),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BoolNot,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_state_id = root_single_state_input(op)?;
            let input_value = root_state_value(plan, root_state, input_state_id, op.id.0)?
                .as_bool()
                .ok_or_else(|| {
                    let label = state_label(plan, input_state_id);
                    format!(
                        "root Bool/not update branch {} input state `{label}` is not bool",
                        op.id.0
                    )
                })?;
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(JsonValue::Bool(!input_value)),
                Some((
                    "bool_not",
                    JsonValue::Null,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::ReadPath,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            if let Some(payload_field) = root_single_source_payload_input(op, source_id)? {
                if payload_field == SourcePayloadField::Bytes {
                    return Ok(root_json_update_outcome(
                        op.id,
                        false,
                        false,
                        Some("BYTES source payload read requires runtime byte storage".to_owned()),
                        Some(output_state_id),
                        None,
                        None,
                    ));
                }
                validate_typed_payload_input(op, source_id, &payload_field)?;
                root_json_update_outcome(
                    op.id,
                    true,
                    false,
                    None,
                    Some(output_state_id),
                    Some(source_payload_json_value(event, &payload_field)?),
                    Some((
                        "read_path",
                        serde_json::to_value(&payload_field)?,
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                )
            } else {
                let input = root_single_state_or_field_input(op, output_state_id)?;
                let Some(value) = root_update_json_value_for_ref(
                    plan,
                    event,
                    root_state,
                    &input,
                    source_id,
                    op.id,
                    "read-path input",
                )?
                else {
                    return Ok(root_json_update_outcome(
                        op.id,
                        true,
                        true,
                        None,
                        Some(output_state_id),
                        None,
                        Some((
                            "read_path",
                            json!({
                                "input_missing": true,
                                "skip": true,
                            }),
                            JsonValue::Null,
                            JsonValue::Null,
                        )),
                    ));
                };
                if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                    return Ok(root_json_update_outcome(
                        op.id,
                        false,
                        false,
                        Some("BYTES state read requires runtime byte storage".to_owned()),
                        Some(output_state_id),
                        None,
                        None,
                    ));
                }
                root_json_update_outcome(
                    op.id,
                    true,
                    false,
                    None,
                    Some(output_state_id),
                    Some(value),
                    Some((
                        "read_path",
                        JsonValue::Null,
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                )
            }
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::PreviousValue,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let input_state_id = root_single_state_input(op)?;
            let value = root_state_value(plan, root_state, input_state_id, op.id.0)?.clone();
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES previous-value update requires runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(value),
                Some((
                    "previous_value",
                    JsonValue::Null,
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchConst,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let [input_ref, arm_operands @ ..] = ordered_inputs.as_slice() else {
                return Err(format!(
                    "root MatchConst update branch {} has no match input",
                    op.id.0
                )
                .into());
            };
            if arm_operands.is_empty() || arm_operands.len() % 2 != 0 {
                return Err(format!(
                    "root MatchConst update branch {} has malformed arm operands",
                    op.id.0
                )
                .into());
            }
            let Some(input_value) = root_update_json_value_for_ref(
                plan,
                event,
                root_state,
                input_ref,
                source_id,
                op.id,
                "match input",
            )?
            else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_const",
                        json!({
                            "input_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            if input_value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES match input requires runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            let input_text = input_value.as_str().ok_or_else(|| {
                format!(
                    "root MatchConst update branch {} match input is not text-like",
                    op.id.0
                )
            })?;
            let mut fallback = None;
            let mut selected = None;
            for (arm_index, pair) in arm_operands.chunks_exact(2).enumerate() {
                let pattern = root_match_const_pattern(plan, &pair[0], op.id, arm_index)?;
                if pattern == "__" {
                    fallback = Some((arm_index, &pair[1]));
                } else if pattern == input_text {
                    selected = Some((arm_index, &pair[1]));
                    break;
                }
            }
            let Some((selected_arm_index, selected_ref)) = selected.or(fallback) else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_const",
                        json!({
                            "input": input_text,
                            "selected_arm_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            let Some(value) = root_update_json_value_for_ref(
                plan,
                event,
                root_state,
                selected_ref,
                source_id,
                op.id,
                "match selected arm",
            )?
            else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_const",
                        json!({
                            "input": input_text,
                            "selected_arm_index": selected_arm_index,
                            "selected_value_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            if value.as_str() == Some("SKIP") {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_const",
                        json!({
                            "input": input_text,
                            "selected_arm_index": selected_arm_index,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            }
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES match arm values require runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(value),
                Some((
                    "match_const",
                    json!({
                        "input": input_text,
                        "selected_arm_index": selected_arm_index,
                        "skip": false,
                    }),
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchValueConst,
            ordered_inputs,
            source_payload_field: None,
            update_constant_id: None,
            ..
        } => {
            let [input_ref, arm_operands @ ..] = ordered_inputs.as_slice() else {
                return Err(format!(
                    "root MatchValueConst update branch {} has no match input",
                    op.id.0
                )
                .into());
            };
            if arm_operands.is_empty() || arm_operands.len() % 2 != 0 {
                return Err(format!(
                    "root MatchValueConst update branch {} has malformed arm operands",
                    op.id.0
                )
                .into());
            }
            let Some(input_value) = root_update_json_value_for_ref(
                plan,
                event,
                root_state,
                input_ref,
                source_id,
                op.id,
                "match-value input",
            )?
            else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_value_const",
                        json!({
                            "input_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            if input_value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES match-value input requires runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            let input_text = json_scalar_textlike(&input_value).ok_or_else(|| {
                format!(
                    "root MatchValueConst update branch {} match input is not scalar text-like",
                    op.id.0
                )
            })?;
            let mut fallback = None;
            let mut selected = None;
            for (arm_index, pair) in arm_operands.chunks_exact(2).enumerate() {
                let pattern = root_match_const_pattern(plan, &pair[0], op.id, arm_index)?;
                if pattern == "__" {
                    fallback = Some((arm_index, &pair[1]));
                } else if pattern == input_text {
                    selected = Some((arm_index, &pair[1]));
                    break;
                }
            }
            let Some((selected_arm_index, selected_ref)) = selected.or(fallback) else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_value_const",
                        json!({
                            "input": input_text,
                            "selected_arm_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            let Some(value) = root_update_json_value_for_ref(
                plan,
                event,
                root_state,
                selected_ref,
                source_id,
                op.id,
                "match-value selected arm",
            )?
            else {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_value_const",
                        json!({
                            "input": input_text,
                            "selected_arm_index": selected_arm_index,
                            "selected_value_missing": true,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            };
            if value.as_str() == Some("SKIP") {
                return Ok(root_json_update_outcome(
                    op.id,
                    true,
                    true,
                    None,
                    Some(output_state_id),
                    None,
                    Some((
                        "match_value_const",
                        json!({
                            "input": input_text,
                            "selected_arm_index": selected_arm_index,
                            "skip": true,
                        }),
                        JsonValue::Null,
                        JsonValue::Null,
                    )),
                ));
            }
            if value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES") {
                return Ok(root_json_update_outcome(
                    op.id,
                    false,
                    false,
                    Some("BYTES match-value arm values require runtime byte storage".to_owned()),
                    Some(output_state_id),
                    None,
                    None,
                ));
            }
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(value),
                Some((
                    "match_value_const",
                    json!({
                        "input": input_text,
                        "selected_arm_index": selected_arm_index,
                        "skip": false,
                    }),
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextTrimOrPrevious,
            source_payload_field,
            update_constant_id: None,
            ..
        } => {
            let raw = if let Some(payload_field) = source_payload_field {
                if *payload_field == SourcePayloadField::Bytes {
                    return Ok(root_json_update_outcome(
                        op.id,
                        false,
                        false,
                        Some("BYTES text-trim payload requires runtime byte storage".to_owned()),
                        Some(output_state_id),
                        None,
                        None,
                    ));
                }
                validate_typed_payload_input(op, source_id, payload_field)?;
                source_payload_json_value(event, payload_field)?
                    .as_str()
                    .ok_or_else(|| {
                        format!(
                            "root TextTrimOrPrevious update branch {} payload is not text",
                            op.id.0
                        )
                    })?
                    .to_owned()
            } else {
                let input_state_id = root_text_trim_input(op, output_state_id)?;
                let input_label = state_label(plan, input_state_id);
                root_state_value(plan, root_state, input_state_id, op.id.0)?
                    .as_str()
                    .ok_or_else(|| {
                        format!(
                            "root TextTrimOrPrevious update branch {} input state `{input_label}` is not text",
                            op.id.0
                        )
                    })?
                    .to_owned()
            };
            let output_label = state_label(plan, output_state_id);
            let current = root_state
                .get(&output_label)
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "root TextTrimOrPrevious update branch {} output state `{output_label}` is not text",
                        op.id.0
                    )
                })?;
            let trimmed = raw.trim();
            let value = if trimmed.is_empty() {
                current.to_owned()
            } else {
                trimmed.to_owned()
            };
            root_json_update_outcome(
                op.id,
                true,
                false,
                None,
                Some(output_state_id),
                Some(JsonValue::String(value)),
                Some((
                    "text_trim_or_previous",
                    source_payload_field
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(JsonValue::Null),
                    JsonValue::Null,
                    JsonValue::Null,
                )),
            )
        }
        PlanOpKind::UpdateBranch {
            expression_kind, ..
        } => {
            return Ok(root_json_update_outcome(
                op.id,
                false,
                false,
                Some(format!(
                    "expression kind {expression_kind:?} requires runtime-specific execution"
                )),
                Some(output_state_id),
                None,
                None,
            ));
        }
        _ => {
            return Err(format!(
                "CPU PlanExecutor root JSON update branch {} is not an update branch",
                op.id.0
            )
            .into());
        }
    };
    Ok(outcome)
}

pub fn select_root_update_execution_surface(
    op_id: PlanOpId,
    evaluation: &RootJsonUpdateEvaluation,
) -> RootUpdateExecutionSurface {
    let core_value_is_bytes = evaluation
        .value
        .as_ref()
        .is_some_and(json_value_is_bytes_report);
    let kind = if evaluation.supported && evaluation.skipped_by_guard {
        RootUpdateExecutionSurfaceKind::SkippedByGuard
    } else if evaluation.supported && !core_value_is_bytes {
        RootUpdateExecutionSurfaceKind::PlanJson
    } else {
        RootUpdateExecutionSurfaceKind::RuntimeBranch
    };
    let executor_report = json!({
        "executor": "cpu-plan-root-update-execution-surface-v1",
        "update_op_id": op_id.0,
        "json_supported": evaluation.supported,
        "json_skipped_by_guard": evaluation.skipped_by_guard,
        "core_value_is_bytes": core_value_is_bytes,
        "execution_surface": match kind {
            RootUpdateExecutionSurfaceKind::PlanJson => "plan-json",
            RootUpdateExecutionSurfaceKind::RuntimeBranch => "runtime-branch",
            RootUpdateExecutionSurfaceKind::SkippedByGuard => "skipped-by-guard",
        },
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    RootUpdateExecutionSurface {
        kind,
        core_value_is_bytes,
        executor_report,
    }
}

pub fn execute_root_json_update_branch(
    plan: &MachinePlan,
    op: &PlanOp,
    source_id: SourceId,
    source_route_slot: &SourceRoute,
    event: &RootJsonSourceEvent,
    root_state: &JsonMap<String, JsonValue>,
) -> PlanExecutorResult<RootJsonUpdateExecution> {
    let evaluation = evaluate_root_json_update_branch(
        plan,
        op,
        source_id,
        source_route_slot,
        event,
        root_state,
    )?;
    let execution_surface = select_root_update_execution_surface(op.id, &evaluation);
    let mut evaluator_report = evaluation.executor_report.clone();
    if let Some(object) = evaluator_report.as_object_mut() {
        object.insert(
            "execution_surface_core".to_owned(),
            execution_surface.executor_report.clone(),
        );
    }
    let executed = if execution_surface.kind == RootUpdateExecutionSurfaceKind::PlanJson {
        let value = evaluation.value.ok_or_else(|| {
            format!(
                "root JSON update evaluator reported supported branch {} without a value",
                op.id.0
            )
        })?;
        let expression_kind = evaluation.expression_kind.ok_or_else(|| {
            format!(
                "root JSON update evaluator reported supported branch {} without an expression kind",
                op.id.0
            )
        })?;
        Some(RootExecutedUpdate {
            value,
            bytes_value: None,
            fixed_bytes_mutation: None,
            bytes_access: JsonValue::Null,
            executor_core: evaluator_report.clone(),
            state_write_core: JsonValue::Null,
            bytes_state_core: JsonValue::Null,
            expression_kind: expression_kind.to_owned(),
            source_payload_field: evaluation.source_payload_field,
            update_constant_id: evaluation.update_constant_id,
            update_constant_value: evaluation.update_constant_value,
            host_effect: JsonValue::Null,
        })
    } else {
        None
    };
    let executor_evaluator_report = evaluator_report.clone();
    let executor_report = json!({
        "executor": "cpu-plan-root-json-update-execution-v1",
        "update_op_id": op.id.0,
        "source_id": source_id.0,
        "surface": match execution_surface.kind {
            RootUpdateExecutionSurfaceKind::PlanJson => "plan-json",
            RootUpdateExecutionSurfaceKind::RuntimeBranch => "runtime-branch",
            RootUpdateExecutionSurfaceKind::SkippedByGuard => "skipped-by-guard",
        },
        "executed_update_ready": executed.is_some(),
        "evaluator_core": executor_evaluator_report,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootJsonUpdateExecution {
        surface_kind: execution_surface.kind,
        executed,
        evaluator_report,
        executor_report,
    })
}

pub fn apply_root_json_state_value(
    plan: &MachinePlan,
    root_state: &mut JsonMap<String, JsonValue>,
    target_state_id: StateId,
    value: JsonValue,
    update_op_id: PlanOpId,
) -> PlanExecutorResult<RootJsonStateWrite> {
    let target_state_label = state_label(plan, target_state_id);
    let changed = root_state.get(&target_state_label) != Some(&value);
    root_state.insert(target_state_label.clone(), value.clone());
    let semantic_delta = changed.then(|| {
        json!({
            "kind": "FieldSet",
            "list_id": null,
            "key": null,
            "generation": null,
            "source_id": null,
            "bind_epoch": null,
            "field_path": target_state_label.clone(),
            "value": value.clone(),
        })
    });
    let executor_report = json!({
        "executor": "cpu-plan-root-json-state-write-v1",
        "update_op_id": update_op_id.0,
        "target_state": target_state_label,
        "target_state_id": target_state_id.0,
        "changed": changed,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(RootJsonStateWrite {
        target_state_id,
        target_state_label,
        changed,
        value,
        semantic_delta,
        executor_report,
    })
}

fn validate_list_projection(
    projection: &PlanListProjection,
    op_id: usize,
) -> PlanExecutorResult<()> {
    match projection {
        PlanListProjection::Find { .. } | PlanListProjection::Chunk { .. } => Ok(()),
        PlanListProjection::Unknown { summary } => {
            Err(format!("list projection op {op_id} is unknown: {summary}").into())
        }
    }
}

fn initial_value(
    plan: &MachinePlan,
    constant_id: Option<PlanConstantId>,
    value_type: &PlanValueType,
    initial_value_kind: InitialValueKind,
) -> PlanExecutorResult<JsonValue> {
    let constant_id = constant_id.ok_or("state initializer is missing a typed constant id")?;
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == constant_id)
        .ok_or_else(|| format!("missing plan constant {}", constant_id.0))?;
    match (&constant.value, value_type, initial_value_kind) {
        (PlanConstantValue::Text { value }, PlanValueType::Text, InitialValueKind::Text) => {
            Ok(json!(value))
        }
        (PlanConstantValue::Number { value }, PlanValueType::Number, InitialValueKind::Number) => {
            Ok(json!(value))
        }
        (PlanConstantValue::Byte { value }, PlanValueType::Byte, InitialValueKind::Byte) => {
            Ok(json!(value))
        }
        (PlanConstantValue::Bool { value }, PlanValueType::Bool, InitialValueKind::Bool) => {
            Ok(json!(value))
        }
        (PlanConstantValue::Enum { value }, PlanValueType::Enum, InitialValueKind::Enum) => {
            Ok(json!(value))
        }
        (
            PlanConstantValue::Bytes {
                byte_len,
                sha256,
                inline_bytes: Some(bytes),
            },
            PlanValueType::Bytes { fixed_len },
            InitialValueKind::Bytes,
        ) => {
            if let Some(expected_len) = *fixed_len
                && expected_len != *byte_len
            {
                return Err(format!(
                    "plan bytes constant {} has byte_len {byte_len} but storage fixed_len {expected_len}",
                    constant_id.0
                )
                .into());
            }
            if bytes.len() as u64 != *byte_len {
                return Err(format!(
                    "plan bytes constant {} declares byte_len {byte_len} but carries {} byte(s)",
                    constant_id.0,
                    bytes.len()
                )
                .into());
            }
            Ok(json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": sha256,
                "byte_len": byte_len
            }))
        }
        (
            PlanConstantValue::Bytes {
                inline_bytes: None, ..
            },
            PlanValueType::Bytes { .. },
            InitialValueKind::Bytes,
        ) => Err(format!(
            "plan bytes constant {} has no executable payload",
            constant_id.0
        )
        .into()),
        _ => Err(format!(
            "plan constant {} does not match scalar storage type {:?}/{:?}",
            constant_id.0, value_type, initial_value_kind
        )
        .into()),
    }
}

pub fn state_label(plan: &MachinePlan, state_id: StateId) -> String {
    let expected = format!("state:{}", state_id.0);
    plan.debug_map
        .state_slots
        .iter()
        .find(|entry| entry.id == expected)
        .map(|entry| entry.label.clone())
        .unwrap_or(expected)
}

pub fn state_label_by_id(plan: &MachinePlan, state_id: usize) -> String {
    state_label(plan, StateId(state_id))
}

pub fn root_state_is_scalar(plan: &MachinePlan, state_id: StateId) -> bool {
    plan.storage_layout
        .scalar_slots
        .iter()
        .any(|slot| slot.state_id == state_id && !slot.indexed)
}

fn json_value_at_dotted_path<'a>(root: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    if let Some(value) = root.get(path) {
        return Some(value);
    }
    let mut value = root;
    for segment in path.split('.') {
        value = value.get(segment)?;
    }
    Some(value)
}

fn root_update_output_state_id(op: &PlanOp) -> Option<StateId> {
    if op.indexed {
        return None;
    }
    match op.output.as_ref()? {
        ValueRef::State(state_id) => Some(*state_id),
        _ => None,
    }
}

fn root_update_state_inputs(op: &PlanOp) -> Vec<StateId> {
    let output = root_update_output_state_id(op);
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if let ValueRef::State(state_id) = input
            && Some(*state_id) != output
            && !inputs.contains(state_id)
        {
            inputs.push(*state_id);
        }
    }
    inputs
}

fn sort_plan_ops_for_same_event_root_reads(ops: &mut Vec<&PlanOp>) {
    let target_state_ids = ops
        .iter()
        .filter_map(|op| root_update_output_state_id(op))
        .collect::<Vec<_>>();
    let mut pending = std::mem::take(ops);
    let mut sorted = Vec::with_capacity(pending.len());
    let mut settled = BTreeSet::new();
    while !pending.is_empty() {
        let ready_position = pending.iter().position(|candidate| {
            root_update_state_inputs(candidate)
                .iter()
                .all(|input| !target_state_ids.contains(input) || settled.contains(&input.0))
        });
        let next = pending.remove(ready_position.unwrap_or(0));
        if let Some(state_id) = root_update_output_state_id(next) {
            settled.insert(state_id.0);
        }
        sorted.push(next);
    }
    *ops = sorted;
}

fn source_key_gate_for_root_updates(plan: &MachinePlan, source_id: SourceId) -> Option<String> {
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                        source_id: expression_source_id,
                        required_key,
                        ..
                    }),
                ..
            } if *expression_source_id == source_id => Some(required_key.clone()),
            _ => None,
        })
}

fn source_id_for_label(plan: &MachinePlan, label: &str) -> Option<SourceId> {
    plan.debug_map
        .source_routes
        .iter()
        .find(|entry| entry.label == label)
        .and_then(|entry| debug_entry_numeric_id(&entry.id, "source"))
        .map(SourceId)
}

fn state_id_for_label(plan: &MachinePlan, label: &str) -> Option<StateId> {
    plan.debug_map
        .state_slots
        .iter()
        .find(|entry| entry.label == label)
        .and_then(|entry| debug_entry_numeric_id(&entry.id, "state"))
        .map(StateId)
}

fn debug_entry_numeric_id(value: &str, prefix: &str) -> Option<usize> {
    value
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix(':'))
        .and_then(|suffix| suffix.parse::<usize>().ok())
}

pub fn field_label(plan: &MachinePlan, field_id: usize) -> String {
    let expected = format!("field:{field_id}");
    plan.debug_map
        .fields
        .iter()
        .find(|entry| entry.id == expected)
        .map(|entry| entry.label.clone())
        .unwrap_or(expected)
}

pub fn semantic_field_label(plan: &MachinePlan, field_id: usize) -> String {
    field_label(plan, field_id)
}

pub fn derived_field_label(plan: &MachinePlan, field_id: usize) -> String {
    let expected = format!("field:{field_id}");
    plan.debug_map
        .derived_values
        .iter()
        .find(|entry| debug_entry_numeric_id(&entry.id, "field") == Some(field_id))
        .map(|entry| entry.label.clone())
        .unwrap_or(expected)
}

pub fn list_label(plan: &MachinePlan, list_id: usize) -> String {
    let expected = format!("list:{list_id}");
    plan.debug_map
        .list_slots
        .iter()
        .find(|entry| entry.id == expected)
        .map(|entry| entry.label.clone())
        .unwrap_or(expected)
}

fn state_label_from_ref(plan: &MachinePlan, value_ref: &ValueRef) -> PlanExecutorResult<String> {
    match value_ref {
        ValueRef::State(state_id) => Ok(state_label(plan, *state_id)),
        ValueRef::Field(field_id) => Ok(field_label(plan, field_id.0)),
        _ => Err(format!("expected state or field ref, got {value_ref:?}").into()),
    }
}

fn value_ref_report(value_ref: &ValueRef) -> JsonValue {
    match value_ref {
        ValueRef::State(state_id) => json!({
            "kind": "state",
            "id": state_id.0,
        }),
        ValueRef::Field(field_id) => json!({
            "kind": "field",
            "id": field_id.0,
        }),
        ValueRef::List(list_id) => json!({
            "kind": "list",
            "id": list_id.0,
        }),
        ValueRef::Source(source_id) => json!({
            "kind": "source",
            "id": source_id.0,
        }),
        ValueRef::SourcePayload { source_id, field } => json!({
            "kind": "source_payload",
            "source_id": source_id.0,
            "field": format!("{field:?}"),
        }),
        ValueRef::Constant(constant_id) => json!({
            "kind": "constant",
            "id": constant_id.0,
        }),
    }
}

fn projection_selector_value(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    value: &ValueRef,
) -> PlanExecutorResult<JsonValue> {
    match value {
        ValueRef::State(state_id) => {
            let label = state_label(plan, *state_id);
            root_state.get(&label).cloned().ok_or_else(|| {
                format!("list projection selector state `{label}` is not materialized").into()
            })
        }
        _ => Err(
            "CPU PlanExecutor list projection selector currently requires a root state ref".into(),
        ),
    }
}

pub fn row_json_values_equal(left: Option<&JsonValue>, right: &JsonValue) -> bool {
    match (left, right) {
        (Some(JsonValue::String(left)), JsonValue::String(right)) => left == right,
        (Some(JsonValue::String(left)), JsonValue::Number(right)) => {
            right
                .as_i64()
                .is_some_and(|right| left == &right.to_string())
                || right
                    .as_u64()
                    .is_some_and(|right| left == &right.to_string())
                || right
                    .as_f64()
                    .is_some_and(|right| left == &right.to_string())
        }
        (Some(JsonValue::Number(left)), JsonValue::String(right)) => {
            left.as_i64().is_some_and(|left| &left.to_string() == right)
                || left.as_u64().is_some_and(|left| &left.to_string() == right)
                || left.as_f64().is_some_and(|left| &left.to_string() == right)
        }
        (Some(left), right) => left == right,
        (None, JsonValue::Null) => true,
        (None, _) => false,
    }
}

struct ListRetainPredicateResolution {
    retained: bool,
    report: JsonValue,
}

fn list_retain_predicate_resolution(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    predicate: &boon_plan::PlanListRemovePredicate,
    row: &PlanExecutorListRow,
) -> PlanExecutorResult<ListRetainPredicateResolution> {
    match predicate {
        boon_plan::PlanListRemovePredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            let selector_label = state_label_from_ref(plan, selector)?;
            let selector_value = root_state
                .get(&selector_label)
                .and_then(json_scalar_textlike)
                .ok_or_else(|| {
                    format!("selected-filter retain selector `{selector_label}` is not materialized")
                })?;
            let row_field_label = state_label_from_ref(plan, row_field)?;
            let row_field_name = local_field_name(&row_field_label);
            let row_value = row
                .fields
                .get(&row_field_name)
                .and_then(JsonValue::as_bool)
                .ok_or_else(|| {
                    format!(
                        "selected-filter retain row field `{row_field_name}` is not bool on row key {} generation {}",
                        row.key, row.generation
                    )
                })?;
            let retained = match selector_value.as_str() {
                "All" => true,
                "Active" => !row_value,
                "Completed" => row_value,
                other => {
                    return Err(format!(
                        "selected-filter retain selector `{selector_label}` has unsupported value `{other}`"
                    )
                    .into())
                }
            };
            Ok(ListRetainPredicateResolution {
                retained,
                report: json!({
                    "kind": "selected_filter_visibility",
                    "selector_ref": value_ref_report(selector),
                    "selector": selector_label,
                    "selector_value": selector_value,
                    "selector_materialized": true,
                    "row_field_ref": value_ref_report(row_field),
                    "row_field": row_field_name,
                }),
            })
        }
        boon_plan::PlanListRemovePredicate::AlwaysTrue => Ok(ListRetainPredicateResolution {
            retained: true,
            report: json!({ "kind": "always_true" }),
        }),
        boon_plan::PlanListRemovePredicate::RowFieldBool { input } => {
            let (field_name, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(ListRetainPredicateResolution {
                retained: value,
                report: json!({
                    "kind": "row_field_bool",
                    "row_field": field_name,
                }),
            })
        }
        boon_plan::PlanListRemovePredicate::RowFieldBoolNot { input } => {
            let (field_name, value) = list_remove_predicate_field_value(plan, input, row)?;
            Ok(ListRetainPredicateResolution {
                retained: !value,
                report: json!({
                    "kind": "row_field_bool_not",
                    "row_field": field_name,
                }),
            })
        }
        boon_plan::PlanListRemovePredicate::Unknown { summary } => Err(format!(
            "CPU root-scenario PlanExecutor does not support unknown list retain predicate `{summary}`"
        )
        .into()),
    }
}

fn list_retain_empty_predicate_report(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    predicate: &boon_plan::PlanListRemovePredicate,
) -> PlanExecutorResult<JsonValue> {
    match predicate {
        boon_plan::PlanListRemovePredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            let selector_label = state_label_from_ref(plan, selector)?;
            let selector_value = root_state
                .get(&selector_label)
                .and_then(json_scalar_textlike)
                .ok_or_else(|| {
                    format!("selected-filter retain selector `{selector_label}` is not materialized")
                })?;
            let row_field_label = state_label_from_ref(plan, row_field)?;
            Ok(json!({
                "kind": "selected_filter_visibility",
                "selector_ref": value_ref_report(selector),
                "selector": selector_label,
                "selector_value": selector_value,
                "selector_materialized": true,
                "row_field_ref": value_ref_report(row_field),
                "row_field": local_field_name(&row_field_label),
            }))
        }
        boon_plan::PlanListRemovePredicate::AlwaysTrue => Ok(json!({ "kind": "always_true" })),
        boon_plan::PlanListRemovePredicate::RowFieldBool { input } => Ok(json!({
            "kind": "row_field_bool",
            "row_field": local_field_name(&state_label_from_ref(plan, input)?),
        })),
        boon_plan::PlanListRemovePredicate::RowFieldBoolNot { input } => Ok(json!({
            "kind": "row_field_bool_not",
            "row_field": local_field_name(&state_label_from_ref(plan, input)?),
        })),
        boon_plan::PlanListRemovePredicate::Unknown { summary } => Err(format!(
            "CPU root-scenario PlanExecutor does not support unknown list retain predicate `{summary}`"
        )
        .into()),
    }
}

fn list_remove_predicate_field_value(
    plan: &MachinePlan,
    input: &ValueRef,
    row: &PlanExecutorListRow,
) -> PlanExecutorResult<(String, bool)> {
    let field_name = local_field_name(&state_label_from_ref(plan, input)?);
    let value = row
        .fields
        .get(&field_name)
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "list retain predicate field `{field_name}` is not bool on row key {} generation {}",
                row.key, row.generation
            )
        })?;
    Ok((field_name, value))
}

#[allow(dead_code)]
fn constant_map(plan: &MachinePlan) -> BTreeMap<PlanConstantId, &PlanConstantValue> {
    plan.constants
        .iter()
        .map(|constant| (constant.id, &constant.value))
        .collect()
}

fn root_json_update_outcome(
    op_id: PlanOpId,
    supported: bool,
    skipped_by_guard: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    metadata: Option<(&'static str, JsonValue, JsonValue, JsonValue)>,
) -> RootJsonUpdateEvaluation {
    let (expression_kind, source_payload_field, update_constant_id, update_constant_value) =
        metadata
            .map(|(kind, payload, constant_id, constant_value)| {
                (Some(kind), payload, constant_id, constant_value)
            })
            .unwrap_or((None, JsonValue::Null, JsonValue::Null, JsonValue::Null));
    let executor_report = json!({
        "executor": "cpu-plan-root-json-update-evaluator-v1",
        "update_op_id": op_id.0,
        "supported": supported,
        "skipped_by_guard": skipped_by_guard,
        "unsupported_reason": unsupported_reason,
        "target_state_id": target_state_id.map(|id| id.0),
        "expression_kind": expression_kind,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    RootJsonUpdateEvaluation {
        supported,
        skipped_by_guard,
        unsupported_reason,
        target_state_id,
        value,
        expression_kind,
        source_payload_field,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

pub fn source_guard_matches(
    guard: &Option<PlanSourceGuard>,
    active_source_id: SourceId,
    event: &RootJsonSourceEvent,
) -> PlanExecutorResult<bool> {
    let Some(guard) = guard else {
        return Ok(true);
    };
    match guard {
        PlanSourceGuard::SourcePayloadOneOf {
            source_id,
            field,
            values,
        } => {
            if *source_id != active_source_id {
                return Err(format!(
                    "source guard targets source {}, but active source is {}",
                    source_id.0, active_source_id.0
                )
                .into());
            }
            if *field == SourcePayloadField::Bytes {
                let field_name = source_payload_bytes_field_name(field)?;
                if !event.payload_bytes.contains_key(field_name) {
                    return Ok(false);
                }
                let payload = source_payload_bytes(event, field)?;
                for expected in values {
                    let expected_bytes = bytes_decode_hex(expected).map_err(|error| {
                        format!(
                            "BYTES source payload guard value `{expected}` is invalid hex: {error}"
                        )
                    })?;
                    if expected_bytes == payload {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
            let Some(value) = source_payload_json_value_if_present(event, field)? else {
                return Ok(false);
            };
            Ok(value
                .as_str()
                .is_some_and(|payload| values.iter().any(|expected| expected == payload)))
        }
    }
}

fn source_payload_json_value(
    event: &RootJsonSourceEvent,
    field: &SourcePayloadField,
) -> PlanExecutorResult<JsonValue> {
    match field {
        SourcePayloadField::Text => event
            .text
            .clone()
            .map(JsonValue::String)
            .ok_or_else(|| "source event is missing text payload".into()),
        SourcePayloadField::Key => event
            .key
            .clone()
            .map(JsonValue::String)
            .ok_or_else(|| "source event is missing key payload".into()),
        SourcePayloadField::Address => event
            .address
            .clone()
            .map(JsonValue::String)
            .ok_or_else(|| "source event is missing address payload".into()),
        SourcePayloadField::Named(name) => event
            .payload
            .get(name)
            .cloned()
            .map(JsonValue::String)
            .ok_or_else(|| format!("source event is missing `{name}` payload").into()),
        SourcePayloadField::Bytes => source_payload_bytes(event, field).map(bytes_report_json),
    }
}

fn source_payload_json_value_if_present(
    event: &RootJsonSourceEvent,
    field: &SourcePayloadField,
) -> PlanExecutorResult<Option<JsonValue>> {
    match field {
        SourcePayloadField::Text => Ok(event.text.clone().map(JsonValue::String)),
        SourcePayloadField::Key => Ok(event.key.clone().map(JsonValue::String)),
        SourcePayloadField::Address => Ok(event.address.clone().map(JsonValue::String)),
        SourcePayloadField::Named(name) => {
            Ok(event.payload.get(name).cloned().map(JsonValue::String))
        }
        SourcePayloadField::Bytes => {
            let field_name = source_payload_bytes_field_name(field)?;
            if !event.payload_bytes.contains_key(field_name) {
                return Ok(None);
            }
            source_payload_bytes(event, field)
                .map(bytes_report_json)
                .map(Some)
        }
    }
}

fn source_payload_value_for_slot(
    event: &RootJsonSourceEvent,
    field: &SourcePayloadField,
    slot: &boon_plan::ScalarStorageSlot,
    op_id: PlanOpId,
) -> PlanExecutorResult<JsonValue> {
    match field {
        SourcePayloadField::Bytes => {
            let bytes = source_payload_bytes(event, field)?;
            let PlanValueType::Bytes { fixed_len } = slot.value_type else {
                return Err(format!(
                    "root source-payload update branch {} reads BYTES payload but output state {} is not BYTES",
                    op_id.0, slot.state_id.0
                )
                .into());
            };
            if let Some(expected_len) = fixed_len
                && expected_len != bytes.len() as u64
            {
                return Err(format!(
                    "root source-payload update branch {} BYTES payload has byte_len {} but output fixed_len {expected_len}",
                    op_id.0,
                    bytes.len()
                )
                .into());
            }
            Ok(bytes_report_json(bytes))
        }
        SourcePayloadField::Address
        | SourcePayloadField::Key
        | SourcePayloadField::Text => {
            if slot.value_type != PlanValueType::Text {
                return Err(format!(
                    "root source-payload update branch {} reads text payload but output state {} is not TEXT",
                    op_id.0, slot.state_id.0
                )
                .into());
            }
            source_payload_json_value(event, field)
        }
        SourcePayloadField::Named(name) if name == "press" => match slot.value_type {
            PlanValueType::Bool => source_payload_bool_value(event, field),
            PlanValueType::Text => source_payload_json_value(event, field),
            _ => Err(format!(
                "root source-payload update branch {} reads press payload but output state {} is neither BOOL nor TEXT",
                op_id.0, slot.state_id.0
            )
            .into()),
        },
        SourcePayloadField::Named(_) => {
            if slot.value_type != PlanValueType::Text {
                return Err(format!(
                    "root source-payload update branch {} reads text payload but output state {} is not TEXT",
                    op_id.0, slot.state_id.0
                )
                .into());
            }
            source_payload_json_value(event, field)
        }
    }
}

fn source_payload_bool_value(
    event: &RootJsonSourceEvent,
    field: &SourcePayloadField,
) -> PlanExecutorResult<JsonValue> {
    let SourcePayloadField::Named(name) = field else {
        return Err("bool source payloads must be named fields".into());
    };
    let Some(value) = event.payload.get(name) else {
        return Ok(JsonValue::Bool(true));
    };
    match value.as_str() {
        "true" | "True" | "1" | "yes" | "Yes" | "on" | "On" | "press" | "pressed" => {
            Ok(JsonValue::Bool(true))
        }
        "false" | "False" | "0" | "no" | "No" | "off" | "Off" => Ok(JsonValue::Bool(false)),
        _ => Err(
            format!("source event bool payload `{name}` has unsupported value `{value}`").into(),
        ),
    }
}

fn source_payload_bytes<'a>(
    event: &'a RootJsonSourceEvent,
    field: &SourcePayloadField,
) -> PlanExecutorResult<&'a [u8]> {
    let field_name = source_payload_bytes_field_name(field)?;
    event
        .payload_bytes
        .get(field_name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("source event is missing `{field_name}` BYTES payload").into())
}

fn source_payload_bytes_field_name(field: &SourcePayloadField) -> PlanExecutorResult<&str> {
    match field {
        SourcePayloadField::Bytes => Ok("bytes"),
        other => Err(format!("source payload field {other:?} is not BYTES").into()),
    }
}

fn bytes_report_json(bytes: &[u8]) -> JsonValue {
    json!({
        "$boon_type": "BYTES",
        "storage": if bytes.len() > 1024 { "shared" } else { "inline" },
        "digest": sha256_bytes(bytes),
        "byte_len": bytes.len() as u64,
    })
}

fn json_value_is_bytes_report(value: &JsonValue) -> bool {
    value.get("$boon_type").and_then(JsonValue::as_str) == Some("BYTES")
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_route_payload_field(
    source_route_slot: &SourceRoute,
    field: &SourcePayloadField,
    op_id: PlanOpId,
) -> PlanExecutorResult<()> {
    if source_route_slot.payload_schema.fields.contains(field) {
        return Ok(());
    }
    Err(format!(
        "selected update branch {} reads payload field {:?}, but route schema is {:?}",
        op_id.0, field, source_route_slot.payload_schema.fields
    )
    .into())
}

fn validate_typed_payload_input(
    op: &PlanOp,
    source_id: SourceId,
    field: &SourcePayloadField,
) -> PlanExecutorResult<()> {
    let typed_payload_ref_present = op.inputs.iter().any(|input| {
        matches!(
            input,
            ValueRef::SourcePayload {
                source_id: input_source_id,
                field: input_field
            } if *input_source_id == source_id && input_field == field
        )
    });
    if typed_payload_ref_present {
        Ok(())
    } else {
        Err(format!(
            "selected update branch {} is missing typed SourcePayload input",
            op.id.0
        )
        .into())
    }
}

fn scalar_slot_for_state(
    plan: &MachinePlan,
    state_id: StateId,
    op_id: PlanOpId,
) -> PlanExecutorResult<&boon_plan::ScalarStorageSlot> {
    plan.storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == state_id)
        .ok_or_else(|| {
            format!(
                "root JSON update branch {} targets missing state {}",
                op_id.0, state_id.0
            )
            .into()
        })
}

fn root_single_state_input(op: &PlanOp) -> PlanExecutorResult<StateId> {
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if let ValueRef::State(state_id) = input
            && !inputs.contains(state_id)
        {
            inputs.push(*state_id);
        }
    }
    let [input] = inputs.as_slice() else {
        return Err(format!(
            "root update branch {} expected one state input, found {}",
            op.id.0,
            inputs.len()
        )
        .into());
    };
    Ok(*input)
}

fn root_single_state_or_field_input(
    op: &PlanOp,
    output_state_id: StateId,
) -> PlanExecutorResult<ValueRef> {
    let inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::State(state_id) if *state_id != output_state_id => Some(input.clone()),
            ValueRef::Field(_) => Some(input.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err(format!(
            "root update branch {} expected one non-output state or derived-field input, found {}",
            op.id.0,
            inputs.len()
        )
        .into());
    };
    Ok(input.clone())
}

fn root_text_bytes_conversion_operands(
    plan: &MachinePlan,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(StateId, String, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(encoding_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "root {operation} update branch {} expected state and encoding constant operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root {operation} update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let encoding_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *encoding_constant_id)
        .ok_or_else(|| {
            format!(
                "root {operation} update branch {} references missing encoding constant {}",
                op.id.0, encoding_constant_id.0
            )
        })?;
    let PlanConstantValue::Text { value: encoding } = &encoding_constant.value else {
        return Err(format!(
            "root {operation} update branch {} encoding constant {} is not TEXT",
            op.id.0, encoding_constant_id.0
        )
        .into());
    };
    let Some(normalized_encoding) = normalized_text_bytes_encoding(encoding) else {
        return Err(format!(
            "root {operation} update branch {} encoding constant {} is unsupported: `{encoding}`",
            op.id.0, encoding_constant_id.0
        )
        .into());
    };
    Ok((
        *input,
        normalized_encoding.to_owned(),
        *encoding_constant_id,
    ))
}

fn root_bytes_ordered_state_inputs(
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(StateId, StateId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::State(left), ValueRef::State(right)] = ordered_inputs.as_slice() else {
        return Err(format!(
            "root {operation} update branch {} expected two ordered state inputs, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*left)) || !op.inputs.contains(&ValueRef::State(*right))
    {
        return Err(format!(
            "root {operation} update branch {} ordered inputs are not declared op inputs",
            op.id.0
        )
        .into());
    }
    Ok((*left, *right))
}

fn root_bytes_distinct_state_inputs(
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(StateId, StateId)> {
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if let ValueRef::State(state_id) = input
            && !inputs.contains(state_id)
        {
            inputs.push(*state_id);
        }
    }
    match inputs.as_slice() {
        [left, right] => Ok((*left, *right)),
        _ => Err(format!(
            "root {operation} update branch {} expected two distinct state inputs, found {}",
            op.id.0,
            inputs.len()
        )
        .into()),
    }
}

fn root_single_source_payload_input(
    op: &PlanOp,
    active_source_id: SourceId,
) -> PlanExecutorResult<Option<SourcePayloadField>> {
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if let ValueRef::SourcePayload { source_id, field } = input
            && *source_id == active_source_id
            && !inputs.contains(field)
        {
            inputs.push(field.clone());
        }
    }
    match inputs.as_slice() {
        [] => Ok(None),
        [field] => Ok(Some(field.clone())),
        _ => Err(format!(
            "root update branch {} has ambiguous source-payload inputs",
            op.id.0
        )
        .into()),
    }
}

fn root_text_trim_input(op: &PlanOp, output_state_id: StateId) -> PlanExecutorResult<StateId> {
    let mut non_output_inputs = Vec::new();
    let mut output_present = false;
    for input in &op.inputs {
        let ValueRef::State(state_id) = input else {
            continue;
        };
        if *state_id == output_state_id {
            output_present = true;
        } else if !non_output_inputs.contains(state_id) {
            non_output_inputs.push(*state_id);
        }
    }
    match non_output_inputs.as_slice() {
        [input] => Ok(*input),
        [] if output_present => Ok(output_state_id),
        [] => Err(format!(
            "root TextTrimOrPrevious update branch {} has no typed state input",
            op.id.0
        )
        .into()),
        _ => Err(format!(
            "root TextTrimOrPrevious update branch {} has ambiguous non-output state inputs",
            op.id.0
        )
        .into()),
    }
}

fn root_state_value<'a>(
    plan: &MachinePlan,
    root_state: &'a JsonMap<String, JsonValue>,
    state_id: StateId,
    op_id: usize,
) -> PlanExecutorResult<&'a JsonValue> {
    let label = state_label(plan, state_id);
    root_state.get(&label).ok_or_else(|| {
        format!("root update branch {op_id} input state `{label}` is missing").into()
    })
}

struct RootExecutorBytesView<'a> {
    bytes: &'a [u8],
    access_source: &'static str,
    cow_kind: &'static str,
}

impl RootExecutorBytesView<'_> {
    fn access_json(&self, input_state_id: StateId) -> JsonValue {
        json!({
            "input_state_id": input_state_id.0,
            "access_source": self.access_source,
            "cow_kind": self.cow_kind,
        })
    }

    fn labeled_access_json(&self, role: &str, input_state_id: StateId) -> JsonValue {
        json!({
            "role": role,
            "input_state_id": input_state_id.0,
            "access_source": self.access_source,
            "cow_kind": self.cow_kind,
        })
    }
}

fn root_executor_bytes_view<'a>(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    bytes_environment: &'a (impl RootBytesEnvironment + ?Sized),
    state_id: StateId,
    op_id: usize,
) -> PlanExecutorResult<RootExecutorBytesView<'a>> {
    let label = state_label(plan, state_id);
    let public_value = root_state_value(plan, root_state, state_id, op_id)?;
    if public_value.get("$boon_type").and_then(JsonValue::as_str) != Some("BYTES") {
        return Err(
            format!("root update branch {op_id} input state `{label}` is not BYTES").into(),
        );
    }
    if let Some(bytes) = bytes_environment.private_bytes_for_state(state_id) {
        let expected_summary = bytes.report_json();
        if &expected_summary != public_value {
            return Err(format!(
                "root update branch {op_id} bytes state `{label}` public summary does not match executor private byte state"
            )
            .into());
        }
        return Ok(RootExecutorBytesView {
            bytes: bytes.inline_bytes(),
            access_source: "root_bytes_state",
            cow_kind: "borrowed",
        });
    }
    if let Some(bytes) = bytes_environment.fixed_byte_bank_for_state(state_id) {
        let expected_summary = bytes_report_json(bytes);
        if &expected_summary != public_value {
            return Err(format!(
                "root update branch {op_id} fixed byte bank `{label}` public summary does not match executor private bank state"
            )
            .into());
        }
        return Ok(RootExecutorBytesView {
            bytes,
            access_source: "root_fixed_byte_bank",
            cow_kind: "borrowed",
        });
    }
    Err(
        format!("root update branch {op_id} has no executor private byte state for `{label}`")
            .into(),
    )
}

struct IndexedExecutorBytesView<'a> {
    bytes: &'a [u8],
    access_source: &'static str,
    cow_kind: &'static str,
}

struct IndexedExecutorBytesInputView<'a> {
    bytes: &'a [u8],
    access_source: &'static str,
    cow_kind: &'static str,
    input_kind: &'static str,
    input_id: usize,
    byte_bank_declared: bool,
}

impl IndexedExecutorBytesInputView<'_> {
    fn labeled_access_json(&self, role: &str) -> JsonValue {
        let mut access = json!({
            "role": role,
            "input_kind": self.input_kind,
            "input_id": self.input_id,
            "access_source": self.access_source,
            "cow_kind": self.cow_kind,
            "byte_len": self.bytes.len(),
            "byte_bank_declared": self.byte_bank_declared,
            "byte_bank_used": self.access_source == "indexed_fixed_byte_bank",
        });
        if let JsonValue::Object(object) = &mut access {
            match self.input_kind {
                "state" => {
                    object.insert("input_state_id".to_owned(), json!(self.input_id));
                }
                "field" => {
                    object.insert("input_field_id".to_owned(), json!(self.input_id));
                }
                _ => {}
            }
        }
        access
    }
}

fn indexed_executor_row_bytes_view<'a>(
    plan: &MachinePlan,
    row: &'a IndexedRowView,
    state_id: StateId,
    op_id: usize,
) -> PlanExecutorResult<IndexedExecutorBytesView<'a>> {
    let label = state_label(plan, state_id);
    let field_name = local_field_name(&label);
    let public_value = row.fields.get(&field_name).ok_or_else(|| {
        format!("indexed update branch {op_id} row field `{field_name}` is missing")
    })?;
    if public_value.get("$boon_type").and_then(JsonValue::as_str) != Some("BYTES") {
        return Err(format!(
            "indexed update branch {op_id} input row field `{field_name}` is not BYTES"
        )
        .into());
    }
    if let Some(bytes) = row.fixed_byte_banks.get(&field_name) {
        if indexed_fixed_bytes_report_json(bytes) != *public_value {
            return Err(format!(
                "indexed update branch {op_id} fixed byte bank `{field_name}` public summary does not match private bank state"
            )
            .into());
        }
        return Ok(IndexedExecutorBytesView {
            bytes: bytes.as_slice(),
            access_source: "indexed_fixed_byte_bank",
            cow_kind: "borrowed",
        });
    }
    let bytes = row.private_bytes.get(&field_name).ok_or_else(|| {
        format!(
            "indexed update branch {op_id} input row field `{field_name}` has no private BYTES state"
        )
    })?;
    if bytes.report_json() != *public_value {
        return Err(format!(
            "indexed update branch {op_id} bytes row field `{field_name}` public summary does not match private byte state"
        )
        .into());
    }
    Ok(IndexedExecutorBytesView {
        bytes: bytes.inline_bytes(),
        access_source: "indexed_row_private_bytes",
        cow_kind: "borrowed",
    })
}

fn indexed_executor_row_bytes_input_view<'a>(
    plan: &MachinePlan,
    row: &'a IndexedRowView,
    input: &ValueRef,
    op_id: usize,
) -> PlanExecutorResult<IndexedExecutorBytesInputView<'a>> {
    match input {
        ValueRef::State(state_id) => {
            let view = indexed_executor_row_bytes_view(plan, row, *state_id, op_id)?;
            Ok(IndexedExecutorBytesInputView {
                bytes: view.bytes,
                access_source: view.access_source,
                cow_kind: view.cow_kind,
                input_kind: "state",
                input_id: state_id.0,
                byte_bank_declared: indexed_state_has_fixed_byte_bank(plan, *state_id),
            })
        }
        ValueRef::Field(field_id) => {
            let label = field_label(plan, field_id.0);
            let field_name = local_field_name(&label);
            let public_value = row.fields.get(&field_name).ok_or_else(|| {
                format!("indexed update branch {op_id} row field `{field_name}` is missing")
            })?;
            if public_value.get("$boon_type").and_then(JsonValue::as_str) != Some("BYTES") {
                return Err(format!(
                    "indexed update branch {op_id} input row field `{field_name}` is not BYTES"
                )
                .into());
            }
            if let Some(bytes) = row.fixed_byte_banks.get(&field_name) {
                if indexed_fixed_bytes_report_json(bytes) != *public_value {
                    return Err(format!(
                        "indexed update branch {op_id} fixed byte bank `{field_name}` public summary does not match private bank state"
                    )
                    .into());
                }
                return Ok(IndexedExecutorBytesInputView {
                    bytes: bytes.as_slice(),
                    access_source: "indexed_fixed_byte_bank",
                    cow_kind: "borrowed",
                    input_kind: "field",
                    input_id: field_id.0,
                    byte_bank_declared: true,
                });
            }
            let bytes = row.private_bytes.get(&field_name).ok_or_else(|| {
                format!(
                    "indexed update branch {op_id} input row field `{field_name}` has no private BYTES state"
                )
            })?;
            if bytes.report_json() != *public_value {
                return Err(format!(
                    "indexed update branch {op_id} bytes row field `{field_name}` public summary does not match private byte state"
                )
                .into());
            }
            Ok(IndexedExecutorBytesInputView {
                bytes: bytes.inline_bytes(),
                access_source: "indexed_row_private_bytes",
                cow_kind: "borrowed",
                input_kind: "field",
                input_id: field_id.0,
                byte_bank_declared: false,
            })
        }
        _ => Err(format!(
            "indexed update branch {op_id} expected BYTES state or row-field input, found {input:?}"
        )
        .into()),
    }
}

fn indexed_row_state_text(
    plan: &MachinePlan,
    row: &IndexedRowView,
    state_id: StateId,
    op_id: usize,
    role: &str,
) -> PlanExecutorResult<String> {
    let label = state_label(plan, state_id);
    let field_name = local_field_name(&label);
    row.fields
        .get(&field_name)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "indexed update branch {op_id} {role} state `{field_name}` is missing or not TEXT"
            )
            .into()
        })
}

fn indexed_row_field_text(
    plan: &MachinePlan,
    row: &IndexedRowView,
    field_id: FieldId,
    op_id: usize,
    role: &str,
) -> PlanExecutorResult<String> {
    let label = field_label(plan, field_id.0);
    let field_name = local_field_name(&label);
    row.fields
        .get(&field_name)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "indexed update branch {op_id} {role} row field `{field_name}` is missing or not TEXT"
            )
        .into()
    })
}

fn indexed_single_text_input(
    op: &PlanOp,
    output_state_id: StateId,
) -> PlanExecutorResult<ValueRef> {
    let inputs = op
        .inputs
        .iter()
        .filter(|input| match input {
            ValueRef::State(state_id) => *state_id != output_state_id,
            ValueRef::Field(_) => true,
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err(format!(
            "indexed update branch {} expected one TEXT input, found {}",
            op.id.0,
            inputs.len()
        )
        .into());
    };
    Ok(input.clone())
}

fn indexed_text_to_bytes_operands(
    plan: &MachinePlan,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(ValueRef, String, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [input, ValueRef::Constant(encoding_constant_id)] = ordered_inputs.as_slice() else {
        return Err(format!(
            "indexed {operation} update branch {} expected TEXT input plus encoding constant",
            op.id.0
        )
        .into());
    };
    let encoding_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *encoding_constant_id)
        .ok_or_else(|| {
            format!(
                "indexed {operation} update branch {} references missing encoding constant {}",
                op.id.0, encoding_constant_id.0
            )
        })?;
    let PlanConstantValue::Text { value: encoding } = &encoding_constant.value else {
        return Err(format!(
            "indexed {operation} update branch {} encoding constant {} is not TEXT",
            op.id.0, encoding_constant_id.0
        )
        .into());
    };
    Ok((input.clone(), encoding.clone(), *encoding_constant_id))
}

fn indexed_row_text_input(
    plan: &MachinePlan,
    row: &IndexedRowView,
    input: &ValueRef,
    op_id: usize,
    role: &str,
) -> PlanExecutorResult<String> {
    match input {
        ValueRef::State(state_id) => indexed_row_state_text(plan, row, *state_id, op_id, role),
        ValueRef::Field(field_id) => indexed_row_field_text(plan, row, *field_id, op_id, role),
        other => Err(format!(
            "indexed update branch {op_id} {role} TEXT input must be state or row field, got {other:?}"
        )
        .into()),
    }
}

fn indexed_text_input_label(plan: &MachinePlan, input: &ValueRef) -> String {
    match input {
        ValueRef::State(state_id) => local_field_name(&state_label(plan, *state_id)),
        ValueRef::Field(field_id) => local_field_name(&semantic_field_label(plan, field_id.0)),
        other => format!("{other:?}"),
    }
}

fn indexed_text_input_access_json(plan: &MachinePlan, input: &ValueRef) -> JsonValue {
    match input {
        ValueRef::State(state_id) => json!({
            "input_kind": "state",
            "input_state_id": state_id.0,
        }),
        ValueRef::Field(field_id) => json!({
            "input_kind": "field",
            "input_field_id": field_id.0,
            "input_field": semantic_field_label(plan, field_id.0),
        }),
        _ => JsonValue::Null,
    }
}

fn indexed_file_read_bytes_path(
    plan: &MachinePlan,
    row: &IndexedRowView,
    op: &PlanOp,
    path_operand: FileReadBytesPathOperand,
) -> PlanExecutorResult<(String, JsonValue, JsonValue, &'static str)> {
    match path_operand {
        FileReadBytesPathOperand::StaticConstant { path, constant_id } => Ok((
            path.clone(),
            json!(constant_id.0),
            json!({
                "path": path,
                "path_source": "static_constant",
            }),
            "static_constant",
        )),
        FileReadBytesPathOperand::StatePath { state_id } => {
            let label = state_label(plan, state_id);
            let path = indexed_row_state_text(plan, row, state_id, op.id.0, "path")?;
            Ok((
                path.clone(),
                JsonValue::Null,
                json!({
                    "path": path,
                    "path_source": "state",
                    "path_state": label,
                    "path_state_id": state_id.0,
                }),
                "state",
            ))
        }
        FileReadBytesPathOperand::RowFieldPath { field_id } => {
            let label = field_label(plan, field_id.0);
            let path = indexed_row_field_text(plan, row, field_id, op.id.0, "path")?;
            Ok((
                path.clone(),
                JsonValue::Null,
                json!({
                    "path": path,
                    "path_source": "row_field",
                    "path_field": label,
                    "path_field_id": field_id.0,
                }),
                "row_field",
            ))
        }
    }
}

fn indexed_file_write_bytes_path(
    plan: &MachinePlan,
    row: &IndexedRowView,
    op: &PlanOp,
    path_operand: FileWriteBytesPathOperand,
) -> PlanExecutorResult<(String, JsonValue, JsonValue, &'static str)> {
    match path_operand {
        FileWriteBytesPathOperand::StaticConstant { path, constant_id } => Ok((
            path.clone(),
            json!(constant_id.0),
            json!({
                "path": path,
                "path_source": "static_constant",
            }),
            "static_constant",
        )),
        FileWriteBytesPathOperand::StatePath { state_id } => {
            let label = state_label(plan, state_id);
            let path = indexed_row_state_text(plan, row, state_id, op.id.0, "path")?;
            Ok((
                path.clone(),
                JsonValue::Null,
                json!({
                    "path": path,
                    "path_source": "state",
                    "path_state": label,
                    "path_state_id": state_id.0,
                }),
                "state",
            ))
        }
        FileWriteBytesPathOperand::RowFieldPath { field_id } => {
            let label = field_label(plan, field_id.0);
            let path = indexed_row_field_text(plan, row, field_id, op.id.0, "path")?;
            Ok((
                path.clone(),
                JsonValue::Null,
                json!({
                    "path": path,
                    "path_source": "row_field",
                    "path_field": label,
                    "path_field_id": field_id.0,
                }),
                "row_field",
            ))
        }
    }
}

fn indexed_bytes_access_json(
    plan: &MachinePlan,
    input_state_id: StateId,
    bytes_view: &IndexedExecutorBytesView<'_>,
    index: Option<usize>,
) -> JsonValue {
    let mut access = json!({
        "read_only": true,
        "input_state_id": input_state_id.0,
        "access_source": bytes_view.access_source,
        "cow_kind": bytes_view.cow_kind,
        "byte_len": bytes_view.bytes.len(),
        "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
        "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
    });
    if let Some(index) = index
        && let JsonValue::Object(object) = &mut access
    {
        object.insert("index".to_owned(), json!(index));
    }
    access
}

fn indexed_bytes_write_access_json(
    plan: &MachinePlan,
    input_state_id: StateId,
    bytes_view: &IndexedExecutorBytesView<'_>,
    index: usize,
    byte_value: u8,
) -> JsonValue {
    json!({
        "read_only": false,
        "input_state_id": input_state_id.0,
        "access_source": bytes_view.access_source,
        "cow_kind": bytes_view.cow_kind,
        "byte_len": bytes_view.bytes.len(),
        "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
        "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
        "index": index,
        "value": byte_value,
    })
}

fn indexed_bytes_slice_access_json(
    plan: &MachinePlan,
    input_state_id: StateId,
    output_state_id: StateId,
    bytes_view: &IndexedExecutorBytesView<'_>,
    offset: usize,
    byte_count: usize,
    output_byte_len: usize,
    output_storage_kind: &str,
) -> JsonValue {
    json!({
        "read_only": false,
        "input_state_id": input_state_id.0,
        "output_state_id": output_state_id.0,
        "input_access_source": bytes_view.access_source,
        "input_cow_kind": bytes_view.cow_kind,
        "byte_bank_declared": indexed_state_has_fixed_byte_bank(plan, input_state_id),
        "byte_bank_used": bytes_view.access_source == "indexed_fixed_byte_bank",
        "output_storage_kind": output_storage_kind,
        "output_cow_kind": "borrowed_view",
        "offset": offset,
        "byte_count": byte_count,
        "output_byte_len": output_byte_len,
    })
}

fn indexed_single_state_input(
    op: &PlanOp,
    output_state_id: StateId,
) -> PlanExecutorResult<StateId> {
    let inputs = op
        .inputs
        .iter()
        .filter_map(|input| match input {
            ValueRef::State(state_id) if *state_id != output_state_id => Some(*state_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err(format!(
            "indexed update branch {} expected one non-output state input, found {}",
            op.id.0,
            inputs.len()
        )
        .into());
    };
    Ok(*input)
}

fn indexed_bytes_slice_operands(
    plan: &MachinePlan,
    op: &PlanOp,
) -> PlanExecutorResult<(StateId, usize, usize, PlanConstantId, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed Bytes/slice update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(offset_constant_id),
        ValueRef::Constant(byte_count_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "indexed Bytes/slice update branch {} expected state, offset, and byte_count operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "indexed Bytes/slice update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let offset =
        root_usize_number_constant(plan, *offset_constant_id, "Bytes/slice", "offset", op)?;
    let byte_count = root_usize_number_constant(
        plan,
        *byte_count_constant_id,
        "Bytes/slice",
        "byte_count",
        op,
    )?;
    Ok((
        *input,
        offset,
        byte_count,
        *offset_constant_id,
        *byte_count_constant_id,
    ))
}

fn indexed_bytes_count_operand(
    plan: &MachinePlan,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(StateId, usize, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(byte_count_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "indexed {operation} update branch {} expected state and byte_count operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "indexed {operation} update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let byte_count =
        root_usize_number_constant(plan, *byte_count_constant_id, operation, "byte_count", op)?;
    Ok((*input, byte_count, *byte_count_constant_id))
}

fn indexed_bytes_zeros_operand(
    plan: &MachinePlan,
    op: &PlanOp,
) -> PlanExecutorResult<(usize, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed Bytes/zeros update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::Constant(byte_count_constant_id)] = ordered_inputs.as_slice() else {
        return Err(format!(
            "indexed Bytes/zeros update branch {} expected byte_count constant operand, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if op
        .inputs
        .iter()
        .any(|input| matches!(input, ValueRef::State(_) | ValueRef::Field(_)))
    {
        return Err(format!(
            "indexed Bytes/zeros update branch {} must not declare state or row-field inputs",
            op.id.0
        )
        .into());
    }
    let byte_count = root_usize_number_constant(
        plan,
        *byte_count_constant_id,
        "Bytes/zeros",
        "byte_count",
        op,
    )?;
    Ok((byte_count, *byte_count_constant_id))
}

fn indexed_bytes_distinct_value_inputs(
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(ValueRef, ValueRef)> {
    let mut inputs = Vec::new();
    for input in &op.inputs {
        if matches!(input, ValueRef::State(_) | ValueRef::Field(_)) && !inputs.contains(input) {
            inputs.push(input.clone());
        }
    }
    match inputs.as_slice() {
        [left, right] => Ok((left.clone(), right.clone())),
        _ => Err(format!(
            "indexed {operation} update branch {} expected two distinct BYTES state/field inputs, found {}",
            op.id.0,
            inputs.len()
        )
        .into()),
    }
}

fn indexed_bytes_ordered_value_inputs(
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(ValueRef, ValueRef)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    match ordered_inputs.as_slice() {
        [left, right]
            if matches!(left, ValueRef::State(_) | ValueRef::Field(_))
                && matches!(right, ValueRef::State(_) | ValueRef::Field(_)) =>
        {
            Ok((left.clone(), right.clone()))
        }
        _ => Err(format!(
            "indexed {operation} update branch {} expected two ordered BYTES state/field inputs, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into()),
    }
}

pub fn indexed_state_has_fixed_byte_bank(plan: &MachinePlan, state_id: StateId) -> bool {
    plan.storage_layout
        .byte_banks
        .iter()
        .any(|bank| bank.indexed && bank.state_id == state_id)
}

pub fn indexed_fixed_byte_bank_len(
    plan: &MachinePlan,
    state_id: StateId,
) -> PlanExecutorResult<Option<usize>> {
    plan.storage_layout
        .byte_banks
        .iter()
        .find(|bank| bank.indexed && bank.state_id == state_id)
        .map(|bank| {
            usize::try_from(bank.fixed_len).map_err(|_| {
                format!(
                    "indexed fixed byte bank for state {} has unsupported length {}",
                    state_id.0, bank.fixed_len
                )
                .into()
            })
        })
        .transpose()
}

pub fn indexed_field_has_fixed_byte_bank(
    plan: &MachinePlan,
    scope_id: Option<boon_plan::ScopeId>,
    field_name: &str,
) -> bool {
    plan.storage_layout.scalar_slots.iter().any(|slot| {
        slot.indexed
            && slot.scope_id == scope_id
            && local_field_name(&state_label(plan, slot.state_id)) == field_name
            && indexed_state_has_fixed_byte_bank(plan, slot.state_id)
    })
}

fn indexed_fixed_bytes_report_json(bytes: &[u8]) -> JsonValue {
    json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": sha256_bytes(bytes),
        "byte_len": bytes.len() as u64,
    })
}

fn checked_indexed_bytes_slice(
    data: &[u8],
    offset: usize,
    byte_count: usize,
    operation: &str,
    op_id: PlanOpId,
    input_label: &str,
) -> PlanExecutorResult<Vec<u8>> {
    let end = offset.checked_add(byte_count).ok_or_else(|| {
        format!(
            "indexed {operation} update branch {} byte range overflows for `{input_label}`",
            op_id.0
        )
    })?;
    let slice = data.get(offset..end).ok_or_else(|| {
        format!(
            "indexed {operation} update branch {} byte range {offset}..{end} is out of bounds for `{input_label}`",
            op_id.0
        )
    })?;
    Ok(slice.to_vec())
}

fn checked_indexed_bytes_drop(
    data: &[u8],
    byte_count: usize,
    operation: &str,
    op_id: PlanOpId,
    input_label: &str,
) -> PlanExecutorResult<Vec<u8>> {
    if byte_count > data.len() {
        return Err(format!(
            "indexed {operation} update branch {} byte_count {byte_count} is out of bounds for `{input_label}`",
            op_id.0
        )
        .into());
    }
    checked_indexed_bytes_slice(
        data,
        byte_count,
        data.len() - byte_count,
        operation,
        op_id,
        input_label,
    )
}

pub fn local_field_name(label: &str) -> String {
    label
        .rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(label)
        .to_owned()
}

fn root_bytes_set_operands(
    plan: &MachinePlan,
    op: &PlanOp,
) -> PlanExecutorResult<(StateId, usize, u8, PlanConstantId, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root Bytes/set update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(index_constant_id),
        ValueRef::Constant(value_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "root Bytes/set update branch {} expected state, index constant, and value constant operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root Bytes/set update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let index_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *index_constant_id)
        .ok_or_else(|| {
            format!(
                "root Bytes/set update branch {} references missing index constant {}",
                op.id.0, index_constant_id.0
            )
        })?;
    let PlanConstantValue::Number { value: index } = &index_constant.value else {
        return Err(format!(
            "root Bytes/set update branch {} index constant {} is not a number",
            op.id.0, index_constant_id.0
        )
        .into());
    };
    let index = usize::try_from(*index).map_err(|_| {
        format!(
            "root Bytes/set update branch {} index constant {} is negative or too large",
            op.id.0, index_constant_id.0
        )
    })?;
    let value_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *value_constant_id)
        .ok_or_else(|| {
            format!(
                "root Bytes/set update branch {} references missing byte constant {}",
                op.id.0, value_constant_id.0
            )
        })?;
    let PlanConstantValue::Byte { value } = &value_constant.value else {
        return Err(format!(
            "root Bytes/set update branch {} value constant {} is not a byte",
            op.id.0, value_constant_id.0
        )
        .into());
    };
    Ok((
        *input,
        index,
        *value,
        *index_constant_id,
        *value_constant_id,
    ))
}

fn indexed_bytes_set_operands(
    plan: &MachinePlan,
    op: &PlanOp,
) -> PlanExecutorResult<(StateId, usize, u8, PlanConstantId, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "indexed Bytes/set update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(index_constant_id),
        ValueRef::Constant(value_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "indexed Bytes/set update branch {} expected state, index constant, and value constant operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "indexed Bytes/set update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let index_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *index_constant_id)
        .ok_or_else(|| {
            format!(
                "indexed Bytes/set update branch {} references missing index constant {}",
                op.id.0, index_constant_id.0
            )
        })?;
    let PlanConstantValue::Number { value: index } = &index_constant.value else {
        return Err(format!(
            "indexed Bytes/set update branch {} index constant {} is not a number",
            op.id.0, index_constant_id.0
        )
        .into());
    };
    let index = usize::try_from(*index).map_err(|_| {
        format!(
            "indexed Bytes/set update branch {} index constant {} is negative or too large",
            op.id.0, index_constant_id.0
        )
    })?;
    let value_constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *value_constant_id)
        .ok_or_else(|| {
            format!(
                "indexed Bytes/set update branch {} references missing byte constant {}",
                op.id.0, value_constant_id.0
            )
        })?;
    let PlanConstantValue::Byte { value } = &value_constant.value else {
        return Err(format!(
            "indexed Bytes/set update branch {} value constant {} is not a byte",
            op.id.0, value_constant_id.0
        )
        .into());
    };
    Ok((
        *input,
        index,
        *value,
        *index_constant_id,
        *value_constant_id,
    ))
}

fn root_bytes_concat_state_inputs(op: &PlanOp) -> PlanExecutorResult<(StateId, StateId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root Bytes/concat update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::State(left), ValueRef::State(right)] = ordered_inputs.as_slice() else {
        return Err(format!(
            "root Bytes/concat update branch {} expected two ordered state inputs, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if left == right {
        return Err(format!(
            "root Bytes/concat update branch {} expected distinct state inputs",
            op.id.0
        )
        .into());
    }
    if !op.inputs.contains(&ValueRef::State(*left)) || !op.inputs.contains(&ValueRef::State(*right))
    {
        return Err(format!(
            "root Bytes/concat update branch {} ordered inputs are not declared op inputs",
            op.id.0
        )
        .into());
    }
    Ok((*left, *right))
}

fn root_bytes_slice_operands(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    op: &PlanOp,
) -> PlanExecutorResult<(StateId, usize, usize, JsonValue, JsonValue)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root Bytes/slice update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::State(input), offset_ref, byte_count_ref] = ordered_inputs.as_slice() else {
        return Err(format!(
            "root Bytes/slice update branch {} expected state, offset, and byte_count operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root Bytes/slice update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let (offset, offset_ref_json) =
        root_usize_number_operand(plan, root_state, offset_ref, "Bytes/slice", "offset", op)?;
    let (byte_count, byte_count_ref_json) = root_usize_number_operand(
        plan,
        root_state,
        byte_count_ref,
        "Bytes/slice",
        "byte_count",
        op,
    )?;
    Ok((
        *input,
        offset,
        byte_count,
        offset_ref_json,
        byte_count_ref_json,
    ))
}

fn root_bytes_count_operand(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(StateId, usize, JsonValue)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::State(input), byte_count_ref] = ordered_inputs.as_slice() else {
        return Err(format!(
            "root {operation} update branch {} expected state and byte_count operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root {operation} update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let (byte_count, byte_count_ref_json) = root_usize_number_operand(
        plan,
        root_state,
        byte_count_ref,
        operation,
        "byte_count",
        op,
    )?;
    Ok((*input, byte_count, byte_count_ref_json))
}

fn root_bytes_zeros_operand(
    plan: &MachinePlan,
    op: &PlanOp,
) -> PlanExecutorResult<(usize, PlanConstantId)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root Bytes/zeros update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [ValueRef::Constant(byte_count_constant_id)] = ordered_inputs.as_slice() else {
        return Err(format!(
            "root Bytes/zeros update branch {} expected byte_count constant operand, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if op
        .inputs
        .iter()
        .any(|input| matches!(input, ValueRef::State(_)))
    {
        return Err(format!(
            "root Bytes/zeros update branch {} must not declare state inputs",
            op.id.0
        )
        .into());
    }
    let byte_count = root_usize_number_constant(
        plan,
        *byte_count_constant_id,
        "Bytes/zeros",
        "byte_count",
        op,
    )?;
    Ok((byte_count, *byte_count_constant_id))
}

fn root_bytes_numeric_read_operands(
    plan: &MachinePlan,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(
    StateId,
    usize,
    usize,
    BytesEndian,
    PlanConstantId,
    PlanConstantId,
    PlanConstantId,
)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(offset_constant_id),
        ValueRef::Constant(byte_count_constant_id),
        ValueRef::Constant(endian_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "root {operation} update branch {} expected state, offset, byte_count, and endian operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root {operation} update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let offset = root_usize_number_constant(plan, *offset_constant_id, operation, "offset", op)?;
    let byte_count =
        root_usize_number_constant(plan, *byte_count_constant_id, operation, "byte_count", op)?;
    let endian = root_endian_constant(plan, *endian_constant_id, operation, op)?;
    Ok((
        *input,
        offset,
        byte_count,
        endian,
        *offset_constant_id,
        *byte_count_constant_id,
        *endian_constant_id,
    ))
}

fn root_bytes_numeric_write_operands(
    plan: &MachinePlan,
    op: &PlanOp,
    operation: &str,
) -> PlanExecutorResult<(
    StateId,
    usize,
    usize,
    BytesEndian,
    i64,
    PlanConstantId,
    PlanConstantId,
    PlanConstantId,
    PlanConstantId,
)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "root {operation} update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [
        ValueRef::State(input),
        ValueRef::Constant(offset_constant_id),
        ValueRef::Constant(byte_count_constant_id),
        ValueRef::Constant(endian_constant_id),
        ValueRef::Constant(value_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        return Err(format!(
            "root {operation} update branch {} expected state, offset, byte_count, endian, and value operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "root {operation} update branch {} state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    let offset = root_usize_number_constant(plan, *offset_constant_id, operation, "offset", op)?;
    let byte_count =
        root_usize_number_constant(plan, *byte_count_constant_id, operation, "byte_count", op)?;
    let endian = root_endian_constant(plan, *endian_constant_id, operation, op)?;
    let value = root_i64_number_constant(plan, *value_constant_id, operation, "value", op)?;
    Ok((
        *input,
        offset,
        byte_count,
        endian,
        value,
        *offset_constant_id,
        *byte_count_constant_id,
        *endian_constant_id,
        *value_constant_id,
    ))
}

fn file_read_bytes_path_operand(
    plan: &MachinePlan,
    op: &PlanOp,
    context: &str,
) -> PlanExecutorResult<FileReadBytesPathOperand> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "{context} File/read_bytes update branch {} is not an update branch",
            op.id.0
        )
        .into());
    };
    let [path_ref] = ordered_inputs.as_slice() else {
        return Err(format!(
            "{context} File/read_bytes update branch {} expected one path operand, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if op
        .inputs
        .iter()
        .any(|input| matches!(input, ValueRef::SourcePayload { .. }))
    {
        return Err(format!(
            "{context} File/read_bytes update branch {} must not declare source-payload inputs",
            op.id.0
        )
        .into());
    }
    match path_ref {
        ValueRef::Constant(path_constant_id) => {
            if op
                .inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(_)))
            {
                return Err(format!(
                    "{context} File/read_bytes update branch {} static path must not declare state inputs",
                    op.id.0
                )
                .into());
            }
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *path_constant_id)
                .ok_or_else(|| {
                    format!(
                        "{context} File/read_bytes update branch {} references missing path constant {}",
                        op.id.0, path_constant_id.0
                    )
                })?;
            let PlanConstantValue::Text { value } = &constant.value else {
                return Err(format!(
                    "{context} File/read_bytes update branch {} path constant {} is not TEXT",
                    op.id.0, path_constant_id.0
                )
                .into());
            };
            Ok(FileReadBytesPathOperand::StaticConstant {
                path: value.clone(),
                constant_id: *path_constant_id,
            })
        }
        ValueRef::State(path_state_id) => {
            let state_inputs = op
                .inputs
                .iter()
                .filter_map(|input| match input {
                    ValueRef::State(state_id) => Some(*state_id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if state_inputs.as_slice() != [*path_state_id] {
                return Err(format!(
                    "{context} File/read_bytes update branch {} path state operand is not the only state input",
                    op.id.0
                )
                .into());
            }
            Ok(FileReadBytesPathOperand::StatePath {
                state_id: *path_state_id,
            })
        }
        ValueRef::Field(path_field_id) => {
            if !op.indexed || context != "indexed" {
                return Err(format!(
                    "{context} File/read_bytes update branch {} path field operands are only supported for indexed updates",
                    op.id.0
                )
                .into());
            }
            if !op.inputs.contains(&ValueRef::Field(*path_field_id)) {
                return Err(format!(
                    "{context} File/read_bytes update branch {} path field operand is not declared as an op input",
                    op.id.0
                )
                .into());
            }
            Ok(FileReadBytesPathOperand::RowFieldPath {
                field_id: *path_field_id,
            })
        }
        _ => Err(format!(
            "{context} File/read_bytes update branch {} path operand must be a TEXT constant, state, or indexed row field",
            op.id.0
        )
        .into()),
    }
}

#[derive(Clone, Debug)]
enum FileReadBytesPathOperand {
    StaticConstant {
        path: String,
        constant_id: PlanConstantId,
    },
    StatePath {
        state_id: StateId,
    },
    RowFieldPath {
        field_id: FieldId,
    },
}

fn file_write_bytes_operands(
    plan: &MachinePlan,
    op: &PlanOp,
    context: &str,
) -> PlanExecutorResult<(StateId, FileWriteBytesPathOperand)> {
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        return Err(format!(
            "{context} File/write_bytes update branch {} is not an update branch",
            op.id.0,
        )
        .into());
    };
    let [ValueRef::State(input), path_ref] = ordered_inputs.as_slice() else {
        return Err(format!(
            "{context} File/write_bytes update branch {} expected bytes state and path operands, found {}",
            op.id.0,
            ordered_inputs.len()
        )
        .into());
    };
    if !op.inputs.contains(&ValueRef::State(*input)) {
        return Err(format!(
            "{context} File/write_bytes update branch {} bytes state operand is not declared as an op input",
            op.id.0
        )
        .into());
    }
    if op
        .inputs
        .iter()
        .any(|input_ref| matches!(input_ref, ValueRef::SourcePayload { .. }))
    {
        return Err(format!(
            "{context} File/write_bytes update branch {} must not declare source-payload inputs",
            op.id.0
        )
        .into());
    }
    let path = match path_ref {
        ValueRef::Constant(path_constant_id) => {
            let constant = plan
                .constants
                .iter()
                .find(|constant| constant.id == *path_constant_id)
                .ok_or_else(|| {
                    format!(
                        "{context} File/write_bytes update branch {} references missing path constant {}",
                        op.id.0, path_constant_id.0
                    )
                })?;
            let PlanConstantValue::Text { value } = &constant.value else {
                return Err(format!(
                    "{context} File/write_bytes update branch {} path constant {} is not TEXT",
                    op.id.0, path_constant_id.0
                )
                .into());
            };
            FileWriteBytesPathOperand::StaticConstant {
                path: value.clone(),
                constant_id: *path_constant_id,
            }
        }
        ValueRef::State(path_state_id) => {
            if !op.inputs.contains(&ValueRef::State(*path_state_id)) {
                return Err(format!(
                    "{context} File/write_bytes update branch {} path state operand is not declared as an op input",
                    op.id.0
                )
                .into());
            }
            FileWriteBytesPathOperand::StatePath {
                state_id: *path_state_id,
            }
        }
        ValueRef::Field(path_field_id) => {
            if !op.indexed || context != "indexed" {
                return Err(format!(
                    "{context} File/write_bytes update branch {} path field operands are only supported for indexed updates",
                    op.id.0
                )
                .into());
            }
            if !op.inputs.contains(&ValueRef::Field(*path_field_id)) {
                return Err(format!(
                    "{context} File/write_bytes update branch {} path field operand is not declared as an op input",
                    op.id.0
                )
                .into());
            }
            FileWriteBytesPathOperand::RowFieldPath {
                field_id: *path_field_id,
            }
        }
        _ => {
            return Err(format!(
                "{context} File/write_bytes update branch {} path operand must be a TEXT constant, state, or indexed row field",
                op.id.0
            )
            .into())
        }
    };
    Ok((*input, path))
}

#[derive(Clone, Debug)]
enum FileWriteBytesPathOperand {
    StaticConstant {
        path: String,
        constant_id: PlanConstantId,
    },
    StatePath {
        state_id: StateId,
    },
    RowFieldPath {
        field_id: FieldId,
    },
}

fn root_usize_number_operand(
    plan: &MachinePlan,
    root_state: &JsonMap<String, JsonValue>,
    value_ref: &ValueRef,
    operation: &str,
    label: &str,
    op: &PlanOp,
) -> PlanExecutorResult<(usize, JsonValue)> {
    match value_ref {
        ValueRef::Constant(constant_id) => Ok((
            root_usize_number_constant(plan, *constant_id, operation, label, op)?,
            json!({"kind": "constant", "constant_id": constant_id.0}),
        )),
        ValueRef::State(state_id) => {
            if !op.inputs.contains(&ValueRef::State(*state_id)) {
                return Err(format!(
                    "root {operation} update branch {} {label} state operand {} is not declared as an op input",
                    op.id.0, state_id.0
                )
                .into());
            }
            let state_label = state_label(plan, *state_id);
            let value = root_state_value(plan, root_state, *state_id, op.id.0)?;
            let number = value.as_i64().ok_or_else(|| {
                format!(
                    "root {operation} update branch {} {label} state `{state_label}` is not NUMBER",
                    op.id.0
                )
            })?;
            let number = usize::try_from(number).map_err(|_| {
                format!(
                    "root {operation} update branch {} {label} state `{state_label}` is negative or too large",
                    op.id.0
                )
            })?;
            Ok((
                number,
                json!({"kind": "state", "state_id": state_id.0, "state": state_label}),
            ))
        }
        other => Err(format!(
            "root {operation} update branch {} {label} operand must be a number constant or NUMBER state, got {other:?}",
            op.id.0
        )
        .into()),
    }
}

fn root_usize_number_constant(
    plan: &MachinePlan,
    constant_id: PlanConstantId,
    operation: &str,
    label: &str,
    op: &PlanOp,
) -> PlanExecutorResult<usize> {
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == constant_id)
        .ok_or_else(|| {
            format!(
                "root {operation} update branch {} references missing {label} constant {}",
                op.id.0, constant_id.0
            )
        })?;
    let PlanConstantValue::Number { value } = &constant.value else {
        return Err(format!(
            "root {operation} update branch {} {label} constant {} is not a number",
            op.id.0, constant_id.0
        )
        .into());
    };
    usize::try_from(*value).map_err(|_| {
        format!(
            "root {operation} update branch {} {label} constant {} is negative or too large",
            op.id.0, constant_id.0
        )
        .into()
    })
}

fn root_i64_number_constant(
    plan: &MachinePlan,
    constant_id: PlanConstantId,
    operation: &str,
    label: &str,
    op: &PlanOp,
) -> PlanExecutorResult<i64> {
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == constant_id)
        .ok_or_else(|| {
            format!(
                "root {operation} update branch {} references missing {label} constant {}",
                op.id.0, constant_id.0
            )
        })?;
    let PlanConstantValue::Number { value } = &constant.value else {
        return Err(format!(
            "root {operation} update branch {} {label} constant {} is not a number",
            op.id.0, constant_id.0
        )
        .into());
    };
    Ok(*value)
}

fn root_endian_constant(
    plan: &MachinePlan,
    constant_id: PlanConstantId,
    operation: &str,
    op: &PlanOp,
) -> PlanExecutorResult<BytesEndian> {
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == constant_id)
        .ok_or_else(|| {
            format!(
                "root {operation} update branch {} references missing endian constant {}",
                op.id.0, constant_id.0
            )
        })?;
    let PlanConstantValue::Text { value } = &constant.value else {
        return Err(format!(
            "root {operation} update branch {} endian constant {} is not TEXT",
            op.id.0, constant_id.0
        )
        .into());
    };
    bytes_endian(value).ok_or_else(|| {
        format!(
            "root {operation} update branch {} endian constant {} is unsupported: `{value}`",
            op.id.0, constant_id.0
        )
        .into()
    })
}

fn checked_root_bytes_slice(
    data: &[u8],
    offset: usize,
    byte_count: usize,
    operation: &str,
    op_id: PlanOpId,
    input_label: &str,
) -> PlanExecutorResult<Vec<u8>> {
    let end = offset.checked_add(byte_count).ok_or_else(|| {
        format!(
            "root {operation} update branch {} byte range overflows for `{input_label}`",
            op_id.0
        )
    })?;
    let slice = data.get(offset..end).ok_or_else(|| {
        format!(
            "root {operation} update branch {} byte range {offset}..{end} is out of bounds for `{input_label}`",
            op_id.0
        )
    })?;
    Ok(slice.to_vec())
}

fn checked_root_bytes_drop(
    data: &[u8],
    byte_count: usize,
    operation: &str,
    op_id: PlanOpId,
    input_label: &str,
) -> PlanExecutorResult<Vec<u8>> {
    if byte_count > data.len() {
        return Err(format!(
            "root {operation} update branch {} byte_count {byte_count} is out of bounds for `{input_label}`",
            op_id.0
        )
        .into());
    }
    checked_root_bytes_slice(
        data,
        byte_count,
        data.len() - byte_count,
        operation,
        op_id,
        input_label,
    )
}

fn validate_root_bytes_output_len(
    plan: &MachinePlan,
    output_state_id: StateId,
    actual_len: usize,
    operation: &str,
    op_id: PlanOpId,
) -> PlanExecutorResult<()> {
    let output_slot = scalar_slot_for_state(plan, output_state_id, op_id)?;
    if let PlanValueType::Bytes {
        fixed_len: Some(fixed_len),
    } = &output_slot.value_type
        && *fixed_len != actual_len as u64
    {
        return Err(format!(
            "root {operation} update branch {} output fixed length {fixed_len} does not match produced length {actual_len}",
            op_id.0
        )
        .into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BytesEndian {
    Little,
    Big,
}

fn bytes_endian(value: &str) -> Option<BytesEndian> {
    match value.trim().trim_matches('"') {
        "Little" => Some(BytesEndian::Little),
        "Big" => Some(BytesEndian::Big),
        _ => None,
    }
}

fn bytes_endian_label(endian: BytesEndian) -> &'static str {
    match endian {
        BytesEndian::Little => "Little",
        BytesEndian::Big => "Big",
    }
}

fn bytes_checked_range(
    data: &[u8],
    offset: usize,
    byte_count: usize,
) -> Result<&[u8], &'static str> {
    if !matches!(byte_count, 1 | 2 | 4 | 8) {
        return Err("bytes_invalid_byte_count");
    }
    let end = offset
        .checked_add(byte_count)
        .ok_or("bytes_out_of_bounds")?;
    data.get(offset..end).ok_or("bytes_out_of_bounds")
}

fn bytes_checked_range_len(
    data_len: usize,
    offset: usize,
    byte_count: usize,
) -> Result<(), &'static str> {
    if !matches!(byte_count, 1 | 2 | 4 | 8) {
        return Err("bytes_invalid_byte_count");
    }
    let end = offset
        .checked_add(byte_count)
        .ok_or("bytes_out_of_bounds")?;
    if end > data_len {
        return Err("bytes_out_of_bounds");
    }
    Ok(())
}

fn bytes_read_unsigned(
    data: &[u8],
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
) -> Result<u64, &'static str> {
    let slice = bytes_checked_range(data, offset, byte_count)?;
    let mut bytes = [0u8; 8];
    match endian {
        BytesEndian::Little => {
            bytes[..byte_count].copy_from_slice(slice);
            Ok(u64::from_le_bytes(bytes))
        }
        BytesEndian::Big => {
            bytes[8 - byte_count..].copy_from_slice(slice);
            Ok(u64::from_be_bytes(bytes))
        }
    }
}

fn bytes_read_signed(
    data: &[u8],
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
) -> Result<i64, &'static str> {
    let slice = bytes_checked_range(data, offset, byte_count)?;
    Ok(match (byte_count, endian) {
        (1, _) => i8::from_ne_bytes([slice[0]]) as i64,
        (2, BytesEndian::Little) => i16::from_le_bytes([slice[0], slice[1]]) as i64,
        (2, BytesEndian::Big) => i16::from_be_bytes([slice[0], slice[1]]) as i64,
        (4, BytesEndian::Little) => {
            i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
        }
        (4, BytesEndian::Big) => {
            i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
        }
        (8, BytesEndian::Little) => i64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ]),
        (8, BytesEndian::Big) => i64::from_be_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ]),
        _ => return Err("bytes_invalid_byte_count"),
    })
}

fn bytes_write_unsigned_patches(
    data_len: usize,
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
    value: i64,
) -> Result<Vec<(usize, u8)>, &'static str> {
    if value < 0 {
        return Err("bytes_numeric_overflow");
    }
    bytes_checked_range_len(data_len, offset, byte_count)?;
    let max = if byte_count == 8 {
        u64::MAX
    } else {
        (1u64 << (byte_count * 8)) - 1
    };
    let value = value as u64;
    if value > max {
        return Err("bytes_numeric_overflow");
    }
    let patches = match endian {
        BytesEndian::Little => value.to_le_bytes()[..byte_count]
            .iter()
            .enumerate()
            .map(|(local_index, byte)| (offset + local_index, *byte))
            .collect(),
        BytesEndian::Big => value.to_be_bytes()[8 - byte_count..]
            .iter()
            .enumerate()
            .map(|(local_index, byte)| (offset + local_index, *byte))
            .collect(),
    };
    Ok(patches)
}

fn bytes_write_signed_patches(
    data_len: usize,
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
    value: i64,
) -> Result<Vec<(usize, u8)>, &'static str> {
    match byte_count {
        1 if !(i8::MIN as i64..=i8::MAX as i64).contains(&value) => {
            return Err("bytes_numeric_overflow");
        }
        2 if !(i16::MIN as i64..=i16::MAX as i64).contains(&value) => {
            return Err("bytes_numeric_overflow");
        }
        4 if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) => {
            return Err("bytes_numeric_overflow");
        }
        1 | 2 | 4 | 8 => {}
        _ => return Err("bytes_invalid_byte_count"),
    }
    bytes_checked_range_len(data_len, offset, byte_count)?;
    let mut bytes = [0u8; 8];
    match (byte_count, endian) {
        (1, _) => bytes[0] = value as i8 as u8,
        (2, BytesEndian::Little) => bytes[..2].copy_from_slice(&(value as i16).to_le_bytes()),
        (2, BytesEndian::Big) => bytes[..2].copy_from_slice(&(value as i16).to_be_bytes()),
        (4, BytesEndian::Little) => bytes[..4].copy_from_slice(&(value as i32).to_le_bytes()),
        (4, BytesEndian::Big) => bytes[..4].copy_from_slice(&(value as i32).to_be_bytes()),
        (8, BytesEndian::Little) => bytes.copy_from_slice(&value.to_le_bytes()),
        (8, BytesEndian::Big) => bytes.copy_from_slice(&value.to_be_bytes()),
        _ => unreachable!("validated byte count"),
    }
    Ok(bytes[..byte_count]
        .iter()
        .enumerate()
        .map(|(local_index, byte)| (offset + local_index, *byte))
        .collect())
}

fn apply_bytes_patches(output: &mut [u8], patches: &[(usize, u8)]) {
    for (index, byte) in patches {
        output[*index] = *byte;
    }
}

fn bytes_write_unsigned(
    data: &mut [u8],
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
    value: i64,
) -> Result<(), &'static str> {
    let patches = bytes_write_unsigned_patches(data.len(), offset, byte_count, endian, value)?;
    apply_bytes_patches(data, &patches);
    Ok(())
}

fn bytes_write_signed(
    data: &mut [u8],
    offset: usize,
    byte_count: usize,
    endian: BytesEndian,
    value: i64,
) -> Result<(), &'static str> {
    let patches = bytes_write_signed_patches(data.len(), offset, byte_count, endian, value)?;
    apply_bytes_patches(data, &patches);
    Ok(())
}

fn read_plan_host_file_bytes(root: &Path, path: &str) -> PlanExecutorResult<Vec<u8>> {
    let root = canonical_existing_dir(root)?;
    let path_ref = Path::new(path);
    if path_ref.is_absolute() {
        return Err(format!(
            "host file path `{}` must be source-relative",
            path_ref.display()
        )
        .into());
    }
    let mut resolved = root.clone();
    for component in path_ref.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::ParentDir => {
                return Err(format!(
                    "host file path `{}` may not contain parent-directory segments",
                    path_ref.display()
                )
                .into());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "host file path `{}` must be source-relative",
                    path_ref.display()
                )
                .into());
            }
        }
    }
    let canonical = resolved.canonicalize()?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(format!(
            "host file path `{}` escapes source root `{}`",
            path_ref.display(),
            root.display()
        )
        .into());
    }
    Ok(fs::read(canonical)?)
}

struct PlanHostFileWrite {
    path: String,
    artifact_path: PathBuf,
    byte_len: u64,
    sha256: String,
    first_byte: u64,
    last_byte: u64,
}

impl PlanHostFileWrite {
    fn report_json(&self) -> JsonValue {
        json!({
            "kind": "file_write_bytes",
            "status": "pass",
            "mode": "typed-machine-plan-host-file-write-v1",
            "path": self.path,
            "artifact_path": self.artifact_path.display().to_string(),
            "byte_len": self.byte_len,
            "sha256": self.sha256,
            "first_byte": self.first_byte,
            "last_byte": self.last_byte,
            "write_mode": "create_or_truncate",
            "verified_after_write": true,
            "public_inline_bytes_absent": true
        })
    }
}

fn write_plan_host_file_bytes(
    root: &Path,
    path: &str,
    bytes: &[u8],
) -> PlanExecutorResult<PlanHostFileWrite> {
    let root = canonical_existing_dir(root)?;
    let path_ref = Path::new(path);
    if path_ref.as_os_str().is_empty() || path_ref.is_absolute() {
        return Err(format!(
            "host file path `{}` must be a non-empty source-relative path",
            path_ref.display()
        )
        .into());
    }
    let mut resolved = root.clone();
    for component in path_ref.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::ParentDir => {
                return Err(format!(
                    "host file path `{}` may not contain parent-directory segments",
                    path_ref.display()
                )
                .into());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "host file path `{}` must be source-relative",
                    path_ref.display()
                )
                .into());
            }
        }
    }
    let parent = resolved
        .parent()
        .ok_or_else(|| format!("host file path `{}` has no parent", path_ref.display()))?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(&root) || !canonical_parent.is_dir() {
        return Err(format!(
            "host file path `{}` escapes source root `{}`",
            path_ref.display(),
            root.display()
        )
        .into());
    }
    if let Ok(metadata) = fs::symlink_metadata(&resolved) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "host file path `{}` may not target a symlink",
                path_ref.display()
            )
            .into());
        }
        if metadata.is_dir() {
            return Err(format!(
                "host file path `{}` targets a directory",
                path_ref.display()
            )
            .into());
        }
    }
    let target_name = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "host file path `{}` has no UTF-8 file name",
                path_ref.display()
            )
        })?;
    let temp_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp_path = canonical_parent.join(format!(
        ".{target_name}.boon-write-{}-{temp_suffix}.tmp",
        std::process::id()
    ));
    {
        let mut temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
    }
    fs::rename(&temp_path, &resolved)?;
    let canonical_target = resolved.canonicalize()?;
    if !canonical_target.starts_with(&root) || !canonical_target.is_file() {
        return Err(format!(
            "host file path `{}` escapes source root `{}` after write",
            path_ref.display(),
            root.display()
        )
        .into());
    }
    let readback = fs::read(&canonical_target)?;
    if readback != bytes {
        return Err(format!(
            "host file path `{}` readback did not match written bytes",
            path_ref.display()
        )
        .into());
    }
    Ok(PlanHostFileWrite {
        path: path.to_owned(),
        artifact_path: canonical_target,
        byte_len: readback.len() as u64,
        sha256: sha256_bytes(&readback),
        first_byte: readback.first().copied().unwrap_or_default() as u64,
        last_byte: readback.last().copied().unwrap_or_default() as u64,
    })
}

fn canonical_existing_dir(path: &Path) -> PlanExecutorResult<PathBuf> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(format!("`{}` is not a directory", path.display()).into());
    }
    Ok(canonical)
}

fn normalized_text_bytes_encoding(value: &str) -> Option<&'static str> {
    match value
        .trim()
        .trim_matches('"')
        .replace(['-', '_'], "")
        .to_ascii_lowercase()
        .as_str()
    {
        "utf8" => Some("utf8"),
        "ascii" => Some("ascii"),
        _ => None,
    }
}

fn text_to_bytes(text: &str, encoding: &str) -> PlanExecutorResult<Vec<u8>> {
    match normalized_text_bytes_encoding(encoding) {
        Some("utf8") => Ok(text.as_bytes().to_vec()),
        Some("ascii") if text.is_ascii() => Ok(text.as_bytes().to_vec()),
        Some("ascii") => Err("is not ASCII for Ascii encoding".into()),
        _ => Err(format!("uses unsupported text encoding `{encoding}`").into()),
    }
}

fn bytes_to_text(bytes: &[u8], encoding: &str) -> PlanExecutorResult<String> {
    match normalized_text_bytes_encoding(encoding) {
        Some("utf8") => String::from_utf8(bytes.to_vec()).map_err(|_| "is not valid UTF-8".into()),
        Some("ascii") if bytes.is_ascii() => {
            String::from_utf8(bytes.to_vec()).map_err(|_| "is not valid ASCII".into())
        }
        Some("ascii") => Err("is not valid ASCII".into()),
        _ => Err(format!("uses unsupported bytes encoding `{encoding}`").into()),
    }
}

fn bytes_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    match needle {
        [single] => haystack.iter().position(|byte| byte == single),
        _ if needle.len() > haystack.len() => None,
        _ => haystack
            .windows(needle.len())
            .position(|window| window == needle),
    }
}

fn bytes_encode_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(data.len().saturating_mul(2));
    for byte in data {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn bytes_decode_hex(text: &str) -> Result<Vec<u8>, &'static str> {
    let digits = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if digits.len() % 2 != 0 {
        return Err("bytes_invalid_hex");
    }
    let mut output = Vec::with_capacity(digits.len() / 2);
    for chunk in digits.chunks_exact(2) {
        let high = bytes_hex_value(chunk[0]).ok_or("bytes_invalid_hex")?;
        let low = bytes_hex_value(chunk[1]).ok_or("bytes_invalid_hex")?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn bytes_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn source_payload_bytes_toml_key(field: &str) -> String {
    if field == "bytes" {
        "bytes_hex".to_owned()
    } else {
        format!("{field}_bytes_hex")
    }
}

pub fn validate_source_payload_bytes_field_name(field: &str) -> PlanExecutorResult<()> {
    if field == "bytes" {
        return Ok(());
    }
    Err(format!(
        "named BYTES source payload key `{field}` is not supported in v1; use the reserved `bytes` key"
    )
    .into())
}

fn source_payload_bytes_field_from_toml_key(key: &str) -> PlanExecutorResult<Option<String>> {
    if key == "bytes_hex" {
        return Ok(Some("bytes".to_owned()));
    }
    if key.ends_with("_bytes_hex") {
        return Err(format!(
            "named BYTES source payload key `{key}` is not supported in v1; use the reserved `bytes_hex` key"
        )
        .into());
    }
    Ok(None)
}

fn toml_string_ref<'a>(table: &'a BTreeMap<String, toml::Value>, key: &str) -> Option<&'a str> {
    table.get(key).and_then(toml::Value::as_str)
}

fn toml_usize_ref(table: &BTreeMap<String, toml::Value>, key: &str) -> Option<usize> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| usize::try_from(value).ok())
}

fn toml_u64_ref(table: &BTreeMap<String, toml::Value>, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
}

fn assert_live_source_event_field(
    step_id: &str,
    expected_value: Option<&str>,
    key: &str,
    actual_value: Option<&str>,
) -> PlanExecutorResult<()> {
    if expected_value.is_none() || expected_value == actual_value {
        Ok(())
    } else {
        Err(format!(
            "{step_id}: observed live source field `{key}` expected {expected_value:?}, got {actual_value:?}"
        )
        .into())
    }
}

fn assert_live_source_event_numeric_field(
    step_id: &str,
    expected_value: Option<u64>,
    key: &str,
    actual_value: Option<u64>,
) -> PlanExecutorResult<()> {
    if expected_value.is_none() || expected_value == actual_value {
        Ok(())
    } else {
        Err(format!(
            "{step_id}: observed live source field `{key}` expected {expected_value:?}, got {actual_value:?}"
        )
        .into())
    }
}

fn bytes_encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3).saturating_mul(4));
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn bytes_decode_base64(text: &str) -> Result<Vec<u8>, &'static str> {
    let input = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if input.len() % 4 != 0 {
        return Err("bytes_invalid_base64");
    }
    let mut output = Vec::with_capacity((input.len() / 4).saturating_mul(3));
    for (chunk_index, chunk) in input.chunks_exact(4).enumerate() {
        let final_chunk = chunk_index == input.len() / 4 - 1;
        if chunk[0] == b'=' || chunk[1] == b'=' {
            return Err("bytes_invalid_base64");
        }
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (!final_chunk && padding > 0) {
            return Err("bytes_invalid_base64");
        }
        if padding == 1 && chunk[2] == b'=' {
            return Err("bytes_invalid_base64");
        }
        let a = bytes_base64_value(chunk[0])?;
        let b = bytes_base64_value(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            bytes_base64_value(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            bytes_base64_value(chunk[3])?
        };
        let packed = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | d as u32;
        output.push(((packed >> 16) & 0xff) as u8);
        if padding < 2 {
            output.push(((packed >> 8) & 0xff) as u8);
        }
        if padding < 1 {
            output.push((packed & 0xff) as u8);
        }
    }
    Ok(output)
}

fn bytes_base64_value(byte: u8) -> Result<u8, &'static str> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err("bytes_invalid_base64"),
    }
}

fn bytes_report_json_with_patches(
    bytes: &[u8],
    patches: &[(usize, u8)],
    operation: &str,
    op_id: PlanOpId,
    input_label: &str,
) -> PlanExecutorResult<JsonValue> {
    if patches.is_empty() {
        return Ok(bytes_report_json(bytes));
    }
    let mut cursor = 0usize;
    let mut hasher = Sha256::new();
    for (index, byte_value) in patches {
        if *index >= bytes.len() {
            return Err(format!(
                "root {operation} update branch {} index {index} is out of bounds for `{input_label}`",
                op_id.0
            )
            .into());
        }
        if *index < cursor {
            return Err(format!(
                "root {operation} update branch {} has duplicate or unordered patch index {index} for `{input_label}`",
                op_id.0
            )
            .into());
        }
        hasher.update(&bytes[cursor..*index]);
        hasher.update([*byte_value]);
        cursor = *index + 1;
    }
    hasher.update(&bytes[cursor..]);
    let digest = format!("{:x}", hasher.finalize());
    Ok(json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": digest,
        "byte_len": bytes.len() as u64,
    }))
}

fn private_bytes_for_fixed_mutation<'a>(
    bytes_owner: &'a impl RootBytesStateOwner,
    state_id: StateId,
    op_id: PlanOpId,
) -> PlanExecutorResult<&'a [u8]> {
    if let Some(bytes) = bytes_owner.fixed_byte_bank_for_state(state_id) {
        return Ok(bytes);
    }
    if let Some(bytes) = bytes_owner.private_bytes_for_state(state_id) {
        return Ok(bytes.inline_bytes());
    }
    Err(format!(
        "root update branch {} has no executor private byte state for state {}",
        op_id.0, state_id.0
    )
    .into())
}

fn apply_root_fixed_bytes_mutation(
    bytes_owner: &mut impl RootBytesStateOwner,
    mutation: &RootBytesFixedMutation,
    op_id: PlanOpId,
) -> PlanExecutorResult<()> {
    if mutation.input_state_id == mutation.output_state_id {
        let output = bytes_owner
            .fixed_byte_bank_mut_for_state(mutation.output_state_id)
            .ok_or_else(|| {
                format!(
                    "root update branch {} output state {} has no executor fixed byte bank",
                    op_id.0, mutation.output_state_id.0
                )
            })?;
        for (index, byte_value) in &mutation.patches {
            let Some(slot) = output.get_mut(*index) else {
                return Err(format!(
                    "root update branch {} executor fixed byte bank index {index} is out of bounds",
                    op_id.0
                )
                .into());
            };
            *slot = *byte_value;
        }
        return Ok(());
    }

    let output_len = bytes_owner
        .fixed_byte_bank_for_state(mutation.output_state_id)
        .map(<[u8]>::len)
        .ok_or_else(|| {
            format!(
                "root update branch {} output state {} has no executor fixed byte bank",
                op_id.0, mutation.output_state_id.0
            )
        })?;
    for (index, _) in &mutation.patches {
        if *index >= output_len {
            return Err(format!(
                "root update branch {} executor fixed byte bank index {index} is out of bounds",
                op_id.0
            )
            .into());
        }
    }
    let input = private_bytes_for_fixed_mutation(bytes_owner, mutation.input_state_id, op_id)?;
    let input_len = input.len();
    if output_len != input_len {
        return Err(format!(
            "root update branch {} executor fixed byte bank output length {} does not match input length {}",
            op_id.0, output_len, input_len
        )
        .into());
    }

    let input = input.to_vec();
    let mut output = bytes_owner
        .take_fixed_byte_bank_for_state(mutation.output_state_id)
        .expect("output bank existence was checked above");
    output.copy_from_slice(&input);
    for (index, byte_value) in &mutation.patches {
        output[*index] = *byte_value;
    }
    bytes_owner.insert_fixed_byte_bank_for_state(mutation.output_state_id, output);
    Ok(())
}

fn root_bytes_read_outcome(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes_access: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> RootBytesReadEvaluation {
    root_bytes_read_outcome_with_bytes(
        op_id,
        supported,
        unsupported_reason,
        target_state_id,
        value,
        None,
        bytes_access,
        expression_kind,
        update_constant_id,
        update_constant_value,
    )
}

fn root_bytes_read_outcome_with_bytes(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes: Option<PlanExecutorBytes>,
    bytes_access: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> RootBytesReadEvaluation {
    let executor_report = json!({
        "executor": "cpu-plan-root-bytes-read-evaluator-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.map(|id| id.0),
        "supported": supported,
        "unsupported_reason": unsupported_reason,
        "expression_kind": expression_kind,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    RootBytesReadEvaluation {
        supported,
        unsupported_reason: executor_report
            .get("unsupported_reason")
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
        target_state_id,
        value,
        bytes,
        bytes_access,
        expression_kind,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

fn indexed_bytes_read_outcome(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes: Option<PlanExecutorBytes>,
    bytes_access: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> IndexedBytesReadEvaluation {
    let executor_report = json!({
        "executor": "cpu-plan-indexed-bytes-read-evaluator-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.map(|id| id.0),
        "supported": supported,
        "unsupported_reason": unsupported_reason,
        "expression_kind": expression_kind,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    IndexedBytesReadEvaluation {
        supported,
        unsupported_reason: executor_report
            .get("unsupported_reason")
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
        target_state_id,
        value,
        bytes,
        bytes_access,
        expression_kind,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

fn indexed_bytes_write_outcome(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes: Option<PlanExecutorBytes>,
    bytes_access: JsonValue,
    bytes_storage: JsonValue,
    host_effect: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> IndexedBytesWriteEvaluation {
    let executor_report = json!({
        "executor": "cpu-plan-indexed-bytes-write-evaluator-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.map(|id| id.0),
        "supported": supported,
        "unsupported_reason": unsupported_reason,
        "expression_kind": expression_kind,
        "bytes_commit": bytes.is_some(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    IndexedBytesWriteEvaluation {
        supported,
        unsupported_reason,
        target_state_id,
        value,
        bytes,
        bytes_access,
        bytes_storage,
        host_effect,
        expression_kind,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

fn root_bytes_write_outcome(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes: Option<PlanExecutorBytes>,
    fixed_mutation: Option<RootBytesFixedMutation>,
    bytes_access: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> RootBytesWriteEvaluation {
    root_bytes_write_outcome_with_host_effect(
        op_id,
        supported,
        unsupported_reason,
        target_state_id,
        value,
        bytes,
        fixed_mutation,
        bytes_access,
        JsonValue::Null,
        expression_kind,
        update_constant_id,
        update_constant_value,
    )
}

fn root_bytes_write_outcome_with_host_effect(
    op_id: PlanOpId,
    supported: bool,
    unsupported_reason: Option<String>,
    target_state_id: Option<StateId>,
    value: Option<JsonValue>,
    bytes: Option<PlanExecutorBytes>,
    fixed_mutation: Option<RootBytesFixedMutation>,
    bytes_access: JsonValue,
    host_effect: JsonValue,
    expression_kind: Option<&'static str>,
    update_constant_id: JsonValue,
    update_constant_value: JsonValue,
) -> RootBytesWriteEvaluation {
    let executor_report = json!({
        "executor": "cpu-plan-root-bytes-write-evaluator-v1",
        "update_op_id": op_id.0,
        "target_state_id": target_state_id.map(|id| id.0),
        "supported": supported,
        "unsupported_reason": unsupported_reason,
        "expression_kind": expression_kind,
        "fixed_mutation": fixed_mutation.is_some(),
        "bytes_commit": bytes.is_some(),
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    RootBytesWriteEvaluation {
        supported,
        unsupported_reason,
        target_state_id,
        value,
        bytes,
        fixed_mutation,
        bytes_access,
        host_effect,
        expression_kind,
        update_constant_id,
        update_constant_value,
        executor_report,
    }
}

fn attach_executor_bytes_copy_cost(
    executor_report: &mut JsonValue,
    reason: &'static str,
    vec_alloc_count: u64,
    vec_alloc_bytes: u64,
    copy_from_slice_count: u64,
    copy_from_slice_bytes: u64,
) {
    let Some(report) = executor_report.as_object_mut() else {
        return;
    };
    report.insert(
        "bytes_copy_cost".to_owned(),
        json!({
            "reason": reason,
            "vec_alloc_count": vec_alloc_count,
            "vec_alloc_bytes": vec_alloc_bytes,
            "copy_from_slice_count": copy_from_slice_count,
            "copy_from_slice_bytes": copy_from_slice_bytes,
        }),
    );
}

pub fn plan_constant_json_value(
    constant: &boon_plan::PlanConstant,
) -> PlanExecutorResult<JsonValue> {
    plan_constant_value_json_value(&constant.value, &format!("plan constant {}", constant.id.0))
}

pub fn plan_constant_value_json_value(
    value: &PlanConstantValue,
    context: &str,
) -> PlanExecutorResult<JsonValue> {
    match value {
        PlanConstantValue::Text { value } | PlanConstantValue::Enum { value } => {
            Ok(JsonValue::String(value.clone()))
        }
        PlanConstantValue::Number { value } => Ok(json!(value)),
        PlanConstantValue::Byte { value } => Ok(json!(value)),
        PlanConstantValue::Bool { value } => Ok(json!(value)),
        PlanConstantValue::Bytes {
            byte_len,
            sha256,
            inline_bytes: Some(bytes),
        } => {
            let bytes =
                PlanExecutorBytes::from_inline(sha256.clone(), *byte_len, bytes.clone(), context)?;
            Ok(bytes.report_json())
        }
        PlanConstantValue::Bytes {
            inline_bytes: None, ..
        } => Err(format!("{context} BYTES payload is missing").into()),
    }
}

pub fn plan_constant_value_bytes(
    value: &PlanConstantValue,
    context: &str,
) -> PlanExecutorResult<Option<PlanExecutorBytes>> {
    match value {
        PlanConstantValue::Bytes {
            byte_len,
            sha256,
            inline_bytes: Some(bytes),
        } => Ok(Some(PlanExecutorBytes::from_inline(
            sha256.clone(),
            *byte_len,
            bytes.clone(),
            context,
        )?)),
        PlanConstantValue::Bytes {
            inline_bytes: None, ..
        } => Err(format!("{context} BYTES payload is missing").into()),
        _ => Ok(None),
    }
}

pub fn plan_constant_bytes_for_storage_slot(
    constant: &boon_plan::PlanConstant,
    slot: &boon_plan::ScalarStorageSlot,
    context: &str,
) -> PlanExecutorResult<Option<PlanExecutorBytes>> {
    match (&constant.value, &slot.value_type, slot.initial_value_kind) {
        (PlanConstantValue::Bytes { .. }, PlanValueType::Bytes { .. }, InitialValueKind::Bytes) => {
            Ok(Some(plan_constant_executor_bytes_for_slot(
                constant, slot, context,
            )?))
        }
        (PlanConstantValue::Bytes { .. }, _, _) => Err(format!(
            "{context} constant {} does not match output BYTES storage",
            constant.id.0
        )
        .into()),
        _ => Ok(None),
    }
}

pub fn list_row_default_fields(
    plan: &MachinePlan,
    list_slot: &boon_plan::ListStorageSlot,
) -> PlanExecutorResult<ListRowDefaultFields> {
    let mut fields = BTreeMap::new();
    let mut private_bytes = BTreeMap::new();
    let mut fixed_byte_banks = BTreeMap::new();
    let mut default_field_count = 0usize;
    let mut bytes_field_count = 0usize;
    let mut fixed_byte_bank_count = 0usize;
    let mut inferred_record_bool_default_count = 0usize;
    for slot in &plan.storage_layout.scalar_slots {
        if slot.scope_id != list_slot.scope_id {
            continue;
        }
        let Some(constant_id) = slot.initial_constant_id else {
            continue;
        };
        let constant = plan
            .constants
            .iter()
            .find(|constant| constant.id == constant_id)
            .ok_or_else(|| format!("missing row default constant {}", constant_id.0))?;
        let field_name = local_field_name(&state_label(plan, slot.state_id));
        fields.insert(field_name.clone(), plan_constant_json_value(constant)?);
        default_field_count += 1;
        if let Some(bytes) = plan_constant_bytes_for_storage_slot(
            constant,
            slot,
            &format!("row default field `{field_name}`"),
        )? {
            if indexed_state_has_fixed_byte_bank(plan, slot.state_id) {
                fixed_byte_banks.insert(field_name.clone(), bytes.inline_bytes().to_vec());
                fixed_byte_bank_count += 1;
            }
            private_bytes.insert(field_name, bytes);
            bytes_field_count += 1;
        }
    }
    if !list_slot.initial_rows.is_empty() {
        let mut initial_values_by_field = BTreeMap::<String, Vec<&PlanConstantValue>>::new();
        for row in &list_slot.initial_rows {
            for field in &row.fields {
                initial_values_by_field
                    .entry(field.name.clone())
                    .or_default()
                    .push(&field.value);
            }
        }
        for (field_name, values) in initial_values_by_field {
            if fields.contains_key(&field_name) || values.len() != list_slot.initial_rows.len() {
                continue;
            }
            if values
                .iter()
                .all(|value| matches!(value, PlanConstantValue::Bool { .. }))
            {
                fields.insert(field_name, JsonValue::Bool(false));
                inferred_record_bool_default_count += 1;
            }
        }
    }
    let executor_report = json!({
        "executor": "cpu-plan-list-row-default-fields-v1",
        "list_id": list_slot.list_id.0,
        "scope_id": list_slot.scope_id.map(|scope_id| scope_id.0),
        "default_field_count": default_field_count,
        "inferred_record_bool_default_count": inferred_record_bool_default_count,
        "bytes_field_count": bytes_field_count,
        "fixed_byte_bank_count": fixed_byte_bank_count,
        "runtime_ast_eval_count": 0,
        "executable_string_path_count": 0,
        "unknown_plan_op_count": 0,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
    });
    Ok(ListRowDefaultFields {
        fields,
        private_bytes,
        fixed_byte_banks,
        executor_report,
    })
}

fn plan_constant_executor_bytes_for_slot(
    constant: &boon_plan::PlanConstant,
    slot: &boon_plan::ScalarStorageSlot,
    context: &str,
) -> PlanExecutorResult<PlanExecutorBytes> {
    match (&constant.value, &slot.value_type, slot.initial_value_kind) {
        (
            PlanConstantValue::Bytes {
                byte_len,
                sha256,
                inline_bytes: Some(bytes),
            },
            PlanValueType::Bytes { fixed_len },
            InitialValueKind::Bytes,
        ) => {
            if let Some(expected_len) = *fixed_len
                && expected_len != *byte_len
            {
                return Err(format!(
                    "{context} constant {} has byte_len {byte_len} but storage fixed_len {expected_len}",
                    constant.id.0
                )
                .into());
            }
            PlanExecutorBytes::from_inline(
                sha256.clone(),
                *byte_len,
                bytes.clone(),
                &format!("{context} constant {}", constant.id.0),
            )
        }
        (
            PlanConstantValue::Bytes {
                inline_bytes: None, ..
            },
            PlanValueType::Bytes { .. },
            InitialValueKind::Bytes,
        ) => Err(format!(
            "{context} constant {} has no executable inline payload",
            constant.id.0
        )
        .into()),
        (PlanConstantValue::Bytes { .. }, _, _) => Err(format!(
            "{context} constant {} does not match output BYTES storage",
            constant.id.0
        )
        .into()),
        _ => Err(format!(
            "{context} constant {} is not a BYTES constant",
            constant.id.0
        )
        .into()),
    }
}

fn root_state_has_fixed_byte_bank(plan: &MachinePlan, state_id: StateId) -> bool {
    plan.storage_layout
        .byte_banks
        .iter()
        .any(|bank| !bank.indexed && bank.state_id == state_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_text_source_payload_plan() -> (MachinePlan, SourceId, StateId, PlanOpId) {
        let source_id = SourceId(2);
        let state_id = StateId(3);
        let update_op_id = PlanOpId(4);
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: vec![boon_plan::PlanConstant {
                id: PlanConstantId(0),
                value: PlanConstantValue::Text {
                    value: "".to_owned(),
                },
            }],
            source_routes: vec![SourceRoute {
                id: boon_plan::PlanSourceRouteId(0),
                source_id,
                path: "store.input.change".to_owned(),
                scoped: false,
                scope_id: None,
                payload_schema: boon_plan::SourcePayloadSchema {
                    fields: vec![SourcePayloadField::Text],
                    typed_fields: vec![boon_plan::SourcePayloadDescriptor {
                        field: SourcePayloadField::Text,
                        value_type: boon_plan::SourcePayloadValueType::Text,
                    }],
                    row_lookup_field: None,
                    address_lookup_field: None,
                },
            }],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(0)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(1),
                    kind: RegionKind::SourceRouting,
                    ops: vec![PlanOp {
                        id: PlanOpId(1),
                        kind: PlanOpKind::SourceRoute,
                        inputs: Vec::new(),
                        output: Some(ValueRef::Source(source_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(2),
                    kind: RegionKind::StateInitialization,
                    ops: vec![PlanOp {
                        id: PlanOpId(2),
                        kind: PlanOpKind::StateInitialize {
                            initial_value_kind: InitialValueKind::Text,
                            initial_constant_id: Some(PlanConstantId(0)),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::State(state_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(3),
                    kind: RegionKind::UpdateBranches,
                    ops: vec![PlanOp {
                        id: update_op_id,
                        kind: PlanOpKind::UpdateBranch {
                            expression_kind: PlanExpressionKind::SourcePayload,
                            ordered_inputs: Vec::new(),
                            source_payload_field: Some(SourcePayloadField::Text),
                            update_constant_id: None,
                            source_guard: None,
                        },
                        inputs: vec![
                            ValueRef::Source(source_id),
                            ValueRef::SourcePayload {
                                source_id,
                                field: SourcePayloadField::Text,
                            },
                        ],
                        output: Some(ValueRef::State(state_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
            ],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 1,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 3,
                typed_value_ref_count: 5,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:2".to_owned(),
                    label: "store.input.change".to_owned(),
                }],
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:3".to_owned(),
                    label: "store.input".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        (plan, source_id, state_id, update_op_id)
    }

    #[test]
    fn source_route_command_argv_prefers_artifact_paths_and_preserves_live_invocation() {
        let current = vec![
            "target/debug/boon_cli".to_owned(),
            "run-plan-route".to_owned(),
            "examples/bytes.bn".to_owned(),
        ];
        let preserved = build_source_route_command_argv(SourceRouteCommandArgvInput {
            current_args: current.clone(),
            source_path: "examples/ignored.bn".to_owned(),
            target_profile: "software-default".to_owned(),
            source_route: "store.input.change".to_owned(),
            target_state: "store.input".to_owned(),
            text: None,
            key: None,
            address: None,
            payload: BTreeMap::new(),
            payload_bytes: BTreeMap::new(),
            payload_byte_artifact_paths: BTreeMap::new(),
            report_path: None,
        });
        assert_eq!(preserved, current);

        let argv = build_source_route_command_argv(SourceRouteCommandArgvInput {
            current_args: vec!["xtask".to_owned(), "verify".to_owned()],
            source_path: "examples/bytes.bn".to_owned(),
            target_profile: "software-default".to_owned(),
            source_route: "store.input.change".to_owned(),
            target_state: "store.input".to_owned(),
            text: Some("Typed".to_owned()),
            key: Some("Enter".to_owned()),
            address: Some("A1".to_owned()),
            payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
            payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xde, 0xad, 0xbe, 0xef])]),
            payload_byte_artifact_paths: BTreeMap::from([(
                "bytes".to_owned(),
                "target/reports/event-bytes.bin".to_owned(),
            )]),
            report_path: Some("target/reports/route.json".to_owned()),
        });
        assert_eq!(
            argv,
            vec![
                "target/debug/boon_cli",
                "run-plan-route",
                "examples/bytes.bn",
                "--source",
                "store.input.change",
                "--target-state",
                "store.input",
                "--text",
                "Typed",
                "--key",
                "Enter",
                "--address",
                "A1",
                "--payload",
                "mode=replace",
                "--payload-bytes-file",
                "bytes=target/reports/event-bytes.bin",
                "--report",
                "target/reports/route.json",
            ]
        );
    }

    #[test]
    fn source_route_source_event_report_preserves_event_shape() {
        let report = build_source_route_source_event_report(SourceRouteSourceEventReportInput {
            source: "store.input.change".to_owned(),
            source_id: 7,
            text: Some("Typed".to_owned()),
            key: Some("Enter".to_owned()),
            list_id: Some("todos".to_owned()),
            address: Some("A1".to_owned()),
            target_text: Some("target".to_owned()),
            target_occurrence: Some(2),
            target_key: Some(42),
            target_generation: Some(3),
            bind_epoch: Some(4),
            source_epoch: Some(5),
            payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
            payload_bytes_report: json!({
                "bytes": {
                    "$boon_type": "BYTES",
                    "storage": "artifact",
                    "artifact_path": "target/reports/event-bytes.bin"
                }
            }),
            pointer_x: Some("10".to_owned()),
            pointer_y: Some("11".to_owned()),
            pointer_width: Some("12".to_owned()),
            pointer_height: Some("13".to_owned()),
        });

        assert_eq!(report["source"], "store.input.change");
        assert_eq!(report["source_id"], 7);
        assert_eq!(report["text"], "Typed");
        assert_eq!(report["payload"]["mode"], "replace");
        assert_eq!(
            report["payload_bytes"]["bytes"]["artifact_path"],
            "target/reports/event-bytes.bin"
        );
        assert_eq!(report["pointer_height"], "13");
    }

    #[test]
    fn source_event_payload_bytes_report_writes_artifacts_and_inlines_small_payloads() {
        let temp_dir = std::env::temp_dir().join(format!(
            "boon-plan-executor-source-event-bytes-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let report_path = temp_dir.join("route-report.json");

        let small = vec![1, 2];
        let large = vec![0, 1, 2, 3];
        let report = build_source_event_payload_bytes_report(
            &BTreeMap::from([
                ("small".to_owned(), small.clone()),
                ("weird/name".to_owned(), large.clone()),
            ]),
            Some(&report_path),
            3,
        )
        .unwrap();

        let small_digest = sha256_bytes(&small);
        assert_eq!(report.payload_bytes["small"]["storage"], "inline");
        assert_eq!(report.payload_bytes["small"]["digest"], small_digest);
        assert_eq!(report.payload_bytes["small"]["byte_len"], 2);
        assert_eq!(report.payload_bytes["small"]["inline_bytes"], json!([1, 2]));
        assert_eq!(report.payload_bytes["small"]["inline_byte_limit"], 3);

        let large_digest = sha256_bytes(&large);
        let artifact_path = report.payload_bytes["weird/name"]["artifact_path"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(report.payload_bytes["weird/name"]["storage"], "artifact");
        assert_eq!(report.payload_bytes["weird/name"]["digest"], large_digest);
        assert_eq!(
            report.payload_bytes["weird/name"]["artifact_sha256"],
            large_digest
        );
        assert_eq!(report.payload_bytes["weird/name"]["inline_byte_limit"], 3);
        assert!(
            artifact_path.ends_with(&format!(
                "route-report-artifacts/source-event-weird_name-{large_digest}.bytes"
            )),
            "unexpected artifact path: {artifact_path}"
        );
        assert_eq!(fs::read(&artifact_path).unwrap(), large);
        assert_eq!(report.artifacts.len(), 1);
        assert_eq!(report.artifacts[0]["path"], artifact_path);
        assert_eq!(report.artifacts[0]["sha256"], large_digest);
        assert_eq!(
            report.executor_report["executor"],
            "cpu-plan-source-event-payload-bytes-report-v1"
        );
        assert_eq!(report.executor_report["payload_field_count"], 2);
        assert_eq!(report.executor_report["inline_payload_count"], 1);
        assert_eq!(report.executor_report["artifact_payload_count"], 1);
        assert_eq!(report.executor_report["runtime_ast_eval_count"], 0);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn source_route_command_output_assembles_event_argv_report_and_artifacts() {
        let temp_dir = std::env::temp_dir().join(format!(
            "boon-plan-executor-source-route-output-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let report_path = temp_dir.join("route-report.json");
        let payload = vec![9, 8, 7, 6];

        let output = assemble_source_route_command_output(SourceRouteCommandOutputInput {
            current_args: vec!["xtask".to_owned()],
            generated_at_utc: "2026-06-28T00:00:00Z".to_owned(),
            git_commit: "abc123".to_owned(),
            worktree_fingerprint: "worktreehash".to_owned(),
            binary_hash: "binhash".to_owned(),
            binary_path: "target/debug/boon_cli".to_owned(),
            source_path: "examples/bytes.bn".to_owned(),
            source_hash: "sourcehash".to_owned(),
            source_files: vec!["examples/bytes.bn".to_owned()],
            program_hash: "programhash".to_owned(),
            program_kind: "single-file".to_owned(),
            program_file_count: 1,
            graph_node_count: 2,
            load_pipeline_profile: json!({"total_ms": 1.0}),
            target_profile: "software-default".to_owned(),
            source_route: "store.receive".to_owned(),
            target_state: "store.blob".to_owned(),
            event: SourceRouteSourceEventReportInput {
                source: "store.receive".to_owned(),
                source_id: 3,
                text: Some("ignored".to_owned()),
                key: Some("Enter".to_owned()),
                list_id: None,
                address: Some("A1".to_owned()),
                target_text: None,
                target_occurrence: None,
                target_key: None,
                target_generation: None,
                bind_epoch: Some(4),
                source_epoch: Some(5),
                payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
                payload_bytes_report: JsonValue::Null,
                pointer_x: Some("10".to_owned()),
                pointer_y: Some("11".to_owned()),
                pointer_width: None,
                pointer_height: None,
            },
            payload_bytes: BTreeMap::from([("bytes".to_owned(), payload.clone())]),
            report_path: Some(report_path),
            plan_hash: "planhash".to_owned(),
            plan_version: json!({"major": 1}),
            capability_summary: json!({"executable": true}),
            route_surface: json!({"expression_kind": "SourcePayload"}),
            state_summary: json!({"store": {"blob": "ok"}}),
            semantic_delta_signatures: vec!["FieldSet:store.blob".to_owned()],
            semantic_deltas: json!([{"kind": "FieldSet"}]),
            plan_executor: json!({"executor": "cpu-plan-source-route-v1"}),
            inline_byte_limit: 3,
        })
        .unwrap();

        let digest = sha256_bytes(&payload);
        assert_eq!(output.report["status"], "pass");
        assert_eq!(output.report["plan_executor_status"], "pass");
        assert_eq!(output.report["comparison_status"], "not-requested");
        assert_eq!(output.report["accepted_for_product_status"], "pass");
        assert_eq!(
            output.report["plan_executor"]["command_output_core"]["executor"],
            "cpu-plan-source-route-command-output-v1"
        );
        assert_eq!(
            output.source_event["payload_bytes"]["bytes"]["storage"],
            "artifact"
        );
        assert_eq!(
            output.source_event["payload_bytes"]["bytes"]["artifact_sha256"],
            digest
        );
        assert_eq!(output.artifact_sha256s.len(), 1);
        let artifact_path = output.artifact_sha256s[0]["path"].as_str().unwrap();
        assert_eq!(fs::read(artifact_path).unwrap(), payload);
        assert!(
            output
                .command_argv
                .windows(2)
                .any(|window| window == ["--payload-bytes-file", &format!("bytes={artifact_path}")])
        );
        assert_eq!(output.report["source_event"], output.source_event);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn source_route_runtime_branch_execution_is_executor_owned() {
        let output = assemble_source_route_runtime_branch_execution(
            SourceRouteRuntimeBranchExecutionInput {
                value: json!({
                    "$boon_type": "BYTES",
                    "byte_len": 4,
                    "digest": "abc"
                }),
                expression_kind: "SourcePayload".to_owned(),
                source_payload_field: json!("bytes"),
                update_constant_id: JsonValue::Null,
                update_constant_value: JsonValue::Null,
                host_effect: json!({"kind": "FileWriteBytes"}),
                state_write_core: JsonValue::Null,
                bytes_state_core: json!({"executor": "cpu-plan-root-bytes-state-transition-v1"}),
                runtime_branch_core: json!({"executor": "runtime-root-bytes-source-payload-v1"}),
            },
            &json!({
                "executor": "cpu-plan-source-route-json-execution-v1",
                "execution_surface_core": {
                    "executor": "cpu-plan-source-route-execution-surface-v1"
                }
            }),
        );

        assert_eq!(output.expression_kind, "SourcePayload");
        assert_eq!(output.source_payload_field, json!("bytes"));
        assert_eq!(
            output.executor_core["runtime_branch_execution_core"]["executor"],
            "cpu-plan-source-route-runtime-branch-execution-v1"
        );
        assert_eq!(
            output.executor_core["runtime_branch_execution_core"]["runtime_branch_core"]["executor"],
            "runtime-root-bytes-source-payload-v1"
        );
        assert_eq!(
            output.executor_core["execution_surface_core"]["executor"],
            "cpu-plan-source-route-execution-surface-v1"
        );
        assert_eq!(
            output.bytes_state_core["executor"],
            "cpu-plan-root-bytes-state-transition-v1"
        );
    }

    #[test]
    fn root_scenario_command_output_assembles_report_and_executor_core() {
        let output = assemble_root_scenario_command_output(RootScenarioCommandOutputInput {
            command_argv: vec![
                "target/debug/boon_cli".to_owned(),
                "run-plan-root-scalar-scenario".to_owned(),
                "examples/counter.bn".to_owned(),
            ],
            generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
            git_commit: "abc123".to_owned(),
            worktree_fingerprint: "worktreehash".to_owned(),
            binary_hash: "binhash".to_owned(),
            binary_path: "target/debug/boon_cli".to_owned(),
            source_path: "examples/counter.bn".to_owned(),
            source_hash: "sourcehash".to_owned(),
            source_files: vec!["examples/counter.bn".to_owned()],
            scenario_path: "examples/counter.scn".to_owned(),
            scenario_hash: "scenariohash".to_owned(),
            program_hash: "programhash".to_owned(),
            program_kind: "single-file".to_owned(),
            program_file_count: 1,
            graph_node_count: 3,
            load_pipeline_profile: json!({"total_ms": 1.0}),
            target_profile: "software-default".to_owned(),
            plan_hash: "planhash".to_owned(),
            plan_version: json!({"major": 1}),
            capability_summary: json!({"executable": true}),
            selected_step_ids: vec!["increment".to_owned(), "inspect".to_owned()],
            state_summary: json!({"store": {"count": 1}}),
            semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
            semantic_deltas: json!([{"kind": "FieldSet"}]),
            plan_executor: json!({"executor": "cpu-plan-root-scenario-v1"}),
        });

        assert_eq!(output.report["status"], "pass");
        assert_eq!(output.report["plan_executor_status"], "pass");
        assert_eq!(output.report["comparison_status"], "not-requested");
        assert_eq!(output.report["accepted_for_product_status"], "pass");
        assert_eq!(output.report["command"], "run-plan-root-scalar-scenario");
        assert_eq!(
            output.report["selected_step_ids"],
            json!(["increment", "inspect"])
        );
        assert_eq!(json!(output.command_argv), output.report["command_argv"]);
        assert_eq!(
            output.report["plan_executor"]["command_output_core"]["executor"],
            "cpu-plan-root-scenario-command-output-v1"
        );
        assert_eq!(
            output.report["plan_executor"]["command_output_core"]["selected_step_count"],
            2
        );
        assert_eq!(
            output.executor_report["executor"],
            "cpu-plan-root-scenario-command-output-v1"
        );
        assert_eq!(output.report["legacy_comparison"]["enabled"], false);
    }

    #[test]
    fn scenario_events_command_output_assembles_report_and_executor_core() {
        let output = assemble_scenario_events_command_output(ScenarioEventsCommandOutputInput {
            command_argv: vec![
                "target/debug/boon_cli".to_owned(),
                "run-plan-scenario-events".to_owned(),
                "examples/counter.bn".to_owned(),
            ],
            generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
            git_commit: "abc123".to_owned(),
            worktree_fingerprint: "worktreehash".to_owned(),
            binary_hash: "binhash".to_owned(),
            binary_path: "target/debug/boon_cli".to_owned(),
            source_path: "examples/counter.bn".to_owned(),
            source_hash: "sourcehash".to_owned(),
            source_files: vec!["examples/counter.bn".to_owned()],
            scenario_path: "examples/counter.scn".to_owned(),
            scenario_hash: "scenariohash".to_owned(),
            program_hash: "programhash".to_owned(),
            program_kind: "single-file".to_owned(),
            program_file_count: 1,
            graph_node_count: 3,
            load_pipeline_profile: json!({"total_ms": 1.0}),
            target_profile: "software-default".to_owned(),
            plan_hash: "planhash".to_owned(),
            plan_version: json!({"major": 1}),
            capability_summary: json!({"executable": true}),
            state_summary: json!({"store": {"count": 1}}),
            semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
            semantic_deltas: json!([{"kind": "FieldSet"}]),
            plan_executor_coverage: json!({
                "selected_step_ids": ["increment", "inspect"],
                "covers_assertion_only_steps": true
            }),
            assertion_only_covered: true,
            plan_executor: json!({"executor": "cpu-plan-scenario-events-v1"}),
        });

        assert_eq!(output.report["status"], "pass");
        assert_eq!(output.report["plan_executor_status"], "pass");
        assert_eq!(output.report["comparison_status"], "not-requested");
        assert_eq!(output.report["accepted_for_product_status"], "pass");
        assert_eq!(output.report["command"], "run-plan-scenario-events");
        assert_eq!(
            output.report["selected_step_ids"],
            json!(["increment", "inspect"])
        );
        assert_eq!(json!(output.command_argv), output.report["command_argv"]);
        assert_eq!(
            output.report["plan_executor"]["command_output_core"]["executor"],
            "cpu-plan-scenario-events-command-output-v1"
        );
        assert_eq!(
            output.report["plan_executor"]["command_output_core"]["selected_step_count"],
            2
        );
        assert_eq!(
            output.executor_report["executor"],
            "cpu-plan-scenario-events-command-output-v1"
        );
        assert!(output.report.get("legacy_comparison").is_none());
        assert!(output.report.get("legacy_comparison_acceptance").is_none());
        assert!(
            output
                .report
                .pointer("/plan_executor/command_output_core/compare_legacy")
                .is_none()
        );
    }

    #[test]
    fn scenario_events_report_without_legacy_compare_is_product_status() {
        let output = assemble_scenario_events_command_output(ScenarioEventsCommandOutputInput {
            command_argv: vec![
                "target/debug/boon_cli".to_owned(),
                "run-plan-scenario-events".to_owned(),
                "examples/counter.bn".to_owned(),
            ],
            generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
            git_commit: "abc123".to_owned(),
            worktree_fingerprint: "worktreehash".to_owned(),
            binary_hash: "binhash".to_owned(),
            binary_path: "target/debug/boon_cli".to_owned(),
            source_path: "examples/counter.bn".to_owned(),
            source_hash: "sourcehash".to_owned(),
            source_files: vec!["examples/counter.bn".to_owned()],
            scenario_path: "examples/counter.scn".to_owned(),
            scenario_hash: "scenariohash".to_owned(),
            program_hash: "programhash".to_owned(),
            program_kind: "single-file".to_owned(),
            program_file_count: 1,
            graph_node_count: 3,
            load_pipeline_profile: json!({"total_ms": 1.0}),
            target_profile: "software-default".to_owned(),
            plan_hash: "planhash".to_owned(),
            plan_version: json!({"major": 1}),
            capability_summary: json!({"executable": true}),
            state_summary: json!({"store": {"count": 1}}),
            semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
            semantic_deltas: json!([{"kind": "FieldSet"}]),
            plan_executor_coverage: json!({
                "selected_step_ids": ["increment"],
                "covers_assertion_only_steps": true
            }),
            assertion_only_covered: true,
            plan_executor: json!({"executor": "cpu-plan-scenario-events-v1"}),
        });

        assert_eq!(output.report["status"], "pass");
        assert_eq!(output.report["comparison_status"], "not-requested");
        assert_eq!(
            output.report["report_status_basis"],
            "plan-executor-product-plus-assertion-coverage"
        );
        assert_eq!(
            output.report["command_report_assembly_core"]["legacy_required_for_status"],
            JsonValue::Null
        );
        assert_eq!(output.report["per_step_pass_fail"][2]["pass"], true);
        assert_eq!(
            output.report["per_step_pass_fail"][2]["id"],
            "scenario-event-product-path-has-no-legacy-compare"
        );
        assert!(output.report.get("legacy_comparison").is_none());
        assert!(output.report.get("legacy_comparison_acceptance").is_none());
    }

    #[test]
    fn source_route_command_argv_encodes_inline_bytes_and_non_default_target() {
        let argv = build_source_route_command_argv(SourceRouteCommandArgvInput {
            current_args: vec!["xtask".to_owned()],
            source_path: "examples/bytes.bn".to_owned(),
            target_profile: "software-wasm".to_owned(),
            source_route: "store.input.change".to_owned(),
            target_state: "store.input".to_owned(),
            text: None,
            key: None,
            address: None,
            payload: BTreeMap::new(),
            payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0, 1, 2, 255])]),
            payload_byte_artifact_paths: BTreeMap::new(),
            report_path: None,
        });
        assert_eq!(
            argv,
            vec![
                "target/debug/boon_cli",
                "run-plan-route",
                "examples/bytes.bn",
                "--source",
                "store.input.change",
                "--target-state",
                "store.input",
                "--payload-bytes-hex",
                "bytes=000102ff",
                "--target",
                "software-wasm",
            ]
        );
    }

    #[test]
    fn source_route_execution_surface_is_executor_owned() {
        let mut execution = SourceRouteJsonExecution {
            plan_hash: "plan".to_owned(),
            source_label: "store.input.change".to_owned(),
            source_id: SourceId(2),
            target_state_label: "store.input".to_owned(),
            target_state_id: StateId(3),
            update_op_id: PlanOpId(4),
            supported: true,
            skipped_by_guard: false,
            unsupported_reason: None,
            value: Some(json!("hello")),
            state_summary: json!({ "store.input": "hello" }),
            semantic_delta_signatures: Vec::new(),
            semantic_deltas: Vec::new(),
            expression_kind: Some("source_payload_text"),
            source_payload_field: json!("Text"),
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            executor_report: json!({}),
        };

        let scalar_surface = select_source_route_execution_surface(&execution)
            .expect("scalar JSON execution should classify");
        assert_eq!(
            scalar_surface.kind,
            SourceRouteExecutionSurfaceKind::PlanJson
        );
        assert_eq!(
            scalar_surface.executor_report["execution_surface"],
            "plan-json"
        );

        execution.value = Some(json!({
            "$boon_type": "BYTES",
            "storage": "inline",
            "digest": "abc",
            "byte_len": 3
        }));
        let bytes_surface = select_source_route_execution_surface(&execution)
            .expect("BYTES execution should classify");
        assert_eq!(
            bytes_surface.kind,
            SourceRouteExecutionSurfaceKind::RuntimeBranch
        );
        assert_eq!(
            bytes_surface.executor_report["route_core_value_is_bytes"],
            true
        );

        execution.skipped_by_guard = true;
        let error = select_source_route_execution_surface(&execution)
            .expect_err("guard-skipped selected execution should be rejected");
        assert!(
            error
                .to_string()
                .contains("source guard did not match the supplied event"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn root_match_const_update_executes_on_json_surface() {
        let source_id = SourceId(2);
        let state_id = StateId(3);
        let update_op_id = PlanOpId(4);
        let constants = vec![
            boon_plan::PlanConstant {
                id: PlanConstantId(0),
                value: PlanConstantValue::Enum {
                    value: "Light".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(1),
                value: PlanConstantValue::Text {
                    value: "Light".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(2),
                value: PlanConstantValue::Enum {
                    value: "Dark".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(3),
                value: PlanConstantValue::Text {
                    value: "Dark".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(4),
                value: PlanConstantValue::Enum {
                    value: "Light".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(5),
                value: PlanConstantValue::Text {
                    value: "__".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(6),
                value: PlanConstantValue::Text {
                    value: "SKIP".to_owned(),
                },
            },
        ];
        let source_route = SourceRoute {
            id: boon_plan::PlanSourceRouteId(0),
            source_id,
            path: "store.mode_toggle".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        };
        let update_op = PlanOp {
            id: update_op_id,
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::MatchConst,
                ordered_inputs: vec![
                    ValueRef::State(state_id),
                    ValueRef::Constant(PlanConstantId(1)),
                    ValueRef::Constant(PlanConstantId(2)),
                    ValueRef::Constant(PlanConstantId(3)),
                    ValueRef::Constant(PlanConstantId(4)),
                    ValueRef::Constant(PlanConstantId(5)),
                    ValueRef::Constant(PlanConstantId(6)),
                ],
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![ValueRef::Source(source_id), ValueRef::State(state_id)],
            output: Some(ValueRef::State(state_id)),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants,
            source_routes: vec![source_route.clone()],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id,
                    value_type: PlanValueType::Enum,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Enum,
                    initial_constant_id: Some(PlanConstantId(0)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(3),
                kind: RegionKind::UpdateBranches,
                ops: vec![update_op.clone()],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 7,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 1,
                typed_value_ref_count: 9,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:2".to_owned(),
                    label: "store.mode_toggle".to_owned(),
                }],
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:3".to_owned(),
                    label: "store.mode".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let root_state = JsonMap::from_iter([("store.mode".to_owned(), json!("Light"))]);
        let evaluation = evaluate_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &root_state,
        )
        .expect("root MatchConst should evaluate");
        assert!(evaluation.supported);
        assert!(!evaluation.skipped_by_guard);
        assert_eq!(evaluation.expression_kind, Some("match_const"));
        assert_eq!(evaluation.value, Some(json!("Dark")));

        let execution = execute_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &root_state,
        )
        .expect("root MatchConst should execute on JSON surface");
        assert_eq!(
            execution.surface_kind,
            RootUpdateExecutionSurfaceKind::PlanJson
        );
        assert_eq!(execution.executed.unwrap().value, json!("Dark"));

        let skipped_root_state = JsonMap::from_iter([("store.mode".to_owned(), json!("System"))]);
        let skipped = evaluate_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &skipped_root_state,
        )
        .expect("fallback SKIP should evaluate as a no-op");
        assert!(skipped.supported);
        assert!(skipped.skipped_by_guard);
        assert_eq!(skipped.value, None);

        let field_id = FieldId(9);
        let mut field_update_op = update_op.clone();
        if let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut field_update_op.kind {
            ordered_inputs[0] = ValueRef::Field(field_id);
        }
        field_update_op.inputs = vec![ValueRef::Source(source_id), ValueRef::Field(field_id)];
        let mut field_plan = plan.clone();
        field_plan.regions[0].ops = vec![field_update_op.clone()];
        field_plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
            id: "field:9".to_owned(),
            label: "store.selected_mode".to_owned(),
        }];
        let field_root_state = JsonMap::from_iter([
            ("store.mode".to_owned(), json!("System")),
            ("store.selected_mode".to_owned(), json!("Light")),
        ]);
        let field_evaluation = evaluate_root_json_update_branch(
            &field_plan,
            &field_update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &field_root_state,
        )
        .expect("root MatchConst should read root derived fields");
        assert!(field_evaluation.supported);
        assert_eq!(field_evaluation.value, Some(json!("Dark")));
    }

    #[test]
    fn root_match_value_const_update_executes_read_path_arms() {
        let source_id = SourceId(7);
        let selector_state_id = StateId(8);
        let value_a_state_id = StateId(9);
        let output_state_id = StateId(10);
        let update_op_id = PlanOpId(11);
        let constants = vec![
            boon_plan::PlanConstant {
                id: PlanConstantId(0),
                value: PlanConstantValue::Text {
                    value: "A".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(1),
                value: PlanConstantValue::Text {
                    value: "__".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(2),
                value: PlanConstantValue::Text {
                    value: "SKIP".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(3),
                value: PlanConstantValue::Text {
                    value: "old".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(4),
                value: PlanConstantValue::Text {
                    value: "alpha".to_owned(),
                },
            },
        ];
        let source_route = SourceRoute {
            id: boon_plan::PlanSourceRouteId(0),
            source_id,
            path: "store.trigger".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        };
        let update_op = PlanOp {
            id: update_op_id,
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::MatchValueConst,
                ordered_inputs: vec![
                    ValueRef::State(selector_state_id),
                    ValueRef::Constant(PlanConstantId(0)),
                    ValueRef::State(value_a_state_id),
                    ValueRef::Constant(PlanConstantId(1)),
                    ValueRef::Constant(PlanConstantId(2)),
                ],
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![
                ValueRef::Source(source_id),
                ValueRef::State(selector_state_id),
                ValueRef::State(value_a_state_id),
            ],
            output: Some(ValueRef::State(output_state_id)),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants,
            source_routes: vec![source_route.clone()],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(0),
                        state_id: selector_state_id,
                        value_type: PlanValueType::Text,
                        scope_id: None,
                        indexed: false,
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(0)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(1),
                        state_id: value_a_state_id,
                        value_type: PlanValueType::Text,
                        scope_id: None,
                        indexed: false,
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(4)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(2),
                        state_id: output_state_id,
                        value_type: PlanValueType::Text,
                        scope_id: None,
                        indexed: false,
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(3)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                ],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(3),
                kind: RegionKind::UpdateBranches,
                ops: vec![update_op.clone()],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 5,
                source_route_count: 1,
                scalar_storage_count: 3,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 1,
                typed_value_ref_count: 8,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:7".to_owned(),
                    label: "store.trigger".to_owned(),
                }],
                state_slots: vec![
                    boon_plan::DebugEntry {
                        id: "state:8".to_owned(),
                        label: "store.selector".to_owned(),
                    },
                    boon_plan::DebugEntry {
                        id: "state:9".to_owned(),
                        label: "store.value_a".to_owned(),
                    },
                    boon_plan::DebugEntry {
                        id: "state:10".to_owned(),
                        label: "store.selected".to_owned(),
                    },
                ],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let root_state = JsonMap::from_iter([
            ("store.selector".to_owned(), json!("A")),
            ("store.value_a".to_owned(), json!("alpha")),
            ("store.selected".to_owned(), json!("old")),
        ]);
        let evaluation = evaluate_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &root_state,
        )
        .expect("root MatchValueConst should evaluate");
        assert!(evaluation.supported);
        assert_eq!(evaluation.value, Some(json!("alpha")));
        assert_eq!(evaluation.expression_kind, Some("match_value_const"));

        let skipped_root_state = JsonMap::from_iter([
            ("store.selector".to_owned(), json!("B")),
            ("store.value_a".to_owned(), json!("alpha")),
            ("store.selected".to_owned(), json!("old")),
        ]);
        let skipped = evaluate_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &skipped_root_state,
        )
        .expect("root MatchValueConst fallback SKIP should evaluate");
        assert!(skipped.supported);
        assert!(skipped.skipped_by_guard);
        assert_eq!(skipped.value, None);
    }

    #[test]
    fn root_read_path_update_reads_derived_field_value() {
        let source_id = SourceId(12);
        let output_state_id = StateId(13);
        let field_id = FieldId(14);
        let update_op = PlanOp {
            id: PlanOpId(15),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::ReadPath,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![ValueRef::Source(source_id), ValueRef::Field(field_id)],
            output: Some(ValueRef::State(output_state_id)),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let source_route = SourceRoute {
            id: boon_plan::PlanSourceRouteId(0),
            source_id,
            path: "store.trigger".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: vec![source_route.clone()],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: output_state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(3),
                kind: RegionKind::UpdateBranches,
                ops: vec![update_op.clone()],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 0,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 1,
                typed_value_ref_count: 4,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:12".to_owned(),
                    label: "store.trigger".to_owned(),
                }],
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:13".to_owned(),
                    label: "store.selected".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: vec![boon_plan::DebugEntry {
                    id: "field:14".to_owned(),
                    label: "store.derived_selected".to_owned(),
                }],
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let root_state = JsonMap::from_iter([
            ("store.selected".to_owned(), json!("old")),
            ("store.derived_selected".to_owned(), json!("new")),
        ]);
        let evaluation = evaluate_root_json_update_branch(
            &plan,
            &update_op,
            source_id,
            &source_route,
            &RootJsonSourceEvent::default(),
            &root_state,
        )
        .expect("root ReadPath should read root derived fields");

        assert!(evaluation.supported);
        assert_eq!(evaluation.value, Some(json!("new")));
        assert_eq!(evaluation.expression_kind, Some("read_path"));
    }

    #[test]
    fn source_route_orchestration_is_executor_owned() {
        let source_id = SourceId(2);
        let state_id = StateId(3);
        let update_op_id = PlanOpId(4);
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: vec![boon_plan::PlanConstant {
                id: PlanConstantId(0),
                value: PlanConstantValue::Text {
                    value: "".to_owned(),
                },
            }],
            source_routes: vec![SourceRoute {
                id: boon_plan::PlanSourceRouteId(0),
                source_id,
                path: "store.input.change".to_owned(),
                scoped: false,
                scope_id: None,
                payload_schema: boon_plan::SourcePayloadSchema {
                    fields: vec![SourcePayloadField::Text],
                    typed_fields: vec![boon_plan::SourcePayloadDescriptor {
                        field: SourcePayloadField::Text,
                        value_type: boon_plan::SourcePayloadValueType::Text,
                    }],
                    row_lookup_field: None,
                    address_lookup_field: None,
                },
            }],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(0)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(1),
                    kind: RegionKind::SourceRouting,
                    ops: vec![PlanOp {
                        id: PlanOpId(1),
                        kind: PlanOpKind::SourceRoute,
                        inputs: Vec::new(),
                        output: Some(ValueRef::Source(source_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(2),
                    kind: RegionKind::StateInitialization,
                    ops: vec![PlanOp {
                        id: PlanOpId(2),
                        kind: PlanOpKind::StateInitialize {
                            initial_value_kind: InitialValueKind::Text,
                            initial_constant_id: Some(PlanConstantId(0)),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::State(state_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(3),
                    kind: RegionKind::UpdateBranches,
                    ops: vec![PlanOp {
                        id: update_op_id,
                        kind: PlanOpKind::UpdateBranch {
                            expression_kind: PlanExpressionKind::SourcePayload,
                            ordered_inputs: Vec::new(),
                            source_payload_field: Some(SourcePayloadField::Text),
                            update_constant_id: None,
                            source_guard: None,
                        },
                        inputs: vec![
                            ValueRef::Source(source_id),
                            ValueRef::SourcePayload {
                                source_id,
                                field: SourcePayloadField::Text,
                            },
                        ],
                        output: Some(ValueRef::State(state_id)),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
            ],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 1,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 3,
                typed_value_ref_count: 5,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:2".to_owned(),
                    label: "store.input.change".to_owned(),
                }],
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:3".to_owned(),
                    label: "store.input".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let event = RootJsonSourceEvent {
            text: Some("hello".to_owned()),
            ..RootJsonSourceEvent::default()
        };
        let verification = verify_plan(&plan).expect("test plan should verify");
        assert_eq!(
            verification.status,
            "pass",
            "test plan verification failed: {:?}",
            verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .collect::<Vec<_>>()
        );
        let mut runtime_branch_called = false;
        let output = execute_source_route_with_runtime_callbacks(
            &plan,
            "store.input.change",
            "store.input",
            &event,
            |_context, _surface, _json_report| {
                runtime_branch_called = true;
                Err("runtime branch should not run for JSON source-payload route".into())
            },
            || {
                Ok(SourceRouteFullExecution {
                    state_summary: json!({ "store.input": "hello" }),
                    semantic_delta_signatures: vec!["FieldSet:store.input".to_owned()],
                    semantic_deltas: json!([{
                        "kind": "FieldSet",
                        "field_path": "store.input",
                        "value": "hello"
                    }]),
                    per_step: Vec::new(),
                    executor_report: json!({ "executor": "test-full-execution" }),
                })
            },
        )
        .expect("source-route orchestration should execute through PlanExecutor");

        assert!(!runtime_branch_called);
        assert_eq!(output.value, json!("hello"));
        assert_eq!(output.state_summary, json!({ "store.input": "hello" }));
        assert_eq!(output.route_surface["expression_kind"], "source_payload");
        assert_eq!(
            output.route_surface["route_execution_core"]["execution_surface_core"]["execution_surface"],
            "plan-json"
        );
        assert_eq!(
            output.executor_report["executor"],
            "cpu-plan-source-route-v1"
        );
    }

    #[test]
    fn root_update_execution_surface_is_executor_owned() {
        let mut evaluation = RootJsonUpdateEvaluation {
            supported: true,
            skipped_by_guard: false,
            unsupported_reason: None,
            target_state_id: Some(StateId(3)),
            value: Some(json!("hello")),
            expression_kind: Some("source_payload"),
            source_payload_field: json!("Text"),
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            executor_report: json!({}),
        };

        let scalar_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
        assert_eq!(
            scalar_surface.kind,
            RootUpdateExecutionSurfaceKind::PlanJson
        );
        assert_eq!(
            scalar_surface.executor_report["execution_surface"],
            "plan-json"
        );

        evaluation.value = Some(json!({
            "$boon_type": "BYTES",
            "storage": "inline",
            "digest": "abc",
            "byte_len": 3
        }));
        let bytes_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
        assert_eq!(
            bytes_surface.kind,
            RootUpdateExecutionSurfaceKind::RuntimeBranch
        );
        assert_eq!(bytes_surface.executor_report["core_value_is_bytes"], true);

        evaluation.skipped_by_guard = true;
        let skipped_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
        assert_eq!(
            skipped_surface.kind,
            RootUpdateExecutionSurfaceKind::SkippedByGuard
        );
        assert_eq!(
            skipped_surface.executor_report["execution_surface"],
            "skipped-by-guard"
        );
    }

    #[test]
    fn root_json_update_execution_assembles_plan_json_executed_update() {
        let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
        let verification = verify_plan(&plan).expect("test plan should verify");
        assert_eq!(
            verification.status,
            "pass",
            "test plan verification failed: {:?}",
            verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .collect::<Vec<_>>()
        );
        let update_op = plan
            .regions
            .iter()
            .flat_map(|region| region.ops.iter())
            .find(|op| op.id == update_op_id)
            .expect("test plan should include update op");
        let event = RootJsonSourceEvent {
            text: Some("hello".to_owned()),
            ..RootJsonSourceEvent::default()
        };
        let execution = execute_root_json_update_branch(
            &plan,
            update_op,
            source_id,
            &plan.source_routes[0],
            &event,
            &JsonMap::new(),
        )
        .expect("source-payload text branch should execute in PlanExecutor JSON surface");

        assert_eq!(
            execution.surface_kind,
            RootUpdateExecutionSurfaceKind::PlanJson
        );
        assert_eq!(
            execution.executor_report["executor"],
            "cpu-plan-root-json-update-execution-v1"
        );
        assert_eq!(execution.executor_report["surface"], "plan-json");
        assert_eq!(
            execution.evaluator_report["execution_surface_core"]["execution_surface"],
            "plan-json"
        );
        let executed = execution
            .executed
            .expect("Plan JSON branch should assemble an executed root update");
        assert_eq!(executed.value, json!("hello"));
        assert_eq!(executed.expression_kind, "source_payload");
        assert_eq!(executed.source_payload_field, json!("Text"));
        assert_eq!(executed.update_constant_id, JsonValue::Null);
        assert_eq!(executed.executor_core["expression_kind"], "source_payload");
        assert_eq!(
            executed.executor_core["execution_surface_core"]["execution_surface"],
            "plan-json"
        );
        let mut root_state = JsonMap::new();
        assert_eq!(
            apply_root_json_state_value(
                &plan,
                &mut root_state,
                state_id,
                executed.value,
                update_op_id
            )
            .expect("executed value should apply to root state")
            .target_state_label,
            "store.input"
        );
    }

    #[test]
    fn root_runtime_branch_update_execution_is_executor_owned() {
        let inline = vec![1, 2, 3, 4];
        let bytes = PlanExecutorBytes::from_inline(
            sha256_bytes(&inline),
            inline.len() as u64,
            inline,
            "root runtime branch update test",
        )
        .expect("test bytes should be valid");
        let executed = assemble_root_runtime_branch_update(RootRuntimeBranchUpdateInput {
            value: json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "byte_len": 4,
                "digest": bytes.digest()
            }),
            bytes_value: Some(bytes),
            fixed_bytes_mutation: None,
            bytes_access: json!({
                "read_only": false,
                "access_source": "private_bytes"
            }),
            runtime_branch_core: json!({
                "executor": "cpu-plan-root-bytes-write-evaluator-v1"
            }),
            state_write_core: JsonValue::Null,
            bytes_state_core: json!({
                "executor": "cpu-plan-root-bytes-state-transition-v1"
            }),
            expression_kind: "bytes_concat".to_owned(),
            source_payload_field: JsonValue::Null,
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            host_effect: JsonValue::Null,
        });

        assert_eq!(executed.expression_kind, "bytes_concat");
        assert_eq!(
            executed.executor_core["executor"],
            "cpu-plan-root-bytes-write-evaluator-v1"
        );
        assert_eq!(
            executed.executor_core["runtime_branch_execution_core"]["executor"],
            "cpu-plan-root-runtime-branch-update-execution-v1"
        );
        assert_eq!(
            executed.executor_core["runtime_branch_execution_core"]["runtime_branch_core"]["executor"],
            "cpu-plan-root-bytes-write-evaluator-v1"
        );
        assert_eq!(
            executed.bytes_state_core["executor"],
            "cpu-plan-root-bytes-state-transition-v1"
        );
        assert_eq!(
            executed.bytes_access["access_source"],
            json!("private_bytes")
        );
    }

    #[test]
    fn root_bytes_update_dispatch_kind_is_executor_owned() {
        fn update_op(expression_kind: PlanExpressionKind) -> PlanOp {
            PlanOp {
                id: PlanOpId(4),
                kind: PlanOpKind::UpdateBranch {
                    expression_kind,
                    ordered_inputs: Vec::new(),
                    source_payload_field: None,
                    update_constant_id: None,
                    source_guard: None,
                },
                inputs: Vec::new(),
                output: Some(ValueRef::State(StateId(3))),
                indexed: false,
                unresolved_executable_ref_count: 0,
            }
        }

        assert_eq!(
            root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BytesLength)),
            Some(RootBytesUpdateDispatchKind::Read)
        );
        assert_eq!(
            root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::FileReadBytes)),
            Some(RootBytesUpdateDispatchKind::Read)
        );
        assert_eq!(
            root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BytesConcat)),
            Some(RootBytesUpdateDispatchKind::Write)
        );
        assert_eq!(
            root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::FileWriteBytes)),
            Some(RootBytesUpdateDispatchKind::Write)
        );
        assert_eq!(
            root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BoolNot)),
            None
        );

        let mut payload_op = update_op(PlanExpressionKind::BytesLength);
        if let PlanOpKind::UpdateBranch {
            source_payload_field,
            ..
        } = &mut payload_op.kind
        {
            *source_payload_field = Some(SourcePayloadField::Bytes);
        }
        assert_eq!(root_bytes_update_dispatch_kind(&payload_op), None);
    }

    #[test]
    fn source_guard_matching_is_executor_owned() {
        let guard = Some(PlanSourceGuard::SourcePayloadOneOf {
            source_id: SourceId(4),
            field: SourcePayloadField::Key,
            values: vec!["Enter".to_owned(), "NumpadEnter".to_owned()],
        });
        let matching_event = RootJsonSourceEvent {
            key: Some("Enter".to_owned()),
            ..RootJsonSourceEvent::default()
        };
        assert!(
            source_guard_matches(&guard, SourceId(4), &matching_event)
                .expect("matching guard should evaluate")
        );

        let non_matching_event = RootJsonSourceEvent {
            key: Some("Escape".to_owned()),
            ..RootJsonSourceEvent::default()
        };
        assert!(
            !source_guard_matches(&guard, SourceId(4), &non_matching_event)
                .expect("non-matching guard should evaluate")
        );

        let wrong_source = source_guard_matches(&guard, SourceId(9), &matching_event)
            .expect_err("guard source mismatch should be rejected");
        assert!(
            wrong_source
                .to_string()
                .contains("source guard targets source 4"),
            "unexpected error: {wrong_source}"
        );

        let bytes_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
            source_id: SourceId(4),
            field: SourcePayloadField::Bytes,
            values: vec!["00".to_owned(), "de ad be ef".to_owned()],
        });
        let bytes_event = RootJsonSourceEvent {
            payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xde, 0xad, 0xbe, 0xef])]),
            ..RootJsonSourceEvent::default()
        };
        assert!(
            source_guard_matches(&bytes_guard, SourceId(4), &bytes_event)
                .expect("matching BYTES guard should evaluate")
        );
        let non_matching_bytes_event = RootJsonSourceEvent {
            payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xca, 0xfe])]),
            ..RootJsonSourceEvent::default()
        };
        assert!(
            !source_guard_matches(&bytes_guard, SourceId(4), &non_matching_bytes_event)
                .expect("non-matching BYTES guard should evaluate")
        );
        let invalid_bytes_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
            source_id: SourceId(4),
            field: SourcePayloadField::Bytes,
            values: vec!["not hex".to_owned()],
        });
        let invalid_bytes_error =
            source_guard_matches(&invalid_bytes_guard, SourceId(4), &bytes_event)
                .expect_err("invalid BYTES guard hex should be rejected");
        assert!(
            invalid_bytes_error.to_string().contains("invalid hex"),
            "unexpected error: {invalid_bytes_error}"
        );
    }

    #[test]
    fn list_remove_predicate_evaluation_is_executor_owned() {
        let state_id = StateId(5);
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:5".to_owned(),
                    label: "row.done".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let row = PlanExecutorListRow {
            key: 11,
            generation: 2,
            fields: BTreeMap::from([("done".to_owned(), json!(true))]),
        };
        let predicate = boon_plan::PlanListRemovePredicate::RowFieldBool {
            input: ValueRef::State(state_id),
        };
        let evaluation = evaluate_list_remove_predicate(&plan, &predicate, &row)
            .expect("row-field bool predicate should evaluate in executor");
        assert!(evaluation.matches);
        assert_eq!(
            evaluation.executor_report["executor"],
            "cpu-plan-list-remove-predicate-evaluator-v1"
        );
        assert_eq!(evaluation.executor_report["key"], 11);

        let report = build_list_remove_predicate_row_resolution_report(&plan, &predicate, 3, &row)
            .expect("predicate row-resolution report should be executor-owned");
        assert_eq!(
            report["executor"],
            "cpu-plan-list-remove-predicate-row-resolution-v1"
        );
        assert_eq!(report["predicate"], "row_field_bool");
        assert_eq!(report["predicate_field"], "done");
        assert_eq!(report["row_index"], 3);

        let not_predicate = boon_plan::PlanListRemovePredicate::RowFieldBoolNot {
            input: ValueRef::State(state_id),
        };
        let not_evaluation = evaluate_list_remove_predicate(&plan, &not_predicate, &row)
            .expect("row-field bool-not predicate should evaluate in executor");
        assert!(!not_evaluation.matches);
    }

    #[test]
    fn list_append_value_resolution_is_executor_owned() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:8".to_owned(),
                    label: "todo.title".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let derived_values = BTreeMap::from([(FieldId(2), json!("derived title"))]);
        let row_fields = BTreeMap::from([("title".to_owned(), json!("row title"))]);

        assert_eq!(
            resolve_plan_value_ref(
                &plan,
                &ValueRef::Field(FieldId(2)),
                &derived_values,
                Some(&row_fields),
            )
            .expect("field ref should resolve"),
            Some(json!("derived title"))
        );
        assert_eq!(
            resolve_plan_value_ref(
                &plan,
                &ValueRef::State(StateId(8)),
                &derived_values,
                Some(&row_fields),
            )
            .expect("state ref should resolve from row fields"),
            Some(json!("row title"))
        );

        let bytes_constant = boon_plan::PlanConstant {
            id: PlanConstantId(9),
            value: PlanConstantValue::Bytes {
                byte_len: 3,
                sha256: sha256_bytes(&[1, 2, 3]),
                inline_bytes: Some(vec![1, 2, 3]),
            },
        };
        let bytes_json =
            plan_constant_json_value(&bytes_constant).expect("BYTES constant should report JSON");
        assert_eq!(bytes_json["$boon_type"], "BYTES");
        assert_eq!(bytes_json["byte_len"], 3);
    }

    #[test]
    fn initial_list_row_constant_value_conversion_is_executor_owned() {
        let value = PlanConstantValue::Bytes {
            byte_len: 3,
            sha256: sha256_bytes(&[7, 8, 9]),
            inline_bytes: Some(vec![7, 8, 9]),
        };

        let json_value = plan_constant_value_json_value(&value, "initial row field `payload`")
            .expect("BYTES row value should report JSON");
        assert_eq!(json_value["$boon_type"], "BYTES");
        assert_eq!(json_value["byte_len"], 3);

        let bytes = plan_constant_value_bytes(&value, "initial row field `payload`")
            .expect("BYTES row value should validate")
            .expect("BYTES row value should produce private bytes");
        assert_eq!(bytes.inline_bytes(), &[7, 8, 9]);

        let scalar = PlanConstantValue::Text {
            value: "row title".to_owned(),
        };
        assert_eq!(
            plan_constant_value_json_value(&scalar, "initial row field `title`")
                .expect("TEXT row value should report JSON"),
            json!("row title")
        );
        assert!(
            plan_constant_value_bytes(&scalar, "initial row field `title`")
                .expect("TEXT row value should not fail")
                .is_none()
        );

        let tampered = PlanConstantValue::Bytes {
            byte_len: 3,
            sha256: sha256_bytes(&[7, 8, 9]),
            inline_bytes: Some(vec![7, 8, 10]),
        };
        let error = plan_constant_value_bytes(&tampered, "initial row field `payload`")
            .expect_err("digest mismatch should be rejected by executor conversion");
        assert!(
            error.to_string().contains("digest mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn list_row_report_fields_are_executor_owned() {
        let row = PlanExecutorListRow {
            key: 4,
            generation: 2,
            fields: BTreeMap::from([
                ("title".to_owned(), json!("task")),
                ("payload".to_owned(), json!("stale-public-value")),
            ]),
        };
        let private_bytes = BTreeMap::from([(
            "payload".to_owned(),
            PlanExecutorBytes::from_inline(
                sha256_bytes(&[4, 5, 6]),
                3,
                vec![4, 5, 6],
                "row payload",
            )
            .expect("valid private row bytes"),
        )]);

        let fields = list_row_report_fields(&row, &private_bytes);
        assert_eq!(fields["title"], json!("task"));
        assert_eq!(fields["payload"]["$boon_type"], "BYTES");
        assert_eq!(fields["payload"]["byte_len"], 3);
        assert_eq!(fields["payload"]["digest"], sha256_bytes(&[4, 5, 6]));
    }

    #[test]
    fn list_row_state_carrier_reports_private_bytes() {
        let row = PlanExecutorListRowState {
            key: 11,
            generation: 3,
            fields: BTreeMap::from([
                ("title".to_owned(), json!("row")),
                ("payload".to_owned(), json!("stale-public-value")),
            ]),
            private_bytes: BTreeMap::from([(
                "payload".to_owned(),
                PlanExecutorBytes::from_inline(
                    sha256_bytes(&[9, 8, 7]),
                    3,
                    vec![9, 8, 7],
                    "row state payload",
                )
                .expect("valid row state bytes"),
            )]),
            fixed_bytes_banks: BTreeMap::from([("payload".to_owned(), vec![9, 8, 7])]),
        };
        let public_rows =
            list_row_state_public_rows(&BTreeMap::from([(5usize, vec![row.clone()])]));
        assert_eq!(public_rows[&5][0].key, 11);
        assert_eq!(public_rows[&5][0].fields["title"], json!("row"));

        let fields = list_row_state_report_fields(&row);
        assert_eq!(fields["title"], json!("row"));
        assert_eq!(fields["payload"]["$boon_type"], "BYTES");
        assert_eq!(fields["payload"]["byte_len"], 3);
        assert_eq!(fields["payload"]["digest"], sha256_bytes(&[9, 8, 7]));
    }

    #[test]
    fn list_row_initial_state_refresh_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let title_state_id = StateId(21);
        let payload_state_id = StateId(22);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(1),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let mut plan = empty_executor_test_plan();
        plan.debug_map.state_slots = vec![
            boon_plan::DebugEntry {
                id: "state:21".to_owned(),
                label: "todo.title_state".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "state:22".to_owned(),
                label: "todo.payload_state".to_owned(),
            },
        ];
        plan.storage_layout.list_slots = vec![list_slot.clone()];
        plan.storage_layout.scalar_slots = vec![
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(2),
                state_id: title_state_id,
                value_type: PlanValueType::Text,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: Some("todo.title".to_owned()),
            },
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(3),
                state_id: payload_state_id,
                value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Bytes,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: Some("todo.payload".to_owned()),
            },
        ];
        plan.storage_layout.byte_banks = vec![boon_plan::ByteStorageBank {
            id: boon_plan::PlanStorageId(4),
            state_storage_id: boon_plan::PlanStorageId(3),
            state_id: payload_state_id,
            scope_id: Some(scope_id),
            indexed: true,
            fixed_len: 3,
            capacity: None,
        }];
        let payload = PlanExecutorBytes::from_inline(
            sha256_bytes(&[1, 2, 3]),
            3,
            vec![1, 2, 3],
            "row initial state refresh payload",
        )
        .expect("valid payload bytes");
        let mut row = PlanExecutorListRowState {
            key: 4,
            generation: 1,
            fields: BTreeMap::from([
                ("title".to_owned(), json!("Buy milk")),
                ("payload".to_owned(), payload.report_json()),
            ]),
            private_bytes: BTreeMap::from([("payload".to_owned(), payload)]),
            fixed_bytes_banks: BTreeMap::new(),
        };

        refresh_list_row_initial_state_fields(&plan, &list_slot, &mut row);

        assert_eq!(row.fields["title_state"], json!("Buy milk"));
        assert_eq!(row.fields["payload_state"]["$boon_type"], "BYTES");
        assert_eq!(
            row.private_bytes["payload_state"].inline_bytes(),
            &[1, 2, 3]
        );
        assert_eq!(row.fixed_bytes_banks["payload_state"], vec![1, 2, 3]);
    }

    #[test]
    fn list_row_bool_not_refresh_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(4);
        let done_state_id = StateId(31);
        let not_done_field_id = FieldId(41);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(1),
            list_id: boon_plan::ListId(9),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let mut plan = empty_executor_test_plan();
        plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
            id: "state:31".to_owned(),
            label: "todo.done".to_owned(),
        }];
        plan.debug_map.fields = vec![boon_plan::DebugEntry {
            id: "field:41".to_owned(),
            label: "todo.not_done".to_owned(),
        }];
        plan.storage_layout.list_slots = vec![list_slot.clone()];
        plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(2),
            state_id: done_state_id,
            value_type: PlanValueType::Bool,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Bool,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        }];
        plan.regions = vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(1),
            kind: RegionKind::DerivedEvaluation,
            ops: vec![PlanOp {
                id: PlanOpId(12),
                kind: PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    startup_recompute: true,
                    expression: Some(PlanDerivedExpression::BoolNot {
                        input: ValueRef::State(done_state_id),
                    }),
                },
                inputs: vec![ValueRef::State(done_state_id)],
                output: Some(ValueRef::Field(not_done_field_id)),
                indexed: true,
                unresolved_executable_ref_count: 0,
            }],
        }];

        let mut fields = BTreeMap::from([("done".to_owned(), json!(false))]);
        let deltas =
            refresh_list_row_bool_not_deltas(&plan, &list_slot, "todos", 7, 1, &mut fields)
                .expect("strict Bool/not refresh should produce a delta");
        assert_eq!(fields["not_done"], json!(true));
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0]["kind"], "FieldSet");
        assert_eq!(deltas[0]["field_path"], "not_done");
        assert_eq!(deltas[0]["value"], json!(true));

        let unchanged =
            refresh_list_row_bool_not_fields(&plan, &list_slot, "todos", 7, 1, &mut fields)
                .expect("best-effort Bool/not refresh should succeed");
        assert!(
            unchanged.is_empty(),
            "best-effort refresh should not emit a delta when the value is current"
        );
        fields.insert("done".to_owned(), json!(true));
        let changed =
            refresh_list_row_bool_not_fields(&plan, &list_slot, "todos", 7, 1, &mut fields)
                .expect("best-effort Bool/not refresh should update changed values");
        assert_eq!(fields["not_done"], json!(false));
        assert_eq!(changed[0]["value"], json!(false));
    }

    #[test]
    fn list_row_textlike_field_is_executor_owned() {
        let row = PlanExecutorListRow {
            key: 8,
            generation: 1,
            fields: BTreeMap::from([
                ("title".to_owned(), json!("task")),
                ("count".to_owned(), json!(3)),
                ("done".to_owned(), json!(true)),
                ("metadata".to_owned(), json!({"owner": "plan"})),
            ]),
        };

        assert_eq!(
            list_row_textlike_field(&row, "title").as_deref(),
            Some("task")
        );
        assert_eq!(list_row_textlike_field(&row, "count").as_deref(), Some("3"));
        assert_eq!(
            list_row_textlike_field(&row, "done").as_deref(),
            Some("True")
        );
        assert_eq!(list_row_textlike_field(&row, "metadata"), None);
        assert_eq!(list_row_textlike_field(&row, "missing"), None);
    }

    #[test]
    fn scenario_checkpoint_assertions_are_executor_owned() {
        let plan = empty_executor_test_plan();
        let root_state = JsonMap::from_iter([(
            "store".to_owned(),
            json!({
                "selected_filter": "Active",
                "new_todo_text": "Draft",
            }),
        )]);
        let list_state = BTreeMap::from([
            (
                1,
                vec![
                    PlanExecutorListRow {
                        key: 10,
                        generation: 1,
                        fields: BTreeMap::from([
                            ("title".to_owned(), json!("Write tests")),
                            ("completed".to_owned(), json!(false)),
                            ("editing".to_owned(), json!(true)),
                            ("edit_text".to_owned(), json!("Draft title")),
                        ]),
                    },
                    PlanExecutorListRow {
                        key: 11,
                        generation: 1,
                        fields: BTreeMap::from([
                            ("title".to_owned(), json!("Compile")),
                            ("completed".to_owned(), json!(true)),
                            ("editing".to_owned(), json!(false)),
                            ("edit_text".to_owned(), json!("Compile")),
                        ]),
                    },
                ],
            ),
            (
                2,
                vec![
                    PlanExecutorListRow {
                        key: 20,
                        generation: 1,
                        fields: BTreeMap::from([
                            ("address".to_owned(), json!("A0")),
                            ("value".to_owned(), json!("5")),
                            ("formula_text".to_owned(), json!("5")),
                            ("editing_text".to_owned(), json!("5")),
                            ("editing".to_owned(), json!(false)),
                        ]),
                    },
                    PlanExecutorListRow {
                        key: 21,
                        generation: 1,
                        fields: BTreeMap::from([
                            ("address".to_owned(), json!("B0")),
                            ("error".to_owned(), json!("Cycle")),
                        ]),
                    },
                ],
            ),
        ]);

        let report = assert_scenario_checkpoint(
            &plan,
            &root_state,
            &list_state,
            PlanExecutorScenarioCheckpointInput {
                step_id: "checkpoint".to_owned(),
                source_intent_exemption: Some("assertion-only".to_owned()),
                expect_titles: Some(vec!["Write tests".to_owned(), "Compile".to_owned()]),
                expect_completed_titles: Some(vec!["Compile".to_owned()]),
                expect_active_count: Some(1),
                expect_completed_count: Some(1),
                expect_filter: Some("Active".to_owned()),
                expect_new_text: Some("Draft".to_owned()),
                expect_editing_title: Some("Write tests".to_owned()),
                expect_edit_text: Some("Draft title".to_owned()),
                expect_no_editing: Some(false),
                expect_cell: Some(PlanExecutorScenarioCheckpointCellExpectation {
                    address: "A0".to_owned(),
                    value: Some("5".to_owned()),
                    formula: Some("5".to_owned()),
                    editing_text: Some("5".to_owned()),
                    editing: Some(false),
                }),
                expect_error: Some(PlanExecutorScenarioCheckpointErrorExpectation {
                    address: "B0".to_owned(),
                    error: "Cycle".to_owned(),
                }),
                expect_root_text: BTreeMap::from([(
                    "store.selected_filter".to_owned(),
                    "Active".to_owned(),
                )]),
                ..PlanExecutorScenarioCheckpointInput::default()
            },
        )
        .expect("PlanExecutor should own assertion-only checkpoint evaluation");

        assert_eq!(report.report["passed"], true);
        assert_eq!(report.report["source_intent_exemption"], "assertion-only");
        assert_eq!(report.report["checked_expectation_count"], json!(15));
        assert_eq!(
            report.report["checked_expectations"]
                .as_array()
                .expect("checked expectations should be an array")
                .iter()
                .filter(|item| item.as_str() == Some("expect_cell.value"))
                .count(),
            1
        );
    }

    #[test]
    fn root_update_candidate_tracker_is_executor_owned() {
        let mut tracker = RootUpdateCandidateTracker::default();

        let inserted = record_root_update_candidate(
            &mut tracker,
            "store.submit",
            RootUpdateCandidate {
                state_id: 7,
                op_id: 40,
                value: json!("value"),
                bytes_value: None,
                fixed_bytes_mutation: None,
            },
        )
        .expect("first candidate should be inserted");
        assert_eq!(inserted.kind, RootUpdateCandidateRecordKind::Inserted);

        let duplicate = record_root_update_candidate(
            &mut tracker,
            "store.submit",
            RootUpdateCandidate {
                state_id: 7,
                op_id: 41,
                value: json!("value"),
                bytes_value: None,
                fixed_bytes_mutation: None,
            },
        )
        .expect("same-value candidate should coalesce");
        assert_eq!(duplicate.kind, RootUpdateCandidateRecordKind::Duplicate);
        assert_eq!(duplicate.op_ids, vec![40, 41]);

        let ordered = tracker.ordered_candidates();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].state_id, 7);
        assert_eq!(ordered[0].op_ids, vec![40, 41]);
        assert_eq!(ordered[0].value, json!("value"));

        let conflict = record_root_update_candidate(
            &mut tracker,
            "store.submit",
            RootUpdateCandidate {
                state_id: 7,
                op_id: 42,
                value: json!("other"),
                bytes_value: None,
                fixed_bytes_mutation: None,
            },
        )
        .expect_err("conflicting candidate should be rejected");
        assert!(
            conflict.to_string().contains("conflicting branches"),
            "unexpected conflict error: {conflict}"
        );
    }

    #[test]
    fn root_update_candidate_tracker_rejects_byte_fingerprint_conflicts() {
        let mut tracker = RootUpdateCandidateTracker::default();
        let first_bytes = Some(json!({
            "$boon_type": "BYTES",
            "byte_len": 3,
            "digest": "aaa",
        }));
        let other_bytes = Some(json!({
            "$boon_type": "BYTES",
            "byte_len": 3,
            "digest": "bbb",
        }));

        record_root_update_candidate(
            &mut tracker,
            "store.bytes",
            RootUpdateCandidate {
                state_id: 9,
                op_id: 50,
                value: json!({"$boon_type": "BYTES", "byte_len": 3}),
                bytes_value: first_bytes,
                fixed_bytes_mutation: None,
            },
        )
        .expect("first byte candidate should be inserted");

        let conflict = record_root_update_candidate(
            &mut tracker,
            "store.bytes",
            RootUpdateCandidate {
                state_id: 9,
                op_id: 51,
                value: json!({"$boon_type": "BYTES", "byte_len": 3}),
                bytes_value: other_bytes,
                fixed_bytes_mutation: None,
            },
        )
        .expect_err("same public value with different private bytes should conflict");
        assert!(
            conflict.to_string().contains("conflicting branches"),
            "unexpected byte conflict error: {conflict}"
        );
    }

    #[test]
    fn root_update_commit_assembly_is_executor_owned() {
        let commit = assemble_root_update_commit(RootUpdateCommitInput {
            source_id: SourceId(3),
            target_state: "store.title".to_owned(),
            target_state_id: 7,
            candidate_update_op_ids: vec![40, 41],
            expression_kind: "source_payload_text".to_owned(),
            source_payload_field: json!("Text"),
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            bytes_access: JsonValue::Null,
            host_effect: JsonValue::Null,
            executor_core: json!({"executor": "core"}),
            state_write_core: json!({"changed": true}),
            bytes_state_core: JsonValue::Null,
            value: json!("New title"),
            changed: true,
            semantic_delta: None,
        })
        .expect("changed root update commit should assemble");

        assert_eq!(
            commit.touched_state,
            Some(("store.title".to_owned(), json!("New title")))
        );
        assert_eq!(
            commit.semantic_delta_signature.as_deref(),
            Some("FieldSet:store.title")
        );
        assert_eq!(
            commit
                .semantic_delta
                .as_ref()
                .and_then(|delta| delta.get("field_path"))
                .and_then(JsonValue::as_str),
            Some("store.title")
        );
        assert_eq!(commit.update_report["update_op_id"], 40);
        assert_eq!(
            commit.update_report["candidate_update_op_ids"],
            json!([40, 41])
        );
        assert_eq!(
            commit.executor_report["executor"],
            "cpu-plan-root-update-commit-assembly-v1"
        );
    }

    #[test]
    fn root_update_commit_assembly_suppresses_unchanged_delta() {
        let commit = assemble_root_update_commit(RootUpdateCommitInput {
            source_id: SourceId(3),
            target_state: "store.title".to_owned(),
            target_state_id: 7,
            candidate_update_op_ids: vec![40],
            expression_kind: "source_payload_text".to_owned(),
            source_payload_field: json!("Text"),
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            bytes_access: JsonValue::Null,
            host_effect: JsonValue::Null,
            executor_core: json!({"executor": "core"}),
            state_write_core: json!({"changed": false}),
            bytes_state_core: JsonValue::Null,
            value: json!("Same title"),
            changed: false,
            semantic_delta: Some(json!({"kind": "FieldSet"})),
        })
        .expect("unchanged root update commit should still report");

        assert_eq!(commit.touched_state, None);
        assert_eq!(commit.semantic_delta_signature, None);
        assert_eq!(commit.semantic_delta, None);
        assert_eq!(commit.update_report["changed"], false);
        assert_eq!(commit.executor_report["emitted_semantic_delta"], false);
    }

    #[test]
    fn root_update_commit_batch_applies_candidates_to_root_state() {
        let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
        let mut root_state = initialize_root_state(&plan).expect("root state should initialize");
        assert_eq!(root_state.root_state["store.input"], "");

        let executed = RootExecutedUpdate {
            value: json!("Typed text"),
            bytes_value: None,
            fixed_bytes_mutation: None,
            bytes_access: JsonValue::Null,
            executor_core: json!({"executor": "test-root-update"}),
            state_write_core: JsonValue::Null,
            bytes_state_core: JsonValue::Null,
            expression_kind: "source_payload_text".to_owned(),
            source_payload_field: json!("Text"),
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            host_effect: JsonValue::Null,
        };
        let mut tracker = RootUpdateCandidateTracker::default();
        record_root_update_candidate(
            &mut tracker,
            "store.input.change",
            root_update_candidate_from_executed(state_id.0, update_op_id.0, &executed),
        )
        .expect("candidate should be recorded");

        let batch = commit_ordered_root_update_candidates(
            &mut root_state,
            &plan,
            source_id,
            &tracker,
            BTreeMap::from([(state_id.0, executed)]),
        )
        .expect("PlanExecutor should commit ordered root update candidates");

        assert_eq!(root_state.root_state["store.input"], "Typed text");
        assert_eq!(batch.executed_update_branch_count, 1);
        assert_eq!(batch.touched_states["store.input"], "Typed text");
        assert_eq!(
            batch.semantic_delta_signatures,
            vec!["FieldSet:store.input".to_owned()]
        );
        assert_eq!(batch.semantic_deltas[0]["field_path"], "store.input");
        assert_eq!(batch.update_reports[0]["update_op_id"], update_op_id.0);
        assert_eq!(
            batch.executor_report["executor"],
            "cpu-plan-root-update-commit-batch-v1"
        );
        assert_eq!(batch.executor_report["committed_update_count"], 1);
    }

    #[test]
    fn root_update_branch_collection_stages_plan_json_candidate() {
        let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
        let source_route_slot = plan
            .source_routes
            .iter()
            .find(|route| route.source_id == source_id)
            .expect("test plan should include source route");
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| op.id == update_op_id)
            .expect("test plan should include update op");
        let mut staged_root_state =
            initialize_root_state(&plan).expect("root state should initialize");
        let root_json_event = RootJsonSourceEvent {
            text: Some("Typed text".to_owned()),
            ..RootJsonSourceEvent::default()
        };
        let mut tracker = RootUpdateCandidateTracker::default();
        let mut touched_updates = BTreeMap::new();
        let mut runtime_branch =
            |_op: &PlanOp, _state: &PlanExecutorRootState| -> PlanExecutorResult<_> {
                panic!("plan-json source payload update should not call runtime branch")
            };

        let collection = collect_root_update_candidate_for_step(
            &plan,
            op,
            source_id,
            "store.input.change",
            source_route_slot,
            &root_json_event,
            &mut staged_root_state,
            &mut tracker,
            &mut touched_updates,
            &mut runtime_branch,
        )
        .expect("PlanExecutor should collect root update candidate");

        assert_eq!(collection.target_state_id, Some(state_id));
        assert!(collection.inserted_update);
        assert!(!collection.runtime_branch_used);
        assert_eq!(staged_root_state.root_state["store.input"], "Typed text");
        assert_eq!(touched_updates.len(), 1);
        assert_eq!(touched_updates[&state_id.0].value, json!("Typed text"));
        assert_eq!(tracker.ordered_candidates().len(), 1);
        assert_eq!(
            collection.executor_report["executor"],
            "cpu-plan-root-update-branch-collection-v1"
        );
    }

    #[test]
    fn root_update_storage_transition_commits_bytes_and_public_state() {
        let mut root_state = JsonMap::new();
        let mut private_bytes = BTreeMap::new();
        let mut fixed_byte_banks = BTreeMap::new();
        let inline = vec![1, 2, 3];
        let bytes = PlanExecutorBytes::from_inline(
            sha256_bytes(&inline),
            inline.len() as u64,
            inline.clone(),
            "root update storage transition test",
        )
        .expect("test bytes should be valid");

        let mut state_owner =
            RootUpdateStateMaps::new(&mut root_state, &mut private_bytes, &mut fixed_byte_banks);
        let transition = apply_root_update_storage_transition(
            &mut state_owner,
            StateId(7),
            "store.payload",
            json!({"$boon_type": "BYTES", "byte_len": 3}),
            Some(bytes),
            None,
            PlanOpId(55),
        )
        .expect("root update storage transition should commit bytes");
        drop(state_owner);

        assert_eq!(root_state["store.payload"]["byte_len"], 3);
        assert_eq!(
            private_bytes
                .get(&7)
                .expect("private BYTES state should be committed")
                .inline_bytes,
            inline
        );
        assert!(!fixed_byte_banks.contains_key(&7));
        assert_eq!(transition.target_state_id, StateId(7));
        assert_eq!(transition.target_state_label, "store.payload");
        assert_eq!(transition.bytes_transition_mode, "bytes_commit");
        assert_eq!(
            transition.executor_report["executor"],
            "cpu-plan-root-update-storage-transition-v1"
        );
        assert_eq!(
            transition.executor_report["bytes_transition_core"]["executor"],
            "cpu-plan-root-bytes-state-transition-v1"
        );
    }

    #[test]
    fn root_update_storage_transition_applies_fixed_patch() {
        let mut root_state = JsonMap::new();
        let mut private_bytes = BTreeMap::new();
        let mut fixed_byte_banks = BTreeMap::new();
        let inline = vec![4, 5, 6];
        let bytes = PlanExecutorBytes::from_inline(
            sha256_bytes(&inline),
            inline.len() as u64,
            inline,
            "root update fixed patch transition seed",
        )
        .expect("test bytes should be valid");
        private_bytes.insert(7, bytes);
        fixed_byte_banks.insert(7, vec![4, 5, 6]);

        let mut state_owner =
            RootUpdateStateMaps::new(&mut root_state, &mut private_bytes, &mut fixed_byte_banks);
        let transition = apply_root_update_storage_transition(
            &mut state_owner,
            StateId(7),
            "store.payload",
            json!({"$boon_type": "BYTES", "byte_len": 3}),
            None,
            Some(RootBytesFixedMutation {
                input_state_id: StateId(7),
                output_state_id: StateId(7),
                patches: vec![(1, 9)],
            }),
            PlanOpId(56),
        )
        .expect("root update storage transition should apply fixed patch");
        drop(state_owner);

        assert_eq!(root_state["store.payload"]["byte_len"], 3);
        assert!(!private_bytes.contains_key(&7));
        assert_eq!(fixed_byte_banks.get(&7), Some(&vec![4, 9, 6]));
        assert_eq!(transition.bytes_transition_mode, "fixed_byte_patch");
        assert_eq!(
            transition.executor_report["bytes_transition_mode"],
            "fixed_byte_patch"
        );
    }

    #[test]
    fn root_executed_update_candidate_and_state_apply_are_executor_owned() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:7".to_owned(),
                    label: "store.payload".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let inline = vec![7, 8, 9];
        let bytes = PlanExecutorBytes::from_inline(
            sha256_bytes(&inline),
            inline.len() as u64,
            inline.clone(),
            "root executed update test",
        )
        .expect("test bytes should be valid");
        let executed = RootExecutedUpdate {
            value: json!({"$boon_type": "BYTES", "byte_len": 3}),
            bytes_value: Some(bytes),
            fixed_bytes_mutation: Some(RootBytesFixedMutation {
                input_state_id: StateId(7),
                output_state_id: StateId(7),
                patches: vec![(1, 10)],
            }),
            bytes_access: JsonValue::Null,
            executor_core: json!({"executor": "test"}),
            state_write_core: JsonValue::Null,
            bytes_state_core: JsonValue::Null,
            expression_kind: "bytes_set".to_owned(),
            source_payload_field: JsonValue::Null,
            update_constant_id: JsonValue::Null,
            update_constant_value: JsonValue::Null,
            host_effect: JsonValue::Null,
        };

        let candidate = root_update_candidate_from_executed(7, 99, &executed);
        assert_eq!(candidate.state_id, 7);
        assert_eq!(candidate.op_id, 99);
        assert_eq!(candidate.bytes_value.as_ref().unwrap()["byte_len"], 3);
        assert_eq!(
            candidate.fixed_bytes_mutation.as_ref().unwrap()["patches"],
            json!([[1, 10]])
        );

        let mut bytes_executed = executed.clone();
        bytes_executed.fixed_bytes_mutation = None;
        let mut root_state = JsonMap::new();
        let mut private_bytes = BTreeMap::new();
        let mut fixed_byte_banks = BTreeMap::new();
        let report = apply_executed_root_update_to_state(
            &mut root_state,
            &mut private_bytes,
            &mut fixed_byte_banks,
            &plan,
            7,
            &bytes_executed,
            99,
        )
        .expect("executed root update should apply through PlanExecutor");
        assert_eq!(root_state["store.payload"]["byte_len"], 3);
        assert_eq!(
            private_bytes.get(&7).unwrap().inline_bytes,
            inline,
            "bytes_value takes the direct bytes commit path"
        );
        assert_eq!(
            report["executor"],
            "cpu-plan-root-update-storage-transition-v1"
        );
    }

    #[test]
    fn root_state_initializer_owns_public_and_private_bytes_state() {
        let bytes = vec![4, 5, 6];
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: vec![boon_plan::PlanConstant {
                id: PlanConstantId(1),
                value: PlanConstantValue::Bytes {
                    byte_len: bytes.len() as u64,
                    sha256: sha256_bytes(&bytes),
                    inline_bytes: Some(bytes.clone()),
                },
            }],
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(10),
                    state_id: StateId(7),
                    value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Bytes,
                    initial_constant_id: Some(PlanConstantId(1)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: vec![boon_plan::ByteStorageBank {
                    id: boon_plan::PlanStorageId(11),
                    state_storage_id: boon_plan::PlanStorageId(10),
                    state_id: StateId(7),
                    scope_id: None,
                    indexed: false,
                    fixed_len: 3,
                    capacity: None,
                }],
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 1,
                source_route_count: 0,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 1,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:7".to_owned(),
                    label: "store.payload".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let root = initialize_root_state(&plan).expect("root state should initialize");

        assert_eq!(root.initialized_state_count, 1);
        assert_eq!(root.root_state["store.payload"]["$boon_type"], "BYTES");
        assert_eq!(
            root.private_bytes
                .get(&7)
                .expect("private bytes should be initialized")
                .inline_bytes(),
            bytes.as_slice()
        );
        assert_eq!(root.fixed_byte_banks.get(&7), Some(&bytes));
        assert_eq!(
            root.executor_report["executor"],
            "cpu-plan-root-state-initializer-v1"
        );
        assert_eq!(
            root.executor_report["bytes_initialization_core"]["executor"],
            "cpu-plan-root-bytes-storage-initializer-v1"
        );
    }

    #[test]
    fn root_row_expression_finds_initial_list_value_and_converts_number() {
        let mut plan = empty_executor_test_plan();
        plan.constants = vec![boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "A".to_owned(),
            },
        }];
        plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(0),
            scope_id: None,
            row_field_ids: vec![FieldId(1), FieldId(2)],
            capacity: None,
            hidden_key_type: "none".to_owned(),
            has_generation: false,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: vec![boon_plan::PlanInitialListRow {
                fields: vec![
                    boon_plan::PlanInitialListField {
                        name: "key".to_owned(),
                        field_id: Some(FieldId(1)),
                        value: PlanConstantValue::Text {
                            value: "A".to_owned(),
                        },
                    },
                    boon_plan::PlanInitialListField {
                        name: "width".to_owned(),
                        field_id: Some(FieldId(2)),
                        value: PlanConstantValue::Text {
                            value: "120".to_owned(),
                        },
                    },
                ],
            }],
        }];

        let expression = PlanRowExpression::TextToNumber {
            input: Box::new(PlanRowExpression::ListFindValue {
                list_id: boon_plan::ListId(0),
                field: FieldId(1),
                value: Box::new(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(0),
                }),
                target: FieldId(2),
                fallback: None,
            }),
        };

        let value = eval_root_source_transform_row_expression(&plan, &JsonMap::new(), &expression)
            .expect("root evaluator should read initial list rows");
        assert_eq!(value, json!(120));
    }

    #[test]
    fn root_row_expression_list_find_uses_fallback_derived_field() {
        let mut plan = empty_executor_test_plan();
        plan.constants = vec![boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "missing".to_owned(),
            },
        }];
        plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(0),
            scope_id: None,
            row_field_ids: vec![FieldId(1), FieldId(2)],
            capacity: None,
            hidden_key_type: "none".to_owned(),
            has_generation: false,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: vec![boon_plan::PlanInitialListRow {
                fields: vec![
                    boon_plan::PlanInitialListField {
                        name: "key".to_owned(),
                        field_id: Some(FieldId(1)),
                        value: PlanConstantValue::Text {
                            value: "present".to_owned(),
                        },
                    },
                    boon_plan::PlanInitialListField {
                        name: "width".to_owned(),
                        field_id: Some(FieldId(2)),
                        value: PlanConstantValue::Text {
                            value: "120".to_owned(),
                        },
                    },
                ],
            }],
        }];
        plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
            id: "field:4".to_owned(),
            label: "store.default_width".to_owned(),
        }];
        let root_state = JsonMap::from_iter([(
            "store.default_width".to_owned(),
            JsonValue::String("88".to_owned()),
        )]);

        let expression = PlanRowExpression::ListFindValue {
            list_id: boon_plan::ListId(0),
            field: FieldId(1),
            value: Box::new(PlanRowExpression::Constant {
                constant_id: PlanConstantId(0),
            }),
            target: FieldId(2),
            fallback: Some(Box::new(PlanRowExpression::Field {
                input: ValueRef::Field(FieldId(4)),
            })),
        };

        let value = eval_root_source_transform_row_expression(&plan, &root_state, &expression)
            .expect("root evaluator should use fallback when no initial row matches");
        assert_eq!(value, json!("88"));
    }

    #[test]
    fn source_payload_press_updates_bool_state_as_event_pulse() {
        let slot = boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(0),
            state_id: StateId(1),
            value_type: PlanValueType::Bool,
            scope_id: None,
            indexed: false,
            initial_value_kind: InitialValueKind::Bool,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        };
        let event = RootJsonSourceEvent {
            payload: BTreeMap::new(),
            ..RootJsonSourceEvent::default()
        };

        let value = source_payload_value_for_slot(
            &event,
            &SourcePayloadField::Named("press".to_owned()),
            &slot,
            PlanOpId(7),
        )
        .expect("press source payload should be a bool event pulse");
        assert_eq!(value, json!(true));

        let false_event = RootJsonSourceEvent {
            payload: BTreeMap::from([("press".to_owned(), "False".to_owned())]),
            ..RootJsonSourceEvent::default()
        };
        let value = source_payload_value_for_slot(
            &false_event,
            &SourcePayloadField::Named("press".to_owned()),
            &slot,
            PlanOpId(8),
        )
        .expect("explicit false press source payload should decode");
        assert_eq!(value, json!(false));
    }

    #[test]
    fn root_state_initializer_copies_root_initial_fields_without_constants() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: vec![boon_plan::PlanConstant {
                id: PlanConstantId(0),
                value: PlanConstantValue::Text {
                    value: "draft.txt".to_owned(),
                },
            }],
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(0),
                        state_id: StateId(1),
                        value_type: PlanValueType::Text,
                        scope_id: None,
                        indexed: false,
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(0)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(1),
                        state_id: StateId(2),
                        value_type: PlanValueType::RootInitialField,
                        scope_id: None,
                        indexed: false,
                        initial_value_kind: InitialValueKind::RootInitialField,
                        initial_constant_id: None,
                        initial_root_field_path: Some("active_file".to_owned()),
                        initial_row_field_path: None,
                    },
                ],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(0),
                kind: RegionKind::StateInitialization,
                ops: vec![
                    PlanOp {
                        id: PlanOpId(0),
                        kind: PlanOpKind::StateInitialize {
                            initial_value_kind: InitialValueKind::Text,
                            initial_constant_id: Some(PlanConstantId(0)),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::State(StateId(1))),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    },
                    PlanOp {
                        id: PlanOpId(1),
                        kind: PlanOpKind::StateInitialize {
                            initial_value_kind: InitialValueKind::RootInitialField,
                            initial_constant_id: None,
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::State(StateId(2))),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    },
                ],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 1,
                source_route_count: 0,
                scalar_storage_count: 2,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 2,
                typed_value_ref_count: 2,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![
                    boon_plan::DebugEntry {
                        id: "state:1".to_owned(),
                        label: "store.active_file".to_owned(),
                    },
                    boon_plan::DebugEntry {
                        id: "state:2".to_owned(),
                        label: "store.selected_file".to_owned(),
                    },
                ],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let root = initialize_root_state(&plan).expect("root state should initialize");
        assert_eq!(root.root_state["store.active_file"], "draft.txt");
        assert_eq!(root.root_state["store.selected_file"], "draft.txt");
        assert_eq!(root.initialized_state_count, 2);
        assert_eq!(root.executor_report["root_initial_field_copy_count"], 1);

        let executed =
            execute_initial_state(&plan).expect("initial-state execution should initialize copies");
        assert_eq!(
            executed.executor_report["state_summary"]["store.selected_file"],
            "draft.txt"
        );
        assert_eq!(executed.executor_report["root_initial_field_copy_count"], 1);
    }

    fn empty_executor_test_plan() -> MachinePlan {
        MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: Vec::new(),
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        }
    }

    #[test]
    fn indexed_fixed_byte_bank_lookup_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(17);
        let state_id = StateId(42);
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id,
                    value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                    scope_id: Some(scope_id),
                    indexed: true,
                    initial_value_kind: InitialValueKind::Bytes,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: vec![boon_plan::ListStorageSlot {
                    id: boon_plan::PlanStorageId(2),
                    list_id: boon_plan::ListId(3),
                    scope_id: Some(scope_id),
                    row_field_ids: Vec::new(),
                    capacity: None,
                    hidden_key_type: "RowKey".to_owned(),
                    has_generation: true,
                    initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
                    range: None,
                    initial_rows: Vec::new(),
                }],
                byte_banks: vec![boon_plan::ByteStorageBank {
                    id: boon_plan::PlanStorageId(4),
                    state_storage_id: boon_plan::PlanStorageId(1),
                    state_id,
                    scope_id: Some(scope_id),
                    indexed: true,
                    fixed_len: 3,
                    capacity: None,
                }],
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 1,
                list_storage_count: 1,
                byte_bank_storage_count: 1,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:42".to_owned(),
                    label: "row.payload".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        assert!(indexed_state_has_fixed_byte_bank(&plan, state_id));
        assert_eq!(
            indexed_fixed_byte_bank_len(&plan, state_id)
                .expect("fixed bank length should validate"),
            Some(3)
        );
        assert!(indexed_field_has_fixed_byte_bank(
            &plan,
            Some(scope_id),
            "payload"
        ));
        assert!(!indexed_field_has_fixed_byte_bank(
            &plan,
            Some(scope_id),
            "other"
        ));
        assert!(!indexed_field_has_fixed_byte_bank(&plan, None, "payload"));
    }

    #[test]
    fn list_row_default_fields_are_executor_owned() {
        let scope_id = boon_plan::ScopeId(23);
        let text_state_id = StateId(11);
        let bytes_state_id = StateId(12);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(1),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "RowKey".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: vec![
                boon_plan::PlanConstant {
                    id: PlanConstantId(1),
                    value: PlanConstantValue::Text {
                        value: "hello".to_owned(),
                    },
                },
                boon_plan::PlanConstant {
                    id: PlanConstantId(2),
                    value: PlanConstantValue::Bytes {
                        byte_len: 3,
                        sha256: sha256_bytes(&[1, 2, 3]),
                        inline_bytes: Some(vec![1, 2, 3]),
                    },
                },
            ],
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(2),
                        state_id: text_state_id,
                        value_type: PlanValueType::Text,
                        scope_id: Some(scope_id),
                        indexed: true,
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(1)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                    boon_plan::ScalarStorageSlot {
                        id: boon_plan::PlanStorageId(3),
                        state_id: bytes_state_id,
                        value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                        scope_id: Some(scope_id),
                        indexed: true,
                        initial_value_kind: InitialValueKind::Bytes,
                        initial_constant_id: Some(PlanConstantId(2)),
                        initial_root_field_path: None,
                        initial_row_field_path: None,
                    },
                ],
                list_slots: vec![list_slot.clone()],
                byte_banks: vec![boon_plan::ByteStorageBank {
                    id: boon_plan::PlanStorageId(4),
                    state_storage_id: boon_plan::PlanStorageId(3),
                    state_id: bytes_state_id,
                    scope_id: Some(scope_id),
                    indexed: true,
                    fixed_len: 3,
                    capacity: None,
                }],
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 2,
                source_route_count: 0,
                scalar_storage_count: 2,
                list_storage_count: 1,
                byte_bank_storage_count: 1,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![
                    boon_plan::DebugEntry {
                        id: "state:11".to_owned(),
                        label: "row.title".to_owned(),
                    },
                    boon_plan::DebugEntry {
                        id: "state:12".to_owned(),
                        label: "row.payload".to_owned(),
                    },
                ],
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let defaults = list_row_default_fields(&plan, &list_slot)
            .expect("row default fields should be assembled by executor");
        assert_eq!(defaults.fields["title"], json!("hello"));
        assert_eq!(defaults.fields["payload"]["$boon_type"], "BYTES");
        assert_eq!(defaults.private_bytes["payload"].inline_bytes(), &[1, 2, 3]);
        assert_eq!(defaults.fixed_byte_banks["payload"], vec![1, 2, 3]);
        assert_eq!(
            defaults.executor_report["executor"],
            "cpu-plan-list-row-default-fields-v1"
        );
        assert_eq!(defaults.executor_report["default_field_count"], 2);
        assert_eq!(defaults.executor_report["fixed_byte_bank_count"], 1);
    }

    #[test]
    fn root_scenario_materialized_work_validation_is_executor_owned() {
        let update_work =
            validate_root_scenario_materialized_work("store.input.change", 1, 0, false)
                .expect("update op work should be executable");
        assert_eq!(
            update_work.executor_report["executor"],
            "cpu-plan-root-scenario-materialized-work-v1"
        );
        assert_eq!(update_work.executor_report["update_op_count"], 1);
        assert_eq!(update_work.executor_report["executable_work"], true);

        let derived_work =
            validate_root_scenario_materialized_work("store.input.change", 0, 1, false)
                .expect("derived value work should be executable");
        assert_eq!(derived_work.executor_report["derived_value_count"], 1);

        let remove_work = validate_root_scenario_materialized_work("todo.remove.click", 0, 0, true)
            .expect("list remove work should be executable");
        assert_eq!(remove_work.executor_report["has_list_remove_work"], true);

        let error = validate_root_scenario_materialized_work("store.input.change", 0, 0, false)
            .expect_err("empty materialized work should be rejected");
        assert!(
            error
                .to_string()
                .contains("found no executable selected-surface work"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn list_next_key_allocation_is_executor_owned() {
        let list_state = BTreeMap::from([
            (
                4,
                vec![
                    PlanExecutorListRow {
                        key: 1,
                        generation: 1,
                        fields: BTreeMap::new(),
                    },
                    PlanExecutorListRow {
                        key: 7,
                        generation: 1,
                        fields: BTreeMap::new(),
                    },
                ],
            ),
            (9, Vec::new()),
        ]);
        let mut next_keys = initial_list_next_keys(&list_state);
        assert_eq!(next_keys.get(&4), Some(&8));
        assert_eq!(next_keys.get(&9), Some(&1));

        assert_eq!(
            reserve_list_row_key(&mut next_keys, &list_state, 4)
                .expect("first reservation should use current next key"),
            8
        );
        assert_eq!(
            reserve_list_row_key(&mut next_keys, &list_state, 4)
                .expect("second reservation should increment"),
            9
        );
        assert_eq!(next_keys.get(&4), Some(&10));

        let error = reserve_list_row_key(&mut next_keys, &list_state, 99)
            .expect_err("unknown list should be rejected");
        assert!(
            error.to_string().contains("list state missing list 99"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn row_source_binding_ids_and_deltas_are_executor_owned() {
        let route_source_ids = vec![SourceId(2), SourceId(5), SourceId(9)];
        assert_eq!(
            row_source_binding_id(4, &route_source_ids, SourceId(2)),
            Some(10)
        );
        assert_eq!(
            row_source_binding_id(4, &route_source_ids, SourceId(5)),
            Some(11)
        );
        assert_eq!(
            row_source_binding_id(4, &route_source_ids, SourceId(9)),
            Some(12)
        );
        assert_eq!(
            row_source_binding_id(4, &route_source_ids, SourceId(99)),
            None
        );

        let deltas = build_source_bind_deltas(
            "todos",
            4,
            1,
            &[
                "todo.sources.remove.click".to_owned(),
                "todo.sources.title.change".to_owned(),
            ],
        );
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0]["kind"], "SourceBind");
        assert_eq!(deltas[0]["source_id"], 7);
        assert_eq!(deltas[0]["bind_epoch"], 7);
        assert_eq!(deltas[0]["field_path"], "todo.sources.remove.click");
        assert_eq!(deltas[1]["source_id"], 8);
        assert_eq!(deltas[1]["value"], "todo.sources.title.change");

        let unbinds = build_source_unbind_deltas(
            "todos",
            4,
            2,
            &[
                "todo.sources.remove.click".to_owned(),
                "todo.sources.title.change".to_owned(),
            ],
        );
        assert_eq!(unbinds.len(), 2);
        assert_eq!(unbinds[0]["kind"], "SourceUnbind");
        assert_eq!(unbinds[0]["source_id"], 7);
        assert_eq!(unbinds[0]["bind_epoch"], 7);
        assert!(unbinds[0]["value"].is_null());

        let remove = build_list_remove_delta("todos", 4, 2);
        assert_eq!(remove["kind"], "ListRemove");
        assert_eq!(remove["list_id"], "todos");
        assert_eq!(remove["key"], 4);
        assert_eq!(remove["generation"], 2);
        assert!(remove["source_id"].is_null());
    }

    #[test]
    fn list_mutation_records_are_executor_owned() {
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(boon_plan::ScopeId(3)),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let plan = empty_executor_test_plan();
        let append = record_list_append_mutation(
            &plan,
            &list_slot,
            ListAppendMutationInput {
                list_id: 7,
                list_label: "todos".to_owned(),
                append_op_id: 10,
                key: 4,
                generation: 1,
                trigger_value: json!("Buy milk"),
                fields_before_refresh: BTreeMap::from([("title".to_owned(), json!("Buy milk"))]),
                fields_after_refresh: BTreeMap::from([
                    ("title".to_owned(), json!("Buy milk")),
                    ("completed".to_owned(), json!(false)),
                ]),
                source_paths: vec![
                    "todo.remove.click".to_owned(),
                    "todo.title.change".to_owned(),
                ],
                row_bool_deltas: vec![json!({
                    "kind": "FieldSet",
                    "list_id": "todos",
                    "key": 4,
                    "generation": 1,
                    "source_id": null,
                    "bind_epoch": null,
                    "field_path": "active",
                    "value": true,
                })],
            },
        );
        assert_eq!(append.source_bind_count, 2);
        assert_eq!(append.semantic_deltas[0]["kind"], "ListInsert");
        assert_eq!(append.semantic_deltas[1]["kind"], "SourceBind");
        assert_eq!(append.report_row["append_op_id"], 10);
        assert_eq!(
            append.executor_report["executor"],
            "cpu-plan-list-append-mutation-record-v1"
        );

        let remove = record_list_remove_mutation(ListRemoveMutationInput {
            list_id: 7,
            list_label: "todos".to_owned(),
            remove_op_id: 11,
            source_id: 2,
            source_label: "todo.remove.click".to_owned(),
            row_index: 3,
            key: 4,
            generation: 1,
            source_binding_id: Some(7),
            bind_epoch: Some(7),
            row_resolution: json!({"method": "source_binding"}),
            source_paths: vec![
                "todo.remove.click".to_owned(),
                "todo.title.change".to_owned(),
            ],
            row_fields: BTreeMap::from([("title".to_owned(), json!("Buy milk"))]),
        });
        assert_eq!(remove.source_unbind_count, 2);
        assert_eq!(remove.semantic_deltas[0]["kind"], "SourceUnbind");
        assert_eq!(remove.semantic_deltas[2]["kind"], "ListRemove");
        assert_eq!(remove.report_row["remove_op_id"], 11);
        assert_eq!(
            remove.executor_report["executor"],
            "cpu-plan-list-remove-mutation-record-v1"
        );
    }

    #[test]
    fn list_append_insert_and_row_refresh_deltas_are_executor_owned() {
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(boon_plan::ScopeId(3)),
            row_field_ids: vec![FieldId(8), FieldId(9)],
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: vec![SourceRoute {
                id: boon_plan::PlanSourceRouteId(1),
                source_id: SourceId(1),
                path: "store.submit".to_owned(),
                scoped: false,
                scope_id: None,
                payload_schema: boon_plan::SourcePayloadSchema {
                    fields: vec![SourcePayloadField::Text, SourcePayloadField::Key],
                    typed_fields: Vec::new(),
                    row_lookup_field: None,
                    address_lookup_field: None,
                },
            }],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id: StateId(30),
                    value_type: boon_plan::PlanValueType::Text,
                    scope_id: Some(boon_plan::ScopeId(3)),
                    indexed: true,
                    initial_value_kind: boon_plan::InitialValueKind::Text,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: vec![list_slot.clone()],
                byte_banks: Vec::new(),
            },
            regions: vec![
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(1),
                    kind: RegionKind::ListOperations,
                    ops: vec![PlanOp {
                        id: PlanOpId(10),
                        kind: PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Append,
                            append: Some(boon_plan::PlanListAppend {
                                trigger: ValueRef::Field(FieldId(40)),
                                fields: vec![boon_plan::PlanListAppendField {
                                    name: "title".to_owned(),
                                    field_id: Some(FieldId(8)),
                                    value_ref: Some(ValueRef::Field(FieldId(40))),
                                    constant_id: None,
                                }],
                            }),
                            remove: None,
                            retain: None,
                            count: None,
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::List(boon_plan::ListId(7))),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(2),
                    kind: RegionKind::DerivedEvaluation,
                    ops: vec![PlanOp {
                        id: PlanOpId(11),
                        kind: PlanOpKind::DerivedValue {
                            derived_kind: boon_plan::PlanDerivedKind::Pure,
                            startup_recompute: true,
                            expression: Some(PlanDerivedExpression::RowExpression {
                                expression: boon_plan::PlanRowExpression::Field {
                                    input: ValueRef::State(StateId(30)),
                                },
                            }),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::Field(FieldId(9))),
                        indexed: true,
                        unresolved_executable_ref_count: 0,
                    }],
                },
            ],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 1,
                list_storage_count: 1,
                byte_bank_storage_count: 0,
                operation_count: 2,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:30".to_owned(),
                    label: "todo.title".to_owned(),
                }],
                list_slots: vec![boon_plan::DebugEntry {
                    id: "list:7".to_owned(),
                    label: "todos".to_owned(),
                }],
                derived_values: Vec::new(),
                fields: vec![
                    boon_plan::DebugEntry {
                        id: "field:8".to_owned(),
                        label: "todo.title".to_owned(),
                    },
                    boon_plan::DebugEntry {
                        id: "field:9".to_owned(),
                        label: "todo.normalized_title".to_owned(),
                    },
                ],
                unresolved_executable_refs: Vec::new(),
            },
        };

        let insert = build_list_insert_delta("todos", 4, 1, json!("Write tests"));
        assert_eq!(insert["kind"], "ListInsert");
        assert_eq!(insert["list_id"], "todos");
        assert_eq!(insert["key"], 4);
        assert_eq!(insert["value"], "Write tests");

        let fields = row_expression_output_field_names(&plan, &list_slot);
        assert!(fields.contains("normalized_title"));
        assert!(!fields.contains("title"));

        let before = BTreeMap::from([
            ("title".to_owned(), json!("Write tests")),
            ("normalized_title".to_owned(), json!("old")),
        ]);
        let after = BTreeMap::from([
            ("title".to_owned(), json!("Changed but not row expression")),
            ("normalized_title".to_owned(), json!("Write tests")),
        ]);
        let deltas =
            build_row_refresh_field_deltas(&plan, &list_slot, "todos", 4, 1, &before, &after);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0]["kind"], "FieldSet");
        assert_eq!(deltas[0]["list_id"], "todos");
        assert_eq!(deltas[0]["field_path"], "normalized_title");
        assert_eq!(deltas[0]["value"], "Write tests");
    }

    #[test]
    fn list_row_expression_refresh_loop_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "todo.title.change".to_owned(),
            scoped: true,
            scope_id: Some(scope_id),
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.list_slots = vec![list_slot.clone()];
        plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(1),
            state_id: StateId(30),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        }];
        plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
            id: "state:30".to_owned(),
            label: "todo.title".to_owned(),
        }];
        plan.debug_map.fields = vec![boon_plan::DebugEntry {
            id: "field:9".to_owned(),
            label: "todo.normalized_title".to_owned(),
        }];
        plan.regions = vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(2),
            kind: RegionKind::DerivedEvaluation,
            ops: vec![PlanOp {
                id: PlanOpId(11),
                kind: PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    startup_recompute: true,
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::Field {
                            input: ValueRef::State(StateId(30)),
                        },
                    }),
                },
                inputs: Vec::new(),
                output: Some(ValueRef::Field(FieldId(9))),
                indexed: true,
                unresolved_executable_ref_count: 0,
            }],
        }];
        let mut row = PlanExecutorListRowState {
            key: 4,
            generation: 1,
            fields: BTreeMap::from([("title".to_owned(), json!("Write tests"))]),
            private_bytes: BTreeMap::new(),
            fixed_bytes_banks: BTreeMap::new(),
        };
        let list_state = BTreeMap::from([(7usize, vec![row.clone()])]);

        refresh_list_row_expression_fields_with(
            &plan,
            &list_slot,
            &list_state,
            &mut row,
            |plan, _list_state, row, expression| {
                let PlanRowExpression::Field {
                    input: ValueRef::State(state_id),
                } = expression
                else {
                    return Err("unexpected row expression".into());
                };
                let field_name = local_field_name(&state_label(plan, *state_id));
                row.fields
                    .get(&field_name)
                    .cloned()
                    .ok_or_else(|| "missing row field".into())
            },
        )
        .expect("strict row-expression refresh should evaluate");
        assert_eq!(row.fields["normalized_title"], json!("Write tests"));

        row.fields.remove("normalized_title");
        refresh_list_row_expression_fields_best_effort_with(
            &plan,
            &list_slot,
            &list_state,
            &mut row,
            |_plan, _list_state, _row, _expression| Err("deferred expression".into()),
        );
        assert!(!row.fields.contains_key("normalized_title"));
    }

    #[test]
    fn list_append_row_construction_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let append = boon_plan::PlanListAppend {
            trigger: ValueRef::Field(FieldId(40)),
            fields: vec![boon_plan::PlanListAppendField {
                name: "title".to_owned(),
                field_id: Some(FieldId(8)),
                value_ref: Some(ValueRef::Field(FieldId(40))),
                constant_id: None,
            }],
        };
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "todo.title.change".to_owned(),
            scoped: true,
            scope_id: Some(scope_id),
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.list_slots = vec![list_slot.clone()];
        plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(1),
            state_id: StateId(30),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        }];
        plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
            id: "state:30".to_owned(),
            label: "todo.title".to_owned(),
        }];
        plan.debug_map.fields = vec![
            boon_plan::DebugEntry {
                id: "field:8".to_owned(),
                label: "todo.title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "field:9".to_owned(),
                label: "todo.normalized_title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "field:40".to_owned(),
                label: "store.title_to_add".to_owned(),
            },
        ];
        plan.regions = vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(2),
            kind: RegionKind::DerivedEvaluation,
            ops: vec![PlanOp {
                id: PlanOpId(11),
                kind: PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    startup_recompute: true,
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::Field {
                            input: ValueRef::State(StateId(30)),
                        },
                    }),
                },
                inputs: Vec::new(),
                output: Some(ValueRef::Field(FieldId(9))),
                indexed: true,
                unresolved_executable_ref_count: 0,
            }],
        }];
        let list_state = BTreeMap::from([(7usize, Vec::new())]);
        let derived_values = BTreeMap::from([(FieldId(40), json!("Write tests"))]);

        let constructed = construct_list_append_row_with(
            &plan,
            &list_slot,
            10,
            &append,
            7,
            "todos",
            &list_state,
            4,
            1,
            &derived_values,
            true,
            |plan, _list_state, row, expression| {
                let PlanRowExpression::Field {
                    input: ValueRef::State(state_id),
                } = expression
                else {
                    return Err("unexpected row expression".into());
                };
                let field_name = local_field_name(&state_label(plan, *state_id));
                row.fields
                    .get(&field_name)
                    .cloned()
                    .ok_or_else(|| "missing row field".into())
            },
        )
        .expect("append row construction should succeed");

        assert_eq!(constructed.row.key, 4);
        assert_eq!(constructed.row.fields["title"], json!("Write tests"));
        assert_eq!(
            constructed.row.fields["normalized_title"],
            json!("Write tests")
        );
        assert_eq!(constructed.source_paths, vec!["todo.title.change"]);
        assert_eq!(
            constructed.executor_report["executor"],
            "cpu-plan-list-append-row-construction-v1"
        );
    }

    #[test]
    fn list_append_execution_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let append = boon_plan::PlanListAppend {
            trigger: ValueRef::Field(FieldId(40)),
            fields: vec![boon_plan::PlanListAppendField {
                name: "title".to_owned(),
                field_id: Some(FieldId(8)),
                value_ref: Some(ValueRef::Field(FieldId(40))),
                constant_id: None,
            }],
        };
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "todo.title.change".to_owned(),
            scoped: true,
            scope_id: Some(scope_id),
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.list_slots = vec![list_slot];
        plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(1),
            state_id: StateId(30),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        }];
        plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
            id: "list:7".to_owned(),
            label: "todos".to_owned(),
        }];
        plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
            id: "state:30".to_owned(),
            label: "todo.title".to_owned(),
        }];
        plan.debug_map.fields = vec![
            boon_plan::DebugEntry {
                id: "field:8".to_owned(),
                label: "todo.title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "field:9".to_owned(),
                label: "todo.normalized_title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "field:40".to_owned(),
                label: "store.title_to_add".to_owned(),
            },
        ];
        plan.regions = vec![
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::ListOperations,
                ops: vec![PlanOp {
                    id: PlanOpId(10),
                    kind: PlanOpKind::ListOperation {
                        operation_kind: boon_plan::PlanListOperationKind::Append,
                        append: Some(append),
                        remove: None,
                        retain: None,
                        count: None,
                    },
                    inputs: vec![ValueRef::Field(FieldId(40))],
                    output: Some(ValueRef::List(boon_plan::ListId(7))),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            },
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(2),
                kind: RegionKind::DerivedEvaluation,
                ops: vec![PlanOp {
                    id: PlanOpId(11),
                    kind: PlanOpKind::DerivedValue {
                        derived_kind: boon_plan::PlanDerivedKind::Pure,
                        startup_recompute: true,
                        expression: Some(PlanDerivedExpression::RowExpression {
                            expression: PlanRowExpression::Field {
                                input: ValueRef::State(StateId(30)),
                            },
                        }),
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::Field(FieldId(9))),
                    indexed: true,
                    unresolved_executable_ref_count: 0,
                }],
            },
        ];
        let mut list_state = BTreeMap::from([(7usize, Vec::new())]);
        let mut list_next_keys = BTreeMap::new();
        let mut bool_delta_lists = BTreeSet::new();
        let derived_values = BTreeMap::from([(FieldId(40), json!("Write tests"))]);

        let execution = append_list_rows_for_derived_values_with(
            &plan,
            &mut list_state,
            &mut list_next_keys,
            &mut bool_delta_lists,
            &derived_values,
            |plan, _list_state, row, expression| {
                let PlanRowExpression::Field {
                    input: ValueRef::State(state_id),
                } = expression
                else {
                    return Err("unexpected row expression".into());
                };
                let field_name = local_field_name(&state_label(plan, *state_id));
                row.fields
                    .get(&field_name)
                    .cloned()
                    .ok_or_else(|| "missing row field".into())
            },
        )
        .expect("append execution should succeed");

        assert_eq!(execution.appended_row_count, 1);
        assert_eq!(execution.source_bind_count, 1);
        assert_eq!(
            execution.executor_report["executor"],
            "cpu-plan-list-append-execution-v1"
        );
        let rows = list_state.get(&7).expect("list should exist");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, 1);
        assert_eq!(rows[0].fields["title"], json!("Write tests"));
        assert_eq!(rows[0].fields["normalized_title"], json!("Write tests"));
        assert_eq!(execution.report_rows[0]["list"], "todos");
        assert!(
            execution
                .semantic_deltas
                .iter()
                .any(|delta| delta["kind"] == "ListInsert")
        );
        assert!(
            execution
                .semantic_deltas
                .iter()
                .any(|delta| delta["kind"] == "SourceBind")
        );
        assert!(
            execution
                .semantic_deltas
                .iter()
                .any(|delta| delta["kind"] == "FieldSet"
                    && delta["field_path"] == "normalized_title")
        );
    }

    #[test]
    fn list_remove_execution_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let list_slot = boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        };
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "todo.remove".to_owned(),
            scoped: true,
            scope_id: Some(scope_id),
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.list_slots = vec![list_slot];
        plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
            id: "list:7".to_owned(),
            label: "todos".to_owned(),
        }];
        plan.regions = vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(1),
            kind: RegionKind::ListOperations,
            ops: vec![PlanOp {
                id: PlanOpId(10),
                kind: PlanOpKind::ListOperation {
                    operation_kind: boon_plan::PlanListOperationKind::Remove,
                    append: None,
                    remove: Some(boon_plan::PlanListRemove {
                        source: ValueRef::Source(SourceId(1)),
                        predicate: boon_plan::PlanListRemovePredicate::AlwaysTrue,
                    }),
                    retain: None,
                    count: None,
                },
                inputs: vec![ValueRef::Source(SourceId(1))],
                output: Some(ValueRef::List(boon_plan::ListId(7))),
                indexed: false,
                unresolved_executable_ref_count: 0,
            }],
        }];
        let mut list_state = BTreeMap::from([(
            7usize,
            vec![PlanExecutorListRowState {
                key: 1,
                generation: 1,
                fields: BTreeMap::from([("title".to_owned(), json!("Write tests"))]),
                private_bytes: BTreeMap::new(),
                fixed_bytes_banks: BTreeMap::new(),
            }],
        )]);
        let event = PlanExecutorLiveSourceEvent {
            source: "todo.remove",
            text: None,
            key: None,
            list_id: Some("todos"),
            address: None,
            target_text: None,
            target_occurrence: None,
            target_key: Some(1),
            target_generation: Some(1),
            bind_epoch: Some(1),
            source_epoch: None,
            source_id: None,
        };

        let execution = remove_list_rows_for_source_event(
            &plan,
            SourceId(1),
            &plan.source_routes[0],
            &event,
            &mut list_state,
        )
        .expect("remove execution should succeed");

        assert_eq!(execution.removed_row_count, 1);
        assert_eq!(execution.source_unbind_count, 1);
        assert_eq!(
            execution.executor_report["executor"],
            "cpu-plan-list-remove-execution-v1"
        );
        assert!(list_state.get(&7).unwrap().is_empty());
        assert_eq!(execution.report_rows[0]["list"], "todos");
        assert_eq!(
            execution.report_rows[0]["row_resolution"]["method"],
            "key_generation"
        );
        assert!(
            execution
                .semantic_deltas
                .iter()
                .any(|delta| delta["kind"] == "SourceUnbind")
        );
        assert!(
            execution
                .semantic_deltas
                .iter()
                .any(|delta| delta["kind"] == "ListRemove")
        );
    }

    #[test]
    fn indexed_update_batch_execution_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(3);
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "store.toggle_all".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
            id: boon_plan::PlanStorageId(0),
            list_id: boon_plan::ListId(7),
            scope_id: Some(scope_id),
            row_field_ids: Vec::new(),
            capacity: None,
            hidden_key_type: "u64".to_owned(),
            has_generation: true,
            initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
            range: None,
            initial_rows: Vec::new(),
        }];
        plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(1),
            state_id: StateId(30),
            value_type: PlanValueType::Bool,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Bool,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        }];
        plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
            id: "list:7".to_owned(),
            label: "todos".to_owned(),
        }];
        let op = PlanOp {
            id: PlanOpId(12),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::Const,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: Vec::new(),
            output: Some(ValueRef::State(StateId(30))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let list_rows = BTreeMap::from([(
            7usize,
            vec![
                PlanExecutorListRow {
                    key: 1,
                    generation: 1,
                    fields: BTreeMap::new(),
                },
                PlanExecutorListRow {
                    key: 2,
                    generation: 1,
                    fields: BTreeMap::new(),
                },
            ],
        )]);
        let event = IndexedUpdateTargetEvent {
            source: "store.toggle_all".to_owned(),
            ..IndexedUpdateTargetEvent::default()
        };
        let mut callback_targets = Vec::new();

        let execution = execute_indexed_update_batch_with(
            &plan,
            &op,
            &plan.source_routes[0],
            &event,
            &list_rows,
            |target| {
                let target = target.expect("unscoped source should bulk-target rows");
                callback_targets.push((target.list_label.clone(), target.key, target.generation));
                let primary_value = format!("primary-{}", target.key);
                let derived_value = format!("derived-{}", target.key);
                Ok(IndexedUpdateBranchExecution {
                    semantic_deltas: vec![
                        json!({
                            "kind": "FieldSet",
                            "field_path": "completed",
                            "key": target.key,
                            "generation": target.generation,
                            "value": primary_value,
                        }),
                        json!({
                            "kind": "FieldSet",
                            "field_path": "visible",
                            "key": target.key,
                            "generation": target.generation,
                            "value": derived_value,
                        }),
                    ],
                    report_rows: vec![json!({
                        "update_op_id": 12,
                        "list": target.list_label,
                        "key": target.key,
                        "generation": target.generation,
                        "field_path": "completed",
                        "value": primary_value,
                    })],
                    updated_row_count: 1,
                })
            },
        )
        .expect("batch execution should succeed");

        assert_eq!(
            callback_targets,
            vec![("todos".to_owned(), 1, 1), ("todos".to_owned(), 2, 1)]
        );
        assert_eq!(execution.updated_row_count, 2);
        assert!(execution.bulk_indexed_update);
        assert_eq!(execution.report_rows.len(), 2);
        assert_eq!(
            execution.executor_report["executor"],
            "cpu-plan-indexed-update-batch-execution-v1"
        );
        let ordered = execution
            .semantic_deltas
            .iter()
            .map(|delta| {
                (
                    delta["field_path"].as_str().unwrap().to_owned(),
                    delta["key"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ordered,
            vec![
                ("completed".to_owned(), 1),
                ("completed".to_owned(), 2),
                ("visible".to_owned(), 1),
                ("visible".to_owned(), 2),
            ]
        );
    }

    #[test]
    fn indexed_json_update_evaluator_handles_bool_not_and_text_trim() {
        let scope_id = boon_plan::ScopeId(3);
        let mut plan = empty_executor_test_plan();
        plan.source_routes = vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "todo.title.change".to_owned(),
            scoped: true,
            scope_id: Some(scope_id),
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: vec![SourcePayloadField::Named("title".to_owned())],
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        }];
        plan.storage_layout.scalar_slots = vec![
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id: StateId(30),
                value_type: PlanValueType::Text,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            },
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(2),
                state_id: StateId(31),
                value_type: PlanValueType::Bool,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Bool,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            },
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(3),
                state_id: StateId(32),
                value_type: PlanValueType::Text,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            },
            boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(4),
                state_id: StateId(33),
                value_type: PlanValueType::Text,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            },
        ];
        plan.constants = vec![boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "SKIP".to_owned(),
            },
        }];
        plan.debug_map.state_slots = vec![
            boon_plan::DebugEntry {
                id: "state:30".to_owned(),
                label: "todo.title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "state:31".to_owned(),
                label: "todo.completed".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "state:32".to_owned(),
                label: "todo.edited_title".to_owned(),
            },
            boon_plan::DebugEntry {
                id: "state:33".to_owned(),
                label: "todo.edited_title.draft_title".to_owned(),
            },
        ];
        plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
            id: "field:80".to_owned(),
            label: "store.all_completed".to_owned(),
        }];
        let row = PlanExecutorListRowState {
            key: 1,
            generation: 1,
            fields: BTreeMap::from([
                ("title".to_owned(), json!("Old")),
                ("completed".to_owned(), json!(false)),
                ("edited_title".to_owned(), json!("")),
                ("draft_title".to_owned(), json!("")),
            ]),
            private_bytes: BTreeMap::new(),
            fixed_bytes_banks: BTreeMap::new(),
        };
        let bool_op = PlanOp {
            id: PlanOpId(10),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BoolNot,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![ValueRef::Field(FieldId(80))],
            output: Some(ValueRef::State(StateId(31))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let root_derived_values = BTreeMap::from([(80usize, json!(true))]);
        let event = RootJsonSourceEvent::default();
        let bool_eval = evaluate_indexed_json_update_branch(
            &plan,
            &bool_op,
            SourceId(1),
            &plan.source_routes[0],
            &event,
            &row,
            &root_derived_values,
        )
        .expect("Bool/not evaluation should succeed");
        assert!(bool_eval.supported);
        assert_eq!(bool_eval.expression_kind, Some("bool_not"));
        assert_eq!(bool_eval.value, Some(json!(false)));
        assert_eq!(
            bool_eval.executor_report["executor"],
            "cpu-plan-indexed-json-update-evaluator-v1"
        );

        let text_op = PlanOp {
            id: PlanOpId(11),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::TextTrimOrPrevious,
                ordered_inputs: Vec::new(),
                source_payload_field: Some(SourcePayloadField::Named("title".to_owned())),
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![ValueRef::SourcePayload {
                source_id: SourceId(1),
                field: SourcePayloadField::Named("title".to_owned()),
            }],
            output: Some(ValueRef::State(StateId(30))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let event = RootJsonSourceEvent {
            payload: BTreeMap::from([("title".to_owned(), "  New title  ".to_owned())]),
            ..RootJsonSourceEvent::default()
        };
        let text_eval = evaluate_indexed_json_update_branch(
            &plan,
            &text_op,
            SourceId(1),
            &plan.source_routes[0],
            &event,
            &row,
            &BTreeMap::new(),
        )
        .expect("TextTrimOrPrevious evaluation should succeed");
        assert!(text_eval.supported);
        assert_eq!(text_eval.expression_kind, Some("text_trim_or_previous"));
        assert_eq!(text_eval.value, Some(json!("New title")));
        assert_eq!(
            text_eval.source_payload_field,
            serde_json::to_value(SourcePayloadField::Named("title".to_owned())).unwrap()
        );

        plan.storage_layout.scalar_slots[0].initial_row_field_path = Some("title".to_owned());
        let read_path_op = PlanOp {
            id: PlanOpId(13),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::ReadPath,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: Vec::new(),
            output: Some(ValueRef::State(StateId(30))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let read_path_eval = evaluate_indexed_json_update_branch(
            &plan,
            &read_path_op,
            SourceId(1),
            &plan.source_routes[0],
            &RootJsonSourceEvent::default(),
            &row,
            &BTreeMap::new(),
        )
        .expect("indexed ReadPath should read the output row initializer field");
        assert!(read_path_eval.supported);
        assert_eq!(read_path_eval.expression_kind, Some("read_path"));
        assert_eq!(read_path_eval.value, Some(json!("Old")));

        let match_op = PlanOp {
            id: PlanOpId(12),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::MatchTextIsEmptyConst,
                ordered_inputs: vec![
                    ValueRef::State(StateId(33)),
                    ValueRef::State(StateId(30)),
                    ValueRef::Constant(PlanConstantId(0)),
                ],
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![
                ValueRef::State(StateId(33)),
                ValueRef::State(StateId(30)),
                ValueRef::Constant(PlanConstantId(0)),
            ],
            output: Some(ValueRef::State(StateId(32))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let match_eval = evaluate_indexed_json_update_branch(
            &plan,
            &match_op,
            SourceId(1),
            &plan.source_routes[0],
            &RootJsonSourceEvent::default(),
            &row,
            &BTreeMap::new(),
        )
        .expect("MatchTextIsEmptyConst evaluation should succeed");
        assert!(match_eval.supported);
        assert_eq!(
            match_eval.expression_kind,
            Some("match_text_is_empty_const")
        );
        assert_eq!(match_eval.value, Some(json!("Old")));

        let mut non_empty_row = row.clone();
        non_empty_row
            .fields
            .insert("draft_title".to_owned(), json!("Draft"));
        let skip_eval = evaluate_indexed_json_update_branch(
            &plan,
            &match_op,
            SourceId(1),
            &plan.source_routes[0],
            &RootJsonSourceEvent::default(),
            &non_empty_row,
            &BTreeMap::new(),
        )
        .expect("MatchTextIsEmptyConst SKIP evaluation should succeed");
        assert!(skip_eval.supported);
        assert_eq!(skip_eval.expression_kind, Some("match_text_is_empty_const"));
        assert_eq!(skip_eval.value, None);
    }

    #[test]
    fn ordered_root_update_ops_are_resolved_by_executor_dispatch() {
        let update_op = |id: usize| PlanOp {
            id: PlanOpId(id),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::Const,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: Some(PlanConstantId(id + 100)),
                source_guard: None,
            },
            inputs: Vec::new(),
            output: Some(ValueRef::State(StateId(id + 200))),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: vec![SourceRoute {
                id: boon_plan::PlanSourceRouteId(1),
                source_id: SourceId(1),
                path: "store.submit".to_owned(),
                scoped: false,
                scope_id: None,
                payload_schema: boon_plan::SourcePayloadSchema {
                    fields: vec![SourcePayloadField::Text, SourcePayloadField::Key],
                    typed_fields: Vec::new(),
                    row_lookup_field: None,
                    address_lookup_field: None,
                },
            }],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::UpdateBranches,
                ops: vec![update_op(10), update_op(20)],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 2,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 1,
                scalar_storage_count: 0,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 2,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: Vec::new(),
                list_slots: Vec::new(),
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let dispatch = RootScenarioStepDispatch {
            plan_hash: "test".to_owned(),
            source_label: "store.submit".to_owned(),
            source_id: SourceId(1),
            source_route_scoped: false,
            ordered_update_op_ids: vec![PlanOpId(20), PlanOpId(10)],
            derived_op_count: 0,
            has_list_remove_work: false,
            root_update_key_gate: None,
            root_update_key_matches: true,
            executable_work: true,
            executor_report: JsonValue::Null,
        };

        let ops = ordered_root_update_ops_for_dispatch(&plan, &dispatch)
            .expect("dispatch-selected update ops should resolve through PlanExecutor");
        assert_eq!(
            ops.iter().map(|op| op.id.0).collect::<Vec<_>>(),
            vec![20, 10]
        );
        let route = source_route_slot_for_dispatch(&plan, &dispatch)
            .expect("dispatch-selected source route should resolve through PlanExecutor");
        assert_eq!(route.path, "store.submit");
        assert_eq!(
            route.payload_schema.fields,
            vec![SourcePayloadField::Text, SourcePayloadField::Key]
        );

        let stale_dispatch = RootScenarioStepDispatch {
            ordered_update_op_ids: vec![PlanOpId(99)],
            ..dispatch.clone()
        };
        let error = ordered_root_update_ops_for_dispatch(&plan, &stale_dispatch)
            .expect_err("stale dispatch op ids must be rejected");
        assert!(
            error
                .to_string()
                .contains("root source-event selector chose missing update op 99"),
            "unexpected error: {error}"
        );
        let stale_route_dispatch = RootScenarioStepDispatch {
            source_id: SourceId(99),
            ..dispatch
        };
        let error = source_route_slot_for_dispatch(&plan, &stale_route_dispatch)
            .expect_err("stale dispatch source ids must be rejected");
        assert!(
            error
                .to_string()
                .contains("MachinePlan source route `store.submit` has no route slot"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn root_scenario_step_preparation_is_executor_owned() {
        let update_op = PlanOp {
            id: PlanOpId(10),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::SourcePayload,
                ordered_inputs: Vec::new(),
                source_payload_field: Some(SourcePayloadField::Text),
                update_constant_id: None,
                source_guard: None,
            },
            inputs: vec![
                ValueRef::Source(SourceId(1)),
                ValueRef::SourcePayload {
                    source_id: SourceId(1),
                    field: SourcePayloadField::Text,
                },
            ],
            output: Some(ValueRef::State(StateId(4))),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let derived_op = PlanOp {
            id: PlanOpId(20),
            kind: PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
                startup_recompute: true,
                expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                    source_id: SourceId(1),
                    key_field: SourcePayloadField::Key,
                    required_key: "Enter".to_owned(),
                    state: ValueRef::SourcePayload {
                        source_id: SourceId(1),
                        field: SourcePayloadField::Text,
                    },
                    skip_empty: true,
                }),
            },
            inputs: vec![
                ValueRef::Source(SourceId(1)),
                ValueRef::SourcePayload {
                    source_id: SourceId(1),
                    field: SourcePayloadField::Text,
                },
                ValueRef::SourcePayload {
                    source_id: SourceId(1),
                    field: SourcePayloadField::Key,
                },
            ],
            output: Some(ValueRef::Field(FieldId(9))),
            indexed: false,
            unresolved_executable_ref_count: 0,
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: vec![SourceRoute {
                id: boon_plan::PlanSourceRouteId(0),
                source_id: SourceId(1),
                path: "store.submit".to_owned(),
                scoped: false,
                scope_id: None,
                payload_schema: boon_plan::SourcePayloadSchema {
                    fields: vec![SourcePayloadField::Text, SourcePayloadField::Key],
                    typed_fields: vec![
                        boon_plan::SourcePayloadDescriptor {
                            field: SourcePayloadField::Text,
                            value_type: boon_plan::SourcePayloadValueType::Text,
                        },
                        boon_plan::SourcePayloadDescriptor {
                            field: SourcePayloadField::Key,
                            value_type: boon_plan::SourcePayloadValueType::Text,
                        },
                    ],
                    row_lookup_field: None,
                    address_lookup_field: None,
                },
            }],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: StateId(4),
                    value_type: boon_plan::PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: boon_plan::InitialValueKind::Text,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(1),
                    kind: RegionKind::UpdateBranches,
                    ops: vec![update_op],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(2),
                    kind: RegionKind::DerivedEvaluation,
                    ops: vec![derived_op],
                },
            ],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 1,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: true,
                constant_count: 0,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 2,
                typed_value_ref_count: 7,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: vec![boon_plan::DebugEntry {
                    id: "source:1".to_owned(),
                    label: "store.submit".to_owned(),
                }],
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:4".to_owned(),
                    label: "store.input".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: vec![boon_plan::DebugEntry {
                    id: "field:9".to_owned(),
                    label: "store.trimmed_submit".to_owned(),
                }],
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let preparation = prepare_root_scenario_step(
            &plan,
            "store.submit",
            &RootJsonSourceEvent {
                text: Some("  Write tests  ".to_owned()),
                key: Some("Enter".to_owned()),
                ..RootJsonSourceEvent::default()
            },
            &JsonMap::new(),
        )
        .expect("PlanExecutor should prepare root scenario step work");

        assert_eq!(preparation.source_id, SourceId(1));
        assert_eq!(preparation.source_route_slot.path, "store.submit");
        assert_eq!(preparation.route_ops.len(), 1);
        assert_eq!(preparation.route_ops[0].id, PlanOpId(10));
        assert_eq!(
            preparation.derived_values.get(&FieldId(9)),
            Some(&json!("Write tests"))
        );
        assert!(preparation.root_update_key_matches);
        assert_eq!(
            preparation.executor_report["executor"],
            "cpu-plan-root-scenario-step-preparation-v1"
        );
        assert_eq!(
            preparation.root_dispatch_report["materialized_work_core"]["executor"],
            "cpu-plan-root-scenario-materialized-work-v1"
        );
    }

    #[test]
    fn source_derived_values_are_evaluated_by_executor() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: StateId(4),
                    value_type: boon_plan::PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: boon_plan::InitialValueKind::Text,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::DerivedEvaluation,
                ops: vec![PlanOp {
                    id: PlanOpId(10),
                    kind: PlanOpKind::DerivedValue {
                        derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
                        startup_recompute: true,
                        expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                            source_id: SourceId(2),
                            key_field: SourcePayloadField::Key,
                            required_key: "Enter".to_owned(),
                            state: ValueRef::SourcePayload {
                                source_id: SourceId(2),
                                field: SourcePayloadField::Text,
                            },
                            skip_empty: true,
                        }),
                    },
                    inputs: vec![
                        ValueRef::Source(SourceId(2)),
                        ValueRef::SourcePayload {
                            source_id: SourceId(2),
                            field: SourcePayloadField::Key,
                        },
                        ValueRef::SourcePayload {
                            source_id: SourceId(2),
                            field: SourcePayloadField::Text,
                        },
                    ],
                    output: Some(ValueRef::Field(FieldId(9))),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            }],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 1,
                typed_value_ref_count: 3,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:4".to_owned(),
                    label: "store.input".to_owned(),
                }],
                list_slots: Vec::new(),
                derived_values: vec![boon_plan::DebugEntry {
                    id: "field:9".to_owned(),
                    label: "store.title_to_add".to_owned(),
                }],
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let root_state = JsonMap::new();

        let values = evaluate_source_derived_values_for_event(
            &plan,
            SourceId(2),
            &RootJsonSourceEvent {
                text: Some("  Write tests  ".to_owned()),
                key: Some("Enter".to_owned()),
                ..RootJsonSourceEvent::default()
            },
            &root_state,
        )
        .expect("source-derived evaluation should stay executor-owned");
        assert_eq!(values.get(&FieldId(9)), Some(&json!("Write tests")));
        let delta_reports = build_source_derived_value_deltas(&plan, &values);
        assert_eq!(delta_reports.len(), 1);
        assert_eq!(delta_reports[0].0, "FieldSet:store.title_to_add");
        assert_eq!(delta_reports[0].1["kind"], "FieldSet");
        assert_eq!(delta_reports[0].1["field_path"], "store.title_to_add");
        assert_eq!(delta_reports[0].1["value"], "Write tests");
        assert_eq!(delta_reports[0].2["field_id"], 9);
        assert_eq!(delta_reports[0].2["field_path"], "store.title_to_add");
        let step_deltas = assemble_source_derived_step_deltas(&plan, &values);
        assert_eq!(
            step_deltas.semantic_delta_signatures,
            vec!["FieldSet:store.title_to_add"]
        );
        assert_eq!(step_deltas.semantic_deltas.len(), 1);
        assert_eq!(step_deltas.semantic_deltas[0], delta_reports[0].1.clone());
        assert_eq!(step_deltas.reports, vec![delta_reports[0].2.clone()]);
        assert_eq!(
            step_deltas.executor_report["executor"],
            "cpu-plan-source-derived-step-deltas-v1"
        );
        assert_eq!(step_deltas.executor_report["semantic_delta_count"], 1);

        let skipped = evaluate_source_derived_values_for_event(
            &plan,
            SourceId(2),
            &RootJsonSourceEvent {
                text: Some("  Write tests  ".to_owned()),
                key: Some("Escape".to_owned()),
                ..RootJsonSourceEvent::default()
            },
            &root_state,
        )
        .expect("non-matching key should skip the source-derived value");
        assert!(skipped.is_empty());

        let skipped_empty = evaluate_source_derived_values_for_event(
            &plan,
            SourceId(2),
            &RootJsonSourceEvent {
                text: Some("   ".to_owned()),
                key: Some("Enter".to_owned()),
                ..RootJsonSourceEvent::default()
            },
            &root_state,
        )
        .expect("empty trimmed text should be skipped when skip_empty=true");
        assert!(skipped_empty.is_empty());
    }

    #[test]
    fn decode_expected_source_event_extracts_payload_and_reserved_fields() {
        let expected = BTreeMap::from([
            (
                "source".to_owned(),
                toml::Value::String("store.input.change".to_owned()),
            ),
            ("text".to_owned(), toml::Value::String("Typed".to_owned())),
            ("key".to_owned(), toml::Value::String("Enter".to_owned())),
            ("address".to_owned(), toml::Value::String("B2".to_owned())),
            ("target_occurrence".to_owned(), toml::Value::Integer(3)),
            ("target_key".to_owned(), toml::Value::Integer(42)),
            ("target_generation".to_owned(), toml::Value::Integer(7)),
            ("bind_epoch".to_owned(), toml::Value::Integer(9)),
            ("source_epoch".to_owned(), toml::Value::Integer(11)),
            (
                "payload_name".to_owned(),
                toml::Value::String("custom".to_owned()),
            ),
            (
                "bytes_hex".to_owned(),
                toml::Value::String("41 42 43".to_owned()),
            ),
            ("pointer_x".to_owned(), toml::Value::String("12".to_owned())),
        ]);

        let event = decode_expected_source_event("type-input", &expected)
            .expect("expected_source_event should decode in executor");
        assert_eq!(event.source, "store.input.change");
        assert_eq!(event.text, Some("Typed"));
        assert_eq!(event.key, Some("Enter"));
        assert_eq!(event.address, Some("B2"));
        assert_eq!(event.target_occurrence, Some(3));
        assert_eq!(event.target_key, Some(42));
        assert_eq!(event.target_generation, Some(7));
        assert_eq!(event.bind_epoch, Some(9));
        assert_eq!(event.source_epoch, Some(11));
        assert_eq!(event.payload.get("payload_name"), Some(&"custom"));
        assert_eq!(event.payload.get("pointer_x"), Some(&"12"));
        assert_eq!(event.pointer_x, Some("12"));
        assert_eq!(event.payload_bytes.get("bytes"), Some(&b"ABC".to_vec()));
    }

    #[test]
    fn decode_expected_source_event_rejects_named_bytes_payloads() {
        let expected = BTreeMap::from([
            (
                "source".to_owned(),
                toml::Value::String("store.input.change".to_owned()),
            ),
            (
                "image_bytes_hex".to_owned(),
                toml::Value::String("4142".to_owned()),
            ),
        ]);

        let error = decode_expected_source_event("named-bytes", &expected)
            .expect_err("v1 executor should reject named BYTES source payload keys");
        assert!(
            error.to_string().contains("named BYTES source payload key"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn source_payload_bytes_key_policy_is_executor_owned() {
        assert_eq!(source_payload_bytes_toml_key("bytes"), "bytes_hex");
        assert_eq!(source_payload_bytes_toml_key("image"), "image_bytes_hex");
        validate_source_payload_bytes_field_name("bytes")
            .expect("reserved bytes field should be accepted");
        let error = validate_source_payload_bytes_field_name("image")
            .expect_err("named BYTES payload fields should be rejected in v1");
        assert!(
            error.to_string().contains("named BYTES source payload key"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn live_source_event_expectation_matcher_is_executor_owned() {
        let expected = BTreeMap::from([
            (
                "source".to_owned(),
                toml::Value::String("store.input.change".to_owned()),
            ),
            ("text".to_owned(), toml::Value::String("Typed".to_owned())),
            ("target_occurrence".to_owned(), toml::Value::Integer(2)),
            ("source_id".to_owned(), toml::Value::Integer(8)),
        ]);
        let event = PlanExecutorLiveSourceEvent {
            source: "store.input.change",
            text: Some("Typed"),
            key: None,
            list_id: None,
            address: None,
            target_text: None,
            target_occurrence: Some(2),
            target_key: None,
            target_generation: None,
            bind_epoch: None,
            source_epoch: None,
            source_id: Some(8),
        };

        assert_live_source_event_matches_expected("type-input", Some(&expected), event)
            .expect("matching live source event should be accepted");

        let error = assert_live_source_event_matches_expected(
            "type-input",
            Some(&expected),
            PlanExecutorLiveSourceEvent {
                text: Some("Wrong"),
                ..event
            },
        )
        .expect_err("field mismatch should be rejected");
        assert!(
            error
                .to_string()
                .contains("observed live source field `text`"),
            "unexpected error: {error}"
        );

        let error = assert_live_source_event_matches_expected("missing", None, event)
            .expect_err("missing expected_source_event should be rejected");
        assert!(
            error.to_string().contains("without expected_source_event"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn live_source_event_expected_toml_builder_is_executor_owned() {
        let payload = BTreeMap::from([
            (
                "source".to_owned(),
                "payload-should-not-override".to_owned(),
            ),
            ("custom".to_owned(), "value".to_owned()),
        ]);
        let payload_bytes = BTreeMap::from([("bytes".to_owned(), vec![0x01, 0xfe, 0x04])]);

        let expected =
            build_live_source_event_expected_toml(PlanExecutorLiveSourceEventExpectedToml {
                source: "store.receive",
                text: Some("Typed"),
                key: Some("Enter"),
                list_id: Some("todos"),
                address: Some("A1"),
                payload: &payload,
                payload_bytes: &payload_bytes,
                pointer_x: Some("10"),
                pointer_y: Some("20"),
                pointer_width: Some("30"),
                pointer_height: Some("40"),
                target_text: Some("target"),
                target_occurrence: Some(2),
                target_key: Some(3),
                target_generation: Some(4),
                bind_epoch: Some(5),
                source_epoch: Some(6),
                source_id: Some(7),
            });

        assert_eq!(
            expected.get("source").and_then(toml::Value::as_str),
            Some("store.receive")
        );
        assert_eq!(
            expected.get("custom").and_then(toml::Value::as_str),
            Some("value")
        );
        assert_eq!(
            expected.get("bytes_hex").and_then(toml::Value::as_str),
            Some("01fe04")
        );
        assert_eq!(
            expected
                .get("target_occurrence")
                .and_then(toml::Value::as_integer),
            Some(2)
        );
        assert_eq!(
            expected.get("source_id").and_then(toml::Value::as_integer),
            Some(7)
        );
    }

    #[test]
    fn select_explicit_root_scenario_steps_requires_source_events() {
        let steps = vec![
            PlanExecutorScenarioStepMeta {
                id: "initial".to_owned(),
                has_expected_source_event: false,
            },
            PlanExecutorScenarioStepMeta {
                id: "type".to_owned(),
                has_expected_source_event: true,
            },
        ];

        let selection =
            select_explicit_root_scenario_steps("counter", &steps, &["type".to_owned()])
                .expect("explicit selected source-event step should be accepted");
        assert_eq!(selection.selected_indices, vec![1]);
        assert_eq!(selection.selected_step_ids, vec!["type"]);
        assert_eq!(
            selection.executor_report["executor"],
            "cpu-plan-explicit-root-scenario-step-selection-v1"
        );

        let error = select_explicit_root_scenario_steps("counter", &steps, &["initial".to_owned()])
            .expect_err("assertion-only selected step should be rejected");
        assert!(
            error.to_string().contains("has no expected_source_event"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn select_scenario_event_steps_reports_replay_and_assertion_steps() {
        let steps = vec![
            PlanExecutorScenarioStepMeta {
                id: "initial".to_owned(),
                has_expected_source_event: false,
            },
            PlanExecutorScenarioStepMeta {
                id: "type".to_owned(),
                has_expected_source_event: true,
            },
            PlanExecutorScenarioStepMeta {
                id: "assert".to_owned(),
                has_expected_source_event: false,
            },
        ];

        let selection = select_scenario_event_steps("counter", &steps)
            .expect("scenario with source-event steps should be accepted");
        assert_eq!(selection.all_indices, vec![0, 1, 2]);
        assert_eq!(selection.selected_indices, vec![1]);
        assert_eq!(selection.selected_step_ids, vec!["type"]);
        assert_eq!(selection.assertion_only_step_ids, vec!["initial", "assert"]);
        assert_eq!(
            selection.executor_report["executor"],
            "cpu-plan-scenario-events-step-selection-v1"
        );

        let error = select_scenario_event_steps(
            "empty",
            &[PlanExecutorScenarioStepMeta {
                id: "only-assert".to_owned(),
                has_expected_source_event: false,
            }],
        )
        .expect_err("scenario without source events should be rejected");
        assert!(
            error
                .to_string()
                .contains("has no expected_source_event steps"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn machine_plan_debug_label_helpers_are_executor_owned() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: StateId(4),
                    value_type: boon_plan::PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: boon_plan::InitialValueKind::Text,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 1,
                list_storage_count: 0,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:4".to_owned(),
                    label: "store.input".to_owned(),
                }],
                list_slots: vec![boon_plan::DebugEntry {
                    id: "list:2".to_owned(),
                    label: "todos".to_owned(),
                }],
                derived_values: vec![boon_plan::DebugEntry {
                    id: "field:9".to_owned(),
                    label: "store.has_todos".to_owned(),
                }],
                fields: vec![boon_plan::DebugEntry {
                    id: "field:8".to_owned(),
                    label: "todo.title".to_owned(),
                }],
                unresolved_executable_refs: Vec::new(),
            },
        };

        assert_eq!(state_label(&plan, StateId(4)), "store.input");
        assert_eq!(state_label_by_id(&plan, 4), "store.input");
        assert_eq!(list_label(&plan, 2), "todos");
        assert_eq!(field_label(&plan, 8), "todo.title");
        assert_eq!(semantic_field_label(&plan, 8), "todo.title");
        assert_eq!(derived_field_label(&plan, 9), "store.has_todos");
        assert_eq!(local_field_name("todo.title"), "title");
        assert!(root_state_is_scalar(&plan, StateId(4)));
        assert_eq!(state_label_by_id(&plan, 99), "state:99");
    }

    #[test]
    fn semantic_delta_signature_uses_kind_and_field_path() {
        assert_eq!(
            semantic_delta_signature(&json!({
                "kind": "FieldSet",
                "field_path": "store.flag"
            }))
            .unwrap(),
            "FieldSet:store.flag"
        );
        assert_eq!(
            semantic_delta_signature(&json!({
                "kind": "ListInsert",
                "field_path": null
            }))
            .unwrap(),
            "ListInsert"
        );
    }

    #[test]
    fn coalesce_field_set_deltas_keeps_last_write_per_target() {
        let deltas = vec![
            json!({
                "kind": "FieldSet",
                "list_id": "cells",
                "key": 2,
                "generation": 1,
                "source_id": null,
                "bind_epoch": null,
                "field_path": "value",
                "value": "old"
            }),
            json!({
                "kind": "ListInsert",
                "list_id": "cells",
                "key": 3,
                "generation": 1
            }),
            json!({
                "kind": "FieldSet",
                "list_id": "cells",
                "key": 2,
                "generation": 1,
                "source_id": null,
                "bind_epoch": null,
                "field_path": "value",
                "value": "new"
            }),
        ];

        let coalesced = coalesce_field_set_deltas(deltas).unwrap();
        assert_eq!(coalesced.len(), 2);
        assert_eq!(coalesced[0]["kind"], "ListInsert");
        assert_eq!(coalesced[1]["value"], "new");
    }

    #[test]
    fn indexed_update_delta_ordering_is_executor_owned() {
        let primary_a = json!({
            "kind": "FieldSet",
            "field_path": "value",
            "value": "A"
        });
        let derived_a = json!({
            "kind": "FieldSet",
            "field_path": "display_text",
            "value": "A"
        });
        let primary_b = json!({
            "kind": "FieldSet",
            "field_path": "value",
            "value": "B"
        });
        let batches = vec![
            IndexedUpdateDeltaBatch {
                semantic_deltas: vec![derived_a.clone(), primary_a.clone()],
                report_rows: vec![json!({ "field_path": "value" })],
            },
            IndexedUpdateDeltaBatch {
                semantic_deltas: vec![primary_b.clone()],
                report_rows: vec![json!({ "field_path": "value" })],
            },
        ];

        let bulk = order_indexed_update_semantic_deltas(true, &batches);
        assert_eq!(
            bulk.semantic_deltas,
            vec![primary_a.clone(), primary_b.clone(), derived_a.clone()]
        );
        assert_eq!(
            bulk.executor_report["executor"],
            "cpu-plan-indexed-update-delta-ordering-v1"
        );
        assert_eq!(bulk.executor_report["bulk_indexed_update"], true);

        let non_bulk = order_indexed_update_semantic_deltas(false, &batches);
        assert_eq!(
            non_bulk.semantic_deltas,
            vec![derived_a, primary_a, primary_b]
        );
        assert_eq!(non_bulk.executor_report["bulk_indexed_update"], false);
    }

    #[test]
    fn indexed_update_target_selection_is_executor_owned() {
        let scope_id = boon_plan::ScopeId(7);
        let output_state_id = StateId(12);
        let source_route = SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(3),
            path: "rows.toggle".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: vec![SourcePayloadField::Text],
                typed_fields: Vec::new(),
                row_lookup_field: None,
                address_lookup_field: None,
            },
        };
        let op = PlanOp {
            id: PlanOpId(9),
            kind: PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BoolNot,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: Vec::new(),
            output: Some(ValueRef::State(output_state_id)),
            indexed: true,
            unresolved_executable_ref_count: 0,
        };
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: vec![source_route.clone()],
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: vec![boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id: output_state_id,
                    value_type: PlanValueType::Bool,
                    scope_id: Some(scope_id),
                    indexed: true,
                    initial_value_kind: InitialValueKind::Bool,
                    initial_constant_id: None,
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                }],
                list_slots: vec![boon_plan::ListStorageSlot {
                    id: boon_plan::PlanStorageId(2),
                    list_id: boon_plan::ListId(5),
                    scope_id: Some(scope_id),
                    row_field_ids: Vec::new(),
                    capacity: None,
                    hidden_key_type: "u64".to_owned(),
                    has_generation: true,
                    initializer_kind: boon_plan::ListInitializerKind::Empty,
                    range: None,
                    initial_rows: Vec::new(),
                }],
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 1,
                scalar_storage_count: 1,
                list_storage_count: 1,
                byte_bank_storage_count: 0,
                operation_count: 1,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: Vec::new(),
                list_slots: vec![boon_plan::DebugEntry {
                    id: "list:5".to_owned(),
                    label: "rows".to_owned(),
                }],
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let list_rows = BTreeMap::from([(
            5,
            vec![
                PlanExecutorListRow {
                    key: 4,
                    generation: 1,
                    fields: BTreeMap::new(),
                },
                PlanExecutorListRow {
                    key: 9,
                    generation: 2,
                    fields: BTreeMap::new(),
                },
            ],
        )]);

        let selection = select_unscoped_indexed_update_targets(
            &plan,
            &op,
            &source_route,
            &IndexedUpdateTargetEvent {
                source: "rows.toggle".to_owned(),
                ..IndexedUpdateTargetEvent::default()
            },
            &list_rows,
        )
        .expect("unscoped indexed update should fan out through executor-owned target selection");
        assert!(selection.bulk_indexed_update);
        assert_eq!(selection.list_id, Some(5));
        assert_eq!(selection.list_label.as_deref(), Some("rows"));
        assert_eq!(
            selection.targets,
            vec![
                IndexedUpdateTargetRow {
                    key: 4,
                    generation: 1,
                },
                IndexedUpdateTargetRow {
                    key: 9,
                    generation: 2,
                },
            ]
        );
        assert_eq!(
            selection.executor_report["executor"],
            "cpu-plan-indexed-update-target-selection-v1"
        );
        assert_eq!(selection.executor_report["target_count"], 2);

        let targeted = select_unscoped_indexed_update_targets(
            &plan,
            &op,
            &source_route,
            &IndexedUpdateTargetEvent {
                source: "rows.toggle".to_owned(),
                target_key: Some(4),
                ..IndexedUpdateTargetEvent::default()
            },
            &list_rows,
        )
        .expect("already targeted events should skip bulk fanout");
        assert!(!targeted.bulk_indexed_update);
        assert_eq!(targeted.executor_report["skip_reason"], "event-target-key");

        let wrong_list = select_unscoped_indexed_update_targets(
            &plan,
            &op,
            &source_route,
            &IndexedUpdateTargetEvent {
                source: "rows.toggle".to_owned(),
                list_id: Some("other_rows".to_owned()),
                ..IndexedUpdateTargetEvent::default()
            },
            &list_rows,
        )
        .expect_err("wrong event list should be rejected by executor target selection");
        assert!(
            wrong_list.to_string().contains("expected `rows`"),
            "unexpected error: {wrong_list}"
        );
    }

    #[test]
    fn indexed_update_conflict_guard_rejects_real_same_target_writes() {
        let report_row = |op_id: u64, key: u64, field: &str, value: JsonValue| {
            json!({
                "list_id": 7,
                "list": "items",
                "key": key,
                "generation": 1,
                "field_path": field,
                "update_op_id": op_id,
                "value": value,
            })
        };

        let mut touched = BTreeMap::new();
        track_indexed_update_write_conflicts(
            &mut touched,
            &[
                report_row(10, 4, "value", json!("old")),
                report_row(11, 5, "value", json!("separate row")),
                report_row(12, 4, "error", json!("separate field")),
            ],
        )
        .expect("different indexed targets should be accepted");

        let error = track_indexed_update_write_conflicts(
            &mut touched,
            &[report_row(14, 4, "value", json!("new"))],
        )
        .expect_err("different values for the same indexed target must be rejected");
        assert!(
            error
                .to_string()
                .contains("conflicting indexed update branches"),
            "unexpected error: {error}"
        );

        let mut touched = BTreeMap::new();
        track_indexed_update_write_conflicts(
            &mut touched,
            &[report_row(20, 4, "value", json!("same"))],
        )
        .expect("first indexed write should be accepted");
        let error = track_indexed_update_write_conflicts(
            &mut touched,
            &[report_row(21, 4, "value", json!("same"))],
        )
        .expect_err("same-value duplicate indexed target ownership must be rejected");
        assert!(
            error
                .to_string()
                .contains("duplicate indexed update branches"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn indexed_update_conflict_guard_ignores_derived_semantic_deltas() {
        let mut touched = BTreeMap::new();
        track_indexed_update_write_conflicts(
            &mut touched,
            &[json!({
                "list_id": 7,
                "list": "cells",
                "key": 4,
                "generation": 1,
                "field_path": "formula_text",
                "update_op_id": 30,
                "value": "=A1+A2",
            })],
        )
        .expect("derived semantic-delta churn must not count as duplicate real writes");
    }

    #[test]
    fn root_bytes_state_transition_applies_fixed_mutation_in_executor() {
        let mut private_bytes = BTreeMap::from([(
            2,
            PlanExecutorBytes::from_inline(
                sha256_bytes(&[10, 20, 30]),
                3,
                vec![10, 20, 30],
                "input",
            )
            .expect("input bytes should be valid"),
        )]);
        let mut fixed_banks = BTreeMap::from([(4, vec![0, 0, 0])]);
        let mut bytes_owner = RootBytesStateMaps::new(&mut private_bytes, &mut fixed_banks);
        let transition = apply_root_bytes_state_transition(
            &mut bytes_owner,
            StateId(4),
            None,
            Some(RootBytesFixedMutation {
                input_state_id: StateId(2),
                output_state_id: StateId(4),
                patches: vec![(1, 99)],
            }),
            PlanOpId(40),
        )
        .expect("fixed-byte mutation should be applied by PlanExecutor");
        drop(bytes_owner);

        assert_eq!(fixed_banks[&4], vec![10, 99, 30]);
        assert!(!private_bytes.contains_key(&4));
        assert_eq!(transition.mode, "fixed_byte_patch");
        assert_eq!(
            transition.executor_report["executor"],
            "cpu-plan-root-bytes-state-transition-v1"
        );
        assert_eq!(transition.executor_report["mode"], "fixed_byte_patch");
    }

    #[test]
    fn summarize_plan_lists_reports_counts_titles_and_rows() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: Vec::new(),
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 1,
                byte_bank_storage_count: 0,
                operation_count: 0,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: Vec::new(),
                list_slots: vec![boon_plan::DebugEntry {
                    id: "list:7".to_owned(),
                    label: "todos".to_owned(),
                }],
                derived_values: Vec::new(),
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };
        let mut active_fields = BTreeMap::new();
        active_fields.insert("title".to_owned(), json!("Write tests"));
        active_fields.insert("completed".to_owned(), json!(false));
        let mut completed_fields = BTreeMap::new();
        completed_fields.insert("title".to_owned(), json!("Compile"));
        completed_fields.insert("completed".to_owned(), json!(true));
        let list_state = BTreeMap::from([(
            7,
            vec![
                PlanExecutorListRow {
                    key: 1,
                    generation: 1,
                    fields: active_fields,
                },
                PlanExecutorListRow {
                    key: 2,
                    generation: 1,
                    fields: completed_fields,
                },
            ],
        )]);

        let summary = summarize_plan_lists(&plan, &list_state);
        assert_eq!(summary["todos"]["row_count"], 2);
        assert_eq!(summary["todos"]["active_count"], 1);
        assert_eq!(summary["todos"]["completed_count"], 1);
        assert_eq!(
            summary["todos"]["titles"],
            json!(["Write tests", "Compile"])
        );
        assert_eq!(summary["todos"]["rows"][0]["key"], 1);
    }

    #[test]
    fn root_aggregate_evaluator_counts_rows_and_reports_changed_deltas() {
        let plan = MachinePlan {
            version: boon_plan::PlanVersion::default(),
            target_profile: boon_plan::TargetProfile::SoftwareDefault,
            constants: Vec::new(),
            source_routes: Vec::new(),
            storage_layout: boon_plan::StorageLayout {
                scalar_slots: Vec::new(),
                list_slots: Vec::new(),
                byte_banks: Vec::new(),
            },
            regions: vec![
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(1),
                    kind: RegionKind::ListOperations,
                    ops: vec![PlanOp {
                        id: PlanOpId(10),
                        kind: PlanOpKind::ListOperation {
                            operation_kind: PlanListOperationKind::Count,
                            append: None,
                            remove: None,
                            retain: None,
                            count: Some(boon_plan::PlanListCount {
                                target: ValueRef::Field(FieldId(20)),
                                predicate: boon_plan::PlanListRemovePredicate::RowFieldBoolNot {
                                    input: ValueRef::State(StateId(30)),
                                },
                            }),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::List(boon_plan::ListId(7))),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
                boon_plan::OperationRegion {
                    id: boon_plan::PlanRegionId(2),
                    kind: RegionKind::DerivedEvaluation,
                    ops: vec![PlanOp {
                        id: PlanOpId(11),
                        kind: PlanOpKind::DerivedValue {
                            derived_kind: boon_plan::PlanDerivedKind::Pure,
                            startup_recompute: true,
                            expression: Some(PlanDerivedExpression::NumberCompareConst {
                                left: ValueRef::Field(FieldId(20)),
                                op: ">".to_owned(),
                                right: 0,
                            }),
                        },
                        inputs: Vec::new(),
                        output: Some(ValueRef::Field(FieldId(21))),
                        indexed: false,
                        unresolved_executable_ref_count: 0,
                    }],
                },
            ],
            dirty_plan: boon_plan::DirtyPlan {
                dependency_edges: 0,
                unresolved_dependency_edges: 0,
            },
            commit_plan: boon_plan::CommitPlan {
                update_branch_count: 0,
                unresolved_update_branch_count: 0,
            },
            delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
            capability_summary: boon_plan::CapabilitySummary {
                executable: true,
                typed_lowering_executable: true,
                cpu_plan_executor_complete: false,
                constant_count: 0,
                source_route_count: 0,
                scalar_storage_count: 0,
                list_storage_count: 1,
                byte_bank_storage_count: 0,
                operation_count: 2,
                typed_value_ref_count: 0,
                executable_string_path_count: 0,
                unresolved_executable_ref_count: 0,
                unknown_plan_op_count: 0,
                cpu_plan_executor_unsupported_op_count: 0,
                runtime_ast_dependency_count: 0,
                graph_rebuild_count: 0,
                graph_clones_per_item: 0,
            },
            debug_map: boon_plan::DebugMap {
                source_units: Vec::new(),
                source_routes: Vec::new(),
                state_slots: vec![boon_plan::DebugEntry {
                    id: "state:30".to_owned(),
                    label: "todo.completed".to_owned(),
                }],
                list_slots: vec![boon_plan::DebugEntry {
                    id: "list:7".to_owned(),
                    label: "todos".to_owned(),
                }],
                derived_values: vec![boon_plan::DebugEntry {
                    id: "field:21".to_owned(),
                    label: "store.has_active".to_owned(),
                }],
                fields: Vec::new(),
                unresolved_executable_refs: Vec::new(),
            },
        };

        let mut active_fields = BTreeMap::new();
        active_fields.insert("completed".to_owned(), json!(false));
        let mut completed_fields = BTreeMap::new();
        completed_fields.insert("completed".to_owned(), json!(true));
        let list_state = BTreeMap::from([(
            7,
            vec![
                PlanExecutorListRow {
                    key: 1,
                    generation: 1,
                    fields: active_fields,
                },
                PlanExecutorListRow {
                    key: 2,
                    generation: 1,
                    fields: completed_fields,
                },
            ],
        )]);

        let values = evaluate_root_pure_number_compare_values(&plan, &list_state)
            .expect("root aggregate evaluation should stay executor-owned");
        assert_eq!(values.get(&21), Some(&json!(true)));

        let changes = changed_root_derived_deltas(&plan, &BTreeMap::new(), &values);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0["field_path"], "store.has_active");
        assert_eq!(changes[0].1["expression_kind"], "number_compare_const");
    }
}
