use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub type ReportSchemaResult<T> = Result<T, Box<dyn std::error::Error>>;
type RuntimeResult<T> = ReportSchemaResult<T>;

#[derive(Clone, Debug, serde::Deserialize)]
struct Scenario {
    #[serde(default)]
    step: Vec<ScenarioStep>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct ScenarioStep {
    id: String,
    #[serde(default)]
    expected_source_event: Option<BTreeMap<String, toml::Value>>,
}

fn parse_scenario(path: &Path) -> RuntimeResult<Scenario> {
    let text = fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn toml_string_ref<'a>(table: &'a BTreeMap<String, toml::Value>, key: &str) -> Option<&'a str> {
    table.get(key).and_then(toml::Value::as_str)
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

pub fn enrich_runtime_execution_surface(report: &mut JsonValue, layer: &str) {
    let Some(object) = report.as_object_mut() else {
        return;
    };
    object.insert("window_mode".to_owned(), runtime_window_mode(layer));
    object.insert("window_backend".to_owned(), runtime_window_backend(layer));
    object.insert("display_server".to_owned(), json!(display_server()));
}

pub fn enrich_headed_runtime_surface(
    object: &mut serde_json::Map<String, JsonValue>,
    example: &str,
) {
    object.insert("window_mode".to_owned(), json!("headed"));
    object.insert("display_server".to_owned(), json!(display_server()));
    object.insert("window_backend".to_owned(), json!("macroquad-ply"));
    object.insert(
        "display_scale".to_owned(),
        json!(std::env::var("GDK_SCALE").unwrap_or_else(|_| "1".to_owned())),
    );
    object.insert("window_pid".to_owned(), json!(std::process::id()));
    object.insert(
        "window_title".to_owned(),
        json!(format!("Boon Circuit {example}")),
    );
    object.insert("input_backend".to_owned(), json!("macroquad-os-events"));
    object.insert("capture_backend".to_owned(), json!("macroquad-framebuffer"));
    object.insert(
        "focused_window_proof".to_owned(),
        json!("captured while process window was active in verifier mode"),
    );
    object.insert(
        "manual_observer".to_owned(),
        json!(std::env::var("USER").unwrap_or_else(|_| "unknown".to_owned())),
    );
}

fn runtime_window_mode(layer: &str) -> JsonValue {
    match layer {
        "headed-ply" | "human" => json!("headed"),
        "ply-headless" => json!("headless"),
        "semantic" | "speed" => json!("none"),
        _ => json!("not-applicable"),
    }
}

fn runtime_window_backend(layer: &str) -> JsonValue {
    match layer {
        "headed-ply" | "human" => json!("macroquad-ply"),
        "ply-headless" => json!("ply-engine-headless"),
        "semantic" | "speed" => {
            json!({"unavailable_reason": "semantic/runtime layer does not open a window"})
        }
        _ => json!("not-applicable"),
    }
}

fn display_server() -> String {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "wayland".to_owned()
    } else if std::env::var("DISPLAY").is_ok() {
        "x11".to_owned()
    } else {
        "none".to_owned()
    }
}

pub fn write_json(path: &Path, value: &JsonValue) -> RuntimeResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_file(path: &Path) -> RuntimeResult<String> {
    Ok(sha256_bytes(&fs::read(path)?))
}

pub fn report_schema_hash() -> String {
    sha256_bytes(include_str!("lib.rs").as_bytes())
}

pub fn verify_report_schema(path: &Path) -> RuntimeResult<()> {
    let report: JsonValue = serde_json::from_slice(&fs::read(path)?)?;
    if report.get("command").and_then(JsonValue::as_str) == Some("compile-artifact") {
        verify_compiled_artifact_report(&report, path)?;
        return Ok(());
    }
    if report.get("command").and_then(JsonValue::as_str) == Some("inspect-compiled-artifact") {
        verify_inspected_compiled_artifact_report(&report, path)?;
        return Ok(());
    }
    if report.get("command").and_then(JsonValue::as_str)
        == Some("verify-compiled-artifact-scenario")
    {
        verify_compiled_artifact_scenario_report(&report, path)?;
        return Ok(());
    }
    let required = [
        "report_version",
        "generated_at_utc",
        "command",
        "command_argv",
        "measurement_mode",
        "exit_status",
        "git_commit",
        "binary_hash",
        "source_hash",
        "scenario_hash",
        "program_hash",
        "budget_hash",
        "graph_node_count",
        "per_step_pass_fail",
        "artifact_sha256s",
    ];
    for key in required {
        if report.get(key).is_none() {
            return Err(format!("{} missing required report field `{key}`", path.display()).into());
        }
    }
    let status = report.get("status").and_then(JsonValue::as_str);
    if status != Some("pass") {
        if status == Some("fail") && report_is_blocker_audit(&report) {
            verify_failing_blocker_report_shape(&report, path)?;
            verify_measurement_mode(&report, path)?;
            verify_artifact_hashes(&report, path)?;
            return Ok(());
        }
        return Err(format!("{} did not pass", path.display()).into());
    }
    verify_common_report_shape(&report, path)?;
    verify_measurement_mode(&report, path)?;
    verify_report_file_hash(&report, path, "source_path", "source_hash")?;
    verify_report_file_hash(&report, path, "scenario_path", "scenario_hash")?;
    verify_artifact_hashes(&report, path)?;
    if report_is_runtime_execution_layer(&report) {
        verify_runtime_execution_metadata(&report, path)?;
    }
    if report.get("playground_surface").is_some()
        || report_layer_is(&report, "headed-smoke")
        || report_layer_is(&report, "headed-ply")
    {
        verify_playground_surface_report(&report, path)?;
    }
    if report_layer_is(&report, "speed") {
        verify_speed_report(&report, path)?;
    }
    if report_command_is(&report, "bench-todomvc") || report_command_is(&report, "bench-example") {
        verify_benchmark_report(&report, path)?;
    }
    if report_layer_is(&report, "headed-ply") {
        verify_headed_artifacts(&report, path)?;
    }
    if report_layer_is(&report, "os-input-probe") {
        verify_os_input_probe_report(&report, path)?;
    }
    if report_layer_is(&report, "human") {
        verify_human_artifacts(&report, path)?;
    }
    if report_command_is(&report, "verify-cells-visible-reality") {
        verify_cells_visible_reality_report(&report, path)?;
    }
    Ok(())
}

fn verify_compiled_artifact_report(report: &JsonValue, path: &Path) -> RuntimeResult<()> {
    for key in [
        "status",
        "report_version",
        "command",
        "command_argv",
        "measurement_mode",
        "exit_status",
        "generated_at_utc",
        "git_commit",
        "binary_hash",
        "source_path",
        "source_hash",
        "program_hash",
        "graph_node_count",
        "semantic_index",
        "compiled_schedule",
        "compiled_artifact",
        "artifact_sections",
        "artifact_sha256s",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} missing compile-artifact report field `{key}`",
                path.display()
            )
            .into());
        }
    }
    if report.get("status").and_then(JsonValue::as_str) != Some("pass") {
        return Err(format!("{} compile-artifact report did not pass", path.display()).into());
    }
    if report.get("measurement_mode").and_then(JsonValue::as_str) != Some("diagnostic") {
        return Err(format!(
            "{} compile-artifact report measurement_mode must be diagnostic",
            path.display()
        )
        .into());
    }
    let artifact = report
        .get("compiled_artifact")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} compiled_artifact is not an object", path.display()))?;
    for key in [
        "path",
        "sha256",
        "format",
        "artifact_version",
        "program_hash",
        "report_schema_hash",
        "source_unit_count",
    ] {
        if artifact.get(key).is_none() {
            return Err(format!("{} compiled_artifact missing `{key}`", path.display()).into());
        }
    }
    if artifact.get("format").and_then(JsonValue::as_str) != Some("boonc-json-v1") {
        return Err(format!("{} compiled_artifact has wrong format", path.display()).into());
    }
    if artifact.get("artifact_version").and_then(JsonValue::as_u64) != Some(1) {
        return Err(format!(
            "{} compiled_artifact has wrong artifact_version",
            path.display()
        )
        .into());
    }
    if artifact.get("program_hash") != report.get("program_hash") {
        return Err(format!(
            "{} compiled_artifact program_hash does not match report",
            path.display()
        )
        .into());
    }
    let sections = report
        .get("artifact_sections")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} artifact_sections is not an object", path.display()))?;
    for key in [
        "semantic_index",
        "symbol_table",
        "storage_layout",
        "source_schemas",
        "route_op_streams",
        "dependency_graph",
        "document_lowering_tables",
        "bridge_schemas",
        "compiled_schedule",
        "runtime_plan",
    ] {
        if sections.get(key).and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!("{} artifact_sections `{key}` is not true", path.display()).into());
        }
    }
    verify_artifact_hashes(report, path)?;
    Ok(())
}

fn verify_inspected_compiled_artifact_report(report: &JsonValue, path: &Path) -> RuntimeResult<()> {
    for key in [
        "status",
        "report_version",
        "command",
        "command_argv",
        "measurement_mode",
        "exit_status",
        "generated_at_utc",
        "git_commit",
        "binary_hash",
        "artifact_path",
        "artifact_hash",
        "program_hash",
        "compiled_artifact",
        "artifact_sections",
        "artifact_sha256s",
        "inspection_result",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} missing inspect-compiled-artifact report field `{key}`",
                path.display()
            )
            .into());
        }
    }
    if report.get("status").and_then(JsonValue::as_str) != Some("pass") {
        return Err(format!(
            "{} inspect-compiled-artifact report did not pass",
            path.display()
        )
        .into());
    }
    if report.get("measurement_mode").and_then(JsonValue::as_str) != Some("diagnostic") {
        return Err(format!(
            "{} inspect-compiled-artifact report measurement_mode must be diagnostic",
            path.display()
        )
        .into());
    }
    let artifact = report
        .get("compiled_artifact")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} compiled_artifact is not an object", path.display()))?;
    for key in [
        "path",
        "sha256",
        "format",
        "artifact_version",
        "program_hash",
        "report_schema_hash",
        "source_unit_count",
    ] {
        if artifact.get(key).is_none() {
            return Err(format!("{} compiled_artifact missing `{key}`", path.display()).into());
        }
    }
    if artifact.get("sha256") != report.get("artifact_hash") {
        return Err(format!(
            "{} compiled_artifact sha256 does not match artifact_hash",
            path.display()
        )
        .into());
    }
    if artifact.get("path") != report.get("artifact_path") {
        return Err(format!(
            "{} compiled_artifact path does not match artifact_path",
            path.display()
        )
        .into());
    }
    if artifact.get("program_hash") != report.get("program_hash") {
        return Err(format!(
            "{} compiled_artifact program_hash does not match report",
            path.display()
        )
        .into());
    }
    let inspection = report
        .get("inspection_result")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} inspection_result is not an object", path.display()))?;
    for key in [
        "artifact_valid",
        "loaded_runtime_from_artifact",
        "runtime_instantiated_from_artifact",
        "runtime_plan_present",
        "runtime_plan_generic_derived_deserialized_from_artifact",
        "runtime_plan_generic_derived_deserialized_counts",
        "runtime_plan_storage_deserialized_from_artifact",
        "runtime_plan_storage_deserialized_counts",
        "runtime_plan_document_lowering_deserialized_from_artifact",
        "runtime_plan_document_lowering_deserialized_counts",
        "runtime_plan_non_route_tables_deserialized_from_artifact",
        "runtime_plan_non_route_tables_deserialized_counts",
        "runtime_plan_source_routes_deserialized_from_artifact",
        "runtime_plan_source_routes_deserialized_counts",
        "source_free_runtime_load_available",
        "source_reparse_required_for_current_runtime",
        "source_reparse_attempted",
        "source_file_access",
        "parser_ast_required_for_execution",
        "typed_ir_required_for_mvp_loader",
        "scenario_execution_available",
        "blocked_task",
        "scenario_execution_pending_task",
        "missing_runtime_plan_sections",
    ] {
        if inspection.get(key).is_none() {
            return Err(format!("{} inspection_result missing `{key}`", path.display()).into());
        }
    }
    if inspection
        .get("artifact_valid")
        .and_then(JsonValue::as_bool)
        != Some(true)
        || inspection
            .get("loaded_runtime_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_instantiated_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("source_free_runtime_load_available")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_plan_generic_derived_deserialized_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_plan_storage_deserialized_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_plan_document_lowering_deserialized_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_plan_non_route_tables_deserialized_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("runtime_plan_source_routes_deserialized_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || inspection
            .get("source_reparse_required_for_current_runtime")
            .and_then(JsonValue::as_bool)
            != Some(false)
        || inspection
            .get("typed_ir_required_for_mvp_loader")
            .and_then(JsonValue::as_bool)
            != Some(false)
        || inspection
            .get("source_reparse_attempted")
            .and_then(JsonValue::as_bool)
            != Some(false)
        || inspection
            .get("source_file_access")
            .and_then(JsonValue::as_str)
            != Some("not_attempted")
        || inspection
            .get("scenario_execution_available")
            .and_then(JsonValue::as_bool)
            != Some(false)
    {
        return Err(format!(
            "{} inspection_result must prove source-free runtime load and must not claim scenario execution",
            path.display()
        )
        .into());
    }
    let generic_counts = inspection
        .get("runtime_plan_generic_derived_deserialized_counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} inspection_result runtime_plan_generic_derived_deserialized_counts is not an object",
                path.display()
            )
        })?;
    for key in [
        "function_count",
        "root_supported_count",
        "indexed_supported_count",
        "unsupported_reason_count",
    ] {
        if generic_counts
            .get(key)
            .and_then(JsonValue::as_u64)
            .is_none()
        {
            return Err(format!(
                "{} inspection_result runtime_plan_generic_derived_deserialized_counts missing `{key}`",
                path.display()
            )
            .into());
        }
    }
    let storage_counts = inspection
        .get("runtime_plan_storage_deserialized_counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} inspection_result runtime_plan_storage_deserialized_counts is not an object",
                path.display()
            )
        })?;
    for key in [
        "root_slot_count",
        "root_initial_field_copy_count",
        "list_slot_count",
        "indexed_row_initial_reset_count",
        "initial_row_count",
    ] {
        if storage_counts
            .get(key)
            .and_then(JsonValue::as_u64)
            .is_none()
        {
            return Err(format!(
                "{} inspection_result runtime_plan_storage_deserialized_counts missing `{key}`",
                path.display()
            )
            .into());
        }
    }
    let document_counts = inspection
        .get("runtime_plan_document_lowering_deserialized_counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} inspection_result runtime_plan_document_lowering_deserialized_counts is not an object",
                path.display()
            )
        })?;
    for key in [
        "root_summary_path_count",
        "list_summary_field_count",
        "dynamic_list_view_list_count",
        "projection_storage_resolution_count",
        "unresolved_projection_storage_path_count",
        "observed_root_path_count",
        "render_slot_count",
        "render_slot_failure_count",
    ] {
        if document_counts
            .get(key)
            .and_then(JsonValue::as_u64)
            .is_none()
        {
            return Err(format!(
                "{} inspection_result runtime_plan_document_lowering_deserialized_counts missing `{key}`",
                path.display()
            )
            .into());
        }
    }
    let non_route_counts = inspection
        .get("runtime_plan_non_route_tables_deserialized_counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} inspection_result runtime_plan_non_route_tables_deserialized_counts is not an object",
                path.display()
            )
        })?;
    for key in [
        "runtime_symbol_count",
        "scalar_source_path_count",
        "scalar_branch_count",
        "derived_text_transform_count",
        "list_operation_count",
        "list_projection_count",
        "list_source_binding_count",
    ] {
        if non_route_counts
            .get(key)
            .and_then(JsonValue::as_u64)
            .is_none()
        {
            return Err(format!(
                "{} inspection_result runtime_plan_non_route_tables_deserialized_counts missing `{key}`",
                path.display()
            )
            .into());
        }
    }
    let source_route_counts = inspection
        .get("runtime_plan_source_routes_deserialized_counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} inspection_result runtime_plan_source_routes_deserialized_counts is not an object",
                path.display()
            )
        })?;
    for key in [
        "route_count",
        "id_slot_count",
        "label_slot_count",
        "routes_with_ids",
        "action_table_slot_count",
        "action_op_stream_count",
        "total_action_op_count",
        "max_action_op_count",
        "source_payload_schema_count",
        "source_payload_field_count",
        "source_payload_text_field_count",
        "source_payload_key_field_count",
        "source_payload_address_field_count",
        "source_payload_pointer_field_count",
    ] {
        if source_route_counts
            .get(key)
            .and_then(JsonValue::as_u64)
            .is_none()
        {
            return Err(format!(
                "{} inspection_result runtime_plan_source_routes_deserialized_counts missing `{key}`",
                path.display()
            )
            .into());
        }
    }
    if inspection
        .get("parser_ast_required_for_execution")
        .and_then(JsonValue::as_bool)
        != Some(false)
    {
        return Err(format!(
            "{} inspection_result must not require parser AST",
            path.display()
        )
        .into());
    }
    let missing_runtime_plan_sections = inspection
        .get("missing_runtime_plan_sections")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} inspection_result missing_runtime_plan_sections is not an array",
                path.display()
            )
        })?;
    if !missing_runtime_plan_sections.is_empty() {
        return Err(format!(
            "{} inspection_result must not list missing runtime plan sections",
            path.display()
        )
        .into());
    }
    verify_compiled_artifact_sections(report, path)?;
    verify_artifact_hashes(report, path)?;
    Ok(())
}

fn verify_compiled_artifact_scenario_report(report: &JsonValue, path: &Path) -> RuntimeResult<()> {
    for key in [
        "status",
        "report_version",
        "command",
        "command_argv",
        "measurement_mode",
        "exit_status",
        "generated_at_utc",
        "git_commit",
        "binary_hash",
        "source_path",
        "source_hash",
        "scenario_path",
        "scenario_hash",
        "program_hash",
        "artifact_path",
        "artifact_hash",
        "compiled_artifact",
        "artifact_sections",
        "artifact_sha256s",
        "artifact_scenario",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} missing verify-compiled-artifact-scenario report field `{key}`",
                path.display()
            )
            .into());
        }
    }
    if report.get("status").and_then(JsonValue::as_str) != Some("pass") {
        return Err(format!(
            "{} verify-compiled-artifact-scenario report did not pass",
            path.display()
        )
        .into());
    }
    if report.get("measurement_mode").and_then(JsonValue::as_str) != Some("proof") {
        return Err(format!(
            "{} verify-compiled-artifact-scenario report measurement_mode must be proof",
            path.display()
        )
        .into());
    }
    let artifact = report
        .get("compiled_artifact")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} compiled_artifact is not an object", path.display()))?;
    for key in [
        "path",
        "sha256",
        "format",
        "artifact_version",
        "program_hash",
        "report_schema_hash",
        "source_unit_count",
    ] {
        if artifact.get(key).is_none() {
            return Err(format!("{} compiled_artifact missing `{key}`", path.display()).into());
        }
    }
    if artifact.get("sha256") != report.get("artifact_hash") {
        return Err(format!(
            "{} compiled_artifact sha256 does not match artifact_hash",
            path.display()
        )
        .into());
    }
    if artifact.get("path") != report.get("artifact_path") {
        return Err(format!(
            "{} compiled_artifact path does not match artifact_path",
            path.display()
        )
        .into());
    }
    if artifact.get("program_hash") != report.get("program_hash") {
        return Err(format!(
            "{} compiled_artifact program_hash does not match report",
            path.display()
        )
        .into());
    }
    let scenario = report
        .get("artifact_scenario")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} artifact_scenario is not an object", path.display()))?;
    for key in [
        "scenario_execution_available",
        "scenario_execution_from_artifact",
        "runtime_instantiated_from_artifact",
        "source_reparse_attempted",
        "source_file_access",
        "typed_ir_required_for_artifact_execution",
        "parser_ast_required_for_artifact_execution",
        "semantic_deltas_match",
        "render_patches_match",
        "state_summary_match",
        "parity_passed",
        "source_signature_hash",
        "artifact_signature_hash",
    ] {
        if scenario.get(key).is_none() {
            return Err(format!("{} artifact_scenario missing `{key}`", path.display()).into());
        }
    }
    if scenario
        .get("scenario_execution_available")
        .and_then(JsonValue::as_bool)
        != Some(true)
        || scenario
            .get("scenario_execution_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || scenario
            .get("runtime_instantiated_from_artifact")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || scenario
            .get("semantic_deltas_match")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || scenario
            .get("render_patches_match")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || scenario
            .get("state_summary_match")
            .and_then(JsonValue::as_bool)
            != Some(true)
        || scenario.get("parity_passed").and_then(JsonValue::as_bool) != Some(true)
    {
        return Err(format!(
            "{} artifact_scenario must prove artifact execution and source parity",
            path.display()
        )
        .into());
    }
    if scenario
        .get("source_reparse_attempted")
        .and_then(JsonValue::as_bool)
        != Some(false)
        || scenario
            .get("source_file_access")
            .and_then(JsonValue::as_str)
            != Some("not_attempted")
        || scenario
            .get("typed_ir_required_for_artifact_execution")
            .and_then(JsonValue::as_bool)
            != Some(false)
        || scenario
            .get("parser_ast_required_for_artifact_execution")
            .and_then(JsonValue::as_bool)
            != Some(false)
    {
        return Err(format!(
            "{} artifact_scenario must be source-free and AST-free",
            path.display()
        )
        .into());
    }
    if scenario
        .get("source_signature_hash")
        .and_then(JsonValue::as_str)
        != scenario
            .get("artifact_signature_hash")
            .and_then(JsonValue::as_str)
    {
        return Err(format!(
            "{} artifact_scenario source/artifact signature hashes differ",
            path.display()
        )
        .into());
    }
    verify_report_file_hash(report, path, "source_path", "source_hash")?;
    verify_report_file_hash(report, path, "scenario_path", "scenario_hash")?;
    verify_compiled_artifact_sections(report, path)?;
    verify_artifact_hashes(report, path)?;
    Ok(())
}

fn verify_compiled_artifact_sections(report: &JsonValue, path: &Path) -> RuntimeResult<()> {
    let sections = report
        .get("artifact_sections")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} artifact_sections is not an object", path.display()))?;
    for key in [
        "semantic_index",
        "symbol_table",
        "storage_layout",
        "source_schemas",
        "route_op_streams",
        "dependency_graph",
        "document_lowering_tables",
        "bridge_schemas",
        "compiled_schedule",
        "runtime_plan",
    ] {
        if sections.get(key).and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!("{} artifact_sections `{key}` is not true", path.display()).into());
        }
    }
    Ok(())
}

fn report_is_blocker_audit(report: &JsonValue) -> bool {
    matches!(
        report.get("command").and_then(JsonValue::as_str),
        Some(
            "audit-machine-readiness"
                | "audit-goal-readiness"
                | "audit-manual-readiness"
                | "boon-native-playground-role"
                | "verify-runtime-finality"
                | "verify-cells-wayland-scroll-speed"
                | "verify-platform-contract"
                | "verify-native-gpu-dependency-graph"
                | "verify-native-gpu-architecture"
                | "verify-native-gpu-layout-contract"
                | "verify-native-gpu-shaders"
                | "verify-native-gpu-multiwindow"
                | "verify-native-gpu-ipc-backpressure"
                | "verify-native-gpu-observability"
                | "verify-native-gpu-idle-wake"
                | "verify-native-real-window-input-environment"
                | "verify-native-gpu-preview-e2e"
                | "verify-native-gpu-novywave-interaction-speed"
                | "verify-native-gpu-scroll-speed"
                | "verify-native-dev-editor-scroll-speed"
                | "verify-native-example-switch-speed"
                | "verify-native-gpu-negative"
                | "verify-native-gpu-all"
                | "verify-boon-source-syntax"
                | "verify-scenario-manifest-integrity"
                | "verify-metamorphic-hidden-fixtures"
                | "verify-native-visible-launch"
                | "verify-native-examples"
                | "verify-native-dev-window-editor"
                | "verify-native-example-tabs"
                | "verify-native-editor-format"
                | "verify-native-example-speed"
                | "verify-native-counter-interaction-speed"
                | "verify-native-cells-interaction-speed"
                | "verify-native-dev-editor-speed"
                | "verify-boon-driver-schema"
                | "verify-boon-driver-e2e"
                | "verify-boon-driver-dev-window"
                | "verify-boon-driver-speed"
                | "verify-boon-driver-all"
                | "verify-linux-human-like-environment"
                | "verify-linux-human-like-e2e"
                | "verify-linux-human-like-speed"
                | "verify-linux-human-like-all"
        )
    )
}

fn verify_failing_blocker_report_shape(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let generated = report
        .get("generated_at_utc")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} generated_at_utc is not a Unix-seconds string",
                report_path.display()
            )
        })?
        .parse::<u64>()
        .map_err(|error| {
            format!(
                "{} generated_at_utc is not parseable Unix seconds: {error}",
                report_path.display()
            )
        })?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if generated > now.saturating_add(5) {
        return Err(format!("{} is future-dated", report_path.display()).into());
    }
    let exit_status = report
        .get("exit_status")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| format!("{} exit_status is not a number", report_path.display()))?;
    if exit_status == 0 {
        return Err(format!(
            "{} failing blocker report has zero exit_status",
            report_path.display()
        )
        .into());
    }
    let blockers = report
        .get("blockers")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} blocker report missing blockers", report_path.display()))?;
    if blockers.is_empty()
        || blockers.iter().any(|blocker| {
            blocker
                .as_str()
                .is_none_or(|blocker| blocker.trim().is_empty())
        })
    {
        return Err(format!(
            "{} blocker report has empty or malformed blockers",
            report_path.display()
        )
        .into());
    }
    let checks = report
        .get("per_step_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} per_step_pass_fail is not an array",
                report_path.display()
            )
        })?;
    let mut saw_failing_check = false;
    for (index, check) in checks.iter().enumerate() {
        let object = check.as_object().ok_or_else(|| {
            format!(
                "{} per_step_pass_fail[{index}] is not an object",
                report_path.display()
            )
        })?;
        let id = object
            .get("id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing string id",
                    report_path.display()
                )
            })?;
        if id.trim().is_empty() {
            return Err(format!(
                "{} per_step_pass_fail[{index}] has empty id",
                report_path.display()
            )
            .into());
        }
        let pass = object
            .get("pass")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing boolean pass",
                    report_path.display()
                )
            })?;
        saw_failing_check |= !pass;
    }
    if !saw_failing_check {
        return Err(format!(
            "{} failing blocker report has no failing per-step check",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_common_report_shape(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let version = report
        .get("report_version")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| format!("{} report_version is not a number", report_path.display()))?;
    if version == 0 {
        return Err(format!("{} report_version must be positive", report_path.display()).into());
    }
    let generated = report
        .get("generated_at_utc")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} generated_at_utc is not a Unix-seconds string",
                report_path.display()
            )
        })?
        .parse::<u64>()
        .map_err(|error| {
            format!(
                "{} generated_at_utc is not parseable Unix seconds: {error}",
                report_path.display()
            )
        })?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if generated > now.saturating_add(5) {
        return Err(format!("{} is future-dated", report_path.display()).into());
    }
    let command = report
        .get("command")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} command is not a string", report_path.display()))?;
    if command.trim().is_empty() {
        return Err(format!("{} command is empty", report_path.display()).into());
    }
    let argv = report
        .get("command_argv")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} command_argv is not an array", report_path.display()))?;
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.as_str().is_none_or(|arg| arg.trim().is_empty()))
    {
        return Err(format!(
            "{} command_argv is empty or malformed",
            report_path.display()
        )
        .into());
    }
    let exit_status = report
        .get("exit_status")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| format!("{} exit_status is not a number", report_path.display()))?;
    if exit_status != 0 {
        return Err(format!(
            "{} pass report has nonzero exit_status",
            report_path.display()
        )
        .into());
    }
    let checks = report
        .get("per_step_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} per_step_pass_fail is not an array",
                report_path.display()
            )
        })?;
    for (index, check) in checks.iter().enumerate() {
        let object = check.as_object().ok_or_else(|| {
            format!(
                "{} per_step_pass_fail[{index}] is not an object",
                report_path.display()
            )
        })?;
        let id = object
            .get("id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing string id",
                    report_path.display()
                )
            })?;
        if id.trim().is_empty() {
            return Err(format!(
                "{} per_step_pass_fail[{index}] has empty id",
                report_path.display()
            )
            .into());
        }
        let pass = object
            .get("pass")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing boolean pass",
                    report_path.display()
                )
            })?;
        if !pass {
            return Err(format!(
                "{} pass report contains failing check `{id}`",
                report_path.display()
            )
            .into());
        }
    }
    if report.get("command").and_then(JsonValue::as_str) == Some("explain-hardware") {
        verify_runtime_profile_metadata(report, report_path)?;
        if report.get("hardware_plan").is_none() {
            return Err(format!("{} missing hardware_plan", report_path.display()).into());
        }
    }
    Ok(())
}

fn verify_measurement_mode(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let mode = report
        .get("measurement_mode")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing measurement_mode", report_path.display()))?;
    if !matches!(mode, "interaction" | "proof" | "diagnostic") {
        return Err(format!(
            "{} has invalid measurement_mode `{mode}`",
            report_path.display()
        )
        .into());
    }
    if mode == "interaction" {
        verify_interaction_flow_id(report, report_path)?;
        verify_interaction_stage_counters(report, report_path)?;
        verify_interaction_required_zero_hot_path_counters(report, report_path)?;
        verify_interaction_mode_excludes_proof_and_diagnostic_hot_path(report, report_path)?;
    }
    Ok(())
}

fn verify_report_mode(report: &JsonValue, report_path: &Path, expected: &str) -> RuntimeResult<()> {
    let mode = report
        .get("measurement_mode")
        .and_then(JsonValue::as_str)
        .unwrap_or("missing");
    if mode != expected {
        return Err(format!(
            "{} expected measurement_mode `{expected}`, got `{mode}`",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_interaction_mode_excludes_proof_and_diagnostic_hot_path(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let mut violations = Vec::new();
    collect_interaction_hot_path_violations(report, "$", &mut violations);
    if !violations.is_empty() {
        return Err(format!(
            "{} interaction report counts proof/diagnostic work in the hot path: {}",
            report_path.display(),
            violations.join(", ")
        )
        .into());
    }
    Ok(())
}

fn verify_interaction_flow_id(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let flow_id = report
        .get("interaction_flow_id")
        .or_else(|| report.get("frame_flow_id"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if flow_id.trim().is_empty() {
        return Err(format!(
            "{} interaction report missing interaction_flow_id or frame_flow_id",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_interaction_stage_counters(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let stages = report
        .get("stage_counters")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} interaction report missing stage_counters object",
                report_path.display()
            )
        })?;
    if stages.is_empty() {
        return Err(format!(
            "{} interaction report has empty stage_counters",
            report_path.display()
        )
        .into());
    }
    for (name, stage) in stages {
        let object = stage.as_object().ok_or_else(|| {
            format!(
                "{} stage_counters.{name} is not an object",
                report_path.display()
            )
        })?;
        for key in ["p50", "p95", "p99", "max"] {
            if object.get(key).and_then(JsonValue::as_f64).is_none() {
                return Err(format!(
                    "{} stage_counters.{name} missing numeric `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
        let sample_count = object
            .get("sample_count")
            .and_then(JsonValue::as_u64)
            .unwrap_or_default();
        if sample_count == 0 {
            return Err(format!(
                "{} stage_counters.{name} missing positive sample_count",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_interaction_required_zero_hot_path_counters(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    for key in [
        "hot_path_png_write_count",
        "hot_path_report_write_count",
        "hot_path_report_serialization_count",
        "hot_path_heavy_json_summary_count",
        "hot_path_proof_readback_count",
        "hot_path_verbose_trace_event_count",
        "hot_path_dev_blocking_ipc_count",
    ] {
        let value = report.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            format!(
                "{} interaction report missing explicit zero `{key}`",
                report_path.display()
            )
        })?;
        if value != 0 {
            return Err(format!(
                "{} interaction report `{key}` must be zero, got {value}",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn collect_interaction_hot_path_violations(
    value: &JsonValue,
    path: &str,
    violations: &mut Vec<String>,
) {
    match value {
        JsonValue::Object(object) => {
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                if interaction_hot_path_counter_key(key) && json_number_is_positive(child) {
                    violations.push(child_path.clone());
                }
                if interaction_hot_path_bool_key(key) && child.as_bool().unwrap_or(false) {
                    violations.push(child_path.clone());
                }
                collect_interaction_hot_path_violations(child, &child_path, violations);
            }
        }
        JsonValue::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_interaction_hot_path_violations(
                    child,
                    &format!("{path}[{index}]"),
                    violations,
                );
            }
        }
        _ => {}
    }
}

fn interaction_hot_path_counter_key(key: &str) -> bool {
    matches!(
        key,
        "hot_path_png_write_count"
            | "hot_path_report_write_count"
            | "hot_path_report_serialization_count"
            | "hot_path_heavy_json_summary_count"
            | "hot_path_summary_write_count"
            | "hot_path_proof_readback_count"
            | "hot_path_readback_count"
            | "proof_readback_count_in_hot_path"
            | "readback_count_in_hot_path"
            | "hot_path_verbose_trace_event_count"
            | "verbose_trace_event_count_in_hot_path"
            | "hot_path_dev_blocking_ipc_count"
            | "dev_blocking_ipc_count"
            | "preview_blocked_on_ipc_count"
    )
}

fn interaction_hot_path_bool_key(key: &str) -> bool {
    matches!(
        key,
        "png_write_in_hot_path"
            | "report_write_in_hot_path"
            | "report_serialization_in_hot_path"
            | "heavy_json_summary_in_hot_path"
            | "proof_readback_in_hot_path"
            | "readback_in_hot_path"
            | "verbose_tracing_in_hot_path"
            | "dev_blocking_ipc_in_hot_path"
    )
}

fn json_number_is_positive(value: &JsonValue) -> bool {
    value.as_u64().is_some_and(|number| number > 0)
        || value.as_i64().is_some_and(|number| number > 0)
        || value.as_f64().is_some_and(|number| number > 0.0)
}

pub fn verify_runtime_execution_metadata(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let execution = report.get("runtime_execution").ok_or_else(|| {
        format!(
            "{} missing runtime_execution metadata for executable example report",
            report_path.display()
        )
    })?;
    for key in [
        "implementation",
        "source_loaded_from_boon",
        "typed_ir_loaded",
        "static_schedule_verified",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "expression_coverage",
        "semantic_index",
        "generic_interpreter_complete",
        "example_behavior_adapter",
        "remaining_example_specific_shell_policy",
        "remaining_example_specific_shells",
    ] {
        if execution.get(key).is_none() {
            return Err(format!(
                "{} runtime_execution missing `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    if execution
        .get("source_loaded_from_boon")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!(
            "{} did not load executable source from Boon",
            report_path.display()
        )
        .into());
    }
    if execution
        .get("typed_ir_loaded")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!("{} did not lower through typed IR", report_path.display()).into());
    }
    if execution
        .get("static_schedule_verified")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!("{} did not verify static schedule", report_path.display()).into());
    }
    if execution
        .get("generic_interpreter_complete")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!(
            "{} did not complete generic interpreter execution",
            report_path.display()
        )
        .into());
    }
    if execution
        .get("example_behavior_adapter")
        .and_then(JsonValue::as_bool)
        != Some(false)
    {
        return Err(format!(
            "{} still reports an example behavior adapter",
            report_path.display()
        )
        .into());
    }
    verify_remaining_example_specific_shells(execution, report_path)?;
    verify_runtime_execution_report_mirror(report, execution, report_path)?;
    if execution.get("implementation").and_then(JsonValue::as_str)
        != Some("static_graph_interpreter")
    {
        return Err(format!(
            "{} did not use static_graph_interpreter implementation",
            report_path.display()
        )
        .into());
    }
    verify_generic_runtime_slice_metadata(report, execution, report_path)?;
    verify_generic_runtime_slice_evidence(report, execution, report_path)?;
    verify_expression_coverage(report, report_path)?;
    verify_semantic_index(report, execution, report_path)?;
    verify_runtime_report_contract(report, report_path)?;
    Ok(())
}

fn verify_runtime_execution_report_mirror(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    for key in [
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "expression_coverage",
        "semantic_index",
    ] {
        let top_level = report.get(key).ok_or_else(|| {
            format!(
                "{} executable report missing top-level `{key}`",
                report_path.display()
            )
        })?;
        let execution_level = execution.get(key).ok_or_else(|| {
            format!(
                "{} runtime_execution missing mirrored `{key}`",
                report_path.display()
            )
        })?;
        if execution_level != top_level {
            return Err(format!(
                "{} runtime_execution `{key}` does not match top-level `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_semantic_index(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let index = execution
        .get("semantic_index")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing semantic_index object",
                report_path.display()
            )
        })?;
    for key in [
        "present",
        "version",
        "computed_from",
        "parser_policy_phase",
        "reuse_key",
        "source_unit_count",
        "source_count",
        "list_count",
        "row_scope_count",
        "function_count",
        "field_count",
        "view_binding_count",
        "diagnostic_span_count",
        "readiness",
        "reuse",
    ] {
        if !index.contains_key(key) {
            return Err(format!("{} semantic_index missing `{key}`", report_path.display()).into());
        }
    }
    if index.get("present").and_then(JsonValue::as_bool) != Some(true) {
        return Err(format!("{} semantic_index is not present", report_path.display()).into());
    }
    if index
        .get("version")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0)
        == 0
    {
        return Err(format!(
            "{} semantic_index has invalid version",
            report_path.display()
        )
        .into());
    }
    if index.get("computed_from").and_then(JsonValue::as_str)
        != Some("parser_ast_ir_typecheck_tables")
    {
        return Err(format!(
            "{} semantic_index is not computed from parser, IR, and typecheck tables",
            report_path.display()
        )
        .into());
    }
    let reuse = index
        .get("reuse")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} semantic_index missing reuse object",
                report_path.display()
            )
        })?;
    for key in [
        "parser_reused_by_ir",
        "typecheck_reused_by_ir",
        "runtime_reports_reuse_index",
    ] {
        if reuse.get(key).and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!(
                "{} semantic_index reuse `{key}` is not true",
                report_path.display()
            )
            .into());
        }
    }
    let shared_tables = reuse
        .get("shared_tables")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} semantic_index reuse missing shared_tables",
                report_path.display()
            )
        })?;
    if shared_tables.is_empty() {
        return Err(format!(
            "{} semantic_index has no shared tables",
            report_path.display()
        )
        .into());
    }
    let readiness = index
        .get("readiness")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} semantic_index missing readiness object",
                report_path.display()
            )
        })?;
    for key in [
        "source_payload_schemas",
        "source_completions",
        "route_critical_unknowns",
        "row_scopes",
        "row_scope_ambiguity",
        "selectors",
        "selector_index_ambiguity",
        "render_contracts",
        "bridge_page_descriptors",
        "dynamic_fallback_count",
    ] {
        if !readiness.contains_key(key) {
            return Err(format!(
                "{} semantic_index readiness missing `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    for key in [
        "source_payload_schemas",
        "source_completions",
        "route_critical_unknowns",
        "row_scopes",
        "row_scope_ambiguity",
        "selectors",
        "selector_index_ambiguity",
        "render_contracts",
        "bridge_page_descriptors",
    ] {
        let status = readiness
            .get(key)
            .and_then(JsonValue::as_object)
            .ok_or_else(|| {
                format!(
                    "{} semantic_index readiness `{key}` is not an object",
                    report_path.display()
                )
            })?;
        for field in ["known_count", "fallback_count", "fallback_reasons"] {
            if !status.contains_key(field) {
                return Err(format!(
                    "{} semantic_index readiness `{key}` missing `{field}`",
                    report_path.display()
                )
                .into());
            }
        }
        if status.get("fallback_count").and_then(JsonValue::as_u64) != Some(0) {
            return Err(format!(
                "{} semantic_index readiness `{key}` has route-critical fallback: {}",
                report_path.display(),
                status
                    .get("fallback_reasons")
                    .cloned()
                    .unwrap_or_else(|| JsonValue::Array(Vec::new()))
            )
            .into());
        }
    }
    if readiness
        .get("dynamic_fallback_count")
        .and_then(JsonValue::as_u64)
        != Some(0)
    {
        return Err(format!(
            "{} semantic_index readiness dynamic_fallback_count is not zero",
            report_path.display()
        )
        .into());
    }
    let top_level = report
        .get("semantic_index")
        .ok_or_else(|| format!("{} missing top-level semantic_index", report_path.display()))?;
    if execution.get("semantic_index") != Some(top_level) {
        return Err(format!(
            "{} runtime_execution semantic_index does not match top-level semantic_index",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_remaining_example_specific_shells(
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let policy = execution
        .get("remaining_example_specific_shell_policy")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing remaining shell policy",
                report_path.display()
            )
        })?;
    if policy != "scenario_assertion_report_glue_only" {
        return Err(format!(
            "{} runtime_execution has unsupported remaining shell policy `{policy}`",
            report_path.display()
        )
        .into());
    }
    let shells = execution
        .get("remaining_example_specific_shells")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing remaining shell list",
                report_path.display()
            )
        })?;
    let slices = execution
        .get("generic_runtime_slices")
        .and_then(JsonValue::as_object);
    let has_example_specific_slices = slices.is_some_and(|slices| {
        slices.iter().any(|(key, value)| {
            value.as_bool() == Some(true)
                && (key.contains("_todomvc_")
                    || key.starts_with("todomvc_")
                    || key.contains("_cells_")
                    || key.starts_with("cells_"))
        })
    });
    if has_example_specific_slices && shells.is_empty() {
        return Err(format!(
            "{} runtime_execution has example-specific runtime slices but no remaining shell listing",
            report_path.display()
        )
        .into());
    }
    for (index, shell) in shells.iter().enumerate() {
        let Some(shell) = shell.as_str() else {
            return Err(format!(
                "{} runtime_execution remaining shell at index {index} is not a string",
                report_path.display()
            )
            .into());
        };
        if !(shell.ends_with("_scenario_glue")
            || shell.ends_with("_assertion_glue")
            || shell.ends_with("_report_glue")
            || shell.ends_with("_render_patch_report_glue")
            || shell.ends_with("_stress_report_glue"))
        {
            return Err(format!(
                "{} runtime_execution remaining shell `{shell}` is not classified as scenario/assertion/report glue",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_generic_runtime_slice_metadata(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let Some(slices) = execution
        .get("generic_runtime_slices")
        .and_then(JsonValue::as_object)
    else {
        return Err(format!("{} missing generic_runtime_slices", report_path.display()).into());
    };
    let example = report
        .get("example")
        .and_then(JsonValue::as_str)
        .or_else(|| execution.get("adapter_kind").and_then(JsonValue::as_str))
        .or_else(|| report_program_kind(report))
        .unwrap_or_default();
    require_generic_runtime_slice_flags(slices, example, report_path)?;
    for (key, value) in slices {
        let Some(flag) = value.as_bool() else {
            continue;
        };
        if key == "surface_driver_borrows_generic_storage_for_tick" {
            if flag {
                return Err(format!(
                    "{} reports surface driver borrowing generic storage for tick execution",
                    report_path.display()
                )
                .into());
            }
            continue;
        }
        if key_is_other_example_runtime_slice(key, example) {
            if flag {
                return Err(format!(
                    "{} reports other-example runtime slice `{key}` as passed for `{example}`",
                    report_path.display()
                )
                .into());
            }
            continue;
        }
        if !flag {
            continue;
        }
    }
    Ok(())
}

pub fn require_generic_runtime_slice_flags(
    slices: &serde_json::Map<String, JsonValue>,
    example: &str,
    report_path: &Path,
) -> RuntimeResult<()> {
    let common_required = if example == "generic" {
        &[
            "generic_executable_surface_inferred_from_ir",
            "ir_update_branch_table_loaded",
            "generic_scenario_loop_executor",
            "generic_schedule_instantiated_before_adapter",
            "loaded_runtime_owns_generic_schedule_storage",
            "generic_source_event_ingest",
            "generic_semantic_delta_emitter",
            "generic_source_mutation_semantic_delta_emitter",
            "generic_render_lowering_plan",
            "generic_loaded_runtime_shell",
            "generic_source_route_action_executor",
            "generic_root_text_tick_executor",
            "generic_loaded_runtime_state_summary_projection",
            "generic_root_source_dispatch",
            "generic_source_event_route_executor",
            "generic_compiled_source_route_index",
            "generic_source_route_classifier",
            "generic_source_action_batch_executor",
            "generic_source_route_scalar_expression_index",
            "generic_root_source_route_index",
            "generic_routed_root_target_application",
            "ir_list_operation_table_loaded",
            "ir_state_initializers_loaded",
            "ir_list_initializers_loaded",
            "ir_derived_value_table_loaded",
        ][..]
    } else {
        &[
            "generic_executable_surface_inferred_from_ir",
            "ir_update_branch_table_loaded",
            "generic_scenario_loop_executor",
            "generic_schedule_instantiated_before_adapter",
            "loaded_runtime_owns_generic_schedule_storage",
            "generic_source_event_ingest",
            "generic_source_binding_store",
            "generic_indexed_branch_evaluator",
            "generic_semantic_delta_emitter",
            "generic_source_mutation_semantic_delta_emitter",
            "generic_derived_value_semantic_delta_emitter",
            "generic_source_bind_semantic_delta_emitter",
            "generic_list_remove_semantic_delta_emitter",
            "generic_source_unbind_semantic_delta_emitter",
            "generic_render_lowering_plan",
            "generic_loaded_runtime_shell",
            "generic_source_route_action_executor",
            "generic_root_text_tick_executor",
            "generic_indexed_hold_commit_executor",
            "generic_list_append_source_binding_executor",
            "generic_list_remove_source_unbinding_executor",
            "generic_list_move_semantic_delta_emitter",
            "generic_list_count_retain_executor",
            "generic_loaded_runtime_state_summary_projection",
            "generic_root_source_dispatch",
            "generic_source_event_route_executor",
            "generic_compiled_source_route_index",
            "generic_source_route_classifier",
            "generic_bound_source_target_resolution",
            "generic_stale_source_key_generation_bind_epoch_rejection",
            "generic_source_action_batch_executor",
            "generic_source_route_scalar_expression_index",
            "generic_indexed_text_route_index",
            "generic_indexed_bool_route_index",
            "generic_root_source_route_index",
            "generic_list_remove_predicate_route",
            "generic_routed_root_target_application",
            "generic_routed_indexed_target_application",
            "ir_list_operation_table_loaded",
            "ir_state_initializers_loaded",
            "ir_list_initializers_loaded",
            "ir_derived_value_table_loaded",
            "generic_list_structural_commit_executor",
        ][..]
    };
    for key in common_required {
        require_slice_bool(slices, key, true, report_path)?;
    }
    require_slice_bool(
        slices,
        "surface_driver_borrows_generic_storage_for_tick",
        false,
        report_path,
    )?;
    let example_specific = match example {
        "todomvc" | "todo_mvc_physical" => &[
            "generic_common_render_patch_lowering",
            "generic_source_effects_through_action_executor",
            "generic_route_selected_indexed_text_commit_executor",
            "generic_route_selected_indexed_bool_field_commit_executor",
            "generic_summary_reads_authoritative_storage",
            "generic_root_holds_no_mirror",
            "generic_rows_hold_no_mirror",
            "generic_delta_identities_from_authoritative_storage",
            "generic_routed_source_event",
            "generic_row_routed_source_event",
            "generic_visible_row_occurrence_resolution",
            "generic_source_action_mutation_batch",
            "generic_append_mutation_batch",
            "generic_list_index_action_input_resolution",
            "generic_scenario_expectation_assertions",
            "generic_scenario_preparation",
            "generic_loaded_runtime_assertion_executor",
            "generic_routed_indexed_bool_target_application",
            "generic_routed_indexed_text_target_application",
            "generic_root_scalar_holds_from_ir",
            "generic_hold_storage_authoritative",
            "generic_indexed_text_hold_from_ir",
            "generic_indexed_bool_hold_from_ir",
            "generic_append_remove_from_ir",
            "generic_count_and_filter_views_from_ir",
        ][..],
        "cells" => &[
            "generic_common_render_patch_lowering",
            "generic_source_effects_through_action_executor",
            "generic_address_row_context_resolution",
            "generic_routed_source_event",
            "generic_cells_scenario_expectation_assertions",
            "generic_cells_scenario_storage_preparation",
            "generic_cells_dependency_cache",
            "generic_cells_evaluation_cache",
            "generic_cells_derived_storage_sync",
            "generic_cells_display_mutation_emitter",
            "generic_cells_display_protocol_lowering",
            "generic_source_action_mutation_batch",
            "generic_editor_route_uses_indexed_targets",
            "generic_committed_fields_hold_no_mirror",
            "generic_cells_edit_state_holds_from_ir",
            "generic_hold_storage_authoritative",
            "generic_summary_reads_authoritative_storage",
            "generic_hidden_list_keys_from_generic_storage",
            "generic_cells_pipeline_from_ir",
        ][..],
        "generic" | "novywave" => &[][..],
        _ => {
            return Err(format!(
                "{} cannot determine executable example for generic runtime slices",
                report_path.display()
            )
            .into());
        }
    };
    for key in example_specific {
        require_slice_bool(slices, key, true, report_path)?;
    }
    Ok(())
}

fn require_slice_bool(
    slices: &serde_json::Map<String, JsonValue>,
    key: &str,
    expected: bool,
    report_path: &Path,
) -> RuntimeResult<()> {
    let actual = slices
        .get(key)
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "{} generic runtime slices missing boolean `{key}`",
                report_path.display()
            )
        })?;
    if actual != expected {
        return Err(format!(
            "{} generic runtime slice `{key}` expected {expected}, got {actual}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_generic_runtime_slice_evidence(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let evidence = execution
        .get("generic_runtime_slice_evidence")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing generic_runtime_slice_evidence",
                report_path.display()
            )
        })?;
    if evidence.get("computed_from").and_then(JsonValue::as_str)
        != Some("typed_ir_and_compiled_program")
    {
        return Err(format!(
            "{} generic runtime slice evidence is not bound to typed IR and compiled program",
            report_path.display()
        )
        .into());
    }
    let Some(compiled) = report
        .get("compiled_schedule")
        .and_then(JsonValue::as_object)
    else {
        return Err(format!("{} missing compiled_schedule", report_path.display()).into());
    };
    for key in [
        "source_route_count",
        "source_route_id_slot_count",
        "source_route_label_slot_count",
        "source_routes_with_ids",
        "source_route_op_stream_count",
        "source_route_total_action_op_count",
        "source_route_max_action_op_count",
        "source_action_hot_path_vector_clone_count",
        "source_route_fallback_count",
        "source_route_deopt_count",
        "list_source_binding_count",
        "update_branch_count",
        "list_operation_count",
        "view_binding_count",
        "source_payload_schema_count",
        "source_payload_field_count",
        "source_payload_text_field_count",
        "source_payload_key_field_count",
        "source_payload_address_field_count",
        "root_text_slot_count",
        "root_bool_slot_count",
        "root_enum_slot_count",
        "list_memory_count",
        "list_row_template_field_count",
        "list_row_text_slot_count",
        "list_row_bool_slot_count",
        "list_row_enum_slot_count",
        "list_hidden_key_slot_count",
        "list_hidden_generation_slot_count",
        "derived_text_transform_count",
    ] {
        if evidence.get(key).and_then(JsonValue::as_u64)
            != compiled.get(key).and_then(JsonValue::as_u64)
        {
            return Err(format!(
                "{} generic runtime slice evidence `{key}` does not match compiled_schedule",
                report_path.display()
            )
            .into());
        }
    }
    if compiled
        .get("source_action_hot_path_access")
        .and_then(JsonValue::as_str)
        != Some("shared_compiled_arc_slice_by_source_id")
        || evidence
            .get("source_action_hot_path_access")
            .and_then(JsonValue::as_str)
            != Some("shared_compiled_arc_slice_by_source_id")
    {
        return Err(format!(
            "{} source route evidence does not use shared compiled action slices",
            report_path.display()
        )
        .into());
    }
    let Some(op_streams) = evidence
        .get("source_route_op_streams")
        .and_then(JsonValue::as_object)
    else {
        return Err(format!(
            "{} generic runtime slice evidence missing source_route_op_streams",
            report_path.display()
        )
        .into());
    };
    if compiled
        .get("source_route_op_streams")
        .and_then(JsonValue::as_object)
        .is_none()
    {
        return Err(format!(
            "{} compiled_schedule missing source_route_op_streams",
            report_path.display()
        )
        .into());
    }
    if op_streams
        .get("event_hot_path_vector_clone_count")
        .and_then(JsonValue::as_u64)
        != Some(0)
        || op_streams.get("fallback_count").and_then(JsonValue::as_u64) != Some(0)
        || op_streams.get("deopt_count").and_then(JsonValue::as_u64) != Some(0)
    {
        return Err(format!(
            "{} source route op streams report clone/fallback/deopt activity",
            report_path.display()
        )
        .into());
    }
    let evidence_storage = evidence
        .get("typed_storage_layout")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} generic runtime slice evidence missing typed_storage_layout",
                report_path.display()
            )
        })?;
    let compiled_storage = compiled
        .get("typed_storage_layout")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} compiled_schedule missing typed_storage_layout",
                report_path.display()
            )
        })?;
    if evidence_storage
        .get("computed_from")
        .and_then(JsonValue::as_str)
        != Some("typed_ir_state_and_list_tables")
    {
        return Err(format!(
            "{} typed storage layout evidence is not derived from typed IR tables",
            report_path.display()
        )
        .into());
    }
    for key in [
        "root_text_slot_count",
        "root_bool_slot_count",
        "root_enum_slot_count",
        "list_memory_count",
        "list_row_template_field_count",
        "list_row_text_slot_count",
        "list_row_bool_slot_count",
        "list_row_enum_slot_count",
        "list_hidden_key_slot_count",
        "list_hidden_generation_slot_count",
    ] {
        if evidence_storage.get(key).and_then(JsonValue::as_u64)
            != compiled_storage.get(key).and_then(JsonValue::as_u64)
        {
            return Err(format!(
                "{} typed storage layout evidence `{key}` does not match compiled_schedule",
                report_path.display()
            )
            .into());
        }
    }
    for (key, expected) in [
        ("list_order_storage_kind", "separate_visible_order_slots"),
        ("list_valid_storage_kind", "bitvec_valid_slots"),
        ("list_free_storage_kind", "free_slot_stack"),
        (
            "list_source_binding_storage_kind",
            "dense_source_and_row_slots",
        ),
    ] {
        if evidence_storage.get(key).and_then(JsonValue::as_str) != Some(expected)
            || compiled_storage.get(key).and_then(JsonValue::as_str) != Some(expected)
        {
            return Err(format!(
                "{} typed storage layout `{key}` is not `{expected}`",
                report_path.display()
            )
            .into());
        }
    }
    let source_route_count = evidence
        .get("source_route_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let route_action_count = evidence
        .get("source_route_action_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if source_route_count == 0 || route_action_count == 0 {
        return Err(format!(
            "{} generic runtime slice evidence has no compiled source route actions",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_expression_coverage(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let coverage = report
        .get("expression_coverage")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing expression_coverage", report_path.display()))?;
    if coverage.get("computed_from").and_then(JsonValue::as_str) != Some("parser_ast_and_typed_ir")
    {
        return Err(format!(
            "{} expression coverage is not bound to parser AST and typed IR",
            report_path.display()
        )
        .into());
    }
    if let (Some(report_expression_count), Some(coverage_expression_count)) = (
        report.get("expression_count").and_then(JsonValue::as_u64),
        coverage
            .get("ast_expression_count")
            .and_then(JsonValue::as_u64),
    ) && report_expression_count != coverage_expression_count
    {
        return Err(format!(
            "{} expression_count does not match expression_coverage.ast_expression_count",
            report_path.display()
        )
        .into());
    }
    for key in [
        "unknown_ast_expression_count",
        "unknown_initial_value_count",
        "unknown_list_initializer_count",
        "unknown_list_initial_value_count",
        "unknown_update_expression_count",
        "unknown_list_predicate_count",
        "unknown_derived_value_count",
    ] {
        let count = coverage
            .get(key)
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} expression_coverage missing numeric `{key}`",
                    report_path.display()
                )
            })?;
        if count != 0 {
            return Err(format!(
                "{} expression_coverage `{key}` is {count}; executable reports cannot rely on unknown parser/lowering fallback",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn key_is_other_example_runtime_slice(key: &str, example: &str) -> bool {
    match example {
        "todomvc" | "todo_mvc_physical" => {
            key.starts_with("generic_cells") || key.starts_with("cells_") || key.contains("_cells_")
        }
        "cells" => {
            key.starts_with("generic_todomvc")
                || key.starts_with("todomvc_")
                || key.contains("_todo_")
                || key.contains("_todomvc_")
        }
        _ => false,
    }
}

fn report_program_kind(report: &JsonValue) -> Option<&str> {
    let candidate = report
        .get("program_kind")
        .or_else(|| report.get("example"))
        .and_then(JsonValue::as_str)
        .or_else(|| {
            report
                .get("runtime_execution")
                .and_then(|execution| execution.get("adapter_kind"))
                .and_then(JsonValue::as_str)
        })?;
    matches!(candidate, "todomvc" | "cells").then_some(candidate)
}

fn verify_runtime_report_contract(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "source_path",
        "runtime_profile",
        "renderer",
        "window_mode",
        "window_backend",
        "display_server",
        "display_scale",
        "window_size",
        "framebuffer_size",
        "total_ticks",
        "total_source_events",
        "total_semantic_deltas",
        "total_render_deltas",
        "max_dirty_nodes",
        "max_dirty_keys",
        "allocations",
        "latency_ms_p50_p95_p99_max",
        "rss_delta_mib_steady_peak",
        "vram_delta_mib_steady_peak_or_unavailable_reason",
        "dirty_node_count_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "failure_artifacts",
        "semantic_delta_protocol_batches",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} runtime report missing documented field `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    verify_semantic_delta_protocol_batches(report, report_path)?;
    verify_render_patch_protocol_identity(report, report_path)?;
    let dirty_node_summary = report
        .get("dirty_node_count_p50_p95_p99_max")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime report dirty_node_count_p50_p95_p99_max is not an object",
                report_path.display()
            )
        })?;
    if dirty_node_summary.get("unavailable_reason").is_some() {
        return Err(format!(
            "{} runtime report still marks dirty node counts unavailable",
            report_path.display()
        )
        .into());
    }
    for key in ["p50", "p95", "p99", "max"] {
        if dirty_node_summary
            .get(key)
            .and_then(JsonValue::as_f64)
            .is_none()
        {
            return Err(format!(
                "{} runtime report dirty_node_count_p50_p95_p99_max missing numeric `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let rss_delta = report
        .get("rss_delta_mib_steady_peak")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime report rss_delta_mib_steady_peak is not an object",
                report_path.display()
            )
        })?;
    for key in ["steady", "peak", "baseline", "measurement"] {
        if rss_delta.get(key).is_none() {
            return Err(format!(
                "{} runtime report RSS delta missing `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let baseline = report
        .get("baseline_rss_mib")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| format!("{} missing numeric baseline_rss_mib", report_path.display()))?;
    let steady = report
        .get("steady_rss_mib")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| format!("{} missing numeric steady_rss_mib", report_path.display()))?;
    if steady < baseline {
        return Err(format!(
            "{} has steady RSS below baseline RSS",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

pub fn verify_playground_surface_report(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    for key in [
        "example_selector",
        "code_editor",
        "run_reset_step_controls",
        "render_preview",
        "semantic_delta_log",
        "selected_value_inspector",
        "dependency_explanation_panel",
    ] {
        let claimed = report
            .get("playground_surface")
            .and_then(|surface| surface.get(key))
            .and_then(JsonValue::as_bool)
            == Some(true);
        if !claimed {
            return Err(format!(
                "{} playground surface `{key}` is not claimed present",
                report_path.display()
            )
            .into());
        }
        let proof = report
            .get("playground_surface_visible_bounds")
            .and_then(|bounds| bounds.get(key))
            .ok_or_else(|| {
                format!(
                    "{} playground surface `{key}` missing visible bounds proof",
                    report_path.display()
                )
            })?;
        if proof.get("pass").and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!(
                "{} playground surface `{key}` visible bounds did not pass",
                report_path.display()
            )
            .into());
        }
        let elements = proof
            .get("elements")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "{} playground surface `{key}` missing element bounds",
                    report_path.display()
                )
            })?;
        if elements.is_empty() {
            return Err(format!(
                "{} playground surface `{key}` has no visible elements",
                report_path.display()
            )
            .into());
        }
        for element in elements {
            let visible = element.get("visible").and_then(JsonValue::as_bool) == Some(true);
            let width = element
                .get("bounds")
                .and_then(|bounds| bounds.get("width"))
                .and_then(JsonValue::as_f64)
                .unwrap_or_default();
            let height = element
                .get("bounds")
                .and_then(|bounds| bounds.get("height"))
                .and_then(JsonValue::as_f64)
                .unwrap_or_default();
            if !visible || width <= 0.0 || height <= 0.0 {
                return Err(format!(
                    "{} playground surface `{key}` has an invisible or zero-size element",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_cells_visible_reality_report(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let dimensions = report
        .get("source_grid_dimensions")
        .ok_or_else(|| format!("{} missing source_grid_dimensions", report_path.display()))?;
    if dimensions.get("columns").and_then(JsonValue::as_u64) != Some(26)
        || dimensions.get("rows").and_then(JsonValue::as_u64) != Some(100)
    {
        return Err(format!(
            "{} Cells visible reality report is not bound to the 26x100 source grid",
            report_path.display()
        )
        .into());
    }
    let viewport = report
        .get("viewport_dimensions")
        .ok_or_else(|| format!("{} missing viewport_dimensions", report_path.display()))?;
    let viewport_columns = viewport
        .get("columns")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let viewport_rows = viewport
        .get("rows")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let viewport_cells = viewport
        .get("cell_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if viewport_columns < 26 || viewport_rows < 100 || viewport_cells < 2600 {
        return Err(format!(
            "{} Cells visible viewport is too small: columns={viewport_columns}, rows={viewport_rows}, cells={viewport_cells}",
            report_path.display()
        )
        .into());
    }
    let rendered = report
        .get("rendered_cell_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if rendered < viewport_cells {
        return Err(format!(
            "{} rendered only {rendered} addressed inputs for {viewport_cells} visible cells",
            report_path.display()
        )
        .into());
    }
    let samples = report
        .get("visible_address_samples")
        .ok_or_else(|| format!("{} missing visible_address_samples", report_path.display()))?;
    let has_required = samples
        .get("required_present")
        .and_then(JsonValue::as_array)
        .is_some_and(|items| {
            let values = items
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<BTreeSet<_>>();
            values.contains("Z0") && values.contains("A99") && values.contains("Z99")
        });
    if !has_required {
        return Err(format!(
            "{} visible address samples do not prove non-A-D spreadsheet cells",
            report_path.display()
        )
        .into());
    }
    let screenshot = report
        .get("screenshot_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing screenshot_path", report_path.display()))?;
    let expected_hash = report
        .get("screenshot_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing screenshot_sha256", report_path.display()))?;
    let actual_hash = sha256_file(Path::new(screenshot))?;
    if actual_hash != expected_hash {
        return Err(format!(
            "{} has stale screenshot_sha256 for `{screenshot}`",
            report_path.display()
        )
        .into());
    }
    let nonblank = report
        .get("nonblank_screenshot_hashes")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} missing nonblank_screenshot_hashes",
                report_path.display()
            )
        })?;
    let proved_nonblank = nonblank.iter().any(|entry| {
        entry
            .get("nonzero_channels")
            .and_then(JsonValue::as_u64)
            .unwrap_or_default()
            > 0
            && entry
                .get("unique_rgba_values")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default()
                > 1
    });
    if !proved_nonblank {
        return Err(format!(
            "{} Cells visible reality screenshot is blank",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_render_patch_protocol_identity(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let patches = report
        .get("render_patches")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing render_patches array", report_path.display()))?;
    let total_render_deltas = report
        .get("total_render_deltas")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default() as usize;
    if patches.len() != total_render_deltas {
        return Err(format!(
            "{} render_patches length {} does not match total_render_deltas {total_render_deltas}",
            report_path.display(),
            patches.len()
        )
        .into());
    }
    for patch in patches {
        let kind = patch
            .get("kind")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unknown>");
        let target = patch
            .get("target")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        let keyed_patch = target.starts_with("todos:");
        if keyed_patch {
            for key in ["list_id", "key", "generation"] {
                if patch.get(key).is_none_or(JsonValue::is_null) {
                    return Err(format!(
                        "{} keyed render patch `{kind}` missing `{key}`",
                        report_path.display()
                    )
                    .into());
                }
            }
        }
        if matches!(kind, "BindSource" | "UnbindSource") {
            for key in ["source_id", "bind_epoch"] {
                if patch.get(key).is_none_or(JsonValue::is_null) {
                    return Err(format!(
                        "{} source render patch `{kind}` missing `{key}`",
                        report_path.display()
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

pub fn verify_semantic_delta_protocol_batches(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let batches = report
        .get("semantic_delta_protocol_batches")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} runtime report semantic_delta_protocol_batches is not an array",
                report_path.display()
            )
        })?;
    let expected_program_hash = report
        .get("program_hash")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing program_hash", report_path.display()))?;
    let mut expected_base_epoch = 0u64;
    let mut expected_runtime_id: Option<String> = None;
    let mut total_changes = 0usize;
    for batch in batches {
        let base_epoch = json_u64(batch, "base_epoch", report_path)?;
        let next_epoch = json_u64(batch, "next_epoch", report_path)?;
        if base_epoch != expected_base_epoch || next_epoch != base_epoch + 1 {
            return Err(format!(
                "{} has non-monotonic semantic delta epochs",
                report_path.display()
            )
            .into());
        }
        if batch.get("program_hash").and_then(JsonValue::as_str) != Some(expected_program_hash) {
            return Err(format!(
                "{} semantic delta batch has stale program_hash",
                report_path.display()
            )
            .into());
        }
        let runtime_id = batch
            .get("runtime_id")
            .and_then(JsonValue::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "{} semantic delta batch missing nonempty runtime_id",
                    report_path.display()
                )
            })?;
        match &expected_runtime_id {
            Some(expected) if expected != runtime_id => {
                return Err(format!(
                    "{} semantic delta batch changed runtime_id",
                    report_path.display()
                )
                .into());
            }
            None => expected_runtime_id = Some(runtime_id.to_owned()),
            _ => {}
        }
        let server_tick = json_u64(batch, "server_tick", report_path)?;
        if server_tick != next_epoch {
            return Err(format!(
                "{} semantic delta batch server_tick does not match next_epoch",
                report_path.display()
            )
            .into());
        }
        if batch
            .get("step_id")
            .and_then(JsonValue::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!(
                "{} semantic delta batch missing nonempty step_id",
                report_path.display()
            )
            .into());
        }
        let changes = batch
            .get("changes")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "{} semantic delta batch missing changes array",
                    report_path.display()
                )
            })?;
        for change in changes {
            verify_semantic_delta_identity(change, report_path)?;
        }
        total_changes += changes.len();
        expected_base_epoch = next_epoch;
    }
    let total_semantic_deltas = report
        .get("total_semantic_deltas")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default() as usize;
    if total_changes != total_semantic_deltas {
        return Err(format!(
            "{} semantic delta batches contain {total_changes} changes, expected {total_semantic_deltas}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_semantic_delta_identity(change: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let kind = change
        .get("kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("<unknown>");
    let list_id_present = change.get("list_id").is_some_and(|value| !value.is_null());
    if list_id_present {
        for key in ["key", "generation"] {
            if change.get(key).is_none_or(JsonValue::is_null) {
                return Err(format!(
                    "{} keyed semantic delta `{kind}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if matches!(kind, "SourceBind" | "SourceUnbind") {
        for key in ["source_id", "bind_epoch"] {
            if change.get(key).is_none_or(JsonValue::is_null) {
                return Err(format!(
                    "{} source semantic delta `{kind}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn json_u64(value: &JsonValue, key: &str, report_path: &Path) -> RuntimeResult<u64> {
    value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
        format!(
            "{} semantic delta protocol batch missing numeric `{key}`",
            report_path.display()
        )
        .into()
    })
}

fn verify_speed_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    verify_report_mode(report, report_path, "interaction")?;
    if report.get("build_profile").and_then(JsonValue::as_str) != Some("release") {
        return Err(format!(
            "{} speed report was not generated by a release binary",
            report_path.display()
        )
        .into());
    }
    for key in [
        "cpu_model",
        "gpu_model_if_available",
        "os",
        "display_server",
        "window_backend",
        "display_scale",
        "semantic_tick_ms_p50_p95_p99_max",
        "render_lowering_ms_p50_p95_p99_max",
        "ply_patch_apply_ms_p50_p95_p99_max",
        "input_to_idle_ms_p50_p95_p99_max",
        "frame_time_ms_p50_p95_p99_max",
        "missed_frame_count",
        "operation_count",
        "per_operation_outliers",
        "baseline_rss_mib",
        "steady_rss_mib",
        "peak_rss_mib",
        "baseline_vram_mib_if_available",
        "steady_vram_mib_if_available",
        "peak_vram_mib_if_available",
        "heap_alloc_count_per_step",
        "heap_alloc_bytes_per_step",
        "apply_heap_alloc_count",
        "apply_heap_alloc_bytes",
        "expectation_heap_alloc_count",
        "expectation_heap_alloc_bytes",
        "graph_node_count",
        "graph_rebuild_count",
        "list_slot_count",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "dirty_node_count_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "render_patch_count_p50_p95_p99_max",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} speed report missing documented field `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    verify_runtime_profile_metadata(report, report_path)?;
    let dirty_node_summary = report
        .get("dirty_node_count_p50_p95_p99_max")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} speed report dirty_node_count_p50_p95_p99_max is not an object",
                report_path.display()
            )
        })?;
    if dirty_node_summary.get("unavailable_reason").is_some() {
        return Err(format!(
            "{} speed report still marks dirty node counts unavailable",
            report_path.display()
        )
        .into());
    }
    for key in ["p50", "p95", "p99", "max"] {
        if dirty_node_summary
            .get(key)
            .and_then(JsonValue::as_f64)
            .is_none()
        {
            return Err(format!(
                "{} speed report dirty_node_count_p50_p95_p99_max missing numeric `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let checks = report
        .get("budget_check")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} speed report missing budget_check",
                report_path.display()
            )
        })?;
    let failed = checks
        .iter()
        .filter_map(|(name, value)| {
            (value.get("pass").and_then(JsonValue::as_bool) != Some(true)).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "{} speed budget failed: {}",
            report_path.display(),
            failed.join(", ")
        )
        .into());
    }
    verify_speed_allocation_budget_semantics(report, report_path)?;
    verify_speed_stress_profiles(report, report_path)
}

fn verify_speed_allocation_budget_semantics(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let runtime_profile = report
        .get("runtime_profile")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} speed report missing runtime_profile",
                report_path.display()
            )
        })?;
    let allocation_budget = report
        .get("budget_check")
        .and_then(|budget| budget.get("allocation_budget"))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} speed report missing allocation budget check",
                report_path.display()
            )
        })?;
    if allocation_budget.get("pass").and_then(JsonValue::as_bool) != Some(true) {
        return Err(format!(
            "{} speed report allocation budget did not pass",
            report_path.display()
        )
        .into());
    }
    let allocation_budget_applies = allocation_budget
        .get("applies")
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "{} speed report allocation budget missing applies flag",
                report_path.display()
            )
        })?;
    let measured_budget_allocs = allocation_budget
        .get("measured_bounded_profile_allocs_after_warmup")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} speed report allocation budget missing measured allocation count",
                report_path.display()
            )
        })?;
    let measured_report_allocs = report
        .get("allocations")
        .and_then(|allocations| allocations.get("bounded_profile_allocs_after_warmup"))
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} speed report allocations missing bounded_profile_allocs_after_warmup",
                report_path.display()
            )
        })?;
    if measured_budget_allocs != measured_report_allocs {
        return Err(format!(
            "{} speed report allocation budget measured {measured_budget_allocs} but allocations report measured {measured_report_allocs}",
            report_path.display()
        )
        .into());
    }
    let unapplied_reason = allocation_budget
        .get("unapplied_reason")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if runtime_profile == "software_dynamic" {
        if allocation_budget_applies {
            return Err(format!(
                "{} software_dynamic speed report must not claim bounded allocation budget applies",
                report_path.display()
            )
            .into());
        }
        if unapplied_reason.trim().is_empty() {
            return Err(format!(
                "{} software_dynamic speed report missing allocation-budget unapplied reason",
                report_path.display()
            )
            .into());
        }
    } else {
        if !matches!(runtime_profile, "software_bounded" | "hardware_bounded") {
            return Err(format!(
                "{} speed report has unknown runtime_profile `{runtime_profile}`",
                report_path.display()
            )
            .into());
        }
        if !allocation_budget_applies {
            return Err(format!(
                "{} bounded speed report must apply the allocation budget",
                report_path.display()
            )
            .into());
        }
        if allocation_budget
            .get("unapplied_reason")
            .is_some_and(|reason| !reason.is_null())
        {
            return Err(format!(
                "{} bounded speed report must not carry an allocation-budget unapplied reason",
                report_path.display()
            )
            .into());
        }
        if measured_report_allocs != 0 {
            return Err(format!(
                "{} bounded speed report does not prove zero post-warmup allocations",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_runtime_profile_metadata(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let profile = report
        .get("runtime_profile")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing runtime_profile", report_path.display()))?;
    if !matches!(
        profile,
        "software_dynamic" | "software_bounded" | "hardware_bounded"
    ) {
        return Err(format!(
            "{} has unknown runtime_profile `{profile}`",
            report_path.display()
        )
        .into());
    }
    let detail_name = report
        .get("runtime_profile_detail")
        .and_then(|detail| detail.get("name"))
        .and_then(JsonValue::as_str);
    if detail_name != Some(profile) {
        return Err(format!(
            "{} runtime_profile_detail.name does not match runtime_profile",
            report_path.display()
        )
        .into());
    }
    let capacities = report
        .get("capacities")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing capacities object", report_path.display()))?;
    let all_lists_bounded = capacities
        .get("all_lists_bounded")
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "{} capacities missing boolean all_lists_bounded",
                report_path.display()
            )
        })?;
    let lists = capacities
        .get("lists")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} capacities missing lists array", report_path.display()))?;
    for list in lists {
        for key in [
            "name",
            "declared_capacity",
            "effective_capacity",
            "capacity_source",
            "dynamic_growth_allowed",
            "overflow_behavior",
        ] {
            if list.get(key).is_none() {
                return Err(format!(
                    "{} capacity report missing list field `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if matches!(profile, "software_bounded" | "hardware_bounded") && !all_lists_bounded {
        return Err(format!(
            "{} claims {profile} while at least one list has no effective capacity",
            report_path.display()
        )
        .into());
    }
    if profile == "software_dynamic" && all_lists_bounded {
        return Err(format!(
            "{} claims software_dynamic while every list has an effective capacity",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_benchmark_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    verify_report_mode(report, report_path, "diagnostic")?;
    let benchmark = report
        .get("benchmark")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing benchmark object",
                report_path.display()
            )
        })?;
    let iterations = benchmark
        .get("iterations")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing positive iteration count",
                report_path.display()
            )
        })?;
    if iterations == 0 {
        return Err(format!(
            "{} benchmark report has zero iterations",
            report_path.display()
        )
        .into());
    }
    let total_ms = benchmark
        .get("total_ms")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing total_ms",
                report_path.display()
            )
        })?;
    let average_ms = benchmark
        .get("average_ms_per_iteration")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing average_ms_per_iteration",
                report_path.display()
            )
        })?;
    if total_ms <= 0.0 || average_ms <= 0.0 {
        return Err(format!(
            "{} benchmark report timing must be positive",
            report_path.display()
        )
        .into());
    }
    let expected_average = total_ms / iterations as f64;
    if (average_ms - expected_average).abs() > 0.001_f64.max(expected_average.abs() * 0.001) {
        return Err(format!(
            "{} benchmark average does not match total_ms / iterations",
            report_path.display()
        )
        .into());
    }
    if benchmark
        .get("speed_report_layer")
        .and_then(JsonValue::as_str)
        != Some("speed")
    {
        return Err(format!(
            "{} benchmark report is not linked to a speed-layer report",
            report_path.display()
        )
        .into());
    }
    if benchmark
        .get("iteration_scope")
        .and_then(JsonValue::as_str)
        .is_none_or(|scope| !scope.contains("full_speed_layer_scenario"))
    {
        return Err(format!(
            "{} benchmark report does not describe full speed-layer scenario iterations",
            report_path.display()
        )
        .into());
    }
    let heap_alloc_count_after_warmup = benchmark
        .get("heap_alloc_count_after_warmup")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing heap_alloc_count_after_warmup",
                report_path.display()
            )
        })?;
    let allocation_budget = report
        .get("budget_check")
        .and_then(|budget| budget.get("allocation_budget"))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing allocation budget check",
                report_path.display()
            )
        })?;
    let allocation_budget_passes = allocation_budget
        .get("pass")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let allocation_budget_applies = allocation_budget
        .get("applies")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    if allocation_budget_applies && heap_alloc_count_after_warmup != 0 {
        return Err(format!(
            "{} benchmark report does not prove zero post-warmup allocations",
            report_path.display()
        )
        .into());
    }
    if !allocation_budget_passes {
        return Err(format!(
            "{} benchmark report allocation budget did not pass",
            report_path.display()
        )
        .into());
    }
    if report_command_is(report, "bench-example")
        && benchmark
            .get("example")
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(format!(
            "{} bench-example report missing benchmark.example",
            report_path.display()
        )
        .into());
    }

    let speed_report_path = benchmark
        .get("speed_report_path")
        .and_then(JsonValue::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing speed_report_path",
                report_path.display()
            )
        })?;
    let speed_report_path = Path::new(speed_report_path);
    verify_report_schema(speed_report_path)?;
    let linked: JsonValue = serde_json::from_slice(&fs::read(speed_report_path)?)?;
    for key in [
        "source_hash",
        "scenario_hash",
        "program_hash",
        "budget_hash",
        "graph_node_count",
        "budget_check",
        "input_to_idle_ms_p50_p95_p99_max",
        "semantic_tick_ms_p50_p95_p99_max",
        "render_lowering_ms_p50_p95_p99_max",
        "ply_patch_apply_ms_p50_p95_p99_max",
        "frame_time_ms_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "render_patch_count_p50_p95_p99_max",
        "graph_rebuild_count",
        "allocations",
        "stress_profiles",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
    ] {
        if report.get(key) != linked.get(key) {
            return Err(format!(
                "{} benchmark report does not copy `{key}` from linked speed report `{}`",
                report_path.display(),
                speed_report_path.display()
            )
            .into());
        }
    }
    let linked_hash = sha256_file(speed_report_path)?;
    let artifact_hash_matches = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .is_some_and(|artifacts| {
            artifacts.iter().any(|artifact| {
                artifact.get("path").and_then(JsonValue::as_str)
                    == Some(speed_report_path.to_string_lossy().as_ref())
                    && artifact.get("sha256").and_then(JsonValue::as_str)
                        == Some(linked_hash.as_str())
            })
        });
    if !artifact_hash_matches {
        return Err(format!(
            "{} benchmark report does not hash linked speed report `{}`",
            report_path.display(),
            speed_report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_speed_stress_profiles(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let stress_profiles = match report.get("stress_profiles") {
        Some(stress_profiles) => stress_profiles.as_array().ok_or_else(|| {
            format!(
                "{} speed report stress_profiles is not an array",
                report_path.display()
            )
        })?,
        None if report.get("runtime_profile").and_then(JsonValue::as_str)
            == Some("software_dynamic") =>
        {
            return Ok(());
        }
        None => {
            return Err(format!(
                "{} speed report missing stress_profiles",
                report_path.display()
            )
            .into());
        }
    };
    if stress_profiles.is_empty() {
        return Err(format!(
            "{} speed report has no stress profiles",
            report_path.display()
        )
        .into());
    }
    let base_graph_node_count = report
        .get("graph_node_count")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} speed report missing graph_node_count",
                report_path.display()
            )
        })?;
    for profile in stress_profiles {
        let name = profile
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unnamed>");
        let graph_node_count = profile
            .get("graph_node_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing graph_node_count",
                    report_path.display()
                )
            })?;
        if graph_node_count != base_graph_node_count {
            return Err(format!(
                "{} stress profile `{name}` changed graph topology",
                report_path.display()
            )
            .into());
        }
        if profile
            .get("graph_clones_per_item")
            .and_then(JsonValue::as_u64)
            != Some(0)
        {
            return Err(format!(
                "{} stress profile `{name}` cloned runtime graph per item",
                report_path.display()
            )
            .into());
        }
        let heap_alloc_count = profile
            .get("heap_alloc_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing heap_alloc_count",
                    report_path.display()
                )
            })?;
        let heap_alloc_bytes = profile
            .get("heap_alloc_bytes")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing heap_alloc_bytes",
                    report_path.display()
                )
            })?;
        if heap_alloc_count != 0 || heap_alloc_bytes != 0 {
            return Err(format!(
                "{} stress profile `{name}` allocated after warmup: {heap_alloc_count} allocations / {heap_alloc_bytes} bytes",
                report_path.display()
            )
            .into());
        }
        let dirty_count = profile
            .get("dirty_key_count")
            .or_else(|| profile.get("dirty_cell_count"))
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing dirty key/cell count",
                    report_path.display()
                )
            })?;
        let expected_fanout = profile.get("expected_fanout").and_then(JsonValue::as_u64);
        let expected_dirty_count = profile
            .get("expected_dirty_cell_count")
            .and_then(JsonValue::as_u64);
        let dirty_count_is_proportional = if let Some(expected_fanout) = expected_fanout {
            let allowed_dirty_count = expected_fanout.saturating_add(1);
            dirty_count == expected_dirty_count.unwrap_or(allowed_dirty_count)
                && dirty_count <= allowed_dirty_count
        } else {
            (1..=8).contains(&dirty_count)
        };
        if !dirty_count_is_proportional {
            return Err(format!(
                "{} stress profile `{name}` has non-proportional dirty work count {dirty_count}",
                report_path.display()
            )
            .into());
        }
        let render_patch_count = profile
            .get("render_patch_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing render_patch_count",
                    report_path.display()
                )
            })?;
        if render_patch_count > 8 {
            return Err(format!(
                "{} stress profile `{name}` has non-proportional render patch count {render_patch_count}",
                report_path.display()
            )
            .into());
        }
    }
    verify_documented_stress_profile_coverage(report, report_path, stress_profiles)?;
    Ok(())
}

fn verify_documented_stress_profile_coverage(
    report: &JsonValue,
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    let program_kind = report_program_kind(report);
    match program_kind {
        Some("todomvc") => verify_todomvc_stress_profile_coverage(report_path, stress_profiles),
        Some("cells") => verify_cells_stress_profile_coverage(report_path, stress_profiles),
        _ => Ok(()),
    }
}

fn verify_todomvc_stress_profile_coverage(
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    let rows = stress_profiles
        .iter()
        .filter_map(|profile| profile.get("rows").and_then(JsonValue::as_u64))
        .collect::<BTreeSet<_>>();
    for required in [1_000, 10_000] {
        if !rows.contains(&required) {
            return Err(format!(
                "{} TodoMVC speed report missing documented {required}-row stress profile",
                report_path.display()
            )
            .into());
        }
    }
    let has_10k_move = stress_profiles.iter().any(|profile| {
        profile
            .get("name")
            .and_then(JsonValue::as_str)
            .is_some_and(|name| name.contains("10000") && name.contains("move"))
    });
    if !has_10k_move {
        return Err(format!(
            "{} TodoMVC speed report missing documented 10,000-row move/LIST-change stress profile",
            report_path.display()
        )
        .into());
    }
    for profile in stress_profiles {
        if let Some(rows) = profile.get("rows").and_then(JsonValue::as_u64) {
            let slots = profile
                .get("list_slot_count")
                .and_then(JsonValue::as_u64)
                .ok_or_else(|| {
                    format!(
                        "{} TodoMVC stress profile missing list_slot_count",
                        report_path.display()
                    )
                })?;
            if slots != rows {
                return Err(format!(
                    "{} TodoMVC stress profile row count {rows} does not match list_slot_count {slots}",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_cells_stress_profile_coverage(
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    for required in [
        "cells-26x100-unrelated-edit",
        "cells-26x100-dependent-edit",
        "cells-26x100-fanout-100-update",
    ] {
        if !stress_profiles
            .iter()
            .any(|profile| profile.get("name").and_then(JsonValue::as_str) == Some(required))
        {
            return Err(format!(
                "{} Cells speed report missing documented stress profile `{required}`",
                report_path.display()
            )
            .into());
        }
    }
    for profile in stress_profiles {
        let name = profile
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unnamed>");
        for key in [
            "cells",
            "dirty_cell_count",
            "recompute_candidate_count",
            "expression_eval_call_count",
            "dependency_edge_walk_count",
            "recomputed_cells",
        ] {
            if profile.get(key).is_none() {
                return Err(format!(
                    "{} Cells stress profile `{name}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
        if profile.get("cells").and_then(JsonValue::as_u64) != Some(26 * 100) {
            return Err(format!(
                "{} Cells stress profile `{name}` is not bound to the documented 26x100 grid",
                report_path.display()
            )
            .into());
        }
    }
    let fanout = stress_profiles
        .iter()
        .find(|profile| {
            profile.get("name").and_then(JsonValue::as_str)
                == Some("cells-26x100-fanout-100-update")
        })
        .ok_or_else(|| {
            format!(
                "{} missing Cells fanout stress profile",
                report_path.display()
            )
        })?;
    let expected_fanout = fanout
        .get("expected_fanout")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} Cells fanout stress profile missing expected_fanout",
                report_path.display()
            )
        })?;
    let expected_dirty = expected_fanout + 1;
    for key in [
        "dirty_cell_count",
        "recompute_candidate_count",
        "recomputed_cell_count",
    ] {
        let value = fanout.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            format!(
                "{} Cells fanout stress profile missing `{key}`",
                report_path.display()
            )
        })?;
        if value != expected_dirty {
            return Err(format!(
                "{} Cells fanout stress profile `{key}`={value}, expected {expected_dirty}",
                report_path.display()
            )
            .into());
        }
    }
    let edge_walks = fanout
        .get("dependency_edge_walk_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if edge_walks < expected_fanout {
        return Err(format!(
            "{} Cells fanout stress profile walked only {edge_walks} dependency edges for fanout {expected_fanout}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_report_file_hash(
    report: &JsonValue,
    report_path: &Path,
    path_key: &str,
    hash_key: &str,
) -> RuntimeResult<()> {
    let Some(expected) = report.get(hash_key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    if matches!(expected, "n/a" | "missing" | "missing-budget") {
        return Ok(());
    }
    if path_key == "source_path" && hash_key == "source_hash" {
        if verify_report_source_files_hash(report, report_path, expected)? {
            return Ok(());
        }
    }
    let Some(file_path) = report.get(path_key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    let actual = sha256_file(Path::new(file_path))?;
    if actual != expected {
        return Err(format!(
            "{} has stale `{hash_key}` for `{file_path}`",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_report_source_files_hash(
    report: &JsonValue,
    report_path: &Path,
    expected: &str,
) -> RuntimeResult<bool> {
    let Some((files_key, files)) = report
        .get("project_files")
        .and_then(JsonValue::as_array)
        .map(|files| ("project_files", files))
        .or_else(|| {
            report
                .get("source_files")
                .and_then(JsonValue::as_array)
                .map(|files| ("source_files", files))
        })
    else {
        return Ok(false);
    };
    let paths = files
        .iter()
        .filter_map(|file| {
            file.as_str().or_else(|| {
                file.get("path")
                    .and_then(JsonValue::as_str)
                    .or_else(|| file.get("source").and_then(JsonValue::as_str))
            })
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(false);
    }
    let actual = sha256_combined_source_files(&paths)?;
    if actual != expected {
        return Err(format!(
            "{} has stale `source_hash` for manifest {files_key}",
            report_path.display()
        )
        .into());
    }
    Ok(true)
}

fn sha256_combined_source_files(paths: &[&str]) -> RuntimeResult<String> {
    if paths.len() == 1 {
        return Ok(sha256_bytes(&fs::read(paths[0])?));
    }
    let mut canonical = String::new();
    for path in paths {
        canonical.push_str(path);
        canonical.push('\0');
        canonical.push_str(&sha256_bytes(&fs::read(path)?));
        canonical.push('\0');
    }
    Ok(sha256_bytes(canonical.as_bytes()))
}

fn verify_artifact_hashes(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let Some(artifacts) = report.get("artifact_sha256s").and_then(JsonValue::as_array) else {
        return Ok(());
    };
    for artifact in artifacts {
        let Some(path) = artifact.get("path").and_then(JsonValue::as_str) else {
            return Err(format!("{} has artifact without path", report_path.display()).into());
        };
        let Some(expected) = artifact.get("sha256").and_then(JsonValue::as_str) else {
            return Err(format!("{} has artifact without sha256", report_path.display()).into());
        };
        let actual = sha256_file(Path::new(path))?;
        if actual != expected {
            return Err(format!(
                "{} has stale artifact hash for `{path}`",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_headed_artifacts(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "window_pid",
        "window_title",
        "display_server",
        "display_socket_or_compositor_connection",
        "display_scale",
        "window_size",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "checkpoint_screenshot_or_video_paths",
    ] {
        if report.get(key).is_none_or(JsonValue::is_null) {
            return Err(
                format!("{} missing headed metadata `{key}`", report_path.display()).into(),
            );
        }
    }
    for key in [
        "window_title",
        "display_server",
        "display_socket_or_compositor_connection",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
    ] {
        if report
            .get(key)
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(format!(
                "{} has empty headed metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let injection_method = report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let focus_free = injection_method == "ply_synthetic_focus_free_render_metadata";
    if focus_free {
        if report.get("input_backend").and_then(JsonValue::as_str)
            != Some("ply-synthetic-focus-free")
        {
            return Err(format!(
                "{} focus-free headed report must use ply-synthetic-focus-free input backend",
                report_path.display()
            )
            .into());
        }
        if report.get("os_focus_required").and_then(JsonValue::as_bool) != Some(false)
            || report
                .get("os_keyboard_or_pointer_used")
                .and_then(JsonValue::as_bool)
                != Some(false)
        {
            return Err(format!(
                "{} focus-free headed report must prove it used no OS keyboard or pointer injection",
                report_path.display()
            )
            .into());
        }
        if !report
            .get("os_input_tools_used")
            .and_then(JsonValue::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Err(format!(
                "{} focus-free headed report must have an empty os_input_tools_used list",
                report_path.display()
            )
            .into());
        }
    }
    if matches!(
        injection_method,
        "direct_source_event" | "semantic_scenario_replay_then_headed_render"
    ) {
        return Err(format!(
            "{} used direct source-event injection in headed replay",
            report_path.display()
        )
        .into());
    }
    if report.get("input_route_contract").is_none() {
        return Err(format!(
            "{} missing headed input route contract",
            report_path.display()
        )
        .into());
    }
    let os_probe_passed = report
        .get("os_input_probe")
        .and_then(|probe| probe.get("status"))
        .and_then(JsonValue::as_str)
        == Some("pass");
    if injection_method.contains("os_") && !os_probe_passed {
        return Err(format!(
            "{} claims OS input but does not include a passing OS input probe",
            report_path.display()
        )
        .into());
    }
    if report
        .get("os_pointer_probe")
        .and_then(|probe| probe.get("status"))
        .and_then(JsonValue::as_str)
        == Some("fail")
    {
        return Err(format!(
            "{} includes an attempted but failed OS pointer probe",
            report_path.display()
        )
        .into());
    }
    if injection_method != "os_pointer_keyboard_to_visible_window" && !focus_free {
        let limitation = report
            .get("os_input_limitation")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if limitation.is_empty() {
            return Err(format!(
                "{} does not prove full per-step OS input and does not explain the limitation",
                report_path.display()
            )
            .into());
        }
    }
    let artifacts = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing headed artifacts", report_path.display()))?;
    if artifacts.is_empty() {
        return Err(format!(
            "{} has no headed screenshot artifacts",
            report_path.display()
        )
        .into());
    }
    let nonblank = report
        .get("nonblank_screenshot_hashes")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} missing nonblank screenshot proof",
                report_path.display()
            )
        })?;
    let proved_nonblank = nonblank.iter().any(|entry| {
        entry
            .get("nonzero_channels")
            .and_then(JsonValue::as_u64)
            .unwrap_or_default()
            > 0
            && entry
                .get("unique_rgba_values")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default()
                > 1
    });
    if !proved_nonblank {
        return Err(format!("{} headed screenshot is blank", report_path.display()).into());
    }
    if injection_method == "os_pointer_keyboard_to_visible_window" {
        verify_full_os_input_steps(report, report_path)?;
    }
    Ok(())
}

fn verify_os_input_probe_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    if report.get("command").and_then(JsonValue::as_str) != Some("os-input-probe") {
        return Err(format!("{} is not an os-input-probe report", report_path.display()).into());
    }
    if report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} standalone OS probe must use os_keyboard_to_visible_window",
            report_path.display()
        )
        .into());
    }
    if report
        .get("input_backend")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} missing input_backend", report_path.display()).into());
    }
    if report
        .get("focused_window_proof")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} missing focused_window_proof", report_path.display()).into());
    }
    let window_pid = report
        .get("window_pid")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if window_pid == 0 {
        return Err(format!("{} missing valid window_pid", report_path.display()).into());
    }
    let probe = report
        .get("os_input_probe")
        .ok_or_else(|| format!("{} missing os_input_probe", report_path.display()))?;
    if probe.get("status").and_then(JsonValue::as_str) != Some("pass")
        || probe.get("typed").and_then(JsonValue::as_bool) != Some(true)
    {
        return Err(format!("{} OS input probe did not pass", report_path.display()).into());
    }
    if probe.get("focused_ply_element").and_then(JsonValue::as_str) != Some("os_probe_input") {
        return Err(format!(
            "{} OS input probe did not target os_probe_input",
            report_path.display()
        )
        .into());
    }
    if probe
        .get("tool")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} OS input probe missing tool", report_path.display()).into());
    }
    let artifact = probe
        .get("artifact")
        .ok_or_else(|| format!("{} OS input probe missing artifact", report_path.display()))?;
    let artifact_path = artifact
        .get("path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} OS input probe artifact missing path",
                report_path.display()
            )
        })?;
    let expected_hash = artifact
        .get("sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} OS input probe artifact missing sha256",
                report_path.display()
            )
        })?;
    let actual_hash = sha256_file(Path::new(artifact_path))?;
    if actual_hash != expected_hash {
        return Err(format!(
            "{} OS input probe artifact hash mismatch for `{artifact_path}`",
            report_path.display()
        )
        .into());
    }
    let nonzero = artifact
        .get("nonzero_channels")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let unique = artifact
        .get("unique_rgba_values")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if nonzero == 0 || unique <= 1 {
        return Err(format!(
            "{} OS input probe screenshot is blank",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_full_os_input_steps(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let scenario_path = report
        .get("scenario_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing scenario_path",
                report_path.display()
            )
        })?;
    let scenario = parse_scenario(Path::new(scenario_path))?;
    let steps = report
        .get("os_input_steps")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing os_input_steps",
                report_path.display()
            )
        })?;
    if steps.len() != scenario.step.len() {
        return Err(format!(
            "{} full OS headed report has {} input steps, expected {}",
            report_path.display(),
            steps.len(),
            scenario.step.len()
        )
        .into());
    }
    let artifact_paths = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    verify_full_os_input_coverage(report, &scenario, report_path)?;
    for (expected, observed) in scenario.step.iter().zip(steps) {
        let id = observed
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if id != expected.id {
            return Err(format!(
                "{} full OS headed step id expected `{}`, got `{id}`",
                report_path.display(),
                expected.id
            )
            .into());
        }
        if observed.get("pass").and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!(
                "{} full OS headed step `{id}` did not pass",
                report_path.display()
            )
            .into());
        }
        if observed
            .get("target_element_id")
            .and_then(JsonValue::as_str)
            .is_none()
        {
            return Err(format!(
                "{} full OS headed step `{id}` missing target_element_id",
                report_path.display()
            )
            .into());
        }
        let bounds = observed.get("visible_bounds").ok_or_else(|| {
            format!(
                "{} full OS headed step `{id}` missing visible_bounds",
                report_path.display()
            )
        })?;
        let width = bounds
            .get("width")
            .and_then(JsonValue::as_f64)
            .unwrap_or_default();
        let height = bounds
            .get("height")
            .and_then(JsonValue::as_f64)
            .unwrap_or_default();
        if width <= 0.0 || height <= 0.0 {
            return Err(format!(
                "{} full OS headed step `{id}` has zero-sized visible bounds",
                report_path.display()
            )
            .into());
        }
        let screenshot = observed
            .get("screenshot_path")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} full OS headed step `{id}` missing screenshot_path",
                    report_path.display()
                )
            })?;
        if !artifact_paths.contains(screenshot) {
            return Err(format!(
                "{} full OS headed step `{id}` screenshot `{screenshot}` is missing from artifact hashes",
                report_path.display()
            )
            .into());
        }
        if let Some(expected_source) = &expected.expected_source_event {
            let expected_source = toml_string_ref(expected_source, "source")
                .ok_or_else(|| format!("{} expected source event missing source", expected.id))?;
            let observed_source = observed
                .get("source_event_observed")
                .and_then(|event| event.get("source"))
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} full OS headed step `{id}` missing observed source event",
                        report_path.display()
                    )
                })?;
            if observed_source != expected_source {
                return Err(format!(
                    "{} full OS headed step `{id}` observed source `{observed_source}`, expected `{expected_source}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_full_os_input_coverage(
    report: &JsonValue,
    scenario: &Scenario,
    report_path: &Path,
) -> RuntimeResult<()> {
    let coverage = report.get("os_input_coverage").ok_or_else(|| {
        format!(
            "{} full OS headed report missing os_input_coverage",
            report_path.display()
        )
    })?;
    for key in [
        "source_event_probe_missing_labels",
        "step_control_missing_labels",
        "missing_full_os_pointer_keyboard_steps",
    ] {
        let Some(items) = coverage.get(key).and_then(JsonValue::as_array) else {
            return Err(format!(
                "{} full OS headed report missing os_input_coverage.{key}",
                report_path.display()
            )
            .into());
        };
        if !items.is_empty() {
            return Err(format!(
                "{} full OS headed report has uncovered OS-input labels in {key}: {items:?}",
                report_path.display()
            )
            .into());
        }
    }

    let source_required = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .map(|step| step.id.as_str())
        .collect::<BTreeSet<_>>();
    let source_observations = report
        .get("visible_source_event_os_input")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing visible_source_event_os_input",
                report_path.display()
            )
        })?;
    let source_covered = source_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(JsonValue::as_bool) == Some(true)
                && observation
                    .get("runtime_mutation_observed")
                    .and_then(JsonValue::as_bool)
                    == Some(true)
                && observation
                    .get("source_event_observed")
                    .is_some_and(|event| !event.is_null())
        })
        .filter_map(|observation| {
            observation
                .get("scenario_step_id")
                .and_then(JsonValue::as_str)
        })
        .collect::<BTreeSet<_>>();
    let missing_source = source_required
        .difference(&source_covered)
        .copied()
        .collect::<Vec<_>>();
    if !missing_source.is_empty() {
        return Err(format!(
            "{} full OS headed report lacks visible SOURCE-event OS-input proof for labels: {missing_source:?}",
            report_path.display()
        )
        .into());
    }

    let step_required = scenario
        .step
        .iter()
        .skip(1)
        .map(|step| step.id.as_str())
        .collect::<BTreeSet<_>>();
    let step_observations = report
        .get("visible_step_control_os_input")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing visible_step_control_os_input",
                report_path.display()
            )
        })?;
    let step_covered = step_observations
        .iter()
        .filter(|observation| observation.get("pass").and_then(JsonValue::as_bool) == Some(true))
        .filter_map(|observation| observation.get("id").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let missing_steps = step_required
        .difference(&step_covered)
        .copied()
        .collect::<Vec<_>>();
    if !missing_steps.is_empty() {
        return Err(format!(
            "{} full OS headed report lacks visible Step-control OS-input proof for labels: {missing_steps:?}",
            report_path.display()
        )
        .into());
    }

    let app_control_passed = report
        .get("visible_app_control_os_input")
        .and_then(JsonValue::as_array)
        .is_some_and(|observations| {
            observations.iter().any(|observation| {
                observation.get("pass").and_then(JsonValue::as_bool) == Some(true)
            })
        });
    if !app_control_passed {
        return Err(format!(
            "{} full OS headed report missing a passing visible app-control OS-input probe",
            report_path.display()
        )
        .into());
    }

    Ok(())
}

fn verify_human_artifacts(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "command_argv",
        "exit_status",
        "manual_report_prepared_by",
        "manual_report_template_path",
        "manual_report_template_sha256",
        "binary_hash",
        "budget_hash",
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "display_scale",
        "window_pid",
        "window_pid_cmdline",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "input_injection_method",
        "checkpoint_screenshot_or_video_paths",
        "visual_checkpoint_pass_fail",
        "headed_report_path",
        "headed_report_sha256",
        "headed_input_injection_method",
        "headed_os_input_step_count",
        "headed_os_input_missing_labels",
        "manual_artifact_capture_method",
        "manual_started_at_utc",
        "manual_finished_at_utc",
        "manual_session_duration_seconds",
    ] {
        if report.get(key).is_none_or(JsonValue::is_null) {
            return Err(format!(
                "{} missing manual report metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    for key in [
        "binary_hash",
        "budget_hash",
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "display_scale",
        "window_pid_cmdline",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "input_injection_method",
        "manual_report_prepared_by",
        "manual_report_template_path",
        "manual_report_template_sha256",
        "manual_notes",
        "manual_artifact_capture_method",
        "headed_report_path",
        "headed_report_sha256",
        "headed_input_injection_method",
        "manual_started_at_utc",
        "manual_finished_at_utc",
        "manual_session_duration_seconds",
    ] {
        reject_manual_placeholder(report, report_path, key)?;
    }
    let headed_report_path = report
        .get("headed_report_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing headed report path", report_path.display()))?;
    let headed_report_hash = report
        .get("headed_report_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing headed report hash", report_path.display()))?;
    let actual_headed_hash = sha256_file(Path::new(headed_report_path))?;
    if actual_headed_hash != headed_report_hash {
        return Err(format!(
            "{} has stale headed report hash for `{headed_report_path}`",
            report_path.display()
        )
        .into());
    }
    verify_report_schema(Path::new(headed_report_path))?;
    let headed_report: JsonValue = serde_json::from_slice(&fs::read(headed_report_path)?)?;
    let current_commit = git_commit();
    let report_commit = json_str_field(report, "git_commit")?;
    if report_commit != current_commit {
        return Err(format!(
            "{} manual report git_commit `{report_commit}` does not match current git commit `{current_commit}`",
            report_path.display()
        )
        .into());
    }
    let headed_commit = json_str_field(&headed_report, "git_commit")?;
    if headed_commit != current_commit {
        return Err(format!(
            "{} linked headed report git_commit `{headed_commit}` does not match current git commit `{current_commit}`",
            report_path.display()
        )
        .into());
    }
    if headed_report.get("layer").and_then(JsonValue::as_str) != Some("headed-ply") {
        return Err(format!(
            "{} linked headed report `{headed_report_path}` is not headed-ply",
            report_path.display()
        )
        .into());
    }
    if headed_report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_pointer_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} linked headed report does not prove full OS pointer/keyboard input",
            report_path.display()
        )
        .into());
    }
    if report
        .get("headed_input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_pointer_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} manual report did not copy full headed OS input method",
            report_path.display()
        )
        .into());
    }
    let headed_missing = report
        .get("headed_os_input_missing_labels")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} missing headed OS input missing-label list",
                report_path.display()
            )
        })?;
    if !headed_missing.is_empty() {
        return Err(format!(
            "{} manual report links headed run with missing OS input labels",
            report_path.display()
        )
        .into());
    }
    let headed_steps = headed_report
        .get("os_input_steps")
        .and_then(JsonValue::as_array)
        .map(Vec::len)
        .unwrap_or_default() as u64;
    if json_u64_field(report, "headed_os_input_step_count")? != headed_steps {
        return Err(format!(
            "{} manual report headed step count does not match linked headed report",
            report_path.display()
        )
        .into());
    }
    for key in ["source_hash", "scenario_hash", "program_hash"] {
        if report.get(key) != headed_report.get(key) {
            return Err(format!(
                "{} manual `{key}` does not match linked headed report",
                report_path.display()
            )
            .into());
        }
    }
    let started = json_u64_field(report, "manual_started_at_utc")?;
    let finished = json_u64_field(report, "manual_finished_at_utc")?;
    let duration = json_u64_field(report, "manual_session_duration_seconds")?;
    let headed_generated = json_u64_field(&headed_report, "generated_at_utc")?;
    if headed_generated > started {
        return Err(format!(
            "{} linked headed report was generated after the recorded manual session started",
            report_path.display()
        )
        .into());
    }
    if started.saturating_sub(headed_generated) > 24 * 60 * 60 {
        return Err(format!(
            "{} linked headed report was not refreshed within 24h before the manual session",
            report_path.display()
        )
        .into());
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if started > now || finished > now {
        return Err(format!(
            "{} has future-dated manual session timing",
            report_path.display()
        )
        .into());
    }
    if finished < started || duration == 0 || finished.saturating_sub(started) != duration {
        return Err(format!(
            "{} has inconsistent manual session timing",
            report_path.display()
        )
        .into());
    }
    let generated = json_u64_field(report, "generated_at_utc")?;
    if generated < finished {
        return Err(format!(
            "{} was generated before the recorded manual session finished",
            report_path.display()
        )
        .into());
    }
    let prepared_by = report
        .get("manual_report_prepared_by")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} missing manual report preparation command",
                report_path.display()
            )
        })?;
    if !(prepared_by.starts_with("prepare-") && prepared_by.ends_with("-human-report")) {
        return Err(format!(
            "{} was not prepared by a repo human-report helper",
            report_path.display()
        )
        .into());
    }
    let command = report
        .get("command")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if command != prepared_by {
        return Err(format!(
            "{} command `{command}` does not match manual_report_prepared_by `{prepared_by}`",
            report_path.display()
        )
        .into());
    }
    let command_argv = report
        .get("command_argv")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing command_argv", report_path.display()))?;
    let command_args = command_argv
        .iter()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();
    for required_arg in [
        prepared_by,
        "--observer",
        "--started",
        "--finished",
        "--notes",
        "--capture-method",
        "--window-pid",
        "--focused-window-proof",
        "--display-server",
        "--display-connection",
        "--display-scale",
        "--window-backend",
        "--artifact",
        "--pass-label",
        "--report",
    ] {
        if !command_args.contains(required_arg) {
            return Err(format!(
                "{} command_argv does not prove helper argument `{required_arg}`",
                report_path.display()
            )
            .into());
        }
    }
    require_command_argv_u64(report_path, command_argv, "--started", started)?;
    require_command_argv_u64(report_path, command_argv, "--finished", finished)?;
    let template_path = report
        .get("manual_report_template_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual template path", report_path.display()))?;
    let template_hash = report
        .get("manual_report_template_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual template hash", report_path.display()))?;
    let actual_template_hash = sha256_file(Path::new(template_path))?;
    if actual_template_hash != template_hash {
        return Err(format!(
            "{} has stale manual template hash for `{template_path}`",
            report_path.display()
        )
        .into());
    }
    let observer = report
        .get("manual_observer")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual observer", report_path.display()))?;
    if matches!(observer, "" | "fixture" | "unknown")
        || observer.contains("fill")
        || observer.contains("copy-from")
        || observer.contains("replace-with")
        || observer.to_ascii_lowercase().contains("codex")
        || observer.to_ascii_lowercase().contains("automation")
        || observer.to_ascii_lowercase().contains("automated")
        || observer.to_ascii_lowercase().contains("script")
    {
        return Err(format!(
            "{} has non-human manual observer `{observer}`",
            report_path.display()
        )
        .into());
    }
    require_command_argv_value(report_path, command_argv, "--observer", observer)?;
    if report.get("manual_input_route").and_then(JsonValue::as_str) != Some("human_visible_window")
    {
        return Err(format!(
            "{} missing human visible-window input route",
            report_path.display()
        )
        .into());
    }
    if report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("human_visible_window")
    {
        return Err(format!(
            "{} manual report must mark input_injection_method as human_visible_window",
            report_path.display()
        )
        .into());
    }
    if json_u64_field(report, "window_pid")? == 0 {
        return Err(format!(
            "{} manual report has invalid visible-window pid",
            report_path.display()
        )
        .into());
    }
    let window_pid_cmdline = json_str_field(report, "window_pid_cmdline")?;
    if !window_pid_cmdline.contains("boon_ply_playground") {
        return Err(format!(
            "{} manual report window_pid_cmdline does not prove a Boon playground process",
            report_path.display()
        )
        .into());
    }
    require_command_argv_u64(
        report_path,
        command_argv,
        "--window-pid",
        json_u64_field(report, "window_pid")?,
    )?;
    if report
        .get("display_scale")
        .and_then(JsonValue::as_f64)
        .is_none_or(|scale| scale <= 0.0)
    {
        return Err(format!(
            "{} manual report has invalid display scale",
            report_path.display()
        )
        .into());
    }
    for key in [
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
    ] {
        if report
            .get(key)
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(format!(
                "{} has empty manual visible-window metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    require_command_argv_value(
        report_path,
        command_argv,
        "--focused-window-proof",
        json_str_field(report, "focused_window_proof")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--display-server",
        json_str_field(report, "display_server")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--display-connection",
        json_str_field(report, "display_socket_or_compositor_connection")?,
    )?;
    require_command_argv_f64(
        report_path,
        command_argv,
        "--display-scale",
        report
            .get("display_scale")
            .and_then(JsonValue::as_f64)
            .ok_or_else(|| format!("{} missing display_scale", report_path.display()))?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--window-backend",
        json_str_field(report, "window_backend")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--notes",
        json_str_field(report, "manual_notes")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--capture-method",
        json_str_field(report, "manual_artifact_capture_method")?,
    )?;
    let checklist = report
        .get("manual_checklist_pass_fail")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing manual checklist", report_path.display()))?;
    if checklist.is_empty() || checklist.contains_key("all_scripted_labels") {
        return Err(format!(
            "{} has fake or collapsed manual checklist",
            report_path.display()
        )
        .into());
    }
    for (label, passed) in checklist {
        if passed.as_bool() != Some(true) {
            return Err(format!(
                "{} manual checklist label `{label}` did not pass",
                report_path.display()
            )
            .into());
        }
    }
    let command_pass_labels = command_argv_values_after(command_argv, "--pass-label");
    let checklist_labels = checklist
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if command_pass_labels != checklist_labels {
        return Err(format!(
            "{} command_argv pass labels do not exactly match the manual checklist labels",
            report_path.display()
        )
        .into());
    }
    if let Some(scenario_path) = report.get("scenario_path").and_then(JsonValue::as_str) {
        let scenario = parse_scenario(Path::new(scenario_path))?;
        let expected_labels = scenario
            .step
            .iter()
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        let observed_labels = checklist
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if observed_labels != expected_labels {
            return Err(format!(
                "{} manual checklist labels do not exactly match scenario labels",
                report_path.display()
            )
            .into());
        }
    }
    let artifacts = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing manual artifacts", report_path.display()))?;
    if artifacts.is_empty() {
        return Err(format!(
            "{} has no manual screenshot/video artifacts",
            report_path.display()
        )
        .into());
    }
    let checkpoint_paths = report
        .get("checkpoint_screenshot_or_video_paths")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing manual checkpoint paths", report_path.display()))?;
    if checkpoint_paths.is_empty() {
        return Err(format!(
            "{} has no manual screenshot/video checkpoint paths",
            report_path.display()
        )
        .into());
    }
    let visual_checks = report
        .get("visual_checkpoint_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing visual checkpoint checks", report_path.display()))?;
    if visual_checks.is_empty() {
        return Err(format!(
            "{} has no visual checkpoint pass/fail entries",
            report_path.display()
        )
        .into());
    }
    let artifact_paths = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let visual_check_paths = visual_checks
        .iter()
        .map(|entry| {
            let path = entry
                .get("path")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} has visual checkpoint entry without a path",
                        report_path.display()
                    )
                })?;
            let passed = entry.get("pass").and_then(JsonValue::as_bool) == Some(true);
            if !passed {
                return Err(format!(
                    "{} visual checkpoint `{path}` did not pass",
                    report_path.display()
                )
                .into());
            }
            Ok(path)
        })
        .collect::<RuntimeResult<BTreeSet<_>>>()?;
    let headed_artifact_paths = headed_report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let mut manual_checkpoint_count = 0usize;
    for checkpoint in checkpoint_paths {
        let path = checkpoint
            .as_str()
            .ok_or_else(|| format!("{} has non-string checkpoint path", report_path.display()))?;
        if !artifact_paths.contains(path) {
            return Err(format!(
                "{} checkpoint `{path}` is missing from artifact hashes",
                report_path.display()
            )
            .into());
        }
        if !visual_check_paths.contains(path) {
            return Err(format!(
                "{} checkpoint `{path}` is missing from visual pass/fail checks",
                report_path.display()
            )
            .into());
        }
        let ext = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        if !matches!(ext, "png" | "webm" | "mp4") {
            return Err(format!(
                "{} checkpoint `{path}` is not a screenshot/video artifact",
                report_path.display()
            )
            .into());
        }
        if !headed_artifact_paths.contains(path) {
            manual_checkpoint_count += 1;
            verify_manual_checkpoint_content(report_path, path, ext)?;
            let file_name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !(file_name.contains("human") || file_name.contains("manual")) {
                return Err(format!(
                    "{} manual checkpoint `{path}` must be named with `human` or `manual`",
                    report_path.display()
                )
                .into());
            }
            let modified = fs::metadata(path)?
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_secs();
            if modified < started.saturating_sub(2) || modified > finished.saturating_add(300) {
                return Err(format!(
                    "{} manual checkpoint `{path}` was not captured during the recorded manual session window",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if manual_checkpoint_count == 0 {
        return Err(format!(
            "{} reuses only automated headed artifacts; add at least one manual screenshot/video checkpoint",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

pub fn command_argv_values_after<'a>(
    command_argv: &'a [JsonValue],
    flag: &str,
) -> BTreeSet<&'a str> {
    command_argv
        .windows(2)
        .filter_map(|window| {
            (window[0].as_str() == Some(flag))
                .then(|| window[1].as_str())
                .flatten()
        })
        .collect()
}

pub fn command_argv_value_after<'a>(command_argv: &'a [JsonValue], flag: &str) -> Option<&'a str> {
    command_argv.windows(2).find_map(|window| {
        (window[0].as_str() == Some(flag))
            .then(|| window[1].as_str())
            .flatten()
    })
}

pub fn require_command_argv_value(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: &str,
) -> RuntimeResult<()> {
    match command_argv_value_after(command_argv, flag) {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into()),
        None => Err(format!(
            "{} command_argv missing `{flag}` value",
            report_path.display()
        )
        .into()),
    }
}

pub fn require_command_argv_u64(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: u64,
) -> RuntimeResult<()> {
    let actual = command_argv_value_after(command_argv, flag)
        .ok_or_else(|| {
            format!(
                "{} command_argv missing `{flag}` value",
                report_path.display()
            )
        })?
        .parse::<u64>()?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into())
    }
}

pub fn require_command_argv_f64(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: f64,
) -> RuntimeResult<()> {
    let actual = command_argv_value_after(command_argv, flag)
        .ok_or_else(|| {
            format!(
                "{} command_argv missing `{flag}` value",
                report_path.display()
            )
        })?
        .parse::<f64>()?;
    if (actual - expected).abs() <= f64::EPSILON {
        Ok(())
    } else {
        Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into())
    }
}

fn verify_manual_checkpoint_content(
    report_path: &Path,
    checkpoint_path: &str,
    ext: &str,
) -> RuntimeResult<()> {
    let metadata = fs::metadata(checkpoint_path)?;
    if metadata.len() < 1024 {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` is too small to be a real screenshot/video",
            report_path.display()
        )
        .into());
    }
    let bytes = fs::read(checkpoint_path)?;
    match ext {
        "png" => verify_manual_png_checkpoint(report_path, checkpoint_path, &bytes)?,
        "mp4" if bytes.get(4..8) != Some(b"ftyp") => {
            return Err(format!(
                "{} manual checkpoint `{checkpoint_path}` is not a valid MP4 artifact",
                report_path.display()
            )
            .into());
        }
        "webm" if !bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) => {
            return Err(format!(
                "{} manual checkpoint `{checkpoint_path}` is not a valid WebM artifact",
                report_path.display()
            )
            .into());
        }
        _ => {}
    }
    Ok(())
}

fn verify_manual_png_checkpoint(
    report_path: &Path,
    checkpoint_path: &str,
    bytes: &[u8],
) -> RuntimeResult<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16) != Some(b"IHDR") {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` is not a valid PNG artifact",
            report_path.display()
        )
        .into());
    }
    let width = u32::from_be_bytes(
        bytes
            .get(16..20)
            .ok_or_else(|| format!("{checkpoint_path} PNG is missing width"))?
            .try_into()?,
    );
    let height = u32::from_be_bytes(
        bytes
            .get(20..24)
            .ok_or_else(|| format!("{checkpoint_path} PNG is missing height"))?
            .try_into()?,
    );
    if width < 32 || height < 32 {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` has implausible PNG dimensions {width}x{height}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn json_u64_field(report: &JsonValue, key: &str) -> RuntimeResult<u64> {
    if let Some(value) = report.get(key).and_then(JsonValue::as_u64) {
        return Ok(value);
    }
    let value = report
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing numeric manual metadata `{key}`"))?;
    Ok(value.parse::<u64>()?)
}

fn json_str_field<'a>(report: &'a JsonValue, key: &str) -> RuntimeResult<&'a str> {
    report
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing string manual metadata `{key}`").into())
}

fn reject_manual_placeholder(
    report: &JsonValue,
    report_path: &Path,
    key: &str,
) -> RuntimeResult<()> {
    let Some(value) = report.get(key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    if value.contains("fill") || value.contains("copy-from") || value.contains("replace-with") {
        return Err(format!(
            "{} has placeholder manual metadata `{key}` = `{value}`",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn report_layer_is(report: &JsonValue, expected: &str) -> bool {
    report.get("layer").and_then(JsonValue::as_str) == Some(expected)
        || report.get("command").and_then(JsonValue::as_str) == Some(expected)
}

fn report_command_is(report: &JsonValue, expected: &str) -> bool {
    report.get("command").and_then(JsonValue::as_str) == Some(expected)
}

fn report_is_runtime_execution_layer(report: &JsonValue) -> bool {
    matches!(
        report.get("layer").and_then(JsonValue::as_str),
        Some("semantic" | "ply-headless" | "headed-ply" | "speed")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_report_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "boon-report-schema-{name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn base_report() -> JsonValue {
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
            "command": "verify-report-schema-test",
            "command_argv": ["verify-report-schema-test"],
            "measurement_mode": "proof",
            "exit_status": 0,
            "git_commit": "test",
            "binary_hash": "test",
            "source_hash": "n/a",
            "scenario_hash": "n/a",
            "program_hash": "n/a",
            "budget_hash": "n/a",
            "graph_node_count": 0,
            "per_step_pass_fail": [{"id": "shape", "pass": true}],
            "artifact_sha256s": []
        })
    }

    fn interaction_report() -> JsonValue {
        let mut report = base_report();
        report["measurement_mode"] = json!("interaction");
        report["interaction_flow_id"] = json!("test-flow");
        report["stage_counters"] = json!({
            "runtime_turn": {
                "p50": 1.0,
                "p95": 2.0,
                "p99": 3.0,
                "max": 4.0,
                "sample_count": 2
            }
        });
        report["hot_path_png_write_count"] = json!(0);
        report["hot_path_report_write_count"] = json!(0);
        report["hot_path_report_serialization_count"] = json!(0);
        report["hot_path_heavy_json_summary_count"] = json!(0);
        report["hot_path_proof_readback_count"] = json!(0);
        report["hot_path_verbose_trace_event_count"] = json!(0);
        report["hot_path_dev_blocking_ipc_count"] = json!(0);
        report
    }

    fn schema_accepts(report: JsonValue, name: &str) -> bool {
        let path = temp_report_path(name);
        write_json(&path, &report).unwrap();
        let accepted = verify_report_schema(&path).is_ok();
        let _ = fs::remove_file(path);
        accepted
    }

    #[test]
    fn measurement_mode_is_required_and_enum_validated() {
        assert!(schema_accepts(base_report(), "valid-proof"));

        let mut missing = base_report();
        missing.as_object_mut().unwrap().remove("measurement_mode");
        assert!(!schema_accepts(missing, "missing-mode"));

        let mut invalid = base_report();
        invalid["measurement_mode"] = json!("fast");
        assert!(!schema_accepts(invalid, "invalid-mode"));
    }

    #[test]
    fn interaction_mode_rejects_hot_path_proof_and_diagnostic_work() {
        assert!(schema_accepts(interaction_report(), "clean-interaction"));

        let mut png = interaction_report();
        png["hot_path_png_write_count"] = json!(1);
        assert!(!schema_accepts(png, "interaction-png"));

        let mut readback = interaction_report();
        readback["proof_readback_in_hot_path"] = json!(true);
        assert!(!schema_accepts(readback, "interaction-readback"));

        let mut ipc = interaction_report();
        ipc["dev_blocking_ipc_count"] = json!(2);
        assert!(!schema_accepts(ipc, "interaction-ipc"));
    }

    #[test]
    fn interaction_mode_requires_flow_id_and_stage_counters() {
        let mut missing_flow = interaction_report();
        missing_flow
            .as_object_mut()
            .unwrap()
            .remove("interaction_flow_id");
        assert!(!schema_accepts(missing_flow, "interaction-missing-flow"));

        let mut missing_stages = interaction_report();
        missing_stages
            .as_object_mut()
            .unwrap()
            .remove("stage_counters");
        assert!(!schema_accepts(
            missing_stages,
            "interaction-missing-stages"
        ));

        let mut empty_stage = interaction_report();
        empty_stage["stage_counters"]["runtime_turn"]["sample_count"] = json!(0);
        assert!(!schema_accepts(empty_stage, "interaction-empty-stage"));
    }

    #[test]
    fn speed_and_benchmark_reports_have_distinct_measurement_modes() {
        let mut speed = base_report();
        speed["measurement_mode"] = json!("proof");
        speed["layer"] = json!("speed");
        assert!(!schema_accepts(speed, "speed-proof"));

        let mut bench = base_report();
        bench["measurement_mode"] = json!("interaction");
        bench["command"] = json!("bench-example");
        assert!(!schema_accepts(bench, "bench-interaction"));
    }

    #[test]
    fn compiled_artifact_report_links_real_artifact_and_sections() {
        let artifact_path = temp_report_path("compiled-artifact-file");
        write_json(
            &artifact_path,
            &json!({
                "artifact_kind": "boonc.compiled_program",
                "artifact_version": 1,
                "format": "boonc-json-v1"
            }),
        )
        .unwrap();
        let artifact_hash = sha256_file(&artifact_path).unwrap();
        let mut report = json!({
            "status": "pass",
            "report_version": 1,
            "command": "compile-artifact",
            "command_argv": ["boon_cli", "compile"],
            "measurement_mode": "diagnostic",
            "exit_status": 0,
            "generated_at_utc": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
            "git_commit": "test",
            "binary_hash": "test",
            "source_path": "examples/todomvc.bn",
            "source_hash": "source",
            "program_hash": "program",
            "graph_node_count": 1,
            "semantic_index": {},
            "compiled_schedule": {},
            "compiled_artifact": {
                "path": artifact_path.display().to_string(),
                "sha256": artifact_hash,
                "format": "boonc-json-v1",
                "artifact_version": 1,
                "program_hash": "program",
                "report_schema_hash": report_schema_hash(),
                "source_unit_count": 1
            },
            "artifact_sections": {
                "semantic_index": true,
                "symbol_table": true,
                "storage_layout": true,
                "source_schemas": true,
                "route_op_streams": true,
                "dependency_graph": true,
                "document_lowering_tables": true,
                "bridge_schemas": true,
                "compiled_schedule": true,
                "runtime_plan": true
            },
            "artifact_sha256s": [{
                "path": artifact_path.display().to_string(),
                "sha256": sha256_file(&artifact_path).unwrap()
            }]
        });
        assert!(schema_accepts(report.clone(), "compiled-artifact-valid"));
        report["artifact_sections"]["route_op_streams"] = json!(false);
        assert!(!schema_accepts(report, "compiled-artifact-missing-section"));
        let _ = fs::remove_file(artifact_path);
    }

    #[test]
    fn inspected_compiled_artifact_report_rejects_fake_runtime_load_claims() {
        let artifact_path = temp_report_path("inspected-compiled-artifact-file");
        write_json(
            &artifact_path,
            &json!({
                "artifact_kind": "boonc.compiled_program",
                "artifact_version": 1,
                "format": "boonc-json-v1"
            }),
        )
        .unwrap();
        let artifact_hash = sha256_file(&artifact_path).unwrap();
        let mut report = json!({
            "status": "pass",
            "report_version": 1,
            "command": "inspect-compiled-artifact",
            "command_argv": ["boon_cli", "inspect-artifact"],
            "measurement_mode": "diagnostic",
            "exit_status": 0,
            "generated_at_utc": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
            "git_commit": "test",
            "binary_hash": "test",
            "artifact_path": artifact_path.display().to_string(),
            "artifact_hash": artifact_hash,
            "program_hash": "program",
            "compiled_artifact": {
                "path": artifact_path.display().to_string(),
                "sha256": sha256_file(&artifact_path).unwrap(),
                "format": "boonc-json-v1",
                "artifact_version": 1,
                "program_hash": "program",
                "report_schema_hash": report_schema_hash(),
                "source_unit_count": 1
            },
            "artifact_sections": {
                "semantic_index": true,
                "symbol_table": true,
                "storage_layout": true,
                "source_schemas": true,
                "route_op_streams": true,
                "dependency_graph": true,
                "document_lowering_tables": true,
                "bridge_schemas": true,
                "compiled_schedule": true,
                "runtime_plan": true
            },
            "artifact_sha256s": [{
                "path": artifact_path.display().to_string(),
                "sha256": sha256_file(&artifact_path).unwrap()
            }],
            "inspection_result": {
                "artifact_valid": true,
                "loaded_runtime_from_artifact": true,
                "runtime_instantiated_from_artifact": true,
                "runtime_plan_present": true,
                "runtime_plan_generic_derived_deserialized_from_artifact": true,
                "runtime_plan_generic_derived_deserialized_counts": {
                    "function_count": 0,
                    "root_supported_count": 1,
                    "indexed_supported_count": 0,
                    "unsupported_reason_count": 0
                },
                "runtime_plan_storage_deserialized_from_artifact": true,
                "runtime_plan_storage_deserialized_counts": {
                    "root_slot_count": 1,
                    "root_initial_field_copy_count": 0,
                    "list_slot_count": 1,
                    "indexed_row_initial_reset_count": 0,
                    "initial_row_count": 0
                },
                "runtime_plan_document_lowering_deserialized_from_artifact": true,
                "runtime_plan_document_lowering_deserialized_counts": {
                    "root_summary_path_count": 1,
                    "list_summary_field_count": 0,
                    "dynamic_list_view_list_count": 0,
                    "projection_storage_resolution_count": 0,
                    "unresolved_projection_storage_path_count": 0,
                    "observed_root_path_count": 0,
                    "render_slot_count": 0,
                    "render_slot_failure_count": 0
                },
                "runtime_plan_non_route_tables_deserialized_from_artifact": true,
                "runtime_plan_non_route_tables_deserialized_counts": {
                    "runtime_symbol_count": 4,
                    "scalar_source_path_count": 1,
                    "scalar_branch_count": 1,
                    "derived_text_transform_count": 0,
                    "list_operation_count": 0,
                    "list_projection_count": 0,
                    "list_source_binding_count": 0
                },
                "source_free_runtime_load_available": true,
                "source_reparse_required_for_current_runtime": false,
                "source_reparse_attempted": false,
                "source_file_access": "not_attempted",
                "parser_ast_required_for_execution": false,
                "typed_ir_required_for_mvp_loader": false,
                "scenario_execution_available": false,
                "blocked_task": "none",
                "scenario_execution_pending_task": "TASK-0901C",
                "missing_runtime_plan_sections": []
            }
        });
        report["inspection_result"]["runtime_plan_source_routes_deserialized_from_artifact"] =
            json!(true);
        report["inspection_result"]["runtime_plan_source_routes_deserialized_counts"] = json!({
            "route_count": 1,
            "id_slot_count": 1,
            "label_slot_count": 1,
            "routes_with_ids": 1,
            "action_table_slot_count": 1,
            "action_op_stream_count": 1,
            "total_action_op_count": 1,
            "max_action_op_count": 1,
            "source_payload_schema_count": 1,
            "source_payload_field_count": 1,
            "source_payload_text_field_count": 1,
            "source_payload_key_field_count": 0,
            "source_payload_address_field_count": 0,
            "source_payload_pointer_field_count": 0
        });
        assert!(schema_accepts(
            report.clone(),
            "inspected-compiled-artifact-valid"
        ));
        report["inspection_result"]["scenario_execution_available"] = json!(true);
        assert!(!schema_accepts(
            report,
            "inspected-compiled-artifact-fake-scenario-execution"
        ));
        let _ = fs::remove_file(artifact_path);
    }

    #[test]
    fn compiled_artifact_scenario_report_requires_source_free_parity() {
        let artifact_path = temp_report_path("compiled-artifact-scenario-file");
        write_json(
            &artifact_path,
            &json!({
                "artifact_kind": "boonc.compiled_program",
                "artifact_version": 1,
                "format": "boonc-json-v1"
            }),
        )
        .unwrap();
        let artifact_hash = sha256_file(&artifact_path).unwrap();
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/counter.bn")
            .canonicalize()
            .unwrap();
        let scenario_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/counter.scn")
            .canonicalize()
            .unwrap();
        let source_hash = sha256_file(&source_path).unwrap();
        let scenario_hash = sha256_file(&scenario_path).unwrap();
        let signature_hash = sha256_bytes(b"matching-runtime-signature");
        let compiled_artifact = json!({
            "path": artifact_path.display().to_string(),
            "sha256": artifact_hash.clone(),
            "format": "boonc-json-v1",
            "artifact_version": 1,
            "program_hash": "program",
            "report_schema_hash": report_schema_hash(),
            "source_unit_count": 1
        });
        let artifact_sections = json!({
            "semantic_index": true,
            "symbol_table": true,
            "storage_layout": true,
            "source_schemas": true,
            "route_op_streams": true,
            "dependency_graph": true,
            "document_lowering_tables": true,
            "bridge_schemas": true,
            "compiled_schedule": true,
            "runtime_plan": true
        });
        let artifact_scenario = json!({
            "scenario_execution_available": true,
            "scenario_execution_from_artifact": true,
            "runtime_instantiated_from_artifact": true,
            "source_reparse_attempted": false,
            "source_file_access": "not_attempted",
            "typed_ir_required_for_artifact_execution": false,
            "parser_ast_required_for_artifact_execution": false,
            "source_oracle_layer": "semantic",
            "artifact_run_step_count": 7,
            "source_run_step_count": 7,
            "source_total_semantic_deltas": 7,
            "artifact_total_semantic_deltas": 7,
            "source_total_render_patches": 7,
            "artifact_total_render_patches": 7,
            "semantic_deltas_match": true,
            "render_patches_match": true,
            "state_summary_match": true,
            "parity_passed": true,
            "source_signature_hash": signature_hash,
            "artifact_signature_hash": signature_hash,
            "artifact_per_step": []
        });
        let report = json!({
            "status": "pass",
            "report_version": 1,
            "command": "verify-compiled-artifact-scenario",
            "command_argv": ["cargo", "xtask", "verify-compiled-artifact-scenario", "counter"],
            "measurement_mode": "proof",
            "exit_status": 0,
            "generated_at_utc": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
            "git_commit": "test",
            "binary_hash": "test",
            "source_path": source_path.display().to_string(),
            "source_hash": source_hash,
            "program_hash": "program",
            "scenario_path": scenario_path.display().to_string(),
            "scenario_hash": scenario_hash,
            "artifact_path": artifact_path.display().to_string(),
            "artifact_hash": artifact_hash.clone(),
            "compiled_artifact": compiled_artifact,
            "artifact_sections": artifact_sections,
            "artifact_sha256s": [{
                "path": artifact_path.display().to_string(),
                "sha256": artifact_hash
            }],
            "artifact_scenario": artifact_scenario
        });
        assert!(schema_accepts(
            report.clone(),
            "compiled-artifact-scenario-valid"
        ));

        let mut source_read = report.clone();
        source_read["artifact_scenario"]["source_file_access"] = json!("source_read");
        assert!(!schema_accepts(
            source_read,
            "compiled-artifact-scenario-source-read"
        ));

        let mut fake_parity = report.clone();
        fake_parity["artifact_scenario"]["parity_passed"] = json!(false);
        assert!(!schema_accepts(
            fake_parity,
            "compiled-artifact-scenario-fake-parity"
        ));

        let mut hash_mismatch = report.clone();
        hash_mismatch["artifact_scenario"]["artifact_signature_hash"] = json!("different");
        assert!(!schema_accepts(
            hash_mismatch,
            "compiled-artifact-scenario-hash-mismatch"
        ));

        let mut ast_required = report;
        ast_required["artifact_scenario"]["parser_ast_required_for_artifact_execution"] =
            json!(true);
        assert!(!schema_accepts(
            ast_required,
            "compiled-artifact-scenario-ast-required"
        ));
        let _ = fs::remove_file(artifact_path);
    }
}
