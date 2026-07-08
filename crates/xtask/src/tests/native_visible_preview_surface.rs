// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn diagnostic_snapshots_drop_null_frame_evidence_placeholders() {
    let real_key = json!({
        "frame_seq": 7,
        "content_revision": 11,
        "layout_revision": 13,
        "render_scene_revision": 17,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "input_event_seq": null,
        "present_id": 7,
        "proof_request_id": null
    });
    let sanitized = remove_null_frame_evidence_keys(json!({
        "initial_readback_probe": {
            "status": "fail",
            "last_report": {
                "frame_evidence_key": null,
                "preview_perf_stats": {
                    "frame_evidence_key": null
                }
            }
        },
        "readback_probe": {
            "capture_method": "wgpu-visible-surface-copy-src-readback",
            "frame_evidence_key": real_key,
            "content_revision": 11,
            "rendered_frame_count": 7,
            "surface_epoch": 1
        }
    }));

    assert!(
        sanitized
            .pointer("/initial_readback_probe/last_report/frame_evidence_key")
            .is_none()
    );
    assert!(
        sanitized
            .pointer("/initial_readback_probe/last_report/preview_perf_stats/frame_evidence_key")
            .is_none()
    );
    assert!(
        sanitized
            .pointer("/readback_probe/frame_evidence_key")
            .is_some()
    );
    let mut reasons = Vec::new();
    collect_native_gpu_frame_evidence_reasons(&sanitized, "$", &mut reasons);
    assert!(
        reasons.is_empty(),
        "sanitized diagnostics should not create frame evidence blockers: {reasons:?}"
    );
}


#[test]
fn post_present_artifacts_synthesize_external_render_proof_for_frame() {
    let frame_key = json!({
        "frame_seq": 12,
        "content_revision": 3,
        "layout_revision": 4,
        "render_scene_revision": 5,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "input_event_seq": 9,
        "present_id": 12,
        "proof_request_id": null
    });
    let layout_dir = PathBuf::from("target/artifacts/native-gpu/document-layout");
    std::fs::create_dir_all(&layout_dir).unwrap();
    let layout_path = layout_dir.join(format!(
        "xtask-post-present-proof-layout-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &layout_path,
        serde_json::to_vec_pretty(&json!({
            "source_intents": [
                {
                    "node": "cell-b0",
                    "intent": "row_lookup",
                    "lookup_field": "address",
                    "lookup_value": "B0"
                }
            ],
            "layout_frame": {
                "display_list": [
                    {
                        "node": "cell-b0",
                        "kind": "button",
                        "text": "15",
                        "bounds": {"x": 80.0, "y": 80.0, "width": 80.0, "height": 24.0},
                        "style": {"selected": true}
                    }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let layout_hash = boon_runtime::sha256_file(&layout_path).unwrap();
    let layout_path_text = layout_path.to_string_lossy().to_string();
    let report = json!({
        "frame_evidence_key": frame_key,
        "last_render_target_kind": "visible-surface-direct",
        "recent_post_present_proof_artifacts": [
            {
                "status": "pass",
                "kind": "visible_bound_text",
                "frame_evidence_key": frame_key,
                "payload": {
                    "status": "pass",
                    "layout_frame_hash": layout_hash,
                    "entries": [
                        {
                            "text": "=A1+B2",
                            "paths": ["store.selected_input.editing_text"],
                            "selected": false,
                            "focused": true
                        },
                        {
                            "node": "cell-b0",
                            "text": "15",
                            "paths": ["@row:2:1:display_text"],
                            "selected": true,
                            "focused": true,
                            "text_truncated": false
                        }
                    ]
                }
            },
            {
                "status": "pass",
                "kind": "retained_bound_sync",
                "frame_evidence_key": frame_key,
                "payload": {
                    "status": "pass",
                    "changed": true,
                    "text_update_count": 1
                }
            }
        ],
        "matching_interactive_readback_artifact_for_frame": {
            "capture_method": "wgpu-visible-surface-copy-src-readback",
            "path": "target/artifacts/native-gpu/test.png",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "frame_evidence_key": frame_key
        }
    });

    let proof = cells_post_present_external_render_proof_for_frame(
        &report,
        report.get("frame_evidence_key").unwrap(),
    )
    .expect("post-present proof");

    assert_eq!(
        proof.pointer("/status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        proof.pointer("/source").and_then(serde_json::Value::as_str),
        Some("post_present_proof_artifacts_by_frame_evidence_key")
    );
    assert_eq!(
        proof.pointer("/proof/frame_evidence_key"),
        report.get("frame_evidence_key")
    );
    assert_eq!(
        proof
            .pointer("/proof/capture_method")
            .and_then(serde_json::Value::as_str),
        Some("wgpu-visible-surface-copy-src-readback")
    );
    assert_eq!(
        proof
            .pointer("/visible_bound_text/entries/0/text")
            .and_then(serde_json::Value::as_str),
        Some("=A1+B2")
    );
    assert_eq!(
        proof
            .pointer("/retained_bound_sync/text_update_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        proof
            .pointer("/layout_artifact")
            .and_then(serde_json::Value::as_str),
        Some(layout_path_text.as_str())
    );
    assert_eq!(
        proof
            .pointer("/input_overlay_focus_state/selected_address")
            .and_then(serde_json::Value::as_str),
        Some("B0")
    );
    let _ = std::fs::remove_file(layout_path);
}


#[test]
fn native_preview_route_evidence_uses_embedded_prelaunch_layout_probe() {
    let report = json!({
        "operator_host_input": true,
        "hit_target_assertions": [],
        "source_intent_assertions": [],
        "operator_host_input_evidence": {
            "target_region": {
                "id": "hit:primary",
                "node": "primary",
                "bounds": {"x": 10.0, "y": 20.0, "width": 80.0, "height": 24.0},
                "basis": "prelaunch-generic-document-layout-proof"
            },
            "host_events": [
                {"kind": "pointer_down", "button": "left"}
            ]
        },
        "prelaunch_layout_probe": {
            "hit_target_assertions": [
                {
                    "id": "hit:secondary",
                    "node": "secondary",
                    "bounds": {"x": 100.0, "y": 20.0, "width": 80.0, "height": 24.0}
                },
                {
                    "id": "hit:primary",
                    "node": "primary",
                    "bounds": {"x": 10.0, "y": 20.0, "width": 80.0, "height": 24.0}
                }
            ],
            "source_intent_assertions": [
                {
                    "node": "secondary",
                    "intent": "click",
                    "source_path": "store.sources.secondary.click"
                },
                {
                    "node": "primary",
                    "intent": "click",
                    "source_path": "store.sources.primary.click"
                }
            ]
        }
    });

    let evidence = native_preview_host_route_evidence("generic", &report);
    assert_eq!(
        evidence.get("status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        evidence
            .pointer("/target_hit_region/node")
            .and_then(serde_json::Value::as_str),
        Some("primary")
    );
    assert_eq!(
        evidence
            .pointer("/source_intents/0/source_path")
            .and_then(serde_json::Value::as_str),
        Some("store.sources.primary.click")
    );
}


#[test]
fn native_visible_reality_accepts_cosmic_portrait_dev_surface_with_area() {
    let report = native_visible_reality_surface_report(1020, 1080);

    let harness = native_visible_reality_harness(&report);

    assert_eq!(
        harness.get("status").and_then(serde_json::Value::as_str),
        Some("pass"),
        "blockers={:?}",
        harness.get("blockers")
    );
}


#[test]
fn native_visible_reality_uses_physical_size_for_window_usability() {
    let mut report = native_visible_reality_surface_report(1180, 820);
    report["dev_surface_proof"]["readback_artifact"]["width"] = json!(1920);
    report["dev_surface_proof"]["readback_artifact"]["height"] = json!(540);
    report["dev_surface_proof"]["readback_artifact"]["nonblank_samples"] = json!(1920 * 540);

    let harness = native_visible_reality_harness(&report);

    assert_eq!(
        harness.get("status").and_then(serde_json::Value::as_str),
        Some("pass"),
        "blockers={:?}",
        harness.get("blockers")
    );
}


#[test]
fn native_visible_reality_rejects_tiny_dev_surface() {
    let report = native_visible_reality_surface_report(820, 720);

    let harness = native_visible_reality_harness(&report);

    assert_eq!(
        harness.get("status").and_then(serde_json::Value::as_str),
        Some("fail")
    );
    let blockers = harness
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        blockers.iter().any(|blocker| blocker
            .as_str()
            .is_some_and(|text| text.contains("dev_surface_proof.physical_size"))),
        "blockers={blockers:?}"
    );
}


#[test]
fn native_runtime_assertion_requires_live_preview_route_not_source_replay() {
    let live_report = json!({
        "native_runtime_assertion_evidence": {
            "status": "pass",
            "live_preview_process_route": true,
            "public_runtime_api": "boon_runtime::LiveRuntime::apply_source_event_for_document_window",
            "assertions": [{"id": "live-preview-route", "pass": true}],
            "outputs": [{"path": "store.items", "value": "changed"}]
        }
    });
    let source_replay_report = json!({
        "native_runtime_assertion_evidence": {
            "status": "pass",
            "assertions": [{
                "source_scenario_replay": {
                    "status": "pass"
                }
            }]
        },
        "native_host_input_route_evidence": {
            "status": "pass",
            "acknowledged_source_events": [{"source": "store.add"}]
        }
    });

    assert!(native_runtime_assertion_proves_live_preview_route(
        &live_report
    ));
    assert!(
        !native_runtime_assertion_proves_live_preview_route(&source_replay_report),
        "PlanExecutor source replay must stay semantic evidence, not native proof"
    );
    let route_only_report = json!({
        "native_runtime_assertion_evidence": {
            "status": "pass",
            "live_preview_process_route": true,
            "public_runtime_api": "boon_runtime::LiveRuntime::source_intent_route",
            "assertions": [{"id": "route-only", "pass": true}],
            "outputs": []
        }
    });
    assert!(
        !native_runtime_assertion_proves_live_preview_route(&route_only_report),
        "source-intent route reachability without preview runtime outputs is not native runtime proof"
    );
}

fn native_visible_reality_surface_report(dev_width: u64, dev_height: u64) -> serde_json::Value {
    let surface = |role: &str, width: u64, height: u64| {
        json!({
            "pid": 10,
            "window_id": format!("{role}:window"),
            "surface_id": format!("{role}:surface"),
            "surface_epoch": 1,
            "window_backend": "app_window-wayland",
            "display_server": "wayland",
            "wgpu_strategy": "NotMainThread",
            "wgpu_surface_strategy": "NotMainThread",
            "main_thread_id": "ThreadId(1)",
            "render_thread_id": "ThreadId(2)",
            "logical_size": {"width": width, "height": height, "scale": 1.0},
            "physical_size": {"width": width, "height": height},
            "presented_frame": true,
            "interactive_frame_loop": true,
            "role": role,
            "readback_artifact": {
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "width": width,
                "height": height,
                "nonblank_samples": width.saturating_mul(height),
                "unique_rgba_values": 16
            }
        })
    };
    let mut dev_surface = surface("dev", dev_width, dev_height);
    let external_render_proof = json!({
        "status": "pass",
        "visible_surface_rendered": true,
        "visible_present_path": true,
        "render_backend_trait": "boon_native_gpu::encode_render_scene_to_surface",
        "visible_surface_metrics": {
            "render_scene_source": "document-render-scene-patch",
            "draw_calls": 4,
            "asset_ref_count": 0,
            "asset_refs": [],
            "asset_cache_hits": 0,
            "asset_cache_misses": 0,
            "asset_cache_evictions": 0,
            "asset_cache_entry_count": 0,
            "asset_cache_byte_count": 0,
            "asset_cache_byte_cap": 0,
            "asset_cache_byte_cap_hit": false,
            "asset_decode_count": 0,
            "asset_raster_count": 0,
            "asset_upload_count": 0,
            "asset_upload_bytes": 0,
            "asset_failure_diagnostics": [],
            "color_only_rect_fallback": false,
            "rect_cap_hit": false,
            "text_cap_hit": false,
            "retained_chunk_count": 1,
            "retained_chunk_hit_count": 1,
            "retained_chunk_miss_count": 0,
            "dirty_chunk_count": 0,
            "retained_chunk_sample_count": 1,
            "retained_chunk_inventory_truncated": false,
            "retained_chunks": [{
                "id": "chunk:test-dev",
                "node": "dev-root",
                "kind": "panel",
                "bounds": {"x": 0.0, "y": 0.0, "width": dev_width, "height": dev_height},
                "transform": {"x": 0.0, "y": 0.0, "scale": 1.0},
                "style_identity": {"style_id": 1},
                "dependency_set": ["dev-root"],
                "gpu_buffer_range": {"start": 0, "end": 6},
                "text_run_ids": [],
                "texture_asset_refs": [],
                "generation": 1,
                "cache_status": "hit"
            }]
        },
        "content_bounds_fill_ratio": 1.0,
        "dev_ui_source": "boon-dev-editor-debug-shell",
        "dev_editor_visible": true,
        "fixture_grid_used": false
    });
    dev_surface["external_render_proof"] = external_render_proof.clone();
    json!({
        "preview_surface_proof": {
            "pid": 10,
            "window_id": "preview:window",
            "surface_id": "preview:surface",
            "surface_epoch": 1,
            "window_backend": "app_window-wayland",
            "display_server": "wayland",
            "wgpu_strategy": "NotMainThread",
            "wgpu_surface_strategy": "NotMainThread",
            "main_thread_id": "ThreadId(1)",
            "render_thread_id": "ThreadId(2)",
            "logical_size": {"width": 1920, "height": 1080, "scale": 1.0},
            "physical_size": {"width": 1920, "height": 1080},
            "presented_frame": true,
            "interactive_frame_loop": true,
            "role": "preview",
            "readback_artifact": {
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "width": 1920,
                "height": 1080,
                "nonblank_samples": 2073600,
                "unique_rgba_values": 16
            },
            "product_render_graph_visible_proof": {
                "status": "pass"
            },
            "external_render_proof": external_render_proof
        },
        "dev_surface_proof": dev_surface,
        "preview_surface_proof_status": "unused",
        "native_runtime_assertion_evidence": {
            "status": "pass",
            "live_preview_process_route": true,
            "assertions": [{
                "id": "live-preview-route",
                "pass": true
            }]
        },
        "native_host_input_route_evidence": {
            "status": "pass",
            "changes_visible_frame": true
        }
    })
}

fn preview_surface_with_metrics(
    mut surface: serde_json::Value,
    metrics: serde_json::Value,
) -> serde_json::Value {
    surface["visible_surface_metrics"] = metrics;
    surface
}


#[test]
fn preview_e2e_surface_proof_does_not_republish_top_level_alias() {
    let visible_proof = native_visible_reality_surface_report(1020, 1080)
        .pointer("/dev_surface_proof/external_render_proof")
        .cloned()
        .expect("fixture should include a real visible render proof");
    assert!(native_visible_render_proof_is_usable(&visible_proof));

    let mut report = json!({
        "preview_native_gpu_render_proof": {
            "status": "pass",
            "proof": {
                "artifact": {
                    "present_result": "copy-to-present-scaffold",
                    "acquired_surface_texture": false
                }
            },
            "visible_surface_rendered": false,
            "visible_present_path": false
        },
        "preview_surface_proof": {
            "external_render_proof": visible_proof.clone()
        }
    });

    native_preview_e2e_promote_child_role_evidence(&mut report);

    assert_eq!(
        report.get("preview_native_gpu_render_proof"),
        Some(&json!({
            "status": "pass",
            "proof": {
                "artifact": {
                    "present_result": "copy-to-present-scaffold",
                    "acquired_surface_texture": false
                }
            },
            "visible_surface_rendered": false,
            "visible_present_path": false
        }))
    );
    assert!(
        report
            .get("preview_native_gpu_render_proof_source")
            .is_none(),
        "surface-scoped proof must not be republished as a top-level acceptance alias"
    );
    assert!(preview_surface_visible_proof_ok(&report));
}


#[test]
fn multiwindow_visible_proof_must_be_surface_scoped() {
    let visible_proof = native_visible_reality_surface_report(1020, 1080)
        .pointer("/dev_surface_proof/external_render_proof")
        .cloned()
        .expect("fixture should include a real visible render proof");
    assert!(native_visible_render_proof_is_usable(&visible_proof));

    let alias_only = json!({
        "preview_native_gpu_render_proof": visible_proof
    });
    assert!(!multiwindow_surface_visible_proof_ok(&alias_only));
    assert!(
        alias_only
            .get("preview_native_gpu_render_proof_source")
            .is_none(),
        "alias-only proof must not be republished as accepted surface evidence"
    );

    let surface_scoped = json!({
        "preview_surface_proof": {
            "external_render_proof": alias_only["preview_native_gpu_render_proof"].clone()
        }
    });
    assert!(multiwindow_surface_visible_proof_ok(&surface_scoped));

    let direct_surface_scoped = json!({
        "preview_surface_proof": native_visible_reality_surface_report(1020, 1080)["preview_surface_proof"].clone()
    });
    assert!(multiwindow_surface_visible_proof_ok(&direct_surface_scoped));
}


#[test]
fn scroll_hot_path_rejects_render_hook_offscreen_proof() {
    let mut blockers = Vec::new();
    require_scroll_render_hook_app_owned_proof_skipped(
        &mut blockers,
        &json!({
            "preview_surface_proof": {
                "external_render_proof": {
                    "render_backend_trait": "boon_native_gpu::render_app_owned_scene_pixels",
                    "offscreen_app_owned_scene_readback_skipped": false
                }
            }
        }),
    );
    assert!(blockers.iter().any(|blocker| {
        blocker.contains("offscreen_app_owned_scene_readback_skipped must be true")
    }));
    assert!(blockers.iter().any(|blocker| {
        blocker.contains("render_backend_trait must not use render_app_owned_scene_pixels")
    }));

    let mut blockers = Vec::new();
    require_scroll_render_hook_app_owned_proof_skipped(
        &mut blockers,
        &json!({
            "preview_surface_proof": {
                "external_render_proof": {
                    "render_backend_trait": "boon_native_gpu::encode_render_scene_to_surface",
                    "offscreen_app_owned_scene_readback_skipped": true
                }
            }
        }),
    );
    assert!(blockers.is_empty(), "{blockers:?}");
}


#[test]
fn axis_specific_scroll_timing_promotes_post_input_window() {
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "axis_specific_real_window_scroll_observation": {
            "status": "pass",
            "vertical_observation": {
                "render_hook_app_owned_proof_skipped": true,
                "surface_post_input_frame_timing": {
                    "first_presented_frame_ms": 300.0,
                    "measured_frame_count": 59,
                    "command_record_ms_max": 5.5,
                    "command_record_ms_p50": 1.1,
                    "command_record_ms_p95": 4.5,
                    "encoder_finish_ms_max": 0.7,
                    "encoder_finish_ms_p50": 0.2,
                    "encoder_finish_ms_p95": 0.6,
                    "queue_submit_ms_max": 0.9,
                    "queue_submit_ms_p50": 0.3,
                    "queue_submit_ms_p95": 0.8,
                    "frame_present_ms_max": 9.7,
                    "frame_present_ms_p50": 6.1,
                    "frame_present_ms_p95": 8.6,
                    "post_present_bookkeeping_ms_max": 0.5,
                    "post_present_bookkeeping_ms_p50": 0.1,
                    "post_present_bookkeeping_ms_p95": 0.4,
                    "presented_frame_ms_max": 17.6,
                    "presented_frame_ms_p50": 10.1,
                    "presented_frame_ms_p95": 12.3,
                    "presented_frame_ms_p99": 17.6,
                    "presented_frame_ms_over_16_7_indices": [12],
                    "presented_frame_ms_over_16_7_max": 17.6,
                    "render_hook_ms_p95": 3.2,
                    "sample_frame_count": 60,
                    "warmup_frame_count": 3
                }
            },
            "horizontal_observation": {
                "render_hook_app_owned_proof_skipped": true,
                "surface_post_input_frame_timing": {
                    "first_presented_frame_ms": 310.0,
                    "measured_frame_count": 59,
                    "command_record_ms_max": 6.5,
                    "command_record_ms_p50": 1.2,
                    "command_record_ms_p95": 5.5,
                    "encoder_finish_ms_max": 0.8,
                    "encoder_finish_ms_p50": 0.3,
                    "encoder_finish_ms_p95": 0.7,
                    "queue_submit_ms_max": 1.0,
                    "queue_submit_ms_p50": 0.4,
                    "queue_submit_ms_p95": 0.9,
                    "frame_present_ms_max": 10.7,
                    "frame_present_ms_p50": 6.2,
                    "frame_present_ms_p95": 9.6,
                    "post_present_bookkeeping_ms_max": 0.6,
                    "post_present_bookkeeping_ms_p50": 0.2,
                    "post_present_bookkeeping_ms_p95": 0.5,
                    "presented_frame_ms_max": 16.8,
                    "presented_frame_ms_p50": 10.2,
                    "presented_frame_ms_p95": 14.9,
                    "presented_frame_ms_p99": 16.8,
                    "presented_frame_ms_over_16_7_indices": [20],
                    "presented_frame_ms_over_16_7_max": 16.8,
                    "render_hook_ms_p95": 3.4,
                    "sample_frame_count": 60,
                    "warmup_frame_count": 3
                }
            }
        }
    });

    assert!(promote_axis_specific_scroll_timing(&mut report));
    assert_eq!(
        report
            .get("preview_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(14.9)
    );
    assert_eq!(
        report
            .pointer("/wheel_to_visible_ms_p95_per_axis/vertical")
            .and_then(serde_json::Value::as_f64),
        Some(12.3)
    );
    assert_eq!(
        report
            .pointer("/wheel_to_visible_ms_p95_per_axis/horizontal")
            .and_then(serde_json::Value::as_f64),
        Some(14.9)
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/measured_frame_count")
            .and_then(serde_json::Value::as_u64),
        Some(118)
    );
    assert_eq!(
        report
            .pointer("/post_input_measured_frame_count_per_axis/vertical")
            .and_then(serde_json::Value::as_u64),
        Some(59)
    );
    assert_eq!(
        report
            .pointer("/post_input_measured_frame_count_per_axis/horizontal")
            .and_then(serde_json::Value::as_u64),
        Some(59)
    );
    assert_eq!(
        report
            .get("speed_timing_window")
            .and_then(serde_json::Value::as_str),
        Some("axis-specific-post-real-window-input")
    );
    assert_eq!(
        report
            .get("render_hook_app_owned_proof_skipped_for_axis_timing")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/presented_frame_ms_over_16_7_count")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/presented_frame_ms_over_16_7_indices")
            .and_then(serde_json::Value::as_array)
            .cloned(),
        Some(vec![json!(12), json!(79)])
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/command_record_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(5.5)
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/frame_present_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(9.6)
    );
    assert_eq!(
        report
            .pointer("/post_input_frame_timing/post_present_bookkeeping_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(0.5)
    );
}
