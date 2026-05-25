use std::sync::Arc;

use crate::{
    models::ModelRegistry,
    server::{Request, Response},
};

pub mod embeddings;
pub mod health;
pub mod models_list;

pub struct AppState {
    pub registry: Arc<ModelRegistry>,
    pub api_key: Option<String>,
}

impl AppState {
    pub const fn new(registry: Arc<ModelRegistry>, api_key: Option<String>) -> Self {
        Self { registry, api_key }
    }

    /// Validate Bearer token. Returns Err(Response) when auth fails so
    /// handlers can do `if let Err(r) = state.check_auth(req) { return r; }`.
    pub fn check_auth(&self, req: &Request<'_>) -> Result<(), Response> {
        let Some(expected) = &self.api_key else {
            return Ok(());
        };
        // req.body only contains the body; headers are parsed in server.rs and
        // we pass the raw header block. We need to re-parse the Authorization header.
        // Since we already store `keep_alive` in Request, the cleanest approach is
        // to store auth header there too — see server.rs Request struct.
        // For now we read it from req.auth_header (added below).
        let provided = req.auth_header.and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");

        if provided == expected {
            Ok(())
        } else {
            Err(Response::json(
                401,
                br#"{"error":{"message":"unauthorized","type":"api_error","code":401}}"#.to_vec(),
            ))
        }
    }
}
