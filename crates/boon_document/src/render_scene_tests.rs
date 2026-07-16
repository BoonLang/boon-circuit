use super::*;
use crate::{
    AccessibilityTree, DocumentDerivedIndexBundle, DocumentFrame, DocumentNode, LayoutMetrics,
    MapCamera, MapCoordinate, MapHitIdentity, MapInteractionPolicy, MapOverlayDescriptor,
    MapOverlayGeometry, MapOverlayId, MapOverlayPaint, MapTileSourceId, MapTileSourceRef,
    MapViewportBounds, MapViewportDescriptor, MapViewportGeneration, TextValue,
};

fn identity() -> ComputedStyleIdentity {
    ComputedStyleIdentity {
        style_id: 1,
        layout_id: 2,
        paint_id: 3,
        material_id: 4,
        font_id: 5,
        pseudo_state_id: 6,
    }
}

fn generic_map_descriptor(generation: u64) -> MapViewportDescriptor {
    let overlay = |id: &str, geometry: MapOverlayGeometry, z_order: i32| MapOverlayDescriptor {
        id: MapOverlayId(id.to_owned()),
        hit_identity: MapHitIdentity(format!("hit:{id}")),
        z_order,
        selected: id == "selected-point",
        focused: false,
        paint: MapOverlayPaint {
            fill: Some("#267a66".to_owned()),
            stroke: Some("#ffffff".to_owned()),
            stroke_width: 2.0,
            opacity: 1.0,
        },
        geometry,
    };
    MapViewportDescriptor {
        generation: MapViewportGeneration(generation),
        camera: MapCamera {
            longitude: 10.75,
            latitude: 59.91,
            zoom: 5.0,
            bearing: 12.0,
        },
        bounds: MapViewportBounds {
            width: 280.0,
            height: 180.0,
            scale: 1.0,
        },
        tile_source: MapTileSourceRef {
            id: MapTileSourceId("generic-fixture".to_owned()),
            url_template_capability: "generic_fixture_tiles".to_owned(),
            min_zoom: 0,
            max_zoom: 10,
            tile_size: 256,
            attribution: "Generic fixture".to_owned(),
            allowed_origins: vec!["boon-local://generic-map".to_owned()],
        },
        interaction: MapInteractionPolicy {
            pan: true,
            wheel_zoom: true,
            pinch_zoom: true,
            keyboard_zoom: true,
        },
        overlays: vec![
            overlay(
                "selected-point",
                MapOverlayGeometry::Point {
                    position: MapCoordinate {
                        longitude: 10.75,
                        latitude: 59.91,
                    },
                    radius: 8.0,
                    symbol_ref: None,
                },
                5,
            ),
            overlay(
                "route",
                MapOverlayGeometry::Polyline {
                    points: vec![
                        MapCoordinate {
                            longitude: 10.70,
                            latitude: 59.90,
                        },
                        MapCoordinate {
                            longitude: 10.80,
                            latitude: 59.92,
                        },
                    ],
                },
                2,
            ),
            overlay(
                "area",
                MapOverlayGeometry::Polygon {
                    rings: vec![vec![
                        MapCoordinate {
                            longitude: 10.72,
                            latitude: 59.90,
                        },
                        MapCoordinate {
                            longitude: 10.78,
                            latitude: 59.90,
                        },
                        MapCoordinate {
                            longitude: 10.75,
                            latitude: 59.94,
                        },
                    ]],
                },
                1,
            ),
            overlay(
                "label",
                MapOverlayGeometry::Label {
                    position: MapCoordinate {
                        longitude: 10.77,
                        latitude: 59.93,
                    },
                    text: "Generic map".to_owned(),
                    collision_priority: 1,
                    font_size: 13.0,
                },
                3,
            ),
        ],
    }
}

fn frame_with_item(item: DisplayItem) -> LayoutFrame {
    LayoutFrame {
        display_list: vec![item],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    }
}

#[test]
fn interactive_controls_have_generic_hover_and_focus_fallbacks() {
    let mut style = StyleMap::new();
    style.insert(
        "background".to_owned(),
        StyleValue::Text("#ffffff".to_owned()),
    );
    style.insert("__hover".to_owned(), StyleValue::Bool(true));
    let item = DisplayItem {
        node: DocumentNodeId("button".to_owned()),
        kind: DocumentNodeKind::Button,
        bounds: Rect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 24.0,
        },
        style,
        text: Some("Button".to_owned()),
        map_viewport: None,
        focused: true,
        style_identity: identity(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame_with_item(item), 160, 80, &mut columns);

    assert!(primitives.iter().any(|primitive| {
        primitive
            .dependency_set
            .iter()
            .any(|dependency| dependency == "primitive:default-hover")
    }));
    assert!(primitives.iter().any(|primitive| {
        primitive.primitive == RenderVisualPrimitiveKind::Border
            && primitive.stroke_width == 2.0
            && primitive.color == [44, 107, 216, 255]
    }));
}

#[test]
fn render_scene_contract_is_renderer_neutral_and_serializable() {
    let item = RenderSceneItem {
        node: DocumentNodeId("row-1".to_owned()),
        retained_chunk_id: "chunk:row-1".to_owned(),
        source_kind: DocumentNodeKind::Row,
        bounds: Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        },
        clip: None,
        transform: [1.0, 0.0, 0.0, 1.0, 1.0, 2.0],
        style_identity: ComputedStyleIdentity {
            style_id: 1,
            layout_id: 2,
            paint_id: 3,
            material_id: 4,
            font_id: 5,
            pseudo_state_id: 6,
        },
        dependency_set: vec!["node:row-1".to_owned()],
        texture_asset_refs: Vec::new(),
        estimated_vertex_count: 6,
    };
    let scene = RenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        },
        items: vec![item],
        visual_primitives: Vec::new(),
        map_viewports: Vec::new(),
        overlay_visual_primitives: Vec::new(),
        quad_batches: vec![RenderQuadBatch {
            retained_chunk_id: Some("chunk:row-1".to_owned()),
            texture: RenderTextureRef::Solid,
            positions: vec![0.0, 1.0],
            colors: vec![0xff00_0000],
            uvs: vec![0.0, 0.0],
        }],
        text_runs: Vec::new(),
        metrics: RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 0,
            rendered_rect_count: 1,
            cap_hit: false,
        },
    };

    assert_eq!(
        scene.quad_batches[0].retained_chunk_id.as_deref(),
        Some("chunk:row-1")
    );
}

#[test]
fn render_visual_primitives_lower_default_fill_asset_and_checkbox_before_gpu() {
    let mut row_style = StyleMap::new();
    row_style.insert(
        "asset_url".to_owned(),
        StyleValue::Text("asset://logo".to_owned()),
    );
    let row = DisplayItem {
        node: DocumentNodeId("row".to_owned()),
        kind: DocumentNodeKind::Row,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
        },
        style: row_style,
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let mut checkbox_style = StyleMap::new();
    checkbox_style.insert("checked".to_owned(), StyleValue::Bool(true));
    let checkbox = DisplayItem {
        node: DocumentNodeId("check".to_owned()),
        kind: DocumentNodeKind::Checkbox,
        bounds: Rect {
            x: 0.0,
            y: 24.0,
            width: 20.0,
            height: 20.0,
        },
        style: checkbox_style,
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let frame = LayoutFrame {
        display_list: vec![row, checkbox],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 320, 200, &mut columns);

    assert!(
        primitives.iter().any(|primitive| {
            primitive.primitive == RenderVisualPrimitiveKind::ViewportBackground
        })
    );
    assert!(primitives.iter().any(|primitive| {
        primitive.node.0 == "row" && primitive.primitive == RenderVisualPrimitiveKind::Fill
    }));
    assert!(primitives.iter().any(|primitive| {
        primitive.node.0 == "row"
            && primitive.primitive == RenderVisualPrimitiveKind::Asset
            && matches!(
                &primitive.texture,
                RenderTextureRef::Asset { url, asset_ref, .. }
                    if url == "asset://logo"
                        && asset_ref.id.starts_with("asset:svg-data-url:")
                        && asset_ref.blob_ref.id.starts_with("blob:sha256:")
                        && asset_ref.blob_ref.sha256.len() == 64
                        && asset_ref.width == 100
                        && asset_ref.height == 20
            )
    }));
    assert!(primitives.iter().any(|primitive| {
        primitive.node.0 == "check" && primitive.primitive == RenderVisualPrimitiveKind::Checkbox
    }));
    assert!(primitives.iter().any(|primitive| {
        primitive.node.0 == "check"
            && primitive.primitive == RenderVisualPrimitiveKind::CheckboxCheckmark
    }));
}

#[test]
fn render_visual_primitives_lower_checkbox_raster_semantics_before_gpu() {
    let mut checkbox_style = StyleMap::new();
    checkbox_style.insert("checked".to_owned(), StyleValue::Bool(true));
    checkbox_style.insert(
        "checked_border".to_owned(),
        StyleValue::Text("#112233".to_owned()),
    );
    checkbox_style.insert(
        "checkbox_background".to_owned(),
        StyleValue::Text("#ddeeff".to_owned()),
    );
    checkbox_style.insert("checkbox_border_width".to_owned(), StyleValue::Number(2.5));
    checkbox_style.insert("checkbox_aa".to_owned(), StyleValue::Number(1.5));
    checkbox_style.insert(
        "checkbox_cast_color".to_owned(),
        StyleValue::Text("#00000040".to_owned()),
    );
    checkbox_style.insert("checkbox_cast_y".to_owned(), StyleValue::Number(2.0));
    checkbox_style.insert("checkbox_cast_blur".to_owned(), StyleValue::Number(4.0));
    checkbox_style.insert(
        "checkbox_inner_shadow".to_owned(),
        StyleValue::Text("#22334455".to_owned()),
    );
    checkbox_style.insert(
        "checkbox_highlight".to_owned(),
        StyleValue::Text("#ffffff80".to_owned()),
    );
    checkbox_style.insert("check_width".to_owned(), StyleValue::Number(3.5));
    checkbox_style.insert("check_aa".to_owned(), StyleValue::Number(1.1));
    checkbox_style.insert(
        "check_color".to_owned(),
        StyleValue::Text("#00aa77".to_owned()),
    );
    let checkbox = DisplayItem {
        node: DocumentNodeId("check".to_owned()),
        kind: DocumentNodeKind::Checkbox,
        bounds: Rect {
            x: 8.0,
            y: 10.0,
            width: 24.0,
            height: 24.0,
        },
        style: checkbox_style,
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let frame = LayoutFrame {
        display_list: vec![checkbox],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 120, 80, &mut columns);
    let checkbox_primitives: Vec<_> = primitives
        .iter()
        .filter(|primitive| primitive.node.0 == "check")
        .collect();
    let kinds: Vec<_> = checkbox_primitives
        .iter()
        .map(|primitive| primitive.primitive)
        .collect();

    assert_eq!(
        kinds,
        vec![
            RenderVisualPrimitiveKind::Fill,
            RenderVisualPrimitiveKind::MaterialHighlight,
            RenderVisualPrimitiveKind::CheckboxCastShadow,
            RenderVisualPrimitiveKind::Checkbox,
            RenderVisualPrimitiveKind::CheckboxInnerShadow,
            RenderVisualPrimitiveKind::CheckboxHighlight,
            RenderVisualPrimitiveKind::CheckboxCheckmark,
        ],
        "checkbox raster descriptors must preserve fill/cast/main/inner/highlight/check paint order"
    );
    let circle = checkbox_primitives
        .iter()
        .find(|primitive| primitive.primitive == RenderVisualPrimitiveKind::Checkbox)
        .expect("main checkbox circle descriptor");
    assert_eq!(circle.color, [17, 34, 51, 255]);
    assert_eq!(circle.secondary_color, [221, 238, 255, 255]);
    assert_eq!(circle.stroke_width, 2.5);
    assert_eq!(circle.antialias, 1.5);
    let checkmark = checkbox_primitives
        .iter()
        .find(|primitive| primitive.primitive == RenderVisualPrimitiveKind::CheckboxCheckmark)
        .expect("checkbox checkmark descriptor");
    assert_eq!(checkmark.stroke_width, 3.5);
    assert_eq!(checkmark.antialias, 1.1);
    assert_eq!(checkmark.control_points.len(), 3);
    assert!(
        checkmark
            .dependency_set
            .iter()
            .any(|dependency| dependency == "primitive:checkbox-checkmark")
    );
}

#[test]
fn render_visual_primitives_skip_checkbox_raster_when_asset_icon_covers_control() {
    let checkbox = DisplayItem {
        node: DocumentNodeId("check".to_owned()),
        kind: DocumentNodeKind::Checkbox,
        bounds: Rect {
            x: 8.0,
            y: 10.0,
            width: 24.0,
            height: 24.0,
        },
        style: StyleMap::new(),
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let mut icon_style = StyleMap::new();
    icon_style.insert(
        "asset_url".to_owned(),
        StyleValue::Text("asset://checkbox".to_owned()),
    );
    let icon = DisplayItem {
        node: DocumentNodeId("check-icon".to_owned()),
        kind: DocumentNodeKind::Stack,
        bounds: Rect {
            x: 9.0,
            y: 11.0,
            width: 22.0,
            height: 22.0,
        },
        style: icon_style,
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let frame = LayoutFrame {
        display_list: vec![checkbox, icon],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 120, 80, &mut columns);

    assert!(
        !primitives.iter().any(|primitive| {
            primitive.node.0 == "check"
                && matches!(
                    primitive.primitive,
                    RenderVisualPrimitiveKind::CheckboxCastShadow
                        | RenderVisualPrimitiveKind::Checkbox
                        | RenderVisualPrimitiveKind::CheckboxInnerShadow
                        | RenderVisualPrimitiveKind::CheckboxHighlight
                        | RenderVisualPrimitiveKind::CheckboxCheckmark
                )
        }),
        "checkbox raster descriptors should be skipped when an asset icon covers the control"
    );
    assert!(primitives.iter().any(|primitive| {
        primitive.node.0 == "check-icon" && primitive.primitive == RenderVisualPrimitiveKind::Asset
    }));
}

#[test]
fn render_visual_primitives_apply_material_fill_adjustments_before_gpu() {
    let mut base_style = StyleMap::new();
    base_style.insert("bg".to_owned(), StyleValue::Text("#ccaa8866".to_owned()));
    let mut material_style = base_style.clone();
    material_style.insert("transparency".to_owned(), StyleValue::Number(0.35));
    material_style.insert("refraction".to_owned(), StyleValue::Number(1.6));
    material_style.insert("frosted_blur".to_owned(), StyleValue::Number(18.0));
    material_style.insert("frosted_saturate".to_owned(), StyleValue::Number(1.28));
    material_style.insert("gloss".to_owned(), StyleValue::Number(0.8));
    material_style.insert("metal".to_owned(), StyleValue::Number(0.45));
    let frame_for_style = |style| LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("material".to_owned()),
            kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 10.0,
                y: 12.0,
                width: 90.0,
                height: 36.0,
            },
            style,
            text: None,
            map_viewport: None,
            focused: false,
            style_identity: identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let base_frame = frame_for_style(base_style);
    let base_color = render_visual_primitives(&base_frame, 320, 200, &mut columns)
        .into_iter()
        .find(|primitive| {
            primitive.node.0 == "material" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("base fill primitive")
        .color;
    let material_frame = frame_for_style(material_style);
    let material_color = render_visual_primitives(&material_frame, 320, 200, &mut columns)
        .into_iter()
        .find(|primitive| {
            primitive.node.0 == "material" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("material fill primitive")
        .color;

    assert!(
        material_color[0] > base_color[0],
        "material refraction/frost/gloss should lift red channel before GPU: base={base_color:?}, material={material_color:?}"
    );
    assert!(
        material_color[3] < base_color[3],
        "transparency/frost should reduce alpha before GPU: base={base_color:?}, material={material_color:?}"
    );
    assert_ne!(
        material_color, base_color,
        "material fill adjustment must be encoded in the neutral primitive"
    );
}

#[test]
fn render_visual_primitives_lower_material_layers_before_gpu() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#ccd4e099".to_owned()));
    style.insert("border_radius".to_owned(), StyleValue::Number(12.0));
    style.insert("frosted_blur".to_owned(), StyleValue::Number(18.0));
    style.insert("frosted_saturate".to_owned(), StyleValue::Number(1.28));
    style.insert("glass_highlight".to_owned(), StyleValue::Number(0.8));
    style.insert(
        "glass_highlight_color".to_owned(),
        StyleValue::Text("#ffffffb8".to_owned()),
    );
    style.insert("gloss".to_owned(), StyleValue::Number(1.0));
    style.insert("depth".to_owned(), StyleValue::Number(8.0));
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("glass".to_owned()),
            kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 20.0,
                y: 24.0,
                width: 96.0,
                height: 44.0,
            },
            style,
            text: None,
            map_viewport: None,
            focused: false,
            style_identity: identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 180, 120, &mut columns);
    let frosted_indices: Vec<_> = primitives
        .iter()
        .enumerate()
        .filter_map(|(index, primitive)| {
            (primitive.node.0 == "glass"
                && primitive.primitive == RenderVisualPrimitiveKind::FrostedMaterialLayer)
                .then_some(index)
        })
        .collect();
    let fill_index = primitives
        .iter()
        .position(|primitive| {
            primitive.node.0 == "glass" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("glass fill primitive");
    let highlight_index = primitives
        .iter()
        .position(|primitive| {
            primitive.node.0 == "glass"
                && primitive.primitive == RenderVisualPrimitiveKind::MaterialHighlight
        })
        .expect("glass highlight primitive");

    assert!(
        frosted_indices.len() >= 2,
        "frosted material should lower to visible pre-fill layer primitives"
    );
    assert!(
        frosted_indices.iter().all(|index| *index < fill_index),
        "frosted material layers must paint before the fill"
    );
    assert!(
        highlight_index > fill_index,
        "material highlights must paint after the fill"
    );
    let first_frost = &primitives[frosted_indices[0]];
    assert!(
        first_frost.bounds.width > 96.0 && first_frost.radius > 12.0,
        "frosted layers should encode their expanded bounds and radius before GPU"
    );
    assert!(
        first_frost.dependency_set.iter().any(|dependency| {
            dependency == "primitive:frosted-material-layer"
                || dependency.contains("frosted-material-layer")
        }),
        "frosted layer primitive should carry material dependency identity"
    );
}

#[test]
fn render_visual_primitives_lower_shadows_before_fill_before_gpu() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    style.insert("border_radius".to_owned(), StyleValue::Number(8.0));
    style.insert(
        "box_shadow_1_color".to_owned(),
        StyleValue::Text("#ff000080".to_owned()),
    );
    style.insert("box_shadow_1_y".to_owned(), StyleValue::Number(1.0));
    style.insert("box_shadow_1_spread".to_owned(), StyleValue::Number(1.0));
    style.insert(
        "box_shadow_2_color".to_owned(),
        StyleValue::Text("#0000ff80".to_owned()),
    );
    style.insert("box_shadow_2_y".to_owned(), StyleValue::Number(2.0));
    style.insert("box_shadow_2_spread".to_owned(), StyleValue::Number(2.0));
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("shadowed".to_owned()),
            kind: DocumentNodeKind::Stack,
            bounds: Rect {
                x: 20.0,
                y: 24.0,
                width: 96.0,
                height: 44.0,
            },
            style,
            text: None,
            map_viewport: None,
            focused: false,
            style_identity: identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 180, 120, &mut columns);
    let shadow_primitives: Vec<_> = primitives
        .iter()
        .enumerate()
        .filter(|(_, primitive)| {
            primitive.node.0 == "shadowed"
                && primitive.primitive == RenderVisualPrimitiveKind::Shadow
        })
        .collect();
    let fill_index = primitives
        .iter()
        .position(|primitive| {
            primitive.node.0 == "shadowed" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("shadowed fill primitive");

    assert_eq!(
        shadow_primitives.len(),
        2,
        "rounded zero-blur shadows should lower to one primitive per authored shadow"
    );
    assert!(
        shadow_primitives
            .iter()
            .all(|(index, _)| *index < fill_index),
        "shadows must paint before the fill"
    );
    assert_eq!(
        shadow_primitives[0].1.color,
        [0, 0, 255, 128],
        "CSS shadow list should be lowered in reverse order so shadow 1 paints topmost"
    );
    assert_eq!(
        shadow_primitives[1].1.color,
        [255, 0, 0, 128],
        "shadow 1 should be emitted after shadow 2"
    );
    assert!(
        shadow_primitives[0].1.radius > 8.0,
        "spread should expand rounded shadow radius before GPU"
    );
    assert!(
        shadow_primitives[1]
            .1
            .dependency_set
            .iter()
            .any(|dependency| {
                dependency == "primitive:box-shadow-1" || dependency.contains("box-shadow-1")
            })
    );
}

#[test]
fn render_visual_primitives_lower_borders_after_descendant_fills_before_gpu() {
    let mut parent_style = StyleMap::new();
    parent_style.insert("border".to_owned(), StyleValue::Text("#112233".to_owned()));
    parent_style.insert("border_width".to_owned(), StyleValue::Number(3.0));
    parent_style.insert("border_radius".to_owned(), StyleValue::Number(6.0));
    parent_style.insert(
        "border_bottom".to_owned(),
        StyleValue::Text("#445566".to_owned()),
    );
    parent_style.insert("border_bottom_width".to_owned(), StyleValue::Number(5.0));
    let mut child_style = StyleMap::new();
    child_style.insert("bg".to_owned(), StyleValue::Text("#ddeeff".to_owned()));
    let frame = LayoutFrame {
        display_list: vec![
            DisplayItem {
                node: DocumentNodeId("parent".to_owned()),
                kind: DocumentNodeKind::Stack,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 64.0,
                },
                style: parent_style,
                text: None,
                map_viewport: None,
                focused: false,
                style_identity: identity(),
            },
            DisplayItem {
                node: DocumentNodeId("child".to_owned()),
                kind: DocumentNodeKind::Row,
                bounds: Rect {
                    x: 8.0,
                    y: 8.0,
                    width: 104.0,
                    height: 48.0,
                },
                style: child_style,
                text: None,
                map_viewport: None,
                focused: false,
                style_identity: identity(),
            },
        ],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 320, 200, &mut columns);
    let child_fill_index = primitives
        .iter()
        .position(|primitive| {
            primitive.node.0 == "child" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("child fill primitive");
    let parent_border_index = primitives
        .iter()
        .position(|primitive| {
            primitive.node.0 == "parent" && primitive.primitive == RenderVisualPrimitiveKind::Border
        })
        .expect("parent border primitive");
    let parent_bottom = primitives
        .iter()
        .find(|primitive| {
            primitive.node.0 == "parent"
                && primitive.primitive == RenderVisualPrimitiveKind::BorderBottom
        })
        .expect("parent bottom border primitive");

    assert!(
        parent_border_index > child_fill_index,
        "document lowerer should append borders after descendant fills to preserve paint order"
    );
    assert_eq!(primitives[parent_border_index].stroke_width, 3.0);
    assert_eq!(primitives[parent_border_index].radius, 6.0);
    assert_eq!(parent_bottom.stroke_width, 5.0);
    assert!(
        parent_bottom
            .dependency_set
            .iter()
            .any(|dependency| { dependency == "primitive:border-bottom" })
    );
}

#[test]
fn render_visual_primitives_lower_text_overlays_before_gpu() {
    let mut editor_style = StyleMap::new();
    editor_style.insert("size".to_owned(), StyleValue::Number(10.0));
    editor_style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    editor_style.insert("editor_selection_start".to_owned(), StyleValue::Number(1.0));
    editor_style.insert("editor_selection_end".to_owned(), StyleValue::Number(3.0));
    editor_style.insert(
        "editor_bracket_columns".to_owned(),
        StyleValue::Text("2".to_owned()),
    );
    editor_style.insert("editor_caret_visible".to_owned(), StyleValue::Bool(true));
    editor_style.insert("editor_caret_column".to_owned(), StyleValue::Number(4.0));
    editor_style.insert("underline_if".to_owned(), StyleValue::Bool(true));
    editor_style.insert("strikethrough".to_owned(), StyleValue::Bool(true));
    let editor = DisplayItem {
        node: DocumentNodeId("editor".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 10.0,
            y: 20.0,
            width: 140.0,
            height: 24.0,
        },
        style: editor_style,
        text: Some("abcd".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    };
    let mut input_style = StyleMap::new();
    input_style.insert("size".to_owned(), StyleValue::Number(12.0));
    input_style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
    input_style.insert("caret_visible".to_owned(), StyleValue::Bool(true));
    input_style.insert("caret_column".to_owned(), StyleValue::Number(1.0));
    input_style.insert("selection_start".to_owned(), StyleValue::Number(0.0));
    input_style.insert("selection_end".to_owned(), StyleValue::Number(1.0));
    let input = DisplayItem {
        node: DocumentNodeId("input".to_owned()),
        kind: DocumentNodeKind::TextInput,
        bounds: Rect {
            x: 20.0,
            y: 60.0,
            width: 100.0,
            height: 28.0,
        },
        style: input_style,
        text: Some("xy".to_owned()),
        map_viewport: None,
        focused: true,
        style_identity: identity(),
    };
    let frame = LayoutFrame {
        display_list: vec![editor, input],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut columns = ApproximateTextColumnMeasurer;
    let primitives = render_visual_primitives(&frame, 320, 200, &mut columns);
    let has = |kind| {
        primitives
            .iter()
            .any(|primitive| primitive.primitive == kind)
    };

    assert!(has(RenderVisualPrimitiveKind::EditorSelection));
    assert!(has(RenderVisualPrimitiveKind::EditorBracketHighlight));
    assert!(has(RenderVisualPrimitiveKind::EditorCaret));
    assert!(has(RenderVisualPrimitiveKind::TextInputSelection));
    assert!(has(RenderVisualPrimitiveKind::Underline));
    assert!(has(RenderVisualPrimitiveKind::Strikethrough));
    assert!(has(RenderVisualPrimitiveKind::TextInputCaret));
    assert!(primitives.iter().any(|primitive| {
        primitive.primitive == RenderVisualPrimitiveKind::EditorSelection
            && primitive
                .dependency_set
                .iter()
                .any(|dependency| dependency == "primitive:editor-selection")
            && primitive.bounds.width > 2.0
    }));
}

#[test]
fn multiline_text_input_hit_testing_and_overlays_preserve_line_coordinates() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(10.0));
    style.insert("line_height".to_owned(), StyleValue::Number(20.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    style.insert(
        "vertical_align".to_owned(),
        StyleValue::Text("Top".to_owned()),
    );
    style.insert("caret_visible".to_owned(), StyleValue::Bool(true));
    style.insert("caret_line".to_owned(), StyleValue::Number(1.0));
    style.insert("caret_column".to_owned(), StyleValue::Number(2.0));
    style.insert("selection_start_line".to_owned(), StyleValue::Number(0.0));
    style.insert("selection_start".to_owned(), StyleValue::Number(1.0));
    style.insert("selection_end_line".to_owned(), StyleValue::Number(2.0));
    style.insert("selection_end".to_owned(), StyleValue::Number(1.0));
    let item = DisplayItem {
        node: DocumentNodeId("multiline-input".to_owned()),
        kind: DocumentNodeKind::TextInput,
        bounds: Rect {
            x: 10.0,
            y: 20.0,
            width: 160.0,
            height: 80.0,
        },
        style,
        text: Some("ab\ncdef\ng".to_owned()),
        map_viewport: None,
        focused: true,
        style_identity: identity(),
    };
    let mut columns = ApproximateTextColumnMeasurer;

    assert_eq!(text_position_at(&item, 17.0, 45.0, &mut columns), (1, 1));
    let primitives = render_visual_primitives(&frame_with_item(item), 200, 120, &mut columns);
    let selections = primitives
        .iter()
        .filter(|primitive| primitive.primitive == RenderVisualPrimitiveKind::TextInputSelection)
        .collect::<Vec<_>>();
    assert_eq!(selections.len(), 3);
    let caret = primitives
        .iter()
        .find(|primitive| primitive.primitive == RenderVisualPrimitiveKind::TextInputCaret)
        .expect("multiline caret primitive");
    assert_eq!(caret.bounds.y, 41.0);
}

#[test]
fn lower_layout_frame_to_render_scene_combines_items_primitives_and_text() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#101820".to_owned()));
    style.insert("color".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);

    assert_eq!(scene.items.len(), 1);
    assert!(
        scene
            .visual_primitives
            .iter()
            .any(|primitive| primitive.primitive == RenderVisualPrimitiveKind::Fill)
    );
    assert_eq!(scene.text_runs.len(), 1);
    assert_eq!(scene.metrics.visible_source_item_count, 1);
    assert_eq!(
        scene.metrics.visual_primitive_count as usize,
        scene.visual_primitives.len()
    );
}

#[test]
fn render_scene_patch_updates_fill_and_invalidates_quad_batches() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#101820".to_owned()));
    style.insert("color".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let mut scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    scene.quad_batches.push(RenderQuadBatch {
        retained_chunk_id: Some("old-chunk".to_owned()),
        texture: RenderTextureRef::Solid,
        positions: vec![0.0, 0.0, 1.0, 1.0],
        colors: vec![0],
        uvs: Vec::new(),
    });
    let mut next_identity = identity();
    next_identity.paint_id = 44;
    let report = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::Paint {
                node: DocumentNodeId("label".to_owned()),
                paint: RenderScenePaintPatch::FillColor {
                    color: [222, 111, 0, 255],
                },
                style_identity: next_identity,
                retained_chunk_id: "chunk:label:paint:next".to_owned(),
            }],
        })
        .unwrap();

    assert_eq!(report.patched_items, 1);
    assert_eq!(report.patched_primitives, 1);
    assert_eq!(report.patched_text_runs, 0);
    assert!(scene.quad_batches.is_empty());
    assert_eq!(scene.items[0].style_identity.paint_id, 44);
    assert_eq!(scene.items[0].retained_chunk_id, "chunk:label:paint:next");
    let fill = scene
        .visual_primitives
        .iter()
        .find(|primitive| {
            primitive.node.0 == "label" && primitive.primitive == RenderVisualPrimitiveKind::Fill
        })
        .expect("fill primitive");
    assert_eq!(fill.color, [222, 111, 0, 255]);
    assert_eq!(fill.style_identity.paint_id, 44);
    assert_eq!(fill.retained_chunk_id, "chunk:label:paint:next");
    assert_eq!(scene.text_runs[0].color, [255, 255, 255, 255]);
}

#[test]
fn render_scene_patch_updates_text_color_without_changing_text_shape() {
    let mut style = StyleMap::new();
    style.insert("color".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let mut scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    let original_font_id = scene.text_runs[0].font_id;
    let original_text = scene.text_runs[0].text.clone();
    let mut next_identity = identity();
    next_identity.paint_id = 77;
    let report = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::Paint {
                node: DocumentNodeId("label".to_owned()),
                paint: RenderScenePaintPatch::TextColor {
                    color: [1, 2, 3, 255],
                },
                style_identity: next_identity,
                retained_chunk_id: "chunk:label:text-paint:next".to_owned(),
            }],
        })
        .unwrap();

    assert_eq!(report.patched_items, 1);
    assert_eq!(report.patched_primitives, 0);
    assert_eq!(report.patched_text_runs, 1);
    assert_eq!(scene.text_runs[0].color, [1, 2, 3, 255]);
    assert_eq!(scene.text_runs[0].paint_id, 77);
    assert_eq!(scene.text_runs[0].font_id, original_font_id);
    assert_eq!(scene.text_runs[0].text, original_text);
}

#[test]
fn render_scene_patch_updates_text_content_and_invalidates_quad_batches() {
    let mut style = StyleMap::new();
    style.insert("color".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let mut scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    scene.quad_batches.push(RenderQuadBatch {
        retained_chunk_id: Some("old-chunk".to_owned()),
        texture: RenderTextureRef::Solid,
        positions: vec![0.0, 0.0, 1.0, 1.0],
        colors: vec![0],
        uvs: Vec::new(),
    });
    let original_font_id = scene.text_runs[0].font_id;
    let original_paint_id = scene.text_runs[0].paint_id;
    let report = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::TextContent {
                node: DocumentNodeId("label".to_owned()),
                text: "Done".to_owned(),
                retained_chunk_id: "chunk:label:text:done".to_owned(),
            }],
        })
        .unwrap();

    assert_eq!(report.patched_items, 1);
    assert_eq!(report.patched_primitives, 0);
    assert_eq!(report.patched_text_runs, 1);
    assert!(scene.quad_batches.is_empty());
    assert_eq!(scene.items[0].retained_chunk_id, "chunk:label:text:done");
    assert_eq!(scene.text_runs[0].text, "Done");
    assert_eq!(scene.text_runs[0].font_id, original_font_id);
    assert_eq!(scene.text_runs[0].paint_id, original_paint_id);
}

#[test]
fn render_scene_patch_rejects_stale_scene_references() {
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Stack,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style: StyleMap::new(),
        text: None,
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let mut scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    let error = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::Paint {
                node: DocumentNodeId("missing".to_owned()),
                paint: RenderScenePaintPatch::FillColor {
                    color: [222, 111, 0, 255],
                },
                style_identity: identity(),
                retained_chunk_id: "chunk:missing".to_owned(),
            }],
        })
        .unwrap_err();

    assert!(matches!(
        error,
        PatchApplyError::StaleReference {
            reference_kind: "render_scene_item",
            ..
        }
    ));
}

#[test]
fn checked_render_scene_uses_retained_layout_keys_for_chunk_identity() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#101820".to_owned()));
    style.insert("color".to_owned(), StyleValue::Text("#ffffff".to_owned()));
    let mut document = DocumentFrame::empty("root");
    let mut label = DocumentNode::new("label", DocumentNodeKind::Text);
    label.parent = Some(DocumentNodeId("root".to_owned()));
    label.text = Some(TextValue {
        text: "Ready".to_owned(),
    });
    label.style = style.clone();
    document
        .nodes
        .get_mut(&DocumentNodeId("root".to_owned()))
        .unwrap()
        .children
        .push(DocumentNodeId("label".to_owned()));
    document
        .nodes
        .insert(DocumentNodeId("label".to_owned()), label);
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let bundle = DocumentDerivedIndexBundle::from_frame(&document).unwrap();
    let mut columns = ApproximateTextColumnMeasurer;
    let scene = bundle
        .try_render_scene(&frame, 320, 200, &mut columns)
        .unwrap();

    assert_eq!(scene.items.len(), 1);
    let item_chunk_id = &scene.items[0].retained_chunk_id;
    assert!(
        item_chunk_id.starts_with("chunk:hot:"),
        "checked render scene should use hot retained node identity, got {item_chunk_id}"
    );
    assert!(item_chunk_id.contains("bounds:41000000:41400000:42f00000:41c00000"));
    assert!(scene.visual_primitives.iter().any(|primitive| {
        primitive.node.0 == "label"
            && primitive.primitive == RenderVisualPrimitiveKind::Fill
            && primitive.retained_chunk_id == *item_chunk_id
    }));
}

#[test]
fn checked_render_scene_rejects_real_nodes_missing_retained_keys() {
    let mut style = StyleMap::new();
    style.insert("bg".to_owned(), StyleValue::Text("#101820".to_owned()));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("label".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 8.0,
            y: 12.0,
            width: 120.0,
            height: 24.0,
        },
        style,
        text: Some("Ready".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let bundle = DocumentDerivedIndexBundle::from_frame(&DocumentFrame::empty("root")).unwrap();
    let mut columns = ApproximateTextColumnMeasurer;
    let error = bundle
        .try_render_scene(&frame, 320, 200, &mut columns)
        .unwrap_err();

    assert!(matches!(
        error,
        PatchApplyError::StaleReference {
            reference_kind: "render_scene_hot_id_table",
            ..
        }
    ));
}

#[test]
fn render_text_runs_lower_placeholder_and_widget_defaults_before_gpu() {
    let mut style = StyleMap::new();
    style.insert(
        "placeholder".to_owned(),
        StyleValue::Text("Search".to_owned()),
    );
    style.insert(
        "placeholder_color".to_owned(),
        StyleValue::Text("#8899aa".to_owned()),
    );
    style.insert("placeholder_size".to_owned(), StyleValue::Number(12.0));
    style.insert("center".to_owned(), StyleValue::Bool(true));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("input".to_owned()),
        kind: DocumentNodeKind::TextInput,
        bounds: Rect {
            x: 10.0,
            y: 20.0,
            width: 160.0,
            height: 30.0,
        },
        style,
        text: Some(String::new()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let runs = render_text_runs(&frame, 320, 200, &mut columns);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "Search");
    assert_eq!(runs[0].size, 12.0);
    assert_eq!(runs[0].color, [136, 153, 170, 255]);
    assert_eq!(runs[0].vertical_align, RenderTextVerticalAlign::Center);
}

#[test]
fn render_text_runs_honor_public_text_align_style() {
    let mut style = StyleMap::new();
    style.insert(
        "text_align".to_owned(),
        StyleValue::Text("Center".to_owned()),
    );
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("title".to_owned()),
        kind: DocumentNodeKind::Stack,
        bounds: Rect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 60.0,
        },
        style,
        text: Some("todos".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let runs = render_text_runs(&frame, 320, 200, &mut columns);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].align, RenderTextAlign::Center);
}

#[test]
fn render_text_runs_treat_public_center_style_as_text_alignment() {
    let mut style = StyleMap::new();
    style.insert("center".to_owned(), StyleValue::Bool(true));
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("footer-line".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 0.0,
            y: 20.0,
            width: 300.0,
            height: 20.0,
        },
        style,
        text: Some("Centered footer".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let runs = render_text_runs(&frame, 320, 200, &mut columns);

    assert_eq!(runs[0].align, RenderTextAlign::Center);
}

#[test]
fn checkbox_accessibility_label_is_not_a_visual_text_run() {
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("checkbox".to_owned()),
        kind: DocumentNodeKind::Checkbox,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        },
        style: StyleMap::new(),
        text: Some("Reference[element:todo-title]".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;

    assert!(render_text_runs(&frame, 80, 80, &mut columns).is_empty());
}

#[test]
fn render_text_runs_lower_syntax_spans_and_type_hints_before_gpu() {
    let mut style = StyleMap::new();
    style.insert(
        "font".to_owned(),
        StyleValue::Text("JetBrains Mono".to_owned()),
    );
    style.insert("size".to_owned(), StyleValue::Number(14.0));
    style.insert(
        "syntax_spans".to_owned(),
        StyleValue::RichTextSpans(vec![StyleRichTextSpan {
            text: "SOURCE".to_owned(),
            source_text: Some("SOURCE".to_owned()),
            color: Some("#ff0000".to_owned()),
            font_style: Some("italic".to_owned()),
            font_weight: Some("bold".to_owned()),
        }]),
    );
    style.insert(
        "editor_type_hints".to_owned(),
        StyleValue::EditorTypeHints(vec![StyleEditorTypeHint {
            anchor_column: 6,
            compact_label: "Number".to_owned(),
            ..StyleEditorTypeHint::default()
        }]),
    );
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("line".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 24.0,
        },
        style,
        text: Some("SOURCE".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let runs = render_text_runs(&frame, 320, 200, &mut columns);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].rich_spans.len(), 1);
    assert_eq!(runs[0].rich_spans[0].font_style, RenderFontStyle::Italic);
    assert_eq!(runs[0].rich_spans[0].font_weight, RenderFontWeight(700));
    assert!(runs[1].node.0.ends_with(":type-hint:0"));
    assert_eq!(runs[1].text, ": Number");
    assert_eq!(runs[1].font_style, RenderFontStyle::Italic);
}

#[test]
fn render_text_contract_keys_track_shape_and_placement_inputs() {
    let mut style = StyleMap::new();
    style.insert(
        "font".to_owned(),
        StyleValue::Text("JetBrains Mono".to_owned()),
    );
    style.insert("size".to_owned(), StyleValue::Number(14.0));
    style.insert("line_height".to_owned(), StyleValue::Number(20.0));
    style.insert(
        "syntax_spans".to_owned(),
        StyleValue::RichTextSpans(vec![StyleRichTextSpan {
            text: "SOURCE".to_owned(),
            source_text: Some("SOURCE".to_owned()),
            color: Some("#ff0000".to_owned()),
            font_style: Some("italic".to_owned()),
            font_weight: Some("bold".to_owned()),
        }]),
    );
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("line".to_owned()),
        kind: DocumentNodeKind::Text,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 24.0,
        },
        style,
        text: Some("SOURCE".to_owned()),
        map_viewport: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let run = render_text_runs(&frame, 320, 200, &mut columns)
        .into_iter()
        .next()
        .expect("text run should be lowered");
    let shape_key = run.shape_key();
    let placement_key = run.placement_key();
    let mut moved_run = run.clone();
    moved_run.bounds.x += 12.0;
    let mut taller_run = run.clone();
    taller_run.line_height += 4.0;
    let mut recolored_run = run.clone();
    recolored_run.rich_spans[0].color = [0, 255, 0, 255];

    assert_eq!(shape_key, moved_run.shape_key());
    assert_ne!(placement_key, moved_run.placement_key());
    assert_ne!(shape_key, taller_run.shape_key());
    assert_ne!(shape_key, recolored_run.shape_key());
    assert_eq!(shape_key.rich_spans[0].font_style, RenderFontStyle::Italic);
    assert_eq!(shape_key.rich_spans[0].font_weight, RenderFontWeight(700));
}

#[test]
fn map_viewport_lowers_tiles_overlays_labels_and_generic_hit_identities() {
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("generic-map".to_owned()),
        kind: DocumentNodeKind::MapViewport,
        map_viewport: Some(Box::new(generic_map_descriptor(1))),
        bounds: Rect {
            x: 20.0,
            y: 16.0,
            width: 280.0,
            height: 180.0,
        },
        style: StyleMap::new(),
        text: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let scene = lower_layout_frame_to_render_scene(&frame, 340, 220, &mut columns);

    assert_eq!(scene.map_viewports.len(), 1);
    let map = &scene.map_viewports[0];
    assert!(!map.visible_tiles.is_empty());
    assert!(
        map.overlay_primitives
            .iter()
            .any(|primitive| primitive.primitive == RenderVisualPrimitiveKind::MapCircle)
    );
    assert!(
        map.overlay_primitives
            .iter()
            .any(|primitive| primitive.primitive == RenderVisualPrimitiveKind::MapPolyline)
    );
    assert!(
        map.overlay_primitives
            .iter()
            .any(|primitive| primitive.primitive == RenderVisualPrimitiveKind::MapPolygon)
    );
    assert!(
        map.overlay_text_runs
            .iter()
            .any(|run| run.text == "Generic map")
    );
    let selected = map
        .hit_regions
        .iter()
        .find(|hit| hit.overlay_id.0 == "selected-point")
        .unwrap();
    assert!(!selected.contains(selected.bounds.x, selected.bounds.y));
    let hit = map
        .hit_test(
            selected.bounds.x + selected.bounds.width / 2.0,
            selected.bounds.y + selected.bounds.height / 2.0,
        )
        .unwrap();
    assert_eq!(hit.hit_identity.0, "hit:selected-point");
}

#[test]
fn map_viewport_patch_changes_one_retained_map_and_rejects_stale_identity() {
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("generic-map".to_owned()),
        kind: DocumentNodeKind::MapViewport,
        map_viewport: Some(Box::new(generic_map_descriptor(1))),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 280.0,
            height: 180.0,
        },
        style: StyleMap::new(),
        text: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let mut scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    let item_count = scene.items.len();
    let previous_camera = scene.map_viewports[0].descriptor.camera;
    let patch = scene.map_viewports[0]
        .descriptor
        .patch_for_input(crate::MapViewportInput::PanPixels {
            delta_x: 40.0,
            delta_y: 0.0,
        })
        .unwrap();
    let report = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::MapViewport {
                node: DocumentNodeId("generic-map".to_owned()),
                patch: Box::new(patch),
                retained_chunk_id: "chunk:generic-map:camera:2".to_owned(),
            }],
        })
        .unwrap();
    assert_eq!(scene.items.len(), item_count);
    assert_eq!(report.patched_items, 1);
    assert_ne!(scene.map_viewports[0].descriptor.camera, previous_camera);
    assert_eq!(
        scene.map_viewports[0].retained_chunk_id,
        "chunk:generic-map:camera:2"
    );

    let error = scene
        .apply_patch(&RenderScenePatch {
            operations: vec![RenderScenePatchOperation::MapViewport {
                node: DocumentNodeId("missing-map".to_owned()),
                patch: Box::new(MapViewportDescriptorPatch::default()),
                retained_chunk_id: "missing".to_owned(),
            }],
        })
        .unwrap_err();
    assert!(matches!(error, PatchApplyError::StaleReference { .. }));
}

#[test]
fn map_label_collision_keeps_the_highest_generic_priority() {
    let mut descriptor = generic_map_descriptor(1);
    descriptor
        .overlays
        .retain(|overlay| !matches!(overlay.geometry, MapOverlayGeometry::Label { .. }));
    for (id, text, priority) in [("low", "Low", 1), ("high", "High", 10)] {
        descriptor.overlays.push(MapOverlayDescriptor {
            id: MapOverlayId(id.to_owned()),
            hit_identity: MapHitIdentity(format!("hit:{id}")),
            z_order: priority,
            selected: false,
            focused: false,
            paint: MapOverlayPaint::default(),
            geometry: MapOverlayGeometry::Label {
                position: MapCoordinate {
                    longitude: 10.75,
                    latitude: 59.91,
                },
                text: text.to_owned(),
                collision_priority: priority,
                font_size: 14.0,
            },
        });
    }
    let frame = frame_with_item(DisplayItem {
        node: DocumentNodeId("generic-map".to_owned()),
        kind: DocumentNodeKind::MapViewport,
        map_viewport: Some(Box::new(descriptor)),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 280.0,
            height: 180.0,
        },
        style: StyleMap::new(),
        text: None,
        focused: false,
        style_identity: identity(),
    });
    let mut columns = ApproximateTextColumnMeasurer;
    let scene = lower_layout_frame_to_render_scene(&frame, 320, 200, &mut columns);
    let labels = scene.map_viewports[0]
        .overlay_text_runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>();
    assert!(labels.contains(&"High"));
    assert!(!labels.contains(&"Low"));
}

#[test]
fn document_diff_applies_camera_change_as_one_retained_nonstructural_map_patch() {
    let mut frame = DocumentFrame::empty("root");
    let root_id = frame.root.clone();
    let mut map_node = DocumentNode::new("generic-map", DocumentNodeKind::MapViewport);
    map_node.parent = Some(root_id.clone());
    map_node.map_viewport = Some(Box::new(generic_map_descriptor(1)));
    map_node
        .style
        .insert("width".to_owned(), StyleValue::Number(280.0));
    map_node
        .style
        .insert("height".to_owned(), StyleValue::Number(180.0));
    frame
        .nodes
        .get_mut(&root_id)
        .unwrap()
        .children
        .push(map_node.id.clone());
    frame.nodes.insert(map_node.id.clone(), map_node);
    let mut columns = ApproximateTextColumnMeasurer;
    let mut retained = crate::RetainedDocument::new(
        frame.clone(),
        crate::Viewport {
            surface: 1,
            width: 320.0,
            height: 220.0,
            scale: 1.0,
        },
        &mut columns,
    )
    .unwrap();
    let before = retained.stats();
    let mut next = frame;
    let descriptor = next
        .nodes
        .get_mut(&DocumentNodeId("generic-map".to_owned()))
        .unwrap()
        .map_viewport
        .as_deref_mut()
        .unwrap();
    descriptor.generation = MapViewportGeneration(2);
    descriptor.camera.longitude += 0.25;
    let patches = crate::diff_document_frames(retained.frame(), &next);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], crate::DocumentPatch::UpsertNode(_)));
    let update = retained.apply_patches(patches, &mut columns).unwrap();
    let after = retained.stats();

    assert!(!update.full_lowered);
    assert!(!update.layout_changed);
    assert!(update.render_changed);
    assert_eq!(after.full_lower_count, before.full_lower_count);
    assert_eq!(after.retained_patch_count, before.retained_patch_count + 1);
    assert_eq!(
        retained.scene().map_viewports[0].descriptor.generation,
        MapViewportGeneration(2)
    );
}
