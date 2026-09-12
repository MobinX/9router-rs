use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub version: &'static str,
}

pub fn router() -> Router {
    let st = Arc::new(AppState { version: "0.1.0" });
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
        .with_state(st)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true}))
}

async fn version(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(serde_json::json!({"version": st.version, "upstream": "0.5.75"}))
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
    let g = nine_oauth::GenericOAuth("generic", "https://accounts.example.com");
    let _ = g;
    (StatusCode::OK, Json(serde_json::json!({"provider": provider, "authorizeUrl": "https://accounts.example.com/oauth/authorize"}))).into_response()
}

async fn messages(headers: HeaderMap, Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    if let Some(r) = require_auth(&headers) {
        return r;
    }
    let _ = body;
    (StatusCode::OK, Json(serde_json::json!({"id": "msg_1", "type": "message", "role": "assistant", "content": [{"type": "text", "text": "ok"}] }))).into_response()
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
        Some((StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": {"message": "missing authorization", "type": "auth_error"}}))).into_response())
    }
}

async fn chat_completions(
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(r) = require_auth(&headers) {
        return r;
    }
    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o")
        .to_string();
    let request_id = nine_core::new_request_id();
    if stream {
        let sse_body = format!(
            "data: {{\"id\":\"{request_id}\",\"object\":\"chat.completion.chunk\",\"model\":\"{model}\",\"choices\":[{{\"delta\":{{\"content\":\"ok\"}},\"index\":0,\"finish_reason\":null}}]}}\n\n             data: {{\"id\":\"{request_id}\",\"object\":\"chat.completion.chunk\",\"model\":\"{model}\",\"choices\":[{{\"delta\":{{}},\"index\":0,\"finish_reason\":\"stop\"}}]}}\n\n             data: [DONE]\n\n"
        );
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("x-request-id", request_id)
            .body(Body::from(sse_body))
            .unwrap();
    }
    let resp = serde_json::json!({
        "id": request_id,
        "object": "chat.completion",
        "model": nine_core::normalize_model_id(&model),
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
    });
    (StatusCode::OK, Json(resp)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn body_json(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, v)
    }

    #[tokio::test]
    async fn health_ok() {
        let (s, v) = body_json(
            router(),
            Request::get("/api/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn chat_requires_auth() {
        let (s, _) = body_json(
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
    async fn chat_non_stream_shape() {
        let (s, v) = body_json(
            router(),
            Request::post("/api/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from(r#"{"model":"GPT_4o","messages":[]}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["model"], "gpt-4o");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn chat_stream_is_sse() {
        let resp = router()
            .oneshot(
                Request::post("/api/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer k")
                    .body(Body::from(r#"{"model":"m","stream":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/event-stream"));
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn unknown_oauth_404() {
        let (s, _) = body_json(
            router(),
            Request::get("/api/oauth/no-such-provider-xyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
}
