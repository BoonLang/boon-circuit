#[test]
fn cells_visual_formula_probe_requires_expected_formula_bar_text_value() {
    let artifact_dir = std::env::temp_dir().join(format!(
        "boon-cells-formula-probe-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&artifact_dir).unwrap();
    let before_path = artifact_dir.join("before.png");
    let after_path = artifact_dir.join("after.png");
    let stale_after_path = artifact_dir.join("stale-after.png");
    let layout_path = artifact_dir.join("layout.json");
    let mut before = image::RgbaImage::from_pixel(480, 260, image::Rgba([255, 255, 255, 255]));
    for y in 76..100 {
        for x in 49..129 {
            before.put_pixel(x, y, image::Rgba([217, 232, 255, 255]));
        }
        for x in 130..210 {
            before.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    let mut after = before.clone();
    for y in 8..38 {
        for x in 8..48 {
            after.put_pixel(x, y, image::Rgba([224, 235, 246, 255]));
        }
        for x in 80..472 {
            after.put_pixel(x, y, image::Rgba([220, 230, 240, 255]));
        }
    }
    for y in 76..100 {
        for x in 49..129 {
            after.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
        for x in 130..210 {
            after.put_pixel(x, y, image::Rgba([217, 232, 255, 255]));
        }
    }
    before.save(&before_path).unwrap();
    after.save(&after_path).unwrap();
    before.save(&stale_after_path).unwrap();
    std::fs::write(
        &layout_path,
        serde_json::to_string(&json!({
            "source_intents": [
                {"node": "formula-address", "intent": "address", "source_path": "A0"},
                {"node": "cell-a0", "intent": "address", "source_path": "A0"},
                {"node": "cell-b0", "intent": "address", "source_path": "B0"}
            ],
            "layout_frame": {
                "display_list": [
                    {
                        "node": "formula-address",
                        "kind": "text",
                        "text": "A0",
                        "bounds": {"x": 8.0, "y": 8.0, "width": 40.0, "height": 30.0},
                        "style": {}
                    },
                    {
                        "node": "formula-input",
                        "kind": "text_input",
                        "text": "5",
                        "bounds": {"x": 80.0, "y": 8.0, "width": 392.0, "height": 30.0},
                        "style": {}
                    },
                    {
                        "node": "cell-a0",
                        "kind": "button",
                        "text": "5",
                        "bounds": {"x": 49.0, "y": 76.0, "width": 80.0, "height": 24.0},
                        "style": {"selected": true}
                    },
                    {
                        "node": "cell-b0",
                        "kind": "button",
                        "text": "15",
                        "bounds": {"x": 130.0, "y": 76.0, "width": 80.0, "height": 24.0},
                        "style": {"selected": false}
                    }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let after_path_text = after_path.to_string_lossy().to_string();
    let before_path_text = before_path.to_string_lossy().to_string();
    let stale_after_path_text = stale_after_path.to_string_lossy().to_string();
    let layout_path_text = layout_path.to_string_lossy().to_string();
    let probe = json!({
        "status": "pass",
        "accepted_by_hash_change": true,
        "accepted_by_retained_bound_text_sync": true,
        "readback_artifact_after": {
            "path": after_path_text
        },
        "last_external_render_proof": {
            "status": "pass",
            "layout_artifact": layout_path_text,
            "retained_bound_sync": {
                "status": "pass",
                "changed": true,
                "text_update_count": 1,
                "text_update_binding_paths": [
                    {
                        "node": "formula-input",
                        "paths": ["store.selected_input.editing_text"]
                    }
                ],
                "text_update_values": [
                    {
                        "node": "formula-input",
                        "text": "=add(A0,A1)",
                        "paths": ["store.selected_input.editing_text"]
                    }
                ]
            }
        }
    });

    assert_eq!(
        cells_visual_formula_probe_from_readback(
            &probe,
            Some(&before_path_text),
            "A0",
            "B0",
            "5",
            "=add(A0,A1)",
        )
        .get("status")
        .and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        cells_visual_formula_probe_from_readback(
            &probe,
            Some(&before_path_text),
            "A0",
            "C0",
            "5",
            "=sum(A0:A2)",
        )
        .get("status")
        .and_then(serde_json::Value::as_str),
        Some("fail"),
        "the Cells visible-click proof must not pass when the top formula input patched to a stale value"
    );
    let mut stale_visual_probe = probe.clone();
    stale_visual_probe["readback_artifact_after"] = json!({
        "path": stale_after_path_text
    });
    assert_eq!(
        cells_visual_formula_probe_from_readback(
            &stale_visual_probe,
            Some(&before_path_text),
            "A0",
            "B0",
            "5",
            "=add(A0,A1)",
        )
        .get("status")
        .and_then(serde_json::Value::as_str),
        Some("fail"),
        "full-frame/readback metadata must not pass when the top formula-bar crop did not change"
    );

    let metadata_only_probe = json!({
        "status": "pass",
        "accepted_by_hash_change": false,
        "accepted_by_retained_bound_text_sync": true,
        "readback_artifact_after": {
            "path": after_path_text
        },
        "last_external_render_proof": probe["last_external_render_proof"].clone()
    });
    let metadata_only_result = cells_visual_formula_probe_from_readback(
        &metadata_only_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        metadata_only_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail"),
        "retained text-input sync metadata is diagnostic only; Cells visible-click proof needs app-owned visual evidence"
    );
    assert_eq!(
        metadata_only_result
            .get("retained_text_sync_matches")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let frame_key = json!({
        "frame_seq": 9,
        "content_revision": 7,
        "layout_revision": 3,
        "render_scene_revision": 5,
        "surface_id": "preview:test-surface",
        "surface_epoch": 1,
        "input_event_seq": 4,
        "present_id": 9,
        "proof_request_id": null
    });
    let structured_probe = json!({
        "status": "pass",
        "accepted_by_hash_change": false,
        "accepted_by_retained_bound_text_sync": true,
        "accepted_by_structured_external_render_proof": true,
        "structured_external_render_proof_is_current": true,
        "frame_evidence_key": frame_key.clone(),
        "last_external_render_proof": {
            "status": "pass",
            "layout_artifact": layout_path_text,
            "visible_surface_rendered": true,
            "visible_present_path": true,
            "render_target_kind": "visible-surface-direct",
            "input_overlay_focus_state": {
                "previous_selected_address": "A0",
                "selected_address": "B0"
            },
            "input_overlay_focused_node_probe": {
                "status": "pass",
                "focused": true,
                "style_selected": true,
                "style_focused": true
            },
            "proof": {
                "status": "pass",
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "replacement_proof": "render-loop visible surface readback artifact",
                "frame_evidence_key": frame_key.clone(),
                "metrics": {
                    "preview_blocked_on_ipc_count": 0
                }
            },
            "retained_bound_sync": {
                "status": "pass",
                "changed": true,
                "text_update_count": 2,
                "text_update_values": [
                    {
                        "node": "formula-input",
                        "text": "=add(A0,A1)",
                        "paths": ["store.selected_input.editing_text"]
                    },
                    {
                        "node": "formula-address",
                        "text": "B0",
                        "paths": ["store.selected_address"]
                    }
                ]
            }
        }
    });
    let structured_result = cells_visual_formula_probe_from_readback(
        &structured_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        structured_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "same-frame structured visible-surface WGPU proof should replace duplicate interaction readback"
    );
    assert_eq!(
        structured_result
            .pointer("/structured_external_visible_surface_probe/frame_evidence_matches")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let mut visible_bound_text_probe = structured_probe.clone();
    visible_bound_text_probe["last_external_render_proof"]["retained_bound_sync"] = json!({
        "status": "pass",
        "changed": false,
        "text_update_count": 0,
        "text_update_values": []
    });
    visible_bound_text_probe["last_external_render_proof"]["visible_bound_text"] = json!({
        "status": "pass",
        "source": "layout-frame-current-bound-text",
        "entry_count": 2,
        "entry_limit": 512,
        "truncated": false,
        "entries": [
            {
                "node": "formula-input",
                "text": "=add(A0,A1)",
                "text_truncated": false,
                "paths": ["store.selected_input.editing_text"]
            },
            {
                "node": "formula-address",
                "text": "B0",
                "text_truncated": false,
                "paths": ["store.selected_input.address"]
            }
        ]
    });
    let visible_bound_text_result = cells_visual_formula_probe_from_readback(
        &visible_bound_text_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        visible_bound_text_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "same-frame visible bound text should prove current formula/address even when no retained text delta was recorded"
    );
    assert_eq!(
        visible_bound_text_result
            .pointer("/structured_external_visible_surface_probe/visible_formula_text_current")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let mut selected_visible_node_probe = visible_bound_text_probe.clone();
    selected_visible_node_probe["last_external_render_proof"]["input_overlay_focus_state"] = json!({
        "previous_selected_address": "B0",
        "selected_address": null,
        "selected_node_count": 1,
        "selected_node_samples": ["cell-b0"],
        "selection_proxy": false
    });
    selected_visible_node_probe["last_external_render_proof"]["input_overlay_focused_node_probe"] = json!({
        "status": "missing_focused_node"
    });
    selected_visible_node_probe["last_external_render_proof"]["visible_bound_text"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "node": "cell-b0",
            "text": "15",
            "text_truncated": false,
            "paths": ["@row:2:1:display_text"],
            "source_intents": [
                {
                    "node": "cell-b0",
                    "intent": "row_lookup",
                    "lookup_field": "address",
                    "lookup_value": "B0"
                }
            ],
            "selected": true,
            "focused": false,
            "kind": "Button"
        }));
    let selected_visible_node_result = cells_visual_formula_probe_from_readback(
        &selected_visible_node_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        selected_visible_node_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "same-frame visible selected-node proof should prove selection when focus overlay metadata is absent"
    );
    assert_eq!(
        selected_visible_node_result
            .pointer(
                "/structured_external_visible_surface_probe/visible_selected_node_matches_address"
            )
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        selected_visible_node_result
            .pointer("/structured_external_visible_surface_probe/focused_node_probe_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    let mut selected_entry_source_intent_probe = selected_visible_node_probe.clone();
    selected_entry_source_intent_probe["last_external_render_proof"]["layout_artifact"] =
        serde_json::Value::Null;
    let selected_entry_source_intent_result = cells_visual_formula_probe_from_readback(
        &selected_entry_source_intent_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        selected_entry_source_intent_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "post-present visible-bound-text source intents should prove the selected node when a patched layout artifact file is unavailable"
    );
    assert_eq!(
        selected_entry_source_intent_result
            .pointer(
                "/structured_external_visible_surface_probe/visible_selected_node_matches_address"
            )
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let mut selected_node_stale_address_probe = selected_visible_node_probe.clone();
    selected_node_stale_address_probe["last_external_render_proof"]["visible_bound_text"]["entries"]
        [1]["text"] = json!("A0");
    let selected_node_stale_address_result = cells_visual_formula_probe_from_readback(
        &selected_node_stale_address_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        selected_node_stale_address_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "same-frame selected-node/source-intent proof should prove the selected address when the formula address text artifact is stale"
    );
    assert_eq!(
        selected_node_stale_address_result
            .pointer(
                "/structured_external_visible_surface_probe/visible_selected_node_matches_address"
            )
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        selected_node_stale_address_result
            .pointer("/structured_external_visible_surface_probe/visible_address_text_current")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    let mut first_click_visible_probe = visible_bound_text_probe.clone();
    first_click_visible_probe["last_external_render_proof"]["input_overlay_focus_state"] = json!({
        "previous_selected_address": null,
        "selected_address": "B0",
        "selected_node_count": 1
    });
    let first_click_visible_result = cells_visual_formula_probe_from_readback(
        &first_click_visible_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        first_click_visible_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "a first click can prove the selected cell with one focused selected target node plus current visible bound text"
    );
    let mut focus_address_structured_probe = structured_probe.clone();
    focus_address_structured_probe["last_external_render_proof"]["retained_bound_sync"]["text_update_values"] = json!([
        {
            "node": "formula-input",
            "text": "=add(A0,A1)",
            "paths": ["store.selected_input.editing_text"]
        }
    ]);
    let focus_address_result = cells_visual_formula_probe_from_readback(
        &focus_address_structured_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        focus_address_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass"),
        "same-frame focus/selected-node evidence should prove the selected address when the visible formula text is current"
    );
    assert_eq!(
        focus_address_result
            .pointer("/structured_external_visible_surface_probe/address_text_retained_visible")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        focus_address_result
            .pointer("/structured_external_visible_surface_probe/focus_state_matches")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let mut stale_structured_probe = structured_probe.clone();
    stale_structured_probe["last_external_render_proof"]["proof"]["frame_evidence_key"]["present_id"] =
        json!(8);
    let stale_structured_result = cells_visual_formula_probe_from_readback(
        &stale_structured_probe,
        Some(&before_path_text),
        "A0",
        "B0",
        "5",
        "=add(A0,A1)",
    );
    assert_eq!(
        stale_structured_result
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail"),
        "structured visible-surface proof must not pass with stale frame identity"
    );
    assert_eq!(
        stale_structured_result
            .pointer("/structured_external_visible_surface_probe/frame_evidence_matches")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    let _ = std::fs::remove_dir_all(&artifact_dir);
}
