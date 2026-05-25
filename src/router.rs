use std::sync::Arc;

use crate::{
    handlers::{AppState, embeddings, health, models_list},
    server::{Request, Response, Routable},
};

impl Routable for Arc<AppState> {
    fn route(self, req: &Request<'_>) -> Response {
        let path = req.path.split('?').next().unwrap_or(req.path);

        match (req.method, path) {
            ("GET", "/health") => health::handle(),
            ("GET", "/v1/models" | "/models") => models_list::handle(&self, req),
            ("POST", "/v1/embeddings" | "/embeddings") => embeddings::handle(&self, req),

            // Known paths, wrong method
            (_, "/health" | "/v1/models" | "/models" | "/v1/embeddings" | "/embeddings") => {
                Response::method_not_allowed()
            }

            _ => Response::not_found(),
        }
    }
}
