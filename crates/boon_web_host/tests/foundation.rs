use boon_document::{
    MapTileCacheKey, MapTileRequestIdentity, MapTileScaleKey, MapTileSourceId,
    MapViewportGeneration, SemanticWebInputEvent,
};
use boon_host::{
    DocumentNodeId, HostEvent, PointerPhase, SemanticActions, SemanticId, SemanticNode,
    SemanticRelations, SemanticRole, SemanticScene, SemanticState, SourceBindingId, SurfaceId,
    WindowId,
};
use boon_native_gpu::MapTileFetchRequest;
use boon_web_host::*;
use std::collections::BTreeSet;

#[test]
fn fetch_capabilities_bind_origin_method_path_headers_and_bytes() {
    let mut same_origin = BrowserFetchCapability::same_origin_api("api", "/api");
    same_origin.methods = [FetchMethod::Get].into_iter().collect();
    same_origin.max_request_bytes = 4;
    let external = BrowserFetchCapability::https_endpoint("tiles", "tiles.example.test", "/raster");
    let capabilities = BrowserFetchCapabilities::new([same_origin, external]).unwrap();

    let validated = capabilities
        .validate_request(BrowserFetchRequest {
            request_id: 7,
            capability: "api".to_owned(),
            method: FetchMethod::Get,
            path_and_query: "/api/stations?q=oslo".to_owned(),
            headers: vec![HeaderValue {
                name: "Accept".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: Vec::new(),
        })
        .unwrap();
    assert_eq!(validated.origin, BrowserFetchOrigin::SameOrigin);
    assert_eq!(validated.request.request_id, 7);

    let external = capabilities
        .validate_request(BrowserFetchRequest {
            request_id: 8,
            capability: "tiles".to_owned(),
            method: FetchMethod::Get,
            path_and_query: "/raster/3/4/5.png".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        external
            .origin
            .request_url(&external.request.path_and_query),
        "https://tiles.example.test/raster/3/4/5.png"
    );

    for bad_path in [
        "https://evil.test/api",
        "//evil.test/api",
        "/api/%2e%2e/secret",
    ] {
        assert!(
            capabilities
                .validate_request(BrowserFetchRequest {
                    request_id: 9,
                    capability: "api".to_owned(),
                    method: FetchMethod::Get,
                    path_and_query: bad_path.to_owned(),
                    headers: Vec::new(),
                    body: Vec::new(),
                })
                .is_err(),
            "{bad_path} must be rejected"
        );
    }
    assert!(
        capabilities
            .validate_request(BrowserFetchRequest {
                request_id: 10,
                capability: "api".to_owned(),
                method: FetchMethod::Post,
                path_and_query: "/api".to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
            })
            .is_err()
    );
    assert!(
        capabilities
            .validate_request(BrowserFetchRequest {
                request_id: 12,
                capability: "api".to_owned(),
                method: FetchMethod::Get,
                path_and_query: "/api".to_owned(),
                headers: Vec::new(),
                body: vec![1],
            })
            .is_err()
    );
    assert!(
        capabilities
            .validate_request(BrowserFetchRequest {
                request_id: 11,
                capability: "api".to_owned(),
                method: FetchMethod::Get,
                path_and_query: "/api".to_owned(),
                headers: vec![HeaderValue {
                    name: "Host".to_owned(),
                    value: "evil.test".to_owned(),
                }],
                body: Vec::new(),
            })
            .is_err()
    );
}

#[test]
fn external_fetch_capabilities_reject_ip_and_local_hosts() {
    for host in ["localhost", "api.localhost", "127.0.0.1", "bad..example"] {
        let capability = BrowserFetchCapability::https_endpoint("external", host, "/");
        assert!(
            BrowserFetchCapabilities::new([capability]).is_err(),
            "{host}"
        );
    }
}

#[test]
fn websocket_capability_and_queues_are_strictly_bounded() {
    let mut capability = BrowserWebSocketCapability::same_origin("live", "/live");
    capability.protocols = ["boon.v1".to_owned()].into_iter().collect();
    capability.max_message_bytes = 5;
    capability.max_queue_messages = 2;
    capability.max_queue_bytes = 8;
    let capabilities = BrowserWebSocketCapabilities::new([capability]).unwrap();
    let validated = capabilities
        .validate_request(BrowserWebSocketRequest {
            connection_id: 1,
            capability: "live".to_owned(),
            path_and_query: "/live?cursor=2".to_owned(),
            protocols: vec!["boon.v1".to_owned()],
        })
        .unwrap();
    assert_eq!(validated.max_message_bytes, 5);
    assert!(
        capabilities
            .validate_request(BrowserWebSocketRequest {
                connection_id: 2,
                capability: "live".to_owned(),
                path_and_query: "/admin".to_owned(),
                protocols: vec!["boon.v1".to_owned()],
            })
            .is_err()
    );

    let mut queue = BoundedSocketQueue::new(5, 2, 8).unwrap();
    queue
        .push(SocketFrame::Text {
            text: "abc".to_owned(),
        })
        .unwrap();
    queue
        .push(SocketFrame::Binary {
            bytes: vec![1, 2, 3, 4],
        })
        .unwrap();
    assert_eq!(queue.byte_len(), 7);
    assert!(
        queue
            .push(SocketFrame::Text {
                text: "x".to_owned()
            })
            .is_err()
    );
    assert!(queue.pop().is_some());
    assert_eq!(queue.byte_len(), 4);
    assert!(
        queue
            .push(SocketFrame::Text {
                text: "123456".to_owned()
            })
            .is_err()
    );
}

#[test]
fn history_and_clipboard_capabilities_do_not_escape_their_bounds() {
    let history = BrowserHistoryCapability {
        path_prefix: "/app".to_owned(),
        max_url_bytes: 64,
        max_state_bytes: 4,
    };
    history
        .validate_entry(&BrowserHistoryEntry {
            path_query_fragment: "/app/map#3/4/5".to_owned(),
            state: vec![1, 2],
        })
        .unwrap();
    assert!(
        history
            .validate_entry(&BrowserHistoryEntry {
                path_query_fragment: "/admin".to_owned(),
                state: Vec::new(),
            })
            .is_err()
    );
    let clipboard = BrowserClipboardCapability {
        max_text_bytes: 4,
        require_user_activation: true,
    };
    assert!(clipboard.validate_text("test", true).is_ok());
    assert!(clipboard.validate_text("test", false).is_err());
    assert!(clipboard.validate_text("oversized", true).is_err());
}

#[test]
fn browser_input_coalesces_only_adjacent_motion_and_wheel() {
    let mut normalizer = BrowserInputNormalizer::new(
        SurfaceId("web".to_owned()),
        WindowId("browser".to_owned()),
        1,
        32,
    )
    .unwrap();
    let mut queue = BrowserEventQueue::new(8).unwrap();
    queue
        .push(
            normalizer
                .pointer(1.0, 2.0, PointerPhase::Move, None)
                .unwrap(),
        )
        .unwrap();
    queue
        .push(
            normalizer
                .pointer(3.0, 4.0, PointerPhase::Move, None)
                .unwrap(),
        )
        .unwrap();
    queue.push(normalizer.focus(true)).unwrap();
    queue
        .push(normalizer.wheel(3.0, 4.0, 2.0, 3.0).unwrap())
        .unwrap();
    queue
        .push(normalizer.wheel(3.0, 4.0, 5.0, 7.0).unwrap())
        .unwrap();
    assert_eq!(queue.len(), 3);
    let events = queue.drain().collect::<Vec<_>>();
    let BrowserHostEvent::Input { envelope } = &events[0] else {
        panic!("expected pointer event")
    };
    assert!(matches!(
        &envelope.event,
        HostEvent::Pointer(pointer) if pointer.x == 3.0 && pointer.y == 4.0
    ));
    let BrowserHostEvent::Input { envelope } = &events[2] else {
        panic!("expected wheel event")
    };
    assert!(matches!(
        &envelope.event,
        HostEvent::Wheel(wheel) if wheel.delta_x == 7.0 && wheel.delta_y == 10.0
    ));
}

#[test]
fn requested_animation_burst_is_bounded_and_returns_to_idle() {
    let mut scheduler = BrowserFrameScheduler::new(BrowserFrameSchedulerConfig {
        burst_min_frames: 2,
        burst_quiet_ms: 10,
        burst_hard_cap_ms: 30,
    })
    .unwrap();
    assert!(scheduler.wake(BrowserFrameWakeReason::VisibleInput, 100));
    assert!(!scheduler.wake(BrowserFrameWakeReason::RuntimePatch, 101));
    assert!(scheduler.begin_animation_frame().render);
    assert!(
        scheduler
            .complete_animation_frame(105, true, false)
            .schedule_next_animation_frame
    );
    assert!(scheduler.begin_animation_frame().render);
    assert!(
        scheduler
            .complete_animation_frame(110, false, false)
            .schedule_next_animation_frame
    );
    assert!(scheduler.begin_animation_frame().render);
    let completion = scheduler.complete_animation_frame(130, false, false);
    assert!(!completion.schedule_next_animation_frame);
    assert_eq!(completion.pacing, BrowserFramePacing::Idle);
}

#[test]
fn semantic_projection_routes_only_declared_actions() {
    let id = SemanticId("semantic:search".to_owned());
    let mut scene = SemanticScene {
        root: Some(id.clone()),
        ..SemanticScene::default()
    };
    scene.nodes.insert(
        id.clone(),
        SemanticNode {
            id: id.clone(),
            node: DocumentNodeId("search".to_owned()),
            role: SemanticRole::TextInput,
            name: Some("Search".to_owned()),
            description: None,
            value: None,
            state: SemanticState::default(),
            actions: SemanticActions {
                focus: true,
                set_text: true,
                ..SemanticActions::default()
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: Some("en".to_owned()),
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:search".to_owned())),
            source_path: Some("store.search".to_owned()),
            source_intent: Some("change".to_owned()),
        },
    );
    let mut projection = SemanticProjectionState::new(scene.clone());
    assert_eq!(projection.bridge().dom.metrics.visual_dom_node_count, 0);
    let dispatch = projection
        .source_dispatch_for_web_event(SemanticWebInputEvent::SetText {
            semantic_id: id.clone(),
            text: "Oslo".to_owned(),
        })
        .unwrap();
    assert_eq!(dispatch.source_path, "store.search");
    assert_eq!(dispatch.text.as_deref(), Some("Oslo"));

    scene.nodes.get_mut(&id).unwrap().name = Some("Find station".to_owned());
    let update = projection.update(scene);
    assert_eq!(update.patch.operations.len(), 1);
    assert_eq!(update.bridge.dom.metrics.visual_dom_node_count, 0);
}

#[test]
fn host_core_uses_public_host_events_and_exposes_unsupported_foundation_parts() {
    let mut host = BrowserDocumentHostCore::new(BrowserDocumentHostConfig::default()).unwrap();
    assert!(host.support().indexed_db_preferences.is_available());
    assert_eq!(
        host.support().unsupported_features(),
        vec!["app_owned_readback"]
    );
    let mut normalizer = BrowserInputNormalizer::new(
        SurfaceId("web".to_owned()),
        WindowId("browser".to_owned()),
        1,
        32,
    )
    .unwrap();
    assert!(host.accept_event(normalizer.focus(true), 1).unwrap());
    assert_eq!(host.queued_event_count(), 1);
    let event = host.drain_events().next().unwrap();
    assert!(matches!(event, BrowserHostEvent::Input { .. }));
}

#[test]
fn websocket_protocol_sets_are_deterministic() {
    let protocols = ["v2".to_owned(), "v1".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(protocols.into_iter().collect::<Vec<_>>(), ["v1", "v2"]);
}

#[test]
fn map_tile_templates_bind_renderer_identity_to_declared_fetch_origin() {
    let mut fetch =
        BrowserFetchCapability::https_endpoint("public_tiles", "tiles.example.test", "/raster");
    fetch.methods = [FetchMethod::Get].into_iter().collect();
    let fetch = BrowserFetchCapabilities::new([fetch]).unwrap();
    let templates = BrowserMapTileCapabilities::new(
        [BrowserMapTileTemplateCapability {
            name: "labelled_raster".to_owned(),
            fetch_capability: "public_tiles".to_owned(),
            path_template: "/raster/{z}/{x}/{y}@{scale}x.png".to_owned(),
        }],
        &fetch,
    )
    .unwrap();
    let request = MapTileFetchRequest {
        viewport: DocumentNodeId("map".to_owned()),
        identity: MapTileRequestIdentity {
            generation: MapViewportGeneration(3),
            tile: MapTileCacheKey {
                source: MapTileSourceId("base".to_owned()),
                z: 7,
                x: 42,
                y: 51,
                scale: MapTileScaleKey::from_viewport_scale(2.0).unwrap(),
            },
        },
        url_template_capability: "labelled_raster".to_owned(),
        allowed_origins: vec!["https://tiles.example.test".to_owned()],
        expected_tile_size: 256,
    };
    let built = templates
        .build_fetch_request(9, &request, "https://app.example.test")
        .unwrap();
    assert_eq!(built.capability, "public_tiles");
    assert_eq!(built.path_and_query, "/raster/7/42/51@2x.png");
    assert!(matches!(built.method, FetchMethod::Get));

    let mut denied = request;
    denied.allowed_origins = vec!["https://other.example.test".to_owned()];
    assert!(
        templates
            .build_fetch_request(10, &denied, "https://app.example.test")
            .is_err()
    );
}
