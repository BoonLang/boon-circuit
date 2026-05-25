use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TIER_RUNTIME: &str = "runtime";
pub const TIER_BOON_DRIVER: &str = "boon-driver";
pub const TIER_REAL_WINDOW: &str = "real-window";
pub const TIER_HUMAN: &str = "human";
pub const LEGACY_TIER_HOST_SYNTHETIC: &str = "host-synthetic";

pub const METHOD_APP_OWNED_HOST_INPUT: &str = "boon-driver-app-owned-host-input";
pub const METHOD_LINUX_HUMAN_LIKE: &str = "linux-human-like-isolated-compositor";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriverEvidenceTier {
    Runtime,
    BoonDriver,
    RealWindow,
    Human,
}

impl DriverEvidenceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => TIER_RUNTIME,
            Self::BoonDriver => TIER_BOON_DRIVER,
            Self::RealWindow => TIER_REAL_WINDOW,
            Self::Human => TIER_HUMAN,
        }
    }
}

pub fn evidence_tier_rank(tier: &str) -> Option<u8> {
    match tier {
        TIER_RUNTIME => Some(0),
        TIER_BOON_DRIVER | LEGACY_TIER_HOST_SYNTHETIC => Some(1),
        TIER_REAL_WINDOW => Some(2),
        TIER_HUMAN => Some(3),
        _ => None,
    }
}

pub fn evidence_tier_satisfies(observed: &str, required: &str) -> bool {
    evidence_tier_rank(observed)
        .zip(evidence_tier_rank(required))
        .is_some_and(|(observed, required)| observed >= required)
}

pub fn app_owned_preview_proof(report: &Value) -> Value {
    let route_status = pointer_str(report, "/native_host_input_route_evidence/status");
    let runtime_assertions = report
        .get("runtime_state_assertions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let runtime_assertions_pass = !runtime_assertions.is_empty()
        && runtime_assertions
            .iter()
            .all(|assertion| assertion.get("pass").and_then(Value::as_bool) == Some(true));
    let mut readbacks = report
        .get("readback_artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if readbacks.is_empty() {
        for pointer in [
            "/preview_surface_proof/readback_artifact",
            "/dev_surface_proof/readback_artifact",
        ] {
            if let Some(readback) = report.pointer(pointer).cloned() {
                readbacks.push(readback);
            }
        }
    }
    let frame_hashes = report
        .get("frame_hashes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let host_routes = report
        .get("per_step_host_input_route")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let operator_ack = report.pointer("/dev_ipc_probe/operator_host_input");
    let operator_outputs = operator_ack
        .and_then(|ack| ack.get("outputs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let route_assertions = operator_ack
        .and_then(|ack| ack.get("host_route_assertions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let source_shortcut = operator_ack
        .and_then(|ack| ack.get("source_event_only_ipc_shortcut"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let operator_real_os_input = operator_ack
        .and_then(|ack| ack.get("real_os_input"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let action_proofs = operator_outputs
        .iter()
        .enumerate()
        .map(|(index, output)| {
            let route = route_assertions
                .get(index)
                .cloned()
                .unwrap_or_else(|| json!(null));
            json!({
                "index": index,
                "status": if route.get("pass").and_then(Value::as_bool) == Some(true) {
                    "pass"
                } else {
                    "fail"
                },
                "target_selector": route.pointer("/source_intent/source").cloned()
                    .or_else(|| route.get("requested_source").cloned())
                    .unwrap_or_else(|| json!(null)),
                "resolved_document_node": route.pointer("/target_hit_region/node").cloned()
                    .unwrap_or_else(|| json!(null)),
                "hit_region": route.get("target_hit_region").cloned()
                    .unwrap_or_else(|| json!(null)),
                "source_binding_resolved": route.get("source_binding_resolved").cloned()
                    .unwrap_or_else(|| json!(false)),
                "hit_test_performed": route.get("hit_test_performed").cloned()
                    .unwrap_or_else(|| json!(false)),
                "runtime_event": output.get("event").cloned().unwrap_or_else(|| json!(null)),
                "semantic_delta_count": output.get("semantic_delta_count").cloned()
                    .unwrap_or_else(|| json!(0)),
                "render_patch_count": output.get("render_patch_count").cloned()
                    .unwrap_or_else(|| json!(0)),
                "framebuffer_delta_evidence": output.get("framebuffer_delta_evidence").cloned()
                    .unwrap_or_else(|| json!(null)),
            })
        })
        .collect::<Vec<_>>();
    let action_proofs_pass = !action_proofs.is_empty()
        && action_proofs
            .iter()
            .all(|proof| proof.get("status").and_then(Value::as_str) == Some("pass"));
    let pass = route_status == Some("pass")
        && runtime_assertions_pass
        && !readbacks.is_empty()
        && !frame_hashes.is_empty()
        && !host_routes.is_empty()
        && action_proofs_pass
        && !source_shortcut
        && !operator_real_os_input;
    json!({
        "status": if pass { "pass" } else { "fail" },
        "evidence_tier": TIER_BOON_DRIVER,
        "legacy_evidence_tier": LEGACY_TIER_HOST_SYNTHETIC,
        "method": METHOD_APP_OWNED_HOST_INPUT,
        "real_window_claimed": false,
        "human_observation_claimed": false,
        "private_runtime_dispatch_used": report
            .get("private_runtime_dispatch_used")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "source_event_only_ipc_shortcut": source_shortcut,
        "operator_real_os_input": operator_real_os_input,
        "route_status": route_status.unwrap_or("missing"),
        "runtime_assertion_count": runtime_assertions.len(),
        "runtime_assertions_pass": runtime_assertions_pass,
        "readback_count": readbacks.len(),
        "frame_hash_count": frame_hashes.len(),
        "host_route_count": host_routes.len(),
        "action_proofs": action_proofs,
        "route_contract": "BoonDriver -> HostInputEvent -> document hit/scroll/focus routing -> SourceBinding -> RuntimeTurn -> render/readback",
        "required_layers": [
            "driver_action",
            "target_resolution",
            "hit_or_scroll_region",
            "source_binding",
            "runtime_dispatch",
            "document_or_render_patch",
            "app_owned_readback"
        ]
    })
}

pub fn app_owned_dev_window_proof(report: &Value) -> Value {
    let probe = report
        .get("dev_shell_interaction_probe")
        .unwrap_or(&Value::Null);
    let commands = ["tab_switch", "run", "format", "reset", "editor_text_input"];
    let command_proofs = commands
        .iter()
        .map(|command| {
            let command_report = probe.get(*command).unwrap_or(&Value::Null);
            json!({
                "command": command,
                "status": command_report.get("status").cloned().unwrap_or_else(|| json!("missing")),
                "source_binding_resolved": command_report
                    .pointer("/host_synthetic_activation/source_binding_resolved")
                    .cloned()
                    .unwrap_or_else(|| json!(null)),
                "hit_test_performed": command_report
                    .pointer("/host_synthetic_activation/hit_test_performed")
                    .cloned()
                    .unwrap_or_else(|| json!(null)),
                "direct_dispatch_without_hit_test": command_report
                    .get("direct_dispatch_without_hit_test")
                    .cloned()
                    .unwrap_or_else(|| json!(null)),
                "preview_transport": command_report.get("preview_transport").cloned()
                    .unwrap_or_else(|| json!(null)),
            })
        })
        .collect::<Vec<_>>();
    let commands_pass = command_proofs.iter().all(|proof| {
        proof.get("status").and_then(Value::as_str) == Some("pass")
            && proof
                .get("direct_dispatch_without_hit_test")
                .and_then(Value::as_bool)
                != Some(true)
    });
    let inventory = probe
        .get("selected_example_structural_inventory")
        .unwrap_or(&Value::Null);
    let structural_pass = inventory.get("status").and_then(Value::as_str) == Some("pass")
        && inventory
            .get("scroll_root_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        && inventory
            .get("materialized_node_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0;
    let pass = probe.get("status").and_then(Value::as_str) == Some("pass")
        && commands_pass
        && structural_pass
        && probe
            .get("internal_command_shortcut")
            .and_then(Value::as_bool)
            == Some(false)
        && probe.get("parser_bypassed").and_then(Value::as_bool) == Some(false)
        && probe
            .get("example_specific_shortcut")
            .and_then(Value::as_bool)
            == Some(false);
    json!({
        "status": if pass { "pass" } else { "fail" },
        "evidence_tier": TIER_BOON_DRIVER,
        "legacy_evidence_tier": probe.get("evidence_tier").and_then(Value::as_str)
            .unwrap_or(LEGACY_TIER_HOST_SYNTHETIC),
        "method": METHOD_APP_OWNED_HOST_INPUT,
        "real_window_claimed": false,
        "command_proofs": command_proofs,
        "commands_pass": commands_pass,
        "structural_inventory_pass": structural_pass,
        "editor_scroll_root_count": inventory
            .get("scroll_root_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "editor_materialized_node_count": inventory
            .get("materialized_node_count")
            .cloned()
            .unwrap_or_else(|| json!(0)),
        "boundary": probe.get("boundary").cloned().unwrap_or_else(|| json!(null)),
        "command_activation_boundary": probe.get("command_activation_boundary")
            .cloned()
            .unwrap_or_else(|| json!(null)),
    })
}

pub fn app_owned_speed_proof(report: &Value) -> Value {
    let budget_pass = report.get("budget_pass").and_then(Value::as_bool) == Some(true);
    let wheel_p95 = report
        .get("wheel_to_visible_ms_p95_per_axis")
        .cloned()
        .unwrap_or_else(|| json!(null));
    let mut readbacks = report
        .get("readback_artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if readbacks.is_empty() {
        for pointer in [
            "/preview_surface_proof/readback_artifact",
            "/dev_surface_proof/readback_artifact",
        ] {
            if let Some(readback) = report.pointer(pointer).cloned() {
                readbacks.push(readback);
            }
        }
    }
    let operator_wheel = report
        .get("operator_host_wheel_input")
        .and_then(Value::as_bool)
        == Some(true);
    let pass = budget_pass && operator_wheel && !readbacks.is_empty();
    json!({
        "status": if pass { "pass" } else { "fail" },
        "evidence_tier": TIER_BOON_DRIVER,
        "legacy_evidence_tier": report.get("evidence_tier").and_then(Value::as_str)
            .unwrap_or(LEGACY_TIER_HOST_SYNTHETIC),
        "method": METHOD_APP_OWNED_HOST_INPUT,
        "real_window_claimed": false,
        "budget_pass": budget_pass,
        "operator_host_wheel_input": operator_wheel,
        "wheel_to_visible_ms_p95_per_axis": wheel_p95,
        "readback_count": readbacks.len(),
        "tested_rows": report.get("logical_rows").cloned().unwrap_or_else(|| json!(null)),
        "tested_columns": report.get("logical_columns").cloned().unwrap_or_else(|| json!(null)),
        "source_line_count": report.get("source_line_count").cloned().unwrap_or_else(|| json!(null)),
    })
}

fn pointer_str<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boon_driver_tier_does_not_satisfy_real_window() {
        assert!(evidence_tier_satisfies(
            TIER_BOON_DRIVER,
            LEGACY_TIER_HOST_SYNTHETIC
        ));
        assert!(evidence_tier_satisfies(TIER_REAL_WINDOW, TIER_BOON_DRIVER));
        assert!(!evidence_tier_satisfies(TIER_BOON_DRIVER, TIER_REAL_WINDOW));
    }

    #[test]
    fn preview_proof_requires_route_runtime_readback_and_no_source_shortcut() {
        let report = json!({
            "native_host_input_route_evidence": {"status": "pass"},
            "runtime_state_assertions": [{"pass": true}],
            "readback_artifacts": [{"path": "frame.png", "sha256": "abc"}],
            "frame_hashes": [{"sha256": "abc"}],
            "per_step_host_input_route": [{"status": "pass"}],
            "real_os_input": false,
            "dev_ipc_probe": {
                "operator_host_input": {
                    "real_os_input": false,
                    "source_event_only_ipc_shortcut": false,
                    "outputs": [{
                        "event": {"source": "store.sources.submit"},
                        "semantic_delta_count": 1,
                        "render_patch_count": 1
                    }],
                    "host_route_assertions": [{
                        "pass": true,
                        "source_binding_resolved": true,
                        "hit_test_performed": true,
                        "target_hit_region": {"node": "submit_button"}
                    }]
                }
            }
        });
        let proof = app_owned_preview_proof(&report);
        assert_eq!(proof.get("status").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            proof.get("real_window_claimed").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn speed_proof_accepts_surface_readback_artifact_fallback() {
        let report = json!({
            "budget_pass": true,
            "operator_host_wheel_input": true,
            "wheel_to_visible_ms_p95_per_axis": {"vertical": 4.0, "horizontal": 4.0},
            "preview_surface_proof": {
                "readback_artifact": {
                    "path": "frame.png",
                    "sha256": "abc",
                    "capture_method": "wgpu-visible-surface-copy-src-readback"
                }
            }
        });
        let proof = app_owned_speed_proof(&report);
        assert_eq!(proof.get("status").and_then(Value::as_str), Some("pass"));
        assert_eq!(proof.get("readback_count").and_then(Value::as_u64), Some(1));
    }
}
