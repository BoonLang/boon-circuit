use boon_document::{LayoutFrame, RenderCapabilities};
use boon_host::SurfaceId;
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Default)]
pub struct NativeGpuRenderer {
    frame_seq: u64,
}

impl NativeGpuRenderer {
    pub fn new_uninitialized() -> Self {
        Self { frame_seq: 0 }
    }

    pub fn required_backend_versions() -> (&'static str, &'static str) {
        (REQUIRED_WGPU_VERSION, REQUIRED_GLYPHON_VERSION)
    }

    pub fn default_frame_format_name() -> String {
        format!("{:?}", wgpu::TextureFormat::Rgba8Unorm)
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
                source_texture_hash: format!("layout-items-{}", frame.display_list.len()),
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
                preview_blocked_on_ipc_count: 0,
            },
        })
    }
}
