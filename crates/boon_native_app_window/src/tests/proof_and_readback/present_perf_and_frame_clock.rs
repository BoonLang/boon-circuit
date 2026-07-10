// Included by `../proof_and_readback.rs`.

// test: background_telemetry_yields_to_product_and_burst_frames
#[test]
fn background_telemetry_yields_to_product_and_burst_frames() {
    assert!(!post_present_background_telemetry_allowed(
        false,
        false,
        false,
        Some(NativeFrameLane::RuntimeOrLayout),
        Some(NativeSchedulerReason::ExternalWake),
    ));
    assert!(!post_present_background_telemetry_allowed(
        true,
        true,
        true,
        Some(NativeFrameLane::ProductInteraction),
        Some(NativeSchedulerReason::HostInput),
    ));
    assert!(!post_present_background_telemetry_allowed(
        true,
        false,
        true,
        Some(NativeFrameLane::AnimationFollowup),
        Some(NativeSchedulerReason::RequestedAnimation),
    ));
    assert!(post_present_background_telemetry_allowed(
        true,
        false,
        true,
        Some(NativeFrameLane::ProofOrHarness),
        Some(NativeSchedulerReason::VerifierFrame),
    ));
    assert!(post_present_background_telemetry_allowed(
        true,
        false,
        false,
        Some(NativeFrameLane::RuntimeOrLayout),
        Some(NativeSchedulerReason::ExternalWake),
    ));
}

// test: present_path_mode_selection_is_explicit_and_generic
#[test]
fn present_path_mode_selection_is_explicit_and_generic() {
    let direct = select_native_present_path_mode(
        true,
        true,
        NativePresentPathMode::DirectVisibleSurface,
        false,
    );
    assert_eq!(
        direct.selected_mode,
        NativePresentPathMode::DirectVisibleSurface
    );
    assert_eq!(direct.reason, "default_direct_visible_surface");

    let direct_with_readback = select_native_present_path_mode(
        true,
        true,
        NativePresentPathMode::DirectVisibleSurface,
        true,
    );
    assert_eq!(
        direct_with_readback.selected_mode,
        NativePresentPathMode::DirectVisibleSurface,
        "offscreen copy-to-present remains explicit because it can regress compositor present latency"
    );
    assert_eq!(
        direct_with_readback.reason,
        "default_direct_visible_surface_with_separate_readback"
    );

    let direct_readback_without_copy_dst = select_native_present_path_mode(
        true,
        false,
        NativePresentPathMode::DirectVisibleSurface,
        true,
    );
    assert_eq!(
        direct_readback_without_copy_dst.selected_mode,
        NativePresentPathMode::DirectVisibleSurface
    );
    assert_eq!(
        direct_readback_without_copy_dst.reason,
        "default_direct_visible_surface_with_separate_readback"
    );

    let direct_readback_without_hook = select_native_present_path_mode(
        false,
        true,
        NativePresentPathMode::DirectVisibleSurface,
        true,
    );
    assert_eq!(
        direct_readback_without_hook.selected_mode,
        NativePresentPathMode::DirectVisibleSurface
    );
    assert_eq!(
        direct_readback_without_hook.reason,
        "default_direct_visible_surface_with_separate_readback"
    );

    let no_hook = select_native_present_path_mode(
        false,
        true,
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        false,
    );
    assert_eq!(
        no_hook.selected_mode,
        NativePresentPathMode::DirectVisibleSurface
    );
    assert_eq!(
        no_hook.reason,
        "offscreen_copy_requested_without_render_hook"
    );

    let no_copy_support = select_native_present_path_mode(
        true,
        false,
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        false,
    );
    assert_eq!(
        no_copy_support.selected_mode,
        NativePresentPathMode::DirectVisibleSurface
    );
    assert_eq!(
        no_copy_support.reason,
        "offscreen_copy_requested_without_surface_copy_dst"
    );

    let offscreen = select_native_present_path_mode(
        true,
        true,
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        false,
    );
    assert_eq!(
        offscreen.selected_mode,
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent
    );
    assert_eq!(offscreen.reason, "explicit_offscreen_copy_to_present");
    assert_eq!(
        offscreen.selected_mode.render_target_kind(),
        "app-owned-offscreen-copy-to-present"
    );
}

// test: present_path_selection_is_recorded_in_render_loop_state
#[test]
fn present_path_selection_is_recorded_in_render_loop_state() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let selection = select_native_present_path_mode(
        true,
        true,
        NativePresentPathMode::AppOwnedOffscreenCopyToPresent,
        true,
    );

    state.note_present_path_selection(selection);

    assert_eq!(
        state.last_present_path_requested_mode,
        Some(NativePresentPathMode::AppOwnedOffscreenCopyToPresent)
    );
    assert_eq!(
        state.last_present_path_mode,
        Some(NativePresentPathMode::AppOwnedOffscreenCopyToPresent)
    );
    assert_eq!(
        state.last_present_path_selection_reason.as_deref(),
        Some("explicit_offscreen_copy_to_present")
    );
    assert!(state.last_present_path_hooks_present);
    assert!(state.last_present_path_surface_copy_to_present_supported);
    assert!(state.last_present_path_readback_enabled);
}

// test: preview_perf_stats_keep_proof_overhead_separate_from_ux_latency
#[test]
fn preview_perf_stats_keep_proof_overhead_separate_from_ux_latency() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_surface_acquired(4.0);
    state.note_surface_acquire_call(0.25);
    state.note_render_hook_completed(6.5);
    state.note_present_completed(12.0);
    state.note_submit_phase_durations(0.25, 0.125, 1.5);
    state.mark_presented_with_content(2, 5);
    let render_metrics = NativeRenderFrameMetrics {
        layout_ms: Some(0.7),
        upload_bytes: Some(4096),
        draw_call_count: Some(12),
        glyph_cache_hit_rate: Some(0.75),
        materialized_item_count: Some(42),
        visible_display_item_count: Some(40),
        queue_write_count: Some(3),
        preview_blocked_on_ipc_count: Some(0),
        render_hook_outer_state_snapshot_ms: None,
        render_hook_outer_input_snapshot_ms: None,
        render_hook_outer_core_ms: None,
        render_hook_outer_revision_ms: None,
        render_hook_outer_total_ms: None,
        render_hook_phase_timings: None,
        product_result: None,
    };
    state.note_render_frame_metrics(Some(render_metrics.clone()));
    let key = frame_evidence_key_for_presented_frame(
        &state,
        &SurfaceId("surface-test".to_owned()),
        1,
        Some(8),
        None,
    );
    let mut accumulator = NativePreviewPerfAccumulator::default();
    accumulator.record(
        Some(2.5),
        Some(&render_metrics),
        Some(1.5),
        state.last_surface_acquire_call_ms,
        state.last_queue_submit_call_ms,
        state.last_present_path_ms,
        Some(8.0),
        Some(NativeFrameLane::ProductInteraction),
        Some(24.0),
    );
    let adapter_identity = NativeAdapterIdentity {
        adapter_name: "test-gpu".to_owned(),
        adapter_backend: "Vulkan".to_owned(),
        adapter_device: 1,
        adapter_vendor: 2,
        adapter_device_type: "DiscreteGpu".to_owned(),
        adapter_is_software: false,
    };

    let stats = native_preview_perf_stats_snapshot(
        NativeWindowRole::Preview,
        &state,
        adapter_identity.clone(),
        NativeAdapterPolicy::AllowSoftwareDiagnostic,
        Duration::from_millis(120),
        60.0,
        &accumulator,
        Some(8.0),
        Some(NativeFrameLane::ProductInteraction),
        "readback",
        Some(24.0),
        Some(key.clone()),
    );

    assert_eq!(stats.render_loop_mode, NativeRenderLoopMode::DemandDriven);
    assert_eq!(stats.adapter_identity, adapter_identity);
    assert_eq!(stats.input_to_present_ms, Some(8.0));
    assert_eq!(stats.frame_lane, Some(NativeFrameLane::ProductInteraction));
    assert_eq!(stats.product_input_to_present_ms, Some(8.0));
    assert_eq!(stats.proof_overhead_ms, Some(24.0));
    assert_eq!(stats.render_hook_ms, Some(2.5));
    assert_eq!(stats.present_call_ms, Some(1.5));
    assert_eq!(stats.frame_present_call_ms, Some(1.5));
    assert_eq!(stats.surface_acquire_call_ms, Some(0.25));
    assert_eq!(stats.queue_submit_call_ms, Some(0.125));
    assert_eq!(stats.present_path_ms, Some(1.875));
    assert_eq!(stats.input_to_present_ms_p50_p95_p99_max.p95, Some(8.0));
    assert_eq!(
        stats.product_input_to_present_ms_p50_p95_p99_max.p95,
        Some(8.0)
    );
    assert_eq!(stats.product_frame_count, 1);
    assert_eq!(stats.product_missed_frame_count, 0);
    assert_eq!(stats.render_hook_ms_p50_p95_p99_max.sample_count, 1);
    assert_eq!(
        stats.surface_acquire_call_ms_p50_p95_p99_max.p95,
        Some(0.25)
    );
    assert_eq!(stats.queue_submit_call_ms_p50_p95_p99_max.p95, Some(0.125));
    assert_eq!(stats.present_path_ms_p50_p95_p99_max.p95, Some(1.875));
    assert_eq!(stats.layout_ms_p50_p95_p99_max.p95, Some(0.7));
    assert_eq!(stats.upload_bytes_p50_p95_max.max, Some(4096.0));
    assert_eq!(stats.draw_call_count_p50_p95_max.max, Some(12.0));
    assert_eq!(stats.glyph_cache_hit_rate, Some(0.75));
    assert_eq!(stats.materialized_item_count, Some(42));
    assert_eq!(stats.materialized_item_count_p50_p95_max.p95, Some(42.0));
    assert_eq!(stats.proof_overhead_ms_p50_p95_max.max, Some(24.0));
    assert_eq!(stats.frame_evidence_key, Some(key));
}

// test: preview_perf_accumulator_keeps_bounded_rolling_summaries
#[test]
fn preview_perf_accumulator_keeps_bounded_rolling_summaries() {
    let mut accumulator = NativePreviewPerfAccumulator::default();
    for value in 0..(PREVIEW_PERF_STATS_WINDOW + 10) {
        let render_metrics = NativeRenderFrameMetrics {
            layout_ms: Some((value * 4) as f64),
            upload_bytes: Some((value * 5) as u64),
            draw_call_count: Some((value * 6) as u64),
            glyph_cache_hit_rate: Some(0.5),
            materialized_item_count: Some((value * 7) as u64),
            visible_display_item_count: None,
            queue_write_count: None,
            preview_blocked_on_ipc_count: None,
            render_hook_outer_state_snapshot_ms: None,
            render_hook_outer_input_snapshot_ms: None,
            render_hook_outer_core_ms: None,
            render_hook_outer_revision_ms: None,
            render_hook_outer_total_ms: None,
            render_hook_phase_timings: None,
            product_result: None,
        };
        accumulator.record(
            Some(value as f64),
            Some(&render_metrics),
            Some((value * 2) as f64),
            Some((value * 8) as f64),
            Some((value * 9) as f64),
            Some((value * 19) as f64),
            Some((value * 3) as f64),
            None,
            None,
        );
    }

    let input = accumulator.input_to_present_summary();
    let render = accumulator.render_hook_summary();

    assert_eq!(input.sample_count, PREVIEW_PERF_STATS_WINDOW);
    assert_eq!(render.sample_count, PREVIEW_PERF_STATS_WINDOW);
    assert_eq!(render.p50, Some(70.0));
    assert_eq!(render.max, Some(129.0));
    assert_eq!(input.max, Some(387.0));
    assert_eq!(accumulator.surface_acquire_call_summary().max, Some(1032.0));
    assert_eq!(accumulator.queue_submit_call_summary().max, Some(1161.0));
    assert_eq!(accumulator.present_path_summary().max, Some(2451.0));
    assert_eq!(accumulator.layout_summary().max, Some(516.0));
    assert_eq!(accumulator.upload_bytes_summary().max, Some(645.0));
    assert_eq!(
        accumulator.materialized_item_count_summary().max,
        Some(903.0)
    );
    assert_eq!(accumulator.proof_overhead_summary().sample_count, 0);
}

// test: native_frame_clock_product_frame_forbids_proof_and_background_telemetry
#[test]
fn native_frame_clock_product_frame_forbids_proof_and_background_telemetry() {
    let policy = NativeFrameClock::policy(
        Some(NativeFrameLane::ProductInteraction),
        Some(NativeSchedulerReason::HostInput),
        true,
        true,
        Some(true),
    );

    assert_eq!(policy.owner, "native_frame_clock");
    assert_eq!(policy.frame_lane, Some(NativeFrameLane::ProductInteraction));
    assert!(policy.product_input_frame);
    assert!(!policy.pre_submit_proof_poll_allowed);
    assert_eq!(
        policy.post_present_background_telemetry_allowed,
        Some(false)
    );
}

// test: native_frame_clock_proof_frame_allows_required_proof_without_background_guessing
#[test]
fn native_frame_clock_proof_frame_allows_required_proof_without_background_guessing() {
    let policy = NativeFrameClock::policy(
        Some(NativeFrameLane::ProofOrHarness),
        Some(NativeSchedulerReason::VerifierFrame),
        false,
        true,
        Some(true),
    );

    assert_eq!(policy.owner, "native_frame_clock");
    assert_eq!(policy.frame_lane, Some(NativeFrameLane::ProofOrHarness));
    assert!(!policy.product_input_frame);
    assert!(policy.pre_submit_proof_poll_allowed);
    assert_eq!(policy.post_present_background_telemetry_allowed, Some(true));
}
