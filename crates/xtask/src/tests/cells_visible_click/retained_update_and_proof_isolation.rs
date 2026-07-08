// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn cells_visible_click_retained_update_contract_rejects_address_selection_fallback() {
    let sample = json!({
        "index": 0,
        "interaction_timing": {
            "layout_source": "visible_state_sync",
            "summary_source": "retained_current",
            "render_patch_count": 1,
            "coalesced_render_patch_count": 0,
            "document_patch_fast_path_rejected": false,
            "layout_patch_profile": {
                "render_scene_patch_applied": true
            }
        },
        "present_probe": {
            "last_external_render_proof": {
                "render_scene_patch_source": "input-overlay-sidecar",
                "retained_bound_sync": {
                    "status": "pass",
                    "selection_overlay_source": "generic-selected-node-set",
                    "address_selection_fallback_count": 0
                }
            }
        }
    });
    let pass_probe = json!({
        "click_samples": [sample.clone()]
    });
    let pass_summary = cells_visible_click_retained_update_contract_summary(&pass_probe);
    assert_eq!(
        pass_summary
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        pass_summary
            .get("address_selection_fallback_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    let mut address_fallback_sample = sample;
    address_fallback_sample["present_probe"]["last_external_render_proof"]["retained_bound_sync"]
        ["selection_overlay_source"] = json!("address-source-intent");
    address_fallback_sample["present_probe"]["last_external_render_proof"]["retained_bound_sync"]
        ["address_selection_fallback_count"] = json!(1);
    let fail_probe = json!({
        "click_samples": [address_fallback_sample]
    });
    let fail_summary = cells_visible_click_retained_update_contract_summary(&fail_probe);
    assert_eq!(
        fail_summary
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert_eq!(
        fail_summary
            .get("address_selection_fallback_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        fail_summary
            .pointer("/sample_failures/0/reason")
            .and_then(serde_json::Value::as_str),
        Some("address_selection_fallback_used")
    );
}


#[test]
fn cells_visible_click_contracts_accept_deferred_product_patch_visual_proof() {
    let probe = json!({
        "click_samples": [
            {
                "index": 0,
                "target_address": "A2",
                "product_formula_state_current": true,
                "native_input_timing": {
                    "fast_path": "simple_source_click_deferred_runtime",
                    "live_events_ms": 0.0,
                    "runtime_work": {
                        "runtime_invoked": false,
                        "source": "deferred_runtime_not_invoked",
                        "rows_scanned": 0,
                        "list_find_rows_scanned": 0,
                        "summary_fields_scanned": 0,
                        "root_materialization_candidates": 0,
                        "recomputed_fields": 0
                    }
                },
                "visual_formula_probe": {
                    "retained_bound_sync": {
                        "status": "pass",
                        "changed": true,
                        "target_node_count": 3,
                        "text_update_count": 1,
                        "style_update_count": 2,
                        "selection_overlay_source": "generic-selected-node-set",
                        "address_selection_fallback_count": 0
                    }
                }
            }
        ]
    });

    let retained_summary = cells_visible_click_retained_update_contract_summary(&probe);
    assert_eq!(
        retained_summary
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        retained_summary
            .get("deferred_retained_input_patch_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let runtime_summary = cells_visible_click_runtime_work_contract_summary(&probe);
    assert_eq!(
        runtime_summary
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        runtime_summary
            .pointer("/runtime_work_source_counts/deferred_runtime_not_invoked")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}


#[test]
fn cells_visible_click_requires_live_probe_post_present_proof_isolation() {
    let sidecar_only = json!({
        "post_present_proof_isolation": {
            "status": "pass",
            "product_path_status": "pass",
            "proof_worker_status": "lagging",
            "product_latency_includes_proof_completion": false,
            "product_blocks_on_proof_subscribers": false,
            "proof_latency_reported_separately": true
        }
    });
    let promoted = cells_visible_click_post_present_proof_isolation(&json!({}));

    assert!(promoted.is_null());
    assert_eq!(
        sidecar_only
            .pointer("/post_present_proof_isolation/proof_worker_status")
            .and_then(serde_json::Value::as_str),
        Some("lagging")
    );
}


#[test]
fn cells_visible_click_keeps_live_probe_post_present_proof_isolation() {
    let live_probe = json!({
        "post_present_proof_isolation": {
            "status": "pass",
            "product_path_status": "pass",
            "proof_worker_status": "settled",
            "product_latency_includes_proof_completion": false,
            "product_blocks_on_proof_subscribers": false,
            "proof_latency_reported_separately": true
        }
    });
    let promoted = cells_visible_click_post_present_proof_isolation(&live_probe);

    assert_eq!(
        promoted
            .get("proof_worker_status")
            .and_then(serde_json::Value::as_str),
        Some("settled")
    );
}

