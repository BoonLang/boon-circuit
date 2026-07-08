#[cfg(test)]
use boon_document::LayoutFrame;
use boon_document::{
    DocumentNodeId, DocumentNodeKind, Rect, StyleMap, StyleRichTextSpan, StyleValue,
    render_scene::{
        RenderAssetRef, RenderFontStyle, RenderFontWeight, RenderRichTextSpan,
        RenderScene as DocumentRenderScene, RenderTextAlign, RenderTextColumnMeasurer,
        RenderTextRun, RenderTextVerticalAlign, RenderTextureRef, RenderVisualPrimitive,
        RenderVisualPrimitiveKind,
    },
};
use boon_host::SurfaceId;
use glyphon::{
    Attrs, Buffer, Cache, Color, ContentType, CustomGlyph, CustomGlyphId, Family, FontSystem,
    LayoutGlyph, Metrics, RasterizeCustomGlyphRequest, RasterizedCustomGlyph, Resolution, Shaping,
    Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
    cosmic_text::{FeatureTag, FontFeatures, fontdb},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub mod generated {
    pub mod shader_bindings;
}

pub const REQUIRED_WGPU_VERSION: &str = "29.0.1";
pub const REQUIRED_GLYPHON_VERSION: &str = "0.11.0";
const JETBRAINS_MONO_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Patched.ttf");
const JETBRAINS_MONO_BOLD_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Patched-Bold.ttf");
const JETBRAINS_MONO_ITALIC_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Patched-Italic.ttf");
const JETBRAINS_MONO_BOLD_ITALIC_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Patched-BoldItalic.ttf");
const EDITOR_FONT_FAMILY: &str = "JetBrains Mono";
const EDITOR_FONT_FEATURES: &str = "zero,calt";
const DOCUMENT_FONT_FAMILY: &str = "Nimbus Sans";
const DOCUMENT_MONOSPACE_FONT_FAMILY: &str = "Liberation Mono";
const MAX_CACHED_QUAD_BATCHES: usize = 4096;
const MAX_CACHED_ASSET_TEXTURE_BYTES: u64 = 32 * 1024 * 1024;
const APP_OWNED_READBACK_TIMEOUT: Duration = Duration::from_secs(5);
const PRODUCT_FRAME_GRAPH_SCHEDULER_KIND: &str = "renderer_owned_product_frame_schedule_v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFormat(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderProof {
    pub artifact: RenderProofArtifact,
    pub metrics: FrameMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderProofArtifact {
    AppOwnedPixels {
        artifact_path: String,
        artifact_sha256: String,
        capture_method: String,
        surface_id: SurfaceId,
        surface_epoch: u64,
        frame_seq: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout_frame_hash: Option<String>,
        render_scene_identity_hash: String,
        width: u32,
        height: u32,
        nonblank_samples: usize,
        unique_rgba_values: usize,
        readback_deadline_ms: u64,
        readback_poll_status: String,
    },
    CopyToPresent {
        source_texture_hash: String,
        target_surface_id: SurfaceId,
        target_surface_epoch: u64,
        target_format: SurfaceFormat,
        width: u32,
        height: u32,
        acquired_surface_texture: bool,
        command_submission_id: String,
        present_result: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RendererRenderGraphPassMetric {
    pub schema_version: u32,
    pub pass_id: String,
    pub pass_kind: String,
    pub input: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_resources: Vec<String>,
    pub product_visible: bool,
    pub proof_or_readback: bool,
    pub duration_ms: f64,
    pub upload_bytes: u64,
    pub dirty_chunk_count: u32,
    pub queue_write_count: u32,
    pub draw_call_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RendererRenderGraphResourceMetric {
    pub schema_version: u32,
    pub resource_id: String,
    pub resource_kind: String,
    pub first_pass_index: u32,
    pub last_pass_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_pass_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumer_pass_ids: Vec<String>,
    pub product_visible: bool,
    pub proof_or_readback: bool,
    #[serde(default)]
    pub retained_epoch: u64,
    #[serde(default)]
    pub retained_dirty: bool,
    #[serde(default)]
    pub retained_reused: bool,
    #[serde(default)]
    pub last_used_frame_seq: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RendererRenderGraphScheduleDecisionMetric {
    pub schema_version: u32,
    pub resource_id: String,
    pub resource_kind: String,
    pub decision_kind: String,
    pub reason: String,
    pub retained_epoch: u64,
    pub product_visible: bool,
    pub proof_or_readback: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProductFrameGraphReport {
    pub schema_version: u32,
    pub owner: String,
    pub graph_kind: String,
    pub execution_kind: String,
    pub plan_hash: String,
    pub workload_hash: String,
    pub pass_count: u32,
    pub product_pass_count: u32,
    pub proof_pass_count: u32,
    pub resource_count: u32,
    pub product_resource_count: u32,
    pub resource_lifetime_hash: String,
    pub retained_resource_epoch_hash: String,
    pub retained_dirty_resource_count: u32,
    pub retained_reused_resource_count: u32,
    pub retained_state_resource_count: u32,
    pub scheduler_kind: String,
    pub schedule_hash: String,
    pub schedule_decision_count: u32,
    pub dirty_resource_decision_count: u32,
    pub reuse_resource_decision_count: u32,
    pub per_present_resource_decision_count: u32,
    #[serde(default)]
    pub passes: Vec<RendererRenderGraphPassMetric>,
    #[serde(default)]
    pub resources: Vec<RendererRenderGraphResourceMetric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule_decisions: Vec<RendererRenderGraphScheduleDecisionMetric>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameMetrics {
    pub frame_seq: u64,
    #[serde(default)]
    pub render_scene_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_frame_graph: Option<ProductFrameGraphReport>,
    #[serde(default)]
    pub document_scene_convert_ms: f64,
    #[serde(default)]
    pub document_scene_cache_hit: bool,
    #[serde(default)]
    pub document_scene_cache_entry_count: u32,
    pub draw_calls: u32,
    pub upload_bytes: u64,
    pub allocated_gpu_bytes: u64,
    pub dirty_upload_range_count: u32,
    pub dirty_upload_ranges: Vec<GpuUploadRangeMetric>,
    pub dirty_upload_chunk_count: u32,
    pub dirty_upload_chunk_ids: Vec<String>,
    pub buffer_reuse_count: u32,
    pub staging_wrap_count: u32,
    pub queue_write_count: u32,
    pub quad_cache_eviction_count: u32,
    pub quad_cache_hit: bool,
    pub quad_cache_entry_count: u32,
    #[serde(default)]
    pub scene_key_ms: f64,
    #[serde(default)]
    pub rect_vertices_ms: f64,
    #[serde(default)]
    pub asset_prepare_ms: f64,
    #[serde(default)]
    pub quad_batch_key_ms: f64,
    #[serde(default)]
    pub quad_upload_ms: f64,
    #[serde(default)]
    pub draw_pass_ms: f64,
    #[serde(default)]
    pub retained_metrics_ms: f64,
    #[serde(default)]
    pub text_render_ms: f64,
    pub visible_display_item_count: u32,
    pub rendered_rect_count: u32,
    pub rect_cap_hit: bool,
    pub visible_text_runs: u32,
    pub shaped_text_runs: u32,
    pub text_runs_shaped: u32,
    pub rendered_text_runs: u32,
    pub shaped_run_cache_hits: u32,
    pub shaped_run_cache_misses: u32,
    pub shaped_run_cache_evictions: u32,
    pub shaped_run_cache_entry_count: u32,
    pub shaped_run_cache_capacity: u32,
    pub shaped_run_cache_bytes: u64,
    pub missing_glyph_count: u32,
    pub glyph_atlas_prepare_count: u32,
    pub glyph_atlas_evictions_observed: u32,
    pub text_cap_hit: bool,
    pub glyphon_text_area_count: u32,
    pub color_only_rect_fallback: bool,
    pub preview_blocked_on_ipc_count: u64,
    pub asset_ref_count: u32,
    pub asset_refs: Vec<AssetRef>,
    pub asset_cache_hits: u32,
    pub asset_cache_misses: u32,
    pub asset_cache_evictions: u32,
    pub asset_cache_entry_count: u32,
    pub asset_cache_byte_count: u64,
    pub asset_cache_byte_cap: u64,
    pub asset_cache_byte_cap_hit: bool,
    pub asset_decode_count: u32,
    pub asset_raster_count: u32,
    pub asset_upload_count: u32,
    pub asset_upload_bytes: u64,
    pub asset_failure_diagnostics: Vec<String>,
    pub retained_chunk_count: u32,
    pub retained_chunk_hit_count: u32,
    pub retained_chunk_miss_count: u32,
    pub retained_chunk_reuse_count: u32,
    pub dirty_chunk_count: u32,
    #[serde(default)]
    pub retained_chunk_sample_count: u32,
    #[serde(default)]
    pub retained_chunk_inventory_truncated: bool,
    pub retained_chunks: Vec<RetainedRenderChunkMetric>,
}

const RENDER_SCENE_SOURCE_DOCUMENT_RENDER_SCENE: &str = "document-render-scene";
const RENDER_SCENE_SOURCE_INTERNAL_RENDER_SCENE: &str = "internal-render-scene";
const RENDER_SCENE_SOURCE_APP_OWNED_DOCUMENT_RENDER_SCENE: &str = "app-owned-document-render-scene";
const RENDER_SCENE_SOURCE_APP_OWNED_WORLD_SCENE_PROJECTION: &str =
    "app-owned-world-scene-projection";
const RETAINED_CHUNK_METRIC_SAMPLE_LIMIT: usize = 16;

pub type AssetRef = RenderAssetRef;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuUploadRangeMetric {
    pub offset: u64,
    pub size: u64,
    pub ring_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_chunk_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetainedRenderChunkMetric {
    pub id: String,
    pub node: DocumentNodeId,
    pub kind: String,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub transform: [f32; 6],
    pub style_identity: boon_document::ComputedStyleIdentity,
    pub dependency_set: Vec<String>,
    pub gpu_buffer_range: std::ops::Range<u32>,
    pub text_run_ids: Vec<String>,
    pub texture_asset_refs: Vec<String>,
    pub generation: u64,
    pub cache_status: String,
}

#[derive(Clone, Debug)]
struct RenderScene {
    viewport: Rect,
    items: Vec<RenderSceneItem>,
    quad_batches: Vec<QuadBatch>,
    rect_metrics: RectVertexMetrics,
    text_runs: Vec<RenderTextRun>,
}

#[derive(Clone, Debug)]
struct RenderSceneItem {
    node: DocumentNodeId,
    retained_chunk_id: String,
    source_kind: String,
    bounds: Rect,
    clip: Option<Rect>,
    transform: [f32; 6],
    style_identity: boon_document::ComputedStyleIdentity,
    dependency_set: Vec<String>,
    texture_asset_refs: Vec<String>,
    estimated_vertex_count: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct InternalRenderSceneCacheKey {
    scene_identity: String,
    width: u32,
    height: u32,
}

const VISIBLE_RENDERER_INTERNAL_SCENE_CACHE_CAP: usize = 64;

#[derive(Debug)]
pub struct RenderError {
    pub message: String,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RenderError {}

pub struct GlyphonTextMeasurer {
    service: GlyphonTextService,
    cache: BTreeMap<(String, TextMeasureStyleKey), boon_document::TextMetrics>,
}

impl GlyphonTextMeasurer {
    pub fn new() -> Self {
        Self {
            service: GlyphonTextService::new(),
            cache: BTreeMap::new(),
        }
    }
}

impl Default for GlyphonTextMeasurer {
    fn default() -> Self {
        Self::new()
    }
}

impl boon_document::TextMeasurer for GlyphonTextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32) -> boon_document::TextMetrics {
        let style_key = TextMeasureStyleKey {
            font_size_bits: font_size.to_bits(),
            line_height_bits: text_line_height(font_size).to_bits(),
            font_family: DOCUMENT_FONT_FAMILY.to_owned(),
            font_style: "normal".to_owned(),
            font_weight: "normal".to_owned(),
        };
        self.measure_with_key(text, style_key)
    }

    fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        style: &StyleMap,
    ) -> boon_document::TextMetrics {
        let font_size = font_size.max(1.0);
        let style_key = TextMeasureStyleKey {
            font_size_bits: font_size.to_bits(),
            line_height_bits: style_line_height(style, font_size).to_bits(),
            font_family: style_text(style, "font")
                .unwrap_or(DOCUMENT_FONT_FAMILY)
                .to_owned(),
            font_style: style_text(style, "font_style")
                .or_else(|| style_text(style, "style"))
                .unwrap_or("normal")
                .to_owned(),
            font_weight: style_text(style, "weight").unwrap_or("normal").to_owned(),
        };
        self.measure_with_key(text, style_key)
    }
}

impl GlyphonTextMeasurer {
    fn measure_with_key(
        &mut self,
        text: &str,
        style_key: TextMeasureStyleKey,
    ) -> boon_document::TextMetrics {
        let cache_key = (text.to_owned(), style_key.clone());
        if let Some(metrics) = self.cache.get(&cache_key) {
            return *metrics;
        }
        if text.is_empty() {
            return boon_document::TextMetrics {
                width: 0.0,
                height: 0.0,
            };
        }
        let metrics = self.service.measure_text(text, &style_key);
        self.cache.insert(cache_key, metrics);
        metrics
    }
}

struct GlyphonTextService {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl GlyphonTextService {
    fn new() -> Self {
        Self {
            font_system: editor_font_system(),
            swash_cache: SwashCache::new(),
        }
    }

    fn measure_text(
        &mut self,
        text: &str,
        style_key: &TextMeasureStyleKey,
    ) -> boon_document::TextMetrics {
        let font_size = f32::from_bits(style_key.font_size_bits).max(1.0);
        let line_height = f32::from_bits(style_key.line_height_bits).max(font_size);
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
        buffer.set_size(&mut self.font_system, None, Some(line_height));
        buffer.set_text(
            &mut self.font_system,
            text,
            &text_attrs(
                &style_key.font_family,
                text_font_style_value(&style_key.font_style),
                text_font_weight_value(&style_key.font_weight),
                [0, 0, 0, 255],
                "",
            ),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        boon_document::TextMetrics {
            width: shaped_line_width(&buffer).unwrap_or_default(),
            height: line_height,
        }
    }

    fn shape_run(&mut self, run: &TextRun) -> Buffer {
        shape_text_run(&mut self.font_system, run)
    }

    fn empty_custom_glyph_buffer(&mut self) -> Buffer {
        empty_custom_glyph_buffer(&mut self.font_system)
    }

    fn rotated_text_glyph(&mut self, run: &TextRun) -> Option<RotatedTextGlyph> {
        rotated_text_glyph_for_run(run, &mut self.font_system, &mut self.swash_cache)
    }

    fn editor_column_edges(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        font_family: &str,
        font_features: &str,
    ) -> Vec<f32> {
        let color = [217, 225, 242, 255];
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size.max(1.0), line_height.max(font_size.max(1.0))),
        );
        buffer.set_size(
            &mut self.font_system,
            None,
            Some(line_height.max(font_size.max(1.0))),
        );
        buffer.set_text(
            &mut self.font_system,
            text,
            &text_attrs(
                font_family,
                Style::Normal,
                Weight::NORMAL,
                color,
                font_features,
            ),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        let line_width = shaped_line_width(&buffer).unwrap_or_default();
        shaped_column_edges(text, &buffer, line_width)
    }

    fn editor_column_edges_for_style(
        &mut self,
        text: &str,
        style: &StyleMap,
        line_height: f32,
    ) -> Vec<f32> {
        let font_size = style_number(style, "size").unwrap_or(14.0).max(1.0);
        let line_height = line_height.max(font_size);
        let color = style_color_u8(style, "color").unwrap_or([217, 225, 242, 255]);
        let font_family = style_text(style, "font").unwrap_or(EDITOR_FONT_FAMILY);
        let font_features = style_text(style, "font_features").unwrap_or(EDITOR_FONT_FEATURES);
        let default_attrs = text_attrs(
            font_family,
            text_font_style(style),
            text_font_weight(style),
            color,
            font_features,
        );
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
        buffer.set_size(&mut self.font_system, None, Some(line_height));
        let rich_spans = rich_text_spans(style, text, color);
        if rich_spans.is_empty() {
            buffer.set_text(
                &mut self.font_system,
                text,
                &default_attrs,
                Shaping::Advanced,
                None,
            );
        } else {
            buffer.set_rich_text(
                &mut self.font_system,
                rich_spans.iter().map(|span| {
                    (
                        span.text.as_str(),
                        text_attrs(
                            font_family,
                            span.font_style,
                            span.font_weight,
                            span.color,
                            font_features,
                        ),
                    )
                }),
                &default_attrs,
                Shaping::Advanced,
                None,
            );
        }
        buffer.shape_until_scroll(&mut self.font_system, false);
        let line_width = shaped_line_width(&buffer).unwrap_or_default();
        shaped_column_edges(text, &buffer, line_width)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct TextMeasureStyleKey {
    font_size_bits: u32,
    line_height_bits: u32,
    font_family: String,
    font_style: String,
    font_weight: String,
}

pub fn required_backend_versions() -> (&'static str, &'static str) {
    (REQUIRED_WGPU_VERSION, REQUIRED_GLYPHON_VERSION)
}

#[derive(Clone, Debug)]
pub struct AppOwnedRenderSceneRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub scene: &'a DocumentRenderScene,
    pub render_identity_hash: &'a str,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub artifact_dir: &'a Path,
    pub artifact_label: &'a str,
}

#[derive(Clone, Debug)]
pub struct AppOwnedWorldSceneRenderRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub scene: &'a boon_scene_model::WorldScene,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub artifact_dir: &'a Path,
    pub artifact_label: &'a str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldScenePickReadbackProof {
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub capture_method: String,
    pub width: u32,
    pub height: u32,
    pub projected_pickable_item_count: usize,
    pub sampled_pick_id_count: usize,
    pub unique_pick_id_count: usize,
    pub sampled_pick_ids: Vec<u32>,
    pub render_identity_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneFeatureDepthReadbackProof {
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub capture_method: String,
    pub width: u32,
    pub height: u32,
    pub projected_instance_count: usize,
    pub sampled_feature_id_count: usize,
    pub unique_feature_id_count: usize,
    pub sampled_feature_ids: Vec<u64>,
    pub min_projection_depth: f32,
    pub max_projection_depth: f32,
    pub render_identity_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneDepthTargetProof {
    pub capture_method: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub sample_count: u32,
    pub clear_depth: f32,
    pub render_identity_hash: String,
    pub submitted_pass_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneDepthPixelSample {
    pub x: u32,
    pub y: u32,
    pub depth: f32,
    pub finite: bool,
    pub visible: bool,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneMeshDrawRange {
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
    pub instance_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneTriangleProbeSample {
    pub x: u32,
    pub y: u32,
    pub pixel_center: [f32; 2],
    pub coordinate_convention: String,
    pub candidate_count: usize,
    pub nearest_triangles: Vec<WorldSceneTriangleProbeCandidate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneTriangleProbeCandidate {
    pub triangle_index: u32,
    pub draw_range_index: Option<usize>,
    pub index_offsets: [u32; 3],
    pub vertex_indices: [u32; 3],
    pub clip_positions: [[f32; 4]; 3],
    pub ndc_positions: [[f32; 3]; 3],
    pub screen_positions: [[f32; 2]; 3],
    pub signed_edge_values: [f32; 3],
    pub edge_distances_px: [f32; 3],
    pub min_edge_distance_px: f32,
    pub barycentric: [f32; 3],
    pub inside_or_on: bool,
    pub feature_rgba: [u8; 4],
    pub pick_rgba: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneMeshPipelineProof {
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub feature_artifact_path: String,
    pub feature_artifact_sha256: String,
    pub pick_artifact_path: String,
    pub pick_artifact_sha256: String,
    pub normal_artifact_path: String,
    pub normal_artifact_sha256: String,
    pub capture_method: String,
    pub camera_projection_method: String,
    pub feature_capture_method: String,
    pub normal_capture_method: String,
    pub depth_capture_method: String,
    pub width: u32,
    pub height: u32,
    pub color_format: String,
    pub feature_format: String,
    pub normal_format: String,
    pub depth_format: String,
    pub primitive_topology: String,
    pub cull_mode: String,
    pub front_face: String,
    pub depth_compare: String,
    pub depth_write_enabled: bool,
    pub index_format: String,
    pub draw_command_encoding: String,
    pub draw_call_count: usize,
    pub draw_ranges: Vec<WorldSceneMeshDrawRange>,
    pub viewport_encoding: String,
    pub scissor_encoding: String,
    pub color_attachment_count: usize,
    pub depth_attachment_count: usize,
    pub visible_instance_count: usize,
    pub rendered_instance_count: usize,
    pub unsupported_geometry_count: usize,
    pub geometry_source: String,
    pub retained_chunk_count: usize,
    pub retained_chunk_vertex_count: usize,
    pub retained_chunk_index_count: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub vertex_buffer_checksum: u32,
    pub vertex_position_buffer_checksum: u32,
    pub vertex_color_buffer_checksum: u32,
    pub vertex_normal_buffer_checksum: u32,
    pub vertex_normal_buffer_bit_samples: Vec<[u32; 4]>,
    pub vertex_feature_buffer_checksum: u32,
    pub vertex_pick_buffer_checksum: u32,
    pub index_buffer_checksum: u32,
    pub camera_uniform_checksum: u32,
    pub nonblank_samples: usize,
    pub unique_rgba_values: usize,
    pub sampled_normal_pixel_count: usize,
    pub unique_normal_rgba_values: usize,
    pub sampled_depth_pixel_count: usize,
    pub visible_depth_pixel_count: usize,
    pub min_depth: f32,
    pub max_depth: f32,
    pub depth_pixel_samples: Vec<WorldSceneDepthPixelSample>,
    pub triangle_probe_samples: Vec<WorldSceneTriangleProbeSample>,
    pub sampled_feature_id_count: usize,
    pub unique_feature_id_count: usize,
    pub sampled_feature_ids: Vec<u64>,
    pub sampled_pick_id_count: usize,
    pub unique_pick_id_count: usize,
    pub sampled_pick_ids: Vec<u32>,
    pub hit_test_capture_method: String,
    pub hit_test_status: String,
    pub hit_test_x: u32,
    pub hit_test_y: u32,
    pub hit_test_feature_id: Option<u64>,
    pub hit_test_sampled_pixel_count: usize,
    pub small_pick_readback_status: String,
    pub small_pick_readback_capture_method: String,
    pub small_pick_readback_x: u32,
    pub small_pick_readback_y: u32,
    pub small_pick_readback_width: u32,
    pub small_pick_readback_height: u32,
    pub small_pick_readback_logical_bytes: u32,
    pub small_pick_readback_transfer_bytes: u32,
    pub small_pick_readback_rgba: [u8; 4],
    pub small_pick_readback_pick_id: Option<u32>,
    pub small_pick_readback_matches_full_pick: bool,
    pub render_identity_hash: String,
}

pub struct SurfaceRenderSceneRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,
    pub scene: &'a DocumentRenderScene,
    pub scene_identity: Option<&'a str>,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

pub struct SurfaceWorldSceneRenderRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,
    pub scene: &'a boon_scene_model::WorldScene,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneSurfaceMeshRenderProof {
    pub capture_method: String,
    pub camera_projection_method: String,
    pub width: u32,
    pub height: u32,
    pub color_format: String,
    pub depth_format: String,
    pub visible_instance_count: usize,
    pub rendered_instance_count: usize,
    pub unsupported_geometry_count: usize,
    pub geometry_source: String,
    pub retained_chunk_count: usize,
    pub retained_chunk_vertex_count: usize,
    pub retained_chunk_index_count: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub render_identity_hash: String,
    pub visible_surface_rendered: bool,
    pub visible_present_path: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSceneWebGpuRenderContract {
    pub status: String,
    pub wgpu_version: String,
    pub shader_language: String,
    pub required_features: String,
    pub required_limits_profile: String,
    pub surface_shader_sha256: String,
    pub app_owned_shader_sha256: String,
    pub vertex_entry_point: String,
    pub fragment_entry_point: String,
    pub vertex_stride_bytes: usize,
    pub vertex_attributes: Vec<String>,
    pub camera_uniform_size_bytes: usize,
    pub bind_group_count: usize,
    pub bind_group_0_binding_0: String,
    pub primitive_topology: String,
    pub index_format: String,
    pub color_formats: Vec<String>,
    pub depth_format: String,
    pub depth_compare: String,
    pub multisample_count: u32,
    pub buffer_usages: Vec<String>,
    pub texture_usages: Vec<String>,
    pub uses_push_constants: bool,
    pub uses_storage_buffers: bool,
    pub uses_storage_textures: bool,
    pub uses_texture_sampling: bool,
    pub uses_timestamp_queries: bool,
    pub uses_indirect_draw: bool,
    pub retained_mesh_source: String,
    pub retained_mesh_payload: String,
    pub retained_mesh_identity_fields: Vec<String>,
    pub retained_mesh_vertex_position_format: String,
    pub retained_mesh_normal_format: String,
    pub retained_mesh_feature_id_encoding: String,
    pub retained_mesh_index_type: String,
    pub retained_mesh_upload_path: String,
    pub browser_render_executed: bool,
    pub browser_render_status: String,
    pub parity_status: String,
}

pub fn world_scene_webgpu_render_contract() -> WorldSceneWebGpuRenderContract {
    let surface_vertex_entry = generated::shader_bindings::world_scene_surface_mesh::vs_main_entry(
        wgpu::VertexStepMode::Vertex,
    );
    let surface_fragment_entry =
        generated::shader_bindings::world_scene_surface_mesh::fs_main_entry([Some(
            wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
        )]);
    WorldSceneWebGpuRenderContract {
        status: "pass".to_owned(),
        wgpu_version: REQUIRED_WGPU_VERSION.to_owned(),
        shader_language: "WGSL".to_owned(),
        required_features: "empty".to_owned(),
        required_limits_profile: "downlevel_webgl2_defaults".to_owned(),
        surface_shader_sha256: generated_shader_wesl_hash("shaders/world_scene_surface_mesh.wesl"),
        app_owned_shader_sha256: generated_shader_wesl_hash(
            "shaders/world_scene_app_owned_mesh.wesl",
        ),
        vertex_entry_point: surface_vertex_entry.entry_point.to_owned(),
        fragment_entry_point: surface_fragment_entry.entry_point.to_owned(),
        vertex_stride_bytes: std::mem::size_of::<NativeGpuWorldMeshVertex>(),
        vertex_attributes: vec![
            "location0:Float32x4@0:world_position".to_owned(),
            "location1:Float32x4@16:color".to_owned(),
            "location2:Float32x4@32:normal_color".to_owned(),
            "location3:Float32x4@48:feature_color".to_owned(),
            "location4:Float32x4@64:pick_color".to_owned(),
        ],
        camera_uniform_size_bytes: std::mem::size_of::<NativeGpuWorldCameraUniform>(),
        bind_group_count: 1,
        bind_group_0_binding_0: "uniform-buffer:vertex-stage:camera".to_owned(),
        primitive_topology: "TriangleList".to_owned(),
        index_format: "Uint32".to_owned(),
        color_formats: vec![
            "surface-host-format".to_owned(),
            "Rgba8UnormSrgb".to_owned(),
            "Rgba8Unorm".to_owned(),
        ],
        depth_format: "Depth32Float".to_owned(),
        depth_compare: "LessEqual".to_owned(),
        multisample_count: 1,
        buffer_usages: vec![
            "VERTEX|COPY_DST".to_owned(),
            "INDEX|COPY_DST".to_owned(),
            "UNIFORM|COPY_DST".to_owned(),
        ],
        texture_usages: vec![
            "RENDER_ATTACHMENT".to_owned(),
            "RENDER_ATTACHMENT|COPY_SRC".to_owned(),
        ],
        uses_push_constants: false,
        uses_storage_buffers: false,
        uses_storage_textures: false,
        uses_texture_sampling: false,
        uses_timestamp_queries: false,
        uses_indirect_draw: false,
        retained_mesh_source: "boon_scene_model::SurfaceChunk".to_owned(),
        retained_mesh_payload: "SurfaceRepresentation::IndexedMesh".to_owned(),
        retained_mesh_identity_fields: vec![
            "SurfaceChunkId.geometry".to_owned(),
            "SurfaceChunkId.spatial_key".to_owned(),
            "SurfaceChunkId.lod".to_owned(),
            "SurfaceChunkId.tolerance_class".to_owned(),
            "SurfaceChunk.geometry_revision".to_owned(),
        ],
        retained_mesh_vertex_position_format: "Float32x3-expanded-to-location0-Float32x4"
            .to_owned(),
        retained_mesh_normal_format: "Float32x3-packed-as-normal-color-location2-Float32x4"
            .to_owned(),
        retained_mesh_feature_id_encoding:
            "FeatureId-low-u32-Rgba8Unorm-target-plus-PickId-u32-Rgba8Unorm-target".to_owned(),
        retained_mesh_index_type: "u32".to_owned(),
        retained_mesh_upload_path: "VERTEX|COPY_DST plus INDEX|COPY_DST".to_owned(),
        browser_render_executed: false,
        browser_render_status: "not-implemented".to_owned(),
        parity_status: "contract-only-browser-webgpu-visual-parity-not-executed".to_owned(),
    }
}

type TextureBindGroup = generated::shader_bindings::native_gpu_rect::WgpuBindGroup0;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AssetTextureKey {
    url: String,
    asset_ref: RenderAssetRef,
    width: u32,
    height: u32,
}

impl AssetTextureKey {
    fn asset_ref(&self) -> AssetRef {
        self.asset_ref.clone()
    }

    fn texture_byte_count(&self) -> u64 {
        u64::from(self.width.max(1))
            .saturating_mul(u64::from(self.height.max(1)))
            .saturating_mul(4)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum QuadTexture {
    Solid,
    Asset(AssetTextureKey),
}

#[derive(Clone, Debug)]
struct QuadBatch {
    retained_chunk_id: String,
    texture: QuadTexture,
    vertices: Vec<NativeGpuQuadVertex>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct NativeGpuQuadVertex {
    position: [f32; 2],
    color: u32,
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct NativeGpuWorldMeshVertex {
    world_position: [f32; 4],
    color: [f32; 4],
    normal_color: [f32; 4],
    feature_color: [f32; 4],
    pick_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct NativeGpuWorldCameraUniform {
    clip_from_world_rows: [[f32; 4]; 4],
}

#[derive(Clone)]
struct GpuQuadBatch {
    texture: QuadTexture,
    vertex_count: u32,
    vertex_buffer: wgpu::Buffer,
    byte_range: std::ops::Range<u64>,
    ring_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GpuQuadDrawRange {
    texture: QuadTexture,
    vertex_count: u32,
    byte_range: std::ops::Range<u64>,
    ring_generation: u64,
    first_batch_index: usize,
    source_batch_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct QuadBatchCacheKey {
    retained_chunk_id: String,
    texture: QuadTexture,
    vertex_count: u32,
    content_key: u64,
}

struct CachedGpuQuadBatch {
    vertex_count: u32,
    vertex_buffer: wgpu::Buffer,
    byte_range: std::ops::Range<u64>,
    ring_generation: u64,
}

fn coalesced_gpu_quad_draw_ranges(gpu_batches: &[GpuQuadBatch]) -> Vec<GpuQuadDrawRange> {
    coalesced_gpu_quad_draw_ranges_from_parts(gpu_batches.iter().enumerate().map(
        |(index, batch)| GpuQuadDrawRange {
            texture: batch.texture.clone(),
            vertex_count: batch.vertex_count,
            byte_range: batch.byte_range.clone(),
            ring_generation: batch.ring_generation,
            first_batch_index: index,
            source_batch_count: 1,
        },
    ))
}

fn coalesced_gpu_quad_draw_ranges_from_parts(
    ranges: impl IntoIterator<Item = GpuQuadDrawRange>,
) -> Vec<GpuQuadDrawRange> {
    let mut coalesced = Vec::<GpuQuadDrawRange>::new();
    for range in ranges {
        if let Some(previous) = coalesced.last_mut()
            && previous.texture == range.texture
            && previous.ring_generation == range.ring_generation
            && previous.byte_range.end == range.byte_range.start
        {
            previous.vertex_count = previous.vertex_count.saturating_add(range.vertex_count);
            previous.byte_range.end = range.byte_range.end;
            previous.source_batch_count = previous
                .source_batch_count
                .saturating_add(range.source_batch_count);
            continue;
        }
        coalesced.push(range);
    }
    coalesced
}

const NATIVE_GPU_QUAD_VERTEX_STRIDE: wgpu::BufferAddress =
    std::mem::size_of::<NativeGpuQuadVertex>() as wgpu::BufferAddress;
const NATIVE_GPU_QUAD_VERTEX_POSITION_OFFSET: wgpu::BufferAddress = 0;
const NATIVE_GPU_QUAD_VERTEX_COLOR_OFFSET: wgpu::BufferAddress = 8;
const NATIVE_GPU_QUAD_VERTEX_UV_OFFSET: wgpu::BufferAddress = 12;
const QUAD_UPLOAD_RING_MIN_BYTES: u64 = 256 * 1024;
const QUAD_UPLOAD_RING_GROW_ON_WRAP_MIN_BYTES: u64 = 4 * 1024 * 1024;
const PREPARED_QUAD_CACHE_CAP: usize = 128;
const QUAD_UPLOAD_RING_MAX_BYTES: u64 = 64 * 1024 * 1024;
const QUAD_UPLOAD_RING_ALIGNMENT: u64 = 4;
const NATIVE_GPU_QUAD_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: NATIVE_GPU_QUAD_VERTEX_POSITION_OFFSET,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32,
        offset: NATIVE_GPU_QUAD_VERTEX_COLOR_OFFSET,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: NATIVE_GPU_QUAD_VERTEX_UV_OFFSET,
        shader_location: 2,
    },
];

fn native_gpu_quad_vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: NATIVE_GPU_QUAD_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &NATIVE_GPU_QUAD_VERTEX_ATTRIBUTES,
    }
}

pub fn native_gpu_quad_vertex_layout_contract() -> serde_json::Value {
    let layout = native_gpu_quad_vertex_buffer_layout();
    let generated = generated::shader_bindings::native_gpu_rect::vs_main_entry(
        wgpu::VertexStepMode::Vertex,
        wgpu::VertexStepMode::Vertex,
        wgpu::VertexStepMode::Vertex,
    );
    let generated_attributes = generated
        .buffers
        .iter()
        .flat_map(|buffer| buffer.attributes.iter())
        .map(|attribute| {
            serde_json::json!({
                "shader_location": attribute.shader_location,
                "offset": attribute.offset,
                "format": format!("{:?}", attribute.format),
            })
        })
        .collect::<Vec<_>>();
    let generated_shader_inputs = generated
        .buffers
        .iter()
        .flat_map(|buffer| buffer.attributes.iter())
        .map(|attribute| {
            serde_json::json!({
                "shader_location": attribute.shader_location,
                "format": format!("{:?}", attribute.format),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "host_struct": "NativeGpuQuadVertex",
        "pod": true,
        "size": std::mem::size_of::<NativeGpuQuadVertex>(),
        "align": std::mem::align_of::<NativeGpuQuadVertex>(),
        "buffer_count": 1,
        "array_stride": layout.array_stride,
        "step_mode": format!("{:?}", layout.step_mode),
        "attributes": layout
            .attributes
            .iter()
            .map(|attribute| {
                serde_json::json!({
                    "shader_location": attribute.shader_location,
                    "offset": attribute.offset,
                    "format": format!("{:?}", attribute.format),
                })
            })
            .collect::<Vec<_>>(),
        "generated_shader_attributes": generated_attributes,
        "generated_shader_inputs": generated_shader_inputs,
    })
}

#[derive(Default)]
struct QuadUploadRing {
    buffer: Option<wgpu::Buffer>,
    capacity_bytes: u64,
    cursor_bytes: u64,
    generation: u64,
}

#[derive(Default)]
struct QuadUploadStats {
    allocated_gpu_bytes: u64,
    upload_bytes: u64,
    dirty_upload_ranges: Vec<GpuUploadRangeMetric>,
    staging_wrap_count: u32,
    queue_write_count: u32,
    cache_eviction_count: u32,
    invalidated_cached_ranges: bool,
}

impl QuadUploadRing {
    fn begin_frame(
        &mut self,
        device: &wgpu::Device,
        frame_reservation_size: u64,
        dirty_reservation_size: u64,
        mut quad_buffers: Option<&mut BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>>,
    ) -> Result<QuadUploadStats, RenderError> {
        let mut stats = QuadUploadStats::default();
        if dirty_reservation_size == 0 {
            return Ok(stats);
        }
        if frame_reservation_size > QUAD_UPLOAD_RING_MAX_BYTES {
            return Err(RenderError {
                message: format!(
                    "native GPU quad upload frame reservation of {frame_reservation_size} bytes exceeds ring cap of {QUAD_UPLOAD_RING_MAX_BYTES} bytes"
                ),
            });
        }
        let would_wrap =
            self.cursor_bytes.saturating_add(dirty_reservation_size) > self.capacity_bytes;
        let cached_range_count = quad_buffers
            .as_ref()
            .map_or(0, |quad_buffers| quad_buffers.len());
        let should_grow_to_preserve_cache = self.buffer.is_some()
            && would_wrap
            && cached_range_count > 0
            && self.capacity_bytes < QUAD_UPLOAD_RING_MAX_BYTES;
        let needs_grow = self.buffer.is_none()
            || dirty_reservation_size > self.capacity_bytes
            || (would_wrap && frame_reservation_size > self.capacity_bytes)
            || should_grow_to_preserve_cache;
        let needs_wrap = !needs_grow && would_wrap;
        if needs_grow {
            let required_capacity = QUAD_UPLOAD_RING_MIN_BYTES
                .max(QUAD_UPLOAD_RING_GROW_ON_WRAP_MIN_BYTES.min(QUAD_UPLOAD_RING_MAX_BYTES))
                .max(self.capacity_bytes.saturating_mul(2))
                .max(frame_reservation_size.next_power_of_two())
                .min(QUAD_UPLOAD_RING_MAX_BYTES);
            self.buffer = Some(create_quad_upload_ring_buffer(device, required_capacity));
            self.capacity_bytes = required_capacity;
            self.cursor_bytes = 0;
            self.generation = self.generation.saturating_add(1);
            stats.allocated_gpu_bytes = stats.allocated_gpu_bytes.saturating_add(required_capacity);
            stats.invalidated_cached_ranges = true;
            if let Some(quad_buffers) = quad_buffers.as_deref_mut() {
                stats.cache_eviction_count = stats
                    .cache_eviction_count
                    .saturating_add(quad_buffers.len() as u32);
                quad_buffers.clear();
            }
        } else if needs_wrap {
            self.cursor_bytes = 0;
            self.generation = self.generation.saturating_add(1);
            stats.staging_wrap_count = stats.staging_wrap_count.saturating_add(1);
            stats.invalidated_cached_ranges = true;
            if let Some(quad_buffers) = quad_buffers.as_deref_mut() {
                stats.cache_eviction_count = stats
                    .cache_eviction_count
                    .saturating_add(quad_buffers.len() as u32);
                quad_buffers.clear();
            }
        }
        Ok(stats)
    }

    fn upload_reserved(
        &mut self,
        queue: &wgpu::Queue,
        vertex_bytes: &[u8],
        vertex_count: u32,
        retained_chunk_id: Option<String>,
    ) -> Result<(CachedGpuQuadBatch, QuadUploadStats), RenderError> {
        let byte_count = vertex_bytes.len() as u64;
        let reservation_size = quad_upload_reservation_size(byte_count);
        if self.buffer.is_none() {
            return Err(RenderError {
                message: "native GPU quad upload ring was not reserved before upload".to_owned(),
            });
        }
        if self.cursor_bytes.saturating_add(reservation_size) > self.capacity_bytes {
            return Err(RenderError {
                message: format!(
                    "native GPU quad upload reservation overflow: cursor={}, reservation={}, capacity={}",
                    self.cursor_bytes, reservation_size, self.capacity_bytes
                ),
            });
        }
        let mut stats = QuadUploadStats::default();
        let offset = self.cursor_bytes;
        let end = offset.saturating_add(byte_count);
        let buffer = self
            .buffer
            .as_ref()
            .expect("quad upload ring buffer allocated")
            .clone();
        queue.write_buffer(&buffer, offset, vertex_bytes);
        self.cursor_bytes = self.cursor_bytes.saturating_add(reservation_size);
        stats.upload_bytes = stats.upload_bytes.saturating_add(byte_count);
        stats.queue_write_count = stats.queue_write_count.saturating_add(1);
        stats.dirty_upload_ranges.push(GpuUploadRangeMetric {
            offset,
            size: byte_count,
            ring_generation: self.generation,
            retained_chunk_id,
        });
        Ok((
            CachedGpuQuadBatch {
                vertex_count,
                vertex_buffer: buffer,
                byte_range: offset..end,
                ring_generation: self.generation,
            },
            stats,
        ))
    }

    fn cached_batch_is_valid(&self, batch: &CachedGpuQuadBatch) -> bool {
        self.buffer.is_some()
            && batch.ring_generation == self.generation
            && upload_range_is_valid(&batch.byte_range, batch.vertex_count, self.capacity_bytes)
    }

    fn gpu_batch_is_valid(&self, batch: &GpuQuadBatch) -> bool {
        self.buffer.is_some()
            && batch.ring_generation == self.generation
            && upload_range_is_valid(&batch.byte_range, batch.vertex_count, self.capacity_bytes)
    }

    fn prepared_cache_is_valid(&self, cache: &PreparedQuadCache) -> bool {
        self.buffer.is_some()
            && cache.ring_generation == self.generation
            && cache
                .gpu_batches
                .iter()
                .all(|batch| self.gpu_batch_is_valid(batch))
    }
}

fn create_quad_upload_ring_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-quad-upload-ring"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn align_u64(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn quad_upload_reservation_size(byte_count: u64) -> u64 {
    align_u64(byte_count, QUAD_UPLOAD_RING_ALIGNMENT)
}

fn upload_range_is_valid(range: &std::ops::Range<u64>, vertex_count: u32, capacity: u64) -> bool {
    range.start <= range.end
        && range.end <= capacity
        && range.start % QUAD_UPLOAD_RING_ALIGNMENT == 0
        && range.end.saturating_sub(range.start)
            == u64::from(vertex_count).saturating_mul(NATIVE_GPU_QUAD_VERTEX_STRIDE)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PreparedQuadCacheKey {
    scene_key: u64,
    width: u32,
    height: u32,
}

struct PreparedQuadCache {
    ring_generation: u64,
    gpu_batches: Vec<GpuQuadBatch>,
    rect_metrics: RectVertexMetrics,
}

#[derive(Debug, Default)]
struct QuadBuilder {
    batches: Vec<QuadBatch>,
    retained_chunk_id: String,
}

impl QuadBuilder {
    fn set_retained_chunk_id(&mut self, retained_chunk_id: &str) {
        self.retained_chunk_id.clear();
        self.retained_chunk_id.push_str(retained_chunk_id);
    }

    fn push_triangle(
        &mut self,
        texture: QuadTexture,
        points: [[f32; 2]; 3],
        uvs: [[f32; 2]; 3],
        surface_width: f32,
        surface_height: f32,
        color: [f32; 4],
    ) {
        let batch = if self.batches.last().is_some_and(|batch| {
            batch.texture == texture && batch.retained_chunk_id == self.retained_chunk_id
        }) {
            self.batches.last_mut().unwrap()
        } else {
            self.batches.push(QuadBatch {
                retained_chunk_id: self.retained_chunk_id.clone(),
                texture,
                vertices: Vec::new(),
            });
            self.batches.last_mut().unwrap()
        };
        let color = pack_rgba8_from_f32(color);
        for (point, uv) in points.into_iter().zip(uvs) {
            batch.vertices.push(NativeGpuQuadVertex {
                position: [
                    (point[0] / surface_width.max(1.0))
                        .mul_add(2.0, -1.0)
                        .clamp(-1.0, 1.0),
                    (1.0 - (point[1] / surface_height.max(1.0)) * 2.0).clamp(-1.0, 1.0),
                ],
                color,
                uv,
            });
        }
    }
}

struct TextureState {
    sampler: wgpu::Sampler,
    _white_texture: wgpu::Texture,
    _white_view: wgpu::TextureView,
    white_bind_group: TextureBindGroup,
    assets: BTreeMap<AssetTextureKey, GpuTextureAsset>,
    cached_asset_bytes: u64,
}

struct GpuTextureAsset {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: TextureBindGroup,
    byte_count: u64,
}

#[derive(Clone, Debug)]
struct AssetFrameMetrics {
    refs: BTreeMap<String, AssetRef>,
    cache_hits: u32,
    cache_misses: u32,
    cache_evictions: u32,
    cache_entry_count: u32,
    cache_byte_count: u64,
    cache_byte_cap: u64,
    cache_byte_cap_hit: bool,
    decode_count: u32,
    raster_count: u32,
    upload_count: u32,
    upload_bytes: u64,
    failure_diagnostics: Vec<String>,
}

impl Default for AssetFrameMetrics {
    fn default() -> Self {
        Self {
            refs: BTreeMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            cache_evictions: 0,
            cache_entry_count: 0,
            cache_byte_count: 0,
            cache_byte_cap: MAX_CACHED_ASSET_TEXTURE_BYTES,
            cache_byte_cap_hit: false,
            decode_count: 0,
            raster_count: 0,
            upload_count: 0,
            upload_bytes: 0,
            failure_diagnostics: Vec::new(),
        }
    }
}

impl AssetFrameMetrics {
    fn finish(mut self, state: &TextureState) -> Self {
        self.cache_entry_count = state.assets.len() as u32;
        self.cache_byte_count = state.assets.values().map(|asset| asset.byte_count).sum();
        self.cache_byte_cap = MAX_CACHED_ASSET_TEXTURE_BYTES;
        self.cache_byte_cap_hit = self.cache_byte_count >= MAX_CACHED_ASSET_TEXTURE_BYTES;
        self
    }

    fn asset_refs(&self) -> Vec<AssetRef> {
        self.refs.values().cloned().collect()
    }
}

impl TextureState {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("boon-native-gpu-texture-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let (white_texture, white_view) = upload_rgba_texture(
            device,
            queue,
            "boon-native-gpu-white-texture",
            1,
            1,
            &[255; 4],
        );
        let white_bind_group = TextureBindGroup::from_bindings(
            device,
            generated::shader_bindings::native_gpu_rect::WgpuBindGroup0Entries::new(
                generated::shader_bindings::native_gpu_rect::WgpuBindGroup0EntriesParams {
                    texture_sampler: &sampler,
                    texture_image: &white_view,
                },
            ),
        );
        Self {
            sampler,
            _white_texture: white_texture,
            _white_view: white_view,
            white_bind_group,
            assets: BTreeMap::new(),
            cached_asset_bytes: 0,
        }
    }

    fn prepare_assets(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[QuadBatch],
    ) -> Result<AssetFrameMetrics, RenderError> {
        let mut metrics = AssetFrameMetrics::default();
        for batch in batches {
            let QuadTexture::Asset(key) = &batch.texture else {
                continue;
            };
            let asset_ref = key.asset_ref();
            metrics.refs.insert(asset_ref.id.clone(), asset_ref);
            if self.assets.contains_key(key) {
                metrics.cache_hits = metrics.cache_hits.saturating_add(1);
                continue;
            }
            metrics.cache_misses = metrics.cache_misses.saturating_add(1);
            let pixels = match rasterize_svg_data_url(&key.url, key.width, key.height) {
                Ok(pixels) => pixels,
                Err(error) => {
                    metrics.failure_diagnostics.push(error.to_string());
                    return Err(error);
                }
            };
            metrics.decode_count = metrics.decode_count.saturating_add(1);
            metrics.raster_count = metrics.raster_count.saturating_add(1);
            let (texture, view) = upload_rgba_texture(
                device,
                queue,
                "boon-native-gpu-asset-texture",
                key.width,
                key.height,
                &pixels,
            );
            let byte_count = key.texture_byte_count();
            if self.cached_asset_bytes.saturating_add(byte_count) > MAX_CACHED_ASSET_TEXTURE_BYTES
                && !self.assets.is_empty()
            {
                metrics.cache_evictions = metrics
                    .cache_evictions
                    .saturating_add(self.assets.len() as u32);
                self.assets.clear();
                self.cached_asset_bytes = 0;
            }
            let bind_group = TextureBindGroup::from_bindings(
                device,
                generated::shader_bindings::native_gpu_rect::WgpuBindGroup0Entries::new(
                    generated::shader_bindings::native_gpu_rect::WgpuBindGroup0EntriesParams {
                        texture_sampler: &self.sampler,
                        texture_image: &view,
                    },
                ),
            );
            self.assets.insert(
                key.clone(),
                GpuTextureAsset {
                    _texture: texture,
                    _view: view,
                    bind_group,
                    byte_count,
                },
            );
            self.cached_asset_bytes = self.cached_asset_bytes.saturating_add(byte_count);
            metrics.upload_count = metrics.upload_count.saturating_add(1);
            metrics.upload_bytes = metrics.upload_bytes.saturating_add(byte_count);
        }
        Ok(metrics.finish(self))
    }

    fn cached_asset_metrics<'a>(
        &self,
        textures: impl IntoIterator<Item = &'a QuadTexture>,
    ) -> AssetFrameMetrics {
        let mut metrics = AssetFrameMetrics::default();
        for texture in textures {
            let QuadTexture::Asset(key) = texture else {
                continue;
            };
            let asset_ref = key.asset_ref();
            metrics.refs.insert(asset_ref.id.clone(), asset_ref);
            if self.assets.contains_key(key) {
                metrics.cache_hits = metrics.cache_hits.saturating_add(1);
            } else {
                metrics.cache_misses = metrics.cache_misses.saturating_add(1);
                metrics.failure_diagnostics.push(format!(
                    "asset texture {} was referenced by prepared quads but missing from cache",
                    key.asset_ref().id
                ));
            }
        }
        metrics.finish(self)
    }

    fn bind_group_for(&self, texture: &QuadTexture) -> Option<&TextureBindGroup> {
        match texture {
            QuadTexture::Solid => Some(&self.white_bind_group),
            QuadTexture::Asset(key) => self.assets.get(key).map(|asset| &asset.bind_group),
        }
    }
}

fn upload_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn rasterize_svg_data_url(url: &str, width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let svg = decode_svg_data_url(url).ok_or_else(|| RenderError {
        message: "native GPU asset URL is not a supported SVG data URL".to_owned(),
    })?;
    let options = resvg::usvg::Options::default();
    let tree =
        resvg::usvg::Tree::from_data(svg.as_bytes(), &options).map_err(|error| RenderError {
            message: format!("parse SVG data URL asset: {error}"),
        })?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| RenderError {
            message: format!("allocate SVG raster target {width}x{height}"),
        })?;
    let svg_size = tree.size();
    let scale_x = width.max(1) as f32 / svg_size.width().max(1.0);
    let scale_y = height.max(1) as f32 / svg_size.height().max(1.0);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.take())
}

fn decode_svg_data_url(url: &str) -> Option<String> {
    let (metadata, data) = url.split_once(',')?;
    let metadata = metadata.trim().to_ascii_lowercase();
    if !metadata.starts_with("data:image/svg+xml") || metadata.contains(";base64") {
        return None;
    }
    percent_decode_utf8(data)
}

fn percent_decode_utf8(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let input = value.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = input.get(index + 1).copied()?;
            let low = input.get(index + 2).copied()?;
            bytes.push(hex_pair(high, low)?);
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_digit(high)? * 16 + hex_digit(low)?)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub struct VisibleLayoutRenderer {
    pipeline: wgpu::RenderPipeline,
    frame_seq: u64,
    text: GlyphonTextState,
    textures: TextureState,
    internal_scene_cache: BTreeMap<InternalRenderSceneCacheKey, RenderScene>,
    quad_buffers: BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>,
    quad_upload_ring: QuadUploadRing,
    prepared_quads: BTreeMap<PreparedQuadCacheKey, PreparedQuadCache>,
    previous_chunk_ids: BTreeSet<String>,
    product_frame_graph: ProductFrameGraphState,
}

impl VisibleLayoutRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = generated::shader_bindings::ShaderEntry::NativeGpuRect;
        let module = shader.create_shader_module_embed_source(device);
        let layout = shader.create_pipeline_layout(device);
        let split_vertex_entry = generated::shader_bindings::native_gpu_rect::vs_main_entry(
            wgpu::VertexStepMode::Vertex,
            wgpu::VertexStepMode::Vertex,
            wgpu::VertexStepMode::Vertex,
        );
        let vertex_entry = generated::shader_bindings::native_gpu_rect::VertexEntry {
            entry_point: split_vertex_entry.entry_point,
            buffers: [native_gpu_quad_vertex_buffer_layout()],
            constants: split_vertex_entry.constants,
        };
        let fragment_entry = generated::shader_bindings::native_gpu_rect::fs_main_entry([Some(
            wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
        )]);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("boon-native-gpu-visible-rect-pipeline"),
            layout: Some(&layout),
            vertex: generated::shader_bindings::native_gpu_rect::vertex_state(
                &module,
                &vertex_entry,
            ),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(generated::shader_bindings::native_gpu_rect::fragment_state(
                &module,
                &fragment_entry,
            )),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            frame_seq: 0,
            text: GlyphonTextState::new(device, queue, format),
            textures: TextureState::new(device, queue),
            internal_scene_cache: BTreeMap::new(),
            quad_buffers: BTreeMap::new(),
            quad_upload_ring: QuadUploadRing::default(),
            prepared_quads: BTreeMap::new(),
            previous_chunk_ids: BTreeSet::new(),
            product_frame_graph: ProductFrameGraphState::default(),
        }
    }

    pub fn encode_scene(
        &mut self,
        request: SurfaceRenderSceneRequest<'_>,
    ) -> Result<FrameMetrics, RenderError> {
        self.frame_seq += 1;
        encode_render_scene_to_surface_with_pipeline(
            request,
            &self.pipeline,
            &mut self.text,
            &mut self.textures,
            &mut self.internal_scene_cache,
            &mut self.quad_buffers,
            &mut self.quad_upload_ring,
            &mut self.prepared_quads,
            &mut self.previous_chunk_ids,
            &mut self.product_frame_graph,
            self.frame_seq,
        )
    }
}

pub fn encode_render_scene_to_surface(
    request: SurfaceRenderSceneRequest<'_>,
) -> Result<FrameMetrics, RenderError> {
    let mut renderer = VisibleLayoutRenderer::new(request.device, request.queue, request.format);
    renderer.encode_scene(request)
}

fn internal_render_scene_cache_key(
    scene: &DocumentRenderScene,
    scene_identity: Option<&str>,
    width: u32,
    height: u32,
) -> InternalRenderSceneCacheKey {
    InternalRenderSceneCacheKey {
        scene_identity: scene_identity
            .map(str::to_owned)
            .unwrap_or_else(|| document_render_scene_fallback_identity(scene)),
        width,
        height,
    }
}

fn document_render_scene_fallback_identity(scene: &DocumentRenderScene) -> String {
    format!(
        "document-render-scene-ptr:{:p}:items:{}:primitives:{}:batches:{}:text:{}:visible:{}:rects:{}",
        scene,
        scene.items.len(),
        scene.visual_primitives.len(),
        scene.quad_batches.len(),
        scene.text_runs.len(),
        scene.metrics.visible_source_item_count,
        scene.metrics.rendered_rect_count,
    )
}

fn evict_internal_scene_cache_if_needed(
    cache: &mut BTreeMap<InternalRenderSceneCacheKey, RenderScene>,
) {
    if cache.len() >= VISIBLE_RENDERER_INTERNAL_SCENE_CACHE_CAP
        && let Some(oldest_key) = cache.keys().next().cloned()
    {
        cache.remove(&oldest_key);
    }
}

fn encode_render_scene_to_surface_with_pipeline(
    request: SurfaceRenderSceneRequest<'_>,
    pipeline: &wgpu::RenderPipeline,
    text: &mut GlyphonTextState,
    textures: &mut TextureState,
    internal_scene_cache: &mut BTreeMap<InternalRenderSceneCacheKey, RenderScene>,
    quad_buffers: &mut BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>,
    quad_upload_ring: &mut QuadUploadRing,
    prepared_quads: &mut BTreeMap<PreparedQuadCacheKey, PreparedQuadCache>,
    previous_chunk_ids: &mut BTreeSet<String>,
    product_frame_graph: &mut ProductFrameGraphState,
    frame_seq: u64,
) -> Result<FrameMetrics, RenderError> {
    let width = request.width.clamp(1, 1920);
    let height = request.height.clamp(1, 1080);
    let convert_started = Instant::now();
    let cache_key =
        internal_render_scene_cache_key(request.scene, request.scene_identity, width, height);
    let cache_hit = internal_scene_cache.contains_key(&cache_key);
    if !cache_hit {
        evict_internal_scene_cache_if_needed(internal_scene_cache);
        internal_scene_cache.insert(
            cache_key.clone(),
            render_scene_from_document_scene(request.scene, width, height),
        );
    }
    let cache_entry_count = internal_scene_cache.len() as u32;
    let scene = internal_scene_cache
        .get(&cache_key)
        .ok_or_else(|| RenderError {
            message: "internal render scene cache was not initialized".to_owned(),
        })?;
    let document_scene_convert_ms = convert_started.elapsed().as_secs_f64() * 1000.0;
    let mut metrics = encode_internal_scene_to_surface(
        SceneEncodeRequest {
            device: request.device,
            queue: request.queue,
            encoder: request.encoder,
            view: request.view,
            width,
            height,
        },
        scene,
        pipeline,
        text,
        textures,
        quad_buffers,
        quad_upload_ring,
        prepared_quads,
        previous_chunk_ids,
        product_frame_graph,
        render_scene_supplied_cache_key(request.scene_identity, width, height),
        frame_seq,
    )?;
    metrics.document_scene_convert_ms = document_scene_convert_ms;
    metrics.document_scene_cache_hit = cache_hit;
    metrics.document_scene_cache_entry_count = cache_entry_count;
    metrics.render_scene_source = RENDER_SCENE_SOURCE_DOCUMENT_RENDER_SCENE.to_owned();
    Ok(metrics)
}

struct SceneEncodeRequest<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Default)]
struct RendererRenderGraphPassStats {
    upload_bytes: u64,
    dirty_chunk_count: u32,
    queue_write_count: u32,
    draw_call_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum ProductFrameGraphResourceId {
    RenderScene,
    RenderSceneItems,
    SceneCacheKey,
    RetainedGpuBuffers,
    ColorTarget,
    FrameMetrics,
    TextRuns,
    NoTextRuns,
}

impl ProductFrameGraphResourceId {
    fn label(self) -> &'static str {
        match self {
            Self::RenderScene => "RenderScene",
            Self::RenderSceneItems => "RenderSceneItems",
            Self::SceneCacheKey => "SceneCacheKey",
            Self::RetainedGpuBuffers => "RetainedGpuBuffers",
            Self::ColorTarget => "ColorTarget",
            Self::FrameMetrics => "FrameMetrics",
            Self::TextRuns => "TextRuns",
            Self::NoTextRuns => "NoTextRuns",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum ProductFrameGraphPassId {
    SceneKey,
    QuadPrepareUpload,
    UiDraw,
    RetainedMetrics,
    TextDraw,
}

impl ProductFrameGraphPassId {
    fn label(self) -> &'static str {
        match self {
            Self::SceneKey => "renderer-scene-key",
            Self::QuadPrepareUpload => "renderer-quad-prepare-upload",
            Self::UiDraw => "renderer-ui-draw",
            Self::RetainedMetrics => "renderer-retained-metrics",
            Self::TextDraw => "renderer-text-draw",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::SceneKey => "scene_identity",
            Self::QuadPrepareUpload => "retained_quad_prepare_and_dirty_upload",
            Self::UiDraw => "ui_draw_pass",
            Self::RetainedMetrics => "retained_metrics",
            Self::TextDraw => "text_draw_pass",
        }
    }
}

#[derive(Clone, Debug)]
struct ProductFrameGraphPass {
    pass_id: ProductFrameGraphPassId,
    input: ProductFrameGraphResourceId,
    output: ProductFrameGraphResourceId,
    product_visible: bool,
    proof_or_readback: bool,
}

impl ProductFrameGraphPass {
    fn product(
        pass_id: ProductFrameGraphPassId,
        input: ProductFrameGraphResourceId,
        output: ProductFrameGraphResourceId,
    ) -> Self {
        Self {
            pass_id,
            input,
            output,
            product_visible: true,
            proof_or_readback: false,
        }
    }

    fn metrics(
        pass_id: ProductFrameGraphPassId,
        input: ProductFrameGraphResourceId,
        output: ProductFrameGraphResourceId,
    ) -> Self {
        Self {
            pass_id,
            input,
            output,
            product_visible: false,
            proof_or_readback: false,
        }
    }

    fn metric(
        &self,
        duration_ms: f64,
        stats: RendererRenderGraphPassStats,
    ) -> RendererRenderGraphPassMetric {
        RendererRenderGraphPassMetric {
            schema_version: 1,
            pass_id: self.pass_id.label().to_owned(),
            pass_kind: self.pass_id.kind().to_owned(),
            input: self.input.label().to_owned(),
            output: self.output.label().to_owned(),
            read_resources: vec![self.input.label().to_owned()],
            write_resources: vec![self.output.label().to_owned()],
            product_visible: self.product_visible,
            proof_or_readback: self.proof_or_readback,
            duration_ms,
            upload_bytes: stats.upload_bytes,
            dirty_chunk_count: stats.dirty_chunk_count,
            queue_write_count: stats.queue_write_count,
            draw_call_count: stats.draw_call_count,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ProductFrameGraph {
    passes: Vec<ProductFrameGraphPass>,
}

impl ProductFrameGraph {
    fn product_surface(text_run_count: usize) -> Self {
        Self {
            passes: vec![
                ProductFrameGraphPass::product(
                    ProductFrameGraphPassId::SceneKey,
                    ProductFrameGraphResourceId::RenderScene,
                    ProductFrameGraphResourceId::SceneCacheKey,
                ),
                ProductFrameGraphPass::product(
                    ProductFrameGraphPassId::QuadPrepareUpload,
                    ProductFrameGraphResourceId::RenderSceneItems,
                    ProductFrameGraphResourceId::RetainedGpuBuffers,
                ),
                ProductFrameGraphPass::product(
                    ProductFrameGraphPassId::UiDraw,
                    ProductFrameGraphResourceId::RetainedGpuBuffers,
                    ProductFrameGraphResourceId::ColorTarget,
                ),
                ProductFrameGraphPass::metrics(
                    ProductFrameGraphPassId::RetainedMetrics,
                    ProductFrameGraphResourceId::RenderScene,
                    ProductFrameGraphResourceId::FrameMetrics,
                ),
                ProductFrameGraphPass::product(
                    ProductFrameGraphPassId::TextDraw,
                    if text_run_count == 0 {
                        ProductFrameGraphResourceId::NoTextRuns
                    } else {
                        ProductFrameGraphResourceId::TextRuns
                    },
                    ProductFrameGraphResourceId::ColorTarget,
                ),
            ],
        }
    }

    fn planned_pass_metrics(&self) -> Vec<RendererRenderGraphPassMetric> {
        self.passes
            .iter()
            .map(|pass| pass.metric(0.0, RendererRenderGraphPassStats::default()))
            .collect()
    }
}

#[derive(Clone, Debug)]
struct ProductFrameSchedule {
    graph: ProductFrameGraph,
    planned_passes: Vec<RendererRenderGraphPassMetric>,
    planned_resources: Vec<RendererRenderGraphResourceMetric>,
    scheduler_kind: &'static str,
}

impl ProductFrameSchedule {
    fn product_surface(text_run_count: usize) -> Self {
        let graph = ProductFrameGraph::product_surface(text_run_count);
        let planned_passes = graph.planned_pass_metrics();
        let planned_resources = renderer_render_graph_resources_for_passes(&planned_passes);
        Self {
            graph,
            planned_passes,
            planned_resources,
            scheduler_kind: PRODUCT_FRAME_GRAPH_SCHEDULER_KIND,
        }
    }

    fn len(&self) -> usize {
        self.graph.passes.len()
    }

    fn pass(&self, index: usize) -> Option<&ProductFrameGraphPass> {
        self.graph.passes.get(index)
    }

    fn plan_hash(&self) -> String {
        renderer_render_graph_plan_hash(&self.planned_passes)
    }

    fn planned_resources(&self) -> Vec<RendererRenderGraphResourceMetric> {
        self.planned_resources.clone()
    }

    fn schedule_decisions(
        &self,
        resources: &[RendererRenderGraphResourceMetric],
        dirty_upload_chunk_ids: &[String],
    ) -> Vec<RendererRenderGraphScheduleDecisionMetric> {
        product_frame_graph_schedule_decisions(resources, dirty_upload_chunk_ids)
    }
}

#[derive(Debug)]
struct ProductFrameGraphExecutor {
    schedule: ProductFrameSchedule,
    next_pass_index: usize,
    executed_passes: Vec<RendererRenderGraphPassMetric>,
}

impl ProductFrameGraphExecutor {
    fn new(schedule: ProductFrameSchedule) -> Self {
        Self {
            schedule,
            next_pass_index: 0,
            executed_passes: Vec::new(),
        }
    }

    fn run_product_pass<T>(
        &mut self,
        pass_id: ProductFrameGraphPassId,
        input: ProductFrameGraphResourceId,
        output: ProductFrameGraphResourceId,
        run: impl FnOnce() -> Result<(T, RendererRenderGraphPassStats), RenderError>,
    ) -> Result<(T, f64), RenderError> {
        self.run_pass(pass_id, input, output, true, false, run)
    }

    fn run_metrics_pass<T>(
        &mut self,
        pass_id: ProductFrameGraphPassId,
        input: ProductFrameGraphResourceId,
        output: ProductFrameGraphResourceId,
        run: impl FnOnce() -> Result<(T, RendererRenderGraphPassStats), RenderError>,
    ) -> Result<(T, f64), RenderError> {
        self.run_pass(pass_id, input, output, false, false, run)
    }

    fn run_pass<T>(
        &mut self,
        pass_id: ProductFrameGraphPassId,
        input: ProductFrameGraphResourceId,
        output: ProductFrameGraphResourceId,
        product_visible: bool,
        proof_or_readback: bool,
        run: impl FnOnce() -> Result<(T, RendererRenderGraphPassStats), RenderError>,
    ) -> Result<(T, f64), RenderError> {
        let graph_pass = self
            .schedule
            .pass(self.next_pass_index)
            .ok_or_else(|| RenderError {
                message: format!(
                    "ProductFrameGraph schedule exhausted before pass `{}`",
                    pass_id.label()
                ),
            })?;
        if graph_pass.pass_id != pass_id
            || graph_pass.input != input
            || graph_pass.output != output
            || graph_pass.product_visible != product_visible
            || graph_pass.proof_or_readback != proof_or_readback
        {
            return Err(RenderError {
                message: format!(
                    "ProductFrameGraph schedule mismatch at pass {}: expected {} {}->{}, got {} {}->{}",
                    self.next_pass_index,
                    graph_pass.pass_id.label(),
                    graph_pass.input.label(),
                    graph_pass.output.label(),
                    pass_id.label(),
                    input.label(),
                    output.label()
                ),
            });
        }
        let graph_pass = graph_pass.clone();
        let started = Instant::now();
        let (value, stats) = run()?;
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.executed_passes
            .push(graph_pass.metric(duration_ms, stats));
        self.next_pass_index += 1;
        Ok((value, duration_ms))
    }

    fn finish(self) -> Result<ProductFrameExecution, RenderError> {
        if self.next_pass_index != self.schedule.len() {
            let missing = self
                .schedule
                .pass(self.next_pass_index)
                .map(|pass| pass.pass_id.label())
                .unwrap_or("unknown");
            return Err(RenderError {
                message: format!(
                    "ProductFrameGraph schedule finished early at pass {} of {}; next pass `{}` was not executed",
                    self.next_pass_index,
                    self.schedule.len(),
                    missing
                ),
            });
        }
        Ok(ProductFrameExecution {
            schedule: self.schedule,
            executed_passes: self.executed_passes,
        })
    }
}

#[derive(Debug)]
struct ProductFrameExecution {
    schedule: ProductFrameSchedule,
    executed_passes: Vec<RendererRenderGraphPassMetric>,
}

#[derive(Clone, Debug, Default)]
struct ProductFrameGraphResourceState {
    epoch: u64,
    last_signature: String,
    last_used_frame_seq: u64,
    dirty: bool,
    reused: bool,
}

#[derive(Clone, Debug, Default)]
struct ProductFrameGraphState {
    resources: BTreeMap<String, ProductFrameGraphResourceState>,
}

#[derive(Clone, Debug, Default)]
struct ProductFrameGraphStateMetrics {
    resource_count: u32,
    dirty_resource_count: u32,
    reused_resource_count: u32,
    resource_epoch_hash: String,
}

#[derive(Clone, Debug, Default)]
struct ProductFrameGraphScheduleMetrics {
    decision_count: u32,
    dirty_resource_decision_count: u32,
    reuse_resource_decision_count: u32,
    per_present_resource_decision_count: u32,
    schedule_hash: String,
}

impl ProductFrameGraphState {
    fn update_resources(
        &mut self,
        frame_seq: u64,
        resources: &mut [RendererRenderGraphResourceMetric],
        signatures: &BTreeMap<String, String>,
    ) -> ProductFrameGraphStateMetrics {
        let mut seen = BTreeSet::new();
        for resource in resources.iter_mut() {
            seen.insert(resource.resource_id.clone());
            let signature = signatures
                .get(&resource.resource_id)
                .cloned()
                .unwrap_or_else(|| {
                    format!(
                        "{}:{}:{}:{}",
                        resource.resource_id,
                        resource.first_pass_index,
                        resource.last_pass_index,
                        u8::from(resource.product_visible)
                    )
                });
            let entry = self
                .resources
                .entry(resource.resource_id.clone())
                .or_default();
            let changed = entry.last_used_frame_seq == 0 || entry.last_signature != signature;
            if changed {
                entry.epoch = entry.epoch.saturating_add(1);
                entry.last_signature = signature;
            }
            entry.last_used_frame_seq = frame_seq;
            entry.dirty = changed;
            entry.reused = !changed;

            resource.retained_epoch = entry.epoch;
            resource.retained_dirty = entry.dirty;
            resource.retained_reused = entry.reused;
            resource.last_used_frame_seq = entry.last_used_frame_seq;
        }
        self.resources
            .retain(|resource_id, _| seen.contains(resource_id));
        self.metrics()
    }

    fn metrics(&self) -> ProductFrameGraphStateMetrics {
        let mut dirty_resource_count = 0_u32;
        let mut reused_resource_count = 0_u32;
        let mut hasher = Sha256::new();
        for (resource_id, state) in &self.resources {
            dirty_resource_count = dirty_resource_count.saturating_add(u32::from(state.dirty));
            reused_resource_count = reused_resource_count.saturating_add(u32::from(state.reused));
            hasher.update(resource_id.as_bytes());
            hasher.update([0]);
            hasher.update(state.epoch.to_le_bytes());
            hasher.update(state.last_used_frame_seq.to_le_bytes());
            hasher.update([u8::from(state.dirty), u8::from(state.reused)]);
        }
        ProductFrameGraphStateMetrics {
            resource_count: self.resources.len() as u32,
            dirty_resource_count,
            reused_resource_count,
            resource_epoch_hash: format!("{:x}", hasher.finalize()),
        }
    }
}

fn product_frame_graph_schedule_decisions(
    resources: &[RendererRenderGraphResourceMetric],
    dirty_upload_chunk_ids: &[String],
) -> Vec<RendererRenderGraphScheduleDecisionMetric> {
    resources
        .iter()
        .map(|resource| {
            let (decision_kind, reason) =
                product_frame_graph_resource_decision(resource, dirty_upload_chunk_ids);
            RendererRenderGraphScheduleDecisionMetric {
                schema_version: 1,
                resource_id: resource.resource_id.clone(),
                resource_kind: resource.resource_kind.clone(),
                decision_kind: decision_kind.to_owned(),
                reason,
                retained_epoch: resource.retained_epoch,
                product_visible: resource.product_visible,
                proof_or_readback: resource.proof_or_readback,
            }
        })
        .collect()
}

fn product_frame_graph_resource_decision(
    resource: &RendererRenderGraphResourceMetric,
    dirty_upload_chunk_ids: &[String],
) -> (&'static str, String) {
    match resource.resource_id.as_str() {
        "ColorTarget" => (
            "per_present_target",
            "visible color target is acquired and presented per frame".to_owned(),
        ),
        "FrameMetrics" => (
            "per_frame_metrics",
            "CPU metrics are produced for each frame".to_owned(),
        ),
        "RetainedGpuBuffers" if resource.retained_dirty && !dirty_upload_chunk_ids.is_empty() => (
            "dirty_upload",
            format!("dirty_upload_chunks={}", dirty_upload_chunk_ids.len()),
        ),
        _ if resource.retained_dirty && resource.retained_epoch <= 1 => (
            "dirty_first_use",
            "retained resource was initialized for this graph".to_owned(),
        ),
        _ if resource.retained_dirty => (
            "dirty_signature_changed",
            "retained resource signature changed".to_owned(),
        ),
        _ if resource.retained_reused => (
            "clean_reuse",
            "retained resource signature is unchanged".to_owned(),
        ),
        _ => (
            "unknown",
            "retained resource state was unavailable".to_owned(),
        ),
    }
}

fn product_frame_graph_schedule_metrics(
    decisions: &[RendererRenderGraphScheduleDecisionMetric],
) -> ProductFrameGraphScheduleMetrics {
    let mut dirty_resource_decision_count = 0_u32;
    let mut reuse_resource_decision_count = 0_u32;
    let mut per_present_resource_decision_count = 0_u32;
    let mut hasher = Sha256::new();
    for decision in decisions {
        dirty_resource_decision_count =
            dirty_resource_decision_count.saturating_add(u32::from(matches!(
                decision.decision_kind.as_str(),
                "dirty_first_use" | "dirty_signature_changed" | "dirty_upload"
            )));
        reuse_resource_decision_count = reuse_resource_decision_count
            .saturating_add(u32::from(decision.decision_kind == "clean_reuse"));
        per_present_resource_decision_count =
            per_present_resource_decision_count.saturating_add(u32::from(matches!(
                decision.decision_kind.as_str(),
                "per_present_target" | "per_frame_metrics"
            )));
        hasher.update(decision.resource_id.as_bytes());
        hasher.update([0]);
        hasher.update(decision.resource_kind.as_bytes());
        hasher.update([0]);
        hasher.update(decision.decision_kind.as_bytes());
        hasher.update([0]);
        hasher.update(decision.retained_epoch.to_le_bytes());
        hasher.update([
            u8::from(decision.product_visible),
            u8::from(decision.proof_or_readback),
        ]);
    }
    ProductFrameGraphScheduleMetrics {
        decision_count: decisions.len() as u32,
        dirty_resource_decision_count,
        reuse_resource_decision_count,
        per_present_resource_decision_count,
        schedule_hash: format!("{:x}", hasher.finalize()),
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_internal_scene_to_surface(
    request: SceneEncodeRequest<'_>,
    scene: &RenderScene,
    pipeline: &wgpu::RenderPipeline,
    text: &mut GlyphonTextState,
    textures: &mut TextureState,
    quad_buffers: &mut BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>,
    quad_upload_ring: &mut QuadUploadRing,
    prepared_quads: &mut BTreeMap<PreparedQuadCacheKey, PreparedQuadCache>,
    previous_chunk_ids: &mut BTreeSet<String>,
    product_frame_graph: &mut ProductFrameGraphState,
    scene_key_override: Option<u64>,
    frame_seq: u64,
) -> Result<FrameMetrics, RenderError> {
    let width = request.width;
    let height = request.height;
    let text_runs_shaped = scene.text_runs.len() as u32;
    let render_schedule = ProductFrameSchedule::product_surface(scene.text_runs.len());
    let mut render_graph = ProductFrameGraphExecutor::new(render_schedule);
    let (scene_key, scene_key_ms) = render_graph.run_product_pass(
        ProductFrameGraphPassId::SceneKey,
        ProductFrameGraphResourceId::RenderScene,
        ProductFrameGraphResourceId::SceneCacheKey,
        || {
            Ok((
                scene_key_override.unwrap_or_else(|| render_scene_cache_key(scene)),
                RendererRenderGraphPassStats::default(),
            ))
        },
    )?;
    let mut upload_bytes = 0u64;
    let mut allocated_gpu_bytes = 0u64;
    let mut dirty_upload_ranges = Vec::new();
    let mut buffer_reuse_count = 0u32;
    let mut staging_wrap_count = 0u32;
    let mut queue_write_count = 0u32;
    let mut quad_cache_eviction_count = 0u32;
    let mut rect_vertices_ms = 0.0_f64;
    let mut asset_prepare_ms = 0.0_f64;
    let mut quad_batch_key_ms = 0.0_f64;
    let mut quad_upload_ms = 0.0_f64;
    let before = quad_buffers.len();
    quad_buffers.retain(|_, batch| quad_upload_ring.cached_batch_is_valid(batch));
    quad_cache_eviction_count =
        quad_cache_eviction_count.saturating_add(before.saturating_sub(quad_buffers.len()) as u32);
    let upload_bytes_before_quads = upload_bytes;
    let queue_write_count_before_quads = queue_write_count;
    let dirty_upload_range_count_before_quads = dirty_upload_ranges.len();
    let mut quad_cache_hit = false;
    let ((gpu_batches, rect_metrics, asset_metrics), _quad_prepare_upload_ms) = render_graph
        .run_product_pass(
            ProductFrameGraphPassId::QuadPrepareUpload,
            ProductFrameGraphResourceId::RenderSceneItems,
            ProductFrameGraphResourceId::RetainedGpuBuffers,
            || {
                let prepared_key = PreparedQuadCacheKey {
                    scene_key,
                    width,
                    height,
                };
                let prepared_hit = {
                    if prepared_quads
                        .get(&prepared_key)
                        .is_some_and(|entry| !quad_upload_ring.prepared_cache_is_valid(entry))
                    {
                        prepared_quads.remove(&prepared_key);
                    }
                    prepared_quads.get(&prepared_key).and_then(|entry| {
                        let asset_prepare_started = Instant::now();
                        let asset_metrics = textures.cached_asset_metrics(
                            entry.gpu_batches.iter().map(|batch| &batch.texture),
                        );
                        asset_prepare_ms += asset_prepare_started.elapsed().as_secs_f64() * 1000.0;
                        asset_metrics
                            .failure_diagnostics
                            .is_empty()
                            .then(|| (entry.gpu_batches.clone(), entry.rect_metrics, asset_metrics))
                    })
                };
                quad_cache_hit = prepared_hit.is_some();
                let (gpu_batches, rect_metrics, asset_metrics) =
                    if let Some((gpu_batches, rect_metrics, asset_metrics)) = prepared_hit {
                        buffer_reuse_count = gpu_batches.len() as u32;
                        (gpu_batches, rect_metrics, asset_metrics)
                    } else {
                        let rect_vertices_started = Instant::now();
                        let (quad_batches, rect_metrics) =
                            rect_vertices_from_scene(&scene, width as f32, height as f32);
                        rect_vertices_ms += rect_vertices_started.elapsed().as_secs_f64() * 1000.0;
                        let asset_prepare_started = Instant::now();
                        let asset_metrics = textures.prepare_assets(
                            request.device,
                            request.queue,
                            &quad_batches,
                        )?;
                        asset_prepare_ms += asset_prepare_started.elapsed().as_secs_f64() * 1000.0;
                        struct QuadUploadCandidate {
                            batch: QuadBatch,
                            vertex_count: u32,
                            cache_key: QuadBatchCacheKey,
                            reservation_size: u64,
                        }
                        let mut candidates = Vec::new();
                        let mut frame_reservation_size = 0u64;
                        let mut dirty_reservation_size = 0u64;
                        let quad_batch_key_started = Instant::now();
                        for batch in quad_batches {
                            let vertex_count = batch.vertices.len() as u32;
                            if vertex_count == 0 {
                                continue;
                            }
                            let vertex_bytes = bytemuck::cast_slice(&batch.vertices);
                            let reservation_size =
                                quad_upload_reservation_size(vertex_bytes.len() as u64);
                            let cache_key = QuadBatchCacheKey {
                                retained_chunk_id: batch.retained_chunk_id.clone(),
                                texture: batch.texture.clone(),
                                vertex_count,
                                content_key: quad_batch_content_key(vertex_bytes),
                            };
                            frame_reservation_size =
                                frame_reservation_size.saturating_add(reservation_size);
                            let cache_hit = quad_buffers.get(&cache_key).is_some_and(|cached| {
                                quad_upload_ring.cached_batch_is_valid(cached)
                            });
                            if !cache_hit {
                                dirty_reservation_size =
                                    dirty_reservation_size.saturating_add(reservation_size);
                            }
                            candidates.push(QuadUploadCandidate {
                                batch,
                                vertex_count,
                                cache_key,
                                reservation_size,
                            });
                        }
                        quad_batch_key_ms +=
                            quad_batch_key_started.elapsed().as_secs_f64() * 1000.0;
                        let quad_upload_started = Instant::now();
                        let begin_stats = quad_upload_ring.begin_frame(
                            request.device,
                            frame_reservation_size,
                            dirty_reservation_size,
                            Some(quad_buffers),
                        )?;
                        upload_bytes = upload_bytes.saturating_add(begin_stats.upload_bytes);
                        allocated_gpu_bytes =
                            allocated_gpu_bytes.saturating_add(begin_stats.allocated_gpu_bytes);
                        staging_wrap_count =
                            staging_wrap_count.saturating_add(begin_stats.staging_wrap_count);
                        queue_write_count =
                            queue_write_count.saturating_add(begin_stats.queue_write_count);
                        quad_cache_eviction_count = quad_cache_eviction_count
                            .saturating_add(begin_stats.cache_eviction_count);
                        let invalidated_cached_ranges = begin_stats.invalidated_cached_ranges;
                        dirty_upload_ranges.extend(begin_stats.dirty_upload_ranges);

                        let mut gpu_batches = Vec::new();
                        for candidate in candidates {
                            let QuadUploadCandidate {
                                batch,
                                vertex_count,
                                cache_key,
                                reservation_size: _reservation_size,
                            } = candidate;
                            let vertex_bytes = bytemuck::cast_slice(&batch.vertices);
                            let gpu_batch = if !invalidated_cached_ranges
                                && let Some(cached) = quad_buffers
                                    .get(&cache_key)
                                    .filter(|cached| quad_upload_ring.cached_batch_is_valid(cached))
                            {
                                buffer_reuse_count = buffer_reuse_count.saturating_add(1);
                                GpuQuadBatch {
                                    texture: batch.texture,
                                    vertex_count: cached.vertex_count,
                                    vertex_buffer: cached.vertex_buffer.clone(),
                                    byte_range: cached.byte_range.clone(),
                                    ring_generation: cached.ring_generation,
                                }
                            } else {
                                if quad_buffers.len() >= MAX_CACHED_QUAD_BATCHES {
                                    quad_cache_eviction_count = quad_cache_eviction_count
                                        .saturating_add(quad_buffers.len() as u32);
                                    quad_buffers.clear();
                                }
                                let (uploaded, stats) = quad_upload_ring.upload_reserved(
                                    request.queue,
                                    vertex_bytes,
                                    vertex_count,
                                    Some(batch.retained_chunk_id.clone()),
                                )?;
                                upload_bytes = upload_bytes.saturating_add(stats.upload_bytes);
                                allocated_gpu_bytes =
                                    allocated_gpu_bytes.saturating_add(stats.allocated_gpu_bytes);
                                staging_wrap_count =
                                    staging_wrap_count.saturating_add(stats.staging_wrap_count);
                                queue_write_count =
                                    queue_write_count.saturating_add(stats.queue_write_count);
                                quad_cache_eviction_count = quad_cache_eviction_count
                                    .saturating_add(stats.cache_eviction_count);
                                dirty_upload_ranges.extend(stats.dirty_upload_ranges);
                                let gpu_batch = GpuQuadBatch {
                                    texture: batch.texture,
                                    vertex_count: uploaded.vertex_count,
                                    vertex_buffer: uploaded.vertex_buffer.clone(),
                                    byte_range: uploaded.byte_range.clone(),
                                    ring_generation: uploaded.ring_generation,
                                };
                                quad_buffers.insert(cache_key.clone(), uploaded);
                                gpu_batch
                            };
                            gpu_batches.push(gpu_batch);
                        }
                        quad_upload_ms += quad_upload_started.elapsed().as_secs_f64() * 1000.0;
                        if prepared_quads.len() >= PREPARED_QUAD_CACHE_CAP
                            && !prepared_quads.contains_key(&prepared_key)
                            && let Some(oldest_key) = prepared_quads.keys().next().copied()
                        {
                            prepared_quads.remove(&oldest_key);
                        }
                        prepared_quads.insert(
                            prepared_key,
                            PreparedQuadCache {
                                ring_generation: quad_upload_ring.generation,
                                gpu_batches: gpu_batches.clone(),
                                rect_metrics,
                            },
                        );
                        (gpu_batches, rect_metrics, asset_metrics)
                    };
                let dirty_chunk_count = dirty_upload_ranges
                    .iter()
                    .skip(dirty_upload_range_count_before_quads)
                    .filter_map(|range| range.retained_chunk_id.as_ref())
                    .collect::<BTreeSet<_>>()
                    .len() as u32;
                Ok((
                    (gpu_batches, rect_metrics, asset_metrics),
                    RendererRenderGraphPassStats {
                        upload_bytes: upload_bytes.saturating_sub(upload_bytes_before_quads),
                        dirty_chunk_count,
                        queue_write_count: queue_write_count
                            .saturating_sub(queue_write_count_before_quads),
                        draw_call_count: 0,
                    },
                ))
            },
        )?;
    let draw_ranges = coalesced_gpu_quad_draw_ranges(&gpu_batches);
    let draw_range_count = draw_ranges.len() as u32;
    let ((), draw_pass_ms) = render_graph.run_product_pass(
        ProductFrameGraphPassId::UiDraw,
        ProductFrameGraphResourceId::RetainedGpuBuffers,
        ProductFrameGraphResourceId::ColorTarget,
        || {
            let mut pass = request
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("boon-native-gpu-visible-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: request.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.04,
                                g: 0.05,
                                b: 0.06,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            pass.set_pipeline(pipeline);
            for range in &draw_ranges {
                let batch = &gpu_batches[range.first_batch_index];
                let bind_group =
                    textures
                        .bind_group_for(&range.texture)
                        .ok_or_else(|| RenderError {
                            message: "native GPU asset texture was not prepared before draw"
                                .to_owned(),
                        })?;
                bind_group.set(&mut pass);
                pass.set_vertex_buffer(0, batch.vertex_buffer.slice(range.byte_range.clone()));
                pass.draw(0..range.vertex_count, 0..1);
            }
            Ok((
                (),
                RendererRenderGraphPassStats {
                    draw_call_count: draw_range_count,
                    ..RendererRenderGraphPassStats::default()
                },
            ))
        },
    )?;
    let (retained_chunk_metrics, retained_metrics_ms) = render_graph.run_metrics_pass(
        ProductFrameGraphPassId::RetainedMetrics,
        ProductFrameGraphResourceId::RenderScene,
        ProductFrameGraphResourceId::FrameMetrics,
        || {
            let metrics = sampled_retained_render_chunks(
                scene,
                frame_seq,
                Some(previous_chunk_ids),
                RETAINED_CHUNK_METRIC_SAMPLE_LIMIT,
            );
            Ok((metrics, RendererRenderGraphPassStats::default()))
        },
    )?;
    *previous_chunk_ids = retained_chunk_metrics.current_chunk_ids.clone();
    let retained_chunk_count = retained_chunk_metrics.retained_chunk_count;
    let retained_chunk_hit_count = retained_chunk_metrics.retained_chunk_hit_count;
    let retained_chunk_miss_count = retained_chunk_metrics.retained_chunk_miss_count;
    let retained_chunk_reuse_count = retained_chunk_hit_count;
    let dirty_chunk_count = retained_chunk_miss_count;
    let mut dirty_upload_chunk_ids = Vec::new();
    let mut seen_dirty_upload_chunk_ids = BTreeSet::new();
    for range in &dirty_upload_ranges {
        if let Some(retained_chunk_id) = range.retained_chunk_id.as_ref()
            && seen_dirty_upload_chunk_ids.insert(retained_chunk_id.clone())
        {
            dirty_upload_chunk_ids.push(retained_chunk_id.clone());
        }
    }
    let dirty_upload_chunk_count = dirty_upload_chunk_ids.len() as u32;
    let ((rendered_text_runs, text_cache_metrics), text_render_ms) = render_graph
        .run_product_pass(
            ProductFrameGraphPassId::TextDraw,
            if scene.text_runs.is_empty() {
                ProductFrameGraphResourceId::NoTextRuns
            } else {
                ProductFrameGraphResourceId::TextRuns
            },
            ProductFrameGraphResourceId::ColorTarget,
            || {
                let glyphon_text_runs = scene
                    .text_runs
                    .iter()
                    .cloned()
                    .map(TextRun::from)
                    .collect::<Vec<_>>();
                let result = text.render(
                    request.device,
                    request.queue,
                    request.encoder,
                    request.view,
                    glyphon_text_runs,
                    width,
                    height,
                )?;
                Ok((
                    result,
                    RendererRenderGraphPassStats {
                        draw_call_count: u32::from(result.0 > 0),
                        ..RendererRenderGraphPassStats::default()
                    },
                ))
            },
        )?;
    let render_execution = render_graph.finish()?;
    let renderer_render_graph_passes = render_execution.executed_passes;
    let mut renderer_render_graph_resources = render_execution.schedule.planned_resources();
    let renderer_render_graph_resource_signatures = renderer_render_graph_resource_signatures(
        scene_key,
        frame_seq,
        text_runs_shaped,
        rendered_text_runs,
        &dirty_upload_chunk_ids,
    );
    let retained_state_metrics = product_frame_graph.update_resources(
        frame_seq,
        &mut renderer_render_graph_resources,
        &renderer_render_graph_resource_signatures,
    );
    let renderer_render_graph_schedule_decisions = render_execution
        .schedule
        .schedule_decisions(&renderer_render_graph_resources, &dirty_upload_chunk_ids);
    let schedule_metrics =
        product_frame_graph_schedule_metrics(&renderer_render_graph_schedule_decisions);
    let renderer_render_graph_plan_hash = render_execution.schedule.plan_hash();
    let renderer_render_graph_workload_hash =
        renderer_render_graph_workload_hash(&renderer_render_graph_passes);
    let renderer_render_graph_resource_lifetime_hash =
        renderer_render_graph_resource_lifetime_hash(&renderer_render_graph_resources);
    let renderer_render_graph_kind = "boon_native_gpu_product_frame_graph".to_owned();
    let renderer_render_graph_execution_kind = "retained_product_frame_graph_linear_v1".to_owned();
    let renderer_render_graph_pass_count = renderer_render_graph_passes.len() as u32;
    let renderer_render_graph_product_pass_count = renderer_render_graph_passes
        .iter()
        .filter(|pass| pass.product_visible)
        .count() as u32;
    let renderer_render_graph_proof_pass_count = renderer_render_graph_passes
        .iter()
        .filter(|pass| pass.proof_or_readback)
        .count() as u32;
    let renderer_render_graph_resource_count = renderer_render_graph_resources.len() as u32;
    let renderer_render_graph_product_resource_count = renderer_render_graph_resources
        .iter()
        .filter(|resource| resource.product_visible)
        .count() as u32;
    let renderer_render_graph_scheduler_kind = render_execution.schedule.scheduler_kind.to_owned();
    let product_frame_graph = ProductFrameGraphReport {
        schema_version: 1,
        owner: "boon_native_gpu".to_owned(),
        graph_kind: renderer_render_graph_kind.clone(),
        execution_kind: renderer_render_graph_execution_kind.clone(),
        plan_hash: renderer_render_graph_plan_hash.clone(),
        workload_hash: renderer_render_graph_workload_hash.clone(),
        pass_count: renderer_render_graph_pass_count,
        product_pass_count: renderer_render_graph_product_pass_count,
        proof_pass_count: renderer_render_graph_proof_pass_count,
        resource_count: renderer_render_graph_resource_count,
        product_resource_count: renderer_render_graph_product_resource_count,
        resource_lifetime_hash: renderer_render_graph_resource_lifetime_hash.clone(),
        retained_resource_epoch_hash: retained_state_metrics.resource_epoch_hash.clone(),
        retained_dirty_resource_count: retained_state_metrics.dirty_resource_count,
        retained_reused_resource_count: retained_state_metrics.reused_resource_count,
        retained_state_resource_count: retained_state_metrics.resource_count,
        scheduler_kind: renderer_render_graph_scheduler_kind.clone(),
        schedule_hash: schedule_metrics.schedule_hash.clone(),
        schedule_decision_count: schedule_metrics.decision_count,
        dirty_resource_decision_count: schedule_metrics.dirty_resource_decision_count,
        reuse_resource_decision_count: schedule_metrics.reuse_resource_decision_count,
        per_present_resource_decision_count: schedule_metrics.per_present_resource_decision_count,
        passes: renderer_render_graph_passes.clone(),
        resources: renderer_render_graph_resources.clone(),
        schedule_decisions: renderer_render_graph_schedule_decisions.clone(),
    };
    Ok(FrameMetrics {
        frame_seq,
        render_scene_source: RENDER_SCENE_SOURCE_INTERNAL_RENDER_SCENE.to_owned(),
        product_frame_graph: Some(product_frame_graph),
        document_scene_convert_ms: 0.0,
        document_scene_cache_hit: false,
        document_scene_cache_entry_count: 0,
        draw_calls: draw_range_count + u32::from(rendered_text_runs > 0),
        upload_bytes,
        allocated_gpu_bytes,
        dirty_upload_range_count: dirty_upload_ranges.len() as u32,
        dirty_upload_ranges,
        dirty_upload_chunk_count,
        dirty_upload_chunk_ids,
        buffer_reuse_count,
        staging_wrap_count,
        queue_write_count,
        quad_cache_eviction_count,
        quad_cache_hit,
        quad_cache_entry_count: quad_buffers.len() as u32,
        scene_key_ms,
        rect_vertices_ms,
        asset_prepare_ms,
        quad_batch_key_ms,
        quad_upload_ms,
        draw_pass_ms,
        retained_metrics_ms,
        text_render_ms,
        visible_display_item_count: rect_metrics.visible_display_item_count,
        rendered_rect_count: rect_metrics.rendered_rect_count,
        rect_cap_hit: rect_metrics.cap_hit,
        visible_text_runs: text_runs_shaped,
        shaped_text_runs: text_cache_metrics.cache_misses,
        text_runs_shaped,
        rendered_text_runs,
        shaped_run_cache_hits: text_cache_metrics.cache_hits,
        shaped_run_cache_misses: text_cache_metrics.cache_misses,
        shaped_run_cache_evictions: text_cache_metrics.cache_evictions,
        shaped_run_cache_entry_count: text_cache_metrics.cache_entry_count,
        shaped_run_cache_capacity: text_cache_metrics.cache_capacity,
        shaped_run_cache_bytes: text_cache_metrics.cache_memory_bytes,
        missing_glyph_count: text_cache_metrics.missing_glyph_count,
        glyph_atlas_prepare_count: text_cache_metrics.glyph_atlas_prepare_count,
        glyph_atlas_evictions_observed: text_cache_metrics.glyph_atlas_evictions_observed,
        text_cap_hit: false,
        glyphon_text_area_count: rendered_text_runs,
        color_only_rect_fallback: rendered_text_runs == 0 && text_runs_shaped > 0,
        preview_blocked_on_ipc_count: 0,
        asset_ref_count: asset_metrics.refs.len() as u32,
        asset_refs: asset_metrics.asset_refs(),
        asset_cache_hits: asset_metrics.cache_hits,
        asset_cache_misses: asset_metrics.cache_misses,
        asset_cache_evictions: asset_metrics.cache_evictions,
        asset_cache_entry_count: asset_metrics.cache_entry_count,
        asset_cache_byte_count: asset_metrics.cache_byte_count,
        asset_cache_byte_cap: asset_metrics.cache_byte_cap,
        asset_cache_byte_cap_hit: asset_metrics.cache_byte_cap_hit,
        asset_decode_count: asset_metrics.decode_count,
        asset_raster_count: asset_metrics.raster_count,
        asset_upload_count: asset_metrics.upload_count,
        asset_upload_bytes: asset_metrics.upload_bytes,
        asset_failure_diagnostics: asset_metrics.failure_diagnostics,
        retained_chunk_count,
        retained_chunk_hit_count,
        retained_chunk_miss_count,
        retained_chunk_reuse_count,
        dirty_chunk_count,
        retained_chunk_sample_count: retained_chunk_metrics.retained_chunks.len() as u32,
        retained_chunk_inventory_truncated: retained_chunk_metrics.retained_chunk_count as usize
            > retained_chunk_metrics.retained_chunks.len(),
        retained_chunks: retained_chunk_metrics.retained_chunks,
    })
}

fn renderer_render_graph_plan_hash(passes: &[RendererRenderGraphPassMetric]) -> String {
    let mut hasher = Sha256::new();
    for pass in passes {
        hasher.update(pass.pass_id.as_bytes());
        hasher.update([0]);
        hasher.update(pass.pass_kind.as_bytes());
        hasher.update([0]);
        hasher.update(pass.input.as_bytes());
        hasher.update([0]);
        hasher.update(pass.output.as_bytes());
        hasher.update([0]);
        for resource in &pass.read_resources {
            hasher.update(resource.as_bytes());
            hasher.update([0]);
        }
        hasher.update([1]);
        for resource in &pass.write_resources {
            hasher.update(resource.as_bytes());
            hasher.update([0]);
        }
        hasher.update([
            u8::from(pass.product_visible),
            u8::from(pass.proof_or_readback),
        ]);
    }
    format!("{:x}", hasher.finalize())
}

fn renderer_render_graph_workload_hash(passes: &[RendererRenderGraphPassMetric]) -> String {
    let mut hasher = Sha256::new();
    for pass in passes {
        hasher.update(pass.pass_id.as_bytes());
        hasher.update([0]);
        hasher.update(pass.upload_bytes.to_le_bytes());
        hasher.update(pass.dirty_chunk_count.to_le_bytes());
        hasher.update(pass.queue_write_count.to_le_bytes());
        hasher.update(pass.draw_call_count.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, Default)]
struct RendererResourceLifetimeBuilder {
    first_pass_index: u32,
    last_pass_index: u32,
    producer_pass_id: Option<String>,
    consumer_pass_ids: BTreeSet<String>,
    product_visible: bool,
    proof_or_readback: bool,
}

fn renderer_render_graph_resources_for_passes(
    passes: &[RendererRenderGraphPassMetric],
) -> Vec<RendererRenderGraphResourceMetric> {
    let mut resources = BTreeMap::<String, RendererResourceLifetimeBuilder>::new();
    for (index, pass) in passes.iter().enumerate() {
        let pass_index = index as u32;
        for resource_id in &pass.read_resources {
            let entry = resources.entry(resource_id.clone()).or_insert_with(|| {
                RendererResourceLifetimeBuilder {
                    first_pass_index: pass_index,
                    last_pass_index: pass_index,
                    ..RendererResourceLifetimeBuilder::default()
                }
            });
            entry.first_pass_index = entry.first_pass_index.min(pass_index);
            entry.last_pass_index = entry.last_pass_index.max(pass_index);
            entry.consumer_pass_ids.insert(pass.pass_id.clone());
            entry.product_visible |= pass.product_visible;
            entry.proof_or_readback |= pass.proof_or_readback;
        }
        for resource_id in &pass.write_resources {
            let entry = resources.entry(resource_id.clone()).or_insert_with(|| {
                RendererResourceLifetimeBuilder {
                    first_pass_index: pass_index,
                    last_pass_index: pass_index,
                    ..RendererResourceLifetimeBuilder::default()
                }
            });
            entry.first_pass_index = entry.first_pass_index.min(pass_index);
            entry.last_pass_index = entry.last_pass_index.max(pass_index);
            entry.producer_pass_id = Some(pass.pass_id.clone());
            entry.product_visible |= pass.product_visible;
            entry.proof_or_readback |= pass.proof_or_readback;
        }
    }
    resources
        .into_iter()
        .map(|(resource_id, entry)| RendererRenderGraphResourceMetric {
            schema_version: 1,
            resource_kind: renderer_render_graph_resource_kind(&resource_id).to_owned(),
            resource_id,
            first_pass_index: entry.first_pass_index,
            last_pass_index: entry.last_pass_index,
            producer_pass_id: entry.producer_pass_id,
            consumer_pass_ids: entry.consumer_pass_ids.into_iter().collect(),
            product_visible: entry.product_visible,
            proof_or_readback: entry.proof_or_readback,
            retained_epoch: 0,
            retained_dirty: false,
            retained_reused: false,
            last_used_frame_seq: 0,
        })
        .collect()
}

fn renderer_render_graph_resource_kind(resource_id: &str) -> &'static str {
    match resource_id {
        "RenderScene" | "RenderSceneItems" | "TextRuns" | "NoTextRuns" => "cpu_scene",
        "SceneCacheKey" => "cpu_identity",
        "RetainedGpuBuffers" => "gpu_buffer",
        "ColorTarget" => "gpu_color_target",
        "FrameMetrics" => "cpu_metrics",
        _ => "generic_resource",
    }
}

fn renderer_render_graph_resource_signatures(
    scene_key: u64,
    frame_seq: u64,
    text_runs_shaped: u32,
    rendered_text_runs: u32,
    _dirty_upload_chunk_ids: &[String],
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("RenderScene".to_owned(), format!("scene:{scene_key}")),
        (
            "RenderSceneItems".to_owned(),
            format!("scene-items:{scene_key}"),
        ),
        ("SceneCacheKey".to_owned(), format!("scene-key:{scene_key}")),
        (
            "RetainedGpuBuffers".to_owned(),
            format!("gpu-buffers:{scene_key}"),
        ),
        (
            "ColorTarget".to_owned(),
            format!("color-target:{frame_seq}"),
        ),
        (
            "FrameMetrics".to_owned(),
            format!("frame-metrics:{frame_seq}"),
        ),
        (
            "TextRuns".to_owned(),
            format!("text-runs:{scene_key}:{text_runs_shaped}:{rendered_text_runs}"),
        ),
        (
            "NoTextRuns".to_owned(),
            format!("no-text-runs:{scene_key}:{text_runs_shaped}"),
        ),
    ])
}

fn renderer_render_graph_resource_lifetime_hash(
    resources: &[RendererRenderGraphResourceMetric],
) -> String {
    let mut hasher = Sha256::new();
    for resource in resources {
        hasher.update(resource.resource_id.as_bytes());
        hasher.update([0]);
        hasher.update(resource.resource_kind.as_bytes());
        hasher.update([0]);
        hasher.update(resource.first_pass_index.to_le_bytes());
        hasher.update(resource.last_pass_index.to_le_bytes());
        if let Some(producer) = resource.producer_pass_id.as_ref() {
            hasher.update(producer.as_bytes());
        }
        hasher.update([0]);
        for consumer in &resource.consumer_pass_ids {
            hasher.update(consumer.as_bytes());
            hasher.update([0]);
        }
        hasher.update([
            u8::from(resource.product_visible),
            u8::from(resource.proof_or_readback),
        ]);
    }
    format!("{:x}", hasher.finalize())
}

fn quad_batch_content_key(vertex_bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    vertex_bytes.len().hash(&mut hasher);
    vertex_bytes.hash(&mut hasher);
    hasher.finish()
}

fn render_scene_cache_key(scene: &RenderScene) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_rect(&mut hasher, scene.viewport);
    scene.items.len().hash(&mut hasher);
    for item in &scene.items {
        item.node.0.hash(&mut hasher);
        item.retained_chunk_id.hash(&mut hasher);
        item.source_kind.hash(&mut hasher);
        hash_rect(&mut hasher, item.bounds);
        item.style_identity.style_id.hash(&mut hasher);
        item.style_identity.layout_id.hash(&mut hasher);
        item.style_identity.paint_id.hash(&mut hasher);
        item.style_identity.material_id.hash(&mut hasher);
        item.style_identity.font_id.hash(&mut hasher);
        item.style_identity.pseudo_state_id.hash(&mut hasher);
        item.estimated_vertex_count.hash(&mut hasher);
    }
    scene.quad_batches.len().hash(&mut hasher);
    for batch in &scene.quad_batches {
        batch.retained_chunk_id.hash(&mut hasher);
        match &batch.texture {
            QuadTexture::Solid => "solid".hash(&mut hasher),
            QuadTexture::Asset(key) => {
                "asset".hash(&mut hasher);
                key.url.hash(&mut hasher);
                key.width.hash(&mut hasher);
                key.height.hash(&mut hasher);
            }
        }
        for vertex in &batch.vertices {
            for coordinate in vertex.position {
                coordinate.to_bits().hash(&mut hasher);
            }
            vertex.color.hash(&mut hasher);
            for coordinate in vertex.uv {
                coordinate.to_bits().hash(&mut hasher);
            }
        }
    }
    scene
        .rect_metrics
        .visible_display_item_count
        .hash(&mut hasher);
    scene.rect_metrics.rendered_rect_count.hash(&mut hasher);
    scene.rect_metrics.cap_hit.hash(&mut hasher);
    hasher.finish()
}

fn render_scene_supplied_cache_key(
    scene_identity: Option<&str>,
    width: u32,
    height: u32,
) -> Option<u64> {
    let scene_identity = scene_identity?;
    let mut hasher = DefaultHasher::new();
    scene_identity.hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    Some(hasher.finish())
}

fn hash_rect(hasher: &mut DefaultHasher, rect: Rect) {
    rect.x.to_bits().hash(hasher);
    rect.y.to_bits().hash(hasher);
    rect.width.to_bits().hash(hasher);
    rect.height.to_bits().hash(hasher);
}

fn render_scene_from_document_scene(
    scene: &DocumentRenderScene,
    width: u32,
    height: u32,
) -> RenderScene {
    let viewport = Rect {
        x: scene.viewport.x,
        y: scene.viewport.y,
        width: scene.viewport.width.min(width as f32).max(1.0),
        height: scene.viewport.height.min(height as f32).max(1.0),
    };
    let items = scene
        .items
        .iter()
        .map(|item| RenderSceneItem {
            node: item.node.clone(),
            retained_chunk_id: document_item_retained_chunk_id(item),
            source_kind: format!("{:?}", item.source_kind),
            bounds: item.bounds,
            clip: item.clip,
            transform: item.transform,
            style_identity: item.style_identity,
            dependency_set: item.dependency_set.clone(),
            texture_asset_refs: item.texture_asset_refs.clone(),
            estimated_vertex_count: item.estimated_vertex_count,
        })
        .collect();
    let quad_batches = if scene.quad_batches.is_empty() {
        quad_batches_from_visual_primitives(&scene.visual_primitives, width as f32, height as f32)
    } else {
        scene
            .quad_batches
            .iter()
            .enumerate()
            .map(|(index, batch)| quad_batch_from_document_batch(batch, index))
            .collect()
    };
    RenderScene {
        viewport,
        items,
        rect_metrics: RectVertexMetrics {
            visible_display_item_count: scene.metrics.visible_source_item_count,
            rendered_rect_count: scene.metrics.rendered_rect_count,
            cap_hit: scene.metrics.cap_hit,
        },
        quad_batches,
        text_runs: scene.text_runs.clone(),
    }
}

fn document_item_retained_chunk_id(item: &boon_document::RenderSceneItem) -> String {
    if !item.retained_chunk_id.is_empty() {
        return item.retained_chunk_id.clone();
    }
    format!(
        "chunk:{}:{:?}:{:x}:{:x}:{:x}:{:x}:{:x}",
        item.node.0,
        item.source_kind,
        item.style_identity.style_id,
        item.style_identity.layout_id,
        item.style_identity.paint_id,
        item.style_identity.material_id,
        item.style_identity.pseudo_state_id
    )
}

fn document_primitive_retained_chunk_id(primitive: &RenderVisualPrimitive) -> String {
    if !primitive.retained_chunk_id.is_empty() {
        return primitive.retained_chunk_id.clone();
    }
    format!(
        "chunk:{}:{:?}:{:x}:{:x}:{:x}:{:x}:{:x}",
        primitive.node.0,
        primitive.source_kind,
        primitive.style_identity.style_id,
        primitive.style_identity.layout_id,
        primitive.style_identity.paint_id,
        primitive.style_identity.material_id,
        primitive.style_identity.pseudo_state_id
    )
}

fn quad_batches_from_visual_primitives(
    primitives: &[RenderVisualPrimitive],
    width: f32,
    height: f32,
) -> Vec<QuadBatch> {
    quad_batches_from_visual_primitives_iter(primitives.iter(), width, height)
}

fn quad_batches_from_visual_primitives_iter<'a>(
    primitives: impl IntoIterator<Item = &'a RenderVisualPrimitive>,
    width: f32,
    height: f32,
) -> Vec<QuadBatch> {
    let mut builder = QuadBuilder::default();
    for primitive in primitives {
        builder.set_retained_chunk_id(&document_primitive_retained_chunk_id(primitive));
        match primitive.primitive {
            RenderVisualPrimitiveKind::Asset => {
                if let RenderTextureRef::Asset { url, .. } = &primitive.texture {
                    push_asset_rect(&mut builder, primitive.bounds, width, height, url);
                }
            }
            RenderVisualPrimitiveKind::Border => {
                push_styled_border_all(
                    &mut builder,
                    primitive.bounds,
                    width,
                    height,
                    linear_f32_from_rgba8(primitive.color),
                    primitive.stroke_width,
                    primitive.radius,
                );
            }
            RenderVisualPrimitiveKind::BorderTop
            | RenderVisualPrimitiveKind::BorderRight
            | RenderVisualPrimitiveKind::BorderBottom
            | RenderVisualPrimitiveKind::BorderLeft => {
                let side = match primitive.primitive {
                    RenderVisualPrimitiveKind::BorderTop => 0,
                    RenderVisualPrimitiveKind::BorderRight => 1,
                    RenderVisualPrimitiveKind::BorderBottom => 2,
                    RenderVisualPrimitiveKind::BorderLeft => 3,
                    _ => unreachable!(),
                };
                push_side_border(
                    &mut builder,
                    primitive.bounds,
                    width,
                    height,
                    side,
                    BorderStroke {
                        color: linear_f32_from_rgba8(primitive.color),
                        thickness: primitive.stroke_width,
                    },
                );
            }
            RenderVisualPrimitiveKind::CheckboxCastShadow
            | RenderVisualPrimitiveKind::Checkbox
            | RenderVisualPrimitiveKind::CheckboxInnerShadow
            | RenderVisualPrimitiveKind::CheckboxHighlight => {
                let (center_x, center_y) = checkbox_circle_center(primitive.bounds);
                push_checkbox_circle_raster(
                    &mut builder,
                    center_x,
                    center_y,
                    primitive.radius.max(1.0),
                    primitive.stroke_width.max(0.0),
                    primitive.antialias.max(0.0),
                    width,
                    height,
                    linear_f32_from_rgba8(primitive.color),
                    linear_f32_from_rgba8(primitive.secondary_color),
                );
            }
            RenderVisualPrimitiveKind::CheckboxCheckmark => {
                let (start, middle, end) =
                    if let [start, middle, end, ..] = primitive.control_points.as_slice() {
                        (
                            (start[0], start[1]),
                            (middle[0], middle[1]),
                            (end[0], end[1]),
                        )
                    } else {
                        checkbox_check_points(primitive.bounds)
                    };
                push_checkbox_check_raster(
                    &mut builder,
                    start,
                    middle,
                    end,
                    primitive.stroke_width.max(1.0),
                    primitive.antialias.max(0.0),
                    width,
                    height,
                    linear_f32_from_rgba8(primitive.color),
                );
            }
            RenderVisualPrimitiveKind::ViewportBackground
            | RenderVisualPrimitiveKind::Shadow
            | RenderVisualPrimitiveKind::FrostedMaterialLayer
            | RenderVisualPrimitiveKind::Fill
            | RenderVisualPrimitiveKind::MaterialHighlight
            | RenderVisualPrimitiveKind::EditorSelection
            | RenderVisualPrimitiveKind::EditorBracketHighlight
            | RenderVisualPrimitiveKind::EditorCaret
            | RenderVisualPrimitiveKind::TextInputCaret
            | RenderVisualPrimitiveKind::Underline
            | RenderVisualPrimitiveKind::Strikethrough
            | RenderVisualPrimitiveKind::ButtonCheckmark => {
                push_styled_rect(
                    &mut builder,
                    primitive.bounds,
                    width,
                    height,
                    linear_f32_from_rgba8(primitive.color),
                    primitive.radius,
                );
            }
        }
    }
    builder.batches
}

fn quad_batch_from_document_batch(
    batch: &boon_document::RenderQuadBatch,
    fallback_index: usize,
) -> QuadBatch {
    QuadBatch {
        retained_chunk_id: batch
            .retained_chunk_id
            .clone()
            .unwrap_or_else(|| format!("document-quad-batch:{fallback_index}")),
        texture: quad_texture_from_render_texture_ref(&batch.texture),
        vertices: quad_vertices_from_split_buffers(&batch.positions, &batch.colors, &batch.uvs),
    }
}

fn quad_vertices_from_split_buffers(
    positions: &[f32],
    colors: &[u32],
    uvs: &[f32],
) -> Vec<NativeGpuQuadVertex> {
    debug_assert_eq!(positions.len() % 2, 0);
    debug_assert_eq!(uvs.len() % 2, 0);
    let vertex_count = (positions.len() / 2).min(colors.len()).min(uvs.len() / 2);
    let mut vertices = Vec::with_capacity(vertex_count);
    for index in 0..vertex_count {
        vertices.push(NativeGpuQuadVertex {
            position: [positions[index * 2], positions[index * 2 + 1]],
            color: colors[index],
            uv: [uvs[index * 2], uvs[index * 2 + 1]],
        });
    }
    vertices
}

fn quad_texture_from_render_texture_ref(texture: &RenderTextureRef) -> QuadTexture {
    match texture {
        RenderTextureRef::Solid => QuadTexture::Solid,
        RenderTextureRef::Asset {
            url,
            asset_ref,
            width,
            height,
        } => QuadTexture::Asset(AssetTextureKey {
            url: url.clone(),
            asset_ref: asset_ref.clone(),
            width: *width,
            height: *height,
        }),
    }
}

fn linear_f32_from_rgba8(color: [u8; 4]) -> [f32; 4] {
    [
        srgb_u8_to_linear_f32(color[0]),
        srgb_u8_to_linear_f32(color[1]),
        srgb_u8_to_linear_f32(color[2]),
        color[3] as f32 / 255.0,
    ]
}

fn rect_vertices_from_scene(
    scene: &RenderScene,
    _width: f32,
    _height: f32,
) -> (Vec<QuadBatch>, RectVertexMetrics) {
    debug_assert!(scene.viewport.width >= 0.0);
    debug_assert!(scene.viewport.height >= 0.0);
    (scene.quad_batches.clone(), scene.rect_metrics)
}

#[derive(Debug)]
struct RetainedRenderChunkMetricSummary {
    retained_chunk_count: u32,
    retained_chunk_hit_count: u32,
    retained_chunk_miss_count: u32,
    retained_chunks: Vec<RetainedRenderChunkMetric>,
    current_chunk_ids: BTreeSet<String>,
}

fn sampled_retained_render_chunks(
    scene: &RenderScene,
    generation: u64,
    previous_chunk_ids: Option<&BTreeSet<String>>,
    sample_limit: usize,
) -> RetainedRenderChunkMetricSummary {
    let mut text_run_ids_by_node: BTreeMap<DocumentNodeId, Vec<String>> = BTreeMap::new();
    for run in &scene.text_runs {
        text_run_ids_by_node
            .entry(run.node.clone())
            .or_default()
            .push(text_run_id(run));
    }
    let mut vertex_start = 0_u32;
    let mut current_chunk_ids = BTreeSet::new();
    let mut retained_chunks = Vec::new();
    let mut retained_chunk_count = 0_u32;
    let mut retained_chunk_hit_count = 0_u32;
    let mut retained_chunk_miss_count = 0_u32;

    for item in &scene.items {
        let vertex_count = item.estimated_vertex_count;
        let start = vertex_start;
        vertex_start = vertex_start.saturating_add(vertex_count);
        let id = retained_chunk_id(item, generation);
        current_chunk_ids.insert(id.clone());
        retained_chunk_count = retained_chunk_count.saturating_add(1);
        let cache_hit = previous_chunk_ids.is_some_and(|previous| previous.contains(&id));
        if cache_hit {
            retained_chunk_hit_count = retained_chunk_hit_count.saturating_add(1);
        } else {
            retained_chunk_miss_count = retained_chunk_miss_count.saturating_add(1);
        }

        let should_sample =
            retained_chunks.len() < sample_limit && (!cache_hit || sample_limit > 0);
        if should_sample {
            retained_chunks.push(RetainedRenderChunkMetric {
                id,
                node: item.node.clone(),
                kind: item.source_kind.clone(),
                bounds: item.bounds,
                clip: item.clip,
                transform: item.transform,
                style_identity: item.style_identity,
                dependency_set: item.dependency_set.clone(),
                gpu_buffer_range: start..vertex_start,
                text_run_ids: text_run_ids_by_node
                    .get(&item.node)
                    .cloned()
                    .unwrap_or_default(),
                texture_asset_refs: item.texture_asset_refs.clone(),
                generation,
                cache_status: if cache_hit {
                    "hit".to_owned()
                } else {
                    "miss".to_owned()
                },
            });
        }
    }

    if retained_chunks
        .iter()
        .all(|chunk| chunk.cache_status == "hit")
        && retained_chunk_miss_count > 0
    {
        retained_chunks.clear();
        vertex_start = 0;
        for item in &scene.items {
            let vertex_count = item.estimated_vertex_count;
            let start = vertex_start;
            vertex_start = vertex_start.saturating_add(vertex_count);
            let id = retained_chunk_id(item, generation);
            let cache_hit = previous_chunk_ids.is_some_and(|previous| previous.contains(&id));
            if cache_hit {
                continue;
            }
            retained_chunks.push(RetainedRenderChunkMetric {
                id,
                node: item.node.clone(),
                kind: item.source_kind.clone(),
                bounds: item.bounds,
                clip: item.clip,
                transform: item.transform,
                style_identity: item.style_identity,
                dependency_set: item.dependency_set.clone(),
                gpu_buffer_range: start..vertex_start,
                text_run_ids: text_run_ids_by_node
                    .get(&item.node)
                    .cloned()
                    .unwrap_or_default(),
                texture_asset_refs: item.texture_asset_refs.clone(),
                generation,
                cache_status: "miss".to_owned(),
            });
            if retained_chunks.len() >= sample_limit {
                break;
            }
        }
    }

    RetainedRenderChunkMetricSummary {
        retained_chunk_count,
        retained_chunk_hit_count,
        retained_chunk_miss_count,
        retained_chunks,
        current_chunk_ids,
    }
}

fn retained_chunk_id(item: &RenderSceneItem, _generation: u64) -> String {
    item.retained_chunk_id.clone()
}

fn text_run_id(run: &RenderTextRun) -> String {
    format!(
        "text:{}:{:x}:{:x}:{}",
        run.node.0,
        run.font_id,
        run.paint_id,
        stable_text_hash(&run.text)
    )
}

fn stable_text_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn render_app_owned_scene_pixels(
    request: AppOwnedRenderSceneRequest<'_>,
) -> Result<RenderProof, RenderError> {
    std::fs::create_dir_all(request.artifact_dir).map_err(|error| RenderError {
        message: format!(
            "create native GPU artifact directory `{}`: {error}",
            request.artifact_dir.display()
        ),
    })?;
    let width = request.width.clamp(1, 1920);
    let height = request.height.clamp(1, 1080);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = request.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-app-owned-scene-texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback_size = padded_bytes_per_row as u64 * height as u64;
    let readback = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-scene-readback-buffer"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = request
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-app-owned-scene-encoder"),
        });
    let mut renderer = VisibleLayoutRenderer::new(request.device, request.queue, format);
    let mut metrics = renderer.encode_scene(SurfaceRenderSceneRequest {
        device: request.device,
        queue: request.queue,
        encoder: &mut encoder,
        view: &view,
        scene: request.scene,
        scene_identity: Some(request.render_identity_hash),
        format,
        width,
        height,
    })?;
    metrics.render_scene_source = RENDER_SCENE_SOURCE_APP_OWNED_DOCUMENT_RENDER_SCENE.to_owned();
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission_index = request.queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    request
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index.clone()),
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: readback_scene_failure_message(
                "poll",
                &request,
                width,
                height,
                Some(format!("{submission_index:?}")),
                &error.to_string(),
            ),
        })?;
    receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: readback_scene_failure_message(
                "callback",
                &request,
                width,
                height,
                Some(format!("{submission_index:?}")),
                &error.to_string(),
            ),
        })?
        .map_err(|error| RenderError {
            message: readback_scene_failure_message(
                "map",
                &request,
                width,
                height,
                Some(format!("{submission_index:?}")),
                &error.to_string(),
            ),
        })?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    readback.unmap();

    let nonblank_samples = pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[0] != 0 || rgba[1] != 0 || rgba[2] != 0 || rgba[3] != 0)
        .count();
    let unique_rgba_values = pixels
        .chunks_exact(4)
        .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
        .collect::<BTreeSet<_>>()
        .len();
    let render_identity_hash = request.render_identity_hash.to_owned();
    let render_hash_prefix = render_identity_hash
        .get(..16)
        .unwrap_or(render_identity_hash.as_str());
    let artifact_path = request.artifact_dir.join(format!(
        "{}-{}-{}x{}-{}-{}.png",
        std::process::id(),
        request.artifact_label,
        width,
        height,
        request.scene.items.len(),
        render_hash_prefix
    ));
    image::save_buffer(
        &artifact_path,
        &pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU artifact `{}`: {error}",
            artifact_path.display()
        ),
    })?;
    let artifact_sha256 = sha256_file(&artifact_path)?;
    Ok(RenderProof {
        artifact: RenderProofArtifact::AppOwnedPixels {
            artifact_path: artifact_path.display().to_string(),
            artifact_sha256,
            capture_method: "wgpu-generated-shader-app-owned-render-scene-readback".to_owned(),
            surface_id: request.surface_id,
            surface_epoch: request.surface_epoch,
            frame_seq: 1,
            layout_frame_hash: None,
            render_scene_identity_hash: render_identity_hash,
            width,
            height,
            nonblank_samples,
            unique_rgba_values,
            readback_deadline_ms: APP_OWNED_READBACK_TIMEOUT.as_millis() as u64,
            readback_poll_status: "completed_before_deadline".to_owned(),
        },
        metrics: FrameMetrics { ..metrics },
    })
}

pub fn render_app_owned_world_scene_pixels(
    request: AppOwnedWorldSceneRenderRequest<'_>,
) -> Result<RenderProof, RenderError> {
    let world_identity_hash = world_scene_identity_hash(request.scene);
    let document_scene =
        world_scene_projection_render_scene(request.scene, request.width, request.height);
    let mut proof = render_app_owned_scene_pixels(AppOwnedRenderSceneRequest {
        device: request.device,
        queue: request.queue,
        scene: &document_scene,
        render_identity_hash: &world_identity_hash,
        surface_id: request.surface_id,
        surface_epoch: request.surface_epoch,
        width: request.width,
        height: request.height,
        artifact_dir: request.artifact_dir,
        artifact_label: request.artifact_label,
    })?;
    proof.metrics.render_scene_source =
        RENDER_SCENE_SOURCE_APP_OWNED_WORLD_SCENE_PROJECTION.to_owned();
    if let RenderProofArtifact::AppOwnedPixels { capture_method, .. } = &mut proof.artifact {
        *capture_method =
            "wgpu-generated-shader-app-owned-world-scene-projection-readback".to_owned();
    }
    Ok(proof)
}

pub fn render_app_owned_world_scene_pick_ids(
    scene: &boon_scene_model::WorldScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
) -> Result<WorldScenePickReadbackProof, RenderError> {
    std::fs::create_dir_all(artifact_dir).map_err(|error| RenderError {
        message: format!(
            "create native GPU world pick artifact directory `{}`: {error}",
            artifact_dir.display()
        ),
    })?;
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 1080);
    let render_identity_hash = world_scene_identity_hash(scene);
    let document_scene = world_scene_projection_render_scene(scene, width, height);
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let mut sampled_pick_ids = BTreeSet::new();
    let mut projected_pickable_item_count = 0_usize;
    for item in &document_scene.items {
        let Some(pick_id) = item
            .dependency_set
            .iter()
            .find_map(|dependency| dependency.strip_prefix("world:pick:"))
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        projected_pickable_item_count += 1;
        sampled_pick_ids.insert(pick_id);
        let color = pick_id_rgba(pick_id);
        let x0 = item.bounds.x.max(0.0).floor() as u32;
        let y0 = item.bounds.y.max(0.0).floor() as u32;
        let x1 = (item.bounds.x + item.bounds.width)
            .ceil()
            .clamp(0.0, width as f32) as u32;
        let y1 = (item.bounds.y + item.bounds.height)
            .ceil()
            .clamp(0.0, height as f32) as u32;
        for y in y0.min(height)..y1.min(height) {
            for x in x0.min(width)..x1.min(width) {
                let offset = (y as usize * width as usize + x as usize) * 4;
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    let sampled_pick_ids = sampled_pick_ids.into_iter().collect::<Vec<_>>();
    let render_hash_prefix = render_identity_hash
        .get(..16)
        .unwrap_or(render_identity_hash.as_str());
    let artifact_path = artifact_dir.join(format!(
        "{}-{}-pick-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        sampled_pick_ids.len(),
        render_hash_prefix
    ));
    image::save_buffer(
        &artifact_path,
        &pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world pick artifact `{}`: {error}",
            artifact_path.display()
        ),
    })?;
    let artifact_sha256 = sha256_file(&artifact_path)?;
    Ok(WorldScenePickReadbackProof {
        artifact_path: artifact_path.display().to_string(),
        artifact_sha256,
        capture_method: "app-owned-world-scene-projection-pick-id-readback".to_owned(),
        width,
        height,
        projected_pickable_item_count,
        sampled_pick_id_count: sampled_pick_ids.len(),
        unique_pick_id_count: sampled_pick_ids.len(),
        sampled_pick_ids,
        render_identity_hash,
    })
}

pub fn render_app_owned_world_scene_feature_depth(
    scene: &boon_scene_model::WorldScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
) -> Result<WorldSceneFeatureDepthReadbackProof, RenderError> {
    std::fs::create_dir_all(artifact_dir).map_err(|error| RenderError {
        message: format!(
            "create native GPU world feature/depth artifact directory `{}`: {error}",
            artifact_dir.display()
        ),
    })?;
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 1080);
    let render_identity_hash = world_scene_identity_hash(scene);
    let document_scene = world_scene_projection_render_scene(scene, width, height);
    let visible_depths = scene
        .instances
        .values()
        .filter(|instance| instance.visibility != boon_scene_model::Visibility::Hidden)
        .map(|instance| instance.transform.translation[2])
        .collect::<Vec<_>>();
    let min_projection_depth = visible_depths.iter().copied().fold(f32::INFINITY, f32::min);
    let max_projection_depth = visible_depths
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let mut sampled_feature_ids = BTreeSet::new();
    let mut projected_instance_count = 0_usize;
    for item in &document_scene.items {
        let Some(feature_id) = item
            .dependency_set
            .iter()
            .find_map(|dependency| dependency.strip_prefix("world:feature:"))
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        let Some(instance_depth) = item
            .dependency_set
            .iter()
            .find_map(|dependency| dependency.strip_prefix("world:instance:"))
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|instance_id| {
                scene
                    .instances
                    .get(&boon_scene_model::InstanceId(instance_id))
                    .map(|instance| instance.transform.translation[2])
            })
        else {
            continue;
        };
        projected_instance_count += 1;
        sampled_feature_ids.insert(feature_id);
        let color = feature_depth_rgba(
            feature_id,
            instance_depth,
            min_projection_depth,
            max_projection_depth,
        );
        let x0 = item.bounds.x.max(0.0).floor() as u32;
        let y0 = item.bounds.y.max(0.0).floor() as u32;
        let x1 = (item.bounds.x + item.bounds.width)
            .ceil()
            .clamp(0.0, width as f32) as u32;
        let y1 = (item.bounds.y + item.bounds.height)
            .ceil()
            .clamp(0.0, height as f32) as u32;
        for y in y0.min(height)..y1.min(height) {
            for x in x0.min(width)..x1.min(width) {
                let offset = (y as usize * width as usize + x as usize) * 4;
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    let sampled_feature_ids = sampled_feature_ids.into_iter().collect::<Vec<_>>();
    let render_hash_prefix = render_identity_hash
        .get(..16)
        .unwrap_or(render_identity_hash.as_str());
    let artifact_path = artifact_dir.join(format!(
        "{}-{}-feature-depth-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        sampled_feature_ids.len(),
        render_hash_prefix
    ));
    image::save_buffer(
        &artifact_path,
        &pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world feature/depth artifact `{}`: {error}",
            artifact_path.display()
        ),
    })?;
    let artifact_sha256 = sha256_file(&artifact_path)?;
    Ok(WorldSceneFeatureDepthReadbackProof {
        artifact_path: artifact_path.display().to_string(),
        artifact_sha256,
        capture_method: "app-owned-world-scene-projection-feature-depth-readback".to_owned(),
        width,
        height,
        projected_instance_count,
        sampled_feature_id_count: sampled_feature_ids.len(),
        unique_feature_id_count: sampled_feature_ids.len(),
        sampled_feature_ids,
        min_projection_depth: if min_projection_depth.is_finite() {
            min_projection_depth
        } else {
            0.0
        },
        max_projection_depth: if max_projection_depth.is_finite() {
            max_projection_depth
        } else {
            0.0
        },
        render_identity_hash,
    })
}

pub fn render_app_owned_world_scene_depth_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &boon_scene_model::WorldScene,
    width: u32,
    height: u32,
) -> Result<WorldSceneDepthTargetProof, RenderError> {
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 1080);
    let format = wgpu::TextureFormat::Depth32Float;
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-depth-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-depth-target-view"),
        ..wgpu::TextureViewDescriptor::default()
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("boon-native-gpu-world-scene-depth-target-encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("boon-native-gpu-world-scene-depth-clear-pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene depth target clear pass: {error}"),
        })?;

    Ok(WorldSceneDepthTargetProof {
        capture_method: "app-owned-world-scene-depth-target-clear-pass".to_owned(),
        width,
        height,
        format: format!("{format:?}"),
        sample_count: 1,
        clear_depth: 1.0,
        render_identity_hash: world_scene_identity_hash(scene),
        submitted_pass_count: 1,
    })
}

pub fn render_app_owned_world_scene_mesh_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &boon_scene_model::WorldScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
) -> Result<WorldSceneMeshPipelineProof, RenderError> {
    render_app_owned_world_scene_mesh_pipeline_inner(
        device,
        queue,
        scene,
        None,
        "world-scene-summary-or-primitive",
        "app-owned-world-scene-indexed-mesh-depth-readback",
        width,
        height,
        artifact_dir,
        artifact_label,
        &[],
        false,
    )
}

pub fn render_app_owned_solid_visual_scene_mesh_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    visual: &boon_scene_model::SolidVisualScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
) -> Result<WorldSceneMeshPipelineProof, RenderError> {
    render_app_owned_solid_visual_scene_mesh_pipeline_with_depth_samples(
        device,
        queue,
        visual,
        width,
        height,
        artifact_dir,
        artifact_label,
        &[],
    )
}

pub fn render_app_owned_solid_visual_scene_mesh_pipeline_with_depth_samples(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    visual: &boon_scene_model::SolidVisualScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
    depth_sample_pixels: &[(u32, u32)],
) -> Result<WorldSceneMeshPipelineProof, RenderError> {
    render_app_owned_world_scene_mesh_pipeline_inner(
        device,
        queue,
        &visual.scene,
        Some(&visual.chunks),
        "solid-visual-retained-surface-chunks",
        "app-owned-solid-visual-scene-retained-chunk-mesh-depth-readback",
        width,
        height,
        artifact_dir,
        artifact_label,
        depth_sample_pixels,
        false,
    )
}

pub fn render_app_owned_solid_visual_scene_mesh_pipeline_with_depth_samples_and_chunk_draws(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    visual: &boon_scene_model::SolidVisualScene,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
    depth_sample_pixels: &[(u32, u32)],
) -> Result<WorldSceneMeshPipelineProof, RenderError> {
    render_app_owned_world_scene_mesh_pipeline_inner(
        device,
        queue,
        &visual.scene,
        Some(&visual.chunks),
        "solid-visual-retained-surface-chunks",
        "app-owned-solid-visual-scene-retained-chunk-mesh-depth-readback",
        width,
        height,
        artifact_dir,
        artifact_label,
        depth_sample_pixels,
        true,
    )
}

pub fn encode_world_scene_mesh_pipeline_to_surface(
    request: SurfaceWorldSceneRenderRequest<'_>,
) -> Result<WorldSceneSurfaceMeshRenderProof, RenderError> {
    let width = request.width.clamp(1, 1920);
    let height = request.height.clamp(1, 1080);
    let render_identity_hash = world_scene_identity_hash(request.scene);
    let camera = request
        .scene
        .cameras
        .values()
        .next()
        .ok_or_else(|| RenderError {
            message: "WorldScene surface mesh pipeline requires at least one camera".to_owned(),
        })?;
    let camera_uniform = NativeGpuWorldCameraUniform {
        clip_from_world_rows: camera_clip_from_world_rows(camera, width, height)?,
    };
    let (vertices, indices, mesh_counts) =
        world_scene_mesh_vertices(request.scene, None, camera, width, height)?;
    if vertices.is_empty() || indices.is_empty() {
        return Err(RenderError {
            message: "WorldScene surface mesh pipeline has no supported visible triangles"
                .to_owned(),
        });
    }

    let depth_format = wgpu::TextureFormat::Depth32Float;
    let depth_texture = request.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-surface-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: depth_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-surface-depth-view"),
        ..wgpu::TextureViewDescriptor::default()
    });

    let vertex_bytes: &[u8] = bytemuck::cast_slice(&vertices);
    let vertex_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-surface-vertices"),
        size: vertex_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    request.queue.write_buffer(&vertex_buffer, 0, vertex_bytes);
    let index_bytes: &[u8] = bytemuck::cast_slice(&indices);
    let index_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-surface-indices"),
        size: index_bytes.len() as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    request.queue.write_buffer(&index_buffer, 0, index_bytes);
    let camera_uniform_bytes: &[u8] = bytemuck::bytes_of(&camera_uniform);
    let camera_uniform_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-surface-camera-uniform"),
        size: camera_uniform_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    request
        .queue
        .write_buffer(&camera_uniform_buffer, 0, camera_uniform_bytes);
    let camera_bind_group =
        generated::shader_bindings::world_scene_surface_mesh::WgpuBindGroup0::from_bindings(
            request.device,
            generated::shader_bindings::world_scene_surface_mesh::WgpuBindGroup0Entries::new(
                generated::shader_bindings::world_scene_surface_mesh::WgpuBindGroup0EntriesParams {
                    camera: camera_uniform_buffer.as_entire_buffer_binding(),
                },
            ),
        );

    let shader = generated::shader_bindings::ShaderEntry::WorldSceneSurfaceMesh
        .create_shader_module_embed_source(request.device);
    let pipeline_layout = generated::shader_bindings::ShaderEntry::WorldSceneSurfaceMesh
        .create_pipeline_layout(request.device);
    let vertex_entry = generated::shader_bindings::world_scene_surface_mesh::vs_main_entry(
        wgpu::VertexStepMode::Vertex,
    );
    let fragment_entry =
        generated::shader_bindings::world_scene_surface_mesh::fs_main_entry([Some(
            wgpu::ColorTargetState {
                format: request.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
        )]);
    let pipeline = request
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("boon-native-gpu-world-scene-surface-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: generated::shader_bindings::world_scene_surface_mesh::vertex_state(
                &shader,
                &vertex_entry,
            ),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(
                generated::shader_bindings::world_scene_surface_mesh::fragment_state(
                    &shader,
                    &fragment_entry,
                ),
            ),
            multiview_mask: None,
            cache: None,
        });

    {
        let mut pass = request
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("boon-native-gpu-world-scene-surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: request.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.025,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(&pipeline);
        camera_bind_group.set(&mut pass);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }

    Ok(WorldSceneSurfaceMeshRenderProof {
        capture_method: "visible-surface-world-scene-indexed-mesh-depth-pass".to_owned(),
        camera_projection_method: "shader-camera-uniform-world-to-clip".to_owned(),
        width,
        height,
        color_format: format!("{:?}", request.format),
        depth_format: format!("{depth_format:?}"),
        visible_instance_count: mesh_counts.visible_instance_count,
        rendered_instance_count: mesh_counts.rendered_instance_count,
        unsupported_geometry_count: mesh_counts.unsupported_geometry_count,
        geometry_source: "world-scene-summary-or-primitive".to_owned(),
        retained_chunk_count: mesh_counts.retained_chunk_count,
        retained_chunk_vertex_count: mesh_counts.retained_chunk_vertex_count,
        retained_chunk_index_count: mesh_counts.retained_chunk_index_count,
        vertex_count: vertices.len(),
        index_count: indices.len(),
        triangle_count: indices.len() / 3,
        render_identity_hash,
        visible_surface_rendered: true,
        visible_present_path: true,
    })
}

fn render_app_owned_world_scene_mesh_pipeline_inner(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &boon_scene_model::WorldScene,
    retained_chunks: Option<&[boon_scene_model::SurfaceChunk]>,
    geometry_source: &str,
    capture_method: &str,
    width: u32,
    height: u32,
    artifact_dir: &Path,
    artifact_label: &str,
    depth_sample_pixels: &[(u32, u32)],
    use_chunk_draw_ranges: bool,
) -> Result<WorldSceneMeshPipelineProof, RenderError> {
    std::fs::create_dir_all(artifact_dir).map_err(|error| RenderError {
        message: format!(
            "create native GPU world mesh artifact directory `{}`: {error}",
            artifact_dir.display()
        ),
    })?;
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 1080);
    let render_identity_hash = world_scene_identity_hash(scene);
    let camera = scene.cameras.values().next().ok_or_else(|| RenderError {
        message: "WorldScene mesh pipeline requires at least one camera".to_owned(),
    })?;
    let camera_uniform = NativeGpuWorldCameraUniform {
        clip_from_world_rows: camera_clip_from_world_rows(camera, width, height)?,
    };
    let (vertices, indices, mesh_counts) =
        world_scene_mesh_vertices(scene, retained_chunks, camera, width, height)?;
    if vertices.is_empty() || indices.is_empty() {
        return Err(RenderError {
            message: "WorldScene mesh pipeline has no supported visible triangles".to_owned(),
        });
    }

    let color_format = wgpu::TextureFormat::Rgba8Unorm;
    let feature_format = wgpu::TextureFormat::Rgba8Unorm;
    let normal_format = wgpu::TextureFormat::Rgba8Unorm;
    let depth_format = wgpu::TextureFormat::Depth32Float;
    let color_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: color_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-color-view"),
        ..wgpu::TextureViewDescriptor::default()
    });
    let feature_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-feature-id"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: feature_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let feature_view = feature_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-feature-id-view"),
        ..wgpu::TextureViewDescriptor::default()
    });
    let pick_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-pick-id"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: feature_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let pick_view = pick_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-pick-id-view"),
        ..wgpu::TextureViewDescriptor::default()
    });
    let normal_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-normal"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: normal_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let normal_view = normal_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-normal-view"),
        ..wgpu::TextureViewDescriptor::default()
    });
    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: depth_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-depth-view"),
        ..wgpu::TextureViewDescriptor::default()
    });

    let vertex_bytes: &[u8] = bytemuck::cast_slice(&vertices);
    let vertex_buffer_checksum = fnv1a_bytes(vertex_bytes);
    let vertex_position_buffer_checksum =
        fnv1a_world_mesh_vertex_component(&vertices, |vertex| &vertex.world_position);
    let vertex_color_buffer_checksum =
        fnv1a_world_mesh_vertex_component(&vertices, |vertex| &vertex.color);
    let vertex_normal_buffer_checksum =
        fnv1a_world_mesh_vertex_component(&vertices, |vertex| &vertex.normal_color);
    let vertex_normal_buffer_bit_samples =
        world_mesh_vertex_component_bit_samples(&vertices, |vertex| &vertex.normal_color, 8);
    let vertex_feature_buffer_checksum =
        fnv1a_world_mesh_vertex_component(&vertices, |vertex| &vertex.feature_color);
    let vertex_pick_buffer_checksum =
        fnv1a_world_mesh_vertex_component(&vertices, |vertex| &vertex.pick_color);
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-vertices"),
        size: vertex_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, vertex_bytes);
    let index_bytes: &[u8] = bytemuck::cast_slice(&indices);
    let index_buffer_checksum = fnv1a_bytes(index_bytes);
    let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-indices"),
        size: index_bytes.len() as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&index_buffer, 0, index_bytes);
    let camera_uniform_bytes: &[u8] = bytemuck::bytes_of(&camera_uniform);
    let camera_uniform_checksum = fnv1a_bytes(camera_uniform_bytes);
    let camera_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-camera-uniform"),
        size: camera_uniform_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&camera_uniform_buffer, 0, camera_uniform_bytes);
    let camera_bind_group =
        generated::shader_bindings::world_scene_app_owned_mesh::WgpuBindGroup0::from_bindings(
            device,
            generated::shader_bindings::world_scene_app_owned_mesh::WgpuBindGroup0Entries::new(
                generated::shader_bindings::world_scene_app_owned_mesh::WgpuBindGroup0EntriesParams {
                    camera: camera_uniform_buffer.as_entire_buffer_binding(),
                },
            ),
        );

    let shader = generated::shader_bindings::ShaderEntry::WorldSceneAppOwnedMesh
        .create_shader_module_embed_source(device);
    let pipeline_layout = generated::shader_bindings::ShaderEntry::WorldSceneAppOwnedMesh
        .create_pipeline_layout(device);
    let vertex_entry = generated::shader_bindings::world_scene_app_owned_mesh::vs_main_entry(
        wgpu::VertexStepMode::Vertex,
    );
    let fragment_entry = generated::shader_bindings::world_scene_app_owned_mesh::fs_main_entry([
        Some(wgpu::ColorTargetState {
            format: color_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        }),
        Some(wgpu::ColorTargetState {
            format: normal_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }),
        Some(wgpu::ColorTargetState {
            format: feature_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }),
        Some(wgpu::ColorTargetState {
            format: feature_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }),
    ]);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: generated::shader_bindings::world_scene_app_owned_mesh::vertex_state(
            &shader,
            &vertex_entry,
        ),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(
            generated::shader_bindings::world_scene_app_owned_mesh::fragment_state(
                &shader,
                &fragment_entry,
            ),
        ),
        multiview_mask: None,
        cache: None,
    });

    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback_size = padded_bytes_per_row as u64 * height as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let normal_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-normal-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let feature_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-feature-id-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let pick_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-pick-id-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let depth_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-depth-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("boon-native-gpu-world-scene-mesh-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("boon-native-gpu-world-scene-mesh-pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.025,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &normal_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.5,
                            g: 0.5,
                            b: 1.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &feature_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &pick_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        camera_bind_group.set(&mut pass);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        if use_chunk_draw_ranges && !mesh_counts.draw_ranges.is_empty() {
            for range in &mesh_counts.draw_ranges {
                pass.draw_indexed(
                    range.first_index..range.first_index.saturating_add(range.index_count),
                    range.base_vertex,
                    0..range.instance_count,
                );
            }
        } else {
            pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &normal_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &normal_readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &feature_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &feature_readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &pick_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &pick_readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &depth_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &depth_readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission_index = queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index.clone()),
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene mesh readback: {error}"),
        })?;
    receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!("wait for native GPU world scene mesh readback callback: {error}"),
        })?
        .map_err(|error| RenderError {
            message: format!("map native GPU world scene mesh readback: {error}"),
        })?;
    let normal_slice = normal_readback.slice(..);
    let (normal_sender, normal_receiver) = mpsc::channel();
    normal_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = normal_sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index.clone()),
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene mesh normal readback: {error}"),
        })?;
    normal_receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!(
                "wait for native GPU world scene mesh normal readback callback: {error}"
            ),
        })?
        .map_err(|error| RenderError {
            message: format!("map native GPU world scene mesh normal readback: {error}"),
        })?;
    let feature_slice = feature_readback.slice(..);
    let (feature_sender, feature_receiver) = mpsc::channel();
    feature_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = feature_sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene mesh feature-id readback: {error}"),
        })?;
    feature_receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!(
                "wait for native GPU world scene mesh feature-id readback callback: {error}"
            ),
        })?
        .map_err(|error| RenderError {
            message: format!("map native GPU world scene mesh feature-id readback: {error}"),
        })?;
    let pick_slice = pick_readback.slice(..);
    let (pick_sender, pick_receiver) = mpsc::channel();
    pick_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = pick_sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene mesh pick-id readback: {error}"),
        })?;
    pick_receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!(
                "wait for native GPU world scene mesh pick-id readback callback: {error}"
            ),
        })?
        .map_err(|error| RenderError {
            message: format!("map native GPU world scene mesh pick-id readback: {error}"),
        })?;
    let depth_slice = depth_readback.slice(..);
    let (depth_sender, depth_receiver) = mpsc::channel();
    depth_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = depth_sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!("poll native GPU world scene mesh depth readback: {error}"),
        })?;
    depth_receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!(
                "wait for native GPU world scene mesh depth readback callback: {error}"
            ),
        })?
        .map_err(|error| RenderError {
            message: format!("map native GPU world scene mesh depth readback: {error}"),
        })?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    readback.unmap();
    let normal_mapped = normal_slice.get_mapped_range();
    let mut normal_pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        normal_pixels.extend_from_slice(&normal_mapped[start..end]);
    }
    drop(normal_mapped);
    normal_readback.unmap();
    let feature_mapped = feature_slice.get_mapped_range();
    let mut feature_pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        feature_pixels.extend_from_slice(&feature_mapped[start..end]);
    }
    drop(feature_mapped);
    feature_readback.unmap();
    let pick_mapped = pick_slice.get_mapped_range();
    let mut pick_pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        pick_pixels.extend_from_slice(&pick_mapped[start..end]);
    }
    drop(pick_mapped);
    pick_readback.unmap();
    let depth_mapped = depth_slice.get_mapped_range();
    let mut sampled_depth_pixel_count = 0_usize;
    let mut visible_depth_pixel_count = 0_usize;
    let mut min_depth = f32::INFINITY;
    let mut max_depth = f32::NEG_INFINITY;
    let mut depth_pixel_samples = Vec::new();
    for row in 0..height as usize {
        let row_start = row * padded_bytes_per_row as usize;
        for column in 0..width as usize {
            let start = row_start + column * 4;
            let Some(bytes) = depth_mapped.get(start..start + 4) else {
                continue;
            };
            let depth = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if depth.is_finite() {
                sampled_depth_pixel_count += 1;
                min_depth = min_depth.min(depth);
                max_depth = max_depth.max(depth);
                if (0.0..1.0).contains(&depth) {
                    visible_depth_pixel_count += 1;
                }
            }
        }
    }
    let mut sampled_depth_coordinates = BTreeSet::new();
    for (x, y) in depth_sample_pixels.iter().copied() {
        let x = x.min(width.saturating_sub(1));
        let y = y.min(height.saturating_sub(1));
        if !sampled_depth_coordinates.insert((x, y)) {
            continue;
        }
        let start = y as usize * padded_bytes_per_row as usize + x as usize * 4;
        let Some(bytes) = depth_mapped.get(start..start + 4) else {
            continue;
        };
        let depth = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        depth_pixel_samples.push(WorldSceneDepthPixelSample {
            x,
            y,
            depth,
            finite: depth.is_finite(),
            visible: depth.is_finite() && (0.0..1.0).contains(&depth),
            source: "explicit-probe".to_owned(),
        });
        if depth_pixel_samples.len() >= 64 {
            break;
        }
    }
    drop(depth_mapped);
    depth_readback.unmap();

    let nonblank_samples = pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[0] != 0 || rgba[1] != 0 || rgba[2] != 0 || rgba[3] != 0)
        .count();
    let unique_rgba_values = pixels
        .chunks_exact(4)
        .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
        .collect::<BTreeSet<_>>()
        .len();
    let sampled_normal_pixel_count = normal_pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[3] != 0)
        .count();
    let unique_normal_rgba_values = normal_pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[3] != 0)
        .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
        .collect::<BTreeSet<_>>()
        .len();
    let sampled_feature_ids = feature_pixels
        .chunks_exact(4)
        .filter_map(id_from_rgba8_low)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sampled_pick_ids = pick_pixels
        .chunks_exact(4)
        .filter_map(id_from_rgba8_low)
        .filter_map(|pick_id| u32::try_from(pick_id).ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let hit_test = decoded_feature_hit_test(&feature_pixels, width, height);
    let small_pick_rgba = read_world_mesh_pick_pixel(
        device,
        queue,
        &pick_texture,
        hit_test.x.min(width.saturating_sub(1)),
        hit_test.y.min(height.saturating_sub(1)),
    )?;
    let small_pick_id = id_from_rgba8_low(&small_pick_rgba)
        .and_then(|pick_id| u32::try_from(pick_id).ok().filter(|pick_id| *pick_id != 0));
    let full_pick_offset = ((hit_test.y.min(height.saturating_sub(1)) * width
        + hit_test.x.min(width.saturating_sub(1)))
        * 4) as usize;
    let full_pick_rgba = pick_pixels
        .get(full_pick_offset..full_pick_offset + 4)
        .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
        .unwrap_or([0, 0, 0, 0]);
    let small_pick_matches_full_pick = small_pick_rgba == full_pick_rgba;
    let small_pick_status = if small_pick_matches_full_pick && small_pick_id.is_some() {
        "app-owned-world-scene-mesh-small-pick-readback-pass"
    } else if small_pick_matches_full_pick {
        "app-owned-world-scene-mesh-small-pick-readback-empty"
    } else {
        "app-owned-world-scene-mesh-small-pick-readback-mismatch"
    };
    let render_hash_prefix = render_identity_hash
        .get(..16)
        .unwrap_or(render_identity_hash.as_str());
    let artifact_path = artifact_dir.join(format!(
        "{}-{}-mesh-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        indices.len() / 3,
        render_hash_prefix
    ));
    image::save_buffer(
        &artifact_path,
        &pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world mesh artifact `{}`: {error}",
            artifact_path.display()
        ),
    })?;
    let artifact_sha256 = sha256_file(&artifact_path)?;
    let normal_artifact_path = artifact_dir.join(format!(
        "{}-{}-mesh-normal-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        indices.len() / 3,
        render_hash_prefix
    ));
    image::save_buffer(
        &normal_artifact_path,
        &normal_pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world mesh normal artifact `{}`: {error}",
            normal_artifact_path.display()
        ),
    })?;
    let normal_artifact_sha256 = sha256_file(&normal_artifact_path)?;
    let feature_artifact_path = artifact_dir.join(format!(
        "{}-{}-mesh-feature-id-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        indices.len() / 3,
        render_hash_prefix
    ));
    image::save_buffer(
        &feature_artifact_path,
        &feature_pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world mesh feature-id artifact `{}`: {error}",
            feature_artifact_path.display()
        ),
    })?;
    let feature_artifact_sha256 = sha256_file(&feature_artifact_path)?;
    let pick_artifact_path = artifact_dir.join(format!(
        "{}-{}-mesh-pick-id-{}x{}-{}-{}.png",
        std::process::id(),
        artifact_label,
        width,
        height,
        indices.len() / 3,
        render_hash_prefix
    ));
    image::save_buffer(
        &pick_artifact_path,
        &pick_pixels,
        width,
        height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| RenderError {
        message: format!(
            "save native GPU world mesh pick-id artifact `{}`: {error}",
            pick_artifact_path.display()
        ),
    })?;
    let pick_artifact_sha256 = sha256_file(&pick_artifact_path)?;
    let draw_ranges = if use_chunk_draw_ranges && !mesh_counts.draw_ranges.is_empty() {
        mesh_counts.draw_ranges.clone()
    } else {
        vec![WorldSceneMeshDrawRange {
            first_index: 0,
            index_count: indices.len() as u32,
            base_vertex: 0,
            instance_count: 1,
        }]
    };
    let draw_command_encoding = if use_chunk_draw_ranges && !mesh_counts.draw_ranges.is_empty() {
        "retained-chunk-index-ranges"
    } else {
        "single-draw-all-indices"
    };
    let triangle_probe_samples = world_scene_triangle_probe_samples(
        depth_sample_pixels,
        width,
        height,
        camera_uniform.clip_from_world_rows,
        &vertices,
        &indices,
        &draw_ranges,
        6,
    );

    Ok(WorldSceneMeshPipelineProof {
        artifact_path: artifact_path.display().to_string(),
        artifact_sha256,
        feature_artifact_path: feature_artifact_path.display().to_string(),
        feature_artifact_sha256,
        pick_artifact_path: pick_artifact_path.display().to_string(),
        pick_artifact_sha256,
        normal_artifact_path: normal_artifact_path.display().to_string(),
        normal_artifact_sha256,
        capture_method: capture_method.to_owned(),
        camera_projection_method: "shader-camera-uniform-world-to-clip".to_owned(),
        feature_capture_method: "app-owned-world-scene-mesh-shader-feature-id32-readback"
            .to_owned(),
        normal_capture_method: "app-owned-world-scene-mesh-shader-normal-readback".to_owned(),
        depth_capture_method: "app-owned-world-scene-mesh-depth32float-readback".to_owned(),
        width,
        height,
        color_format: format!("{color_format:?}"),
        feature_format: format!("{feature_format:?}"),
        normal_format: format!("{normal_format:?}"),
        depth_format: format!("{depth_format:?}"),
        primitive_topology: "TriangleList".to_owned(),
        cull_mode: "None".to_owned(),
        front_face: "Ccw".to_owned(),
        depth_compare: "LessEqual".to_owned(),
        depth_write_enabled: true,
        index_format: "Uint32".to_owned(),
        draw_command_encoding: draw_command_encoding.to_owned(),
        draw_call_count: draw_ranges.len(),
        draw_ranges,
        viewport_encoding: "default-full-target".to_owned(),
        scissor_encoding: "default-full-target".to_owned(),
        color_attachment_count: 4,
        depth_attachment_count: 1,
        visible_instance_count: mesh_counts.visible_instance_count,
        rendered_instance_count: mesh_counts.rendered_instance_count,
        unsupported_geometry_count: mesh_counts.unsupported_geometry_count,
        geometry_source: geometry_source.to_owned(),
        retained_chunk_count: mesh_counts.retained_chunk_count,
        retained_chunk_vertex_count: mesh_counts.retained_chunk_vertex_count,
        retained_chunk_index_count: mesh_counts.retained_chunk_index_count,
        vertex_count: vertices.len(),
        index_count: indices.len(),
        triangle_count: indices.len() / 3,
        vertex_buffer_checksum,
        vertex_position_buffer_checksum,
        vertex_color_buffer_checksum,
        vertex_normal_buffer_checksum,
        vertex_normal_buffer_bit_samples,
        vertex_feature_buffer_checksum,
        vertex_pick_buffer_checksum,
        index_buffer_checksum,
        camera_uniform_checksum,
        nonblank_samples,
        unique_rgba_values,
        sampled_normal_pixel_count,
        unique_normal_rgba_values,
        sampled_depth_pixel_count,
        visible_depth_pixel_count,
        min_depth: if min_depth.is_finite() {
            min_depth
        } else {
            0.0
        },
        max_depth: if max_depth.is_finite() {
            max_depth
        } else {
            0.0
        },
        depth_pixel_samples,
        triangle_probe_samples,
        sampled_feature_id_count: sampled_feature_ids.len(),
        unique_feature_id_count: sampled_feature_ids.len(),
        sampled_feature_ids,
        sampled_pick_id_count: sampled_pick_ids.len(),
        unique_pick_id_count: sampled_pick_ids.len(),
        sampled_pick_ids,
        hit_test_capture_method: "app-owned-world-scene-mesh-feature-target-hit-test".to_owned(),
        hit_test_status: if hit_test.feature_id.is_some() {
            "feature-target-hit".to_owned()
        } else {
            "feature-target-miss".to_owned()
        },
        hit_test_x: hit_test.x,
        hit_test_y: hit_test.y,
        hit_test_feature_id: hit_test.feature_id,
        hit_test_sampled_pixel_count: hit_test.sampled_pixel_count,
        small_pick_readback_status: small_pick_status.to_owned(),
        small_pick_readback_capture_method:
            "app-owned-world-scene-mesh-pick-target-copyTextureToBuffer-1x1".to_owned(),
        small_pick_readback_x: hit_test.x,
        small_pick_readback_y: hit_test.y,
        small_pick_readback_width: 1,
        small_pick_readback_height: 1,
        small_pick_readback_logical_bytes: 4,
        small_pick_readback_transfer_bytes: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
        small_pick_readback_rgba: small_pick_rgba,
        small_pick_readback_pick_id: small_pick_id,
        small_pick_readback_matches_full_pick: small_pick_matches_full_pick,
        render_identity_hash,
    })
}

fn read_world_mesh_pick_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pick_texture: &wgpu::Texture,
    x: u32,
    y: u32,
) -> Result<[u8; 4], RenderError> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-world-scene-small-pick-readback"),
        size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("boon-native-gpu-world-scene-small-pick-readback-encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: pick_texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let submission_index = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index.clone()),
            timeout: Some(APP_OWNED_READBACK_TIMEOUT),
        })
        .map_err(|error| RenderError {
            message: format!(
                "poll native GPU world scene 1x1 pick readback: backend=wgpu requested_rect=1x1@{x},{y} report_context=world-scene-mesh-pick deadline_ms={} submission={submission_index:?}: {error}",
                APP_OWNED_READBACK_TIMEOUT.as_millis()
            ),
        })?;
    receiver
        .recv_timeout(APP_OWNED_READBACK_TIMEOUT)
        .map_err(|error| RenderError {
            message: format!(
                "wait for native GPU world scene 1x1 pick readback callback: backend=wgpu requested_rect=1x1@{x},{y} report_context=world-scene-mesh-pick deadline_ms={}: {error}",
                APP_OWNED_READBACK_TIMEOUT.as_millis()
            ),
        })?
        .map_err(|error| RenderError {
            message: format!(
                "map native GPU world scene 1x1 pick readback: backend=wgpu requested_rect=1x1@{x},{y} report_context=world-scene-mesh-pick deadline_ms={}: {error}",
                APP_OWNED_READBACK_TIMEOUT.as_millis()
            ),
        })?;
    let mapped = slice.get_mapped_range();
    let rgba = [mapped[0], mapped[1], mapped[2], mapped[3]];
    drop(mapped);
    buffer.unmap();
    Ok(rgba)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FeatureHitTestSample {
    x: u32,
    y: u32,
    feature_id: Option<u64>,
    sampled_pixel_count: usize,
}

fn decoded_feature_hit_test(
    feature_pixels: &[u8],
    width: u32,
    height: u32,
) -> FeatureHitTestSample {
    let mut hit_pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            let Some(low) = feature_pixels.get(offset..offset + 4) else {
                continue;
            };
            if let Some(feature_id) = id_from_rgba8_low(low) {
                hit_pixels.push((x, y, feature_id));
            }
        }
    }
    let Some((x, y, feature_id)) = hit_pixels
        .get(hit_pixels.len().saturating_sub(1) / 2)
        .copied()
    else {
        return FeatureHitTestSample {
            x: width / 2,
            y: height / 2,
            feature_id: None,
            sampled_pixel_count: 0,
        };
    };
    FeatureHitTestSample {
        x,
        y,
        feature_id: Some(feature_id),
        sampled_pixel_count: hit_pixels.len(),
    }
}

#[derive(Clone, Debug, Default)]
struct WorldMeshBuildCounts {
    visible_instance_count: usize,
    rendered_instance_count: usize,
    unsupported_geometry_count: usize,
    retained_chunk_count: usize,
    retained_chunk_vertex_count: usize,
    retained_chunk_index_count: usize,
    draw_ranges: Vec<WorldSceneMeshDrawRange>,
}

fn world_scene_mesh_vertices(
    scene: &boon_scene_model::WorldScene,
    retained_chunks: Option<&[boon_scene_model::SurfaceChunk]>,
    camera: &boon_scene_model::Camera,
    width: u32,
    height: u32,
) -> Result<
    (
        Vec<NativeGpuWorldMeshVertex>,
        Vec<u32>,
        WorldMeshBuildCounts,
    ),
    RenderError,
> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut counts = WorldMeshBuildCounts::default();
    let retained_chunks_by_geometry = retained_chunks.map(surface_chunks_by_geometry);
    for instance in scene.instances.values() {
        if instance.visibility == boon_scene_model::Visibility::Hidden {
            continue;
        }
        counts.visible_instance_count += 1;
        let Some(geometry) = scene.geometries.get(&instance.geometry) else {
            counts.unsupported_geometry_count += 1;
            continue;
        };
        let Some(material) = scene.appearances.get(&instance.appearance) else {
            counts.unsupported_geometry_count += 1;
            continue;
        };
        let mesh_sources = match retained_chunks_by_geometry
            .as_ref()
            .and_then(|chunks_by_geometry| chunks_by_geometry.get(&instance.geometry))
        {
            Some(sources) => sources.clone(),
            None => mesh_sources_for_geometry(&geometry.kind).unwrap_or_default(),
        };
        if mesh_sources.is_empty() {
            counts.unsupported_geometry_count += 1;
            continue;
        }
        let mut rendered_any_source = false;
        let mut unsupported_source_count = 0_usize;
        for mesh_source in mesh_sources {
            let mut world_positions = Vec::with_capacity(mesh_source.positions.len());
            let mut projectable = true;
            for position in &mesh_source.positions {
                let world = transform_point(instance.transform, *position);
                if project_world_point(camera, world, width, height).is_some() {
                    world_positions.push([world[0], world[1], world[2], 1.0]);
                } else {
                    projectable = false;
                    break;
                }
            }
            if !projectable {
                unsupported_source_count += 1;
                continue;
            }
            let base = vertices.len() as u32;
            let first_index = indices.len() as u32;
            let index_count = mesh_source.indices.len() as u32;
            let color = material.base_color;
            let feature_color = rgba_f32_low_from_u64(instance.feature_id.0);
            let pick_color = rgba_f32_low_from_u64(u64::from(instance.pick_id.0));
            vertices.extend(world_positions.into_iter().zip(&mesh_source.normals).map(
                |(world_position, normal)| NativeGpuWorldMeshVertex {
                    world_position,
                    color,
                    normal_color: normal_rgba_f32(transform_normal(instance.transform, *normal)),
                    feature_color,
                    pick_color,
                },
            ));
            indices.extend(mesh_source.indices.into_iter().map(|index| base + index));
            if index_count > 0 {
                counts.draw_ranges.push(WorldSceneMeshDrawRange {
                    first_index,
                    index_count,
                    base_vertex: 0,
                    instance_count: 1,
                });
            }
            counts.retained_chunk_count += usize::from(mesh_source.retained_chunk);
            counts.retained_chunk_vertex_count +=
                usize::from(mesh_source.retained_chunk) * mesh_source.vertex_count;
            counts.retained_chunk_index_count +=
                usize::from(mesh_source.retained_chunk) * mesh_source.index_count;
            rendered_any_source = true;
        }
        if rendered_any_source {
            counts.rendered_instance_count += 1;
        } else {
            counts.unsupported_geometry_count += unsupported_source_count.max(1);
            continue;
        }
    }
    Ok((vertices, indices, counts))
}

fn rgba_f32_low_from_u64(id: u64) -> [f32; 4] {
    let bytes = id.to_le_bytes();
    [
        bytes[0] as f32 / 255.0,
        bytes[1] as f32 / 255.0,
        bytes[2] as f32 / 255.0,
        bytes[3] as f32 / 255.0,
    ]
}

fn fnv1a_bytes(bytes: &[u8]) -> u32 {
    let mut checksum = 2_166_136_261_u32;
    for byte in bytes {
        checksum ^= u32::from(*byte);
        checksum = checksum.wrapping_mul(16_777_619);
    }
    checksum
}

fn fnv1a_world_mesh_vertex_component(
    vertices: &[NativeGpuWorldMeshVertex],
    component: fn(&NativeGpuWorldMeshVertex) -> &[f32; 4],
) -> u32 {
    let mut checksum = 2_166_136_261_u32;
    for vertex in vertices {
        for byte in bytemuck::bytes_of(component(vertex)) {
            checksum ^= u32::from(*byte);
            checksum = checksum.wrapping_mul(16_777_619);
        }
    }
    checksum
}

fn world_mesh_vertex_component_bit_samples(
    vertices: &[NativeGpuWorldMeshVertex],
    component: fn(&NativeGpuWorldMeshVertex) -> &[f32; 4],
    limit: usize,
) -> Vec<[u32; 4]> {
    vertices
        .iter()
        .take(limit)
        .map(|vertex| component(vertex).map(f32::to_bits))
        .collect()
}

fn world_scene_triangle_probe_samples(
    pixels: &[(u32, u32)],
    width: u32,
    height: u32,
    clip_from_world_rows: [[f32; 4]; 4],
    vertices: &[NativeGpuWorldMeshVertex],
    indices: &[u32],
    draw_ranges: &[WorldSceneMeshDrawRange],
    candidate_limit: usize,
) -> Vec<WorldSceneTriangleProbeSample> {
    pixels
        .iter()
        .copied()
        .take(64)
        .filter(|(x, y)| *x < width && *y < height)
        .map(|(x, y)| {
            let pixel_center = [x as f32 + 0.5, y as f32 + 0.5];
            let mut candidates = Vec::new();
            for (triangle_index, triangle) in indices.chunks_exact(3).enumerate() {
                let vertex_indices = [triangle[0], triangle[1], triangle[2]];
                let Some(candidate) = world_scene_triangle_probe_candidate(
                    triangle_index as u32,
                    vertex_indices,
                    pixel_center,
                    width,
                    height,
                    clip_from_world_rows,
                    vertices,
                    draw_ranges,
                ) else {
                    continue;
                };
                candidates.push(candidate);
            }
            candidates.sort_by(|left, right| {
                left.min_edge_distance_px
                    .total_cmp(&right.min_edge_distance_px)
                    .then_with(|| left.triangle_index.cmp(&right.triangle_index))
            });
            let candidate_count = candidates.len();
            candidates.truncate(candidate_limit);
            WorldSceneTriangleProbeSample {
                x,
                y,
                pixel_center,
                coordinate_convention:
                    "pixel centers are x+0.5/y+0.5; screen x=(ndc.x*0.5+0.5)*width; screen y=(0.5-ndc.y*0.5)*height"
                        .to_owned(),
                candidate_count,
                nearest_triangles: candidates,
            }
        })
        .collect()
}

fn world_scene_triangle_probe_candidate(
    triangle_index: u32,
    vertex_indices: [u32; 3],
    pixel_center: [f32; 2],
    width: u32,
    height: u32,
    clip_from_world_rows: [[f32; 4]; 4],
    vertices: &[NativeGpuWorldMeshVertex],
    draw_ranges: &[WorldSceneMeshDrawRange],
) -> Option<WorldSceneTriangleProbeCandidate> {
    let v0 = *vertices.get(vertex_indices[0] as usize)?;
    let v1 = *vertices.get(vertex_indices[1] as usize)?;
    let v2 = *vertices.get(vertex_indices[2] as usize)?;
    let world_positions = [v0.world_position, v1.world_position, v2.world_position];
    let clip_positions = world_positions.map(|position| {
        mat4_rows_mul_vec4(clip_from_world_rows, position).map(|component| component)
    });
    if clip_positions.iter().any(|position| {
        position[3].abs() <= f32::EPSILON || !position.iter().all(|v| v.is_finite())
    }) {
        return None;
    }
    let ndc_positions = clip_positions.map(|position| {
        [
            position[0] / position[3],
            position[1] / position[3],
            position[2] / position[3],
        ]
    });
    if ndc_positions
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let screen_positions = ndc_positions.map(|position| {
        [
            (position[0] * 0.5 + 0.5) * width as f32,
            (0.5 - position[1] * 0.5) * height as f32,
        ]
    });
    let signed_edge_values = [
        edge_function(screen_positions[1], screen_positions[2], pixel_center),
        edge_function(screen_positions[2], screen_positions[0], pixel_center),
        edge_function(screen_positions[0], screen_positions[1], pixel_center),
    ];
    let edge_distances_px = [
        point_line_distance_px(screen_positions[1], screen_positions[2], pixel_center),
        point_line_distance_px(screen_positions[2], screen_positions[0], pixel_center),
        point_line_distance_px(screen_positions[0], screen_positions[1], pixel_center),
    ];
    let min_edge_distance_px = edge_distances_px
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let triangle_area = edge_function(
        screen_positions[0],
        screen_positions[1],
        screen_positions[2],
    );
    let barycentric = if triangle_area.abs() > f32::EPSILON {
        signed_edge_values.map(|value| value / triangle_area)
    } else {
        [0.0, 0.0, 0.0]
    };
    let edge_epsilon = 0.001;
    let inside_or_on = if triangle_area >= 0.0 {
        signed_edge_values
            .iter()
            .all(|value| *value >= -edge_epsilon)
    } else {
        signed_edge_values
            .iter()
            .all(|value| *value <= edge_epsilon)
    };
    let first_index = triangle_index.saturating_mul(3);
    Some(WorldSceneTriangleProbeCandidate {
        triangle_index,
        draw_range_index: draw_range_index_for_index(first_index, draw_ranges),
        index_offsets: [first_index, first_index + 1, first_index + 2],
        vertex_indices,
        clip_positions,
        ndc_positions,
        screen_positions,
        signed_edge_values,
        edge_distances_px,
        min_edge_distance_px,
        barycentric,
        inside_or_on,
        feature_rgba: rgba8_from_f32(v0.feature_color),
        pick_rgba: rgba8_from_f32(v0.pick_color),
    })
}

fn mat4_rows_mul_vec4(rows: [[f32; 4]; 4], value: [f32; 4]) -> [f32; 4] {
    rows.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2] + row[3] * value[3])
}

fn edge_function(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])
}

fn point_line_distance_px(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    if length <= f32::EPSILON {
        return f32::INFINITY;
    }
    edge_function(a, b, p).abs() / length
}

fn draw_range_index_for_index(
    index_offset: u32,
    draw_ranges: &[WorldSceneMeshDrawRange],
) -> Option<usize> {
    draw_ranges.iter().position(|range| {
        index_offset >= range.first_index
            && index_offset < range.first_index.saturating_add(range.index_count)
    })
}

fn id_from_rgba8_low(low: &[u8]) -> Option<u64> {
    if low.len() < 4 {
        return None;
    }
    if low.iter().all(|byte| *byte == 0) {
        return None;
    }
    let value = u32::from_le_bytes([low[0], low[1], low[2], low[3]]) as u64;
    (value != 0).then_some(value)
}

fn normal_rgba_f32(normal: [f32; 3]) -> [f32; 4] {
    let normal = normalize3(normal).unwrap_or([0.0, 0.0, 1.0]);
    [
        normal[0] * 0.5 + 0.5,
        normal[1] * 0.5 + 0.5,
        normal[2] * 0.5 + 0.5,
        1.0,
    ]
}

fn transform_normal(transform: boon_scene_model::Transform3D, normal: [f32; 3]) -> [f32; 3] {
    normalize3(rotate_vector_by_quaternion(transform.rotation_xyzw, normal)).unwrap_or(normal)
}

fn positions_center(positions: &[[f32; 3]]) -> [f32; 3] {
    if positions.is_empty() {
        return [0.0, 0.0, 0.0];
    }
    let mut sum = [0.0, 0.0, 0.0];
    for position in positions {
        sum[0] += position[0];
        sum[1] += position[1];
        sum[2] += position[2];
    }
    let count = positions.len() as f32;
    [sum[0] / count, sum[1] / count, sum[2] / count]
}

fn normal_from_center_f32(position: [f32; 3], center: [f32; 3]) -> [f32; 3] {
    normalize3([
        position[0] - center[0],
        position[1] - center[1],
        position[2] - center[2],
    ])
    .unwrap_or([0.0, 0.0, 1.0])
}

fn normalize3(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    (length > f32::EPSILON).then(|| [value[0] / length, value[1] / length, value[2] / length])
}

#[derive(Clone, Debug)]
struct MeshSource {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    retained_chunk: bool,
    vertex_count: usize,
    index_count: usize,
}

fn surface_chunks_by_geometry(
    chunks: &[boon_scene_model::SurfaceChunk],
) -> BTreeMap<boon_scene_model::GeometryLogicalId, Vec<MeshSource>> {
    let mut by_geometry = BTreeMap::new();
    for chunk in chunks {
        let boon_scene_model::SurfaceRepresentation::IndexedMesh(mesh) = &chunk.representation
        else {
            continue;
        };
        by_geometry
            .entry(chunk.id.geometry)
            .or_insert_with(Vec::new)
            .push(MeshSource {
                positions: mesh.vertices.iter().map(|vertex| vertex.position).collect(),
                normals: mesh.vertices.iter().map(|vertex| vertex.normal).collect(),
                indices: mesh.indices.clone(),
                retained_chunk: true,
                vertex_count: mesh.vertices.len(),
                index_count: mesh.indices.len(),
            });
    }
    by_geometry
}

fn mesh_sources_for_geometry(geometry: &boon_scene_model::GeometryKind) -> Option<Vec<MeshSource>> {
    match geometry {
        boon_scene_model::GeometryKind::SharedPrimitive(
            boon_scene_model::PrimitiveGeometry::Cube { size },
        ) => Some(vec![mesh_source_from_parts(cube_positions_and_indices(
            *size,
        ))]),
        boon_scene_model::GeometryKind::IndexedMeshSummary { bounds, .. } => {
            Some(vec![mesh_source_from_parts(bounds_positions_and_indices(
                *bounds,
            ))])
        }
        boon_scene_model::GeometryKind::SharedPrimitive(
            boon_scene_model::PrimitiveGeometry::Sphere { .. }
            | boon_scene_model::PrimitiveGeometry::Cylinder { .. },
        ) => None,
    }
}

fn mesh_source_from_parts((positions, indices): (Vec<[f32; 3]>, Vec<u32>)) -> MeshSource {
    let center = positions_center(&positions);
    let normals = positions
        .iter()
        .map(|position| normal_from_center_f32(*position, center))
        .collect::<Vec<_>>();
    MeshSource {
        vertex_count: positions.len(),
        index_count: indices.len(),
        positions,
        normals,
        indices,
        retained_chunk: false,
    }
}

fn cube_positions_and_indices(size: [f32; 3]) -> (Vec<[f32; 3]>, Vec<u32>) {
    let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    bounds_positions_and_indices(boon_scene_model::Bounds3D {
        min: [-half[0], -half[1], -half[2]],
        max: [half[0], half[1], half[2]],
    })
}

fn bounds_positions_and_indices(bounds: boon_scene_model::Bounds3D) -> (Vec<[f32; 3]>, Vec<u32>) {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    (
        vec![
            [min_x, min_y, min_z],
            [max_x, min_y, min_z],
            [max_x, max_y, min_z],
            [min_x, max_y, min_z],
            [min_x, min_y, max_z],
            [max_x, min_y, max_z],
            [max_x, max_y, max_z],
            [min_x, max_y, max_z],
        ],
        vec![
            0, 1, 2, 0, 2, 3, // back
            4, 6, 5, 4, 7, 6, // front
            0, 4, 5, 0, 5, 1, // bottom
            3, 2, 6, 3, 6, 7, // top
            1, 5, 6, 1, 6, 2, // right
            0, 3, 7, 0, 7, 4, // left
        ],
    )
}

fn transform_point(transform: boon_scene_model::Transform3D, point: [f32; 3]) -> [f32; 3] {
    let scaled = [
        point[0] * transform.scale[0],
        point[1] * transform.scale[1],
        point[2] * transform.scale[2],
    ];
    let rotated = rotate_vector_by_quaternion(transform.rotation_xyzw, scaled);
    [
        rotated[0] + transform.translation[0],
        rotated[1] + transform.translation[1],
        rotated[2] + transform.translation[2],
    ]
}

fn camera_clip_from_world_rows(
    camera: &boon_scene_model::Camera,
    width: u32,
    height: u32,
) -> Result<[[f32; 4]; 4], RenderError> {
    let [camera_x, camera_y, camera_z] = camera_space_from_world_rows(camera.transform);
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let rows = match camera.projection {
        boon_scene_model::CameraProjection::Perspective {
            vertical_fov_degrees,
            near,
            far,
        } => {
            let tan_half = (vertical_fov_degrees.to_radians() * 0.5).tan();
            if tan_half <= f32::EPSILON || aspect <= f32::EPSILON || far <= near {
                return Err(RenderError {
                    message: format!(
                        "invalid WorldScene perspective camera projection: fov={vertical_fov_degrees}, near={near}, far={far}, aspect={aspect}"
                    ),
                });
            }
            let z_a = -far / (far - near);
            let z_b = -(far * near) / (far - near);
            [
                scale4(camera_x, 1.0 / (tan_half * aspect)),
                scale4(camera_y, 1.0 / tan_half),
                add4(scale4(camera_z, z_a), [0.0, 0.0, 0.0, z_b]),
                scale4(camera_z, -1.0),
            ]
        }
        boon_scene_model::CameraProjection::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            if vertical_size <= f32::EPSILON || aspect <= f32::EPSILON || far <= near {
                return Err(RenderError {
                    message: format!(
                        "invalid WorldScene orthographic camera projection: vertical_size={vertical_size}, near={near}, far={far}, aspect={aspect}"
                    ),
                });
            }
            let half_height = vertical_size * 0.5;
            let half_width = half_height * aspect;
            [
                scale4(camera_x, 1.0 / half_width),
                scale4(camera_y, 1.0 / half_height),
                add4(
                    scale4(camera_z, -1.0 / (far - near)),
                    [0.0, 0.0, 0.0, -near / (far - near)],
                ),
                [0.0, 0.0, 0.0, 1.0],
            ]
        }
    };
    if rows.iter().flatten().all(|component| component.is_finite()) {
        Ok(rows)
    } else {
        Err(RenderError {
            message: "WorldScene camera matrix contains non-finite values".to_owned(),
        })
    }
}

fn camera_space_from_world_rows(transform: boon_scene_model::Transform3D) -> [[f32; 4]; 3] {
    let inverse_rotation = inverse_unit_quaternion(transform.rotation_xyzw);
    let world_x_in_camera = rotate_vector_by_quaternion(inverse_rotation, [1.0, 0.0, 0.0]);
    let world_y_in_camera = rotate_vector_by_quaternion(inverse_rotation, [0.0, 1.0, 0.0]);
    let world_z_in_camera = rotate_vector_by_quaternion(inverse_rotation, [0.0, 0.0, 1.0]);
    let row_x = [
        world_x_in_camera[0],
        world_y_in_camera[0],
        world_z_in_camera[0],
    ];
    let row_y = [
        world_x_in_camera[1],
        world_y_in_camera[1],
        world_z_in_camera[1],
    ];
    let row_z = [
        world_x_in_camera[2],
        world_y_in_camera[2],
        world_z_in_camera[2],
    ];
    [
        affine_row_from_linear(row_x, transform.translation),
        affine_row_from_linear(row_y, transform.translation),
        affine_row_from_linear(row_z, transform.translation),
    ]
}

fn affine_row_from_linear(row: [f32; 3], translation: [f32; 3]) -> [f32; 4] {
    [
        row[0],
        row[1],
        row[2],
        -(row[0] * translation[0] + row[1] * translation[1] + row[2] * translation[2]),
    ]
}

fn scale4(value: [f32; 4], scale: f32) -> [f32; 4] {
    [
        value[0] * scale,
        value[1] * scale,
        value[2] * scale,
        value[3] * scale,
    ]
}

fn add4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

fn project_world_point(
    camera: &boon_scene_model::Camera,
    world: [f32; 3],
    width: u32,
    height: u32,
) -> Option<[f32; 4]> {
    let relative = [
        world[0] - camera.transform.translation[0],
        world[1] - camera.transform.translation[1],
        world[2] - camera.transform.translation[2],
    ];
    let camera_space = rotate_vector_by_quaternion(
        inverse_unit_quaternion(camera.transform.rotation_xyzw),
        relative,
    );
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    match camera.projection {
        boon_scene_model::CameraProjection::Perspective {
            vertical_fov_degrees,
            near,
            far,
        } => {
            let depth = -camera_space[2];
            if depth <= near || depth >= far {
                return None;
            }
            let tan_half = (vertical_fov_degrees.to_radians() * 0.5).tan();
            if tan_half <= f32::EPSILON || aspect <= f32::EPSILON {
                return None;
            }
            let x = camera_space[0] / (depth * tan_half * aspect);
            let y = camera_space[1] / (depth * tan_half);
            let z = ((depth - near) / (far - near)).clamp(0.0, 1.0);
            finite_clip_position([x, y, z, 1.0])
        }
        boon_scene_model::CameraProjection::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            let depth = -camera_space[2];
            if depth <= near || depth >= far || vertical_size <= f32::EPSILON {
                return None;
            }
            let half_height = vertical_size * 0.5;
            let half_width = half_height * aspect;
            let x = camera_space[0] / half_width;
            let y = camera_space[1] / half_height;
            let z = ((depth - near) / (far - near)).clamp(0.0, 1.0);
            finite_clip_position([x, y, z, 1.0])
        }
    }
}

fn finite_clip_position(position: [f32; 4]) -> Option<[f32; 4]> {
    position
        .iter()
        .all(|value| value.is_finite())
        .then_some(position)
}

fn inverse_unit_quaternion(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn rotate_vector_by_quaternion(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let qv = [q[0], q[1], q[2]];
    let uv = cross(qv, v);
    let uuv = cross(qv, uv);
    [
        v[0] + (uv[0] * q[3] + uuv[0]) * 2.0,
        v[1] + (uv[1] * q[3] + uuv[1]) * 2.0,
        v[2] + (uv[2] * q[3] + uuv[2]) * 2.0,
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn pick_id_rgba(pick_id: u32) -> [u8; 4] {
    [
        (pick_id & 0xff) as u8,
        ((pick_id >> 8) & 0xff) as u8,
        ((pick_id >> 16) & 0xff) as u8,
        255,
    ]
}

fn feature_depth_rgba(feature_id: u64, depth: f32, min_depth: f32, max_depth: f32) -> [u8; 4] {
    let depth_alpha = if min_depth.is_finite()
        && max_depth.is_finite()
        && (max_depth - min_depth).abs() > f32::EPSILON
    {
        let normalized = ((depth - min_depth) / (max_depth - min_depth)).clamp(0.0, 1.0);
        (normalized * 254.0).round() as u8 + 1
    } else {
        128
    };
    [
        (feature_id & 0xff) as u8,
        ((feature_id >> 8) & 0xff) as u8,
        ((feature_id >> 16) & 0xff) as u8,
        depth_alpha,
    ]
}

fn world_scene_identity_hash(scene: &boon_scene_model::WorldScene) -> String {
    let bytes = serde_json::to_vec(scene).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn generated_shader_wesl_hash(path: &str) -> String {
    generated::shader_bindings::NATIVE_GPU_SHADER_WESL_SHA256S
        .iter()
        .find_map(|(shader_path, hash)| (*shader_path == path).then_some((*hash).to_owned()))
        .unwrap_or_default()
}

fn world_scene_projection_render_scene(
    scene: &boon_scene_model::WorldScene,
    width: u32,
    height: u32,
) -> DocumentRenderScene {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let mut items = Vec::new();
    let mut visual_primitives = Vec::new();
    for (index, instance) in scene.instances.values().enumerate() {
        if instance.visibility == boon_scene_model::Visibility::Hidden {
            continue;
        }
        let node = DocumentNodeId(format!("world-instance-{}", instance.id.0));
        let retained_chunk_id = format!(
            "chunk:world:instance:{}:geometry:{}:material:{}",
            instance.id.0, instance.geometry.0, instance.appearance.0
        );
        let size = world_geometry_projected_extent(scene, instance.geometry);
        let scale = instance.transform.scale;
        let scale_factor = ((scale[0].abs() + scale[1].abs() + scale[2].abs()) / 3.0).max(0.1);
        let extent = (size * scale_factor * 44.0).clamp(24.0, 180.0);
        let depth = (extent * 0.24).clamp(8.0, 42.0);
        let translation = instance.transform.translation;
        let center_x = width as f32 * 0.5 + translation[0] * 28.0 + index as f32 * 6.0;
        let center_y = height as f32 * 0.54 - translation[1] * 28.0 - translation[2] * 2.0;
        let bounds = Rect {
            x: (center_x - extent * 0.5).clamp(0.0, width.saturating_sub(1) as f32),
            y: (center_y - extent * 0.5).clamp(0.0, height.saturating_sub(1) as f32),
            width: extent.min(width.max(1) as f32),
            height: extent.min(height.max(1) as f32),
        };
        let style_identity = world_style_identity(instance);
        let dependency_set = vec![
            format!("world:instance:{}", instance.id.0),
            format!("world:geometry:{}", instance.geometry.0),
            format!("world:material:{}", instance.appearance.0),
            format!("world:pick:{}", instance.pick_id.0),
            format!("world:feature:{}", instance.feature_id.0),
        ];
        items.push(boon_document::RenderSceneItem {
            node: node.clone(),
            retained_chunk_id: retained_chunk_id.clone(),
            source_kind: DocumentNodeKind::Stack,
            bounds,
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, bounds.x, bounds.y],
            style_identity,
            dependency_set: dependency_set.clone(),
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 18,
        });
        let base = world_instance_color(scene, instance);
        let top = shade_color(base, 1.18);
        let side = shade_color(base, 0.78);
        let shadow = shade_color(base, 0.42);
        visual_primitives.push(world_fill_primitive(
            &node,
            &retained_chunk_id,
            Rect {
                x: bounds.x + depth * 0.35,
                y: bounds.y + bounds.height + depth * 0.12,
                width: bounds.width,
                height: depth * 0.36,
            },
            [shadow[0], shadow[1], shadow[2], 96],
            style_identity,
            dependency_set.clone(),
        ));
        visual_primitives.push(world_fill_primitive(
            &node,
            &retained_chunk_id,
            Rect {
                x: bounds.x + depth,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            side,
            style_identity,
            dependency_set.clone(),
        ));
        visual_primitives.push(world_fill_primitive(
            &node,
            &retained_chunk_id,
            Rect {
                x: bounds.x + depth * 0.5,
                y: bounds.y - depth * 0.5,
                width: bounds.width,
                height: depth,
            },
            top,
            style_identity,
            dependency_set.clone(),
        ));
        visual_primitives.push(world_fill_primitive(
            &node,
            &retained_chunk_id,
            bounds,
            base,
            style_identity,
            dependency_set,
        ));
        if scene
            .selection
            .as_ref()
            .is_some_and(|selection| selection.instance == instance.id)
        {
            let mut selection_dependencies = vec![
                "world:selection".to_owned(),
                format!("world:selection:instance:{}", instance.id.0),
                format!("world:selection:pick:{}", instance.pick_id.0),
            ];
            selection_dependencies.extend([
                format!("world:instance:{}", instance.id.0),
                format!("world:geometry:{}", instance.geometry.0),
                format!("world:material:{}", instance.appearance.0),
                format!("world:pick:{}", instance.pick_id.0),
                format!("world:feature:{}", instance.feature_id.0),
            ]);
            let outline_margin = 4.0;
            let outline_bounds = Rect {
                x: (bounds.x - outline_margin).max(0.0),
                y: (bounds.y - depth * 0.5 - outline_margin).max(0.0),
                width: (bounds.width + depth + outline_margin * 2.0).min(width.max(1) as f32),
                height: (bounds.height + depth * 0.5 + outline_margin * 2.0)
                    .min(height.max(1) as f32),
            };
            visual_primitives.push(world_selection_outline_primitive(
                &node,
                &format!("chunk:world:selection:instance:{}", instance.id.0),
                outline_bounds,
                style_identity,
                selection_dependencies,
            ));
        }
    }
    let metrics = boon_document::RenderSceneMetrics {
        visible_source_item_count: items.len() as u32,
        visual_primitive_count: visual_primitives.len() as u32,
        rendered_rect_count: visual_primitives.len() as u32,
        cap_hit: false,
    };
    DocumentRenderScene {
        viewport,
        items,
        visual_primitives,
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics,
    }
}

fn world_fill_primitive(
    node: &DocumentNodeId,
    retained_chunk_id: &str,
    bounds: Rect,
    color: [u8; 4],
    style_identity: boon_document::ComputedStyleIdentity,
    dependency_set: Vec<String>,
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: node.clone(),
        retained_chunk_id: retained_chunk_id.to_owned(),
        source_kind: DocumentNodeKind::Stack,
        primitive: RenderVisualPrimitiveKind::Fill,
        bounds,
        clip: None,
        radius: 2.0,
        stroke_width: 0.0,
        color,
        secondary_color: [0, 0, 0, 0],
        antialias: 0.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity,
        dependency_set,
    }
}

fn world_selection_outline_primitive(
    node: &DocumentNodeId,
    retained_chunk_id: &str,
    bounds: Rect,
    style_identity: boon_document::ComputedStyleIdentity,
    dependency_set: Vec<String>,
) -> RenderVisualPrimitive {
    RenderVisualPrimitive {
        node: node.clone(),
        retained_chunk_id: retained_chunk_id.to_owned(),
        source_kind: DocumentNodeKind::Stack,
        primitive: RenderVisualPrimitiveKind::Border,
        bounds,
        clip: None,
        radius: 5.0,
        stroke_width: 3.0,
        color: [255, 214, 10, 255],
        secondary_color: [0, 0, 0, 0],
        antialias: 1.0,
        control_points: Vec::new(),
        texture: RenderTextureRef::Solid,
        style_identity,
        dependency_set,
    }
}

fn world_geometry_projected_extent(
    scene: &boon_scene_model::WorldScene,
    geometry_id: boon_scene_model::GeometryLogicalId,
) -> f32 {
    let Some(geometry) = scene.geometries.get(&geometry_id) else {
        return 1.0;
    };
    match &geometry.kind {
        boon_scene_model::GeometryKind::SharedPrimitive(primitive) => match primitive {
            boon_scene_model::PrimitiveGeometry::Cube { size } => {
                ((size[0].abs() + size[1].abs() + size[2].abs()) / 3.0).max(0.1)
            }
            boon_scene_model::PrimitiveGeometry::Sphere { radius, .. } => (*radius * 2.0).max(0.1),
            boon_scene_model::PrimitiveGeometry::Cylinder { radius, height, .. } => {
                ((*radius * 2.0 + height.abs()) * 0.5).max(0.1)
            }
        },
        boon_scene_model::GeometryKind::IndexedMeshSummary { bounds, .. } => {
            let x = (bounds.max[0] - bounds.min[0]).abs();
            let y = (bounds.max[1] - bounds.min[1]).abs();
            let z = (bounds.max[2] - bounds.min[2]).abs();
            ((x + y + z) / 3.0).max(0.1)
        }
    }
}

fn world_instance_color(
    scene: &boon_scene_model::WorldScene,
    instance: &boon_scene_model::ModelInstance,
) -> [u8; 4] {
    let base = scene
        .appearances
        .get(&instance.appearance)
        .map(|appearance| appearance.base_color)
        .unwrap_or([0.2, 0.55, 0.95, 1.0]);
    [
        unit_float_to_u8(base[0]),
        unit_float_to_u8(base[1]),
        unit_float_to_u8(base[2]),
        unit_float_to_u8(base[3]),
    ]
}

fn unit_float_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn shade_color(color: [u8; 4], factor: f32) -> [u8; 4] {
    [
        ((color[0] as f32 * factor).clamp(0.0, 255.0)).round() as u8,
        ((color[1] as f32 * factor).clamp(0.0, 255.0)).round() as u8,
        ((color[2] as f32 * factor).clamp(0.0, 255.0)).round() as u8,
        color[3],
    ]
}

fn world_style_identity(
    instance: &boon_scene_model::ModelInstance,
) -> boon_document::ComputedStyleIdentity {
    boon_document::ComputedStyleIdentity {
        style_id: instance.id.0,
        layout_id: instance.geometry.0,
        paint_id: instance.appearance.0,
        material_id: instance.appearance.0,
        font_id: 0,
        pseudo_state_id: u64::from(instance.pick_id.0),
    }
}

fn readback_scene_failure_message(
    phase: &str,
    request: &AppOwnedRenderSceneRequest<'_>,
    width: u32,
    height: u32,
    submission_index: Option<String>,
    reason: &str,
) -> String {
    format!(
        "native GPU readback {phase} failed before deadline: backend=wgpu adapter=unavailable frame_id={} surface={} requested_rect=0,0,{width},{height} submission={}; report_context=app_owned_render_scene_pixels artifact_label={} deadline_ms={} reason={reason}",
        request.render_identity_hash,
        request.surface_id.0,
        submission_index.unwrap_or_else(|| "unsubmitted".to_owned()),
        request.artifact_label,
        APP_OWNED_READBACK_TIMEOUT.as_millis(),
    )
}

struct GlyphonTextState {
    service: GlyphonTextService,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    buffers: Vec<Buffer>,
    buffer_signatures: Vec<TextRunSignature>,
    prepared_signatures: Vec<TextRunPlacementSignature>,
    prepared_viewport: Option<(u32, u32)>,
    custom_glyph_ids: BTreeMap<RotatedTextKey, CustomGlyphId>,
    custom_glyph_rasters: BTreeMap<CustomGlyphId, RasterizedCustomGlyph>,
    next_custom_glyph_id: CustomGlyphId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RichTextSpan {
    text: String,
    color: [u8; 4],
    font_style: Style,
    font_weight: Weight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRunSignature {
    font_id: u64,
    paint_id: u64,
    text: String,
    rich_spans: Vec<RichTextSpan>,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    font_features: String,
    text_inset: u32,
    text_clip_padding: u32,
    line_height: u32,
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
    vertical_align: TextVerticalAlign,
    rotate_degrees: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRunPlacementSignature {
    text: String,
    rich_spans: Vec<RichTextSpan>,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    font_features: String,
    text_inset: u32,
    text_clip_padding: u32,
    line_height: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
    vertical_align: TextVerticalAlign,
    rotate_degrees: u32,
    clip_x: Option<u32>,
    clip_y: Option<u32>,
    clip_width: Option<u32>,
    clip_height: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct RotatedTextKey {
    text: String,
    font_family: String,
    font_style: u8,
    font_weight: u16,
    font_features: String,
    size: u32,
    line_height: u32,
    rotate_degrees: u32,
}

struct RotatedTextGlyph {
    key: RotatedTextKey,
    mask: Vec<u8>,
    width: u16,
    height: u16,
    left: f32,
    top: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TextFrameCacheMetrics {
    cache_hits: u32,
    cache_misses: u32,
    cache_evictions: u32,
    cache_entry_count: u32,
    cache_capacity: u32,
    cache_memory_bytes: u64,
    missing_glyph_count: u32,
    glyph_atlas_prepare_count: u32,
    glyph_atlas_evictions_observed: u32,
}

impl TextRunSignature {
    fn from_run(run: &TextRun) -> Self {
        Self {
            font_id: run.font_id,
            paint_id: run.paint_id,
            text: run.text.clone(),
            rich_spans: run.rich_spans.clone(),
            font_family: run.font_family.clone(),
            font_style: run.font_style,
            font_weight: run.font_weight,
            font_features: run.font_features.clone(),
            text_inset: run.text_inset.to_bits(),
            text_clip_padding: run.text_clip_padding.to_bits(),
            line_height: run.line_height.to_bits(),
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
            vertical_align: run.vertical_align,
            rotate_degrees: run.rotate_degrees,
        }
    }
}

fn text_buffer_cache_memory_bytes(signatures: &[TextRunSignature]) -> u64 {
    signatures
        .iter()
        .map(|signature| {
            let span_bytes = signature
                .rich_spans
                .iter()
                .map(|span| span.text.len() as u64 + 32)
                .sum::<u64>();
            std::mem::size_of::<TextRunSignature>() as u64
                + signature.text.len() as u64
                + signature.font_family.len() as u64
                + signature.font_features.len() as u64
                + span_bytes
        })
        .sum()
}

impl TextRunPlacementSignature {
    fn from_run(run: &TextRun) -> Self {
        Self {
            text: run.text.clone(),
            rich_spans: run.rich_spans.clone(),
            font_family: run.font_family.clone(),
            font_style: run.font_style,
            font_weight: run.font_weight,
            font_features: run.font_features.clone(),
            text_inset: run.text_inset.to_bits(),
            text_clip_padding: run.text_clip_padding.to_bits(),
            line_height: run.line_height.to_bits(),
            x: run.bounds.x.to_bits(),
            y: run.bounds.y.to_bits(),
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
            vertical_align: run.vertical_align,
            rotate_degrees: run.rotate_degrees,
            clip_x: run.clip.map(|clip| clip.x.to_bits()),
            clip_y: run.clip.map(|clip| clip.y.to_bits()),
            clip_width: run.clip.map(|clip| clip.width.to_bits()),
            clip_height: run.clip.map(|clip| clip.height.to_bits()),
        }
    }
}

impl GlyphonTextState {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            service: GlyphonTextService::new(),
            viewport,
            atlas,
            renderer,
            buffers: Vec::new(),
            buffer_signatures: Vec::new(),
            prepared_signatures: Vec::new(),
            prepared_viewport: None,
            custom_glyph_ids: BTreeMap::new(),
            custom_glyph_rasters: BTreeMap::new(),
            next_custom_glyph_id: 1,
        }
    }

    fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        runs: Vec<TextRun>,
        width: u32,
        height: u32,
    ) -> Result<(u32, TextFrameCacheMetrics), RenderError> {
        if runs.is_empty() {
            return Ok((0, TextFrameCacheMetrics::default()));
        }
        self.viewport.update(queue, Resolution { width, height });
        let placement_signatures = runs
            .iter()
            .map(TextRunPlacementSignature::from_run)
            .collect::<Vec<_>>();
        let mut normal_runs = Vec::new();
        let mut rotated_runs = Vec::new();
        for run in runs {
            if is_rotated_quarter_text_run(&run) {
                if let Some(glyph) = self.rotated_text_glyph(&run) {
                    rotated_runs.push((run, glyph));
                } else {
                    normal_runs.push(run);
                }
            } else {
                normal_runs.push(run);
            }
        }
        let mut text_cache_metrics = self.ensure_buffers(&normal_runs);
        if self.prepared_signatures != placement_signatures
            || self.prepared_viewport != Some((width, height))
        {
            text_cache_metrics.glyph_atlas_prepare_count = text_cache_metrics
                .glyph_atlas_prepare_count
                .saturating_add(1);
            let mut custom_buffers = Vec::with_capacity(rotated_runs.len());
            let mut custom_glyph_lists = Vec::with_capacity(rotated_runs.len());
            for (run, glyph) in &rotated_runs {
                let id = self.custom_glyph_id(glyph.key.clone());
                self.custom_glyph_rasters.insert(
                    id,
                    RasterizedCustomGlyph {
                        data: glyph.mask.clone(),
                        content_type: ContentType::Mask,
                    },
                );
                custom_glyph_lists.push(vec![CustomGlyph {
                    id,
                    left: glyph.left,
                    top: glyph.top,
                    width: f32::from(glyph.width),
                    height: f32::from(glyph.height),
                    color: Some(Color::rgba(
                        run.color[0],
                        run.color[1],
                        run.color[2],
                        run.color[3],
                    )),
                    snap_to_physical_pixel: true,
                    metadata: 0,
                }]);
                custom_buffers.push(self.service.empty_custom_glyph_buffer());
            }

            let mut areas = Vec::with_capacity(self.buffers.len() + custom_buffers.len());
            for (run, buffer) in normal_runs.iter().zip(self.buffers.iter()) {
                let line_width =
                    shaped_line_width(buffer).unwrap_or_else(|| estimated_text_width(run));
                let left = text_paint_left_for_width(run, line_width);
                let top = text_paint_top_for_height(run);
                areas.push(TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds: text_bounds(run, width, height),
                    default_color: Color::rgba(
                        run.color[0],
                        run.color[1],
                        run.color[2],
                        run.color[3],
                    ),
                    custom_glyphs: &[],
                });
            }
            for (buffer, glyphs) in custom_buffers.iter().zip(custom_glyph_lists.iter()) {
                areas.push(TextArea {
                    buffer,
                    left: 0.0,
                    top: 0.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: width as i32,
                        bottom: height as i32,
                    },
                    default_color: Color::rgba(0, 0, 0, 255),
                    custom_glyphs: glyphs,
                });
            }
            let custom_rasters = self.custom_glyph_rasters.clone();
            self.renderer
                .prepare_with_custom(
                    device,
                    queue,
                    &mut self.service.font_system,
                    &mut self.atlas,
                    &self.viewport,
                    areas,
                    &mut self.service.swash_cache,
                    |request: RasterizeCustomGlyphRequest| custom_rasters.get(&request.id).cloned(),
                )
                .map_err(|error| RenderError {
                    message: format!("glyphon prepare: {error}"),
                })?;
            self.prepared_signatures = placement_signatures;
            self.prepared_viewport = Some((width, height));
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("boon-native-gpu-glyphon-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|error| RenderError {
                    message: format!("glyphon render: {error}"),
                })?;
        }
        text_cache_metrics.cache_entry_count = self.buffers.len() as u32;
        text_cache_metrics.cache_capacity = normal_runs.len() as u32;
        text_cache_metrics.cache_memory_bytes =
            text_buffer_cache_memory_bytes(&self.buffer_signatures);
        Ok((
            (normal_runs.len() + rotated_runs.len()) as u32,
            text_cache_metrics,
        ))
    }

    fn ensure_buffers(&mut self, runs: &[TextRun]) -> TextFrameCacheMetrics {
        let signatures = runs
            .iter()
            .map(TextRunSignature::from_run)
            .collect::<Vec<_>>();
        if self.buffer_signatures == signatures {
            return TextFrameCacheMetrics {
                cache_hits: signatures.len() as u32,
                cache_misses: 0,
                cache_evictions: 0,
                cache_entry_count: self.buffers.len() as u32,
                cache_capacity: runs.len() as u32,
                cache_memory_bytes: text_buffer_cache_memory_bytes(&self.buffer_signatures),
                ..TextFrameCacheMetrics::default()
            };
        }
        let old_signatures = std::mem::take(&mut self.buffer_signatures);
        let (cache_hits, cache_misses, cache_evictions) =
            text_cache_reuse_counts(&old_signatures, &signatures);
        let old_buffers = std::mem::take(&mut self.buffers);
        let mut old_buffers = old_signatures
            .into_iter()
            .zip(old_buffers)
            .collect::<Vec<_>>();
        let mut metrics = TextFrameCacheMetrics {
            cache_hits,
            cache_misses,
            cache_evictions,
            cache_capacity: runs.len() as u32,
            ..TextFrameCacheMetrics::default()
        };
        self.buffers.reserve(runs.len());
        for (signature, run) in signatures.iter().cloned().zip(runs.iter()) {
            if let Some(index) = old_buffers
                .iter()
                .position(|(old_signature, _)| *old_signature == signature)
            {
                let (_, buffer) = old_buffers.swap_remove(index);
                self.buffers.push(buffer);
            } else {
                let buffer = self.service.shape_run(run);
                self.buffers.push(buffer);
            }
        }
        self.buffer_signatures = signatures;
        metrics.cache_entry_count = self.buffers.len() as u32;
        metrics.cache_memory_bytes = text_buffer_cache_memory_bytes(&self.buffer_signatures);
        metrics
    }

    fn custom_glyph_id(&mut self, key: RotatedTextKey) -> CustomGlyphId {
        if let Some(id) = self.custom_glyph_ids.get(&key) {
            return *id;
        }
        let id = self.next_custom_glyph_id;
        self.next_custom_glyph_id = self.next_custom_glyph_id.saturating_add(1).max(1);
        self.custom_glyph_ids.insert(key, id);
        id
    }

    fn rotated_text_glyph(&mut self, run: &TextRun) -> Option<RotatedTextGlyph> {
        self.service.rotated_text_glyph(run)
    }
}

fn text_cache_reuse_counts(
    old_signatures: &[TextRunSignature],
    new_signatures: &[TextRunSignature],
) -> (u32, u32, u32) {
    let mut reusable = old_signatures.to_vec();
    let mut hits = 0_u32;
    let mut misses = 0_u32;
    for signature in new_signatures {
        if let Some(index) = reusable
            .iter()
            .position(|old_signature| old_signature == signature)
        {
            reusable.swap_remove(index);
            hits += 1;
        } else {
            misses += 1;
        }
    }
    (hits, misses, reusable.len() as u32)
}

fn rotated_text_glyph_for_run(
    run: &TextRun,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) -> Option<RotatedTextGlyph> {
    let key = rotated_text_key(run)?;
    let buffer = shape_text_run(font_system, run);
    let mut samples = Vec::new();
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    buffer.draw(
        font_system,
        swash_cache,
        Color::rgba(255, 255, 255, 255),
        |x, y, w, h, color| {
            let alpha = color.a();
            if alpha == 0 {
                return;
            }
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    let px = x + dx;
                    let py = y + dy;
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
                    samples.push((px, py, alpha));
                }
            }
        },
    );
    if samples.is_empty() {
        return None;
    }
    let raw_width = (max_x - min_x + 1).clamp(1, u16::MAX as i32) as u16;
    let raw_height = (max_y - min_y + 1).clamp(1, u16::MAX as i32) as u16;
    let mut raw_mask = vec![0; usize::from(raw_width) * usize::from(raw_height)];
    for (x, y, alpha) in samples {
        let x = (x - min_x) as usize;
        let y = (y - min_y) as usize;
        let index = y * usize::from(raw_width) + x;
        raw_mask[index] = raw_mask[index].max(alpha);
    }
    let (mask, width, height) = rotate_mask(raw_mask, raw_width, raw_height, run.rotate_degrees);
    let left = (run.bounds.x + (run.bounds.width - f32::from(width)) * 0.5).round();
    let top = (run.bounds.y + (run.bounds.height - f32::from(height)) * 0.5).round();
    Some(RotatedTextGlyph {
        key,
        mask,
        width,
        height,
        left,
        top,
    })
}

fn empty_custom_glyph_buffer(font_system: &mut FontSystem) -> Buffer {
    let mut buffer = Buffer::new(font_system, Metrics::new(1.0, 1.0));
    buffer.set_size(font_system, Some(1.0), Some(1.0));
    buffer.set_text(font_system, "", &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn is_rotated_quarter_text_run(run: &TextRun) -> bool {
    run.rotate_degrees != 0
        && run.rich_spans.is_empty()
        && !run.text.trim().is_empty()
        && run.text.chars().count() <= 8
}

fn rotated_text_key(run: &TextRun) -> Option<RotatedTextKey> {
    is_rotated_quarter_text_run(run).then(|| RotatedTextKey {
        text: run.text.clone(),
        font_family: run.font_family.clone(),
        font_style: font_style_code(run.font_style),
        font_weight: run.font_weight.0,
        font_features: run.font_features.clone(),
        size: run.size.to_bits(),
        line_height: run.line_height.to_bits(),
        rotate_degrees: run.rotate_degrees,
    })
}

fn font_style_code(style: Style) -> u8 {
    match style {
        Style::Normal => 0,
        Style::Italic => 1,
        Style::Oblique => 2,
    }
}

fn rotate_mask(mask: Vec<u8>, width: u16, height: u16, rotate_degrees: u32) -> (Vec<u8>, u16, u16) {
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    match rotate_degrees % 360 {
        90 => {
            let mut rotated = vec![0; width_usize * height_usize];
            for y in 0..height_usize {
                for x in 0..width_usize {
                    let new_x = height_usize - 1 - y;
                    let new_y = x;
                    rotated[new_y * height_usize + new_x] = mask[y * width_usize + x];
                }
            }
            (rotated, height, width)
        }
        180 => {
            let mut rotated = vec![0; width_usize * height_usize];
            for y in 0..height_usize {
                for x in 0..width_usize {
                    let new_x = width_usize - 1 - x;
                    let new_y = height_usize - 1 - y;
                    rotated[new_y * width_usize + new_x] = mask[y * width_usize + x];
                }
            }
            (rotated, width, height)
        }
        270 => {
            let mut rotated = vec![0; width_usize * height_usize];
            for y in 0..height_usize {
                for x in 0..width_usize {
                    let new_x = y;
                    let new_y = width_usize - 1 - x;
                    rotated[new_y * height_usize + new_x] = mask[y * width_usize + x];
                }
            }
            (rotated, height, width)
        }
        _ => (mask, width, height),
    }
}

fn shape_text_run(font_system: &mut FontSystem, run: &TextRun) -> Buffer {
    let bounds = run.bounds;
    let font_size = run.size.clamp(8.0, 120.0);
    let default_attrs = text_attrs(
        &run.font_family,
        run.font_style,
        run.font_weight,
        run.color,
        &run.font_features,
    );
    let line_height = run.line_height.max(font_size);
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
    buffer.set_size(font_system, None, Some(bounds.height.max(line_height)));
    if run.rich_spans.is_empty() {
        buffer.set_text(
            font_system,
            &run.text,
            &default_attrs,
            Shaping::Advanced,
            None,
        );
    } else {
        buffer.set_rich_text(
            font_system,
            run.rich_spans.iter().map(|span| {
                (
                    span.text.as_str(),
                    text_attrs(
                        &run.font_family,
                        span.font_style,
                        span.font_weight,
                        span.color,
                        &run.font_features,
                    ),
                )
            }),
            &default_attrs,
            Shaping::Advanced,
            None,
        );
    }
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn text_attrs<'a>(
    font_family: &'a str,
    font_style: Style,
    font_weight: Weight,
    color: [u8; 4],
    font_features: &str,
) -> Attrs<'a> {
    let font_family = resolved_font_family(font_family);
    let family = match font_family {
        "ui-monospace" | "SFMono-Regular" | "Menlo" | "Monaco" | "Consolas" | "Liberation Mono" => {
            Family::Name(DOCUMENT_MONOSPACE_FONT_FAMILY)
        }
        "SansSerif" | "sans-serif" => Family::SansSerif,
        "Serif" | "serif" => Family::Serif,
        "Monospace" | "monospace" => Family::Monospace,
        "Segoe UI" | "Roboto" | "Helvetica Neue" | "Helvetica" | "Arial" => {
            Family::Name(DOCUMENT_FONT_FAMILY)
        }
        _ => Family::Name(font_family),
    };
    Attrs::new()
        .family(family)
        .style(font_style)
        .weight(font_weight)
        .color(Color::rgba(color[0], color[1], color[2], color[3]))
        .font_features(text_font_features(font_features))
}

fn resolved_font_family(value: &str) -> &str {
    value
        .split(|ch| ch == ',' || ch == '|')
        .map(str::trim)
        .find(|family| !family.is_empty())
        .unwrap_or(value)
}

fn editor_font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_font_data(JETBRAINS_MONO_FONT_BYTES.to_vec());
    db.load_font_data(JETBRAINS_MONO_BOLD_FONT_BYTES.to_vec());
    db.load_font_data(JETBRAINS_MONO_ITALIC_FONT_BYTES.to_vec());
    db.load_font_data(JETBRAINS_MONO_BOLD_ITALIC_FONT_BYTES.to_vec());
    db.load_system_fonts();
    db.set_monospace_family("JetBrains Mono");
    FontSystem::new_with_locale_and_db("en-US".to_owned(), db)
}

#[derive(Clone, Debug)]
struct TextRun {
    #[cfg(test)]
    node: DocumentNodeId,
    font_id: u64,
    paint_id: u64,
    bounds: Rect,
    clip: Option<Rect>,
    text: String,
    rich_spans: Vec<RichTextSpan>,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    font_features: String,
    text_inset: f32,
    text_clip_padding: f32,
    color: [u8; 4],
    size: f32,
    line_height: f32,
    align: TextAlign,
    vertical_align: TextVerticalAlign,
    rotate_degrees: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextVerticalAlign {
    Top,
    Center,
    Bottom,
}

#[cfg(test)]
fn text_runs(frame: &LayoutFrame, width: u32, height: u32) -> Vec<TextRun> {
    neutral_text_runs(frame, width, height)
        .into_iter()
        .map(TextRun::from)
        .collect()
}

#[cfg(test)]
fn neutral_text_runs(frame: &LayoutFrame, width: u32, height: u32) -> Vec<RenderTextRun> {
    let mut columns = GlyphonRenderTextColumnMeasurer::new();
    boon_document::render_scene::render_text_runs(frame, width, height, &mut columns)
}

pub struct GlyphonRenderTextColumnMeasurer {
    service: GlyphonTextService,
}

impl GlyphonRenderTextColumnMeasurer {
    pub fn new() -> Self {
        Self {
            service: GlyphonTextService::new(),
        }
    }
}

impl Default for GlyphonRenderTextColumnMeasurer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderTextColumnMeasurer for GlyphonRenderTextColumnMeasurer {
    fn column_edges(&mut self, text: &str, style: &StyleMap, line_height: f32) -> Vec<f32> {
        self.service
            .editor_column_edges_for_style(text, style, line_height)
    }
}

impl From<RenderTextRun> for TextRun {
    fn from(run: RenderTextRun) -> Self {
        Self {
            #[cfg(test)]
            node: run.node,
            font_id: run.font_id,
            paint_id: run.paint_id,
            bounds: run.bounds,
            clip: run.clip,
            text: run.text,
            rich_spans: run.rich_spans.into_iter().map(RichTextSpan::from).collect(),
            font_family: run.font_family,
            font_style: glyphon_font_style(run.font_style),
            font_weight: glyphon_font_weight(run.font_weight),
            font_features: run.font_features,
            text_inset: run.text_inset,
            text_clip_padding: run.text_clip_padding,
            color: run.color,
            size: run.size,
            line_height: run.line_height,
            align: text_align_from_render(run.align),
            vertical_align: text_vertical_align_from_render(run.vertical_align),
            rotate_degrees: run.rotate_degrees,
        }
    }
}

impl From<RenderRichTextSpan> for RichTextSpan {
    fn from(span: RenderRichTextSpan) -> Self {
        Self {
            text: span.text,
            color: span.color,
            font_style: glyphon_font_style(span.font_style),
            font_weight: glyphon_font_weight(span.font_weight),
        }
    }
}

fn glyphon_font_style(style: RenderFontStyle) -> Style {
    match style {
        RenderFontStyle::Normal => Style::Normal,
        RenderFontStyle::Italic => Style::Italic,
        RenderFontStyle::Oblique => Style::Oblique,
    }
}

fn glyphon_font_weight(weight: RenderFontWeight) -> Weight {
    Weight(weight.0)
}

fn text_align_from_render(align: RenderTextAlign) -> TextAlign {
    match align {
        RenderTextAlign::Left => TextAlign::Left,
        RenderTextAlign::Center => TextAlign::Center,
        RenderTextAlign::Right => TextAlign::Right,
    }
}

fn text_vertical_align_from_render(align: RenderTextVerticalAlign) -> TextVerticalAlign {
    match align {
        RenderTextVerticalAlign::Top => TextVerticalAlign::Top,
        RenderTextVerticalAlign::Center => TextVerticalAlign::Center,
        RenderTextVerticalAlign::Bottom => TextVerticalAlign::Bottom,
    }
}

fn rich_text_spans(style: &StyleMap, text: &str, default_color: [u8; 4]) -> Vec<RichTextSpan> {
    let payloads = rich_text_span_payloads(style);
    let mut source_text = String::new();
    let spans = payloads
        .into_iter()
        .map(|payload| {
            source_text.push_str(payload.source_text.as_deref().unwrap_or(&payload.text));
            RichTextSpan {
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
                    .unwrap_or(Style::Normal),
                font_weight: payload
                    .font_weight
                    .as_deref()
                    .map(text_font_weight_value)
                    .unwrap_or(Weight::NORMAL),
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

pub fn editor_text_column_edges(
    text: &str,
    font_size: f32,
    line_height: f32,
    font_family: &str,
    font_features: &str,
) -> Vec<f32> {
    GlyphonTextService::new().editor_column_edges(
        text,
        font_size,
        line_height,
        font_family,
        font_features,
    )
}

pub fn editor_text_column_edges_for_style(
    text: &str,
    style: &StyleMap,
    line_height: f32,
) -> Vec<f32> {
    GlyphonTextService::new().editor_column_edges_for_style(text, style, line_height)
}

fn text_font_features(value: &str) -> FontFeatures {
    let mut features = FontFeatures::new();
    for tag in value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace() || ch == '\'' || ch == '"')
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && *tag != "1")
    {
        match tag {
            "zero" => {
                features.enable(FeatureTag::new(b"zero"));
            }
            "calt" => {
                features.enable(FeatureTag::CONTEXTUAL_ALTERNATES);
            }
            "liga" => {
                features.enable(FeatureTag::STANDARD_LIGATURES);
            }
            "clig" => {
                features.enable(FeatureTag::CONTEXTUAL_LIGATURES);
            }
            "dlig" => {
                features.enable(FeatureTag::DISCRETIONARY_LIGATURES);
            }
            "kern" => {
                features.enable(FeatureTag::KERNING);
            }
            _ => {}
        }
    }
    features
}

fn text_font_style(style: &StyleMap) -> Style {
    style_text(style, "font_style")
        .or_else(|| style_text(style, "style"))
        .map(text_font_style_value)
        .unwrap_or(Style::Normal)
}

fn text_font_style_value(value: &str) -> Style {
    match value.to_ascii_lowercase().as_str() {
        "italic" | "cursive" => Style::Italic,
        "oblique" => Style::Oblique,
        _ => Style::Normal,
    }
}

fn text_font_weight(style: &StyleMap) -> Weight {
    style_text(style, "weight")
        .map(text_font_weight_value)
        .or_else(|| {
            style_number(style, "weight")
                .map(|value| Weight(value.round().clamp(100.0, 900.0) as u16))
        })
        .unwrap_or(Weight::NORMAL)
}

fn text_font_weight_value(value: &str) -> Weight {
    match value.to_ascii_lowercase().as_str() {
        "hairline" | "thin" => Weight(100),
        "extralight" | "extra-light" | "ultralight" | "ultra-light" => Weight(200),
        "light" => Weight(300),
        "bold" => Weight::BOLD,
        "bolder" => Weight::EXTRA_BOLD,
        "semibold" | "semi-bold" => Weight::SEMIBOLD,
        "medium" => Weight::MEDIUM,
        "normal" => Weight::NORMAL,
        value => value.parse::<u16>().map(Weight).unwrap_or(Weight::NORMAL),
    }
}

fn estimated_text_width(run: &TextRun) -> f32 {
    if run.text.is_empty() {
        0.0
    } else {
        run.bounds.width
    }
}

fn shaped_line_width(buffer: &Buffer) -> Option<f32> {
    buffer.layout_runs().next().map(|run| run.line_w)
}

fn text_left_for_width(run: &TextRun, text_width: f32) -> f32 {
    match run.align {
        TextAlign::Left => run.bounds.x + run.text_inset,
        TextAlign::Center => {
            run.bounds.x + ((run.bounds.width - text_width) / 2.0).max(run.text_inset)
        }
        TextAlign::Right => {
            run.bounds.x + (run.bounds.width - text_width - run.text_inset).max(run.text_inset)
        }
    }
}

fn snap_text_paint_origin(value: f32) -> f32 {
    value.round()
}

fn text_paint_left_for_width(run: &TextRun, text_width: f32) -> f32 {
    snap_text_paint_origin(text_left_for_width(run, text_width))
}

fn text_top_for_height(run: &TextRun) -> f32 {
    text_render_top_for_parts(
        run.bounds,
        run.line_height,
        run.text_inset,
        run.vertical_align,
    )
}

fn text_paint_top_for_height(run: &TextRun) -> f32 {
    snap_text_paint_origin(text_top_for_height(run))
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

fn text_top_for_parts(
    bounds: Rect,
    line_height: f32,
    text_inset: f32,
    vertical_align: TextVerticalAlign,
) -> f32 {
    let line_height = line_height.max(1.0);
    match vertical_align {
        TextVerticalAlign::Top => bounds.y + 1.0,
        TextVerticalAlign::Center => bounds.y + ((bounds.height - line_height) / 2.0).max(0.0),
        TextVerticalAlign::Bottom => bounds.y + (bounds.height - line_height - text_inset).max(0.0),
    }
}

fn text_render_top_for_parts(
    bounds: Rect,
    line_height: f32,
    text_inset: f32,
    vertical_align: TextVerticalAlign,
) -> f32 {
    text_top_for_parts(bounds, line_height, text_inset, vertical_align)
}

fn shaped_column_edges(text: &str, buffer: &Buffer, line_width: f32) -> Vec<f32> {
    let char_count = text.chars().count();
    let mut edges = vec![None; char_count.saturating_add(1)];
    if let Some(first) = edges.first_mut() {
        *first = Some(0.0);
    }
    if let Some(last) = edges.last_mut() {
        *last = Some(line_width.max(0.0));
    }
    if let Some(run) = buffer.layout_runs().next() {
        for glyph in run.glyphs {
            apply_glyph_edges(text, &mut edges, glyph);
        }
    }
    fill_missing_column_edges(&mut edges, line_width.max(0.0));
    edges.into_iter().map(|edge| edge.unwrap_or(0.0)).collect()
}

fn apply_glyph_edges(text: &str, edges: &mut [Option<f32>], glyph: &LayoutGlyph) {
    let start = byte_to_char_column(text, glyph.start.min(text.len()));
    let end = byte_to_char_column(text, glyph.end.min(text.len()));
    if start >= edges.len() || end > edges.len() || end <= start {
        return;
    }
    let glyph_left = glyph.x;
    let glyph_right = glyph.x + glyph.w;
    let span = (end - start) as f32;
    for column in start..=end {
        let fraction = (column - start) as f32 / span;
        edges[column] = Some(glyph_left + (glyph_right - glyph_left) * fraction);
    }
}

fn byte_to_char_column(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

fn fill_missing_column_edges(edges: &mut [Option<f32>], line_width: f32) {
    let last_index = edges.len().saturating_sub(1);
    let fallback_advance = if last_index > 0 {
        line_width / last_index as f32
    } else {
        0.0
    };
    for index in 0..edges.len() {
        if edges[index].is_none() {
            edges[index] = Some(index as f32 * fallback_advance);
        }
    }
    let mut previous = 0.0;
    for edge in edges.iter_mut() {
        let clamped = edge.unwrap_or(previous).max(previous);
        *edge = Some(clamped);
        previous = clamped;
    }
}

fn style_number(style: &StyleMap, key: &str) -> Option<f32> {
    match state_style_value(style, key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) | StyleValue::RichTextSpans(_) | StyleValue::EditorTypeHints(_) => None,
    }
}

fn style_text<'a>(style: &'a StyleMap, key: &str) -> Option<&'a str> {
    match state_style_value(style, key)? {
        StyleValue::Text(value) => Some(value.as_str()),
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

fn text_bounds(run: &TextRun, width: u32, height: u32) -> TextBounds {
    let bounds = run
        .clip
        .and_then(|clip| rect_intersection(run.bounds, clip))
        .unwrap_or(run.bounds);
    TextBounds {
        left: (bounds.x - run.text_clip_padding).max(0.0) as i32,
        top: (bounds.y - run.text_clip_padding).max(0.0) as i32,
        right: (bounds.x + bounds.width + run.text_clip_padding).clamp(0.0, width as f32) as i32,
        bottom: (bounds.y + bounds.height + run.text_clip_padding).clamp(0.0, height as f32) as i32,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RectVertexMetrics {
    visible_display_item_count: u32,
    rendered_rect_count: u32,
    cap_hit: bool,
}

#[derive(Clone, Copy, Debug)]
struct BorderStroke {
    color: [f32; 4],
    thickness: f32,
}

fn push_rect(builder: &mut QuadBuilder, rect: Rect, width: f32, height: f32, color: [f32; 4]) {
    push_textured_rect(builder, QuadTexture::Solid, rect, width, height, color);
}

fn push_asset_rect(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    asset_url: &str,
) {
    let texture_width = rect.width.ceil().clamp(1.0, 2048.0) as u32;
    let texture_height = rect.height.ceil().clamp(1.0, 2048.0) as u32;
    push_textured_rect(
        builder,
        QuadTexture::Asset(AssetTextureKey {
            url: asset_url.to_owned(),
            asset_ref: RenderAssetRef::inline_svg_data_url(
                asset_url,
                texture_width,
                texture_height,
            ),
            width: texture_width,
            height: texture_height,
        }),
        rect,
        width,
        height,
        [1.0, 1.0, 1.0, 1.0],
    );
}

fn push_textured_rect(
    builder: &mut QuadBuilder,
    texture: QuadTexture,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let x0 = rect.x;
    let x1 = rect.x + rect.width;
    let y0 = rect.y;
    let y1 = rect.y + rect.height;
    builder.push_triangle(
        texture.clone(),
        [[x0, y0], [x1, y0], [x1, y1]],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        width,
        height,
        color,
    );
    builder.push_triangle(
        texture,
        [[x0, y0], [x1, y1], [x0, y1]],
        [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        width,
        height,
        color,
    );
}

const CHECKBOX_VECTOR_SEGMENTS: usize = 24;
const SOLID_TRIANGLE_UVS: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

fn push_solid_triangle(
    builder: &mut QuadBuilder,
    points: [[f32; 2]; 3],
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if color[3] <= 0.001 {
        return;
    }
    builder.push_triangle(
        QuadTexture::Solid,
        points,
        SOLID_TRIANGLE_UVS,
        width,
        height,
        color,
    );
}

fn circle_point(center_x: f32, center_y: f32, radius: f32, angle: f32) -> [f32; 2] {
    [
        center_x + radius * angle.cos(),
        center_y + radius * angle.sin(),
    ]
}

#[allow(clippy::too_many_arguments)]
fn push_circle_fan(
    builder: &mut QuadBuilder,
    center_x: f32,
    center_y: f32,
    radius: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if color[3] <= 0.001 || radius <= 0.001 {
        return;
    }
    let center = [center_x, center_y];
    for segment in 0..CHECKBOX_VECTOR_SEGMENTS {
        let angle0 = std::f32::consts::TAU * segment as f32 / CHECKBOX_VECTOR_SEGMENTS as f32;
        let angle1 = std::f32::consts::TAU * (segment + 1) as f32 / CHECKBOX_VECTOR_SEGMENTS as f32;
        push_solid_triangle(
            builder,
            [
                center,
                circle_point(center_x, center_y, radius, angle0),
                circle_point(center_x, center_y, radius, angle1),
            ],
            width,
            height,
            color,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_circle_annulus(
    builder: &mut QuadBuilder,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if color[3] <= 0.001 || outer_radius <= inner_radius.max(0.0) + 0.001 {
        return;
    }
    let inner_radius = inner_radius.max(0.0);
    if inner_radius <= 0.001 {
        push_circle_fan(
            builder,
            center_x,
            center_y,
            outer_radius,
            width,
            height,
            color,
        );
        return;
    }
    for segment in 0..CHECKBOX_VECTOR_SEGMENTS {
        let angle0 = std::f32::consts::TAU * segment as f32 / CHECKBOX_VECTOR_SEGMENTS as f32;
        let angle1 = std::f32::consts::TAU * (segment + 1) as f32 / CHECKBOX_VECTOR_SEGMENTS as f32;
        let outer0 = circle_point(center_x, center_y, outer_radius, angle0);
        let outer1 = circle_point(center_x, center_y, outer_radius, angle1);
        let inner0 = circle_point(center_x, center_y, inner_radius, angle0);
        let inner1 = circle_point(center_x, center_y, inner_radius, angle1);
        push_solid_triangle(builder, [outer0, outer1, inner1], width, height, color);
        push_solid_triangle(builder, [outer0, inner1, inner0], width, height, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn push_line_segment_quad(
    builder: &mut QuadBuilder,
    start: (f32, f32),
    end: (f32, f32),
    thickness: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if color[3] <= 0.001 {
        return;
    }
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let length = dx.hypot(dy);
    if length <= 0.001 {
        return;
    }
    let nx = -dy / length * thickness * 0.5;
    let ny = dx / length * thickness * 0.5;
    let p0 = [start.0 + nx, start.1 + ny];
    let p1 = [end.0 + nx, end.1 + ny];
    let p2 = [end.0 - nx, end.1 - ny];
    let p3 = [start.0 - nx, start.1 - ny];
    push_solid_triangle(builder, [p0, p1, p2], width, height, color);
    push_solid_triangle(builder, [p0, p2, p3], width, height, color);
}

fn push_styled_rect(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
    radius: f32,
) {
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    if radius <= 0.25 {
        push_rect(builder, rect, width, height, color);
        return;
    }
    push_rounded_rect(builder, rect, width, height, color, radius);
}

fn push_styled_border_all(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    border_color: [f32; 4],
    thickness: f32,
    radius: f32,
) {
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    if radius <= 0.25 {
        push_border_all(builder, rect, width, height, border_color, thickness);
        return;
    }
    push_rounded_border_all(
        builder,
        rect,
        width,
        height,
        border_color,
        thickness,
        radius,
    );
}

fn push_rounded_border_all(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
    thickness: f32,
    radius: f32,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    let thickness = thickness
        .max(0.25)
        .min(radius.max(0.25))
        .min(rect.width.min(rect.height) * 0.5);
    let center_width = (rect.width - radius * 2.0).max(0.0);
    let center_height = (rect.height - radius * 2.0).max(0.0);
    push_rect(
        builder,
        Rect {
            x: rect.x + radius,
            y: rect.y,
            width: center_width,
            height: thickness,
        },
        width,
        height,
        color,
    );
    push_rect(
        builder,
        Rect {
            x: rect.x + radius,
            y: rect.y + rect.height - thickness,
            width: center_width,
            height: thickness,
        },
        width,
        height,
        color,
    );
    push_rect(
        builder,
        Rect {
            x: rect.x,
            y: rect.y + radius,
            width: thickness,
            height: center_height,
        },
        width,
        height,
        color,
    );
    push_rect(
        builder,
        Rect {
            x: rect.x + rect.width - thickness,
            y: rect.y + radius,
            width: thickness,
            height: center_height,
        },
        width,
        height,
        color,
    );

    let segments = ((radius * 1.5).ceil() as usize).clamp(4, 12);
    let inner_radius = (radius - thickness).max(0.0);
    push_corner_ring(
        builder,
        [rect.x + radius, rect.y + radius],
        radius,
        inner_radius,
        std::f32::consts::PI,
        std::f32::consts::PI * 1.5,
        segments,
        width,
        height,
        color,
    );
    push_corner_ring(
        builder,
        [rect.x + rect.width - radius, rect.y + radius],
        radius,
        inner_radius,
        std::f32::consts::PI * 1.5,
        std::f32::consts::PI * 2.0,
        segments,
        width,
        height,
        color,
    );
    push_corner_ring(
        builder,
        [rect.x + rect.width - radius, rect.y + rect.height - radius],
        radius,
        inner_radius,
        0.0,
        std::f32::consts::PI * 0.5,
        segments,
        width,
        height,
        color,
    );
    push_corner_ring(
        builder,
        [rect.x + radius, rect.y + rect.height - radius],
        radius,
        inner_radius,
        std::f32::consts::PI * 0.5,
        std::f32::consts::PI,
        segments,
        width,
        height,
        color,
    );
}

fn push_rounded_rect(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
    radius: f32,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    if radius <= 0.25 {
        push_rect(builder, rect, width, height, color);
        return;
    }
    push_rect(
        builder,
        Rect {
            x: rect.x + radius,
            y: rect.y,
            width: (rect.width - radius * 2.0).max(0.0),
            height: rect.height,
        },
        width,
        height,
        color,
    );
    push_rect(
        builder,
        Rect {
            x: rect.x,
            y: rect.y + radius,
            width: radius,
            height: (rect.height - radius * 2.0).max(0.0),
        },
        width,
        height,
        color,
    );
    push_rect(
        builder,
        Rect {
            x: rect.x + rect.width - radius,
            y: rect.y + radius,
            width: radius,
            height: (rect.height - radius * 2.0).max(0.0),
        },
        width,
        height,
        color,
    );

    let segments = ((radius * 1.5).ceil() as usize).clamp(4, 12);
    push_corner_fan(
        builder,
        [rect.x + radius, rect.y + radius],
        radius,
        std::f32::consts::PI,
        std::f32::consts::PI * 1.5,
        segments,
        width,
        height,
        color,
    );
    push_corner_fan(
        builder,
        [rect.x + rect.width - radius, rect.y + radius],
        radius,
        std::f32::consts::PI * 1.5,
        std::f32::consts::PI * 2.0,
        segments,
        width,
        height,
        color,
    );
    push_corner_fan(
        builder,
        [rect.x + rect.width - radius, rect.y + rect.height - radius],
        radius,
        0.0,
        std::f32::consts::PI * 0.5,
        segments,
        width,
        height,
        color,
    );
    push_corner_fan(
        builder,
        [rect.x + radius, rect.y + rect.height - radius],
        radius,
        std::f32::consts::PI * 0.5,
        std::f32::consts::PI,
        segments,
        width,
        height,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_corner_fan(
    builder: &mut QuadBuilder,
    center: [f32; 2],
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    for index in 0..segments {
        let a0 = start + (end - start) * (index as f32 / segments as f32);
        let a1 = start + (end - start) * ((index + 1) as f32 / segments as f32);
        push_triangle(
            builder,
            center,
            [center[0] + a0.cos() * radius, center[1] + a0.sin() * radius],
            [center[0] + a1.cos() * radius, center[1] + a1.sin() * radius],
            width,
            height,
            color,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_corner_ring(
    builder: &mut QuadBuilder,
    center: [f32; 2],
    outer_radius: f32,
    inner_radius: f32,
    start: f32,
    end: f32,
    segments: usize,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    if inner_radius <= 0.0 {
        push_corner_fan(
            builder,
            center,
            outer_radius,
            start,
            end,
            segments,
            width,
            height,
            color,
        );
        return;
    }
    for index in 0..segments {
        let a0 = start + (end - start) * (index as f32 / segments as f32);
        let a1 = start + (end - start) * ((index + 1) as f32 / segments as f32);
        let outer0 = [
            center[0] + a0.cos() * outer_radius,
            center[1] + a0.sin() * outer_radius,
        ];
        let outer1 = [
            center[0] + a1.cos() * outer_radius,
            center[1] + a1.sin() * outer_radius,
        ];
        let inner0 = [
            center[0] + a0.cos() * inner_radius,
            center[1] + a0.sin() * inner_radius,
        ];
        let inner1 = [
            center[0] + a1.cos() * inner_radius,
            center[1] + a1.sin() * inner_radius,
        ];
        push_triangle(builder, outer0, outer1, inner1, width, height, color);
        push_triangle(builder, outer0, inner1, inner0, width, height, color);
    }
}

fn push_triangle(
    builder: &mut QuadBuilder,
    a: [f32; 2],
    b: [f32; 2],
    c: [f32; 2],
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    builder.push_triangle(
        QuadTexture::Solid,
        [a, b, c],
        [[0.0, 0.0]; 3],
        width,
        height,
        color,
    );
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
fn circle_segments_for_radius(radius: f32) -> u32 {
    if radius <= 3.0 {
        24
    } else if radius <= 10.0 {
        96
    } else {
        192
    }
}

fn checkbox_circle_center(rect: Rect) -> (f32, f32) {
    (
        (rect.x + rect.width * 0.5).floor() + 0.5,
        (rect.y + rect.height * 0.5).floor() + 0.5,
    )
}

fn checkbox_check_points(rect: Rect) -> ((f32, f32), (f32, f32), (f32, f32)) {
    let point = |x: f32, y: f32| (rect.x + rect.width * x, rect.y + rect.height * y);
    (point(0.33, 0.55), point(0.45, 0.67), point(0.70, 0.35))
}

#[cfg(test)]
fn circle_coverage(radius: f32, aa: f32, distance: f32) -> f32 {
    if aa <= 0.0 {
        return if distance <= radius { 1.0 } else { 0.0 };
    }
    let edge0 = radius - aa;
    let edge1 = radius + aa;
    let t = ((distance - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    1.0 - smooth
}

#[allow(clippy::too_many_arguments)]
fn push_checkbox_check_raster(
    builder: &mut QuadBuilder,
    start: (f32, f32),
    middle: (f32, f32),
    end: (f32, f32),
    thickness: f32,
    aa: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let thickness = thickness.max(1.0) + aa.max(0.0) * 0.35;
    push_line_segment_quad(builder, start, middle, thickness, width, height, color);
    push_line_segment_quad(builder, middle, end, thickness, width, height, color);
}

#[allow(clippy::too_many_arguments)]
fn push_checkbox_circle_raster(
    builder: &mut QuadBuilder,
    center_x: f32,
    center_y: f32,
    radius: f32,
    border_width: f32,
    aa: f32,
    width: f32,
    height: f32,
    ring_color: [f32; 4],
    inner_color: [f32; 4],
) {
    let outer_radius = (radius + aa.max(0.0).min(2.0) * 0.25).max(0.5);
    let border_width = border_width.max(0.0);
    if border_width <= 0.001 {
        push_circle_fan(
            builder,
            center_x,
            center_y,
            outer_radius,
            width,
            height,
            inner_color,
        );
        return;
    }
    let inner_radius = (outer_radius - border_width).max(0.0);
    push_circle_fan(
        builder,
        center_x,
        center_y,
        inner_radius,
        width,
        height,
        inner_color,
    );
    push_circle_annulus(
        builder,
        center_x,
        center_y,
        outer_radius,
        inner_radius,
        width,
        height,
        ring_color,
    );
}

fn push_border_all(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
    thickness: f32,
) {
    let thickness = thickness.max(0.25);
    for edge in [
        Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: thickness,
        },
        Rect {
            x: rect.x,
            y: rect.y + rect.height - thickness,
            width: rect.width,
            height: thickness,
        },
        Rect {
            x: rect.x,
            y: rect.y,
            width: thickness,
            height: rect.height,
        },
        Rect {
            x: rect.x + rect.width - thickness,
            y: rect.y,
            width: thickness,
            height: rect.height,
        },
    ] {
        push_rect(builder, edge, width, height, color);
    }
}

fn push_side_border(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    side: usize,
    stroke: BorderStroke,
) {
    let thickness = stroke.thickness.max(0.25);
    let edge = match side {
        0 => Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: thickness,
        },
        1 => Rect {
            x: rect.x + rect.width - thickness,
            y: rect.y,
            width: thickness,
            height: rect.height,
        },
        2 => Rect {
            x: rect.x,
            y: rect.y + rect.height - thickness,
            width: rect.width,
            height: thickness,
        },
        3 => Rect {
            x: rect.x,
            y: rect.y,
            width: thickness,
            height: rect.height,
        },
        _ => unreachable!(),
    };
    push_rect(builder, edge, width, height, stroke.color);
}

fn rgba8_from_f32(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn pack_rgba8_from_f32(color: [f32; 4]) -> u32 {
    let [r, g, b, a] = rgba8_from_f32(color);
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16) | (u32::from(a) << 24)
}

#[cfg(test)]
fn rgba8_from_packed(color: u32) -> [u8; 4] {
    [
        (color & 255) as u8,
        ((color >> 8) & 255) as u8,
        ((color >> 16) & 255) as u8,
        ((color >> 24) & 255) as u8,
    ]
}

fn srgb_u8_to_linear_f32(channel: u8) -> f32 {
    let channel = channel as f32 / 255.0;
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn style_color_u8(style: &StyleMap, key: &str) -> Option<[u8; 4]> {
    match state_style_value(style, key)? {
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

#[cfg(test)]
mod tests;

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn sha256_file(path: &Path) -> Result<String, RenderError> {
    let bytes = std::fs::read(path).map_err(|error| RenderError {
        message: format!("read native GPU artifact `{}`: {error}", path.display()),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}
