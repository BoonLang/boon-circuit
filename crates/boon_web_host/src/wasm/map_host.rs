use super::{BrowserMapTileAdapter, CanvasFrameResult, WebGpuCanvasHost};
use crate::{
    BrowserGestureEvent, BrowserHostEvent, MapViewportHostController, MapViewportHostEvent,
    WebGpuFrameIdentity, WebHostError, WebHostResult,
};
use boon_document::{DocumentNodeId, MapTileCacheKey, RenderScene};
use boon_native_gpu::{MapTileEvent, MapTileFetchRequest, MapTileSubmission};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;

const MAX_RETAINED_MAP_TILE_EVENTS: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserMapTilePumpConfig {
    pub max_in_flight: usize,
    pub max_retries: u8,
}

impl Default for BrowserMapTilePumpConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 16,
            max_retries: 2,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrowserMapTilePumpMetrics {
    pub dispatched: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub retried: u64,
    pub stale_rejected: u64,
    pub prewarm_uploads: u64,
    pub prewarm_bytes: u64,
}

type RequestKey = (DocumentNodeId, MapTileCacheKey);

struct PendingRequest {
    cancellation: super::BrowserFetchCancellation,
}

struct CompletedRequest {
    request: MapTileFetchRequest,
    result: WebHostResult<boon_native_gpu::DecodedMapTile>,
}

/// Bounded asynchronous fetch/decode lane for browser map tiles. Completion
/// wakes the demand-driven frame scheduler, but decoding and network work never
/// execute inside a product frame.
pub struct BrowserMapTilePump {
    adapter: Rc<BrowserMapTileAdapter>,
    config: BrowserMapTilePumpConfig,
    pending: BTreeMap<RequestKey, PendingRequest>,
    attempts: BTreeMap<RequestKey, u8>,
    completed: Rc<RefCell<VecDeque<CompletedRequest>>>,
    wake: Rc<dyn Fn()>,
    next_request_id: u64,
    metrics: BrowserMapTilePumpMetrics,
    events: VecDeque<MapTileEvent>,
}

impl BrowserMapTilePump {
    pub fn new(
        adapter: BrowserMapTileAdapter,
        config: BrowserMapTilePumpConfig,
        wake: Rc<dyn Fn()>,
    ) -> WebHostResult<Self> {
        if config.max_in_flight == 0 {
            return Err(WebHostError::InvalidInput {
                field: "browser map tile pump max_in_flight".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self {
            adapter: Rc::new(adapter),
            config,
            pending: BTreeMap::new(),
            attempts: BTreeMap::new(),
            completed: Rc::new(RefCell::new(VecDeque::new())),
            wake,
            next_request_id: 1,
            metrics: BrowserMapTilePumpMetrics::default(),
            events: VecDeque::new(),
        })
    }

    pub fn metrics(&self) -> &BrowserMapTilePumpMetrics {
        &self.metrics
    }

    pub fn has_pending_work(&self) -> bool {
        !self.pending.is_empty() || !self.completed.borrow().is_empty()
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = MapTileEvent> + '_ {
        self.events.drain(..)
    }

    fn wake_product_frame(&self) {
        (self.wake)();
    }

    pub fn service_before_frame(&mut self, canvas: &mut WebGpuCanvasHost) -> WebHostResult<bool> {
        self.consume_renderer_events(canvas);
        let mut visible_changed = false;
        let completed = self.completed.borrow_mut().drain(..).collect::<Vec<_>>();
        for completed in completed {
            let key = request_key(&completed.request);
            self.pending.remove(&key);
            self.metrics.completed = self.metrics.completed.saturating_add(1);
            match completed.result {
                Ok(tile) => match canvas.submit_map_tile(tile).map_err(|error| {
                    WebHostError::platform("submit decoded browser map tile", error.to_string())
                })? {
                    MapTileSubmission::Accepted => {
                        self.attempts.remove(&key);
                        visible_changed = true;
                    }
                    MapTileSubmission::StaleRejected => {
                        self.attempts.remove(&key);
                        self.metrics.stale_rejected = self.metrics.stale_rejected.saturating_add(1);
                    }
                    MapTileSubmission::UnexpectedRejected => {
                        self.attempts.remove(&key);
                    }
                },
                Err(error) => {
                    self.metrics.failed = self.metrics.failed.saturating_add(1);
                    let retryable = matches!(error, WebHostError::Platform { .. });
                    let submitted = canvas.submit_map_tile_failure(
                        &completed.request.viewport,
                        &completed.request.identity,
                        error.to_string(),
                        retryable,
                    );
                    let attempts = self.attempts.get(&key).copied().unwrap_or(1);
                    if submitted == MapTileSubmission::Accepted
                        && retryable
                        && attempts <= self.config.max_retries
                        && canvas.retry_map_tile(
                            &completed.request.viewport,
                            &completed.request.identity.tile,
                        )
                    {
                        self.metrics.retried = self.metrics.retried.saturating_add(1);
                    } else {
                        self.attempts.remove(&key);
                    }
                }
            }
        }
        if visible_changed {
            let prepared = canvas.prepare_map_tile_uploads()?;
            self.metrics.prewarm_uploads = self
                .metrics
                .prewarm_uploads
                .saturating_add(u64::from(prepared.upload_count));
            self.metrics.prewarm_bytes = self
                .metrics
                .prewarm_bytes
                .saturating_add(prepared.upload_bytes);
        }
        self.dispatch(canvas)?;
        Ok(visible_changed)
    }

    pub fn service_after_frame(&mut self, canvas: &mut WebGpuCanvasHost) -> WebHostResult<()> {
        self.consume_renderer_events(canvas);
        self.dispatch(canvas)
    }

    fn consume_renderer_events(&mut self, canvas: &mut WebGpuCanvasHost) {
        for event in canvas.drain_map_tile_events() {
            if self.events.len() == MAX_RETAINED_MAP_TILE_EVENTS {
                self.events.pop_front();
            }
            self.events.push_back(event.clone());
            if let MapTileEvent::Cancelled { viewport, identity } = event {
                let key = (viewport, identity.tile);
                if let Some(pending) = self.pending.remove(&key) {
                    pending.cancellation.cancel();
                    self.metrics.cancelled = self.metrics.cancelled.saturating_add(1);
                }
                self.attempts.remove(&key);
            }
        }
    }

    fn dispatch(&mut self, canvas: &mut WebGpuCanvasHost) -> WebHostResult<()> {
        let available = self.config.max_in_flight.saturating_sub(self.pending.len());
        for request in canvas.take_map_tile_requests(available) {
            let key = request_key(&request);
            if self.pending.contains_key(&key) {
                continue;
            }
            let cancellation = super::BrowserFetchCancellation::new()?;
            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.saturating_add(1);
            let adapter = Rc::clone(&self.adapter);
            let completed = Rc::clone(&self.completed);
            let wake = Rc::clone(&self.wake);
            let task_request = request.clone();
            let task_cancellation = cancellation.clone();
            spawn_local(async move {
                let result = adapter
                    .fetch_and_decode(request_id, task_request.clone(), &task_cancellation)
                    .await;
                completed.borrow_mut().push_back(CompletedRequest {
                    request: task_request,
                    result,
                });
                wake();
            });
            self.pending
                .insert(key.clone(), PendingRequest { cancellation });
            let attempt = self
                .attempts
                .get(&key)
                .copied()
                .unwrap_or_default()
                .saturating_add(1);
            self.attempts.insert(key, attempt);
            self.metrics.dispatched = self.metrics.dispatched.saturating_add(1);
        }
        Ok(())
    }
}

/// Complete browser MapViewport host boundary: descriptor-driven camera
/// interaction, asynchronous tile service, retained WebGPU rendering and
/// device-loss reconstruction. Semantic DOM projection remains the separate
/// keyed accessibility path owned by `SemanticProjectionState`.
pub struct BrowserMapViewportHost {
    canvas: WebGpuCanvasHost,
    tiles: BrowserMapTilePump,
    interaction: MapViewportHostController,
    rendered_interaction_revision: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowserMapInputOutcome {
    pub consumed: bool,
    pub visible_changed: bool,
}

impl BrowserMapViewportHost {
    pub fn new(canvas: WebGpuCanvasHost, tiles: BrowserMapTilePump) -> Self {
        Self {
            canvas,
            tiles,
            interaction: MapViewportHostController::default(),
            rendered_interaction_revision: 0,
        }
    }

    pub fn canvas(&self) -> &WebGpuCanvasHost {
        &self.canvas
    }

    pub fn canvas_mut(&mut self) -> &mut WebGpuCanvasHost {
        &mut self.canvas
    }

    pub fn tile_metrics(&self) -> &BrowserMapTilePumpMetrics {
        self.tiles.metrics()
    }

    pub fn handle_event(
        &mut self,
        scene: &RenderScene,
        event: &BrowserHostEvent,
    ) -> WebHostResult<BrowserMapInputOutcome> {
        match event {
            BrowserHostEvent::Input { envelope } => {
                let consumed = self.interaction.consumes_host_event(scene, &envelope.event);
                let visible_changed = self
                    .interaction
                    .handle_host_event(scene, &envelope.event)
                    .map_err(map_input_error)?;
                Ok(BrowserMapInputOutcome {
                    consumed,
                    visible_changed,
                })
            }
            BrowserHostEvent::Gesture { event } => match event {
                BrowserGestureEvent::TouchStart { pointer_id, x, y } => {
                    let consumed = self.interaction.contains_map_point(scene, *x, *y);
                    self.interaction.touch_start(scene, *pointer_id, *x, *y);
                    Ok(BrowserMapInputOutcome {
                        consumed,
                        visible_changed: false,
                    })
                }
                BrowserGestureEvent::TouchMove { pointer_id, x, y } => {
                    let consumed = self.interaction.active_touch_count() != 0;
                    let visible_changed = self
                        .interaction
                        .touch_move(*pointer_id, *x, *y)
                        .map_err(map_input_error)?;
                    Ok(BrowserMapInputOutcome {
                        consumed,
                        visible_changed,
                    })
                }
                BrowserGestureEvent::TouchEnd { pointer_id, .. } => {
                    let consumed = self.interaction.active_touch_count() != 0;
                    self.interaction.touch_end(*pointer_id);
                    Ok(BrowserMapInputOutcome {
                        consumed,
                        visible_changed: false,
                    })
                }
                BrowserGestureEvent::Pinch {
                    center_x,
                    center_y,
                    scale_delta,
                } => {
                    let consumed = self
                        .interaction
                        .contains_map_point(scene, *center_x, *center_y);
                    let visible_changed = self
                        .interaction
                        .pinch(scene, *center_x, *center_y, *scale_delta)
                        .map_err(map_input_error)?;
                    Ok(BrowserMapInputOutcome {
                        consumed,
                        visible_changed,
                    })
                }
            },
            _ => Ok(BrowserMapInputOutcome::default()),
        }
    }

    pub fn drain_map_events(&mut self) -> impl Iterator<Item = MapViewportHostEvent> + '_ {
        self.interaction.drain_events()
    }

    pub fn drain_tile_events(&mut self) -> impl Iterator<Item = MapTileEvent> + '_ {
        self.tiles.drain_events()
    }

    pub async fn render(
        &mut self,
        scene: &RenderScene,
        identity: &WebGpuFrameIdentity,
    ) -> WebHostResult<CanvasFrameResult> {
        self.canvas.resize_to_display_size()?;
        if self.canvas.device_lost_reason().is_some() {
            self.canvas.recover_lost_device().await?;
        }
        let interaction_revision = self.interaction.revision();
        let interaction_dirty = interaction_revision != self.rendered_interaction_revision;
        if !interaction_dirty {
            self.tiles.service_before_frame(&mut self.canvas)?;
        }
        let scene = self
            .interaction
            .scene_for_render(scene)
            .map_err(map_input_error)?;
        let result = self.canvas.render(&scene, identity)?;
        self.rendered_interaction_revision = interaction_revision;
        if interaction_dirty {
            if self.tiles.service_before_frame(&mut self.canvas)? {
                self.tiles.wake_product_frame();
            }
        } else {
            self.tiles.service_after_frame(&mut self.canvas)?;
        }
        Ok(result)
    }
}

fn request_key(request: &MapTileFetchRequest) -> RequestKey {
    (request.viewport.clone(), request.identity.tile.clone())
}

fn map_input_error(error: boon_document::MapViewportError) -> WebHostError {
    WebHostError::InvalidInput {
        field: format!("MapViewport {}", error.path),
        reason: error.message,
    }
}
