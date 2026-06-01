use boon_document::{
    DisplayItem, DocumentNodeId, DocumentNodeKind, LayoutFrame, Rect, RenderCapabilities, StyleMap,
    StyleValue,
};
use boon_host::SurfaceId;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, LayoutGlyph, Metrics, Resolution, Shaping,
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
        let line_height = f32::from_bits(style_key.line_height_bits)
            .max(font_size)
            .max(text_line_height(font_size));
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

pub struct VisibleLayoutRenderer {
    pipeline: wgpu::RenderPipeline,
    frame_seq: u64,
    text: GlyphonTextState,
}

impl VisibleLayoutRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = generated::shader_bindings::ShaderEntry::NativeGpuRect;
        let module = shader.create_shader_module_embed_source(device);
        let layout = shader.create_pipeline_layout(device);
        let vertex_entry = generated::shader_bindings::native_gpu_rect::vs_main_entry(
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
    frame_seq: u64,
) -> Result<FrameMetrics, RenderError> {
    let width = request.width.clamp(1, 1920);
    let height = request.height.clamp(1, 1080);
    let visible_text_runs = text_runs(request.frame, width, height);
    let text_runs_shaped = visible_text_runs.len() as u32;
    let text_layout_nodes = text_layout_metric_nodes(request.frame);
    let text_layout_metrics = match text.as_mut() {
        Some(text) if !text_layout_nodes.is_empty() => {
            Some(text.layout_metrics_for_runs(&visible_text_runs, &text_layout_nodes))
        }
        None => None,
        Some(_) => None,
    };
    let (positions, colors, rect_metrics) = rect_vertices(
        request.frame,
        width as f32,
        height as f32,
        text_layout_metrics.as_ref(),
    );
    let vertex_count = (positions.len() / 2) as u32;
    let position_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-position-buffer"),
        size: (positions.len() * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let color_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-color-buffer"),
        size: colors.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    request
        .queue
        .write_buffer(&position_buffer, 0, &f32_slice_bytes(&positions));
    request.queue.write_buffer(&color_buffer, 0, &colors);
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
        pass.set_vertex_buffer(0, position_buffer.slice(..));
        pass.set_vertex_buffer(1, color_buffer.slice(..));
        pass.draw(0..vertex_count, 0..1);
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
        draw_calls: 1 + u32::from(rendered_text_runs > 0),
        upload_bytes: ((positions.len() * std::mem::size_of::<f32>()) + colors.len()) as u64,
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
    let artifact_path = request.artifact_dir.join(format!(
        "{}-{}-{}.png",
        std::process::id(),
        request.artifact_label,
        request.frame.display_list.len()
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
            layout_frame_hash: layout_frame_hash(request.frame),
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
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
    vertical_align: TextVerticalAlign,
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
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
    vertical_align: TextVerticalAlign,
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
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
            vertical_align: run.vertical_align,
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
            x: run.bounds.x.to_bits(),
            y: run.bounds.y.to_bits(),
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
            vertical_align: run.vertical_align,
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
        self.ensure_buffers(&runs);
        let placement_signatures = runs
            .iter()
            .map(TextRunPlacementSignature::from_run)
            .collect::<Vec<_>>();
        if self.prepared_signatures != placement_signatures
            || self.prepared_viewport != Some((width, height))
        {
            let mut areas = Vec::with_capacity(self.buffers.len());
            for (run, buffer) in runs.iter().zip(self.buffers.iter()) {
                let line_width =
                    shaped_line_width(buffer).unwrap_or_else(|| estimated_text_width(run));
                let left = text_left_for_width(run, line_width);
                let top = text_top_for_height(run);
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
            self.renderer
                .prepare(
                    device,
                    queue,
                    &mut self.font_system,
                    &mut self.atlas,
                    &self.viewport,
                    areas,
                    &mut self.swash_cache,
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
        Ok(self.buffers.len() as u32)
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
                let left = text_left_for_width(run, line_width);
                let column_edges = shaped_column_edges(&run.text, buffer, line_width);
                (
                    run.node.clone(),
                    TextRunLayoutMetrics { left, column_edges },
                )
            })
            .collect()
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
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, font_size * 1.25));
    buffer.set_size(font_system, None, Some(bounds.height.max(font_size * 1.25)));
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
        "SansSerif" | "sans-serif" => Family::SansSerif,
        "Serif" | "serif" => Family::Serif,
        "Monospace" | "monospace" => Family::Monospace,
        "Helvetica Neue" | "Helvetica" | "Arial" => Family::Name(DOCUMENT_FONT_FAMILY),
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
    align: TextAlign,
    vertical_align: TextVerticalAlign,
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
        let size = style_number(&item.style, "size").unwrap_or(14.0);
        let raw_text = item.text.as_deref().unwrap_or_default();
        if style_bool(&item.style, "paint") == Some(false)
            || (style_bool(&item.style, "hover_visible") == Some(true)
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
            let color = if style_bool(&item.style, "color_if") == Some(true) {
                style_color_u8(&item.style, "if_color")
                    .or_else(|| style_color_u8(&item.style, "color"))
                    .unwrap_or([36, 44, 58, 255])
            } else {
                style_color_u8(&item.style, "color").unwrap_or([36, 44, 58, 255])
            };
            (raw_text.to_owned(), color)
        };
        let font_family = if checked && !matches!(item.kind, DocumentNodeKind::Checkbox) {
            "DejaVu Sans"
        } else {
            style_text(&item.style, "font").unwrap_or(DOCUMENT_FONT_FAMILY)
        };
        let rich_spans = rich_text_spans(&item.style, &text, color);
        runs.push(TextRun {
            node: item.node.clone(),
            bounds: item.bounds,
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
            font_weight: text_font_weight(&item.style),
            font_features: style_text(&item.style, "font_features")
                .unwrap_or("")
                .to_owned(),
            text_inset: style_number(&item.style, "text_inset").unwrap_or(4.0),
            text_clip_padding: style_number(&item.style, "text_clip_padding").unwrap_or(0.0),
            color,
            size,
            align: text_align(&item.kind, &item.style),
            vertical_align: text_vertical_align(&item.kind, &item.style),
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
                align: TextAlign::Left,
                vertical_align: TextVerticalAlign::Center,
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

fn text_top_for_height(run: &TextRun) -> f32 {
    text_top_for_parts(run.bounds, run.size, run.text_inset, run.vertical_align)
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
    .max(font_size)
}

fn text_top_for_parts(
    bounds: Rect,
    font_size: f32,
    text_inset: f32,
    vertical_align: TextVerticalAlign,
) -> f32 {
    let line_height = text_line_height(font_size);
    match vertical_align {
        TextVerticalAlign::Top => bounds.y + 1.0,
        TextVerticalAlign::Center => bounds.y + ((bounds.height - line_height) / 2.0).max(0.0),
        TextVerticalAlign::Bottom => bounds.y + (bounds.height - line_height - text_inset).max(0.0),
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

fn text_layout_metric_nodes(frame: &LayoutFrame) -> BTreeSet<DocumentNodeId> {
    frame
        .display_list
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                DocumentNodeKind::Text | DocumentNodeKind::TextInput
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
    match style_text(style, "vertical_align").or_else(|| style_text(style, "align_y")) {
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
    match style.get(key)? {
        StyleValue::Number(value) => Some(*value as f32),
        StyleValue::Text(value) => value.parse::<f32>().ok(),
        StyleValue::Bool(_) => None,
    }
}

fn style_bool(style: &StyleMap, key: &str) -> Option<bool> {
    match style.get(key)? {
        StyleValue::Bool(value) => Some(*value),
        StyleValue::Text(value) => value.parse::<bool>().ok(),
        StyleValue::Number(_) => None,
    }
}

fn style_text<'a>(style: &'a StyleMap, key: &str) -> Option<&'a str> {
    match style.get(key)? {
        StyleValue::Text(value) => Some(value.as_str()),
        StyleValue::Number(_) | StyleValue::Bool(_) => None,
    }
}

fn text_bounds(run: &TextRun, width: u32, height: u32) -> TextBounds {
    let bounds = run.bounds;
    TextBounds {
        left: bounds.x.max(0.0) as i32,
        top: bounds.y.max(0.0) as i32,
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

fn rect_vertices(
    frame: &LayoutFrame,
    width: f32,
    height: f32,
    text_layouts: Option<&TextRunLayoutMap>,
) -> (Vec<f32>, Vec<u8>, RectVertexMetrics) {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut metrics = RectVertexMetrics {
        rendered_rect_count: 1,
        ..RectVertexMetrics::default()
    };
    push_rect(
        &mut positions,
        &mut colors,
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
        if style_bool(&item.style, "paint") == Some(false)
            || (style_bool(&item.style, "hover_visible") == Some(true)
                && style_bool(&item.style, "__hover_paint") != Some(true))
        {
            continue;
        }
        push_shadows(
            &mut positions,
            &mut colors,
            item.bounds,
            width,
            height,
            &item.style,
        );
        let fill = style_color_f32(&item.style, "bg")
            .or_else(|| style_color_f32(&item.style, "background"))
            .unwrap_or_else(|| default_fill_for_kind(&item.kind, index));
        push_rect(
            &mut positions,
            &mut colors,
            item.bounds,
            width,
            height,
            fill,
        );
        metrics.rendered_rect_count += 1;
        let selected_border = (style_bool(&item.style, "selected") == Some(true))
            .then(|| style_color_f32(&item.style, "selected_border"))
            .flatten();
        let focus_border = (item.focused || style_bool(&item.style, "focus") == Some(true))
            .then(|| style_color_f32(&item.style, "focus_border"))
            .flatten();
        if let Some(border) = selected_border.or_else(|| style_color_f32(&item.style, "border")) {
            push_border_all(
                &mut positions,
                &mut colors,
                item.bounds,
                width,
                height,
                if item.focused && focus_border.is_none() {
                    [0.098, 0.459, 0.824, 1.0]
                } else {
                    border
                },
                style_number(&item.style, "border_width").unwrap_or(2.0),
            );
            metrics.rendered_rect_count += 1;
        }
        push_side_borders(
            &mut positions,
            &mut colors,
            item.bounds,
            width,
            height,
            &item.style,
        );
        if let Some(border) = focus_border {
            push_border_all(
                &mut positions,
                &mut colors,
                item.bounds,
                width,
                height,
                border,
                style_number(&item.style, "focus_border_width").unwrap_or(2.0),
            );
            metrics.rendered_rect_count += 1;
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
                    &mut positions,
                    &mut colors,
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
                        &mut positions,
                        &mut colors,
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
                    &mut positions,
                    &mut colors,
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
        if style_bool(&item.style, "strike_if") == Some(true) {
            let color = style_color_f32(&item.style, "if_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.58, 0.58, 0.58, 1.0]);
            push_rect(
                &mut positions,
                &mut colors,
                Rect {
                    x: item.bounds.x + 4.0,
                    y: item.bounds.y + item.bounds.height * 0.58,
                    width: (item.bounds.width - 8.0).max(1.0),
                    height: 2.0,
                },
                width,
                height,
                color,
            );
            metrics.rendered_rect_count += 1;
        }
        if style_bool(&item.style, "underline_if") == Some(true) {
            let color = style_color_f32(&item.style, "underline_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.58, 0.58, 0.58, 1.0]);
            push_rect(
                &mut positions,
                &mut colors,
                Rect {
                    x: item.bounds.x + 4.0,
                    y: item.bounds.y + item.bounds.height - 5.0,
                    width: (item.bounds.width - 8.0).max(1.0),
                    height: 1.0,
                },
                width,
                height,
                color,
            );
            metrics.rendered_rect_count += 1;
        }
        if matches!(item.kind, DocumentNodeKind::Checkbox) {
            push_checkbox(
                &mut positions,
                &mut colors,
                item.bounds,
                width,
                height,
                &item.style,
            );
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
                &mut positions,
                &mut colors,
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
                &mut positions,
                &mut colors,
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
            && style_bool(&item.style, "caret_visible").unwrap_or(true)
        {
            let color = style_color_f32(&item.style, "caret_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.22, 0.22, 0.22, 1.0]);
            let font_size = style_number(&item.style, "size").unwrap_or(14.0);
            let text_inset = style_number(&item.style, "text_inset").unwrap_or(4.0);
            let vertical_align = text_vertical_align(&item.kind, &item.style);
            let line_top = text_top_for_parts(item.bounds, font_size, text_inset, vertical_align);
            let line_height = text_line_height(font_size).min(item.bounds.height.max(1.0));
            let caret_column = style_number(&item.style, "caret_column").unwrap_or(0.0);
            let caret_x = text_layouts
                .and_then(|layouts| layouts.get(&item.node))
                .map(|layout| layout.x_for_column(caret_column.max(0.0)))
                .unwrap_or(item.bounds.x + text_inset);
            push_rect(
                &mut positions,
                &mut colors,
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
    if positions.is_empty() {
        positions.extend_from_slice(&[
            -0.9, 0.9, -0.7, 0.9, -0.7, 0.7, -0.9, 0.9, -0.7, 0.7, -0.9, 0.7,
        ]);
        for _ in 0..6 {
            colors.extend_from_slice(&rgba8_from_f32([0.2, 0.6, 0.9, 1.0]));
        }
    }
    (positions, colors, metrics)
}

fn push_rect(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let x0 = (rect.x / width.max(1.0))
        .mul_add(2.0, -1.0)
        .clamp(-1.0, 1.0);
    let x1 = ((rect.x + rect.width) / width.max(1.0))
        .mul_add(2.0, -1.0)
        .clamp(-1.0, 1.0);
    let y0 = (1.0 - (rect.y / height.max(1.0)) * 2.0).clamp(-1.0, 1.0);
    let y1 = (1.0 - ((rect.y + rect.height) / height.max(1.0)) * 2.0).clamp(-1.0, 1.0);
    positions.extend_from_slice(&[x0, y0, x1, y0, x1, y1, x0, y0, x1, y1, x0, y1]);
    let color = rgba8_from_f32(color);
    for _ in 0..6 {
        colors.extend_from_slice(&color);
    }
}

fn push_shadows(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
) {
    for index in 1..=6 {
        let color_key = format!("shadow{index}_color");
        let Some(color) = style_color_f32(style, &color_key) else {
            continue;
        };
        let x = style_number(style, &format!("shadow{index}_x")).unwrap_or(0.0);
        let y = style_number(style, &format!("shadow{index}_y")).unwrap_or(0.0);
        let blur = style_number(style, &format!("shadow{index}_blur")).unwrap_or(0.0);
        let spread = style_number(style, &format!("shadow{index}_spread")).unwrap_or(0.0);
        let inset = style_bool(style, &format!("shadow{index}_inset")) == Some(true);
        if inset {
            let thickness = blur.max(1.0);
            push_rect(
                positions,
                colors,
                Rect {
                    x: rect.x,
                    y: rect.y + rect.height - thickness + y,
                    width: rect.width,
                    height: thickness,
                },
                width,
                height,
                color,
            );
        } else {
            if blur <= 0.0 {
                push_rect(
                    positions,
                    colors,
                    Rect {
                        x: rect.x + x - spread,
                        y: rect.y + y - spread,
                        width: (rect.width + spread * 2.0).max(1.0),
                        height: (rect.height + spread * 2.0).max(1.0),
                    },
                    width,
                    height,
                    color,
                );
                continue;
            }
            let base = Rect {
                x: rect.x + x - spread,
                y: rect.y + y - spread,
                width: (rect.width + spread * 2.0).max(1.0),
                height: (rect.height + spread * 2.0).max(1.0),
            };
            let steps = (blur / 4.0).ceil().clamp(2.0, 10.0) as u32;
            for step in 0..steps {
                let inner_expand = blur * step as f32 / steps as f32;
                let outer_expand = blur * (step + 1) as f32 / steps as f32;
                let t = (step + 1) as f32 / steps as f32;
                push_shadow_halo(
                    positions,
                    colors,
                    base,
                    inner_expand,
                    outer_expand,
                    width,
                    height,
                    color_with_alpha_scale(color, (1.0 - t).max(0.04) / steps as f32),
                );
            }
        }
    }
}

fn push_shadow_halo(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
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
            push_rect(positions, colors, band, width, height, color);
        }
    }
}

fn expanded_rect(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: (rect.width + amount * 2.0).max(1.0),
        height: (rect.height + amount * 2.0).max(1.0),
    }
}

fn push_circle(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    center_x: f32,
    center_y: f32,
    radius: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let color = rgba8_from_f32(color);
    let segments = 128;
    for index in 0..segments {
        let a0 = std::f32::consts::TAU * index as f32 / segments as f32;
        let a1 = std::f32::consts::TAU * (index + 1) as f32 / segments as f32;
        for (x, y) in [
            (center_x, center_y),
            (center_x + radius * a0.cos(), center_y + radius * a0.sin()),
            (center_x + radius * a1.cos(), center_y + radius * a1.sin()),
        ] {
            positions.push((x / width.max(1.0)).mul_add(2.0, -1.0).clamp(-1.0, 1.0));
            positions.push((1.0 - (y / height.max(1.0)) * 2.0).clamp(-1.0, 1.0));
            colors.extend_from_slice(&color);
        }
    }
}

fn push_annulus(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    push_annulus_gradient(
        positions,
        colors,
        center_x,
        center_y,
        outer_radius,
        inner_radius,
        width,
        height,
        color,
        color,
    );
}

fn push_annulus_gradient(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
    width: f32,
    height: f32,
    outer_color: [f32; 4],
    inner_color: [f32; 4],
) {
    if outer_radius <= 0.0 || inner_radius < 0.0 || inner_radius >= outer_radius {
        return;
    }
    let outer_color = rgba8_from_f32(outer_color);
    let inner_color = rgba8_from_f32(inner_color);
    let segments = 128;
    for index in 0..segments {
        let a0 = std::f32::consts::TAU * index as f32 / segments as f32;
        let a1 = std::f32::consts::TAU * (index + 1) as f32 / segments as f32;
        let outer0 = (
            center_x + outer_radius * a0.cos(),
            center_y + outer_radius * a0.sin(),
        );
        let outer1 = (
            center_x + outer_radius * a1.cos(),
            center_y + outer_radius * a1.sin(),
        );
        let inner0 = (
            center_x + inner_radius * a0.cos(),
            center_y + inner_radius * a0.sin(),
        );
        let inner1 = (
            center_x + inner_radius * a1.cos(),
            center_y + inner_radius * a1.sin(),
        );
        for ((x, y), color) in [
            (outer0, outer_color),
            (outer1, outer_color),
            (inner1, inner_color),
            (outer0, outer_color),
            (inner1, inner_color),
            (inner0, inner_color),
        ] {
            positions.push((x / width.max(1.0)).mul_add(2.0, -1.0).clamp(-1.0, 1.0));
            positions.push((1.0 - (y / height.max(1.0)) * 2.0).clamp(-1.0, 1.0));
            colors.extend_from_slice(&color);
        }
    }
}

fn push_stroked_segment(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    from: (f32, f32),
    to: (f32, f32),
    thickness: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        return;
    }
    let half = thickness.max(1.0) * 0.5;
    let px = -dy / length * half;
    let py = dx / length * half;
    let points = [
        (from.0 + px, from.1 + py),
        (to.0 + px, to.1 + py),
        (to.0 - px, to.1 - py),
        (from.0 - px, from.1 - py),
    ];
    let color_bytes = rgba8_from_f32(color);
    for (x, y) in [
        points[0], points[1], points[2], points[0], points[2], points[3],
    ] {
        positions.push((x / width.max(1.0)).mul_add(2.0, -1.0).clamp(-1.0, 1.0));
        positions.push((1.0 - (y / height.max(1.0)) * 2.0).clamp(-1.0, 1.0));
        colors.extend_from_slice(&color_bytes);
    }
    push_circle(
        positions, colors, from.0, from.1, half, width, height, color,
    );
    push_circle(positions, colors, to.0, to.1, half, width, height, color);
}

fn push_checkbox(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
) {
    let checked = style_bool(style, "checked") == Some(true);
    let center_x = rect.x + rect.width * 0.5;
    let center_y = rect.y + rect.height * 0.5;
    let radius = (rect.width.min(rect.height) * 0.5
        - style_number(style, "checkbox_inset").unwrap_or(2.0))
    .max(1.0);
    let border_width = style_number(style, "checkbox_border_width").unwrap_or(1.5);
    let ring_color = if checked {
        style_color_f32(style, "checked_border").unwrap_or([0.101, 0.356, 0.292, 1.0])
    } else {
        style_color_f32(style, "checkbox_border").unwrap_or([0.830, 0.830, 0.830, 1.0])
    };
    let inner_color = style_color_f32(style, "checkbox_background").unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let aa = style_number(style, "checkbox_aa")
        .unwrap_or(0.8)
        .clamp(0.0, 1.5);
    let inner_radius = (radius - border_width).max(0.0);
    let inner_solid_radius = (inner_radius - aa).max(0.0);
    push_annulus_gradient(
        positions,
        colors,
        center_x,
        center_y,
        radius + aa,
        radius,
        width,
        height,
        color_with_alpha_scale(ring_color, 0.0),
        ring_color,
    );
    push_annulus(
        positions,
        colors,
        center_x,
        center_y,
        radius,
        inner_radius,
        width,
        height,
        ring_color,
    );
    if inner_radius > inner_solid_radius {
        push_annulus_gradient(
            positions,
            colors,
            center_x,
            center_y,
            inner_radius,
            inner_solid_radius,
            width,
            height,
            color_with_alpha_scale(inner_color, 0.0),
            inner_color,
        );
    }
    push_circle(
        positions,
        colors,
        center_x,
        center_y,
        inner_solid_radius,
        width,
        height,
        inner_color,
    );
    if checked {
        let map_svg = |x: f32, y: f32| {
            (
                rect.x + ((x + 10.0) / 100.0) * rect.width,
                rect.y + ((y + 18.0) / 135.0) * rect.height,
            )
        };
        let start = map_svg(27.0, 56.0);
        let middle = map_svg(42.0, 71.0);
        let end = map_svg(72.0, 25.0);
        let color = style_color_f32(style, "check_color").unwrap_or([0.108, 0.540, 0.432, 1.0]);
        let thickness = style_number(style, "check_width").unwrap_or(3.0);
        let check_aa = style_number(style, "check_aa")
            .unwrap_or(0.8)
            .clamp(0.0, 1.5);
        if check_aa > 0.0 {
            let aa_color = color_with_alpha_scale(color, 0.28);
            push_stroked_segment(
                positions,
                colors,
                start,
                middle,
                thickness + check_aa * 2.0,
                width,
                height,
                aa_color,
            );
            push_stroked_segment(
                positions,
                colors,
                middle,
                end,
                thickness + check_aa * 2.0,
                width,
                height,
                aa_color,
            );
        }
        push_stroked_segment(
            positions, colors, start, middle, thickness, width, height, color,
        );
        push_stroked_segment(
            positions, colors, middle, end, thickness, width, height, color,
        );
    }
}

fn push_border_all(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
    thickness: f32,
) {
    let thickness = thickness.max(1.0);
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
        push_rect(positions, colors, edge, width, height, color);
    }
}

fn push_side_borders(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    style: &StyleMap,
) {
    for side in ["top", "right", "bottom", "left"] {
        let Some(color) = style_color_f32(style, &format!("border_{side}")) else {
            continue;
        };
        let thickness = style_number(style, &format!("border_{side}_width"))
            .or_else(|| style_number(style, "border_width"))
            .unwrap_or(1.0)
            .max(1.0);
        let edge = match side {
            "top" => Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: thickness,
            },
            "right" => Rect {
                x: rect.x + rect.width - thickness,
                y: rect.y,
                width: thickness,
                height: rect.height,
            },
            "bottom" => Rect {
                x: rect.x,
                y: rect.y + rect.height - thickness,
                width: rect.width,
                height: thickness,
            },
            "left" => Rect {
                x: rect.x,
                y: rect.y,
                width: thickness,
                height: rect.height,
            },
            _ => unreachable!(),
        };
        push_rect(positions, colors, edge, width, height, color);
    }
}

fn rgba8_from_f32(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
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
        DocumentNodeKind::Button => [0.92, 0.95, 0.97, 1.0],
        DocumentNodeKind::Checkbox => [1.0, 1.0, 1.0, 0.0],
        DocumentNodeKind::Table | DocumentNodeKind::TableCell => [1.0, 1.0, 1.0, 1.0],
        DocumentNodeKind::Text => [0.965, 0.972, 0.982, 1.0],
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
    let StyleValue::Text(value) = style.get(key)? else {
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
        let (_, _, metrics) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        assert!(
            metrics.rendered_rect_count >= 6,
            "background + item + selection + two brackets + caret should render"
        );

        let (positions, colors, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let selection_rect = 2usize;
        assert_eq!(
            &colors[selection_rect * 24..selection_rect * 24 + 4],
            &[12, 15, 21, 255],
            "selection highlight must stay opaque while bracket highlights are softened"
        );
        let first_bracket_rect = 3usize;
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
        assert!(
            left > run.bounds.x + run.text_inset,
            "centered button text should not use the left inset"
        );
        assert!((top - 30.0).abs() <= 0.5, "button text top={top}");
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
        assert!(
            (text_top_for_height(run) - 24.5).abs() <= 0.5,
            "input text top should be vertically centered"
        );
        let text_layouts = test_text_layouts(&frame, 320, 120);
        let expected_caret_x = text_layouts
            .get(&DocumentNodeId("input".to_owned()))
            .unwrap()
            .x_for_column(1.0);
        let (positions, _, _) = rect_vertices(&frame, 320.0, 120.0, Some(&text_layouts));
        let caret_rect = 2usize;
        let caret_x = ((positions[caret_rect * 12] + 1.0) * 0.5) * 320.0;
        assert!(
            (caret_x - expected_caret_x).abs() <= 0.5,
            "input caret should use measured glyph edges"
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
