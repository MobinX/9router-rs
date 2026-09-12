use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use nine_providers::Provider;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct Upstream {
    pub provider: &'static str,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone)]
pub struct AppState {
    pub version: &'static str,
    pub client: reqwest::Client,
    pub upstreams: Vec<Upstream>,
    pub timeout_ms: u64,
}

impl AppState {
    pub fn new(upstreams: Vec<Upstream>, timeout_ms: u64) -> Self {
        Self {
            version: "0.1.0",
            client: reqwest::Client::new(),
            upstreams,
            timeout_ms,
        }
    }
}

pub fn router() -> Router {
    router_with_state(AppState::new(Vec::new(), 30_000))
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/init", get(init))
        .route("/api/v1/models", get(models))
        .route("/v1/models", get(models))
        .route("/api/v1/chat/completions", post(chat_completions))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/v1/responses", post(responses))
        .route("/v1/responses", post(responses))
        .route("/codex/:path", post(responses))
        .route("/api/v1/messages", post(messages))
        .route("/v1/messages", post(messages))
        .route("/api/v1beta/models", get(models))
        .route("/v1beta/models", get(models))
        .route("/api/oauth/:provider", get(oauth_start))
        .route("/api/models", get(models))
        .route("/api/providers", get(providers))
        .route("/api/usage/stats", get(usage_stats))
        .route("/api/settings", get(settings))
        .route("/api/auth/status", get(auth_status))
        .route("/api/keys", get(keys))
        .with_state(Arc::new(state))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true}))
}

async fn version(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(
        serde_json::json!({"version": st.version, "currentVersion": st.version, "latestVersion": "0.5.75", "hasUpdate": false, "upstream": "0.5.75"}),
    )
}

async fn init() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "requiresLogin": false}))
}

async fn models() -> impl IntoResponse {
    Json(serde_json::json!({"object": "list", "data": [{"id": "gpt-4o", "object": "model"}]}))
}

async fn providers() -> impl IntoResponse {
    Json(
        serde_json::json!({"providers": nine_providers::PROVIDER_IDS.iter().take(5).collect::<Vec<_>>(), "total": nine_providers::PROVIDER_IDS.len()}),
    )
}

async fn usage_stats() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "stats": {}}))
}

async fn settings() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true}))
}

async fn auth_status() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "authenticated": false}))
}

async fn keys() -> impl IntoResponse {
    Json(serde_json::json!({"keys": []}))
}

async fn oauth_start(Path(provider): Path<String>) -> impl IntoResponse {
    if !nine_oauth::OAUTH_PROVIDERS.contains(&provider.as_str())
        && !nine_providers::is_known_provider(&provider)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": {"message": "unknown provider"}})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"provider": provider, "authorizeUrl": "https://accounts.example.com/oauth/authorize"})),
    )
        .into_response()
}

async fn messages(headers: HeaderMap, Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    if let Some(r) = require_auth(&headers) {
        return r;
    }
    let _ = body;
    (
        StatusCode::OK,
        Json(serde_json::json!({"id": "msg_1", "type": "message", "role": "assistant", "content": [{"type": "text", "text": "ok"}]})),
    )
        .into_response()
}

async fn responses(headers: HeaderMap, Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    if let Some(r) = require_auth(&headers) {
        return r;
    }
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o");
    (
        StatusCode::OK,
        Json(
            serde_json::json!({"id": "resp_1", "object": "response", "model": model, "output": []}),
        ),
    )
        .into_response()
}

fn require_auth(headers: &HeaderMap) -> Option<Response> {
    if headers.contains_key("authorization") {
        None
    } else {
        Some(err_inner(
            401,
            "missing authorization",
            "authentication_error",
        ))
    }
}

fn err_inner(status: u16, message: &str, typ: &str) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (
        code,
        Json(serde_json::json!({"error": {"message": message, "type": typ}})),
    )
        .into_response()
}

fn with_id(resp: Response, req_id: &str) -> Response {
    let (mut parts, body) = resp.into_parts();
    if let Ok(v) = req_id.parse() {
        parts.headers.insert("x-request-id", v);
    }
    Response::from_parts(parts, body)
}

async fn map_upstream_err(status: u16, resp: reqwest::Response) -> Response {
    let text = resp.text().await.unwrap_or_default();
    let trunc: String = text.chars().take(1000).collect();
    let msg = serde_json::from_str::<serde_json::Value>(&trunc)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or(trunc);
    let typ = match status {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        408 => "timeout_error",
        429 => "rate_limit_error",
        _ => "upstream_error",
    };
    err_inner(status, &msg, typ)
}

fn forward_json(bytes: Bytes, model: &str, req_id: &str) -> Response {
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(mut v) => {
            if let Some(o) = v.as_object_mut() {
                o.entry("id").or_insert(serde_json::json!(req_id));
                o.insert(
                    "model".into(),
                    serde_json::json!(nine_core::normalize_model_id(model)),
                );
                o.entry("usage").or_insert(serde_json::json!({
                    "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0
                }));
            }
            let body = serde_json::to_vec(&v).unwrap_or_default();
            with_id(
                (
                    StatusCode::OK,
                    Json(serde_json::from_slice::<serde_json::Value>(&body).unwrap()),
                )
                    .into_response(),
                req_id,
            )
        }
        Err(_) => {
            let resp = Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(bytes))
                .unwrap();
            with_id(resp, req_id)
        }
    }
}

async fn chat_completions(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = require_auth(&headers) {
        return r;
    }
    let req_id = nine_core::new_request_id();
    let v: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return with_id(
                err_inner(400, "invalid json", "invalid_request_error"),
                &req_id,
            )
        }
    };
    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o")
        .to_string();
    let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let (provider_opt, _) = nine_core::split_provider_model(&model);
    let mut ordered: Vec<&Upstream> = st.upstreams.iter().collect();
    ordered.sort_by_key(|u| {
        if Some(u.provider) == provider_opt {
            0
        } else {
            1
        }
    });
    if ordered.is_empty() {
        return with_id(
            err_inner(502, "no upstream configured", "upstream_error"),
            &req_id,
        );
    }
    let messages = v
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let role = m
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user")
                .to_string();
            let content = match m.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            nine_providers::ChatMessage { role, content }
        })
        .collect();
    let adapter = nine_providers::OpenAiPassthrough {
        provider_id: "openai",
        base_url: "",
    };
    let chat_req = nine_providers::ChatRequest {
        model: model.clone(),
        messages,
        stream,
    };
    let payload = adapter.translate_request(&chat_req);
    let mut last_err = err_inner(502, "all upstreams failed", "upstream_error");
    for up in ordered {
        let url = format!("{}/chat/completions", up.base_url.trim_end_matches('/'));
        let send = st
            .client
            .post(&url)
            .bearer_auth(&up.api_key)
            .json(&payload)
            .send();
        match tokio::time::timeout(Duration::from_millis(st.timeout_ms), send).await {
            Ok(Ok(resp)) => {
                let status = resp.status();
                if status.is_success() {
                    if stream {
                        let s = resp.bytes_stream();
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .header("x-request-id", req_id)
                            .body(Body::from_stream(s))
                            .unwrap();
                    }
                    match resp.bytes().await {
                        Ok(b) => return forward_json(b, &model, &req_id),
                        Err(_) => {
                            last_err = err_inner(502, "upstream read failed", "upstream_error");
                        }
                    }
                } else if nine_routing::should_retry(status.as_u16()) {
                    last_err = map_upstream_err(status.as_u16(), resp).await;
                } else {
                    return with_id(map_upstream_err(status.as_u16(), resp).await, &req_id);
                }
            }
            Ok(Err(_)) | Err(_) => {
                last_err = err_inner(504, "upstream timeout", "timeout_error");
            }
        }
    }
    with_id(last_err, &req_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn body_json(
        app: Router,
        req: Request<Body>,
    ) -> (StatusCode, HeaderMap, serde_json::Value) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, headers, v)
    }

    #[tokio::test]
    async fn health_ok() {
        let (s, _, v) = body_json(
            router(),
            Request::get("/api/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn chat_requires_auth() {
        let (s, _, _) = body_json(
            router(),
            Request::post("/api/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_no_upstream_502() {
        let (s, h, v) = body_json(
            router(),
            Request::post("/api/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from(r#"{"model":"m"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_GATEWAY);
        assert_eq!(v["error"]["type"], "upstream_error");
        assert!(h.contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn chat_invalid_json_400() {
        let (s, _, v) = body_json(
            router(),
            Request::post("/api/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from("{oops"))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"]["type"], "invalid_request_error");
    }

    #[tokio::test]
    async fn unknown_oauth_404() {
        let (s, _, _) = body_json(
            router(),
            Request::get("/api/oauth/no-such-provider-xyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
}
