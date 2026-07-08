// Included by `../proof_and_readback.rs`.

// test: product_frame_commit_adds_visible_surface_readback_request_once
#[test]
fn product_frame_commit_adds_visible_surface_readback_request_once() {
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
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let proof_requests = vec![NativePostPresentProofRequestSummary {
        kind: NativePostPresentProofRequestKind::VisibleBoundText,
        built_pre_present: false,
        frame_local_snapshot_required: true,
    }];
    state.note_render_frame_metrics(Some(NativeRenderFrameMetrics {
        product_result: Some(test_product_result(
            NativeRenderedProductFrame {
                schema_version: 1,
                render_target_kind: "visible-surface-direct".to_owned(),
                visible_surface_rendered: true,
                visible_present_path: true,
                layout_identity: Some("layout:1".to_owned()),
                render_scene_identity: Some("scene:1".to_owned()),
                proof_json_built_pre_present: false,
                render_hook_proof_built_pre_present: false,
                post_present_proof_request_count: proof_requests.len() as u32,
                product_patch: None,
            },
            proof_requests,
        )),
        ..NativeRenderFrameMetrics::default()
    }));

    let mut commit = product_frame_commit_for_presented_frame(
        &state,
        key.clone(),
        NativeAdapterIdentity::default(),
        NativeFrameLane::ProductInteraction,
        Some(NativeSchedulerReason::HostInput),
        None,
        None,
    );

    assert_eq!(commit.post_present_proof_request_count, 1);
    add_post_present_proof_request_to_commit(
        &mut commit,
        visible_surface_readback_post_present_request(),
    );
    add_post_present_proof_request_to_commit(
        &mut commit,
        visible_surface_readback_post_present_request(),
    );

    assert_eq!(commit.post_present_proof_request_count, 2);
    assert_eq!(commit.post_present_proof_requests.len(), 2);
    assert_eq!(commit.pre_present_proof_request_count, 0);
    assert!(commit.post_present_proof_requests.iter().any(|request| {
        request.kind == NativePostPresentProofRequestKind::VisibleSurfaceReadback
            && !request.built_pre_present
            && request.frame_local_snapshot_required
    }));
    assert_eq!(
        commit
            .product_frame
            .as_ref()
            .map(|frame| frame.post_present_proof_request_count),
        Some(2)
    );

    let mut queue_state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    queue_state.note_product_frame_commit(commit);
    assert_eq!(queue_state.post_present_proof_queue_enqueued_count, 2);
    assert_eq!(queue_state.post_present_proof_queue_deferred_count, 2);
    assert_eq!(
        queue_state
            .recent_post_present_proof_queue
            .iter()
            .filter(|entry| {
                entry.request.kind == NativePostPresentProofRequestKind::VisibleSurfaceReadback
                    && entry.frame_evidence_key == key
            })
            .count(),
        1
    );
}

// test: recent_interactive_readback_registry_matches_exact_frame_key
#[test]
fn recent_interactive_readback_registry_matches_exact_frame_key() {
    let surface_id = SurfaceId("surface-test".to_owned());
    let mut recent = VecDeque::new();
    let mut expected_key = None;

    for index in 0..(RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT + 2) {
        let key = FrameEvidenceKey {
            frame_seq: index as u64,
            content_revision: 100 + index as u64,
            layout_revision: 200 + index as u64,
            render_scene_revision: 300 + index as u64,
            surface_id: surface_id.clone(),
            surface_epoch: 4,
            input_event_seq: Some(index as u64),
            present_id: index as u64,
            proof_request_id: None,
        };
        if index == RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT {
            expected_key = Some(key.clone());
        }
        let artifact = test_readback_artifact(key, index as u64);
        push_recent_interactive_readback_artifact(&mut recent, &artifact);
    }

    assert_eq!(recent.len(), RECENT_INTERACTIVE_READBACK_ARTIFACT_LIMIT);
    assert_eq!(
        recent
            .front()
            .unwrap()
            .frame_evidence_key
            .as_ref()
            .unwrap()
            .frame_seq,
        2
    );
    let expected_key = expected_key.expect("test should capture a retained key");
    let matched = recent_interactive_readback_artifact_for_frame(
        recent.make_contiguous(),
        Some(&expected_key),
    )
    .expect("registry should find exact frame evidence key");
    assert_eq!(matched.frame_evidence_key.as_ref(), Some(&expected_key));

    let missing_key = FrameEvidenceKey {
        frame_seq: 999,
        content_revision: 999,
        layout_revision: 999,
        render_scene_revision: 999,
        surface_id,
        surface_epoch: 4,
        input_event_seq: Some(999),
        present_id: 999,
        proof_request_id: None,
    };
    assert!(
        recent_interactive_readback_artifact_for_frame(
            recent.make_contiguous(),
            Some(&missing_key),
        )
        .is_none()
    );
}

// test: external_visible_readback_proof_gets_frame_evidence_key
#[test]
fn external_visible_readback_proof_gets_frame_evidence_key() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.mark_presented_with_revisions(7, 42, 43, 44);
    let surface_id = SurfaceId("surface-test".to_owned());
    let key = frame_evidence_key_for_presented_frame(&state, &surface_id, 9, Some(3), None);
    let proof = serde_json::json!({
        "status": "pass",
        "proof": {
            "capture_method": "wgpu-visible-surface-copy-src-readback",
            "artifact": {
                "capture_method": "metadata-only"
            }
        },
        "nested": [
            {
                "capture_method": "wgpu-visible-surface-copy-src-readback"
            }
        ]
    });

    let enriched = external_render_proof_with_frame_evidence_key(Some(&proof), Some(&key)).unwrap();

    assert_eq!(
        enriched.pointer("/proof/frame_evidence_key/frame_seq"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        enriched.pointer("/nested/0/frame_evidence_key/surface_id"),
        Some(&serde_json::json!("surface-test"))
    );
    assert!(
        enriched
            .pointer("/proof/artifact/frame_evidence_key")
            .is_none()
    );
}

// test: external_visible_readback_proof_replaces_duplicate_interactive_readback
#[test]
fn external_visible_readback_proof_replaces_duplicate_interactive_readback() {
    let proof = serde_json::json!({
        "status": "pass",
        "renderer": "boon_native_gpu",
        "proof": {
            "status": "pass",
            "capture_method": "wgpu-visible-surface-copy-src-readback",
            "replacement_proof": "render-loop visible surface readback artifact"
        }
    });

    assert!(external_render_proof_replaces_interactive_readback(Some(
        &proof
    )));

    let failing_proof = serde_json::json!({
        "status": "fail",
        "proof": {
            "status": "fail",
            "capture_method": "wgpu-visible-surface-copy-src-readback"
        }
    });
    assert!(!external_render_proof_replaces_interactive_readback(Some(
        &failing_proof
    )));

    let desktop_capture = serde_json::json!({
        "status": "pass",
        "proof": {
            "capture_method": "desktop-screenshot"
        }
    });
    assert!(!external_render_proof_replaces_interactive_readback(Some(
        &desktop_capture
    )));
}

// test: final_report_drain_completes_pending_interactive_readback
#[test]
fn final_report_drain_completes_pending_interactive_readback() {
    let frame_evidence_key = FrameEvidenceKey {
        frame_seq: 42,
        content_revision: 7,
        layout_revision: 5,
        render_scene_revision: 6,
        surface_id: SurfaceId("surface-test".to_owned()),
        surface_epoch: 1,
        input_event_seq: Some(3),
        present_id: 42,
        proof_request_id: None,
    };
    let artifact = AppWindowReadbackArtifact {
        path: "target/artifacts/native-gpu/frames/test.png".to_owned(),
        sha256: "0".repeat(64),
        width: 4,
        height: 4,
        presented_revision: Some(7),
        content_revision: Some(7),
        rendered_frame_count: Some(42),
        frame_evidence_key: Some(frame_evidence_key.clone()),
        capture_method: "wgpu-visible-surface-copy-src-readback".to_owned(),
        texture_format: "Bgra8UnormSrgb".to_owned(),
        nonblank_samples: 16,
        unique_rgba_values: 2,
        readback_deadline_ms: 5_000,
        readback_poll_status: "completed_before_deadline".to_owned(),
    };
    let (sender, receiver) = mpsc::channel();
    sender
        .send(Ok(AsyncInteractiveReadbackResult {
            artifact,
            finish_ms: 1.0,
            completed_elapsed_ms: 2.0,
        }))
        .unwrap();
    let mut job = Some(AsyncInteractiveReadbackJob { receiver });

    let result = finish_interactive_readback_job_before_report(&mut job, Duration::from_millis(1))
        .expect("pending readback should complete before final report")
        .expect("readback result should be ok");

    assert!(job.is_none());
    assert_eq!(
        result.artifact.frame_evidence_key.as_ref(),
        Some(&frame_evidence_key)
    );
    assert_eq!(result.completed_elapsed_ms, 2.0);
}

// test: final_report_drain_preserves_pending_interactive_readback_on_timeout
#[test]
fn final_report_drain_preserves_pending_interactive_readback_on_timeout() {
    let (_sender, receiver) = mpsc::channel();
    let mut job = Some(AsyncInteractiveReadbackJob { receiver });

    let result = finish_interactive_readback_job_before_report(&mut job, Duration::from_millis(0));

    assert!(result.is_none());
    assert!(job.is_some());
}

