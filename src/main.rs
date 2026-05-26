use anyhow::Result;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Arc;

mod config;
mod error;
mod handlers;
mod logger;
mod models;
mod router;
mod server;

use config::Config;
use handlers::AppState;
use models::ModelRegistry;

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        let addr = std::env::var("M2V_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:22671".into());
        let socket: std::net::SocketAddr = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("cannot parse M2V_LISTEN_ADDR {addr}: {e}"))?;
        let target = if socket.ip().is_unspecified() {
            std::net::SocketAddr::new(std::net::IpAddr::from([127, 0, 0, 1]), socket.port())
        } else {
            socket
        };
        let _ = TcpStream::connect_timeout(&target, std::time::Duration::from_secs(3))
            .map_err(|e| anyhow::anyhow!("healthcheck failed: {e}"))?;
        eprintln!("healthcheck ok — {target}");
        return Ok(());
    }

    let dotenv = load_dotenv_values();
    logger::init();

    let config = Config::from_env(&dotenv)?;
    log::info!(
        "starting model2vec-api — listen={} models={} auth={} max_batch={}",
        config.listen_addr,
        config.models.len(),
        if config.auth_disabled {
            "DISABLED"
        } else {
            "enabled"
        },
        config.max_batch_size,
    );

    for m in &config.models {
        log::info!("model alias={}", m.alias);
    }

    let registry = Arc::new(ModelRegistry::load_with_token(
        &config.models,
        config.hf_token.as_deref(),
    )?);
    let mut app_state = AppState::new(registry, config.api_key.clone(), config.max_batch_size);
    if config.auth_disabled {
        app_state = app_state.with_auth_disabled();
    }
    let state = Arc::new(app_state);

    let worker_threads = config.worker_threads;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_io()
        .enable_time()
        .build()?
        .block_on(server::serve(&config.listen_addr, state))?;

    Ok(())
}

fn load_dotenv_values() -> HashMap<String, String> {
    let Ok(contents) = std::fs::read_to_string(".env") else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let k = key.trim().to_string();
            let v = val.trim().trim_matches('"').trim_matches('\'').to_string();
            let _ = map.entry(k).or_insert(v);
        }
    }
    map
}
