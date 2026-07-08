// Included by `../proof_and_readback.rs`.

// test: product_frame_commit_uses_typed_product_result
#[test]
fn product_frame_commit_uses_typed_product_result() {
    let key = FrameEvidenceKey {
        frame_seq: 9,
        content_revision: 11,
        layout_revision: 3,
        render_scene_revision: 5,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(13),
        present_id: 9,
        proof_request_id: None,
    };
    let typed_frame = NativeRenderedProductFrame {
        schema_version: 1,
        render_target_kind: "visible-surface-direct".to_owned(),
        visible_surface_rendered: true,
        visible_present_path: true,
        layout_identity: Some("layout:typed".to_owned()),
        render_scene_identity: Some("scene:typed".to_owned()),
        proof_json_built_pre_present: false,
        render_hook_proof_built_pre_present: false,
        post_present_proof_request_count: 1,
        product_patch: Some(NativeProductPatchSummary {
            schema_version: 1,
            status: "pass".to_owned(),
            owner: "preview_active_scene".to_owned(),
            patch_kind: "direct_input_overlay_render_scene_patch".to_owned(),
            source: "retained_bound_sync".to_owned(),
            active_scene_identity: Some("active-preview-scene:test".to_owned()),
            route_identity: Some("route:test".to_owned()),
            touched_node_count: 1,
            touched_node_samples: vec!["node:test".to_owned()],
            retained_text_update_count: 1,
            retained_style_update_count: 1,
            hover_node_count: 0,
            focus_node_count: 1,
            direct_render_scene_patch: true,
            full_scene_build_before_present: false,
            proof_json_required: false,
            latest_report_required: false,
        }),
    };
    let typed_requests = vec![NativePostPresentProofRequestSummary {
        kind: NativePostPresentProofRequestKind::VisibleBoundText,
        built_pre_present: false,
        frame_local_snapshot_required: true,
    }];
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
        product_result: Some(NativeProductFrameResult {
            schema_version: 1,
            owner: "preview_active_scene".to_owned(),
            result_kind: "active_preview_scene_patch".to_owned(),
            product_frame: typed_frame,
            render_graph: None,
            present_plan: None,
            post_present_proof_requests: typed_requests,
        }),
        ..NativeRenderFrameMetrics::default()
    }));

    let commit = product_frame_commit_for_presented_frame(
        &state,
        key,
        NativeAdapterIdentity::default(),
        NativeFrameLane::ProductInteraction,
        Some(NativeSchedulerReason::HostInput),
        None,
        None,
    );

    assert_eq!(commit.product_result_source, "native_product_render_result");
    assert_eq!(
        commit.product_result_owner.as_deref(),
        Some("preview_active_scene")
    );
    assert_eq!(
        commit.product_result_kind.as_deref(),
        Some("active_preview_scene_patch")
    );
    assert_eq!(
        commit
            .product_frame
            .as_ref()
            .and_then(|frame| frame.layout_identity.as_deref()),
        Some("layout:typed")
    );
    assert_eq!(commit.pre_present_proof_request_count, 0);
    assert_eq!(commit.post_present_proof_requests.len(), 1);
    assert_eq!(
        commit.post_present_proof_requests[0].kind,
        NativePostPresentProofRequestKind::VisibleBoundText
    );
}

// test: frame_evidence_key_tracks_presented_frame_identity
#[test]
fn frame_evidence_key_tracks_presented_frame_identity() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented_with_revisions(7, 42, 43, 44);
    let surface_id = SurfaceId("surface-test".to_owned());

    let key = frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(3), Some(11));

    assert_eq!(key.frame_seq, 1);
    assert_eq!(key.present_id, 1);
    assert_eq!(key.content_revision, 42);
    assert_eq!(key.layout_revision, 43);
    assert_eq!(key.render_scene_revision, 44);
    assert_eq!(key.surface_id, surface_id);
    assert_eq!(key.surface_epoch, 9);
    assert_eq!(key.input_event_seq, Some(3));
    assert_eq!(key.proof_request_id, Some(11));
}

// test: non_product_presented_frame_enqueues_proof_without_product_commit
#[test]
fn non_product_presented_frame_enqueues_proof_without_product_commit() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 3,
        content_revision: 4,
        layout_revision: 5,
        render_scene_revision: 6,
        surface_id: SurfaceId("surface-test".to_owned()),
        surface_epoch: 7,
        input_event_seq: None,
        present_id: 3,
        proof_request_id: None,
    };
    let requests = vec![NativePostPresentProofRequestSummary {
        kind: NativePostPresentProofRequestKind::VisibleBoundText,
        built_pre_present: false,
        frame_local_snapshot_required: true,
    }];

    state.note_non_product_presented_frame(
        &key,
        NativeFrameLane::AnimationFollowup,
        "non_product_frame_lane",
        &requests,
        Some(42.0),
    );

    assert_eq!(state.product_frame_commit_count, 0);
    assert!(state.last_product_frame_commit.is_none());
    assert_eq!(state.non_product_presented_frame_count, 1);
    assert_eq!(
        state.last_non_product_presented_frame_lane,
        Some(NativeFrameLane::AnimationFollowup)
    );
    assert_eq!(
        state.last_non_product_presented_frame_key,
        Some(key.clone())
    );
    assert_eq!(
        state.last_non_product_presented_frame_reason.as_deref(),
        Some("non_product_frame_lane")
    );
    assert_eq!(state.post_present_proof_queue_enqueued_count, 1);
    assert_eq!(state.post_present_proof_queue_deferred_count, 1);
    assert_eq!(
        state.recent_post_present_proof_queue[0].frame_evidence_key,
        key
    );
}

// test: scheduler_preserves_previous_role_dirty_reason_when_later_poll_has_no_role_reason
#[test]
fn scheduler_preserves_previous_role_dirty_reason_when_later_poll_has_no_role_reason() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);

    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision.saturating_add(1),
            scheduler_reason: Some(NativeSchedulerReason::ExternalWake),
            role_dirty_reason: Some(NativeRoleDirtyReason::SourcePayloadAccepted),
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        },
        false,
    );
    state.mark_presented(state.dirty_revision);
    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision,
            scheduler_reason: Some(NativeSchedulerReason::VerifierFrame),
            role_dirty_reason: None,
            frame_lane: None,
            accepted_host_input_event_hint: None,
            next_wake_after_ms: None,
            cursor_icon: NativeCursorIcon::Default,
            wants_animation_frame: false,
            diagnostics: None,
            accessibility_update: None,
        },
        false,
    );

    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::SourcePayloadAccepted)
    );
    assert!(state.should_render(Instant::now(), false));
    assert_eq!(state.dirty_revision, state.presented_revision + 1);
    assert_eq!(
        state.current_frame_lane,
        Some(NativeFrameLane::ProofOrHarness)
    );
}

// test: verifier_frame_does_not_invent_new_content_revision
#[test]
fn verifier_frame_does_not_invent_new_content_revision() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let poll = NativePollResult {
        dirty: true,
        role_revision: 0,
        scheduler_reason: Some(NativeSchedulerReason::VerifierFrame),
        role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
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
    assert!(
        NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 1,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: false,
            role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
        }
        .validate_for_presented_revision(state.dirty_revision)
        .is_ok()
    );
}

// test: animation_request_on_dirty_role_revision_does_not_invent_unrenderable_revision
#[test]
fn animation_request_on_dirty_role_revision_does_not_invent_unrenderable_revision() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let poll = NativePollResult {
        dirty: true,
        role_revision: 2,
        scheduler_reason: Some(NativeSchedulerReason::Timer),
        role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
        frame_lane: None,
        accepted_host_input_event_hint: None,
        next_wake_after_ms: Some(16),
        cursor_icon: NativeCursorIcon::Default,
        wants_animation_frame: true,
        diagnostics: None,
        accessibility_update: None,
    };

    state.apply_poll_result(&poll, false);

    assert_eq!(state.dirty_revision, 2);
    assert!(
        (NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: 2,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: true,
            role_dirty_reason: Some(NativeRoleDirtyReason::VerifierFrame),
        })
        .validate_for_presented_revision_with_scheduler(
            state.dirty_revision,
            state.current_scheduler_reason,
            state.current_role_dirty_reason,
        )
        .is_ok(),
        "animation scheduling must not demand a content revision the role never produced"
    );
}

// test: requested_animation_can_repaint_existing_scheduler_only_content
#[test]
fn requested_animation_can_repaint_existing_scheduler_only_content() {
    let render = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 2,
        layout_revision: None,
        render_scene_revision: None,
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: true,
        role_dirty_reason: None,
    };

    assert!(
        render
            .validate_for_presented_revision_with_scheduler(
                3,
                Some(NativeSchedulerReason::RequestedAnimation),
                None,
            )
            .is_ok(),
        "requested animation frames are scheduler-owned repaints"
    );
    assert_eq!(
        render.presented_content_revision(3, Some(NativeSchedulerReason::RequestedAnimation), None),
        2
    );
}

// test: structured_render_result_rejects_stale_or_missing_revisions
#[test]
fn structured_render_result_rejects_stale_or_missing_revisions() {
    let mut zero = NativeRenderHookResult::rendered_with_proof(serde_json::json!({}));
    assert!(zero.validate_for_presented_revision(1).is_err());

    zero.content_revision = 1;
    assert!(zero.validate_for_presented_revision(2).is_err());

    zero.content_revision = 2;
    zero.rendered = false;
    assert!(zero.validate_for_presented_revision(2).is_err());

    zero.rendered = true;
    assert!(zero.validate_for_presented_revision(2).is_ok());

    zero.layout_revision = Some(0);
    assert!(zero.validate_for_presented_revision(2).is_err());

    zero.layout_revision = Some(3);
    zero.render_scene_revision = Some(0);
    assert!(zero.validate_for_presented_revision(2).is_err());
}

// test: render_hook_result_can_present_without_external_proof_payload
#[test]
fn render_hook_result_can_present_without_external_proof_payload() {
    let mut render = NativeRenderHookResult::rendered_without_proof();
    render.content_revision = 3;
    render.layout_revision = Some(3);
    render.render_scene_revision = Some(3);

    assert_eq!(render.proof, None);
    assert!(
        render.validate_for_presented_revision(3).is_ok(),
        "product counters frames must not need pre-present proof JSON"
    );
}

// test: render_hook_result_can_carry_independent_layer_revisions
#[test]
fn render_hook_result_can_carry_independent_layer_revisions() {
    let render = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 10,
        layout_revision: Some(4),
        render_scene_revision: Some(7),
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: true,
        role_dirty_reason: None,
    };

    assert_eq!(
        render.presented_revisions(
            10,
            Some(NativeSchedulerReason::ExternalWake),
            Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        ),
        (10, 4, 7)
    );
}

// test: surface_dirty_revision_can_present_existing_content_revision
#[test]
fn surface_dirty_revision_can_present_existing_content_revision() {
    let render = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 1,
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
                2,
                Some(NativeSchedulerReason::SurfaceChanged),
                None,
            )
            .is_ok(),
        "surface resize should be allowed to repaint unchanged document content"
    );
    assert_eq!(
        render.presented_content_revision(2, Some(NativeSchedulerReason::SurfaceChanged), None),
        1
    );
    assert!(
        render
            .validate_for_presented_revision_with_scheduler(
                2,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::SourcePayloadAccepted),
            )
            .is_err(),
        "runtime/source wakes must still reject stale content revisions"
    );
}

// test: external_runtime_cleanup_can_repaint_existing_content_revision
#[test]
fn external_runtime_cleanup_can_repaint_existing_content_revision() {
    let same_content_runtime_cleanup = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 13,
        layout_revision: Some(7),
        render_scene_revision: Some(9),
        render_frame_metrics: None,
        post_present_proof_subscribers: Vec::new(),
        rendered: true,
        content_changed: false,
        role_dirty_reason: Some(NativeRoleDirtyReason::RuntimeTurnApplied),
    };

    assert!(
        same_content_runtime_cleanup
            .validate_for_presented_revision_with_scheduler(
                14,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            )
            .is_ok(),
        "queued runtime cleanup may repaint the current content frame without inventing a semantic content revision"
    );
    assert_eq!(
        same_content_runtime_cleanup.presented_revisions(
            14,
            Some(NativeSchedulerReason::ExternalWake),
            Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        ),
        (13, 7, 9),
        "frame revision may advance while content/layout/render-scene revisions stay keyed to the actual rendered state"
    );

    let changed_runtime_cleanup = NativeRenderHookResult {
        content_changed: true,
        ..same_content_runtime_cleanup
    };
    assert!(
        changed_runtime_cleanup
            .validate_for_presented_revision_with_scheduler(
                14,
                Some(NativeSchedulerReason::ExternalWake),
                Some(NativeRoleDirtyReason::RuntimeTurnApplied),
            )
            .is_err(),
        "changed runtime content must provide a current content revision"
    );
}

// test: idle_same_content_frame_can_repaint_existing_content_revision
#[test]
fn idle_same_content_frame_can_repaint_existing_content_revision() {
    let render = NativeRenderHookResult {
        proof: Some(serde_json::json!({})),
        content_revision: 4,
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
            .validate_for_presented_revision_with_scheduler(5, None, None)
            .is_ok(),
        "continuous verifier frames may repaint unchanged already-presented content"
    );
    assert_eq!(render.presented_content_revision(5, None, None), 4);

    let changed = NativeRenderHookResult {
        content_changed: true,
        ..render
    };
    assert!(
        changed
            .validate_for_presented_revision_with_scheduler(5, None, None)
            .is_err(),
        "new content without a scheduler/role reason must not be backdated"
    );
}

// test: scheduler_only_repaint_ignores_sticky_previous_role_dirty_reason
#[test]
fn scheduler_only_repaint_ignores_sticky_previous_role_dirty_reason() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented(state.dirty_revision);
    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: state.presented_revision.saturating_add(1),
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
        false,
    );
    let semantic_revision = state.dirty_revision;
    state.mark_presented(semantic_revision);

    state.apply_poll_result(
        &NativePollResult {
            dirty: true,
            role_revision: semantic_revision,
            scheduler_reason: Some(NativeSchedulerReason::HostInput),
            role_dirty_reason: None,
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

    assert_eq!(
        state.last_role_dirty_reason,
        Some(NativeRoleDirtyReason::RuntimeTurnApplied),
        "reporting should preserve the last semantic role dirty reason"
    );
    assert_eq!(state.current_role_dirty_reason, None);
    assert_eq!(
        state.current_scheduler_reason,
        Some(NativeSchedulerReason::HostInput)
    );
    assert!(
        (NativeRenderHookResult {
            proof: Some(serde_json::json!({})),
            content_revision: semantic_revision,
            layout_revision: None,
            render_scene_revision: None,
            render_frame_metrics: None,
            post_present_proof_subscribers: Vec::new(),
            rendered: true,
            content_changed: false,
            role_dirty_reason: None,
        })
        .validate_for_presented_revision_with_scheduler(
            state.dirty_revision,
            state.current_scheduler_reason,
            state.current_role_dirty_reason,
        )
        .is_ok(),
        "host focus/mouse repaint must not be rejected because of a previous runtime dirty reason"
    );
}
