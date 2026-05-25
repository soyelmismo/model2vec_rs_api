use axum::{extract::State, http::HeaderMap, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::error::Result;
use super::AppState;

#[derive(Serialize)]
pub struct ModelCard {
    pub id: String,
    pub object: &'static str,
    pub owned_by: &'static str,
}

#[derive(Serialize)]
pub struct ModelListResponse {
    pub object: &'static str,
    pub data: Vec<ModelCard>,
}

pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ModelListResponse>> {
    state.check_auth(&headers)?;

    let data = state
        .registry
        .aliases()
        .into_iter()
        .map(|alias| ModelCard {
            id: alias.to_string(),
            object: "model",
            owned_by: "model2vec-api",
        })
        .collect();

    Ok(Json(ModelListResponse {
        object: "list",
        data,
    }))
}
