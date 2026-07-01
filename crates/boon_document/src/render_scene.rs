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
        }
    }
    Ok(report)
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
                if text_run.node == *node {
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
        let Some((_, style_identity)) = updates.iter().find_map(|(node, update)| {
            render_text_run_belongs_to_node(&text_run.node, node).then_some(update)
        }) else {
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
        |text_run| &text_run.node,
        render_text_run_belongs_to_any_node,
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
        let node = node_for_entry(&entry).clone();
        let remove = entry_belongs_to_nodes(&node, node_set);
        saw_existing |= remove;
        if remove {
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

fn render_text_run_belongs_to_any_node(
    text_run_node: &DocumentNodeId,
    nodes: &BTreeSet<DocumentNodeId>,
) -> bool {
    nodes
        .iter()
        .any(|node| render_text_run_belongs_to_node(text_run_node, node))
}

fn render_text_run_belongs_to_node(text_run_node: &DocumentNodeId, node: &DocumentNodeId) -> bool {
    text_run_node == node
        || text_run_node
            .0
            .strip_prefix(node.0.as_str())
            .is_some_and(|suffix| suffix.starts_with(':'))
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
        if text_run.node == *node {
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
    }
    primitives.extend(border_primitives);
    primitives
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
        && style_bool(&item.style, "caret_visible") == Some(true)
    {
        let text_bounds = text_content_bounds_for_item(item);
        let font_size = style_number(&item.style, "size").unwrap_or(14.0);
        let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
        let vertical_align = text_vertical_align(&item.kind, &item.style);
        let line_height =
            style_line_height(&item.style, font_size).min(text_bounds.height.max(1.0));
        let line_top = text_top_for_parts(text_bounds, line_height, text_inset, vertical_align);
        let caret_column = style_number(&item.style, "caret_column").unwrap_or(0.0);
        primitives.push(text_overlay_primitive(
            item,
            RenderVisualPrimitiveKind::TextInputCaret,
            Rect {
                x: x_for_column(caret_column.max(0.0)),
                y: line_top,
                width: 2.0,
                height: line_height,
            },
            style_color_u8(&item.style, "caret_color")
                .or_else(|| style_color_u8(&item.style, "color"))
                .unwrap_or([56, 56, 56, 255]),
        ));
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
    match state_style_value(style, "syntax_spans_json") {
        Some(StyleValue::RichTextSpans(spans)) => spans.clone(),
        _ => style_text(style, "syntax_spans_json")
            .and_then(|spans_json| serde_json::from_str::<Vec<StyleRichTextSpan>>(spans_json).ok())
            .unwrap_or_default(),
    }
}

fn editor_type_hint_payloads(style: &StyleMap) -> Vec<StyleEditorTypeHint> {
    match state_style_value(style, "editor_type_hints_json") {
        Some(StyleValue::EditorTypeHints(hints)) => hints.clone(),
        _ => style_text(style, "editor_type_hints_json")
            .and_then(|hints_json| {
                serde_json::from_str::<Vec<StyleEditorTypeHint>>(hints_json).ok()
            })
            .unwrap_or_default(),
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
    } else if align.eq_ignore_ascii_case("center") {
        RenderTextAlign::Center
    } else if matches!(kind, DocumentNodeKind::Button | DocumentNodeKind::Checkbox) {
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
            if index % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [242, 246, 251, 255]
            }
        }
        DocumentNodeKind::TextInput => [255, 255, 255, 255],
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
mod tests {
    use super::*;
    use crate::{
        AccessibilityTree, DocumentDerivedIndexBundle, DocumentFrame, DocumentNode, LayoutMetrics,
        TextValue,
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

        let encoded = serde_json::to_string(&scene).expect("render scene should serialize");
        assert!(encoded.contains("row-1"));
        assert!(!encoded.contains(&["w", "gpu"].concat()));
        assert!(!encoded.contains(&["glyph", "on"].concat()));
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

        assert!(primitives.iter().any(|primitive| {
            primitive.primitive == RenderVisualPrimitiveKind::ViewportBackground
        }));
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
            primitive.node.0 == "check"
                && primitive.primitive == RenderVisualPrimitiveKind::Checkbox
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
            primitive.node.0 == "check-icon"
                && primitive.primitive == RenderVisualPrimitiveKind::Asset
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
                primitive.node.0 == "material"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
            })
            .expect("base fill primitive")
            .color;
        let material_frame = frame_for_style(material_style);
        let material_color = render_visual_primitives(&material_frame, 320, 200, &mut columns)
            .into_iter()
            .find(|primitive| {
                primitive.node.0 == "material"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
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
                primitive.node.0 == "glass"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
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
                primitive.node.0 == "shadowed"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
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
                primitive.node.0 == "child"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
            })
            .expect("child fill primitive");
        let parent_border_index = primitives
            .iter()
            .position(|primitive| {
                primitive.node.0 == "parent"
                    && primitive.primitive == RenderVisualPrimitiveKind::Border
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
            focused: false,
            style_identity: identity(),
        };
        let mut input_style = StyleMap::new();
        input_style.insert("size".to_owned(), StyleValue::Number(12.0));
        input_style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
        input_style.insert("caret_visible".to_owned(), StyleValue::Bool(true));
        input_style.insert("caret_column".to_owned(), StyleValue::Number(1.0));
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
                primitive.node.0 == "label"
                    && primitive.primitive == RenderVisualPrimitiveKind::Fill
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
            focused: false,
            style_identity: identity(),
        });
        let mut columns = ApproximateTextColumnMeasurer;
        let runs = render_text_runs(&frame, 320, 200, &mut columns);

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].align, RenderTextAlign::Center);
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
            "syntax_spans_json".to_owned(),
            StyleValue::RichTextSpans(vec![StyleRichTextSpan {
                text: "SOURCE".to_owned(),
                source_text: Some("SOURCE".to_owned()),
                color: Some("#ff0000".to_owned()),
                font_style: Some("italic".to_owned()),
                font_weight: Some("bold".to_owned()),
            }]),
        );
        style.insert(
            "editor_type_hints_json".to_owned(),
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
            "syntax_spans_json".to_owned(),
            StyleValue::Text(
                r##"[{"text":"SOURCE","source_text":"SOURCE","color":"#ff0000","font_style":"italic","font_weight":"bold"}]"##
                    .to_owned(),
            ),
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
}
