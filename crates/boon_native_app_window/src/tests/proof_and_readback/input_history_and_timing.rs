// Included by `../proof_and_readback.rs`.

// test: merge_input_adapter_proof_keeps_current_button_and_key_state
#[test]
fn merge_input_adapter_proof_keeps_current_button_and_key_state() {
    let mut base = empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full);

    let mut pressed = empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full);
    pressed.real_os_events_observed = true;
    pressed.mouse_button_event_count = 2;
    pressed.keyboard_key_event_count = 3;
    pressed.mouse_buttons_down = vec!["left".to_owned()];
    pressed.pressed_keys = vec!["KeyA".to_owned()];
    merge_input_adapter_proof(&mut base, &pressed);

    assert_eq!(base.mouse_button_event_count, 2);
    assert_eq!(base.keyboard_key_event_count, 3);
    assert_eq!(base.mouse_buttons_down, vec!["left".to_owned()]);
    assert_eq!(base.pressed_keys, vec!["KeyA".to_owned()]);

    let mut released = empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full);
    released.mouse_button_event_count = 4;
    released.keyboard_key_event_count = 5;
    merge_input_adapter_proof(&mut base, &released);

    assert_eq!(base.mouse_button_event_count, 4);
    assert_eq!(base.keyboard_key_event_count, 5);
    assert!(
        base.mouse_buttons_down.is_empty(),
        "current mouse button state must clear after a release/no-buttons sample"
    );
    assert!(
        base.pressed_keys.is_empty(),
        "current keyboard state must clear after a no-keys sample"
    );
}

// test: recent_history_compacts_visible_bound_text_without_losing_selection_evidence
#[test]
fn recent_history_compacts_visible_bound_text_without_losing_selection_evidence() {
    let mut entries = (0..96)
        .map(|index| {
            serde_json::json!({
                "node": format!("unrelated-{index}"),
                "kind": "Text",
                "text": format!("bulk-{index}"),
                "text_truncated": false,
                "paths": [format!("bulk.path.{index}")],
                "focused": false,
                "selected": false
            })
        })
        .collect::<Vec<_>>();
    entries.push(serde_json::json!({
        "node": "formula-bar",
        "kind": "TextInput",
        "text": "selected formula",
        "text_truncated": false,
        "paths": ["store.selected_input.editing_text"],
        "focused": false,
        "selected": false
    }));
    entries.push(serde_json::json!({
        "node": "selected-cell",
        "kind": "Text",
        "text": "selected row",
        "text_truncated": false,
        "paths": ["record.selected_label"],
        "focused": true,
        "selected": true
    }));
    let proof = serde_json::json!({
        "visible_bound_text": {
            "status": "pass",
            "source": "layout-frame-current-bound-text",
            "entry_count": entries.len(),
            "entry_limit": 512,
            "truncated": false,
            "entries": entries
        }
    });

    let compact = compact_visible_bound_text_for_recent_history(&proof);
    let compact_entries = compact
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .expect("compact visible text entries");

    assert_eq!(compact["recent_history_compacted"], true);
    assert_eq!(compact["entry_count"], 98);
    assert!(compact_entries.len() <= 64);
    assert!(compact_entries.iter().any(|entry| {
        entry.get("node").and_then(serde_json::Value::as_str) == Some("formula-bar")
            && entry.get("text").and_then(serde_json::Value::as_str) == Some("selected formula")
    }));
    assert!(compact_entries.iter().any(|entry| {
        entry.get("node").and_then(serde_json::Value::as_str) == Some("selected-cell")
            && entry.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
    }));
    assert!(
        !compact_entries.iter().any(|entry| {
            entry.get("node").and_then(serde_json::Value::as_str) == Some("unrelated-95")
        }),
        "bulk unrelated entries should not dominate recent frame history"
    );
}

// test: recent_history_preserves_render_hook_phase_timings
#[test]
fn recent_history_preserves_render_hook_phase_timings() {
    let key = FrameEvidenceKey {
        frame_seq: 7,
        content_revision: 11,
        layout_revision: 3,
        render_scene_revision: 5,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(13),
        present_id: 17,
        proof_request_id: None,
    };
    let proof = serde_json::json!({
        "status": "pass",
        "render_hook_phase_timings_ms": {
            "encode_scene_ms": 0.42,
            "report_json_ms": 0.11,
            "total_with_report_json_ms": 0.57
        },
        "proof": {
            "status": "pass",
            "capture_method": "wgpu-visible-surface-copy-src-readback"
        }
    });

    let compact = compact_external_render_proof_for_recent_history(Some(&proof), &key);

    assert_eq!(
        compact
            .pointer("/render_hook_phase_timings_ms/encode_scene_ms")
            .and_then(serde_json::Value::as_f64),
        Some(0.42)
    );
    assert_eq!(
        compact
            .pointer("/render_hook_phase_timings_ms/total_with_report_json_ms")
            .and_then(serde_json::Value::as_f64),
        Some(0.57)
    );
    assert_eq!(
        compact.pointer("/proof/frame_evidence_key"),
        Some(&serde_json::to_value(key).expect("frame evidence key serializes"))
    );
}

// test: async_post_present_proof_worker_completes_history_and_report_requests
#[test]
fn async_post_present_proof_worker_completes_history_and_report_requests() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 11,
        content_revision: 13,
        layout_revision: 5,
        render_scene_revision: 7,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(17),
        present_id: 21,
        proof_request_id: None,
    };
    state.enqueue_post_present_proof_requests(
        &key,
        &[
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ProofHistory,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::RenderHookReportJson,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
        ],
        Some(70.0),
    );
    let mut worker = AsyncPostPresentProofSubscriberWorker::new();
    let enqueue_report = worker
        .enqueue(
            vec![
                proof_history_post_present_subscriber(3),
                render_hook_report_json_post_present_subscriber(
                    "target/reports/native-gpu/loop.json".to_owned(),
                    0.42,
                ),
            ],
            key.clone(),
            Some(72.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::BackgroundTelemetry,
        )
        .expect("history and report subscriber batch should enqueue");
    state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

    worker.shutdown_and_drain(&mut state);

    assert_eq!(
        state.post_present_proof_subscriber_worker_completed_count,
        2
    );
    assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
    assert_eq!(state.post_present_proof_artifact_count, 2);
    assert_eq!(state.post_present_proof_queue_completed_count, 2);
    assert!(
        state
            .recent_post_present_proof_queue
            .iter()
            .all(|entry| entry.status == NativePostPresentProofQueueStatus::CompletedPostPresent)
    );
    let artifacts: Vec<_> = state.recent_post_present_proof_artifacts.iter().collect();
    assert!(artifacts.iter().any(|artifact| {
        artifact.kind == NativePostPresentProofRequestKind::ProofHistory
            && artifact.frame_evidence_key == key
            && artifact.payload["recent_frame_evidence_count"] == serde_json::json!(3)
    }));
    assert!(artifacts.iter().any(|artifact| {
        artifact.kind == NativePostPresentProofRequestKind::RenderHookReportJson
            && artifact.frame_evidence_key == key
            && artifact.payload["report_snapshot_enqueued"] == serde_json::json!(true)
            && artifact.payload["report_path"]
                == serde_json::json!("target/reports/native-gpu/loop.json")
    }));
}

// test: post_present_subscriber_drain_yields_to_pending_host_input
#[test]
fn post_present_subscriber_drain_yields_to_pending_host_input() {
    assert!(post_present_subscriber_drain_allowed(3, 3));
    assert!(post_present_subscriber_drain_allowed(2, 3));
    assert!(!post_present_subscriber_drain_allowed(4, 3));
}

// test: recent_history_preserves_poll_input_timing_samples
#[test]
fn recent_history_preserves_poll_input_timing_samples() {
    let diagnostics = serde_json::json!({
        "kind": "preview_role_poll",
        "dirty": true,
        "accessibility_snapshot_status": "deferred_product_input",
        "phase_timings_ms": {
            "source_input_ms": 5.0,
            "passive_hover_ms": 0.25
        },
        "recent_native_input_timing_samples": [
            {
                "fast_path": "simple_source_click",
                "total_ms": 1.25,
                "resolve_source": "cached_click_candidate"
            }
        ],
        "native_input_reject_counts": {}
    });

    let compact = compact_poll_diagnostics_for_recent_history(Some(&diagnostics));

    assert_eq!(
        compact
            .pointer("/accessibility_snapshot_status")
            .and_then(serde_json::Value::as_str),
        Some("deferred_product_input")
    );
    assert_eq!(
        compact
            .pointer("/recent_native_input_timing_samples/0/fast_path")
            .and_then(serde_json::Value::as_str),
        Some("simple_source_click")
    );
    assert_eq!(
        compact
            .pointer("/phase_timings_ms/source_input_ms")
            .and_then(serde_json::Value::as_f64),
        Some(5.0)
    );
}

// test: render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats
#[test]
fn render_loop_report_uses_frame_scoped_input_latency_for_preview_perf_stats() {
    let dir = std::env::temp_dir().join(format!(
        "boon-native-report-frame-input-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.json");

    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_poll_started(18.0);
    state.note_accepted_host_input(3, 20.0, false, None);
    state.note_dirty_poll(21.0);
    state.note_render_started(22.0);
    state.note_surface_acquired(22.5);
    state.note_surface_acquire_call(0.5);
    state.note_render_hook_completed(24.0);
    state.note_queue_submitted(26.0);
    state.note_submit_phase_durations(0.2, 0.3, 1.5);
    state.note_present_completed(27.5);
    state.mark_presented_with_revisions(1, 2, 3, 4);
    state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
    let frame_input_to_present_ms =
        state.take_frame_accepted_input_to_present_ms(state.current_frame_input_event_seq(3));
    state.note_dirty_poll(140.0);
    state.note_render_started(150.0);
    state.note_surface_acquired(151.0);
    state.note_surface_acquire_call(5.0);
    state.note_render_hook_completed(152.0);
    state.note_queue_submitted(153.0);
    state.note_submit_phase_durations(6.0, 7.0, 8.0);
    state.note_present_completed(160.0);
    let proof_requests = vec![
        NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::VisibleBoundText,
            built_pre_present: true,
            frame_local_snapshot_required: true,
        },
        NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::RenderHookReportJson,
            built_pre_present: true,
            frame_local_snapshot_required: true,
        },
    ];
    state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
        layout_ms: Some(0.25),
        product_result: Some(test_product_result(
            NativeRenderedProductFrame {
                schema_version: 1,
                render_target_kind: "visible-surface-direct".to_owned(),
                visible_surface_rendered: true,
                visible_present_path: true,
                layout_identity: Some("layout-test".to_owned()),
                render_scene_identity: Some("scene-test".to_owned()),
                proof_json_built_pre_present: true,
                render_hook_proof_built_pre_present: true,
                post_present_proof_request_count: proof_requests.len() as u32,
                product_patch: None,
            },
            proof_requests,
        )),
        ..NativeRenderFrameMetrics::default()
    }));
    let product_commit_key = frame_evidence_key_for_presented_frame(
        &state,
        &SurfaceId("surface-test".to_owned()),
        1,
        Some(3),
        None,
    );
    state.note_product_frame_commit(product_frame_commit_for_presented_frame(
        &state,
        product_commit_key.clone(),
        NativeAdapterIdentity::default(),
        NativeFrameLane::ProductInteraction,
        Some(NativeSchedulerReason::HostInput),
        None,
        state.last_accounted_input_frame_timing.clone(),
    ));
    state.note_frame_clock_policy(NativeFrameClock::policy(
        Some(NativeFrameLane::ProductInteraction),
        Some(NativeSchedulerReason::HostInput),
        true,
        true,
        Some(false),
    ));
    let mut recent_product_frame_commits = VecDeque::new();
    if let Some(commit) = state.last_product_frame_commit.as_ref() {
        push_recent_product_frame_commit(&mut recent_product_frame_commits, commit);
    }
    let mut perf_accumulator = NativePreviewPerfAccumulator::default();
    perf_accumulator.record(
        None,
        None,
        state.last_present_call_ms,
        state.last_surface_acquire_call_ms,
        state.last_queue_submit_call_ms,
        state.last_present_path_ms,
        frame_input_to_present_ms,
        Some(NativeFrameLane::ProductInteraction),
        None,
    );

    write_render_loop_state_report(
        &path,
        NativeWindowRole::Preview,
        std::process::id(),
        &WindowId("window-test".to_owned()),
        &SurfaceId("surface-test".to_owned()),
        &NativeSurfaceLifecycleReport {
            surface_epoch: 1,
            final_width: 1,
            final_height: 1,
            ..NativeSurfaceLifecycleReport::default()
        },
        &state,
        Duration::from_millis(16),
        0,
        None,
        &perf_accumulator,
        NativeRenderLoopReportExtras {
            present_mode: "Immediate".to_owned(),
            surface_format: "Bgra8Unorm".to_owned(),
            desired_maximum_frame_latency: 1,
            desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
            ..NativeRenderLoopReportExtras::default()
        }
        .with_input_generation(3, 3, None, None)
        .with_frame_evidence_key(Some(&product_commit_key))
        .with_recent_product_frame_commits(&recent_product_frame_commits),
        None,
    )
    .unwrap();

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(report["frame_input_to_present_ms"], serde_json::json!(7.5));
    assert_eq!(report["input_accept_to_present_ms"], serde_json::json!(7.5));
    assert_eq!(
        report["top_level_phase_timing_scope"],
        serde_json::json!("accepted_visible_host_input_frame")
    );
    assert_eq!(
        report["latest_frame_timing_scope"],
        serde_json::json!("latest_presented_frame_raw_last_fields")
    );
    assert_eq!(
        report["input_to_present_accounted_event_wake_count"],
        serde_json::json!(3)
    );
    assert_eq!(
        report["accepted_input_frame_timing"]["timing_scope"],
        serde_json::json!("accepted_visible_host_input_frame")
    );
    assert_eq!(
        report["native_frame_clock_owner"],
        serde_json::json!("native_frame_clock")
    );
    assert_eq!(
        report["native_frame_clock_policy"]["frame_lane"],
        serde_json::json!("product_interaction")
    );
    assert_eq!(
        report["native_frame_clock_policy"]["pre_submit_proof_poll_allowed"],
        serde_json::json!(false)
    );
    assert_eq!(
        report["poll_started_to_input_accept_ms"],
        serde_json::json!(2.0),
        "pre-accept poll/hook work must remain visible separately from accepted product latency"
    );
    assert_eq!(
        report["accepted_input_frame_timing"]["dirty_poll_to_render_started_ms"],
        serde_json::json!(1.0)
    );
    assert_eq!(
        report["dirty_poll_to_render_started_ms"],
        serde_json::json!(1.0),
        "verifier-facing phase fields must describe the accepted input frame, not a later requested-animation frame"
    );
    assert_eq!(
        report["render_hook_completed_to_present_ms"],
        serde_json::json!(3.5)
    );
    assert_eq!(report["queue_to_present_ms"], serde_json::json!(1.5));
    assert_eq!(report["surface_acquire_call_ms"], serde_json::json!(0.5));
    assert_eq!(report["queue_submit_call_ms"], serde_json::json!(0.3));
    assert_eq!(report["present_call_ms"], serde_json::json!(1.5));
    assert_eq!(report["frame_present_call_ms"], serde_json::json!(1.5));
    assert_eq!(report["present_path_ms"], serde_json::json!(2.3));
    assert_eq!(
        report["last_present_call_ms"],
        serde_json::json!(8.0),
        "latest-frame debug fields should still expose the most recent follow-up frame"
    );
    assert_eq!(
        report["preview_perf_stats"]["input_to_present_ms"],
        serde_json::json!(7.5)
    );
    assert_eq!(
        report["preview_perf_stats"]["input_to_present_ms_p50_p95_p99_max"]["p95"],
        serde_json::json!(7.5)
    );
    assert_eq!(
        report["preview_perf_stats"]["frame_lane"],
        serde_json::json!("product_interaction")
    );
    assert_eq!(
        report["preview_perf_stats"]["product_input_to_present_ms_p50_p95_p99_max"]["p95"],
        serde_json::json!(7.5)
    );
    assert_eq!(
        report["preview_perf_stats"]["product_missed_frame_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["last_product_render_frame"]["render_target_kind"],
        serde_json::json!("visible-surface-direct")
    );
    assert_eq!(
        report["last_product_render_frame"]["proof_json_built_pre_present"],
        serde_json::json!(true)
    );
    assert_eq!(
        report["post_present_proof_request_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["pre_present_proof_request_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["product_proof_built_pre_present"],
        serde_json::json!(true)
    );
    assert_eq!(report["product_frame_commit_count"], serde_json::json!(1));
    assert_eq!(
        report["recent_product_frame_commit_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        report["product_frame_commit_matches_frame_evidence_key"],
        serde_json::json!(true)
    );
    assert_eq!(
        report["last_product_frame_commit"]["commit_source"],
        serde_json::json!("app_window_product_frame_commit")
    );
    assert_eq!(
        report["last_product_frame_commit"]["frame_lane"],
        serde_json::json!("product_interaction")
    );
    assert_eq!(
        report["last_product_frame_commit"]["input_to_present_ms"],
        serde_json::json!(7.5)
    );
    assert_eq!(
        report["last_product_frame_commit"]["post_present_proof_request_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["last_product_frame_commit"]["pre_present_proof_request_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["post_present_proof_queue_enqueued_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["post_present_proof_queue_deferred_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["post_present_proof_queue_pre_present_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["post_present_proof_queue_completed_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["post_present_subscriber_drain_deferred_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["last_post_present_subscriber_drain_deferred_reason"],
        serde_json::Value::Null
    );
    assert_eq!(
        report["post_present_proof_artifact_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["post_present_proof_subscriber_error_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["recent_post_present_proof_artifact_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        report["recent_post_present_proof_queue_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        report["recent_post_present_proof_queue"][0]["frame_evidence_key"],
        report["frame_evidence_key"]
    );
    assert_eq!(
        report["recent_post_present_proof_queue"][0]["status"],
        serde_json::json!("already_built_pre_present")
    );
    assert_eq!(
        report["recent_product_frame_commits"][0]["frame_evidence_key"],
        report["frame_evidence_key"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// test: input_resample_counters_distinguish_inline_and_deferred_turns
#[test]
fn input_resample_counters_distinguish_inline_and_deferred_turns() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

    state.note_input_inline_resample(2);
    assert_eq!(state.input_inline_resample_count, 1);
    assert_eq!(state.input_deferred_resample_count, 0);
    assert_eq!(state.input_inline_resample_event_gap_count, 2);
    assert_eq!(state.input_deferred_resample_event_gap_count, 0);
    assert_eq!(state.last_input_resample_event_gap_count, 2);
    assert_eq!(
        state.last_input_resample_kind.as_deref(),
        Some("inline_before_hook")
    );

    state.note_input_deferred_resample(3);
    assert_eq!(state.input_inline_resample_count, 1);
    assert_eq!(state.input_deferred_resample_count, 1);
    assert_eq!(state.input_inline_resample_event_gap_count, 2);
    assert_eq!(state.input_deferred_resample_event_gap_count, 3);
    assert_eq!(state.last_input_resample_event_gap_count, 3);
    assert_eq!(
        state.last_input_resample_kind.as_deref(),
        Some("deferred_next_loop")
    );

    state.note_input_pre_present_resample(4);
    assert_eq!(state.input_inline_resample_count, 1);
    assert_eq!(state.input_deferred_resample_count, 2);
    assert_eq!(state.input_inline_resample_event_gap_count, 2);
    assert_eq!(state.input_deferred_resample_event_gap_count, 7);
    assert_eq!(state.last_input_resample_event_gap_count, 4);
    assert_eq!(
        state.last_input_resample_kind.as_deref(),
        Some("pre_present_drop")
    );

    state.note_input_post_present_stale_readback_skip(5);
    assert_eq!(state.input_inline_resample_count, 1);
    assert_eq!(state.input_deferred_resample_count, 3);
    assert_eq!(state.input_inline_resample_event_gap_count, 2);
    assert_eq!(state.input_deferred_resample_event_gap_count, 12);
    assert_eq!(state.last_input_resample_event_gap_count, 5);
    assert_eq!(
        state.last_input_resample_kind.as_deref(),
        Some("post_present_stale_readback_skip")
    );
}

// test: interactive_surface_readback_is_coalesced_while_previous_proof_is_pending
#[test]
fn interactive_surface_readback_is_coalesced_while_previous_proof_is_pending() {
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            false,
            false,
            false,
            false,
        ),
        InteractiveSurfaceReadbackDecision::Queue
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            true,
            false,
            false,
            false,
            false,
        ),
        InteractiveSurfaceReadbackDecision::SkipExternalProof
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            true,
            false,
            false,
            false,
        ),
        InteractiveSurfaceReadbackDecision::SkipBackpressure
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Dev,
            true,
            false,
            false,
            false,
            false,
            false
        ),
        InteractiveSurfaceReadbackDecision::Queue
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Dev,
            true,
            false,
            true,
            false,
            false,
            false
        ),
        InteractiveSurfaceReadbackDecision::SkipBackpressure
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            true,
            false,
            false,
            true,
        ),
        InteractiveSurfaceReadbackDecision::DeferInteractionBurst
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            false,
            false,
            false,
            false,
            false,
            false,
        ),
        InteractiveSurfaceReadbackDecision::Off
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            false,
            true,
            false,
            true,
        ),
        InteractiveSurfaceReadbackDecision::DeferProductInput
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Dev,
            true,
            false,
            true,
            true,
            false,
            true
        ),
        InteractiveSurfaceReadbackDecision::DeferProductInput
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            false,
            true,
            true,
            true,
        ),
        InteractiveSurfaceReadbackDecision::Queue
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            true,
            false,
            true,
            true,
            true,
        ),
        InteractiveSurfaceReadbackDecision::Queue
    );
    assert_eq!(
        interactive_surface_readback_decision(
            NativeWindowRole::Preview,
            true,
            false,
            true,
            true,
            true,
            true,
        ),
        InteractiveSurfaceReadbackDecision::SkipBackpressure
    );
}

// test: accepted_host_input_summary_honors_semantic_press_hint_in_coalesced_batch
#[test]
fn accepted_host_input_summary_honors_semantic_press_hint_in_coalesced_batch() {
    let mut input = empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full);
    input.real_os_events_observed = true;
    input.mouse_button_event_count = 2;
    input.mouse_button_events = vec![
        NativeMouseButtonEventProof {
            sequence: 1,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(9),
            event_elapsed_ms: Some(10.0),
        },
        NativeMouseButtonEventProof {
            sequence: 2,
            button: "left".to_owned(),
            pressed: false,
            window_protocol_id: Some(9),
            event_elapsed_ms: Some(18.0),
        },
    ];
    let hint = NativeHostInputEventHint {
        kind: "mouse_button".to_owned(),
        source_intent: Some("press".to_owned()),
        sequence: Some(1),
        window_protocol_id: Some(9),
        button: Some("left".to_owned()),
        pressed: Some(true),
        key: None,
        event_elapsed_ms: Some(10.0),
    };

    let summary = accepted_host_input_event_summary(&input, 2, Some(18.0), 20.0, Some(&hint));

    assert_eq!(summary.source_intent.as_deref(), Some("press"));
    assert_eq!(summary.sequence, Some(1));
    assert_eq!(summary.pressed, Some(true));
    assert_eq!(summary.event_elapsed_ms, Some(10.0));
    assert_eq!(summary.wake_elapsed_ms, Some(10.0));
    assert_eq!(summary.wake_to_accept_ms, Some(10.0));
    assert_eq!(summary.raw_wake_elapsed_ms, Some(18.0));
    assert_eq!(summary.mouse_button_delta_count, 2);
}

// test: accepted_input_frame_timing_is_not_rewritten_by_followup_burst_frames
#[test]
fn accepted_input_frame_timing_is_not_rewritten_by_followup_burst_frames() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_accepted_host_input(7, 100.0, false, None);
    state.note_dirty_poll(101.0);
    state.note_render_started(102.0);
    state.note_surface_acquired(103.0);
    state.note_surface_acquire_call(0.25);
    state.note_render_hook_completed(104.0);
    state.note_queue_submitted(104.0);
    state.note_submit_phase_durations(0.125, 0.5, 2.0);
    state.note_render_target_kind("surface");
    state.note_present_path_selection(NativePresentPathSelection {
        requested_mode: NativePresentPathMode::DirectVisibleSurface,
        selected_mode: NativePresentPathMode::DirectVisibleSurface,
        reason: "test-direct",
        hooks_present: true,
        surface_copy_to_present_supported: false,
        readback_enabled: true,
    });
    state.note_present_completed(106.0);

    let timing = state
        .take_frame_accepted_input_timing(Some(1))
        .expect("input frame should be accounted");
    assert_eq!(timing.input_to_present_ms, 6.0);
    assert_eq!(timing.input_accept_to_dirty_poll_ms, Some(1.0));
    assert_eq!(timing.dirty_poll_to_render_started_ms, Some(1.0));
    assert_eq!(timing.render_started_to_surface_acquired_ms, Some(1.0));
    assert_eq!(timing.render_started_to_render_hook_completed_ms, Some(2.0));
    assert_eq!(
        timing.surface_acquired_to_render_hook_completed_ms,
        Some(1.0)
    );
    assert_eq!(timing.render_hook_completed_to_present_ms, Some(2.0));
    assert_eq!(timing.render_hook_to_queue_ms, Some(0.0));
    assert_eq!(timing.queue_to_present_ms, Some(2.0));
    assert_eq!(timing.surface_acquire_call_ms, Some(0.25));
    assert_eq!(timing.encoder_finish_ms, Some(0.125));
    assert_eq!(timing.queue_submit_call_ms, Some(0.5));
    assert_eq!(timing.present_call_ms, Some(2.0));
    assert_eq!(timing.present_path_ms, Some(2.75));
    assert_eq!(timing.render_target_kind.as_deref(), Some("surface"));
    assert_eq!(
        timing.present_path_mode,
        Some(NativePresentPathMode::DirectVisibleSurface)
    );

    state.note_render_started(140.0);
    state.note_surface_acquired(140.5);
    state.note_surface_acquire_call(3.25);
    state.note_render_hook_completed(141.0);
    state.note_queue_submitted(143.0);
    state.note_submit_phase_durations(3.125, 3.5, 4.0);
    state.note_render_target_kind("offscreen");
    state.note_present_path_selection(NativePresentPathSelection {
        requested_mode: NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        selected_mode: NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        reason: "test-offscreen",
        hooks_present: true,
        surface_copy_to_present_supported: true,
        readback_enabled: true,
    });
    state.note_present_completed(144.0);

    assert_eq!(state.take_frame_accepted_input_timing(Some(1)), None);
    let stored = state
        .last_accounted_input_frame_timing
        .as_ref()
        .expect("accepted input timing should remain available for later reports");
    assert_eq!(stored.input_to_present_ms, 6.0);
    assert_eq!(
        stored.dirty_poll_to_render_started_ms,
        Some(1.0),
        "later requested-animation frames must not make the product UX phase breakdown compare stale dirty-poll time with a newer render start"
    );
    assert_eq!(stored.surface_acquire_call_ms, Some(0.25));
    assert_eq!(stored.encoder_finish_ms, Some(0.125));
    assert_eq!(stored.queue_submit_call_ms, Some(0.5));
    assert_eq!(stored.present_call_ms, Some(2.0));
    assert_eq!(stored.present_path_ms, Some(2.75));
    assert_eq!(stored.render_target_kind.as_deref(), Some("surface"));
    assert_eq!(
        stored.present_path_mode,
        Some(NativePresentPathMode::DirectVisibleSurface)
    );
}

// test: pre_input_subscriber_drain_skip_is_counted
#[test]
fn pre_input_subscriber_drain_skip_is_counted() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

    state.note_pre_input_subscriber_drain_skipped("pending_host_input");
    state.note_pre_input_subscriber_drain_skipped("pending_host_input");

    assert_eq!(state.pre_input_subscriber_drain_skip_count, 2);
    assert_eq!(
        state.last_pre_input_subscriber_drain_skip_reason.as_deref(),
        Some("pending_host_input")
    );
}

// test: same_content_host_input_repaint_can_use_existing_content_revision
#[test]
fn same_content_host_input_repaint_can_use_existing_content_revision() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);

    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        },
        true,
    );

    assert_eq!(state.dirty_revision, state.presented_revision + 1);
    assert!(state.should_render(Instant::now(), false));
    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::ScrollChanged)
    );
    let same_content = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: state.presented_revision,
        layout_revision: None,
        render_scene_revision: None,
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: false,
        role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
    };
    assert!(
        same_content
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                state.current_scheduler_reason,
                state.current_role_dirty_reason,
            )
            .is_ok(),
        "same-content host input repaint should not require a new document content revision"
    );
    let changed_content_with_stale_revision = NativeRenderHookResult {
        content_changed: true,
        ..same_content
    };
    assert!(
        changed_content_with_stale_revision
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                state.current_scheduler_reason,
                state.current_role_dirty_reason,
            )
            .is_ok(),
        "host-input retained/runtime repaint may present an existing content revision until frame and content revisions are split"
    );
    assert!(
        changed_content_with_stale_revision
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::LayoutChanged),
            )
            .is_ok(),
        "external retained layout repaint may present an existing content revision"
    );
    assert!(
        changed_content_with_stale_revision
            .validate_for_presented_revision_with_scheduler(
                state.dirty_revision,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            )
            .is_err(),
        "external runtime changes must still provide a current content revision"
    );
}

// test: scheduler_only_host_input_can_repaint_existing_content_revision
#[test]
fn scheduler_only_host_input_can_repaint_existing_content_revision() {
    let render = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 2,
        layout_revision: None,
        render_scene_revision: None,
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: false,
        role_dirty_reason: None,
    };

    assert!(
        render
            .validate_for_presented_revision_with_scheduler(
                3,
                Some(NativeSchedulerReason::HostInput),
                None,
            )
            .is_ok(),
        "focus/activation/mouse movement can repaint without semantic content changes"
    );
    assert_eq!(
        render.presented_content_revision(3, Some(NativeSchedulerReason::HostInput), None),
        2
    );
    let changed_host_input_content = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 2,
        layout_revision: None,
        render_scene_revision: None,
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: true,
        role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
    };
    assert!(
        changed_host_input_content
            .validate_for_presented_revision_with_scheduler(
                3,
                Some(NativeSchedulerReason::HostInput),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            )
            .is_ok(),
        "host-input retained/runtime repaint may present an existing content revision until frame and content revisions are split"
    );
    let changed_external_content = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 2,
        layout_revision: None,
        render_scene_revision: None,
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: true,
        role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
    };
    assert!(
        changed_external_content
            .validate_for_presented_revision_with_scheduler(
                3,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            )
            .is_err(),
        "external runtime input must not present stale semantic content"
    );
}

// test: input_cursor_accepts_events_only_after_role_update
#[test]
fn input_cursor_accepts_events_only_after_role_update() {
    let mut cursor = NativeInputCursor::default();
    let input = NativeInputAdapterProof {
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        keyboard_events: vec![NativeKeyboardEventProof {
            sequence: 11,
            key: "A".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
        }],
        mouse_scroll_event_count: 3,
        scroll_delta_x: 4.0,
        scroll_delta_y: 8.0,
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };

    assert_eq!(cursor.last_mouse_button_sequence, 0);
    cursor.accept(&input);

    assert_eq!(cursor.last_mouse_button_sequence, 7);
    assert_eq!(cursor.last_keyboard_sequence, 11);
    assert_eq!(cursor.last_mouse_scroll_event_count, 3);
}

// test: button_press_only_input_delta_is_coalescible
#[test]
fn button_press_only_input_delta_is_coalescible() {
    let press_only = NativeInputAdapterProof {
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let click_pair = NativeInputAdapterProof {
        mouse_button_events: vec![
            NativeMouseButtonEventProof {
                sequence: 7,
                button: "left".to_owned(),
                pressed: true,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            },
            NativeMouseButtonEventProof {
                sequence: 8,
                button: "left".to_owned(),
                pressed: false,
                window_protocol_id: Some(42),
                event_elapsed_ms: None,
            },
        ],
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };

    assert!(native_input_delta_is_button_press_only(&press_only));
    assert!(!native_input_delta_is_button_press_only(&click_pair));
}

// test: clean_press_only_poll_does_not_accept_input_cursor
#[test]
fn clean_press_only_poll_does_not_accept_input_cursor() {
    let press_only = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let clean_poll = NativePollResult::clean(0);

    assert!(
        !should_accept_input_cursor_after_poll(true, &press_only, &clean_poll),
        "a clean role poll must not consume the only press edge before source input accepts it"
    );
}

// test: dirty_press_only_poll_accepts_input_cursor
#[test]
fn dirty_press_only_poll_accepts_input_cursor() {
    let press_only = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let dirty_poll = NativePollResult {
        dirty: true,
        scheduler_reason: Some(NativeSchedulerReason::HostInput),
        ..NativePollResult::clean(1)
    };

    assert!(
        should_accept_input_cursor_after_poll(true, &press_only, &dirty_poll),
        "once a role accepts a press edge, the native cursor can advance"
    );
}

// test: raw_wake_without_input_delta_is_not_reportable_host_input
#[test]
fn raw_wake_without_input_delta_is_not_reportable_host_input() {
    let raw_wake = NativeInputAdapterProof {
        real_os_events_observed: false,
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let press = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let motion = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_motion_event_count: 3,
        mouse_window_pos: Some(NativeMouseWindowPosition {
            x: 10.0,
            y: 20.0,
            window_width: 640.0,
            window_height: 480.0,
        }),
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };

    assert!(!native_input_delta_has_reportable_host_event(&raw_wake));
    assert!(native_input_delta_has_reportable_host_event(&press));
    assert!(native_input_delta_has_reportable_host_event(&motion));
}

// test: pointer_motion_only_input_delta_can_yield_to_newer_input
#[test]
fn pointer_motion_only_input_delta_can_yield_to_newer_input() {
    let motion_only = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_motion_event_count: 3,
        mouse_window_pos: Some(NativeMouseWindowPosition {
            x: 10.0,
            y: 20.0,
            window_width: 640.0,
            window_height: 480.0,
        }),
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let button_delta = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_button_events: vec![NativeMouseButtonEventProof {
            sequence: 7,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(42),
            event_elapsed_ms: None,
        }],
        mouse_window_pos: motion_only.mouse_window_pos.clone(),
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };
    let scroll_delta = NativeInputAdapterProof {
        real_os_events_observed: true,
        mouse_window_pos: motion_only.mouse_window_pos.clone(),
        scroll_delta_y: 120.0,
        ..empty_input_adapter_proof(false, NativeSyntheticInputProbeKind::Full)
    };

    assert!(native_input_delta_is_pointer_motion_only(&motion_only));
    assert!(!native_input_delta_is_pointer_motion_only(&button_delta));
    assert!(!native_input_delta_is_pointer_motion_only(&scroll_delta));
}

