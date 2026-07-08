// Included by `../proof_and_readback.rs`.

// test: post_present_proof_queue_tracks_deferred_and_pre_present_requests_by_frame_key
#[test]
fn post_present_proof_queue_tracks_deferred_and_pre_present_requests_by_frame_key() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
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
    let requests = vec![
        NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::VisibleBoundText,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        },
        NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::ExternalAppOwnedReadback,
            built_pre_present: true,
            frame_local_snapshot_required: true,
        },
    ];

    state.enqueue_post_present_proof_requests(&key, &requests, Some(42.0));

    assert_eq!(state.post_present_proof_queue_enqueued_count, 2);
    assert_eq!(state.post_present_proof_queue_deferred_count, 1);
    assert_eq!(state.post_present_proof_queue_pre_present_count, 1);
    assert_eq!(state.recent_post_present_proof_queue.len(), 2);
    assert_eq!(
        state.recent_post_present_proof_queue[0].frame_evidence_key,
        key
    );
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::Queued
    );
    assert_eq!(
        state.recent_post_present_proof_queue[1].status,
        NativePostPresentProofQueueStatus::AlreadyBuiltPrePresent
    );
    assert_eq!(
        state.recent_post_present_proof_queue[0].enqueued_elapsed_ms,
        Some(42.0)
    );
    assert!(state.note_post_present_proof_request_completed(
        &key,
        NativePostPresentProofRequestKind::VisibleBoundText,
        Some(48.0),
    ));
    assert!(!state.note_post_present_proof_request_completed(
        &key,
        NativePostPresentProofRequestKind::ExternalAppOwnedReadback,
        Some(49.0),
    ));
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::CompletedPostPresent
    );
    assert_eq!(
        state.recent_post_present_proof_queue[0].completed_elapsed_ms,
        Some(48.0)
    );
    assert_eq!(
        state.recent_post_present_proof_queue[1].status,
        NativePostPresentProofQueueStatus::AlreadyBuiltPrePresent
    );
}

// test: post_present_proof_subscriber_artifact_completes_matching_queue_request
#[test]
fn post_present_proof_subscriber_artifact_completes_matching_queue_request() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 9,
        content_revision: 12,
        layout_revision: 4,
        render_scene_revision: 6,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(15),
        present_id: 19,
        proof_request_id: None,
    };
    state.enqueue_post_present_proof_requests(
        &key,
        &[NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::RetainedBoundSync,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        }],
        Some(50.0),
    );

    let subscriber = native_post_present_json_proof_subscriber(
        NativePostPresentProofRequestKind::RetainedBoundSync,
        |context| {
            serde_json::json!({
                "frame_seq": context.frame_evidence_key.frame_seq,
                "completed_elapsed_ms": context.completed_elapsed_ms
            })
        },
    );

    run_post_present_proof_subscribers(&mut state, vec![subscriber], &key, Some(51.0));

    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert_eq!(state.post_present_proof_subscriber_error_count, 0);
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::CompletedPostPresent
    );
    assert_eq!(
        state.recent_post_present_proof_queue[0].completed_elapsed_ms,
        Some(51.0)
    );
    let artifact = state
        .recent_post_present_proof_artifacts
        .back()
        .expect("subscriber artifact");
    assert_eq!(
        artifact.kind,
        NativePostPresentProofRequestKind::RetainedBoundSync
    );
    assert_eq!(artifact.frame_evidence_key, key);
    assert_eq!(
        artifact
            .payload
            .pointer("/frame_seq")
            .and_then(|value| value.as_u64()),
        Some(9)
    );
}

// test: async_post_present_proof_worker_records_keyed_artifact
#[test]
fn async_post_present_proof_worker_records_keyed_artifact() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 10,
        content_revision: 12,
        layout_revision: 4,
        render_scene_revision: 6,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(16),
        present_id: 20,
        proof_request_id: None,
    };
    state.enqueue_post_present_proof_requests(
        &key,
        &[NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::VisibleBoundText,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        }],
        Some(50.0),
    );
    let subscriber = native_post_present_json_proof_subscriber(
        NativePostPresentProofRequestKind::VisibleBoundText,
        |context| {
            serde_json::json!({
                "frame_seq": context.frame_evidence_key.frame_seq,
                "completed_elapsed_ms": context.completed_elapsed_ms
            })
        },
    );
    let mut worker = AsyncPostPresentProofSubscriberWorker::new();
    let enqueue_report = worker
        .enqueue(
            vec![subscriber],
            key.clone(),
            Some(61.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::RequiredFrameProof,
        )
        .expect("non-empty subscriber batch should enqueue");
    state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

    assert_eq!(
        state.post_present_proof_subscriber_worker_enqueued_batch_count,
        1
    );
    assert_eq!(state.post_present_proof_subscriber_worker_enqueued_count, 1);
    assert_eq!(
        state.post_present_proof_subscriber_worker_pending_batch_count,
        1
    );

    worker.shutdown_and_drain(&mut state);

    assert_eq!(
        state.post_present_proof_subscriber_worker_completed_count,
        1
    );
    assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
    assert_eq!(
        state.post_present_proof_subscriber_worker_pending_batch_count,
        0
    );
    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::CompletedPostPresent
    );
    let artifact = state
        .recent_post_present_proof_artifacts
        .back()
        .expect("worker should record artifact");
    assert_eq!(
        artifact.kind,
        NativePostPresentProofRequestKind::VisibleBoundText
    );
    assert_eq!(artifact.frame_evidence_key, key);
    assert_eq!(artifact.completed_elapsed_ms, Some(61.0));
}

// test: async_post_present_proof_worker_drain_can_be_bounded
#[test]
fn async_post_present_proof_worker_drain_can_be_bounded() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 11,
        content_revision: 12,
        layout_revision: 4,
        render_scene_revision: 6,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(16),
        present_id: 21,
        proof_request_id: None,
    };
    let shared = Arc::new((
        Mutex::new(AsyncPostPresentProofSubscriberShared::default()),
        Condvar::new(),
    ));
    let (sender, results) = mpsc::channel();
    let worker = AsyncPostPresentProofSubscriberWorker {
        shared,
        results,
        worker: None,
    };
    for index in 0..3_u64 {
        sender
            .send(Ok(native_post_present_json_proof_artifact(
                NativePostPresentProofRequestKind::VisibleBoundText,
                key.clone(),
                Some(index as f64),
                serde_json::json!({ "index": index }),
            )))
            .expect("send proof artifact");
    }

    assert_eq!(worker.drain_completed_limit(&mut state, 2), 2);
    assert_eq!(state.post_present_proof_artifact_count, 2);
    assert_eq!(worker.drain_completed(&mut state), 1);
    assert_eq!(state.post_present_proof_artifact_count, 3);
}

// test: async_post_present_proof_worker_supersedes_background_without_dropping_required
#[test]
fn async_post_present_proof_worker_supersedes_background_without_dropping_required() {
    let key = FrameEvidenceKey {
        frame_seq: 12,
        content_revision: 12,
        layout_revision: 4,
        render_scene_revision: 6,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(16),
        present_id: 22,
        proof_request_id: None,
    };
    let shared = Arc::new((
        Mutex::new(AsyncPostPresentProofSubscriberShared::default()),
        Condvar::new(),
    ));
    let (_sender, results) = mpsc::channel();
    let worker = AsyncPostPresentProofSubscriberWorker {
        shared: Arc::clone(&shared),
        results,
        worker: None,
    };
    let subscriber = |kind| {
        native_post_present_json_proof_subscriber(
            kind,
            |_context| serde_json::json!({"status": "pass"}),
        )
    };

    let first_background = worker
        .enqueue(
            vec![subscriber(NativePostPresentProofRequestKind::ProofHistory)],
            key.clone(),
            Some(1.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::BackgroundTelemetry,
        )
        .expect("first background enqueue");
    assert_eq!(first_background.pending_batches, 1);
    assert_eq!(first_background.superseded_batches, 0);

    let required = worker
        .enqueue(
            vec![subscriber(
                NativePostPresentProofRequestKind::VisibleBoundText,
            )],
            key.clone(),
            Some(2.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::RequiredFrameProof,
        )
        .expect("required enqueue");
    assert_eq!(required.pending_batches, 2);
    assert_eq!(required.dropped_batches, 0);

    let second_background = worker
        .enqueue(
            vec![subscriber(
                NativePostPresentProofRequestKind::RenderHookReportJson,
            )],
            key,
            Some(3.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::BackgroundTelemetry,
        )
        .expect("second background enqueue");
    assert_eq!(second_background.superseded_batches, 1);
    assert_eq!(second_background.superseded_subscribers, 1);
    assert_eq!(second_background.pending_batches, 2);

    let shared = shared.0.lock().expect("proof queue lock");
    let required_count = shared
        .pending
        .iter()
        .filter(|batch| batch.priority == AsyncPostPresentProofBatchPriority::RequiredFrameProof)
        .count();
    let background_count = shared
        .pending
        .iter()
        .filter(|batch| batch.priority == AsyncPostPresentProofBatchPriority::BackgroundTelemetry)
        .count();
    assert_eq!(required_count, 1);
    assert_eq!(background_count, 1);
}

// test: lagging_post_present_proof_worker_is_reported_without_blocking_product_frame
#[test]
fn lagging_post_present_proof_worker_is_reported_without_blocking_product_frame() {
    let dir = std::env::temp_dir().join(format!(
        "boon-post-present-proof-isolation-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.json");

    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.note_poll_started(1.0);
    state.note_accepted_host_input(1, 2.0, false, None);
    state.note_dirty_poll(2.5);
    state.note_render_started(3.0);
    state.note_surface_acquired(3.5);
    state.note_surface_acquire_call(0.5);
    state.note_render_hook_completed(5.0);
    state.note_queue_submitted(5.5);
    state.note_submit_phase_durations(0.2, 0.3, 1.0);
    state.note_present_completed(6.5);
    state.mark_presented_with_revisions(1, 2, 3, 4);
    state.current_frame_lane = Some(NativeFrameLane::ProductInteraction);
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
                layout_identity: Some("layout-proof-isolation".to_owned()),
                render_scene_identity: Some("scene-proof-isolation".to_owned()),
                proof_json_built_pre_present: false,
                render_hook_proof_built_pre_present: false,
                post_present_proof_request_count: proof_requests.len() as u32,
                product_patch: None,
            },
            proof_requests,
        )),
        ..NativeRenderFrameMetrics::default()
    }));
    let input_timing = state.take_frame_accepted_input_timing(Some(1));
    let key = frame_evidence_key_for_presented_frame(
        &state,
        &SurfaceId("preview:proof-isolation".to_owned()),
        1,
        Some(1),
        None,
    );
    state.note_product_frame_commit(product_frame_commit_for_presented_frame(
        &state,
        key.clone(),
        NativeAdapterIdentity::default(),
        NativeFrameLane::ProductInteraction,
        Some(NativeSchedulerReason::HostInput),
        None,
        input_timing,
    ));

    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let subscriber: NativePostPresentProofSubscriber = Box::new(move |context| {
        started_tx
            .send(())
            .expect("test should observe blocked subscriber start");
        release_rx
            .recv()
            .expect("test should release blocked proof subscriber");
        Ok(native_post_present_json_proof_artifact(
            NativePostPresentProofRequestKind::VisibleBoundText,
            context.frame_evidence_key.clone(),
            context.completed_elapsed_ms,
            serde_json::json!({
                "status": "pass",
                "blocked_worker_test": true,
                "frame_seq": context.frame_evidence_key.frame_seq
            }),
        ))
    });
    let mut worker = AsyncPostPresentProofSubscriberWorker::new();
    let enqueue_report = worker
        .enqueue(
            vec![subscriber],
            key.clone(),
            Some(7.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::RequiredFrameProof,
        )
        .expect("proof subscriber batch should enqueue");
    state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("proof subscriber should be running but blocked");
    worker.drain_completed(&mut state);

    let mut recent_product_frame_commits = VecDeque::new();
    if let Some(commit) = state.last_product_frame_commit.as_ref() {
        push_recent_product_frame_commit(&mut recent_product_frame_commits, commit);
    }
    write_render_loop_state_report(
        &path,
        NativeWindowRole::Preview,
        std::process::id(),
        &WindowId("window-proof-isolation".to_owned()),
        &SurfaceId("preview:proof-isolation".to_owned()),
        &NativeSurfaceLifecycleReport {
            surface_epoch: 1,
            final_width: 1,
            final_height: 1,
            ..NativeSurfaceLifecycleReport::default()
        },
        &state,
        Duration::from_millis(16),
        1,
        None,
        &NativePreviewPerfAccumulator::default(),
        NativeRenderLoopReportExtras {
            present_mode: "Immediate".to_owned(),
            surface_format: "Bgra8Unorm".to_owned(),
            desired_maximum_frame_latency: 1,
            desired_maximum_frame_latency_source: "present_mode_default".to_owned(),
            ..NativeRenderLoopReportExtras::default()
        }
        .with_input_generation(1, 1, Some(0.0), Some(0.0))
        .with_frame_evidence_key(Some(&key))
        .with_recent_product_frame_commits(&recent_product_frame_commits),
        None,
    )
    .unwrap();

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        report["post_present_proof_isolation"]["status"],
        serde_json::json!("pass")
    );
    assert_eq!(
        report["post_present_proof_isolation"]["product_latency_includes_proof_completion"],
        serde_json::json!(false)
    );
    assert_eq!(
        report["post_present_proof_isolation"]["product_blocks_on_proof_subscribers"],
        serde_json::json!(false)
    );
    assert_eq!(
        report["post_present_proof_isolation"]["proof_worker_status"],
        serde_json::json!("lagging")
    );
    assert_eq!(
        report["post_present_proof_isolation"]["queued_request_count"],
        serde_json::json!(1)
    );
    assert_eq!(report["product_frame_commit_count"], serde_json::json!(1));
    assert_eq!(
        report["post_present_proof_artifact_count"],
        serde_json::json!(0)
    );

    release_tx.send(()).unwrap();
    worker.shutdown_and_drain(&mut state);
    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

// test: async_post_present_proof_worker_completes_artifact_hash_request
#[test]
fn async_post_present_proof_worker_completes_artifact_hash_request() {
    let dir = std::env::temp_dir().join(format!(
        "boon-post-present-artifact-hash-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let artifact_path = dir.join("proof-artifact.bin");
    std::fs::write(&artifact_path, b"post-present artifact hash").unwrap();
    let expected_sha256 = sha256_file(&artifact_path).unwrap();

    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let key = FrameEvidenceKey {
        frame_seq: 12,
        content_revision: 14,
        layout_revision: 6,
        render_scene_revision: 8,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 2,
        input_event_seq: Some(18),
        present_id: 22,
        proof_request_id: None,
    };
    state.enqueue_post_present_proof_requests(
        &key,
        &[NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::ArtifactHash,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        }],
        Some(80.0),
    );
    let mut worker = AsyncPostPresentProofSubscriberWorker::new();
    let enqueue_report = worker
        .enqueue(
            vec![native_post_present_artifact_hash_subscriber(vec![
                artifact_path.display().to_string(),
            ])],
            key.clone(),
            Some(82.0),
            Instant::now(),
            AsyncPostPresentProofBatchPriority::BackgroundTelemetry,
        )
        .expect("artifact hash subscriber should enqueue");
    state.note_post_present_proof_subscriber_worker_enqueue(enqueue_report);

    worker.shutdown_and_drain(&mut state);

    assert_eq!(
        state.post_present_proof_subscriber_worker_completed_count,
        1
    );
    assert_eq!(state.post_present_proof_subscriber_worker_error_count, 0);
    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::CompletedPostPresent
    );
    let artifact = state
        .recent_post_present_proof_artifacts
        .back()
        .expect("artifact hash worker should record artifact");
    assert_eq!(
        artifact.kind,
        NativePostPresentProofRequestKind::ArtifactHash
    );
    assert_eq!(artifact.frame_evidence_key, key);
    assert_eq!(
        artifact.payload["artifact_hash_status"],
        serde_json::json!("hashed_registered_artifacts")
    );
    assert_eq!(artifact.payload["registered_artifact_count"], 1);
    assert_eq!(artifact.payload["hashed_artifact_count"], 1);
    assert_eq!(
        artifact.payload["artifact_sha256s"][0]["sha256"],
        serde_json::json!(expected_sha256)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// test: artifact_hash_paths_are_collected_for_exact_frame_artifacts
#[test]
fn artifact_hash_paths_are_collected_for_exact_frame_artifacts() {
    let dir = std::env::temp_dir().join(format!(
        "boon-post-present-artifact-paths-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let readback_path = dir.join("readback.bin");
    let external_path = dir.join("external.bin");
    let stale_path = dir.join("stale.bin");
    std::fs::write(&readback_path, b"readback artifact").unwrap();
    std::fs::write(&external_path, b"external artifact").unwrap();
    std::fs::write(&stale_path, b"stale artifact").unwrap();

    let key = FrameEvidenceKey {
        frame_seq: 21,
        content_revision: 23,
        layout_revision: 25,
        render_scene_revision: 27,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 3,
        input_event_seq: Some(29),
        present_id: 31,
        proof_request_id: None,
    };
    let stale_key = FrameEvidenceKey {
        frame_seq: 20,
        content_revision: 22,
        layout_revision: 24,
        render_scene_revision: 26,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 3,
        input_event_seq: Some(28),
        present_id: 30,
        proof_request_id: None,
    };
    let mut current_artifact = test_readback_artifact(key.clone(), 1);
    current_artifact.path = readback_path.display().to_string();
    let mut stale_artifact = test_readback_artifact(stale_key.clone(), 2);
    stale_artifact.path = stale_path.display().to_string();
    let mut recent_artifacts = VecDeque::new();
    recent_artifacts.push_back(stale_artifact.clone());
    recent_artifacts.push_back(current_artifact.clone());
    let external_proof = serde_json::json!({
        "status": "pass",
        "proof": {
            "status": "pass",
            "artifact": {
                "kind": "app_owned_pixels",
                "artifact_path": external_path.display().to_string(),
                "artifact_sha256": "0".repeat(64),
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "surface_id": key.surface_id.clone(),
                "surface_epoch": key.surface_epoch,
                "frame_seq": key.frame_seq
            },
            "stale_artifact": {
                "kind": "app_owned_pixels",
                "artifact_path": stale_path.display().to_string(),
                "artifact_sha256": "1".repeat(64),
                "capture_method": "wgpu-visible-surface-copy-src-readback",
                "frame_evidence_key": stale_key.clone()
            }
        }
    });

    let artifact_paths = post_present_artifact_hash_paths_for_frame(
        &key,
        Some(&current_artifact),
        &recent_artifacts,
        Some(&external_proof),
    );

    assert_eq!(artifact_paths.len(), 2);
    assert!(artifact_paths.contains(&readback_path.display().to_string()));
    assert!(artifact_paths.contains(&external_path.display().to_string()));
    assert!(!artifact_paths.contains(&stale_path.display().to_string()));
    std::fs::remove_dir_all(&dir).unwrap();
}

// test: empty_artifact_hash_is_deferred_while_exact_readback_is_pending
#[test]
fn empty_artifact_hash_is_deferred_while_exact_readback_is_pending() {
    let key = FrameEvidenceKey {
        frame_seq: 30,
        content_revision: 31,
        layout_revision: 32,
        render_scene_revision: 33,
        surface_id: SurfaceId("preview:test".to_owned()),
        surface_epoch: 4,
        input_event_seq: Some(34),
        present_id: 35,
        proof_request_id: None,
    };
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.enqueue_post_present_proof_requests(
        &key,
        &[
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ArtifactHash,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
        ],
        Some(90.0),
    );

    assert!(should_defer_empty_artifact_hash_for_pending_readback(
        &[],
        &state,
        &key,
        true
    ));
    assert!(!should_defer_empty_artifact_hash_for_pending_readback(
        &["target/artifacts/native-gpu/frames/already-ready.png".to_owned()],
        &state,
        &key,
        true
    ));
    assert!(!should_defer_empty_artifact_hash_for_pending_readback(
        &[],
        &state,
        &key,
        false
    ));

    let mut no_readback_state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    no_readback_state.enqueue_post_present_proof_requests(
        &key,
        &[NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::ArtifactHash,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        }],
        Some(90.0),
    );
    assert!(!should_defer_empty_artifact_hash_for_pending_readback(
        &[],
        &no_readback_state,
        &key,
        true
    ));
}

// test: completed_interactive_readback_records_visible_surface_post_present_artifact
#[test]
fn completed_interactive_readback_records_visible_surface_post_present_artifact() {
    let key = FrameEvidenceKey {
        frame_seq: 42,
        content_revision: 100,
        layout_revision: 200,
        render_scene_revision: 300,
        surface_id: SurfaceId("surface-test".to_owned()),
        surface_epoch: 4,
        input_event_seq: Some(7),
        present_id: 42,
        proof_request_id: Some(99),
    };
    let artifact = test_readback_artifact(key.clone(), 1);
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.enqueue_post_present_proof_requests(
        &key,
        &[NativePostPresentProofRequestSummary {
            kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
            built_pre_present: false,
            frame_local_snapshot_required: true,
        }],
        Some(1.0),
    );
    let mut recent = VecDeque::new();
    let mut last = None;
    let mut deferred_artifact_hash_readback_keys = VecDeque::new();

    note_completed_interactive_readback_artifact(
        &mut state,
        &mut recent,
        &mut last,
        &mut deferred_artifact_hash_readback_keys,
        artifact,
        Some(12.5),
    );

    assert_eq!(recent.len(), 1);
    assert_eq!(
        last.as_ref()
            .and_then(|artifact| artifact.frame_evidence_key.as_ref()),
        Some(&key)
    );
    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert_eq!(
        state.recent_post_present_proof_queue[0].status,
        NativePostPresentProofQueueStatus::CompletedPostPresent
    );

    let post_present_artifact = state
        .recent_post_present_proof_artifacts
        .back()
        .expect("completed readback should record a post-present artifact");
    assert_eq!(
        post_present_artifact.kind,
        NativePostPresentProofRequestKind::VisibleSurfaceReadback
    );
    assert_eq!(post_present_artifact.frame_evidence_key, key);
    assert_eq!(post_present_artifact.completed_elapsed_ms, Some(12.5));
    assert_eq!(post_present_artifact.payload["status"], "pass");
    assert_eq!(
        post_present_artifact.payload["capture_method"],
        "wgpu-visible-surface-copy-src-readback"
    );
    assert_eq!(
        post_present_artifact.payload["artifact_path"],
        "artifact-1.png"
    );
    assert_eq!(post_present_artifact.payload["artifact_sha256"], "sha-1");
    assert_eq!(post_present_artifact.payload["frame_seq"], 42);
    assert_eq!(post_present_artifact.payload["input_event_seq"], 7);
    assert_eq!(post_present_artifact.payload["present_id"], 42);
    assert_eq!(
        post_present_artifact.payload["frame_evidence_key"],
        serde_json::json!(key)
    );
}

// test: completed_interactive_readback_completes_matching_artifact_hash_request
#[test]
fn completed_interactive_readback_completes_matching_artifact_hash_request() {
    let key = FrameEvidenceKey {
        frame_seq: 43,
        content_revision: 101,
        layout_revision: 201,
        render_scene_revision: 301,
        surface_id: SurfaceId("surface-test".to_owned()),
        surface_epoch: 4,
        input_event_seq: Some(8),
        present_id: 43,
        proof_request_id: Some(100),
    };
    let artifact = test_readback_artifact(key.clone(), 2);
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.enqueue_post_present_proof_requests(
        &key,
        &[
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ArtifactHash,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
        ],
        Some(2.0),
    );
    let mut recent = VecDeque::new();
    let mut last = None;
    let mut deferred_artifact_hash_readback_keys = VecDeque::new();
    push_deferred_artifact_hash_readback_key(&mut deferred_artifact_hash_readback_keys, &key);

    note_completed_interactive_readback_artifact(
        &mut state,
        &mut recent,
        &mut last,
        &mut deferred_artifact_hash_readback_keys,
        artifact,
        Some(13.5),
    );

    assert_eq!(recent.len(), 1);
    assert_eq!(state.post_present_proof_artifact_count, 2);
    assert_eq!(state.post_present_proof_queue_completed_count, 2);
    assert!(state.recent_post_present_proof_queue.iter().all(|entry| {
        entry.status == NativePostPresentProofQueueStatus::CompletedPostPresent
            && entry.frame_evidence_key == key
            && entry.completed_elapsed_ms == Some(13.5)
    }));
    let artifact_hash = state
        .recent_post_present_proof_artifacts
        .iter()
        .find(|artifact| artifact.kind == NativePostPresentProofRequestKind::ArtifactHash)
        .expect("completed readback should also record artifact hash proof");
    assert_eq!(artifact_hash.frame_evidence_key, key);
    assert_eq!(
        artifact_hash.payload["artifact_hash_status"],
        serde_json::json!("hashed_registered_artifacts")
    );
    assert_eq!(artifact_hash.payload["registered_artifact_count"], 1);
    assert_eq!(artifact_hash.payload["hashed_artifact_count"], 1);
    assert_eq!(
        artifact_hash.payload["artifact_sha256s"][0]["path"],
        serde_json::json!("artifact-2.png")
    );
    assert_eq!(
        artifact_hash.payload["artifact_sha256s"][0]["sha256"],
        serde_json::json!("sha-2")
    );
    assert_eq!(
        artifact_hash.payload["artifact_sha256s"][0]["source"],
        serde_json::json!("visible_surface_readback_completion")
    );
    assert!(deferred_artifact_hash_readback_keys.is_empty());
}

// test: completed_interactive_readback_does_not_race_non_deferred_artifact_hash
#[test]
fn completed_interactive_readback_does_not_race_non_deferred_artifact_hash() {
    let key = FrameEvidenceKey {
        frame_seq: 44,
        content_revision: 102,
        layout_revision: 202,
        render_scene_revision: 302,
        surface_id: SurfaceId("surface-test".to_owned()),
        surface_epoch: 4,
        input_event_seq: Some(9),
        present_id: 44,
        proof_request_id: Some(101),
    };
    let artifact = test_readback_artifact(key.clone(), 3);
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    state.enqueue_post_present_proof_requests(
        &key,
        &[
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::VisibleSurfaceReadback,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
            NativePostPresentProofRequestSummary {
                kind: NativePostPresentProofRequestKind::ArtifactHash,
                built_pre_present: false,
                frame_local_snapshot_required: true,
            },
        ],
        Some(3.0),
    );
    let mut recent = VecDeque::new();
    let mut last = None;
    let mut deferred_artifact_hash_readback_keys = VecDeque::new();

    note_completed_interactive_readback_artifact(
        &mut state,
        &mut recent,
        &mut last,
        &mut deferred_artifact_hash_readback_keys,
        artifact,
        Some(14.5),
    );

    assert_eq!(state.post_present_proof_artifact_count, 1);
    assert_eq!(state.post_present_proof_queue_completed_count, 1);
    assert!(
        state
            .recent_post_present_proof_artifacts
            .iter()
            .all(|artifact| artifact.kind != NativePostPresentProofRequestKind::ArtifactHash)
    );
    let artifact_hash_entry = state
        .recent_post_present_proof_queue
        .iter()
        .find(|entry| entry.request.kind == NativePostPresentProofRequestKind::ArtifactHash)
        .expect("artifact hash request remains queued for the real subscriber");
    assert_eq!(
        artifact_hash_entry.status,
        NativePostPresentProofQueueStatus::Queued
    );
}

// test: verifier_readback_backpressure_never_defers_product_rendering
#[test]
fn verifier_readback_backpressure_never_defers_product_rendering() {
    assert!(should_defer_render_for_interactive_readback(
        true,
        true,
        false,
        Some(NativeSchedulerReason::VerifierFrame)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        false,
        false,
        Some(NativeSchedulerReason::VerifierFrame)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        true,
        false,
        Some(NativeSchedulerReason::Timer)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        true,
        false,
        Some(NativeSchedulerReason::RequestedAnimation)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        true,
        true,
        Some(NativeSchedulerReason::HostInput)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        true,
        false,
        Some(NativeSchedulerReason::HostInput)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        false,
        true,
        false,
        Some(NativeSchedulerReason::Timer)
    ));
    assert!(!should_defer_render_for_interactive_readback(
        true,
        false,
        false,
        Some(NativeSchedulerReason::Timer)
    ));
}

// test: proof_readback_defers_during_product_interaction_burst
#[test]
fn proof_readback_defers_during_product_interaction_burst() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);
    let now = Instant::now();
    state.mark_presented(state.dirty_revision);

    state.request_animation_burst(now, 10.0, NativeSchedulerReason::HostInput);
    state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);

    assert!(state.interaction_burst_active(11.0));
    assert!(state.defer_proof_readback_for_product_lane(11.0));

    state.note_proof_readback_deferred("interaction_burst");
    assert_eq!(state.proof_readback_deferred_count, 1);
    assert_eq!(state.proof_readback_deferred_for_interaction_burst_count, 1);
    assert_eq!(state.proof_readback_deferred_for_product_input_count, 0);
    assert_eq!(
        state.last_proof_readback_deferred_reason.as_deref(),
        Some("interaction_burst")
    );

    state.current_frame_lane = Some(NativeFrameLane::ProofOrHarness);
    assert!(!state.defer_proof_readback_for_product_lane(11.0));

    state.current_frame_lane = Some(NativeFrameLane::AnimationFollowup);
    state.requested_animation_burst_frames_remaining = 0;
    assert!(state.interaction_burst_active(11.0));
    assert!(state.interaction_quiet_proof_readback_frame(11.0));
    assert!(state.defer_proof_readback_for_product_lane(11.0));

    state.current_frame_lane = Some(NativeFrameLane::ProofOrHarness);
    assert!(!state.defer_proof_readback_for_product_lane(11.0));
}

// test: proof_frames_poll_completed_readback_during_interaction_burst
#[test]
fn proof_frames_poll_completed_readback_during_interaction_burst() {
    assert!(!pre_submit_proof_poll_allowed(
        true,
        true,
        Some(NativeFrameLane::ProductInteraction),
        Some(NativeSchedulerReason::HostInput),
    ));
    assert!(!pre_submit_proof_poll_allowed(
        false,
        true,
        Some(NativeFrameLane::AnimationFollowup),
        Some(NativeSchedulerReason::RequestedAnimation),
    ));
    assert!(pre_submit_proof_poll_allowed(
        false,
        true,
        Some(NativeFrameLane::ProofOrHarness),
        Some(NativeSchedulerReason::VerifierFrame),
    ));
    assert!(pre_submit_proof_poll_allowed(
        false,
        false,
        Some(NativeFrameLane::RuntimeOrLayout),
        Some(NativeSchedulerReason::ExternalWake),
    ));
}

