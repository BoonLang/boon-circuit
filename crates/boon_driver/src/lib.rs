use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, io};

pub const TIER_RUNTIME: &str = "runtime";
pub const TIER_BOON_DRIVER: &str = "boon-driver";
pub const TIER_REAL_WINDOW: &str = "real-window";
pub const TIER_HUMAN: &str = "human";
pub const TIER_HOST_SYNTHETIC: &str = "host-synthetic";

pub const METHOD_APP_OWNED_HOST_INPUT: &str = "boon-driver-app-owned-host-input";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIntent {
    pub sequence_id: u64,
    pub event_id: u64,
    pub source_path: String,
    pub source_id: Option<u64>,
    pub source_epoch: Option<u64>,
    pub payload: BTreeMap<String, String>,
    pub row_identity: Option<SourceRowIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRowIdentity {
    pub list: String,
    pub key: u64,
    pub generation: u64,
}

impl SourceIntent {
    pub fn new(sequence_id: u64, event_id: u64, source_path: impl Into<String>) -> Self {
        Self {
            sequence_id,
            event_id,
            source_path: source_path.into(),
            source_id: None,
            source_epoch: None,
            payload: BTreeMap::new(),
            row_identity: None,
        }
    }
}

pub fn source_intent_boundary_schema() -> Value {
    json!({
        "boundary": "BoonDriver SourceIntent -> playground SourceBatch -> boon_runtime dispatch",
        "driver_depends_on_boon_runtime": false,
        "required_fields": [
            "sequence_id",
            "event_id",
            "source_path",
            "payload"
        ],
        "runtime_resolved_fields": [
            "source_id",
            "source_epoch",
            "row_identity.key",
            "row_identity.generation"
        ],
        "private_runtime_mutation_allowed": false
    })
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DriverScenario {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub step: Vec<DriverScenarioStep>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DriverScenarioStep {
    pub id: String,
    #[serde(default)]
    pub user_action: Option<BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pub expected_source_event: Option<BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pub source_intent_exemption: Option<String>,
}

pub fn parse_scenario_str(source: &str) -> Result<DriverScenario, toml::de::Error> {
    toml::from_str(source)
}

pub fn parse_scenario_path(path: &Path) -> Result<DriverScenario, DriverScenarioParseError> {
    let source = fs::read_to_string(path).map_err(DriverScenarioParseError::Io)?;
    parse_scenario_str(&source).map_err(DriverScenarioParseError::Toml)
}

#[derive(Debug)]
pub enum DriverScenarioParseError {
    Io(io::Error),
    Toml(toml::de::Error),
}

impl std::fmt::Display for DriverScenarioParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Toml(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for DriverScenarioParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Toml(error) => Some(error),
        }
    }
}

pub fn scenario_engine_proof(
    scenario: &DriverScenario,
    runtime_report: &Value,
    native_preview_proof: Option<&Value>,
) -> Value {
    let runtime_steps = runtime_report
        .get("per_step_pass_fail")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let native_action_proofs = native_preview_proof
        .and_then(|proof| proof.get("action_proofs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let runtime_status_pass = runtime_report.get("status").and_then(Value::as_str) == Some("pass");
    let native_proof_pass = native_preview_proof
        .and_then(|proof| proof.get("status"))
        .and_then(Value::as_str)
        == Some("pass");
    let no_real_window_claim = native_preview_proof
        .and_then(|proof| proof.get("real_window_claimed"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        == false;
    let no_human_claim = native_preview_proof
        .and_then(|proof| proof.get("human_observation_claimed"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        == false;
    let runtime_step_by_id = runtime_steps
        .iter()
        .filter_map(|step| Some((step.get("id")?.as_str()?.to_owned(), step.clone())))
        .collect::<BTreeMap<_, _>>();
    let mut action_index = 0usize;
    let mut per_step = Vec::new();
    let mut blockers = Vec::new();
    let mut source_intent_recorded = true;
    let mut runtime_dispatch_recorded = true;
    let mut document_render_evidence_recorded = true;
    let mut selectors_resolved = true;
    let mut host_input_routed = true;
    for step in &scenario.step {
        let runtime_step = runtime_step_by_id.get(&step.id).cloned();
        let user_action_present = step.user_action.is_some();
        let expected_source = step_expected_source(step);
        let source_producing_action = user_action_present && expected_source.is_some();
        let exempt_action = user_action_present
            && expected_source.is_none()
            && step.source_intent_exemption.is_some();
        let expected_event_json = toml_map_to_json(step.expected_source_event.as_ref());
        let target_selector = target_selector_for_step(step);
        let native_action = if source_producing_action {
            let action = native_action_proofs.get(action_index).cloned();
            action_index += 1;
            action
        } else {
            None
        };
        let runtime_source = runtime_step
            .as_ref()
            .and_then(|step| step.pointer("/source_route_execution/source"))
            .and_then(Value::as_str);
        let native_source = native_action
            .as_ref()
            .and_then(|action| action.pointer("/runtime_event/source"))
            .and_then(Value::as_str);
        let runtime_pass = runtime_step
            .as_ref()
            .and_then(|step| step.get("pass"))
            .and_then(Value::as_bool)
            == Some(true);
        let runtime_input_route = runtime_step
            .as_ref()
            .and_then(|step| step.get("input_route_verified"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let native_status_pass = native_action
            .as_ref()
            .and_then(|action| action.get("status"))
            .and_then(Value::as_str)
            == Some("pass");
        let native_source_binding = native_action
            .as_ref()
            .and_then(|action| action.get("source_binding_resolved"))
            .and_then(Value::as_bool)
            == Some(true);
        let native_hit_test = native_action
            .as_ref()
            .and_then(|action| action.get("hit_test_performed"))
            .and_then(Value::as_bool)
            == Some(true);
        let source_matches = match expected_source {
            Some(expected) => runtime_source == Some(expected) && native_source == Some(expected),
            None => !source_producing_action,
        };
        let has_runtime_dispatch = !source_producing_action
            || runtime_step
                .as_ref()
                .and_then(|step| step.get("source_route_execution"))
                .is_some();
        let has_document_render_evidence = !source_producing_action
            || runtime_step
                .as_ref()
                .and_then(|step| step.get("render_patch_count"))
                .and_then(Value::as_u64)
                .is_some()
                && native_action
                    .as_ref()
                    .and_then(|action| action.get("framebuffer_delta_evidence"))
                    .is_some();
        let step_pass = runtime_pass
            && (!source_producing_action && (!user_action_present || exempt_action)
                || source_producing_action
                    && expected_source.is_some()
                    && runtime_input_route
                    && native_status_pass
                    && native_source_binding
                    && native_hit_test
                    && source_matches
                    && has_runtime_dispatch
                    && has_document_render_evidence);
        if user_action_present
            && expected_source.is_none()
            && step.source_intent_exemption.is_none()
        {
            source_intent_recorded = false;
        }
        if source_producing_action && !has_runtime_dispatch {
            runtime_dispatch_recorded = false;
        }
        if source_producing_action && !has_document_render_evidence {
            document_render_evidence_recorded = false;
        }
        if source_producing_action
            && !(native_status_pass && native_source_binding && native_hit_test)
        {
            selectors_resolved = false;
            host_input_routed = false;
        }
        if !step_pass {
            blockers.push(format!(
                "scenario step `{}` did not prove driver route",
                step.id
            ));
        }
        per_step.push(json!({
            "id": &step.id,
            "pass": step_pass,
            "user_action_present": user_action_present,
            "source_producing_action": source_producing_action,
            "source_intent_exempt_action": exempt_action,
            "target_selector": target_selector,
            "expected_source_event": expected_event_json,
            "source_intent_recorded": expected_source.is_some() || step.source_intent_exemption.is_some(),
            "source_intent_exemption": step.source_intent_exemption.clone(),
            "runtime_step_present": runtime_step.is_some(),
            "runtime_step_pass": runtime_pass,
            "runtime_input_route_verified": runtime_input_route,
            "runtime_source": runtime_source,
            "native_action_index": if source_producing_action { Some(action_index - 1) } else { None },
            "native_action_present": native_action.is_some(),
            "native_action_status": native_action
                .as_ref()
                .and_then(|action| action.get("status"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "native_source": native_source,
            "source_matches_scenario": source_matches,
            "host_input_routed": !source_producing_action || (native_status_pass && native_source_binding),
            "hit_focus_scroll_evidence": native_action
                .as_ref()
                .and_then(|action| action.get("hit_focus_scroll_evidence"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "runtime_dispatch_recorded": has_runtime_dispatch,
            "document_render_evidence_recorded": has_document_render_evidence,
            "runtime_dispatch": runtime_step
                .as_ref()
                .and_then(|step| step.get("source_route_execution"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "semantic_delta_count": runtime_step
                .as_ref()
                .and_then(|step| step.get("semantic_delta_count"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "render_patch_count": runtime_step
                .as_ref()
                .and_then(|step| step.get("render_patch_count"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "framebuffer_delta_evidence": native_action
                .as_ref()
                .and_then(|action| action.get("framebuffer_delta_evidence"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
        }));
    }
    let action_step_count = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .count();
    let action_counts_match =
        action_index == native_action_proofs.len() && action_index == action_step_count;
    if !action_counts_match {
        blockers.push(format!(
            "scenario action count {action_step_count} did not match native action proof count {}",
            native_action_proofs.len()
        ));
    }
    if !runtime_status_pass {
        blockers.push("fresh runtime scenario report did not pass".to_owned());
    }
    if !native_proof_pass {
        blockers.push("app-owned native preview proof did not pass".to_owned());
    }
    if !no_real_window_claim {
        blockers.push("BoonDriver scenario proof cannot claim real-window evidence".to_owned());
    }
    if !no_human_claim {
        blockers.push("BoonDriver scenario proof cannot claim human observation".to_owned());
    }
    let all_steps_pass = per_step
        .iter()
        .all(|step| step.get("pass").and_then(Value::as_bool) == Some(true));
    let pass = runtime_status_pass
        && native_proof_pass
        && no_real_window_claim
        && no_human_claim
        && action_counts_match
        && all_steps_pass
        && source_intent_recorded
        && runtime_dispatch_recorded
        && document_render_evidence_recorded
        && selectors_resolved
        && host_input_routed;
    json!({
        "status": if pass { "pass" } else { "fail" },
        "evidence_tier": TIER_BOON_DRIVER,
        "runtime_evidence_tier": TIER_RUNTIME,
        "method": METHOD_APP_OWNED_HOST_INPUT,
        "scenario_parser": {
            "name": &scenario.name,
            "source": &scenario.source,
            "step_count": scenario.step.len(),
            "action_step_count": action_step_count,
        },
        "parsed_scenario": true,
        "selectors_resolved": selectors_resolved,
        "waits_performed": runtime_steps.len() == scenario.step.len() && runtime_status_pass,
        "actions_dispatched": action_counts_match && action_step_count > 0,
        "host_input_routed": host_input_routed,
        "source_intent_recorded": source_intent_recorded,
        "runtime_dispatch_recorded": runtime_dispatch_recorded,
        "document_render_evidence_recorded": document_render_evidence_recorded,
        "assertions_performed": all_steps_pass,
        "driver_owns_per_step_route": true,
        "real_window_claimed": false,
        "human_observation_claimed": false,
        "cannot_claim_real_window_or_human": true,
        "runtime_status": runtime_report.get("status").cloned().unwrap_or_else(|| json!(null)),
        "native_preview_status": native_preview_proof
            .and_then(|proof| proof.get("status"))
            .cloned()
            .unwrap_or_else(|| json!(null)),
        "action_proofs": per_step,
        "native_preview_proof": native_preview_proof.cloned().unwrap_or_else(|| json!(null)),
        "blockers": if blockers.is_empty() { json!(null) } else { json!(blockers) },
    })
}

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
        TIER_BOON_DRIVER | TIER_HOST_SYNTHETIC => Some(1),
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
    let operator_host_input_evidence = report.get("operator_host_input_evidence");
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
        .unwrap_or_else(|| {
            operator_host_input_evidence
                .and_then(|evidence| evidence.get("status"))
                .and_then(Value::as_str)
                != Some("pass")
        });
    let operator_real_os_input = operator_ack
        .and_then(|ack| ack.get("real_os_input"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            report
                .get("real_os_input")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        });
    let action_proofs = if operator_outputs.is_empty() {
        host_routes
            .iter()
            .enumerate()
            .map(|(index, route)| {
                let source_binding_resolved = route
                    .get("source_intents")
                    .and_then(Value::as_array)
                    .is_some_and(|intents| !intents.is_empty());
                json!({
                    "index": index,
                    "status": if source_binding_resolved
                        && route.get("operator_host_input_observed").and_then(Value::as_bool) == Some(true)
                    {
                        "pass"
                    } else {
                        "fail"
                    },
                    "target_selector": route
                        .get("source_intents")
                        .and_then(Value::as_array)
                        .and_then(|intents| intents.first())
                        .and_then(|intent| intent.get("source_path"))
                        .cloned()
                        .unwrap_or_else(|| json!(null)),
                    "resolved_document_node": route.pointer("/target_hit_region/node").cloned()
                        .unwrap_or_else(|| json!(null)),
                    "hit_region": route.get("target_hit_region").cloned()
                        .unwrap_or_else(|| json!(null)),
                    "hit_focus_scroll_evidence": route_hit_focus_scroll_evidence(route),
                    "source_binding_resolved": source_binding_resolved,
                    "hit_test_performed": route.get("target_hit_region").is_some(),
                    "runtime_event": route.get("host_events").cloned().unwrap_or_else(|| json!([])),
                    "semantic_delta_count": 0,
                    "render_patch_count": if route.get("changes_visible_frame").and_then(Value::as_bool) == Some(true) { 1 } else { 0 },
                    "framebuffer_delta_evidence": route.get("visible_frame_change_method").cloned()
                        .unwrap_or_else(|| json!(null)),
                })
            })
            .collect::<Vec<_>>()
    } else {
        operator_outputs
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
                    "hit_focus_scroll_evidence": route_hit_focus_scroll_evidence(&route),
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
            .collect::<Vec<_>>()
    };
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
        "host_synthetic_evidence_tier": TIER_HOST_SYNTHETIC,
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
            "focus_route",
            "scroll_route",
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
                "target_hit_region": command_report
                    .pointer("/host_synthetic_activation/target_hit_region")
                    .cloned()
                    .unwrap_or_else(|| json!(null)),
                "typed_hit_side_table": command_report
                    .pointer("/host_synthetic_activation/typed_hit_side_table")
                    .cloned()
                    .unwrap_or_else(|| json!(null)),
                "hit_focus_scroll_evidence": route_hit_focus_scroll_evidence(
                    command_report
                        .get("host_synthetic_activation")
                        .unwrap_or(&Value::Null)
                ),
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
        "host_synthetic_evidence_tier": probe.get("evidence_tier").and_then(Value::as_str)
            .unwrap_or(TIER_HOST_SYNTHETIC),
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
        "host_synthetic_evidence_tier": report.get("evidence_tier").and_then(Value::as_str)
            .unwrap_or(TIER_HOST_SYNTHETIC),
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

fn route_hit_focus_scroll_evidence(route: &Value) -> Value {
    let hit_region = route
        .get("target_hit_region")
        .or_else(|| route.pointer("/host_synthetic_activation/target_hit_region"))
        .unwrap_or(&Value::Null);
    json!({
        "hit_region": hit_region,
        "hit_node": hit_region.get("node").cloned().unwrap_or_else(|| json!(null)),
        "source_binding_id": hit_region
            .get("source_binding_id")
            .cloned()
            .unwrap_or_else(|| json!(null)),
        "focus": {
            "node": route
                .get("focus_node")
                .or_else(|| route.get("focused_node"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "status": route
                .get("focus_status")
                .cloned()
                .unwrap_or_else(|| json!("not-required-for-pointer-route"))
        },
        "scroll": {
            "root": route
                .get("scroll_root")
                .or_else(|| hit_region.get("scroll_root"))
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "region": route
                .get("scroll_region")
                .cloned()
                .unwrap_or_else(|| json!(null))
        },
        "row_identity": {
            "key": hit_region
                .get("row_key")
                .cloned()
                .unwrap_or_else(|| json!(null)),
            "generation": hit_region
                .get("row_generation")
                .cloned()
                .unwrap_or_else(|| json!(null))
        }
    })
}

fn step_expected_source(step: &DriverScenarioStep) -> Option<&str> {
    step.expected_source_event
        .as_ref()
        .and_then(|event| event.get("source"))
        .and_then(toml_value_as_str)
}

fn toml_value_as_str(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn target_selector_for_step(step: &DriverScenarioStep) -> Value {
    let Some(action) = step.user_action.as_ref() else {
        return json!(null);
    };
    json!({
        "kind": action.get("kind").and_then(toml_value_as_str),
        "target": action.get("target").and_then(toml_value_as_str),
        "target_text": action.get("target_text").and_then(toml_value_as_str),
        "address": action.get("address").and_then(toml_value_as_str),
        "raw": toml_map_to_json(Some(action)),
    })
}

fn toml_map_to_json(map: Option<&BTreeMap<String, toml::Value>>) -> Value {
    let Some(map) = map else {
        return json!(null);
    };
    let mut object = serde_json::Map::new();
    for (key, value) in map {
        object.insert(key.clone(), toml_value_to_json(value));
    }
    Value::Object(object)
}

fn toml_value_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => json!(value),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) => json!(value),
        toml::Value::Boolean(value) => json!(value),
        toml::Value::Datetime(value) => json!(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_value_to_json).collect()),
        toml::Value::Table(table) => {
            let mut object = serde_json::Map::new();
            for (key, value) in table {
                object.insert(key.clone(), toml_value_to_json(value));
            }
            Value::Object(object)
        }
    }
}

#[cfg(test)]
mod tests;
