use crate::handlers::AppState;
use crate::server::{Request, Response};

pub fn handle(state: &AppState, req: &Request<'_>) -> Response {
    if let Err(r) = state.check_auth(req) {
        return r;
    }

    let data: Vec<_> = state.registry.aliases().iter().map(|alias| serde_json::json!({
        "id": alias,
        "object": "model",
        "owned_by": "model2vec-api"
    })).collect();

    let body = serde_json::json!({
        "object": "list",
        "data": data
    });

    Response::json(200, serde_json::to_vec(&body).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::AppState;
    use crate::models::ModelRegistry;
    use crate::server::Request;
    use std::sync::Arc;

    fn empty_state() -> AppState {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        AppState::new(Arc::new(registry), None)
    }

    fn authed_state(key: &str) -> AppState {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        AppState::new(Arc::new(registry), Some(key.to_string()))
    }

    fn dummy_req(auth: Option<&'static str>) -> Request<'static> {
        Request {
            method: "GET",
            path: "/v1/models",
            body: b"{}",
            auth_header: auth,
        }
    }

    #[test]
    fn models_list_no_auth_returns_200() {
        let state = empty_state();
        let resp = handle(&state, &dummy_req(None));
        assert_eq!(resp.status, 200);
        let body = std::str::from_utf8(&resp.body).unwrap();
        assert!(body.contains("\"object\":\"list\""));
    }

    #[test]
    fn models_list_auth_rejects_bad_token() {
        let state = authed_state("secret");
        let resp = handle(&state, &dummy_req(Some("Bearer wrong")));
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn models_list_auth_accepts_correct_token() {
        let state = authed_state("secret");
        let resp = handle(&state, &dummy_req(Some("Bearer secret")));
        assert_eq!(resp.status, 200);
    }
}
