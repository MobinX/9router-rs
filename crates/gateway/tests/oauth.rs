use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use base64::Engine;
use nine_gateway::{router_with_state, AppState};
use nine_oauth::OAuthSpec;
use nine_storage::Store;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

fn fake_id_token(email: &str) -> String {
    let enc = |v: serde_json::Value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&v).unwrap())
    };
    let header = enc(serde_json::json!({"alg": "none", "typ": "JWT"}));
    let payload = enc(serde_json::json!({"email": email, "name": "Test User", "sub": "sub-1"}));
    format!("{header}.{payload}.sig")
}

async fn spawn_token_server() -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let refresh_hits = Arc::new(AtomicUsize::new(0));
    let exchange_hits = Arc::new(AtomicUsize::new(0));
    let rh = refresh_hits.clone();
    let eh = exchange_hits.clone();
    let app = Router::new().route(
        "/token",
        post(move |body: String| {
            let rh = rh.clone();
            let eh = eh.clone();
            async move {
                let params: std::collections::HashMap<String, String> = body
                    .split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                let grant = params.get("grant_type").map(String::as_str).unwrap_or("");
                if grant == "refresh_token" {
                    rh.fetch_add(1, Ordering::SeqCst);
                    return axum::Json(serde_json::json!({
                        "access_token": "refreshed-access",
                        "refresh_token": "rt-2",
                        "expires_in": 7200,
                    }))
                    .into_response();
                }
                eh.fetch_add(1, Ordering::SeqCst);
                if params.get("code").map(String::as_str) == Some("bad-code") {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": {"message": "invalid grant", "type": "invalid_request_error"}})),
                    )
                        .into_response();
                }
                axum::Json(serde_json::json!({
                    "access_token": "fresh-access",
                    "refresh_token": "rt-1",
                    "expires_in": 3600,
                    "token_type": "Bearer",
                    "scope": "openid profile",
                    "id_token": fake_id_token("user@example.com"),
                }))
                .into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        format!("http://127.0.0.1:{port}/token"),
        exchange_hits,
        refresh_hits,
    )
}

fn test_spec(token_url: &str) -> OAuthSpec {
    OAuthSpec {
        provider: "codex".into(),
        authorize_url: "https://auth.openai.com/oauth/authorize".into(),
        token_url: token_url.to_string(),
        client_id: "test-client".into(),
        client_secret: None,
        scopes: vec!["openid".into(), "offline_access".into()],
        pkce: true,
        fixed_port: Some(1455),
        callback_path: "/auth/callback".into(),
        device_flow: false,
        extra_params: Default::default(),
        refresh_lead_ms: 0,
    }
}

fn app_with(store: Arc<Store>, token_url: &str) -> Router {
    router_with_state(
        AppState::new(Vec::new(), 5_000)
            .with_store(store)
            .with_oauth_specs(vec![test_spec(token_url)])
            .with_callback_base("http://127.0.0.1:1455"),
    )
}

async fn get_json(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let b = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
    )
}

async fn post_json(
    app: Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let b = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn start_returns_authorize_url_with_pkce_and_state() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let (s, v) = get_json(app_with(store, &token_url), "/api/oauth/codex").await;
    assert_eq!(s, StatusCode::OK);
    let url = v["authorizeUrl"].as_str().unwrap();
    assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("client_id=test-client"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A1455%2Fauth%2Fcallback"));
    assert_eq!(v["codeChallengeMethod"], "S256");
    assert_eq!(v["clientIdConfigured"], true);
    assert!(!v["state"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_provider_and_action() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let (s, _) = get_json(app_with(store.clone(), &token_url), "/api/oauth/nope-xyz").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s2, v2) = post_json(
        app_with(store, &token_url),
        "/api/oauth/codex/bogus",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(s2, StatusCode::NOT_FOUND);
    assert_eq!(v2["error"]["code"], "unknown_action");
}

#[tokio::test]
async fn callback_exchanges_code_and_stores_connection() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, exchange_hits, _) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (_, start) = get_json(app.clone(), "/api/oauth/codex").await;
    let state = start["state"].as_str().unwrap().to_string();

    let (s, v) = get_json(
        app.clone(),
        &format!("/api/oauth/callback?code=good-code&state={state}"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(v["provider"], "codex");
    assert_eq!(v["connection"]["email"], "user@example.com");
    assert_eq!(exchange_hits.load(Ordering::SeqCst), 1);

    let conns = store.list_connections(Some("codex")).unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].data["access_token"], "fresh-access");
    assert_eq!(conns[0].data["refresh_token"], "rt-1");
    assert_eq!(conns[0].auth_type, "oauth");

    // providers endpoint must expose it
    let (ps, pv) = get_json(app, "/api/providers").await;
    assert_eq!(ps, StatusCode::OK);
    assert_eq!(pv["connections"][0]["provider"], "codex");
    assert_eq!(pv["connections"][0]["email"], "user@example.com");
}

#[tokio::test]
async fn callback_rejects_missing_state_and_bad_code() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store, &token_url);
    let (s, _) = get_json(app.clone(), "/api/oauth/callback?code=x").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, start) = get_json(app.clone(), "/api/oauth/codex").await;
    let state = start["state"].as_str().unwrap().to_string();
    let (s2, v2) = get_json(
        app,
        &format!("/api/oauth/callback?code=bad-code&state={state}"),
    )
    .await;
    assert_eq!(s2, StatusCode::BAD_REQUEST);
    assert_eq!(v2["error"]["code"], "token_exchange_failed");
}

#[tokio::test]
async fn exchange_action_works_without_stored_state() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (s, v) = post_json(
        app,
        "/api/oauth/codex/exchange",
        serde_json::json!({"code": "good-code"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "body: {v}");
    assert_eq!(v["ok"], true);
    assert_eq!(store.list_connections(Some("codex")).unwrap().len(), 1);
}

#[tokio::test]
async fn missing_code_400_and_import_token_and_logout() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (s, v) = post_json(
        app.clone(),
        "/api/oauth/codex/exchange",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "missing_code");

    let (is, iv) = post_json(
        app.clone(),
        "/api/oauth/codex/import-token",
        serde_json::json!({"accessToken": "pasted-token"}),
    )
    .await;
    assert_eq!(is, StatusCode::OK);
    let id = iv["connection"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        store.get_connection(&id).unwrap().unwrap().data["access_token"],
        "pasted-token"
    );

    let (ls, _) = post_json(
        app,
        "/api/oauth/codex/logout",
        serde_json::json!({"connectionId": id}),
    )
    .await;
    assert_eq!(ls, StatusCode::OK);
    assert!(store.get_connection(&id).unwrap().is_none());
}

#[tokio::test]
async fn refresh_rotates_tokens() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, refresh_hits) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (_, start) = get_json(app.clone(), "/api/oauth/codex").await;
    let state = start["state"].as_str().unwrap().to_string();
    let (_, cb) = get_json(
        app.clone(),
        &format!("/api/oauth/callback?code=good-code&state={state}"),
    )
    .await;
    let id = cb["connection"]["id"].as_str().unwrap().to_string();

    let (s, v) = post_json(
        app,
        "/api/oauth/codex/refresh",
        serde_json::json!({"connectionId": id}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(refresh_hits.load(Ordering::SeqCst), 1);
    let conn = store.get_connection(&id).unwrap().unwrap();
    assert_eq!(conn.data["access_token"], "refreshed-access");
    assert_eq!(conn.data["refresh_token"], "rt-2");
    assert!(conn.data["expires_at"].as_i64().unwrap() > chrono::Utc::now().timestamp());
}

#[tokio::test]
async fn refresh_without_token_400_and_unknown_connection_404() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (is, iv) = post_json(
        app.clone(),
        "/api/oauth/codex/import-token",
        serde_json::json!({"accessToken": "static"}),
    )
    .await;
    assert_eq!(is, StatusCode::OK);
    let id = iv["connection"]["id"].as_str().unwrap().to_string();
    let (s, v) = post_json(
        app.clone(),
        "/api/oauth/codex/refresh",
        serde_json::json!({"connectionId": id}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "no_refresh_token");
    let (s2, _) = post_json(
        app,
        "/api/oauth/codex/refresh",
        serde_json::json!({"connectionId": "missing"}),
    )
    .await;
    assert_eq!(s2, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unconfigured_client_returns_400() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut spec = test_spec("https://example.invalid/token");
    spec.client_id = String::new();
    let app = router_with_state(
        AppState::new(Vec::new(), 5_000)
            .with_store(store)
            .with_oauth_specs(vec![spec]),
    );
    let (s, v) = post_json(
        app,
        "/api/oauth/codex/exchange",
        serde_json::json!({"code": "x"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "oauth_client_not_configured");
}

#[tokio::test]
async fn interactive_actions_are_explicitly_not_implemented() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store, &token_url);
    for action in ["auto-import", "social-exchange", "import-cli-proxy"] {
        let (s, v) = post_json(
            app.clone(),
            &format!("/api/oauth/codex/{action}"),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_IMPLEMENTED, "{action}");
        assert_eq!(v["error"]["type"], "not_implemented", "{action}");
    }
}

#[tokio::test]
async fn expired_state_rejected_and_purged() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store.clone(), &token_url);
    let (_, start) = get_json(app.clone(), "/api/oauth/codex").await;
    let state = start["state"].as_str().unwrap().to_string();
    store
        .kv_set(
            "oauth_state",
            &state,
            &serde_json::json!({
                "state": state, "provider": "codex",
                "redirectUri": "http://127.0.0.1:1455/auth/callback",
                "createdAt": "2000-01-01T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
    let (s, v) = get_json(
        app,
        &format!("/api/oauth/callback?code=good-code&state={state}"),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "expired_state");
    assert!(store.kv_get("oauth_state", &state).unwrap().is_none());
}

#[tokio::test]
async fn provider_error_text_capped() {
    let store = Arc::new(Store::open_memory().unwrap());
    let (token_url, _, _) = spawn_token_server().await;
    let app = app_with(store, &token_url);
    let long = "x".repeat(500);
    let (s, v) = get_json(app, &format!("/api/oauth/callback?error={long}&state=zzz")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().len() <= 200);
}
