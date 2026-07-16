use boon_document::{
    DocumentNodeId, MapTileCacheKey, MapTileRequestIdentity, MapTileResultFreshness, Rect,
    RenderMapViewport,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const DEFAULT_MAP_TILE_OVERSCAN: u8 = 1;
pub const DEFAULT_MAP_TILE_MAX_PENDING_REQUESTS: usize = 32;
pub const DEFAULT_MAP_TILE_MAX_DECODED_ENTRIES: usize = 256;
pub const DEFAULT_MAP_TILE_DECODED_BYTE_CAP: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAP_TILE_GPU_BYTE_CAP: u64 = 96 * 1024 * 1024;
pub const DEFAULT_MAP_TILE_MAX_GPU_UPLOADS_PER_PREPARE: usize = 8;
pub const MAX_MAP_TILE_ENCODED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileCacheConfig {
    pub max_pending_requests: usize,
    pub max_decoded_entries: usize,
    pub decoded_byte_cap: u64,
    pub gpu_byte_cap: u64,
    pub max_gpu_uploads_per_prepare: usize,
}

impl Default for MapTileCacheConfig {
    fn default() -> Self {
        Self {
            max_pending_requests: DEFAULT_MAP_TILE_MAX_PENDING_REQUESTS,
            max_decoded_entries: DEFAULT_MAP_TILE_MAX_DECODED_ENTRIES,
            decoded_byte_cap: DEFAULT_MAP_TILE_DECODED_BYTE_CAP,
            gpu_byte_cap: DEFAULT_MAP_TILE_GPU_BYTE_CAP,
            max_gpu_uploads_per_prepare: DEFAULT_MAP_TILE_MAX_GPU_UPLOADS_PER_PREPARE,
        }
    }
}

impl MapTileCacheConfig {
    fn validate(&self) -> Result<(), MapTileCacheError> {
        if self.max_pending_requests == 0 {
            return Err(MapTileCacheError::InvalidConfig(
                "max_pending_requests must be at least 1".to_owned(),
            ));
        }
        if self.max_decoded_entries == 0 {
            return Err(MapTileCacheError::InvalidConfig(
                "max_decoded_entries must be at least 1".to_owned(),
            ));
        }
        if self.decoded_byte_cap < 4 || self.gpu_byte_cap < 4 {
            return Err(MapTileCacheError::InvalidConfig(
                "decoded and GPU byte caps must hold at least one RGBA pixel".to_owned(),
            ));
        }
        if self.max_gpu_uploads_per_prepare == 0 {
            return Err(MapTileCacheError::InvalidConfig(
                "max_gpu_uploads_per_prepare must be at least 1".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileFetchRequest {
    pub viewport: DocumentNodeId,
    pub identity: MapTileRequestIdentity,
    pub url_template_capability: String,
    pub allowed_origins: Vec<String>,
    pub expected_tile_size: u16,
}

impl MapTileFetchRequest {
    pub fn accepts_origin(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|allowed| allowed == origin)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DecodedMapTile {
    pub viewport: DocumentNodeId,
    pub identity: MapTileRequestIdentity,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for DecodedMapTile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecodedMapTile")
            .field("viewport", &self.viewport)
            .field("identity", &self.identity)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba_bytes", &self.rgba.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapTileSubmission {
    Accepted,
    StaleRejected,
    UnexpectedRejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapTileEvent {
    Requested {
        viewport: DocumentNodeId,
        identity: MapTileRequestIdentity,
    },
    Cancelled {
        viewport: DocumentNodeId,
        identity: MapTileRequestIdentity,
    },
    Loaded {
        viewport: DocumentNodeId,
        identity: MapTileRequestIdentity,
    },
    Failed {
        viewport: DocumentNodeId,
        identity: MapTileRequestIdentity,
        message: String,
        retryable: bool,
    },
    StaleRejected {
        viewport: DocumentNodeId,
        identity: MapTileRequestIdentity,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileCacheMetrics {
    pub active_viewport_count: u32,
    pub visible_tile_count: u32,
    pub ready_visible_tile_count: u32,
    pub fallback_visible_tile_count: u32,
    pub queued_request_count: u32,
    pub in_flight_request_count: u32,
    pub decoded_cache_entry_count: u32,
    pub decoded_cache_bytes: u64,
    pub decoded_cache_evictions: u64,
    pub stale_result_rejections: u64,
    pub failed_request_count: u32,
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileGpuPrepareMetrics {
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub cache_evictions: u32,
    pub cache_entry_count: u32,
    pub cache_byte_count: u64,
    pub cache_byte_cap: u64,
    pub upload_count: u32,
    pub upload_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MapTileCacheError {
    InvalidConfig(String),
    InvalidPixels(String),
    CapacityExceeded(String),
}

impl std::fmt::Display for MapTileCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid map tile cache: {message}"),
            Self::InvalidPixels(message) => {
                write!(formatter, "invalid decoded map tile: {message}")
            }
            Self::CapacityExceeded(message) => write!(formatter, "map tile cache full: {message}"),
        }
    }
}

impl std::error::Error for MapTileCacheError {}

pub fn decode_map_tile_bytes(
    request: &MapTileFetchRequest,
    media_type: &str,
    bytes: &[u8],
) -> Result<DecodedMapTile, MapTileCacheError> {
    if bytes.is_empty() || bytes.len() > MAX_MAP_TILE_ENCODED_BYTES {
        return Err(MapTileCacheError::InvalidPixels(format!(
            "encoded response size {} is outside 1..={MAX_MAP_TILE_ENCODED_BYTES}",
            bytes.len()
        )));
    }
    let format = match media_type {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        _ => {
            return Err(MapTileCacheError::InvalidPixels(format!(
                "unsupported raster tile media type `{media_type}`"
            )));
        }
    };
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|error| MapTileCacheError::InvalidPixels(format!("decode raster tile: {error}")))?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    let expected = u32::from(request.expected_tile_size);
    if width != expected || height != expected {
        return Err(MapTileCacheError::InvalidPixels(format!(
            "decoded tile is {width}x{height}, expected {expected}x{expected}"
        )));
    }
    Ok(DecodedMapTile {
        viewport: request.viewport.clone(),
        identity: request.identity.clone(),
        width,
        height,
        rgba: decoded.into_raw(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingState {
    Queued,
    InFlight,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingTile {
    request: MapTileFetchRequest,
    state: PendingState,
}

#[derive(Clone, Debug, PartialEq)]
struct ActiveMapViewport {
    map: RenderMapViewport,
    desired: BTreeSet<MapTileCacheKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedTileEntry {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    byte_count: u64,
    last_used: u64,
}

#[derive(Clone, Eq, PartialEq)]
pub struct MapTileCpuSnapshot {
    entries: Vec<(MapTileCacheKey, u32, u32, Vec<u8>, u64)>,
}

impl std::fmt::Debug for MapTileCpuSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let byte_count = self
            .entries
            .iter()
            .map(|(_, _, _, rgba, _)| rgba.len())
            .sum::<usize>();
        formatter
            .debug_struct("MapTileCpuSnapshot")
            .field("entry_count", &self.entries.len())
            .field("rgba_bytes", &byte_count)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MapTileRenderPart {
    pub retained_chunk_id: String,
    pub texture: MapTileCacheKey,
    pub points: [[f32; 2]; 4],
    pub uvs: [[f32; 2]; 4],
    pub clip: Rect,
}

pub struct MapTileCache {
    config: MapTileCacheConfig,
    active: BTreeMap<DocumentNodeId, ActiveMapViewport>,
    pending: BTreeMap<(DocumentNodeId, MapTileCacheKey), PendingTile>,
    request_queue: VecDeque<(DocumentNodeId, MapTileCacheKey)>,
    decoded: BTreeMap<MapTileCacheKey, DecodedTileEntry>,
    decoded_bytes: u64,
    access_tick: u64,
    revision: u64,
    evictions: u64,
    stale_rejections: u64,
    events: VecDeque<MapTileEvent>,
}

impl Default for MapTileCache {
    fn default() -> Self {
        Self::new(MapTileCacheConfig::default())
            .expect("default map tile cache config must be valid")
    }
}

impl MapTileCache {
    pub fn new(config: MapTileCacheConfig) -> Result<Self, MapTileCacheError> {
        config.validate()?;
        Ok(Self {
            config,
            active: BTreeMap::new(),
            pending: BTreeMap::new(),
            request_queue: VecDeque::new(),
            decoded: BTreeMap::new(),
            decoded_bytes: 0,
            access_tick: 0,
            revision: 1,
            evictions: 0,
            stale_rejections: 0,
            events: VecDeque::new(),
        })
    }

    pub fn config(&self) -> &MapTileCacheConfig {
        &self.config
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn sync_scene(&mut self, maps: &[RenderMapViewport]) {
        let mut next = BTreeMap::new();
        for map in maps {
            let desired = map
                .visible_tiles
                .iter()
                .map(|tile| tile.request.tile.clone())
                .collect::<BTreeSet<_>>();
            next.insert(
                map.node.clone(),
                ActiveMapViewport {
                    map: map.clone(),
                    desired,
                },
            );
        }
        if self.active != next {
            self.active = next;
            self.bump_revision();
        }
        self.cancel_obsolete_requests();
        self.enqueue_missing_requests();
        self.touch_visible_cache_entries();
    }

    pub fn take_requests(&mut self, limit: usize) -> Vec<MapTileFetchRequest> {
        let in_flight = self
            .pending
            .values()
            .filter(|pending| pending.state == PendingState::InFlight)
            .count();
        let available = self
            .config
            .max_pending_requests
            .saturating_sub(in_flight)
            .min(limit);
        let mut requests = Vec::with_capacity(available);
        while requests.len() < available {
            let Some(key) = self.request_queue.pop_front() else {
                break;
            };
            let Some(pending) = self.pending.get_mut(&key) else {
                continue;
            };
            if pending.state != PendingState::Queued {
                continue;
            }
            pending.state = PendingState::InFlight;
            requests.push(pending.request.clone());
        }
        requests
    }

    pub fn submit_decoded(
        &mut self,
        tile: DecodedMapTile,
    ) -> Result<MapTileSubmission, MapTileCacheError> {
        let key = (tile.viewport.clone(), tile.identity.tile.clone());
        if self.result_freshness(&tile.viewport, &tile.identity)
            != Some(MapTileResultFreshness::Current)
        {
            self.reject_stale(tile.viewport, tile.identity);
            return Ok(MapTileSubmission::StaleRejected);
        }
        let Some(pending) = self.pending.get(&key) else {
            return Ok(MapTileSubmission::UnexpectedRejected);
        };
        if pending.request.identity != tile.identity || pending.state != PendingState::InFlight {
            return Ok(MapTileSubmission::UnexpectedRejected);
        }
        let pixel_len = u64::from(tile.width)
            .checked_mul(u64::from(tile.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| MapTileCacheError::InvalidPixels("dimensions overflow".to_owned()))?;
        if tile.width == 0
            || tile.height == 0
            || tile.width > 4096
            || tile.height > 4096
            || pixel_len != tile.rgba.len() as u64
        {
            return Err(MapTileCacheError::InvalidPixels(format!(
                "{}x{} requires {pixel_len} RGBA bytes, got {}",
                tile.width,
                tile.height,
                tile.rgba.len()
            )));
        }
        if pixel_len > self.config.decoded_byte_cap {
            return Err(MapTileCacheError::CapacityExceeded(format!(
                "single tile has {pixel_len} bytes but cap is {}",
                self.config.decoded_byte_cap
            )));
        }
        self.evict_for_insert(&tile.identity.tile, pixel_len)?;
        self.pending.remove(&key);
        self.access_tick = self.access_tick.saturating_add(1);
        if let Some(previous) = self.decoded.remove(&tile.identity.tile) {
            self.decoded_bytes = self.decoded_bytes.saturating_sub(previous.byte_count);
        }
        self.decoded_bytes = self.decoded_bytes.saturating_add(pixel_len);
        self.decoded.insert(
            tile.identity.tile.clone(),
            DecodedTileEntry {
                width: tile.width,
                height: tile.height,
                rgba: tile.rgba,
                byte_count: pixel_len,
                last_used: self.access_tick,
            },
        );
        self.events.push_back(MapTileEvent::Loaded {
            viewport: tile.viewport,
            identity: tile.identity,
        });
        self.bump_revision();
        Ok(MapTileSubmission::Accepted)
    }

    pub fn submit_failure(
        &mut self,
        viewport: &DocumentNodeId,
        identity: &MapTileRequestIdentity,
        message: impl Into<String>,
        retryable: bool,
    ) -> MapTileSubmission {
        if self.result_freshness(viewport, identity) != Some(MapTileResultFreshness::Current) {
            self.reject_stale(viewport.clone(), identity.clone());
            return MapTileSubmission::StaleRejected;
        }
        let key = (viewport.clone(), identity.tile.clone());
        let Some(pending) = self.pending.get_mut(&key) else {
            return MapTileSubmission::UnexpectedRejected;
        };
        if pending.request.identity != *identity || pending.state != PendingState::InFlight {
            return MapTileSubmission::UnexpectedRejected;
        }
        pending.state = PendingState::Failed;
        self.events.push_back(MapTileEvent::Failed {
            viewport: viewport.clone(),
            identity: identity.clone(),
            message: message.into(),
            retryable,
        });
        MapTileSubmission::Accepted
    }

    pub fn retry(&mut self, viewport: &DocumentNodeId, tile: &MapTileCacheKey) -> bool {
        let key = (viewport.clone(), tile.clone());
        let Some(pending) = self.pending.get_mut(&key) else {
            return false;
        };
        if pending.state != PendingState::Failed {
            return false;
        }
        pending.state = PendingState::Queued;
        self.request_queue.push_front(key);
        true
    }

    pub fn drain_events(&mut self) -> Vec<MapTileEvent> {
        self.events.drain(..).collect()
    }

    pub fn metrics(&self) -> MapTileCacheMetrics {
        let visible_tile_count = self
            .active
            .values()
            .map(|viewport| viewport.desired.len() as u32)
            .sum();
        let mut ready_visible_tile_count = 0u32;
        let mut fallback_visible_tile_count = 0u32;
        for viewport in self.active.values() {
            for tile in &viewport.desired {
                if self.decoded.contains_key(tile) {
                    ready_visible_tile_count = ready_visible_tile_count.saturating_add(1);
                } else if self.fallback_key(tile).is_some() {
                    fallback_visible_tile_count = fallback_visible_tile_count.saturating_add(1);
                }
            }
        }
        MapTileCacheMetrics {
            active_viewport_count: self.active.len() as u32,
            visible_tile_count,
            ready_visible_tile_count,
            fallback_visible_tile_count,
            queued_request_count: self
                .pending
                .values()
                .filter(|pending| pending.state == PendingState::Queued)
                .count() as u32,
            in_flight_request_count: self
                .pending
                .values()
                .filter(|pending| pending.state == PendingState::InFlight)
                .count() as u32,
            decoded_cache_entry_count: self.decoded.len() as u32,
            decoded_cache_bytes: self.decoded_bytes,
            decoded_cache_evictions: self.evictions,
            stale_result_rejections: self.stale_rejections,
            failed_request_count: self
                .pending
                .values()
                .filter(|pending| pending.state == PendingState::Failed)
                .count() as u32,
            revision: self.revision,
        }
    }

    pub fn snapshot(&self) -> MapTileCpuSnapshot {
        MapTileCpuSnapshot {
            entries: self
                .decoded
                .iter()
                .map(|(key, tile)| {
                    (
                        key.clone(),
                        tile.width,
                        tile.height,
                        tile.rgba.clone(),
                        tile.last_used,
                    )
                })
                .collect(),
        }
    }

    pub fn restore(&mut self, snapshot: MapTileCpuSnapshot) -> Result<(), MapTileCacheError> {
        let mut decoded = BTreeMap::new();
        let mut decoded_bytes = 0u64;
        let mut access_tick = self.access_tick;
        for (key, width, height, rgba, last_used) in snapshot.entries {
            let byte_count = u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| MapTileCacheError::InvalidPixels("snapshot overflow".to_owned()))?;
            if width == 0 || height == 0 || byte_count != rgba.len() as u64 {
                return Err(MapTileCacheError::InvalidPixels(
                    "snapshot contains malformed RGBA pixels".to_owned(),
                ));
            }
            decoded_bytes = decoded_bytes.saturating_add(byte_count);
            if decoded.len() >= self.config.max_decoded_entries
                || decoded_bytes > self.config.decoded_byte_cap
            {
                return Err(MapTileCacheError::CapacityExceeded(
                    "snapshot exceeds configured decoded cache bounds".to_owned(),
                ));
            }
            access_tick = access_tick.max(last_used);
            decoded.insert(
                key,
                DecodedTileEntry {
                    width,
                    height,
                    rgba,
                    byte_count,
                    last_used,
                },
            );
        }
        self.decoded = decoded;
        self.decoded_bytes = decoded_bytes;
        self.access_tick = access_tick;
        self.bump_revision();
        Ok(())
    }

    pub(crate) fn decoded_tile(&self, key: &MapTileCacheKey) -> Option<(u32, u32, &[u8])> {
        self.decoded
            .get(key)
            .map(|tile| (tile.width, tile.height, tile.rgba.as_slice()))
    }

    pub(crate) fn render_parts(&mut self, map: &RenderMapViewport) -> Vec<MapTileRenderPart> {
        let available = self.decoded.keys().cloned().collect::<BTreeSet<_>>();
        self.render_parts_with_available(map, &available)
    }

    pub(crate) fn render_parts_with_available(
        &mut self,
        map: &RenderMapViewport,
        available: &BTreeSet<MapTileCacheKey>,
    ) -> Vec<MapTileRenderPart> {
        let mut parts = Vec::new();
        for visible in &map.visible_tiles {
            let requested = &visible.request.tile;
            if available.contains(requested) {
                self.touch(requested);
                parts.push(render_part_for_key(
                    map,
                    requested,
                    requested,
                    visible
                        .screen_quad
                        .points
                        .map(|point| [point.x as f32, point.y as f32]),
                    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    "exact",
                ));
                continue;
            }
            if let Some(parent) = parent_fallback_key_in(requested, available) {
                self.touch(&parent);
                let difference = requested.z.saturating_sub(parent.z);
                let subdivisions = 1_u32 << difference;
                let child_x = requested.x % subdivisions;
                let child_y = requested.y % subdivisions;
                let scale = 1.0 / subdivisions as f32;
                let u0 = child_x as f32 * scale;
                let v0 = child_y as f32 * scale;
                parts.push(render_part_for_key(
                    map,
                    requested,
                    &parent,
                    visible
                        .screen_quad
                        .points
                        .map(|point| [point.x as f32, point.y as f32]),
                    [
                        [u0, v0],
                        [u0 + scale, v0],
                        [u0 + scale, v0 + scale],
                        [u0, v0 + scale],
                    ],
                    "parent",
                ));
                continue;
            }
            for child in immediate_children(requested) {
                if !available.contains(&child) {
                    continue;
                }
                let Ok(quad) = map.descriptor.tile_screen_quad(&child) else {
                    continue;
                };
                self.touch(&child);
                parts.push(render_part_for_key(
                    map,
                    requested,
                    &child,
                    quad.points.map(|point| [point.x as f32, point.y as f32]),
                    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    "child",
                ));
            }
        }
        parts
    }

    pub(crate) fn required_render_texture_keys(&mut self) -> BTreeSet<MapTileCacheKey> {
        let maps = self
            .active
            .values()
            .map(|active| active.map.clone())
            .collect::<Vec<_>>();
        maps.iter()
            .flat_map(|map| self.render_parts(map))
            .map(|part| part.texture)
            .collect()
    }

    fn result_freshness(
        &self,
        viewport: &DocumentNodeId,
        identity: &MapTileRequestIdentity,
    ) -> Option<MapTileResultFreshness> {
        let active = self.active.get(viewport)?;
        if !active.desired.contains(&identity.tile) {
            return Some(MapTileResultFreshness::Stale);
        }
        Some(identity.freshness(active.map.descriptor.generation))
    }

    fn reject_stale(&mut self, viewport: DocumentNodeId, identity: MapTileRequestIdentity) {
        self.pending
            .remove(&(viewport.clone(), identity.tile.clone()));
        self.stale_rejections = self.stale_rejections.saturating_add(1);
        self.events
            .push_back(MapTileEvent::StaleRejected { viewport, identity });
    }

    fn cancel_obsolete_requests(&mut self) {
        let obsolete = self
            .pending
            .iter()
            .filter(|((viewport, tile), pending)| {
                self.active.get(viewport).is_none_or(|active| {
                    !active.desired.contains(tile)
                        || pending.request.identity.generation != active.map.descriptor.generation
                })
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in obsolete {
            if let Some(pending) = self.pending.remove(&key) {
                self.events.push_back(MapTileEvent::Cancelled {
                    viewport: pending.request.viewport,
                    identity: pending.request.identity,
                });
            }
        }
        self.request_queue
            .retain(|key| self.pending.contains_key(key));
    }

    fn enqueue_missing_requests(&mut self) {
        let mut candidates = Vec::new();
        for (viewport, active) in &self.active {
            let center_x = active.map.bounds.x + active.map.bounds.width / 2.0;
            let center_y = active.map.bounds.y + active.map.bounds.height / 2.0;
            for visible in &active.map.visible_tiles {
                let tile = &visible.request.tile;
                let key = (viewport.clone(), tile.clone());
                if self.decoded.contains_key(tile) || self.pending.contains_key(&key) {
                    continue;
                }
                let bounds = visible.screen_quad.bounds();
                let tile_x = active.map.bounds.x + bounds.x as f32 + bounds.width as f32 / 2.0;
                let tile_y = active.map.bounds.y + bounds.y as f32 + bounds.height as f32 / 2.0;
                let dx = tile_x - center_x;
                let dy = tile_y - center_y;
                candidates.push((
                    dx.mul_add(dx, dy * dy).to_bits(),
                    viewport.clone(),
                    visible.request.clone(),
                    active.map.descriptor.tile_source.clone(),
                ));
            }
        }
        candidates.sort_by(|left, right| {
            f32::from_bits(left.0)
                .total_cmp(&f32::from_bits(right.0))
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.tile.cmp(&right.2.tile))
        });
        let capacity = self
            .config
            .max_pending_requests
            .saturating_sub(self.pending.len());
        for (_, viewport, identity, source) in candidates.into_iter().take(capacity) {
            let key = (viewport.clone(), identity.tile.clone());
            let request = MapTileFetchRequest {
                viewport: viewport.clone(),
                identity: identity.clone(),
                url_template_capability: source.url_template_capability,
                allowed_origins: source.allowed_origins,
                expected_tile_size: source.tile_size,
            };
            self.pending.insert(
                key.clone(),
                PendingTile {
                    request,
                    state: PendingState::Queued,
                },
            );
            self.request_queue.push_back(key);
            self.events
                .push_back(MapTileEvent::Requested { viewport, identity });
        }
    }

    fn touch_visible_cache_entries(&mut self) {
        let keys = self
            .active
            .values()
            .flat_map(|active| active.desired.iter().cloned())
            .collect::<Vec<_>>();
        for key in keys {
            if self.decoded.contains_key(&key) {
                self.touch(&key);
            } else if let Some(fallback) = self.fallback_key(&key) {
                self.touch(&fallback);
            }
        }
    }

    fn touch(&mut self, key: &MapTileCacheKey) {
        self.access_tick = self.access_tick.saturating_add(1);
        if let Some(tile) = self.decoded.get_mut(key) {
            tile.last_used = self.access_tick;
        }
    }

    fn fallback_key(&self, requested: &MapTileCacheKey) -> Option<MapTileCacheKey> {
        self.parent_fallback_key(requested).or_else(|| {
            immediate_children(requested)
                .into_iter()
                .find(|child| self.decoded.contains_key(child))
        })
    }

    fn parent_fallback_key(&self, requested: &MapTileCacheKey) -> Option<MapTileCacheKey> {
        let mut candidate = requested.clone();
        while candidate.z > 0 {
            candidate.z -= 1;
            candidate.x /= 2;
            candidate.y /= 2;
            if self.decoded.contains_key(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn evict_for_insert(
        &mut self,
        incoming: &MapTileCacheKey,
        incoming_bytes: u64,
    ) -> Result<(), MapTileCacheError> {
        let replacing_bytes = self
            .decoded
            .get(incoming)
            .map(|entry| entry.byte_count)
            .unwrap_or_default();
        while self
            .decoded
            .len()
            .saturating_add(usize::from(!self.decoded.contains_key(incoming)))
            > self.config.max_decoded_entries
            || self
                .decoded_bytes
                .saturating_sub(replacing_bytes)
                .saturating_add(incoming_bytes)
                > self.config.decoded_byte_cap
        {
            let candidate = self
                .decoded
                .iter()
                .filter(|(key, _)| *key != incoming)
                .min_by(|left, right| {
                    left.1
                        .last_used
                        .cmp(&right.1.last_used)
                        .then_with(|| left.0.cmp(right.0))
                })
                .map(|(key, _)| key.clone());
            let Some(candidate) = candidate else {
                return Err(MapTileCacheError::CapacityExceeded(
                    "no evictable decoded tile remains".to_owned(),
                ));
            };
            if let Some(removed) = self.decoded.remove(&candidate) {
                self.decoded_bytes = self.decoded_bytes.saturating_sub(removed.byte_count);
                self.evictions = self.evictions.saturating_add(1);
            }
        }
        Ok(())
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn parent_fallback_key_in(
    requested: &MapTileCacheKey,
    available: &BTreeSet<MapTileCacheKey>,
) -> Option<MapTileCacheKey> {
    let mut candidate = requested.clone();
    while candidate.z > 0 {
        candidate.z -= 1;
        candidate.x /= 2;
        candidate.y /= 2;
        if available.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn immediate_children(requested: &MapTileCacheKey) -> [MapTileCacheKey; 4] {
    let mut child = requested.clone();
    child.z = child.z.saturating_add(1);
    child.x = child.x.saturating_mul(2);
    child.y = child.y.saturating_mul(2);
    [
        child.clone(),
        MapTileCacheKey {
            x: child.x.saturating_add(1),
            ..child.clone()
        },
        MapTileCacheKey {
            y: child.y.saturating_add(1),
            ..child.clone()
        },
        MapTileCacheKey {
            x: child.x.saturating_add(1),
            y: child.y.saturating_add(1),
            ..child
        },
    ]
}

fn render_part_for_key(
    map: &RenderMapViewport,
    requested: &MapTileCacheKey,
    texture: &MapTileCacheKey,
    mut points: [[f32; 2]; 4],
    uvs: [[f32; 2]; 4],
    fallback: &str,
) -> MapTileRenderPart {
    for point in &mut points {
        point[0] += map.bounds.x;
        point[1] += map.bounds.y;
    }
    MapTileRenderPart {
        retained_chunk_id: format!(
            "{}:map-tile:{}:{}:{}:{}:{}",
            map.retained_chunk_id, requested.z, requested.x, requested.y, fallback, texture.z
        ),
        texture: texture.clone(),
        points,
        uvs,
        clip: map.clip.unwrap_or(map.bounds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document::{
        MapCamera, MapHitIdentity, MapInteractionPolicy, MapOverlayDescriptor, MapOverlayGeometry,
        MapOverlayId, MapOverlayPaint, MapTileSourceId, MapTileSourceRef, MapViewportBounds,
        MapViewportDescriptor, MapViewportGeneration, RenderMapHitRegion,
    };

    fn map(generation: u64, zoom: f64) -> RenderMapViewport {
        let descriptor = MapViewportDescriptor {
            generation: MapViewportGeneration(generation),
            camera: MapCamera {
                longitude: 10.75,
                latitude: 59.91,
                zoom,
                bearing: 0.0,
            },
            bounds: MapViewportBounds {
                width: 256.0,
                height: 256.0,
                scale: 1.0,
            },
            tile_source: MapTileSourceRef {
                id: MapTileSourceId("fixture".to_owned()),
                url_template_capability: "fixture_tiles".to_owned(),
                min_zoom: 0,
                max_zoom: 8,
                tile_size: 256,
                attribution: "Fixture".to_owned(),
                allowed_origins: vec!["boon-local://fixture".to_owned()],
            },
            interaction: MapInteractionPolicy::default(),
            overlays: vec![MapOverlayDescriptor {
                id: MapOverlayId("point".to_owned()),
                hit_identity: MapHitIdentity("point-hit".to_owned()),
                z_order: 1,
                selected: false,
                focused: false,
                paint: MapOverlayPaint::default(),
                geometry: MapOverlayGeometry::Point {
                    position: boon_document::MapCoordinate {
                        longitude: 10.75,
                        latitude: 59.91,
                    },
                    radius: 5.0,
                    symbol_ref: None,
                },
            }],
        };
        RenderMapViewport {
            node: DocumentNodeId("generic-map".to_owned()),
            retained_chunk_id: "chunk:generic-map".to_owned(),
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 256.0,
                height: 256.0,
            },
            clip: None,
            visible_tiles: descriptor.visible_xyz_tiles(0).unwrap(),
            descriptor,
            overlay_primitives: Vec::new(),
            overlay_text_runs: Vec::new(),
            hit_regions: Vec::<RenderMapHitRegion>::new(),
        }
    }

    fn complete_first_request(cache: &mut MapTileCache) -> MapTileFetchRequest {
        let request = cache.take_requests(1).remove(0);
        cache
            .submit_decoded(DecodedMapTile {
                viewport: request.viewport.clone(),
                identity: request.identity.clone(),
                width: 2,
                height: 2,
                rgba: vec![120; 16],
            })
            .unwrap();
        request
    }

    #[test]
    fn request_lifecycle_is_bounded_and_stale_results_fail_closed() {
        let mut cache = MapTileCache::new(MapTileCacheConfig {
            max_pending_requests: 2,
            max_decoded_entries: 2,
            decoded_byte_cap: 32,
            gpu_byte_cap: 32,
            max_gpu_uploads_per_prepare: 2,
        })
        .unwrap();
        let first_map = map(1, 2.0);
        cache.sync_scene(std::slice::from_ref(&first_map));
        assert_eq!(cache.metrics().queued_request_count, 2);
        let stale = cache.take_requests(1).remove(0);
        let next_map = map(2, 3.0);
        cache.sync_scene(std::slice::from_ref(&next_map));
        let status = cache
            .submit_decoded(DecodedMapTile {
                viewport: stale.viewport,
                identity: stale.identity,
                width: 2,
                height: 2,
                rgba: vec![0; 16],
            })
            .unwrap();
        assert_eq!(status, MapTileSubmission::StaleRejected);
        assert_eq!(cache.metrics().stale_result_rejections, 1);
        assert!(cache.metrics().queued_request_count <= 2);
    }

    #[test]
    fn failed_tile_requires_an_explicit_retry_and_emits_app_visible_events() {
        let mut cache = MapTileCache::default();
        let map = map(1, 2.0);
        cache.sync_scene(std::slice::from_ref(&map));
        let request = cache.take_requests(1).remove(0);
        assert_eq!(
            cache.submit_failure(
                &request.viewport,
                &request.identity,
                "fixture unavailable",
                true,
            ),
            MapTileSubmission::Accepted
        );
        assert_eq!(cache.metrics().failed_request_count, 1);
        assert!(cache.retry(&request.viewport, &request.identity.tile));
        let retried = cache.take_requests(1).remove(0);
        assert_eq!(retried.identity, request.identity);
        let events = cache.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            MapTileEvent::Failed {
                retryable: true,
                ..
            }
        )));
    }

    #[test]
    fn decoded_lru_is_bounded_and_snapshot_restores_after_device_loss() {
        let mut cache = MapTileCache::new(MapTileCacheConfig {
            max_pending_requests: 8,
            max_decoded_entries: 2,
            decoded_byte_cap: 32,
            gpu_byte_cap: 32,
            max_gpu_uploads_per_prepare: 2,
        })
        .unwrap();
        cache.sync_scene(&[map(1, 2.0)]);
        complete_first_request(&mut cache);
        complete_first_request(&mut cache);
        complete_first_request(&mut cache);
        assert_eq!(cache.metrics().decoded_cache_evictions, 1);
        assert_eq!(cache.metrics().decoded_cache_entry_count, 2);
        let snapshot = cache.snapshot();
        let mut restored = MapTileCache::new(cache.config().clone()).unwrap();
        restored.restore(snapshot).unwrap();
        assert_eq!(restored.metrics().decoded_cache_entry_count, 2);
        assert_eq!(restored.metrics().decoded_cache_bytes, 32);
    }

    #[test]
    fn parent_tile_is_retained_while_zoomed_children_load() {
        let mut cache = MapTileCache::default();
        let parent_map = map(1, 1.0);
        cache.sync_scene(std::slice::from_ref(&parent_map));
        complete_first_request(&mut cache);
        let child_map = map(2, 2.0);
        cache.sync_scene(std::slice::from_ref(&child_map));
        let parts = cache.render_parts(&child_map);
        assert!(!parts.is_empty());
        assert!(
            parts
                .iter()
                .any(|part| part.retained_chunk_id.contains(":parent:"))
        );
    }

    #[test]
    fn child_tile_is_retained_while_zoomed_parent_loads() {
        let mut cache = MapTileCache::default();
        let child_map = map(1, 2.0);
        cache.sync_scene(std::slice::from_ref(&child_map));
        complete_first_request(&mut cache);
        let parent_map = map(2, 1.0);
        cache.sync_scene(std::slice::from_ref(&parent_map));
        let parts = cache.render_parts(&parent_map);
        assert!(
            parts
                .iter()
                .any(|part| part.retained_chunk_id.contains(":child:"))
        );
    }

    #[test]
    fn raster_decode_and_origin_checks_are_explicit_and_bounded() {
        use image::ImageEncoder;

        let identity = map(1, 2.0).visible_tiles[0].request.clone();
        let request = MapTileFetchRequest {
            viewport: DocumentNodeId("generic-map".to_owned()),
            identity,
            url_template_capability: "fixture_tiles".to_owned(),
            allowed_origins: vec!["https://tiles.example.test".to_owned()],
            expected_tile_size: 2,
        };
        assert!(request.accepts_origin("https://tiles.example.test"));
        assert!(!request.accepts_origin("https://other.example.test"));
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&[12, 34, 56, 255].repeat(4), 2, 2, image::ColorType::Rgba8)
            .unwrap();
        let decoded = decode_map_tile_bytes(&request, "image/png", &encoded).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(decoded.rgba.len(), 16);
        assert!(decode_map_tile_bytes(&request, "image/svg+xml", &encoded).is_err());
    }
}
