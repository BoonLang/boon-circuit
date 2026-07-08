// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn render_loop_report_bytes_replace_existing_file_atomically() {
    let dir = std::env::temp_dir().join(format!(
        "boon-native-report-atomic-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.json");

    write_atomic_report_bytes(&path, br#"{"old":true}"#).unwrap();
    write_atomic_report_bytes(&path, br#"{"new":true}"#).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"new":true}"#);
    let leftovers = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "atomic report writes must not leave temp files on success: {leftovers:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}


#[test]
fn async_render_loop_report_writer_flushes_latest_report_on_shutdown() {
    let dir = std::env::temp_dir().join(format!(
        "boon-native-report-async-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.json");
    let writer = AsyncRenderLoopReportWriter::new();

    writer.enqueue(
        test_render_loop_report_snapshot(&path, 1, None),
        Instant::now(),
    );
    writer.enqueue(
        test_render_loop_report_snapshot(
            &path,
            7,
            Some(AsyncRenderLoopReportStats {
                enqueued_count: 2,
                ..AsyncRenderLoopReportStats::default()
            }),
        ),
        Instant::now(),
    );
    let stats = writer.shutdown();

    assert!(
        stats.completed_count >= 1,
        "async report writer should flush at least the latest pending report: {stats:?}"
    );
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(report["status"], "pass");
    assert_eq!(report["rendered_frame_count"], 7);
    assert_eq!(
        report["render_loop_report_write_mode"],
        "async_latest_wins_atomic_replace"
    );
    assert_eq!(report["render_loop_report_async_enqueued_count"], 2);
    std::fs::remove_dir_all(&dir).unwrap();
}


#[test]
fn frame_evidence_key_can_be_preissued_before_present() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented_with_revisions(7, 42, 43, 44);
    let surface_id = SurfaceId("surface-test".to_owned());

    let preissued = frame_evidence_key_for_next_presented_frame_with_revisions(
        &state,
        &surface_id,
        9,
        Some(5),
        Some(12),
        52,
        53,
        54,
    );
    state.note_preissued_frame_evidence_key(preissued.clone(), 3.5);

    assert_eq!(preissued.frame_seq, 2);
    assert_eq!(preissued.present_id, 2);
    assert_eq!(preissued.content_revision, 52);
    assert_eq!(preissued.layout_revision, 53);
    assert_eq!(preissued.render_scene_revision, 54);
    assert_eq!(
        state.last_preissued_frame_evidence_key.as_ref(),
        Some(&preissued)
    );
    assert_eq!(state.last_preissued_frame_evidence_elapsed_ms, Some(3.5));
    assert!(state.last_frame_evidence_key_issued_before_present);

    state.mark_presented_with_revisions(8, 52, 53, 54);

    let presented =
        frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(5), Some(12));
    assert_eq!(presented, preissued);
}


#[test]
fn surface_lifecycle_reconfigure_increments_epoch_and_records_reason() {
    let mut lifecycle = NativeSurfaceLifecycleState::new(800, 600);
    assert_eq!(lifecycle.epoch(), 1);

    lifecycle.reconfigured("resize", 1024, 768);

    assert_eq!(lifecycle.epoch(), 2);
    assert_eq!(lifecycle.report().resize_reconfigure_count, 1);
    assert_eq!(lifecycle.report().final_width, 1024);
    assert_eq!(lifecycle.report().final_height, 768);
    assert_eq!(
        lifecycle.report().last_lifecycle_event.as_deref(),
        Some("resize")
    );
}


#[test]
fn surface_lifecycle_skips_nonpresentable_frames_without_epoch_commit() {
    let mut lifecycle = NativeSurfaceLifecycleState::new(800, 600);

    lifecycle.note_timeout_skip();
    lifecycle.note_occluded_skip();
    lifecycle.note_zero_size_skip();

    assert_eq!(lifecycle.epoch(), 1);
    assert_eq!(lifecycle.report().timeout_skip_count, 1);
    assert_eq!(lifecycle.report().occluded_skip_count, 1);
    assert_eq!(lifecycle.report().zero_size_skip_count, 1);
    assert_eq!(
        lifecycle.report().last_lifecycle_event.as_deref(),
        Some("zero_size_skip")
    );
}
