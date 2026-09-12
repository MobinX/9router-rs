use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use nine_gateway::{router_with_state, AppState, Upstream};
use tower::ServiceExt;

fn anthropic_body() -> serde_json::Value {
    serde_json::json!({
        "id": "msg_01",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "hello from claude"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 12, "output_tokens": 4}
    })
}

fn gemini_body() -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"parts": [{"text": "hello from gemini"}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 9, "candidatesTokenCount": 5}
    })
}

const ANTHROPIC_SSE: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

async fn spawn_mock() -> String {
    let app = Router::new()
        .route(
            "/messages",
            post(|body: axum::Json<serde_json::Value>| async move {
                if body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false) {
                    return axum::response::Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from(ANTHROPIC_SSE))
                        .unwrap()
                        .into_response();
                }
                axum::Json(anthropic_body()).into_response()
            }),
        )
        .route(
            "/models/*rest",
            post(|_body: axum::Json<serde_json::Value>| async move {
                axum::Json(gemini_body()).into_response()
            }),
        )
        .route(
            "/rate/messages",
            post(|| async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(serde_json::json!({"error": {"message": "slow down", "type": "rate_limit_error"}})),
                )
                    .into_response()
            }),
        )
        .route(
            "/bad/messages",
            post(|| async {
                axum::response::Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .unwrap()
                    .into_response()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

async fn call(app: Router, path: &str, body: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}

fn up(provider: &'static str, base: String) -> Upstream {
    Upstream {
        provider,
        base_url: base,
        api_key: "k".into(),
    }
}

#[tokio::test]
async fn anthropic_translates_to_openai_shape() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("anthropic", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"anthropic/claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["content"], "hello from claude");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["prompt_tokens"], 12);
    assert_eq!(v["usage"]["completion_tokens"], 4);
}

#[tokio::test]
async fn anthropic_stream_translates_to_openai_chunks() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("anthropic", base)], 5_000));
    let resp = app
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from(
                    r#"{"model":"anthropic/claude-sonnet-4-6","stream":true,"messages":[]}"#,
                ))
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
    assert!(
        text.contains("\"content\":\"hello\""),
        "missing content delta: {text}"
    );
    assert!(text.contains("chat.completion.chunk"));
    assert!(text.contains("\"finish_reason\":\"stop\""));
    assert!(text.trim_end().ends_with("data: [DONE]"));
}

#[tokio::test]
async fn gemini_translates_to_openai_shape() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("gemini", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"gemini/gemini-2.5-pro","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello from gemini");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["total_tokens"], 14);
}

#[tokio::test]
async fn anthropic_rate_limit_falls_back() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(
        vec![
            Upstream {
                provider: "anthropic",
                base_url: format!("{base}/rate"),
                api_key: "k".into(),
            },
            Upstream {
                provider: "anthropic",
                base_url: base,
                api_key: "k".into(),
            },
        ],
        5_000,
    ));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"anthropic/claude-sonnet-4-6"}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello from claude");
}

#[tokio::test]
async fn anthropic_malformed_json_502() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(
        vec![Upstream {
            provider: "anthropic",
            base_url: format!("{base}/bad"),
            api_key: "k".into(),
        }],
        5_000,
    ));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"anthropic/claude-sonnet-4-6"}"#,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["error"]["code"], "malformed_json");
}

#[tokio::test]
async fn messages_endpoint_passthrough() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("anthropic", base)], 5_000));
    let (s, b) = call(app, "/v1/messages", r#"{"model":"claude-sonnet-4-6","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#).await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["type"], "message");
    assert_eq!(v["content"][0]["text"], "hello from claude");
}

#[tokio::test]
async fn messages_stream_passthrough_keeps_anthropic_events() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("anthropic", base)], 5_000));
    let resp = app
        .oneshot(
            Request::post("/v1/messages")
                .header("content-type", "application/json")
                .header("x-api-key", "k")
                .body(Body::from(
                    r#"{"model":"claude-sonnet-4-6","stream":true,"max_tokens":16,"messages":[]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        text.contains("message_start"),
        "anthropic events must pass through: {text}"
    );
    assert!(text.contains("message_stop"));
}

#[tokio::test]
async fn gemini_generate_content_passthrough() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("gemini", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1beta/models/gemini/gemini-2.5-pro:generateContent",
        r#"{"contents":[{"parts":[{"text":"hi"}]}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(
        v["candidates"][0]["content"]["parts"][0]["text"],
        "hello from gemini"
    );
}

#[tokio::test]
async fn responses_wire_translates_to_chat_shape() {
    let app = Router::new().route(
        "/responses",
        post(|body: axum::Json<serde_json::Value>| async move {
            assert_eq!(body["model"], "gpt-5");
            assert_eq!(body["input"][0]["content"][0]["text"], "hi");
            axum::Json(serde_json::json!({
                "id": "resp_9", "object": "response",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "codex says hi"}]}],
                "usage": {"input_tokens": 6, "output_tokens": 3},
            }))
            .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let app = router_with_state(AppState::new(vec![up("codex", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"codex/gpt-5","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["content"], "codex says hi");
    assert_eq!(v["usage"]["total_tokens"], 9);
}

#[tokio::test]
async fn native_wire_returns_explicit_501() {
    let base = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("cursor", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"cursor/auto","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["error"]["code"], "model_not_supported");
}

#[tokio::test]
async fn cline_envelope_unwrapped_to_openai_shape() {
    let app_mock = Router::new().route(
        "/chat/completions",
        post(|_body: axum::Json<serde_json::Value>| async {
            axum::Json(serde_json::json!({
                "success": true,
                "data": {
                    "id": "chatcmpl-cline",
                    "object": "chat.completion",
                    "created": 1700000000,
                    "model": "x",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "cline says hi"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8}
                }
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app_mock).await.unwrap() });
    let app = router_with_state(AppState::new(vec![up("cline", base)], 5_000));
    let (s, b) = call(
        app,
        "/v1/chat/completions",
        r#"{"model":"cline/anthropic-claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}]}"#
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "cline says hi");
    assert!(v.get("success").is_none(), "envelope must be unwrapped");
}
