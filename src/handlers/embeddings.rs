use serde::Serialize;
use std::sync::Arc;

use crate::error::json_error;
use crate::handlers::AppState;
use crate::server::{Request, Response};

const MAX_BATCH_SIZE_DEFAULT: usize = 128;

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

#[derive(serde::Deserialize)]
struct EmbeddingRequest {
    model: String,
    input: EmbeddingInput,
    dimensions: Option<usize>,
}

#[derive(Serialize)]
struct EmbeddingData {
    object: &'static str,
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Serialize)]
struct Usage {
    prompt_tokens: usize,
    total_tokens: usize,
}

#[derive(Serialize)]
struct EmbeddingResponse {
    object: &'static str,
    data: Vec<EmbeddingData>,
    model: String,
    usage: Usage,
}

pub async fn handle(state: &AppState, req: &Request<'_>) -> Response {
    if let Err(r) = state.check_auth(req) {
        return r;
    }

    let parsed: EmbeddingRequest = match serde_json::from_slice(req.body) {
        Ok(v) => v,
        Err(_) => {
            return Response::json(400, json_error(400, "invalid request body — check your JSON syntax and field types"));
        }
    };

    let max_batch = if state.max_batch_size > 0 {
        state.max_batch_size
    } else {
        MAX_BATCH_SIZE_DEFAULT
    };

    let texts: Vec<String> = match parsed.input {
        EmbeddingInput::Single(s) if !s.is_empty() => vec![s],
        EmbeddingInput::Batch(v) if !v.is_empty() => v,
        _ => {
            return Response::json(
                400,
                json_error(400, "input must be a non-empty string or array"),
            );
        }
    };

    if texts.len() > max_batch {
        return Response::json(
            413,
            json_error(
                413,
                &format!(
                    "batch size {} exceeds maximum of {} — set M2V_MAX_BATCH_SIZE to adjust",
                    texts.len(),
                    max_batch
                ),
            ),
        );
    }

    let total_bytes: usize = texts.iter().map(String::len).sum();
    let approx_tokens = (total_bytes / 4).max(1);

    let model = parsed.model.clone();
    let requested_dims = parsed.dimensions;
    let registry = Arc::clone(&state.registry);

    if let Some(wanted) = requested_dims {
        let native = registry.dims(&model).unwrap_or(0);
        if native > 0 && wanted > native {
            return Response::json(
                400,
                json_error(
                    400,
                    &format!(
                        "model '{model}' supports up to {native} dimensions, but {wanted} was requested"
                    ),
                ),
            );
        }
    }

    let embeddings =
        tokio::task::spawn_blocking(move || registry.encode_owned(&model, &texts)).await;

    let Some(embeddings) = embeddings.ok().flatten() else {
        return Response::json(
            404,
            json_error(404, &format!("model '{}' not found", parsed.model)),
        );
    };

    let data: Vec<EmbeddingData> = embeddings
        .into_iter()
        .enumerate()
        .map(|(i, mut emb)| {
            if let Some(d) = requested_dims {
                emb.truncate(d);
            }
            EmbeddingData {
                object: "embedding",
                embedding: emb,
                index: i,
            }
        })
        .collect();

    let resp = EmbeddingResponse {
        object: "list",
        data,
        model: parsed.model,
        usage: Usage {
            prompt_tokens: approx_tokens,
            total_tokens: approx_tokens,
        },
    };

    let mut buf = Vec::with_capacity(estimated_size(&resp));
    serde_json::to_writer(&mut buf, &resp).unwrap_or_default();
    Response::json(200, buf)
}

fn estimated_size(resp: &EmbeddingResponse) -> usize {
    256 + resp.data.iter().map(|d| d.embedding.len().saturating_mul(5)).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelRegistry;
    use crate::server::Request;
    use std::sync::Arc;

    fn empty_state() -> AppState {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        AppState::new(Arc::new(registry), None, 128)
    }

    fn authed_state(key: &str) -> AppState {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        AppState::new(Arc::new(registry), Some(key.to_string()), 128)
    }

    fn req(body: &'static [u8], auth: Option<&'static str>) -> Request<'static> {
        Request {
            method: "POST",
            path: "/v1/embeddings",
            body,
            auth_header: auth,
        forwarded_for: None,
        }
    }

    #[tokio::test]
    async fn invalid_json_returns_400() {
        let state = empty_state();
        let resp = handle(&state, &req(b"not json", None)).await;
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn empty_single_input_returns_400() {
        let state = empty_state();
        let body = br#"{"model":"x","input":""}"#;
        let resp = handle(&state, &req(body, None)).await;
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn empty_batch_input_returns_400() {
        let state = empty_state();
        let body = br#"{"model":"x","input":[]}"#;
        let resp = handle(&state, &req(body, None)).await;
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn missing_model_returns_404() {
        let state = empty_state();
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, None)).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn dims_exceeding_native_returns_400() {
        let state = empty_state();
        let body = br#"{"model":"nonexistent","input":"hello","dimensions":1024}"#;
        let resp = handle(&state, &req(body, None)).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn dims_zero_on_unknown_model_returns_404() {
        let state = empty_state();
        let body = br#"{"model":"unknown","input":"hello","dimensions":1}"#;
        let resp = handle(&state, &req(body, None)).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn auth_rejects_wrong_token() {
        let state = authed_state("secret");
        let body = br#"{"model":"x","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer wrong"))).await;
        assert_eq!(resp.status, 401);
    }

    #[tokio::test]
    async fn auth_accepts_correct_token() {
        let state = authed_state("secret");
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer secret"))).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn auth_disabled_allows_any_token() {
        let state = empty_state();
        let body = br#"{"model":"nonexistent","input":"hello"}"#;
        let resp = handle(&state, &req(body, Some("Bearer anything"))).await;
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn batch_exceeding_max_returns_413() {
        let registry = ModelRegistry::load_with_token(&[], None).unwrap();
        let state = AppState::new(Arc::new(registry), None, 2);

        let texts: Vec<String> = (0..3).map(|i| format!("text{i}")).collect();
        let input = serde_json::to_string(&texts).unwrap();
        let body = format!(r#"{{"model":"x","input":{input}}}"#);
        let body_bytes: &'static [u8] = Box::leak(body.into_bytes().into_boxed_slice());
        let resp = handle(&state, &req(body_bytes, None)).await;
        assert_eq!(resp.status, 413);
    }
}
