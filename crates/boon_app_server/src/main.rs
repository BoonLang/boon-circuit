use boon_app_package::{BundleFileKind, LoadedAppBundle};
use boon_app_server::{
    DEFAULT_PACKAGE_DIR, DataDirectoryLock, LifecycleState, PRODUCTION_BIND_ADDRESS,
    ProcessEnvironment, ProductionProgram, RuntimeConfig, ServerTransientEffectHost, StaticAssets,
};
use boon_host_runtime::{
    ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileEffectAdapter,
    HostServiceEffectAdapter, NamedSecret,
};
use boon_host_services::{HostServiceConfig, HostServices};
use boon_http_client::{ClientConfig, HttpClient};
use boon_http_runtime::OutboundHttpEffectAdapter;
use boon_persistence::{PersistenceWorkerConfig, RedbDriver};
use boon_runtime::DistributedProgramBundle;
use boon_server_host::bind;
use boon_server_runtime::{
    BoonServerProgram, DistributedSessionRegistryConfig, PersistentServerConfig,
    TransientEffectLimits,
};
use boon_wellen_host::{WaveformEffectLimits, WaveformEffectWorker};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const RUNTIME_CONTENT_STORE_ENTRIES: usize = 256;
const RUNTIME_CONTENT_STORE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const FILE_CAPABILITIES: usize = 64;
const ACTIVE_FILE_STREAMS: usize = 8;
const ACTIVE_HTTP_REQUESTS: usize = 32;
const ACTIVE_DEADLINES: usize = 1_024;
const CACHED_WAVEFORMS: usize = 8;
const PENDING_WAVEFORM_CALLS: usize = 32;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("boon-app-server: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serve".to_owned());
    match command.as_str() {
        "serve" => serve().await,
        "healthcheck" => healthcheck().await,
        "help" | "-h" | "--help" => {
            println!("usage: boon-app-server [serve|healthcheck]");
            Ok(())
        }
        _ => Err(format!("unknown command `{command}`").into()),
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let package_dir = std::env::var_os("BOON_PACKAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PACKAGE_DIR));
    let bundle = LoadedAppBundle::load(&package_dir)?;
    let runtime_config = RuntimeConfig::from_source(bundle.manifest(), &ProcessEnvironment)?;
    let data_lock = DataDirectoryLock::acquire(runtime_config.data_dir())?;
    let static_assets = StaticAssets::from_bundle(&bundle)?;
    let server_config = runtime_config.server_config()?;
    let lifecycle = LifecycleState::new();
    let persistence = RedbDriver::open(runtime_config.data_dir().join("state.redb"))?;
    let distributed_bundle = DistributedProgramBundle::new(vec![
        bundle.client_artifact().clone(),
        bundle.session_artifact().clone(),
        bundle.server_artifact().clone(),
    ])?;
    let (mut program, startup) = BoonServerProgram::with_distributed_persistence(
        &distributed_bundle,
        persistence,
        PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
        DistributedSessionRegistryConfig::default(),
    )?;
    let package_asset_entries = bundle
        .manifest()
        .files
        .iter()
        .filter(|file| file.kind == BundleFileKind::Asset)
        .count();
    let package_asset_bytes = bundle
        .manifest()
        .files
        .iter()
        .filter(|file| file.kind == BundleFileKind::Asset)
        .try_fold(0_u64, |total, file| {
            u64::try_from(file.bytes_len)
                .ok()
                .and_then(|bytes| total.checked_add(bytes))
        })
        .ok_or("package asset byte count overflow")?;
    let content_store_entries = RUNTIME_CONTENT_STORE_ENTRIES
        .checked_add(package_asset_entries)
        .ok_or("content store entry limit overflow")?;
    let content_store_bytes = RUNTIME_CONTENT_STORE_BYTES
        .checked_add(package_asset_bytes)
        .ok_or("content store byte limit overflow")?;
    let content_store = ContentStore::new(
        runtime_config.data_dir().join("content"),
        ContentStoreLimits::new(content_store_entries, content_store_bytes),
    )?;
    let file_streams = FileEffectAdapter::new(
        FileCapabilityRegistry::new(FILE_CAPABILITIES)?,
        content_store.clone(),
        ACTIVE_FILE_STREAMS,
    )?;
    let outbound_http = OutboundHttpEffectAdapter::new(
        HttpClient::new(ClientConfig::new(Vec::new()))?,
        ACTIVE_HTTP_REQUESTS,
    )?;
    let host_services = HostServiceEffectAdapter::new(
        HostServices::new(HostServiceConfig::default()),
        Vec::<NamedSecret>::new(),
        ACTIVE_DEADLINES,
    )?;
    let waveforms = WaveformEffectWorker::start(
        content_store,
        WaveformEffectLimits::new(CACHED_WAVEFORMS),
        PENDING_WAVEFORM_CALLS,
    )?;
    let transient_host = ServerTransientEffectHost::new_for_bundle(
        &bundle,
        file_streams,
        outbound_http,
        host_services,
        waveforms,
    )?;
    program
        .attach_transient_effect_host(Box::new(transient_host), TransientEffectLimits::default())?;
    let production =
        ProductionProgram::new(program, static_assets, lifecycle.clone(), bundle.manifest());
    let address = PRODUCTION_BIND_ADDRESS.parse::<SocketAddr>()?;
    let running = bind(address, server_config, production).await?;
    lifecycle.mark_ready();
    println!(
        "boon-app-server: ready package={} revision={} mode={} namespace={} origin={} restore={:?} epoch={} data_lock={}",
        bundle.manifest().package_id,
        bundle.manifest().source_revision,
        runtime_config.run_mode().as_str(),
        runtime_config.state_namespace(),
        runtime_config.public_origin(),
        startup.disposition,
        startup.restore_epoch,
        data_lock.path().display(),
    );
    wait_for_termination().await?;
    lifecycle.begin_shutdown();
    running.shutdown().await?;
    drop(data_lock);
    Ok(())
}

async fn wait_for_termination() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn healthcheck() -> Result<(), Box<dyn std::error::Error>> {
    let readiness_path =
        std::env::var("BOON_READINESS_PATH").unwrap_or_else(|_| "/api/readiness".to_owned());
    if !readiness_path.starts_with('/')
        || readiness_path.contains(['\r', '\n', ' ', '?', '#'])
        || readiness_path.len() > 256
    {
        return Err("BOON_READINESS_PATH is not a canonical HTTP path".into());
    }
    let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    let request =
        format!("GET {readiness_path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let status = response
        .split(|byte| *byte == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .unwrap_or_default();
    if !status.contains(" 200 ") {
        return Err(format!("readiness returned `{}`", status.trim()).into());
    }
    Ok(())
}
