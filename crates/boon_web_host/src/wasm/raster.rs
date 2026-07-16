use super::network::{BrowserFetchAdapter, BrowserFetchCancellation};
use super::{js_error, window};
use crate::{
    BrowserFetchCapabilities, BrowserMapTileCapabilities, BrowserMapTileTemplateCapability,
    WebHostError, WebHostResult,
};
use boon_native_gpu::{DecodedMapTile, MapTileFetchRequest};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, ImageBitmap, OffscreenCanvas, OffscreenCanvasRenderingContext2d,
};

pub const DEFAULT_MAX_ENCODED_RASTER_TILE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAX_DECODED_RASTER_TILE_BYTES: usize = 64 * 1024 * 1024;

pub struct BrowserMapTileAdapter {
    templates: BrowserMapTileCapabilities,
    fetch: BrowserFetchAdapter,
    decoder: BrowserRasterTileDecoder,
    same_origin: String,
}

impl BrowserMapTileAdapter {
    pub fn new(
        templates: impl IntoIterator<Item = BrowserMapTileTemplateCapability>,
        fetch_capabilities: BrowserFetchCapabilities,
        max_in_flight: usize,
        decoder: BrowserRasterTileDecoder,
    ) -> WebHostResult<Self> {
        let templates = BrowserMapTileCapabilities::new(templates, &fetch_capabilities)?;
        let fetch = BrowserFetchAdapter::new(fetch_capabilities, max_in_flight)?;
        let same_origin = window()?
            .location()
            .origin()
            .map_err(|error| js_error("read browser origin for map tiles", error))?;
        Ok(Self {
            templates,
            fetch,
            decoder,
            same_origin,
        })
    }

    pub async fn fetch_and_decode(
        &self,
        request_id: u64,
        tile: MapTileFetchRequest,
        cancellation: &BrowserFetchCancellation,
    ) -> WebHostResult<DecodedMapTile> {
        let fetch_request =
            self.templates
                .build_fetch_request(request_id, &tile, &self.same_origin)?;
        let response = self
            .fetch
            .execute_cancellable(fetch_request, cancellation)
            .await?;
        if !(200..=299).contains(&response.status) {
            return Err(WebHostError::platform(
                "fetch raster tile",
                format!("server returned HTTP {}", response.status),
            ));
        }
        let media_type = response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("content-type"))
            .map(|header| header.value.as_str())
            .ok_or_else(|| WebHostError::InvalidInput {
                field: "raster tile Content-Type".to_owned(),
                reason: "response did not declare a media type".to_owned(),
            })?;
        if cancellation.is_cancelled() {
            return Err(WebHostError::platform(
                "decode raster tile",
                "request was cancelled before decode",
            ));
        }
        let decoded = self
            .decoder
            .decode(tile, media_type, &response.body)
            .await?;
        if cancellation.is_cancelled() {
            return Err(WebHostError::platform(
                "decode raster tile",
                "request became stale during decode",
            ));
        }
        Ok(decoded)
    }

    pub fn active_request_count(&self) -> usize {
        self.fetch.active_request_count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserRasterTileDecoder {
    max_encoded_bytes: usize,
    max_decoded_bytes: usize,
    allowed_media_types: Vec<String>,
}

impl BrowserRasterTileDecoder {
    pub fn new(
        max_encoded_bytes: usize,
        max_decoded_bytes: usize,
        allowed_media_types: impl IntoIterator<Item = String>,
    ) -> WebHostResult<Self> {
        if max_encoded_bytes == 0 {
            return Err(WebHostError::InvalidInput {
                field: "raster tile max_encoded_bytes".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        if max_decoded_bytes < 4 {
            return Err(WebHostError::InvalidInput {
                field: "raster tile max_decoded_bytes".to_owned(),
                reason: "must hold at least one RGBA pixel".to_owned(),
            });
        }
        let mut allowed_media_types = allowed_media_types.into_iter().collect::<Vec<_>>();
        allowed_media_types.sort();
        allowed_media_types.dedup();
        if allowed_media_types.is_empty()
            || allowed_media_types.iter().any(|media_type| {
                !matches!(
                    media_type.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                )
            })
        {
            return Err(WebHostError::InvalidInput {
                field: "raster tile media types".to_owned(),
                reason: "must be a non-empty subset of image/png, image/jpeg and image/webp"
                    .to_owned(),
            });
        }
        Ok(Self {
            max_encoded_bytes,
            max_decoded_bytes,
            allowed_media_types,
        })
    }

    pub fn standard() -> Self {
        Self::new(
            DEFAULT_MAX_ENCODED_RASTER_TILE_BYTES,
            DEFAULT_MAX_DECODED_RASTER_TILE_BYTES,
            ["image/png", "image/jpeg", "image/webp"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("standard browser raster tile limits are valid")
    }

    pub async fn decode(
        &self,
        request: MapTileFetchRequest,
        media_type: &str,
        encoded: &[u8],
    ) -> WebHostResult<DecodedMapTile> {
        let media_type = media_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if self
            .allowed_media_types
            .binary_search_by(|candidate| candidate.as_str().cmp(&media_type))
            .is_err()
        {
            return Err(WebHostError::CapabilityDenied {
                capability: request.url_template_capability,
                reason: format!("raster media type {media_type} is not allowed"),
            });
        }
        if encoded.is_empty() || encoded.len() > self.max_encoded_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "encoded raster tile".to_owned(),
                limit: self.max_encoded_bytes,
            });
        }
        let source = Uint8Array::from(encoded);
        let parts = Array::new();
        parts.push(source.as_ref());
        let options = BlobPropertyBag::new();
        options.set_type(&media_type);
        let blob = Blob::new_with_u8_array_sequence_and_options(parts.as_ref(), &options)
            .map_err(|error| js_error("create raster tile Blob", error))?;
        let bitmap = DecodedBitmap(
            JsFuture::from(
                window()?
                    .create_image_bitmap_with_blob(&blob)
                    .map_err(|error| js_error("start raster tile decode", error))?,
            )
            .await
            .map_err(|error| js_error("decode raster tile", error))?
            .dyn_into::<ImageBitmap>()
            .map_err(|error| js_error("cast decoded raster tile", error))?,
        );
        let width = bitmap.0.width();
        let height = bitmap.0.height();
        let expected = u32::from(request.expected_tile_size);
        if width != expected || height != expected {
            return Err(WebHostError::InvalidInput {
                field: "decoded raster tile dimensions".to_owned(),
                reason: format!("expected {expected}x{expected}, got {width}x{height}"),
            });
        }
        let expected_bytes = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| WebHostError::LimitExceeded {
                resource: "decoded raster tile".to_owned(),
                limit: usize::MAX,
            })?;
        if expected_bytes > self.max_decoded_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "decoded raster tile".to_owned(),
                limit: self.max_decoded_bytes,
            });
        }
        let canvas = OffscreenCanvas::new(width, height)
            .map_err(|error| js_error("create raster decode canvas", error))?;
        let context = canvas
            .get_context("2d")
            .map_err(|error| js_error("acquire raster decode context", error))?
            .ok_or_else(|| {
                WebHostError::unsupported(
                    "OffscreenCanvas 2D",
                    "browser returned no raster decode context",
                )
            })?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()
            .map_err(|error| js_error("cast raster decode context", error))?;
        context
            .draw_image_with_image_bitmap(&bitmap.0, 0.0, 0.0)
            .map_err(|error| js_error("draw decoded raster tile", error))?;
        let image = context
            .get_image_data(0.0, 0.0, f64::from(width), f64::from(height))
            .map_err(|error| js_error("read decoded raster pixels", error))?;
        let rgba = image.data().0;
        if rgba.len() != expected_bytes {
            return Err(WebHostError::platform(
                "read decoded raster pixels",
                format!("expected {expected_bytes} RGBA bytes, got {}", rgba.len()),
            ));
        }
        Ok(DecodedMapTile {
            viewport: request.viewport,
            identity: request.identity,
            width,
            height,
            rgba,
        })
    }
}

struct DecodedBitmap(ImageBitmap);

impl Drop for DecodedBitmap {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Default for BrowserRasterTileDecoder {
    fn default() -> Self {
        Self::standard()
    }
}
