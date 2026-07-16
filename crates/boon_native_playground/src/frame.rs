use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use boon_document::{DocumentFrame, DocumentNodeId, RenderScene};
use boon_host::{HostEvent, HostEventEnvelope, PointerPhase};
use boon_native_app_window::{
    NativeHostError, NativeSurfaceHost, SurfaceAcquireError, SurfacePreferences,
};
use boon_native_gpu::{
    FrameMetrics, MapTileEvent, RenderAssetSource, SurfaceRenderSceneRequest, VisibleLayoutRenderer,
};
use boon_web_host::{MapViewportHostController, MapViewportHostEvent};
use sha2::{Digest, Sha256};

use crate::map_host::NativeMapTileHost;
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

pub fn host_event_digest(envelope: &HostEventEnvelope) -> String {
    let canonical = format!(
        "sequence={};surface_epoch={};origin={:?};event={:?}",
        envelope.sequence, envelope.surface_epoch, envelope.origin, envelope.event
    );
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
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
    pub map_tile_dispatched: u64,
    pub map_tile_failed: u64,
    pub map_tile_cancelled: u64,
    pub map_tile_prewarm_bytes: u64,
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
    virtual_cursor: Option<(f32, f32)>,
    present_target: Option<ProductPresentTarget>,
    last_presented_key: Option<FrameEvidenceKey>,
    last_presented_captured: bool,
    map_interaction: MapViewportHostController,
    map_tiles: NativeMapTileHost,
    device_lost_reason: Arc<Mutex<Option<String>>>,
    rendered_map_interaction_revision: u64,
}

struct ProductPresentTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

pub struct PresentedReadbackTicket {
    pub key: FrameEvidenceKey,
    pub device: wgpu::Device,
    pub submission_index: wgpu::SubmissionIndex,
    pub buffer: wgpu::Buffer,
    pub width: u32,
    pub height: u32,
    pub unpadded_bytes_per_row: u32,
    pub padded_bytes_per_row: u32,
    pub format: wgpu::TextureFormat,
    pub artifact_label: String,
}

impl ProductFrame {
    pub async fn attach(
        host: &mut NativeSurfaceHost,
        role: ObserverRole,
        proof_enabled: bool,
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
        let device_lost_reason = install_device_lost_callback(&device);
        let binding = host
            .configure(&adapter, &device, SurfacePreferences::default())
            .await?;
        if proof_enabled && !binding.usage.contains(wgpu::TextureUsages::COPY_DST) {
            return Err("proof mode requires a surface with COPY_DST support".into());
        }
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
            stats: PreviewPerfStats {
                proof_enabled,
                ..PreviewPerfStats::default()
            },
            virtual_cursor: None,
            present_target: None,
            last_presented_key: None,
            last_presented_captured: false,
            map_interaction: MapViewportHostController::default(),
            map_tiles: NativeMapTileHost::from_env()?,
            device_lost_reason,
            rendered_map_interaction_revision: 0,
        })
    }

    pub fn role_metadata(&self) -> RoleMetadata {
        self.metadata.clone()
    }

    pub fn current_role_metadata(
        &self,
        host: &NativeSurfaceHost,
        surface_epoch: u64,
    ) -> RoleMetadata {
        let mut metadata = self.metadata.clone();
        let viewport = host.viewport();
        metadata.surface_epoch = surface_epoch;
        metadata.logical_width = viewport.logical_size.width;
        metadata.logical_height = viewport.logical_size.height;
        metadata.physical_width = viewport.physical_size.width;
        metadata.physical_height = viewport.physical_size.height;
        metadata.scale = viewport.scale;
        metadata
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
        let mut stats = self.stats;
        let map = self.map_tiles.metrics();
        stats.map_tile_dispatched = map.dispatched;
        stats.map_tile_failed = map.failed;
        stats.map_tile_cancelled = map.cancelled;
        stats.map_tile_prewarm_bytes = map.prewarm_bytes;
        stats
    }

    pub fn last_presented_key(&self) -> Option<&FrameEvidenceKey> {
        self.last_presented_key.as_ref()
    }

    pub fn set_virtual_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.virtual_cursor = cursor;
    }

    pub fn handle_map_input(
        &mut self,
        scene: &RenderScene,
        event: &HostEvent,
    ) -> Result<(bool, bool, Vec<MapViewportHostEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let consumed = self.map_interaction.consumes_host_event(scene, event);
        let visible_changed = self.map_interaction.handle_host_event(scene, event)?;
        let events = self.map_interaction.drain_events().collect();
        Ok((consumed, visible_changed, events))
    }

    pub async fn next_map_tile_wake(&mut self) -> Option<()> {
        self.map_tiles.next_wake().await
    }

    pub fn service_map_tiles(
        &mut self,
    ) -> Result<(bool, Vec<MapTileEvent>), Box<dyn std::error::Error + Send + Sync>> {
        let changed =
            self.map_tiles
                .service_before_frame(&mut self.renderer, &self.device, &self.queue)?;
        let events = self.map_tiles.drain_events().collect();
        Ok((changed, events))
    }

    pub fn replace_asset_sources(
        &mut self,
        sources: Vec<RenderAssetSource>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.renderer.replace_asset_sources(sources)?;
        Ok(())
    }

    pub fn capture_presented(
        &mut self,
        key: &FrameEvidenceKey,
        artifact_label: String,
    ) -> Result<PresentedReadbackTicket, Box<dyn std::error::Error + Send + Sync>> {
        if !self.stats.proof_enabled {
            return Err("production-target capture requires proof mode".into());
        }
        if self.last_presented_key.as_ref() != Some(key) {
            return Err("proof capture key is not the latest production frame".into());
        }
        if self.last_presented_captured {
            return Err("latest production frame was already captured".into());
        }
        let target = self
            .present_target
            .as_ref()
            .ok_or("production render target is unavailable")?;
        let unpadded_bytes_per_row = target
            .width
            .checked_mul(4)
            .ok_or("production readback row size overflow")?;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)?;
        let readback_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(target.height))
            .ok_or("production readback buffer size overflow")?;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("boon-playground-product-readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-playground-product-readback-encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(target.height),
                },
            },
            wgpu::Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
        let submission_index = self.queue.submit([encoder.finish()]);
        self.last_presented_captured = true;
        Ok(PresentedReadbackTicket {
            key: key.clone(),
            device: self.device.clone(),
            submission_index,
            buffer,
            width: target.width,
            height: target.height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            format: target.format,
            artifact_label,
        })
    }

    pub async fn present(
        &mut self,
        host: &mut NativeSurfaceHost,
        view: &RetainedView,
    ) -> Result<Option<PresentedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        let revisions = view.revisions();
        if let Some((x, y)) = self.virtual_cursor {
            let scene = view.scene_with_cursor(x, y);
            self.present_scene(
                host,
                &scene,
                format!("scene-{}-native-cursor-{x:.1}-{y:.1}", revisions.2),
                revisions,
            )
            .await
        } else {
            self.present_scene(
                host,
                view.scene(),
                format!("scene-{}", revisions.2),
                revisions,
            )
            .await
        }
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
        if self
            .device_lost_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
            .is_some()
        {
            self.recover_lost_device(host).await?;
        }
        let interaction_revision = self.map_interaction.revision();
        let interaction_dirty = interaction_revision != self.rendered_map_interaction_revision;
        let scene = self.map_interaction.scene_for_render(scene)?;
        if !interaction_dirty {
            self.map_tiles
                .service_before_frame(&mut self.renderer, &self.device, &self.queue)?;
        }
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
        let surface_view = frame
            .texture()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("boon-playground-product-frame"),
            });

        let render_started = Instant::now();
        let metrics: FrameMetrics = if self.stats.proof_enabled {
            self.ensure_present_target(viewport.physical_size.width, viewport.physical_size.height);
            let target = self
                .present_target
                .as_ref()
                .expect("ensured production render target");
            let metrics = self.renderer.encode_scene(SurfaceRenderSceneRequest {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                view: &target.view,
                scene: &scene,
                scene_identity: Some(&scene_identity),
                format: self.format,
                width: viewport.physical_size.width,
                height: viewport.physical_size.height,
            })?;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &target.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: frame.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: target.width,
                    height: target.height,
                    depth_or_array_layers: 1,
                },
            );
            metrics
        } else {
            self.renderer.encode_scene(SurfaceRenderSceneRequest {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                view: &surface_view,
                scene: &scene,
                scene_identity: Some(&scene_identity),
                format: self.format,
                width: viewport.physical_size.width,
                height: viewport.physical_size.height,
            })?
        };
        self.rendered_map_interaction_revision = interaction_revision;
        if interaction_dirty {
            if self
                .map_tiles
                .service_before_frame(&mut self.renderer, &self.device, &self.queue)?
            {
                self.map_tiles.wake_product_frame();
            }
        } else {
            self.map_tiles.service_after_frame(&mut self.renderer);
        }
        let render_us = duration_us(render_started.elapsed());

        let submit_started = Instant::now();
        self.queue.submit([encoder.finish()]);
        let submit_us = duration_us(submit_started.elapsed());

        let present_started = Instant::now();
        let receipt = frame.present().await?;
        let present_us = duration_us(present_started.elapsed());
        debug_assert_eq!(receipt.epoch, surface_epoch);

        self.frame_id = self
            .frame_id
            .checked_add(1)
            .ok_or("product frame ID overflow")?;
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
            present_id: receipt.present_id,
            proof_id: receipt.present_id,
        };
        self.last_presented_key = Some(key.clone());
        self.last_presented_captured = false;

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

    async fn recover_lost_device(
        &mut self,
        host: &mut NativeSurfaceHost,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let adapter = host
            .request_adapter(wgpu::PowerPreference::HighPerformance, false)
            .await?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-playground-product-recovered-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .map_err(|error| format!("recover product WGPU device: {error}"))?;
        let lost_reason = install_device_lost_callback(&device);
        let binding = host
            .configure(&adapter, &device, SurfacePreferences::default())
            .await?;
        if self.stats.proof_enabled && !binding.usage.contains(wgpu::TextureUsages::COPY_DST) {
            return Err("recovered proof surface lacks COPY_DST support".into());
        }
        self.renderer
            .rebuild_device_resources(&device, &queue, binding.format)?;
        self.device = device;
        self.queue = queue;
        self.format = binding.format;
        self.device_lost_reason = lost_reason;
        self.present_target = None;
        self.last_presented_key = None;
        self.last_presented_captured = false;
        self.metadata.surface_epoch = binding.epoch;
        self.metadata.surface_format = format_name(binding.format);
        self.metadata.present_mode = present_mode_name(binding.present_mode).to_owned();
        self.metadata.adapter_name = adapter_info.name.clone();
        self.metadata.adapter_backend = backend_name(adapter_info.backend).to_owned();
        self.metadata.adapter_device_type = device_type_name(adapter_info.device_type).to_owned();
        let adapter_name = adapter_info.name.to_ascii_lowercase();
        self.metadata.software_adapter = matches!(adapter_info.device_type, wgpu::DeviceType::Cpu)
            || ["llvmpipe", "softpipe", "software", "swiftshader"]
                .iter()
                .any(|needle| adapter_name.contains(needle));
        self.map_tiles
            .service_before_frame(&mut self.renderer, &self.device, &self.queue)?;
        Ok(())
    }

    fn ensure_present_target(&mut self, width: u32, height: u32) {
        let reusable = self.present_target.as_ref().is_some_and(|target| {
            target.width == width && target.height == height && target.format == self.format
        });
        if reusable {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("boon-playground-product-present-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.present_target = Some(ProductPresentTarget {
            texture,
            view,
            width,
            height,
            format: self.format,
        });
    }
}

fn install_device_lost_callback(device: &wgpu::Device) -> Arc<Mutex<Option<String>>> {
    let lost_reason = Arc::new(Mutex::new(None));
    let callback_reason = Arc::clone(&lost_reason);
    device.set_device_lost_callback(move |reason, message| {
        if let Ok(mut slot) = callback_reason.lock() {
            *slot = Some(format!("{reason:?}: {message}"));
        }
    });
    lost_reason
}

fn align_to(value: u32, alignment: u32) -> Result<u32, &'static str> {
    let remainder = value % alignment;
    if remainder == 0 {
        Ok(value)
    } else {
        value
            .checked_add(alignment - remainder)
            .ok_or("production readback row alignment overflow")
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
