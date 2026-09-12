use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use nine_gateway::{router_with_state, AppState, Upstream};
use tower::ServiceExt;

async fn spawn_mock_openai() -> String {
    let app = Router::new().route(
        "/chat/completions",
        post(|| async {
            axum::Json(serde_json::json!({
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "responses api output" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
            }))
            .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

async fn json_request(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let v: serde_json::Value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, v)
}

#[tokio::test]
async fn shutdown_endpoint_matches_production_policy() {
    let app = router_with_state(AppState::new(Vec::new(), 5_000));
    let (s, v) = json_request(
        app,
        Request::post("/api/shutdown").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["success"], false);
    assert_eq!(v["message"], "Not allowed in production");
}

#[tokio::test]
async fn cli_tools_all_statuses_returns_all_tools() {
    let app = router_with_state(AppState::new(Vec::new(), 5_000));
    let (s, v) = json_request(
        app,
        Request::get("/api/cli-tools/all-statuses")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let obj = v.as_object().unwrap();
    for tool in [
        "claude",
        "codex",
        "opencode",
        "droid",
        "openclaw",
        "hermes",
        "cowork",
        "cline",
        "kilo",
        "deepseek-tui",
        "jcode",
        "grok-build",
        "devin",
    ] {
        assert!(obj.contains_key(tool), "Missing tool status: {tool}");
        assert!(obj[tool]["installed"].is_boolean());
    }
}

#[tokio::test]
async fn cli_tools_individual_get_and_post() {
    let app = router_with_state(AppState::new(Vec::new(), 5_000));
    // Check claude (not installed in container)
    let (s1, v1) = json_request(
        app.clone(),
        Request::get("/api/cli-tools/claude-settings")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(v1["installed"], false);

    // Missing fields on POST returns 400
    let (s2, v2) = json_request(
        app.clone(),
        Request::post("/api/cli-tools/codex-settings")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"model":"gpt-4o"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(s2, StatusCode::BAD_REQUEST);
    assert_eq!(v2["error"]["code"], "missing_fields");
}

#[tokio::test]
async fn responses_api_translates_and_formats_output() {
    let mock_url = spawn_mock_openai().await;
    let upstreams = vec![Upstream {
        provider: "openai",
        base_url: mock_url,
        api_key: "k".into(),
    }];
    let app = router_with_state(AppState::new(upstreams, 5_000));

    // Request using wire responses format: input: [{type: "message", role: "user", content: [...]}]
    let req_body = serde_json::json!({
        "model": "openai/gpt-4o",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello responses" }
                ]
            }
        ]
    });

    let (s, v) = json_request(
        app.clone(),
        Request::post("/v1/responses")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test")
            .body(Body::from(req_body.to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["object"], "response");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["model"], "gpt-4o");
    assert_eq!(v["output"][0]["type"], "message");
    assert_eq!(v["output"][0]["role"], "assistant");
    assert_eq!(v["output"][0]["content"][0]["text"], "responses api output");
    assert_eq!(v["usage"]["total_tokens"], 15);

    // Also test the /codex/:path rewrite
    let (s2, v2) = json_request(
        app,
        Request::post("/codex/responses")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test")
            .body(Body::from(req_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(v2["object"], "response");
    assert_eq!(
        v2["output"][0]["content"][0]["text"],
        "responses api output"
    );
}
