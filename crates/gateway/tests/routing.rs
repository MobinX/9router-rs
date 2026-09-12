use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use nine_gateway::{router_with_state, AppState, Upstream};
use nine_storage::Store;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

async fn spawn_mock_models() -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let m1_hits = Arc::new(AtomicUsize::new(0));
    let m2_hits = Arc::new(AtomicUsize::new(0));
    let h1 = m1_hits.clone();
    let h2 = m2_hits.clone();

    let app = Router::new()
        .route(
            "/m1/chat/completions",
            post(move || {
                let h1 = h1.clone();
                async move {
                    let hits = h1.fetch_add(1, Ordering::SeqCst);
                    if hits == 0 {
                        // First attempt fails with 429 to trigger fallback
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            axum::Json(serde_json::json!({ "error": { "message": "rate limited", "type": "rate_limit_error" } })),
                        )
                            .into_response()
                    } else {
                        axum::Json(serde_json::json!({
                            "id": "cmpl-m1",
                            "object": "chat.completion",
                            "model": "model-1",
                            "choices": [{ "index": 0, "message": { "role": "assistant", "content": "from-m1" }, "finish_reason": "stop" }],
                        }))
                        .into_response()
                    }
                }
            }),
        )
        .route(
            "/m2/chat/completions",
            post(move || {
                let h2 = h2.clone();
                async move {
                    h2.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "id": "cmpl-m2",
                        "object": "chat.completion",
                        "model": "model-2",
                        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "from-m2" }, "finish_reason": "stop" }],
                    }))
                    .into_response()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), m1_hits, m2_hits)
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
async fn combo_crud_api_endpoints() {
    let store = Arc::new(Store::open_memory().unwrap());
    let app = router_with_state(AppState::new(Vec::new(), 5_000).with_store(store));

    // 1. List initially empty
    let (s, v) = json_request(
        app.clone(),
        Request::get("/api/combos").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["combos"].as_array().unwrap().len(), 0);

    // 2. Create combo
    let req_body = serde_json::json!({
        "name": "blend-1",
        "models": ["openai/gpt-4o", "anthropic/claude-sonnet"]
    });
    let (s, v) = json_request(
        app.clone(),
        Request::post("/api/combos")
            .header("content-type", "application/json")
            .body(Body::from(req_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(v["name"], "blend-1");
    let combo_id = v["id"].as_str().unwrap().to_string();

    // 3. Duplicate name rejected with 400
    let (s, v) = json_request(
        app.clone(),
        Request::post("/api/combos")
            .header("content-type", "application/json")
            .body(Body::from(req_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "name_exists");

    // 4. Get by ID
    let (s, v) = json_request(
        app.clone(),
        Request::get(format!("/api/combos/{combo_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["id"], combo_id);
    assert_eq!(v["models"].as_array().unwrap().len(), 2);

    // 5. Update
    let update_body = serde_json::json!({
        "models": ["openai/gpt-4o"]
    });
    let (s, v) = json_request(
        app.clone(),
        Request::put(format!("/api/combos/{combo_id}"))
            .header("content-type", "application/json")
            .body(Body::from(update_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"].as_array().unwrap().len(), 1);

    // 6. Delete
    let (s, v) = json_request(
        app.clone(),
        Request::delete(format!("/api/combos/{combo_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);

    // 7. Get deleted -> 404
    let (s, _) = json_request(
        app,
        Request::get(format!("/api/combos/{combo_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn model_alias_crud_api_endpoints() {
    let store = Arc::new(Store::open_memory().unwrap());
    let app = router_with_state(AppState::new(Vec::new(), 5_000).with_store(store));

    // 1. Initially empty
    let (s, v) = json_request(
        app.clone(),
        Request::get("/api/models/alias")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["aliases"].as_object().unwrap().is_empty());

    // 2. Put alias
    let put_body = serde_json::json!({
        "alias": "fast",
        "model": "openai/gpt-4o-mini"
    });
    let (s, v) = json_request(
        app.clone(),
        Request::put("/api/models/alias")
            .header("content-type", "application/json")
            .body(Body::from(put_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);

    // 3. Verify in list
    let (s, v) = json_request(
        app.clone(),
        Request::get("/api/models/alias")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["aliases"]["fast"], "openai/gpt-4o-mini");

    // 4. Delete alias
    let (s, v) = json_request(
        app.clone(),
        Request::delete("/api/models/alias?alias=fast")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);

    // 5. Verify deleted
    let (s, v) = json_request(
        app,
        Request::get("/api/models/alias")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["aliases"].get("fast").is_none());
}

#[tokio::test]
async fn combo_fallback_across_models() {
    let (base_url, m1_hits, m2_hits) = spawn_mock_models().await;
    let store = Arc::new(Store::open_memory().unwrap());

    // Register a combo with model-1 and model-2
    let combo = nine_storage::Combo {
        id: "c1".into(),
        name: "DualCombo".into(),
        kind: Some("llm".into()),
        models: vec!["m1/model-1".into(), "m2/model-2".into()],
        created_at: "2026-09-13T00:00:00Z".into(),
        updated_at: "2026-09-13T00:00:00Z".into(),
    };
    store.upsert_combo(&combo).unwrap();

    let upstreams = vec![
        Upstream {
            provider: "m1",
            base_url: format!("{base_url}/m1"),
            api_key: "k1".into(),
        },
        Upstream {
            provider: "m2",
            base_url: format!("{base_url}/m2"),
            api_key: "k2".into(),
        },
    ];

    let app = router_with_state(AppState::new(upstreams, 5_000).with_store(store));

    // Request to DualCombo: model-1 will fail with 429, gateway falls back to model-2
    let chat_body = serde_json::json!({
        "model": "DualCombo",
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let (s, v) = json_request(
        app,
        Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test")
            .body(Body::from(chat_body.to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["choices"][0]["message"]["content"], "from-m2");
    assert_eq!(m1_hits.load(Ordering::SeqCst), 1);
    assert_eq!(m2_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn alias_chain_routes_to_target() {
    let (base_url, _, m2_hits) = spawn_mock_models().await;
    let store = Arc::new(Store::open_memory().unwrap());

    // Chain: "my-alias" -> "middle-alias" -> "m2/model-2"
    store.set_model_alias("my-alias", "middle-alias").unwrap();
    store.set_model_alias("middle-alias", "m2/model-2").unwrap();

    let upstreams = vec![Upstream {
        provider: "m2",
        base_url: format!("{base_url}/m2"),
        api_key: "k2".into(),
    }];
    let app = router_with_state(AppState::new(upstreams, 5_000).with_store(store));

    let chat_body = serde_json::json!({
        "model": "my-alias",
        "messages": [{ "role": "user", "content": "hello" }]
    });
    let (s, v) = json_request(
        app,
        Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test")
            .body(Body::from(chat_body.to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["choices"][0]["message"]["content"], "from-m2");
    assert_eq!(m2_hits.load(Ordering::SeqCst), 1);
}
