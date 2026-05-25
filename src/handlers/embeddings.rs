use axum::{
    extract::State,
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::{AppError, Result};
use super::AppState;

// ─── OpenAI-compatible request / response types ──────────────────────────────

/// The `input` field accepts either a single string or an array of strings,
/// exactly as defined by the OpenAI Embeddings API.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

/// OpenAI Embeddings request body.
/// Fields marked "ignored" are accepted for API compatibility but not used.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct EmbeddingRequest {
    /// Model alias as configured via the MODELS env var.
    pub model: String,
    /// Text(s) to embed.
    pub input: EmbeddingInput,
    /// Ignored — accepted for OpenAI API compatibility.
    #[serde(default)]
    pub encoding_format: Option<String>,
    /// Ignored — accepted for OpenAI API compatibility.
    #[serde(default)]
    pub dimensions: Option<usize>,
    /// Ignored — accepted for OpenAI API compatibility.
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingObject {
    pub object: &'static str,
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingResponse {
    pub object: &'static str,
    pub data: Vec<EmbeddingObject>,
    pub model: String,
    pub usage: UsageInfo,
}

// ─── Handler ─────────────────────────────────────────────────────────────────

pub async fn create_embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<EmbeddingRequest>,
) -> Result<Json<EmbeddingResponse>> {
    state.check_auth(&headers)?;

    // Normalise input to Vec<String>
    let texts: Vec<String> = match req.input {
        EmbeddingInput::Single(s) => {
            if s.is_empty() {
                return Err(AppError::InvalidInput);
            }
            vec![s]
        }
        EmbeddingInput::Batch(v) => {
            if v.is_empty() {
                return Err(AppError::InvalidInput);
            }
            v
        }
    };

    let total_chars: usize = texts.iter().map(|t| t.len()).sum();

    let embeddings = state
        .registry
        .encode(&req.model, &texts)
        .ok_or_else(|| AppError::ModelNotFound(req.model.clone()))?;

    let data: Vec<EmbeddingObject> = embeddings
        .into_iter()
        .enumerate()
        .map(|(index, embedding)| EmbeddingObject {
            object: "embedding",
            embedding,
            index,
        })
        .collect();

    // OpenAI reports token counts; we approximate with char/4 (good enough for compatibility).
    let approx_tokens = (total_chars / 4).max(1);

    Ok(Json(EmbeddingResponse {
        object: "list",
        data,
        model: req.model,
        usage: UsageInfo {
            prompt_tokens: approx_tokens,
            total_tokens: approx_tokens,
        },
    }))
}
