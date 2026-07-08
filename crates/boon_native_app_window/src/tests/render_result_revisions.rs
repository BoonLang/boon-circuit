// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn presented_state_records_render_layer_revisions() {
    let mut state = NativeRenderLoopState::new(NativeRenderLoopMode::DemandDriven);

    state.mark_presented_with_revisions(1, 3, 4, 5);

    assert_eq!(state.presented_revision, 1);
    assert_eq!(state.last_render_content_revision, 3);
    assert_eq!(state.last_render_layout_revision, 4);
    assert_eq!(state.last_render_scene_revision, 5);
    assert_eq!(state.rendered_frame_count, 1);
}
