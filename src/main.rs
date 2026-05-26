use anyhow::Result;
use std::collections::HashMap;
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
    let dotenv = load_dotenv_values();
    logger::init();

    let config = Config::from_env(&dotenv)?;
    log::info!(
        "starting model2vec-api — listen={} models={} auth={}",
        config.listen_addr,
        config.models.len(),
        config.api_key.is_some(),
    );

    log::info!("models configured: {:?}", config.models);

    let registry = Arc::new(ModelRegistry::load_with_token(
        &config.models,
        config.hf_token.as_deref(),
    )?);
    let state = Arc::new(AppState::new(registry, config.api_key));

    let worker_threads = config.worker_threads;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_io()
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
