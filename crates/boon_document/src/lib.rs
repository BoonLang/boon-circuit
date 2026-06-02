pub use boon_document_model::{
    Axis, DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, DocumentPatch,
    MaterializedRange, StyleMap, StylePatch, StyleValue, TextValue,
};
use boon_host::Viewport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> TextMetrics;

    fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        style: &BTreeMap<String, StyleValue>,
    ) -> TextMetrics {
        let _ = style;
        self.measure(text, font_size)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderCapabilities {
    pub max_texture_dimension_2d: u32,
    pub supports_instancing: bool,
    pub supports_clip_rects: bool,
    pub text_backend_class: String,
}

impl RenderCapabilities {
    pub fn fake_portable() -> Self {
        Self {
            max_texture_dimension_2d: 4096,
            supports_instancing: true,
            supports_clip_rects: true,
            text_backend_class: "fake-portable".to_owned(),
        }
    }
}

pub struct LayoutInput<'a> {
    pub document: &'a DocumentFrame,
    pub viewport: Viewport,
    pub text: &'a mut dyn TextMeasurer,
    pub capabilities: RenderCapabilities,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutFrame {
    pub display_list: Vec<DisplayItem>,
    pub hit_regions: Vec<HitRegion>,
    pub scroll_regions: Vec<ScrollRegion>,
    pub accessibility: AccessibilityTree,
    pub demands: Vec<LayoutDemand>,
    pub metrics: LayoutMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayItem {
    pub node: DocumentNodeId,
    pub kind: DocumentNodeKind,
    pub bounds: Rect,
    pub text: Option<String>,
    pub style: BTreeMap<String, StyleValue>,
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitRegion {
    pub id: String,
    pub node: DocumentNodeId,
    pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollRegion {
    pub id: String,
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTree {
    pub node_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutDemand {
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayoutMetrics {
    pub node_count: usize,
    pub display_item_count: usize,
    pub materialized_range_count: usize,
    pub native_capability_required: bool,
}

#[derive(Clone, Debug)]
pub struct DocumentState {
    frame: DocumentFrame,
}

impl DocumentState {
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            frame: DocumentFrame::empty(root),
        }
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn apply_patch(&mut self, patch: DocumentPatch) {
        match patch {
            DocumentPatch::UpsertNode(node) => {
                self.frame.nodes.insert(node.id.clone(), node);
            }
            DocumentPatch::RemoveNode { id } => {
                self.frame.nodes.remove(&id);
            }
            DocumentPatch::SetText { id, text } => {
                if let Some(node) = self.frame.nodes.get_mut(&id) {
                    node.text = Some(text);
                }
            }
            DocumentPatch::SetStyle { id, patch } => {
                if let Some(node) = self.frame.nodes.get_mut(&id) {
                    apply_style_patch(&mut node.style, patch);
                }
            }
            DocumentPatch::SetBinding { id, binding } => {
                if let Some(node) = self.frame.nodes.get_mut(&id) {
                    node.source_binding = Some(binding);
                }
            }
            DocumentPatch::SetScroll { id, scroll } => {
                if let Some(node) = self.frame.nodes.get_mut(&id) {
                    node.scroll = Some(scroll);
                }
            }
            DocumentPatch::SetListMaterialization { id, materialized } => {
                if let Some(node) = self.frame.nodes.get_mut(&id) {
                    node.materialized.push(materialized);
                }
            }
        }
    }
}

pub fn layout(input: LayoutInput<'_>) -> LayoutFrame {
    let mut builder = LayoutBuilder {
        document: input.document,
        text: input.text,
        display_list: Vec::new(),
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        demands: Vec::new(),
        materialized_range_count: 0,
    };
    if let Some(root) = input.document.nodes.get(&input.document.root).cloned() {
        let mut cursor_y = 0.0;
        for child in root.children {
            let rect = builder.layout_node(
                &child,
                0.0,
                cursor_y,
                input.viewport.width,
                input.viewport.height,
            );
            cursor_y += rect.height;
        }
    }

    LayoutFrame {
        accessibility: AccessibilityTree {
            node_count: input.document.nodes.len(),
        },
        metrics: LayoutMetrics {
            node_count: input.document.nodes.len(),
            display_item_count: builder.display_list.len(),
            materialized_range_count: builder.materialized_range_count,
            native_capability_required: false,
        },
        display_list: builder.display_list,
        hit_regions: builder.hit_regions,
        scroll_regions: builder.scroll_regions,
        demands: builder.demands,
    }
}

struct LayoutBuilder<'a, 'b> {
    document: &'a DocumentFrame,
    text: &'b mut dyn TextMeasurer,
    display_list: Vec<DisplayItem>,
    hit_regions: Vec<HitRegion>,
    scroll_regions: Vec<ScrollRegion>,
    demands: Vec<LayoutDemand>,
    materialized_range_count: usize,
}

impl LayoutBuilder<'_, '_> {
    fn layout_node(
        &mut self,
        id: &DocumentNodeId,
        x: f32,
        y: f32,
        available_width: f32,
        available_height: f32,
    ) -> Rect {
        let Some(node) = self.document.nodes.get(id).cloned() else {
            return Rect {
                x,
                y,
                width: 0.0,
                height: 0.0,
            };
        };
        let padding = style_edges(&node.style, "padding");
        let gap = style_spacing(&node.style, "gap").unwrap_or(0.0);
        let box_size = match node.kind {
            DocumentNodeKind::Checkbox => style_spacing(&node.style, "box_size")
                .or_else(|| style_spacing(&node.style, "size")),
            DocumentNodeKind::Button | DocumentNodeKind::Stack | DocumentNodeKind::TableCell
                if node.text.is_none() =>
            {
                style_spacing(&node.style, "box_size")
            }
            _ => None,
        };
        let auto_width = style_text(&node.style, "width")
            .is_some_and(|value| value.eq_ignore_ascii_case("auto"));
        let explicit_width = style_dimension(&node.style, "width", available_width).or(box_size);
        let explicit_height = style_dimension(&node.style, "height", available_height).or(box_size);
        let text = node.text.as_ref().map(|value| value.text.clone());
        let measurement_text = text
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                matches!(node.kind, DocumentNodeKind::TextInput)
                    .then(|| style_text(&node.style, "placeholder"))
                    .flatten()
            });
        let mut measured = measurement_text
            .filter(|value| !value.is_empty())
            .map(|value| {
                self.text.measure_styled(
                    value,
                    style_spacing(&node.style, "size").unwrap_or(14.0),
                    &node.style,
                )
            })
            .unwrap_or(TextMetrics {
                width: 0.0,
                height: 0.0,
            });
        if matches!(node.kind, DocumentNodeKind::Text)
            && (node.style.contains_key("relief") || node.style.contains_key("depth"))
            && measured.width > 0.0
        {
            measured.width += 8.0;
        }
        let shrink_to_child_width = explicit_width.is_none()
            && text.is_none()
            && !node.children.is_empty()
            && matches!(
                node.kind,
                DocumentNodeKind::Button | DocumentNodeKind::Checkbox
            );
        let mut width = if auto_width {
            let auto_padding = style_spacing(&node.style, "auto_padding")
                .unwrap_or_else(|| style_spacing(&node.style, "size").unwrap_or(14.0) * 0.9);
            (measured.width + auto_padding + padding.horizontal()).max(1.0)
        } else if shrink_to_child_width {
            padding.horizontal().max(1.0)
        } else {
            explicit_width
                .unwrap_or_else(|| measured.width.max(available_width))
                .max(1.0)
        };
        width = constrain_dimension(width, &node.style, "width", available_width);
        let mut height =
            explicit_height.unwrap_or_else(|| measured.height.max(24.0) + padding.vertical());
        height = constrain_dimension(height, &node.style, "height", available_height);
        let centered = style_bool(&node.style, "center").unwrap_or(false);
        let align_x = style_text(&node.style, "align_x").unwrap_or_default();
        let mut node_x = if centered && width < available_width {
            x + (available_width - width) / 2.0
        } else if align_x.eq_ignore_ascii_case("right") && width < available_width {
            x + available_width - width
        } else {
            x
        };
        let display_index = self.display_list.len();
        self.display_list.push(DisplayItem {
            node: node.id.clone(),
            kind: node.kind.clone(),
            bounds: Rect {
                x: node_x,
                y,
                width,
                height,
            },
            text,
            style: node.style.clone(),
            focused: self.document.focus.as_ref() == Some(&node.id),
        });
        let subtree_hit_start = self.hit_regions.len();
        let subtree_scroll_start = self.scroll_regions.len();

        if !node.children.is_empty() {
            let content_x = node_x + padding.left;
            let content_y = y + padding.top;
            let content_width = (width - padding.horizontal()).max(1.0);
            match node.kind {
                DocumentNodeKind::Row => {
                    let display_start = self.display_list.len();
                    let hit_start = self.hit_regions.len();
                    let scroll_start = self.scroll_regions.len();
                    let child_count = node.children.len();
                    let fill_child_count =
                        node.children
                            .iter()
                            .filter(|child| {
                                self.document.nodes.get(child).is_some_and(|child| {
                                    style_dimension_is_fill(&child.style, "width")
                                })
                            })
                            .count();
                    let row_gap_total = if child_count > 0 {
                        gap * child_count.saturating_sub(1) as f32
                    } else {
                        0.0
                    };
                    let fixed_child_width: f32 = if fill_child_count > 0 {
                        node.children
                            .iter()
                            .filter_map(|child| self.document.nodes.get(child))
                            .filter(|child| !style_dimension_is_fill(&child.style, "width"))
                            .filter_map(|child| preferred_row_child_width(child, self.text))
                            .sum()
                    } else {
                        0.0
                    };
                    let fill_child_width = if fill_child_count > 0 {
                        ((content_width - row_gap_total - fixed_child_width)
                            / fill_child_count as f32)
                            .max(1.0)
                    } else {
                        0.0
                    };
                    let mut cursor_x = content_x;
                    let mut max_child_height: f32 = 0.0;
                    for child in &node.children {
                        let child_available_width = self
                            .document
                            .nodes
                            .get(child)
                            .and_then(|child_node| {
                                style_dimension_is_fill(&child_node.style, "width")
                                    .then_some(fill_child_width)
                                    .or_else(|| {
                                        (fill_child_count > 0)
                                            .then(|| {
                                                preferred_row_child_width(child_node, self.text)
                                            })
                                            .flatten()
                                    })
                            })
                            .unwrap_or_else(|| (content_x + content_width - cursor_x).max(1.0))
                            .max(1.0);
                        let child_rect = self.layout_node(
                            child,
                            cursor_x,
                            content_y,
                            child_available_width,
                            (height - padding.vertical()).max(1.0),
                        );
                        cursor_x += child_rect.width + gap;
                        max_child_height = max_child_height.max(child_rect.height);
                    }
                    if style_bool(&node.style, "center").unwrap_or(false) {
                        let total_child_width = (cursor_x - content_x - gap).max(0.0);
                        let offset_x = ((content_width - total_child_width) / 2.0).max(0.0);
                        if offset_x > f32::EPSILON {
                            for item in &mut self.display_list[display_start..] {
                                item.bounds.x += offset_x;
                            }
                            for hit in &mut self.hit_regions[hit_start..] {
                                hit.bounds.x += offset_x;
                            }
                            for scroll in &mut self.scroll_regions[scroll_start..] {
                                scroll.bounds.x += offset_x;
                            }
                        }
                    }
                    if explicit_height.is_none() {
                        height = (max_child_height + padding.vertical()).max(24.0);
                    }
                }
                _ => {
                    let mut cursor_y = content_y;
                    let mut max_child_width: f32 = 0.0;
                    for child in &node.children {
                        let child_rect = self.layout_node(
                            child,
                            content_x,
                            cursor_y,
                            content_width,
                            (content_y + height - cursor_y).max(1.0),
                        );
                        cursor_y += child_rect.height + gap;
                        max_child_width = max_child_width.max(child_rect.width);
                    }
                    if explicit_width.is_none() {
                        width = if shrink_to_child_width {
                            let padded_button_safety =
                                if matches!(node.kind, DocumentNodeKind::Button)
                                    && padding.horizontal() > 0.0
                                {
                                    16.0
                                } else {
                                    0.0
                                };
                            (max_child_width + padding.horizontal() + padded_button_safety).max(1.0)
                        } else {
                            constrain_dimension(
                                max_child_width.max(width).max(1.0) + padding.horizontal(),
                                &node.style,
                                "width",
                                available_width,
                            )
                        };
                    }
                    if explicit_height.is_none() {
                        height = constrain_dimension(
                            (cursor_y - y - gap).max(24.0) + padding.bottom,
                            &node.style,
                            "height",
                            available_height,
                        );
                    }
                }
            }
        }

        let final_node_x = if centered && width < available_width {
            x + (available_width - width) / 2.0
        } else if align_x.eq_ignore_ascii_case("right") && width < available_width {
            x + available_width - width
        } else {
            x
        };
        let node_delta_x = final_node_x - node_x;
        if node_delta_x.abs() > f32::EPSILON {
            node_x = final_node_x;
            for item in &mut self.display_list[display_index..] {
                item.bounds.x += node_delta_x;
            }
            for hit in &mut self.hit_regions[subtree_hit_start..] {
                hit.bounds.x += node_delta_x;
            }
            for scroll in &mut self.scroll_regions[subtree_scroll_start..] {
                scroll.bounds.x += node_delta_x;
            }
        }

        let rect = Rect {
            x: node_x,
            y,
            width,
            height,
        };
        self.display_list[display_index].bounds = rect;
        if node.source_binding.is_some() || style_bool(&node.style, "__hover_scope") == Some(true) {
            self.hit_regions.push(HitRegion {
                id: format!("hit:{}", node.id.0),
                node: node.id.clone(),
                bounds: rect,
            });
        }
        for range in &node.materialized {
            self.materialized_range_count += 1;
            self.scroll_regions.push(ScrollRegion {
                id: format!("scroll:{}", node.id.0),
                node: node.id.clone(),
                axis: range.axis,
                bounds: rect,
            });
            self.demands.push(demand_from_materialized(&node, range));
        }
        rect
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeSpacing {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl EdgeSpacing {
    fn horizontal(self) -> f32 {
        self.left + self.right
    }

    fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

fn style_edges(style: &BTreeMap<String, StyleValue>, prefix: &str) -> EdgeSpacing {
    let all = style_spacing(style, prefix).unwrap_or(0.0);
    EdgeSpacing {
        top: style_spacing(style, &format!("{prefix}_top")).unwrap_or(all),
        right: style_spacing(style, &format!("{prefix}_right")).unwrap_or(all),
        bottom: style_spacing(style, &format!("{prefix}_bottom")).unwrap_or(all),
        left: style_spacing(style, &format!("{prefix}_left")).unwrap_or(all),
    }
}

fn style_spacing(style: &BTreeMap<String, StyleValue>, key: &str) -> Option<f32> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value
            .split(',')
            .next()
            .and_then(|value| value.trim().parse::<f32>().ok()),
        StyleValue::Bool(_) => None,
    }
}

fn style_bool(style: &BTreeMap<String, StyleValue>, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) => None,
    }
}

fn style_text<'a>(style: &'a BTreeMap<String, StyleValue>, key: &str) -> Option<&'a str> {
    match style.get(key)? {
        StyleValue::Text(value) => Some(value.as_str()),
        StyleValue::Bool(_) | StyleValue::Number(_) => None,
    }
}

fn style_dimension(
    style: &BTreeMap<String, StyleValue>,
    key: &str,
    fill_width: f32,
) -> Option<f32> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) if value == "Fill" || value == "fill" => Some(fill_width),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) => None,
    }
}

fn style_dimension_is_fill(style: &BTreeMap<String, StyleValue>, key: &str) -> bool {
    matches!(
        style.get(key),
        Some(StyleValue::Text(value)) if value.eq_ignore_ascii_case("fill")
    )
}

fn preferred_row_child_width(node: &DocumentNode, text: &mut dyn TextMeasurer) -> Option<f32> {
    let padding = style_edges(&node.style, "padding");
    let box_size = match node.kind {
        DocumentNodeKind::Checkbox => {
            style_spacing(&node.style, "box_size").or_else(|| style_spacing(&node.style, "size"))
        }
        DocumentNodeKind::Button | DocumentNodeKind::Stack | DocumentNodeKind::TableCell
            if node.text.is_none() =>
        {
            style_spacing(&node.style, "box_size")
        }
        _ => None,
    };
    if style_text(&node.style, "width").is_some_and(|value| value.eq_ignore_ascii_case("auto")) {
        let auto_padding = style_spacing(&node.style, "auto_padding")
            .unwrap_or_else(|| style_spacing(&node.style, "size").unwrap_or(14.0) * 0.9);
        let measured_width = row_child_measurement_text(node)
            .map(|value| {
                text.measure_styled(
                    value,
                    style_spacing(&node.style, "size").unwrap_or(14.0),
                    &node.style,
                )
                .width
            })
            .unwrap_or(0.0);
        return Some((measured_width + auto_padding + padding.horizontal()).max(1.0));
    }
    style_dimension(&node.style, "width", 0.0)
        .or(box_size)
        .or_else(|| {
            row_child_measurement_text(node).map(|value| {
                let mut measured_width = text
                    .measure_styled(
                        value,
                        style_spacing(&node.style, "size").unwrap_or(14.0),
                        &node.style,
                    )
                    .width;
                if matches!(node.kind, DocumentNodeKind::Text)
                    && (node.style.contains_key("relief") || node.style.contains_key("depth"))
                    && measured_width > 0.0
                {
                    measured_width += 8.0;
                }
                (measured_width + padding.horizontal()).max(1.0)
            })
        })
}

fn row_child_measurement_text(node: &DocumentNode) -> Option<&str> {
    node.text
        .as_ref()
        .map(|value| value.text.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            matches!(node.kind, DocumentNodeKind::TextInput)
                .then(|| style_text(&node.style, "placeholder"))
                .flatten()
                .filter(|value| !value.is_empty())
        })
}

fn constrain_dimension(
    value: f32,
    style: &BTreeMap<String, StyleValue>,
    key: &str,
    fill_extent: f32,
) -> f32 {
    let min = style_dimension(style, &format!("min_{key}"), fill_extent);
    let max = style_dimension(style, &format!("max_{key}"), fill_extent);
    let mut constrained = value;
    if let Some(min) = min {
        constrained = constrained.max(min);
    }
    if let Some(max) = max {
        constrained = constrained.min(max);
    }
    constrained.max(1.0)
}

#[derive(Default)]
pub struct SimpleTextMeasurer;

impl TextMeasurer for SimpleTextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> TextMetrics {
        TextMetrics {
            width: text.chars().count() as f32 * font_size,
            height: font_size * 1.4,
        }
    }
}

pub fn fixture_frame_with_virtualized_table() -> DocumentFrame {
    let mut frame = DocumentFrame::empty("root");
    let mut table = DocumentNode::new(
        "virtual-table",
        boon_document_model::DocumentNodeKind::Table,
    );
    table.parent = Some(frame.root.clone());
    table.text = Some(TextValue {
        text: "Virtualized logical table".to_owned(),
    });
    table.materialized.push(MaterializedRange {
        axis: Axis::Vertical,
        visible: 0..20,
        overscan: 0..28,
    });
    table.materialized.push(MaterializedRange {
        axis: Axis::Horizontal,
        visible: 0..8,
        overscan: 0..12,
    });
    if let Some(root) = frame.nodes.get_mut(&frame.root) {
        root.children.push(table.id.clone());
    }
    frame.nodes.insert(table.id.clone(), table);
    frame
}

fn apply_style_patch(style: &mut BTreeMap<String, StyleValue>, patch: StylePatch) {
    for (key, value) in patch {
        match value {
            Some(value) => {
                style.insert(key, value);
            }
            None => {
                style.remove(&key);
            }
        }
    }
}

fn demand_from_materialized(node: &DocumentNode, materialized: &MaterializedRange) -> LayoutDemand {
    LayoutDemand {
        node: node.id.clone(),
        axis: materialized.axis,
        visible: materialized.visible.clone(),
        overscan: materialized.overscan.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_fill_uses_remaining_width_after_fixed_siblings() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(40.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(8.0));
        row.children.push(DocumentNodeId("fixed".to_owned()));
        row.children.push(DocumentNodeId("fill".to_owned()));

        let mut fixed = DocumentNode::new("fixed", DocumentNodeKind::Text);
        fixed.parent = Some(row.id.clone());
        fixed
            .style
            .insert("width".to_owned(), StyleValue::Number(50.0));
        fixed
            .style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        let mut fill = DocumentNode::new("fill", DocumentNodeKind::Text);
        fill.parent = Some(row.id.clone());
        fill.style
            .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
        fill.style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(fixed.id.clone(), fixed);
        frame.nodes.insert(fill.id.clone(), fill);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let fixed = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "fixed")
            .expect("fixed child should be laid out");
        let fill = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "fill")
            .expect("fill child should be laid out");

        assert_eq!(fixed.bounds.width, 50.0);
        assert_eq!(fill.bounds.x, 58.0);
        assert_eq!(fill.bounds.width, 242.0);
        assert!(fill.bounds.x + fill.bounds.width <= 300.0);
    }

    #[test]
    fn row_multiple_fill_children_share_remaining_width() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(330.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(40.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(15.0));

        for id in ["left", "middle", "right"] {
            row.children.push(DocumentNodeId(id.to_owned()));
            let mut child = DocumentNode::new(id, DocumentNodeKind::Stack);
            child.parent = Some(row.id.clone());
            child
                .style
                .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
            child
                .style
                .insert("height".to_owned(), StyleValue::Number(20.0));
            frame.nodes.insert(child.id.clone(), child);
        }

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 330.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let child = |id: &str| {
            layout
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap_or_else(|| panic!("child `{id}` should be laid out"))
        };
        let left = child("left");
        let middle = child("middle");
        let right = child("right");

        assert_eq!(left.bounds.width, 100.0);
        assert_eq!(middle.bounds.width, 100.0);
        assert_eq!(right.bounds.width, 100.0);
        assert_eq!(middle.bounds.x, 115.0);
        assert_eq!(right.bounds.x, 230.0);
        assert!(right.bounds.x + right.bounds.width <= 330.0);
    }

    #[test]
    fn button_with_element_label_shrinks_to_label_child() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(10.0));
        row.children.push(DocumentNodeId("one-button".to_owned()));
        row.children.push(DocumentNodeId("two-button".to_owned()));

        let mut one_button = DocumentNode::new("one-button", DocumentNodeKind::Button);
        one_button.parent = Some(row.id.clone());
        one_button
            .children
            .push(DocumentNodeId("one-label".to_owned()));

        let mut one_label = DocumentNode::new("one-label", DocumentNodeKind::Text);
        one_label.parent = Some(one_button.id.clone());
        one_label.text = Some(TextValue {
            text: "One".to_owned(),
        });

        let mut two_button = DocumentNode::new("two-button", DocumentNodeKind::Button);
        two_button.parent = Some(row.id.clone());
        two_button
            .children
            .push(DocumentNodeId("two-label".to_owned()));

        let mut two_label = DocumentNode::new("two-label", DocumentNodeKind::Text);
        two_label.parent = Some(two_button.id.clone());
        two_label.text = Some(TextValue {
            text: "Two".to_owned(),
        });

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(one_button.id.clone(), one_button);
        frame.nodes.insert(one_label.id.clone(), one_label);
        frame.nodes.insert(two_button.id.clone(), two_button);
        frame.nodes.insert(two_label.id.clone(), two_label);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let one_button = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "one-button")
            .expect("first button should be laid out");
        let one_label = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "one-label")
            .expect("first label should be laid out");
        let two_button = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "two-button")
            .expect("second button should be laid out");

        assert_eq!(one_label.bounds.width, 42.0);
        assert_eq!(one_button.bounds.width, one_label.bounds.width);
        assert_eq!(two_button.bounds.x, one_button.bounds.width + 10.0);
        assert!(two_button.bounds.x + two_button.bounds.width < 300.0);
    }

    #[test]
    fn checkbox_size_wins_over_accessibility_label_text() {
        let mut frame = DocumentFrame::empty("root");

        let mut checkbox = DocumentNode::new("checkbox", DocumentNodeKind::Checkbox);
        checkbox.parent = Some(frame.root.clone());
        checkbox
            .style
            .insert("size".to_owned(), StyleValue::Number(40.0));
        checkbox.text = Some(TextValue {
            text: "Reference[element:todo.title]".to_owned(),
        });

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(checkbox.id.clone());
        frame.nodes.insert(checkbox.id.clone(), checkbox);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let checkbox = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "checkbox")
            .expect("checkbox should be laid out");

        assert_eq!(checkbox.bounds.width, 40.0);
        assert_eq!(checkbox.bounds.height, 40.0);
    }

    #[test]
    fn inherited_font_size_does_not_force_stack_box_size() {
        let mut frame = DocumentFrame::empty("root");

        let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
        stack.parent = Some(frame.root.clone());
        stack
            .style
            .insert("size".to_owned(), StyleValue::Number(14.0));
        stack.children.push(DocumentNodeId("child".to_owned()));

        let mut child = DocumentNode::new("child", DocumentNodeKind::Text);
        child.parent = Some(stack.id.clone());
        child
            .style
            .insert("width".to_owned(), StyleValue::Number(100.0));
        child
            .style
            .insert("height".to_owned(), StyleValue::Number(50.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(stack.id.clone());
        frame.nodes.insert(stack.id.clone(), stack);
        frame.nodes.insert(child.id.clone(), child);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let stack = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "stack")
            .expect("stack should be laid out");

        assert_eq!(stack.bounds.height, 50.0);
    }
}
