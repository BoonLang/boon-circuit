pub use boon_document_model::{
    Axis, DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, DocumentPatch,
    MaterializedRange, ScrollRootId, SourceBindingId, StyleEditorTypeHint, StyleMap, StylePatch,
    StyleRichTextSpan, StyleValue, TextValue,
};
pub mod render_scene;
use boon_host::Viewport;
pub use render_scene::{
    RenderFontStyle, RenderFontWeight, RenderQuadBatch, RenderRichTextSpan, RenderScene,
    RenderSceneItem, RenderSceneMetrics, RenderTextAlign, RenderTextPlacementKey, RenderTextRun,
    RenderTextShapeKey, RenderTextVerticalAlign, RenderTextureRef, RenderVisualPrimitive,
    RenderVisualPrimitiveKind, RetainedRenderChunkDescriptor,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
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

pub struct LayoutSubtreeInput<'a> {
    pub document: &'a DocumentFrame,
    pub root: &'a DocumentNodeId,
    pub x: f32,
    pub y: f32,
    pub available_width: f32,
    pub available_height: f32,
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
    pub materialization: Vec<MaterializationReport>,
    pub metrics: LayoutMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayItem {
    pub node: DocumentNodeId,
    pub kind: DocumentNodeKind,
    pub bounds: Rect,
    pub text: Option<String>,
    pub style_identity: ComputedStyleIdentity,
    pub style: BTreeMap<String, StyleValue>,
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComputedStyleIdentity {
    pub style_id: u64,
    pub layout_id: u64,
    pub paint_id: u64,
    pub material_id: u64,
    pub font_id: u64,
    pub pseudo_state_id: u64,
}

impl ComputedStyleIdentity {
    pub fn from_style(style: &BTreeMap<String, StyleValue>) -> Self {
        computed_style_identity(style)
    }
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HitSideTable {
    pub bucket_size: f32,
    pub entries: Vec<HitSideTableEntry>,
    pub buckets: BTreeMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitSideTableEntry {
    pub hit_id: String,
    pub node: DocumentNodeId,
    pub source_binding_id: Option<SourceBindingId>,
    pub source_path: Option<String>,
    pub source_intent: Option<String>,
    pub bounds: Rect,
    pub z_depth: u32,
    pub scroll_root: Option<ScrollRootId>,
    pub row_key: Option<u64>,
    pub row_generation: Option<u64>,
    pub spatial_bucket: HitSpatialBucket,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct HitSpatialBucket {
    pub x: i32,
    pub y: i32,
}

impl HitSideTable {
    pub const DEFAULT_BUCKET_SIZE: f32 = 128.0;

    pub fn from_document_layout(document: &DocumentFrame, layout: &LayoutFrame) -> Self {
        Self::from_document_layout_with_bucket_size(document, layout, Self::DEFAULT_BUCKET_SIZE)
    }

    pub fn from_document_layout_with_bucket_size(
        document: &DocumentFrame,
        layout: &LayoutFrame,
        bucket_size: f32,
    ) -> Self {
        let bucket_size = if bucket_size.is_finite() && bucket_size > 0.0 {
            bucket_size
        } else {
            Self::DEFAULT_BUCKET_SIZE
        };
        let mut table = Self {
            bucket_size,
            entries: Vec::with_capacity(layout.hit_regions.len()),
            buckets: BTreeMap::new(),
        };
        for (index, hit) in layout.hit_regions.iter().enumerate() {
            let node = document.nodes.get(&hit.node);
            let binding = node.and_then(|node| node.source_binding.as_ref());
            let spatial_bucket = hit_bucket_for_point(hit.bounds.x, hit.bounds.y, bucket_size);
            let entry_index = table.entries.len();
            let entry = HitSideTableEntry {
                hit_id: hit.id.clone(),
                node: hit.node.clone(),
                source_binding_id: binding.map(|binding| binding.id.clone()),
                source_path: binding.map(|binding| binding.source_path.clone()),
                source_intent: binding.map(|binding| binding.intent.clone()),
                bounds: hit.bounds,
                z_depth: index as u32,
                scroll_root: scroll_root_for_node(document, &hit.node),
                row_key: node.and_then(|node| {
                    style_u64_any(&node.style, &["row_key", "target_key", "__row_key"])
                }),
                row_generation: node.and_then(|node| {
                    style_u64_any(
                        &node.style,
                        &[
                            "row_generation",
                            "target_generation",
                            "generation",
                            "__row_generation",
                        ],
                    )
                }),
                spatial_bucket,
            };
            for bucket in buckets_for_rect(hit.bounds, bucket_size) {
                table
                    .buckets
                    .entry(hit_bucket_key(bucket))
                    .or_default()
                    .push(entry_index);
            }
            table.entries.push(entry);
        }
        table
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<&HitSideTableEntry> {
        let bucket = hit_bucket_for_point(x, y, self.bucket_size);
        let candidates = self.buckets.get(&hit_bucket_key(bucket))?;
        candidates
            .iter()
            .filter_map(|index| self.entries.get(*index).map(|entry| (*index, entry)))
            .filter(|(_, entry)| rect_contains(entry.bounds, x, y))
            .min_by(|left, right| compare_typed_hit_priority(left, right, x, y))
            .map(|(_, entry)| entry)
    }

    pub fn entry_for_source_path(&self, source_path: &str) -> Option<&HitSideTableEntry> {
        self.entries
            .iter()
            .find(|entry| entry.source_path.as_deref() == Some(source_path))
    }

    pub fn bucket_indices(&self, bucket: HitSpatialBucket) -> Option<&Vec<usize>> {
        self.buckets.get(&hit_bucket_key(bucket))
    }
}

fn hit_bucket_for_point(x: f32, y: f32, bucket_size: f32) -> HitSpatialBucket {
    HitSpatialBucket {
        x: (x / bucket_size).floor() as i32,
        y: (y / bucket_size).floor() as i32,
    }
}

fn hit_bucket_key(bucket: HitSpatialBucket) -> String {
    format!("{},{}", bucket.x, bucket.y)
}

fn buckets_for_rect(rect: Rect, bucket_size: f32) -> Vec<HitSpatialBucket> {
    let min = hit_bucket_for_point(rect.x, rect.y, bucket_size);
    let max = hit_bucket_for_point(
        rect.x + rect.width.max(0.0),
        rect.y + rect.height.max(0.0),
        bucket_size,
    );
    let mut buckets = Vec::new();
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            buckets.push(HitSpatialBucket { x, y });
        }
    }
    buckets
}

fn compare_typed_hit_priority(
    left: &(usize, &HitSideTableEntry),
    right: &(usize, &HitSideTableEntry),
    x: f32,
    y: f32,
) -> std::cmp::Ordering {
    let left_live = left.1.source_path.is_some();
    let right_live = right.1.source_path.is_some();
    right_live
        .cmp(&left_live)
        .then_with(|| {
            rect_center_distance2(left.1.bounds, x, y).total_cmp(&rect_center_distance2(
                right.1.bounds,
                x,
                y,
            ))
        })
        .then_with(|| rect_area(left.1.bounds).total_cmp(&rect_area(right.1.bounds)))
        .then_with(|| right.1.z_depth.cmp(&left.1.z_depth))
        .then_with(|| right.0.cmp(&left.0))
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn rect_area(rect: Rect) -> f32 {
    rect.width.max(0.0) * rect.height.max(0.0)
}

fn rect_center_distance2(rect: Rect, x: f32, y: f32) -> f32 {
    let center_x = rect.x + rect.width / 2.0;
    let center_y = rect.y + rect.height / 2.0;
    let dx = center_x - x;
    let dy = center_y - y;
    dx * dx + dy * dy
}

fn scroll_root_for_node(document: &DocumentFrame, node: &DocumentNodeId) -> Option<ScrollRootId> {
    let mut current = Some(node.clone());
    while let Some(id) = current {
        let scroll_root = ScrollRootId(id.0.clone());
        if document.scroll_roots.contains_key(&scroll_root) {
            return Some(scroll_root);
        }
        current = document.nodes.get(&id).and_then(|node| node.parent.clone());
    }
    None
}

fn style_u64_any(style: &StyleMap, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| style_u64(style, key))
}

fn style_u64(style: &StyleMap, key: &str) -> Option<u64> {
    match style.get(key)? {
        StyleValue::Number(value) if *value >= 0.0 => Some(*value as u64),
        StyleValue::Text(value) => value.parse::<u64>().ok(),
        StyleValue::Bool(_)
        | StyleValue::Number(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
    }
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
    pub logical_item_count: u64,
    pub materialized_item_count: u64,
    pub stable_key_prefix: String,
    pub first_stable_key: Option<String>,
    pub last_stable_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaterializationReport {
    pub node: DocumentNodeId,
    pub axis: Axis,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
    pub logical_item_count: u64,
    pub materialized_item_count: u64,
    pub stable_key_prefix: String,
    pub first_stable_key: Option<String>,
    pub last_stable_key: Option<String>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchInvalidationClass {
    Structure,
    Text,
    Style,
    Binding,
    Scroll,
    Materialization,
    Layout,
    HitRegion,
    PaintOnly,
    LayoutOnly,
    SourceBinding,
    ListStructure,
    ConditionalStructure,
    ScrollOffsetOnly,
    MaterializationOnly,
    FullDocument,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PatchApplyReport {
    pub patch_kind: &'static str,
    pub target: Option<DocumentNodeId>,
    pub invalidation: Vec<PatchInvalidationClass>,
    pub removed_nodes: Vec<DocumentNodeId>,
    pub node_count_after: usize,
    pub materialization: Option<MaterializationReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchApplyError {
    MissingTarget {
        patch_kind: &'static str,
        id: DocumentNodeId,
    },
    MissingParent {
        id: DocumentNodeId,
        parent: DocumentNodeId,
    },
    CannotRemoveRoot {
        id: DocumentNodeId,
    },
    DuplicateChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
    },
    OrphanedChild {
        parent: DocumentNodeId,
        child: DocumentNodeId,
    },
    InvalidParentChildLink {
        parent: DocumentNodeId,
        child: DocumentNodeId,
        actual_parent: Option<DocumentNodeId>,
    },
    OrphanedNode {
        id: DocumentNodeId,
        parent: Option<DocumentNodeId>,
    },
    Cycle {
        id: DocumentNodeId,
    },
    StaleReference {
        reference_kind: &'static str,
        id: DocumentNodeId,
    },
}

impl fmt::Display for PatchApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTarget { patch_kind, id } => {
                write!(f, "{patch_kind} target `{}` does not exist", id.0)
            }
            Self::MissingParent { id, parent } => {
                write!(f, "node `{}` parent `{}` does not exist", id.0, parent.0)
            }
            Self::CannotRemoveRoot { id } => write!(f, "cannot remove root node `{}`", id.0),
            Self::DuplicateChild { parent, child } => write!(
                f,
                "node `{}` lists child `{}` more than once",
                parent.0, child.0
            ),
            Self::OrphanedChild { parent, child } => {
                write!(
                    f,
                    "node `{}` references missing child `{}`",
                    parent.0, child.0
                )
            }
            Self::InvalidParentChildLink {
                parent,
                child,
                actual_parent,
            } => write!(
                f,
                "node `{}` references child `{}` whose parent is {:?}",
                parent.0,
                child.0,
                actual_parent.as_ref().map(|id| id.0.as_str())
            ),
            Self::OrphanedNode { id, parent } => write!(
                f,
                "node `{}` has parent {:?} but is not reachable from the root",
                id.0,
                parent.as_ref().map(|id| id.0.as_str())
            ),
            Self::Cycle { id } => write!(f, "node `{}` participates in a parent cycle", id.0),
            Self::StaleReference { reference_kind, id } => {
                write!(f, "{reference_kind} references stale node `{}`", id.0)
            }
        }
    }
}

impl std::error::Error for PatchApplyError {}

impl DocumentState {
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            frame: DocumentFrame::empty(root),
        }
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn apply_patch(
        &mut self,
        patch: DocumentPatch,
    ) -> Result<PatchApplyReport, PatchApplyError> {
        validate_frame_integrity(&self.frame)?;
        let report = match patch {
            DocumentPatch::UpsertNode(node) => {
                let target = node.id.clone();
                apply_upsert_node(&mut self.frame, node)?;
                PatchApplyReport {
                    patch_kind: "upsert_node",
                    target: Some(target),
                    invalidation: vec![
                        PatchInvalidationClass::Structure,
                        PatchInvalidationClass::ListStructure,
                        PatchInvalidationClass::ConditionalStructure,
                        PatchInvalidationClass::Layout,
                        PatchInvalidationClass::LayoutOnly,
                        PatchInvalidationClass::HitRegion,
                        PatchInvalidationClass::FullDocument,
                    ],
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::RemoveNode { id } => {
                let removed_nodes = remove_subtree(&mut self.frame, &id)?;
                PatchApplyReport {
                    patch_kind: "remove_node",
                    target: Some(id),
                    invalidation: vec![
                        PatchInvalidationClass::Structure,
                        PatchInvalidationClass::ListStructure,
                        PatchInvalidationClass::Binding,
                        PatchInvalidationClass::SourceBinding,
                        PatchInvalidationClass::Scroll,
                        PatchInvalidationClass::ScrollOffsetOnly,
                        PatchInvalidationClass::Materialization,
                        PatchInvalidationClass::MaterializationOnly,
                        PatchInvalidationClass::Layout,
                        PatchInvalidationClass::LayoutOnly,
                        PatchInvalidationClass::HitRegion,
                        PatchInvalidationClass::FullDocument,
                    ],
                    removed_nodes,
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::SetText { id, text } => {
                let node = required_node_mut(&mut self.frame, "set_text", &id)?;
                node.text = Some(text);
                PatchApplyReport {
                    patch_kind: "set_text",
                    target: Some(id),
                    invalidation: vec![
                        PatchInvalidationClass::Text,
                        PatchInvalidationClass::PaintOnly,
                        PatchInvalidationClass::Layout,
                        PatchInvalidationClass::LayoutOnly,
                        PatchInvalidationClass::HitRegion,
                    ],
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::SetStyle { id, patch } => {
                let node = required_node_mut(&mut self.frame, "set_style", &id)?;
                let changed_keys = apply_style_patch(&mut node.style, patch);
                let invalidation = style_patch_invalidation(&changed_keys);
                PatchApplyReport {
                    patch_kind: "set_style",
                    target: Some(id),
                    invalidation,
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::SetBinding { id, binding } => {
                let node = required_node_mut(&mut self.frame, "set_binding", &id)?;
                node.source_binding = Some(binding);
                PatchApplyReport {
                    patch_kind: "set_binding",
                    target: Some(id),
                    invalidation: vec![
                        PatchInvalidationClass::Binding,
                        PatchInvalidationClass::SourceBinding,
                        PatchInvalidationClass::HitRegion,
                    ],
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::SetScroll { id, scroll } => {
                let node = required_node_mut(&mut self.frame, "set_scroll", &id)?;
                node.scroll = Some(scroll);
                PatchApplyReport {
                    patch_kind: "set_scroll",
                    target: Some(id),
                    invalidation: vec![
                        PatchInvalidationClass::Scroll,
                        PatchInvalidationClass::ScrollOffsetOnly,
                        PatchInvalidationClass::Layout,
                        PatchInvalidationClass::LayoutOnly,
                    ],
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: None,
                }
            }
            DocumentPatch::SetListMaterialization { id, materialized } => {
                let node = required_node_mut(&mut self.frame, "set_list_materialization", &id)?;
                let report = materialization_report(node, &materialized);
                node.materialized.push(materialized);
                PatchApplyReport {
                    patch_kind: "set_list_materialization",
                    target: Some(id),
                    invalidation: vec![
                        PatchInvalidationClass::Materialization,
                        PatchInvalidationClass::MaterializationOnly,
                        PatchInvalidationClass::Layout,
                        PatchInvalidationClass::LayoutOnly,
                        PatchInvalidationClass::HitRegion,
                    ],
                    removed_nodes: Vec::new(),
                    node_count_after: self.frame.nodes.len(),
                    materialization: Some(report),
                }
            }
        };
        validate_frame_integrity(&self.frame)?;
        Ok(report)
    }
}

fn required_node_mut<'a>(
    frame: &'a mut DocumentFrame,
    patch_kind: &'static str,
    id: &DocumentNodeId,
) -> Result<&'a mut DocumentNode, PatchApplyError> {
    frame
        .nodes
        .get_mut(id)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind,
            id: id.clone(),
        })
}

fn apply_upsert_node(frame: &mut DocumentFrame, node: DocumentNode) -> Result<(), PatchApplyError> {
    let id = node.id.clone();
    if id == frame.root && node.parent.is_some() {
        return Err(PatchApplyError::InvalidParentChildLink {
            parent: id.clone(),
            child: id,
            actual_parent: node.parent,
        });
    }
    if id != frame.root {
        let parent = node
            .parent
            .clone()
            .ok_or_else(|| PatchApplyError::OrphanedNode {
                id: id.clone(),
                parent: None,
            })?;
        if !frame.nodes.contains_key(&parent) {
            return Err(PatchApplyError::MissingParent {
                id: id.clone(),
                parent,
            });
        }
    }
    validate_child_refs(frame, &node)?;

    if let Some(old_parent) = frame.nodes.get(&id).and_then(|old| old.parent.clone())
        && old_parent != node.parent.clone().unwrap_or_else(|| frame.root.clone())
        && let Some(parent) = frame.nodes.get_mut(&old_parent)
    {
        parent.children.retain(|child| child != &id);
    }
    let parent = node.parent.clone();
    frame.nodes.insert(id.clone(), node);
    if let Some(parent_id) = parent
        && let Some(parent) = frame.nodes.get_mut(&parent_id)
        && !parent.children.contains(&id)
    {
        parent.children.push(id);
    }
    Ok(())
}

fn validate_child_refs(frame: &DocumentFrame, node: &DocumentNode) -> Result<(), PatchApplyError> {
    let mut seen = BTreeSet::new();
    for child in &node.children {
        if !seen.insert(child.clone()) {
            return Err(PatchApplyError::DuplicateChild {
                parent: node.id.clone(),
                child: child.clone(),
            });
        }
        let Some(child_node) = frame.nodes.get(child) else {
            return Err(PatchApplyError::OrphanedChild {
                parent: node.id.clone(),
                child: child.clone(),
            });
        };
        if child_node.parent.as_ref() != Some(&node.id) {
            return Err(PatchApplyError::InvalidParentChildLink {
                parent: node.id.clone(),
                child: child.clone(),
                actual_parent: child_node.parent.clone(),
            });
        }
    }
    Ok(())
}

fn remove_subtree(
    frame: &mut DocumentFrame,
    id: &DocumentNodeId,
) -> Result<Vec<DocumentNodeId>, PatchApplyError> {
    if id == &frame.root {
        return Err(PatchApplyError::CannotRemoveRoot { id: id.clone() });
    }
    if !frame.nodes.contains_key(id) {
        return Err(PatchApplyError::MissingTarget {
            patch_kind: "remove_node",
            id: id.clone(),
        });
    }
    let mut removed = Vec::new();
    collect_subtree(frame, id, &mut removed)?;
    if let Some(parent_id) = frame.nodes.get(id).and_then(|node| node.parent.clone())
        && let Some(parent) = frame.nodes.get_mut(&parent_id)
    {
        parent.children.retain(|child| child != id);
    }
    for node_id in &removed {
        frame.nodes.remove(node_id);
    }
    if frame
        .focus
        .as_ref()
        .is_some_and(|focus| removed.contains(focus))
    {
        frame.focus = None;
    }
    Ok(removed)
}

fn collect_subtree(
    frame: &DocumentFrame,
    id: &DocumentNodeId,
    removed: &mut Vec<DocumentNodeId>,
) -> Result<(), PatchApplyError> {
    let node = frame
        .nodes
        .get(id)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind: "remove_node",
            id: id.clone(),
        })?;
    removed.push(id.clone());
    for child in &node.children {
        collect_subtree(frame, child, removed)?;
    }
    Ok(())
}

fn validate_frame_integrity(frame: &DocumentFrame) -> Result<(), PatchApplyError> {
    let root = frame
        .nodes
        .get(&frame.root)
        .ok_or_else(|| PatchApplyError::MissingTarget {
            patch_kind: "frame_root",
            id: frame.root.clone(),
        })?;
    if root.parent.is_some() {
        return Err(PatchApplyError::InvalidParentChildLink {
            parent: frame.root.clone(),
            child: frame.root.clone(),
            actual_parent: root.parent.clone(),
        });
    }
    for node in frame.nodes.values() {
        validate_child_refs(frame, node)?;
        if node.id != frame.root {
            let Some(parent) = node.parent.as_ref() else {
                return Err(PatchApplyError::OrphanedNode {
                    id: node.id.clone(),
                    parent: None,
                });
            };
            let Some(parent_node) = frame.nodes.get(parent) else {
                return Err(PatchApplyError::MissingParent {
                    id: node.id.clone(),
                    parent: parent.clone(),
                });
            };
            if !parent_node.children.contains(&node.id) {
                return Err(PatchApplyError::InvalidParentChildLink {
                    parent: parent.clone(),
                    child: node.id.clone(),
                    actual_parent: node.parent.clone(),
                });
            }
            validate_parent_chain_reaches_root(frame, &node.id)?;
        }
    }
    if let Some(focus) = &frame.focus
        && !frame.nodes.contains_key(focus)
    {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "focus",
            id: focus.clone(),
        });
    }
    Ok(())
}

fn validate_parent_chain_reaches_root(
    frame: &DocumentFrame,
    id: &DocumentNodeId,
) -> Result<(), PatchApplyError> {
    let mut seen = BTreeSet::new();
    let mut current = id.clone();
    while current != frame.root {
        if !seen.insert(current.clone()) {
            return Err(PatchApplyError::Cycle { id: current });
        }
        let Some(node) = frame.nodes.get(&current) else {
            return Err(PatchApplyError::MissingTarget {
                patch_kind: "parent_chain",
                id: current,
            });
        };
        let Some(parent) = node.parent.clone() else {
            return Err(PatchApplyError::OrphanedNode {
                id: node.id.clone(),
                parent: None,
            });
        };
        current = parent;
    }
    Ok(())
}

pub fn try_layout(input: LayoutInput<'_>) -> Result<LayoutFrame, PatchApplyError> {
    validate_frame_integrity(input.document)?;
    Ok(layout_unchecked(input))
}

pub fn layout(input: LayoutInput<'_>) -> LayoutFrame {
    try_layout(input).expect("document layout frame failed integrity validation")
}

pub fn try_layout_subtree(input: LayoutSubtreeInput<'_>) -> Result<LayoutFrame, PatchApplyError> {
    validate_frame_integrity(input.document)?;
    Ok(layout_subtree_unchecked(input))
}

pub fn layout_subtree(input: LayoutSubtreeInput<'_>) -> LayoutFrame {
    try_layout_subtree(input).expect("document subtree layout frame failed integrity validation")
}

fn layout_unchecked(input: LayoutInput<'_>) -> LayoutFrame {
    let mut builder = LayoutBuilder {
        document: input.document,
        text: input.text,
        display_list: Vec::new(),
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        demands: Vec::new(),
        materialization: Vec::new(),
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
        materialization: builder.materialization,
    }
}

fn layout_subtree_unchecked(input: LayoutSubtreeInput<'_>) -> LayoutFrame {
    let mut builder = LayoutBuilder {
        document: input.document,
        text: input.text,
        display_list: Vec::new(),
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        demands: Vec::new(),
        materialization: Vec::new(),
        materialized_range_count: 0,
    };
    let subtree_node_count = document_subtree_node_count(input.document, input.root);
    builder.layout_node(
        input.root,
        input.x,
        input.y,
        input.available_width,
        input.available_height,
    );

    LayoutFrame {
        accessibility: AccessibilityTree {
            node_count: subtree_node_count,
        },
        metrics: LayoutMetrics {
            node_count: subtree_node_count,
            display_item_count: builder.display_list.len(),
            materialized_range_count: builder.materialized_range_count,
            native_capability_required: false,
        },
        display_list: builder.display_list,
        hit_regions: builder.hit_regions,
        scroll_regions: builder.scroll_regions,
        demands: builder.demands,
        materialization: builder.materialization,
    }
}

fn document_subtree_node_count(document: &DocumentFrame, root: &DocumentNodeId) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root.clone()];
    while let Some(id) = stack.pop() {
        let Some(node) = document.nodes.get(&id) else {
            continue;
        };
        count = count.saturating_add(1);
        stack.extend(node.children.iter().cloned());
    }
    count
}

struct LayoutBuilder<'a, 'b> {
    document: &'a DocumentFrame,
    text: &'b mut dyn TextMeasurer,
    display_list: Vec<DisplayItem>,
    hit_regions: Vec<HitRegion>,
    scroll_regions: Vec<ScrollRegion>,
    demands: Vec<LayoutDemand>,
    materialization: Vec<MaterializationReport>,
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
        let style_identity = computed_style_identity(&node.style);
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
            style_identity,
            style: node.style.clone(),
            focused: self.document.focus.as_ref() == Some(&node.id),
        });
        let subtree_display_start = self.display_list.len();
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
                _ if style_bool(&node.style, "overlay_children").unwrap_or(false) => {
                    let mut max_child_width: f32 = 0.0;
                    let mut max_child_height: f32 = 0.0;
                    for child in &node.children {
                        let child_rect = self.layout_node(
                            child,
                            content_x,
                            content_y,
                            content_width,
                            (height - padding.vertical()).max(1.0),
                        );
                        max_child_width = max_child_width.max(child_rect.width);
                        max_child_height = max_child_height.max(child_rect.height);
                    }
                    if explicit_width.is_none() {
                        width = constrain_dimension(
                            max_child_width.max(width).max(1.0) + padding.horizontal(),
                            &node.style,
                            "width",
                            available_width,
                        );
                    }
                    if explicit_height.is_none() {
                        height = constrain_dimension(
                            (max_child_height + padding.vertical()).max(24.0),
                            &node.style,
                            "height",
                            available_height,
                        );
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
        if !node.materialized.is_empty() {
            apply_clip_to_display_items(&mut self.display_list[subtree_display_start..], rect);
        }
        if node.source_binding.is_some() || style_bool(&node.style, "__hover_scope") == Some(true) {
            self.hit_regions.push(HitRegion {
                id: format!("hit:{}", node.id.0),
                node: node.id.clone(),
                bounds: rect,
            });
        }
        for range in &node.materialized {
            self.materialized_range_count += 1;
            let report = materialization_report(&node, range);
            self.scroll_regions.push(ScrollRegion {
                id: format!("scroll:{}", node.id.0),
                node: node.id.clone(),
                axis: range.axis,
                bounds: rect,
            });
            self.demands.push(demand_from_report(&report));
            self.materialization.push(report);
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
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_bool(style: &BTreeMap<String, StyleValue>, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => {
            None
        }
    }
}

fn apply_clip_to_display_items(items: &mut [DisplayItem], clip: Rect) {
    for item in items {
        let clip = item_clip_rect(item)
            .and_then(|existing| rect_intersection(existing, clip))
            .unwrap_or(clip);
        item.style
            .insert("__clip_x".to_owned(), StyleValue::Number(f64::from(clip.x)));
        item.style
            .insert("__clip_y".to_owned(), StyleValue::Number(f64::from(clip.y)));
        item.style.insert(
            "__clip_width".to_owned(),
            StyleValue::Number(f64::from(clip.width)),
        );
        item.style.insert(
            "__clip_height".to_owned(),
            StyleValue::Number(f64::from(clip.height)),
        );
        item.style_identity = computed_style_identity(&item.style);
    }
}

fn item_clip_rect(item: &DisplayItem) -> Option<Rect> {
    Some(Rect {
        x: style_spacing(&item.style, "__clip_x")?,
        y: style_spacing(&item.style, "__clip_y")?,
        width: style_spacing(&item.style, "__clip_width")?,
        height: style_spacing(&item.style, "__clip_height")?,
    })
}

fn rect_intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn style_text<'a>(style: &'a BTreeMap<String, StyleValue>, key: &str) -> Option<&'a str> {
    match style.get(key)? {
        StyleValue::Text(value) => Some(value.as_str()),
        StyleValue::Bool(_)
        | StyleValue::Number(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
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
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
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

fn apply_style_patch(
    style: &mut BTreeMap<String, StyleValue>,
    patch: StylePatch,
) -> BTreeSet<String> {
    let mut changed_keys = BTreeSet::new();
    for (key, value) in patch {
        match value {
            Some(value) => {
                if style.get(&key) != Some(&value) {
                    changed_keys.insert(key.clone());
                    style.insert(key, value);
                }
            }
            None => {
                if style.remove(&key).is_some() {
                    changed_keys.insert(key);
                }
            }
        }
    }
    changed_keys
}

fn style_patch_invalidation(changed_keys: &BTreeSet<String>) -> Vec<PatchInvalidationClass> {
    let mut invalidation = vec![PatchInvalidationClass::Style];
    if changed_keys.is_empty() {
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        return invalidation;
    }
    for key in changed_keys {
        if !style_key_is_known(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::FullDocument);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::Layout);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
            continue;
        }
        if style_key_affects_layout(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::Layout);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::LayoutOnly);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
        if style_key_affects_paint(key) || style_key_affects_material(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
        }
        if style_key_affects_pseudo_state(key) {
            push_unique_invalidation(
                &mut invalidation,
                PatchInvalidationClass::ConditionalStructure,
            );
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
        if style_key_affects_source_binding(key) {
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::SourceBinding);
            push_unique_invalidation(&mut invalidation, PatchInvalidationClass::HitRegion);
        }
    }
    if invalidation.len() == 1 {
        push_unique_invalidation(&mut invalidation, PatchInvalidationClass::PaintOnly);
    }
    invalidation
}

fn push_unique_invalidation(
    invalidation: &mut Vec<PatchInvalidationClass>,
    class: PatchInvalidationClass,
) {
    if !invalidation.contains(&class) {
        invalidation.push(class);
    }
}

fn computed_style_identity(style: &BTreeMap<String, StyleValue>) -> ComputedStyleIdentity {
    ComputedStyleIdentity {
        style_id: stable_style_hash(style, StyleHashCategory::All),
        layout_id: stable_style_hash(style, StyleHashCategory::Layout),
        paint_id: stable_style_hash(style, StyleHashCategory::Paint),
        material_id: stable_style_hash(style, StyleHashCategory::Material),
        font_id: stable_style_hash(style, StyleHashCategory::Font),
        pseudo_state_id: stable_style_hash(style, StyleHashCategory::PseudoState),
    }
}

#[derive(Clone, Copy)]
enum StyleHashCategory {
    All,
    Layout,
    Paint,
    Material,
    Font,
    PseudoState,
}

fn stable_style_hash(style: &BTreeMap<String, StyleValue>, category: StyleHashCategory) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    stable_hash_bytes(&mut hash, b"boon-style-v1");
    for (key, value) in style {
        if !style_key_in_hash_category(key, category) {
            continue;
        }
        stable_hash_bytes(&mut hash, key.as_bytes());
        stable_hash_bytes(&mut hash, &[0]);
        stable_hash_style_value(&mut hash, value);
        stable_hash_bytes(&mut hash, &[0xff]);
    }
    hash
}

fn stable_hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn stable_hash_style_value(hash: &mut u64, value: &StyleValue) {
    match value {
        StyleValue::Text(value) => {
            stable_hash_bytes(hash, b"text:");
            stable_hash_bytes(hash, value.as_bytes());
        }
        StyleValue::Number(value) => {
            stable_hash_bytes(hash, b"number:");
            stable_hash_bytes(hash, &value.to_bits().to_le_bytes());
        }
        StyleValue::Bool(value) => {
            stable_hash_bytes(hash, b"bool:");
            stable_hash_bytes(hash, &[*value as u8]);
        }
        StyleValue::RichTextSpans(spans) => {
            stable_hash_bytes(hash, b"rich_text_spans:");
            stable_hash_bytes(hash, &spans.len().to_le_bytes());
            for span in spans {
                stable_hash_bytes(hash, span.text.as_bytes());
                stable_hash_optional_text(hash, span.source_text.as_deref());
                stable_hash_optional_text(hash, span.color.as_deref());
                stable_hash_optional_text(hash, span.font_style.as_deref());
                stable_hash_optional_text(hash, span.font_weight.as_deref());
            }
        }
        StyleValue::EditorTypeHints(hints) => {
            stable_hash_bytes(hash, b"editor_type_hints:");
            stable_hash_bytes(hash, &hints.len().to_le_bytes());
            for hint in hints {
                stable_hash_bytes(hash, &hint.line.to_le_bytes());
                stable_hash_bytes(hash, &hint.start.to_le_bytes());
                stable_hash_bytes(hash, &hint.end.to_le_bytes());
                stable_hash_bytes(hash, &hint.anchor_column.to_le_bytes());
                stable_hash_bytes(hash, hint.category.as_bytes());
                stable_hash_bytes(hash, hint.compact_label.as_bytes());
                stable_hash_bytes(hash, hint.detail_label.as_bytes());
            }
        }
    }
}

fn stable_hash_optional_text(hash: &mut u64, value: Option<&str>) {
    match value {
        Some(value) => {
            stable_hash_bytes(hash, &[1]);
            stable_hash_bytes(hash, value.as_bytes());
        }
        None => stable_hash_bytes(hash, &[0]),
    }
}

fn style_key_in_hash_category(key: &str, category: StyleHashCategory) -> bool {
    match category {
        StyleHashCategory::All => true,
        StyleHashCategory::Layout => style_key_affects_layout(key),
        StyleHashCategory::Paint => style_key_affects_paint(key),
        StyleHashCategory::Material => style_key_affects_material(key),
        StyleHashCategory::Font => style_key_affects_font(key),
        StyleHashCategory::PseudoState => style_key_affects_pseudo_state(key),
    }
}

fn style_key_is_known(key: &str) -> bool {
    style_key_affects_layout(key)
        || style_key_affects_paint(key)
        || style_key_affects_material(key)
        || style_key_affects_font(key)
        || style_key_affects_pseudo_state(key)
        || style_key_affects_source_binding(key)
}

fn style_key_affects_layout(key: &str) -> bool {
    key == "width"
        || key == "height"
        || key == "min_width"
        || key == "max_width"
        || key == "min_height"
        || key == "max_height"
        || key == "gap"
        || key == "size"
        || key == "box_size"
        || key == "auto_padding"
        || key == "center"
        || key == "align_x"
        || key == "overlay_children"
        || key == "placeholder"
        || key.starts_with("padding")
        || key.starts_with("__clip_")
}

fn style_key_affects_paint(key: &str) -> bool {
    key == "color"
        || key == "background"
        || key == "background_color"
        || key == "border_color"
        || key == "opacity"
        || key == "relief"
        || key == "depth"
        || key == "shadow"
        || key == "outline"
}

fn style_key_affects_material(key: &str) -> bool {
    key == "material"
        || key == "texture"
        || key == "image"
        || key == "shader"
        || key == "border_radius"
        || key == "clip"
}

fn style_key_affects_font(key: &str) -> bool {
    key == "size"
        || key == "font"
        || key == "font_family"
        || key == "font_weight"
        || key == "font_style"
        || key == "line_height"
        || key == "letter_spacing"
}

fn style_key_affects_pseudo_state(key: &str) -> bool {
    key == "__hover_scope"
        || key == "hover"
        || key == "focus"
        || key == "active"
        || key == "disabled"
        || key == "selected"
        || key == "checked"
}

fn style_key_affects_source_binding(key: &str) -> bool {
    key == "source_intent" || key == "source_binding" || key == "__source_binding"
}

fn materialization_report(
    node: &DocumentNode,
    materialized: &MaterializedRange,
) -> MaterializationReport {
    let logical_item_count = materialized
        .overscan
        .end
        .max(materialized.visible.end)
        .max(materialized.overscan.start)
        .max(materialized.visible.start);
    let materialized_item_count = materialized
        .overscan
        .end
        .saturating_sub(materialized.overscan.start);
    let stable_key_prefix = stable_materialization_key_prefix(node, materialized.axis);
    MaterializationReport {
        node: node.id.clone(),
        axis: materialized.axis,
        visible: materialized.visible.clone(),
        overscan: materialized.overscan.clone(),
        logical_item_count,
        materialized_item_count,
        first_stable_key: (materialized_item_count > 0)
            .then(|| format!("{}:{}", stable_key_prefix, materialized.overscan.start)),
        last_stable_key: (materialized_item_count > 0).then(|| {
            format!(
                "{}:{}",
                stable_key_prefix,
                materialized.overscan.end.saturating_sub(1)
            )
        }),
        stable_key_prefix,
    }
}

fn stable_materialization_key_prefix(node: &DocumentNode, axis: Axis) -> String {
    let axis = match axis {
        Axis::Horizontal => "x",
        Axis::Vertical => "y",
    };
    format!("materialized:{}:{axis}", node.id.0)
}

fn demand_from_report(report: &MaterializationReport) -> LayoutDemand {
    LayoutDemand {
        node: report.node.clone(),
        axis: report.axis,
        visible: report.visible.clone(),
        overscan: report.overscan.clone(),
        logical_item_count: report.logical_item_count,
        materialized_item_count: report.materialized_item_count,
        stable_key_prefix: report.stable_key_prefix.clone(),
        first_stable_key: report.first_stable_key.clone(),
        last_stable_key: report.last_stable_key.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: DocumentNodeKind, parent: Option<&str>) -> DocumentNode {
        let mut node = DocumentNode::new(id, kind);
        node.parent = parent.map(|parent| DocumentNodeId(parent.to_owned()));
        node
    }

    #[test]
    fn document_patch_reports_text_and_layout_invalidation() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::SetText {
                id: DocumentNodeId("label".to_owned()),
                text: TextValue {
                    text: "Updated".to_owned(),
                },
            })
            .unwrap();

        assert_eq!(report.patch_kind, "set_text");
        assert_eq!(report.target, Some(DocumentNodeId("label".to_owned())));
        assert!(report.invalidation.contains(&PatchInvalidationClass::Text));
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Layout)
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::HitRegion)
        );
        assert_eq!(report.node_count_after, 2);
        assert_eq!(
            state.frame().nodes[&DocumentNodeId("label".to_owned())]
                .text
                .as_ref()
                .unwrap()
                .text,
            "Updated"
        );
    }

    #[test]
    fn style_identity_splits_layout_paint_material_font_and_pseudo_state() {
        let mut base = StyleMap::new();
        base.insert("width".to_owned(), StyleValue::Number(120.0));
        base.insert("color".to_owned(), StyleValue::Text("red".to_owned()));
        base.insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
        base.insert(
            "font_weight".to_owned(),
            StyleValue::Text("bold".to_owned()),
        );
        base.insert("__hover_scope".to_owned(), StyleValue::Bool(true));

        let identity = computed_style_identity(&base);
        let same_identity = computed_style_identity(&base);
        assert_eq!(identity, same_identity);

        let mut paint_change = base.clone();
        paint_change.insert("color".to_owned(), StyleValue::Text("blue".to_owned()));
        let paint_identity = computed_style_identity(&paint_change);
        assert_ne!(identity.style_id, paint_identity.style_id);
        assert_eq!(identity.layout_id, paint_identity.layout_id);
        assert_ne!(identity.paint_id, paint_identity.paint_id);
        assert_eq!(identity.material_id, paint_identity.material_id);
        assert_eq!(identity.font_id, paint_identity.font_id);
        assert_eq!(identity.pseudo_state_id, paint_identity.pseudo_state_id);

        let mut layout_change = base.clone();
        layout_change.insert("width".to_owned(), StyleValue::Number(180.0));
        let layout_identity = computed_style_identity(&layout_change);
        assert_ne!(identity.layout_id, layout_identity.layout_id);
        assert_eq!(identity.paint_id, layout_identity.paint_id);
    }

    #[test]
    fn typed_hit_side_table_carries_route_identity_and_bucket_index() {
        let mut frame = DocumentFrame::empty("root");
        let mut scroll = node("scroll", DocumentNodeKind::ScrollRoot, Some("root"));
        scroll
            .style
            .insert("height".to_owned(), StyleValue::Number(120.0));
        scroll.materialized.push(MaterializedRange {
            axis: Axis::Vertical,
            visible: 0..4,
            overscan: 0..8,
        });
        let mut button = node("row-button", DocumentNodeKind::Button, Some("scroll"));
        button
            .style
            .insert("width".to_owned(), StyleValue::Number(80.0));
        button
            .style
            .insert("height".to_owned(), StyleValue::Number(24.0));
        button
            .style
            .insert("row_key".to_owned(), StyleValue::Number(42.0));
        button.style.insert(
            "row_generation".to_owned(),
            StyleValue::Text("7".to_owned()),
        );
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: SourceBindingId("source:row-button:press".to_owned()),
            source_path: "rows.press".to_owned(),
            intent: "press".to_owned(),
        });
        frame
            .nodes
            .get_mut(&DocumentNodeId("root".to_owned()))
            .unwrap()
            .children
            .push(DocumentNodeId("scroll".to_owned()));
        scroll
            .children
            .push(DocumentNodeId("row-button".to_owned()));
        frame
            .nodes
            .insert(DocumentNodeId("scroll".to_owned()), scroll);
        frame
            .nodes
            .insert(DocumentNodeId("row-button".to_owned()), button);
        frame.scroll_roots.insert(
            ScrollRootId("scroll".to_owned()),
            boon_document_model::ScrollState { x: 0.0, y: 0.0 },
        );

        let mut measurer = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: boon_host::Viewport {
                surface: 1,
                width: 320.0,
                height: 240.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let table = HitSideTable::from_document_layout_with_bucket_size(&frame, &layout, 64.0);

        let entry = table
            .entry_for_source_path("rows.press")
            .expect("source path should have a typed hit entry");
        assert_eq!(entry.node, DocumentNodeId("row-button".to_owned()));
        assert_eq!(
            entry.source_binding_id,
            Some(SourceBindingId("source:row-button:press".to_owned()))
        );
        assert_eq!(entry.source_intent.as_deref(), Some("press"));
        assert_eq!(entry.scroll_root, Some(ScrollRootId("scroll".to_owned())));
        assert_eq!(entry.row_key, Some(42));
        assert_eq!(entry.row_generation, Some(7));
        assert_eq!(entry.z_depth, 0);
        assert!(
            table
                .bucket_indices(entry.spatial_bucket)
                .is_some_and(|bucket| !bucket.is_empty())
        );
        let hit = table
            .hit_test(entry.bounds.x + 1.0, entry.bounds.y + 1.0)
            .expect("typed hit side table should route by point");
        assert_eq!(hit.source_path.as_deref(), Some("rows.press"));
    }

    #[test]
    fn style_patch_reports_precise_invalidation_classes() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let mut patch = StylePatch::new();
        patch.insert("width".to_owned(), Some(StyleValue::Number(240.0)));
        patch.insert(
            "background_color".to_owned(),
            Some(StyleValue::Text("black".to_owned())),
        );
        patch.insert("__hover_scope".to_owned(), Some(StyleValue::Bool(true)));
        patch.insert(
            "source_intent".to_owned(),
            Some(StyleValue::Text("activate".to_owned())),
        );

        let report = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("label".to_owned()),
                patch,
            })
            .unwrap();

        for class in [
            PatchInvalidationClass::Style,
            PatchInvalidationClass::Layout,
            PatchInvalidationClass::LayoutOnly,
            PatchInvalidationClass::PaintOnly,
            PatchInvalidationClass::ConditionalStructure,
            PatchInvalidationClass::SourceBinding,
            PatchInvalidationClass::HitRegion,
        ] {
            assert!(
                report.invalidation.contains(&class),
                "missing invalidation class {class:?}"
            );
        }
    }

    #[test]
    fn style_patch_unknown_keys_fail_toward_full_document_invalidation() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();

        let mut patch = StylePatch::new();
        patch.insert(
            "future_renderer_knob".to_owned(),
            Some(StyleValue::Text("unknown".to_owned())),
        );

        let report = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("label".to_owned()),
                patch,
            })
            .unwrap();

        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::FullDocument),
            "unknown style keys must invalidate conservatively"
        );
    }

    #[test]
    fn document_patch_missing_targets_fail_closed() {
        let mut state = DocumentState::new("root");

        let text_error = state
            .apply_patch(DocumentPatch::SetText {
                id: DocumentNodeId("missing".to_owned()),
                text: TextValue {
                    text: "Lost".to_owned(),
                },
            })
            .unwrap_err();
        assert!(matches!(
            text_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_text",
                id
            } if id.0 == "missing"
        ));

        let style_error = state
            .apply_patch(DocumentPatch::SetStyle {
                id: DocumentNodeId("missing".to_owned()),
                patch: StylePatch::new(),
            })
            .unwrap_err();
        assert!(matches!(
            style_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_style",
                id
            } if id.0 == "missing"
        ));

        let materialized_error = state
            .apply_patch(DocumentPatch::SetListMaterialization {
                id: DocumentNodeId("missing".to_owned()),
                materialized: MaterializedRange {
                    axis: Axis::Vertical,
                    visible: 0..1,
                    overscan: 0..2,
                },
            })
            .unwrap_err();
        assert!(matches!(
            materialized_error,
            PatchApplyError::MissingTarget {
                patch_kind: "set_list_materialization",
                id
            } if id.0 == "missing"
        ));
    }

    #[test]
    fn materialization_patch_reports_logical_counts_and_stable_keys() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "virtual-list",
                DocumentNodeKind::ScrollRoot,
                Some("root"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::SetListMaterialization {
                id: DocumentNodeId("virtual-list".to_owned()),
                materialized: MaterializedRange {
                    axis: Axis::Vertical,
                    visible: 10..20,
                    overscan: 8..24,
                },
            })
            .unwrap();
        let materialization = report
            .materialization
            .expect("materialization patch should report protocol metadata");

        assert_eq!(materialization.node.0, "virtual-list");
        assert_eq!(materialization.visible, 10..20);
        assert_eq!(materialization.overscan, 8..24);
        assert_eq!(materialization.logical_item_count, 24);
        assert_eq!(materialization.materialized_item_count, 16);
        assert_eq!(
            materialization.stable_key_prefix,
            "materialized:virtual-list:y"
        );
        assert_eq!(
            materialization.first_stable_key.as_deref(),
            Some("materialized:virtual-list:y:8")
        );
        assert_eq!(
            materialization.last_stable_key.as_deref(),
            Some("materialized:virtual-list:y:23")
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Materialization)
        );
    }

    #[test]
    fn materialization_layout_demands_visible_overscan_and_stable_keys() {
        let frame = fixture_frame_with_virtualized_table();
        let mut measurer = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 640.0,
                height: 480.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: RenderCapabilities::fake_portable(),
        });

        assert_eq!(layout.demands.len(), 2);
        assert_eq!(layout.materialization.len(), 2);
        let vertical = layout
            .demands
            .iter()
            .find(|demand| demand.axis == Axis::Vertical)
            .expect("vertical materialization demand should exist");
        assert_eq!(vertical.visible, 0..20);
        assert_eq!(vertical.overscan, 0..28);
        assert_eq!(vertical.logical_item_count, 28);
        assert_eq!(vertical.materialized_item_count, 28);
        assert_eq!(vertical.stable_key_prefix, "materialized:virtual-table:y");
        assert_eq!(
            vertical.last_stable_key.as_deref(),
            Some("materialized:virtual-table:y:27")
        );
        assert_eq!(layout.metrics.materialized_range_count, 2);
    }

    #[test]
    fn document_upsert_rejects_orphaned_children_and_bad_parent_links() {
        let mut state = DocumentState::new("root");
        let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
        parent
            .children
            .push(DocumentNodeId("missing-child".to_owned()));
        let orphan_error = state
            .apply_patch(DocumentPatch::UpsertNode(parent))
            .unwrap_err();
        assert!(matches!(
            orphan_error,
            PatchApplyError::OrphanedChild { parent, child }
                if parent.0 == "parent" && child.0 == "missing-child"
        ));

        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "parent",
                DocumentNodeKind::Stack,
                Some("root"),
            )))
            .unwrap();
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "child",
                DocumentNodeKind::Text,
                Some("root"),
            )))
            .unwrap();
        let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
        parent.children.push(DocumentNodeId("child".to_owned()));
        let link_error = state
            .apply_patch(DocumentPatch::UpsertNode(parent))
            .unwrap_err();
        assert!(matches!(
            link_error,
            PatchApplyError::InvalidParentChildLink {
                parent,
                child,
                actual_parent: Some(actual_parent),
            } if parent.0 == "parent" && child.0 == "child" && actual_parent.0 == "root"
        ));
    }

    #[test]
    fn document_remove_node_removes_subtree_and_detaches_parent() {
        let mut state = DocumentState::new("root");
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "panel",
                DocumentNodeKind::Stack,
                Some("root"),
            )))
            .unwrap();
        state
            .apply_patch(DocumentPatch::UpsertNode(node(
                "label",
                DocumentNodeKind::Text,
                Some("panel"),
            )))
            .unwrap();

        let report = state
            .apply_patch(DocumentPatch::RemoveNode {
                id: DocumentNodeId("panel".to_owned()),
            })
            .unwrap();

        assert_eq!(report.patch_kind, "remove_node");
        assert_eq!(
            report.removed_nodes,
            vec![
                DocumentNodeId("panel".to_owned()),
                DocumentNodeId("label".to_owned())
            ]
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::Structure)
        );
        assert!(
            report
                .invalidation
                .contains(&PatchInvalidationClass::HitRegion)
        );
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("panel".to_owned()))
        );
        assert!(
            !state
                .frame()
                .nodes
                .contains_key(&DocumentNodeId("label".to_owned()))
        );
        assert!(
            state.frame().nodes[&DocumentNodeId("root".to_owned())]
                .children
                .is_empty()
        );
    }

    #[test]
    fn document_remove_root_is_explicit_error() {
        let mut state = DocumentState::new("root");
        let error = state
            .apply_patch(DocumentPatch::RemoveNode {
                id: DocumentNodeId("root".to_owned()),
            })
            .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::CannotRemoveRoot { id } if id.0 == "root"
        ));
    }

    #[test]
    fn layout_rejects_stale_focus_and_orphan_child_references() {
        let mut frame = DocumentFrame::empty("root");
        frame.focus = Some(DocumentNodeId("missing-focus".to_owned()));
        let mut text = SimpleTextMeasurer;
        let error = try_layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 100.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        })
        .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::StaleReference {
                reference_kind: "focus",
                id
            } if id.0 == "missing-focus"
        ));

        let mut frame = DocumentFrame::empty("root");
        frame
            .nodes
            .get_mut(&DocumentNodeId("root".to_owned()))
            .unwrap()
            .children
            .push(DocumentNodeId("missing-child".to_owned()));
        let mut text = SimpleTextMeasurer;
        let error = try_layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 100.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        })
        .unwrap_err();
        assert!(matches!(
            error,
            PatchApplyError::OrphanedChild { parent, child }
                if parent.0 == "root" && child.0 == "missing-child"
        ));
    }

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
    fn layout_subtree_matches_whole_frame_row_geometry() {
        let mut frame = DocumentFrame::empty("root");

        let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
        row.parent = Some(frame.root.clone());
        row.style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(80.0));
        row.style.insert("gap".to_owned(), StyleValue::Number(0.0));
        row.children.push(DocumentNodeId("panel".to_owned()));
        row.children.push(DocumentNodeId("sibling".to_owned()));

        let mut panel = DocumentNode::new("panel", DocumentNodeKind::Stack);
        panel.parent = Some(row.id.clone());
        panel
            .style
            .insert("width".to_owned(), StyleValue::Number(180.0));
        panel
            .style
            .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));
        panel
            .style
            .insert("padding".to_owned(), StyleValue::Number(10.0));
        panel
            .style
            .insert("gap".to_owned(), StyleValue::Number(4.0));
        panel.children.push(DocumentNodeId("header".to_owned()));

        let mut header = DocumentNode::new("header", DocumentNodeKind::Row);
        header.parent = Some(panel.id.clone());
        header
            .style
            .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
        header
            .style
            .insert("height".to_owned(), StyleValue::Number(20.0));

        let mut sibling = DocumentNode::new("sibling", DocumentNodeKind::Stack);
        sibling.parent = Some(row.id.clone());
        sibling
            .style
            .insert("width".to_owned(), StyleValue::Number(50.0));
        sibling
            .style
            .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(row.id.clone());
        frame.nodes.insert(row.id.clone(), row);
        frame.nodes.insert(panel.id.clone(), panel);
        frame.nodes.insert(header.id.clone(), header);
        frame.nodes.insert(sibling.id.clone(), sibling);

        let mut full_text = SimpleTextMeasurer;
        let full = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 100.0,
                scale: 1.0,
            },
            text: &mut full_text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let mut subtree_text = SimpleTextMeasurer;
        let subtree = layout_subtree(LayoutSubtreeInput {
            document: &frame,
            root: &DocumentNodeId("row".to_owned()),
            x: 0.0,
            y: 0.0,
            available_width: 300.0,
            available_height: 80.0,
            text: &mut subtree_text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        for id in ["row", "panel", "header", "sibling"] {
            let full_bounds = full
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap()
                .bounds;
            let subtree_bounds = subtree
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap()
                .bounds;
            assert_eq!(subtree_bounds, full_bounds, "bounds differ for {id}");
        }
        assert_eq!(subtree.metrics.node_count, 4);
        assert_eq!(subtree.metrics.display_item_count, 4);
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

    #[test]
    fn stack_overlay_children_share_parent_origin() {
        let mut frame = DocumentFrame::empty("root");

        let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
        stack.parent = Some(frame.root.clone());
        stack
            .style
            .insert("width".to_owned(), StyleValue::Number(300.0));
        stack
            .style
            .insert("height".to_owned(), StyleValue::Number(180.0));
        stack
            .style
            .insert("overlay_children".to_owned(), StyleValue::Bool(true));
        stack.children.push(DocumentNodeId("content".to_owned()));
        stack.children.push(DocumentNodeId("modal".to_owned()));

        let mut content = DocumentNode::new("content", DocumentNodeKind::Stack);
        content.parent = Some(stack.id.clone());
        content
            .style
            .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
        content
            .style
            .insert("height".to_owned(), StyleValue::Text("Fill".to_owned()));

        let mut modal = DocumentNode::new("modal", DocumentNodeKind::Stack);
        modal.parent = Some(stack.id.clone());
        modal
            .style
            .insert("width".to_owned(), StyleValue::Number(120.0));
        modal
            .style
            .insert("height".to_owned(), StyleValue::Number(60.0));
        modal
            .style
            .insert("center".to_owned(), StyleValue::Bool(true));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(stack.id.clone());
        frame.nodes.insert(stack.id.clone(), stack);
        frame.nodes.insert(content.id.clone(), content);
        frame.nodes.insert(modal.id.clone(), modal);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 180.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let content = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "content")
            .expect("content layer should be laid out");
        let modal = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "modal")
            .expect("modal layer should be laid out");

        assert_eq!(content.bounds.x, 0.0);
        assert_eq!(content.bounds.y, 0.0);
        assert_eq!(content.bounds.height, 180.0);
        assert_eq!(modal.bounds.x, 90.0);
        assert_eq!(modal.bounds.y, 0.0);
        assert_eq!(modal.bounds.height, 60.0);
    }

    #[test]
    fn materialized_scroll_node_marks_descendants_with_clip_rect() {
        let mut frame = DocumentFrame::empty("root");

        let mut scroll = DocumentNode::new("scroll", DocumentNodeKind::Stack);
        scroll.parent = Some(frame.root.clone());
        scroll
            .style
            .insert("width".to_owned(), StyleValue::Number(200.0));
        scroll
            .style
            .insert("height".to_owned(), StyleValue::Number(80.0));
        scroll.materialized.push(MaterializedRange {
            axis: Axis::Vertical,
            visible: 0..4,
            overscan: 0..8,
        });
        scroll.children.push(DocumentNodeId("row".to_owned()));

        let mut row = DocumentNode::new("row", DocumentNodeKind::Text);
        row.parent = Some(scroll.id.clone());
        row.text = Some(TextValue {
            text: "oversized row".to_owned(),
        });
        row.style
            .insert("width".to_owned(), StyleValue::Number(200.0));
        row.style
            .insert("height".to_owned(), StyleValue::Number(160.0));

        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(scroll.id.clone());
        frame.nodes.insert(scroll.id.clone(), scroll);
        frame.nodes.insert(row.id.clone(), row);

        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width: 300.0,
                height: 200.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });

        let row = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "row")
            .expect("scroll child should be laid out");

        assert_eq!(style_spacing(&row.style, "__clip_x"), Some(0.0));
        assert_eq!(style_spacing(&row.style, "__clip_y"), Some(0.0));
        assert_eq!(style_spacing(&row.style, "__clip_width"), Some(200.0));
        assert_eq!(style_spacing(&row.style, "__clip_height"), Some(80.0));
    }
}
