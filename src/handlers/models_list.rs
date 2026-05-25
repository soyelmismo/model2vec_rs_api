use crate::server::{Request, Response};
use crate::handlers::AppState;

pub fn handle(state: &AppState, req: &Request<'_>) -> Response {
    if let Err(r) = state.check_auth(req) {
        return r;
    }

    let body = serde_json::json!({
        "object": "list",
        "data": state.registry.aliases().iter().map(|alias| serde_json::json!({
            "id": alias,
            "object": "model",
            "owned_by": "model2vec-api"
        })).collect::<Vec<_>>()
    });

    Response::json(200, body.to_string().into_bytes())
}
