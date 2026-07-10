// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn demand_driven_scheduler_renders_first_dirty_revision_once() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();

    assert!(state.should_render(now, false));
    state.mark_presented(state.dirty_revision);

    assert!(!state.should_render(now, false));
    state.note_idle_poll();
    assert_eq!(state.rendered_frame_count, 1);
    assert_eq!(state.skipped_idle_poll_count, 1);
}


#[test]
fn demand_driven_idle_wait_has_no_poll_deadline_without_scheduled_work() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    assert_eq!(state.scheduled_wait_timeout(now), None);

    state.schedule_wake_after(now, Duration::from_millis(30));
    assert_eq!(
        state.scheduled_wait_timeout(now),
        Some(Duration::from_millis(30))
    );

    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);
    state.schedule_wake_after(now, Duration::from_millis(4));
    assert_eq!(
        state.scheduled_wait_timeout(now),
        Some(Duration::from_millis(4))
    );
}


#[test]
fn demand_driven_idle_gap_before_host_input_is_not_a_missed_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(10.0);

    state.mark_dirty(NativeSchedulerReason::HostInput, None);
    state.note_present_completed(160.0);

    assert_eq!(
        state.last_present_interval_ms,
        Some(150.0),
        "DemandDriven still reports the idle gap for diagnostics"
    );
    assert!(
        state.last_frame_lateness_ms.unwrap_or_default() > NATIVE_TARGET_FRAME_INTERVAL_MS,
        "the interval is late relative to continuous pacing, but DemandDriven idle is allowed"
    );
    assert_eq!(
        state.missed_frame_count, 0,
        "a long healthy DemandDriven idle gap before new host input is not a dropped frame"
    );
    assert_eq!(state.last_missed_frame_cause, None);
}


#[test]
fn requested_animation_followup_gap_counts_as_missed_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(10.0);

    state.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
    state.note_present_completed(60.0);

    assert_eq!(state.missed_frame_count, 1);
    assert_eq!(
        state.last_missed_frame_cause.as_deref(),
        Some("requested_animation_present_interval_exceeded_two_frames")
    );
}


#[test]
fn continuous_probe_long_present_gap_counts_as_missed_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::ContinuousProbe);
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(10.0);
    state.note_present_completed(60.0);

    assert_eq!(state.missed_frame_count, 1);
    assert_eq!(
        state.last_missed_frame_cause.as_deref(),
        Some("continuous_probe_present_interval_exceeded_two_frames")
    );
}


#[test]
fn demand_driven_idle_wait_reports_timeout_and_wake_reason() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

    state.note_idle_wait(Duration::from_millis(30), Duration::from_millis(12), 4, 4);
    assert_eq!(state.idle_wait_count, 1);
    assert_eq!(state.idle_wait_total_ms, 12);
    assert_eq!(state.last_idle_wait_timeout_ms, 30);
    assert_eq!(state.last_idle_wait_actual_ms, 12);
    assert_eq!(state.last_idle_wait_wake_reason.as_deref(), Some("timeout"));

    state.note_idle_wait(Duration::from_millis(100), Duration::from_millis(3), 4, 5);
    assert_eq!(state.idle_wait_count, 2);
    assert_eq!(state.idle_wait_total_ms, 15);
    assert_eq!(
        state.last_idle_wait_wake_reason.as_deref(),
        Some("external_wake")
    );
}


#[test]
fn input_event_wake_elapsed_ms_uses_generation_timeline() {
    let hold_started = Instant::now();
    let timeline = Arc::new(Mutex::new(VecDeque::from([
        (1, hold_started + Duration::from_millis(3)),
        (2, hold_started + Duration::from_millis(7)),
    ])));

    assert_eq!(
        input_event_wake_elapsed_ms_for_generation(&timeline, 2, hold_started),
        Some(7.0)
    );
    assert_eq!(
        input_event_wake_elapsed_ms_for_generation(&timeline, 3, hold_started),
        None
    );
}


#[test]
fn accepted_host_input_timing_defines_product_input_to_present_latency() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let host_input_event = NativeHostInputEventSummary {
        scope: "accepted_host_input_delta".to_owned(),
        kind: "mouse_button".to_owned(),
        source_intent: None,
        sequence: Some(17),
        input_event_wake_count: 3,
        wake_elapsed_ms: Some(8.0),
        raw_wake_elapsed_ms: Some(8.0),
        event_elapsed_ms: Some(8.0),
        accepted_elapsed_ms: Some(20.0),
        wake_to_accept_ms: Some(12.0),
        event_to_accept_ms: Some(12.0),
        window_protocol_id: Some(99),
        button: Some("left".to_owned()),
        pressed: Some(false),
        key: None,
        mouse_button_delta_count: 1,
        keyboard_delta_count: 0,
        has_scroll_delta: false,
        has_motion_delta: false,
    };
    state.note_accepted_host_input(3, 20.0, false, Some(host_input_event.clone()));
    state.note_dirty_poll(21.0);
    state.note_render_started(22.0);
    state.note_surface_acquire_call(0.1);
    state.note_surface_acquired(23.0);
    state.note_render_hook_completed(24.0);
    state.note_queue_submitted(26.0);
    state.note_present_completed(28.0);
    state.note_submit_phase_durations(0.2, 0.3, 1.0);

    let raw_wake_elapsed_ms = Some(8.0);
    let raw_wake_to_present_ms =
        elapsed_delta_ms(raw_wake_elapsed_ms, state.last_present_completed_elapsed_ms);
    state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
    let accepted_input_to_present_ms =
        state.take_frame_accepted_input_to_present_ms(state.current_frame_input_event_seq(3));
    let mut accumulator = NativePreviewPerfAccumulator::default();
    accumulator.record(
        None,
        None,
        None,
        None,
        None,
        None,
        accepted_input_to_present_ms,
        Some(NativeFrameLane::ProductInteraction),
        None,
    );
    let stats = native_preview_perf_stats_snapshot(
        NativeWindowRole::Preview,
        &state,
        NativeAdapterIdentity::default(),
        NativeAdapterPolicy::AllowSoftwareDiagnostic,
        Duration::from_millis(64),
        60.0,
        &accumulator,
        accepted_input_to_present_ms,
        Some(NativeFrameLane::ProductInteraction),
        "off",
        None,
        None,
    );

    assert_eq!(raw_wake_to_present_ms, Some(20.0));
    assert_eq!(accepted_input_to_present_ms, Some(8.0));
    assert_eq!(
        stats.input_to_present_ms,
        Some(8.0),
        "product UX latency starts when the role poll hook accepts visible-changing host input, not at an earlier raw input wake"
    );
    assert_eq!(stats.input_to_present_ms_p50_p95_p99_max.p95, Some(8.0));
    assert_eq!(state.last_accepted_host_input_event_wake_count, 3);
    assert_eq!(state.last_input_to_present_accounted_event_wake_count, 3);
    assert_eq!(state.last_input_to_present_accounted_input_seq, 1);
    let accepted_timing = state
        .last_accounted_input_frame_timing
        .as_ref()
        .expect("accepted input timing should be captured once");
    assert_eq!(accepted_timing.input_event_seq, 1);
    assert_eq!(accepted_timing.input_event_source, "real_os");
    assert_eq!(accepted_timing.input_event_wake_count, 3);
    assert_eq!(
        accepted_timing.host_input_event.as_ref(),
        Some(&host_input_event)
    );
    assert_eq!(accepted_timing.input_wake_elapsed_ms, Some(8.0));
    assert_eq!(accepted_timing.input_wake_to_input_accept_ms, Some(12.0));
    assert_eq!(accepted_timing.input_wake_to_present_ms, Some(20.0));
    assert_eq!(accepted_timing.input_to_present_ms, 8.0);
    assert_eq!(accepted_timing.input_accept_to_dirty_poll_ms, Some(1.0));
    assert_eq!(accepted_timing.dirty_poll_to_render_started_ms, Some(1.0));
    assert_eq!(
        accepted_timing.render_started_to_render_hook_completed_ms,
        Some(2.0)
    );
    assert_eq!(
        accepted_timing.render_hook_completed_to_present_ms,
        Some(4.0)
    );
    assert_eq!(accepted_timing.queue_to_present_ms, Some(2.0));
    assert_eq!(accepted_timing.present_path_ms, Some(1.4));
    assert!(!state.last_accepted_host_input_press_only);

    let key = frame_evidence_key_for_presented_frame(
        &state,
        &SurfaceId("surface-test".to_owned()),
        1,
        Some(accepted_timing.input_event_seq),
        None,
    );
    let commit = product_frame_commit_for_presented_frame(
        &state,
        key,
        NativeAdapterIdentity::default(),
        NativeFrameLane::ProductInteraction,
        Some(NativeSchedulerReason::HostInput),
        None,
        Some(accepted_timing.clone()),
    );
    assert_eq!(commit.host_input_event.as_ref(), Some(&host_input_event));
    assert_eq!(
        commit
            .input_timing
            .as_ref()
            .and_then(|timing| timing.input_wake_to_present_ms),
        Some(20.0)
    );
}


#[test]
fn accepted_operator_host_input_publishes_product_timing_without_real_os_wake() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let hint = NativeHostInputEventHint {
        kind: "mouse_button".to_owned(),
        source_intent: Some("press".to_owned()),
        sequence: Some(1),
        window_protocol_id: Some(101),
        button: Some("left".to_owned()),
        pressed: Some(true),
        key: None,
        event_elapsed_ms: None,
    };
    let summary = accepted_operator_host_input_event_summary(12.0, Some(&hint));
    state.current_scheduler_reason = Some(NativeSchedulerReason::HostInput);
    state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
    state.note_accepted_operator_host_input(12.0, true, Some(summary.clone()));
    state.note_dirty_poll(12.2);
    state.note_render_started(12.4);
    state.note_surface_acquired(12.7);
    state.note_render_hook_completed(13.1);
    state.note_queue_submitted(13.7);
    state.note_present_completed(14.3);

    let input_event_seq = state.current_frame_input_event_seq(0);
    assert_eq!(input_event_seq, Some(1));
    let timing = state
        .take_frame_accepted_input_timing(input_event_seq)
        .expect("operator host input should define product timing");

    assert_eq!(timing.timing_scope, "accepted_operator_host_input_frame");
    assert_eq!(timing.input_event_seq, 1);
    assert_eq!(timing.input_event_source, "operator_host");
    assert_eq!(timing.input_event_wake_count, 0);
    assert!((timing.input_to_present_ms - 2.3).abs() < 1.0e-9);
    assert_eq!(timing.host_input_event.as_ref(), Some(&summary));
    assert_eq!(state.last_input_to_present_accounted_input_seq, 1);
    assert_eq!(state.last_input_to_present_accounted_event_wake_count, 0);

    let key = frame_evidence_key_for_presented_frame(
        &state,
        &SurfaceId("surface-test".to_owned()),
        1,
        Some(timing.input_event_seq),
        None,
    );
    assert_eq!(key.input_event_seq, Some(1));
    let policy = NativeFrameClock::product_commit_policy(
        NativeFrameLane::ProductInteraction,
        true,
        Some(&timing),
    );
    assert!(policy.publish_product_commit);
}


#[test]
fn accepted_host_input_latency_is_frame_scoped_and_single_use() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_accepted_host_input(3, 20.0, false, None);
    state.note_present_completed(27.5);

    assert_eq!(state.take_frame_accepted_input_to_present_ms(Some(2)), None);
    assert_eq!(
        state.take_frame_accepted_input_to_present_ms(Some(1)),
        Some(7.5)
    );
    assert_eq!(state.take_frame_accepted_input_to_present_ms(Some(1)), None);

    state.note_present_completed(44.0);
    assert_eq!(
        state.take_frame_accepted_input_to_present_ms(Some(1)),
        None,
        "later frames must not keep reusing the old accepted input timestamp"
    );

    state.note_accepted_host_input(4, 45.0, false, None);
    state.note_present_completed(50.0);
    assert_eq!(
        state.take_frame_accepted_input_to_present_ms(Some(2)),
        Some(5.0)
    );
    assert_eq!(state.last_input_to_present_accounted_event_wake_count, 4);
}


#[test]
fn accepted_host_input_timing_owns_lane_during_requested_animation_burst() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.current_scheduler_reason = Some(NativeSchedulerReason::RequestedAnimation);
    state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);
    state.note_accepted_host_input(7, 10.0, false, None);
    state.note_dirty_poll(10.5);
    state.note_render_started(11.0);
    state.note_surface_acquired(11.5);
    state.note_render_hook_completed(12.0);
    state.note_queue_submitted(13.0);
    state.note_present_completed(14.0);

    let timing = state
        .take_frame_accepted_input_timing(Some(1))
        .expect("accepted host input should define product timing");

    assert_eq!(timing.frame_lane, NativeFrameLane::ProductInteraction);
    assert_eq!(
        timing.scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(timing.input_to_present_ms, 4.0);
}


#[test]
fn demand_driven_scheduler_wakes_for_surface_change() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);

    let dirty = state.mark_dirty(NativeSchedulerReason::SurfaceChanged, None);

    assert_eq!(dirty, 2);
    assert!(state.should_render(Instant::now(), false));
    state.mark_presented(dirty);
    assert_eq!(state.presented_revision, dirty);
    assert_eq!(
        state.last_scheduler_reason,
        Some(NativeSchedulerReason::SurfaceChanged)
    );
}


#[test]
fn requested_animation_burst_is_bounded_inside_demand_driven_mode() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);

    assert_eq!(
        native_frame_pacing_snapshot(&state).state,
        NativeFramePacingState::RequestedAnimationBurst
    );
    assert_eq!(
        state.requested_animation_burst_frames_remaining,
        REQUESTED_ANIMATION_BURST_MIN_FRAMES
    );
    assert!(!state.should_render(now, false));

    let due = now + Duration::from_millis(17);
    assert!(state.consume_due_wake(due));
    assert_eq!(
        state.last_scheduler_reason,
        Some(NativeSchedulerReason::RequestedAnimation)
    );
    assert!(state.should_render(due, false));

    let dirty = state.dirty_revision;
    state.mark_presented(dirty);
    state.note_present_completed(27.0);
    state.schedule_requested_animation_followup(due, 27.0);
    assert_eq!(state.requested_animation_burst_frames_remaining, 1);
    assert!(state.next_wake_at.is_some());

    let second_due = due + Duration::from_millis(17);
    assert!(state.consume_due_wake(second_due));
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(44.0);
    state.clear_requested_animation_burst_if_quiet(200.0);
    assert_eq!(
        native_frame_pacing_snapshot(&state).state,
        NativeFramePacingState::Idle
    );
}


#[test]
fn native_frame_clock_product_commit_policy_requires_accepted_product_input() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_accepted_host_input(7, 10.0, false, None);
    state.note_dirty_poll(11.0);
    state.note_render_started(12.0);
    state.note_surface_acquired(13.0);
    state.note_render_hook_completed(14.0);
    state.note_queue_submitted(15.0);
    state.note_present_completed(16.0);
    state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
    let timing = state
        .take_frame_accepted_input_timing(Some(1))
        .expect("accepted input timing");

    let product_policy = NativeFrameClock::product_commit_policy(
        NativeFrameLane::ProductInteraction,
        true,
        Some(&timing),
    );
    assert!(product_policy.publish_product_commit);
    assert_eq!(product_policy.reason, "accepted_product_interaction_frame");

    let animation_policy = NativeFrameClock::product_commit_policy(
        NativeFrameLane::AnimationFollowup,
        false,
        Some(&timing),
    );
    assert!(!animation_policy.publish_product_commit);
    assert_eq!(animation_policy.reason, "non_product_frame_lane");

    let missing_timing_policy =
        NativeFrameClock::product_commit_policy(NativeFrameLane::ProductInteraction, true, None);
    assert!(!missing_timing_policy.publish_product_commit);
    assert_eq!(
        missing_timing_policy.reason,
        "missing_accepted_input_timing"
    );
}


#[test]
fn requested_animation_burst_paces_until_quiet_interval_expires() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
    assert!(!state.consume_due_wake_after_poll(now));
    assert!(!state.should_render(now, false));
    let due = now + native_target_frame_interval_duration();
    assert!(state.consume_due_wake(due));
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(12.0);
    state.schedule_requested_animation_followup(due, 12.0);
    assert!(state.next_wake_at.is_some());

    let next_due = due + native_target_frame_interval_duration();
    assert!(state.consume_due_wake(next_due));
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(29.0);
    state.schedule_requested_animation_followup(next_due, 29.0);
    assert!(
        state.next_wake_at.is_some(),
        "the burst should keep pacing frames during the quiet window after min frames are consumed"
    );

    state.clear_requested_animation_burst_if_quiet(200.0);
    assert_eq!(
        native_frame_pacing_snapshot(&state).state,
        NativeFramePacingState::Idle
    );
}


#[test]
fn requested_animation_followup_uses_frame_start_deadline_after_blocking_present() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.mark_dirty(NativeSchedulerReason::RequestedAnimation, None);
    state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
    assert!(!state.consume_due_wake_after_poll(now));
    state.note_render_started(10.0);
    state.mark_presented(state.dirty_revision);
    state.note_present_completed(25.0);

    let present_return = now + Duration::from_millis(25);
    state.schedule_requested_animation_followup(present_return, 25.0);
    let next_wake = state
        .next_wake_at
        .expect("burst follow-up should schedule a wake");

    assert!(
        next_wake <= present_return + Duration::from_millis(2),
        "follow-up should target the next frame deadline from render start, not present return plus a full interval"
    );
}


#[test]
fn host_input_product_frame_starts_bounded_followup_burst() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.mark_dirty(NativeSchedulerReason::HostInput, None);
    state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);

    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(
        state.requested_animation_burst_frames_remaining,
        REQUESTED_ANIMATION_BURST_MIN_FRAMES
    );
    assert!(
        state.next_wake_at.is_some_and(|wake_at| wake_at > now),
        "visible-changing host input should keep a short paced burst hot after the current product frame"
    );
}


#[test]
fn pointer_motion_prewarm_schedules_hot_poll_without_dirtying_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);
    let presented = state.presented_revision;

    state.request_interactive_prewarm_burst(now, 10.0);

    assert_eq!(state.dirty_revision, presented);
    assert_eq!(state.current_scheduler_reason, None);
    assert_eq!(state.current_frame_lane, None);
    assert!(!state.should_render(now, false));
    assert_eq!(state.requested_animation_prewarm_count, 1);
    assert_eq!(state.armed_frame_token, 1);
    assert!(state.armed_frame_pending);
    assert_eq!(state.armed_frame_started_elapsed_ms, None);
    assert_eq!(
        state.requested_animation_burst_frames_remaining,
        REQUESTED_ANIMATION_BURST_MIN_FRAMES
    );

    let due = now + native_target_frame_interval_duration();
    assert!(state.consume_due_wake(due));
    assert_eq!(
        state.last_scheduler_reason,
        Some(NativeSchedulerReason::RequestedAnimation)
    );
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::RequestedAnimation)
    );
    assert_eq!(
        state.current_frame_lane,
        Some(NativeFrameLane::AnimationFollowup)
    );
    assert!(!state.should_render(due, false));
    assert_eq!(
        state.requested_animation_burst_frames_remaining,
        REQUESTED_ANIMATION_BURST_MIN_FRAMES
    );
    state.note_armed_frame_input_sampled(27.0);
    assert!(state.clean_armed_poll_pending());
    state.skip_clean_armed_burst_present(due, 27.0);
    assert_eq!(state.clean_armed_poll_count, 1);
    assert_eq!(state.skipped_clean_burst_present_count, 1);
}


#[test]
fn host_input_after_pointer_prewarm_preserves_product_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_interactive_prewarm_burst(now, 10.0);
    let prewarm_wake = state
        .next_wake_at
        .expect("pointer prewarm should schedule a delayed frame");

    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            role_dirty_reason: Some(NativeRoleDirtyReason::FocusChanged),
            frame_lane: Some(NativeFrameLane::ProductInteraction),
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        },
        true,
    );
    state.note_accepted_host_input(11, 12.0, false, None);
    assert_eq!(state.input_waited_for_already_armed_frame_count, 1);

    assert!(state.consume_due_wake_after_poll(prewarm_wake));
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(
        state.current_frame_lane,
        Some(NativeFrameLane::ProductInteraction)
    );
    assert!(state.should_render(prewarm_wake, false));
}


#[test]
fn host_input_burst_keeps_followup_wake_off_current_input_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
    let animation_wake = state
        .next_wake_at
        .expect("requested animation should schedule a delayed wake");
    assert!(animation_wake > now);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);

    assert_eq!(
        state.next_wake_at,
        Some(animation_wake),
        "host input should preserve the current frame for HostInput and use the burst wake for the follow-up frame"
    );
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
}


#[test]
fn due_burst_wake_after_host_input_poll_does_not_steal_product_frame() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::RequestedAnimation);
    let due = state
        .next_wake_at
        .expect("requested animation should schedule a wake");
    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            frame_lane: Some(NativeFrameLane::ProductInteraction),
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        },
        true,
    );
    state.note_accepted_host_input(9, 12.0, false, None);

    assert!(state.consume_due_wake_after_poll(due));
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput),
        "a due burst wake must not relabel an accepted host-input product frame"
    );
    assert_eq!(
        state.current_frame_lane,
        Some(NativeFrameLane::ProductInteraction)
    );
    assert!(state.should_render(due, false));
}


#[test]
fn host_input_animation_burst_can_repaint_without_waiting_a_frame_interval() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
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
    assert!(state.should_render(now, false));

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);
    assert!(!state.consume_due_wake_after_poll(now));

    assert!(state.should_render(now, false));
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(
        state.current_role_dirty_reason,
        Some(NativeRoleDirtyReason::ScrollChanged)
    );
    assert_eq!(state.dirty_revision, state.presented_revision + 1);
}


#[test]
fn accepted_real_input_repaint_is_presentable_even_with_runtime_dirty_reason() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision,
            scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
            role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
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

    assert!(state.should_render(now, false));
    assert_eq!(state.dirty_revision, state.presented_revision + 1);
    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::RuntimeTurnApplied)
    );
}


#[test]
fn demand_driven_scheduler_wakes_for_role_dirty_reason_without_branching_on_it() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);

    let dirty = state.mark_dirty(
        NativeSchedulerReason::ExternalWake,
        Some(NativeRoleDirtyReason::SourcePayloadAccepted),
    );

    assert!(state.should_render(Instant::now(), false));
    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::SourcePayloadAccepted)
    );
    state.mark_presented(dirty);
    assert!(!state.should_render(Instant::now(), false));
}


#[test]
fn continuous_probe_scheduler_always_renders() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::ContinuousProbe);
    state.mark_presented(state.dirty_revision);

    assert!(state.should_render(Instant::now(), false));
}


#[test]
fn wake_handle_changes_generation() {
    let wake_handle = NativeWakeHandle::new();
    assert_eq!(wake_handle.generation(), 0);
    assert_eq!(wake_handle.wake(), 1);
    assert_eq!(wake_handle.generation(), 1);
}


#[test]
fn wake_handle_interrupts_idle_wait() {
    let wake_handle = NativeWakeHandle::new();
    let worker_wake = wake_handle.clone();
    let started = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        worker_wake.wake();
    });

    let observed = wake_handle.wait_for_wake_after_signal(0);

    assert_eq!(observed, 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}


#[test]
fn scheduled_wake_is_not_pushed_later_by_repeated_poll_results() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);
    let now = Instant::now();
    let first = state.schedule_wake_after(now, Duration::from_millis(500));
    let second =
        state.schedule_wake_after(now + Duration::from_millis(100), Duration::from_millis(500));

    assert_eq!(first, second);
    assert!(!state.consume_due_wake(now + Duration::from_millis(499)));
    assert!(state.consume_due_wake(now + Duration::from_millis(500)));
    assert_eq!(
        state.last_scheduler_reason,
        Some(NativeSchedulerReason::Timer)
    );
    assert!(!state.should_render(now + Duration::from_millis(500), false));
}


#[test]
fn poll_result_uses_role_revision_as_presentable_dirty_revision() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let poll = NativePollResult {
        dirty: true,
        role_revision: 1,
        scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
        role_dirty_reason: Some(NativeRoleDirtyReason::DocumentPatchApplied),
        frame_lane: None,
        accepted_host_input_event_hint: None,
        next_wake_after_ms: None,
        cursor_icon: NativeCursorIcon::Default,
        wants_animation_frame: false,
        diagnostics: None,
        accessibility_update: None,
    };

    state.apply_poll_result(&poll, false);

    assert_eq!(state.dirty_revision, 1);
    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::DocumentPatchApplied)
    );

    state.mark_presented(1);
    state.apply_poll_result(&poll, false);

    assert_eq!(state.dirty_revision, 1);
    assert!(!state.should_render(Instant::now(), false));
}


#[test]
fn host_input_dirty_reason_is_presentable_even_if_dirty_flag_was_false() {
    let poll = NativePollResult {
        dirty: false,
        role_revision: 7,
        scheduler_reason: Some(NativeSchedulerReason::HostInput),
        role_dirty_reason: Some(NativeRoleDirtyReason::ScrollChanged),
        frame_lane: None,
        accepted_host_input_event_hint: None,
        next_wake_after_ms: None,
        cursor_icon: NativeCursorIcon::Default,
        wants_animation_frame: false,
        diagnostics: None,
        accessibility_update: None,
    };

    let effective = effective_poll_result_for_host_input(poll.clone(), true);
    assert!(effective.dirty);
    assert_eq!(
        effective.role_dirty_reason,
        Some(NativeRoleDirtyReason::ScrollChanged)
    );
    assert_eq!(
        effective.scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(
        effective.frame_lane,
        Some(NativeFrameLane::ProductInteraction)
    );

    let without_real_input = effective_poll_result_for_host_input(poll, false);
    assert!(!without_real_input.dirty);
}


#[test]
fn host_input_dominates_requested_animation_burst_accounting() {
    let poll = NativePollResult {
        dirty: true,
        role_revision: 7,
        scheduler_reason: Some(NativeSchedulerReason::RequestedAnimation),
        role_dirty_reason: Some(NativeRoleDirtyReason::FocusChanged),
        frame_lane: Some(NativeFrameLane::AnimationFollowup),
        accepted_host_input_event_hint: None,
        next_wake_after_ms: None,
        cursor_icon: NativeCursorIcon::Default,
        wants_animation_frame: true,
        diagnostics: None,
        accessibility_update: None,
    };

    let effective = effective_poll_result_for_host_input(poll, true);

    assert_eq!(
        effective.scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert_eq!(
        effective.frame_lane,
        Some(NativeFrameLane::ProductInteraction)
    );
    assert_eq!(
        native_frame_lane_for_scheduler(
            Some(NativeSchedulerReason::RequestedAnimation),
            None,
            true,
            true,
        ),
        NativeFrameLane::ProductInteraction
    );
}
