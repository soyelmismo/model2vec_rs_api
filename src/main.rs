use anyhow::Result;
use std::sync::Arc;

mod config;
mod error;
mod handlers;
mod logger;
mod models;
mod router;

use config::Config;
use models::ModelRegistry;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present — simple key=value parser, no deps
    load_dotenv();

    logger::init();

    let config = Config::from_env()?;
    log::info!(
        "starting model2vec-api — listen={} models={} auth={}",
        config.listen_addr,
        config.models.len(),
        config.api_key.is_some(),
    );

    let registry = Arc::new(ModelRegistry::load_with_token(
        &config.models,
        config.hf_token.as_deref(),
    )?);
    let app = router::build_with_config(registry, config.api_key);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    log::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}

/// Reads a `.env` file and inserts missing vars into the process environment.
/// Silently does nothing if the file doesn't exist — no deps required.
fn load_dotenv() {
    let Ok(contents) = std::fs::read_to_string(".env") else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'');
            // Only set if not already present — real env takes priority
            if std::env::var(key).is_err() {
                // SAFETY: called before any threads are spawned (before tokio runtime)
                unsafe { std::env::set_var(key, val) };
            }
        }
    }
}
