use crate::config::AppConfig;
use crate::error::AppError;
use crate::providers::{
    anthropic_to_openai_sse_body, dispatch, http_client, pipe_sse_body,
    vertex_gemini_to_openai_sse_body, StreamMap, UpstreamResponse,
};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub http: Client,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let timeout = config.timeout;
        Self {
            http: http_client(timeout),
            config: Arc::new(config),
        }
    }
}

pub async fn root(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "service": "serverless-litellm",
        "runtime": "rust",
        "docs": "OpenAI-compatible: POST /v1/chat/completions, GET /v1/models",
        "models": state.config.model_names,
    }))
}

pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn list_models(State(state): State<AppState>) -> Json<Value> {
    let data: Vec<Value> = state
        .config
        .model_names
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "serverless-litellm",
            })
        })
        .collect();
    Json(json!({"object": "list", "data": data}))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .ok_or_else(|| AppError::BadRequest("Missing required field: model".into()))?;

    let route = state.config.resolve(model).ok_or_else(|| {
        AppError::NotFound(format!(
            "Model \"{model}\" not found. Known: {}",
            state.config.model_names.join(", ")
        ))
    })?;

    if route.api_key.is_empty() && !route.provider.uses_gcp_adc() {
        return Err(AppError::Internal(format!(
            "Upstream key missing for \"{model}\". Set env {}.",
            route.api_key_env
        )));
    }

    let upstream: UpstreamResponse = dispatch(
        &state.http,
        route,
        body,
        state.config.drop_params,
    )
    .await?;

    if let Some(err) = upstream.error_json {
        return Ok((upstream.status, Json(err)).into_response());
    }

    if upstream.is_stream {
        let res = upstream
            .stream
            .ok_or_else(|| AppError::Internal("empty upstream stream".into()))?;

        let body = match upstream.stream_map {
            StreamMap::Anthropic => {
                anthropic_to_openai_sse_body(res, upstream.request_model)
            }
            StreamMap::VertexGemini => {
                vertex_gemini_to_openai_sse_body(res, upstream.request_model)
            }
            StreamMap::Passthrough => pipe_sse_body(res),
        };

        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-cache, no-transform")
            .header(header::CONNECTION, "keep-alive")
            .header("X-Accel-Buffering", "no")
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))?);
    }

    let json = upstream
        .json
        .ok_or_else(|| AppError::Internal("empty upstream json".into()))?;
    Ok(Json(json).into_response())
}
