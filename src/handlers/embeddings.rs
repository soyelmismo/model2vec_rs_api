use serde::Deserialize;

use crate::handlers::AppState;
use crate::server::{Request, Response};

// ── OpenAI-compatible request types ──────────────────────────────────────────

#[derive(Deserialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

/// OpenAI Embeddings request body.
/// Fields beyond `model` and `input` are accepted for API compatibility.
#[derive(Deserialize)]
#[allow(dead_code)]
struct EmbeddingRequest {
    model: String,
    input: EmbeddingInput,
    encoding_format: Option<String>,
    dimensions: Option<usize>,
    user: Option<String>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

pub fn handle(state: &AppState, req: &Request<'_>) -> Response {
    if let Err(r) = state.check_auth(req) {
        return r;
    }

    // Parse JSON body
    let parsed: EmbeddingRequest = match serde_json::from_slice(req.body) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("{{\"error\":{{\"message\":\"{e}\",\"type\":\"api_error\",\"code\":400}}}}");
            return Response::json(400, msg.into_bytes());
        }
    };

    let texts: Vec<String> = match parsed.input {
        EmbeddingInput::Single(s) if !s.is_empty() => vec![s],
        EmbeddingInput::Batch(v) if !v.is_empty() => v,
        _ => {
            return Response::json(
                400,
                br#"{"error":{"message":"input must be a non-empty string or array","type":"api_error","code":400}}"#.to_vec(),
            );
        }
    };

    let total_chars: usize = texts.iter().map(|t| t.len()).sum();

    let embeddings = match state.registry.encode(&parsed.model, &texts) {
        Some(e) => e,
        None => {
            let msg = format!(
                "{{\"error\":{{\"message\":\"model '{}' not found\",\"type\":\"api_error\",\"code\":404}}}}",
                parsed.model
            );
            return Response::json(404, msg.into_bytes());
        }
    };

    let approx_tokens = (total_chars / 4).max(1);

    // Build response with serde_json
    let data: Vec<serde_json::Value> = embeddings
        .into_iter()
        .enumerate()
        .map(|(i, emb)| {
            serde_json::json!({
                "object": "embedding",
                "embedding": emb,
                "index": i
            })
        })
        .collect();

    let resp = serde_json::json!({
        "object": "list",
        "data": data,
        "model": parsed.model,
        "usage": {
            "prompt_tokens": approx_tokens,
            "total_tokens": approx_tokens
        }
    });

    Response::json(200, resp.to_string().into_bytes())
}
