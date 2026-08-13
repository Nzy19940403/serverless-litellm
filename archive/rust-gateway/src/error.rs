use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
    #[error("upstream timeout")]
    Timeout,
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    fn body(&self) -> serde_json::Value {
        match self {
            Self::Unauthorized(msg) => json!({
                "error": {
                    "message": msg,
                    "type": "authentication_error",
                    "code": "invalid_api_key"
                }
            }),
            Self::NotFound(msg) => json!({
                "error": {
                    "message": msg,
                    "type": "invalid_request_error",
                    "code": "model_not_found"
                }
            }),
            Self::BadRequest(msg) => json!({
                "error": {
                    "message": msg,
                    "type": "invalid_request_error"
                }
            }),
            Self::Timeout => json!({
                "error": {
                    "message": "Upstream request timed out",
                    "type": "timeout"
                }
            }),
            Self::Internal(msg) => json!({
                "error": {
                    "message": msg,
                    "type": "server_error"
                }
            }),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status(), Json(self.body())).into_response()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Timeout
        } else {
            Self::Internal(e.to_string())
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
