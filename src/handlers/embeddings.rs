use serde::Deserialize;

use crate::handlers::AppState;
use crate::server::{Request, Response};

#[derive(Deserialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

#[derive(Deserialize)]
struct EmbeddingRequest {
    model: String,
    input: EmbeddingInput,
}

pub fn handle(state: &AppState, req: &Request<'_>) -> Response {
    if let Err(r) = state.check_auth(req) {
        return r;
    }

    let parsed: EmbeddingRequest = match serde_json::from_slice(req.body) {
        Ok(v) => v,
        Err(e) => {
            let error = serde_json::json!({
                "error": {
                    "message": e.to_string(),
                    "type": "api_error",
                    "code": 400
                }
            });
            return Response::json(400, serde_json::to_vec(&error).unwrap_or_default());
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

    let Some(embeddings) = state.registry.encode(&parsed.model, &texts) else {
        let error = serde_json::json!({
            "error": {
                "message": format!("model '{}' not found", parsed.model),
                "type": "api_error",
                "code": 404
            }
        });
        return Response::json(404, serde_json::to_vec(&error).unwrap_or_default());
    };

    // ~4 chars per token — BPE heuristic, matches model2vec-rs internal tokenizer
    let total_chars: usize = texts.iter().map(String::len).sum();
    let approx_tokens = (total_chars / 4).max(1);

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

    Response::json(200, serde_json::to_vec(&resp).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn req(body: &'static [u8], auth: Option<&'static str>) -> Request<'static> {
        Request { method: "POST", path: "/v1/embeddings", body, auth_header: auth }
    }

    #[test]
    fn invalid_json_returns_400() {
        let state = empty_state();
        let resp = handle(&state, &req(b"not json", None));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn empty_single_input_returns_400() {
        let state = empty_state();
        let body = br#"{"model":"x","input":""}"#;
        let resp = handle(&state, &req(body, None));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn empty_batch_input_returns_400() {
        let state = empty_state();
        let body = br#"{"model":"x","input":[]}"#;
        let resp = handle(&state, &req(body, None));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn missing_model_returns_404() {
        let state = empty_state();
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, None));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn auth_rejects_wrong_token() {
        let state = authed_state("secret");
        let body = br#"{"model":"x","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer wrong")));
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn auth_accepts_correct_token() {
        let state = authed_state("secret");
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer secret")));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn auth_disabled_allows_any_token() {
        let state = empty_state();
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer anything")));
        assert_eq!(resp.status, 404);
    }
}
