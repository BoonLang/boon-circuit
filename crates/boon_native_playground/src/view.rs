use boon_document::render_scene::RenderTextColumnMeasurer;
use boon_document::{
    Axis, ComputedStyleIdentity, DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentPatch,
    PatchApplyError, Rect, RenderScene, RenderTextureRef, RenderVisualPrimitive,
    RenderVisualPrimitiveKind, RetainedDocument, RetainedDocumentUpdate,
};
use boon_host::Viewport;

#[derive(Clone, Debug, PartialEq)]
pub struct HitTarget {
    pub node: String,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
    pub row_key: Option<u64>,
    pub row_generation: Option<u64>,
    pub scroll_root: Option<String>,
    pub center_x: f32,
    pub center_y: f32,
    pub bounds_x: f32,
    pub bounds_y: f32,
    pub bounds_width: f32,
    pub bounds_height: f32,
    pub text_column: Option<usize>,
}

pub struct RetainedView {
    retained: RetainedDocument,
}

impl RetainedView {
    pub fn new(
        document: DocumentFrame,
        viewport: Viewport,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<Self, PatchApplyError> {
        Ok(Self {
            retained: RetainedDocument::new(document, viewport, columns)?,
        })
    }

    pub fn replace(
        &mut self,
        document: DocumentFrame,
        viewport: Viewport,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<(), PatchApplyError> {
        self.retained.replace(document, viewport, columns)?;
        Ok(())
    }

    pub fn resize(
        &mut self,
        viewport: Viewport,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<(), PatchApplyError> {
        self.retained.resize(viewport, columns)?;
        Ok(())
    }

    pub fn apply_patches(
        &mut self,
        patches: Vec<DocumentPatch>,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<RetainedDocumentUpdate, PatchApplyError> {
        self.retained.apply_patches(patches, columns)
    }

    pub fn set_interaction_state(
        &mut self,
        hovered: Option<&str>,
        focused: Option<&str>,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Result<RetainedDocumentUpdate, PatchApplyError> {
        self.retained.set_interaction_state(
            hovered.map(|id| DocumentNodeId(id.to_owned())),
            focused.map(|id| DocumentNodeId(id.to_owned())),
            columns,
        )
    }

    pub fn scene(&self) -> &RenderScene {
        self.retained.scene()
    }

    pub fn frame(&self) -> &DocumentFrame {
        self.retained.frame()
    }

    pub fn node_bounds(&self, id: &str) -> Option<Rect> {
        self.retained
            .layout()
            .display_list
            .iter()
            .find(|item| item.node.0 == id)
            .map(|item| item.bounds)
    }

    pub fn demands(&self) -> &[boon_document::LayoutDemand] {
        self.retained.demands()
    }

    pub fn scene_with_cursor(&self, x: f32, y: f32) -> RenderScene {
        let mut scene = self.retained.scene().clone();
        let identity = ComputedStyleIdentity {
            style_id: 0x4355_5253_4f52,
            layout_id: 0x4355_5253_4f52,
            paint_id: 0x4355_5253_4f52,
            material_id: 0,
            font_id: 0,
            pseudo_state_id: 0,
        };
        let pointer_parts = [
            Rect {
                x,
                y,
                width: 2.0,
                height: 14.0,
            },
            Rect {
                x: x + 2.0,
                y: y + 2.0,
                width: 2.0,
                height: 12.0,
            },
            Rect {
                x: x + 4.0,
                y: y + 4.0,
                width: 2.0,
                height: 10.0,
            },
            Rect {
                x: x + 6.0,
                y: y + 6.0,
                width: 2.0,
                height: 8.0,
            },
            Rect {
                x: x + 8.0,
                y: y + 8.0,
                width: 2.0,
                height: 4.0,
            },
            Rect {
                x: x + 5.0,
                y: y + 11.0,
                width: 3.0,
                height: 9.0,
            },
        ];
        for (layer, color) in [[255, 255, 255, 255], [24, 28, 36, 255]]
            .into_iter()
            .enumerate()
        {
            for (part, bounds) in pointer_parts.into_iter().enumerate() {
                let bounds = if layer == 0 {
                    Rect {
                        x: bounds.x - 1.0,
                        y: bounds.y - 1.0,
                        width: bounds.width + 2.0,
                        height: bounds.height + 2.0,
                    }
                } else {
                    bounds
                };
                scene.visual_primitives.push(RenderVisualPrimitive {
                    node: DocumentNodeId("operator.cursor".to_owned()),
                    retained_chunk_id: format!("operator-cursor-{layer}-{part}"),
                    source_kind: DocumentNodeKind::Root,
                    primitive: RenderVisualPrimitiveKind::Fill,
                    bounds,
                    clip: Some(scene.viewport),
                    radius: 0.5,
                    stroke_width: 0.0,
                    color,
                    secondary_color: [255, 255, 255, 255],
                    antialias: 1.0,
                    control_points: Vec::new(),
                    texture: RenderTextureRef::Solid,
                    style_identity: identity,
                    dependency_set: vec!["operator-cursor".to_owned()],
                });
            }
        }
        scene.metrics.visual_primitive_count =
            scene.visual_primitives.len().try_into().unwrap_or(u32::MAX);
        scene
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<&str> {
        self.retained
            .hits()
            .hit_test(x, y)
            .map(|entry| entry.node.0.as_str())
    }

    pub fn hit_target(&self, x: f32, y: f32) -> Option<HitTarget> {
        self.retained.hits().hit_test(x, y).map(hit_target)
    }

    pub fn hit_target_with_text_column(
        &self,
        x: f32,
        y: f32,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Option<HitTarget> {
        let mut target = self.hit_target(x, y)?;
        target.text_column = self
            .retained
            .layout()
            .display_list
            .iter()
            .find(|item| item.node.0 == target.node && item.kind == DocumentNodeKind::TextInput)
            .map(|item| boon_document::render_scene::text_column_at(item, x, columns));
        Some(target)
    }

    pub fn wheel_target(&self, x: f32, y: f32, delta_x: f32, delta_y: f32) -> Option<HitTarget> {
        let mut target = self.hit_target(x, y);
        let axis = if delta_y != 0.0 && delta_y.abs() >= delta_x.abs() {
            Axis::Vertical
        } else {
            Axis::Horizontal
        };
        let scroll_root = scroll_root_at(self.retained.frame(), self.retained.layout(), axis, x, y);
        match (&mut target, scroll_root) {
            (Some(target), Some(root)) => target.scroll_root = Some(root),
            (None, Some(root)) => {
                target = Some(HitTarget {
                    node: root.clone(),
                    source_path: None,
                    source_intent: None,
                    row_key: None,
                    row_generation: None,
                    scroll_root: Some(root),
                    center_x: x,
                    center_y: y,
                    bounds_x: x,
                    bounds_y: y,
                    bounds_width: 0.0,
                    bounds_height: 0.0,
                    text_column: None,
                });
            }
            _ => {}
        }
        target
    }

    pub fn first_visible_hit_target(&self) -> Option<HitTarget> {
        let hits = self.retained.hits();
        hits.entries.iter().find_map(|entry| {
            let target = hit_target(entry);
            (target.center_x.is_finite()
                && target.center_y.is_finite()
                && hits
                    .hit_test(target.center_x, target.center_y)
                    .is_some_and(|hit| hit.node == entry.node))
            .then_some(target)
        })
    }

    pub fn target_for_source(
        &self,
        source_path: &str,
        target_text: Option<&str>,
    ) -> Option<HitTarget> {
        self.target_for_scenario(source_path, None, target_text, None, None)
    }

    pub fn target_for_scenario(
        &self,
        source_path: &str,
        action_kind: Option<&str>,
        target_text: Option<&str>,
        address: Option<&str>,
        target_row: Option<(u64, u64)>,
    ) -> Option<HitTarget> {
        self.retained.hits().entries.iter().find_map(|entry| {
            let preferred_intent = action_source_intent(action_kind);
            let route = preferred_intent
                .and_then(|intent| {
                    entry
                        .source_routes
                        .iter()
                        .find(|route| route.source_path == source_path && route.intent == intent)
                })
                .or_else(|| {
                    entry
                        .source_routes
                        .iter()
                        .find(|route| route.source_path == source_path)
                })?;
            let node = self.retained.frame().nodes.get(&entry.node);
            if let Some((key, generation)) = target_row {
                if entry.row_key.is_some() {
                    if entry.row_key != Some(key) || entry.row_generation.unwrap_or(1) != generation
                    {
                        return None;
                    }
                } else {
                    if let Some(expected) = target_text
                        && !entry_matches_row_text(self.retained.frame(), entry, expected)
                    {
                        return None;
                    }
                    if let Some(expected) = address
                        && node_semantic_identity(node) != Some(expected)
                    {
                        return None;
                    }
                }
            } else if let Some(expected) = target_text {
                if !entry_matches_row_text(self.retained.frame(), entry, expected) {
                    return None;
                }
            }
            if target_row.is_none()
                && let Some(expected) = address
            {
                if node_semantic_identity(node) != Some(expected) {
                    return None;
                }
            }
            let mut target = hit_target(entry);
            target.source_path = Some(route.source_path.clone());
            target.source_intent = Some(route.intent.clone());
            Some(target)
        })
    }

    pub fn revisions(&self) -> (u64, u64, u64) {
        let stats = self.retained.stats();
        (
            stats.content_revision,
            stats.layout_revision,
            stats.render_revision,
        )
    }

    #[cfg(test)]
    pub fn retained_stats(&self) -> boon_document::RetainedDocumentStats {
        self.retained.stats()
    }
}

fn node_semantic_identity(node: Option<&boon_document::DocumentNode>) -> Option<&str> {
    node.and_then(|node| {
        ["address", "key", "target"].iter().find_map(|key| {
            node.style.get(*key).and_then(|value| match value {
                boon_document::StyleValue::Text(value) => Some(value.as_str()),
                _ => None,
            })
        })
    })
}

fn action_source_intent(action: Option<&str>) -> Option<&str> {
    match action? {
        "blur" => Some("blur"),
        "click" => Some("click"),
        "double_click" => Some("double_click"),
        "focus" => Some("focus"),
        "key_down" => Some("key_down"),
        "type_text" => Some("change"),
        _ => None,
    }
}

fn entry_matches_row_text(
    frame: &DocumentFrame,
    entry: &boon_document::HitSideTableEntry,
    expected: &str,
) -> bool {
    let Some(node) = frame.nodes.get(&entry.node) else {
        return false;
    };
    if subtree_matches_semantic_text(frame, node, expected) {
        return true;
    }
    let (Some(row_key), Some(row_list)) = (entry.row_key, style_identity(node, "row_list")) else {
        return false;
    };
    let row_generation = entry.row_generation.unwrap_or(1);
    frame.nodes.values().any(|candidate| {
        style_identity(candidate, "row_key") == Some(row_key)
            && style_identity(candidate, "row_list") == Some(row_list)
            && style_identity(candidate, "row_generation").unwrap_or(1) == row_generation
            && node_matches_semantic_text(candidate, expected)
    })
}

fn subtree_matches_semantic_text(
    frame: &DocumentFrame,
    root: &boon_document::DocumentNode,
    expected: &str,
) -> bool {
    let mut pending = vec![root.id.clone()];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(node) = frame.nodes.get(&id) else {
            continue;
        };
        if node_matches_semantic_text(node, expected) {
            return true;
        }
        pending.extend(node.children.iter().cloned());
    }
    false
}

fn node_matches_semantic_text(node: &boon_document::DocumentNode, expected: &str) -> bool {
    node.text.as_ref().is_some_and(|text| text.text == expected)
        || ["target", "label"].iter().any(|name| {
            matches!(
                node.style.get(*name),
                Some(boon_document::StyleValue::Text(value)) if value == expected
            )
        })
}

fn style_identity(node: &boon_document::DocumentNode, name: &str) -> Option<u64> {
    match node.style.get(name) {
        Some(boon_document::StyleValue::Number(value)) if value.is_finite() && *value >= 0.0 => {
            Some(*value as u64)
        }
        Some(boon_document::StyleValue::Text(value)) => value.parse().ok(),
        _ => None,
    }
}

fn hit_target(entry: &boon_document::HitSideTableEntry) -> HitTarget {
    HitTarget {
        node: entry.node.0.clone(),
        source_path: entry.source_path.clone(),
        source_intent: entry.source_intent.clone(),
        row_key: entry.row_key,
        row_generation: entry.row_generation,
        scroll_root: entry.scroll_root.as_ref().map(|root| root.0.clone()),
        center_x: entry.bounds.x + entry.bounds.width * 0.5,
        center_y: entry.bounds.y + entry.bounds.height * 0.5,
        bounds_x: entry.bounds.x,
        bounds_y: entry.bounds.y,
        bounds_width: entry.bounds.width,
        bounds_height: entry.bounds.height,
        text_column: None,
    }
}

fn scroll_root_at(
    document: &DocumentFrame,
    layout: &boon_document::LayoutFrame,
    axis: Axis,
    x: f32,
    y: f32,
) -> Option<String> {
    layout
        .display_list
        .iter()
        .filter(|item| {
            rect_contains(item.bounds, x, y)
                && document
                    .nodes
                    .get(&item.node)
                    .is_some_and(|node| node_scrolls_axis(document, node, axis))
        })
        .min_by(|left, right| rect_area(left.bounds).total_cmp(&rect_area(right.bounds)))
        .map(|item| item.node.0.clone())
        .or_else(|| nearest_scroll_region(&layout.scroll_regions, axis, x, y))
}

fn nearest_scroll_region(
    regions: &[boon_document::ScrollRegion],
    axis: Axis,
    x: f32,
    y: f32,
) -> Option<String> {
    regions
        .iter()
        .filter(|region| region.axis == axis && rect_contains(region.bounds, x, y))
        .min_by(|left, right| rect_area(left.bounds).total_cmp(&rect_area(right.bounds)))
        .map(|region| region.node.0.clone())
}

fn node_scrolls_axis(
    document: &DocumentFrame,
    node: &boon_document::DocumentNode,
    axis: Axis,
) -> bool {
    document
        .scroll_roots
        .contains_key(&boon_document::ScrollRootId(node.id.0.clone()))
        || node.kind == DocumentNodeKind::ScrollRoot
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scrollbars")
        || match axis {
            Axis::Horizontal => style_bool(&node.style, "scroll_x"),
            Axis::Vertical => style_bool(&node.style, "scroll_y"),
        }
}

fn style_bool(style: &boon_document::StyleMap, key: &str) -> bool {
    match style.get(key) {
        Some(boon_document::StyleValue::Bool(value)) => *value,
        Some(boon_document::StyleValue::Text(value)) => value.parse().unwrap_or(false),
        Some(boon_document::StyleValue::Number(value)) => *value != 0.0,
        _ => false,
    }
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn rect_area(rect: Rect) -> f32 {
    rect.width.max(0.0) * rect.height.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_target_uses_the_smallest_scroll_region_on_the_requested_axis() {
        let regions = vec![
            boon_document::ScrollRegion {
                id: "outer".to_owned(),
                node: DocumentNodeId("outer".to_owned()),
                axis: Axis::Vertical,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 200.0,
                },
            },
            boon_document::ScrollRegion {
                id: "inner".to_owned(),
                node: DocumentNodeId("inner".to_owned()),
                axis: Axis::Vertical,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 80.0,
                    height: 80.0,
                },
            },
            boon_document::ScrollRegion {
                id: "horizontal".to_owned(),
                node: DocumentNodeId("horizontal".to_owned()),
                axis: Axis::Horizontal,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 80.0,
                    height: 80.0,
                },
            },
        ];

        assert_eq!(
            nearest_scroll_region(&regions, Axis::Vertical, 40.0, 40.0).as_deref(),
            Some("inner")
        );
        assert_eq!(
            nearest_scroll_region(&regions, Axis::Horizontal, 40.0, 40.0).as_deref(),
            Some("horizontal")
        );
        assert_eq!(
            nearest_scroll_region(&regions, Axis::Vertical, 240.0, 40.0),
            None
        );
    }

    #[test]
    fn retained_view_keeps_distinct_content_layout_and_render_revisions() {
        let document = DocumentFrame::empty("root");
        let viewport = Viewport {
            surface: 1,
            width: 320.0,
            height: 240.0,
            scale: 1.0,
        };
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let mut view = RetainedView::new(document.clone(), viewport, &mut columns).unwrap();
        assert_eq!(view.revisions(), (1, 1, 1));
        view.resize(
            Viewport {
                width: 640.0,
                ..viewport
            },
            &mut columns,
        )
        .unwrap();
        assert_eq!(view.revisions(), (1, 2, 2));
        view.replace(document, viewport, &mut columns).unwrap();
        assert_eq!(view.revisions(), (2, 3, 3));
    }

    #[test]
    fn scenario_cursor_is_a_high_contrast_pointer_with_a_stable_hotspot() {
        let document = DocumentFrame::empty("root");
        let viewport = Viewport {
            surface: 1,
            width: 320.0,
            height: 240.0,
            scale: 1.0,
        };
        let mut columns = boon_document::render_scene::ApproximateTextColumnMeasurer;
        let view = RetainedView::new(document, viewport, &mut columns).unwrap();
        let scene = view.scene_with_cursor(40.0, 50.0);
        let cursor = scene
            .visual_primitives
            .iter()
            .filter(|primitive| primitive.node.0 == "operator.cursor")
            .collect::<Vec<_>>();
        assert_eq!(cursor.len(), 12);
        assert!(cursor.iter().all(|primitive| {
            primitive.dependency_set == ["operator-cursor"]
                && primitive.clip == Some(scene.viewport)
        }));
        assert!(
            cursor
                .iter()
                .any(|primitive| primitive.color == [255, 255, 255, 255])
        );
        assert!(
            cursor
                .iter()
                .any(|primitive| primitive.color == [24, 28, 36, 255])
        );
        assert!(cursor.iter().any(|primitive| {
            primitive.color == [24, 28, 36, 255]
                && primitive.bounds.x == 40.0
                && primitive.bounds.y == 50.0
        }));
    }
}
