use crate::{MapHitIdentity, MapOverlayId, MapTileSourceId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;
use std::fmt;

pub const WEB_MERCATOR_MAX_LATITUDE: f64 = 85.051_128_779_806_6;
pub const MAX_XYZ_ZOOM: u8 = 30;
pub const MAX_MAP_TILE_OVERSCAN: u8 = 4;
pub const MAX_VISIBLE_XYZ_TILE_COUNT: usize = 16_384;
pub const MAX_MAP_VIEWPORT_EXTENT: f64 = 32_768.0;
pub const MAX_MAP_VIEWPORT_SCALE: f64 = 16.0;
pub const MAX_MAP_TILE_ALLOWED_ORIGINS: usize = 32;
pub const MAX_MAP_OVERLAY_COUNT: usize = 100_000;
pub const MAX_MAP_OVERLAY_GEOMETRY_POINTS: usize = 1_000_000;
pub const MAX_MAP_LABEL_BYTES: usize = 4_096;
pub const MAX_MAP_ATTRIBUTION_BYTES: usize = 2_048;
pub const MAX_MAP_TILE_CAPABILITY_BYTES: usize = 256;

const TILE_SCALE_UNITS: f64 = 1024.0;
const MIN_MAP_VIEWPORT_SCALE: f64 = 1.0 / TILE_SCALE_UNITS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapViewportDescriptor {
    pub generation: MapViewportGeneration,
    pub camera: MapCamera,
    pub bounds: MapViewportBounds,
    pub tile_source: MapTileSourceRef,
    pub interaction: MapInteractionPolicy,
    pub overlays: Vec<MapOverlayDescriptor>,
}

impl MapViewportDescriptor {
    pub fn validate(&self) -> Result<(), MapViewportError> {
        validate_finite("camera.longitude", self.camera.longitude)?;
        validate_latitude("camera.latitude", self.camera.latitude)?;
        validate_finite("camera.zoom", self.camera.zoom)?;
        validate_finite("camera.bearing", self.camera.bearing)?;
        validate_positive_extent("bounds.width", self.bounds.width)?;
        validate_positive_extent("bounds.height", self.bounds.height)?;
        validate_inclusive_number(
            "bounds.scale",
            self.bounds.scale,
            MIN_MAP_VIEWPORT_SCALE,
            MAX_MAP_VIEWPORT_SCALE,
        )?;
        self.tile_source.validate()?;
        if self.camera.zoom < f64::from(self.tile_source.min_zoom)
            || self.camera.zoom > f64::from(self.tile_source.max_zoom)
        {
            return Err(MapViewportError::new(
                "camera.zoom",
                format!(
                    "must be within tile source zoom range {}..={}",
                    self.tile_source.min_zoom, self.tile_source.max_zoom
                ),
            ));
        }
        validate_overlays(&self.overlays)
    }

    pub fn retained_patch_to(
        &self,
        next: &Self,
    ) -> Result<MapViewportDescriptorPatch, MapViewportError> {
        self.validate()?;
        next.validate()?;
        Ok(MapViewportDescriptorPatch {
            generation: (self.generation != next.generation).then_some(next.generation),
            camera: (self.camera != next.camera).then_some(next.camera),
            bounds: (self.bounds != next.bounds).then_some(next.bounds),
            tile_source: (self.tile_source != next.tile_source).then_some(next.tile_source.clone()),
            interaction: (self.interaction != next.interaction).then_some(next.interaction),
            overlays: MapOverlayPatchSet::between(&self.overlays, &next.overlays)?,
        })
    }

    pub fn visible_xyz_tile_requests(
        &self,
        overscan: u8,
    ) -> Result<Vec<MapTileRequestIdentity>, MapViewportError> {
        self.validate()?;
        if overscan > MAX_MAP_TILE_OVERSCAN {
            return Err(MapViewportError::new(
                "overscan",
                format!("must be at most {MAX_MAP_TILE_OVERSCAN}"),
            ));
        }

        let z = self.camera.zoom.floor() as u8;
        let tile_count = 1_i64
            .checked_shl(u32::from(z))
            .ok_or_else(|| MapViewportError::new("camera.zoom", "XYZ zoom is too large"))?;
        let projected = project_web_mercator(MapCoordinate {
            longitude: self.camera.longitude,
            latitude: self.camera.latitude,
        })?;
        let center_x = projected.x * tile_count as f64;
        let center_y = projected.y * tile_count as f64;
        let fractional_scale = 2_f64.powf(self.camera.zoom - f64::from(z));
        let tile_screen_size = f64::from(self.tile_source.tile_size) * fractional_scale;
        let bearing = self.camera.bearing.rem_euclid(360.0).to_radians();
        let (sin, cos) = bearing.sin_cos();
        let half_width = self.bounds.width / 2.0;
        let half_height = self.bounds.height / 2.0;
        let projected_half_width = cos.abs() * half_width + sin.abs() * half_height;
        let projected_half_height = sin.abs() * half_width + cos.abs() * half_height;
        let horizontal_tiles = projected_half_width / tile_screen_size;
        let vertical_tiles = projected_half_height / tile_screen_size;
        let overscan = i64::from(overscan);

        let start_x = (center_x - horizontal_tiles).floor() as i64 - overscan;
        let end_x = (center_x + horizontal_tiles).ceil() as i64 + overscan;
        let start_y = ((center_y - vertical_tiles).floor() as i64 - overscan).max(0);
        let end_y = ((center_y + vertical_tiles).ceil() as i64 + overscan).min(tile_count);
        let x_count = (end_x - start_x).clamp(0, tile_count);
        let y_count = (end_y - start_y).max(0);
        let request_count = usize::try_from(x_count)
            .ok()
            .and_then(|x| usize::try_from(y_count).ok().and_then(|y| x.checked_mul(y)))
            .ok_or_else(|| {
                MapViewportError::new("tile_selection", "visible tile count overflowed")
            })?;
        if request_count > MAX_VISIBLE_XYZ_TILE_COUNT {
            return Err(MapViewportError::new(
                "tile_selection",
                format!("visible tile count {request_count} exceeds {MAX_VISIBLE_XYZ_TILE_COUNT}"),
            ));
        }

        let scale = MapTileScaleKey::from_viewport_scale(self.bounds.scale)?;
        let mut requests = Vec::with_capacity(request_count);
        for x_offset in 0..x_count {
            let x = (start_x + x_offset).rem_euclid(tile_count) as u32;
            for y in start_y..end_y {
                requests.push(MapTileRequestIdentity {
                    generation: self.generation,
                    tile: MapTileCacheKey {
                        source: self.tile_source.id.clone(),
                        z,
                        x,
                        y: y as u32,
                        scale,
                    },
                });
            }
        }
        requests.sort_unstable_by(|left, right| left.tile.cmp(&right.tile));
        Ok(requests)
    }

    pub fn visible_xyz_tiles(&self, overscan: u8) -> Result<Vec<MapVisibleTile>, MapViewportError> {
        self.visible_xyz_tile_requests(overscan)?
            .into_iter()
            .map(|request| {
                let screen_quad = self.tile_screen_quad(&request.tile)?;
                Ok(MapVisibleTile {
                    request,
                    screen_quad,
                })
            })
            .collect()
    }

    pub fn tile_screen_quad(
        &self,
        tile: &MapTileCacheKey,
    ) -> Result<MapTileScreenQuad, MapViewportError> {
        self.validate()?;
        if tile.source != self.tile_source.id {
            return Err(MapViewportError::new(
                "tile.source",
                "does not match the viewport tile source",
            ));
        }
        if tile.z < self.tile_source.min_zoom || tile.z > self.tile_source.max_zoom {
            return Err(MapViewportError::new(
                "tile.z",
                "is outside the viewport tile source zoom range",
            ));
        }
        let tile_count = 1_u64
            .checked_shl(u32::from(tile.z))
            .ok_or_else(|| MapViewportError::new("tile.z", "XYZ zoom is too large"))?;
        if u64::from(tile.x) >= tile_count || u64::from(tile.y) >= tile_count {
            return Err(MapViewportError::new(
                "tile.coordinate",
                "XYZ coordinate is outside its zoom level",
            ));
        }
        let expected_scale = MapTileScaleKey::from_viewport_scale(self.bounds.scale)?;
        if tile.scale != expected_scale {
            return Err(MapViewportError::new(
                "tile.scale",
                "does not match the viewport scale",
            ));
        }

        let center = project_web_mercator(MapCoordinate {
            longitude: self.camera.longitude,
            latitude: self.camera.latitude,
        })?;
        let count = tile_count as f64;
        let center_x = center.x * count;
        let center_y = center.y * count;
        let mut tile_x = f64::from(tile.x);
        let delta_x = tile_x - center_x;
        if delta_x > count / 2.0 {
            tile_x -= count;
        } else if delta_x < -count / 2.0 {
            tile_x += count;
        }
        let tile_size = f64::from(self.tile_source.tile_size)
            * 2_f64.powf(self.camera.zoom - f64::from(tile.z));
        let bearing = self.camera.bearing.rem_euclid(360.0).to_radians();
        let (sin, cos) = bearing.sin_cos();
        let project_corner = |x: f64, y: f64| {
            let world_x = (x - center_x) * tile_size;
            let world_y = (y - center_y) * tile_size;
            MapScreenPoint {
                x: self.bounds.width / 2.0 + world_x * cos - world_y * sin,
                y: self.bounds.height / 2.0 + world_x * sin + world_y * cos,
            }
        };
        let tile_y = f64::from(tile.y);
        Ok(MapTileScreenQuad {
            points: [
                project_corner(tile_x, tile_y),
                project_corner(tile_x + 1.0, tile_y),
                project_corner(tile_x + 1.0, tile_y + 1.0),
                project_corner(tile_x, tile_y + 1.0),
            ],
        })
    }

    pub fn patch_for_input(
        &self,
        input: MapViewportInput,
    ) -> Result<MapViewportDescriptorPatch, MapViewportError> {
        self.validate()?;
        let generation = self
            .generation
            .checked_next()
            .ok_or_else(|| MapViewportError::new("generation", "cannot advance beyond u64::MAX"))?;
        let mut next = self.clone();
        next.generation = generation;
        match input {
            MapViewportInput::PanPixels { delta_x, delta_y } => {
                if !self.interaction.pan {
                    return Err(MapViewportError::new("interaction.pan", "is disabled"));
                }
                validate_finite("input.delta_x", delta_x)?;
                validate_finite("input.delta_y", delta_y)?;
                let center = project_web_mercator(MapCoordinate {
                    longitude: self.camera.longitude,
                    latitude: self.camera.latitude,
                })?;
                let bearing = self.camera.bearing.rem_euclid(360.0).to_radians();
                let (sin, cos) = bearing.sin_cos();
                let world_size =
                    f64::from(self.tile_source.tile_size) * 2_f64.powf(self.camera.zoom);
                let world_x = delta_x * cos + delta_y * sin;
                let world_y = -delta_x * sin + delta_y * cos;
                let coordinate = unproject_web_mercator(WebMercatorPoint {
                    x: (center.x - world_x / world_size).rem_euclid(1.0),
                    y: (center.y - world_y / world_size).clamp(0.0, 1.0),
                })?;
                next.camera.longitude = coordinate.longitude;
                next.camera.latitude = coordinate
                    .latitude
                    .clamp(-WEB_MERCATOR_MAX_LATITUDE, WEB_MERCATOR_MAX_LATITUDE);
            }
            MapViewportInput::Zoom { delta } => {
                if !self.interaction.wheel_zoom
                    && !self.interaction.pinch_zoom
                    && !self.interaction.keyboard_zoom
                {
                    return Err(MapViewportError::new(
                        "interaction.zoom",
                        "all zoom inputs are disabled",
                    ));
                }
                validate_finite("input.delta", delta)?;
                next.camera.zoom = (self.camera.zoom + delta).clamp(
                    f64::from(self.tile_source.min_zoom),
                    f64::from(self.tile_source.max_zoom),
                );
            }
            MapViewportInput::ZoomAt { delta, anchor } => {
                if !self.interaction.wheel_zoom && !self.interaction.pinch_zoom {
                    return Err(MapViewportError::new(
                        "interaction.zoom_at",
                        "wheel and pinch zoom inputs are disabled",
                    ));
                }
                validate_finite("input.delta", delta)?;
                validate_finite("input.anchor.x", anchor.x)?;
                validate_finite("input.anchor.y", anchor.y)?;
                let anchored_coordinate = unproject_from_map_viewport(self, anchor)?;
                let anchored = project_web_mercator(anchored_coordinate)?;
                next.camera.zoom = (self.camera.zoom + delta).clamp(
                    f64::from(self.tile_source.min_zoom),
                    f64::from(self.tile_source.max_zoom),
                );
                let bearing = next.camera.bearing.rem_euclid(360.0).to_radians();
                let (sin, cos) = bearing.sin_cos();
                let screen_x = anchor.x - next.bounds.width / 2.0;
                let screen_y = anchor.y - next.bounds.height / 2.0;
                let world_x = screen_x * cos + screen_y * sin;
                let world_y = -screen_x * sin + screen_y * cos;
                let world_size =
                    f64::from(next.tile_source.tile_size) * 2_f64.powf(next.camera.zoom);
                let center = WebMercatorPoint {
                    x: (anchored.x - world_x / world_size).rem_euclid(1.0),
                    y: (anchored.y - world_y / world_size).clamp(0.0, 1.0),
                };
                let coordinate = unproject_web_mercator(center)?;
                next.camera.longitude = coordinate.longitude;
                next.camera.latitude = coordinate.latitude;
            }
            MapViewportInput::Resize { bounds } => {
                validate_positive_extent("input.bounds.width", bounds.width)?;
                validate_positive_extent("input.bounds.height", bounds.height)?;
                MapTileScaleKey::from_viewport_scale(bounds.scale)?;
                next.bounds = bounds;
            }
        }
        self.retained_patch_to(&next)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapCamera {
    pub longitude: f64,
    pub latitude: f64,
    pub zoom: f64,
    pub bearing: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapViewportBounds {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapInteractionPolicy {
    pub pan: bool,
    pub wheel_zoom: bool,
    pub pinch_zoom: bool,
    pub keyboard_zoom: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileSourceRef {
    pub id: MapTileSourceId,
    pub url_template_capability: String,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tile_size: u16,
    pub attribution: String,
    pub allowed_origins: Vec<String>,
}

impl MapTileSourceRef {
    pub fn validate(&self) -> Result<(), MapViewportError> {
        validate_nonempty("tile_source.id", &self.id.0)?;
        validate_nonempty(
            "tile_source.url_template_capability",
            &self.url_template_capability,
        )?;
        if self.url_template_capability.len() > MAX_MAP_TILE_CAPABILITY_BYTES
            || self.url_template_capability.contains("://")
            || self
                .url_template_capability
                .chars()
                .any(char::is_whitespace)
        {
            return Err(MapViewportError::new(
                "tile_source.url_template_capability",
                format!(
                    "must be an opaque capability name of at most {MAX_MAP_TILE_CAPABILITY_BYTES} bytes, not a URL"
                ),
            ));
        }
        if self.min_zoom > self.max_zoom {
            return Err(MapViewportError::new(
                "tile_source.min_zoom",
                "must not exceed max_zoom",
            ));
        }
        if self.max_zoom > MAX_XYZ_ZOOM {
            return Err(MapViewportError::new(
                "tile_source.max_zoom",
                format!("must be at most {MAX_XYZ_ZOOM}"),
            ));
        }
        if !(64..=4096).contains(&self.tile_size) || !self.tile_size.is_power_of_two() {
            return Err(MapViewportError::new(
                "tile_source.tile_size",
                "must be a power of two in 64..=4096",
            ));
        }
        validate_nonempty("tile_source.attribution", &self.attribution)?;
        if self.attribution.len() > MAX_MAP_ATTRIBUTION_BYTES {
            return Err(MapViewportError::new(
                "tile_source.attribution",
                format!("must contain at most {MAX_MAP_ATTRIBUTION_BYTES} UTF-8 bytes"),
            ));
        }
        if self.allowed_origins.is_empty() {
            return Err(MapViewportError::new(
                "tile_source.allowed_origins",
                "must contain at least one origin",
            ));
        }
        if self.allowed_origins.len() > MAX_MAP_TILE_ALLOWED_ORIGINS {
            return Err(MapViewportError::new(
                "tile_source.allowed_origins",
                format!("must contain at most {MAX_MAP_TILE_ALLOWED_ORIGINS} origins"),
            ));
        }
        let mut origins = BTreeSet::new();
        for (index, origin) in self.allowed_origins.iter().enumerate() {
            validate_nonempty(&format!("tile_source.allowed_origins[{index}]"), origin)?;
            let explicit_authority = origin.split_once("://").is_some_and(|(scheme, authority)| {
                !scheme.is_empty() && !authority.is_empty() && !authority.contains('/')
            });
            if origin == "*"
                || !explicit_authority
                || origin.contains('{')
                || origin.contains('}')
                || origin.contains('#')
                || origin.contains('?')
            {
                return Err(MapViewportError::new(
                    format!("tile_source.allowed_origins[{index}]"),
                    "must be an explicit scheme-and-authority origin without wildcards, templates, paths, queries, or fragments",
                ));
            }
            if !origins.insert(origin) {
                return Err(MapViewportError::new(
                    format!("tile_source.allowed_origins[{index}]"),
                    format!("duplicates origin `{origin}`"),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapOverlayDescriptor {
    pub id: MapOverlayId,
    pub hit_identity: MapHitIdentity,
    pub z_order: i32,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub paint: MapOverlayPaint,
    pub geometry: MapOverlayGeometry,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapOverlayGeometry {
    Point {
        position: MapCoordinate,
        radius: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        symbol_ref: Option<String>,
    },
    Cluster {
        position: MapCoordinate,
        count: u64,
        radius: f64,
    },
    Polyline {
        points: Vec<MapCoordinate>,
    },
    Polygon {
        rings: Vec<Vec<MapCoordinate>>,
    },
    Label {
        position: MapCoordinate,
        text: String,
        collision_priority: i32,
        font_size: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapOverlayPaint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    pub stroke_width: f64,
    pub opacity: f64,
}

impl Default for MapOverlayPaint {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapCoordinate {
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebMercatorPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapScreenPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapTileScreenQuad {
    pub points: [MapScreenPoint; 4],
}

impl MapTileScreenQuad {
    pub fn bounds(self) -> MapScreenRect {
        let min_x = self
            .points
            .iter()
            .map(|point| point.x)
            .fold(f64::INFINITY, f64::min);
        let min_y = self
            .points
            .iter()
            .map(|point| point.y)
            .fold(f64::INFINITY, f64::min);
        let max_x = self
            .points
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = self
            .points
            .iter()
            .map(|point| point.y)
            .fold(f64::NEG_INFINITY, f64::max);
        MapScreenRect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapVisibleTile {
    pub request: MapTileRequestIdentity,
    pub screen_quad: MapTileScreenQuad,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapViewportInput {
    PanPixels { delta_x: f64, delta_y: f64 },
    Zoom { delta: f64 },
    ZoomAt { delta: f64, anchor: MapScreenPoint },
    Resize { bounds: MapViewportBounds },
}

pub fn project_web_mercator(
    coordinate: MapCoordinate,
) -> Result<WebMercatorPoint, MapViewportError> {
    validate_coordinate("coordinate", coordinate)?;
    let x = ((coordinate.longitude + 180.0) / 360.0).rem_euclid(1.0);
    let latitude = coordinate.latitude.to_radians();
    let y = (1.0 - latitude.tan().asinh() / PI) / 2.0;
    Ok(WebMercatorPoint { x, y })
}

pub fn unproject_web_mercator(point: WebMercatorPoint) -> Result<MapCoordinate, MapViewportError> {
    validate_finite("point.x", point.x)?;
    validate_inclusive_number("point.y", point.y, 0.0, 1.0)?;
    let x = point.x.rem_euclid(1.0);
    let longitude = x * 360.0 - 180.0;
    let latitude = (PI * (1.0 - 2.0 * point.y)).sinh().atan().to_degrees();
    Ok(MapCoordinate {
        longitude,
        latitude,
    })
}

pub fn project_to_map_viewport(
    descriptor: &MapViewportDescriptor,
    coordinate: MapCoordinate,
) -> Result<MapScreenPoint, MapViewportError> {
    descriptor.validate()?;
    let center = project_web_mercator(MapCoordinate {
        longitude: descriptor.camera.longitude,
        latitude: descriptor.camera.latitude,
    })?;
    let point = project_web_mercator(coordinate)?;
    let mut delta_x = point.x - center.x;
    if delta_x > 0.5 {
        delta_x -= 1.0;
    } else if delta_x < -0.5 {
        delta_x += 1.0;
    }
    let world_size =
        f64::from(descriptor.tile_source.tile_size) * 2_f64.powf(descriptor.camera.zoom);
    let delta_y = point.y - center.y;
    let bearing = descriptor.camera.bearing.rem_euclid(360.0).to_radians();
    let (sin, cos) = bearing.sin_cos();
    let world_x = delta_x * world_size;
    let world_y = delta_y * world_size;
    Ok(MapScreenPoint {
        x: descriptor.bounds.width / 2.0 + world_x * cos - world_y * sin,
        y: descriptor.bounds.height / 2.0 + world_x * sin + world_y * cos,
    })
}

pub fn unproject_from_map_viewport(
    descriptor: &MapViewportDescriptor,
    point: MapScreenPoint,
) -> Result<MapCoordinate, MapViewportError> {
    descriptor.validate()?;
    validate_finite("point.x", point.x)?;
    validate_finite("point.y", point.y)?;
    let center = project_web_mercator(MapCoordinate {
        longitude: descriptor.camera.longitude,
        latitude: descriptor.camera.latitude,
    })?;
    let screen_x = point.x - descriptor.bounds.width / 2.0;
    let screen_y = point.y - descriptor.bounds.height / 2.0;
    let bearing = descriptor.camera.bearing.rem_euclid(360.0).to_radians();
    let (sin, cos) = bearing.sin_cos();
    let world_x = screen_x * cos + screen_y * sin;
    let world_y = -screen_x * sin + screen_y * cos;
    let world_size =
        f64::from(descriptor.tile_source.tile_size) * 2_f64.powf(descriptor.camera.zoom);
    unproject_web_mercator(WebMercatorPoint {
        x: (center.x + world_x / world_size).rem_euclid(1.0),
        y: (center.y + world_y / world_size).clamp(0.0, 1.0),
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MapViewportGeneration(pub u64);

impl MapViewportGeneration {
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MapTileScaleKey(u16);

impl MapTileScaleKey {
    pub fn from_viewport_scale(scale: f64) -> Result<Self, MapViewportError> {
        validate_inclusive_number(
            "bounds.scale",
            scale,
            MIN_MAP_VIEWPORT_SCALE,
            MAX_MAP_VIEWPORT_SCALE,
        )?;
        Ok(Self((scale * TILE_SCALE_UNITS).round() as u16))
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / TILE_SCALE_UNITS
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MapTileCacheKey {
    pub source: MapTileSourceId,
    pub z: u8,
    pub x: u32,
    pub y: u32,
    pub scale: MapTileScaleKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapTileRequestIdentity {
    pub generation: MapViewportGeneration,
    pub tile: MapTileCacheKey,
}

impl MapTileRequestIdentity {
    pub fn freshness(&self, active_generation: MapViewportGeneration) -> MapTileResultFreshness {
        match self.generation.cmp(&active_generation) {
            std::cmp::Ordering::Less => MapTileResultFreshness::Stale,
            std::cmp::Ordering::Equal => MapTileResultFreshness::Current,
            std::cmp::Ordering::Greater => MapTileResultFreshness::Future,
        }
    }

    pub fn should_cancel(&self, active_generation: MapViewportGeneration) -> bool {
        self.freshness(active_generation) != MapTileResultFreshness::Current
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapTileResultFreshness {
    Current,
    Stale,
    Future,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapViewportDescriptorPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<MapViewportGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<MapCamera>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<MapViewportBounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_source: Option<MapTileSourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<MapInteractionPolicy>,
    pub overlays: MapOverlayPatchSet,
}

impl MapViewportDescriptorPatch {
    pub fn is_empty(&self) -> bool {
        self.generation.is_none()
            && self.camera.is_none()
            && self.bounds.is_none()
            && self.tile_source.is_none()
            && self.interaction.is_none()
            && self.overlays.is_empty()
    }

    pub fn apply_to(&self, descriptor: &mut MapViewportDescriptor) -> Result<(), MapViewportError> {
        descriptor.validate()?;
        let mut next = descriptor.clone();
        if let Some(generation) = self.generation {
            next.generation = generation;
        }
        if let Some(camera) = self.camera {
            next.camera = camera;
        }
        if let Some(bounds) = self.bounds {
            next.bounds = bounds;
        }
        if let Some(tile_source) = &self.tile_source {
            next.tile_source = tile_source.clone();
        }
        if let Some(interaction) = self.interaction {
            next.interaction = interaction;
        }
        self.overlays.apply_to(&mut next.overlays)?;
        next.validate()?;
        *descriptor = next;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapOverlayPatchSet {
    pub removed: Vec<MapOverlayId>,
    pub upserted: Vec<MapOverlayDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<MapOverlayId>>,
}

impl MapOverlayPatchSet {
    pub fn between(
        previous: &[MapOverlayDescriptor],
        next: &[MapOverlayDescriptor],
    ) -> Result<Self, MapViewportError> {
        validate_overlays(previous)?;
        validate_overlays(next)?;
        let previous_by_id = previous
            .iter()
            .map(|overlay| (&overlay.id, overlay))
            .collect::<BTreeMap<_, _>>();
        let next_by_id = next
            .iter()
            .map(|overlay| (&overlay.id, overlay))
            .collect::<BTreeMap<_, _>>();
        let removed = previous
            .iter()
            .filter(|overlay| !next_by_id.contains_key(&overlay.id))
            .map(|overlay| overlay.id.clone())
            .collect();
        let upserted = next
            .iter()
            .filter(|overlay| previous_by_id.get(&overlay.id).copied() != Some(*overlay))
            .cloned()
            .collect();
        let previous_order = previous
            .iter()
            .map(|overlay| overlay.id.clone())
            .collect::<Vec<_>>();
        let next_order = next
            .iter()
            .map(|overlay| overlay.id.clone())
            .collect::<Vec<_>>();
        Ok(Self {
            removed,
            upserted,
            order: (previous_order != next_order).then_some(next_order),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.upserted.is_empty() && self.order.is_none()
    }

    pub fn apply_to(
        &self,
        overlays: &mut Vec<MapOverlayDescriptor>,
    ) -> Result<(), MapViewportError> {
        validate_overlays(overlays)?;
        let removed = self.removed.iter().collect::<BTreeSet<_>>();
        if removed.len() != self.removed.len() {
            return Err(MapViewportError::new(
                "overlays.removed",
                "contains duplicate ids",
            ));
        }
        overlays.retain(|overlay| !removed.contains(&overlay.id));
        for upsert in &self.upserted {
            if let Some(existing) = overlays.iter_mut().find(|entry| entry.id == upsert.id) {
                *existing = upsert.clone();
            } else {
                overlays.push(upsert.clone());
            }
        }
        if let Some(order) = &self.order {
            let order_set = order.iter().collect::<BTreeSet<_>>();
            let overlay_set = overlays
                .iter()
                .map(|overlay| &overlay.id)
                .collect::<BTreeSet<_>>();
            if order_set.len() != order.len() || order_set != overlay_set {
                return Err(MapViewportError::new(
                    "overlays.order",
                    "must contain every resulting overlay id exactly once",
                ));
            }
            let mut by_id = std::mem::take(overlays)
                .into_iter()
                .map(|overlay| (overlay.id.clone(), overlay))
                .collect::<BTreeMap<_, _>>();
            overlays.extend(order.iter().filter_map(|id| by_id.remove(id)));
        }
        validate_overlays(overlays)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MapViewportError {
    pub path: String,
    pub message: String,
}

impl MapViewportError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for MapViewportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MapViewport {}: {}", self.path, self.message)
    }
}

impl std::error::Error for MapViewportError {}

fn validate_overlays(overlays: &[MapOverlayDescriptor]) -> Result<(), MapViewportError> {
    if overlays.len() > MAX_MAP_OVERLAY_COUNT {
        return Err(MapViewportError::new(
            "overlays",
            format!("must contain at most {MAX_MAP_OVERLAY_COUNT} entries"),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut geometry_points = 0usize;
    for (index, overlay) in overlays.iter().enumerate() {
        let path = format!("overlays[{index}]");
        validate_nonempty(&format!("{path}.id"), &overlay.id.0)?;
        if !ids.insert(&overlay.id) {
            return Err(MapViewportError::new(
                format!("{path}.id"),
                format!("duplicates overlay id `{}`", overlay.id.0),
            ));
        }
        validate_nonempty(&format!("{path}.hit_identity"), &overlay.hit_identity.0)?;
        if let Some(fill) = &overlay.paint.fill {
            validate_nonempty(&format!("{path}.paint.fill"), fill)?;
        }
        if let Some(stroke) = &overlay.paint.stroke {
            validate_nonempty(&format!("{path}.paint.stroke"), stroke)?;
        }
        validate_inclusive_number(
            &format!("{path}.paint.stroke_width"),
            overlay.paint.stroke_width,
            0.0,
            f64::MAX,
        )?;
        validate_inclusive_number(
            &format!("{path}.paint.opacity"),
            overlay.paint.opacity,
            0.0,
            1.0,
        )?;
        match &overlay.geometry {
            MapOverlayGeometry::Point {
                position,
                radius,
                symbol_ref,
            } => {
                geometry_points = geometry_points.saturating_add(1);
                validate_coordinate(&format!("{path}.position"), *position)?;
                validate_positive(&format!("{path}.radius"), *radius)?;
                if let Some(symbol_ref) = symbol_ref {
                    validate_nonempty(&format!("{path}.symbol_ref"), symbol_ref)?;
                }
            }
            MapOverlayGeometry::Cluster {
                position,
                count,
                radius,
            } => {
                geometry_points = geometry_points.saturating_add(1);
                validate_coordinate(&format!("{path}.position"), *position)?;
                if *count == 0 {
                    return Err(MapViewportError::new(
                        format!("{path}.count"),
                        "must be at least 1",
                    ));
                }
                validate_positive(&format!("{path}.radius"), *radius)?;
            }
            MapOverlayGeometry::Polyline { points } => {
                geometry_points = geometry_points.saturating_add(points.len());
                if points.len() < 2 {
                    return Err(MapViewportError::new(
                        format!("{path}.points"),
                        "must contain at least 2 positions",
                    ));
                }
                for (point_index, point) in points.iter().enumerate() {
                    validate_coordinate(&format!("{path}.points[{point_index}]"), *point)?;
                }
            }
            MapOverlayGeometry::Polygon { rings } => {
                if rings.is_empty() {
                    return Err(MapViewportError::new(
                        format!("{path}.rings"),
                        "must contain at least one ring",
                    ));
                }
                for (ring_index, ring) in rings.iter().enumerate() {
                    geometry_points = geometry_points.saturating_add(ring.len());
                    if ring.len() < 3 {
                        return Err(MapViewportError::new(
                            format!("{path}.rings[{ring_index}]"),
                            "must contain at least 3 positions",
                        ));
                    }
                    for (point_index, point) in ring.iter().enumerate() {
                        validate_coordinate(
                            &format!("{path}.rings[{ring_index}][{point_index}]"),
                            *point,
                        )?;
                    }
                }
            }
            MapOverlayGeometry::Label {
                position,
                text,
                font_size,
                ..
            } => {
                geometry_points = geometry_points.saturating_add(1);
                validate_coordinate(&format!("{path}.position"), *position)?;
                validate_nonempty(&format!("{path}.text"), text)?;
                if text.len() > MAX_MAP_LABEL_BYTES {
                    return Err(MapViewportError::new(
                        format!("{path}.text"),
                        format!("must contain at most {MAX_MAP_LABEL_BYTES} UTF-8 bytes"),
                    ));
                }
                validate_positive(&format!("{path}.font_size"), *font_size)?;
            }
        }
        if geometry_points > MAX_MAP_OVERLAY_GEOMETRY_POINTS {
            return Err(MapViewportError::new(
                "overlays",
                format!(
                    "geometry must contain at most {MAX_MAP_OVERLAY_GEOMETRY_POINTS} positions"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_coordinate(path: &str, coordinate: MapCoordinate) -> Result<(), MapViewportError> {
    validate_finite(&format!("{path}.longitude"), coordinate.longitude)?;
    validate_latitude(&format!("{path}.latitude"), coordinate.latitude)
}

fn validate_latitude(path: &str, latitude: f64) -> Result<(), MapViewportError> {
    validate_inclusive_number(
        path,
        latitude,
        -WEB_MERCATOR_MAX_LATITUDE,
        WEB_MERCATOR_MAX_LATITUDE,
    )
}

fn validate_positive_extent(path: &str, value: f64) -> Result<(), MapViewportError> {
    validate_inclusive_number(path, value, f64::MIN_POSITIVE, MAX_MAP_VIEWPORT_EXTENT)
}

fn validate_positive(path: &str, value: f64) -> Result<(), MapViewportError> {
    validate_inclusive_number(path, value, f64::MIN_POSITIVE, f64::MAX)
}

fn validate_finite(path: &str, value: f64) -> Result<(), MapViewportError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(MapViewportError::new(path, "must be finite"))
    }
}

fn validate_inclusive_number(
    path: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), MapViewportError> {
    validate_finite(path, value)?;
    if value < minimum || value > maximum {
        return Err(MapViewportError::new(
            path,
            format!("must be within {minimum}..={maximum}"),
        ));
    }
    Ok(())
}

fn validate_nonempty(path: &str, value: &str) -> Result<(), MapViewportError> {
    if value.trim().is_empty() {
        Err(MapViewportError::new(path, "must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentNode, DocumentNodeKind};

    fn point(id: &str, longitude: f64) -> MapOverlayDescriptor {
        MapOverlayDescriptor {
            id: MapOverlayId(id.to_owned()),
            hit_identity: MapHitIdentity(format!("hit-{id}")),
            z_order: 2,
            selected: false,
            focused: false,
            paint: MapOverlayPaint {
                fill: Some("#267a66".to_owned()),
                ..MapOverlayPaint::default()
            },
            geometry: MapOverlayGeometry::Point {
                position: MapCoordinate {
                    longitude,
                    latitude: 0.0,
                },
                radius: 6.0,
                symbol_ref: None,
            },
        }
    }

    fn descriptor() -> MapViewportDescriptor {
        MapViewportDescriptor {
            generation: MapViewportGeneration(7),
            camera: MapCamera {
                longitude: 0.0,
                latitude: 0.0,
                zoom: 2.0,
                bearing: 0.0,
            },
            bounds: MapViewportBounds {
                width: 256.0,
                height: 256.0,
                scale: 1.25,
            },
            tile_source: MapTileSourceRef {
                id: MapTileSourceId("fixture".to_owned()),
                url_template_capability: "fixture_xyz".to_owned(),
                min_zoom: 0,
                max_zoom: 6,
                tile_size: 256,
                attribution: "Fixture tiles".to_owned(),
                allowed_origins: vec!["boon-local://tiles".to_owned()],
            },
            interaction: MapInteractionPolicy {
                pan: true,
                wheel_zoom: true,
                pinch_zoom: true,
                keyboard_zoom: true,
            },
            overlays: vec![point("alpha", 0.0), point("beta", 1.0)],
        }
    }

    #[test]
    fn descriptor_validation_is_path_specific_and_deterministic() {
        let mut invalid = descriptor();
        invalid.camera.bearing = f64::NAN;
        assert_eq!(
            invalid.validate().unwrap_err(),
            MapViewportError::new("camera.bearing", "must be finite")
        );

        let mut duplicate = descriptor();
        duplicate.overlays[1].id = duplicate.overlays[0].id.clone();
        assert_eq!(duplicate.validate().unwrap_err().path, "overlays[1].id");

        let mut wildcard = descriptor();
        wildcard.tile_source.allowed_origins = vec!["*".to_owned()];
        assert_eq!(
            wildcard.validate().unwrap_err().path,
            "tile_source.allowed_origins[0]"
        );
    }

    #[test]
    fn web_mercator_helpers_round_trip_and_project_the_origin() {
        let projected = project_web_mercator(MapCoordinate {
            longitude: 0.0,
            latitude: 0.0,
        })
        .unwrap();
        assert!((projected.x - 0.5).abs() < 1e-12);
        assert!((projected.y - 0.5).abs() < 1e-12);
        let restored = unproject_web_mercator(projected).unwrap();
        assert!(restored.longitude.abs() < 1e-12);
        assert!(restored.latitude.abs() < 1e-12);

        let screen = project_to_map_viewport(
            &descriptor(),
            MapCoordinate {
                longitude: 0.0,
                latitude: 0.0,
            },
        )
        .unwrap();
        assert_eq!(screen, MapScreenPoint { x: 128.0, y: 128.0 });

        let mut rotated = descriptor();
        rotated.camera.bearing = 27.0;
        let coordinate = MapCoordinate {
            longitude: 1.25,
            latitude: 0.75,
        };
        let screen = project_to_map_viewport(&rotated, coordinate).unwrap();
        let round_trip = unproject_from_map_viewport(&rotated, screen).unwrap();
        assert!((round_trip.longitude - coordinate.longitude).abs() < 1e-9);
        assert!((round_trip.latitude - coordinate.latitude).abs() < 1e-9);
    }

    #[test]
    fn visible_xyz_selection_is_stable_wrapped_and_overscan_bounded() {
        let descriptor = descriptor();
        let requests = descriptor.visible_xyz_tile_requests(0).unwrap();
        let coordinates = requests
            .iter()
            .map(|request| (request.tile.z, request.tile.x, request.tile.y))
            .collect::<Vec<_>>();
        assert_eq!(
            coordinates,
            vec![(2, 1, 1), (2, 1, 2), (2, 2, 1), (2, 2, 2)]
        );
        assert!(
            requests
                .iter()
                .all(|request| request.generation == MapViewportGeneration(7))
        );
        assert_eq!(requests[0].tile.scale.as_f64(), 1.25);

        let mut wrapped = descriptor;
        wrapped.camera.longitude = 179.9;
        let wrapped_x = wrapped
            .visible_xyz_tile_requests(0)
            .unwrap()
            .into_iter()
            .map(|request| request.tile.x)
            .collect::<BTreeSet<_>>();
        assert_eq!(wrapped_x, BTreeSet::from([0, 3]));
        assert_eq!(
            wrapped
                .visible_xyz_tile_requests(MAX_MAP_TILE_OVERSCAN + 1)
                .unwrap_err()
                .path,
            "overscan"
        );
    }

    #[test]
    fn retained_overlay_patch_uses_ids_and_only_upserts_changes() {
        let previous = descriptor();
        let mut next = previous.clone();
        next.generation = MapViewportGeneration(8);
        next.camera.longitude = 2.0;
        next.overlays.swap(0, 1);
        next.overlays[0].selected = true;

        let patch = previous.retained_patch_to(&next).unwrap();
        assert_eq!(patch.generation, Some(MapViewportGeneration(8)));
        assert_eq!(patch.camera, Some(next.camera));
        assert_eq!(patch.overlays.removed, Vec::<MapOverlayId>::new());
        assert_eq!(patch.overlays.upserted, vec![next.overlays[0].clone()]);
        assert_eq!(
            patch.overlays.order,
            Some(vec![
                MapOverlayId("beta".to_owned()),
                MapOverlayId("alpha".to_owned())
            ])
        );
    }

    #[test]
    fn request_generation_rejects_stale_and_future_results() {
        let request = descriptor().visible_xyz_tile_requests(0).unwrap().remove(0);
        assert_eq!(
            request.freshness(MapViewportGeneration(7)),
            MapTileResultFreshness::Current
        );
        assert_eq!(
            request.freshness(MapViewportGeneration(8)),
            MapTileResultFreshness::Stale
        );
        assert_eq!(
            request.freshness(MapViewportGeneration(6)),
            MapTileResultFreshness::Future
        );
        assert!(request.should_cancel(MapViewportGeneration(8)));
        assert_eq!(
            MapViewportGeneration(7).checked_next(),
            Some(MapViewportGeneration(8))
        );
        assert_eq!(MapViewportGeneration(u64::MAX).checked_next(), None);
    }

    #[test]
    fn visible_tiles_have_stable_screen_quads_and_rotate_with_camera() {
        let descriptor = descriptor();
        let visible = descriptor.visible_xyz_tiles(1).unwrap();
        assert!(!visible.is_empty());
        assert!(visible.iter().all(|tile| {
            let bounds = tile.screen_quad.bounds();
            bounds.width.is_finite()
                && bounds.height.is_finite()
                && bounds.width > 0.0
                && bounds.height > 0.0
        }));

        let center_request = visible
            .iter()
            .min_by(|left, right| {
                let left = left.screen_quad.bounds();
                let right = right.screen_quad.bounds();
                let left_distance = (left.x + left.width / 2.0 - descriptor.bounds.width / 2.0)
                    .abs()
                    + (left.y + left.height / 2.0 - descriptor.bounds.height / 2.0).abs();
                let right_distance = (right.x + right.width / 2.0 - descriptor.bounds.width / 2.0)
                    .abs()
                    + (right.y + right.height / 2.0 - descriptor.bounds.height / 2.0).abs();
                left_distance.total_cmp(&right_distance)
            })
            .unwrap();
        let mut rotated = descriptor.clone();
        rotated.camera.bearing = 35.0;
        let rotated_quad = rotated
            .tile_screen_quad(&center_request.request.tile)
            .unwrap();
        assert_ne!(rotated_quad, center_request.screen_quad);
    }

    #[test]
    fn retained_input_and_overlay_patches_apply_without_replacing_descriptor() {
        let mut descriptor = descriptor();
        let original_overlay = descriptor.overlays[0].clone();
        let pan = descriptor
            .patch_for_input(MapViewportInput::PanPixels {
                delta_x: 24.0,
                delta_y: -12.0,
            })
            .unwrap();
        assert!(pan.camera.is_some());
        assert_eq!(pan.generation, Some(MapViewportGeneration(8)));
        pan.apply_to(&mut descriptor).unwrap();
        assert_eq!(descriptor.generation, MapViewportGeneration(8));

        let mut selected = original_overlay;
        selected.selected = true;
        let patch = MapViewportDescriptorPatch {
            generation: Some(MapViewportGeneration(9)),
            overlays: MapOverlayPatchSet {
                upserted: vec![selected.clone()],
                ..MapOverlayPatchSet::default()
            },
            ..MapViewportDescriptorPatch::default()
        };
        patch.apply_to(&mut descriptor).unwrap();
        assert_eq!(descriptor.overlays[0], selected);
        assert_eq!(descriptor.generation, MapViewportGeneration(9));

        let anchor = MapScreenPoint { x: 42.0, y: 71.0 };
        let before = unproject_from_map_viewport(&descriptor, anchor).unwrap();
        let zoom = descriptor
            .patch_for_input(MapViewportInput::ZoomAt { delta: 1.0, anchor })
            .unwrap();
        zoom.apply_to(&mut descriptor).unwrap();
        let after = unproject_from_map_viewport(&descriptor, anchor).unwrap();
        assert!((before.longitude - after.longitude).abs() < 1e-9);
        assert!((before.latitude - after.latitude).abs() < 1e-9);
    }

    #[test]
    fn malformed_overlay_order_fails_closed() {
        let mut descriptor = descriptor();
        let error = MapViewportDescriptorPatch {
            overlays: MapOverlayPatchSet {
                order: Some(Vec::new()),
                ..MapOverlayPatchSet::default()
            },
            ..MapViewportDescriptorPatch::default()
        }
        .apply_to(&mut descriptor)
        .unwrap_err();
        assert_eq!(error.path, "overlays.order");
    }

    #[test]
    fn descriptor_has_a_stable_toml_round_trip() {
        let descriptor = descriptor();
        let encoded = toml::to_string(&descriptor).unwrap();
        let decoded: MapViewportDescriptor = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, descriptor);
        decoded.validate().unwrap();

        let mut node = DocumentNode::new("map", DocumentNodeKind::MapViewport);
        node.map_viewport = Some(Box::new(descriptor));
        let encoded_node = toml::to_string(&node).unwrap();
        let decoded_node: DocumentNode = toml::from_str(&encoded_node).unwrap();
        assert_eq!(decoded_node, node);
    }
}
