use crate::{
    ComputedStyleIdentity, DisplayItem, DocumentHotIdTable, DocumentNodeId, DocumentNodeKind,
    DocumentRetainedLayoutKeyTable, LayoutFrame, PatchApplyError, Rect, StyleEditorTypeHint,
    StyleMap, StyleRichTextSpan, StyleValue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub const DEFAULT_DOCUMENT_FONT_FAMILY: &str = "Nimbus Sans";
pub const DEFAULT_EDITOR_FONT_FAMILY: &str = "JetBrains Mono";
pub const DEFAULT_EDITOR_FONT_FEATURES: &str = "zero,calt";

pub trait RenderTextColumnMeasurer {
    fn column_edges(&mut self, text: &str, style: &StyleMap, line_height: f32) -> Vec<f32>;
}

pub fn text_column_at(
    item: &DisplayItem,
    x: f32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> usize {
    let text = item.text.as_deref().unwrap_or_default();
    let font_size = style_number(&item.style, "size").unwrap_or(14.0);
    let line_height = style_line_height(&item.style, font_size);
    let edges = columns.column_edges(text, &item.style, line_height);
    let local_x = x - text_left_for_width(item, edges.last().copied().unwrap_or_default());
    edges
        .windows(2)
        .position(|edge| local_x < (edge[0] + edge[1]) * 0.5)
        .unwrap_or_else(|| edges.len().saturating_sub(1))
}

pub fn text_position_at(
    item: &DisplayItem,
    x: f32,
    y: f32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> (usize, usize) {
    let text = item.text.as_deref().unwrap_or_default();
    let lines = text.split('\n').collect::<Vec<_>>();
    let font_size = style_number(&item.style, "size").unwrap_or(14.0);
    let line_height = style_line_height(&item.style, font_size).max(1.0);
    let text_bounds = text_content_bounds_for_item(item);
    let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
    let top = text_top_for_parts(
        text_bounds,
        line_height * lines.len().max(1) as f32,
        text_inset,
        text_vertical_align(&item.kind, &item.style),
    );
    let line =
        (((y - top) / line_height).floor().max(0.0) as usize).min(lines.len().saturating_sub(1));
    let line_text = lines.get(line).copied().unwrap_or_default();
    let edges = columns.column_edges(line_text, &item.style, line_height);
    let local_x = x - text_left_for_width(item, edges.last().copied().unwrap_or_default());
    let column = edges
        .windows(2)
        .position(|edge| local_x < (edge[0] + edge[1]) * 0.5)
        .unwrap_or_else(|| edges.len().saturating_sub(1));
    (line, column)
}

#[derive(Default)]
pub struct ApproximateTextColumnMeasurer;

impl RenderTextColumnMeasurer for ApproximateTextColumnMeasurer {
    fn column_edges(&mut self, text: &str, style: &StyleMap, _line_height: f32) -> Vec<f32> {
        let font_size = style_number(style, "size").unwrap_or(14.0).max(1.0);
        let advance = font_size * 0.62;
        (0..=text.chars().count())
            .map(|column| column as f32 * advance)
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderScene {
    pub viewport: Rect,
    pub items: Vec<RenderSceneItem>,
    pub visual_primitives: Vec<RenderVisualPrimitive>,
    pub quad_batches: Vec<RenderQuadBatch>,
    pub text_runs: Vec<RenderTextRun>,
    pub metrics: RenderSceneMetrics,
}

impl RenderScene {
    pub fn apply_patch(
        &mut self,
        patch: &RenderScenePatch,
    ) -> Result<RenderScenePatchReport, PatchApplyError> {
        apply_render_scene_patch(self, patch)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderScenePatchReport {
    pub patched_items: usize,
    pub patched_primitives: usize,
    pub patched_text_runs: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderScenePatch {
    pub operations: Vec<RenderScenePatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderScenePatchOperation {
    Paint {
        node: DocumentNodeId,
        paint: RenderScenePaintPatch,
        style_identity: ComputedStyleIdentity,
        retained_chunk_id: String,
    },
    TextContent {
        node: DocumentNodeId,
        text: String,
        retained_chunk_id: String,
    },
    ReplaceNodeEntries {
        nodes: Vec<DocumentNodeId>,
        items: Vec<RenderSceneItem>,
        visual_primitives: Vec<RenderVisualPrimitive>,
        text_runs: Vec<RenderTextRun>,
    },
    RetagNodeEntries {
        items: Vec<RenderSceneItem>,
    },
    TranslateNodeEntries {
        nodes: Vec<DocumentNodeId>,
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderScenePaintPatch {
    FillColor { color: [u8; 4] },
    TextColor { color: [u8; 4] },
}

pub fn apply_render_scene_patch(
    scene: &mut RenderScene,
    patch: &RenderScenePatch,
) -> Result<RenderScenePatchReport, PatchApplyError> {
    let mut report = RenderScenePatchReport::default();
    for operation in &patch.operations {
        match operation {
            RenderScenePatchOperation::Paint {
                node,
                paint,
                style_identity,
                retained_chunk_id,
            } => {
                let op_report = apply_render_scene_paint_patch(
                    scene,
                    node,
                    paint,
                    *style_identity,
                    retained_chunk_id,
                )?;
                report.patched_items = report.patched_items.saturating_add(op_report.patched_items);
                report.patched_primitives = report
                    .patched_primitives
                    .saturating_add(op_report.patched_primitives);
                report.patched_text_runs = report
                    .patched_text_runs
                    .saturating_add(op_report.patched_text_runs);
            }
            RenderScenePatchOperation::TextContent {
                node,
                text,
                retained_chunk_id,
            } => {
                let op_report =
                    apply_render_scene_text_content_patch(scene, node, text, retained_chunk_id)?;
                report.patched_items = report.patched_items.saturating_add(op_report.patched_items);
                report.patched_text_runs = report
                    .patched_text_runs
                    .saturating_add(op_report.patched_text_runs);
            }
            RenderScenePatchOperation::ReplaceNodeEntries {
                nodes,
                items,
                visual_primitives,
                text_runs,
            } => {
                let op_report = apply_render_scene_replace_node_entries_patch(
                    scene,
                    nodes,
                    items,
                    visual_primitives,
                    text_runs,
                )?;
                report.patched_items = report.patched_items.saturating_add(op_report.patched_items);
                report.patched_primitives = report
                    .patched_primitives
                    .saturating_add(op_report.patched_primitives);
                report.patched_text_runs = report
                    .patched_text_runs
                    .saturating_add(op_report.patched_text_runs);
            }
            RenderScenePatchOperation::RetagNodeEntries { items } => {
                let op_report = apply_render_scene_retag_node_entries_patch(scene, items)?;
                report.patched_items = report.patched_items.saturating_add(op_report.patched_items);
                report.patched_primitives = report
                    .patched_primitives
                    .saturating_add(op_report.patched_primitives);
                report.patched_text_runs = report
                    .patched_text_runs
                    .saturating_add(op_report.patched_text_runs);
            }
            RenderScenePatchOperation::TranslateNodeEntries {
                nodes,
                delta_x,
                delta_y,
            } => {
                let op_report = apply_render_scene_translate_node_entries_patch(
                    scene, nodes, *delta_x, *delta_y,
                );
                report.patched_items = report.patched_items.saturating_add(op_report.patched_items);
                report.patched_primitives = report
                    .patched_primitives
                    .saturating_add(op_report.patched_primitives);
                report.patched_text_runs = report
                    .patched_text_runs
                    .saturating_add(op_report.patched_text_runs);
            }
        }
    }
    Ok(report)
}

fn apply_render_scene_translate_node_entries_patch(
    scene: &mut RenderScene,
    nodes: &[DocumentNodeId],
    delta_x: f32,
    delta_y: f32,
) -> RenderScenePatchReport {
    let node_set = nodes.iter().cloned().collect::<BTreeSet<_>>();
    let mut report = RenderScenePatchReport::default();
    for item in &mut scene.items {
        if node_set.contains(&item.node) {
            translate_rect(&mut item.bounds, delta_x, delta_y);
            report.patched_items = report.patched_items.saturating_add(1);
        }
    }
    for primitive in &mut scene.visual_primitives {
        if node_set.contains(&primitive.node) {
            translate_rect(&mut primitive.bounds, delta_x, delta_y);
            for point in &mut primitive.control_points {
                point[0] += delta_x;
                point[1] += delta_y;
            }
            report.patched_primitives = report.patched_primitives.saturating_add(1);
        }
    }
    for text_run in &mut scene.text_runs {
        if node_set.contains(&text_run.owner_node) {
            translate_rect(&mut text_run.bounds, delta_x, delta_y);
            report.patched_text_runs = report.patched_text_runs.saturating_add(1);
        }
    }
    scene.quad_batches.clear();
    report
}

fn translate_rect(rect: &mut Rect, delta_x: f32, delta_y: f32) {
    rect.x += delta_x;
    rect.y += delta_y;
}

fn apply_render_scene_paint_patch(
    scene: &mut RenderScene,
    node: &DocumentNodeId,
    paint: &RenderScenePaintPatch,
    style_identity: ComputedStyleIdentity,
    retained_chunk_id: &str,
) -> Result<RenderScenePatchReport, PatchApplyError> {
    let mut report = RenderScenePatchReport::default();
    let mut saw_item = false;
    for item in &mut scene.items {
        if item.node == *node {
            item.style_identity = style_identity;
            item.retained_chunk_id = retained_chunk_id.to_owned();
            report.patched_items = report.patched_items.saturating_add(1);
            saw_item = true;
        }
    }
    if !saw_item {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "render_scene_item",
            id: node.clone(),
        });
    }
    match paint {
        RenderScenePaintPatch::FillColor { color } => {
            for primitive in &mut scene.visual_primitives {
                if primitive.node == *node && primitive.primitive == RenderVisualPrimitiveKind::Fill
                {
                    primitive.color = *color;
                    primitive.style_identity = style_identity;
                    primitive.retained_chunk_id = retained_chunk_id.to_owned();
                    report.patched_primitives = report.patched_primitives.saturating_add(1);
                }
            }
            if report.patched_primitives == 0 {
                return Err(PatchApplyError::StaleReference {
                    reference_kind: "render_scene_fill_primitive",
                    id: node.clone(),
                });
            }
        }
        RenderScenePaintPatch::TextColor { color } => {
            for text_run in &mut scene.text_runs {
                if text_run.owner_node == *node {
                    text_run.color = *color;
                    text_run.paint_id = style_identity.paint_id;
                    report.patched_text_runs = report.patched_text_runs.saturating_add(1);
                }
            }
            if report.patched_text_runs == 0 {
                return Err(PatchApplyError::StaleReference {
                    reference_kind: "render_scene_text_run",
                    id: node.clone(),
                });
            }
        }
    }
    scene.quad_batches.clear();
    Ok(report)
}

fn apply_render_scene_retag_node_entries_patch(
    scene: &mut RenderScene,
    items: &[RenderSceneItem],
) -> Result<RenderScenePatchReport, PatchApplyError> {
    let mut report = RenderScenePatchReport::default();
    let mut updates = BTreeMap::<DocumentNodeId, (String, ComputedStyleIdentity)>::new();
    for replacement in items {
        let Some(item) = scene
            .items
            .iter_mut()
            .find(|item| item.node == replacement.node)
        else {
            return Err(PatchApplyError::StaleReference {
                reference_kind: "render_scene_retag_item",
                id: replacement.node.clone(),
            });
        };
        *item = replacement.clone();
        updates.insert(
            replacement.node.clone(),
            (
                replacement.retained_chunk_id.clone(),
                replacement.style_identity,
            ),
        );
        report.patched_items = report.patched_items.saturating_add(1);
    }
    for primitive in &mut scene.visual_primitives {
        let Some((retained_chunk_id, style_identity)) = updates.get(&primitive.node) else {
            continue;
        };
        primitive.retained_chunk_id = retained_chunk_id.clone();
        primitive.style_identity = *style_identity;
        report.patched_primitives = report.patched_primitives.saturating_add(1);
    }
    for text_run in &mut scene.text_runs {
        let Some((_, style_identity)) = updates.get(&text_run.owner_node) else {
            continue;
        };
        text_run.paint_id = style_identity.paint_id;
        report.patched_text_runs = report.patched_text_runs.saturating_add(1);
    }
    scene.quad_batches.clear();
    Ok(report)
}

fn apply_render_scene_replace_node_entries_patch(
    scene: &mut RenderScene,
    nodes: &[DocumentNodeId],
    items: &[RenderSceneItem],
    visual_primitives: &[RenderVisualPrimitive],
    text_runs: &[RenderTextRun],
) -> Result<RenderScenePatchReport, PatchApplyError> {
    let node_set = nodes.iter().cloned().collect::<BTreeSet<_>>();
    let mut report = RenderScenePatchReport::default();
    if !replace_render_scene_entries_for_nodes(
        &mut scene.items,
        &node_set,
        items,
        |item| &item.node,
        |node, nodes| nodes.contains(node),
        true,
    )? {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "render_scene_replace_item",
            id: nodes
                .first()
                .cloned()
                .unwrap_or_else(|| DocumentNodeId(String::new())),
        });
    }
    report.patched_items = report.patched_items.saturating_add(items.len());

    replace_render_scene_entries_for_nodes(
        &mut scene.visual_primitives,
        &node_set,
        visual_primitives,
        |primitive| &primitive.node,
        |node, nodes| nodes.contains(node),
        false,
    )?;
    report.patched_primitives = report
        .patched_primitives
        .saturating_add(visual_primitives.len());

    replace_render_scene_entries_for_nodes(
        &mut scene.text_runs,
        &node_set,
        text_runs,
        |text_run| &text_run.owner_node,
        |owner, nodes| nodes.contains(owner),
        false,
    )?;
    report.patched_text_runs = report.patched_text_runs.saturating_add(text_runs.len());

    scene.quad_batches.clear();
    Ok(report)
}

fn replace_render_scene_entries_for_nodes<T: Clone>(
    entries: &mut Vec<T>,
    node_set: &BTreeSet<DocumentNodeId>,
    replacements: &[T],
    node_for_entry: impl Fn(&T) -> &DocumentNodeId,
    entry_belongs_to_nodes: impl Fn(&DocumentNodeId, &BTreeSet<DocumentNodeId>) -> bool,
    require_existing: bool,
) -> Result<bool, PatchApplyError> {
    let first = entries
        .iter()
        .position(|entry| entry_belongs_to_nodes(node_for_entry(entry), node_set));
    let mut saw_existing = first.is_some();
    if first.is_none() && require_existing && !replacements.is_empty() {
        return Ok(false);
    }
    let insert_at = first.unwrap_or(entries.len());
    let original = std::mem::take(entries);
    let mut inserted_nodes = BTreeSet::new();
    for entry in original {
        let remove = entry_belongs_to_nodes(node_for_entry(&entry), node_set);
        saw_existing |= remove;
        if remove {
            let node = node_for_entry(&entry).clone();
            if inserted_nodes.insert(node.clone()) {
                entries.extend(
                    replacements
                        .iter()
                        .filter(|replacement| node_for_entry(replacement) == &node)
                        .cloned(),
                );
            }
        } else {
            entries.push(entry);
        }
    }
    let remaining = replacements
        .iter()
        .filter(|replacement| !inserted_nodes.contains(node_for_entry(replacement)))
        .cloned()
        .collect::<Vec<_>>();
    if !remaining.is_empty() {
        entries.splice(insert_at..insert_at, remaining);
    }
    Ok(saw_existing || replacements.is_empty())
}

fn apply_render_scene_text_content_patch(
    scene: &mut RenderScene,
    node: &DocumentNodeId,
    text: &str,
    retained_chunk_id: &str,
) -> Result<RenderScenePatchReport, PatchApplyError> {
    let mut report = RenderScenePatchReport::default();
    let mut saw_item = false;
    for item in &mut scene.items {
        if item.node == *node {
            item.retained_chunk_id = retained_chunk_id.to_owned();
            report.patched_items = report.patched_items.saturating_add(1);
            saw_item = true;
        }
    }
    if !saw_item {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "render_scene_item",
            id: node.clone(),
        });
    }
    for text_run in &mut scene.text_runs {
        if text_run.owner_node == *node {
            text_run.text = text.to_owned();
            report.patched_text_runs = report.patched_text_runs.saturating_add(1);
        }
    }
    if report.patched_text_runs == 0 {
        return Err(PatchApplyError::StaleReference {
            reference_kind: "render_scene_text_run",
            id: node.clone(),
        });
    }
    scene.quad_batches.clear();
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderSceneItem {
    pub node: DocumentNodeId,
    #[serde(default)]
    pub retained_chunk_id: String,
    pub source_kind: DocumentNodeKind,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub transform: [f32; 6],
    pub style_identity: ComputedStyleIdentity,
    pub dependency_set: Vec<String>,
    pub texture_asset_refs: Vec<String>,
    pub estimated_vertex_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderSceneMetrics {
    pub visible_source_item_count: u32,
    pub visual_primitive_count: u32,
    pub rendered_rect_count: u32,
    pub cap_hit: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderVisualPrimitive {
    pub node: DocumentNodeId,
    #[serde(default)]
    pub retained_chunk_id: String,
    pub source_kind: DocumentNodeKind,
    pub primitive: RenderVisualPrimitiveKind,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub radius: f32,
    pub stroke_width: f32,
    pub color: [u8; 4],
    pub secondary_color: [u8; 4],
    pub antialias: f32,
    pub control_points: Vec<[f32; 2]>,
    pub texture: RenderTextureRef,
    pub style_identity: ComputedStyleIdentity,
    pub dependency_set: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderVisualPrimitiveKind {
    ViewportBackground,
    Shadow,
    FrostedMaterialLayer,
    Fill,
    MaterialHighlight,
    Asset,
    CheckboxCastShadow,
    Checkbox,
    CheckboxInnerShadow,
    CheckboxHighlight,
    CheckboxCheckmark,
    EditorSelection,
    EditorBracketHighlight,
    EditorCaret,
    TextInputSelection,
    TextInputCaret,
    Underline,
    Strikethrough,
    ButtonCheckmark,
    Border,
    BorderTop,
    BorderRight,
    BorderBottom,
    BorderLeft,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderQuadBatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_chunk_id: Option<String>,
    pub texture: RenderTextureRef,
    pub positions: Vec<f32>,
    pub colors: Vec<u32>,
    pub uvs: Vec<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderTextureRef {
    Solid,
    Asset {
        url: String,
        asset_ref: RenderAssetRef,
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RenderBlobRef {
    pub id: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RenderAssetRef {
    pub id: String,
    pub blob_ref: RenderBlobRef,
    pub width: u32,
    pub height: u32,
}

impl RenderAssetRef {
    pub fn inline_svg_data_url(url: &str, width: u32, height: u32) -> Self {
        let blob_sha256 = sha256_hex(url.as_bytes());
        let width = width.max(1);
        let height = height.max(1);
        let id_digest = sha256_hex(format!("{blob_sha256}\n{width}x{height}").as_bytes());
        Self {
            id: format!("asset:svg-data-url:{id_digest}:{width}x{height}"),
            blob_ref: RenderBlobRef {
                id: format!("blob:sha256:{blob_sha256}"),
                sha256: blob_sha256,
            },
            width,
            height,
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderTextRun {
    pub node: DocumentNodeId,
    pub owner_node: DocumentNodeId,
    pub font_id: u64,
    pub paint_id: u64,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub text: String,
    pub rich_spans: Vec<RenderRichTextSpan>,
    pub font_family: String,
    pub font_style: RenderFontStyle,
    pub font_weight: RenderFontWeight,
    pub font_features: String,
    pub text_inset: f32,
    pub text_clip_padding: f32,
    pub color: [u8; 4],
    pub size: f32,
    pub line_height: f32,
    pub align: RenderTextAlign,
    pub vertical_align: RenderTextVerticalAlign,
    pub rotate_degrees: u32,
    #[serde(default)]
    pub wrap: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RenderTextShapeKey {
    pub font_id: u64,
    pub paint_id: u64,
    pub text: String,
    pub rich_spans: Vec<RenderRichTextShapeSpanKey>,
    pub font_family: String,
    pub font_style: RenderFontStyle,
    pub font_weight: RenderFontWeight,
    pub font_features: String,
    pub text_inset: u32,
    pub text_clip_padding: u32,
    pub line_height: u32,
    pub width: u32,
    pub height: u32,
    pub size: u32,
    pub color: [u8; 4],
    pub align: RenderTextAlign,
    pub vertical_align: RenderTextVerticalAlign,
    pub rotate_degrees: u32,
    #[serde(default)]
    pub wrap: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RenderTextPlacementKey {
    pub shape: RenderTextShapeKey,
    pub x: u32,
    pub y: u32,
    pub clip_x: Option<u32>,
    pub clip_y: Option<u32>,
    pub clip_width: Option<u32>,
    pub clip_height: Option<u32>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RenderRichTextShapeSpanKey {
    pub text: String,
    pub color: [u8; 4],
    pub font_style: RenderFontStyle,
    pub font_weight: RenderFontWeight,
}

impl RenderTextRun {
    pub fn shape_key(&self) -> RenderTextShapeKey {
        RenderTextShapeKey {
            font_id: self.font_id,
            paint_id: self.paint_id,
            text: self.text.clone(),
            rich_spans: self
                .rich_spans
                .iter()
                .map(RenderRichTextShapeSpanKey::from)
                .collect(),
            font_family: self.font_family.clone(),
            font_style: self.font_style,
            font_weight: self.font_weight,
            font_features: self.font_features.clone(),
            text_inset: self.text_inset.to_bits(),
            text_clip_padding: self.text_clip_padding.to_bits(),
            line_height: self.line_height.to_bits(),
            width: self.bounds.width.to_bits(),
            height: self.bounds.height.to_bits(),
            size: self.size.to_bits(),
            color: self.color,
            align: self.align,
            vertical_align: self.vertical_align,
            rotate_degrees: self.rotate_degrees,
            wrap: self.wrap,
        }
    }

    pub fn placement_key(&self) -> RenderTextPlacementKey {
        RenderTextPlacementKey {
            shape: self.shape_key(),
            x: self.bounds.x.to_bits(),
            y: self.bounds.y.to_bits(),
            clip_x: self.clip.map(|clip| clip.x.to_bits()),
            clip_y: self.clip.map(|clip| clip.y.to_bits()),
            clip_width: self.clip.map(|clip| clip.width.to_bits()),
            clip_height: self.clip.map(|clip| clip.height.to_bits()),
        }
    }
}

impl From<&RenderRichTextSpan> for RenderRichTextShapeSpanKey {
    fn from(span: &RenderRichTextSpan) -> Self {
        Self {
            text: span.text.clone(),
            color: span.color,
            font_style: span.font_style,
            font_weight: span.font_weight,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderRichTextSpan {
    pub text: String,
    pub color: [u8; 4],
    pub font_style: RenderFontStyle,
    pub font_weight: RenderFontWeight,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderFontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RenderFontWeight(pub u16);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderTextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderTextVerticalAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetainedRenderChunkDescriptor {
    pub id: String,
    pub node: DocumentNodeId,
    pub source_kind: DocumentNodeKind,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub transform: [f32; 6],
    pub style_identity: ComputedStyleIdentity,
    pub dependency_set: Vec<String>,
    pub gpu_buffer_range: Range<u32>,
    pub text_run_ids: Vec<String>,
    pub texture_asset_refs: Vec<String>,
}

pub fn lower_layout_frame_to_render_scene(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> RenderScene {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let items = render_scene_items(frame, width, height);
    let visual_primitives = render_visual_primitives(frame, width, height, columns);
    let text_runs = render_text_runs(frame, width, height, columns);
    RenderScene {
        viewport,
        metrics: RenderSceneMetrics {
            visible_source_item_count: items.len() as u32,
            visual_primitive_count: visual_primitives.len() as u32,
            rendered_rect_count: visual_primitives.len() as u32,
            cap_hit: false,
        },
        items,
        visual_primitives,
        quad_batches: Vec::new(),
        text_runs,
    }
}

pub fn lower_layout_frame_to_render_scene_with_retained_keys(
    frame: &LayoutFrame,
    hot_ids: &DocumentHotIdTable,
    retained_layout_keys: &DocumentRetainedLayoutKeyTable,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> Result<RenderScene, PatchApplyError> {
    let mut scene = lower_layout_frame_to_render_scene(frame, width, height, columns);
    let mut retained_chunk_ids_by_node = BTreeMap::new();
    for item in &mut scene.items {
        let retained_chunk_id =
            checked_retained_chunk_id_for_item(item, hot_ids, retained_layout_keys)?;
        item.retained_chunk_id = retained_chunk_id.clone();
        retained_chunk_ids_by_node.insert(item.node.clone(), retained_chunk_id);
    }
    for primitive in &mut scene.visual_primitives {
        if primitive.node.0 == "__viewport__" || render_scene_synthetic_node(&primitive.node) {
            continue;
        }
        let retained_chunk_id =
            retained_chunk_ids_by_node
                .get(&primitive.node)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "render_scene_primitive_retained_chunk",
                    id: primitive.node.clone(),
                })?;
        primitive.retained_chunk_id = retained_chunk_id.clone();
    }
    Ok(scene)
}

pub fn render_scene_items(frame: &LayoutFrame, width: u32, height: u32) -> Vec<RenderSceneItem> {
    render_scene_items_for_nodes(frame, width, height, None)
}

pub fn render_scene_items_for_touched_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    nodes: &BTreeSet<DocumentNodeId>,
) -> Vec<RenderSceneItem> {
    render_scene_items_for_nodes(frame, width, height, Some(nodes))
}

pub fn render_scene_entries_for_touched_nodes_with_retained_keys(
    frame: &LayoutFrame,
    hot_ids: &DocumentHotIdTable,
    retained_layout_keys: &DocumentRetainedLayoutKeyTable,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
    nodes: &BTreeSet<DocumentNodeId>,
) -> Result<
    (
        Vec<RenderSceneItem>,
        Vec<RenderVisualPrimitive>,
        Vec<RenderTextRun>,
    ),
    PatchApplyError,
> {
    let mut items = render_scene_items_for_touched_nodes(frame, width, height, nodes);
    let mut retained_chunk_ids_by_node = BTreeMap::new();
    for item in &mut items {
        let retained_chunk_id =
            checked_retained_chunk_id_for_item(item, hot_ids, retained_layout_keys)?;
        item.retained_chunk_id = retained_chunk_id.clone();
        retained_chunk_ids_by_node.insert(item.node.clone(), retained_chunk_id);
    }
    let mut visual_primitives =
        render_visual_primitives_for_touched_nodes(frame, width, height, columns, nodes);
    for primitive in &mut visual_primitives {
        if primitive.node.0 == "__viewport__" || render_scene_synthetic_node(&primitive.node) {
            continue;
        }
        let retained_chunk_id =
            retained_chunk_ids_by_node
                .get(&primitive.node)
                .ok_or_else(|| PatchApplyError::StaleReference {
                    reference_kind: "render_scene_primitive_retained_chunk",
                    id: primitive.node.clone(),
                })?;
        primitive.retained_chunk_id = retained_chunk_id.clone();
    }
    let text_runs = render_text_runs_for_touched_nodes(frame, width, height, columns, nodes);
    Ok((items, visual_primitives, text_runs))
}

fn render_scene_items_for_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    nodes: Option<&BTreeSet<DocumentNodeId>>,
) -> Vec<RenderSceneItem> {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    frame
        .display_list
        .iter()
        .filter(|item| rect_intersects(item.bounds, viewport))
        .filter(|item| nodes.is_none_or(|nodes| nodes.contains(&item.node)))
        .map(render_scene_item)
        .collect()
}

fn render_scene_item(item: &DisplayItem) -> RenderSceneItem {
    RenderSceneItem {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        bounds: item.bounds,
        clip: clip_rect_for_style(&item.style),
        transform: [1.0, 0.0, 0.0, 1.0, item.bounds.x, item.bounds.y],
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(item, "source-item"),
        texture_asset_refs: style_asset_url(&item.style)
            .map(|asset| {
                vec![
                    RenderAssetRef::inline_svg_data_url(
                        asset,
                        item.bounds.width.ceil().clamp(1.0, 2048.0) as u32,
                        item.bounds.height.ceil().clamp(1.0, 2048.0) as u32,
                    )
                    .id,
                ]
            })
            .unwrap_or_default(),
        estimated_vertex_count: retained_chunk_vertex_estimate_for_bounds(item.bounds),
    }
}

fn retained_chunk_id_for_item(item: &DisplayItem) -> String {
    format!(
        "chunk:{}:{:?}:{:x}:{:x}:{:x}:{:x}:{:x}",
        item.node.0,
        item.kind,
        item.style_identity.style_id,
        item.style_identity.layout_id,
        item.style_identity.paint_id,
        item.style_identity.material_id,
        item.style_identity.pseudo_state_id
    )
}

fn checked_retained_chunk_id_for_item(
    item: &RenderSceneItem,
    hot_ids: &DocumentHotIdTable,
    retained_layout_keys: &DocumentRetainedLayoutKeyTable,
) -> Result<String, PatchApplyError> {
    if render_scene_synthetic_node(&item.node) {
        return Ok(item.retained_chunk_id.clone());
    }
    let hot_ref = hot_ids
        .hot_ref(&item.node)
        .ok_or_else(|| PatchApplyError::StaleReference {
            reference_kind: "render_scene_hot_id_table",
            id: item.node.clone(),
        })?;
    let retained_entry =
        retained_layout_keys
            .entry(hot_ref.id)
            .ok_or_else(|| PatchApplyError::StaleReference {
                reference_kind: "render_scene_retained_layout_key_table",
                id: item.node.clone(),
            })?;
    Ok(format!(
        "chunk:hot:{}:gen:{}:kind:{:?}:layout:{}:text_style:{}:text:{}:bounds:{:08x}:{:08x}:{:08x}:{:08x}:style:{:x}:paint:{:x}:material:{:x}:font:{:x}:pseudo:{:x}",
        hot_ref.id.0,
        retained_entry.node.generation.0,
        retained_entry.key.kind,
        retained_entry.key.layout_style.0,
        retained_entry.key.text_style.0,
        retained_entry.key.text.map(|id| id.0).unwrap_or(u32::MAX),
        item.bounds.x.to_bits(),
        item.bounds.y.to_bits(),
        item.bounds.width.to_bits(),
        item.bounds.height.to_bits(),
        item.style_identity.style_id,
        item.style_identity.paint_id,
        item.style_identity.material_id,
        item.style_identity.font_id,
        item.style_identity.pseudo_state_id
    ))
}

fn render_scene_synthetic_node(node: &DocumentNodeId) -> bool {
    node.0.starts_with("__")
        || node.0.starts_with("preview-")
        || node.0.starts_with("headed-scenario-")
}

pub fn render_visual_primitives(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> Vec<RenderVisualPrimitive> {
    render_visual_primitives_for_nodes(frame, width, height, columns, None)
}

pub fn render_visual_primitives_for_touched_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
    nodes: &BTreeSet<DocumentNodeId>,
) -> Vec<RenderVisualPrimitive> {
    render_visual_primitives_for_nodes(frame, width, height, columns, Some(nodes))
}

fn render_visual_primitives_for_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
    nodes: Option<&BTreeSet<DocumentNodeId>>,
) -> Vec<RenderVisualPrimitive> {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let mut primitives = Vec::new();
    if nodes.is_none() {
        primitives.push(RenderVisualPrimitive {
            node: DocumentNodeId("__viewport__".to_owned()),
            retained_chunk_id: "chunk:__viewport__:Root:viewport".to_owned(),
            source_kind: DocumentNodeKind::Root,
            primitive: RenderVisualPrimitiveKind::ViewportBackground,
            bounds: viewport,
            clip: None,
            radius: 0.0,
            stroke_width: 0.0,
            color: [246, 248, 251, 255],
            secondary_color: [0, 0, 0, 0],
            antialias: 0.0,
            control_points: Vec::new(),
            texture: RenderTextureRef::Solid,
            style_identity: ComputedStyleIdentity::from_style(&StyleMap::new()),
            dependency_set: vec!["viewport-background".to_owned()],
        });
    }
    let mut border_primitives = Vec::new();
    for (index, item) in frame
        .display_list
        .iter()
        .filter(|item| rect_intersects(item.bounds, viewport))
        .enumerate()
    {
        if nodes.is_some_and(|nodes| !nodes.contains(&item.node)) {
            continue;
        }
        let Some(item_bounds) = clipped_item_bounds(item) else {
            continue;
        };
        if style_bool(&item.style, "paint") == Some(false)
            || (style_bool(&item.style, "__hover_visible") == Some(true)
                && style_bool(&item.style, "__hover_paint") != Some(true))
        {
            continue;
        }
        let radius = style_number(&item.style, "border_radius").unwrap_or(0.0);
        primitives.extend(shadow_primitives_for_item(item, item_bounds, radius));
        primitives.extend(frosted_material_layer_primitives_for_item(
            item,
            item_bounds,
            radius,
        ));
        let fill = style_color_u8(&item.style, "bg")
            .or_else(|| style_color_u8(&item.style, "background"))
            .unwrap_or_else(|| default_fill_for_kind(&item.kind, index));
        primitives.push(RenderVisualPrimitive {
            node: item.node.clone(),
            retained_chunk_id: retained_chunk_id_for_item(item),
            source_kind: item.kind.clone(),
            primitive: RenderVisualPrimitiveKind::Fill,
            bounds: item_bounds,
            clip: clip_rect_for_style(&item.style),
            radius,
            stroke_width: 0.0,
            color: material_adjusted_fill_u8(fill, &item.style),
            secondary_color: [0, 0, 0, 0],
            antialias: 0.0,
            control_points: Vec::new(),
            texture: RenderTextureRef::Solid,
            style_identity: item.style_identity,
            dependency_set: visual_primitive_dependencies(item, "fill"),
        });
        if interactive_kind(&item.kind)
            && style_bool_raw(&item.style, "__hover") == Some(true)
            && !has_pseudo_override(&item.style, "hover", &["bg", "background"])
        {
            primitives.push(RenderVisualPrimitive {
                node: item.node.clone(),
                retained_chunk_id: retained_chunk_id_for_item(item),
                source_kind: item.kind.clone(),
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: item_bounds,
                clip: clip_rect_for_style(&item.style),
                radius,
                stroke_width: 0.0,
                color: [36, 112, 220, 24],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity: item.style_identity,
                dependency_set: visual_primitive_dependencies(item, "default-hover"),
            });
        }
        primitives.extend(material_highlight_primitives_for_item(
            item,
            item_bounds,
            radius,
        ));
        if let Some(asset_url) = style_asset_url(&item.style) {
            let asset_width = item_bounds.width.ceil().clamp(1.0, 2048.0) as u32;
            let asset_height = item_bounds.height.ceil().clamp(1.0, 2048.0) as u32;
            primitives.push(RenderVisualPrimitive {
                node: item.node.clone(),
                retained_chunk_id: retained_chunk_id_for_item(item),
                source_kind: item.kind.clone(),
                primitive: RenderVisualPrimitiveKind::Asset,
                bounds: item_bounds,
                clip: clip_rect_for_style(&item.style),
                radius,
                stroke_width: 0.0,
                color: [255, 255, 255, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Asset {
                    url: asset_url.to_owned(),
                    asset_ref: RenderAssetRef::inline_svg_data_url(
                        asset_url,
                        asset_width,
                        asset_height,
                    ),
                    width: asset_width,
                    height: asset_height,
                },
                style_identity: item.style_identity,
                dependency_set: visual_primitive_dependencies(item, "asset"),
            });
        }
        if matches!(item.kind, DocumentNodeKind::Checkbox) && !checkbox_has_asset_icon(frame, item)
        {
            primitives.extend(checkbox_primitives_for_item(item));
        }
        primitives.extend(text_overlay_primitives(item, columns));
        border_primitives.extend(border_primitives_for_item(item, item_bounds, radius));
        if interactive_kind(&item.kind)
            && item.focused
            && !has_pseudo_override(&item.style, "focus", &["border", "outline"])
        {
            border_primitives.push(border_primitive(
                item,
                RenderVisualPrimitiveKind::Border,
                item_bounds,
                radius,
                2.0,
                [44, 107, 216, 255],
            ));
        }
    }
    primitives.extend(border_primitives);
    primitives
}

fn interactive_kind(kind: &DocumentNodeKind) -> bool {
    matches!(
        kind,
        DocumentNodeKind::Button | DocumentNodeKind::Checkbox | DocumentNodeKind::TextInput
    )
}

fn has_pseudo_override(style: &StyleMap, pseudo: &str, keys: &[&str]) -> bool {
    keys.iter()
        .any(|key| style.contains_key(&format!("__{pseudo}_{key}")))
}

fn border_primitives_for_item(
    item: &DisplayItem,
    bounds: Rect,
    radius: f32,
) -> Vec<RenderVisualPrimitive> {
    let mut primitives = Vec::new();
    if let Some(color) = style_color_u8(&item.style, "border") {
        primitives.push(border_primitive(
            item,
            RenderVisualPrimitiveKind::Border,
            bounds,
            radius,
            style_number(&item.style, "border_width").unwrap_or(2.0),
            color,
        ));
    }
    for (kind, side) in [
        (RenderVisualPrimitiveKind::BorderTop, "top"),
        (RenderVisualPrimitiveKind::BorderRight, "right"),
        (RenderVisualPrimitiveKind::BorderBottom, "bottom"),
        (RenderVisualPrimitiveKind::BorderLeft, "left"),
    ] {
        let Some(color) = style_color_u8(&item.style, &format!("border_{side}")) else {
            continue;
        };
        primitives.push(border_primitive(
            item,
            kind,
            bounds,
            radius,
            style_number(&item.style, &format!("border_{side}_width"))
                .or_else(|| style_number(&item.style, "border_width"))
                .unwrap_or(1.0),
            color,
        ));
    }
    primitives
}

fn shadow_primitives_for_item(
    item: &DisplayItem,
    bounds: Rect,
    radius: f32,
) -> Vec<RenderVisualPrimitive> {
    let radius = radius.clamp(0.0, bounds.width.min(bounds.height) * 0.5);
    let mut primitives = Vec::new();
    for index in (1..=8).rev() {
        let Some(color) = style_color_u8(&item.style, &format!("box_shadow_{index}_color")) else {
            continue;
        };
        let color = color.map(|channel| channel as f32 / 255.0);
        let x = style_number(&item.style, &format!("box_shadow_{index}_x")).unwrap_or(0.0);
        let y = style_number(&item.style, &format!("box_shadow_{index}_y")).unwrap_or(0.0);
        let blur = style_number(&item.style, &format!("box_shadow_{index}_blur")).unwrap_or(0.0);
        let spread =
            style_number(&item.style, &format!("box_shadow_{index}_spread")).unwrap_or(0.0);
        let inset = style_bool(&item.style, &format!("box_shadow_{index}_inset")) == Some(true);
        let dependency = format!("box-shadow-{index}");
        if inset {
            let thickness = blur.max(1.0);
            primitives.push(shadow_primitive(
                item,
                Rect {
                    x: bounds.x,
                    y: bounds.y + bounds.height - thickness + y,
                    width: bounds.width,
                    height: thickness,
                },
                radius,
                rgba8_from_f32(color),
                &dependency,
            ));
            continue;
        }
        let base = Rect {
            x: bounds.x + x - spread,
            y: bounds.y + y - spread,
            width: (bounds.width + spread * 2.0).max(1.0),
            height: (bounds.height + spread * 2.0).max(1.0),
        };
        if radius > 0.25 {
            let base_radius =
                (radius + spread.max(0.0)).clamp(0.0, base.width.min(base.height) * 0.5);
            if blur <= 0.0 {
                primitives.push(shadow_primitive(
                    item,
                    base,
                    base_radius,
                    rgba8_from_f32(color),
                    &dependency,
                ));
                continue;
            }
            primitives.push(shadow_primitive(
                item,
                base,
                base_radius,
                rgba8_from_f32(color_with_alpha_scale(color, 0.42)),
                &dependency,
            ));
            let steps = blur.ceil().clamp(2.0, 18.0) as u32;
            for step in (0..steps).rev() {
                let outer_expand = blur * (step + 1) as f32 / steps as f32;
                let t = (step + 1) as f32 / steps as f32;
                let alpha_scale = (1.0 - t).powi(2) * 0.36;
                if alpha_scale < 0.01 {
                    continue;
                }
                let layer = expanded_rect(base, outer_expand);
                let layer_radius =
                    (base_radius + outer_expand).clamp(0.0, layer.width.min(layer.height) * 0.5);
                primitives.push(shadow_primitive(
                    item,
                    layer,
                    layer_radius,
                    rgba8_from_f32(color_with_alpha_scale(color, alpha_scale)),
                    &dependency,
                ));
            }
            continue;
        }
        if blur <= 0.0 {
            push_shadow_rect_difference_primitives(
                &mut primitives,
                item,
                base,
                bounds,
                color,
                &dependency,
            );
            continue;
        }
        push_shadow_rect_difference_primitives(
            &mut primitives,
            item,
            base,
            bounds,
            color_with_alpha_scale(color, 0.78),
            &dependency,
        );
        let steps = blur.ceil().clamp(2.0, 18.0) as u32;
        for step in 0..steps {
            let inner_expand = blur * step as f32 / steps as f32;
            let outer_expand = blur * (step + 1) as f32 / steps as f32;
            let t = (step + 1) as f32 / steps as f32;
            let alpha_scale = (1.0 - t).powi(2);
            if alpha_scale < 0.01 {
                continue;
            }
            push_shadow_halo_primitives(
                &mut primitives,
                item,
                base,
                bounds,
                inner_expand,
                outer_expand,
                color_with_alpha_scale(color, alpha_scale),
                &dependency,
            );
        }
    }
    primitives
}

#[allow(clippy::too_many_arguments)]
fn push_shadow_halo_primitives(
    primitives: &mut Vec<RenderVisualPrimitive>,
    item: &DisplayItem,
    rect: Rect,
    occluder: Rect,
    inner_expand: f32,
    outer_expand: f32,
    color: [f32; 4],
    dependency: &str,
) {
    let inner = expanded_rect(rect, inner_expand);
    let outer = expanded_rect(rect, outer_expand);
    let top_height = (inner.y - outer.y).max(0.0);
    let bottom_y = inner.y + inner.height;
    let bottom_height = (outer.y + outer.height - bottom_y).max(0.0);
    let left_width = (inner.x - outer.x).max(0.0);
    let right_x = inner.x + inner.width;
    let right_width = (outer.x + outer.width - right_x).max(0.0);
    for band in [
        Rect {
            x: outer.x,
            y: outer.y,
            width: outer.width,
            height: top_height,
        },
        Rect {
            x: outer.x,
            y: bottom_y,
            width: outer.width,
            height: bottom_height,
        },
        Rect {
            x: outer.x,
            y: inner.y,
            width: left_width,
            height: inner.height,
        },
        Rect {
            x: right_x,
            y: inner.y,
            width: right_width,
            height: inner.height,
        },
    ] {
        if band.width > 0.0 && band.height > 0.0 {
            push_shadow_rect_difference_primitives(
                primitives, item, band, occluder, color, dependency,
            );
        }
    }
}

fn push_shadow_rect_difference_primitives(
    primitives: &mut Vec<RenderVisualPrimitive>,
    item: &DisplayItem,
    rect: Rect,
    cutout: Rect,
    color: [f32; 4],
    dependency: &str,
) {
    let Some(overlap) = rect_intersection(rect, cutout) else {
        primitives.push(shadow_primitive(
            item,
            rect,
            0.0,
            rgba8_from_f32(color),
            dependency,
        ));
        return;
    };
    let top_height = (overlap.y - rect.y).max(0.0);
    let bottom_y = overlap.y + overlap.height;
    let bottom_height = (rect.y + rect.height - bottom_y).max(0.0);
    let left_width = (overlap.x - rect.x).max(0.0);
    let right_x = overlap.x + overlap.width;
    let right_width = (rect.x + rect.width - right_x).max(0.0);
    for band in [
        Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: top_height,
        },
        Rect {
            x: rect.x,
            y: bottom_y,
            width: rect.width,
            height: bottom_height,
        },
        Rect {
            x: rect.x,
            y: overlap.y,
            width: left_width,
            height: overlap.height,
        },
        Rect {
            x: right_x,
            y: overlap.y,
            width: right_width,
            height: overlap.height,
        },
    ] {
        if band.width > 0.0 && band.height > 0.0 {
            primitives.push(shadow_primitive(
                item,
                band,
                0.0,
                rgba8_from_f32(color),
                dependency,
            ));
        }
    }
}

fn shadow_primitive(
    item: &DisplayItem,
    bounds: Rect,
    radius: f32,
    color: [u8; 4],
    dependency: &str,
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        primitive: RenderVisualPrimitiveKind::Shadow,
        bounds,
        clip: clip_rect_for_style(&item.style),
        radius,
        stroke_width: 0.0,
        color,
        secondary_color: [0, 0, 0, 0],
        antialias: 0.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(item, dependency),
    }
}

fn checkbox_primitives_for_item(item: &DisplayItem) -> Vec<RenderVisualPrimitive> {
    let checked = style_bool(&item.style, "checked") == Some(true);
    let rect = item.bounds;
    let radius = (rect.width.min(rect.height) * 0.5
        - style_number(&item.style, "checkbox_inset").unwrap_or(2.0)
        - 0.5)
        .max(1.0);
    let border_width = style_number(&item.style, "checkbox_border_width").unwrap_or(1.5);
    let ring_color = if checked {
        style_color_u8(&item.style, "checked_border").unwrap_or([26, 91, 74, 255])
    } else {
        style_color_u8(&item.style, "checkbox_border").unwrap_or([212, 212, 212, 255])
    };
    let inner_color =
        style_color_u8(&item.style, "checkbox_background").unwrap_or([255, 255, 255, 255]);
    let aa = style_number(&item.style, "checkbox_aa")
        .unwrap_or(1.25)
        .clamp(0.0, 2.0);
    let mut primitives = Vec::new();
    if let Some(shadow_color) = style_color_u8(&item.style, "checkbox_cast_color")
        && shadow_color[3] > 0
    {
        let shadow_x = style_number(&item.style, "checkbox_cast_x").unwrap_or(0.0);
        let shadow_y = style_number(&item.style, "checkbox_cast_y").unwrap_or(0.0);
        let shadow_blur = style_number(&item.style, "checkbox_cast_blur")
            .unwrap_or(2.0)
            .clamp(0.0, 8.0);
        let shadow_spread = style_number(&item.style, "checkbox_cast_spread")
            .unwrap_or(0.0)
            .clamp(-2.0, 4.0);
        primitives.push(checkbox_primitive(
            item,
            RenderVisualPrimitiveKind::CheckboxCastShadow,
            Rect {
                x: rect.x + shadow_x,
                y: rect.y + shadow_y,
                ..rect
            },
            (radius + shadow_spread + shadow_blur * 0.3).max(1.0),
            0.0,
            [0, 0, 0, 0],
            shadow_color,
            (aa + shadow_blur).clamp(0.0, 8.0),
            "checkbox-cast-shadow",
        ));
    }
    primitives.push(checkbox_primitive(
        item,
        RenderVisualPrimitiveKind::Checkbox,
        rect,
        radius,
        border_width,
        ring_color,
        inner_color,
        aa,
        "checkbox-circle",
    ));
    if let Some(inner_shadow) = style_color_u8(&item.style, "checkbox_inner_shadow")
        && inner_shadow[3] > 0
    {
        primitives.push(checkbox_primitive(
            item,
            RenderVisualPrimitiveKind::CheckboxInnerShadow,
            rect,
            (radius - border_width * 0.5).max(1.0),
            style_number(&item.style, "checkbox_inner_shadow_width")
                .unwrap_or(1.0)
                .max(0.25),
            inner_shadow,
            [0, 0, 0, 0],
            aa,
            "checkbox-inner-shadow",
        ));
    }
    if let Some(highlight) = style_color_u8(&item.style, "checkbox_highlight")
        && highlight[3] > 0
    {
        primitives.push(checkbox_primitive(
            item,
            RenderVisualPrimitiveKind::CheckboxHighlight,
            Rect {
                x: rect.x - 0.5,
                y: rect.y - 0.5,
                ..rect
            },
            (radius - border_width * 0.35).max(1.0),
            style_number(&item.style, "checkbox_highlight_width")
                .unwrap_or(1.0)
                .max(0.0),
            highlight,
            [0, 0, 0, 0],
            aa,
            "checkbox-highlight",
        ));
    }
    if checked {
        let (start, middle, end) = checkbox_check_points(rect);
        let mut checkmark = checkbox_primitive(
            item,
            RenderVisualPrimitiveKind::CheckboxCheckmark,
            rect,
            0.0,
            style_number(&item.style, "check_width").unwrap_or(3.0),
            style_color_u8(&item.style, "check_color").unwrap_or([28, 138, 110, 255]),
            [0, 0, 0, 0],
            style_number(&item.style, "check_aa")
                .unwrap_or(0.9)
                .clamp(0.0, 1.75),
            "checkbox-checkmark",
        );
        checkmark.control_points = vec![[start.0, start.1], [middle.0, middle.1], [end.0, end.1]];
        primitives.push(checkmark);
    }
    primitives
}

fn checkbox_check_points(rect: Rect) -> ((f32, f32), (f32, f32), (f32, f32)) {
    let point = |x: f32, y: f32| (rect.x + rect.width * x, rect.y + rect.height * y);
    (point(0.33, 0.55), point(0.45, 0.67), point(0.70, 0.35))
}

#[allow(clippy::too_many_arguments)]
fn checkbox_primitive(
    item: &DisplayItem,
    primitive: RenderVisualPrimitiveKind,
    bounds: Rect,
    radius: f32,
    stroke_width: f32,
    color: [u8; 4],
    secondary_color: [u8; 4],
    antialias: f32,
    dependency: &str,
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        primitive,
        bounds,
        clip: clip_rect_for_style(&item.style),
        radius,
        stroke_width,
        color,
        secondary_color,
        antialias,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(item, dependency),
    }
}

fn frosted_material_layer_primitives_for_item(
    item: &DisplayItem,
    bounds: Rect,
    radius: f32,
) -> Vec<RenderVisualPrimitive> {
    let frosted_blur = style_number(&item.style, "frosted_blur")
        .unwrap_or(0.0)
        .clamp(0.0, 40.0);
    let frosted_saturate = style_number(&item.style, "frosted_saturate")
        .unwrap_or(1.0)
        .clamp(0.0, 2.0);
    if frosted_blur <= 0.01 && frosted_saturate <= 1.01 {
        return Vec::new();
    }
    let highlight = style_number(&item.style, "glass_highlight")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let mut haze = style_color_u8(&item.style, "glass_highlight_color")
        .unwrap_or([255, 255, 255, 255])
        .map(|channel| channel as f32 / 255.0);
    let steps = (frosted_blur / 7.0).ceil().clamp(2.0, 5.0) as u32;
    let mut primitives = Vec::new();
    for step in 0..steps {
        let t = (step + 1) as f32 / steps as f32;
        let expand = frosted_blur * 0.18 * t;
        haze[3] = ((frosted_blur / 40.0) * 0.030 + (frosted_saturate - 1.0) * 0.025)
            .mul_add(1.0 - t * 0.55, highlight * 0.010)
            .clamp(0.004, 0.055);
        primitives.push(material_primitive(
            item,
            RenderVisualPrimitiveKind::FrostedMaterialLayer,
            expanded_rect(bounds, expand),
            radius + expand,
            rgba8_from_f32(haze),
            "frosted-material-layer",
        ));
    }
    primitives
}

fn material_highlight_primitives_for_item(
    item: &DisplayItem,
    bounds: Rect,
    radius: f32,
) -> Vec<RenderVisualPrimitive> {
    let radius = radius.clamp(0.0, bounds.width.min(bounds.height) * 0.5);
    let gloss = style_number(&item.style, "gloss")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let transparency = style_number(&item.style, "transparency")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let refraction = style_number(&item.style, "refraction")
        .unwrap_or(0.0)
        .max(0.0);
    let depth = style_number(&item.style, "depth").unwrap_or(0.0).max(0.0);
    let glass_highlight = style_number(&item.style, "glass_highlight")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let highlight_color = style_color_u8(&item.style, "glass_highlight_color")
        .unwrap_or([255, 255, 255, 255])
        .map(|channel| channel as f32 / 255.0);
    let mut primitives = Vec::new();
    let top_alpha =
        (gloss * 0.11 + transparency * 0.08 + refraction * 0.015 + glass_highlight * 0.12)
            .clamp(0.0, 0.30);
    if top_alpha > 0.01 && bounds.width > 2.0 && bounds.height > 2.0 {
        let band = (1.0 + gloss * 2.0 + transparency * 2.0 + glass_highlight * 4.0).clamp(1.0, 7.0);
        primitives.push(material_primitive(
            item,
            RenderVisualPrimitiveKind::MaterialHighlight,
            Rect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: band.min(bounds.height),
            },
            radius,
            rgba8_from_f32(color_with_alpha_scale(
                highlight_color,
                top_alpha / highlight_color[3].max(0.001),
            )),
            "material-highlight-top",
        ));
        primitives.push(material_primitive(
            item,
            RenderVisualPrimitiveKind::MaterialHighlight,
            Rect {
                x: bounds.x,
                y: bounds.y,
                width: band.min(bounds.width),
                height: bounds.height,
            },
            radius,
            rgba8_from_f32(color_with_alpha_scale(
                highlight_color,
                (top_alpha * 0.45) / highlight_color[3].max(0.001),
            )),
            "material-highlight-left",
        ));
        if glass_highlight > 0.01 && bounds.width > 24.0 && bounds.height > 16.0 {
            let glint_width = (bounds.width * 0.18).clamp(10.0, 34.0);
            primitives.push(material_primitive(
                item,
                RenderVisualPrimitiveKind::MaterialHighlight,
                Rect {
                    x: bounds.x + bounds.width - glint_width - 2.0,
                    y: bounds.y + 2.0,
                    width: glint_width,
                    height: (band * 0.75).min(bounds.height),
                },
                radius,
                rgba8_from_f32(color_with_alpha_scale(
                    highlight_color,
                    (top_alpha * 0.65) / highlight_color[3].max(0.001),
                )),
                "material-highlight-glint",
            ));
        }
    }
    let bottom_alpha = ((1.0 - gloss) * 0.035 + depth * 0.006).clamp(0.0, 0.18);
    if bottom_alpha > 0.01 && bounds.width > 2.0 && bounds.height > 3.0 {
        let band = (1.0 + depth * 0.16).clamp(1.0, 4.0);
        primitives.push(material_primitive(
            item,
            RenderVisualPrimitiveKind::MaterialHighlight,
            Rect {
                x: bounds.x,
                y: bounds.y + bounds.height - band.min(bounds.height),
                width: bounds.width,
                height: band.min(bounds.height),
            },
            radius,
            rgba8_from_f32([0.0, 0.0, 0.0, bottom_alpha]),
            "material-highlight-bottom",
        ));
    }
    primitives
}

fn material_primitive(
    item: &DisplayItem,
    primitive: RenderVisualPrimitiveKind,
    bounds: Rect,
    radius: f32,
    color: [u8; 4],
    dependency: &str,
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        primitive,
        bounds,
        clip: clip_rect_for_style(&item.style),
        radius,
        stroke_width: 0.0,
        color,
        secondary_color: [0, 0, 0, 0],
        antialias: 0.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(item, dependency),
    }
}

fn border_primitive(
    item: &DisplayItem,
    primitive: RenderVisualPrimitiveKind,
    bounds: Rect,
    radius: f32,
    stroke_width: f32,
    color: [u8; 4],
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        primitive,
        bounds,
        clip: clip_rect_for_style(&item.style),
        radius,
        stroke_width,
        color,
        secondary_color: [0, 0, 0, 0],
        antialias: 0.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(item, border_dependency_name(primitive)),
    }
}

fn border_dependency_name(primitive: RenderVisualPrimitiveKind) -> &'static str {
    match primitive {
        RenderVisualPrimitiveKind::Border => "border",
        RenderVisualPrimitiveKind::BorderTop => "border-top",
        RenderVisualPrimitiveKind::BorderRight => "border-right",
        RenderVisualPrimitiveKind::BorderBottom => "border-bottom",
        RenderVisualPrimitiveKind::BorderLeft => "border-left",
        _ => "border",
    }
}

fn text_overlay_primitives(
    item: &DisplayItem,
    columns: &mut impl RenderTextColumnMeasurer,
) -> Vec<RenderVisualPrimitive> {
    let mut primitives = Vec::new();
    let raw_text = item.text.as_deref().unwrap_or_default();
    let font_size = style_number(&item.style, "size").unwrap_or(14.0);
    let line_height = style_line_height(&item.style, font_size);
    let column_edges = columns.column_edges(raw_text, &item.style, line_height);
    let text_width = column_edges.last().copied().unwrap_or_default();
    let text_left = text_left_for_width(item, text_width);
    let x_for_column = |column: f32| {
        let column = column.max(0.0);
        let lower = column.floor() as usize;
        let fraction = column - lower as f32;
        let lower_x = column_edges
            .get(lower)
            .copied()
            .or_else(|| column_edges.last().copied())
            .unwrap_or_default();
        let upper_x = column_edges
            .get(lower.saturating_add(1))
            .copied()
            .or_else(|| column_edges.last().copied())
            .unwrap_or(lower_x);
        text_left + lower_x + (upper_x - lower_x) * fraction
    };
    if matches!(item.kind, DocumentNodeKind::Text) {
        let line_top = item.bounds.y + 2.0;
        let editor_line_height = (item.bounds.height - 4.0).max(font_size);
        if let (Some(start), Some(end)) = (
            style_number(&item.style, "editor_selection_start"),
            style_number(&item.style, "editor_selection_end"),
        ) {
            let start = start.max(0.0);
            let end = end.max(start);
            let start_x = x_for_column(start);
            let end_x = x_for_column(end);
            primitives.push(text_overlay_primitive(
                item,
                RenderVisualPrimitiveKind::EditorSelection,
                Rect {
                    x: start_x,
                    y: line_top,
                    width: (end_x - start_x).max(2.0),
                    height: editor_line_height,
                },
                style_color_u8(&item.style, "editor_selection_color").unwrap_or([12, 15, 20, 255]),
            ));
        }
        if let Some(columns_value) = style_text(&item.style, "editor_bracket_columns") {
            let bracket_color =
                style_color_u8(&item.style, "editor_bracket_color").unwrap_or([82, 139, 255, 51]);
            for column in columns_value
                .split(',')
                .filter_map(|column| column.parse::<f32>().ok())
            {
                let column = column.max(0.0);
                let cell_width = (x_for_column(column + 1.0) - x_for_column(column)).max(1.0);
                let bracket_width = (cell_width * 0.72).max(2.0);
                let bracket_x = x_for_column(column) + (cell_width - bracket_width) * 0.5;
                primitives.push(text_overlay_primitive(
                    item,
                    RenderVisualPrimitiveKind::EditorBracketHighlight,
                    Rect {
                        x: bracket_x,
                        y: line_top,
                        width: bracket_width,
                        height: editor_line_height,
                    },
                    bracket_color,
                ));
            }
        }
        if style_bool(&item.style, "editor_caret_visible") == Some(true)
            && let Some(column) = style_number(&item.style, "editor_caret_column")
        {
            primitives.push(text_overlay_primitive(
                item,
                RenderVisualPrimitiveKind::EditorCaret,
                Rect {
                    x: x_for_column(column.max(0.0)),
                    y: line_top,
                    width: 2.0,
                    height: editor_line_height,
                },
                style_color_u8(&item.style, "editor_caret_color")
                    .or_else(|| style_color_u8(&item.style, "color"))
                    .unwrap_or([23, 59, 255, 255]),
            ));
        }
    }
    if style_bool(&item.style, "strikethrough") == Some(true) {
        primitives.push(text_overlay_primitive(
            item,
            RenderVisualPrimitiveKind::Strikethrough,
            strikethrough_rect_for_item(item, &x_for_column),
            style_color_u8(&item.style, "if_color")
                .or_else(|| style_color_u8(&item.style, "color"))
                .unwrap_or([148, 148, 148, 255]),
        ));
    }
    if style_bool(&item.style, "underline_if") == Some(true) {
        primitives.push(text_overlay_primitive(
            item,
            RenderVisualPrimitiveKind::Underline,
            underline_rect_for_item(item, &x_for_column),
            style_color_u8(&item.style, "underline_color")
                .or_else(|| style_color_u8(&item.style, "color"))
                .unwrap_or([148, 148, 148, 255]),
        ));
    }
    if matches!(item.kind, DocumentNodeKind::Button)
        && style_bool(&item.style, "checked") == Some(true)
    {
        let color = style_color_u8(&item.style, "check_color")
            .or_else(|| style_color_u8(&item.style, "color"))
            .unwrap_or([92, 194, 176, 255]);
        let left = item.bounds.x + 9.0;
        let top = item.bounds.y + 12.0;
        for rect in [
            Rect {
                x: left,
                y: top + 11.0,
                width: 3.0,
                height: 9.0,
            },
            Rect {
                x: left + 5.0,
                y: top + 4.0,
                width: 3.0,
                height: 17.0,
            },
        ] {
            primitives.push(text_overlay_primitive(
                item,
                RenderVisualPrimitiveKind::ButtonCheckmark,
                rect,
                color,
            ));
        }
    }
    if matches!(item.kind, DocumentNodeKind::TextInput)
        && (item.focused || style_bool(&item.style, "focus") == Some(true))
    {
        let text_bounds = text_content_bounds_for_item(item);
        let font_size = style_number(&item.style, "size").unwrap_or(14.0);
        let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
        let vertical_align = text_vertical_align(&item.kind, &item.style);
        let line_height = style_line_height(&item.style, font_size)
            .min(text_bounds.height.max(1.0))
            .max(1.0);
        let lines = raw_text.split('\n').collect::<Vec<_>>();
        let line_top = text_top_for_parts(
            text_bounds,
            line_height * lines.len().max(1) as f32,
            text_inset,
            vertical_align,
        );
        let mut x_for_line_column = |line: usize, column: f32| {
            let line = lines.get(line).copied().unwrap_or_default();
            let edges = columns.column_edges(line, &item.style, line_height);
            let left = text_left_for_width(item, edges.last().copied().unwrap_or_default());
            let column = column.max(0.0);
            let lower = column.floor() as usize;
            let fraction = column - lower as f32;
            let lower_x = edges
                .get(lower)
                .copied()
                .or_else(|| edges.last().copied())
                .unwrap_or_default();
            let upper_x = edges
                .get(lower.saturating_add(1))
                .copied()
                .or_else(|| edges.last().copied())
                .unwrap_or(lower_x);
            left + lower_x + (upper_x - lower_x) * fraction
        };
        if let (Some(start), Some(end)) = (
            style_number(&item.style, "selection_start"),
            style_number(&item.style, "selection_end"),
        ) {
            let start = start.max(0.0);
            let end = end.max(start);
            let start_line = style_number(&item.style, "selection_start_line")
                .unwrap_or(0.0)
                .max(0.0) as usize;
            let end_line = style_number(&item.style, "selection_end_line")
                .unwrap_or(start_line as f32)
                .max(start_line as f32) as usize;
            for line in start_line..=end_line.min(lines.len().saturating_sub(1)) {
                let line_start = if line == start_line { start } else { 0.0 };
                let line_end = if line == end_line {
                    end
                } else {
                    lines.get(line).map_or(0, |line| line.chars().count()) as f32
                };
                let start_x = x_for_line_column(line, line_start);
                let end_x = x_for_line_column(line, line_end);
                primitives.push(text_overlay_primitive(
                    item,
                    RenderVisualPrimitiveKind::TextInputSelection,
                    Rect {
                        x: start_x,
                        y: line_top + line as f32 * line_height,
                        width: (end_x - start_x).max(2.0),
                        height: line_height,
                    },
                    style_color_u8(&item.style, "selection_color").unwrap_or([82, 139, 255, 72]),
                ));
            }
        }
        if style_bool(&item.style, "caret_visible") == Some(true) {
            let caret_column = style_number(&item.style, "caret_column").unwrap_or(0.0);
            let caret_line = style_number(&item.style, "caret_line")
                .unwrap_or(0.0)
                .max(0.0) as usize;
            primitives.push(text_overlay_primitive(
                item,
                RenderVisualPrimitiveKind::TextInputCaret,
                Rect {
                    x: x_for_line_column(caret_line, caret_column.max(0.0)),
                    y: line_top + caret_line as f32 * line_height,
                    width: 2.0,
                    height: line_height,
                },
                style_color_u8(&item.style, "caret_color")
                    .or_else(|| style_color_u8(&item.style, "color"))
                    .unwrap_or([56, 56, 56, 255]),
            ));
        }
    }
    primitives
}

fn text_overlay_primitive(
    item: &DisplayItem,
    primitive: RenderVisualPrimitiveKind,
    bounds: Rect,
    color: [u8; 4],
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: item.node.clone(),
        retained_chunk_id: retained_chunk_id_for_item(item),
        source_kind: item.kind.clone(),
        primitive,
        bounds,
        clip: clip_rect_for_style(&item.style),
        radius: 0.0,
        stroke_width: 0.0,
        color,
        secondary_color: [0, 0, 0, 0],
        antialias: 0.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity: item.style_identity,
        dependency_set: visual_primitive_dependencies(
            item,
            text_overlay_dependency_name(primitive),
        ),
    }
}

fn text_overlay_dependency_name(primitive: RenderVisualPrimitiveKind) -> &'static str {
    match primitive {
        RenderVisualPrimitiveKind::EditorSelection => "editor-selection",
        RenderVisualPrimitiveKind::EditorBracketHighlight => "editor-bracket-highlight",
        RenderVisualPrimitiveKind::EditorCaret => "editor-caret",
        RenderVisualPrimitiveKind::TextInputSelection => "text-input-selection",
        RenderVisualPrimitiveKind::TextInputCaret => "text-input-caret",
        RenderVisualPrimitiveKind::Underline => "underline",
        RenderVisualPrimitiveKind::Strikethrough => "strikethrough",
        RenderVisualPrimitiveKind::ButtonCheckmark => "button-checkmark",
        RenderVisualPrimitiveKind::ViewportBackground
        | RenderVisualPrimitiveKind::Shadow
        | RenderVisualPrimitiveKind::FrostedMaterialLayer
        | RenderVisualPrimitiveKind::Fill
        | RenderVisualPrimitiveKind::MaterialHighlight
        | RenderVisualPrimitiveKind::Asset
        | RenderVisualPrimitiveKind::CheckboxCastShadow
        | RenderVisualPrimitiveKind::Checkbox
        | RenderVisualPrimitiveKind::CheckboxInnerShadow
        | RenderVisualPrimitiveKind::CheckboxHighlight
        | RenderVisualPrimitiveKind::CheckboxCheckmark
        | RenderVisualPrimitiveKind::Border
        | RenderVisualPrimitiveKind::BorderTop
        | RenderVisualPrimitiveKind::BorderRight
        | RenderVisualPrimitiveKind::BorderBottom
        | RenderVisualPrimitiveKind::BorderLeft => "visual",
    }
}

fn retained_chunk_vertex_estimate_for_bounds(bounds: Rect) -> u32 {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        0
    } else {
        6
    }
}

pub fn render_text_runs(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
) -> Vec<RenderTextRun> {
    render_text_runs_for_nodes(frame, width, height, columns, None)
}

pub fn render_text_runs_for_touched_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
    nodes: &BTreeSet<DocumentNodeId>,
) -> Vec<RenderTextRun> {
    render_text_runs_for_nodes(frame, width, height, columns, Some(nodes))
}

fn render_text_runs_for_nodes(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
    columns: &mut impl RenderTextColumnMeasurer,
    nodes: Option<&BTreeSet<DocumentNodeId>>,
) -> Vec<RenderTextRun> {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let mut runs = Vec::new();
    for item in frame
        .display_list
        .iter()
        .filter(|item| rect_intersects(item.bounds, viewport))
    {
        if nodes.is_some_and(|nodes| !nodes.contains(&item.node)) {
            continue;
        }
        let Some(_) = clipped_item_bounds(item) else {
            continue;
        };
        let size = style_number(&item.style, "size").unwrap_or(14.0);
        let line_height = style_line_height(&item.style, size);
        let raw_text = item.text.as_deref().unwrap_or_default();
        if style_bool(&item.style, "paint") == Some(false)
            || (style_bool(&item.style, "__hover_visible") == Some(true)
                && style_bool(&item.style, "__hover_paint") != Some(true))
        {
            continue;
        }
        if matches!(item.kind, DocumentNodeKind::Checkbox) {
            continue;
        }
        let checked = style_bool(&item.style, "checked") == Some(true);
        let input_wants_caret_layout = matches!(item.kind, DocumentNodeKind::TextInput)
            && (item.focused
                || style_bool(&item.style, "focus") == Some(true)
                || style_bool(&item.style, "caret_visible").is_some()
                || item.style.contains_key("caret_column"));
        let mut placeholder_active = false;
        let (text, color) = if raw_text.trim().is_empty() {
            if checked && !matches!(item.kind, DocumentNodeKind::Checkbox) {
                (
                    "✓".to_owned(),
                    style_color_u8(&item.style, "check_color").unwrap_or([92, 194, 175, 255]),
                )
            } else if let Some(placeholder) =
                style_text(&item.style, "placeholder").filter(|text| !text.trim().is_empty())
            {
                placeholder_active = true;
                (
                    placeholder.to_owned(),
                    style_color_u8(&item.style, "placeholder_color")
                        .unwrap_or([154, 154, 154, 255]),
                )
            } else if input_wants_caret_layout {
                (
                    String::new(),
                    style_color_u8(&item.style, "color").unwrap_or([36, 44, 58, 255]),
                )
            } else {
                continue;
            }
        } else {
            let color = style_color_u8(&item.style, "color").unwrap_or([36, 44, 58, 255]);
            (raw_text.to_owned(), color)
        };
        let run_size = if placeholder_active {
            style_number(&item.style, "placeholder_size").unwrap_or(size)
        } else {
            size
        };
        let run_line_height = if placeholder_active {
            style_number(&item.style, "placeholder_line_height").unwrap_or(line_height)
        } else {
            line_height
        };
        let font_family = if checked && !matches!(item.kind, DocumentNodeKind::Checkbox) {
            "DejaVu Sans"
        } else if placeholder_active {
            style_text(&item.style, "placeholder_font")
                .or_else(|| style_text(&item.style, "font"))
                .unwrap_or(DEFAULT_DOCUMENT_FONT_FAMILY)
        } else {
            style_text(&item.style, "font").unwrap_or(DEFAULT_DOCUMENT_FONT_FAMILY)
        };
        let rich_spans = rich_text_spans(&item.style, &text, color);
        runs.push(RenderTextRun {
            node: item.node.clone(),
            owner_node: item.node.clone(),
            font_id: item.style_identity.font_id,
            paint_id: item.style_identity.paint_id,
            bounds: text_content_bounds_for_item(item),
            clip: clip_rect_for_style(&item.style),
            text,
            rich_spans,
            font_family: font_family.to_owned(),
            font_style: if placeholder_active {
                style_text(&item.style, "placeholder_style")
                    .map(text_font_style_value)
                    .unwrap_or_else(|| text_font_style(&item.style))
            } else {
                text_font_style(&item.style)
            },
            font_weight: if placeholder_active {
                placeholder_font_weight(&item.style)
                    .unwrap_or_else(|| text_font_weight(&item.style))
            } else {
                text_font_weight(&item.style)
            },
            font_features: style_text(&item.style, "font_features")
                .unwrap_or("")
                .to_owned(),
            text_inset: style_number(&item.style, "text_inset").unwrap_or(4.0),
            text_clip_padding: style_number(&item.style, "text_clip_padding").unwrap_or(0.0),
            color,
            size: run_size,
            line_height: run_line_height,
            align: text_align(&item.kind, &item.style),
            vertical_align: text_vertical_align(&item.kind, &item.style),
            rotate_degrees: normalized_quarter_turn(style_number(&item.style, "rotate")),
            wrap: style_bool(&item.style, "text_wrap") == Some(true),
        });
        runs.extend(editor_type_hint_runs(item, columns));
    }
    runs
}

fn editor_type_hint_runs(
    item: &DisplayItem,
    columns: &mut impl RenderTextColumnMeasurer,
) -> Vec<RenderTextRun> {
    if !matches!(item.kind, DocumentNodeKind::Text) {
        return Vec::new();
    }
    let hints = editor_type_hint_payloads(&item.style);
    if hints.is_empty() {
        return Vec::new();
    }
    let source_text = item.text.as_deref().unwrap_or_default();
    let column_edges = columns.column_edges(source_text, &item.style, item.bounds.height);
    let inset = style_number(&item.style, "text_inset").unwrap_or(0.0);
    let font_size = (style_number(&item.style, "size").unwrap_or(14.0) - 1.0).max(10.0);
    let font_family = style_text(&item.style, "font").unwrap_or(DEFAULT_EDITOR_FONT_FAMILY);
    let font_features = style_text(&item.style, "font_features")
        .unwrap_or("")
        .to_owned();
    let color =
        style_color_u8(&item.style, "editor_type_hint_color").unwrap_or([138, 160, 184, 255]);
    let source_len = source_text.chars().count();
    hints
        .into_iter()
        .take(1)
        .enumerate()
        .filter_map(|(index, hint)| {
            if hint.compact_label.trim().is_empty() {
                return None;
            }
            let column = hint
                .anchor_column
                .saturating_sub(1)
                .max(source_len)
                .min(source_len);
            let x = item.bounds.x
                + inset
                + column_edges
                    .get(column)
                    .copied()
                    .or_else(|| column_edges.last().copied())
                    .unwrap_or_default()
                + 12.0;
            let available_width = item.bounds.x + item.bounds.width - x;
            if available_width < font_size * 2.0 {
                return None;
            }
            Some(RenderTextRun {
                node: DocumentNodeId(format!("{}:type-hint:{index}", item.node.0)),
                owner_node: item.node.clone(),
                font_id: item.style_identity.font_id,
                paint_id: item.style_identity.paint_id,
                bounds: Rect {
                    x,
                    y: item.bounds.y,
                    width: available_width,
                    height: item.bounds.height,
                },
                clip: clip_rect_for_style(&item.style),
                text: format!(": {}", hint.compact_label),
                rich_spans: Vec::new(),
                font_family: font_family.to_owned(),
                font_style: RenderFontStyle::Italic,
                font_weight: RenderFontWeight(400),
                font_features: font_features.clone(),
                text_inset: 0.0,
                text_clip_padding: 0.0,
                color,
                size: font_size,
                line_height: item.bounds.height,
                align: RenderTextAlign::Left,
                vertical_align: RenderTextVerticalAlign::Center,
                rotate_degrees: 0,
                wrap: false,
            })
        })
        .collect()
}

fn rich_text_spans(
    style: &StyleMap,
    text: &str,
    default_color: [u8; 4],
) -> Vec<RenderRichTextSpan> {
    let payloads = rich_text_span_payloads(style);
    let mut source_text = String::new();
    let spans = payloads
        .into_iter()
        .map(|payload| {
            source_text.push_str(payload.source_text.as_deref().unwrap_or(&payload.text));
            RenderRichTextSpan {
                text: payload.text,
                color: payload
                    .color
                    .as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(default_color),
                font_style: payload
                    .font_style
                    .as_deref()
                    .map(text_font_style_value)
                    .unwrap_or(RenderFontStyle::Normal),
                font_weight: payload
                    .font_weight
                    .as_deref()
                    .map(text_font_weight_value)
                    .unwrap_or(RenderFontWeight(400)),
            }
        })
        .collect::<Vec<_>>();
    if source_text == text {
        spans
    } else {
        Vec::new()
    }
}

fn rich_text_span_payloads(style: &StyleMap) -> Vec<StyleRichTextSpan> {
    match state_style_value(style, "syntax_spans") {
        Some(StyleValue::RichTextSpans(spans)) => spans.clone(),
        _ => Vec::new(),
    }
}

fn editor_type_hint_payloads(style: &StyleMap) -> Vec<StyleEditorTypeHint> {
    match state_style_value(style, "editor_type_hints") {
        Some(StyleValue::EditorTypeHints(hints)) => hints.clone(),
        _ => Vec::new(),
    }
}

fn text_font_style(style: &StyleMap) -> RenderFontStyle {
    style_text(style, "font_style")
        .or_else(|| style_text(style, "style"))
        .map(text_font_style_value)
        .unwrap_or(RenderFontStyle::Normal)
}

fn text_font_style_value(value: &str) -> RenderFontStyle {
    match value.to_ascii_lowercase().as_str() {
        "italic" | "cursive" => RenderFontStyle::Italic,
        "oblique" => RenderFontStyle::Oblique,
        _ => RenderFontStyle::Normal,
    }
}

fn text_font_weight(style: &StyleMap) -> RenderFontWeight {
    style_text(style, "weight")
        .map(text_font_weight_value)
        .or_else(|| {
            style_number(style, "weight")
                .map(|value| RenderFontWeight(value.round().clamp(100.0, 900.0) as u16))
        })
        .unwrap_or(RenderFontWeight(400))
}

fn placeholder_font_weight(style: &StyleMap) -> Option<RenderFontWeight> {
    style_text(style, "placeholder_weight")
        .map(text_font_weight_value)
        .or_else(|| {
            style_number(style, "placeholder_weight")
                .map(|value| RenderFontWeight(value.round().clamp(100.0, 900.0) as u16))
        })
}

fn text_font_weight_value(value: &str) -> RenderFontWeight {
    let weight = match value.to_ascii_lowercase().as_str() {
        "hairline" | "thin" => 100,
        "extralight" | "extra-light" | "ultralight" | "ultra-light" => 200,
        "light" => 300,
        "bold" => 700,
        "bolder" => 800,
        "semibold" | "semi-bold" => 600,
        "medium" => 500,
        "normal" => 400,
        value => value.parse::<u16>().unwrap_or(400),
    };
    RenderFontWeight(weight)
}

fn text_line_height(font_size: f32) -> f32 {
    (font_size.clamp(8.0, 120.0) * 1.25).max(1.0)
}

fn style_line_height(style: &StyleMap, font_size: f32) -> f32 {
    let fallback = text_line_height(font_size);
    match style_number(style, "line_height") {
        Some(value) if value > 0.0 && value < 4.0 => font_size * value,
        Some(value) if value > 0.0 => value,
        _ => fallback,
    }
    .max(1.0)
}

fn normalized_quarter_turn(value: Option<f32>) -> u32 {
    let Some(value) = value else {
        return 0;
    };
    let rounded = value.round();
    if (value - rounded).abs() > 0.01 {
        return 0;
    }
    match ((rounded as i32 % 360) + 360) % 360 {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    }
}

fn text_align(kind: &DocumentNodeKind, style: &StyleMap) -> RenderTextAlign {
    let align = style_text(style, "text_align")
        .or_else(|| style_text(style, "align"))
        .unwrap_or("");
    if align.eq_ignore_ascii_case("left") {
        RenderTextAlign::Left
    } else if align.eq_ignore_ascii_case("right") {
        RenderTextAlign::Right
    } else if align.eq_ignore_ascii_case("center")
        || style_bool(style, "center") == Some(true)
        || matches!(kind, DocumentNodeKind::Button | DocumentNodeKind::Checkbox)
    {
        RenderTextAlign::Center
    } else {
        RenderTextAlign::Left
    }
}

fn text_vertical_align(kind: &DocumentNodeKind, style: &StyleMap) -> RenderTextVerticalAlign {
    match style_text(style, "vertical_align")
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "top" => RenderTextVerticalAlign::Top,
        "bottom" => RenderTextVerticalAlign::Bottom,
        "center" => RenderTextVerticalAlign::Center,
        _ if matches!(
            kind,
            DocumentNodeKind::Button
                | DocumentNodeKind::Checkbox
                | DocumentNodeKind::TextInput
                | DocumentNodeKind::TableCell
        ) =>
        {
            RenderTextVerticalAlign::Center
        }
        _ => RenderTextVerticalAlign::Top,
    }
}

fn text_content_bounds_for_item(item: &DisplayItem) -> Rect {
    let padding = style_edges(&item.style, "padding");
    Rect {
        x: item.bounds.x + padding.left,
        y: item.bounds.y + padding.top,
        width: (item.bounds.width - padding.horizontal()).max(1.0),
        height: (item.bounds.height - padding.vertical()).max(1.0),
    }
}

fn text_left_for_width(item: &DisplayItem, text_width: f32) -> f32 {
    let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
    match text_align(&item.kind, &item.style) {
        RenderTextAlign::Left => item.bounds.x + text_inset,
        RenderTextAlign::Center => {
            item.bounds.x + ((item.bounds.width - text_width) / 2.0).max(text_inset)
        }
        RenderTextAlign::Right => {
            item.bounds.x + (item.bounds.width - text_width - text_inset).max(text_inset)
        }
    }
}

fn text_top_for_parts(
    bounds: Rect,
    line_height: f32,
    text_inset: f32,
    vertical_align: RenderTextVerticalAlign,
) -> f32 {
    let line_height = line_height.max(1.0);
    match vertical_align {
        RenderTextVerticalAlign::Top => bounds.y + 1.0,
        RenderTextVerticalAlign::Center => {
            bounds.y + ((bounds.height - line_height) / 2.0).max(0.0)
        }
        RenderTextVerticalAlign::Bottom => {
            bounds.y + (bounds.height - line_height - text_inset).max(0.0)
        }
    }
}

fn strikethrough_rect_for_item(item: &DisplayItem, x_for_column: &impl Fn(f32) -> f32) -> Rect {
    let text_columns = item
        .text
        .as_deref()
        .map(|text| text.chars().count() as f32)
        .unwrap_or_default();
    let line_height = style_line_height(
        &item.style,
        style_number(&item.style, "size").unwrap_or(14.0),
    );
    let line_top = text_top_for_parts(
        item.bounds,
        line_height,
        style_number(&item.style, "text_inset").unwrap_or(4.0),
        text_vertical_align(&item.kind, &item.style),
    );
    let thickness = style_number(&item.style, "text_decoration_thickness").unwrap_or(1.6);
    let x = x_for_column(0.0);
    let x1 = x_for_column(text_columns);
    Rect {
        x,
        y: line_top + line_height * 0.5 - thickness * 0.5,
        width: (x1 - x).max(1.0),
        height: thickness,
    }
}

fn underline_rect_for_item(item: &DisplayItem, x_for_column: &impl Fn(f32) -> f32) -> Rect {
    let text_columns = item
        .text
        .as_deref()
        .map(|text| text.chars().count() as f32)
        .unwrap_or_default();
    let font_size = style_number(&item.style, "size").unwrap_or(14.0);
    let line_height = style_line_height(&item.style, font_size).min(item.bounds.height.max(1.0));
    let line_top = text_top_for_parts(
        item.bounds,
        line_height,
        style_number(&item.style, "text_inset").unwrap_or(4.0),
        text_vertical_align(&item.kind, &item.style),
    );
    let thickness = style_number(&item.style, "text_decoration_thickness").unwrap_or(1.0);
    let x = x_for_column(0.0);
    let x1 = x_for_column(text_columns);
    Rect {
        x,
        y: (line_top + line_height * 0.88).min(item.bounds.y + item.bounds.height - thickness),
        width: (x1 - x).max(1.0),
        height: thickness,
    }
}

fn visual_primitive_dependencies(item: &DisplayItem, primitive: &str) -> Vec<String> {
    let mut dependencies = vec![
        format!("node:{}", item.node.0),
        format!("kind:{:?}", item.kind),
        format!("primitive:{primitive}"),
        format!("style:{}", item.style_identity.style_id),
        format!("layout:{}", item.style_identity.layout_id),
        format!("paint:{}", item.style_identity.paint_id),
        format!("material:{}", item.style_identity.material_id),
        format!("pseudo:{}", item.style_identity.pseudo_state_id),
    ];
    if item.text.is_some() {
        dependencies.push("text".to_owned());
    }
    if clip_rect_for_style(&item.style).is_some() {
        dependencies.push("clip".to_owned());
    }
    dependencies
}

fn style_asset_url(style: &StyleMap) -> Option<&str> {
    style_text(style, "asset_url")
        .or_else(|| style_text(style, "background_url"))
        .filter(|url| !url.trim().is_empty())
}

fn default_fill_for_kind(kind: &DocumentNodeKind, index: usize) -> [u8; 4] {
    match kind {
        DocumentNodeKind::Root | DocumentNodeKind::Stack | DocumentNodeKind::ScrollRoot => {
            [246, 248, 251, 255]
        }
        DocumentNodeKind::Row => {
            if index.is_multiple_of(2) {
                [255, 255, 255, 255]
            } else {
                [242, 246, 251, 255]
            }
        }
        DocumentNodeKind::TextInput => [255, 255, 255, 255],
        DocumentNodeKind::EmbeddedProgram | DocumentNodeKind::EmbeddedMedia => [255, 255, 255, 0],
        DocumentNodeKind::Button | DocumentNodeKind::Checkbox | DocumentNodeKind::Text => {
            [255, 255, 255, 0]
        }
        DocumentNodeKind::Table | DocumentNodeKind::TableCell => [255, 255, 255, 255],
    }
}

fn material_adjusted_fill_u8(fill: [u8; 4], style: &StyleMap) -> [u8; 4] {
    let mut fill = [
        fill[0] as f32 / 255.0,
        fill[1] as f32 / 255.0,
        fill[2] as f32 / 255.0,
        fill[3] as f32 / 255.0,
    ];
    let transparency = style_number(style, "transparency")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let gloss = style_number(style, "gloss").unwrap_or(0.0).clamp(0.0, 1.0);
    let refraction = style_number(style, "refraction").unwrap_or(0.0).max(0.0);
    let metal = style_number(style, "metal").unwrap_or(0.0).clamp(0.0, 1.0);
    let frosted_blur = style_number(style, "frosted_blur")
        .unwrap_or(0.0)
        .clamp(0.0, 40.0);
    let frosted_saturate = style_number(style, "frosted_saturate")
        .unwrap_or(1.0)
        .clamp(0.0, 2.0);
    if transparency > 0.0 {
        fill[3] *= (1.0 - transparency * 0.58).clamp(0.28, 1.0);
        let lift = (transparency * 0.08 + refraction * 0.015).clamp(0.0, 0.16);
        fill[0] = mix_f32(fill[0], 1.0, lift);
        fill[1] = mix_f32(fill[1], 1.0, lift);
        fill[2] = mix_f32(fill[2], 1.0, lift);
    }
    if frosted_blur > 0.0 {
        let frost = (frosted_blur / 40.0 * 0.10).clamp(0.0, 0.10);
        fill[0] = mix_f32(fill[0], 1.0, frost);
        fill[1] = mix_f32(fill[1], 1.0, frost);
        fill[2] = mix_f32(fill[2], 1.0, frost);
        fill[3] *= (1.0 - frost * 0.35).clamp(0.72, 1.0);
    }
    if frosted_saturate > 1.0 {
        let average = (fill[0] + fill[1] + fill[2]) / 3.0;
        let boost = ((frosted_saturate - 1.0) * 0.18).clamp(0.0, 0.18);
        fill[0] = (fill[0] + (fill[0] - average) * boost).clamp(0.0, 1.0);
        fill[1] = (fill[1] + (fill[1] - average) * boost).clamp(0.0, 1.0);
        fill[2] = (fill[2] + (fill[2] - average) * boost).clamp(0.0, 1.0);
    }
    if gloss > 0.0 {
        let lift = (gloss * 0.035).clamp(0.0, 0.05);
        fill[0] = mix_f32(fill[0], 1.0, lift);
        fill[1] = mix_f32(fill[1], 1.0, lift);
        fill[2] = mix_f32(fill[2], 1.0, lift);
    }
    if metal > 0.0 {
        let average = (fill[0] + fill[1] + fill[2]) / 3.0;
        let t = (metal * 0.18).clamp(0.0, 0.18);
        fill[0] = mix_f32(fill[0], average, t);
        fill[1] = mix_f32(fill[1], average, t);
        fill[2] = mix_f32(fill[2], average, t);
    }
    fill.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn mix_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn color_with_alpha_scale(mut color: [f32; 4], scale: f32) -> [f32; 4] {
    color[3] = (color[3] * scale).clamp(0.0, 1.0);
    color
}

fn rgba8_from_f32(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn expanded_rect(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: (rect.width + amount * 2.0).max(1.0),
        height: (rect.height + amount * 2.0).max(1.0),
    }
}

fn clipped_item_bounds(item: &DisplayItem) -> Option<Rect> {
    clip_rect_for_style(&item.style).map_or(Some(item.bounds), |clip| {
        rect_intersection(item.bounds, clip)
    })
}

fn checkbox_has_asset_icon(frame: &LayoutFrame, checkbox: &DisplayItem) -> bool {
    if style_asset_url(&checkbox.style).is_some() {
        return true;
    }
    frame.display_list.iter().any(|item| {
        item.node != checkbox.node
            && style_asset_url(&item.style).is_some()
            && rect_contains_with_epsilon(checkbox.bounds, item.bounds, 1.0)
            && item.bounds.width >= checkbox.bounds.width * 0.5
            && item.bounds.height >= checkbox.bounds.height * 0.5
    })
}

fn rect_contains_with_epsilon(outer: Rect, inner: Rect, epsilon: f32) -> bool {
    inner.x + epsilon >= outer.x
        && inner.y + epsilon >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + epsilon
        && inner.y + inner.height <= outer.y + outer.height + epsilon
}

fn clip_rect_for_style(style: &StyleMap) -> Option<Rect> {
    Some(Rect {
        x: style_number(style, "__clip_x")?,
        y: style_number(style, "__clip_y")?,
        width: style_number(style, "__clip_width")?,
        height: style_number(style, "__clip_height")?,
    })
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

fn style_edges(style: &StyleMap, prefix: &str) -> EdgeSpacing {
    let all = style_number(style, prefix).unwrap_or(0.0);
    EdgeSpacing {
        top: style_number(style, &format!("{prefix}_top")).unwrap_or(all),
        right: style_number(style, &format!("{prefix}_right")).unwrap_or(all),
        bottom: style_number(style, &format!("{prefix}_bottom")).unwrap_or(all),
        left: style_number(style, &format!("{prefix}_left")).unwrap_or(all),
    }
}

fn style_number(style: &StyleMap, key: &str) -> Option<f32> {
    match state_style_value(style, key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_bool(style: &StyleMap, key: &str) -> Option<bool> {
    match state_style_value(style, key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => {
            None
        }
    }
}

fn style_text<'a>(style: &'a StyleMap, key: &str) -> Option<&'a str> {
    match state_style_value(style, key)? {
        StyleValue::Text(value) => Some(value),
        StyleValue::Number(_)
        | StyleValue::Bool(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
    }
}

fn state_style_value<'a>(style: &'a StyleMap, key: &str) -> Option<&'a StyleValue> {
    if style_bool_raw(style, "__hover") == Some(true) {
        let hover_key = format!("__hover_{key}");
        if let Some(value) = style.get(&hover_key) {
            return Some(value);
        }
    }
    if style_bool_raw(style, "__focused") == Some(true)
        || style_bool_raw(style, "focus") == Some(true)
    {
        let focus_key = format!("__focus_{key}");
        if let Some(value) = style.get(&focus_key) {
            return Some(value);
        }
    }
    if style_bool_raw(style, "selected") == Some(true) {
        let selected_key = format!("__selected_{key}");
        if let Some(value) = style.get(&selected_key) {
            return Some(value);
        }
    }
    style.get(key)
}

fn style_bool_raw(style: &StyleMap, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => {
            None
        }
    }
}

fn style_color_u8(style: &StyleMap, key: &str) -> Option<[u8; 4]> {
    style_value_color_u8(state_style_value(style, key)?)
}

pub fn style_value_color_u8(value: &StyleValue) -> Option<[u8; 4]> {
    match value {
        StyleValue::Text(value) => parse_oklch_color(value).or_else(|| parse_hex_color(value)),
        StyleValue::Number(_)
        | StyleValue::Bool(_)
        | StyleValue::RichTextSpans(_)
        | StyleValue::EditorTypeHints(_) => None,
    }
}

fn parse_oklch_color(value: &str) -> Option<[u8; 4]> {
    let body = value.trim().strip_prefix("Oklch[")?.strip_suffix(']')?;
    let mut lightness = None;
    let mut chroma = Some(0.0);
    let mut hue = Some(0.0);
    let mut alpha = Some(1.0);
    for part in body.split(',') {
        let (key, value) = part.split_once(':')?;
        let number = value.trim().parse::<f64>().ok()?;
        match key.trim() {
            "lightness" => lightness = Some(number),
            "chroma" => chroma = Some(number),
            "hue" => hue = Some(number),
            "alpha" => alpha = Some(number),
            _ => {}
        }
    }
    let l = lightness?;
    let c = chroma.unwrap_or_default();
    let h = hue.unwrap_or_default().to_radians();
    let a = c * h.cos();
    let b = c * h.sin();
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let blue = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s;
    let to_u8 = |channel: f64| (linear_to_srgb(channel).clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([
        to_u8(r),
        to_u8(g),
        to_u8(blue),
        (alpha.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
    ])
}

fn linear_to_srgb(channel: f64) -> f64 {
    if channel <= 0.003_130_8 {
        12.92 * channel
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn parse_hex_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.trim().strip_prefix('#')?;
    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    match hex.len() {
        6 => Some([parse(0..2)?, parse(2..4)?, parse(4..6)?, 255]),
        8 => Some([parse(0..2)?, parse(2..4)?, parse(4..6)?, parse(6..8)?]),
        _ => None,
    }
}

fn rect_intersects(rect: Rect, viewport: Rect) -> bool {
    rect.x < viewport.x + viewport.width
        && rect.x + rect.width > viewport.x
        && rect.y < viewport.y + viewport.height
        && rect.y + rect.height > viewport.y
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

#[cfg(test)]
#[path = "render_scene_tests.rs"]
mod tests;
