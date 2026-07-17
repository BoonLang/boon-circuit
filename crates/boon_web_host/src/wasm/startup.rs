use super::{BrowserFetchAdapter, window};
use crate::{
    BrowserAppStartup, BrowserFetchCapabilities, BrowserFetchCapability, BrowserFetchRequest,
    FetchMethod, WebHostError, WebHostResult, decode_browser_app_config,
};
use boon_app_package::{BrowserAppConfig, MAX_BROWSER_APP_CONFIG_BYTES};
use js_sys::Uint8Array;
use std::cell::RefCell;
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use web_sys::HtmlCanvasElement;

const BOOTSTRAP_FETCH_CAPABILITY: &str = "boon-browser-bootstrap";

enum BrowserWasmStartupState {
    Idle,
    Starting,
    Started(Box<ActiveBrowserApp>),
}

struct ActiveBrowserApp {
    _startup: BrowserAppStartup,
    _canvas: HtmlCanvasElement,
}

thread_local! {
    static STARTUP_STATE: RefCell<BrowserWasmStartupState> =
        const { RefCell::new(BrowserWasmStartupState::Idle) };
}

/// Consume the package-generated CBOR bootstrap, verify the bounded public
/// Client artifact, and mount exactly one browser Client session.
#[wasm_bindgen]
pub async fn start_boon_app(config_bytes: Uint8Array) -> Result<(), JsValue> {
    begin_startup().map_err(js_startup_error)?;
    let result = start_boon_app_inner(config_bytes).await;
    match result {
        Ok(active) => {
            STARTUP_STATE.with(|state| {
                *state.borrow_mut() = BrowserWasmStartupState::Started(Box::new(active));
            });
            Ok(())
        }
        Err(error) => {
            STARTUP_STATE.with(|state| {
                *state.borrow_mut() = BrowserWasmStartupState::Idle;
            });
            Err(js_startup_error(error))
        }
    }
}

async fn start_boon_app_inner(config_bytes: Uint8Array) -> WebHostResult<ActiveBrowserApp> {
    let config_len = usize::try_from(config_bytes.length()).unwrap_or(usize::MAX);
    if config_len == 0 || config_len > MAX_BROWSER_APP_CONFIG_BYTES {
        return Err(WebHostError::LimitExceeded {
            resource: "browser app config".to_owned(),
            limit: MAX_BROWSER_APP_CONFIG_BYTES,
        });
    }
    let config = decode_browser_app_config(&config_bytes.to_vec())?;
    let canvas = browser_canvas(&config)?;
    let artifact_bytes = fetch_client_artifact(&config).await?;
    let startup = BrowserAppStartup::from_artifact_bytes(config, artifact_bytes)?;
    Ok(ActiveBrowserApp {
        _startup: startup,
        _canvas: canvas,
    })
}

fn begin_startup() -> WebHostResult<()> {
    STARTUP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        match &*state {
            BrowserWasmStartupState::Idle => {
                *state = BrowserWasmStartupState::Starting;
                Ok(())
            }
            BrowserWasmStartupState::Starting => Err(WebHostError::InvalidInput {
                field: "browser app startup".to_owned(),
                reason: "startup is already in progress".to_owned(),
            }),
            BrowserWasmStartupState::Started(active) => Err(WebHostError::InvalidInput {
                field: "browser app startup".to_owned(),
                reason: format!(
                    "package `{}` is already started",
                    active._startup.config().package_id
                ),
            }),
        }
    })
}

fn browser_canvas(config: &BrowserAppConfig) -> WebHostResult<HtmlCanvasElement> {
    let document = window()?
        .document()
        .ok_or_else(|| WebHostError::unsupported("Document", "window has no document"))?;
    document
        .get_element_by_id(&config.canvas_id)
        .ok_or_else(|| WebHostError::InvalidInput {
            field: "browser canvas_id".to_owned(),
            reason: format!("element `{}` does not exist", config.canvas_id),
        })?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| WebHostError::InvalidInput {
            field: "browser canvas_id".to_owned(),
            reason: format!("element `{}` is not a canvas", config.canvas_id),
        })
}

async fn fetch_client_artifact(config: &BrowserAppConfig) -> WebHostResult<Vec<u8>> {
    let mut capability = BrowserFetchCapability::same_origin_api(
        BOOTSTRAP_FETCH_CAPABILITY,
        config.client_artifact_path.clone(),
    );
    capability.methods = [FetchMethod::Get].into_iter().collect();
    capability.request_headers.clear();
    capability.max_url_bytes = config.client_artifact_path.len();
    capability.max_request_bytes = 1;
    capability.max_response_bytes = config.client_artifact_bytes_len;
    let adapter = BrowserFetchAdapter::new(BrowserFetchCapabilities::new([capability])?, 1)?;
    let response = adapter
        .execute(BrowserFetchRequest {
            request_id: 1,
            capability: BOOTSTRAP_FETCH_CAPABILITY.to_owned(),
            method: FetchMethod::Get,
            path_and_query: config.client_artifact_path.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await?;
    if response.status != 200 {
        return Err(WebHostError::platform(
            "fetch browser client artifact",
            format!("server returned HTTP {}", response.status),
        ));
    }
    Ok(response.body)
}

fn js_startup_error(error: WebHostError) -> JsValue {
    JsValue::from_str(&error.to_string())
}
