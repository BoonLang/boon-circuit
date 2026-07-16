use crate::{
    BrowserFetchCapabilities, BrowserFetchRequest, FetchMethod, HeaderValue, WebHostError,
    WebHostResult,
};
use boon_native_gpu::MapTileFetchRequest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserMapTileTemplateCapability {
    pub name: String,
    pub fetch_capability: String,
    pub path_template: String,
}

#[derive(Clone, Debug, Default)]
pub struct BrowserMapTileCapabilities {
    entries: BTreeMap<String, BrowserMapTileTemplateCapability>,
    fetch: BrowserFetchCapabilities,
}

impl BrowserMapTileCapabilities {
    pub fn new(
        capabilities: impl IntoIterator<Item = BrowserMapTileTemplateCapability>,
        fetch: &BrowserFetchCapabilities,
    ) -> WebHostResult<Self> {
        let mut entries = BTreeMap::new();
        for capability in capabilities {
            validate_template(&capability)?;
            let fetch_capability =
                fetch
                    .capability(&capability.fetch_capability)
                    .ok_or_else(|| WebHostError::CapabilityDenied {
                        capability: capability.fetch_capability.clone(),
                        reason: format!(
                            "map tile capability {} references an undeclared fetch capability",
                            capability.name
                        ),
                    })?;
            if !fetch_capability.methods.contains(&FetchMethod::Get) {
                return Err(WebHostError::CapabilityDenied {
                    capability: capability.fetch_capability.clone(),
                    reason: "map tile fetch capability must allow GET".to_owned(),
                });
            }
            let expanded_probe = expand_template(
                &capability.path_template,
                &MapTileCoordinates {
                    z: 0,
                    x: 0,
                    y: 0,
                    scale: 1.0,
                },
            );
            let probe = BrowserFetchRequest {
                request_id: 1,
                capability: capability.fetch_capability.clone(),
                method: FetchMethod::Get,
                path_and_query: expanded_probe,
                headers: vec![tile_accept_header()],
                body: Vec::new(),
            };
            fetch.validate_request(probe)?;
            if entries
                .insert(capability.name.clone(), capability.clone())
                .is_some()
            {
                return Err(WebHostError::InvalidInput {
                    field: "map tile capability".to_owned(),
                    reason: format!("duplicate capability {}", capability.name),
                });
            }
        }
        Ok(Self {
            entries,
            fetch: fetch.clone(),
        })
    }

    pub fn build_fetch_request(
        &self,
        request_id: u64,
        tile: &MapTileFetchRequest,
        same_origin: &str,
    ) -> WebHostResult<BrowserFetchRequest> {
        let capability = self
            .entries
            .get(&tile.url_template_capability)
            .ok_or_else(|| WebHostError::CapabilityDenied {
                capability: tile.url_template_capability.clone(),
                reason: "map tile URL template capability is not declared".to_owned(),
            })?;
        let fetch_capability = self
            .fetch
            .capability(&capability.fetch_capability)
            .ok_or_else(|| WebHostError::CapabilityDenied {
                capability: capability.fetch_capability.clone(),
                reason: "map tile fetch origin is not declared".to_owned(),
            })?;
        let origin = fetch_capability.origin.resolved_origin(same_origin)?;
        if !tile
            .allowed_origins
            .iter()
            .any(|allowed| allowed == &origin)
        {
            return Err(WebHostError::CapabilityDenied {
                capability: tile.url_template_capability.clone(),
                reason: format!(
                    "resolved fetch origin {origin} is absent from the MapViewport descriptor"
                ),
            });
        }
        let coordinates = MapTileCoordinates {
            z: tile.identity.tile.z,
            x: tile.identity.tile.x,
            y: tile.identity.tile.y,
            scale: tile.identity.tile.scale.as_f64(),
        };
        let request = BrowserFetchRequest {
            request_id,
            capability: capability.fetch_capability.clone(),
            method: FetchMethod::Get,
            path_and_query: expand_template(&capability.path_template, &coordinates),
            headers: vec![tile_accept_header()],
            body: Vec::new(),
        };
        Ok(self.fetch.validate_request(request)?.request)
    }
}

struct MapTileCoordinates {
    z: u8,
    x: u32,
    y: u32,
    scale: f64,
}

fn validate_template(capability: &BrowserMapTileTemplateCapability) -> WebHostResult<()> {
    crate::capability::validate_capability_name(&capability.name)?;
    if !capability.path_template.starts_with('/')
        || capability.path_template.starts_with("//")
        || capability.path_template.contains(['#', '\\'])
    {
        return Err(WebHostError::InvalidInput {
            field: "map tile path template".to_owned(),
            reason: "must be a same-origin path without a fragment or backslash".to_owned(),
        });
    }
    for required in ["{z}", "{x}", "{y}"] {
        if capability.path_template.matches(required).count() != 1 {
            return Err(WebHostError::InvalidInput {
                field: "map tile path template".to_owned(),
                reason: format!("must contain {required} exactly once"),
            });
        }
    }
    let unknown = capability
        .path_template
        .replace("{z}", "")
        .replace("{x}", "")
        .replace("{y}", "")
        .replace("{scale}", "");
    if unknown.contains(['{', '}']) {
        return Err(WebHostError::InvalidInput {
            field: "map tile path template".to_owned(),
            reason: "contains an unknown or unmatched placeholder".to_owned(),
        });
    }
    Ok(())
}

fn expand_template(template: &str, coordinates: &MapTileCoordinates) -> String {
    template
        .replace("{z}", &coordinates.z.to_string())
        .replace("{x}", &coordinates.x.to_string())
        .replace("{y}", &coordinates.y.to_string())
        .replace("{scale}", &format_scale(coordinates.scale))
}

fn format_scale(scale: f64) -> String {
    if scale.fract() == 0.0 {
        format!("{scale:.0}")
    } else {
        format!("{scale:.3}").trim_end_matches('0').to_owned()
    }
}

fn tile_accept_header() -> HeaderValue {
    HeaderValue {
        name: "accept".to_owned(),
        value: "image/png,image/jpeg,image/webp".to_owned(),
    }
}
