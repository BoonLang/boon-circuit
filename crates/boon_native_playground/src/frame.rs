use std::time::{Duration, Instant};

use boon_document::{DocumentFrame, DocumentNodeId, RenderScene};
use boon_host::{HostEvent, HostEventEnvelope, PointerPhase};
use boon_native_app_window::{
    NativeHostError, NativeSurfaceHost, SurfaceAcquireError, SurfacePreferences,
};
use boon_native_gpu::{
    FrameMetrics, RenderAssetSource, SurfaceRenderSceneRequest, VisibleLayoutRenderer,
};

use crate::observer::{
    FrameEvidenceKey, FramePresented, InputKind, NATIVE_SESSION_ID_ENV, ObserverEvent,
    ObserverRole, RoleMetadata,
};
use crate::view::RetainedView;

pub struct AcceptedNativeEvent {
    pub envelope: HostEventEnvelope,
    pub accepted_at: Instant,
}

pub async fn drain_native_events(
    host: &mut NativeSurfaceHost,
    first: Result<HostEventEnvelope, NativeHostError>,
) -> Result<Vec<AcceptedNativeEvent>, NativeHostError> {
    let mut events = Vec::with_capacity(8);
    events.push(AcceptedNativeEvent {
        envelope: first?,
        accepted_at: Instant::now(),
    });
    events.extend(
        host.drain_events()
            .await?
            .into_iter()
            .map(|envelope| AcceptedNativeEvent {
                envelope,
                accepted_at: Instant::now(),
            }),
    );
    Ok(events)
}

pub fn pointer_button_pressed(event: &HostEvent) -> Option<bool> {
    match event {
        HostEvent::Pointer(pointer) if pointer.button.is_some() => match pointer.phase {
            PointerPhase::Down => Some(true),
            PointerPhase::Up => Some(false),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Default)]
pub struct NativeFrameTransaction {
    dirty: bool,
    accepted_input: Option<(HostEventEnvelope, Instant)>,
    event_dispatch_us: u64,
    executor_us: u64,
    runtime_document_us: u64,
    document_update_us: u64,
}

impl NativeFrameTransaction {
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn visible_change(&mut self, event: &AcceptedNativeEvent) {
        self.dirty = true;
        match &mut self.accepted_input {
            Some((envelope, _accepted_at)) => {
                *envelope = event.envelope.clone();
            }
            None => {
                self.accepted_input = Some((event.envelope.clone(), event.accepted_at));
            }
        }
    }

    pub fn record_work(
        &mut self,
        event_dispatch_us: u64,
        executor_us: u64,
        runtime_document_us: u64,
        document_update_us: u64,
    ) {
        self.event_dispatch_us = self.event_dispatch_us.saturating_add(event_dispatch_us);
        self.executor_us = self.executor_us.saturating_add(executor_us);
        self.runtime_document_us = self.runtime_document_us.saturating_add(runtime_document_us);
        self.document_update_us = self.document_update_us.saturating_add(document_update_us);
    }

    pub async fn present(
        self,
        product: &mut ProductFrame,
        host: &mut NativeSurfaceHost,
        view: &RetainedView,
    ) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.dirty {
            return Ok(None);
        }
        if let Some((envelope, accepted_at)) = self.accepted_input {
            product.accept_visible_input(
                &envelope,
                accepted_at,
                self.event_dispatch_us,
                self.executor_us,
                self.runtime_document_us,
                self.document_update_us,
            );
        }
        product.present(host, view).await
    }
}

pub fn role_message_frame(title: &str, message: &str, background: &str) -> DocumentFrame {
    let mut frame = crate::ui::message_frame(title, message, background);
    let parent = Some(DocumentNodeId("message.stack".to_owned()));
    for id in ["message.title", "message.body"] {
        if let Some(node) = frame.nodes.get_mut(&DocumentNodeId(id.to_owned())) {
            node.parent.clone_from(&parent);
        }
    }
    frame
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviewPerfStats {
    pub frame_id: u64,
    pub last_render_us: u64,
    pub last_submit_us: u64,
    pub last_present_us: u64,
    pub last_input_to_present_us: u64,
    pub p95_input_to_present_us: u64,
    pub missed_frame_count: u64,
    pub dropped_snapshot_count: u64,
    pub proof_enabled: bool,
}

#[derive(Clone, Debug)]
struct AcceptedInput {
    sequence: u64,
    callback_to_host_ns: u64,
    kind: InputKind,
    accepted_at: Instant,
    event_dispatch_us: u64,
    executor_us: u64,
    runtime_document_us: u64,
    document_update_us: u64,
}

#[derive(Clone, Debug)]
pub struct PresentedFrame {
    pub key: FrameEvidenceKey,
    pub event_sequence: Option<u64>,
    pub input_kind: Option<InputKind>,
    pub callback_to_host_ns: u64,
    pub input_to_present_us: u64,
    pub event_dispatch_us: u64,
    pub executor_us: u64,
    pub runtime_document_us: u64,
    pub document_update_us: u64,
    pub render_us: u64,
    pub document_scene_convert_us: u64,
    pub scene_key_us: u64,
    pub rect_vertices_us: u64,
    pub asset_prepare_us: u64,
    pub quad_batch_key_us: u64,
    pub quad_upload_us: u64,
    pub draw_pass_us: u64,
    pub retained_metrics_us: u64,
    pub text_render_us: u64,
    pub submit_us: u64,
    pub present_us: u64,
    pub frame_us: u64,
}

impl PresentedFrame {
    pub fn observer_event(&self, role: ObserverRole, observer_drop_count: u64) -> ObserverEvent {
        ObserverEvent::FramePresented(FramePresented {
            role,
            key: self.key.clone(),
            event_sequence: self.event_sequence,
            input_kind: self.input_kind,
            callback_to_host_ns: self.callback_to_host_ns,
            input_to_present_us: self.input_to_present_us,
            event_dispatch_us: self.event_dispatch_us,
            executor_us: self.executor_us,
            runtime_document_us: self.runtime_document_us,
            document_update_us: self.document_update_us,
            render_us: self.render_us,
            document_scene_convert_us: self.document_scene_convert_us,
            scene_key_us: self.scene_key_us,
            rect_vertices_us: self.rect_vertices_us,
            asset_prepare_us: self.asset_prepare_us,
            quad_batch_key_us: self.quad_batch_key_us,
            quad_upload_us: self.quad_upload_us,
            draw_pass_us: self.draw_pass_us,
            retained_metrics_us: self.retained_metrics_us,
            text_render_us: self.text_render_us,
            submit_us: self.submit_us,
            present_us: self.present_us,
            frame_us: self.frame_us,
            observer_drop_count,
        })
    }
}

pub struct ProductFrame {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: VisibleLayoutRenderer,
    format: wgpu::TextureFormat,
    metadata: RoleMetadata,
    frame_id: u64,
    accepted_input: Option<AcceptedInput>,
    interaction_samples: Vec<u64>,
    stats: PreviewPerfStats,
}

impl ProductFrame {
    pub async fn attach(
        host: &mut NativeSurfaceHost,
        role: ObserverRole,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let adapter = host
            .request_adapter(wgpu::PowerPreference::HighPerformance, false)
            .await?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-playground-product-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .map_err(|error| format!("request product WGPU device: {error}"))?;
        let binding = host
            .configure(&adapter, &device, SurfacePreferences::default())
            .await?;
        let mut renderer = VisibleLayoutRenderer::new(&device, &queue, binding.format);
        renderer.set_diagnostics_enabled(false);
        let viewport = binding.viewport;
        let adapter_name_lower = adapter_info.name.to_ascii_lowercase();
        let software_adapter = matches!(adapter_info.device_type, wgpu::DeviceType::Cpu)
            || ["llvmpipe", "softpipe", "software", "swiftshader"]
                .iter()
                .any(|needle| adapter_name_lower.contains(needle));
        let pid = std::process::id();
        let session_id = std::env::var(NATIVE_SESSION_ID_ENV)
            .ok()
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .unwrap_or_else(|| format!("process-{pid}"));
        let metadata = RoleMetadata {
            role,
            pid,
            surface_id: binding.surface_id.0.clone(),
            session_id,
            surface_epoch: binding.epoch,
            logical_width: viewport.logical_size.width,
            logical_height: viewport.logical_size.height,
            physical_width: viewport.physical_size.width,
            physical_height: viewport.physical_size.height,
            scale: viewport.scale,
            adapter_name: adapter_info.name,
            adapter_backend: backend_name(adapter_info.backend).to_owned(),
            adapter_device_type: device_type_name(adapter_info.device_type).to_owned(),
            software_adapter,
            surface_format: format_name(binding.format),
            present_mode: present_mode_name(binding.present_mode).to_owned(),
            window_backend: if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "wayland".to_owned()
            } else if cfg!(target_os = "windows") {
                "windows".to_owned()
            } else if cfg!(target_os = "macos") {
                "macos".to_owned()
            } else {
                "x11".to_owned()
            },
        };
        Ok(Self {
            device,
            queue,
            renderer,
            format: binding.format,
            metadata,
            frame_id: 0,
            accepted_input: None,
            interaction_samples: Vec::with_capacity(256),
            stats: PreviewPerfStats::default(),
        })
    }

    pub fn role_metadata(&self) -> RoleMetadata {
        self.metadata.clone()
    }

    pub fn accept_visible_input(
        &mut self,
        envelope: &HostEventEnvelope,
        accepted_at: Instant,
        event_dispatch_us: u64,
        executor_us: u64,
        runtime_document_us: u64,
        document_update_us: u64,
    ) {
        self.accepted_input = Some(AcceptedInput {
            sequence: envelope.sequence,
            callback_to_host_ns: envelope.callback_to_host_ns.get(),
            kind: input_kind(&envelope.event),
            accepted_at,
            event_dispatch_us,
            executor_us,
            runtime_document_us,
            document_update_us,
        });
    }

    pub fn stats(&self) -> PreviewPerfStats {
        self.stats
    }

    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }

    pub fn set_proof_enabled(&mut self, enabled: bool) {
        self.stats.proof_enabled = enabled;
    }

    pub fn replace_asset_sources(
        &mut self,
        sources: Vec<RenderAssetSource>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.renderer.replace_asset_sources(sources)?;
        Ok(())
    }

    pub async fn present(
        &mut self,
        host: &mut NativeSurfaceHost,
        view: &RetainedView,
    ) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        let revisions = view.revisions();
        self.present_scene(
            host,
            view.scene(),
            format!("scene-{}", revisions.2),
            revisions,
        )
        .await
    }

    pub async fn present_cursor(
        &mut self,
        host: &mut NativeSurfaceHost,
        view: &RetainedView,
        x: f32,
        y: f32,
    ) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        let revisions = view.revisions();
        let scene = view.scene_with_cursor(x, y);
        self.present_scene(
            host,
            &scene,
            format!("scene-{}-cursor-{x:.1}-{y:.1}", revisions.2),
            revisions,
        )
        .await
    }

    async fn present_scene(
        &mut self,
        host: &mut NativeSurfaceHost,
        scene: &RenderScene,
        scene_identity: String,
        revisions: (u64, u64, u64),
    ) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        let viewport = host.viewport();
        if viewport.is_zero_sized() {
            return Ok(None);
        }
        let frame_started = Instant::now();
        let frame = match host.acquire_frame().await {
            Ok(frame) => frame,
            Err(
                SurfaceAcquireError::Timeout
                | SurfaceAcquireError::Occluded
                | SurfaceAcquireError::Reconfigured { .. },
            ) => return Ok(None),
            Err(error) => return Err(Box::new(error)),
        };
        let surface_epoch = frame.epoch();
        let view_texture = frame
            .texture()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-playground-product-frame"),
            });

        let render_started = Instant::now();
        let metrics: FrameMetrics = self.renderer.encode_scene(SurfaceRenderSceneRequest {
            device: &self.device,
            queue: &self.queue,
            encoder: &mut encoder,
            view: &view_texture,
            scene,
            scene_identity: Some(&scene_identity),
            format: self.format,
            width: viewport.physical_size.width,
            height: viewport.physical_size.height,
        })?;
        let render_us = duration_us(render_started.elapsed());

        let submit_started = Instant::now();
        self.queue.submit([encoder.finish()]);
        let submit_us = duration_us(submit_started.elapsed());

        let present_started = Instant::now();
        let receipt = frame.present().await?;
        let present_us = duration_us(present_started.elapsed());
        debug_assert_eq!(receipt.epoch, surface_epoch);

        self.frame_id = self.frame_id.saturating_add(1);
        let frame_us = duration_us(frame_started.elapsed());
        let accepted = self.accepted_input.take();
        let input_to_present_us = accepted
            .as_ref()
            .map(|input| duration_us(input.accepted_at.elapsed()))
            .unwrap_or(0);
        let input_id = accepted
            .as_ref()
            .map(|input| input.sequence)
            .unwrap_or(self.frame_id);
        let key = FrameEvidenceKey {
            surface_id: self.metadata.surface_id.clone(),
            process_id: self.metadata.pid,
            session_id: self.metadata.session_id.clone(),
            frame_id: self.frame_id,
            input_id: input_id.max(1),
            content_id: revisions.0.max(1),
            layout_id: revisions.1.max(1),
            render_id: revisions.2.max(1),
            surface_epoch: receipt.epoch.max(1),
            present_id: self.frame_id,
            proof_id: self.frame_id,
        };

        self.stats.frame_id = self.frame_id;
        self.stats.last_render_us = render_us;
        self.stats.last_submit_us = submit_us;
        self.stats.last_present_us = present_us;
        if frame_us > 16_700 {
            self.stats.missed_frame_count = self.stats.missed_frame_count.saturating_add(1);
        }
        if input_to_present_us != 0 {
            self.stats.last_input_to_present_us = input_to_present_us;
            if self.interaction_samples.len() == 256 {
                self.interaction_samples.remove(0);
            }
            self.interaction_samples.push(input_to_present_us);
            self.stats.p95_input_to_present_us = nearest_rank(&self.interaction_samples, 95);
        }

        Ok(Some(PresentedFrame {
            key,
            event_sequence: accepted.as_ref().map(|input| input.sequence),
            input_kind: accepted.as_ref().map(|input| input.kind),
            callback_to_host_ns: accepted
                .as_ref()
                .map(|input| input.callback_to_host_ns)
                .unwrap_or(0),
            input_to_present_us,
            event_dispatch_us: accepted
                .as_ref()
                .map(|input| input.event_dispatch_us)
                .unwrap_or(0),
            executor_us: accepted
                .as_ref()
                .map(|input| input.executor_us)
                .unwrap_or(0),
            runtime_document_us: accepted
                .as_ref()
                .map(|input| input.runtime_document_us)
                .unwrap_or(0),
            document_update_us: accepted
                .as_ref()
                .map(|input| input.document_update_us)
                .unwrap_or(0),
            render_us,
            document_scene_convert_us: milliseconds_us(metrics.document_scene_convert_ms),
            scene_key_us: milliseconds_us(metrics.scene_key_ms),
            rect_vertices_us: milliseconds_us(metrics.rect_vertices_ms),
            asset_prepare_us: milliseconds_us(metrics.asset_prepare_ms),
            quad_batch_key_us: milliseconds_us(metrics.quad_batch_key_ms),
            quad_upload_us: milliseconds_us(metrics.quad_upload_ms),
            draw_pass_us: milliseconds_us(metrics.draw_pass_ms),
            retained_metrics_us: milliseconds_us(metrics.retained_metrics_ms),
            text_render_us: milliseconds_us(metrics.text_render_ms),
            submit_us,
            present_us,
            frame_us,
        }))
    }
}

fn milliseconds_us(milliseconds: f64) -> u64 {
    (milliseconds.max(0.0) * 1_000.0).round() as u64
}

pub fn input_kind(event: &HostEvent) -> InputKind {
    match event {
        HostEvent::Pointer(pointer) if pointer.phase == boon_host::PointerPhase::Move => {
            InputKind::PointerMove
        }
        HostEvent::Pointer(_) => InputKind::PointerButton,
        HostEvent::Wheel(_) => InputKind::Wheel,
        HostEvent::Keyboard(_) => InputKind::Keyboard,
        HostEvent::TextInput(_) => InputKind::Text,
        HostEvent::SensitiveInput(_) => InputKind::Sensitive,
        HostEvent::Ime(_) => InputKind::Ime,
        HostEvent::Focus { .. } => InputKind::Focus,
        HostEvent::Resize(_) => InputKind::Resize,
        HostEvent::Accessibility(_) => InputKind::Accessibility,
        HostEvent::CloseRequested { .. } => InputKind::Close,
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Gl => "gl",
        wgpu::Backend::BrowserWebGpu => "browser-webgpu",
        wgpu::Backend::Noop => "noop",
    }
}

fn device_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::IntegratedGpu => "integrated-gpu",
        wgpu::DeviceType::DiscreteGpu => "discrete-gpu",
        wgpu::DeviceType::VirtualGpu => "virtual-gpu",
        wgpu::DeviceType::Cpu => "cpu",
        wgpu::DeviceType::Other => "other",
    }
}

fn present_mode_name(mode: wgpu::PresentMode) -> &'static str {
    match mode {
        wgpu::PresentMode::Fifo => "fifo",
        wgpu::PresentMode::FifoRelaxed => "fifo-relaxed",
        wgpu::PresentMode::Immediate => "immediate",
        wgpu::PresentMode::Mailbox => "mailbox",
        wgpu::PresentMode::AutoVsync => "auto-vsync",
        wgpu::PresentMode::AutoNoVsync => "auto-no-vsync",
    }
}

fn format_name(format: wgpu::TextureFormat) -> String {
    format!("{format:?}").to_ascii_lowercase()
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn nearest_rank(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = percentile.saturating_mul(sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_keeps_outliers_in_the_sample_set() {
        let samples = [1, 2, 3, 4, 100];
        assert_eq!(nearest_rank(&samples, 50), 3);
        assert_eq!(nearest_rank(&samples, 95), 100);
    }

    #[test]
    fn every_host_event_has_a_stable_metric_class() {
        let event = HostEvent::Focus {
            surface: boon_host::SurfaceId("preview".to_owned()),
            focused: true,
        };
        assert_eq!(input_kind(&event), InputKind::Focus);
    }
}
