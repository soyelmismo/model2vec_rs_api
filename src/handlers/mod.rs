use axum::http::HeaderMap;
use std::sync::Arc;

use crate::{
    error::{AppError, Result},
    models::ModelRegistry,
};

pub mod embeddings;
pub mod health;
pub mod models_list;

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ModelRegistry>,
    /// If set, every request must supply `Authorization: Bearer <api_key>`.
    pub api_key: Option<String>,
}

impl AppState {
    pub fn new(registry: Arc<ModelRegistry>, api_key: Option<String>) -> Self {
        Self { registry, api_key }
    }

    /// Validate the bearer token when `api_key` is configured.
    pub fn check_auth(&self, headers: &HeaderMap) -> Result<()> {
        let Some(expected) = &self.api_key else {
            return Ok(());
        };

        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");

        if provided == expected {
            Ok(())
        } else {
            Err(AppError::Unauthorized)
        }
    }
}
