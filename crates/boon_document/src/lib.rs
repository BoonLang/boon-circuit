use boon_document_model::{
    Axis, DocumentFrame, DocumentNode, DocumentNodeId, DocumentPatch, MaterializedRange,
    StylePatch, StyleValue, TextValue,
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
    pub bounds: Rect,
    pub text: Option<String>,
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
    let mut y = 0.0;
    let mut display_list = Vec::new();
    let mut hit_regions = Vec::new();
    let mut scroll_regions = Vec::new();
    let mut demands = Vec::new();
    let mut materialized_range_count = 0usize;

    for node in input.document.nodes.values() {
        let text = node.text.as_ref().map(|value| value.text.clone());
        let metrics = text
            .as_deref()
            .map(|text| input.text.measure(text, 14.0))
            .unwrap_or(TextMetrics {
                width: input.viewport.width,
                height: 24.0,
            });
        let height = metrics.height.max(24.0);
        let rect = Rect {
            x: 0.0,
            y,
            width: input.viewport.width.min(metrics.width.max(1.0)),
            height,
        };
        y += height;
        display_list.push(DisplayItem {
            node: node.id.clone(),
            bounds: rect,
            text,
        });
        if node.source_binding.is_some() {
            hit_regions.push(HitRegion {
                id: format!("hit:{}", node.id.0),
                node: node.id.clone(),
                bounds: rect,
            });
        }
        for range in &node.materialized {
            materialized_range_count += 1;
            scroll_regions.push(ScrollRegion {
                id: format!("scroll:{}", node.id.0),
                node: node.id.clone(),
                axis: range.axis,
                bounds: rect,
            });
            demands.push(demand_from_materialized(node, range));
        }
    }

    LayoutFrame {
        accessibility: AccessibilityTree {
            node_count: input.document.nodes.len(),
        },
        metrics: LayoutMetrics {
            node_count: input.document.nodes.len(),
            display_item_count: display_list.len(),
            materialized_range_count,
            native_capability_required: false,
        },
        display_list,
        hit_regions,
        scroll_regions,
        demands,
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
