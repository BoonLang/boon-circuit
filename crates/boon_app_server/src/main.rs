use boon_app_package::LoadedAppBundle;
use boon_app_server::{
    DEFAULT_PACKAGE_DIR, DataDirectoryLock, LifecycleState, PRODUCTION_BIND_ADDRESS,
    ProcessEnvironment, ProductionProgram, RuntimeConfig, StaticAssets,
};
use boon_persistence::{PersistenceWorkerConfig, RedbDriver};
use boon_server_host::bind;
use boon_server_runtime::{BoonServerProgram, PersistentServerConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    let (program, startup) = BoonServerProgram::with_persistence(
        bundle.server_artifact().clone(),
        persistence,
        PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
    )?;
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
