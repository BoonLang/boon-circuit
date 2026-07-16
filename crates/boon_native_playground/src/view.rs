use boon_document::render_scene::RenderTextColumnMeasurer;
use boon_document::{
    Axis, ComputedStyleIdentity, DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentPatch,
    PatchApplyError, Rect, RenderScene, RenderTextureRef, RenderVisualPrimitive,
    RenderVisualPrimitiveKind, RetainedDocument, RetainedDocumentUpdate,
};
use boon_host::Viewport;

pub(crate) const OPERATOR_CURSOR_LIGHT: [u8; 4] = [255, 255, 255, 255];
pub(crate) const OPERATOR_CURSOR_DARK: [u8; 4] = [24, 28, 36, 255];
pub(crate) const OPERATOR_CURSOR_PARTS: [[f32; 4]; 6] = [
    [0.0, 0.0, 2.0, 14.0],
    [2.0, 2.0, 2.0, 12.0],
    [4.0, 4.0, 2.0, 10.0],
    [6.0, 6.0, 2.0, 8.0],
    [8.0, 8.0, 2.0, 4.0],
    [5.0, 11.0, 3.0, 9.0],
];

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
    pub scroll_max_x: f32,
    pub scroll_max_y: f32,
    pub text_line: Option<usize>,
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
        for (layer, color) in [OPERATOR_CURSOR_LIGHT, OPERATOR_CURSOR_DARK]
            .into_iter()
            .enumerate()
        {
            for (part, [dx, dy, width, height]) in OPERATOR_CURSOR_PARTS.into_iter().enumerate() {
                let bounds = Rect {
                    x: x + dx,
                    y: y + dy,
                    width,
                    height,
                };
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
        let position = self
            .retained
            .layout()
            .display_list
            .iter()
            .find(|item| item.node.0 == target.node && item.kind == DocumentNodeKind::TextInput)
            .map(|item| boon_document::render_scene::text_position_at(item, x, y, columns));
        target.text_line = position.map(|position| position.0);
        target.text_column = position.map(|position| position.1);
        Some(target)
    }

    pub fn wheel_target(
        &self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        columns: &mut impl RenderTextColumnMeasurer,
    ) -> Option<HitTarget> {
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
                    scroll_max_x: 0.0,
                    scroll_max_y: 0.0,
                    text_line: None,
                    text_column: None,
                });
            }
            _ => {}
        }
        let measured_root = target
            .as_ref()
            .and_then(|target| target.scroll_root.clone());
        if let (Some(target), Some(root)) = (&mut target, measured_root) {
            let limits = scroll_limits(
                self.retained.frame(),
                self.retained.layout(),
                &DocumentNodeId(root),
                columns,
            );
            target.scroll_max_x = limits.x;
            target.scroll_max_y = limits.y;
        }
        target
    }

    pub fn first_visible_hit_target(&self) -> Option<HitTarget> {
        self.retained
            .hits()
            .entries
            .iter()
            .find_map(|entry| self.visible_hit_target(entry))
    }

    pub fn target_for_source(
        &self,
        source_path: &str,
        target_text: Option<&str>,
    ) -> Option<HitTarget> {
        self.target_for_scenario(source_path, None, target_text, None, None)
    }

    pub fn visible_source_action_bounds(&self) -> Vec<(String, String, Rect)> {
        self.retained
            .hits()
            .entries
            .iter()
            .filter_map(|entry| {
                self.visible_hit_target(entry).map(|target| {
                    let bounds = Rect {
                        x: target.bounds_x,
                        y: target.bounds_y,
                        width: target.bounds_width,
                        height: target.bounds_height,
                    };
                    (entry, bounds)
                })
            })
            .flat_map(|(entry, bounds)| {
                entry
                    .source_routes
                    .iter()
                    .map(move |route| (route.source_path.clone(), route.intent.clone(), bounds))
            })
            .collect()
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
            } else if let Some(expected) = target_text
                && !entry_matches_row_text(self.retained.frame(), entry, expected)
            {
                return None;
            }
            if target_row.is_none()
                && let Some(expected) = address
                && node_semantic_identity(node) != Some(expected)
            {
                return None;
            }
            let mut target = self.visible_hit_target(entry)?;
            target.source_path = Some(route.source_path.clone());
            target.source_intent = Some(route.intent.clone());
            Some(target)
        })
    }

    fn visible_hit_target(&self, entry: &boon_document::HitSideTableEntry) -> Option<HitTarget> {
        let mut visible = rect_intersection(entry.bounds, self.retained.scene().viewport)?;
        if let Some(item) = self
            .retained
            .layout()
            .display_list
            .iter()
            .find(|item| item.node == entry.node)
            && let Some(clip) = clip_rect(&item.style)
        {
            visible = rect_intersection(visible, clip)?;
        }
        let mut target = hit_target(entry);
        target.bounds_x = visible.x;
        target.bounds_y = visible.y;
        target.bounds_width = visible.width;
        target.bounds_height = visible.height;
        target.center_x = visible.x + visible.width * 0.5;
        target.center_y = visible.y + visible.height * 0.5;
        self.retained
            .hits()
            .hit_test(target.center_x, target.center_y)
            .is_some_and(|hit| hit.node == entry.node)
            .then_some(target)
    }

    pub fn revisions(&self) -> (u64, u64, u64) {
        let stats = self.retained.stats();
        (
            stats.content_revision,
            stats.layout_revision,
            stats.render_revision,
        )
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
        scroll_max_x: 0.0,
        scroll_max_y: 0.0,
        text_line: None,
        text_column: None,
    }
}

fn scroll_limits(
    document: &DocumentFrame,
    layout: &boon_document::LayoutFrame,
    root: &DocumentNodeId,
    columns: &mut impl RenderTextColumnMeasurer,
) -> boon_document::ScrollState {
    let Some(root_item) = layout.display_list.iter().find(|item| &item.node == root) else {
        return boon_document::ScrollState { x: 0.0, y: 0.0 };
    };
    let current = document
        .nodes
        .get(root)
        .and_then(|node| node.scroll)
        .unwrap_or(boon_document::ScrollState { x: 0.0, y: 0.0 });
    let mut max_right = root_item.bounds.x + root_item.bounds.width;
    let mut max_bottom = root_item.bounds.y + root_item.bounds.height;
    for item in &layout.display_list {
        if item.node != *root && document_node_is_below(document, &item.node, root) {
            max_right = max_right.max(item.bounds.x + item.bounds.width + current.x);
            max_bottom = max_bottom.max(item.bounds.y + item.bounds.height + current.y);
        }
    }
    let text = boon_document::render_scene::text_scroll_limits(root_item, columns);
    let mut limits = boon_document::ScrollState {
        x: text
            .x
            .max(max_right - (root_item.bounds.x + root_item.bounds.width)),
        y: text
            .y
            .max(max_bottom - (root_item.bounds.y + root_item.bounds.height)),
    };
    for demand in &layout.demands {
        if demand.node != *root && !document_node_is_below(document, &demand.node, root) {
            continue;
        }
        let Some(item_extent) = demand
            .item_extent_milli
            .map(|extent| extent as f32 / 1_000.0)
        else {
            continue;
        };
        let viewport_extent = demand.viewport_extent_milli as f32 / 1_000.0;
        let logical_extent = item_extent * demand.logical_item_count as f32;
        let maximum = (logical_extent - viewport_extent).max(0.0);
        match demand.axis {
            Axis::Horizontal => limits.x = limits.x.max(maximum),
            Axis::Vertical => limits.y = limits.y.max(maximum),
        }
    }
    limits
}

fn document_node_is_below(
    document: &DocumentFrame,
    node: &DocumentNodeId,
    ancestor: &DocumentNodeId,
) -> bool {
    let mut current = document
        .nodes
        .get(node)
        .and_then(|node| node.parent.as_ref());
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = document.nodes.get(id).and_then(|node| node.parent.as_ref());
    }
    false
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

fn rect_intersection(left: Rect, right: Rect) -> Option<Rect> {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let width = (left.x + left.width).min(right.x + right.width) - x;
    let height = (left.y + left.height).min(right.y + right.height) - y;
    (width > 0.0 && height > 0.0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn clip_rect(style: &boon_document::StyleMap) -> Option<Rect> {
    let number = |name| match style.get(name) {
        Some(boon_document::StyleValue::Number(value)) if value.is_finite() => Some(*value as f32),
        _ => None,
    };
    Some(Rect {
        x: number("__clip_x")?,
        y: number("__clip_y")?,
        width: number("__clip_width")?,
        height: number("__clip_height")?,
    })
}

fn rect_area(rect: Rect) -> f32 {
    rect.width.max(0.0) * rect.height.max(0.0)
}
