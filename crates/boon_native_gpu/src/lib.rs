use boon_document::{
    DocumentNodeKind, LayoutFrame, Rect, RenderCapabilities, StyleMap, StyleValue,
};
use boon_host::SurfaceId;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, Style,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc;

pub mod generated {
    pub mod shader_bindings;
}

pub const REQUIRED_WGPU_VERSION: &str = "29.0.1";
pub const REQUIRED_GLYPHON_VERSION: &str = "0.11.0";
const JETBRAINS_MONO_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Patched.woff2");
const MONOSPACE_TEXT_WIDTH_FACTOR: f32 = 0.60;

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
                blend: Some(wgpu::BlendState::REPLACE),
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
    let (positions, colors, rect_metrics) =
        rect_vertices(request.frame, width as f32, height as f32);
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
    let visible_text_runs = text_runs(request.frame, width, height);
    let text_runs_shaped = visible_text_runs.len() as u32;
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
struct TextRunSignature {
    text: String,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    text_inset: u32,
    text_clip_padding: u32,
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRunPlacementSignature {
    text: String,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    text_inset: u32,
    text_clip_padding: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    size: u32,
    color: [u8; 4],
    align: TextAlign,
}

impl TextRunSignature {
    fn from_run(run: &TextRun) -> Self {
        Self {
            text: run.text.clone(),
            font_family: run.font_family.clone(),
            font_style: run.font_style,
            font_weight: run.font_weight,
            text_inset: run.text_inset.to_bits(),
            text_clip_padding: run.text_clip_padding.to_bits(),
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
        }
    }
}

impl TextRunPlacementSignature {
    fn from_run(run: &TextRun) -> Self {
        Self {
            text: run.text.clone(),
            font_family: run.font_family.clone(),
            font_style: run.font_style,
            font_weight: run.font_weight,
            text_inset: run.text_inset.to_bits(),
            text_clip_padding: run.text_clip_padding.to_bits(),
            x: run.bounds.x.to_bits(),
            y: run.bounds.y.to_bits(),
            width: run.bounds.width.to_bits(),
            height: run.bounds.height.to_bits(),
            size: run.size.to_bits(),
            color: run.color,
            align: run.align,
        }
    }
}

impl GlyphonTextState {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        font_system
            .db_mut()
            .load_font_data(JETBRAINS_MONO_FONT_BYTES.to_vec());
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
        let signatures = runs
            .iter()
            .map(TextRunSignature::from_run)
            .collect::<Vec<_>>();
        let placement_signatures = runs
            .iter()
            .map(TextRunPlacementSignature::from_run)
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
                    let buffer = self.shape_text_run(run);
                    self.buffers.push(buffer);
                }
            }
            self.buffer_signatures = signatures;
        }
        if self.prepared_signatures != placement_signatures
            || self.prepared_viewport != Some((width, height))
        {
            let mut areas = Vec::with_capacity(self.buffers.len());
            for (run, buffer) in runs.iter().zip(self.buffers.iter()) {
                let left = text_left(run);
                let top = run.bounds.y + 1.0;
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

    fn shape_text_run(&mut self, run: &TextRun) -> Buffer {
        let bounds = run.bounds;
        let font_size = run.size.clamp(8.0, 120.0);
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size, font_size * 1.25),
        );
        buffer.set_size(
            &mut self.font_system,
            Some((bounds.width + run.text_clip_padding).max(1.0)),
            Some(bounds.height.max(font_size * 1.25)),
        );
        buffer.set_text(
            &mut self.font_system,
            &run.text,
            &Attrs::new()
                .family(Family::Name(&run.font_family))
                .style(run.font_style)
                .weight(run.font_weight),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }
}

struct TextRun {
    bounds: Rect,
    text: String,
    font_family: String,
    font_style: Style,
    font_weight: Weight,
    text_inset: f32,
    text_clip_padding: f32,
    color: [u8; 4],
    size: f32,
    align: TextAlign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextAlign {
    Left,
    Center,
    Right,
}

fn text_runs(frame: &LayoutFrame, width: u32, height: u32) -> Vec<TextRun> {
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
        .filter_map(|item| {
            let size = style_number(&item.style, "size").unwrap_or(14.0);
            let raw_text = item.text.as_deref().unwrap_or_default();
            let checked = style_bool(&item.style, "checked") == Some(true);
            let (text, color) = if raw_text.trim().is_empty() {
                if checked {
                    (
                        "✓".to_owned(),
                        style_color_u8(&item.style, "check_color").unwrap_or([92, 194, 175, 255]),
                    )
                } else if let Some(placeholder) =
                    style_text(&item.style, "placeholder").filter(|text| !text.trim().is_empty())
                {
                    (
                        placeholder.to_owned(),
                        style_color_u8(&item.style, "placeholder_color")
                            .unwrap_or([154, 154, 154, 255]),
                    )
                } else {
                    return None;
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
            let font_family = if checked {
                "DejaVu Sans"
            } else {
                style_text(&item.style, "font").unwrap_or("JetBrains Mono")
            };
            Some(TextRun {
                bounds: item.bounds,
                text,
                font_family: font_family.to_owned(),
                font_style: text_font_style(&item.style),
                font_weight: text_font_weight(&item.style),
                text_inset: style_number(&item.style, "text_inset").unwrap_or(4.0),
                text_clip_padding: style_number(&item.style, "text_clip_padding").unwrap_or(0.0),
                color,
                size,
                align: text_align(&item.style),
            })
        })
        .collect()
}

fn text_font_style(style: &StyleMap) -> Style {
    match style_text(style, "font_style")
        .or_else(|| style_text(style, "style"))
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("italic") | Some("cursive") => Style::Italic,
        Some("oblique") => Style::Oblique,
        _ => Style::Normal,
    }
}

fn text_font_weight(style: &StyleMap) -> Weight {
    match style_text(style, "weight").map(str::to_ascii_lowercase) {
        Some(value) if value == "bold" => Weight::BOLD,
        Some(value) if value == "bolder" => Weight::EXTRA_BOLD,
        Some(value) if value == "semibold" || value == "semi-bold" => Weight::SEMIBOLD,
        Some(value) if value == "medium" => Weight::MEDIUM,
        Some(value) if value == "normal" => Weight::NORMAL,
        Some(value) => value.parse::<u16>().map(Weight).unwrap_or(Weight::NORMAL),
        None => style_number(style, "weight")
            .map(|value| Weight(value.round().clamp(100.0, 900.0) as u16))
            .unwrap_or(Weight::NORMAL),
    }
}

fn text_left(run: &TextRun) -> f32 {
    let estimated_width = run.text.chars().count() as f32 * run.size * MONOSPACE_TEXT_WIDTH_FACTOR;
    match run.align {
        TextAlign::Left => run.bounds.x + run.text_inset,
        TextAlign::Center => {
            run.bounds.x + ((run.bounds.width - estimated_width) / 2.0).max(run.text_inset)
        }
        TextAlign::Right => {
            run.bounds.x + (run.bounds.width - estimated_width - run.text_inset).max(run.text_inset)
        }
    }
}

fn rect_intersects(rect: Rect, viewport: Rect) -> bool {
    rect.x < viewport.x + viewport.width
        && rect.x + rect.width > viewport.x
        && rect.y < viewport.y + viewport.height
        && rect.y + rect.height > viewport.y
}

fn text_align(style: &StyleMap) -> TextAlign {
    if style_bool(style, "center") == Some(true) {
        return TextAlign::Center;
    }
    match style_text(style, "align") {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
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
        if let Some(border) = selected_border.or_else(|| style_color_f32(&item.style, "border")) {
            push_border(
                &mut positions,
                &mut colors,
                item.bounds,
                width,
                height,
                if item.focused {
                    [0.098, 0.459, 0.824, 1.0]
                } else {
                    border
                },
            );
            metrics.rendered_rect_count += 1;
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
        {
            let color = style_color_f32(&item.style, "caret_color")
                .or_else(|| style_color_f32(&item.style, "color"))
                .unwrap_or([0.22, 0.22, 0.22, 1.0]);
            push_rect(
                &mut positions,
                &mut colors,
                Rect {
                    x: item.bounds.x + 12.0,
                    y: item.bounds.y + 11.0,
                    width: 2.0,
                    height: (item.bounds.height - 22.0).max(16.0),
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

fn push_border(
    positions: &mut Vec<f32>,
    colors: &mut Vec<u8>,
    rect: Rect,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let thickness = 2.0;
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

fn rgba8_from_f32(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
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
        DocumentNodeKind::Grid | DocumentNodeKind::GridCell => [1.0, 1.0, 1.0, 1.0],
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
