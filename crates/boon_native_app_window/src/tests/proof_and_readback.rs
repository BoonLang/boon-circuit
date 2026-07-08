// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

























































fn test_render_loop_report_snapshot(
    path: &Path,
    rendered_frame_count: u64,
    writer_stats: Option<AsyncRenderLoopReportStats>,
) -> NativeRenderLoopReportSnapshot {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.dirty_revision = rendered_frame_count;
    state.presented_revision = rendered_frame_count;
    state.rendered_frame_count = rendered_frame_count;
    state.last_render_content_revision = rendered_frame_count;
    state.last_render_layout_revision = rendered_frame_count;
    state.last_render_scene_revision = rendered_frame_count;
    state.last_present_call_ms = Some(1.0);
    let mut extras = NativeRenderLoopReportExtras {
        present_mode: "Immediate".to_owned(),
        surface_format: "Bgra8Unorm".to_owned(),
        desired_maximum_frame_latency: 1,
        desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
        ..NativeRenderLoopReportExtras::default()
    };
    extras = extras.with_report_writer_stats(writer_stats);
    let mut perf_accumulator = NativePreviewPerfAccumulator::default();
    perf_accumulator.record(
        None,
        None,
        state.last_present_call_ms,
        state.last_surface_acquire_call_ms,
        state.last_queue_submit_call_ms,
        state.last_present_path_ms,
        None,
        None,
        None,
    );
    render_loop_report_snapshot(
        path,
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
        extras,
        None,
    )
}

// Nested behavior-area shards keep broad test groups navigable without widening production APIs.
include!("proof_and_readback/input_history_and_timing.rs");
include!("proof_and_readback/present_perf_and_frame_clock.rs");
include!("proof_and_readback/proof_queue_and_worker.rs");
include!("proof_and_readback/readback_registry.rs");
include!("proof_and_readback/scheduler_revisions.rs");
