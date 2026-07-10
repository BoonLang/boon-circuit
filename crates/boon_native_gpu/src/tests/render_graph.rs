// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn renderer_graph_plan_hash_ignores_workload_metrics() {
    let low_workload = vec![test_graph_pass(128, 1)];
    let high_workload = vec![test_graph_pass(4096, 8)];

    assert_eq!(
        renderer_render_graph_plan_hash(&low_workload),
        renderer_render_graph_plan_hash(&high_workload)
    );
    assert_ne!(
        renderer_render_graph_workload_hash(&low_workload),
        renderer_render_graph_workload_hash(&high_workload)
    );
}

#[test]
fn product_frame_graph_executor_emits_typed_pass_and_resource_metrics() {
    let schedule = ProductFrameSchedule::product_surface(1);
    let mut graph = ProductFrameGraphExecutor::new(schedule);
    let (scene_key, _scene_key_ms) = graph
        .run_product_pass(
            ProductFrameGraphPassId::SceneKey,
            ProductFrameGraphResourceId::RenderScene,
            ProductFrameGraphResourceId::SceneCacheKey,
            || {
                Ok((
                    17_u64,
                    RendererRenderGraphPassStats {
                        queue_write_count: 0,
                        ..RendererRenderGraphPassStats::default()
                    },
                ))
            },
        )
        .expect("scene-key graph pass should run");
    assert_eq!(scene_key, 17);
    graph
        .run_product_pass(
            ProductFrameGraphPassId::QuadPrepareUpload,
            ProductFrameGraphResourceId::RenderSceneItems,
            ProductFrameGraphResourceId::RetainedGpuBuffers,
            || {
                Ok((
                    (),
                    RendererRenderGraphPassStats {
                        upload_bytes: 128,
                        dirty_chunk_count: 1,
                        queue_write_count: 1,
                        ..RendererRenderGraphPassStats::default()
                    },
                ))
            },
        )
        .expect("quad prepare graph pass should run");
    graph
        .run_product_pass(
            ProductFrameGraphPassId::UiDraw,
            ProductFrameGraphResourceId::RetainedGpuBuffers,
            ProductFrameGraphResourceId::ColorTarget,
            || {
                Ok((
                    (),
                    RendererRenderGraphPassStats {
                        draw_call_count: 1,
                        ..RendererRenderGraphPassStats::default()
                    },
                ))
            },
        )
        .expect("ui draw graph pass should run");
    let ((), _metrics_ms) = graph
        .run_metrics_pass(
            ProductFrameGraphPassId::RetainedMetrics,
            ProductFrameGraphResourceId::RenderScene,
            ProductFrameGraphResourceId::FrameMetrics,
            || Ok(((), RendererRenderGraphPassStats::default())),
        )
        .expect("retained-metrics graph pass should run");
    graph
        .run_product_pass(
            ProductFrameGraphPassId::TextDraw,
            ProductFrameGraphResourceId::TextRuns,
            ProductFrameGraphResourceId::ColorTarget,
            || {
                Ok((
                    (),
                    RendererRenderGraphPassStats {
                        draw_call_count: 1,
                        ..RendererRenderGraphPassStats::default()
                    },
                ))
            },
        )
        .expect("text draw graph pass should run");

    let execution = graph.finish().expect("full graph schedule should finish");
    let passes = execution.executed_passes;
    assert_eq!(passes.len(), 5);
    assert_eq!(passes[0].pass_id, "renderer-scene-key");
    assert_eq!(passes[0].pass_kind, "scene_identity");
    assert_eq!(passes[0].read_resources, vec!["RenderScene"]);
    assert_eq!(passes[0].write_resources, vec!["SceneCacheKey"]);
    assert!(passes[0].product_visible);
    assert!(!passes[0].proof_or_readback);
    assert_eq!(passes[3].pass_id, "renderer-retained-metrics");
    assert_eq!(passes[3].pass_kind, "retained_metrics");
    assert!(!passes[3].product_visible);
    assert!(!passes[3].proof_or_readback);

    let resources = renderer_render_graph_resources_for_passes(&passes);
    assert!(resources.iter().any(|resource| {
        resource.resource_id == "RenderScene" && resource.resource_kind == "cpu_scene"
    }));
    assert!(resources.iter().any(|resource| {
        resource.resource_id == "SceneCacheKey" && resource.resource_kind == "cpu_identity"
    }));
    assert!(resources.iter().any(|resource| {
        resource.resource_id == "FrameMetrics" && resource.resource_kind == "cpu_metrics"
    }));
}


#[test]
fn product_frame_graph_state_tracks_dirty_and_reused_resource_epochs() {
    let passes = vec![
        ProductFrameGraphPass::product(
            ProductFrameGraphPassId::SceneKey,
            ProductFrameGraphResourceId::RenderScene,
            ProductFrameGraphResourceId::SceneCacheKey,
        )
        .metric(0.0, RendererRenderGraphPassStats::default()),
    ];
    let mut resources = renderer_render_graph_resources_for_passes(&passes);
    let mut state = ProductFrameGraphState::default();
    let first_signatures = BTreeMap::from([
        ("RenderScene".to_owned(), "scene:1".to_owned()),
        ("SceneCacheKey".to_owned(), "scene-key:1".to_owned()),
    ]);

    let first = state.update_resources(1, &mut resources, &first_signatures);
    assert_eq!(first.resource_count, 2);
    assert_eq!(first.dirty_resource_count, 2);
    assert_eq!(first.reused_resource_count, 0);
    assert!(first.resource_epoch_hash.len() == 64);
    assert!(
        resources
            .iter()
            .all(|resource| resource.retained_epoch == 1)
    );
    assert!(resources.iter().all(|resource| resource.retained_dirty));
    assert!(resources.iter().all(|resource| !resource.retained_reused));
    assert!(
        resources
            .iter()
            .all(|resource| resource.last_used_frame_seq == 1)
    );

    let mut second_resources = renderer_render_graph_resources_for_passes(&passes);
    let second = state.update_resources(2, &mut second_resources, &first_signatures);
    assert_eq!(second.resource_count, 2);
    assert_eq!(second.dirty_resource_count, 0);
    assert_eq!(second.reused_resource_count, 2);
    assert!(
        second_resources
            .iter()
            .all(|resource| resource.retained_epoch == 1)
    );
    assert!(
        second_resources
            .iter()
            .all(|resource| !resource.retained_dirty)
    );
    assert!(
        second_resources
            .iter()
            .all(|resource| resource.retained_reused)
    );

    let changed_signatures = BTreeMap::from([
        ("RenderScene".to_owned(), "scene:2".to_owned()),
        ("SceneCacheKey".to_owned(), "scene-key:2".to_owned()),
    ]);
    let mut third_resources = renderer_render_graph_resources_for_passes(&passes);
    let third = state.update_resources(3, &mut third_resources, &changed_signatures);
    assert_eq!(third.dirty_resource_count, 2);
    assert_eq!(third.reused_resource_count, 0);
    assert!(
        third_resources
            .iter()
            .all(|resource| resource.retained_epoch == 2)
    );
}


#[test]
fn product_frame_graph_scheduler_classifies_resource_decisions() {
    let decisions = product_frame_graph_schedule_decisions(
        &[
            RendererRenderGraphResourceMetric {
                schema_version: 1,
                resource_id: "RetainedGpuBuffers".to_owned(),
                resource_kind: "gpu_buffer".to_owned(),
                first_pass_index: 1,
                last_pass_index: 2,
                producer_pass_id: Some("renderer-quad-prepare-upload".to_owned()),
                consumer_pass_ids: vec!["renderer-ui-draw".to_owned()],
                product_visible: true,
                proof_or_readback: false,
                retained_epoch: 2,
                retained_dirty: true,
                retained_reused: false,
                last_used_frame_seq: 7,
            },
            RendererRenderGraphResourceMetric {
                schema_version: 1,
                resource_id: "RenderScene".to_owned(),
                resource_kind: "cpu_scene".to_owned(),
                first_pass_index: 0,
                last_pass_index: 3,
                producer_pass_id: None,
                consumer_pass_ids: vec!["renderer-scene-key".to_owned()],
                product_visible: true,
                proof_or_readback: false,
                retained_epoch: 1,
                retained_dirty: false,
                retained_reused: true,
                last_used_frame_seq: 7,
            },
            RendererRenderGraphResourceMetric {
                schema_version: 1,
                resource_id: "ColorTarget".to_owned(),
                resource_kind: "gpu_color_target".to_owned(),
                first_pass_index: 2,
                last_pass_index: 4,
                producer_pass_id: Some("renderer-ui-draw".to_owned()),
                consumer_pass_ids: vec!["renderer-text-draw".to_owned()],
                product_visible: true,
                proof_or_readback: false,
                retained_epoch: 7,
                retained_dirty: true,
                retained_reused: false,
                last_used_frame_seq: 7,
            },
            RendererRenderGraphResourceMetric {
                schema_version: 1,
                resource_id: "FrameMetrics".to_owned(),
                resource_kind: "cpu_metrics".to_owned(),
                first_pass_index: 3,
                last_pass_index: 3,
                producer_pass_id: Some("renderer-retained-metrics".to_owned()),
                consumer_pass_ids: Vec::new(),
                product_visible: false,
                proof_or_readback: false,
                retained_epoch: 7,
                retained_dirty: true,
                retained_reused: false,
                last_used_frame_seq: 7,
            },
        ],
        &["chunk:right".to_owned()],
    );
    let by_resource = decisions
        .iter()
        .map(|decision| {
            (
                decision.resource_id.as_str(),
                decision.decision_kind.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(by_resource["RetainedGpuBuffers"], "dirty_upload");
    assert_eq!(by_resource["RenderScene"], "clean_reuse");
    assert_eq!(by_resource["ColorTarget"], "per_present_target");
    assert_eq!(by_resource["FrameMetrics"], "per_frame_metrics");

    let metrics = product_frame_graph_schedule_metrics(&decisions);
    assert_eq!(metrics.decision_count, 4);
    assert_eq!(metrics.dirty_resource_decision_count, 1);
    assert_eq!(metrics.reuse_resource_decision_count, 1);
    assert_eq!(metrics.per_present_resource_decision_count, 2);
    assert_eq!(metrics.schedule_hash.len(), 64);
}


#[test]
fn product_frame_schedule_declares_resources_before_execution() {
    let schedule = ProductFrameSchedule::product_surface(3);
    assert_eq!(schedule.scheduler_kind, PRODUCT_FRAME_GRAPH_SCHEDULER_KIND);
    assert_eq!(schedule.len(), 5);
    assert_eq!(schedule.plan_hash().len(), 64);
    let resources = schedule.planned_resources();
    let resource_ids = resources
        .iter()
        .map(|resource| resource.resource_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(resource_ids.contains("RenderScene"));
    assert!(resource_ids.contains("RenderSceneItems"));
    assert!(resource_ids.contains("RetainedGpuBuffers"));
    assert!(resource_ids.contains("ColorTarget"));
    assert!(resource_ids.contains("FrameMetrics"));
    assert!(resource_ids.contains("TextRuns"));
    assert!(!resource_ids.contains("NoTextRuns"));
}


#[test]
fn product_frame_schedule_uses_no_text_resource_for_empty_text() {
    let schedule = ProductFrameSchedule::product_surface(0);
    let resources = schedule.planned_resources();
    let resource_ids = resources
        .iter()
        .map(|resource| resource.resource_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(resource_ids.contains("NoTextRuns"));
    assert!(!resource_ids.contains("TextRuns"));
}


#[test]
fn product_frame_graph_executor_rejects_out_of_order_pass() {
    let schedule = ProductFrameSchedule::product_surface(1);
    let mut executor = ProductFrameGraphExecutor::new(schedule);
    let error = executor
        .run_product_pass(
            ProductFrameGraphPassId::UiDraw,
            ProductFrameGraphResourceId::RetainedGpuBuffers,
            ProductFrameGraphResourceId::ColorTarget,
            || Ok(((), RendererRenderGraphPassStats::default())),
        )
        .expect_err("executor should reject a pass that skips the scheduled scene-key pass");
    assert!(
        error
            .message
            .contains("ProductFrameGraph schedule mismatch"),
        "{error:?}"
    );
}


#[test]
fn product_frame_graph_executor_rejects_early_finish() {
    let schedule = ProductFrameSchedule::product_surface(1);
    let mut executor = ProductFrameGraphExecutor::new(schedule);
    executor
        .run_product_pass(
            ProductFrameGraphPassId::SceneKey,
            ProductFrameGraphResourceId::RenderScene,
            ProductFrameGraphResourceId::SceneCacheKey,
            || Ok((42_u64, RendererRenderGraphPassStats::default())),
        )
        .expect("first scheduled pass should run");
    let error = executor
        .finish()
        .expect_err("executor should reject a partially executed schedule");
    assert!(
        error
            .message
            .contains("ProductFrameGraph schedule finished early"),
        "{error:?}"
    );
}
