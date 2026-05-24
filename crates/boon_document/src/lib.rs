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
            let rect = builder.layout_node(&child, 0.0, cursor_y, input.viewport.width);
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
    fn layout_node(&mut self, id: &DocumentNodeId, x: f32, y: f32, available_width: f32) -> Rect {
        let Some(node) = self.document.nodes.get(id).cloned() else {
            return Rect {
                x,
                y,
                width: 0.0,
                height: 0.0,
            };
        };
        let padding = style_spacing(&node.style, "padding").unwrap_or(0.0);
        let gap = style_spacing(&node.style, "gap").unwrap_or(0.0);
        let control_size = style_spacing(&node.style, "size").filter(|_| {
            matches!(
                node.kind,
                DocumentNodeKind::Button | DocumentNodeKind::GridCell
            ) && node.text.is_none()
        });
        let explicit_width =
            style_dimension(&node.style, "width", available_width).or(control_size);
        let explicit_height = style_dimension(&node.style, "height", 0.0).or(control_size);
        let text = node.text.as_ref().map(|value| value.text.clone());
        let measured = text
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                self.text
                    .measure(value, style_spacing(&node.style, "size").unwrap_or(14.0))
            })
            .unwrap_or(TextMetrics {
                width: 0.0,
                height: 0.0,
            });
        let mut width = explicit_width
            .unwrap_or_else(|| measured.width.max(available_width))
            .max(1.0);
        let mut height = explicit_height.unwrap_or_else(|| measured.height.max(24.0));
        let centered = style_bool(&node.style, "center").unwrap_or(false);
        let node_x = if centered && width < available_width {
            x + (available_width - width) / 2.0
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

        if !node.children.is_empty() {
            let content_x = node_x + padding;
            let content_y = y + padding;
            let content_width = (width - padding * 2.0).max(1.0);
            match node.kind {
                DocumentNodeKind::Row => {
                    let mut cursor_x = content_x;
                    let mut max_child_height: f32 = 0.0;
                    for child in &node.children {
                        let child_rect =
                            self.layout_node(child, cursor_x, content_y, content_width);
                        cursor_x += child_rect.width + gap;
                        max_child_height = max_child_height.max(child_rect.height);
                    }
                    if explicit_height.is_none() {
                        height = (max_child_height + padding * 2.0).max(24.0);
                    }
                }
                _ => {
                    let mut cursor_y = content_y;
                    let mut max_child_width: f32 = 0.0;
                    for child in &node.children {
                        let child_rect =
                            self.layout_node(child, content_x, cursor_y, content_width);
                        cursor_y += child_rect.height + gap;
                        max_child_width = max_child_width.max(child_rect.width);
                    }
                    if explicit_width.is_none() {
                        width = max_child_width.max(width).max(1.0) + padding * 2.0;
                    }
                    if explicit_height.is_none() {
                        height = (cursor_y - y - gap).max(24.0) + padding;
                    }
                }
            }
        }

        let rect = Rect {
            x: node_x,
            y,
            width,
            height,
        };
        self.display_list[display_index].bounds = rect;
        if node.source_binding.is_some() {
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

fn style_dimension(
    style: &BTreeMap<String, StyleValue>,
    key: &str,
    fill_width: f32,
) -> Option<f32> {
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) if value == "fill" => Some(fill_width),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) => None,
    }
}

#[derive(Default)]
pub struct SimpleTextMeasurer;

impl TextMeasurer for SimpleTextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> TextMetrics {
        TextMetrics {
            width: text.chars().count() as f32 * font_size * 0.55,
            height: font_size * 1.4,
        }
    }
}

pub fn fixture_frame_with_virtualized_grid() -> DocumentFrame {
    let mut frame = DocumentFrame::empty("root");
    let mut grid = DocumentNode::new("virtual-grid", boon_document_model::DocumentNodeKind::Grid);
    grid.parent = Some(frame.root.clone());
    grid.text = Some(TextValue {
        text: "Virtualized logical grid".to_owned(),
    });
    grid.materialized.push(MaterializedRange {
        axis: Axis::Vertical,
        visible: 0..20,
        overscan: 0..28,
    });
    grid.materialized.push(MaterializedRange {
        axis: Axis::Horizontal,
        visible: 0..8,
        overscan: 0..12,
    });
    if let Some(root) = frame.nodes.get_mut(&frame.root) {
        root.children.push(grid.id.clone());
    }
    frame.nodes.insert(grid.id.clone(), grid);
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
