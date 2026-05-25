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
        let provided = req.auth_header.and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");

        if provided == expected {
            Ok(())
        } else {
            Err(Response::json(
                401,
                br#"{"error":{"message":"unauthorized","type":"api_error","code":401}}"# as &'static [u8],
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelRegistry;

    fn dummy_req(auth: Option<&'static str>) -> Request<'static> {
        Request {
            method: "POST",
            path: "/v1/embeddings",
            body: b"{}",
            auth_header: auth,
        }
    }

    fn empty_state() -> AppState {
        let configs = vec![];
        let registry = ModelRegistry::load_with_token(&configs, None).unwrap();
        AppState::new(Arc::new(registry), None)
    }

    fn authed_state(key: &str) -> AppState {
        let configs = vec![];
        let registry = ModelRegistry::load_with_token(&configs, None).unwrap();
        AppState::new(Arc::new(registry), Some(key.to_string()))
    }

    #[test]
    fn auth_disabled_allows_all() {
        let state = empty_state();
        assert!(state.check_auth(&dummy_req(None)).is_ok());
        assert!(state.check_auth(&dummy_req(Some("Bearer x"))).is_ok());
    }

    #[test]
    fn auth_enabled_rejects_missing() {
        let state = authed_state("secret");
        let err = state.check_auth(&dummy_req(None)).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn auth_enabled_rejects_wrong_token() {
        let state = authed_state("secret");
        let err = state.check_auth(&dummy_req(Some("Bearer wrong"))).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn auth_enabled_accepts_correct_token() {
        let state = authed_state("secret");
        assert!(state.check_auth(&dummy_req(Some("Bearer secret"))).is_ok());
    }

    #[test]
    fn auth_requires_bearer_prefix() {
        let state = authed_state("secret");
        let err = state.check_auth(&dummy_req(Some("secret"))).unwrap_err();
        assert_eq!(err.status, 401);
    }
}
