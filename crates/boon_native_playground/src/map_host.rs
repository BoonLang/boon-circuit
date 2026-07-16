use boon_document::{DocumentNodeId, MapTileCacheKey, MapTileRequestIdentity};
use boon_native_gpu::{
    DecodedMapTile, MapTileEvent, MapTileFetchRequest, MapTileSubmission, VisibleLayoutRenderer,
    decode_map_tile_bytes,
};
use futures::StreamExt;
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::Duration;
use tokio::task::{AbortHandle, JoinSet};

pub const NATIVE_MAP_TILE_CAPABILITIES_ENV: &str = "BOON_NATIVE_MAP_TILE_CAPABILITIES";

const DEFAULT_MAX_IN_FLIGHT: usize = 8;
const DEFAULT_MAX_RETRIES: u8 = 2;
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MAX_RETAINED_MAP_TILE_EVENTS: usize = 1_024;

type RequestKey = (DocumentNodeId, MapTileCacheKey);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeMapTileCapabilityConfig {
    name: String,
    url_template: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Clone, Debug)]
enum NativeMapTileSource {
    Http {
        url: reqwest::Url,
        timeout: Duration,
    },
    Fixture {
        media_type: String,
        encoded: Vec<u8>,
    },
    #[cfg(test)]
    DelayedFixture {
        media_type: String,
        encoded: Vec<u8>,
        delay: Duration,
    },
}

#[derive(Clone, Debug, Default)]
struct NativeMapTileCapabilities {
    entries: BTreeMap<String, NativeMapTileCapabilityConfig>,
    fixtures: BTreeMap<String, (String, Vec<u8>)>,
}

impl NativeMapTileCapabilities {
    fn from_env() -> Result<Self, String> {
        let Some(value) = std::env::var_os(NATIVE_MAP_TILE_CAPABILITIES_ENV) else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| format!("{NATIVE_MAP_TILE_CAPABILITIES_ENV} must be UTF-8"))?;
        let entries = serde_json::from_str::<Vec<NativeMapTileCapabilityConfig>>(&value)
            .map_err(|error| format!("decode {NATIVE_MAP_TILE_CAPABILITIES_ENV}: {error}"))?;
        Self::new(entries)
    }

    fn new(entries: Vec<NativeMapTileCapabilityConfig>) -> Result<Self, String> {
        let mut by_name = BTreeMap::new();
        for entry in entries {
            validate_capability_name(&entry.name)?;
            validate_url_template(&entry.url_template)?;
            if entry.timeout_ms == 0 || entry.timeout_ms > 60_000 {
                return Err(format!(
                    "native map capability {} timeout must be within 1..=60000ms",
                    entry.name
                ));
            }
            if by_name.insert(entry.name.clone(), entry).is_some() {
                return Err("duplicate native map tile capability".to_owned());
            }
        }
        Ok(Self {
            entries: by_name,
            fixtures: BTreeMap::new(),
        })
    }

    #[cfg(test)]
    fn with_fixture(name: &str, media_type: &str, encoded: Vec<u8>) -> Result<Self, String> {
        validate_capability_name(name)?;
        let mut fixtures = BTreeMap::new();
        fixtures.insert(name.to_owned(), (media_type.to_owned(), encoded));
        Ok(Self {
            entries: BTreeMap::new(),
            fixtures,
        })
    }

    fn resolve(&self, request: &MapTileFetchRequest) -> Result<NativeMapTileSource, String> {
        if let Some((media_type, encoded)) = self.fixtures.get(&request.url_template_capability) {
            return Ok(NativeMapTileSource::Fixture {
                media_type: media_type.clone(),
                encoded: encoded.clone(),
            });
        }
        let capability = self
            .entries
            .get(&request.url_template_capability)
            .ok_or_else(|| {
                format!(
                    "native map tile capability `{}` is not configured",
                    request.url_template_capability
                )
            })?;
        let url = reqwest::Url::parse(&expand_url_template(&capability.url_template, request))
            .map_err(|_| "native map tile capability resolved an invalid URL".to_owned())?;
        if !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "native map tile URL must be HTTP(S) without credentials or fragment".into(),
            );
        }
        let origin = url.origin().ascii_serialization();
        if !request.accepts_origin(&origin) {
            return Err(format!(
                "native map tile capability origin {origin} is absent from the descriptor"
            ));
        }
        Ok(NativeMapTileSource::Http {
            url,
            timeout: Duration::from_millis(capability.timeout_ms),
        })
    }
}

#[derive(Clone, Debug)]
struct WorkerFailure {
    message: String,
    retryable: bool,
}

#[derive(Debug)]
struct WorkerCompletion {
    request: MapTileFetchRequest,
    result: Result<DecodedMapTile, WorkerFailure>,
}

enum WorkerCommand {
    Fetch {
        key: RequestKey,
        request: Box<MapTileFetchRequest>,
        source: NativeMapTileSource,
    },
    Cancel(RequestKey),
    Shutdown,
}

struct NativeMapTileWorker {
    commands: tokio::sync::mpsc::Sender<WorkerCommand>,
    completions: Receiver<WorkerCompletion>,
    wake_sender: futures::channel::mpsc::UnboundedSender<()>,
    wakes: futures::channel::mpsc::UnboundedReceiver<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl NativeMapTileWorker {
    fn start(max_in_flight: usize) -> Result<Self, String> {
        let (commands, mut command_rx) =
            tokio::sync::mpsc::channel::<WorkerCommand>(max_in_flight.saturating_mul(2).max(1));
        let (completion_tx, completions) = std::sync::mpsc::channel::<WorkerCompletion>();
        let (wake_tx, wakes) = futures::channel::mpsc::unbounded::<()>();
        let worker_wake_tx = wake_tx.clone();
        let worker = thread::Builder::new()
            .name("boon-native-map-tiles".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        eprintln!("create native map tile runtime: {error}");
                        return;
                    }
                };
                runtime.block_on(async move {
                    let client = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .ok();
                    let mut tasks = JoinSet::<(RequestKey, WorkerCompletion)>::new();
                    let mut aborts = BTreeMap::<RequestKey, AbortHandle>::new();
                    loop {
                        tokio::select! {
                            command = command_rx.recv() => match command {
                                Some(WorkerCommand::Fetch { key, request, source }) => {
                                    if let Some(previous) = aborts.remove(&key) {
                                        previous.abort();
                                    }
                                    let client = client.clone();
                                    let task_key = key.clone();
                                    let request = *request;
                                    let handle = tasks.spawn(async move {
                                        let result = fetch_and_decode_native_tile(client, request.clone(), source).await;
                                        (task_key, WorkerCompletion { request, result })
                                    });
                                    aborts.insert(key, handle);
                                }
                                Some(WorkerCommand::Cancel(key)) => {
                                    if let Some(handle) = aborts.remove(&key) {
                                        handle.abort();
                                    }
                                }
                                Some(WorkerCommand::Shutdown) | None => {
                                    tasks.abort_all();
                                    while tasks.join_next().await.is_some() {}
                                    break;
                                }
                            },
                            joined = tasks.join_next(), if !tasks.is_empty() => {
                                if let Some(Ok((key, completion))) = joined {
                                    aborts.remove(&key);
                                    if completion_tx.send(completion).is_err() {
                                        tasks.abort_all();
                                        break;
                                    }
                                    let _ = worker_wake_tx.unbounded_send(());
                                }
                            }
                        }
                    }
                });
            })
            .map_err(|error| format!("spawn native map tile worker: {error}"))?;
        Ok(Self {
            commands,
            completions,
            wake_sender: wake_tx,
            wakes,
            thread: Some(worker),
        })
    }

    fn try_fetch(
        &self,
        key: RequestKey,
        request: MapTileFetchRequest,
        source: NativeMapTileSource,
    ) -> Result<(), String> {
        self.commands
            .try_send(WorkerCommand::Fetch {
                key,
                request: Box::new(request),
                source,
            })
            .map_err(|_| "native map tile worker queue is full or closed".to_owned())
    }

    fn cancel(&self, key: RequestKey) {
        let _ = self.commands.try_send(WorkerCommand::Cancel(key));
    }

    async fn next_wake(&mut self) -> Option<()> {
        self.wakes.next().await
    }

    fn wake_again(&self) {
        let _ = self.wake_sender.unbounded_send(());
    }
}

impl Drop for NativeMapTileWorker {
    fn drop(&mut self) {
        let _ = self.commands.blocking_send(WorkerCommand::Shutdown);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeMapTileHostMetrics {
    pub dispatched: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub failed: u64,
    pub retried: u64,
    pub stale_rejected: u64,
    pub prewarm_uploads: u64,
    pub prewarm_bytes: u64,
}

pub struct NativeMapTileHost {
    capabilities: NativeMapTileCapabilities,
    worker: NativeMapTileWorker,
    max_in_flight: usize,
    max_retries: u8,
    in_flight: BTreeMap<RequestKey, MapTileRequestIdentity>,
    attempts: BTreeMap<RequestKey, u8>,
    metrics: NativeMapTileHostMetrics,
    events: VecDeque<MapTileEvent>,
}

impl NativeMapTileHost {
    pub fn from_env() -> Result<Self, String> {
        Self::new(
            NativeMapTileCapabilities::from_env()?,
            DEFAULT_MAX_IN_FLIGHT,
            DEFAULT_MAX_RETRIES,
        )
    }

    fn new(
        capabilities: NativeMapTileCapabilities,
        max_in_flight: usize,
        max_retries: u8,
    ) -> Result<Self, String> {
        if max_in_flight == 0 {
            return Err("native map tile max_in_flight must be non-zero".to_owned());
        }
        Ok(Self {
            capabilities,
            worker: NativeMapTileWorker::start(max_in_flight)?,
            max_in_flight,
            max_retries,
            in_flight: BTreeMap::new(),
            attempts: BTreeMap::new(),
            metrics: NativeMapTileHostMetrics::default(),
            events: VecDeque::new(),
        })
    }

    pub fn metrics(&self) -> NativeMapTileHostMetrics {
        self.metrics
    }

    pub async fn next_wake(&mut self) -> Option<()> {
        self.worker.next_wake().await
    }

    pub fn wake_product_frame(&self) {
        self.worker.wake_again();
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = MapTileEvent> + '_ {
        self.events.drain(..)
    }

    pub fn service_before_frame(
        &mut self,
        renderer: &mut VisibleLayoutRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<bool, String> {
        self.consume_events(renderer);
        let mut visible_changed = false;
        loop {
            let completion = match self.worker.completions.try_recv() {
                Ok(completion) => completion,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err("native map tile worker stopped".to_owned());
                }
            };
            let key = request_key(&completion.request);
            self.in_flight.remove(&key);
            self.metrics.completed = self.metrics.completed.saturating_add(1);
            match completion.result {
                Ok(tile) => match renderer
                    .submit_map_tile(tile)
                    .map_err(|error| error.to_string())?
                {
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
                Err(failure) => {
                    self.metrics.failed = self.metrics.failed.saturating_add(1);
                    let submitted = renderer.submit_map_tile_failure(
                        &completion.request.viewport,
                        &completion.request.identity,
                        failure.message,
                        failure.retryable,
                    );
                    let attempts = self.attempts.get(&key).copied().unwrap_or(1);
                    if submitted == MapTileSubmission::Accepted
                        && failure.retryable
                        && attempts <= self.max_retries
                        && renderer.retry_map_tile(
                            &completion.request.viewport,
                            &completion.request.identity.tile,
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
            let prepared = renderer
                .prepare_map_tile_uploads(device, queue)
                .map_err(|error| error.message)?;
            self.metrics.prewarm_uploads = self
                .metrics
                .prewarm_uploads
                .saturating_add(u64::from(prepared.upload_count));
            self.metrics.prewarm_bytes = self
                .metrics
                .prewarm_bytes
                .saturating_add(prepared.upload_bytes);
        }
        self.dispatch(renderer);
        Ok(visible_changed)
    }

    pub fn service_after_frame(&mut self, renderer: &mut VisibleLayoutRenderer) {
        self.consume_events(renderer);
        self.dispatch(renderer);
    }

    fn consume_events(&mut self, renderer: &mut VisibleLayoutRenderer) {
        for event in renderer.drain_map_tile_events() {
            if self.events.len() == MAX_RETAINED_MAP_TILE_EVENTS {
                self.events.pop_front();
            }
            self.events.push_back(event.clone());
            if let MapTileEvent::Cancelled { viewport, identity } = event {
                let key = (viewport, identity.tile);
                if self.in_flight.remove(&key).is_some() {
                    self.worker.cancel(key.clone());
                    self.metrics.cancelled = self.metrics.cancelled.saturating_add(1);
                }
                self.attempts.remove(&key);
            }
        }
    }

    fn dispatch(&mut self, renderer: &mut VisibleLayoutRenderer) {
        let available = self.max_in_flight.saturating_sub(self.in_flight.len());
        for request in renderer.take_map_tile_requests(available) {
            self.dispatch_request(renderer, request);
        }
    }

    fn dispatch_request(
        &mut self,
        renderer: &mut VisibleLayoutRenderer,
        request: MapTileFetchRequest,
    ) {
        let key = request_key(&request);
        if self.in_flight.contains_key(&key) {
            return;
        }
        let source = match self.capabilities.resolve(&request) {
            Ok(source) => source,
            Err(message) => {
                renderer.submit_map_tile_failure(
                    &request.viewport,
                    &request.identity,
                    message,
                    false,
                );
                self.metrics.failed = self.metrics.failed.saturating_add(1);
                return;
            }
        };
        let attempt = self
            .attempts
            .get(&key)
            .copied()
            .unwrap_or_default()
            .saturating_add(1);
        match self.worker.try_fetch(key.clone(), request.clone(), source) {
            Ok(()) => {
                self.attempts.insert(key.clone(), attempt);
                self.in_flight.insert(key, request.identity);
                self.metrics.dispatched = self.metrics.dispatched.saturating_add(1);
            }
            Err(message) => {
                renderer.submit_map_tile_failure(
                    &request.viewport,
                    &request.identity,
                    message,
                    true,
                );
                self.metrics.failed = self.metrics.failed.saturating_add(1);
            }
        }
    }
}

async fn fetch_and_decode_native_tile(
    client: Option<reqwest::Client>,
    request: MapTileFetchRequest,
    source: NativeMapTileSource,
) -> Result<DecodedMapTile, WorkerFailure> {
    let (media_type, encoded) = match source {
        NativeMapTileSource::Fixture {
            media_type,
            encoded,
        } => (media_type, encoded),
        #[cfg(test)]
        NativeMapTileSource::DelayedFixture {
            media_type,
            encoded,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            (media_type, encoded)
        }
        NativeMapTileSource::Http { url, timeout } => {
            let client = client.ok_or_else(|| WorkerFailure {
                message: "initialize native map HTTP client".to_owned(),
                retryable: false,
            })?;
            let mut response = client
                .get(url)
                .timeout(timeout)
                .header(reqwest::header::ACCEPT, "image/png,image/jpeg,image/webp")
                .send()
                .await
                .map_err(|error| WorkerFailure {
                    message: if error.is_timeout() {
                        "native map tile transport timed out".to_owned()
                    } else if error.is_connect() {
                        "native map tile transport could not connect".to_owned()
                    } else {
                        "native map tile transport failed".to_owned()
                    },
                    retryable: error.is_timeout() || error.is_connect() || error.is_request(),
                })?;
            if !response.status().is_success() {
                return Err(WorkerFailure {
                    message: format!(
                        "native map tile server returned HTTP {}",
                        response.status().as_u16()
                    ),
                    retryable: response.status().is_server_error()
                        || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS,
                });
            }
            let media_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(str::trim)
                .unwrap_or_default()
                .to_owned();
            let mut encoded = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| WorkerFailure {
                message: "read native map tile response failed".to_owned(),
                retryable: true,
            })? {
                if encoded.len().saturating_add(chunk.len())
                    > boon_native_gpu::MAX_MAP_TILE_ENCODED_BYTES
                {
                    return Err(WorkerFailure {
                        message: "native map tile response exceeded the encoded byte cap".into(),
                        retryable: false,
                    });
                }
                encoded.extend_from_slice(&chunk);
            }
            (media_type, encoded)
        }
    };
    tokio::task::spawn_blocking(move || decode_map_tile_bytes(&request, &media_type, &encoded))
        .await
        .map_err(|error| WorkerFailure {
            message: format!("native map tile decoder stopped: {error}"),
            retryable: true,
        })?
        .map_err(|error| WorkerFailure {
            message: error.to_string(),
            retryable: false,
        })
}

fn validate_capability_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("native map tile capability name is invalid".to_owned());
    }
    Ok(())
}

fn validate_url_template(template: &str) -> Result<(), String> {
    if template.len() > 4_096 || template.contains(['\0', '\\', '#']) {
        return Err("native map tile URL template is invalid".to_owned());
    }
    for placeholder in ["{z}", "{x}", "{y}"] {
        if template.matches(placeholder).count() != 1 {
            return Err(format!(
                "native map tile URL template must contain {placeholder} exactly once"
            ));
        }
    }
    let unknown = template
        .replace("{z}", "")
        .replace("{x}", "")
        .replace("{y}", "")
        .replace("{scale}", "");
    if unknown.contains(['{', '}']) {
        return Err("native map tile URL template has an unknown placeholder".to_owned());
    }
    let probe = reqwest::Url::parse(
        &template
            .replace("{z}", "0")
            .replace("{x}", "0")
            .replace("{y}", "0")
            .replace("{scale}", "1"),
    )
    .map_err(|_| "native map tile URL template is not an absolute URL".to_owned())?;
    if !matches!(probe.scheme(), "https" | "http")
        || !probe.username().is_empty()
        || probe.password().is_some()
    {
        return Err("native map tile URL template must be credential-free HTTP(S)".to_owned());
    }
    Ok(())
}

fn expand_url_template(template: &str, request: &MapTileFetchRequest) -> String {
    template
        .replace("{z}", &request.identity.tile.z.to_string())
        .replace("{x}", &request.identity.tile.x.to_string())
        .replace("{y}", &request.identity.tile.y.to_string())
        .replace(
            "{scale}",
            &format_scale(request.identity.tile.scale.as_f64()),
        )
}

fn format_scale(scale: f64) -> String {
    if scale.fract() == 0.0 {
        format!("{scale:.0}")
    } else {
        format!("{scale:.3}").trim_end_matches('0').to_owned()
    }
}

fn request_key(request: &MapTileFetchRequest) -> RequestKey {
    (request.viewport.clone(), request.identity.tile.clone())
}

const fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document::{MapTileCacheKey, MapTileScaleKey, MapTileSourceId, MapViewportGeneration};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;
    use std::time::Instant;

    fn png_tile(size: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(size, size, Rgba([30, 130, 90, 255]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn request(capability: &str, origin: &str) -> MapTileFetchRequest {
        MapTileFetchRequest {
            viewport: DocumentNodeId("generic.map".to_owned()),
            identity: MapTileRequestIdentity {
                generation: MapViewportGeneration(1),
                tile: MapTileCacheKey {
                    source: MapTileSourceId("generic.fixture".to_owned()),
                    z: 2,
                    x: 1,
                    y: 2,
                    scale: MapTileScaleKey::from_viewport_scale(1.0).unwrap(),
                },
            },
            url_template_capability: capability.to_owned(),
            allowed_origins: vec![origin.to_owned()],
            expected_tile_size: 64,
        }
    }

    #[test]
    fn capability_resolution_requires_descriptor_origin() {
        let capabilities = NativeMapTileCapabilities::new(vec![NativeMapTileCapabilityConfig {
            name: "generic_xyz".to_owned(),
            url_template: "https://tiles.example.test/{z}/{x}/{y}@{scale}.png".to_owned(),
            timeout_ms: 100,
        }])
        .unwrap();
        let resolved = capabilities
            .resolve(&request("generic_xyz", "https://tiles.example.test"))
            .unwrap();
        assert!(matches!(resolved, NativeMapTileSource::Http { .. }));
        assert!(
            capabilities
                .resolve(&request("generic_xyz", "https://other.example.test"))
                .is_err()
        );
    }

    #[test]
    fn fixture_decode_runs_on_bounded_worker_and_preserves_identity() {
        let capabilities =
            NativeMapTileCapabilities::with_fixture("fixture_xyz", "image/png", png_tile(64))
                .unwrap();
        let host = NativeMapTileHost::new(capabilities.clone(), 2, 0).unwrap();
        let request = request("fixture_xyz", "https://fixture.invalid");
        let source = capabilities.resolve(&request).unwrap();
        host.worker
            .try_fetch(request_key(&request), request.clone(), source)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let completion = loop {
            match host.worker.completions.try_recv() {
                Ok(completion) => break completion,
                Err(TryRecvError::Empty) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                result => panic!("worker did not complete: {result:?}"),
            }
        };
        let decoded = completion.result.unwrap();
        assert_eq!(decoded.identity, request.identity);
        assert_eq!((decoded.width, decoded.height), (64, 64));
        assert_eq!(decoded.rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn stale_request_cancellation_aborts_transport_before_completion() {
        let host = NativeMapTileHost::new(NativeMapTileCapabilities::default(), 2, 0).unwrap();
        let request = request("delayed_fixture", "https://fixture.invalid");
        let key = request_key(&request);
        host.worker
            .try_fetch(
                key.clone(),
                request,
                NativeMapTileSource::DelayedFixture {
                    media_type: "image/png".to_owned(),
                    encoded: png_tile(64),
                    delay: Duration::from_millis(500),
                },
            )
            .unwrap();
        host.worker.cancel(key);
        thread::sleep(Duration::from_millis(650));
        assert!(matches!(
            host.worker.completions.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }
}
