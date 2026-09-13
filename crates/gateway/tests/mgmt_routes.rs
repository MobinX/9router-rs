use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use nine_gateway::{router_with_state, AppState, Upstream};
use nine_providers::ModelEntry;
use nine_storage::Store;
use std::sync::Arc;
use tower::ServiceExt;

fn entry(id: &str) -> ModelEntry {
    ModelEntry {
        id: id.into(),
        name: id.rsplit('/').next().unwrap_or(id).into(),
        kind: "llm".into(),
        owned_by: id.split('/').next().unwrap_or("openai").into(),
        endpoint: "/v1/chat/completions".into(),
    }
}

fn app() -> Router {
    router_with_state(
        AppState::new(Vec::new(), 5_000)
            .with_store(Arc::new(Store::open_memory().unwrap()))
            .with_catalog(vec![
                entry("openai/gpt-4o"),
                entry("anthropic/claude-opus-4-6"),
            ]),
    )
}

async fn req(
    app: Router,
    req: Request<Body>,
) -> (StatusCode, serde_json::Value, axum::http::HeaderMap) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, v, headers)
}

fn json(method: &str, path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn get(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
    let (s, v, _) = req(app, Request::get(path).body(Body::empty()).unwrap()).await;
    (s, v)
}

// ─── keys ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn keys_validation_and_lifecycle() {
    let a = app();
    let (s, _, _) = req(a.clone(), json("POST", "/api/keys", serde_json::json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json("POST", "/api/keys", serde_json::json!({"name": "t"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(v["key"].as_str().unwrap().starts_with("sk-"));
    let id = v["id"].as_str().unwrap().to_string();
    let (s, v) = get(a.clone(), "/api/keys").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["keys"].as_array().unwrap().len(), 1);
    let (s, _) = get(a.clone(), &format!("/api/keys/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PUT",
            &format!("/api/keys/{id}"),
            serde_json::json!({"isActive": false}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["key"]["isActive"], false);
    let (s, _, _) = req(
        a.clone(),
        Request::delete(format!("/api/keys/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = get(a.clone(), &format!("/api/keys/{id}")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ─── locale / tags ──────────────────────────────────────────────────────────

#[tokio::test]
async fn locale_validation() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/locale", serde_json::json!({"locale": "xx"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, h) = req(
        a.clone(),
        json("POST", "/api/locale", serde_json::json!({"locale": "VI"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["locale"], "vi");
    assert!(h
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("locale=vi"));
}

#[tokio::test]
async fn tags_shape() {
    let (s, v) = get(app(), "/api/tags").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"].as_array().unwrap().len(), 2);
}

// ─── pricing / settings ─────────────────────────────────────────────────────

#[tokio::test]
async fn pricing_roundtrip() {
    let a = app();
    let (s, v) = get(a.clone(), "/api/pricing").await;
    assert_eq!((s, v), (StatusCode::OK, serde_json::json!({})));
    let (s, _, _) = req(
        a.clone(),
        json("PATCH", "/api/pricing", serde_json::json!({"x": 1})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PATCH",
            "/api/pricing",
            serde_json::json!({"openai": {"gpt-4o": {"input": 1}}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["openai"]["gpt-4o"]["input"], 1);
    let (s, _, _) = req(
        a.clone(),
        Request::delete("/api/pricing").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = get(a.clone(), "/api/pricing").await;
    assert_eq!((s, v), (StatusCode::OK, serde_json::json!({})));
}

#[tokio::test]
async fn settings_patch_and_require_login() {
    let a = app();
    let (s, v, _) = req(
        a.clone(),
        json(
            "PATCH",
            "/api/settings",
            serde_json::json!({"tunnelUrl": "https://x"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["tunnelUrl"], "https://x");
    let (s, v) = get(a.clone(), "/api/settings").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["tunnelUrl"], "https://x");
    let (s, v) = get(a.clone(), "/api/settings/require-login").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["requireLogin"], true);
}

#[tokio::test]
async fn database_export_import() {
    let a = app();
    let (s, v) = get(a.clone(), "/api/settings/database").await;
    assert_eq!(s, StatusCode::OK);
    assert!(v.get("connections").is_some());
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/settings/database",
            serde_json::json!({"settings": {"a": 1}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
}

#[tokio::test]
async fn proxy_test_rejects_garbage() {
    let (s, v, _) = req(
        app(),
        json(
            "POST",
            "/api/settings/proxy-test",
            serde_json::json!({"proxyUrl": "not a url"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], false);
}

// ─── providers ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn provider_crud_and_validate() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/providers", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers",
            serde_json::json!({"provider": "nope"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers",
            serde_json::json!({"provider": "openai", "authType": "apiKey", "apiKey": "k"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(v.get("data").is_none());
    let id = v["id"].as_str().unwrap().to_string();
    let (s, _) = get(a.clone(), &format!("/api/providers/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PUT",
            &format!("/api/providers/{id}"),
            serde_json::json!({"priority": 3}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["priority"], 3);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers/validate",
            serde_json::json!({"provider": "openai"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers/validate",
            serde_json::json!({"provider": "openai", "baseUrl": "::::"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = get(a.clone(), "/api/providers/suggested-models?provider=openai").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"][0]["id"], "openai/gpt-4o");
    let (s, v) = get(a.clone(), "/api/providers/client").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["connections"].as_array().unwrap().len(), 1);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers/test-batch",
            serde_json::json!({"ids": []}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let _ = v;
    let (s, _, _) = req(
        a.clone(),
        Request::delete(format!("/api/providers/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = get(a.clone(), &format!("/api/providers/{id}")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn provider_live_probe_against_mock() {
    let mock = Router::new().route(
        "/models",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({"data": [{"id": "gpt-4o"}]}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let base = format!("http://127.0.0.1:{port}");
    let a = router_with_state(
        AppState::new(Vec::new(), 5_000).with_store(Arc::new(Store::open_memory().unwrap())),
    );
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/providers",
            serde_json::json!({"provider": "openai", "baseUrl": base, "apiKey": "k"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let id = v["id"].as_str().unwrap().to_string();
    let (s, v) = get(a.clone(), &format!("/api/providers/{id}/models")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"][0]["id"], "gpt-4o");
    let (s, v, _) = req(
        a.clone(),
        Request::post(format!("/api/providers/{id}/test"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
}

// ─── models extras ──────────────────────────────────────────────────────────

#[tokio::test]
async fn models_extras() {
    let a = app();
    let (s, _, _) = req(a.clone(), json("PUT", "/api/models", serde_json::json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PUT",
            "/api/models",
            serde_json::json!({"model": "openai/gpt-4o", "alias": "fast"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(a.clone(), "/api/models/alias").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["aliases"]["openai/gpt-4o"], "fast");
    let (s, v) = get(a.clone(), "/api/models/catalog-sync").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"], 2);
    assert_eq!(v["providers"], 2);
    let (s, v, _) = req(
        a.clone(),
        json("POST", "/api/models/catalog-sync", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/models/custom",
            serde_json::json!({"id": "c1"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(a.clone(), "/api/models/custom").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"].as_array().unwrap().len(), 1);
    let (s, _, _) = req(
        a.clone(),
        Request::delete("/api/models/custom?id=c1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/models/disabled",
            serde_json::json!({"providerAlias": "openai", "ids": ["gpt-4o"]}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(a.clone(), "/api/models/disabled?providerAlias=openai").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ids"][0], "gpt-4o");
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/models/test",
            serde_json::json!({"model": "openai/gpt-4o"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/models/test",
            serde_json::json!({"model": "nope/nada"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], false);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/models/availability",
            serde_json::json!({"connectionId": "c", "model": "m"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(a.clone(), "/api/models/availability").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["models"][0]["connectionId"], "c");
}

// ─── usage ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn usage_routes() {
    let a = app();
    for p in [
        "/api/usage/history",
        "/api/usage/chart",
        "/api/usage/logs",
        "/api/usage/request-logs",
        "/api/usage/request-details",
        "/api/usage/providers",
        "/api/usage/stream",
    ] {
        let (s, _) = get(a.clone(), p).await;
        assert_eq!(s, StatusCode::OK, "{p}");
    }
    let (s, _, _) = req(
        a.clone(),
        Request::get("/api/usage/chart?period=bogus")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(a.clone(), "/api/usage/does-not-exist").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = req(
        a.clone(),
        Request::get("/api/usage/does-not-exist/codex-reset-credits")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ─── oauth imports ──────────────────────────────────────────────────────────

#[tokio::test]
async fn oauth_imports() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/oauth/codex/import-token",
            serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJlbWFpbCI6InRAdC5jb20ifQ.c2ln";
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/oauth/codex/import-token",
            serde_json::json!({"accessToken": jwt}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    assert_eq!(v["provider"], "codex");
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/oauth/codex/bulk-import",
            serde_json::json!({"tokens": ["a", "b", ""]}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["imported"], 2);
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/oauth/gitlab/pat", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/oauth/cursor/import",
            serde_json::json!({"token": "ct"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["provider"], "cursor");
    let (s, v) = get(a.clone(), "/api/oauth/cursor/auto-import").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["imported"], 0);
    let (s, _, _) = req(
        a.clone(),
        Request::get("/api/oauth/kiro/social-authorize?provider=x")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        Request::get("/api/oauth/kiro/social-authorize?provider=google")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["authUrl"]
        .as_str()
        .unwrap()
        .contains("prod.us-east-1.auth.desktop.kiro.dev"));
    assert!(v["authUrl"].as_str().unwrap().contains("idp=Google"));
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/oauth/kiro/social-exchange",
            serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ─── cli extras / tts ───────────────────────────────────────────────────────

#[tokio::test]
async fn cli_tool_extras() {
    let a = app();
    let (s, v) = get(a.clone(), "/api/cli-tools/antigravity-mitm").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["running"], false);
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/cli-tools/antigravity-mitm",
            serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PATCH",
            "/api/cli-tools/antigravity-mitm",
            serde_json::json!({"base": "http://x"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v, _) = req(
        a.clone(),
        json(
            "PUT",
            "/api/cli-tools/antigravity-mitm/alias",
            serde_json::json!({"tool": "codex", "aliases": {"a": "b"}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(
        a.clone(),
        "/api/cli-tools/antigravity-mitm/alias?tool=codex",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["aliases"]["a"], "b");
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/cli-tools/cowork-mcp-tools",
            serde_json::json!({"url": "nope"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/cli-tools/cowork-mcp-tools",
            serde_json::json!({"url": "http://localhost:9"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tts_directory() {
    let (s, v) = get(app(), "/api/media-providers/tts/voices").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["voices"], serde_json::json!([]));
    let (s, _) = get(app(), "/api/media-providers/tts/elevenlabs/voices").await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
}

// ─── v1 media ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn media_without_upstream_is_501() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/v1/embeddings",
            serde_json::json!({"input": "hi"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, _) = get(a.clone(), "/api/v1/videos/abc").await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn media_passthrough_with_mock() {
    let mock = Router::new().route(
        "/v1/embeddings",
        axum::routing::post(|| async {
            axum::Json(serde_json::json!({"object": "list", "data": []}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let a = router_with_state(
        AppState::new(
            vec![Upstream {
                provider: "openai",
                base_url: format!("http://127.0.0.1:{port}"),
                api_key: "k".into(),
            }],
            5_000,
        )
        .with_store(Arc::new(Store::open_memory().unwrap())),
    );
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/v1/embeddings",
            serde_json::json!({"input": "hi"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["object"], "list");
    let (s, v, _) = req(
        a.clone(),
        json("POST", "/v1/embeddings", serde_json::json!({"input": "hi"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["object"], "list");
    let (s, _, _) = req(
        a.clone(),
        Request::post("/api/v1/api/chat")
            .header("content-type", "application/json")
            .body(Body::from("{bad"))
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_model_detail() {
    let (s, _) = get(app(), "/api/v1/models/openai/gpt-4o").await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = get(app(), "/api/v1/models/nope/nada").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ─── auth ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn local_auth_flow() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/auth/login",
            serde_json::json!({"password": "wrong"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, h) = req(
        a.clone(),
        json(
            "POST",
            "/api/auth/login",
            serde_json::json!({"password": "123456"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let cookie = h.get("set-cookie").unwrap().to_str().unwrap().to_string();
    let token = cookie.split(';').next().unwrap();
    let (s, v, _) = req(
        a.clone(),
        Request::get("/api/auth/status")
            .header("cookie", token)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["authenticated"], true);
    let (s, v, _) = req(
        a.clone(),
        Request::post("/api/auth/logout")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/auth/reset-password",
            serde_json::json!({"newPassword": "x"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(a.clone(), "/api/auth/saml/metadata").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

// ─── nodes / pools ──────────────────────────────────────────────────────────

#[tokio::test]
async fn nodes_crud() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/provider-nodes/validate",
            serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/provider-nodes",
            serde_json::json!({"name": "n1", "type": "t"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let id = v["id"].as_str().unwrap().to_string();
    let (s, v) = get(a.clone(), "/api/provider-nodes").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["nodes"].as_array().unwrap().len(), 1);
    let (s, _, _) = req(
        a.clone(),
        json(
            "PUT",
            &format!("/api/provider-nodes/{id}"),
            serde_json::json!({"name": "n2"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = req(
        a.clone(),
        Request::delete(format!("/api/provider-nodes/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn pools_crud_and_test() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/proxy-pools", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/proxy-pools",
            serde_json::json!({"name": "p", "proxyUrl": "http://127.0.0.1:9"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let id = v["id"].as_str().unwrap().to_string();
    let (s, v) = get(a.clone(), "/api/proxy-pools").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["pools"].as_array().unwrap().len(), 1);
    let (s, _) = get(a.clone(), &format!("/api/proxy-pools/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = req(
        a.clone(),
        Request::post(format!("/api/proxy-pools/{id}/test"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], false);
    let (s, _, _) = req(
        a.clone(),
        Request::post("/api/proxy-pools/vercel-deploy")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, _, _) = req(
        a.clone(),
        Request::delete(format!("/api/proxy-pools/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// ─── translator / mcp / daemons / version ───────────────────────────────────

#[tokio::test]
async fn translator_flows() {
    let a = app();
    let (s, v, _) = req(
        a.clone(),
        json("POST", "/api/translator/translate", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], false);
    let (s, v, _) = req(a.clone(), json("POST", "/api/translator/translate",
        serde_json::json!({"step": 1, "body": {"model": "openai/gpt-4o", "messages": [{"role": "user", "content": "hi"}]}}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["result"]["sourceFormat"], "openai");
    assert_eq!(v["result"]["targetFormat"], "openai");
    let (s, _, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/translator/translate",
            serde_json::json!({"step": 2, "body": {}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/translator/send", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, v, _) = req(
        a.clone(),
        json(
            "POST",
            "/api/translator/save",
            serde_json::json!({"file": "1_req_client.json", "content": "{}"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["success"], true);
    let (s, v) = get(a.clone(), "/api/translator/load?file=1_req_client.json").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["content"], "{}");
    let (s, v) = get(a.clone(), "/api/translator/console-logs").await;
    assert_eq!(s, StatusCode::OK);
    assert!(!v["logs"].as_array().unwrap().is_empty());
    let (s, _, _) = req(
        a.clone(),
        Request::delete("/api/translator/console-logs")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = get(a.clone(), "/api/translator/console-logs/stream").await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn mcp_daemons_version() {
    let a = app();
    let (s, _, _) = req(
        a.clone(),
        json("POST", "/api/mcp/gh/message", serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    let (s, _) = get(a.clone(), "/api/mcp/gh/sse").await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    for p in [
        "/api/headroom/status",
        "/api/headroom/extras",
        "/api/pxpipe/status",
        "/api/pxpipe/stats",
        "/api/pxpipe/logs",
        "/api/tunnel/status",
        "/api/tunnel/tailscale-check",
    ] {
        let (s, _) = get(a.clone(), p).await;
        assert_eq!(s, StatusCode::OK, "{p}");
    }
    let (_, v) = get(a.clone(), "/api/pxpipe/status").await;
    assert_eq!(v["running"], false);
    for p in [
        "/api/headroom/start",
        "/api/pxpipe/start",
        "/api/tunnel/enable",
        "/api/tunnel/tailscale-install",
    ] {
        let (s, _, _) = req(a.clone(), json("POST", p, serde_json::json!({}))).await;
        assert!(
            s == StatusCode::NOT_IMPLEMENTED || s == StatusCode::BAD_REQUEST,
            "{p} {s}"
        );
    }
    let (s, _, _) = req(
        a.clone(),
        Request::post("/api/version/shutdown")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v, _) = req(
        a.clone(),
        Request::post("/api/version/update")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["hasUpdate"], false);
}
