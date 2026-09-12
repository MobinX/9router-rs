use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use nine_gateway::{router_with_state, AppState, Upstream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

struct Mock {
    base: String,
    flaky_hits: Arc<AtomicUsize>,
}

async fn spawn_mock() -> Mock {
    let flaky_hits = Arc::new(AtomicUsize::new(0));
    let hits = flaky_hits.clone();
    let app = axum::Router::new()
        .route(
            "/ok/chat/completions",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "id": "chatcmpl-up",
                    "object": "chat.completion",
                    "model": "gpt-4o",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
                }))
            }),
        )
        .route(
            "/flaky/chat/completions",
            axum::routing::post(move || {
                let hits = hits.clone();
                async move {
                    if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(serde_json::json!({"error": {"message": "boom"}})),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::NOT_FOUND,
                            axum::Json(serde_json::json!({"error": "unreachable"})),
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/denied/chat/completions",
            axum::routing::post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({"error": {"message": "bad key", "type": "auth_error"}})),
                )
                    .into_response()
            }),
        )
        .route(
            "/slow/chat/completions",
            axum::routing::post(|| async {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                axum::Json(serde_json::json!({})).into_response()
            }),
        )
        .route(
            "/sse/chat/completions",
            axum::routing::post(|| async {
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from(body))
                    .unwrap()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Mock {
        base: format!("http://127.0.0.1:{port}"),
        flaky_hits,
    }
}

fn up(provider: &'static str, base: String, suffix: &str) -> Upstream {
    Upstream {
        provider,
        base_url: format!("{base}/{suffix}"),
        api_key: "test-key".into(),
    }
}

async fn post(
    app: axum::Router,
    path: &str,
    body: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
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
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

#[tokio::test]
async fn non_stream_passthrough_shape() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("openai", mock.base, "ok")], 5_000));
    let (s, h, b) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"GPT_4o","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["model"], "gpt-4o");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["total_tokens"], 5);
    assert!(h.contains_key("x-request-id"));
}

#[tokio::test]
async fn sse_bytes_passthrough() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("openai", mock.base, "sse")], 5_000));
    let resp = app
        .oneshot(
            Request::post("/v1/chat/completions")
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
    assert!(resp.headers().contains_key("x-request-id"));
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("data: [DONE]"));
}

#[tokio::test]
async fn upstream_401_no_fallback() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(
        vec![
            up("openai", mock.base.clone(), "denied"),
            up("openai", mock.base, "ok"),
        ],
        5_000,
    ));
    let (s, _, b) = post(app, "/v1/chat/completions", r#"{"model":"m"}"#).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["error"]["message"], "bad key");
}

#[tokio::test]
async fn retryable_500_falls_through() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(
        vec![
            up("openai", mock.base.clone(), "flaky"),
            up("openai", mock.base, "ok"),
        ],
        5_000,
    ));
    let (s, _, b) = post(app, "/v1/chat/completions", r#"{"model":"m"}"#).await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["id"], "chatcmpl-up");
    assert_eq!(mock.flaky_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn slow_upstream_times_out() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(vec![up("openai", mock.base, "slow")], 100));
    let (s, _, b) = post(app, "/v1/chat/completions", r#"{"model":"m"}"#).await;
    assert_eq!(s, StatusCode::GATEWAY_TIMEOUT);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["error"]["type"], "timeout_error");
}

#[tokio::test]
async fn provider_prefix_picks_matching_upstream() {
    let mock = spawn_mock().await;
    let app = router_with_state(AppState::new(
        vec![
            up("beta", mock.base.clone(), "denied"),
            up("alpha", mock.base, "ok"),
        ],
        5_000,
    ));
    let (s, _, _) = post(app, "/v1/chat/completions", r#"{"model":"alpha/gpt-4o"}"#).await;
    assert_eq!(s, StatusCode::OK);
}
