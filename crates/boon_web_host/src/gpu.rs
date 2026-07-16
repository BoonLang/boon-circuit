use crate::{WebHostError, WebHostResult};
use boon_document::render_scene::RenderScene;
use boon_native_gpu::{
    DecodedMapTile, FrameMetrics, MapTileCacheError, MapTileCacheMetrics, MapTileCpuSnapshot,
    MapTileEvent, MapTileFetchRequest, MapTileGpuPrepareMetrics, MapTileSubmission,
    RenderAssetSource, SurfaceRenderSceneRequest, VisibleLayoutRenderer,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebGpuAdapterClass {
    Hardware,
    Software,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebGpuAdapterMetadata {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub vendor: u32,
    pub device: u32,
    pub class: WebGpuAdapterClass,
    pub is_software: bool,
}

impl WebGpuAdapterMetadata {
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let class = match info.device_type {
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu => {
                WebGpuAdapterClass::Hardware
            }
            wgpu::DeviceType::Cpu => WebGpuAdapterClass::Software,
            wgpu::DeviceType::Other | wgpu::DeviceType::VirtualGpu => WebGpuAdapterClass::Unknown,
        };
        Self {
            name: info.name,
            backend: format!("{:?}", info.backend).to_ascii_lowercase(),
            device_type: format!("{:?}", info.device_type).to_ascii_lowercase(),
            vendor: info.vendor,
            device: info.device,
            is_software: class == WebGpuAdapterClass::Software,
            class,
        }
    }

    pub fn hardware_backed(&self) -> bool {
        self.class == WebGpuAdapterClass::Hardware
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebGpuFrameIdentity {
    pub surface_id: boon_host::SurfaceId,
    pub content_revision: u64,
    pub layout_revision: u64,
    pub render_scene_revision: u64,
    pub scene_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_event_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_request_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebGpuFrameEvidence {
    pub frame_seq: u64,
    pub present_id: u64,
    pub surface_id: boon_host::SurfaceId,
    pub scene_identity: String,
    pub content_revision: u64,
    pub layout_revision: u64,
    pub render_scene_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_event_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_request_id: Option<u64>,
    pub surface_epoch: u64,
    pub width: u32,
    pub height: u32,
    pub adapter: WebGpuAdapterMetadata,
    pub draw_calls: u32,
    pub upload_bytes: u64,
    pub dirty_upload_chunks: u32,
    pub visible_items: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebGpuFrameResult {
    pub metrics: FrameMetrics,
    pub evidence: WebGpuFrameEvidence,
}

/// Target-neutral retained renderer used by both native WGPU and browser
/// WebGPU. This wrapper does not own a browser surface; the Wasm adapter does.
pub struct RetainedWebGpuRenderer {
    renderer: VisibleLayoutRenderer,
    adapter: WebGpuAdapterMetadata,
    frame_seq: u64,
}

impl RetainedWebGpuRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        adapter: WebGpuAdapterMetadata,
    ) -> Self {
        let mut renderer = VisibleLayoutRenderer::new(device, queue, format);
        renderer.set_diagnostics_enabled(false);
        Self {
            renderer,
            adapter,
            frame_seq: 0,
        }
    }

    pub fn adapter(&self) -> &WebGpuAdapterMetadata {
        &self.adapter
    }

    pub fn set_diagnostics_enabled(&mut self, enabled: bool) {
        self.renderer.set_diagnostics_enabled(enabled);
    }

    pub fn replace_asset_sources(&mut self, sources: Vec<RenderAssetSource>) -> WebHostResult<()> {
        self.renderer
            .replace_asset_sources(sources)
            .map_err(|error| WebHostError::platform("replace WebGPU assets", error.message))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        scene: &RenderScene,
        identity: &WebGpuFrameIdentity,
        format: wgpu::TextureFormat,
        surface_epoch: u64,
        width: u32,
        height: u32,
    ) -> WebHostResult<WebGpuFrameResult> {
        if identity.scene_identity.is_empty() || identity.surface_id.0.is_empty() {
            return Err(WebHostError::InvalidInput {
                field: "WebGPU frame identity".to_owned(),
                reason: "retained browser frames require stable scene and surface identities"
                    .to_owned(),
            });
        }
        if width == 0 || height == 0 {
            return Err(WebHostError::InvalidInput {
                field: "WebGPU surface dimensions".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        let metrics = self
            .renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device,
                queue,
                encoder,
                view,
                scene,
                scene_identity: Some(&identity.scene_identity),
                format,
                width,
                height,
            })
            .map_err(|error| {
                WebHostError::platform("encode retained WebGPU scene", error.message)
            })?;
        self.frame_seq = self.frame_seq.saturating_add(1);
        let evidence = WebGpuFrameEvidence {
            frame_seq: self.frame_seq,
            present_id: self.frame_seq,
            surface_id: identity.surface_id.clone(),
            scene_identity: identity.scene_identity.clone(),
            content_revision: identity.content_revision,
            layout_revision: identity.layout_revision,
            render_scene_revision: identity.render_scene_revision,
            input_event_seq: identity.input_event_seq,
            proof_request_id: identity.proof_request_id,
            surface_epoch,
            width,
            height,
            adapter: self.adapter.clone(),
            draw_calls: metrics.draw_calls,
            upload_bytes: metrics.upload_bytes,
            dirty_upload_chunks: metrics.dirty_upload_chunk_count,
            visible_items: metrics.visible_display_item_count,
        };
        Ok(WebGpuFrameResult { metrics, evidence })
    }

    pub fn request_presented_frame_readback(&self) -> WebHostResult<()> {
        Err(WebHostError::unsupported(
            "browser app-owned readback",
            "the shared presented-frame proof transaction is not yet browser-safe",
        ))
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

    pub fn prepare_map_tile_uploads(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> WebHostResult<MapTileGpuPrepareMetrics> {
        self.renderer
            .prepare_map_tile_uploads(device, queue)
            .map_err(|error| WebHostError::platform("prewarm WebGPU map tiles", error.message))
    }

    pub fn map_tile_cpu_snapshot(&self) -> MapTileCpuSnapshot {
        self.renderer.map_tile_cpu_snapshot()
    }

    pub fn restore_map_tile_cpu_snapshot(
        &mut self,
        snapshot: MapTileCpuSnapshot,
    ) -> Result<(), MapTileCacheError> {
        self.renderer.restore_map_tile_cpu_snapshot(snapshot)
    }

    pub fn rebuild_device_resources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        adapter: WebGpuAdapterMetadata,
    ) -> WebHostResult<()> {
        self.renderer
            .rebuild_device_resources(device, queue, format)
            .map_err(|error| {
                WebHostError::platform("rebuild retained WebGPU resources", error.message)
            })?;
        self.adapter = adapter;
        Ok(())
    }
}
