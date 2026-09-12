//! Phase 8 offline differential tests: Rust gateway output vs recorded
//! original 9Router (npm 9router@0.5.75) fixtures, via nine-testing
//! normalize/shape/SSE helpers. Nondeterministic fields normalized first.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use nine_gateway::{router_with_state, AppState, Upstream};
use nine_testing::{normalize, parse_sse, shape, sse_event_types};
use tower::ServiceExt;

fn up(provider: &'static str, base: String, suffix: &str) -> Upstream {
    Upstream {
        provider,
        base_url: format!("{base}/{suffix}"),
        api_key: "k".into(),
    }
}

async fn call(
    app: Router,
    req: Request<Body>,
) -> (
    StatusCode,
    axum::http::HeaderMap,
    serde_json::Value,
    Vec<u8>,
) {
    let resp = app.oneshot(req).await.unwrap();
    let s = resp.status();
    let h = resp.headers().clone();
    let b = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    let v = serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null);
    (s, h, v, b)
}

fn chat_req(body: &str, auth: Option<&str>) -> Request<Body> {
    let mut b = Request::post("/v1/chat/completions").header("content-type", "application/json");
    if let Some(a) = auth {
        b = b.header("authorization", a);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

// Recorded from original 9Router: no/mismatched key, keyed mode.
const ORIG_BAD_KEY_ERR: &str = r#"{"error":{"message":"Invalid API key","type":"authentication_error","code":"invalid_api_key"}}"#;
const ORIG_MISSING_KEY_ERR: &str = r#"{"error":{"message":"Missing API key","type":"authentication_error","code":"invalid_api_key"}}"#;

#[tokio::test]
async fn diff_auth_missing_key_shape() {
    let app = router_with_state(AppState::new(vec![], 1000).with_api_keys(vec!["good".into()]));
    let (s, h, v, _) = call(app, chat_req(r#"{"model":"m"}"#, None)).await;
    let orig: serde_json::Value = serde_json::from_str(ORIG_MISSING_KEY_ERR).unwrap();
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(normalize(v.clone()), normalize(orig));
    assert!(h.contains_key("x-request-id"));
}

#[tokio::test]
async fn diff_auth_bad_key_shape() {
    let app = router_with_state(AppState::new(vec![], 1000).with_api_keys(vec!["good".into()]));
    let (s, _, v, _) = call(app, chat_req(r#"{"model":"m"}"#, Some("Bearer bad"))).await;
    let orig: serde_json::Value = serde_json::from_str(ORIG_BAD_KEY_ERR).unwrap();
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(shape(&v), shape(&orig));
}

#[tokio::test]
async fn diff_openai_passthrough_shape() {
    // Recorded original: passthrough preserves object/choices/usage envelope.
    let orig: serde_json::Value = serde_json::json!({
        "id": "chatcmpl-abc", "object": "chat.completion", "created": 1, "model": "gpt-4o",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
    });
    let app = Router::new().route("/o/chat/completions", axum::routing::post(|| async {
        axum::Json(serde_json::json!({
            "id": "chatcmpl-up", "object": "chat.completion", "created": 999, "model": "gpt-4o",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
        }))
    }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("openai", base, "o")], 5_000));
    let (s, h, v, _) = call(
        gw,
        chat_req(
            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
            Some("Bearer k"),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(shape(&v), shape(&orig));
    assert_eq!(normalize(v)["choices"], normalize(orig)["choices"]);
    assert!(h.contains_key("x-request-id"));
}

#[tokio::test]
async fn diff_anthropic_translation_shape() {
    // Original translates anthropic message -> openai choices[0].message + stop.
    let orig_shape: serde_json::Value = serde_json::json!({
        "id": "x", "object": "x", "created": 1, "model": "x",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 1},
    });
    let app = Router::new().route(
        "/messages",
        axum::routing::post(|| async {
            axum::Json(serde_json::json!({
                "id": "msg_01", "type": "message", "role": "assistant", "model": "claude",
                "content": [{"type": "text", "text": "hi"}],
                "stop_reason": "end_turn", "usage": {"input_tokens": 5, "output_tokens": 3}
            }))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("anthropic", base, "")], 5_000));
    let (s, _, v, _) = call(
        gw,
        chat_req(
            r#"{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}]}"#,
            Some("Bearer k"),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(shape(&v), shape(&orig_shape));
    assert_eq!(v["choices"][0]["message"]["content"], "hi");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn diff_openai_sse_event_sequence() {
    let body: &'static str = "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
    let app = Router::new().route(
        "/o/chat/completions",
        axum::routing::post(move || async {
            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(body.to_string()))
                .unwrap()
                .into_response()
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("openai", base, "o")], 5_000));
    let resp = gw
        .oneshot(chat_req(r#"{"model":"m","stream":true}"#, Some("Bearer k")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(sse_event_types(&text), vec!["message", "[DONE]"]);
    let chunks = parse_sse(&text);
    assert_eq!(chunks.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&chunks[0].1).unwrap();
    assert_eq!(v["choices"][0]["delta"]["content"], "hi");
}

#[tokio::test]
async fn diff_anthropic_sse_translated_to_openai_chunks() {
    // Original emits openai-style chunks for anthropic streams; every data
    // line must parse and carry choices[0].delta.
    let sse: &'static str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hey\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    let app = Router::new().route(
        "/messages",
        axum::routing::post(move || async {
            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(sse.to_string()))
                .unwrap()
                .into_response()
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("anthropic", base, "")], 5_000));
    let resp = gw
        .oneshot(chat_req(
            r#"{"model":"claude-x","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some("Bearer k"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("data: [DONE]"));
    let chunks = parse_sse(&text);
    assert!(!chunks.is_empty());
    for (_, d) in &chunks {
        let v: serde_json::Value = serde_json::from_str(d).unwrap();
        assert!(v.get("choices").is_some(), "chunk missing choices: {d}");
    }
    let joined: String = chunks
        .iter()
        .filter_map(|(_, d)| {
            serde_json::from_str::<serde_json::Value>(d)
                .ok()
                .and_then(|v| {
                    v["choices"][0]["delta"]["content"]
                        .as_str()
                        .map(str::to_string)
                })
        })
        .collect();
    assert!(joined.contains("hey"), "lost streamed text: {text}");
}

#[tokio::test]
async fn diff_upstream_error_envelope_preserved() {
    let app = Router::new().route("/o/chat/completions", axum::routing::post(|| async {
        (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"error": {"message": "bad req", "type": "invalid_request_error"}}))).into_response()
    }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("openai", base, "o")], 5_000));
    let (s, _, v, _) = call(gw, chat_req(r#"{"model":"m"}"#, Some("Bearer k"))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["message"], "bad req");
}

#[tokio::test]
async fn diff_responses_api_shape() {
    // Original /v1/responses returns object:"response" with output array.
    let app = Router::new().route("/o/chat/completions", axum::routing::post(|| async {
        axum::Json(serde_json::json!({
            "id": "chatcmpl-r", "object": "chat.completion", "created": 1, "model": "gpt-4o",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "yo"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
        }))
    }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let gw = router_with_state(AppState::new(vec![up("openai", base, "o")], 5_000));
    let (s, _, v, _) = call(gw, Request::post("/v1/responses").header("content-type", "application/json")
        .header("authorization", "Bearer k")
        .body(Body::from(r#"{"model":"gpt-4o","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}]}"#)).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["object"], "response");
    assert!(v.get("output").is_some());
    let text = serde_json::to_string(&v["output"]).unwrap();
    assert!(text.contains("yo"), "response text lost: {text}");
}

#[tokio::test]
async fn diff_health_version_keys() {
    let gw = router_with_state(AppState::new(vec![], 1000));
    let (s, _, v, _) = call(
        gw.clone(),
        Request::get("/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    let (s2, _, v2, _) = call(
        gw,
        Request::get("/api/version").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    for k in ["currentVersion", "latestVersion", "hasUpdate"] {
        assert!(v2.get(k).is_some(), "version missing {k}: {v2}");
    }
}
