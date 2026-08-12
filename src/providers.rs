use crate::config::{ModelRoute, ProviderKind};
use crate::error::AppError;
use axum::body::Body;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;

const OPENAI_DROP: &[&str] = &[
    "logit_bias",
    "user",
    "n",
    "presence_penalty",
    "frequency_penalty",
    "seed",
    "logprobs",
    "top_logprobs",
];

pub fn http_client(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .expect("reqwest client")
}

fn strip_dropped(mut body: Value, drop_params: bool) -> Value {
    if !drop_params {
        return body;
    }
    if let Some(obj) = body.as_object_mut() {
        for k in OPENAI_DROP {
            obj.remove(*k);
        }
    }
    body
}

pub struct UpstreamResponse {
    pub status: reqwest::StatusCode,
    pub is_stream: bool,
    pub is_anthropic: bool,
    pub request_model: String,
    pub json: Option<Value>,
    pub error_json: Option<Value>,
    pub stream: Option<reqwest::Response>,
}

pub async fn dispatch(
    client: &Client,
    route: &ModelRoute,
    mut body: Value,
    drop_params: bool,
) -> Result<UpstreamResponse, AppError> {
    // Vertex uses GCP ADC / metadata token — no static API key required
    if route.api_key.is_empty() && !route.provider.uses_gcp_adc() {
        return Err(AppError::Internal(format!(
            "Missing API key for model \"{}\". Set env {}.",
            route.model_name,
            if route.api_key_env.is_empty() {
                "API_KEY"
            } else {
                &route.api_key_env
            }
        )));
    }

    let request_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(&route.model_name)
        .to_string();

    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    match route.provider {
        ProviderKind::OpenAiCompatible => {
            body = strip_dropped(body, drop_params);
            if let Some(obj) = body.as_object_mut() {
                obj.insert("model".into(), json!(route.upstream_model));
            }
            call_openai_compatible(client, route, body, stream, request_model).await
        }
        ProviderKind::Anthropic => {
            call_anthropic(client, route, body, stream, request_model).await
        }
        ProviderKind::VertexAnthropic => {
            call_vertex_anthropic(client, route, body, stream, request_model).await
        }
    }
}

/// Obtain OAuth access token for Vertex AI.
/// Order: route.api_key / VERTEX_ACCESS_TOKEN / GOOGLE_OAUTH_ACCESS_TOKEN → GCE metadata.
async fn gcp_access_token(client: &Client, route: &ModelRoute) -> Result<String, AppError> {
    if !route.api_key.is_empty() {
        return Ok(route.api_key.clone());
    }
    if let Ok(t) = std::env::var("VERTEX_ACCESS_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    if let Ok(t) = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }

    // Cloud Run / GCE metadata server
    let res = client
        .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to fetch GCP access token from metadata (are you on Cloud Run?). \
                 Locally set VERTEX_ACCESS_TOKEN=$(gcloud auth print-access-token). Error: {e}"
            ))
        })?;

    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Metadata token endpoint failed: {t}"
        )));
    }

    let v: Value = res.json().await.map_err(|e| AppError::Internal(e.to_string()))?;
    v.get("access_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Internal("metadata token response missing access_token".into()))
}

async fn resolve_gcp_project(client: &Client, route: &ModelRoute) -> Result<String, AppError> {
    if !route.vertex_project.is_empty() {
        return Ok(route.vertex_project.clone());
    }
    // Metadata project id on Cloud Run
    let res = client
        .get("http://metadata.google.internal/computeMetadata/v1/project/project-id")
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "GCP project not set. Set env GCP_PROJECT (or GOOGLE_CLOUD_PROJECT). Metadata error: {e}"
            ))
        })?;
    if !res.status().is_success() {
        return Err(AppError::Internal(
            "GCP project not set. Set env GCP_PROJECT.".into(),
        ));
    }
    let id = res.text().await.map_err(|e| AppError::Internal(e.to_string()))?;
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(AppError::Internal("empty project id from metadata".into()));
    }
    Ok(id)
}

fn vertex_anthropic_url(project: &str, location: &str, model: &str, stream: bool) -> String {
    let action = if stream {
        "streamRawPredict"
    } else {
        "rawPredict"
    };
    // URL-encode model id safely (contains @)
    let model_enc = urlencoding_minimal(model);
    if location.eq_ignore_ascii_case("global") {
        format!(
            "https://aiplatform.googleapis.com/v1/projects/{project}/locations/global/publishers/anthropic/models/{model_enc}:{action}"
        )
    } else {
        format!(
            "https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{model_enc}:{action}"
        )
    }
}

fn urlencoding_minimal(s: &str) -> String {
    // model ids are like claude-sonnet-4@20250514 — encode @ only
    s.replace('@', "%40")
}

fn to_vertex_anthropic_body(body: &Value) -> Value {
    let mut payload = to_anthropic_body(body, "");
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("model"); // model is in the URL on Vertex
        obj.insert(
            "anthropic_version".into(),
            json!("vertex-2023-10-16"),
        );
    }
    payload
}

async fn call_vertex_anthropic(
    client: &Client,
    route: &ModelRoute,
    body: Value,
    stream: bool,
    request_model: String,
) -> Result<UpstreamResponse, AppError> {
    let token = gcp_access_token(client, route).await?;
    let project = resolve_gcp_project(client, route).await?;
    let location = if route.vertex_location.is_empty() {
        "us-east5".to_string()
    } else {
        route.vertex_location.clone()
    };

    let url = vertex_anthropic_url(&project, &location, &route.upstream_model, stream);
    let payload = to_vertex_anthropic_body(&body);

    tracing::debug!(%url, %project, %location, model = %route.upstream_model, "vertex anthropic request");

    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let status = res.status();
    if !status.is_success() {
        let err = parse_error_body(res).await;
        return Ok(UpstreamResponse {
            status,
            is_stream: false,
            is_anthropic: true,
            request_model,
            json: None,
            error_json: Some(err),
            stream: None,
        });
    }

    if stream {
        return Ok(UpstreamResponse {
            status,
            is_stream: true,
            is_anthropic: true,
            request_model,
            json: None,
            error_json: None,
            stream: Some(res),
        });
    }

    let data: Value = res.json().await?;
    let openai = anthropic_to_openai(&data, &request_model);
    Ok(UpstreamResponse {
        status,
        is_stream: false,
        is_anthropic: true,
        request_model,
        json: Some(openai),
        error_json: None,
        stream: None,
    })
}

async fn call_openai_compatible(
    client: &Client,
    route: &ModelRoute,
    body: Value,
    stream: bool,
    request_model: String,
) -> Result<UpstreamResponse, AppError> {
    let url = format!("{}/chat/completions", route.api_base);
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", route.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = res.status();
    if !status.is_success() {
        let err = parse_error_body(res).await;
        return Ok(UpstreamResponse {
            status,
            is_stream: false,
            is_anthropic: false,
            request_model,
            json: None,
            error_json: Some(err),
            stream: None,
        });
    }

    if stream {
        return Ok(UpstreamResponse {
            status,
            is_stream: true,
            is_anthropic: false,
            request_model,
            json: None,
            error_json: None,
            stream: Some(res),
        });
    }

    let mut json: Value = res.json().await?;
    if let Some(obj) = json.as_object_mut() {
        obj.entry("model")
            .or_insert_with(|| json!(request_model.clone()));
    }

    Ok(UpstreamResponse {
        status,
        is_stream: false,
        is_anthropic: false,
        request_model,
        json: Some(json),
        error_json: None,
        stream: None,
    })
}

async fn call_anthropic(
    client: &Client,
    route: &ModelRoute,
    body: Value,
    stream: bool,
    request_model: String,
) -> Result<UpstreamResponse, AppError> {
    let payload = to_anthropic_body(&body, &route.upstream_model);
    let url = format!("{}/v1/messages", route.api_base);
    let res = client
        .post(&url)
        .header("x-api-key", &route.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let status = res.status();
    if !status.is_success() {
        let err = parse_error_body(res).await;
        return Ok(UpstreamResponse {
            status,
            is_stream: false,
            is_anthropic: true,
            request_model,
            json: None,
            error_json: Some(err),
            stream: None,
        });
    }

    if stream {
        return Ok(UpstreamResponse {
            status,
            is_stream: true,
            is_anthropic: true,
            request_model,
            json: None,
            error_json: None,
            stream: Some(res),
        });
    }

    let data: Value = res.json().await?;
    let openai = anthropic_to_openai(&data, &request_model);
    Ok(UpstreamResponse {
        status,
        is_stream: false,
        is_anthropic: true,
        request_model,
        json: Some(openai),
        error_json: None,
        stream: None,
    })
}

async fn parse_error_body(res: reqwest::Response) -> Value {
    let text = res.text().await.unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_else(|_| {
        json!({
            "error": {
                "message": text,
                "type": "upstream_error"
            }
        })
    })
}

fn to_anthropic_body(body: &Value, upstream_model: &str) -> Value {
    let messages = body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let mut system: Option<String> = None;
    let mut converted = Vec::new();

    for m in messages {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        if role == "system" {
            let text = content_to_text(m.get("content"));
            system = Some(match system {
                Some(prev) => format!("{prev}\n{text}"),
                None => text,
            });
            continue;
        }
        let arole = if role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        let content = match m.get("content") {
            Some(Value::String(s)) => json!(s),
            Some(Value::Array(parts)) => {
                let mapped: Vec<Value> = parts
                    .iter()
                    .map(|p| {
                        if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                            json!({
                                "type": "text",
                                "text": p.get("text").and_then(|t| t.as_str()).unwrap_or("")
                            })
                        } else {
                            json!({"type": "text", "text": p.to_string()})
                        }
                    })
                    .collect();
                json!(mapped)
            }
            other => json!(content_to_text(other)),
        };
        converted.push(json!({"role": arole, "content": content}));
    }

    if converted.is_empty() {
        converted.push(json!({"role": "user", "content": "Hello"}));
    }

    let max_tokens = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(4096);

    let mut out = json!({
        "model": upstream_model,
        "messages": converted,
        "max_tokens": max_tokens,
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    let obj = out.as_object_mut().unwrap();
    if let Some(sys) = system {
        obj.insert("system".into(), json!(sys));
    }
    if let Some(t) = body.get("temperature") {
        obj.insert("temperature".into(), t.clone());
    }
    if let Some(t) = body.get("top_p") {
        obj.insert("top_p".into(), t.clone());
    }

    out
}

fn content_to_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn map_stop(reason: Option<&str>) -> &'static str {
    match reason {
        Some("max_tokens") => "length",
        _ => "stop",
    }
}

fn anthropic_to_openai(data: &Value, request_model: &str) -> Value {
    let text = data
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|c| c.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let in_tok = data
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let out_tok = data
        .pointer("/usage/output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    json!({
        "id": data.get("id").cloned().unwrap_or_else(|| json!(format!("chatcmpl_{}", chrono::Utc::now().timestamp_millis()))),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": request_model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": map_stop(data.get("stop_reason").and_then(|s| s.as_str())),
        }],
        "usage": {
            "prompt_tokens": in_tok,
            "completion_tokens": out_tok,
            "total_tokens": in_tok + out_tok,
        }
    })
}

/// Pipe OpenAI-compatible SSE as-is.
pub fn pipe_sse_body(res: reqwest::Response) -> Body {
    let stream = res.bytes_stream().map(|item| {
        item.map_err(|e| std::io::Error::other(e.to_string()))
    });
    Body::from_stream(stream)
}

/// Anthropic SSE → OpenAI chat.completion.chunk SSE.
pub fn anthropic_to_openai_sse_body(res: reqwest::Response, request_model: String) -> Body {
    let id = format!("chatcmpl_{}", chrono::Utc::now().timestamp_millis());
    let mut byte_stream = res.bytes_stream();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    tokio::spawn(async move {
        let mut buffer = String::new();
        while let Some(item) = byte_stream.next().await {
            match item {
                Ok(chunk) => {
                    buffer.push_str(&String::from_utf8_lossy(&chunk));
                    while let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].trim_end_matches('\r').to_string();
                        buffer.drain(..=pos);
                        if let Some(data) = line.strip_prefix("data:") {
                            let data = data.trim();
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            if let Ok(evt) = serde_json::from_str::<Value>(data) {
                                if let Some(out) =
                                    anthropic_event_to_openai_chunk(&evt, &id, &request_model)
                                {
                                    let line = format!("data: {out}\n\n");
                                    if tx.send(Ok(Bytes::from(line))).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    return;
                }
            }
        }
        let _ = tx
            .send(Ok(Bytes::from_static(b"data: [DONE]\n\n")))
            .await;
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|item| (item, rx))
    });
    Body::from_stream(stream)
}

fn anthropic_event_to_openai_chunk(evt: &Value, id: &str, model: &str) -> Option<String> {
    let ty = evt.get("type")?.as_str()?;
    let (delta, finish) = match ty {
        "content_block_delta"
            if evt.pointer("/delta/type").and_then(|t| t.as_str()) == Some("text_delta") =>
        {
            (
                json!({
                    "content": evt.pointer("/delta/text").and_then(|t| t.as_str()).unwrap_or("")
                }),
                None,
            )
        }
        "message_start" => (json!({"role": "assistant", "content": ""}), None),
        "message_delta" => {
            let finish = evt
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .map(|s| map_stop(Some(s)));
            if finish.is_none() {
                return None;
            }
            (json!({}), finish)
        }
        _ => return None,
    };

    let chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish,
        }]
    });
    Some(chunk.to_string())
}
