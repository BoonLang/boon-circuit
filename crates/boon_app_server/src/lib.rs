//! Compiler-free process orchestration for immutable Boon app bundles.
//!
//! The server loads content-addressed artifacts, validates configuration before
//! binding, and wraps the structural HTTP/WebSocket host with generic static,
//! health, and readiness handling.

mod transient_host;

pub use transient_host::ServerTransientEffectHost;

use async_trait::async_trait;
use boon_app_package::{
    BundleFileDescriptor, BundleManifest, EnvironmentRedaction, EnvironmentValueKind,
    LoadedAppBundle, PackageError, RunMode, StaticCachePolicy, validate_scalar_shape,
};
use boon_server_host::{
    CallCancellation, Header, HttpRequest, HttpResponse, OriginPolicy, ServerConfig, ServerProgram,
    TrustedProxyPolicy, WebSocketAction, WebSocketEvent,
};
use fs2::FileExt;
use ipnet::IpNet;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use url::{Host, Url};

pub const PRODUCTION_BIND_ADDRESS: &str = "0.0.0.0:8080";
pub const DEFAULT_PACKAGE_DIR: &str = "/opt/boon/app";
const MAX_STATIC_BYTES: usize = 192 * 1024 * 1024;

pub trait EnvironmentSource {
    fn value(&self, name: &str) -> Result<Option<String>, ServerConfigError>;
}

pub struct ProcessEnvironment;

impl EnvironmentSource for ProcessEnvironment {
    fn value(&self, name: &str) -> Result<Option<String>, ServerConfigError> {
        std::env::var_os(name)
            .map(|value| {
                value.into_string().map_err(|_| {
                    ServerConfigError::new(format!(
                        "environment variable `{name}` is not valid UTF-8"
                    ))
                })
            })
            .transpose()
    }
}

impl EnvironmentSource for BTreeMap<String, String> {
    fn value(&self, name: &str) -> Result<Option<String>, ServerConfigError> {
        Ok(self.get(name).cloned())
    }
}

pub struct RuntimeConfig {
    run_mode: RunMode,
    public_origin: Url,
    data_dir: PathBuf,
    state_namespace: String,
    values: BTreeMap<String, ValidatedValue>,
}

struct ValidatedValue {
    value: String,
    redaction: EnvironmentRedaction,
}

impl RuntimeConfig {
    pub fn from_source(
        manifest: &BundleManifest,
        source: &impl EnvironmentSource,
    ) -> Result<Self, ServerConfigError> {
        let run_mode_text = required(source, "BOON_RUN_MODE")?;
        let run_mode = run_mode_text
            .parse::<RunMode>()
            .map_err(ServerConfigError::from_package)?;
        if run_mode != manifest.run_mode {
            return Err(ServerConfigError::new(format!(
                "BOON_RUN_MODE `{}` differs from bundle mode `{}`",
                run_mode.as_str(),
                manifest.run_mode.as_str()
            )));
        }
        if manifest.namespace_profile == boon_app_package::NamespaceProfile::Production
            && run_mode != RunMode::Live
        {
            return Err(ServerConfigError::new(
                "production namespace cannot start outside live mode",
            ));
        }
        let state_namespace = required(source, "BOON_STATE_NAMESPACE")?;
        let expected_namespace = &manifest
            .artifact(boon_plan::ProgramRole::Server)
            .ok_or_else(|| ServerConfigError::new("bundle has no server artifact"))?
            .state_namespace;
        if &state_namespace != expected_namespace {
            return Err(ServerConfigError::new(format!(
                "BOON_STATE_NAMESPACE `{state_namespace}` differs from server artifact namespace `{expected_namespace}`"
            )));
        }
        let data_dir = PathBuf::from(required(source, "BOON_DATA_DIR")?);
        if !data_dir.is_absolute() {
            return Err(ServerConfigError::new(
                "BOON_DATA_DIR must be an absolute path",
            ));
        }
        let public_origin_text = required(source, "BOON_PUBLIC_ORIGIN")?;
        let public_origin = validate_origin("BOON_PUBLIC_ORIGIN", &public_origin_text, run_mode)?;

        let mut values = BTreeMap::new();
        for variable in &manifest.environment {
            let value = source
                .value(&variable.name)?
                .or_else(|| variable.default.clone());
            let required_for_mode = variable.required_modes.contains(&run_mode);
            let Some(value) = value else {
                if required_for_mode {
                    return Err(ServerConfigError::new(format!(
                        "required environment variable `{}` is missing in {} mode",
                        variable.name,
                        run_mode.as_str()
                    )));
                }
                continue;
            };
            validate_scalar_shape(variable, &value).map_err(ServerConfigError::from_package)?;
            match variable.kind {
                EnvironmentValueKind::Origin => {
                    validate_origin(&variable.name, &value, run_mode)?;
                }
                EnvironmentValueKind::CidrList => {
                    validate_cidr_list(&variable.name, &value)?;
                }
                _ => {}
            }
            values.insert(
                variable.name.clone(),
                ValidatedValue {
                    value,
                    redaction: variable.redaction,
                },
            );
        }
        for (name, expected) in [
            ("BOON_RUN_MODE", run_mode_text.as_str()),
            ("BOON_PUBLIC_ORIGIN", public_origin_text.as_str()),
            (
                "BOON_DATA_DIR",
                data_dir
                    .to_str()
                    .ok_or_else(|| ServerConfigError::new("BOON_DATA_DIR is not valid UTF-8"))?,
            ),
            ("BOON_STATE_NAMESPACE", state_namespace.as_str()),
        ] {
            if let Some(value) = values.get(name)
                && value.value != expected
            {
                return Err(ServerConfigError::new(format!(
                    "validated `{name}` differs from the generic host value"
                )));
            }
        }
        Ok(Self {
            run_mode,
            public_origin,
            data_dir,
            state_namespace,
            values,
        })
    }

    pub const fn run_mode(&self) -> RunMode {
        self.run_mode
    }

    pub fn public_origin(&self) -> &Url {
        &self.public_origin
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn state_namespace(&self) -> &str {
        &self.state_namespace
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|entry| entry.value.as_str())
    }

    pub fn redacted_snapshot(&self) -> BTreeMap<String, String> {
        self.values
            .iter()
            .map(|(name, entry)| {
                let value = match entry.redaction {
                    EnvironmentRedaction::Public => entry.value.clone(),
                    EnvironmentRedaction::Sensitive => "[redacted]".to_owned(),
                    EnvironmentRedaction::Reference => "[secret-reference]".to_owned(),
                };
                (name.clone(), value)
            })
            .collect()
    }

    pub fn server_config(&self) -> Result<ServerConfig, ServerConfigError> {
        let mut config = ServerConfig {
            request_header_allowlist: BTreeSet::from([
                "accept".to_owned(),
                "accept-language".to_owned(),
                "content-type".to_owned(),
                "if-match".to_owned(),
                "if-none-match".to_owned(),
                "user-agent".to_owned(),
            ]),
            origin_policy: OriginPolicy::exact([self.public_origin.origin().ascii_serialization()]),
            ..ServerConfig::default()
        };
        if let Some(cidrs) = self.value("TRUSTED_PROXY_CIDRS")
            && !cidrs.is_empty()
        {
            config.trusted_proxy = TrustedProxyPolicy::from_cidrs(cidrs.split(',').map(str::trim))
                .map_err(|error| ServerConfigError::new(error.to_string()))?;
        }
        config
            .validate()
            .map_err(|error| ServerConfigError::new(error.to_string()))?;
        Ok(config)
    }
}

fn required(source: &impl EnvironmentSource, name: &str) -> Result<String, ServerConfigError> {
    source
        .value(name)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ServerConfigError::new(format!("required environment variable `{name}` is missing"))
        })
}

fn validate_origin(name: &str, value: &str, mode: RunMode) -> Result<Url, ServerConfigError> {
    let url = Url::parse(value).map_err(|error| {
        ServerConfigError::new(format!(
            "environment variable `{name}` is not an origin: {error}"
        ))
    })?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || !matches!(url.scheme(), "http" | "https")
    {
        return Err(ServerConfigError::new(format!(
            "environment variable `{name}` must contain only an HTTP(S) origin"
        )));
    }
    if mode == RunMode::Live && url.scheme() != "https" {
        return Err(ServerConfigError::new(format!(
            "environment variable `{name}` must use HTTPS in live mode"
        )));
    }
    if mode == RunMode::Deterministic && url.scheme() == "http" {
        let loopback = match url.host() {
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            None => false,
        };
        if !loopback {
            return Err(ServerConfigError::new(format!(
                "environment variable `{name}` may use HTTP only for loopback deterministic origins"
            )));
        }
    }
    Ok(url)
}

fn validate_cidr_list(name: &str, value: &str) -> Result<(), ServerConfigError> {
    if value.is_empty() {
        return Ok(());
    }
    let parts = value.split(',').collect::<Vec<_>>();
    if parts.len() > 256 {
        return Err(ServerConfigError::new(format!(
            "environment variable `{name}` contains too many CIDR entries"
        )));
    }
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            return Err(ServerConfigError::new(format!(
                "environment variable `{name}` contains an empty CIDR entry"
            )));
        }
        if part.parse::<IpNet>().is_err() {
            return Err(ServerConfigError::new(format!(
                "environment variable `{name}` contains invalid CIDR `{part}`"
            )));
        }
    }
    Ok(())
}

pub struct DataDirectoryLock {
    file: File,
    path: PathBuf,
}

impl DataDirectoryLock {
    pub fn acquire(data_dir: &Path) -> Result<Self, ServerConfigError> {
        fs::create_dir_all(data_dir).map_err(|error| {
            ServerConfigError::new(format!(
                "create data directory `{}`: {error}",
                data_dir.display()
            ))
        })?;
        let canonical = fs::canonicalize(data_dir).map_err(|error| {
            ServerConfigError::new(format!(
                "canonicalize data directory `{}`: {error}",
                data_dir.display()
            ))
        })?;
        let path = canonical.join(".boon-server.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| {
                ServerConfigError::new(format!(
                    "open data-directory lock `{}`: {error}",
                    path.display()
                ))
            })?;
        file.try_lock_exclusive().map_err(|error| {
            ServerConfigError::new(format!(
                "data directory `{}` is already owned or cannot be locked: {error}",
                canonical.display()
            ))
        })?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DataDirectoryLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Clone)]
pub struct LifecycleState {
    ready: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
}

impl LifecycleState {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn mark_ready(&self) {
        if !self.shutting_down.load(Ordering::Acquire) {
            self.ready.store(true, Ordering::Release);
        }
    }

    pub fn begin_shutdown(&self) {
        self.ready.store(false, Ordering::Release);
        self.shutting_down.store(true, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) && !self.shutting_down.load(Ordering::Acquire)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StaticAssets {
    assets: BTreeMap<String, StaticAsset>,
    spa_fallback: bool,
}

struct StaticAsset {
    body: Arc<[u8]>,
    content_type: &'static str,
    etag: String,
    cache: StaticCachePolicy,
}

impl StaticAssets {
    pub fn from_bundle(bundle: &LoadedAppBundle) -> Result<Self, ServerConfigError> {
        let mut total = 0usize;
        let mut assets = BTreeMap::new();
        for descriptor in bundle.manifest().public_files() {
            total = total
                .checked_add(descriptor.bytes_len)
                .ok_or_else(|| ServerConfigError::new("public static asset byte count overflow"))?;
            if total > MAX_STATIC_BYTES {
                return Err(ServerConfigError::new(format!(
                    "public static assets exceed {MAX_STATIC_BYTES} bytes"
                )));
            }
            let bytes = bundle
                .read_file(descriptor)
                .map_err(ServerConfigError::from_package)?;
            insert_static_asset(&mut assets, descriptor, bytes)?;
        }
        if !assets.contains_key("index.html") {
            return Err(ServerConfigError::new("bundle has no public index.html"));
        }
        Ok(Self {
            assets,
            spa_fallback: bundle.manifest().http.spa_fallback,
        })
    }

    fn response(&self, request: &HttpRequest) -> Option<HttpResponse> {
        let requested = request.path_segments.join("/");
        let requested = if requested.is_empty() {
            "index.html"
        } else {
            requested.as_str()
        };
        let asset = self.assets.get(requested).or_else(|| {
            (self.spa_fallback
                && !requested
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.contains('.')))
            .then(|| self.assets.get("index.html"))
            .flatten()
        })?;
        let not_modified = request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("if-none-match")
                && header.value.as_slice() == asset.etag.as_bytes()
        });
        if not_modified {
            return Some(HttpResponse {
                status: 304,
                headers: static_headers(asset),
                body: Vec::new(),
            });
        }
        let head = request.method.eq_ignore_ascii_case("HEAD");
        Some(HttpResponse {
            status: 200,
            headers: static_headers(asset),
            body: if head {
                Vec::new()
            } else {
                asset.body.as_ref().to_vec()
            },
        })
    }
}

fn insert_static_asset(
    assets: &mut BTreeMap<String, StaticAsset>,
    descriptor: &BundleFileDescriptor,
    bytes: Vec<u8>,
) -> Result<(), ServerConfigError> {
    if assets.contains_key(&descriptor.path) {
        return Err(ServerConfigError::new(format!(
            "public static path `{}` is duplicated",
            descriptor.path
        )));
    }
    assets.insert(
        descriptor.path.clone(),
        StaticAsset {
            body: bytes.into(),
            content_type: content_type(&descriptor.path),
            etag: format!("\"{}\"", descriptor.bytes_sha256),
            cache: descriptor.cache,
        },
    );
    Ok(())
}

fn static_headers(asset: &StaticAsset) -> Vec<Header> {
    let cache = match asset.cache {
        StaticCachePolicy::Revalidate => "no-cache",
        StaticCachePolicy::Immutable => "public, max-age=31536000, immutable",
    };
    vec![
        Header::new("content-type", asset.content_type),
        Header::new("cache-control", cache),
        Header::new("etag", asset.etag.clone()),
        Header::new("x-content-type-options", "nosniff"),
        Header::new("referrer-policy", "strict-origin-when-cross-origin"),
        Header::new(
            "content-security-policy",
            "default-src 'self'; script-src 'self'; connect-src 'self' https: wss:; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    ]
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "cbor" => "application/cbor",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub struct ProductionProgram<P> {
    delegate: P,
    static_assets: StaticAssets,
    lifecycle: LifecycleState,
    package_id: String,
    source_revision: String,
    health_path: String,
    readiness_path: String,
    program_path_prefixes: BTreeSet<String>,
}

impl<P> ProductionProgram<P> {
    pub fn new(
        delegate: P,
        static_assets: StaticAssets,
        lifecycle: LifecycleState,
        manifest: &BundleManifest,
    ) -> Self {
        Self {
            delegate,
            static_assets,
            lifecycle,
            package_id: manifest.package_id.clone(),
            source_revision: manifest.source_revision.clone(),
            health_path: manifest.http.health_path.clone(),
            readiness_path: manifest.http.readiness_path.clone(),
            program_path_prefixes: manifest
                .http
                .program_path_prefixes
                .iter()
                .cloned()
                .collect(),
        }
    }

    fn is_program_path(&self, request: &HttpRequest) -> bool {
        request
            .path_segments
            .first()
            .is_some_and(|segment| self.program_path_prefixes.contains(segment))
    }

    fn request_path(request: &HttpRequest) -> String {
        format!("/{}", request.path_segments.join("/"))
    }

    fn lifecycle_response(&self, readiness: bool) -> HttpResponse {
        let ready = self.lifecycle.is_ready();
        let status = if readiness && !ready { 503 } else { 200 };
        let lifecycle = if readiness {
            if ready { "ready" } else { "not_ready" }
        } else if self.lifecycle.is_shutting_down() {
            "shutting_down"
        } else {
            "live"
        };
        let body = format!(
            "status={lifecycle}\npackage_id={}\nsource_revision={}\n",
            self.package_id, self.source_revision
        )
        .into_bytes();
        HttpResponse {
            status,
            headers: vec![
                Header::new("content-type", "text/plain; charset=utf-8"),
                Header::new("cache-control", "no-store"),
            ],
            body,
        }
    }
}

#[async_trait]
impl<P: ServerProgram> ServerProgram for ProductionProgram<P> {
    async fn on_http(
        &mut self,
        request: HttpRequest,
        cancellation: CallCancellation,
    ) -> HttpResponse {
        let path = Self::request_path(&request);
        if path == self.health_path {
            return self.lifecycle_response(false);
        }
        if path == self.readiness_path {
            return self.lifecycle_response(true);
        }
        if matches!(request.method.as_str(), "GET" | "HEAD") && !self.is_program_path(&request) {
            return self
                .static_assets
                .response(&request)
                .unwrap_or_else(|| HttpResponse::new(404, "not found"));
        }
        self.delegate.on_http(request, cancellation).await
    }

    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        self.delegate.on_websocket(event, cancellation).await
    }

    fn has_distributed_session_transport(&self) -> bool {
        self.delegate.has_distributed_session_transport()
    }

    async fn on_distributed_session(
        &mut self,
        connection: boon_server_host::DistributedSessionConnectionId,
        event: boon_server_host::DistributedSessionEvent,
        cancellation: CallCancellation,
    ) -> Vec<boon_server_host::DistributedSessionAction> {
        self.delegate
            .on_distributed_session(connection, event, cancellation)
            .await
    }

    fn distributed_session_next_deadline(&self) -> Option<std::time::Instant> {
        self.delegate.distributed_session_next_deadline()
    }

    async fn on_distributed_session_timer(
        &mut self,
        now: std::time::Instant,
        cancellation: CallCancellation,
    ) -> Vec<boon_server_host::DistributedSessionAction> {
        self.delegate
            .on_distributed_session_timer(now, cancellation)
            .await
    }

    fn has_pending_internal_work(&self) -> bool {
        self.delegate.has_pending_internal_work()
    }

    async fn on_internal_work(&mut self) -> Vec<boon_server_host::DistributedSessionAction> {
        self.delegate.on_internal_work().await
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        connection: boon_server_host::DistributedSessionConnectionId,
    ) {
        self.delegate
            .on_distributed_session_send_accepted(connection);
    }

    async fn on_distributed_session_cancelled(
        &mut self,
        connection: Option<boon_server_host::DistributedSessionConnectionId>,
        reason: boon_server_host::CancellationReason,
    ) {
        self.delegate
            .on_distributed_session_cancelled(connection, reason)
            .await;
    }

    async fn on_http_cancelled(&mut self, reason: boon_server_host::CancellationReason) {
        self.delegate.on_http_cancelled(reason).await;
    }

    async fn on_websocket_cancelled(&mut self, reason: boon_server_host::CancellationReason) {
        self.delegate.on_websocket_cancelled(reason).await;
    }

    async fn on_shutdown(&mut self) {
        self.lifecycle.begin_shutdown();
        self.delegate.on_shutdown().await;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfigError(String);

impl ServerConfigError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn from_package(error: PackageError) -> Self {
        Self(error.to_string())
    }
}

impl std::fmt::Display for ServerConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ServerConfigError {}

#[cfg(test)]
mod tests;
