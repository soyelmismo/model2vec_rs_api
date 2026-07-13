use std::sync::Arc;

use subtle::ConstantTimeEq;

use crate::{
    error::json_error,
    models::ModelRegistry,
    server::{Request, Response},
};

pub mod embeddings;
pub mod health;
pub mod models_list;

pub struct AppState {
    pub registry: Arc<ModelRegistry>,
    pub api_key: Option<String>,
    pub auth_disabled: bool,
    pub max_batch_size: usize,
}

impl AppState {
    pub const fn new(
        registry: Arc<ModelRegistry>,
        api_key: Option<String>,
        max_batch_size: usize,
    ) -> Self {
        Self {
            registry,
            api_key,
            auth_disabled: false,
            max_batch_size,
        }
    }

    pub const fn with_auth_disabled(mut self) -> Self {
        self.auth_disabled = true;
        self
    }

    pub fn check_auth(&self, req: &Request<'_>) -> Result<(), Response> {
        if self.auth_disabled {
            return Ok(());
        }
        let Some(expected) = &self.api_key else {
            return Ok(());
        };
        let provided = req.auth_header.and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");

        if bool::from(provided.as_bytes().ct_eq(expected.as_bytes())) {
            Ok(())
        } else {
            Err(Response::json(401, json_error(401, "unauthorized")))
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
            forwarded_for: None,
        }
    }

    fn empty_state() -> AppState {
        let configs = vec![];
        let registry = ModelRegistry::load_with_token(&configs, None).unwrap();
        AppState::new(Arc::new(registry), None, 128)
    }

    fn authed_state(key: &str) -> AppState {
        let configs = vec![];
        let registry = ModelRegistry::load_with_token(&configs, None).unwrap();
        AppState::new(Arc::new(registry), Some(key.to_string()), 128)
    }

    #[test]
    fn test_with_auth_disabled_sets_flag() {
        let state = empty_state();
        assert!(!state.auth_disabled);
        let state = state.with_auth_disabled();
        assert!(state.auth_disabled);
    }

    #[test]
    fn auth_disabled_allows_all() {
        let state = empty_state();
        assert!(state.check_auth(&dummy_req(None)).is_ok());
        assert!(state.check_auth(&dummy_req(Some("Bearer x"))).is_ok());
    }

    #[test]
    fn auth_explicitly_disabled_allows_all() {
        let state = authed_state("secret").with_auth_disabled();
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
