use super::*;

#[test]
fn boon_driver_tier_does_not_satisfy_real_window() {
    assert!(evidence_tier_satisfies(
        TIER_BOON_DRIVER,
        TIER_HOST_SYNTHETIC
    ));
    assert!(evidence_tier_satisfies(TIER_REAL_WINDOW, TIER_BOON_DRIVER));
    assert!(!evidence_tier_satisfies(TIER_BOON_DRIVER, TIER_REAL_WINDOW));
}

#[test]
fn source_intent_schema_is_host_neutral_and_batch_ready() {
    let mut intent = SourceIntent::new(7, 11, "store.sources.submit.press");
    intent.source_id = Some(3);
    intent.source_epoch = Some(99);
    intent.payload.insert("key".to_owned(), "Enter".to_owned());
    intent.row_identity = Some(SourceRowIdentity {
        list: "todos".to_owned(),
        key: 42,
        generation: 2,
    });
    let value = serde_json::to_value(&intent).unwrap();
    assert_eq!(value["sequence_id"], 7);
    assert_eq!(value["event_id"], 11);
    assert_eq!(value["source_id"], 3);
    assert_eq!(value["source_epoch"], 99);
    assert_eq!(value["row_identity"]["generation"], 2);

    let schema = source_intent_boundary_schema();
    assert_eq!(
        schema["boundary"],
        "BoonDriver SourceIntent -> playground SourceBatch -> boon_runtime dispatch"
    );
    assert_eq!(schema["driver_depends_on_boon_runtime"], false);
    assert_eq!(schema["private_runtime_mutation_allowed"], false);
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
                    "target_hit_region": {
                        "node": "submit_button",
                        "source_binding_id": "source:submit_button:press",
                        "scroll_root": "main-scroll",
                        "row_key": 42,
                        "row_generation": 3
                    }
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
    assert_eq!(
        proof["action_proofs"][0]["hit_focus_scroll_evidence"]["scroll"]["root"],
        "main-scroll"
    );
    assert_eq!(
        proof["action_proofs"][0]["hit_focus_scroll_evidence"]["row_identity"]["generation"],
        3
    );
}

#[test]
fn scenario_engine_proof_binds_parsed_steps_to_runtime_and_native_actions() {
    let scenario = parse_scenario_str(
        r#"
name = "driver-smoke"
source = "examples/driver-smoke.bn"

[[step]]
id = "initial"

[[step]]
id = "press-submit"
user_action = { kind = "click", target = "submit", target_text = "Submit" }
expected_source_event = { source = "store.sources.submit.press", target_text = "Submit" }
"#,
    )
    .unwrap();
    let runtime_report = json!({
        "status": "pass",
        "per_step_pass_fail": [
            {"id": "initial", "pass": true, "input_route_verified": false, "semantic_delta_count": 0, "render_patch_count": 0},
            {
                "id": "press-submit",
                "pass": true,
                "input_route_verified": true,
                "semantic_delta_count": 1,
                "render_patch_count": 1,
                "source_route_execution": {
                    "source": "store.sources.submit.press",
                    "source_id": 7,
                    "route_id": 3
                }
            }
        ]
    });
    let native_proof = json!({
        "status": "pass",
        "real_window_claimed": false,
        "human_observation_claimed": false,
        "action_proofs": [{
            "status": "pass",
            "source_binding_resolved": true,
            "hit_test_performed": true,
            "runtime_event": {"source": "store.sources.submit.press"},
            "hit_focus_scroll_evidence": {
                "hit_node": "submit_button",
                "scroll": {"root": "main"},
                "focus": {"status": "not-required-for-pointer-route"}
            },
            "framebuffer_delta_evidence": {"method": "render-patch-backed-framebuffer-change-required"}
        }]
    });
    let proof = scenario_engine_proof(&scenario, &runtime_report, Some(&native_proof));
    assert_eq!(proof.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        proof
            .pointer("/scenario_parser/action_step_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        proof
            .pointer("/action_proofs/1/target_selector/target_text")
            .and_then(Value::as_str),
        Some("Submit")
    );
    assert_eq!(
        proof
            .pointer("/action_proofs/1/runtime_dispatch/source")
            .and_then(Value::as_str),
        Some("store.sources.submit.press")
    );
}

#[test]
fn scenario_engine_proof_rejects_missing_native_route_and_tier_inflation() {
    let scenario = parse_scenario_str(
        r#"
name = "driver-smoke"
source = "examples/driver-smoke.bn"

[[step]]
id = "press-submit"
user_action = { kind = "click", target = "submit" }
expected_source_event = { source = "store.sources.submit.press" }
"#,
    )
    .unwrap();
    let runtime_report = json!({
        "status": "pass",
        "per_step_pass_fail": [{
            "id": "press-submit",
            "pass": true,
            "input_route_verified": true,
            "semantic_delta_count": 1,
            "render_patch_count": 1,
            "source_route_execution": {"source": "store.sources.submit.press"}
        }]
    });
    let inflated_native_proof = json!({
        "status": "pass",
        "real_window_claimed": true,
        "human_observation_claimed": false,
        "action_proofs": []
    });
    let proof = scenario_engine_proof(&scenario, &runtime_report, Some(&inflated_native_proof));
    assert_eq!(proof.get("status").and_then(Value::as_str), Some("fail"));
    assert_eq!(
        proof
            .get("cannot_claim_real_window_or_human")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        proof
            .get("blockers")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .any(|blocker| blocker
                .as_str()
                .is_some_and(|text| text.contains("real-window")))
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
