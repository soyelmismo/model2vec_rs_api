use std::sync::Arc;

use crate::{
    handlers::{AppState, embeddings, health, models_list},
    server::{Request, Response, Routable},
};

impl Routable for Arc<AppState> {
    fn route(&self, req: &Request<'_>) -> Response {
        let path = req.path.split('?').next().unwrap_or(req.path);

        match (req.method, path) {
            ("GET", "/health") => health::handle(),
            ("GET", "/v1/models" | "/models") => models_list::handle(self, req),
            ("POST", "/v1/embeddings" | "/embeddings") => embeddings::handle(self, req),

            // Known paths, wrong method
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
        Arc::new(AppState::new(Arc::new(registry), None))
    }

    fn req(method: &'static str, path: &'static str) -> Request<'static> {
        Request { method, path, body: b"{}", auth_header: None }
    }

    #[test]
    fn health_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/health"));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn v1_models_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/v1/models"));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn models_get() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/models"));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn v1_embeddings_post() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/v1/embeddings"));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn embeddings_post() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/embeddings"));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn health_wrong_method() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/health"));
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn models_wrong_method() {
        let state = empty_state();
        let resp = state.route(&req("POST", "/v1/models"));
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn unknown_path() {
        let state = empty_state();
        let resp = state.route(&req("GET", "/unknown"));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn query_string_stripped() {
        let state = empty_state();
        let req = Request { method: "GET", path: "/health?token=x", body: b"{}", auth_header: None };
        let resp = state.route(&req);
        assert_eq!(resp.status, 200);
    }
}
