use boon_document::{LayoutFrame, Rect, RenderCapabilities};
use boon_host::SurfaceId;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
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
    pub text_runs_shaped: u32,
    pub rendered_text_runs: u32,
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
                text_runs_shaped: 0,
                rendered_text_runs: 0,
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
    let (positions, colors) = rect_vertices(request.frame, width as f32, height as f32);
    let vertex_count = (positions.len() / 2) as u32;
    let position_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-position-buffer"),
        size: (positions.len() * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let color_buffer = request.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("boon-native-gpu-visible-color-buffer"),
        size: (colors.len() * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    request
        .queue
        .write_buffer(&position_buffer, 0, &f32_slice_bytes(&positions));
    request
        .queue
        .write_buffer(&color_buffer, 0, &f32_slice_bytes(&colors));
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
    let text_runs_shaped = text_runs(request.frame).len() as u32;
    let rendered_text_runs = match text.as_mut() {
        Some(text) => text.render(
            request.device,
            request.queue,
            request.encoder,
            request.view,
            request.frame,
            width,
            height,
        )?,
        None => 0,
    };
    Ok(FrameMetrics {
        frame_seq,
        draw_calls: 1 + u32::from(rendered_text_runs > 0),
        upload_bytes: ((positions.len() + colors.len()) * std::mem::size_of::<f32>()) as u64,
        text_runs_shaped,
        rendered_text_runs,
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
}

impl GlyphonTextState {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let font_system = FontSystem::new();
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
        }
    }

    fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        frame: &LayoutFrame,
        width: u32,
        height: u32,
    ) -> Result<u32, RenderError> {
        let runs = text_runs(frame);
        if runs.is_empty() {
            return Ok(0);
        }
        self.viewport.update(queue, Resolution { width, height });
        self.buffers.clear();
        self.buffers.reserve(runs.len());
        for (_, text) in &runs {
            let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(14.0, 18.0));
            buffer.set_size(&mut self.font_system, Some(4096.0), Some(64.0));
            buffer.set_text(
                &mut self.font_system,
                text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            self.buffers.push(buffer);
        }
        let mut areas = Vec::with_capacity(self.buffers.len());
        for ((bounds, _), buffer) in runs.iter().zip(self.buffers.iter()) {
            let left = bounds.x + 4.0;
            let top = bounds.y + 4.0;
            areas.push(TextArea {
                buffer,
                left,
                top,
                scale: 1.0,
                bounds: text_bounds(*bounds, width, height),
                default_color: Color::rgb(238, 244, 248),
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
        self.atlas.trim();
        Ok(self.buffers.len() as u32)
    }
}

fn text_runs(frame: &LayoutFrame) -> Vec<(Rect, String)> {
    frame
        .display_list
        .iter()
        .filter_map(|item| {
            let text = item.text.as_deref()?.trim();
            (!text.is_empty()).then(|| (item.bounds, text.to_owned()))
        })
        .take(256)
        .collect()
}

fn text_bounds(bounds: Rect, width: u32, height: u32) -> TextBounds {
    TextBounds {
        left: bounds.x.max(0.0) as i32,
        top: bounds.y.max(0.0) as i32,
        right: (bounds.x + bounds.width).clamp(0.0, width as f32) as i32,
        bottom: (bounds.y + bounds.height).clamp(0.0, height as f32) as i32,
    }
}

fn rect_vertices(frame: &LayoutFrame, width: f32, height: f32) -> (Vec<f32>, Vec<f32>) {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    for (index, item) in frame.display_list.iter().enumerate().take(512) {
        let x0 = (item.bounds.x / width.max(1.0))
            .mul_add(2.0, -1.0)
            .clamp(-1.0, 1.0);
        let x1 = ((item.bounds.x + item.bounds.width) / width.max(1.0))
            .mul_add(2.0, -1.0)
            .clamp(-1.0, 1.0);
        let y0 = (1.0 - (item.bounds.y / height.max(1.0)) * 2.0).clamp(-1.0, 1.0);
        let y1 =
            (1.0 - ((item.bounds.y + item.bounds.height) / height.max(1.0)) * 2.0).clamp(-1.0, 1.0);
        positions.extend_from_slice(&[x0, y0, x1, y0, x1, y1, x0, y0, x1, y1, x0, y1]);
        let color = color_for_index(index);
        for _ in 0..6 {
            colors.extend_from_slice(&color);
        }
    }
    if positions.is_empty() {
        positions.extend_from_slice(&[
            -0.9, 0.9, -0.7, 0.9, -0.7, 0.7, -0.9, 0.9, -0.7, 0.7, -0.9, 0.7,
        ]);
        for _ in 0..6 {
            colors.extend_from_slice(&[0.2, 0.6, 0.9, 1.0]);
        }
    }
    (positions, colors)
}

fn color_for_index(index: usize) -> [f32; 4] {
    let r = 0.18 + ((index * 37) % 80) as f32 / 140.0;
    let g = 0.28 + ((index * 53) % 70) as f32 / 150.0;
    let b = 0.38 + ((index * 29) % 60) as f32 / 160.0;
    [r.min(0.92), g.min(0.92), b.min(0.92), 1.0]
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
