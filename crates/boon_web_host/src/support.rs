use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSupport {
    Available,
    Unsupported { reason: String },
}

impl FeatureSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserHostSupport {
    pub canvas_webgpu: FeatureSupport,
    pub retained_document_renderer: FeatureSupport,
    pub semantic_accessibility_projection: FeatureSupport,
    pub input_translation: FeatureSupport,
    pub same_origin_fetch: FeatureSupport,
    pub allowlisted_https_fetch: FeatureSupport,
    pub same_origin_websocket: FeatureSupport,
    pub url_history: FeatureSupport,
    pub indexed_db_preferences: FeatureSupport,
    pub app_owned_readback: FeatureSupport,
    pub retained_map_tiles: FeatureSupport,
    pub browser_raster_tile_decode: FeatureSupport,
}

impl BrowserHostSupport {
    /// Compile-time foundation capabilities. Runtime WebGPU availability is
    /// established only after adapter and surface acquisition.
    pub fn foundation() -> Self {
        Self {
            canvas_webgpu: FeatureSupport::Available,
            retained_document_renderer: FeatureSupport::Available,
            semantic_accessibility_projection: FeatureSupport::Available,
            input_translation: FeatureSupport::Available,
            same_origin_fetch: FeatureSupport::Available,
            allowlisted_https_fetch: FeatureSupport::Available,
            same_origin_websocket: FeatureSupport::Available,
            url_history: FeatureSupport::Available,
            indexed_db_preferences: FeatureSupport::Available,
            app_owned_readback: FeatureSupport::Unsupported {
                reason: "the shared renderer does not yet expose a browser-safe presented-frame readback transaction".to_owned(),
            },
            retained_map_tiles: FeatureSupport::Available,
            browser_raster_tile_decode: FeatureSupport::Available,
        }
    }

    pub fn unsupported_features(&self) -> Vec<&'static str> {
        let mut unsupported = Vec::new();
        for (name, support) in [
            ("canvas_webgpu", &self.canvas_webgpu),
            (
                "retained_document_renderer",
                &self.retained_document_renderer,
            ),
            (
                "semantic_accessibility_projection",
                &self.semantic_accessibility_projection,
            ),
            ("input_translation", &self.input_translation),
            ("same_origin_fetch", &self.same_origin_fetch),
            ("allowlisted_https_fetch", &self.allowlisted_https_fetch),
            ("same_origin_websocket", &self.same_origin_websocket),
            ("url_history", &self.url_history),
            ("indexed_db_preferences", &self.indexed_db_preferences),
            ("app_owned_readback", &self.app_owned_readback),
            ("retained_map_tiles", &self.retained_map_tiles),
            (
                "browser_raster_tile_decode",
                &self.browser_raster_tile_decode,
            ),
        ] {
            if !support.is_available() {
                unsupported.push(name);
            }
        }
        unsupported
    }
}
