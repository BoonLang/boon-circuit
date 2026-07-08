// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn frame_metrics_expose_render_scene_source_for_report_provenance() {
    let metrics = FrameMetrics {
        render_scene_source: RENDER_SCENE_SOURCE_DOCUMENT_RENDER_SCENE.to_owned(),
        ..FrameMetrics::default()
    };
    let encoded = serde_json::to_value(&metrics).unwrap();

    assert_eq!(
        encoded
            .get("render_scene_source")
            .and_then(serde_json::Value::as_str),
        Some(RENDER_SCENE_SOURCE_DOCUMENT_RENDER_SCENE)
    );
}
