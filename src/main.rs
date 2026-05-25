use anyhow::Result;
use std::sync::Arc;

mod config;
mod handlers;
mod logger;
mod models;
mod router;
mod server;

use config::Config;
use handlers::AppState;
use models::ModelRegistry;

fn main() -> Result<()> {
    load_dotenv();
    logger::init();

    let config = Config::from_env()?;
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

    // Manual runtime — no #[tokio::main] macro, no tokio-macros crate
    tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()?
        .block_on(server::serve(&config.listen_addr, state))?;

    Ok(())
}

/// Reads `.env` and sets missing env vars — no deps, called before any threads.
fn load_dotenv() {
    let Ok(contents) = std::fs::read_to_string(".env") else { return };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'');
            if std::env::var(key).is_err() {
                // SAFETY: single-threaded, before tokio runtime starts
                unsafe { std::env::set_var(key, val) };
            }
        }
    }
}
