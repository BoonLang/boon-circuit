use boon_document::{
    ComputedStyleIdentity, DocumentNodeId, DocumentNodeKind, MapCamera, MapInteractionPolicy,
    MapTileSourceId, MapTileSourceRef, MapViewportBounds, MapViewportDescriptor,
    MapViewportGeneration, Rect, RenderMapViewport, RenderScene, RenderSceneItem,
    RenderSceneMetrics,
};
use boon_host::{
    HostEvent, LogicalSize, PhysicalSize, PointerButton, PointerEvent, PointerPhase, SurfaceId,
    SurfaceResizeEvent, WheelEvent,
};
use boon_web_host::{MapViewportHostController, MapViewportHostEvent};

fn descriptor(generation: u64) -> MapViewportDescriptor {
    MapViewportDescriptor {
        generation: MapViewportGeneration(generation),
        camera: MapCamera {
            longitude: 10.75,
            latitude: 59.91,
            zoom: 4.0,
            bearing: 0.0,
        },
        bounds: MapViewportBounds {
            width: 320.0,
            height: 200.0,
            scale: 1.0,
        },
        tile_source: MapTileSourceRef {
            id: MapTileSourceId("generic-raster".to_owned()),
            url_template_capability: "generic_xyz".to_owned(),
            min_zoom: 0,
            max_zoom: 12,
            tile_size: 256,
            attribution: "Generic fixture".to_owned(),
            allowed_origins: vec!["https://tiles.example.test".to_owned()],
        },
        interaction: MapInteractionPolicy {
            pan: true,
            wheel_zoom: true,
            pinch_zoom: true,
            keyboard_zoom: true,
        },
        overlays: Vec::new(),
    }
}

fn scene(generation: u64) -> RenderScene {
    let descriptor = descriptor(generation);
    let node = DocumentNodeId("generic.map".to_owned());
    let bounds = Rect {
        x: 20.0,
        y: 30.0,
        width: 320.0,
        height: 200.0,
    };
    let style_identity = ComputedStyleIdentity {
        style_id: 1,
        layout_id: 2,
        paint_id: 3,
        material_id: 77,
        font_id: 4,
        pseudo_state_id: 5,
    };
    RenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
        },
        items: vec![RenderSceneItem {
            node: node.clone(),
            retained_chunk_id: "generic-map-chunk".to_owned(),
            source_kind: DocumentNodeKind::MapViewport,
            bounds,
            clip: Some(bounds),
            transform: [1.0, 0.0, 0.0, 1.0, bounds.x, bounds.y],
            style_identity,
            dependency_set: vec!["physical-material:77".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 6,
        }],
        visual_primitives: Vec::new(),
        map_viewports: vec![RenderMapViewport {
            node,
            retained_chunk_id: "generic-map-chunk".to_owned(),
            bounds,
            clip: Some(bounds),
            visible_tiles: descriptor.visible_xyz_tiles(1).unwrap(),
            descriptor,
            overlay_primitives: Vec::new(),
            overlay_text_runs: Vec::new(),
            hit_regions: Vec::new(),
        }],
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: RenderSceneMetrics::default(),
    }
}

fn pointer(phase: PointerPhase, x: f32, y: f32) -> HostEvent {
    HostEvent::Pointer(PointerEvent {
        surface: SurfaceId("generic-map-surface".to_owned()),
        x,
        y,
        phase,
        button: matches!(phase, PointerPhase::Down | PointerPhase::Up)
            .then_some(PointerButton::Primary),
    })
}

#[test]
fn retained_host_camera_handles_drag_wheel_pinch_resize_and_app_authority() {
    let base = scene(1);
    let mut controller = MapViewportHostController::default();
    assert!(controller.consumes_host_event(&base, &pointer(PointerPhase::Down, 100.0, 100.0)));
    assert!(
        !controller
            .handle_host_event(&base, &pointer(PointerPhase::Down, 100.0, 100.0))
            .unwrap()
    );
    assert!(
        controller
            .handle_host_event(&base, &pointer(PointerPhase::Move, 140.0, 115.0))
            .unwrap()
    );
    assert!(
        controller
            .handle_host_event(
                &base,
                &HostEvent::Wheel(WheelEvent {
                    surface: SurfaceId("generic-map-surface".to_owned()),
                    x: 140.0,
                    y: 115.0,
                    delta_x: 0.0,
                    delta_y: -120.0,
                }),
            )
            .unwrap()
    );
    controller.touch_start(&base, 1, 100.0, 100.0);
    controller.touch_start(&base, 2, 180.0, 100.0);
    assert!(controller.pinch(&base, 140.0, 100.0, 1.2).unwrap());

    let rendered = controller.scene_for_render(&base).unwrap();
    let map = &rendered.map_viewports[0];
    assert_ne!(
        map.descriptor.camera,
        base.map_viewports[0].descriptor.camera
    );
    assert!(map.descriptor.generation.0 > 1);
    assert_eq!(rendered.items[0].style_identity.material_id, 77);
    assert_eq!(rendered.items[0].dependency_set, ["physical-material:77"]);
    assert!(
        controller
            .drain_events()
            .any(|event| matches!(event, MapViewportHostEvent::CameraChanged { .. }))
    );

    let resize = HostEvent::Resize(SurfaceResizeEvent {
        surface: SurfaceId("generic-map-surface".to_owned()),
        logical_size: LogicalSize {
            width: 640.0,
            height: 480.0,
        },
        scale: 2.0,
        physical_size: PhysicalSize {
            width: 1280,
            height: 960,
        },
        epoch: 2,
    });
    assert!(controller.handle_host_event(&base, &resize).unwrap());
    let resized = controller.scene_for_render(&base).unwrap();
    assert_eq!(resized.map_viewports[0].descriptor.bounds.scale, 2.0);

    let mut authoritative = scene(50);
    authoritative.map_viewports[0].descriptor.camera.longitude = -73.98;
    let reset = controller.scene_for_render(&authoritative).unwrap();
    assert_eq!(reset.map_viewports[0].descriptor.camera.longitude, -73.98);
    assert_eq!(reset.map_viewports[0].descriptor.generation.0, 50);
}

#[test]
fn pointer_outside_map_is_not_consumed_or_patched() {
    let base = scene(1);
    let mut controller = MapViewportHostController::default();
    let event = pointer(PointerPhase::Down, 500.0, 400.0);
    assert!(!controller.consumes_host_event(&base, &event));
    assert!(!controller.handle_host_event(&base, &event).unwrap());
    assert_eq!(controller.scene_for_render(&base).unwrap(), base);
}

#[test]
fn touch_gestures_suppress_the_duplicate_browser_pointer_stream() {
    let base = scene(1);
    let mut controller = MapViewportHostController::default();
    controller.touch_start(&base, 7, 100.0, 100.0);
    let duplicate = pointer(PointerPhase::Move, 130.0, 110.0);
    assert!(controller.consumes_host_event(&base, &duplicate));
    assert!(!controller.handle_host_event(&base, &duplicate).unwrap());
    assert!(controller.touch_move(7, 130.0, 110.0).unwrap());
    controller.touch_end(7);
    assert_eq!(controller.active_touch_count(), 0);
}
