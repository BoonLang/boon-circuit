use super::{js_error, window};
use crate::{
    RetainedWebGpuRenderer, WebGpuAdapterMetadata, WebGpuFrameIdentity, WebGpuFrameResult,
    WebHostError, WebHostResult,
};
use boon_document::render_scene::RenderScene;
use boon_native_gpu::{
    DecodedMapTile, MapTileCacheError, MapTileCacheMetrics, MapTileEvent, MapTileFetchRequest,
    MapTileGpuPrepareMetrics, MapTileSubmission, RenderAssetSource,
};
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlCanvasElement, HtmlElement};

pub struct BrowserCanvasShell {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    unsupported: HtmlElement,
}

impl BrowserCanvasShell {
    /// Mounts only generic host chrome: one full-size canvas and a normally
    /// hidden unsupported-state message. Product layout never enters the DOM.
    pub fn mount(parent: &Element, canvas_id: &str) -> WebHostResult<Self> {
        if canvas_id.is_empty()
            || !canvas_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(WebHostError::InvalidInput {
                field: "canvas_id".to_owned(),
                reason: "must contain ASCII letters, digits, '-' or '_'".to_owned(),
            });
        }
        let document = window()?
            .document()
            .ok_or_else(|| WebHostError::unsupported("Document", "browser document is absent"))?;
        let root = document
            .create_element("div")
            .map_err(|error| js_error("create browser canvas root", error))?
            .dyn_into::<HtmlElement>()
            .map_err(|error| js_error("cast browser canvas root", error))?;
        root.set_attribute("data-boon-web-host", "true")
            .map_err(|error| js_error("mark browser canvas root", error))?;
        root.style().set_css_text(
            "position:relative;width:100%;height:100%;overflow:hidden;background:#f5f7f8;",
        );
        let canvas = document
            .create_element("canvas")
            .map_err(|error| js_error("create WebGPU canvas", error))?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|error| js_error("cast WebGPU canvas", error))?;
        canvas.set_id(canvas_id);
        canvas
            .set_attribute("aria-label", "Boon application")
            .map_err(|error| js_error("label WebGPU canvas", error))?;
        canvas
            .style()
            .set_css_text("display:block;width:100%;height:100%;outline:none;touch-action:none;");
        let unsupported = document
            .create_element("p")
            .map_err(|error| js_error("create unsupported message", error))?
            .dyn_into::<HtmlElement>()
            .map_err(|error| js_error("cast unsupported message", error))?;
        unsupported
            .set_attribute("role", "alert")
            .map_err(|error| js_error("configure unsupported message", error))?;
        unsupported.set_hidden(true);
        unsupported.style().set_css_text(
            "position:absolute;inset:0;display:grid;place-items:center;margin:0;padding:24px;background:#f5f7f8;color:#1b2228;font:16px system-ui,sans-serif;text-align:center;",
        );
        root.append_child(&canvas)
            .map_err(|error| js_error("mount WebGPU canvas", error))?;
        root.append_child(&unsupported)
            .map_err(|error| js_error("mount unsupported message", error))?;
        parent
            .append_child(&root)
            .map_err(|error| js_error("mount browser canvas root", error))?;
        Ok(Self {
            root,
            canvas,
            unsupported,
        })
    }

    pub fn root(&self) -> &HtmlElement {
        &self.root
    }

    pub fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    pub fn show_unsupported(&self, message: &str) {
        self.unsupported.set_text_content(Some(message));
        self.unsupported.set_hidden(false);
        self.canvas.set_hidden(true);
    }

    pub fn show_canvas(&self) {
        self.unsupported.set_hidden(true);
        self.canvas.set_hidden(false);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanvasFrameDisposition {
    Presented,
    PresentedSuboptimal,
    SkippedTimeout,
    SkippedOccluded,
    ReconfigureRequired,
    SurfaceLost,
    DeviceLost,
}

#[derive(Debug)]
pub struct CanvasFrameResult {
    pub disposition: CanvasFrameDisposition,
    pub frame: Option<WebGpuFrameResult>,
}

pub struct WebGpuCanvasHost {
    canvas: HtmlCanvasElement,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: RetainedWebGpuRenderer,
    surface_epoch: u64,
    lost_reason: Arc<Mutex<Option<String>>>,
}

impl WebGpuCanvasHost {
    /// Acquires browser WebGPU explicitly. No WebGL backend is enabled or used.
    pub async fn acquire(canvas: HtmlCanvasElement) -> WebHostResult<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| {
                WebHostError::platform("create WebGPU canvas surface", error.to_string())
            })?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|error| {
                WebHostError::unsupported(
                    "WebGPU adapter",
                    format!("no compatible browser adapter: {error}"),
                )
            })?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-webgpu-document-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| WebHostError::platform("request WebGPU device", error.to_string()))?;

        let lost_reason = Arc::new(Mutex::new(None));
        let lost_reason_for_callback = Arc::clone(&lost_reason);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut slot) = lost_reason_for_callback.lock() {
                *slot = Some(format!("{reason:?}: {message}"));
            }
        });

        let (width, height) = physical_canvas_size(&canvas)?;
        canvas.set_width(width);
        canvas.set_height(height);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| {
                WebHostError::unsupported(
                    "WebGPU surface",
                    "adapter exposes no presentation format for this canvas",
                )
            })?;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);
        let adapter_metadata = WebGpuAdapterMetadata::from_adapter(&adapter);
        let renderer =
            RetainedWebGpuRenderer::new(&device, &queue, config.format, adapter_metadata);
        Ok(Self {
            canvas,
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
            renderer,
            surface_epoch: 1,
            lost_reason,
        })
    }

    pub fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    pub fn adapter_metadata(&self) -> &WebGpuAdapterMetadata {
        self.renderer.adapter()
    }

    pub fn surface_epoch(&self) -> u64 {
        self.surface_epoch
    }

    pub fn set_diagnostics_enabled(&mut self, enabled: bool) {
        self.renderer.set_diagnostics_enabled(enabled);
    }

    pub fn replace_asset_sources(&mut self, sources: Vec<RenderAssetSource>) -> WebHostResult<()> {
        self.renderer.replace_asset_sources(sources)
    }

    pub fn resize_to_display_size(&mut self) -> WebHostResult<bool> {
        let (width, height) = physical_canvas_size(&self.canvas)?;
        if self.config.width == width && self.config.height == height {
            return Ok(false);
        }
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.surface_epoch = self.surface_epoch.saturating_add(1);
        Ok(true)
    }

    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
        self.surface_epoch = self.surface_epoch.saturating_add(1);
    }

    pub fn recover_lost_surface(&mut self) -> WebHostResult<()> {
        let surface = self
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(self.canvas.clone()))
            .map_err(|error| {
                WebHostError::platform("recreate WebGPU canvas surface", error.to_string())
            })?;
        let (width, height) = physical_canvas_size(&self.canvas)?;
        let mut config = surface
            .get_default_config(&self.adapter, width, height)
            .ok_or_else(|| {
                WebHostError::unsupported(
                    "WebGPU surface",
                    "adapter exposes no presentation format for the recovered canvas",
                )
            })?;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&self.device, &config);
        self.surface = surface;
        self.config = config;
        self.surface_epoch = self.surface_epoch.saturating_add(1);
        Ok(())
    }

    /// Reacquires a browser device while preserving retained CPU descriptors,
    /// decoded map tiles, and ordinary renderer asset sources. GPU resources
    /// are rebuilt before the next product frame.
    pub async fn recover_lost_device(&mut self) -> WebHostResult<()> {
        let adapter = self
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&self.surface),
            })
            .await
            .map_err(|error| {
                WebHostError::unsupported(
                    "WebGPU adapter recovery",
                    format!("no compatible browser adapter: {error}"),
                )
            })?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-webgpu-document-recovered-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| WebHostError::platform("recover WebGPU device", error.to_string()))?;
        let lost_reason = Arc::new(Mutex::new(None));
        let lost_reason_for_callback = Arc::clone(&lost_reason);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut slot) = lost_reason_for_callback.lock() {
                *slot = Some(format!("{reason:?}: {message}"));
            }
        });
        let (width, height) = physical_canvas_size(&self.canvas)?;
        let mut config = self
            .surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| {
                WebHostError::unsupported(
                    "WebGPU surface recovery",
                    "adapter exposes no presentation format for the recovered device",
                )
            })?;
        config.desired_maximum_frame_latency = 1;
        self.surface.configure(&device, &config);
        self.renderer.rebuild_device_resources(
            &device,
            &queue,
            config.format,
            WebGpuAdapterMetadata::from_adapter(&adapter),
        )?;
        self.adapter = adapter;
        self.device = device;
        self.queue = queue;
        self.config = config;
        self.lost_reason = lost_reason;
        self.surface_epoch = self.surface_epoch.saturating_add(1);
        self.prepare_map_tile_uploads()?;
        Ok(())
    }

    pub fn render(
        &mut self,
        scene: &RenderScene,
        identity: &WebGpuFrameIdentity,
    ) -> WebHostResult<CanvasFrameResult> {
        if self.device_lost_reason().is_some() {
            return Ok(CanvasFrameResult {
                disposition: CanvasFrameDisposition::DeviceLost,
                frame: None,
            });
        }
        self.prepare_map_tile_uploads()?;
        let (surface_texture, disposition) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => {
                (texture, CanvasFrameDisposition::Presented)
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                (texture, CanvasFrameDisposition::PresentedSuboptimal)
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(CanvasFrameResult {
                    disposition: CanvasFrameDisposition::SkippedTimeout,
                    frame: None,
                });
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(CanvasFrameResult {
                    disposition: CanvasFrameDisposition::SkippedOccluded,
                    frame: None,
                });
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                return Ok(CanvasFrameResult {
                    disposition: CanvasFrameDisposition::ReconfigureRequired,
                    frame: None,
                });
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Ok(CanvasFrameResult {
                    disposition: CanvasFrameDisposition::SurfaceLost,
                    frame: None,
                });
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(WebHostError::platform(
                    "acquire WebGPU canvas texture",
                    "surface validation failed",
                ));
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-webgpu-document-frame"),
            });
        let frame = self.renderer.encode(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            scene,
            identity,
            self.config.format,
            self.surface_epoch,
            self.config.width,
            self.config.height,
        )?;
        self.queue.submit([encoder.finish()]);
        surface_texture.present();
        Ok(CanvasFrameResult {
            disposition,
            frame: Some(frame),
        })
    }

    pub fn device_lost_reason(&self) -> Option<String> {
        self.lost_reason.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn request_presented_frame_readback(&self) -> WebHostResult<()> {
        self.renderer.request_presented_frame_readback()
    }

    pub fn take_map_tile_requests(&mut self, limit: usize) -> Vec<MapTileFetchRequest> {
        self.renderer.take_map_tile_requests(limit)
    }

    pub fn submit_map_tile(
        &mut self,
        tile: DecodedMapTile,
    ) -> Result<MapTileSubmission, MapTileCacheError> {
        self.renderer.submit_map_tile(tile)
    }

    pub fn map_tile_metrics(&self) -> MapTileCacheMetrics {
        self.renderer.map_tile_metrics()
    }

    pub fn drain_map_tile_events(&mut self) -> Vec<MapTileEvent> {
        self.renderer.drain_map_tile_events()
    }

    pub fn submit_map_tile_failure(
        &mut self,
        viewport: &boon_document::DocumentNodeId,
        identity: &boon_document::MapTileRequestIdentity,
        message: impl Into<String>,
        retryable: bool,
    ) -> MapTileSubmission {
        self.renderer
            .submit_map_tile_failure(viewport, identity, message, retryable)
    }

    pub fn retry_map_tile(
        &mut self,
        viewport: &boon_document::DocumentNodeId,
        tile: &boon_document::MapTileCacheKey,
    ) -> bool {
        self.renderer.retry_map_tile(viewport, tile)
    }

    pub fn prepare_map_tile_uploads(&mut self) -> WebHostResult<MapTileGpuPrepareMetrics> {
        self.renderer
            .prepare_map_tile_uploads(&self.device, &self.queue)
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }
}

fn physical_canvas_size(canvas: &HtmlCanvasElement) -> WebHostResult<(u32, u32)> {
    let window = window()?;
    let scale = window.device_pixel_ratio();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(WebHostError::InvalidInput {
            field: "devicePixelRatio".to_owned(),
            reason: "must be finite and positive".to_owned(),
        });
    }
    let logical_width = canvas.client_width().max(1) as f64;
    let logical_height = canvas.client_height().max(1) as f64;
    let width = (logical_width * scale).round().clamp(1.0, u32::MAX as f64) as u32;
    let height = (logical_height * scale).round().clamp(1.0, u32::MAX as f64) as u32;
    Ok((width, height))
}

pub fn request_animation_frame(callback: &js_sys::Function) -> WebHostResult<i32> {
    window()?
        .request_animation_frame(callback)
        .map_err(|error| js_error("requestAnimationFrame", error))
}
