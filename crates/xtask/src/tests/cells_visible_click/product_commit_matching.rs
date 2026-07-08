#[test]
fn cells_visible_click_product_commit_for_interaction_prefers_accepted_input_commit() {
    let accepted_key = json!({
        "frame_seq": 7,
        "present_id": 7,
        "input_event_seq": 4,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 6,
        "layout_revision": 1,
        "render_scene_revision": 4
    });
    let presented_key = json!({
        "frame_seq": 8,
        "present_id": 8,
        "input_event_seq": 5,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 6,
        "layout_revision": 1,
        "render_scene_revision": 4
    });
    let report = json!({
        "accepted_host_input_event_wake_count": 4,
        "input_to_present_accounted_event_wake_count": 4,
        "presented_input_event_wake_count": 5,
        "frame_evidence_key": presented_key,
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 4,
            "input_to_present_ms": 10.996183,
            "frame_evidence_key": accepted_key
        }]
    });

    let (commit, source, input_event_seq) =
        cells_visible_click_product_commit_for_interaction(&report, 5);

    assert_eq!(
        source,
        "recent_product_frame_commits_by_accepted_input_event_seq"
    );
    assert_eq!(input_event_seq, Some(4));
    assert_eq!(
        commit
            .and_then(|commit| commit.get("frame_evidence_key"))
            .cloned(),
        Some(accepted_key)
    );
}


#[test]
fn cells_visible_click_product_commit_match_rejects_nearby_input_generation_even_for_same_frame_context()
 {
    let commit_key = json!({
        "frame_seq": 350,
        "present_id": 350,
        "input_event_seq": 130,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 251,
        "layout_revision": 1,
        "render_scene_revision": 90
    });
    let visible_product_key = json!({
        "frame_seq": 353,
        "present_id": 353,
        "input_event_seq": 131,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 251,
        "layout_revision": 1,
        "render_scene_revision": 90
    });
    let proof_key = json!({
        "frame_seq": 356,
        "present_id": 356,
        "input_event_seq": 131,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 251,
        "layout_revision": 1,
        "render_scene_revision": 90
    });
    let report = json!({
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 130,
            "input_to_present_ms": 10.838645,
            "frame_evidence_key": commit_key
        }]
    });

    let matched = cells_visible_click_product_commit_match_from_report(
        &report,
        Some(&visible_product_key),
        Some(&proof_key),
        Some(10.838645),
    );

    assert_eq!(matched["status"], json!("missing"));
    assert_eq!(matched["match_method"], json!("missing_product_commit"));
}


#[test]
fn cells_visible_click_product_commit_match_rejects_nearby_input_generation_for_different_context()
{
    let report = json!({
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 130,
            "input_to_present_ms": 10.838645,
            "frame_evidence_key": {
                "frame_seq": 350,
                "present_id": 350,
                "input_event_seq": 130,
                "surface_id": "preview:test",
                "surface_epoch": 1,
                "content_revision": 251,
                "layout_revision": 1,
                "render_scene_revision": 90
            }
        }]
    });
    let stale_visible_key = json!({
        "frame_seq": 353,
        "present_id": 353,
        "input_event_seq": 131,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 252,
        "layout_revision": 1,
        "render_scene_revision": 90
    });

    let matched = cells_visible_click_product_commit_match_from_report(
        &report,
        Some(&stale_visible_key),
        Some(&stale_visible_key),
        Some(10.838645),
    );

    assert_eq!(matched["status"], json!("missing"));
    assert_eq!(matched["match_method"], json!("missing_product_commit"));
}


#[test]
fn cells_visible_click_preview_hold_scales_with_target_count() {
    assert_eq!(cells_visible_click_preview_hold_ms(0), 90_000);
    assert_eq!(cells_visible_click_preview_hold_ms(4), 90_000);
    assert_eq!(cells_visible_click_preview_hold_ms(64), 350_000);
}


#[test]
fn cells_visible_click_verifier_requests_wait_for_readback_backpressure() {
    assert!(cells_visible_click_interactive_readback_busy(&json!({
        "render_loop_state": {
            "last_interactive_surface_readback_pending": true
        }
    })));
    assert!(!cells_visible_click_interactive_readback_busy(&json!({
        "last_interactive_surface_readback_pending": false
    })));

    assert!(cells_visible_click_verifier_frame_request_consumed(
        &json!({
            "last_poll_diagnostics": {
                "verifier_frame_request": {
                    "consumed_count": 42
                }
            }
        }),
        42,
    ));
    assert!(!cells_visible_click_verifier_frame_request_consumed(
        &json!({
            "last_poll_diagnostics": {
                "verifier_frame_request": {
                    "consumed_count": 41
                }
            }
        }),
        42,
    ));
}


