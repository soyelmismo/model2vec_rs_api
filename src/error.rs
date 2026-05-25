use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("model '{0}' not found")]
    ModelNotFound(String),

    #[error("input must be a non-empty string or array of strings")]
    InvalidInput,

    #[error("unauthorized")]
    Unauthorized,

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::ModelNotFound(m) => (
                StatusCode::NOT_FOUND,
                format!("model '{m}' not found"),
            ),
            AppError::InvalidInput => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Internal(e) => {
                log::error!("internal error: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        (
            status,
            Json(json!({
                "error": {
                    "message": message,
                    "type": "api_error",
                    "code": status.as_u16()
                }
            })),
        )
            .into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
