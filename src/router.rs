use std::sync::Arc;

use crate::{
    handlers::{AppState, embeddings, health, models_list},
    server::{Request, Response, Routable},
};

#[async_trait::async_trait]
impl Routable for Arc<AppState> {
    async fn route(&self, req: &Request<'_>) -> Response {
        let path = req.path.split('?').next().unwrap_or(req.path);

        match (req.method, path) {
            ("GET", "/health") => {
                if let Err(r) = self.check_auth(req) {
                    return r;
                }
                health::handle()
            }
            ("GET", "/v1/models" | "/models") => models_list::handle(self, req),
            ("POST", "/v1/embeddings" | "/embeddings") => embeddings::handle(self, req).await,

            (_, "/health" | "/v1/models" | "/models" | "/v1/embeddings" | "/embeddings") => {
                Response::method_not_allowed()
            }

            _ => Response::not_found(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelRegistry;
    use crate::server::Request;
    use std::sync::Arc;

    fn empty_state() -> Arc<AppState> {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        Arc::new(AppState::new(Arc::new(registry), None, 128))
    }

    fn authed_state(key: &str) -> Arc<AppState> {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        Arc::new(AppState::new(
            Arc::new(registry),
            Some(key.to_string()),
            128,
        ))
    }

    fn req(method: &'static str, path: &'static str) -> Request<'static> {
        Request {
            method,
            path,
            body: b"{}",
            auth_header: None,
            forwarded_for: None,
        }
    }

    fn req_with_auth(
        method: &'static str,
        path: &'static str,
        auth: &'static str,
    ) -> Request<'static> {
        Request {
            method,
            path,
            body: b"{}",
            auth_header: Some(auth),
            forwarded_for: None,
        }
    }

    #[tokio::test]
    async fn health_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/health")).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn health_with_auth_accepts_valid() {
        let state = authed_state("secret");
        let resp = state.route(&req_with_auth("GET", "/health", "Bearer secret")).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn health_with_auth_rejects_invalid() {
        let state = authed_state("secret");
        let resp = state.route(&req_with_auth("GET", "/health", "Bearer wrong")).await;
        assert_eq!(resp.status, 401);
    }

    #[tokio::test]
    async fn health_no_auth_when_required_rejected() {
        let state = authed_state("secret");
        let resp = state.route(&req("GET", "/health")).await;
        assert_eq!(resp.status, 401);
    }

    #[tokio::test]
    async fn v1_models_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/v1/models")).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn models_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/models")).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn v1_embeddings_post() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/v1/embeddings")).await;
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn embeddings_post() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/embeddings")).await;
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn health_wrong_method() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/health")).await;
        assert_eq!(resp.status, 405);
    }

    #[tokio::test]
    async fn models_wrong_method() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/v1/models")).await;
        assert_eq!(resp.status, 405);
    }

    #[tokio::test]
    async fn unknown_path() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/unknown")).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn query_string_stripped() {
        let state = empty_state();
        let req = Request {
            method: "GET",
            path: "/health?token=x",
            body: b"{}",
            auth_header: None,
            forwarded_for: None,
        };
        let resp = state.route(&req).await;
        assert_eq!(resp.status, 200);
    }
}
