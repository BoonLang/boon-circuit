use boon_document::{
    DisplayItem, DocumentNodeId, DocumentNodeKind, LayoutFrame, Rect, RenderCapabilities, StyleMap,
    StyleValue,
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
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::mpsc;

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
const MAX_CACHED_QUAD_BATCHES: usize = 64;

pub trait PresentSurface {
    fn id(&self) -> SurfaceId;
    fn viewport_width(&self) -> f32;
    fn viewport_height(&self) -> f32;
    fn format(&self) -> SurfaceFormat;
    fn epoch(&self) -> u64;
}

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
        layout_frame_hash: String,
        width: u32,
        height: u32,
        nonblank_samples: usize,
        unique_rgba_values: usize,
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
pub struct FrameMetrics {
    pub frame_seq: u64,
    pub draw_calls: u32,
    pub upload_bytes: u64,
    pub quad_cache_hit: bool,
    pub quad_cache_entry_count: u32,
    pub visible_display_item_count: u32,
    pub rendered_rect_count: u32,
    pub rect_cap_hit: bool,
    pub text_runs_shaped: u32,
    pub rendered_text_runs: u32,
    pub text_cap_hit: bool,
    pub glyphon_text_area_count: u32,
    pub color_only_rect_fallback: bool,
    pub preview_blocked_on_ipc_count: u64,
}

pub trait RenderBackend<T: PresentSurface + ?Sized> {
    fn capabilities(&self) -> RenderCapabilities;
    fn render(&mut self, target: &mut T, frame: &LayoutFrame) -> Result<RenderProof, RenderError>;
}

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
    font_system: FontSystem,
    cache: BTreeMap<(String, TextMeasureStyleKey), boon_document::TextMetrics>,
}

impl GlyphonTextMeasurer {
    pub fn new() -> Self {
        Self {
            font_system: editor_font_system(),
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
        let metrics = boon_document::TextMetrics {
            width: shaped_line_width(&buffer).unwrap_or_default(),
            height: line_height,
        };
        self.cache.insert(cache_key, metrics);
        metrics
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

#[derive(Clone, Debug)]
pub struct NativeGpuRenderer {
    frame_seq: u64,
    rect_shader: generated::shader_bindings::ShaderEntry,
}

impl NativeGpuRenderer {
    pub fn new_uninitialized() -> Self {
        Self {
            frame_seq: 0,
            rect_shader: generated::shader_bindings::ShaderEntry::NativeGpuRect,
        }
    }

    pub fn required_backend_versions() -> (&'static str, &'static str) {
        (REQUIRED_WGPU_VERSION, REQUIRED_GLYPHON_VERSION)
    }

    pub fn default_frame_format_name() -> String {
        format!("{:?}", wgpu::TextureFormat::Rgba8Unorm)
    }

    pub fn rect_shader_entry(&self) -> generated::shader_bindings::ShaderEntry {
        self.rect_shader
    }
}

impl<T: PresentSurface + ?Sized> RenderBackend<T> for NativeGpuRenderer {
    fn capabilities(&self) -> RenderCapabilities {
        RenderCapabilities {
            max_texture_dimension_2d: 4096,
            supports_instancing: true,
            supports_clip_rects: true,
            text_backend_class: "glyphon".to_owned(),
        }
    }

    fn render(&mut self, target: &mut T, frame: &LayoutFrame) -> Result<RenderProof, RenderError> {
        self.frame_seq += 1;
        Ok(RenderProof {
            artifact: RenderProofArtifact::CopyToPresent {
                source_texture_hash: format!(
                    "{:?}:layout-items-{}",
                    self.rect_shader_entry(),
                    frame.display_list.len()
                ),
                target_surface_id: target.id(),
                target_surface_epoch: target.epoch(),
                target_format: target.format(),
                width: target.viewport_width().max(0.0) as u32,
                height: target.viewport_height().max(0.0) as u32,
                acquired_surface_texture: false,
                command_submission_id: "not-presented-scaffold".to_owned(),
                present_result: "scaffold-no-surface".to_owned(),
            },
            metrics: FrameMetrics {
                frame_seq: self.frame_seq,
                draw_calls: 0,
                upload_bytes: 0,
                quad_cache_hit: false,
                quad_cache_entry_count: 0,
                visible_display_item_count: 0,
                rendered_rect_count: 0,
                rect_cap_hit: false,
                text_runs_shaped: 0,
                rendered_text_runs: 0,
                text_cap_hit: false,
                glyphon_text_area_count: 0,
                color_only_rect_fallback: false,
                preview_blocked_on_ipc_count: 0,
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct AppOwnedRenderRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub frame: &'a LayoutFrame,
    pub surface_id: SurfaceId,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub artifact_dir: &'a Path,
    pub artifact_label: &'a str,
}

pub struct SurfaceRenderRequest<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,
    pub frame: &'a LayoutFrame,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

type TextureBindGroup = generated::shader_bindings::native_gpu_rect::WgpuBindGroup0;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AssetTextureKey {
    url: String,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum QuadTexture {
    Solid,
    Asset(AssetTextureKey),
}

#[derive(Debug)]
struct QuadBatch {
    texture: QuadTexture,
    positions: Vec<f32>,
    colors: Vec<u32>,
    uvs: Vec<f32>,
}

#[derive(Clone)]
struct GpuQuadBatch {
    texture: QuadTexture,
    vertex_count: u32,
    position_buffer: wgpu::Buffer,
    color_buffer: wgpu::Buffer,
    uv_buffer: wgpu::Buffer,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct QuadBatchCacheKey {
    texture: QuadTexture,
    vertex_count: u32,
    content_hash: [u8; 32],
}

struct CachedGpuQuadBatch {
    vertex_count: u32,
    position_buffer: wgpu::Buffer,
    color_buffer: wgpu::Buffer,
    uv_buffer: wgpu::Buffer,
}

struct PreparedQuadCache {
    frame: LayoutFrame,
    width: u32,
    height: u32,
    gpu_batches: Vec<GpuQuadBatch>,
    rect_metrics: RectVertexMetrics,
}

#[derive(Debug, Default)]
struct QuadBuilder {
    batches: Vec<QuadBatch>,
}

impl QuadBuilder {
    fn push_triangle(
        &mut self,
        texture: QuadTexture,
        points: [[f32; 2]; 3],
        uvs: [[f32; 2]; 3],
        surface_width: f32,
        surface_height: f32,
        color: [f32; 4],
    ) {
        let batch = if self
            .batches
            .last()
            .is_some_and(|batch| batch.texture == texture)
        {
            self.batches.last_mut().unwrap()
        } else {
            self.batches.push(QuadBatch {
                texture,
                positions: Vec::new(),
                colors: Vec::new(),
                uvs: Vec::new(),
            });
            self.batches.last_mut().unwrap()
        };
        for (point, uv) in points.into_iter().zip(uvs) {
            batch.positions.extend_from_slice(&[
                (point[0] / surface_width.max(1.0))
                    .mul_add(2.0, -1.0)
                    .clamp(-1.0, 1.0),
                (1.0 - (point[1] / surface_height.max(1.0)) * 2.0).clamp(-1.0, 1.0),
            ]);
            batch.colors.push(pack_rgba8_from_f32(color));
            batch.uvs.extend_from_slice(&uv);
        }
    }
}

struct TextureState {
    sampler: wgpu::Sampler,
    _white_texture: wgpu::Texture,
    _white_view: wgpu::TextureView,
    white_bind_group: TextureBindGroup,
    assets: BTreeMap<AssetTextureKey, GpuTextureAsset>,
}

struct GpuTextureAsset {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: TextureBindGroup,
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
        }
    }

    fn prepare_assets(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[QuadBatch],
    ) -> Result<(), RenderError> {
        for batch in batches {
            let QuadTexture::Asset(key) = &batch.texture else {
                continue;
            };
            if self.assets.contains_key(key) {
                continue;
            }
            let pixels = rasterize_svg_data_url(&key.url, key.width, key.height)?;
            let (texture, view) = upload_rgba_texture(
                device,
                queue,
                "boon-native-gpu-asset-texture",
                key.width,
                key.height,
                &pixels,
            );
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
                },
            );
        }
        Ok(())
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
    quad_buffers: BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>,
    prepared_quads: Option<PreparedQuadCache>,
}

impl VisibleLayoutRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = generated::shader_bindings::ShaderEntry::NativeGpuRect;
        let module = shader.create_shader_module_embed_source(device);
        let layout = shader.create_pipeline_layout(device);
        let vertex_entry = generated::shader_bindings::native_gpu_rect::vs_main_entry(
            wgpu::VertexStepMode::Vertex,
            wgpu::VertexStepMode::Vertex,
            wgpu::VertexStepMode::Vertex,
        );
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
            quad_buffers: BTreeMap::new(),
            prepared_quads: None,
        }
    }

    pub fn encode(
        &mut self,
        request: SurfaceRenderRequest<'_>,
    ) -> Result<FrameMetrics, RenderError> {
        self.frame_seq += 1;
        encode_layout_to_surface_with_pipeline(
            request,
            &self.pipeline,
            Some(&mut self.text),
            &mut self.textures,
            Some(&mut self.quad_buffers),
            Some(&mut self.prepared_quads),
            self.frame_seq,
        )
    }
}

pub fn encode_layout_to_surface(
    request: SurfaceRenderRequest<'_>,
) -> Result<FrameMetrics, RenderError> {
    let mut renderer = VisibleLayoutRenderer::new(request.device, request.queue, request.format);
    renderer.encode(request)
}

fn encode_layout_to_surface_with_pipeline(
    request: SurfaceRenderRequest<'_>,
    pipeline: &wgpu::RenderPipeline,
    mut text: Option<&mut GlyphonTextState>,
    textures: &mut TextureState,
    mut quad_buffers: Option<&mut BTreeMap<QuadBatchCacheKey, CachedGpuQuadBatch>>,
    mut prepared_quads: Option<&mut Option<PreparedQuadCache>>,
    frame_seq: u64,
) -> Result<FrameMetrics, RenderError> {
    let width = request.width.clamp(1, 1920);
    let height = request.height.clamp(1, 1080);
    let visible_text_runs = text_runs(request.frame, width, height);
    let text_runs_shaped = visible_text_runs.len() as u32;
    let mut upload_bytes = 0u64;
    let prepared_hit = prepared_quads
        .as_deref()
        .and_then(Option::as_ref)
        .filter(|cache| {
            cache.width == width && cache.height == height && cache.frame == *request.frame
        })
        .map(|cache| (cache.gpu_batches.clone(), cache.rect_metrics));
    let quad_cache_hit = prepared_hit.is_some();
    let (gpu_batches, rect_metrics) = if let Some(hit) = prepared_hit {
        hit
    } else {
        let text_layout_nodes = text_layout_metric_nodes(request.frame);
        let text_layout_metrics = match text.as_mut() {
            Some(text) if !text_layout_nodes.is_empty() => {
                Some(text.layout_metrics_for_runs(&visible_text_runs, &text_layout_nodes))
            }
            None => None,
            Some(_) => None,
        };
        let (quad_batches, rect_metrics) = rect_vertices(
            request.frame,
            width as f32,
            height as f32,
            text_layout_metrics.as_ref(),
        );
        textures.prepare_assets(request.device, request.queue, &quad_batches)?;
        let mut gpu_batches = Vec::new();
        for batch in quad_batches {
            let vertex_count = (batch.positions.len() / 2) as u32;
            if vertex_count == 0 {
                continue;
            }
            let position_bytes = f32_slice_bytes(&batch.positions);
            let color_bytes = u32_slice_bytes(&batch.colors);
            let uv_bytes = f32_slice_bytes(&batch.uvs);
            let cache_key = QuadBatchCacheKey {
                texture: batch.texture.clone(),
                vertex_count,
                content_hash: quad_batch_content_hash(&position_bytes, &color_bytes, &uv_bytes),
            };
            let cached = if let Some(quad_buffers) = quad_buffers.as_deref_mut() {
                if !quad_buffers.contains_key(&cache_key) {
                    if quad_buffers.len() >= MAX_CACHED_QUAD_BATCHES {
                        quad_buffers.clear();
                    }
                    let uploaded = create_gpu_quad_batch(
                        request.device,
                        request.queue,
                        &position_bytes,
                        &color_bytes,
                        &uv_bytes,
                        vertex_count,
                    );
                    upload_bytes +=
                        (position_bytes.len() + color_bytes.len() + uv_bytes.len()) as u64;
                    quad_buffers.insert(cache_key.clone(), uploaded);
                }
                quad_buffers
                    .get(&cache_key)
                    .expect("quad buffer cache insert")
            } else {
                let uploaded = create_gpu_quad_batch(
                    request.device,
                    request.queue,
                    &position_bytes,
                    &color_bytes,
                    &uv_bytes,
                    vertex_count,
                );
                upload_bytes += (position_bytes.len() + color_bytes.len() + uv_bytes.len()) as u64;
                gpu_batches.push(GpuQuadBatch {
                    texture: batch.texture,
                    vertex_count: uploaded.vertex_count,
                    position_buffer: uploaded.position_buffer,
                    color_buffer: uploaded.color_buffer,
                    uv_buffer: uploaded.uv_buffer,
                });
                continue;
            };
            gpu_batches.push(GpuQuadBatch {
                texture: batch.texture,
                vertex_count: cached.vertex_count,
                position_buffer: cached.position_buffer.clone(),
                color_buffer: cached.color_buffer.clone(),
                uv_buffer: cached.uv_buffer.clone(),
            });
        }
        if let Some(prepared_quads) = prepared_quads.as_deref_mut() {
            *prepared_quads = Some(PreparedQuadCache {
                frame: request.frame.clone(),
                width,
                height,
                gpu_batches: gpu_batches.clone(),
                rect_metrics,
            });
        }
        (gpu_batches, rect_metrics)
    };
    {
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
        for batch in &gpu_batches {
            let bind_group =
                textures
                    .bind_group_for(&batch.texture)
                    .ok_or_else(|| RenderError {
                        message: "native GPU asset texture was not prepared before draw".to_owned(),
                    })?;
            bind_group.set(&mut pass);
            pass.set_vertex_buffer(0, batch.position_buffer.slice(..));
            pass.set_vertex_buffer(1, batch.color_buffer.slice(..));
            pass.set_vertex_buffer(2, batch.uv_buffer.slice(..));
            pass.draw(0..batch.vertex_count, 0..1);
        }
    }
    let rendered_text_runs = match text.as_mut() {
        Some(text) => text.render(
            request.device,
            request.queue,
            request.encoder,
            request.view,
            visible_text_runs,
            width,
            height,
        )?,
        None => 0,
    };
    Ok(FrameMetrics {
        frame_seq,
        draw_calls: gpu_batches.len() as u32 + u32::from(rendered_text_runs > 0),
        upload_bytes,
        quad_cache_hit,
        quad_cache_entry_count: quad_buffers
            .as_deref()
            .map_or(0, |cache| cache.len() as u32),
        visible_display_item_count: rect_metrics.visible_display_item_count,
        rendered_rect_count: rect_metrics.rendered_rect_count,
        rect_cap_hit: rect_metrics.cap_hit,
        text_runs_shaped,
        rendered_text_runs,
        text_cap_hit: false,
        glyphon_text_area_count: rendered_text_runs,
        color_only_rect_fallback: rendered_text_runs == 0 && text_runs_shaped > 0,
        preview_blocked_on_ipc_count: 0,
    })
}

fn create_gpu_quad_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    position_bytes: &[u8],
    color_bytes: &[u8],
    uv_bytes: &[u8],
    vertex_count: u32,
) -> CachedGpuQuadBatch {
    let position_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-position-buffer"),
        size: position_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-color-buffer"),
        size: color_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uv_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-uv-buffer"),
        size: uv_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&position_buffer, 0, position_bytes);
    queue.write_buffer(&color_buffer, 0, color_bytes);
    queue.write_buffer(&uv_buffer, 0, uv_bytes);
    CachedGpuQuadBatch {
        vertex_count,
        position_buffer,
        color_buffer,
        uv_buffer,
    }
}

fn quad_batch_content_hash(position_bytes: &[u8], color_bytes: &[u8], uv_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update((position_bytes.len() as u64).to_le_bytes());
    hasher.update(position_bytes);
    hasher.update((color_bytes.len() as u64).to_le_bytes());
    hasher.update(color_bytes);
    hasher.update((uv_bytes.len() as u64).to_le_bytes());
    hasher.update(uv_bytes);
    hasher.finalize().into()
}

pub fn render_app_owned_pixels(
    request: AppOwnedRenderRequest<'_>,
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
        label: Some("boon-native-gpu-app-owned-texture"),
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
        label: Some("boon-native-gpu-readback-buffer"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = request
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-app-owned-encoder"),
        });
    let mut renderer = VisibleLayoutRenderer::new(request.device, request.queue, format);
    let metrics = renderer.encode(SurfaceRenderRequest {
        device: request.device,
        queue: request.queue,
        encoder: &mut encoder,
        view: &view,
        frame: request.frame,
        format,
        width,
        height,
    })?;
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
    request.queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    request
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| RenderError {
            message: format!("native GPU readback poll: {error}"),
        })?;
    receiver
        .recv()
        .map_err(|error| RenderError {
            message: format!("native GPU readback callback: {error}"),
        })?
        .map_err(|error| RenderError {
            message: format!("native GPU readback map: {error}"),
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
    let layout_frame_hash = layout_frame_hash(request.frame);
    let layout_hash_prefix = layout_frame_hash
        .get(..16)
        .unwrap_or(layout_frame_hash.as_str());
    let artifact_path = request.artifact_dir.join(format!(
        "{}-{}-{}x{}-{}-{}.png",
        std::process::id(),
        request.artifact_label,
        width,
        height,
        request.frame.display_list.len(),
        layout_hash_prefix
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
            capture_method: "wgpu-generated-shader-app-owned-readback".to_owned(),
            surface_id: request.surface_id,
            surface_epoch: request.surface_epoch,
            frame_seq: 1,
            layout_frame_hash,
            width,
            height,
            nonblank_samples,
            unique_rgba_values,
        },
        metrics: FrameMetrics { ..metrics },
    })
}

struct GlyphonTextState {
    font_system: FontSystem,
    swash_cache: SwashCache,
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

#[derive(Clone, Debug, Deserialize)]
struct RichTextSpanPayload {
    text: String,
    source_text: Option<String>,
    color: Option<String>,
    font_style: Option<String>,
    font_weight: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct EditorTypeHintPayload {
    anchor_column: usize,
    compact_label: String,
}

#[derive(Clone, Debug)]
struct TextRunLayoutMetrics {
    left: f32,
    column_edges: Vec<f32>,
}

impl TextRunLayoutMetrics {
    fn x_for_column(&self, column: f32) -> f32 {
        let column = column.max(0.0);
        let lower = column.floor() as usize;
        let fraction = column - lower as f32;
        let lower_x = self
            .column_edges
            .get(lower)
            .copied()
            .or_else(|| self.column_edges.last().copied())
            .unwrap_or(0.0);
        let upper_x = self
            .column_edges
            .get(lower.saturating_add(1))
            .copied()
            .or_else(|| self.column_edges.last().copied())
            .unwrap_or(lower_x);
        self.left + lower_x + (upper_x - lower_x) * fraction
    }

    fn width_for_column(&self, column: f32) -> f32 {
        (self.x_for_column(column + 1.0) - self.x_for_column(column)).max(1.0)
    }
}

type TextRunLayoutMap = BTreeMap<DocumentNodeId, TextRunLayoutMetrics>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRunSignature {
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

impl TextRunSignature {
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
        let font_system = editor_font_system();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            font_system,
            swash_cache,
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
    ) -> Result<u32, RenderError> {
        if runs.is_empty() {
            return Ok(0);
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
        self.ensure_buffers(&normal_runs);
        if self.prepared_signatures != placement_signatures
            || self.prepared_viewport != Some((width, height))
        {
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
                custom_buffers.push(empty_custom_glyph_buffer(&mut self.font_system));
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
                    &mut self.font_system,
                    &mut self.atlas,
                    &self.viewport,
                    areas,
                    &mut self.swash_cache,
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
        Ok((normal_runs.len() + rotated_runs.len()) as u32)
    }

    fn ensure_buffers(&mut self, runs: &[TextRun]) {
        let signatures = runs
            .iter()
            .map(TextRunSignature::from_run)
            .collect::<Vec<_>>();
        if self.buffer_signatures != signatures {
            let old_signatures = std::mem::take(&mut self.buffer_signatures);
            let old_buffers = std::mem::take(&mut self.buffers);
            let mut old_buffers = old_signatures
                .into_iter()
                .zip(old_buffers)
                .collect::<Vec<_>>();
            self.buffers.reserve(runs.len());
            for (signature, run) in signatures.iter().cloned().zip(runs.iter()) {
                if let Some(index) = old_buffers
                    .iter()
                    .position(|(old_signature, _)| *old_signature == signature)
                {
                    let (_, buffer) = old_buffers.swap_remove(index);
                    self.buffers.push(buffer);
                } else {
                    let buffer = shape_text_run(&mut self.font_system, run);
                    self.buffers.push(buffer);
                }
            }
            self.buffer_signatures = signatures;
        }
    }

    fn layout_metrics_for_runs(
        &mut self,
        runs: &[TextRun],
        required_nodes: &BTreeSet<DocumentNodeId>,
    ) -> TextRunLayoutMap {
        self.ensure_buffers(runs);
        runs.iter()
            .zip(self.buffers.iter())
            .filter(|(run, _)| required_nodes.contains(&run.node))
            .map(|(run, buffer)| {
                let line_width =
                    shaped_line_width(buffer).unwrap_or_else(|| estimated_text_width(run));
                let left = text_paint_left_for_width(run, line_width);
                let column_edges = shaped_column_edges(&run.text, buffer, line_width);
                (
                    run.node.clone(),
                    TextRunLayoutMetrics { left, column_edges },
                )
            })
            .collect()
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
        rotated_text_glyph_for_run(run, &mut self.font_system, &mut self.swash_cache)
    }
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

struct TextRun {
    node: DocumentNodeId,
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

fn text_runs(frame: &LayoutFrame, width: u32, height: u32) -> Vec<TextRun> {
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
                .unwrap_or(DOCUMENT_FONT_FAMILY)
        } else {
            style_text(&item.style, "font").unwrap_or(DOCUMENT_FONT_FAMILY)
        };
        let rich_spans = rich_text_spans(&item.style, &text, color);
        runs.push(TextRun {
            node: item.node.clone(),
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
        runs.extend(editor_type_hint_runs(item, width, height));
    }
    runs
}

fn editor_type_hint_runs(item: &DisplayItem, _width: u32, _height: u32) -> Vec<TextRun> {
    if !matches!(item.kind, DocumentNodeKind::Text) {
        return Vec::new();
    }
    let Some(hints_json) = style_text(&item.style, "editor_type_hints_json") else {
        return Vec::new();
    };
    let Ok(hints) = serde_json::from_str::<Vec<EditorTypeHintPayload>>(hints_json) else {
        return Vec::new();
    };
    if hints.is_empty() {
        return Vec::new();
    }
    let source_text = item.text.as_deref().unwrap_or_default();
    let column_edges =
        editor_text_column_edges_for_style(source_text, &item.style, item.bounds.height);
    let inset = style_number(&item.style, "text_inset").unwrap_or(0.0);
    let font_size = (style_number(&item.style, "size").unwrap_or(14.0) - 1.0).max(10.0);
    let font_family = style_text(&item.style, "font").unwrap_or("JetBrains Mono");
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
            let text = format!(": {}", hint.compact_label);
            Some(TextRun {
                node: DocumentNodeId(format!("{}:type-hint:{index}", item.node.0)),
                bounds: Rect {
                    x,
                    y: item.bounds.y,
                    width: available_width,
                    height: item.bounds.height,
                },
                clip: clip_rect_for_style(&item.style),
                text,
                rich_spans: Vec::new(),
                font_family: font_family.to_owned(),
                font_style: Style::Italic,
                font_weight: Weight::NORMAL,
                font_features: font_features.clone(),
                text_inset: 0.0,
                text_clip_padding: 0.0,
                color,
                size: font_size,
                line_height: item.bounds.height,
                align: TextAlign::Left,
                vertical_align: TextVerticalAlign::Center,
                rotate_degrees: 0,
            })
        })
        .collect()
}

fn rich_text_spans(style: &StyleMap, text: &str, default_color: [u8; 4]) -> Vec<RichTextSpan> {
    let Some(spans_json) = style_text(style, "syntax_spans_json") else {
        return Vec::new();
    };
    let Ok(payloads) = serde_json::from_str::<Vec<RichTextSpanPayload>>(spans_json) else {
        return Vec::new();
    };
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

pub fn editor_text_column_edges(
    text: &str,
    font_size: f32,
    line_height: f32,
    font_family: &str,
    font_features: &str,
) -> Vec<f32> {
    let mut font_system = editor_font_system();
    let color = [217, 225, 242, 255];
    let mut buffer = Buffer::new(
        &mut font_system,
        Metrics::new(font_size.max(1.0), line_height.max(font_size.max(1.0))),
    );
    buffer.set_size(
        &mut font_system,
        None,
        Some(line_height.max(font_size.max(1.0))),
    );
    buffer.set_text(
        &mut font_system,
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
    buffer.shape_until_scroll(&mut font_system, false);
    let line_width = shaped_line_width(&buffer).unwrap_or_default();
    shaped_column_edges(text, &buffer, line_width)
}

pub fn editor_text_column_edges_for_style(
    text: &str,
    style: &StyleMap,
    line_height: f32,
) -> Vec<f32> {
    let mut font_system = editor_font_system();
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
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
    buffer.set_size(&mut font_system, None, Some(line_height));
    let rich_spans = rich_text_spans(style, text, color);
    if rich_spans.is_empty() {
        buffer.set_text(
            &mut font_system,
            text,
            &default_attrs,
            Shaping::Advanced,
            None,
        );
    } else {
        buffer.set_rich_text(
            &mut font_system,
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
    buffer.shape_until_scroll(&mut font_system, false);
    let line_width = shaped_line_width(&buffer).unwrap_or_default();
    shaped_column_edges(text, &buffer, line_width)
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

fn placeholder_font_weight(style: &StyleMap) -> Option<Weight> {
    style_text(style, "placeholder_weight")
        .map(text_font_weight_value)
        .or_else(|| {
            style_number(style, "placeholder_weight")
                .map(|value| Weight(value.round().clamp(100.0, 900.0) as u16))
        })
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

fn text_content_bounds_for_item(item: &DisplayItem) -> Rect {
    let padding = style_edges(&item.style, "padding");
    Rect {
        x: item.bounds.x + padding.left,
        y: item.bounds.y + padding.top,
        width: (item.bounds.width - padding.horizontal()).max(1.0),
        height: (item.bounds.height - padding.vertical()).max(1.0),
    }
}

fn strikethrough_rect_for_item(
    item: &DisplayItem,
    text_layout: Option<&TextRunLayoutMetrics>,
) -> Rect {
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
    let x = text_layout
        .map(|layout| layout.x_for_column(0.0))
        .unwrap_or(item.bounds.x + 4.0);
    let x1 = text_layout
        .map(|layout| layout.x_for_column(text_columns))
        .unwrap_or(item.bounds.x + item.bounds.width - 4.0);
    Rect {
        x,
        y: line_top + line_height * 0.5 - thickness * 0.5,
        width: (x1 - x).max(1.0),
        height: thickness,
    }
}

fn underline_rect_for_item(item: &DisplayItem, text_layout: Option<&TextRunLayoutMetrics>) -> Rect {
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
    let x = text_layout
        .map(|layout| layout.x_for_column(0.0))
        .unwrap_or(item.bounds.x + 4.0);
    let x1 = text_layout
        .map(|layout| layout.x_for_column(text_columns))
        .unwrap_or(item.bounds.x + item.bounds.width - 4.0);
    Rect {
        x,
        y: (line_top + line_height * 0.88).min(item.bounds.y + item.bounds.height - thickness),
        width: (x1 - x).max(1.0),
        height: thickness,
    }
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

fn rect_intersects(rect: Rect, viewport: Rect) -> bool {
    rect.x < viewport.x + viewport.width
        && rect.x + rect.width > viewport.x
        && rect.y < viewport.y + viewport.height
        && rect.y + rect.height > viewport.y
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

fn text_layout_metric_nodes(frame: &LayoutFrame) -> BTreeSet<DocumentNodeId> {
    frame
        .display_list
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                DocumentNodeKind::Text | DocumentNodeKind::TextInput | DocumentNodeKind::Button
            )
        })
        .filter(|item| {
            item.style.contains_key("editor_selection_start")
                || item.style.contains_key("editor_selection_end")
                || item.style.contains_key("editor_bracket_columns")
                || item.style.contains_key("editor_caret_column")
                || item.style.contains_key("caret_column")
                || item.focused
                || style_bool(&item.style, "focus") == Some(true)
                || style_bool(&item.style, "strikethrough") == Some(true)
                || style_bool(&item.style, "underline_if") == Some(true)
                || style_bool(&item.style, "__hover_underline_if") == Some(true)
        })
        .map(|item| item.node.clone())
        .collect()
}

fn text_align(kind: &DocumentNodeKind, style: &StyleMap) -> TextAlign {
    if style_bool(style, "center") == Some(true) {
        return TextAlign::Center;
    }
    match style_text(style, "text_align")
        .or_else(|| style_text(style, "align_text"))
        .or_else(|| style_text(style, "align"))
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("left") => TextAlign::Left,
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ if matches!(kind, DocumentNodeKind::Button | DocumentNodeKind::Checkbox) => {
            TextAlign::Center
        }
        _ => TextAlign::Left,
    }
}

fn text_vertical_align(kind: &DocumentNodeKind, style: &StyleMap) -> TextVerticalAlign {
    if style_bool(style, "center_y") == Some(true)
        || style_bool(style, "center_vertical") == Some(true)
    {
        return TextVerticalAlign::Center;
    }
    let vertical_align = style_text(style, "vertical_align")
        .or_else(|| style_text(style, "align_y"))
        .map(str::to_ascii_lowercase);
    match vertical_align.as_deref() {
        Some("top") => TextVerticalAlign::Top,
        Some("center") => TextVerticalAlign::Center,
        Some("bottom") => TextVerticalAlign::Bottom,
        _ if matches!(
            kind,
            DocumentNodeKind::Button
                | DocumentNodeKind::Checkbox
                | DocumentNodeKind::Text
                | DocumentNodeKind::TextInput
                | DocumentNodeKind::TableCell
        ) =>
        {
            TextVerticalAlign::Center
        }
        _ => TextVerticalAlign::Top,
    }
}

fn style_number(style: &StyleMap, key: &str) -> Option<f32> {
    match state_style_value(style, key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) => None,
    }
}

fn style_bool(style: &StyleMap, key: &str) -> Option<bool> {
    match state_style_value(style, key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) => None,
    }
}

fn style_text<'a>(style: &'a StyleMap, key: &str) -> Option<&'a str> {
    match state_style_value(style, key)? {
        StyleValue::Text(value) => Some(value.as_str()),
        StyleValue::Number(_) | StyleValue::Bool(_) => None,
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
    style.get(key)
}

fn style_bool_raw(style: &StyleMap, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) => None,
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

fn clipped_item_bounds(item: &DisplayItem) -> Option<Rect> {
    clip_rect_for_style(&item.style).map_or(Some(item.bounds), |clip| {
        rect_intersection(item.bounds, clip)
    })
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

#[derive(Clone, Copy, Debug)]
struct BorderDecoration {
    rect: Rect,
    radius: f32,
    all: Option<BorderStroke>,
    sides: [Option<BorderStroke>; 4],
}

impl BorderDecoration {
    fn metric_rect_count(self) -> u32 {
        u32::from(self.all.is_some())
            + self.sides.iter().filter(|side| side.is_some()).count() as u32
    }
}

fn rect_vertices(
    frame: &LayoutFrame,
    width: f32,
    height: f32,
    text_layouts: Option<&TextRunLayoutMap>,
) -> (Vec<QuadBatch>, RectVertexMetrics) {
    let mut builder = QuadBuilder::default();
    let mut border_decorations = Vec::new();
    let mut metrics = RectVertexMetrics {
        rendered_rect_count: 1,
        ..RectVertexMetrics::default()
    };
    push_rect(
        &mut builder,
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        width,
        height,
        [0.965, 0.972, 0.982, 1.0],
    );
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    for (index, item) in frame
        .display_list
        .iter()
        .filter(|item| rect_intersects(item.bounds, viewport))
        .enumerate()
    {
        metrics.visible_display_item_count += 1;
        let Some(item_bounds) = clipped_item_bounds(item) else {
            continue;
        };
        if style_bool(&item.style, "paint") == Some(false)
            || (style_bool(&item.style, "__hover_visible") == Some(true)
                && style_bool(&item.style, "__hover_paint") != Some(true))
        {
            continue;
        }
        let border_radius = style_number(&item.style, "border_radius").unwrap_or(0.0);
        push_shadows(
            &mut builder,
            item_bounds,
            width,
            height,
            &item.style,
            border_radius,
        );
        metrics.rendered_rect_count += push_frosted_material_layers(
            &mut builder,
            item_bounds,
            width,
            height,
            &item.style,
            border_radius,
        );
        let fill = style_color_f32(&item.style, "bg")
            .or_else(|| style_color_f32(&item.style, "background"))
            .unwrap_or_else(|| default_fill_for_kind(&item.kind, index));
        let fill = material_adjusted_fill(fill, &item.style);
        push_styled_rect(
            &mut builder,
            item_bounds,
            width,
            height,
            fill,
            border_radius,
        );
        metrics.rendered_rect_count += 1;
        push_material_highlights(
            &mut builder,
            item_bounds,
            width,
            height,
            &item.style,
            border_radius,
        );
        if let Some(asset_url) = style_asset_url(&item.style) {
            push_asset_rect(&mut builder, item_bounds, width, height, asset_url);
            metrics.rendered_rect_count += 1;
        }
        if let Some(decoration) =
            border_decoration_for_style(item_bounds, &item.style, border_radius)
        {
            metrics.rendered_rect_count += decoration.metric_rect_count();
            border_decorations.push(decoration);
        }
        if matches!(item.kind, DocumentNodeKind::Text) {
            let font_size = style_number(&item.style, "size").unwrap_or(14.0);
            let text_layout = text_layouts.and_then(|layouts| layouts.get(&item.node));
            let line_top = item.bounds.y + 2.0;
            let line_height = (item.bounds.height - 4.0).max(font_size);
            if let (Some(start), Some(end)) = (
                style_number(&item.style, "editor_selection_start"),
                style_number(&item.style, "editor_selection_end"),
            ) && let Some(text_layout) = text_layout
            {
                let selection_color = style_color_f32(&item.style, "editor_selection_color")
                    .unwrap_or([0.048, 0.06, 0.08, 1.0]);
                let start = start.max(0.0);
                let end = end.max(start);
                let start_x = text_layout.x_for_column(start);
                let end_x = text_layout.x_for_column(end);
                push_rect(
                    &mut builder,
                    Rect {
                        x: start_x,
                        y: line_top,
                        width: (end_x - start_x).max(2.0),
                        height: line_height,
                    },
                    width,
                    height,
                    selection_color,
                );
                metrics.rendered_rect_count += 1;
            }
            if let Some(columns) = style_text(&item.style, "editor_bracket_columns")
                && let Some(text_layout) = text_layout
            {
                let bracket_color = style_color_f32(&item.style, "editor_bracket_color")
                    .unwrap_or([0.322, 0.545, 1.0, 0.20]);
                for column in columns
                    .split(',')
                    .filter_map(|column| column.parse::<f32>().ok())
                {
                    let cell_width = text_layout.width_for_column(column.max(0.0));
                    let bracket_width = (cell_width * 0.72).max(2.0);
                    let bracket_x = text_layout.x_for_column(column.max(0.0))
                        + (cell_width - bracket_width) * 0.5;
                    push_rect(
                        &mut builder,
                        Rect {
                            x: bracket_x,
                            y: line_top,
                            width: bracket_width,
                            height: line_height,
                        },
                        width,
                        height,
                        bracket_color,
                    );
                    metrics.rendered_rect_count += 1;
                }
            }
            if style_bool(&item.style, "editor_caret_visible") == Some(true)
                && let Some(column) = style_number(&item.style, "editor_caret_column")
                && let Some(text_layout) = text_layout
            {
                let caret_color = style_color_f32(&item.style, "editor_caret_color")
                    .or_else(|| style_color_f32(&item.style, "color"))
                    .unwrap_or([0.09, 0.23, 1.0, 1.0]);
                push_rect(
                    &mut builder,
                    Rect {
                        x: text_layout.x_for_column(column.max(0.0)),
                        y: line_top,
                        width: 2.0,
                        height: line_height,
                    },
                    width,
                    height,
                    caret_color,
                );
                metrics.rendered_rect_count += 1;
            }
        }
        if style_bool(&item.style, "strikethrough") == Some(true) {
            let color = style_color_f32(&item.style, "if_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.58, 0.58, 0.58, 1.0]);
            let text_layout = text_layouts.and_then(|layouts| layouts.get(&item.node));
            let rect = strikethrough_rect_for_item(item, text_layout);
            push_rect(&mut builder, rect, width, height, color);
            metrics.rendered_rect_count += 1;
        }
        if style_bool(&item.style, "underline_if") == Some(true) {
            let color = style_color_f32(&item.style, "underline_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.58, 0.58, 0.58, 1.0]);
            let text_layout = text_layouts.and_then(|layouts| layouts.get(&item.node));
            push_rect(
                &mut builder,
                underline_rect_for_item(item, text_layout),
                width,
                height,
                color,
            );
            metrics.rendered_rect_count += 1;
        }
        if matches!(item.kind, DocumentNodeKind::Checkbox) && !checkbox_has_asset_icon(frame, item)
        {
            push_checkbox(&mut builder, item.bounds, width, height, &item.style);
            metrics.rendered_rect_count += 1;
        }
        if matches!(item.kind, DocumentNodeKind::Button)
            && style_bool(&item.style, "checked") == Some(true)
        {
            let color = style_color_f32(&item.style, "check_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.36, 0.76, 0.69, 1.0]);
            let left = item.bounds.x + 9.0;
            let top = item.bounds.y + 12.0;
            push_rect(
                &mut builder,
                Rect {
                    x: left,
                    y: top + 11.0,
                    width: 3.0,
                    height: 9.0,
                },
                width,
                height,
                color,
            );
            push_rect(
                &mut builder,
                Rect {
                    x: left + 5.0,
                    y: top + 4.0,
                    width: 3.0,
                    height: 17.0,
                },
                width,
                height,
                color,
            );
            metrics.rendered_rect_count += 2;
        }
        if matches!(item.kind, DocumentNodeKind::TextInput)
            && (item.focused || style_bool(&item.style, "focus") == Some(true))
            && style_bool(&item.style, "caret_visible") == Some(true)
        {
            let text_bounds = text_content_bounds_for_item(item);
            let color = style_color_f32(&item.style, "caret_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.22, 0.22, 0.22, 1.0]);
            let font_size = style_number(&item.style, "size").unwrap_or(14.0);
            let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
            let vertical_align = text_vertical_align(&item.kind, &item.style);
            let line_height =
                style_line_height(&item.style, font_size).min(text_bounds.height.max(1.0));
            let line_top = text_top_for_parts(text_bounds, line_height, text_inset, vertical_align);
            let caret_column = style_number(&item.style, "caret_column").unwrap_or(0.0);
            let caret_x = text_layouts
                .and_then(|layouts| layouts.get(&item.node))
                .map(|layout| layout.x_for_column(caret_column.max(0.0)))
                .unwrap_or(text_bounds.x + text_inset);
            push_rect(
                &mut builder,
                Rect {
                    x: caret_x,
                    y: line_top,
                    width: 2.0,
                    height: line_height,
                },
                width,
                height,
                color,
            );
            metrics.rendered_rect_count += 1;
        }
    }
    for decoration in border_decorations {
        push_border_decoration(&mut builder, decoration, width, height);
    }
    if builder.batches.is_empty() {
        push_rect(
            &mut builder,
            Rect {
                x: width * 0.05,
                y: height * 0.05,
                width: width * 0.1,
                height: height * 0.1,
            },
            width,
            height,
            [0.2, 0.6, 0.9, 1.0],
        );
    }
    (builder.batches, metrics)
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

fn style_asset_url(style: &StyleMap) -> Option<&str> {
    style_text(style, "asset_url")
        .or_else(|| style_text(style, "background_url"))
        .filter(|url| !url.trim().is_empty())
}

fn border_decoration_for_style(
    rect: Rect,
    style: &StyleMap,
    radius: f32,
) -> Option<BorderDecoration> {
    let all = style_color_f32(style, "border").map(|color| BorderStroke {
        color,
        thickness: style_number(style, "border_width").unwrap_or(2.0),
    });
    let mut sides = [None, None, None, None];
    for (index, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
        let Some(color) = style_color_f32(style, &format!("border_{side}")) else {
            continue;
        };
        sides[index] = Some(BorderStroke {
            color,
            thickness: style_number(style, &format!("border_{side}_width"))
                .or_else(|| style_number(style, "border_width"))
                .unwrap_or(1.0),
        });
    }
    if all.is_none() && sides.iter().all(Option::is_none) {
        return None;
    }
    Some(BorderDecoration {
        rect,
        radius,
        all,
        sides,
    })
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

fn push_border_decoration(
    builder: &mut QuadBuilder,
    decoration: BorderDecoration,
    width: f32,
    height: f32,
) {
    if let Some(stroke) = decoration.all {
        push_styled_border_all(
            builder,
            decoration.rect,
            width,
            height,
            stroke.color,
            stroke.thickness,
            decoration.radius,
        );
    }
    for (index, stroke) in decoration.sides.into_iter().enumerate() {
        if let Some(stroke) = stroke {
            push_side_border(builder, decoration.rect, width, height, index, stroke);
        }
    }
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

fn push_shadows(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
    radius: f32,
) {
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    for index in (1..=8).rev() {
        let color_key = format!("box_shadow_{index}_color");
        let Some(color) = style_color_f32(style, &color_key) else {
            continue;
        };
        let x = style_number(style, &format!("box_shadow_{index}_x")).unwrap_or(0.0);
        let y = style_number(style, &format!("box_shadow_{index}_y")).unwrap_or(0.0);
        let blur = style_number(style, &format!("box_shadow_{index}_blur")).unwrap_or(0.0);
        let spread = style_number(style, &format!("box_shadow_{index}_spread")).unwrap_or(0.0);
        let inset = style_bool(style, &format!("box_shadow_{index}_inset")) == Some(true);
        if inset {
            let thickness = blur.max(1.0);
            let band = Rect {
                x: rect.x,
                y: rect.y + rect.height - thickness + y,
                width: rect.width,
                height: thickness,
            };
            push_styled_rect(builder, band, width, height, color, radius);
        } else {
            let base = Rect {
                x: rect.x + x - spread,
                y: rect.y + y - spread,
                width: (rect.width + spread * 2.0).max(1.0),
                height: (rect.height + spread * 2.0).max(1.0),
            };
            if radius > 0.25 {
                let base_radius =
                    (radius + spread.max(0.0)).clamp(0.0, base.width.min(base.height) * 0.5);
                if blur <= 0.0 {
                    push_styled_rect(builder, base, width, height, color, base_radius);
                    continue;
                }
                push_styled_rect(
                    builder,
                    base,
                    width,
                    height,
                    color_with_alpha_scale(color, 0.42),
                    base_radius,
                );
                let steps = blur.ceil().clamp(2.0, 18.0) as u32;
                for step in (0..steps).rev() {
                    let outer_expand = blur * (step + 1) as f32 / steps as f32;
                    let t = (step + 1) as f32 / steps as f32;
                    let alpha_scale = (1.0 - t).powi(2) * 0.36;
                    if alpha_scale < 0.01 {
                        continue;
                    }
                    let layer = expanded_rect(base, outer_expand);
                    let layer_radius = (base_radius + outer_expand)
                        .clamp(0.0, layer.width.min(layer.height) * 0.5);
                    push_styled_rect(
                        builder,
                        layer,
                        width,
                        height,
                        color_with_alpha_scale(color, alpha_scale),
                        layer_radius,
                    );
                }
                continue;
            }
            if blur <= 0.0 {
                push_rect_difference(builder, base, rect, width, height, color);
                continue;
            }
            push_rect_difference(
                builder,
                base,
                rect,
                width,
                height,
                color_with_alpha_scale(color, 0.78),
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
                push_shadow_halo(
                    builder,
                    base,
                    rect,
                    inner_expand,
                    outer_expand,
                    width,
                    height,
                    color_with_alpha_scale(color, alpha_scale),
                );
            }
        }
    }
}

fn material_adjusted_fill(mut fill: [f32; 4], style: &StyleMap) -> [f32; 4] {
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
    fill
}

fn push_frosted_material_layers(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
    radius: f32,
) -> u32 {
    let frosted_blur = style_number(style, "frosted_blur")
        .unwrap_or(0.0)
        .clamp(0.0, 40.0);
    let frosted_saturate = style_number(style, "frosted_saturate")
        .unwrap_or(1.0)
        .clamp(0.0, 2.0);
    if frosted_blur <= 0.01 && frosted_saturate <= 1.01 {
        return 0;
    }
    let highlight = style_number(style, "glass_highlight")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let mut haze = style_color_f32(style, "glass_highlight_color").unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let steps = (frosted_blur / 7.0).ceil().clamp(2.0, 5.0) as u32;
    let mut count = 0;
    for step in 0..steps {
        let t = (step + 1) as f32 / steps as f32;
        let expand = frosted_blur * 0.18 * t;
        haze[3] = ((frosted_blur / 40.0) * 0.030 + (frosted_saturate - 1.0) * 0.025)
            .mul_add(1.0 - t * 0.55, highlight * 0.010)
            .clamp(0.004, 0.055);
        push_styled_rect(
            builder,
            expanded_rect(rect, expand),
            width,
            height,
            haze,
            radius + expand,
        );
        count += 1;
    }
    count
}

fn push_material_highlights(
    builder: &mut QuadBuilder,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
    radius: f32,
) {
    let radius = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    let gloss = style_number(style, "gloss").unwrap_or(0.0).clamp(0.0, 1.0);
    let transparency = style_number(style, "transparency")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let refraction = style_number(style, "refraction").unwrap_or(0.0).max(0.0);
    let depth = style_number(style, "depth").unwrap_or(0.0).max(0.0);
    let glass_highlight = style_number(style, "glass_highlight")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let highlight_color =
        style_color_f32(style, "glass_highlight_color").unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let top_alpha =
        (gloss * 0.11 + transparency * 0.08 + refraction * 0.015 + glass_highlight * 0.12)
            .clamp(0.0, 0.30);
    if top_alpha > 0.01 && rect.width > 2.0 && rect.height > 2.0 {
        let band = (1.0 + gloss * 2.0 + transparency * 2.0 + glass_highlight * 4.0).clamp(1.0, 7.0);
        push_styled_rect(
            builder,
            Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: band.min(rect.height),
            },
            width,
            height,
            color_with_alpha_scale(highlight_color, top_alpha / highlight_color[3].max(0.001)),
            radius,
        );
        push_styled_rect(
            builder,
            Rect {
                x: rect.x,
                y: rect.y,
                width: band.min(rect.width),
                height: rect.height,
            },
            width,
            height,
            color_with_alpha_scale(
                highlight_color,
                (top_alpha * 0.45) / highlight_color[3].max(0.001),
            ),
            radius,
        );
        if glass_highlight > 0.01 && rect.width > 24.0 && rect.height > 16.0 {
            let glint_width = (rect.width * 0.18).clamp(10.0, 34.0);
            push_styled_rect(
                builder,
                Rect {
                    x: rect.x + rect.width - glint_width - 2.0,
                    y: rect.y + 2.0,
                    width: glint_width,
                    height: (band * 0.75).min(rect.height),
                },
                width,
                height,
                color_with_alpha_scale(
                    highlight_color,
                    (top_alpha * 0.65) / highlight_color[3].max(0.001),
                ),
                radius,
            );
        }
    }
    let bottom_alpha = ((1.0 - gloss) * 0.035 + depth * 0.006).clamp(0.0, 0.18);
    if bottom_alpha > 0.01 && rect.width > 2.0 && rect.height > 3.0 {
        let band = (1.0 + depth * 0.16).clamp(1.0, 4.0);
        push_styled_rect(
            builder,
            Rect {
                x: rect.x,
                y: rect.y + rect.height - band.min(rect.height),
                width: rect.width,
                height: band.min(rect.height),
            },
            width,
            height,
            [0.0, 0.0, 0.0, bottom_alpha],
            radius,
        );
    }
}

fn mix_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
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

fn push_shadow_halo(
    builder: &mut QuadBuilder,
    rect: Rect,
    occluder: Rect,
    inner_expand: f32,
    outer_expand: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
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
            push_rect_difference(builder, band, occluder, width, height, color);
        }
    }
}

fn push_rect_difference(
    builder: &mut QuadBuilder,
    rect: Rect,
    cutout: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let Some(overlap) = rect_intersection(rect, cutout) else {
        push_rect(builder, rect, width, height, color);
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
            push_rect(builder, band, width, height, color);
        }
    }
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

fn expanded_rect(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: (rect.width + amount * 2.0).max(1.0),
        height: (rect.height + amount * 2.0).max(1.0),
    }
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

fn distance_to_segment(point: (f32, f32), start: (f32, f32), end: (f32, f32)) -> f32 {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return (point.0 - start.0).hypot(point.1 - start.1);
    }
    let t =
        (((point.0 - start.0) * dx + (point.1 - start.1) * dy) / length_squared).clamp(0.0, 1.0);
    let closest = (start.0 + dx * t, start.1 + dy * t);
    (point.0 - closest.0).hypot(point.1 - closest.1)
}

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
    let half = thickness.max(1.0) * 0.5;
    let aa = aa.max(0.0);
    let margin = half + aa + 1.0;
    let min_x = (start.0.min(middle.0).min(end.0) - margin).floor() as i32;
    let max_x = (start.0.max(middle.0).max(end.0) + margin).ceil() as i32;
    let min_y = (start.1.min(middle.1).min(end.1) - margin).floor() as i32;
    let max_y = (start.1.max(middle.1).max(end.1) + margin).ceil() as i32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let sample = (x as f32 + 0.5, y as f32 + 0.5);
            let distance = distance_to_segment(sample, start, middle)
                .min(distance_to_segment(sample, middle, end));
            let coverage = if aa <= 0.0 {
                if distance <= half { 1.0 } else { 0.0 }
            } else {
                let edge0 = half - aa;
                let edge1 = half + aa;
                let t = ((distance - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                let smooth = t * t * (3.0 - 2.0 * t);
                1.0 - smooth
            };
            if coverage <= 0.001 {
                continue;
            }
            push_rect(
                builder,
                Rect {
                    x: x as f32,
                    y: y as f32,
                    width: 1.0,
                    height: 1.0,
                },
                width,
                height,
                color_with_alpha_scale(color, coverage),
            );
        }
    }
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
    let inner_radius = (radius - border_width).max(0.0);
    let min_x = (center_x - radius - aa).floor() as i32;
    let max_x = (center_x + radius + aa).ceil() as i32;
    let min_y = (center_y - radius - aa).floor() as i32;
    let max_y = (center_y + radius + aa).ceil() as i32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let sample_x = x as f32 + 0.5;
            let sample_y = y as f32 + 0.5;
            let distance = (sample_x - center_x).hypot(sample_y - center_y);
            let outer = circle_coverage(radius, aa, distance);
            if outer <= 0.001 {
                continue;
            }
            let inner = circle_coverage(inner_radius, aa, distance);
            let ring_alpha = (outer - inner).clamp(0.0, 1.0);
            if inner > 0.001 {
                push_rect(
                    builder,
                    Rect {
                        x: x as f32,
                        y: y as f32,
                        width: 1.0,
                        height: 1.0,
                    },
                    width,
                    height,
                    color_with_alpha_scale(inner_color, inner),
                );
            }
            if ring_alpha > 0.001 {
                push_rect(
                    builder,
                    Rect {
                        x: x as f32,
                        y: y as f32,
                        width: 1.0,
                        height: 1.0,
                    },
                    width,
                    height,
                    color_with_alpha_scale(ring_color, ring_alpha),
                );
            }
        }
    }
}

fn push_checkbox(builder: &mut QuadBuilder, rect: Rect, width: f32, height: f32, style: &StyleMap) {
    let checked = style_bool(style, "checked") == Some(true);
    let (center_x, center_y) = checkbox_circle_center(rect);
    let radius = (rect.width.min(rect.height) * 0.5
        - style_number(style, "checkbox_inset").unwrap_or(2.0)
        - 0.5)
        .max(1.0);
    let border_width = style_number(style, "checkbox_border_width").unwrap_or(1.5);
    let ring_color = if checked {
        style_color_f32(style, "checked_border").unwrap_or([0.101, 0.356, 0.292, 1.0])
    } else {
        style_color_f32(style, "checkbox_border").unwrap_or([0.830, 0.830, 0.830, 1.0])
    };
    let inner_color = style_color_f32(style, "checkbox_background").unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let aa = style_number(style, "checkbox_aa")
        .unwrap_or(1.25)
        .clamp(0.0, 2.0);
    if let Some(shadow_color) = style_color_f32(style, "checkbox_cast_color")
        && shadow_color[3] > 0.001
    {
        let shadow_x = style_number(style, "checkbox_cast_x").unwrap_or(0.0);
        let shadow_y = style_number(style, "checkbox_cast_y").unwrap_or(0.0);
        let shadow_blur = style_number(style, "checkbox_cast_blur")
            .unwrap_or(2.0)
            .clamp(0.0, 8.0);
        let shadow_spread = style_number(style, "checkbox_cast_spread")
            .unwrap_or(0.0)
            .clamp(-2.0, 4.0);
        push_checkbox_circle_raster(
            builder,
            center_x + shadow_x,
            center_y + shadow_y,
            (radius + shadow_spread + shadow_blur * 0.3).max(1.0),
            0.0,
            (aa + shadow_blur).clamp(0.0, 8.0),
            width,
            height,
            [0.0, 0.0, 0.0, 0.0],
            shadow_color,
        );
    }
    push_checkbox_circle_raster(
        builder,
        center_x,
        center_y,
        radius,
        border_width,
        aa,
        width,
        height,
        ring_color,
        inner_color,
    );
    if let Some(inner_shadow) = style_color_f32(style, "checkbox_inner_shadow")
        && inner_shadow[3] > 0.001
    {
        push_checkbox_circle_raster(
            builder,
            center_x,
            center_y,
            (radius - border_width * 0.5).max(1.0),
            style_number(style, "checkbox_inner_shadow_width")
                .unwrap_or(1.0)
                .max(0.25),
            aa,
            width,
            height,
            inner_shadow,
            [0.0, 0.0, 0.0, 0.0],
        );
    }
    if let Some(highlight) = style_color_f32(style, "checkbox_highlight")
        && highlight[3] > 0.001
    {
        push_checkbox_circle_raster(
            builder,
            center_x - 0.5,
            center_y - 0.5,
            (radius - border_width * 0.35).max(1.0),
            style_number(style, "checkbox_highlight_width")
                .unwrap_or(1.0)
                .max(0.0),
            aa,
            width,
            height,
            highlight,
            [0.0, 0.0, 0.0, 0.0],
        );
    }
    if checked {
        let (start, middle, end) = checkbox_check_points(rect);
        let color = style_color_f32(style, "check_color").unwrap_or([0.108, 0.540, 0.432, 1.0]);
        let thickness = style_number(style, "check_width").unwrap_or(3.0);
        let check_aa = style_number(style, "check_aa")
            .unwrap_or(0.9)
            .clamp(0.0, 1.75);
        push_checkbox_check_raster(
            builder, start, middle, end, thickness, check_aa, width, height, color,
        );
    }
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

fn color_with_alpha_scale(mut color: [f32; 4], scale: f32) -> [f32; 4] {
    color[3] = (color[3] * scale).clamp(0.0, 1.0);
    color
}

fn default_fill_for_kind(kind: &DocumentNodeKind, index: usize) -> [f32; 4] {
    match kind {
        DocumentNodeKind::Root | DocumentNodeKind::Stack | DocumentNodeKind::ScrollRoot => {
            [0.965, 0.972, 0.982, 1.0]
        }
        DocumentNodeKind::Row => {
            if index % 2 == 0 {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.949, 0.965, 0.984, 1.0]
            }
        }
        DocumentNodeKind::TextInput => [1.0, 1.0, 1.0, 1.0],
        DocumentNodeKind::Button => [1.0, 1.0, 1.0, 0.0],
        DocumentNodeKind::Checkbox => [1.0, 1.0, 1.0, 0.0],
        DocumentNodeKind::Table | DocumentNodeKind::TableCell => [1.0, 1.0, 1.0, 1.0],
        DocumentNodeKind::Text => [1.0, 1.0, 1.0, 0.0],
    }
}

fn style_color_f32(style: &StyleMap, key: &str) -> Option<[f32; 4]> {
    style_color_u8(style, key).map(|color| {
        [
            srgb_u8_to_linear_f32(color[0]),
            srgb_u8_to_linear_f32(color[1]),
            srgb_u8_to_linear_f32(color[2]),
            color[3] as f32 / 255.0,
        ]
    })
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
    let StyleValue::Text(value) = state_style_value(style, key)? else {
        return None;
    };
    parse_oklch_color(value).or_else(|| parse_hex_color(value))
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
mod tests {
    use super::*;
    use boon_document::{AccessibilityTree, DisplayItem, DocumentNodeId, LayoutMetrics};

    fn flatten_quad_batches(batches: &[QuadBatch]) -> (Vec<f32>, Vec<u8>) {
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        for batch in batches {
            positions.extend_from_slice(&batch.positions);
            for color in &batch.colors {
                colors.extend_from_slice(&rgba8_from_packed(*color));
            }
        }
        (positions, colors)
    }

    fn vertex_pixels_for_color(
        positions: &[f32],
        colors: &[u8],
        surface_width: f32,
        surface_height: f32,
        target_color: [u8; 4],
    ) -> Vec<(f32, f32)> {
        colors
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(index, color)| {
                (color == target_color).then(|| {
                    let x_ndc = positions[index * 2];
                    let y_ndc = positions[index * 2 + 1];
                    let x = (x_ndc + 1.0) * 0.5 * surface_width;
                    let y = (1.0 - y_ndc) * 0.5 * surface_height;
                    (x, y)
                })
            })
            .collect()
    }

    fn has_vertex_at_pixel(vertices: &[(f32, f32)], x: f32, y: f32) -> bool {
        vertices
            .iter()
            .any(|(vx, vy)| (*vx - x).abs() <= 0.01 && (*vy - y).abs() <= 0.01)
    }

    fn shape_glyph_ids(text: &str, font_features: FontFeatures) -> Vec<u16> {
        shape_glyphs(text, font_features)
            .into_iter()
            .map(|(glyph_id, _)| glyph_id)
            .collect()
    }

    fn shape_glyphs(text: &str, font_features: FontFeatures) -> Vec<(u16, f32)> {
        let mut font_system = editor_font_system();
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 22.0));
        buffer.set_size(&mut font_system, Some(320.0), Some(32.0));
        buffer.set_text(
            &mut font_system,
            text,
            &Attrs::new()
                .family(Family::Name("JetBrains Mono"))
                .font_features(font_features),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);
        buffer.lines[0]
            .shape_opt()
            .expect("line should be shaped")
            .spans
            .iter()
            .flat_map(|span| span.words.iter())
            .flat_map(|word| word.glyphs.iter())
            .map(|glyph| (glyph.glyph_id, glyph.x_advance))
            .collect()
    }

    fn shape_rich_glyph_ids(spans: &[(&str, [u8; 4], Style, Weight)]) -> Vec<u16> {
        shape_rich_glyphs(spans)
            .into_iter()
            .map(|(glyph_id, _)| glyph_id)
            .collect()
    }

    fn shape_rich_glyphs(spans: &[(&str, [u8; 4], Style, Weight)]) -> Vec<(u16, f32)> {
        let mut font_system = editor_font_system();
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 22.0));
        buffer.set_size(&mut font_system, Some(320.0), Some(32.0));
        let default_attrs = text_attrs(
            "JetBrains Mono",
            Style::Normal,
            Weight::NORMAL,
            [217, 225, 242, 255],
            "zero,calt",
        );
        buffer.set_rich_text(
            &mut font_system,
            spans.iter().map(|(text, color, style, weight)| {
                (
                    *text,
                    text_attrs("JetBrains Mono", *style, *weight, *color, "zero,calt"),
                )
            }),
            &default_attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);
        buffer.lines[0]
            .shape_opt()
            .expect("line should be shaped")
            .spans
            .iter()
            .flat_map(|span| span.words.iter())
            .flat_map(|word| word.glyphs.iter())
            .map(|glyph| (glyph.glyph_id, glyph.x_advance))
            .collect()
    }

    fn disabled_editor_ligature_features() -> FontFeatures {
        let mut features = FontFeatures::new();
        features.disable(FeatureTag::CONTEXTUAL_ALTERNATES);
        features.disable(FeatureTag::STANDARD_LIGATURES);
        features.disable(FeatureTag::CONTEXTUAL_LIGATURES);
        features
    }

    #[test]
    fn bundled_editor_font_applies_calt_ligature_substitutions() {
        let enabled = text_font_features("zero,calt");
        assert_ne!(
            shape_glyph_ids("--", disabled_editor_ligature_features()),
            shape_glyph_ids("--", enabled.clone()),
            "patched JetBrains Mono must substitute dash sequences through calt"
        );
        let raw_pipe = shape_glyph_ids("|>", disabled_editor_ligature_features());
        let shaped_pipe = shape_glyph_ids("|>", enabled);
        assert_eq!(raw_pipe.len(), 2);
        assert_eq!(shaped_pipe.len(), 2);
        assert_ne!(
            raw_pipe, shaped_pipe,
            "patched JetBrains Mono must substitute pipe-forward through calt"
        );
        assert_ne!(
            raw_pipe[0], shaped_pipe[0],
            "pipe-forward must replace the raw bar with the pipe ligature glyph"
        );
        assert_ne!(
            raw_pipe[1], shaped_pipe[1],
            "pipe-forward must replace the raw greater-than with an invisible filler glyph"
        );
    }

    #[test]
    fn rich_editor_spans_shape_pipe_forward_inside_operator_span() {
        let raw_pipe = shape_glyph_ids("|>", disabled_editor_ligature_features());
        let rich_pipe = shape_rich_glyph_ids(&[
            ("0 ", [217, 225, 242, 255], Style::Normal, Weight::NORMAL),
            ("|>", [255, 159, 67, 255], Style::Normal, Weight::BOLD),
            (
                " HOLD",
                [210, 105, 30, 255],
                Style::Italic,
                Weight::EXTRA_BOLD,
            ),
        ]);
        assert!(
            !rich_pipe
                .windows(raw_pipe.len())
                .any(|window| window == raw_pipe)
        );
        assert!(
            rich_pipe.iter().any(|glyph_id| *glyph_id == 1563),
            "rich editor spans must shape |> to the bundled pipe-forward ligature glyph"
        );
    }

    #[test]
    fn styled_editor_spans_keep_dash_ligatures_on_patched_jetbrains_variants() {
        let raw_dash = shape_glyph_ids("--", disabled_editor_ligature_features());
        let styled_dash = shape_rich_glyph_ids(&[(
            "-- comment",
            [119, 136, 153, 255],
            Style::Italic,
            Weight::NORMAL,
        )]);
        assert!(
            !styled_dash
                .windows(raw_dash.len())
                .any(|window| window == raw_dash)
        );
        assert!(
            styled_dash.iter().any(|glyph_id| *glyph_id == 876),
            "italic comment spans must shape -- through the bundled patched JetBrains variant"
        );
    }

    #[test]
    fn styled_editor_punctuation_stays_monospace_across_weights() {
        let punctuation = shape_rich_glyphs(&[
            ("_([", [210, 105, 30, 255], Style::Normal, Weight::BOLD),
            (
                " (([]))",
                [210, 105, 30, 255],
                Style::Italic,
                Weight::EXTRA_BOLD,
            ),
        ]);
        assert_eq!(punctuation.len(), 10);
        let first_advance = punctuation
            .first()
            .map(|(_, advance)| *advance)
            .expect("punctuation should shape");
        assert!(
            punctuation
                .iter()
                .all(|(_, advance)| (*advance - first_advance).abs() < f32::EPSILON),
            "styled punctuation must stay on the bundled monospace JetBrains variants: {punctuation:?}"
        );
    }

    #[test]
    fn editor_text_overlays_emit_selection_bracket_and_caret_rects() {
        let mut style = StyleMap::new();
        style.insert("bg".to_owned(), StyleValue::Text("#282c34".to_owned()));
        style.insert("size".to_owned(), StyleValue::Number(16.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert("editor_selection_start".to_owned(), StyleValue::Number(1.0));
        style.insert("editor_selection_end".to_owned(), StyleValue::Number(4.0));
        style.insert(
            "editor_selection_color".to_owned(),
            StyleValue::Text("#3E4451".to_owned()),
        );
        style.insert(
            "editor_bracket_columns".to_owned(),
            StyleValue::Text("0,5".to_owned()),
        );
        style.insert(
            "editor_bracket_color".to_owned(),
            StyleValue::Text("#528bff40".to_owned()),
        );
        style.insert("editor_caret_column".to_owned(), StyleValue::Number(3.0));
        style.insert("editor_caret_visible".to_owned(), StyleValue::Bool(true));
        style.insert(
            "editor_caret_color".to_owned(),
            StyleValue::Text("#528bff".to_owned()),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("editor-line".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 200.0,
                    height: 22.0,
                },
                text: Some("(abc)".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let text_layouts = test_text_layouts(&frame, 320, 120);
        let (_, metrics) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        assert!(
            metrics.rendered_rect_count >= 6,
            "background + item + selection + two brackets + caret should render"
        );

        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let (positions, colors) = flatten_quad_batches(&batches);
        let rect_colors = colors
            .chunks_exact(24)
            .map(|color| [color[0], color[1], color[2], color[3]])
            .collect::<Vec<_>>();
        let selection_rect = rect_colors
            .iter()
            .position(|color| *color == [12, 15, 21, 255])
            .expect("selection highlight rect should render");
        assert_eq!(
            &colors[selection_rect * 24..selection_rect * 24 + 4],
            &[12, 15, 21, 255],
            "selection highlight must stay opaque while bracket highlights are softened"
        );
        let first_bracket_rect = rect_colors
            .iter()
            .position(|color| *color == [22, 66, 255, 64])
            .expect("bracket highlight rect should render");
        let x0_ndc = positions[first_bracket_rect * 12];
        let x1_ndc = positions[first_bracket_rect * 12 + 2];
        let bracket_width = ((x1_ndc - x0_ndc) * 0.5) * 320.0;
        let cell_width = text_layouts
            .get(&DocumentNodeId("editor-line".to_owned()))
            .expect("text layout should exist")
            .width_for_column(0.0);
        assert!(
            bracket_width < cell_width,
            "bracket highlight should be narrower than a full cell to avoid bleeding into neighbors"
        );
        assert_eq!(
            &colors[first_bracket_rect * 24..first_bracket_rect * 24 + 4],
            &[22, 66, 255, 64]
        );
    }

    #[test]
    fn focused_borders_use_style_color_not_hard_coded_blue() {
        let mut style = StyleMap::new();
        style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        style.insert("border".to_owned(), StyleValue::Text("#ff0000".to_owned()));
        style.insert("border_width".to_owned(), StyleValue::Number(1.0));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("input".to_owned()),
                kind: DocumentNodeKind::TextInput,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 32.0,
                },
                text: Some("abc".to_owned()),
                style,
                focused: true,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let text_layouts = test_text_layouts(&frame, 320, 120);
        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let (_, colors) = flatten_quad_batches(&batches);
        assert!(
            colors
                .chunks_exact(4)
                .any(|color| color == [255, 0, 0, 255]),
            "focused border should keep the explicit style color"
        );
        assert!(
            !colors
                .chunks_exact(4)
                .any(|color| color == [25, 117, 210, 255]),
            "focused border must not fall back to the old hard-coded blue"
        );
    }

    #[test]
    fn parent_borders_render_after_descendant_backgrounds() {
        let mut parent_style = StyleMap::new();
        parent_style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        parent_style.insert("border".to_owned(), StyleValue::Text("#ff0000".to_owned()));
        parent_style.insert("border_width".to_owned(), StyleValue::Number(1.0));

        let mut child_style = StyleMap::new();
        child_style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );

        let frame = LayoutFrame {
            display_list: vec![
                DisplayItem {
                    node: DocumentNodeId("parent".to_owned()),
                    kind: DocumentNodeKind::Row,
                    bounds: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 160.0,
                        height: 48.0,
                    },
                    text: None,
                    style: parent_style,
                    focused: false,
                },
                DisplayItem {
                    node: DocumentNodeId("child".to_owned()),
                    kind: DocumentNodeKind::TextInput,
                    bounds: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 160.0,
                        height: 48.0,
                    },
                    text: None,
                    style: child_style,
                    focused: false,
                },
            ],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, None);
        let (_, colors) = flatten_quad_batches(&batches);
        let vertex_colors = colors
            .chunks_exact(4)
            .map(|color| [color[0], color[1], color[2], color[3]])
            .collect::<Vec<_>>();

        assert!(
            vertex_colors
                .iter()
                .rev()
                .take(24)
                .all(|color| *color == [255, 0, 0, 255]),
            "a parent border should be emitted as an overlay after descendant backgrounds"
        );
    }

    #[test]
    fn text_without_background_uses_transparent_default_fill() {
        let mut style = StyleMap::new();
        style.insert("color".to_owned(), StyleValue::Text("#333333".to_owned()));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("label".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 24.0,
                },
                text: Some("label".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, None);
        let (_, colors) = flatten_quad_batches(&batches);
        assert!(
            colors
                .chunks_exact(4)
                .any(|color| color == [255, 255, 255, 0]),
            "text display items without an explicit background should not paint backing rectangles"
        );
    }

    #[test]
    fn button_without_background_uses_transparent_default_fill() {
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("button".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 32.0,
                },
                text: None,
                style: StyleMap::new(),
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, None);
        let (_, colors) = flatten_quad_batches(&batches);
        assert!(
            colors
                .chunks_exact(4)
                .any(|color| color == [255, 255, 255, 0]),
            "buttons without explicit material/background should not receive fallback chrome"
        );
    }

    #[test]
    fn rounded_material_highlights_respect_control_shape() {
        let mut style = StyleMap::new();
        style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        style.insert("border_radius".to_owned(), StyleValue::Number(12.0));
        style.insert("gloss".to_owned(), StyleValue::Number(1.0));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("rounded-button".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 80.0,
                    height: 24.0,
                },
                text: None,
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let (batches, _) = rect_vertices(&frame, 160.0, 90.0, None);
        let (positions, colors) = flatten_quad_batches(&batches);
        let highlight_vertices =
            vertex_pixels_for_color(&positions, &colors, 160.0, 90.0, [255, 255, 255, 28]);
        assert!(
            !highlight_vertices.is_empty(),
            "test fixture should emit a gloss highlight"
        );
        assert!(
            !has_vertex_at_pixel(&highlight_vertices, 20.0, 20.0),
            "rounded gloss highlights must not emit square top-left corner vertices"
        );
    }

    #[test]
    fn rounded_shadows_respect_control_shape() {
        let mut style = StyleMap::new();
        style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        style.insert("border_radius".to_owned(), StyleValue::Number(12.0));
        style.insert(
            "box_shadow_1_color".to_owned(),
            StyleValue::Text("#00000080".to_owned()),
        );
        style.insert("box_shadow_1_spread".to_owned(), StyleValue::Number(4.0));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("rounded-shadow".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 80.0,
                    height: 24.0,
                },
                text: None,
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let (batches, _) = rect_vertices(&frame, 160.0, 90.0, None);
        let (positions, colors) = flatten_quad_batches(&batches);
        let shadow_vertices =
            vertex_pixels_for_color(&positions, &colors, 160.0, 90.0, [0, 0, 0, 128]);
        assert!(
            !shadow_vertices.is_empty(),
            "test fixture should emit a rounded shadow"
        );
        assert!(
            !has_vertex_at_pixel(&shadow_vertices, 16.0, 16.0),
            "rounded shadows must not emit square expanded-corner vertices"
        );
    }

    #[test]
    fn focused_text_input_needs_explicit_caret_visibility_to_draw_caret() {
        let mut style = StyleMap::new();
        style.insert(
            "background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        style.insert("color".to_owned(), StyleValue::Text("#000000".to_owned()));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("input".to_owned()),
                kind: DocumentNodeKind::TextInput,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 32.0,
                },
                text: None,
                style,
                focused: true,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let text_layouts = test_text_layouts(&frame, 320, 120);
        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let (_, colors) = flatten_quad_batches(&batches);
        assert!(
            !colors.chunks_exact(4).any(|color| color == [0, 0, 0, 255]),
            "declarative focus alone should not draw a static preview caret"
        );
    }

    #[test]
    fn svg_asset_data_url_renders_into_app_owned_pixels() {
        futures::executor::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await
            {
                Ok(adapter) => adapter,
                Err(error) => {
                    eprintln!("skipping SVG asset readback test: request_adapter failed: {error}");
                    return;
                }
            };
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("boon-native-gpu-svg-asset-test-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::default(),
                    experimental_features: wgpu::ExperimentalFeatures::default(),
                })
                .await
                .expect("test WGPU device should be available when adapter exists");

            let mut style = StyleMap::new();
            style.insert(
                "asset_url".to_owned(),
                StyleValue::Text(
                    "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2240%22%20height%3D%2240%22%3E%3Crect%20x%3D%228%22%20y%3D%228%22%20width%3D%2224%22%20height%3D%2224%22%20fill%3D%22%2300ff00%22/%3E%3C/svg%3E".to_owned(),
                ),
            );
            let frame = LayoutFrame {
                display_list: vec![DisplayItem {
                    node: DocumentNodeId("svg-asset".to_owned()),
                    kind: DocumentNodeKind::Stack,
                    bounds: Rect {
                        x: 20.0,
                        y: 20.0,
                        width: 40.0,
                        height: 40.0,
                    },
                    text: None,
                    style,
                    focused: false,
                }],
                hit_regions: Vec::new(),
                scroll_regions: Vec::new(),
                accessibility: AccessibilityTree::default(),
                demands: Vec::new(),
                metrics: LayoutMetrics::default(),
            };
            let artifact_dir = Path::new("target/artifacts/native-gpu/tests");
            let proof = render_app_owned_pixels(AppOwnedRenderRequest {
                device: &device,
                queue: &queue,
                frame: &frame,
                surface_id: SurfaceId("svg-asset-test".to_owned()),
                surface_epoch: 1,
                width: 80,
                height: 80,
                artifact_dir,
                artifact_label: "svg-asset-readback",
            })
            .expect("SVG asset frame should render to app-owned pixels");
            let RenderProofArtifact::AppOwnedPixels { artifact_path, .. } = proof.artifact else {
                panic!("expected app-owned pixel artifact");
            };
            let image = image::open(&artifact_path)
                .expect("readback PNG should decode")
                .to_rgba8();
            let center = image.get_pixel(40, 40).0;
            assert!(
                center[1] > center[0].saturating_add(48)
                    && center[1] > center[2].saturating_add(48),
                "SVG asset center should be green-dominant after texture rendering, got {center:?}"
            );
            assert!(
                proof.metrics.draw_calls >= 2,
                "asset rendering should add a textured batch draw call"
            );
        });
    }

    #[test]
    fn button_text_runs_are_centered_by_default() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(16.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("button".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 40.0,
                },
                text: Some("RUN".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 320, 120);
        let run = runs.first().expect("button text should render");
        assert_eq!(run.align, TextAlign::Center);
        assert_eq!(run.vertical_align, TextVerticalAlign::Center);

        let mut font_system = editor_font_system();
        let buffer = shape_text_run(&mut font_system, run);
        let line_width = shaped_line_width(&buffer).expect("button label should shape");
        let left = text_left_for_width(run, line_width);
        let top = text_top_for_height(run);
        let paint_left = text_paint_left_for_width(run, line_width);
        let paint_top = text_paint_top_for_height(run);
        assert!(
            left > run.bounds.x + run.text_inset,
            "centered button text should not use the left inset"
        );
        assert_eq!(
            paint_left.fract(),
            0.0,
            "button text should paint on a whole-pixel x origin"
        );
        let line_box_top = text_top_for_parts(
            run.bounds,
            run.line_height,
            run.text_inset,
            run.vertical_align,
        );
        assert!((line_box_top - 30.0).abs() <= 0.5);
        assert!(
            (top - 30.0).abs() <= 0.5,
            "centered glyph paint should use the line-box top without an optical offset, top={top}"
        );
        assert_eq!(
            paint_top, 30.0,
            "centered glyph paint origin should snap to whole pixels"
        );
    }

    #[test]
    fn quarter_turn_text_run_rasterizes_a_centered_rotated_mask() {
        let run = TextRun {
            node: DocumentNodeId("toggle-all".to_owned()),
            bounds: Rect {
                x: 75.0,
                y: 130.0,
                width: 45.0,
                height: 65.0,
            },
            clip: None,
            text: "❯".to_owned(),
            rich_spans: Vec::new(),
            font_family: "Helvetica Neue, Helvetica, Arial, SansSerif".to_owned(),
            font_style: Style::Normal,
            font_weight: Weight::NORMAL,
            font_features: String::new(),
            text_inset: 0.0,
            text_clip_padding: 0.0,
            color: [148, 148, 148, 255],
            size: 22.0,
            line_height: 27.5,
            align: TextAlign::Center,
            vertical_align: TextVerticalAlign::Center,
            rotate_degrees: 90,
        };
        let mut font_system = editor_font_system();
        let mut swash_cache = SwashCache::new();
        let glyph = rotated_text_glyph_for_run(&run, &mut font_system, &mut swash_cache)
            .expect("rotated chevron should rasterize through the generic custom glyph path");

        assert!(glyph.mask.iter().any(|alpha| *alpha > 0));
        assert!(
            glyph.width > glyph.height,
            "90-degree ❯ should become a wider down-chevron mask"
        );
        assert!(
            (glyph.left - (run.bounds.x + (run.bounds.width - f32::from(glyph.width)) * 0.5)).abs()
                <= 0.5
        );
        assert!(
            (glyph.top - (run.bounds.y + (run.bounds.height - f32::from(glyph.height)) * 0.5))
                .abs()
                <= 0.5
        );
    }

    #[test]
    fn negative_spread_outer_shadow_draws_only_the_visible_sheet() {
        let mut style = StyleMap::new();
        style.insert(
            "box_shadow_1_color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.973]".to_owned()),
        );
        style.insert("box_shadow_1_y".to_owned(), StyleValue::Number(8.0));
        style.insert("box_shadow_1_spread".to_owned(), StyleValue::Number(-3.0));
        let source = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
        };
        let mut builder = QuadBuilder::default();
        push_shadows(&mut builder, source, 160.0, 120.0, &style, 0.0);
        let (positions, _) = flatten_quad_batches(&builder.batches);

        assert_eq!(
            positions.len(),
            12,
            "the inset sheet should be a single bottom band, not a full source-sized slab"
        );
        let y_top = (1.0 - positions[1]) * 0.5 * 120.0;
        let y_bottom = (1.0 - positions[5]) * 0.5 * 120.0;
        let x_left = (positions[0] + 1.0) * 0.5 * 160.0;
        let x_right = (positions[2] + 1.0) * 0.5 * 160.0;
        assert!(y_top >= source.y + source.height);
        assert!(y_bottom <= source.y + source.height + 5.5);
        assert!((x_left - (source.x + 3.0)).abs() <= 0.5);
        assert!((x_right - (source.x + source.width - 3.0)).abs() <= 0.5);
    }

    #[test]
    fn css_shadow_list_order_paints_first_shadow_topmost() {
        let mut style = StyleMap::new();
        style.insert(
            "box_shadow_1_color".to_owned(),
            StyleValue::Text("#ff0000".to_owned()),
        );
        style.insert("box_shadow_1_y".to_owned(), StyleValue::Number(1.0));
        style.insert(
            "box_shadow_2_color".to_owned(),
            StyleValue::Text("#00ff00".to_owned()),
        );
        style.insert("box_shadow_2_y".to_owned(), StyleValue::Number(2.0));
        let mut builder = QuadBuilder::default();
        push_shadows(
            &mut builder,
            Rect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
            },
            160.0,
            120.0,
            &style,
            0.0,
        );
        let (_, colors) = flatten_quad_batches(&builder.batches);

        assert_eq!(
            colors.chunks_exact(4).next().unwrap(),
            &[0, 255, 0, 255],
            "later CSS shadows should be emitted first as the back layer"
        );
        assert_eq!(
            colors.chunks_exact(4).last().unwrap(),
            &[255, 0, 0, 255],
            "the first CSS shadow should be emitted last so it remains topmost"
        );
    }

    #[test]
    fn frosted_material_tokens_emit_frosted_layers_and_adjust_fill() {
        let mut style = StyleMap::new();
        style.insert("frosted_blur".to_owned(), StyleValue::Number(18.0));
        style.insert("frosted_saturate".to_owned(), StyleValue::Number(1.28));
        style.insert("glass_highlight".to_owned(), StyleValue::Number(0.8));
        style.insert(
            "glass_highlight_color".to_owned(),
            StyleValue::Text("#ffffffb8".to_owned()),
        );
        let rect = Rect {
            x: 24.0,
            y: 18.0,
            width: 96.0,
            height: 44.0,
        };
        let mut builder = QuadBuilder::default();
        let layer_count =
            push_frosted_material_layers(&mut builder, rect, 180.0, 120.0, &style, 12.0);

        assert!(
            layer_count >= 2,
            "frosted_blur should emit visible frosted material layers"
        );
        assert!(
            !builder.batches.is_empty(),
            "frosted material layers should be renderer quads, not metadata only"
        );
        let base = [0.8, 0.72, 0.64, 0.62];
        let adjusted = material_adjusted_fill(base, &style);
        assert!(
            adjusted[0] > base[0] && adjusted[3] < base[3],
            "frosted blur should frost/lift translucent glass fill: base={base:?}, adjusted={adjusted:?}"
        );
    }

    #[test]
    fn strikethrough_uses_the_same_vertical_center_as_text() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(24.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert("strikethrough".to_owned(), StyleValue::Bool(true));
        let item = DisplayItem {
            node: DocumentNodeId("completed-title".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 405.0,
                height: 42.0,
            },
            text: Some("Completed todo".to_owned()),
            style,
            focused: false,
        };

        let rect = strikethrough_rect_for_item(&item, None);
        let center_y = rect.y + rect.height * 0.5;
        let line_height = style_line_height(&item.style, 24.0);
        let text_center_y =
            text_top_for_parts(item.bounds, line_height, 0.0, TextVerticalAlign::Center)
                + line_height * 0.5;
        assert!(
            (center_y - text_center_y).abs() <= 0.01,
            "strikethrough center {center_y} should match text center {text_center_y}"
        );
    }

    #[test]
    fn checkbox_circle_snaps_to_pixel_center_for_smoother_edges() {
        let rect = Rect {
            x: 194.0,
            y: 263.6,
            width: 40.0,
            height: 40.0,
        };

        assert_eq!(checkbox_circle_center(rect), (214.5, 283.5));
        assert!(circle_segments_for_radius(17.5) >= 192);
        assert!(circle_segments_for_radius(1.1) <= 24);
    }

    #[test]
    fn checkbox_circle_coverage_feathers_edges() {
        let radius = 17.5;
        let aa = 1.25;

        assert_eq!(circle_coverage(radius, aa, radius - aa - 0.1), 1.0);
        assert_eq!(circle_coverage(radius, aa, radius + aa + 0.1), 0.0);
        let edge = circle_coverage(radius, aa, radius);
        assert!(
            edge > 0.45 && edge < 0.55,
            "edge coverage should be half-ish, got {edge}"
        );
    }

    #[test]
    fn checkbox_check_points_are_centered_inside_circle() {
        let rect = Rect {
            x: 194.0,
            y: 263.6,
            width: 40.0,
            height: 40.0,
        };
        let (start, middle, end) = checkbox_check_points(rect);
        let min_x = start.0.min(middle.0).min(end.0);
        let max_x = start.0.max(middle.0).max(end.0);
        let min_y = start.1.min(middle.1).min(end.1);
        let max_y = start.1.max(middle.1).max(end.1);
        let check_center = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
        let circle_center = checkbox_circle_center(rect);

        assert!(
            (check_center.0 - circle_center.0).abs() <= 0.5,
            "check x center {:?} should stay near circle center {:?}",
            check_center,
            circle_center
        );
        assert!(
            (check_center.1 - circle_center.1).abs() <= 0.5,
            "check y center {:?} should stay near circle center {:?}",
            check_center,
            circle_center
        );
    }

    #[test]
    fn checkbox_material_shadow_and_highlight_emit_pixels() {
        let mut flat = StyleMap::new();
        flat.insert("checked".to_owned(), StyleValue::Bool(true));
        flat.insert(
            "checkbox_background".to_owned(),
            StyleValue::Text("#ffffff".to_owned()),
        );
        flat.insert(
            "checkbox_border".to_owned(),
            StyleValue::Text("#d8dde4".to_owned()),
        );
        flat.insert(
            "checked_border".to_owned(),
            StyleValue::Text("#62c6aa".to_owned()),
        );
        flat.insert(
            "check_color".to_owned(),
            StyleValue::Text("#43bc9a".to_owned()),
        );

        let mut material = flat.clone();
        material.insert(
            "checkbox_cast_color".to_owned(),
            StyleValue::Text("#1e293b44".to_owned()),
        );
        material.insert("checkbox_cast_blur".to_owned(), StyleValue::Number(4.0));
        material.insert("checkbox_cast_y".to_owned(), StyleValue::Number(2.0));
        material.insert(
            "checkbox_highlight".to_owned(),
            StyleValue::Text("#ffffffb5".to_owned()),
        );
        material.insert(
            "checkbox_highlight_width".to_owned(),
            StyleValue::Number(1.0),
        );

        let rect = Rect {
            x: 20.0,
            y: 20.0,
            width: 40.0,
            height: 40.0,
        };
        let mut flat_builder = QuadBuilder::default();
        push_checkbox(&mut flat_builder, rect, 96.0, 96.0, &flat);
        let mut material_builder = QuadBuilder::default();
        push_checkbox(&mut material_builder, rect, 96.0, 96.0, &material);

        let flat_vertices = flat_builder
            .batches
            .iter()
            .map(|batch| batch.colors.len())
            .sum::<usize>();
        let material_vertices = material_builder
            .batches
            .iter()
            .map(|batch| batch.colors.len())
            .sum::<usize>();
        assert!(
            material_vertices > flat_vertices,
            "checkbox shadow/highlight material keys should add rendered pixels, not only style metadata"
        );
    }

    #[test]
    fn explicit_button_text_alignment_overrides_center_default() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(16.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
        style.insert("align".to_owned(), StyleValue::Text("left".to_owned()));
        style.insert(
            "vertical_align".to_owned(),
            StyleValue::Text("top".to_owned()),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("button".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 40.0,
                },
                text: Some("RUN".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 320, 120);
        let run = runs.first().expect("button text should render");
        assert_eq!(run.align, TextAlign::Left);
        assert_eq!(run.vertical_align, TextVerticalAlign::Top);
        assert_eq!(text_left_for_width(run, 30.0), 14.0);
        assert_eq!(text_top_for_height(run), 21.0);
        assert_eq!(text_paint_left_for_width(run, 30.0), 14.0);
        assert_eq!(text_paint_top_for_height(run), 21.0);
    }

    #[test]
    fn text_run_signatures_include_line_height() {
        let mut compact_style = StyleMap::new();
        compact_style.insert("size".to_owned(), StyleValue::Number(16.0));
        compact_style.insert("line_height".to_owned(), StyleValue::Number(18.0));
        let mut tall_style = compact_style.clone();
        tall_style.insert("line_height".to_owned(), StyleValue::Number(28.0));
        let frame = |style: StyleMap| LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("line-height-sensitive".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 160.0,
                    height: 40.0,
                },
                text: Some("Line height".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let compact_run = text_runs(&frame(compact_style), 320, 120)
            .pop()
            .expect("compact text should render");
        let tall_run = text_runs(&frame(tall_style), 320, 120)
            .pop()
            .expect("tall text should render");

        assert_ne!(
            TextRunSignature::from_run(&compact_run),
            TextRunSignature::from_run(&tall_run),
            "changing only line_height must invalidate shaped text buffers"
        );
        assert_ne!(
            TextRunPlacementSignature::from_run(&compact_run),
            TextRunPlacementSignature::from_run(&tall_run),
            "changing only line_height must invalidate placement caches"
        );
    }

    #[test]
    fn state_style_values_apply_hover_and_focus_variants() {
        let mut style = StyleMap::new();
        style.insert(
            "color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.20]".to_owned()),
        );
        style.insert(
            "__hover_color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.70]".to_owned()),
        );
        style.insert(
            "__focus_color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.50]".to_owned()),
        );
        style.insert("underline_if".to_owned(), StyleValue::Bool(false));
        style.insert("__hover_underline_if".to_owned(), StyleValue::Bool(true));
        style.insert("__hover".to_owned(), StyleValue::Bool(true));
        assert_eq!(style_bool(&style, "underline_if"), Some(true));
        let hover_color = style_color_u8(&style, "color").expect("hover color");

        style.insert("__hover".to_owned(), StyleValue::Bool(false));
        style.insert("__focused".to_owned(), StyleValue::Bool(true));
        assert_eq!(style_bool(&style, "underline_if"), Some(false));
        let focus_color = style_color_u8(&style, "color").expect("focus color");
        assert_ne!(hover_color, focus_color);

        style.insert("__focused".to_owned(), StyleValue::Bool(false));
        let base_color = style_color_u8(&style, "color").expect("base color");
        assert_ne!(hover_color, base_color);
        assert_ne!(focus_color, base_color);

        let frame_for_style = |style: StyleMap| LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("hover-button".to_owned()),
                kind: DocumentNodeKind::Button,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 32.0,
                },
                text: Some("Clear completed".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let mut base_style = StyleMap::new();
        base_style.insert(
            "color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.20]".to_owned()),
        );
        base_style.insert(
            "__hover_color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.70]".to_owned()),
        );
        base_style.insert("underline_if".to_owned(), StyleValue::Bool(false));
        base_style.insert("__hover_underline_if".to_owned(), StyleValue::Bool(true));
        let base_frame = frame_for_style(base_style.clone());
        let base_run = text_runs(&base_frame, 320, 120)
            .pop()
            .expect("base text should render");

        base_style.insert("__hover".to_owned(), StyleValue::Bool(true));
        let hover_frame = frame_for_style(base_style);
        let hover_run = text_runs(&hover_frame, 320, 120)
            .pop()
            .expect("hover text should render");
        assert_ne!(
            hover_run.color, base_run.color,
            "__hover_color must affect the rendered text run, not only the style lookup helper"
        );
        let base_layouts = test_text_layouts(&base_frame, 320, 120);
        let hover_layouts = test_text_layouts(&hover_frame, 320, 120);
        let (_, base_metrics) = rect_vertices(&base_frame, 320.0, 120.0, Some(&base_layouts));
        let (_, hover_metrics) = rect_vertices(&hover_frame, 320.0, 120.0, Some(&hover_layouts));
        assert!(
            hover_metrics.rendered_rect_count > base_metrics.rendered_rect_count,
            "__hover_underline_if must add a rendered underline rect"
        );
    }

    #[test]
    fn underline_sits_below_compact_footer_text() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(11.0));
        style.insert("line_height".to_owned(), StyleValue::Number(15.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert(
            "vertical_align".to_owned(),
            StyleValue::Text("Center".to_owned()),
        );
        let item = DisplayItem {
            node: DocumentNodeId("footer-link".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 40.0,
                width: 76.0,
                height: 15.0,
            },
            text: Some("Martin Kavík".to_owned()),
            style,
            focused: false,
        };

        let underline = underline_rect_for_item(&item, None);
        let line_height = style_line_height(&item.style, 11.0).min(item.bounds.height);
        let line_top = text_top_for_parts(item.bounds, line_height, 0.0, TextVerticalAlign::Center);
        let text_center_y = line_top + line_height * 0.5;
        assert!(
            underline.y > text_center_y,
            "underline y={} should stay below the compact footer text center {}",
            underline.y,
            text_center_y
        );
        assert!(
            underline.y + underline.height <= item.bounds.y + item.bounds.height,
            "underline should remain inside the footer line box"
        );
    }

    #[test]
    fn editor_type_hints_render_as_virtual_text_without_changing_source_run() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(14.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert(
            "font".to_owned(),
            StyleValue::Text("JetBrains Mono".to_owned()),
        );
        style.insert(
            "font_features".to_owned(),
            StyleValue::Text("zero,calt".to_owned()),
        );
        style.insert(
            "syntax_spans_json".to_owned(),
            StyleValue::Text(
                r##"[{"text":"count","source_text":"count","color":"#eeeeee"}]"##.to_owned(),
            ),
        );
        style.insert(
            "editor_type_hints_json".to_owned(),
            StyleValue::Text(
                r#"[{"anchor_column":6,"compact_label":"Number","line":1,"start":0,"end":5,"category":"definition","detail_label":"Number"}]"#
                    .to_owned(),
            ),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 260.0,
                    height: 22.0,
                },
                text: Some("count".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 320, 120);
        assert!(runs.iter().any(|run| run.text == "count"));
        assert!(runs.iter().any(|run| run.text == ": Number"));
        let source_run = runs
            .iter()
            .find(|run| run.text == "count")
            .expect("source run should remain source-exact");
        assert_eq!(source_run.node.0, "dev-code-editor-line-text-1");
    }

    #[test]
    fn editor_type_hints_do_not_render_sliced_runs_outside_source_bounds() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(14.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert(
            "font".to_owned(),
            StyleValue::Text("JetBrains Mono".to_owned()),
        );
        style.insert(
            "font_features".to_owned(),
            StyleValue::Text("zero,calt".to_owned()),
        );
        style.insert(
            "editor_type_hints_json".to_owned(),
            StyleValue::Text(
                r#"[{"anchor_column":27,"compact_label":"Number","line":1,"start":0,"end":26,"category":"definition","detail_label":"Number"}]"#
                    .to_owned(),
            ),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 42.0,
                    height: 22.0,
                },
                text: Some("abcdefghijklmnopqrstuvwxyz".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 320, 120);

        assert!(
            runs.iter()
                .any(|run| run.text == "abcdefghijklmnopqrstuvwxyz")
        );
        assert!(
            !runs.iter().any(|run| run.text == ": Number"),
            "off-row virtual type hints should be skipped instead of rendered as clipped slices"
        );
    }

    #[test]
    fn text_runs_shape_as_single_unwrapped_lines_when_bounds_are_narrow() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(14.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert(
            "font".to_owned(),
            StyleValue::Text("JetBrains Mono".to_owned()),
        );
        style.insert(
            "font_features".to_owned(),
            StyleValue::Text("zero,calt".to_owned()),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 22.0,
                },
                text: Some("active_count == 0 |> Bool/and(completed_count > 0)".to_owned()),
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 640, 160);
        let run = runs.first().expect("text run should render");
        let mut font_system = editor_font_system();
        let buffer = shape_text_run(&mut font_system, run);
        let line_count = buffer.layout_runs().count();
        let line_width = shaped_line_width(&buffer).expect("text should shape");

        assert_eq!(line_count, 1);
        assert!(
            line_width > run.bounds.width,
            "logical text width should exceed visible clip bounds instead of wrapping"
        );
    }

    #[test]
    fn text_inputs_center_text_and_place_caret_from_text_metrics() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(12.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
        style.insert("caret_column".to_owned(), StyleValue::Number(1.0));
        style.insert("caret_visible".to_owned(), StyleValue::Bool(true));
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("input".to_owned()),
                kind: DocumentNodeKind::TextInput,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 90.0,
                    height: 24.0,
                },
                text: Some("30".to_owned()),
                style,
                focused: true,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let runs = text_runs(&frame, 320, 120);
        let run = runs.first().expect("focused input text should render");
        assert_eq!(run.vertical_align, TextVerticalAlign::Center);
        let line_box_top = text_top_for_parts(
            run.bounds,
            run.line_height,
            run.text_inset,
            run.vertical_align,
        );
        assert!(
            (line_box_top - 24.5).abs() <= 0.5,
            "input line box top should stay geometrically centered"
        );
        assert!(
            (text_top_for_height(run) - 24.5).abs() <= 0.5,
            "input glyph paint should use the same line-box top as placeholder text"
        );
        assert_eq!(
            text_paint_top_for_height(run),
            25.0,
            "input glyph paint origin should snap to whole pixels for sharper text"
        );
        let text_layouts = test_text_layouts(&frame, 320, 120);
        let expected_caret_x = text_layouts
            .get(&DocumentNodeId("input".to_owned()))
            .unwrap()
            .x_for_column(1.0);
        let (batches, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let (positions, _) = flatten_quad_batches(&batches);
        assert!(
            positions.chunks_exact(12).any(|rect| {
                let x0 = ((rect[0] + 1.0) * 0.5) * 320.0;
                let x1 = ((rect[2] + 1.0) * 0.5) * 320.0;
                let rect_width = (x1 - x0).abs();
                (x0 - expected_caret_x).abs() <= 0.5 && (1.5..=2.5).contains(&rect_width)
            }),
            "input caret should use measured glyph edges"
        );
    }

    #[test]
    fn unfocused_empty_text_inputs_render_placeholder_text() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(24.0));
        style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        style.insert(
            "placeholder".to_owned(),
            StyleValue::Text("What needs to be done?".to_owned()),
        );
        style.insert(
            "placeholder_color".to_owned(),
            StyleValue::Text("Oklch[lightness:0.68]".to_owned()),
        );
        style.insert(
            "placeholder_style".to_owned(),
            StyleValue::Text("Italic".to_owned()),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("input".to_owned()),
                kind: DocumentNodeKind::TextInput,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 320.0,
                    height: 65.0,
                },
                text: None,
                style,
                focused: false,
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let run = text_runs(&frame, 640, 160)
            .into_iter()
            .find(|run| run.node.0 == "input")
            .expect("unfocused empty text input should still render placeholder text");
        assert_eq!(run.text, "What needs to be done?");
        assert_eq!(run.font_style, Style::Italic);
        assert_eq!(
            run.color,
            parse_oklch_color("Oklch[lightness:0.68]").expect("placeholder color should parse")
        );
        assert!(
            run.color[0] <= 190 && run.color[1] <= 190 && run.color[2] <= 190,
            "placeholder text should be readable gray, not washed-out near-white: {:?}",
            run.color
        );
    }

    fn max_shaped_word_gap(buffer: &Buffer) -> Option<f32> {
        let mut previous_right = None;
        let mut max_gap = 0.0_f32;
        for glyph in buffer
            .layout_runs()
            .next()?
            .glyphs
            .iter()
            .filter(|glyph| glyph.w > 0.0)
        {
            let left = glyph.x;
            if let Some(previous_right) = previous_right {
                max_gap = max_gap.max(left - previous_right);
            }
            previous_right = Some(left + glyph.w);
        }
        Some(max_gap)
    }

    #[test]
    fn wide_todo_text_controls_shape_with_natural_word_spacing() {
        for (
            font_family,
            title_weight,
            placeholder_weight,
            placeholder_style,
            title_size,
            placeholder_size,
            title_text,
        ) in [
            (
                "Helvetica Neue, Helvetica, Arial, SansSerif",
                Weight(300),
                Weight(300),
                "Italic",
                24.0,
                24.0,
                "Read documentation",
            ),
            (
                "Segoe UI, Roboto, Helvetica, Arial, SansSerif",
                Weight(300),
                Weight(300),
                "Italic",
                25.0,
                25.0,
                "Read documentation",
            ),
            (
                "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, monospace",
                Weight(800),
                Weight::NORMAL,
                "Normal",
                23.0,
                22.0,
                "Read documentation",
            ),
        ] {
            let mut input_style = StyleMap::new();
            input_style.insert("size".to_owned(), StyleValue::Number(title_size));
            input_style.insert("line_height".to_owned(), StyleValue::Number(33.6));
            input_style.insert("text_inset".to_owned(), StyleValue::Number(6.0));
            input_style.insert("font".to_owned(), StyleValue::Text(font_family.to_owned()));
            input_style.insert(
                "weight".to_owned(),
                StyleValue::Number(f64::from(title_weight.0)),
            );
            input_style.insert(
                "placeholder_size".to_owned(),
                StyleValue::Number(placeholder_size),
            );
            input_style.insert(
                "placeholder_weight".to_owned(),
                StyleValue::Number(f64::from(placeholder_weight.0)),
            );
            input_style.insert(
                "placeholder_font".to_owned(),
                StyleValue::Text(font_family.to_owned()),
            );
            input_style.insert(
                "placeholder".to_owned(),
                StyleValue::Text("What needs to be done?".to_owned()),
            );
            input_style.insert(
                "placeholder_style".to_owned(),
                StyleValue::Text(placeholder_style.to_owned()),
            );

            let mut title_style = StyleMap::new();
            title_style.insert("size".to_owned(), StyleValue::Number(title_size));
            title_style.insert("line_height".to_owned(), StyleValue::Number(33.6));
            title_style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
            title_style.insert("font".to_owned(), StyleValue::Text(font_family.to_owned()));
            title_style.insert(
                "weight".to_owned(),
                StyleValue::Number(f64::from(title_weight.0)),
            );

            let frame = LayoutFrame {
                display_list: vec![
                    DisplayItem {
                        node: DocumentNodeId("new-todo-input".to_owned()),
                        kind: DocumentNodeKind::TextInput,
                        bounds: Rect {
                            x: 55.0,
                            y: 151.0,
                            width: 495.0,
                            height: 65.0,
                        },
                        text: None,
                        style: input_style,
                        focused: false,
                    },
                    DisplayItem {
                        node: DocumentNodeId("active-title".to_owned()),
                        kind: DocumentNodeKind::Text,
                        bounds: Rect {
                            x: 109.0,
                            y: 217.0,
                            width: 441.0,
                            height: 65.0,
                        },
                        text: Some(title_text.to_owned()),
                        style: title_style,
                        focused: false,
                    },
                ],
                hit_regions: Vec::new(),
                scroll_regions: Vec::new(),
                accessibility: AccessibilityTree::default(),
                demands: Vec::new(),
                metrics: LayoutMetrics::default(),
            };

            let mut font_system = editor_font_system();
            for run in text_runs(&frame, 640, 360) {
                let buffer = shape_text_run(&mut font_system, &run);
                let line_width = shaped_line_width(&buffer).expect("todo text should shape");
                let max_gap = max_shaped_word_gap(&buffer).expect("todo glyphs should shape");
                assert!(
                    line_width < run.bounds.width * 0.75,
                    "`{}` in `{}` should keep natural word spacing instead of expanding to its whole control width: line_width={line_width}, bounds={:?}",
                    run.text,
                    font_family,
                    run.bounds
                );
                let run_size = if run.text == "What needs to be done?" {
                    placeholder_size
                } else {
                    title_size
                };
                assert!(
                    max_gap <= (run_size as f32) * 0.65,
                    "`{}` in `{}` should not shape with stretched spaces: max_gap={max_gap}, size={run_size}",
                    run.text,
                    font_family
                );
            }
        }
    }

    #[test]
    fn text_clip_padding_expands_text_bounds_on_all_edges() {
        let run = TextRun {
            node: DocumentNodeId("accented-footer-link".to_owned()),
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 10.0,
            },
            clip: None,
            text: "Kavík".to_owned(),
            rich_spans: Vec::new(),
            font_family: "Nimbus Sans".to_owned(),
            font_style: Style::Normal,
            font_weight: Weight::NORMAL,
            font_features: String::new(),
            text_inset: 0.0,
            text_clip_padding: 3.0,
            color: [90, 90, 90, 255],
            size: 11.0,
            line_height: 15.0,
            align: TextAlign::Left,
            vertical_align: TextVerticalAlign::Center,
            rotate_degrees: 0,
        };

        let bounds = text_bounds(&run, 100, 100);
        assert_eq!(bounds.left, 7);
        assert_eq!(bounds.top, 17);
        assert_eq!(bounds.right, 53);
        assert_eq!(bounds.bottom, 33);
    }

    #[test]
    fn text_input_placeholder_and_value_share_line_box_top() {
        let mut style = StyleMap::new();
        style.insert("size".to_owned(), StyleValue::Number(24.0));
        style.insert("line_height".to_owned(), StyleValue::Number(33.6));
        style.insert("text_inset".to_owned(), StyleValue::Number(6.0));
        style.insert(
            "placeholder".to_owned(),
            StyleValue::Text("What needs to be done?".to_owned()),
        );
        let frame_for_text = |text: Option<&str>| LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("new-todo-input".to_owned()),
                kind: DocumentNodeKind::TextInput,
                bounds: Rect {
                    x: 55.0,
                    y: 151.0,
                    width: 495.0,
                    height: 65.0,
                },
                text: text.map(str::to_owned),
                style: style.clone(),
                focused: text.is_some(),
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let placeholder_run = text_runs(&frame_for_text(None), 640, 240)
            .pop()
            .expect("placeholder should render");
        let value_run = text_runs(&frame_for_text(Some("abc")), 640, 240)
            .pop()
            .expect("value should render");

        assert_eq!(
            text_paint_top_for_height(&placeholder_run),
            text_paint_top_for_height(&value_run),
            "placeholder and real text should use one vertical line-box calculation"
        );
    }

    fn test_text_layouts(frame: &LayoutFrame, width: u32, height: u32) -> TextRunLayoutMap {
        let runs = text_runs(frame, width, height);
        let required_nodes = text_layout_metric_nodes(frame);
        let mut font_system = editor_font_system();
        runs.iter()
            .filter(|run| required_nodes.contains(&run.node))
            .map(|run| {
                let buffer = shape_text_run(&mut font_system, run);
                let line_width =
                    shaped_line_width(&buffer).unwrap_or_else(|| estimated_text_width(run));
                let left = text_left_for_width(run, line_width);
                let column_edges = shaped_column_edges(&run.text, &buffer, line_width);
                (
                    run.node.clone(),
                    TextRunLayoutMetrics { left, column_edges },
                )
            })
            .collect()
    }

    #[test]
    fn rich_text_spans_preserve_exact_line_text() {
        let mut style = StyleMap::new();
        style.insert(
            "syntax_spans_json".to_owned(),
            StyleValue::Text(
                r##"[{"text":"SOURCE","color":"#D2691E","font_weight":"800","font_style":"italic"},{"text":" ","color":"#d9e1f2","font_weight":null,"font_style":null},{"text":"]","color":"#D2691E","font_weight":"700","font_style":null}]"##
                    .to_owned(),
            ),
        );

        let spans = rich_text_spans(&style, "SOURCE ]", [217, 225, 242, 255]);
        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            vec!["SOURCE", " ", "]"]
        );
        assert!(rich_text_spans(&style, "SOURCE]", [217, 225, 242, 255]).is_empty());
    }

    #[test]
    fn rich_text_spans_preserve_pipe_forward_source_text() {
        let mut style = StyleMap::new();
        style.insert(
            "syntax_spans_json".to_owned(),
            StyleValue::Text(
                r##"[{"text":"|>","source_text":"|>","color":"#ff9f43","font_weight":"600","font_style":null}]"##
                    .to_owned(),
            ),
        );

        let spans = rich_text_spans(&style, "|>", [255, 159, 67, 255]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "|>");
        assert!(rich_text_spans(&style, "\u{276F} ", [255, 159, 67, 255]).is_empty());
    }
}

fn f32_slice_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(values));
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn u32_slice_bytes(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(values));
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn layout_frame_hash(frame: &LayoutFrame) -> String {
    let bytes = serde_json::to_vec(frame).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String, RenderError> {
    let bytes = std::fs::read(path).map_err(|error| RenderError {
        message: format!("read native GPU artifact `{}`: {error}", path.display()),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}
