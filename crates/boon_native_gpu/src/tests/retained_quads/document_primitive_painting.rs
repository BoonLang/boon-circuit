#[test]
fn renderer_paints_external_document_border_primitives() {
    let style_identity = test_style_identity();
    let document_scene = DocumentRenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 90.0,
        },
        items: vec![boon_document::RenderSceneItem {
            node: DocumentNodeId("bordered".to_owned()),
            retained_chunk_id: "chunk:bordered".to_owned(),
            source_kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 12.0,
                y: 10.0,
                width: 80.0,
                height: 36.0,
            },
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, 12.0, 10.0],
            style_identity,
            dependency_set: vec!["prelowered:bordered".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 12,
        }],
        visual_primitives: vec![
            RenderVisualPrimitive {
                node: DocumentNodeId("bordered".to_owned()),
                retained_chunk_id: "chunk:bordered".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: Rect {
                    x: 12.0,
                    y: 10.0,
                    width: 80.0,
                    height: 36.0,
                },
                clip: None,
                radius: 0.0,
                stroke_width: 0.0,
                color: [220, 230, 240, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:fill".to_owned()],
            },
            RenderVisualPrimitive {
                node: DocumentNodeId("bordered".to_owned()),
                retained_chunk_id: "chunk:bordered".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::BorderBottom,
                bounds: Rect {
                    x: 12.0,
                    y: 10.0,
                    width: 80.0,
                    height: 36.0,
                },
                clip: None,
                radius: 0.0,
                stroke_width: 4.0,
                color: [16, 32, 48, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:border-bottom".to_owned()],
            },
        ],
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 2,
            rendered_rect_count: 2,
            cap_hit: false,
        },
    };

    let scene = render_scene_from_document_scene(&document_scene, 160, 90);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 160.0, 90.0);
    let (positions, colors) = flatten_quad_batches(&batches);

    assert_eq!(metrics.rendered_rect_count, 2);
    assert!(
        positions.len() >= 24,
        "fill plus border should emit at least two rect quads"
    );
    let expected_border_color = rgba8_from_f32(linear_f32_from_rgba8([16, 32, 48, 255]));
    assert!(
        colors
            .chunks_exact(4)
            .any(|color| color == expected_border_color),
        "external border primitive color should be present in GPU quad data"
    );
}


#[test]
fn renderer_paints_external_document_material_layer_primitives() {
    let style_identity = test_style_identity();
    let document_scene = DocumentRenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 90.0,
        },
        items: vec![boon_document::RenderSceneItem {
            node: DocumentNodeId("glass".to_owned()),
            retained_chunk_id: "chunk:glass".to_owned(),
            source_kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 12.0,
                y: 10.0,
                width: 80.0,
                height: 36.0,
            },
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, 12.0, 10.0],
            style_identity,
            dependency_set: vec!["prelowered:glass".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 18,
        }],
        visual_primitives: vec![
            RenderVisualPrimitive {
                node: DocumentNodeId("glass".to_owned()),
                retained_chunk_id: "chunk:glass".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::FrostedMaterialLayer,
                bounds: Rect {
                    x: 10.0,
                    y: 8.0,
                    width: 84.0,
                    height: 40.0,
                },
                clip: None,
                radius: 10.0,
                stroke_width: 0.0,
                color: [255, 255, 255, 12],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:frosted-material-layer".to_owned()],
            },
            RenderVisualPrimitive {
                node: DocumentNodeId("glass".to_owned()),
                retained_chunk_id: "chunk:glass".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: Rect {
                    x: 12.0,
                    y: 10.0,
                    width: 80.0,
                    height: 36.0,
                },
                clip: None,
                radius: 8.0,
                stroke_width: 0.0,
                color: [220, 230, 240, 180],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:fill".to_owned()],
            },
            RenderVisualPrimitive {
                node: DocumentNodeId("glass".to_owned()),
                retained_chunk_id: "chunk:glass".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::MaterialHighlight,
                bounds: Rect {
                    x: 12.0,
                    y: 10.0,
                    width: 80.0,
                    height: 4.0,
                },
                clip: None,
                radius: 8.0,
                stroke_width: 0.0,
                color: [255, 255, 255, 32],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:material-highlight-top".to_owned()],
            },
        ],
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 3,
            rendered_rect_count: 3,
            cap_hit: false,
        },
    };

    let scene = render_scene_from_document_scene(&document_scene, 160, 90);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 160.0, 90.0);
    let (positions, colors) = flatten_quad_batches(&batches);

    assert_eq!(metrics.rendered_rect_count, 3);
    assert!(
        positions.len() >= 36,
        "frosted layer, fill, and highlight should emit at least three rect quads"
    );
    for expected in [[255, 255, 255, 12], [255, 255, 255, 32]] {
        let expected = rgba8_from_f32(linear_f32_from_rgba8(expected));
        assert!(
            colors.chunks_exact(4).any(|color| color == expected),
            "external material primitive color should be present in GPU quad data"
        );
    }
}


#[test]
fn renderer_paints_external_document_shadow_primitives() {
    let style_identity = test_style_identity();
    let document_scene = DocumentRenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 90.0,
        },
        items: vec![boon_document::RenderSceneItem {
            node: DocumentNodeId("shadowed".to_owned()),
            retained_chunk_id: "chunk:shadowed".to_owned(),
            source_kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 12.0,
                y: 10.0,
                width: 80.0,
                height: 36.0,
            },
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, 12.0, 10.0],
            style_identity,
            dependency_set: vec!["prelowered:shadowed".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 12,
        }],
        visual_primitives: vec![
            RenderVisualPrimitive {
                node: DocumentNodeId("shadowed".to_owned()),
                retained_chunk_id: "chunk:shadowed".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Shadow,
                bounds: Rect {
                    x: 10.0,
                    y: 12.0,
                    width: 84.0,
                    height: 40.0,
                },
                clip: None,
                radius: 10.0,
                stroke_width: 0.0,
                color: [12, 24, 48, 96],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:box-shadow-1".to_owned()],
            },
            RenderVisualPrimitive {
                node: DocumentNodeId("shadowed".to_owned()),
                retained_chunk_id: "chunk:shadowed".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: Rect {
                    x: 12.0,
                    y: 10.0,
                    width: 80.0,
                    height: 36.0,
                },
                clip: None,
                radius: 8.0,
                stroke_width: 0.0,
                color: [240, 244, 248, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:fill".to_owned()],
            },
        ],
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 2,
            rendered_rect_count: 2,
            cap_hit: false,
        },
    };

    let scene = render_scene_from_document_scene(&document_scene, 160, 90);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 160.0, 90.0);
    let (positions, colors) = flatten_quad_batches(&batches);

    assert_eq!(metrics.rendered_rect_count, 2);
    assert!(
        positions.len() >= 24,
        "shadow plus fill should emit at least two rect quads"
    );
    let expected_shadow_color = rgba8_from_f32(linear_f32_from_rgba8([12, 24, 48, 96]));
    assert!(
        colors
            .chunks_exact(4)
            .any(|color| color == expected_shadow_color),
        "external shadow primitive color should be present in GPU quad data"
    );
}


#[test]
fn renderer_paints_external_document_checkbox_raster_primitives() {
    let style_identity = test_style_identity();
    let document_scene = DocumentRenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 96.0,
            height: 96.0,
        },
        items: vec![boon_document::RenderSceneItem {
            node: DocumentNodeId("check".to_owned()),
            retained_chunk_id: "chunk:check".to_owned(),
            source_kind: DocumentNodeKind::Checkbox,
            bounds: Rect {
                x: 24.0,
                y: 24.0,
                width: 24.0,
                height: 24.0,
            },
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, 24.0, 24.0],
            style_identity,
            dependency_set: vec!["prelowered:check".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 200,
        }],
        visual_primitives: vec![
            RenderVisualPrimitive {
                node: DocumentNodeId("check".to_owned()),
                retained_chunk_id: "chunk:check".to_owned(),
                source_kind: DocumentNodeKind::Checkbox,
                primitive: RenderVisualPrimitiveKind::Checkbox,
                bounds: Rect {
                    x: 24.0,
                    y: 24.0,
                    width: 24.0,
                    height: 24.0,
                },
                clip: None,
                radius: 9.5,
                stroke_width: 2.0,
                color: [16, 96, 72, 255],
                secondary_color: [224, 248, 240, 255],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:checkbox-circle".to_owned()],
            },
            RenderVisualPrimitive {
                node: DocumentNodeId("check".to_owned()),
                retained_chunk_id: "chunk:check".to_owned(),
                source_kind: DocumentNodeKind::Checkbox,
                primitive: RenderVisualPrimitiveKind::CheckboxCheckmark,
                bounds: Rect {
                    x: 24.0,
                    y: 24.0,
                    width: 24.0,
                    height: 24.0,
                },
                clip: None,
                radius: 0.0,
                stroke_width: 3.0,
                color: [0, 128, 96, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: vec![[31.92, 37.2], [34.8, 40.08], [40.8, 32.4]],
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:checkbox-checkmark".to_owned()],
            },
        ],
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 2,
            rendered_rect_count: 2,
            cap_hit: false,
        },
    };

    let scene = render_scene_from_document_scene(&document_scene, 96, 96);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 96.0, 96.0);
    let (positions, colors) = flatten_quad_batches(&batches);

    assert_eq!(metrics.rendered_rect_count, 2);
    let vertex_count = positions.len() / 2;
    assert!(
        (100..=260).contains(&vertex_count),
        "checkbox raster primitives should render with bounded geometry, got {vertex_count} vertices"
    );
    for expected in [[16, 96, 72, 255], [224, 248, 240, 255], [0, 128, 96, 255]] {
        let expected = rgba8_from_f32(linear_f32_from_rgba8(expected));
        assert!(
            colors.chunks_exact(4).any(|color| color == expected),
            "external checkbox raster primitive color should be present in GPU quad data"
        );
    }
}
