use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

use crate::config::{is_prod, AppConfig};
use crate::error::AppError;

/// Master-key auth (same idea as LiteLLM).
/// Public: `/`, `/health`, `/healthz`
pub async fn master_key_auth(
    axum::extract::State(cfg): axum::extract::State<Arc<AppConfig>>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = req.uri().path();
    if matches!(path, "/" | "/health" | "/healthz") {
        return Ok(next.run(req).await);
    }

    if cfg.master_key.is_empty() {
        if is_prod() {
            return Err(AppError::Internal(
                "LITELLM_MASTER_KEY is not set. Set it as a Cloud Run secret/env.".into(),
            ));
        }
        return Ok(next.run(req).await);
    }

    let token = extract_token(&req);
    if token.as_deref() != Some(cfg.master_key.as_str()) {
        return Err(AppError::Unauthorized(
            "Invalid API key. Use Authorization: Bearer <master_key>".into(),
        ));
    }

    Ok(next.run(req).await)
}

fn extract_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    req.headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
