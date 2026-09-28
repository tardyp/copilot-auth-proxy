use crate::auth::{COPILOT_API_VERSION, COPILOT_CLI_USER_AGENT, COPILOT_INTEGRATION_ID};
use crate::logger::JsonLogger;
use crate::storage::CopilotOAuthData;
use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::{header, HeaderMap, Method, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub client: reqwest::Client,
    pub creds: CopilotOAuthData,
    pub client_token: String,
    pub agent_whitelist: String,
    pub version: &'static str,
    pub logger: JsonLogger,
}

pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/healthz", get(healthz_handler).options(options_handler))
        .route("/v1/models", get(models_handler).options(options_handler))
        .route(
            "/v1/chat/completions",
            post(chat_completions_handler).options(options_handler),
        )
        .route(
            "/v1/responses",
            post(responses_handler).options(options_handler),
        )
        .fallback(fallback_handler)
        .layer(cors)
        .with_state(state)
}

fn check_auth(headers: &HeaderMap, expected_token: &str) -> bool {
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(val) = auth.to_str() {
            let val = val.trim();
            if let Some(token) = val.strip_prefix("Bearer ") {
                return token.trim() == expected_token;
            }
        }
    }
    false
}

fn headers_to_json(headers: &HeaderMap) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        let key = k.as_str().to_string();
        let val = if k == header::AUTHORIZATION {
            // Mask or redact token slightly or keep if needed, but for debugging keeping first 10 chars
            let s = v.to_str().unwrap_or("<binary>");
            if let Some(rest) = s.strip_prefix("Bearer ") {
                if rest.len() > 10 {
                    format!("Bearer {}...{}", &rest[..4], &rest[rest.len() - 4..])
                } else {
                    "Bearer ***".to_string()
                }
            } else {
                "***".to_string()
            }
        } else {
            v.to_str().unwrap_or("<binary>").to_string()
        };
        map.insert(key, Value::String(val));
    }
    Value::Object(map)
}

fn bytes_to_json_or_string(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }
    if let Ok(val) = serde_json::from_slice::<Value>(bytes) {
        val
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        Value::String(s.to_string())
    } else {
        Value::String(format!("<binary data, {} bytes>", bytes.len()))
    }
}

async fn options_handler() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn healthz_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let req_id = state.logger.next_id();
    let start = Instant::now();

    state.logger.log_raw(json!({
        "type": "request",
        "request_id": req_id,
        "method": "GET",
        "path": "/healthz",
        "headers": headers_to_json(&headers),
        "body": null,
    }));

    let resp_body = json!({
        "ok": true,
        "version": state.version
    });

    state.logger.log_raw(json!({
        "type": "response",
        "request_id": req_id,
        "status_code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": resp_body,
        "duration_ms": start.elapsed().as_millis() as u64,
    }));

    Json(resp_body)
}

async fn models_handler(
    State(state): State<Arc<AppState>>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let req_id = state.logger.next_id();
    let start = Instant::now();

    state.logger.log_raw(json!({
        "type": "request",
        "request_id": req_id,
        "method": "GET",
        "path": original_uri.path(),
        "query": original_uri.query(),
        "headers": headers_to_json(&headers),
        "body": null,
    }));

    if !check_auth(&headers, &state.client_token) {
        let err_json = json!({ "error": "unauthorized" });
        state.logger.log_raw(json!({
            "type": "response",
            "request_id": req_id,
            "status_code": 401,
            "headers": { "content-type": "application/json" },
            "body": err_json,
            "duration_ms": start.elapsed().as_millis() as u64,
        }));
        return Err((StatusCode::UNAUTHORIZED, Json(err_json)));
    }

    let base_url = state
        .creds
        .api_endpoint
        .as_deref()
        .unwrap_or("https://api.githubcopilot.com");
    let target_url = format!("{}/models", base_url);

    forward_request(
        &state,
        req_id,
        start,
        Method::GET,
        &target_url,
        None,
        Some(&state.agent_whitelist),
    )
    .await
}

async fn chat_completions_handler(
    State(state): State<Arc<AppState>>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let req_id = state.logger.next_id();
    let start = Instant::now();

    state.logger.log_raw(json!({
        "type": "request",
        "request_id": req_id,
        "method": "POST",
        "path": original_uri.path(),
        "query": original_uri.query(),
        "headers": headers_to_json(&headers),
        "body": bytes_to_json_or_string(&body),
    }));

    if !check_auth(&headers, &state.client_token) {
        let err_json = json!({ "error": "unauthorized" });
        state.logger.log_raw(json!({
            "type": "response",
            "request_id": req_id,
            "status_code": 401,
            "headers": { "content-type": "application/json" },
            "body": err_json,
            "duration_ms": start.elapsed().as_millis() as u64,
        }));
        return Err((StatusCode::UNAUTHORIZED, Json(err_json)));
    }

    let base_url = state
        .creds
        .api_endpoint
        .as_deref()
        .unwrap_or("https://api.githubcopilot.com");
    let target_url = format!("{}/chat/completions", base_url);

    forward_request(
        &state,
        req_id,
        start,
        Method::POST,
        &target_url,
        Some(body),
        None,
    )
    .await
}

async fn responses_handler(
    State(state): State<Arc<AppState>>,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let req_id = state.logger.next_id();
    let start = Instant::now();

    state.logger.log_raw(json!({
        "type": "request",
        "request_id": req_id,
        "method": "POST",
        "path": original_uri.path(),
        "query": original_uri.query(),
        "headers": headers_to_json(&headers),
        "body": bytes_to_json_or_string(&body),
    }));

    if !check_auth(&headers, &state.client_token) {
        let err_json = json!({ "error": "unauthorized" });
        state.logger.log_raw(json!({
            "type": "response",
            "request_id": req_id,
            "status_code": 401,
            "headers": { "content-type": "application/json" },
            "body": err_json,
            "duration_ms": start.elapsed().as_millis() as u64,
        }));
        return Err((StatusCode::UNAUTHORIZED, Json(err_json)));
    }

    let base_url = state
        .creds
        .api_endpoint
        .as_deref()
        .unwrap_or("https://api.githubcopilot.com");
    let target_url = format!("{}/responses", base_url);

    forward_request(
        &state,
        req_id,
        start,
        Method::POST,
        &target_url,
        Some(body),
        None,
    )
    .await
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            star_value_index = value_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

fn filter_models_payload(mut payload: Value, whitelist: &str) -> Value {
    if let Some(models) = payload.get_mut("data").and_then(Value::as_array_mut) {
        models.retain(|model| {
            model
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| wildcard_match(whitelist, id))
        });
    }
    payload
}

async fn forward_request(
    state: &AppState,
    req_id: u64,
    start: Instant,
    method: Method,
    target_url: &str,
    body: Option<axum::body::Bytes>,
    model_whitelist: Option<&str>,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let mut req_builder = state.client.request(method, target_url);

    req_builder = req_builder
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", state.creds.access),
        )
        .header(header::USER_AGENT, COPILOT_CLI_USER_AGENT)
        .header("Editor-Version", COPILOT_CLI_USER_AGENT)
        .header("Copilot-Integration-Id", COPILOT_INTEGRATION_ID)
        .header("Openai-Intent", "conversation-agent")
        .header("X-GitHub-Api-Version", COPILOT_API_VERSION);

    if let Some(bytes) = body {
        req_builder = req_builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(bytes);
    }

    let upstream_resp = match req_builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            let err_json = json!({ "error": format!("Upstream request failed: {}", e) });
            state.logger.log_raw(json!({
                "type": "response",
                "request_id": req_id,
                "status_code": 502,
                "headers": { "content-type": "application/json" },
                "body": err_json,
                "duration_ms": start.elapsed().as_millis() as u64,
            }));
            return Err((StatusCode::BAD_GATEWAY, Json(err_json)));
        }
    };

    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response_builder = Response::builder().status(status);
    let mut resp_headers_map = serde_json::Map::new();

    for (key, val) in upstream_resp.headers() {
        let key_str = key.as_str().to_string();
        let val_str = val.to_str().unwrap_or("<binary>").to_string();
        resp_headers_map.insert(key_str, Value::String(val_str));

        if key == header::TRANSFER_ENCODING || key == header::CONTENT_LENGTH {
            continue;
        }
        response_builder = response_builder.header(key, val);
    }

    // Log the response immediately (headers, status, time to first byte) without waiting for stream completion
    state.logger.log_raw(json!({
        "type": "response",
        "request_id": req_id,
        "status_code": status.as_u16(),
        "headers": Value::Object(resp_headers_map),
        "target_url": target_url,
        "duration_ms": start.elapsed().as_millis() as u64,
    }));
    if let Some(whitelist) = model_whitelist {
        if status.is_success() && whitelist != "*" {
            let response_bytes = upstream_resp.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("Failed to read models response: {}", e) })),
                )
            })?;

            let filtered_bytes = match serde_json::from_slice::<Value>(&response_bytes) {
                Ok(payload) => serde_json::to_vec(&filter_models_payload(payload, whitelist))
                    .unwrap_or_else(|_| response_bytes.to_vec()),
                Err(_) => response_bytes.to_vec(),
            };

            state.logger.log_raw(json!({
                "type": "stream_chunk",
                "request_id": req_id,
                "chunk_index": 1,
                "bytes": filtered_bytes.len(),
                "data": String::from_utf8_lossy(&filtered_bytes),
            }));

            return response_builder
                .body(Body::from(filtered_bytes))
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("Failed to build response: {}", e) })),
                    )
                });
        }
    }

    let raw_stream = upstream_resp.bytes_stream();
    let logger = state.logger.clone();
    let mut chunk_index: u64 = 0;

    let logging_stream = raw_stream.map(move |item| {
        match &item {
            Ok(bytes) => {
                chunk_index += 1;
                let chunk_str = String::from_utf8_lossy(bytes).into_owned();
                logger.log_raw(json!({
                    "type": "stream_chunk",
                    "request_id": req_id,
                    "chunk_index": chunk_index,
                    "bytes": bytes.len(),
                    "data": chunk_str,
                }));
            }
            Err(e) => {
                logger.log_raw(json!({
                    "type": "stream_error",
                    "request_id": req_id,
                    "error": format!("{}", e),
                }));
            }
        }
        item.map_err(|e| std::io::Error::other(format!("Stream error: {}", e)))
    });

    let body = Body::from_stream(logging_stream);
    response_builder.body(body).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to build response: {}", e) })),
        )
    })
}

async fn fallback_handler(
    State(state): State<Arc<AppState>>,
    OriginalUri(original_uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> impl IntoResponse {
    let req_id = state.logger.next_id();
    let start = Instant::now();

    state.logger.log_raw(json!({
        "type": "request",
        "request_id": req_id,
        "method": method.as_str(),
        "path": original_uri.path(),
        "query": original_uri.query(),
        "headers": headers_to_json(&headers),
        "body": null,
    }));

    let resp_body = json!({
        "error": {
            "message": format!("No route: {} {}", method, original_uri.path()),
            "type": "invalid_request_error"
        }
    });

    state.logger.log_raw(json!({
        "type": "response",
        "request_id": req_id,
        "status_code": 404,
        "headers": {
            "content-type": "application/json"
        },
        "body": resp_body,
        "duration_ms": start.elapsed().as_millis() as u64,
    }));

    (StatusCode::NOT_FOUND, Json(resp_body))
}
#[cfg(test)]
mod tests {
    use super::{filter_models_payload, wildcard_match};
    use serde_json::json;

    #[test]
    fn wildcard_match_supports_stars_and_single_character_wildcards() {
        assert!(wildcard_match("*oss*", "oss-emu/Test/Qwen"));
        assert!(wildcard_match("gpt-?", "gpt-5"));
        assert!(!wildcard_match("*oss*", "gpt-5.6-luna"));
    }

    #[test]
    fn filters_models_by_agent_whitelist() {
        let payload = json!({
            "object": "list",
            "data": [
                {"id": "oss-emu/Test/Qwen"},
                {"id": "gpt-5.6-luna"},
                {"id": "oss-emu/Test/GLM"}
            ]
        });

        let filtered = filter_models_payload(payload, "*oss*");
        let ids: Vec<&str> = filtered["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect();

        assert_eq!(ids, ["oss-emu/Test/Qwen", "oss-emu/Test/GLM"]);
    }
}
