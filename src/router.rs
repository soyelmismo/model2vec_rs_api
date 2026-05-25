use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

use crate::{
    handlers::{
        AppState,
        embeddings::create_embeddings,
        health::health,
        models_list::list_models,
    },
    models::ModelRegistry,
};

pub fn build_with_config(registry: Arc<ModelRegistry>, api_key: Option<String>) -> Router {
    let state = Arc::new(AppState::new(registry, api_key));

    Router::new()
        .route("/health", get(health))
        .route("/v1/embeddings", post(create_embeddings))
        .route("/v1/models", get(list_models))
        // Convenience aliases (some clients omit /v1)
        .route("/embeddings", post(create_embeddings))
        .route("/models", get(list_models))
        .with_state(state)
}
