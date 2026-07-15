use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, ThreadId};

use app_window::coordinates::{Position, Size};
use app_window::event::{WindowEventEnvelope, WindowEventReceiver, WindowEventTimestamp};
use app_window::surface::Surface as AppWindowSurface;
use app_window::window::Window;
use app_window::{WGPU_STRATEGY, WGPU_SURFACE_STRATEGY, WGPUStrategy};
use boon_host::{
    CallbackToHostNs, HostEvent, HostEventEnvelope, LogicalSize, PhysicalSize, RoleId,
    SensitiveInputHandle, SurfaceId, WindowId,
};
use wgpu::{CurrentSurfaceTexture, SurfaceTargetUnsafe};

use crate::error::{
    NativeHostError, SurfaceAcquireError, SurfacePresentError, SurfaceReconfigureReason,
};
use crate::event::{
    AdaptedWindowEvent, EventAdapter, NativeEventCapabilities, map_event_error, viewport,
};
use crate::sensitive_input::SensitiveInputTarget;

static HOST_OPEN: AtomicBool = AtomicBool::new(false);

struct HostClaim;

impl HostClaim {
    fn acquire() -> Result<Self, NativeHostError> {
        HOST_OPEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| NativeHostError::HostAlreadyOpen)?;
        Ok(Self)
    }
}

impl Drop for HostClaim {
    fn drop(&mut self) {
        HOST_OPEN.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHostIds {
    pub role: RoleId,
    pub window: WindowId,
    pub surface: SurfaceId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeWindowConfig {
    pub ids: NativeHostIds,
    pub title: String,
    pub position: WindowPosition,
    pub initial_logical_size: LogicalSize,
}

impl NativeWindowConfig {
    fn validate(&self) -> Result<(), NativeHostError> {
        if self.ids.role.0.is_empty() {
            return Err(NativeHostError::InvalidConfig("role ID is empty"));
        }
        if self.ids.window.0.is_empty() {
            return Err(NativeHostError::InvalidConfig("window ID is empty"));
        }
        if self.ids.surface.0.is_empty() {
            return Err(NativeHostError::InvalidConfig("surface ID is empty"));
        }
        if self.title.is_empty() {
            return Err(NativeHostError::InvalidConfig("window title is empty"));
        }
        for (name, value) in [
            ("window x", self.position.x),
            ("window y", self.position.y),
            ("initial width", f64::from(self.initial_logical_size.width)),
            (
                "initial height",
                f64::from(self.initial_logical_size.height),
            ),
        ] {
            if !value.is_finite() || (name.starts_with("initial") && value < 0.0) {
                return Err(NativeHostError::InvalidNumber { field: name, value });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeViewport {
    pub logical_size: LogicalSize,
    pub scale: f64,
    pub physical_size: PhysicalSize,
}

impl NativeViewport {
    pub fn is_zero_sized(self) -> bool {
        self.physical_size.width == 0 || self.physical_size.height == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSurfaceLifecycle {
    Unconfigured,
    Configured,
    Suspended,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeThreadStrategy {
    MainThread,
    NotMainThread,
    Relaxed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeThreadContract {
    pub wgpu: NativeThreadStrategy,
    pub surface: NativeThreadStrategy,
}

pub fn native_thread_contract() -> NativeThreadContract {
    NativeThreadContract {
        wgpu: public_strategy(WGPU_STRATEGY),
        surface: public_strategy(WGPU_SURFACE_STRATEGY),
    }
}

#[derive(Clone, Debug)]
pub struct SurfacePreferences {
    pub preferred_format: Option<wgpu::TextureFormat>,
    pub preferred_present_mode: Option<wgpu::PresentMode>,
    pub desired_maximum_frame_latency: u32,
    pub allow_copy_dst: bool,
}

impl Default for SurfacePreferences {
    fn default() -> Self {
        Self {
            preferred_format: None,
            preferred_present_mode: Some(wgpu::PresentMode::AutoNoVsync),
            desired_maximum_frame_latency: 2,
            allow_copy_dst: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeSurfaceBinding {
    pub surface_id: SurfaceId,
    pub epoch: u64,
    pub configuration_generation: u64,
    pub viewport: NativeViewport,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
    pub present_mode: wgpu::PresentMode,
    pub alpha_mode: wgpu::CompositeAlphaMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfacePresentReceipt {
    pub surface_id: SurfaceId,
    pub epoch: u64,
    pub configuration_generation: u64,
    pub present_id: u64,
    pub recovered_suboptimal_surface: bool,
}

pub struct NativeSurfaceHost {
    ids: NativeHostIds,
    viewport: NativeViewport,
    epoch: u64,
    configuration_generation: u64,
    present_id: u64,
    lifecycle: NativeSurfaceLifecycle,
    capabilities: NativeEventCapabilities,
    event_adapter: EventAdapter,
    events: Option<WindowEventReceiver>,
    wgpu_surface: Option<Arc<wgpu::Surface<'static>>>,
    app_window_surface: Option<Arc<AppWindowSurface>>,
    window: Option<Window>,
    instance: Option<wgpu::Instance>,
    adapter: Option<wgpu::Adapter>,
    device: Option<wgpu::Device>,
    configuration: Option<wgpu::SurfaceConfiguration>,
    render_thread: ThreadId,
    frame_in_flight: bool,
    _claim: HostClaim,
}

impl NativeSurfaceHost {
    pub async fn open(config: NativeWindowConfig) -> Result<Self, NativeHostError> {
        config.validate()?;
        let claim = HostClaim::acquire()?;
        let render_thread = thread::current().id();
        ensure_strategy(WGPU_STRATEGY, "WGPU instance creation")?;

        let mut window = Window::new(
            Position::new(config.position.x, config.position.y),
            Size::new(
                f64::from(config.initial_logical_size.width),
                f64::from(config.initial_logical_size.height),
            ),
            config.title,
        )
        .await;
        ensure_thread(render_thread, "native window creation")?;

        let app_window_surface = Arc::new(window.surface().await);
        let events = app_window_surface
            .take_events()
            .map_err(|_| NativeHostError::WindowEventsAlreadyTaken)?;
        let capabilities = NativeEventCapabilities::from_app_window(events.capabilities());
        let (size, scale) = app_window_surface.size_scale().await;
        ensure_thread(render_thread, "native surface creation")?;
        let viewport = viewport(size, scale)?;

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let wgpu_surface = create_wgpu_surface(&instance, &app_window_surface).await?;
        ensure_thread(render_thread, "WGPU surface creation")?;

        Ok(Self {
            ids: config.ids.clone(),
            viewport,
            epoch: 1,
            configuration_generation: 0,
            present_id: 0,
            lifecycle: NativeSurfaceLifecycle::Unconfigured,
            capabilities,
            event_adapter: EventAdapter::new(config.ids),
            events: Some(events),
            wgpu_surface: Some(wgpu_surface),
            app_window_surface: Some(app_window_surface),
            window: Some(window),
            instance: Some(instance),
            adapter: None,
            device: None,
            configuration: None,
            render_thread,
            frame_in_flight: false,
            _claim: claim,
        })
    }

    pub fn ids(&self) -> &NativeHostIds {
        &self.ids
    }

    pub fn viewport(&self) -> NativeViewport {
        self.viewport
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn lifecycle(&self) -> NativeSurfaceLifecycle {
        self.lifecycle
    }

    pub fn event_capabilities(&self) -> NativeEventCapabilities {
        self.capabilities
    }

    pub fn focus_sensitive_input(
        &mut self,
        target: SensitiveInputTarget,
    ) -> Result<SensitiveInputHandle, NativeHostError> {
        self.ensure_render_thread("sensitive input focus")?;
        self.event_adapter.focus_sensitive_input(target)
    }

    pub fn clear_sensitive_input_focus(&mut self) -> Result<(), NativeHostError> {
        self.ensure_render_thread("sensitive input focus clear")?;
        self.event_adapter.clear_sensitive_input_focus();
        Ok(())
    }

    pub fn restart_sensitive_inputs(&mut self) -> Result<(), NativeHostError> {
        self.ensure_render_thread("sensitive input restart")?;
        self.event_adapter.restart_sensitive_inputs();
        Ok(())
    }

    pub fn with_sensitive_input<R>(
        &self,
        handle: SensitiveInputHandle,
        use_bytes: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, NativeHostError> {
        self.ensure_render_thread("sensitive input access")?;
        self.event_adapter
            .with_sensitive_input(handle, use_bytes)
            .map_err(Into::into)
    }

    pub fn binding(&self) -> Option<NativeSurfaceBinding> {
        self.configuration
            .as_ref()
            .map(|configuration| NativeSurfaceBinding {
                surface_id: self.ids.surface.clone(),
                epoch: self.epoch,
                configuration_generation: self.configuration_generation,
                viewport: self.viewport,
                format: configuration.format,
                usage: configuration.usage,
                present_mode: configuration.present_mode,
                alpha_mode: configuration.alpha_mode,
            })
    }

    pub async fn request_adapter(
        &self,
        power_preference: wgpu::PowerPreference,
        force_fallback_adapter: bool,
    ) -> Result<wgpu::Adapter, NativeHostError> {
        self.ensure_render_thread("WGPU adapter request")?;
        let adapter = self
            .instance
            .as_ref()
            .expect("native instance missing")
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                force_fallback_adapter,
                compatible_surface: Some(
                    self.wgpu_surface
                        .as_deref()
                        .expect("native WGPU surface missing"),
                ),
            })
            .await
            .map_err(|error| NativeHostError::RequestAdapter(error.to_string()))?;
        self.ensure_render_thread("WGPU adapter request")?;
        Ok(adapter)
    }

    pub async fn configure(
        &mut self,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        preferences: SurfacePreferences,
    ) -> Result<NativeSurfaceBinding, NativeHostError> {
        self.ensure_render_thread("surface configuration")?;
        if self.lifecycle == NativeSurfaceLifecycle::Closing {
            return Err(NativeHostError::InvalidConfig("surface is closing"));
        }
        let surface = Arc::clone(
            self.wgpu_surface
                .as_ref()
                .expect("native WGPU surface missing"),
        );
        let capability_adapter = adapter.clone();
        let capabilities = on_surface_thread("surface capabilities", move || {
            surface.get_capabilities(&capability_adapter)
        })
        .await?;
        let format = choose_format(&capabilities, preferences.preferred_format)
            .ok_or(NativeHostError::SurfaceUnsupported)?;
        let present_mode = choose_present_mode(&capabilities, preferences.preferred_present_mode)
            .ok_or(NativeHostError::SurfaceUnsupported)?;
        let alpha_mode =
            choose_alpha_mode(&capabilities).ok_or(NativeHostError::SurfaceUnsupported)?;
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if preferences.allow_copy_dst && capabilities.usages.contains(wgpu::TextureUsages::COPY_DST)
        {
            usage |= wgpu::TextureUsages::COPY_DST;
        }
        let configuration = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: self.viewport.physical_size.width.max(1),
            height: self.viewport.physical_size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: preferences.desired_maximum_frame_latency.max(1),
            alpha_mode,
            view_formats: Vec::new(),
        };

        if self.configuration.is_some() {
            self.increment_epoch()?;
        }
        self.adapter = Some(adapter.clone());
        self.device = Some(device.clone());
        self.configuration = Some(configuration);
        self.increment_configuration_generation()?;
        self.configure_live_surface().await?;
        Ok(self
            .binding()
            .expect("surface binding missing after configure"))
    }

    pub async fn next_event(&mut self) -> Result<HostEventEnvelope, NativeHostError> {
        self.ensure_render_thread("event receive")?;
        loop {
            let event = self
                .events
                .as_mut()
                .expect("native event receiver missing")
                .next()
                .await
                .map_err(map_event_error)?;
            if let Some(event) = self.process_event(event).await? {
                return Ok(event);
            }
        }
    }

    pub async fn drain_events(&mut self) -> Result<Vec<HostEventEnvelope>, NativeHostError> {
        self.ensure_render_thread("event drain")?;
        let events = self
            .events
            .as_mut()
            .expect("native event receiver missing")
            .try_drain()
            .map_err(map_event_error)?;
        let mut translated = Vec::with_capacity(events.len());
        for event in events {
            if let Some(event) = self.process_event(event).await? {
                translated.push(event);
            }
        }
        Ok(translated)
    }

    pub async fn acquire_frame(&mut self) -> Result<NativeSurfaceFrame<'_>, SurfaceAcquireError> {
        self.ensure_render_thread("surface acquisition")?;
        match self.lifecycle {
            NativeSurfaceLifecycle::Unconfigured => return Err(SurfaceAcquireError::Unconfigured),
            NativeSurfaceLifecycle::Suspended => return Err(SurfaceAcquireError::Suspended),
            NativeSurfaceLifecycle::Closing => return Err(SurfaceAcquireError::Closing),
            NativeSurfaceLifecycle::Configured => {}
        }
        if self.frame_in_flight {
            return Err(SurfaceAcquireError::FrameInFlight);
        }
        let surface = Arc::clone(
            self.wgpu_surface
                .as_ref()
                .expect("native WGPU surface missing"),
        );
        let current =
            on_surface_thread("surface acquisition", move || surface.get_current_texture()).await?;
        match current {
            CurrentSurfaceTexture::Success(texture) => Ok(self.frame(texture, false)),
            CurrentSurfaceTexture::Suboptimal(texture) => Ok(self.frame(texture, true)),
            CurrentSurfaceTexture::Timeout => Err(SurfaceAcquireError::Timeout),
            CurrentSurfaceTexture::Occluded => Err(SurfaceAcquireError::Occluded),
            CurrentSurfaceTexture::Validation => Err(SurfaceAcquireError::Validation),
            CurrentSurfaceTexture::Outdated => {
                self.repair_surface(SurfaceReconfigureReason::Outdated)
                    .await?;
                Err(SurfaceAcquireError::Reconfigured {
                    reason: SurfaceReconfigureReason::Outdated,
                    epoch: self.epoch,
                })
            }
            CurrentSurfaceTexture::Lost => {
                self.repair_surface(SurfaceReconfigureReason::Lost).await?;
                Err(SurfaceAcquireError::Reconfigured {
                    reason: SurfaceReconfigureReason::Lost,
                    epoch: self.epoch,
                })
            }
        }
    }

    pub fn begin_close(&mut self) {
        self.event_adapter.restart_sensitive_inputs();
        self.lifecycle = NativeSurfaceLifecycle::Closing;
    }

    pub fn shutdown(self) {}

    async fn process_event(
        &mut self,
        event: WindowEventEnvelope,
    ) -> Result<Option<HostEventEnvelope>, NativeHostError> {
        let WindowEventEnvelope { event, callback_at } = event;
        match self.event_adapter.adapt(event)? {
            AdaptedWindowEvent::Omitted => Ok(None),
            AdaptedWindowEvent::Host(event) => {
                if matches!(event, HostEvent::CloseRequested { .. }) {
                    self.begin_close();
                }
                self.event_adapter
                    .envelope(event, self.epoch, callback_to_host_ns(&callback_at))
                    .map(Some)
            }
            AdaptedWindowEvent::Resize(viewport) => {
                self.apply_resize(viewport).await?;
                self.event_adapter
                    .resize_envelope(viewport, self.epoch, callback_to_host_ns(&callback_at))
                    .map(Some)
            }
        }
    }

    async fn apply_resize(&mut self, viewport: NativeViewport) -> Result<(), NativeHostError> {
        self.ensure_render_thread("surface resize")?;
        self.viewport = viewport;
        self.increment_epoch()?;
        if let Some(configuration) = self.configuration.as_mut() {
            configuration.width = viewport.physical_size.width.max(1);
            configuration.height = viewport.physical_size.height.max(1);
            self.increment_configuration_generation()?;
            self.configure_live_surface().await?;
        }
        Ok(())
    }

    async fn configure_live_surface(&mut self) -> Result<(), NativeHostError> {
        if self.lifecycle == NativeSurfaceLifecycle::Closing {
            return Ok(());
        }
        if self.configuration.is_none() {
            self.lifecycle = NativeSurfaceLifecycle::Unconfigured;
            return Ok(());
        }
        if self.viewport.is_zero_sized() {
            self.lifecycle = NativeSurfaceLifecycle::Suspended;
            return Ok(());
        }
        let surface = Arc::clone(
            self.wgpu_surface
                .as_ref()
                .expect("native WGPU surface missing"),
        );
        let device = self
            .device
            .as_ref()
            .expect("surface device missing")
            .clone();
        let configuration = self
            .configuration
            .as_ref()
            .expect("surface configuration missing")
            .clone();
        on_surface_thread("surface configuration", move || {
            surface.configure(&device, &configuration);
        })
        .await?;
        self.lifecycle = NativeSurfaceLifecycle::Configured;
        Ok(())
    }

    async fn repair_surface(
        &mut self,
        reason: SurfaceReconfigureReason,
    ) -> Result<(), NativeHostError> {
        self.ensure_render_thread("surface repair")?;
        self.increment_epoch()?;
        if reason == SurfaceReconfigureReason::Lost {
            let instance = self.instance.as_ref().expect("native instance missing");
            let app_surface = self
                .app_window_surface
                .as_ref()
                .expect("native app_window surface missing");
            let replacement = create_wgpu_surface(instance, app_surface).await?;
            let old = self.wgpu_surface.replace(replacement);
            if let Some(old) = old {
                drop_wgpu_surface(old);
            }
            self.validate_recreated_surface().await?;
        }
        self.increment_configuration_generation()?;
        self.configure_live_surface().await
    }

    async fn validate_recreated_surface(&self) -> Result<(), NativeHostError> {
        let Some(configuration) = self.configuration.as_ref() else {
            return Ok(());
        };
        let surface = Arc::clone(
            self.wgpu_surface
                .as_ref()
                .expect("native WGPU surface missing"),
        );
        let adapter = self
            .adapter
            .as_ref()
            .expect("surface adapter missing")
            .clone();
        let capabilities = on_surface_thread("surface capabilities", move || {
            surface.get_capabilities(&adapter)
        })
        .await?;
        if !capabilities.formats.contains(&configuration.format)
            || !capabilities
                .present_modes
                .contains(&configuration.present_mode)
            || !capabilities.alpha_modes.contains(&configuration.alpha_mode)
            || !capabilities.usages.contains(configuration.usage)
        {
            return Err(NativeHostError::SurfaceCapabilitiesChanged);
        }
        Ok(())
    }

    fn frame(&mut self, texture: wgpu::SurfaceTexture, suboptimal: bool) -> NativeSurfaceFrame<'_> {
        self.frame_in_flight = true;
        let epoch = self.epoch;
        let configuration_generation = self.configuration_generation;
        NativeSurfaceFrame {
            host: self,
            texture: Some(texture),
            epoch,
            configuration_generation,
            suboptimal,
            finished: false,
        }
    }

    fn increment_epoch(&mut self) -> Result<(), NativeHostError> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(NativeHostError::CounterOverflow("surface epoch"))?;
        Ok(())
    }

    fn increment_configuration_generation(&mut self) -> Result<(), NativeHostError> {
        self.configuration_generation = self.configuration_generation.checked_add(1).ok_or(
            NativeHostError::CounterOverflow("surface configuration generation"),
        )?;
        Ok(())
    }

    fn ensure_render_thread(&self, operation: &'static str) -> Result<(), NativeHostError> {
        ensure_thread(self.render_thread, operation)?;
        ensure_strategy(WGPU_STRATEGY, operation)
    }
}

fn callback_to_host_ns(callback_at: &WindowEventTimestamp) -> CallbackToHostNs {
    CallbackToHostNs::saturating_from_duration(callback_at.elapsed())
}

impl Drop for NativeSurfaceHost {
    fn drop(&mut self) {
        self.lifecycle = NativeSurfaceLifecycle::Closing;
        debug_assert!(!self.frame_in_flight);
        self.events.take();
        if let Some(surface) = self.wgpu_surface.take() {
            drop_wgpu_surface(surface);
        }
        self.app_window_surface.take();
        self.window.take();
        self.configuration.take();
        self.device.take();
        self.adapter.take();
        self.instance.take();
    }
}

pub struct NativeSurfaceFrame<'host> {
    host: &'host mut NativeSurfaceHost,
    texture: Option<wgpu::SurfaceTexture>,
    epoch: u64,
    configuration_generation: u64,
    suboptimal: bool,
    finished: bool,
}

impl NativeSurfaceFrame<'_> {
    pub fn texture(&self) -> &wgpu::Texture {
        &self
            .texture
            .as_ref()
            .expect("surface frame finished")
            .texture
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn configuration_generation(&self) -> u64 {
        self.configuration_generation
    }

    pub async fn present(mut self) -> Result<SurfacePresentReceipt, SurfacePresentError> {
        if self.host.lifecycle == NativeSurfaceLifecycle::Closing {
            return Err(SurfacePresentError::Closing);
        }
        if self.epoch != self.host.epoch {
            return Err(SurfacePresentError::StaleFrame {
                frame_epoch: self.epoch,
                surface_epoch: self.host.epoch,
            });
        }
        let texture = self.texture.take().expect("surface frame finished");
        on_surface_thread("surface present", move || texture.present()).await?;
        self.host.frame_in_flight = false;
        self.finished = true;
        self.host.present_id =
            self.host
                .present_id
                .checked_add(1)
                .ok_or(SurfacePresentError::Host(NativeHostError::CounterOverflow(
                    "surface present ID",
                )))?;

        let recovered_suboptimal_surface = self.suboptimal;
        let presented_epoch = self.epoch;
        let presented_generation = self.configuration_generation;
        if self.suboptimal {
            self.host
                .repair_surface(SurfaceReconfigureReason::Suboptimal)
                .await?;
        }
        Ok(SurfacePresentReceipt {
            surface_id: self.host.ids.surface.clone(),
            epoch: presented_epoch,
            configuration_generation: presented_generation,
            present_id: self.host.present_id,
            recovered_suboptimal_surface,
        })
    }

    pub fn discard(mut self) {
        if let Some(texture) = self.texture.take() {
            drop_surface_texture(texture);
        }
        self.host.frame_in_flight = false;
        self.finished = true;
    }
}

impl Drop for NativeSurfaceFrame<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(texture) = self.texture.take() {
            drop_surface_texture(texture);
        }
        self.host.frame_in_flight = false;
        self.finished = true;
    }
}

fn choose_format(
    capabilities: &wgpu::SurfaceCapabilities,
    preferred: Option<wgpu::TextureFormat>,
) -> Option<wgpu::TextureFormat> {
    preferred
        .filter(|format| capabilities.formats.contains(format))
        .or_else(|| {
            capabilities
                .formats
                .iter()
                .copied()
                .find(|format| format.is_srgb())
        })
        .or_else(|| capabilities.formats.first().copied())
}

fn choose_present_mode(
    capabilities: &wgpu::SurfaceCapabilities,
    preferred: Option<wgpu::PresentMode>,
) -> Option<wgpu::PresentMode> {
    let explicit = [preferred.unwrap_or(wgpu::PresentMode::Fifo)];
    let preferences: &[wgpu::PresentMode] = match preferred {
        Some(wgpu::PresentMode::AutoNoVsync) => &[
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ],
        Some(wgpu::PresentMode::AutoVsync) => {
            &[wgpu::PresentMode::Fifo, wgpu::PresentMode::FifoRelaxed]
        }
        Some(_) => &explicit,
        None => &[],
    };
    preferences
        .iter()
        .copied()
        .find(|mode| capabilities.present_modes.contains(mode))
        .or_else(|| {
            capabilities
                .present_modes
                .contains(&wgpu::PresentMode::Fifo)
                .then_some(wgpu::PresentMode::Fifo)
        })
        .or_else(|| capabilities.present_modes.first().copied())
}

fn choose_alpha_mode(capabilities: &wgpu::SurfaceCapabilities) -> Option<wgpu::CompositeAlphaMode> {
    capabilities
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::Opaque)
        .then_some(wgpu::CompositeAlphaMode::Opaque)
        .or_else(|| capabilities.alpha_modes.first().copied())
}

async fn create_wgpu_surface(
    instance: &wgpu::Instance,
    app_surface: &Arc<AppWindowSurface>,
) -> Result<Arc<wgpu::Surface<'static>>, NativeHostError> {
    let instance = instance.clone();
    let app_surface = Arc::clone(app_surface);
    let surface = on_surface_thread("WGPU surface creation", move || unsafe {
        let raw_display_handle = app_surface.raw_display_handle();
        let raw_window_handle = app_surface.raw_window_handle();
        instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    })
    .await?
    .map_err(|error| NativeHostError::CreateSurface(error.to_string()))?;
    Ok(Arc::new(surface))
}

async fn on_surface_thread<R, F>(operation: &'static str, task: F) -> Result<R, NativeHostError>
where
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    match WGPU_SURFACE_STRATEGY {
        WGPUStrategy::MainThread if app_window::application::is_main_thread() => Ok(task()),
        WGPUStrategy::MainThread => {
            Ok(app_window::application::on_main_thread(operation.to_owned(), task).await)
        }
        WGPUStrategy::NotMainThread if app_window::application::is_main_thread() => {
            Err(NativeHostError::WrongWgpuThread {
                operation,
                requirement: "a non-main thread",
            })
        }
        WGPUStrategy::NotMainThread | WGPUStrategy::Relaxed => Ok(task()),
        _ => Err(NativeHostError::UnsupportedWgpuStrategy { operation }),
    }
}

fn ensure_strategy(strategy: WGPUStrategy, operation: &'static str) -> Result<(), NativeHostError> {
    match strategy {
        WGPUStrategy::MainThread if !app_window::application::is_main_thread() => {
            Err(NativeHostError::WrongWgpuThread {
                operation,
                requirement: "the main thread",
            })
        }
        WGPUStrategy::NotMainThread if app_window::application::is_main_thread() => {
            Err(NativeHostError::WrongWgpuThread {
                operation,
                requirement: "a non-main thread",
            })
        }
        WGPUStrategy::MainThread | WGPUStrategy::NotMainThread | WGPUStrategy::Relaxed => Ok(()),
        _ => Err(NativeHostError::UnsupportedWgpuStrategy { operation }),
    }
}

fn ensure_thread(expected: ThreadId, operation: &'static str) -> Result<(), NativeHostError> {
    if thread::current().id() != expected {
        return Err(NativeHostError::WrongRenderThread { operation });
    }
    Ok(())
}

fn public_strategy(strategy: WGPUStrategy) -> NativeThreadStrategy {
    match strategy {
        WGPUStrategy::MainThread => NativeThreadStrategy::MainThread,
        WGPUStrategy::NotMainThread => NativeThreadStrategy::NotMainThread,
        WGPUStrategy::Relaxed => NativeThreadStrategy::Relaxed,
        _ => NativeThreadStrategy::Relaxed,
    }
}

fn drop_wgpu_surface(surface: Arc<wgpu::Surface<'static>>) {
    run_surface_drop("WGPU surface drop", move || drop(surface));
}

fn drop_surface_texture(texture: wgpu::SurfaceTexture) {
    run_surface_drop("WGPU surface texture discard", move || drop(texture));
}

fn run_surface_drop<F>(operation: &'static str, task: F)
where
    F: FnOnce() + Send + 'static,
{
    match WGPU_SURFACE_STRATEGY {
        WGPUStrategy::MainThread if !app_window::application::is_main_thread() => {
            futures::executor::block_on(app_window::application::on_main_thread(
                operation.to_owned(),
                task,
            ));
        }
        WGPUStrategy::NotMainThread if app_window::application::is_main_thread() => {
            thread::Builder::new()
                .name("boon-wgpu-surface-drop".to_owned())
                .spawn(task)
                .expect("failed to spawn WGPU surface drop thread")
                .join()
                .expect("WGPU surface drop thread panicked");
        }
        _ => task(),
    }
}
