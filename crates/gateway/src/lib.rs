use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use nine_providers::{ApiStyle, ModelEntry, Provider};
use nine_storage::{ProviderConnection, Store};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

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
    pub catalog: Arc<Vec<ModelEntry>>,
    /// Dev/test mode: any non-empty credential is accepted. The CLI sets this to
    /// false as soon as the apiKeys table has at least one active key.
    pub open_mode: bool,
    pub api_keys: Arc<Vec<String>>,
    pub store: Option<Arc<Store>>,
    pub oauth_specs: Arc<Vec<nine_oauth::OAuthSpec>>,
    pub data_dir: String,
    pub callback_base: String,
    pub router_srr: Arc<nine_routing::StickyRoundRobin>,
    pub sticky_limit: usize,
    pub static_combos: Arc<Vec<nine_routing::ComboTarget>>,
    pub static_aliases: Arc<std::collections::HashMap<String, String>>,
}

impl AppState {
    pub fn new(upstreams: Vec<Upstream>, timeout_ms: u64) -> Self {
        Self {
            version: "0.1.0",
            client: reqwest::Client::new(),
            upstreams,
            timeout_ms,
            catalog: Arc::new(Vec::new()),
            open_mode: true,
            api_keys: Arc::new(Vec::new()),
            store: None,
            oauth_specs: Arc::new(Vec::new()),
            data_dir: std::env::var("HOME")
                .map(|h| format!("{h}/.9router"))
                .unwrap_or_else(|_| ".".into()),
            callback_base: "http://127.0.0.1:20128".into(),
            router_srr: Arc::new(nine_routing::StickyRoundRobin::new()),
            sticky_limit: 1,
            static_combos: Arc::new(Vec::new()),
            static_aliases: Arc::new(std::collections::HashMap::new()),
        }
    }

    pub fn with_catalog(mut self, catalog: Vec<ModelEntry>) -> Self {
        self.catalog = Arc::new(catalog);
        self
    }

    pub fn with_store(mut self, store: Arc<Store>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn with_oauth_specs(mut self, specs: Vec<nine_oauth::OAuthSpec>) -> Self {
        self.oauth_specs = Arc::new(specs);
        self
    }

    pub fn with_combos(mut self, combos: Vec<nine_routing::ComboTarget>) -> Self {
        self.static_combos = Arc::new(combos);
        self
    }

    pub fn with_aliases(mut self, aliases: std::collections::HashMap<String, String>) -> Self {
        self.static_aliases = Arc::new(aliases);
        self
    }

    pub fn with_sticky_limit(mut self, limit: usize) -> Self {
        self.sticky_limit = limit;
        self
    }

    pub fn with_callback_base(mut self, base: &str) -> Self {
        self.callback_base = base.to_string();
        self
    }

    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.open_mode = keys.is_empty();
        self.api_keys = Arc::new(keys);
        self
    }

    fn provider_base(&self, provider: &str) -> Option<(String, String)> {
        if let Some(u) = self.upstreams.iter().find(|u| u.provider == provider) {
            return Some((u.base_url.clone(), u.api_key.clone()));
        }
        if let Some(u) = self.upstreams.first() {
            return Some((u.base_url.clone(), u.api_key.clone()));
        }
        // Avoid accidental egress to public APIs unless explicitly enabled.
        if std::env::var("NINE_ALLOW_DIRECT").ok().as_deref() != Some("1") {
            return None;
        }
        let base = nine_providers::base_url_for(provider);
        if base.is_empty() {
            None
        } else {
            Some((
                base.to_string(),
                std::env::var("NINE_UPSTREAM_KEY").unwrap_or_default(),
            ))
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
        .route("/api/v1/models/info", get(model_info))
        .route("/v1/models/info", get(model_info))
        .route("/api/v1/chat/completions", post(chat_completions))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/v1/responses", post(responses))
        .route("/v1/responses", post(responses))
        .route("/codex/:path", post(responses))
        .route("/api/v1/messages", post(messages))
        .route("/v1/messages", post(messages))
        .route("/api/v1beta/models", get(models))
        .route("/v1beta/models", get(gemini_models))
        .route("/v1beta/models/*path", post(gemini_generate))
        .route("/api/oauth/callback", get(oauth_callback))
        .route("/api/oauth/:provider", get(oauth_start))
        .route("/api/oauth/:provider/:action", post(oauth_action))
        .route("/api/models", get(models))
        .route("/api/providers", get(providers))
        .route("/api/usage/stats", get(usage_stats))
        .route("/api/settings", get(settings))
        .route("/api/auth/status", get(auth_status))
        .route("/api/keys", get(keys))
        .route("/api/combos", get(list_combos).post(create_combo))
        .route(
            "/api/combos/:id",
            get(get_combo).put(update_combo).delete(delete_combo),
        )
        .route(
            "/api/models/alias",
            get(get_model_aliases)
                .put(put_model_alias)
                .delete(delete_model_alias),
        )
        .with_state(Arc::new(state))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true}))
}

async fn version(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "version": st.version,
        "currentVersion": st.version,
        "latestVersion": "0.5.75",
        "hasUpdate": false,
        "upstream": "0.5.75"
    }))
}

async fn init() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "requiresLogin": false}))
}

async fn models(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(nine_providers::models_payload(&st.catalog))
}

async fn gemini_models(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(nine_providers::gemini_models_payload(&st.catalog))
}

async fn model_info(
    State(st): State<Arc<AppState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(id) = q.get("id") else {
        return err(
            400,
            "Missing required query param: id (e.g. ?id=openai/dall-e-3)",
            "invalid_request_error",
            "invalid_request_error",
        );
    };
    match st.catalog.iter().find(|m| &m.id == id) {
        Some(m) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": m.id,
                "name": m.name,
                "kind": m.kind,
                "owned_by": m.owned_by,
                "endpoint": m.endpoint,
            })),
        )
            .into_response(),
        None => err(
            404,
            &format!("model not found: {id}"),
            "not_found_error",
            "model_not_found",
        ),
    }
}

async fn providers(State(st): State<Arc<AppState>>) -> Response {
    let rows = match &st.store {
        Some(store) => {
            let res = tokio::task::spawn_blocking({
                let store = store.clone();
                move || store.list_connections(None)
            })
            .await;
            match res {
                Ok(Ok(v)) => v,
                _ => {
                    return err(
                        500,
                        "failed to load connections",
                        "internal_error",
                        "connection_load_failed",
                    )
                }
            }
        }
        None => Vec::new(),
    };
    let connections: Vec<Value> = rows
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "provider": c.provider,
                "authType": c.auth_type,
                "name": c.name,
                "email": c.email,
                "priority": c.priority,
                "isActive": c.is_active,
                "expiresAt": c.data.get("expires_at").or_else(|| c.data.get("expiresAt")).and_then(|v| v.as_i64()).and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)).map(|dt| dt.to_rfc3339()),
                "scope": c.data.get("scope").and_then(|v| v.as_str()).unwrap_or_default(),
                "createdAt": c.created_at,
                "updatedAt": c.updated_at,
            })
        })
        .collect();
    Json(serde_json::json!({"connections": connections})).into_response()
}

async fn usage_stats(State(st): State<Arc<AppState>>) -> Response {
    let Some(store) = &st.store else {
        return Json(serde_json::json!({"totalRequests":0,"totalPromptTokens":0,"totalCompletionTokens":0,"totalCachedTokens":0,"totalCost":0,"byProvider":{}})).into_response();
    };
    match tokio::task::spawn_blocking({
        let store = store.clone();
        move || store.usage_totals()
    })
    .await
    {
        Ok(Ok(v)) => Json(v).into_response(),
        _ => err(500, "usage stats failed", "internal_error", "usage_failed").into_response(),
    }
}

async fn settings() -> impl IntoResponse {
    Json(serde_json::json!({
        "cloudEnabled": false,
        "tunnelEnabled": false,
        "tunnelUrl": "",
        "tunnelProvider": "cloudflare",
        "tailscaleEnabled": false,
        "tailscaleUrl": "",
        "stickyRoundRobinLimit": 3,
        "providerStrategies": {}
    }))
}

async fn auth_status() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "authenticated": false}))
}

async fn keys() -> impl IntoResponse {
    Json(serde_json::json!({"keys": []}))
}

async fn oauth_start(State(st): State<Arc<AppState>>, Path(provider): Path<String>) -> Response {
    let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &provider) else {
        return err(
            404,
            "unknown provider",
            "not_found_error",
            "unknown_provider",
        );
    };
    let pkce = if spec.pkce {
        Some(nine_oauth::Pkce::generate())
    } else {
        None
    };
    let state = nine_oauth::new_state();
    let redirect_uri = format!("{}{}", st.callback_base, spec.callback_path);
    let authorize_url = spec.authorize_url(&state, pkce.as_ref(), &redirect_uri);
    let resp = serde_json::json!({
        "provider": provider,
        "authorizeUrl": authorize_url,
        "state": state,
        "codeChallengeMethod": if spec.pkce { "S256" } else { "" },
        "deviceFlow": spec.device_flow,
        "clientIdConfigured": !spec.client_id.is_empty(),
        "scopes": spec.scopes,
    });
    if let Some(store) = &st.store {
        let entry = serde_json::json!({
            "state": state,
            "codeChallenge": pkce.as_ref().map(|p| p.challenge.clone()),
            "codeVerifier": pkce.as_ref().map(|p| p.verifier.clone()),
            "provider": provider,
            "redirectUri": redirect_uri,
            "scopes": spec.scopes,
            "createdAt": chrono::Utc::now().to_rfc3339()
        });
        let store = store.clone();
        let _ = tokio::task::spawn_blocking(move || {
            store.kv_set(
                "oauth_state",
                &state,
                &serde_json::to_string(&entry).unwrap(),
            )
        })
        .await;
    }
    (StatusCode::OK, Json(resp)).into_response()
}

#[derive(serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    State(st): State<Arc<AppState>>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let Some(state_val) = &q.state else {
        return err(
            400,
            "missing state",
            "invalid_request_error",
            "missing_state",
        );
    };
    if let Some(err_desc) = &q.error {
        return err(400, err_desc, "invalid_request_error", "oauth_error");
    }
    let Some(code) = &q.code else {
        return err(400, "missing code", "invalid_request_error", "missing_code");
    };
    let entry: Value = if let Some(store) = &st.store {
        let store = store.clone();
        let key = state_val.clone();
        match tokio::task::spawn_blocking(move || store.kv_get("oauth_state", &key)).await {
            Ok(Ok(Some(text))) => serde_json::from_str(&text).unwrap_or(Value::Null),
            _ => Value::Null,
        }
    } else {
        Value::Null
    };
    let provider = entry
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let redirect_uri = entry
        .get("redirectUri")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &provider) else {
        return err(
            400,
            "missing spec for state",
            "invalid_request_error",
            "missing_spec",
        );
    };
    if spec.client_id.is_empty() {
        return err(
            400,
            "client not configured (set NINE_<PROVIDER>_CLIENT_ID or oauth-specs.json)",
            "invalid_request_error",
            "oauth_client_not_configured",
        );
    }
    let pkce = entry
        .get("codeVerifier")
        .and_then(|v| v.as_str())
        .map(nine_oauth::Pkce::from_verifier);
    let body = spec.exchange_body(code, &redirect_uri, pkce.as_ref());
    let resp = match st
        .client
        .post(&spec.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(nine_oauth::form_encode(&body))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return err(
                502,
                &format!("token exchange failed: {e}"),
                "upstream_error",
                "token_exchange_failed",
            )
        }
    };
    let cb_status = resp.status();
    if !cb_status.is_success() {
        let txt = resp.text().await.unwrap_or_default();
        let msg: String = serde_json::from_str::<Value>(&txt)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| txt.chars().take(300).collect());
        return err(
            400,
            &format!("HTTP {cb_status}: {msg}"),
            "invalid_request_error",
            "token_exchange_failed",
        );
    }
    let token_json = resp.json::<Value>().await.unwrap_or(Value::Null);
    let now = chrono::Utc::now().timestamp();
    let parsed = nine_oauth::parse_token_response(&provider, &provider, &token_json, now);
    let account_id = parsed
        .id_token
        .as_deref()
        .and_then(|t| t.split('.').nth(1))
        .and_then(|seg| {
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, seg).ok()
        })
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v.get("sub")
                .or_else(|| v.get("email"))
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("user_{}", uuid::Uuid::new_v4().simple()));
    let conn = ProviderConnection {
        id: uuid::Uuid::new_v4().to_string(),
        provider: provider.clone(),
        auth_type: "oauth".into(),
        name: Some(
            parsed
                .id_token
                .as_deref()
                .and_then(|t| t.split('.').nth(1))
                .and_then(|b| {
                    base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b)
                        .ok()
                })
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| {
                    v.get("name")
                        .or_else(|| v.get("email"))
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| account_id.clone()),
        ),
        email: parsed
            .id_token
            .as_deref()
            .and_then(|t| t.split('.').nth(1))
            .and_then(|b| {
                base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b).ok()
            })
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("email").and_then(|s| s.as_str()).map(str::to_string)),
        priority: None,
        is_active: true,
        data: serde_json::json!({
            "access_token": parsed.access_token,
            "refresh_token": parsed.refresh_token,
            "expires_at": parsed.expires_at,
            "id_token": parsed.id_token,
            "scope": parsed.scope,
            "token_type": parsed.token_type,
            "account_id": account_id,
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Some(store) = &st.store {
        let store = store.clone();
        let conn = conn.clone();
        let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&conn)).await;
        let store2 = st.store.clone().unwrap();
        let key = state_val.clone();
        let _ = tokio::task::spawn_blocking(move || store2.kv_delete("oauth_state", &key)).await;
    }
    (StatusCode::OK, Json(serde_json::json!({
        "ok": true,
        "provider": provider,
        "connection": {
            "id": conn.id,
            "provider": conn.provider,
            "authType": conn.auth_type,
            "name": conn.name,
            "email": conn.email,
            "expiresAt": chrono::DateTime::from_timestamp(conn.data.get("expires_at").and_then(|v| v.as_i64()).unwrap_or(0), 0).map(|dt| dt.to_rfc3339()),
            "scope": conn.data.get("scope").and_then(|v| v.as_str()),
        }
    }))).into_response()
}

#[derive(serde::Deserialize, Default)]
struct OAuthAction {
    code: Option<String>,
    state: Option<String>,
    #[serde(rename = "codeVerifier")]
    code_verifier: Option<String>,
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "connectionId")]
    connection_id: Option<String>,
}

async fn oauth_action(
    State(st): State<Arc<AppState>>,
    Path((provider, action)): Path<(String, String)>,
    Json(body): Json<OAuthAction>,
) -> Response {
    match action.as_str() {
        "exchange" => {
            let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &provider) else {
                return err(
                    404,
                    "unknown provider",
                    "not_found_error",
                    "unknown_provider",
                );
            };
            if spec.client_id.is_empty() {
                return err(
                    400,
                    "client not configured",
                    "invalid_request_error",
                    "oauth_client_not_configured",
                );
            }
            let code = match body.code {
                Some(c) if !c.is_empty() => c,
                _ => return err(400, "missing code", "invalid_request_error", "missing_code"),
            };
            let redirect_uri = String::new();
            let pkce = body
                .code_verifier
                .as_deref()
                .map(nine_oauth::Pkce::from_verifier);
            let resp = match st
                .client
                .post(&spec.token_url)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(nine_oauth::form_encode(&spec.exchange_body(
                    &code,
                    &redirect_uri,
                    pkce.as_ref(),
                )))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return err(
                        502,
                        &format!("token exchange failed: {e}"),
                        "upstream_error",
                        "token_exchange_failed",
                    )
                }
            };
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                let msg: String = serde_json::from_str::<Value>(&txt)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| txt.chars().take(300).collect());
                return err(
                    status.as_u16(),
                    &format!("HTTP {status}: {msg}"),
                    "invalid_request_error",
                    "token_exchange_failed",
                );
            }
            let token_json: Value = serde_json::from_str(&txt).unwrap_or(Value::Null);
            let now = chrono::Utc::now().timestamp();
            let parsed = nine_oauth::parse_token_response(&provider, &provider, &token_json, now);
            let conn = ProviderConnection {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider.clone(),
                auth_type: "oauth".into(),
                name: Some(
                    parsed
                        .id_token
                        .as_deref()
                        .and_then(|t| t.split('.').nth(1))
                        .and_then(|b| {
                            base64::Engine::decode(
                                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                                b,
                            )
                            .ok()
                        })
                        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                        .and_then(|v| {
                            v.get("name")
                                .or_else(|| v.get("email"))
                                .and_then(|s| s.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "account".into()),
                ),
                email: parsed
                    .id_token
                    .as_deref()
                    .and_then(|t| t.split('.').nth(1))
                    .and_then(|b| {
                        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b)
                            .ok()
                    })
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .and_then(|v| v.get("email").and_then(|s| s.as_str()).map(str::to_string)),
                priority: None,
                is_active: true,
                data: serde_json::json!({"access_token": parsed.access_token, "refresh_token": parsed.refresh_token, "expires_at": parsed.expires_at, "id_token": parsed.id_token, "scope": parsed.scope}),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Some(store) = &st.store {
                let store = store.clone();
                let c = conn.clone();
                let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&c)).await;
            }
            (StatusCode::OK, Json(serde_json::json!({"ok":true,"connection":{"id":conn.id,"provider":conn.provider,"authType":conn.auth_type,"name":conn.name,"email":conn.email}}))).into_response()
        }
        "import-token" => {
            let access_token = body.access_token.clone().filter(|a| !a.is_empty());
            let Some(access) = access_token.filter(|a| !a.is_empty()) else {
                return err(
                    400,
                    "missing accessToken",
                    "invalid_request_error",
                    "missing_access_token",
                );
            };
            let conn = ProviderConnection {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider.clone(),
                auth_type: "token".into(),
                name: Some(provider.clone()),
                email: None,
                priority: None,
                is_active: true,
                data: serde_json::json!({"access_token": access, "refresh_token": body.refresh_token, "expires_at": chrono::Utc::now().timestamp() + 86400*30}),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Some(store) = &st.store {
                let store = store.clone();
                let c = conn.clone();
                let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&c)).await;
            }
            (StatusCode::OK, Json(serde_json::json!({"ok":true,"connection":{"id":conn.id,"provider":conn.provider,"authType":conn.auth_type,"name":conn.name}}))).into_response()
        }
        "api-key" => {
            let Some(key) = body.access_token else {
                return err(
                    400,
                    "missing apiKey",
                    "invalid_request_error",
                    "missing_api_key",
                );
            };
            let conn = ProviderConnection {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider.clone(),
                auth_type: "api_key".into(),
                name: Some(provider.clone()),
                email: None,
                priority: None,
                is_active: true,
                data: serde_json::json!({"api_key": key}),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Some(store) = &st.store {
                let store = store.clone();
                let c = conn.clone();
                let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&c)).await;
            }
            (StatusCode::OK, Json(serde_json::json!({"ok":true,"connection":{"id":conn.id,"provider":conn.provider,"authType":conn.auth_type}}))).into_response()
        }
        "refresh" => {
            let conn_id = body
                .connection_id
                .clone()
                .or_else(|| body.state.clone())
                .unwrap_or_default();
            let conn = if let Some(store) = &st.store {
                let store = store.clone();
                let cid = conn_id.clone();
                match tokio::task::spawn_blocking(move || store.get_connection(&cid)).await {
                    Ok(Ok(Some(c))) => c,
                    _ => {
                        return err(
                            404,
                            "connection not found",
                            "not_found_error",
                            "connection_not_found",
                        )
                    }
                }
            } else {
                return err(
                    501,
                    "storage unavailable",
                    "not_implemented",
                    "storage_unavailable",
                );
            };
            let refresh_token = conn
                .data
                .get("refresh_token")
                .or_else(|| conn.data.get("refreshToken"))
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if refresh_token.is_empty() {
                return err(
                    400,
                    "no refresh token available",
                    "invalid_request_error",
                    "no_refresh_token",
                );
            }
            let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &conn.provider) else {
                return err(
                    404,
                    "provider spec not found",
                    "not_found_error",
                    "unknown_provider",
                );
            };
            let resp = match st
                .client
                .post(&spec.token_url)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(nine_oauth::form_encode(&spec.refresh_body(refresh_token)))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return err(
                        502,
                        &format!("refresh failed: {e}"),
                        "upstream_error",
                        "refresh_failed",
                    )
                }
            };
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                let msg: String = serde_json::from_str::<Value>(&txt)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| txt.chars().take(300).collect());
                return err(status.as_u16(), &msg, "upstream_error", "refresh_failed");
            }
            let token_json: Value = serde_json::from_str(&txt).unwrap_or(Value::Null);
            let parsed = nine_oauth::parse_token_response(
                &conn.provider,
                &conn.id,
                &token_json,
                chrono::Utc::now().timestamp(),
            );
            let mut data = conn.data.clone();
            data["access_token"] = serde_json::json!(parsed.access_token);
            if let Some(rt) = parsed.refresh_token {
                data["refresh_token"] = serde_json::json!(rt);
            }
            data["expires_at"] = serde_json::json!(parsed.expires_at);
            if let Some(store) = &st.store {
                let store = store.clone();
                let cid = conn.id.clone();
                let now = chrono::Utc::now().to_rfc3339();
                let _ = tokio::task::spawn_blocking(move || {
                    store.update_connection_data(&cid, &data, &now)
                })
                .await;
            }
            (StatusCode::OK, Json(serde_json::json!({"ok":true,"connection":{"id":conn.id,"provider":conn.provider,"expiresAt": chrono::DateTime::from_timestamp(parsed.expires_at,0).map(|dt| dt.to_rfc3339())}}))).into_response()
        }
        "logout" => {
            let conn_id = body
                .connection_id
                .clone()
                .or_else(|| body.state.clone())
                .unwrap_or_default();
            if let Some(store) = &st.store {
                let store = store.clone();
                let cid = conn_id.clone();
                let _ = tokio::task::spawn_blocking(move || store.delete_connection(&cid)).await;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok":true,"connection":{"id":conn_id}})),
            )
                .into_response()
        }
        "auto-import" | "import" | "import-cli-proxy" | "social-authorize" | "social-exchange" => {
            // Stub: documented interactive flows requiring browser loopback/device polling.
            err(
                501,
                &format!("{action} not yet implemented (interactive flow)"),
                "not_implemented",
                &format!("{action}_not_implemented"),
            )
        }
        other => err(
            404,
            &format!("unknown action: {other}"),
            "not_found_error",
            "unknown_action",
        ),
    }
}

// ─── Auth helpers ─────────────────────────────────────────────────────────

fn extract_credential(
    headers: &HeaderMap,
    query: Option<&std::collections::HashMap<String, String>>,
) -> Option<String> {
    if let Some(q) = query {
        if let Some(k) = q.get("key") {
            if !k.is_empty() {
                return Some(k.clone());
            }
        }
    }
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    };
    if let Some(h) = get("x-api-key") {
        return Some(h);
    }
    if let Some(h) = get("x-goog-api-key") {
        return Some(h);
    }
    get("authorization").map(|v| v.trim_start_matches("Bearer ").trim().to_string())
}

fn authorize(
    st: &AppState,
    headers: &HeaderMap,
    query: Option<&std::collections::HashMap<String, String>>,
) -> Option<Response> {
    let cred = extract_credential(headers, query);
    match cred {
        None => Some(err(
            401,
            "Missing API key",
            "authentication_error",
            "invalid_api_key",
        )),
        Some(k) => {
            if st.open_mode || st.api_keys.iter().any(|x| x == &k) {
                None
            } else {
                Some(err(
                    401,
                    "Invalid API key",
                    "authentication_error",
                    "invalid_api_key",
                ))
            }
        }
    }
}

// ─── Errors ───────────────────────────────────────────────────────────────

fn retry(resp: Response) -> Result<Response, Box<Response>> {
    Err(Box::new(resp))
}

fn err(status: u16, message: &str, typ: &str, code: &str) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (
        status,
        Json(serde_json::json!({"error": {"message": message, "type": typ, "code": code}})),
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
    let (typ, code) = match status {
        400 => ("invalid_request_error", "invalid_request_error"),
        401 => ("authentication_error", "invalid_api_key"),
        403 => ("permission_error", "permission_denied"),
        404 => ("not_found_error", "not_found"),
        408 => ("timeout_error", "timeout"),
        429 => ("rate_limit_error", "rate_limit_exceeded"),
        _ => ("upstream_error", "upstream_error"),
    };
    err(status, &msg, typ, code)
}

// ─── Combos Endpoints ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateComboReq {
    name: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
}

fn valid_combo_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

async fn list_combos(State(st): State<Arc<AppState>>) -> Response {
    if let Some(store) = &st.store {
        let store = store.clone();
        let res = tokio::task::spawn_blocking(move || store.list_combos()).await;
        match res {
            Ok(Ok(combos)) => Json(serde_json::json!({ "combos": combos })).into_response(),
            _ => err(500, "Failed to fetch combos", "internal_error", "db_error"),
        }
    } else {
        let combos: Vec<Value> = st
            .static_combos
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id, "name": c.name, "models": c.models, "kind": null,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                    "updatedAt": chrono::Utc::now().to_rfc3339(),
                })
            })
            .collect();
        Json(serde_json::json!({ "combos": combos })).into_response()
    }
}

async fn create_combo(
    State(st): State<Arc<AppState>>,
    Json(body): Json<CreateComboReq>,
) -> Response {
    let Some(name) = body.name.filter(|n| !n.is_empty()) else {
        return err(
            400,
            "Name is required",
            "invalid_request_error",
            "invalid_name",
        );
    };
    if !valid_combo_name(&name) {
        return err(
            400,
            "Name can only contain letters, numbers, -, _ and .",
            "invalid_request_error",
            "invalid_name",
        );
    }
    let combo = nine_storage::Combo {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
        kind: body.kind,
        models: body.models,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Some(store) = &st.store {
        let store = store.clone();
        let c = combo.clone();
        let res = tokio::task::spawn_blocking(move || -> Result<bool, nine_storage::DbError> {
            if store.get_combo_by_name(&c.name)?.is_some() {
                return Ok(false);
            }
            store.upsert_combo(&c)?;
            Ok(true)
        })
        .await;
        match res {
            Ok(Ok(true)) => (StatusCode::CREATED, Json(combo)).into_response(),
            Ok(Ok(false)) => err(
                400,
                "Combo name already exists",
                "invalid_request_error",
                "name_exists",
            ),
            _ => err(500, "Failed to create combo", "internal_error", "db_error"),
        }
    } else {
        (StatusCode::CREATED, Json(combo)).into_response()
    }
}

async fn get_combo(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    if let Some(store) = &st.store {
        let store = store.clone();
        let cid = id.clone();
        match tokio::task::spawn_blocking(move || store.get_combo(&cid)).await {
            Ok(Ok(Some(c))) => Json(c).into_response(),
            Ok(Ok(None)) => err(404, "Combo not found", "not_found_error", "combo_not_found"),
            _ => err(500, "Failed to fetch combo", "internal_error", "db_error"),
        }
    } else {
        match st.static_combos.iter().find(|c| c.id == id) {
            Some(c) => Json(serde_json::json!({
                "id": c.id, "name": c.name, "models": c.models, "kind": null,
            }))
            .into_response(),
            None => err(404, "Combo not found", "not_found_error", "combo_not_found"),
        }
    }
}

#[derive(Deserialize)]
struct UpdateComboReq {
    name: Option<String>,
    models: Option<Vec<String>>,
    kind: Option<String>,
}

async fn update_combo(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateComboReq>,
) -> Response {
    if let Some(ref name) = body.name {
        if !valid_combo_name(name) {
            return err(
                400,
                "Name can only contain letters, numbers, -, _ and .",
                "invalid_request_error",
                "invalid_name",
            );
        }
    }
    let Some(store) = &st.store else {
        return err(501, "Database not configured", "not_implemented", "no_db");
    };
    let store = store.clone();
    let cid = id.clone();
    let res = tokio::task::spawn_blocking(
        move || -> Result<Option<nine_storage::Combo>, nine_storage::DbError> {
            let Some(mut existing) = store.get_combo(&cid)? else {
                return Ok(None);
            };
            if let Some(ref new_name) = body.name {
                if new_name != &existing.name {
                    if let Some(other) = store.get_combo_by_name(new_name)? {
                        if other.id != cid {
                            return Ok(Some(existing));
                        }
                    }
                    existing.name = new_name.clone();
                }
            }
            if let Some(models) = body.models {
                existing.models = models;
            }
            if let Some(kind) = body.kind {
                existing.kind = Some(kind);
            }
            existing.updated_at = chrono::Utc::now().to_rfc3339();
            store.upsert_combo(&existing)?;
            Ok(Some(existing))
        },
    )
    .await;

    match res {
        Ok(Ok(Some(c))) => Json(c).into_response(),
        Ok(Ok(None)) => err(404, "Combo not found", "not_found_error", "combo_not_found"),
        Ok(Err(_)) => err(500, "Failed to update combo", "internal_error", "db_error"),
        _ => err(500, "Failed to update combo", "internal_error", "db_error"),
    }
}

async fn delete_combo(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some(store) = &st.store else {
        return err(501, "Database not configured", "not_implemented", "no_db");
    };
    let store = store.clone();
    let cid = id.clone();
    let res = tokio::task::spawn_blocking(move || store.delete_combo(&cid)).await;
    match res {
        Ok(Ok(n)) if n > 0 => Json(serde_json::json!({ "success": true })).into_response(),
        Ok(Ok(_)) => err(404, "Combo not found", "not_found_error", "combo_not_found"),
        _ => err(500, "Failed to delete combo", "internal_error", "db_error"),
    }
}

// ─── Model Aliases Endpoints ──────────────────────────────────────────────

async fn get_model_aliases(State(st): State<Arc<AppState>>) -> Response {
    if let Some(store) = &st.store {
        let store = store.clone();
        match tokio::task::spawn_blocking(move || store.get_model_aliases()).await {
            Ok(Ok(aliases)) => Json(serde_json::json!({ "aliases": aliases })).into_response(),
            _ => err(500, "Failed to fetch aliases", "internal_error", "db_error"),
        }
    } else {
        Json(serde_json::json!({ "aliases": *st.static_aliases })).into_response()
    }
}

#[derive(Deserialize)]
struct PutAliasReq {
    model: Option<String>,
    alias: Option<String>,
}

async fn put_model_alias(
    State(st): State<Arc<AppState>>,
    Json(body): Json<PutAliasReq>,
) -> Response {
    let (Some(model), Some(alias)) = (body.model, body.alias) else {
        return err(
            400,
            "Model and alias required",
            "invalid_request_error",
            "missing_fields",
        );
    };
    if let Some(store) = &st.store {
        let store = store.clone();
        let m = model.clone();
        let a = alias.clone();
        let res = tokio::task::spawn_blocking(move || store.set_model_alias(&a, &m)).await;
        match res {
            Ok(Ok(())) => {
                Json(serde_json::json!({ "success": true, "model": model, "alias": alias }))
                    .into_response()
            }
            _ => err(500, "Failed to update alias", "internal_error", "db_error"),
        }
    } else {
        Json(serde_json::json!({ "success": true, "model": model, "alias": alias })).into_response()
    }
}

#[derive(Deserialize)]
struct DeleteAliasQuery {
    alias: Option<String>,
}

async fn delete_model_alias(
    State(st): State<Arc<AppState>>,
    Query(q): Query<DeleteAliasQuery>,
) -> Response {
    let Some(alias) = q.alias else {
        return err(
            400,
            "Alias required",
            "invalid_request_error",
            "missing_alias",
        );
    };
    if let Some(store) = &st.store {
        let store = store.clone();
        let a = alias.clone();
        let res = tokio::task::spawn_blocking(move || store.delete_model_alias(&a)).await;
        match res {
            Ok(Ok(_)) => Json(serde_json::json!({ "success": true })).into_response(),
            _ => err(500, "Failed to delete alias", "internal_error", "db_error"),
        }
    } else {
        Json(serde_json::json!({ "success": true })).into_response()
    }
}

// ─── Chat completions ─────────────────────────────────────────────────────

fn parse_messages(v: &serde_json::Value) -> Vec<nine_providers::ChatMessage> {
    v.get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|m| nine_providers::ChatMessage {
            role: m
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user")
                .to_string(),
            content: match m.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            },
        })
        .collect()
}

async fn chat_completions(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = authorize(&st, &headers, None) {
        return r;
    }
    let req_id = nine_core::new_request_id();
    let v: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return with_id(
                err(400, "invalid json", "invalid_request_error", "invalid_json"),
                &req_id,
            )
        }
    };
    let raw_model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o")
        .to_string();
    let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    // 1. Alias resolution chain
    let aliases = if let Some(store) = &st.store {
        let store = store.clone();
        tokio::task::spawn_blocking(move || store.get_model_aliases().unwrap_or_default())
            .await
            .unwrap_or_default()
    } else {
        (*st.static_aliases).clone()
    };
    let resolved_model = nine_routing::resolve_alias_chain(&raw_model, &aliases);

    // 2. Combo expansion & sticky round-robin
    let combos = if let Some(store) = &st.store {
        let store = store.clone();
        tokio::task::spawn_blocking(move || store.list_combos().unwrap_or_default())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| nine_routing::ComboTarget {
                id: c.id,
                name: c.name,
                models: c.models,
            })
            .collect()
    } else {
        (*st.static_combos).clone()
    };

    let model_candidates =
        if let Some(expanded) = nine_routing::expand_combo(&resolved_model, &combos) {
            st.router_srr
                .select(&resolved_model, &expanded, st.sticky_limit)
        } else {
            vec![resolved_model.clone()]
        };

    let messages = parse_messages(&v);
    let mut last: Option<Response> = None;

    // 3. Fallback execution across combo model candidates
    for cand_model in &model_candidates {
        let (prefix, bare) = nine_core::split_provider_model(cand_model);
        let provider = prefix.unwrap_or_else(|| nine_providers::infer_provider(bare));
        let chat_req = nine_providers::ChatRequest {
            model: bare.to_string(),
            messages: messages.clone(),
            stream,
        };
        let upstreams = upstream_candidates(&st, provider);
        if upstreams.is_empty() {
            continue;
        }
        for (cand_provider, base, key) in &upstreams {
            let payload = match nine_providers::api_style(cand_provider) {
                ApiStyle::Anthropic => {
                    nine_providers::AnthropicAdapter.translate_request(&chat_req)
                }
                ApiStyle::Gemini => nine_providers::GeminiAdapter.translate_request(&chat_req),
                ApiStyle::OpenAi => nine_providers::OpenAiPassthrough {
                    provider_id: "openai",
                    base_url: "",
                }
                .translate_request(&chat_req),
            };
            let url = nine_providers::chat_url(cand_provider, base, bare);
            match try_upstream(
                &st,
                &req_id,
                cand_model,
                cand_provider,
                &url,
                key,
                payload,
                stream,
            )
            .await
            {
                Ok(resp) => return resp,
                Err(resp) => last = Some(*resp),
            }
        }
    }

    with_id(
        last.unwrap_or_else(|| {
            err(
                502,
                "all model candidates and upstreams failed",
                "upstream_error",
                "no_upstream",
            )
        }),
        &req_id,
    )
}

/// Ordered fallback candidates: provider-prefix match first, then the rest.
fn upstream_candidates(st: &AppState, provider: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for u in st.upstreams.iter().filter(|u| u.provider == provider) {
        out.push((
            u.provider.to_string(),
            u.base_url.clone(),
            u.api_key.clone(),
        ));
    }
    for u in st.upstreams.iter().filter(|u| u.provider != provider) {
        out.push((
            u.provider.to_string(),
            u.base_url.clone(),
            u.api_key.clone(),
        ));
    }
    if out.is_empty() {
        if let Some((base, key)) = st.provider_base(provider) {
            out.push((provider.to_string(), base, key));
        }
    }
    out
}

/// Forward a translated request and translate the response back to OpenAI shape.
/// Err(response) means a retryable failure (timeout / 408 / 429 / 5xx); Ok is final.
#[allow(clippy::too_many_arguments)]
async fn try_upstream(
    st: &AppState,
    req_id: &str,
    model: &str,
    provider: &str,
    url: &str,
    key: &str,
    payload: serde_json::Value,
    stream: bool,
) -> Result<Response, Box<Response>> {
    let mut req = st.client.post(url).json(&payload);
    for (name, value) in nine_providers::auth_headers(provider, key) {
        req = req.header(name, value);
    }
    let send = req.send();
    let resp = match tokio::time::timeout(Duration::from_millis(st.timeout_ms), send).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return retry(with_id(
                err(
                    502,
                    &format!("upstream error: {e}"),
                    "upstream_error",
                    "upstream_error",
                ),
                req_id,
            ))
        }
        Err(_) => {
            return retry(with_id(
                err(504, "upstream timeout", "timeout_error", "timeout"),
                req_id,
            ))
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let mapped = map_upstream_err(status.as_u16(), resp).await;
        return if nine_routing::should_retry(status.as_u16()) {
            retry(mapped)
        } else {
            Ok(mapped)
        };
    }
    let resp = match nine_providers::api_style(provider) {
        ApiStyle::OpenAi => {
            if stream {
                passthrough_sse(resp, req_id)
            } else {
                match resp.bytes().await {
                    Ok(b) => with_id(forward_json(b), req_id),
                    Err(_) => {
                        return retry(with_id(
                            err(
                                502,
                                "upstream read failed",
                                "upstream_error",
                                "upstream_error",
                            ),
                            req_id,
                        ))
                    }
                }
            }
        }
        ApiStyle::Anthropic => {
            if stream {
                translate_sse_stream(resp, provider, model, req_id)
            } else {
                match resp.bytes().await {
                    Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
                        Ok(v) => {
                            let out =
                                nine_providers::translate_anthropic_response(&v, model, req_id);
                            (StatusCode::OK, Json(out)).into_response()
                        }
                        Err(_) => {
                            return retry(with_id(
                                err(
                                    502,
                                    "malformed upstream json",
                                    "upstream_error",
                                    "malformed_json",
                                ),
                                req_id,
                            ))
                        }
                    },
                    Err(_) => {
                        return retry(with_id(
                            err(
                                502,
                                "upstream read failed",
                                "upstream_error",
                                "upstream_error",
                            ),
                            req_id,
                        ))
                    }
                }
            }
        }
        ApiStyle::Gemini => {
            if stream {
                translate_sse_stream(resp, provider, model, req_id)
            } else {
                match resp.bytes().await {
                    Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
                        Ok(v) => {
                            let out = nine_providers::translate_gemini_response(&v, model, req_id);
                            (StatusCode::OK, Json(out)).into_response()
                        }
                        Err(_) => {
                            return retry(with_id(
                                err(
                                    502,
                                    "malformed upstream json",
                                    "upstream_error",
                                    "malformed_json",
                                ),
                                req_id,
                            ))
                        }
                    },
                    Err(_) => {
                        return retry(with_id(
                            err(
                                502,
                                "upstream read failed",
                                "upstream_error",
                                "upstream_error",
                            ),
                            req_id,
                        ))
                    }
                }
            }
        }
    };
    Ok(resp)
}

fn forward_json(bytes: Bytes) -> Response {
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(mut v) => {
            if let Some(o) = v.as_object_mut() {
                o.entry("usage").or_insert(serde_json::json!({
                    "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0
                }));
            }
            (StatusCode::OK, Json(v)).into_response()
        }
        Err(_) => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(bytes))
            .unwrap(),
    }
}

fn sse_response(
    stream: impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    req_id: &str,
) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-request-id", req_id)
        .body(Body::from_stream(stream))
        .unwrap()
}

/// Pass upstream SSE bytes through unchanged.
fn passthrough_sse(resp: reqwest::Response, req_id: &str) -> Response {
    let stream = resp
        .bytes_stream()
        .map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
    sse_response(stream, req_id)
}

/// Translate an Anthropic/Gemini SSE stream into OpenAI chat.completion.chunk events.
fn translate_sse_stream(
    resp: reqwest::Response,
    provider: &str,
    model: &str,
    req_id: &str,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    let provider = provider.to_string();
    let model = model.to_string();
    let rid = req_id.to_string();
    tokio::spawn(async move {
        let mut upstream = resp.bytes_stream();
        let mut buf = String::new();
        let mut finished = false;
        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    return;
                }
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(idx) = buf.find("\n\n") {
                let block = buf[..idx].to_string();
                buf.drain(..idx + 2);
                let data: Vec<&str> = block
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                for d in data {
                    if finished {
                        continue;
                    }
                    if d == "[DONE]" {
                        finished = true;
                        let _ = tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n"))).await;
                        continue;
                    }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(d) else {
                        continue;
                    };
                    let chunks = if nine_providers::api_style(&provider) == ApiStyle::Gemini {
                        vec![nine_providers::translate_gemini_sse(&v, &model, &rid)]
                    } else {
                        nine_providers::translate_anthropic_sse(&v, &model, &rid)
                    };
                    for c in chunks {
                        let line = format!("data: {c}\n\n");
                        if tx.send(Ok(Bytes::from(line))).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
        if !finished {
            let _ = tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n"))).await;
        }
    });
    let stream = ReceiverStream::new(rx);
    sse_response(stream, req_id)
}

// ─── Anthropic /v1/messages ───────────────────────────────────────────────

async fn messages(State(st): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(r) = authorize(&st, &headers, None) {
        return r;
    }
    let req_id = nine_core::new_request_id();
    let v: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return with_id(
                err(400, "invalid json", "invalid_request_error", "invalid_json"),
                &req_id,
            )
        }
    };
    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude-sonnet-4-6")
        .to_string();
    let (prefix, bare) = nine_core::split_provider_model(&model);
    let provider = prefix.unwrap_or("anthropic");
    let Some((base, key)) = st.provider_base(provider) else {
        return with_id(
            err(
                502,
                "no upstream configured",
                "upstream_error",
                "no_upstream",
            ),
            &req_id,
        );
    };
    let mut payload = v.clone();
    if let Some(o) = payload.as_object_mut() {
        o.insert(
            "model".into(),
            serde_json::json!(nine_core::normalize_model_id(bare)),
        );
    }
    let url = format!("{}/messages", base.trim_end_matches('/'));
    let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let mut req = st.client.post(&url).json(&payload);
    for (name, value) in nine_providers::auth_headers(provider, &key) {
        req = req.header(name, value);
    }
    let resp = match tokio::time::timeout(Duration::from_millis(st.timeout_ms), req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return with_id(
                err(
                    502,
                    &format!("upstream error: {e}"),
                    "upstream_error",
                    "upstream_error",
                ),
                &req_id,
            )
        }
        Err(_) => {
            return with_id(
                err(504, "upstream timeout", "timeout_error", "timeout"),
                &req_id,
            )
        }
    };
    let status = resp.status();
    if !status.is_success() {
        return with_id(map_upstream_err(status.as_u16(), resp).await, &req_id);
    }
    if stream {
        return passthrough_sse(resp, &req_id);
    }
    match resp.bytes().await {
        Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err(_) => with_id(
                err(
                    502,
                    "malformed upstream json",
                    "upstream_error",
                    "malformed_json",
                ),
                &req_id,
            ),
        },
        Err(_) => with_id(
            err(
                502,
                "upstream read failed",
                "upstream_error",
                "upstream_error",
            ),
            &req_id,
        ),
    }
}

// ─── Gemini generateContent ───────────────────────────────────────────────

async fn gemini_generate(
    State(st): State<Arc<AppState>>,
    Path(path): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = authorize(&st, &headers, Some(&query)) {
        return r;
    }
    let req_id = nine_core::new_request_id();
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return with_id(
                err(400, "invalid json", "invalid_request_error", "invalid_json"),
                &req_id,
            )
        }
    };
    let clean = path.strip_prefix("models/").unwrap_or(&path);
    let model = clean.split(':').next().unwrap_or(clean).to_string();
    let (prefix, _) = nine_core::split_provider_model(&model);
    let provider = prefix.unwrap_or("gemini");
    let Some((base, key)) = st.provider_base(provider) else {
        return with_id(
            err(
                502,
                "no upstream configured",
                "upstream_error",
                "no_upstream",
            ),
            &req_id,
        );
    };
    let url = format!(
        "{}/models/{}",
        base.trim_end_matches('/'),
        path.strip_prefix("models/").unwrap_or(&path)
    );
    let stream = path.contains("streamGenerateContent");
    let mut req = st.client.post(&url).json(&payload);
    for (name, value) in nine_providers::auth_headers(provider, &key) {
        req = req.header(name, value);
    }
    let resp = match tokio::time::timeout(Duration::from_millis(st.timeout_ms), req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return with_id(
                err(
                    502,
                    &format!("upstream error: {e}"),
                    "upstream_error",
                    "upstream_error",
                ),
                &req_id,
            )
        }
        Err(_) => {
            return with_id(
                err(504, "upstream timeout", "timeout_error", "timeout"),
                &req_id,
            )
        }
    };
    let status = resp.status();
    if !status.is_success() {
        return with_id(map_upstream_err(status.as_u16(), resp).await, &req_id);
    }
    if stream {
        return passthrough_sse(resp, &req_id);
    }
    match resp.bytes().await {
        Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err(_) => with_id(
                err(
                    502,
                    "malformed upstream json",
                    "upstream_error",
                    "malformed_json",
                ),
                &req_id,
            ),
        },
        Err(_) => with_id(
            err(
                502,
                "upstream read failed",
                "upstream_error",
                "upstream_error",
            ),
            &req_id,
        ),
    }
}

async fn responses() -> impl IntoResponse {
    err(
        501,
        "responses API not yet implemented",
        "not_implemented",
        "not_implemented",
    )
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
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, headers, v)
    }

    fn entry(id: &str) -> ModelEntry {
        ModelEntry {
            id: id.into(),
            name: id.rsplit('/').next().unwrap_or(id).into(),
            kind: "llm".into(),
            owned_by: id.split('/').next().unwrap_or("openai").into(),
            endpoint: "/v1/chat/completions".into(),
        }
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
    async fn models_public_list() {
        let app = router_with_state(
            AppState::new(Vec::new(), 1000).with_catalog(vec![entry("openai/gpt-4o")]),
        );
        let (s, _, v) =
            body_json(app, Request::get("/v1/models").body(Body::empty()).unwrap()).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["object"], "list");
        assert_eq!(v["data"][0]["id"], "openai/gpt-4o");
    }

    #[tokio::test]
    async fn gemini_models_shape() {
        let app = router_with_state(
            AppState::new(Vec::new(), 1000).with_catalog(vec![entry("google/gemini-2.5-pro")]),
        );
        let (s, _, v) = body_json(
            app,
            Request::get("/v1beta/models").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["models"][0]["name"], "models/google/gemini-2.5-pro");
    }

    #[tokio::test]
    async fn model_info_requires_id() {
        let (s, _, v) = body_json(
            router(),
            Request::get("/v1/models/info").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"]["type"], "invalid_request_error");
    }

    #[tokio::test]
    async fn model_info_found_and_missing() {
        let app = router_with_state(
            AppState::new(Vec::new(), 1000).with_catalog(vec![entry("openai/gpt-4o")]),
        );
        let (s, _, v) = body_json(
            app.clone(),
            Request::get("/v1/models/info?id=openai/gpt-4o")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["id"], "openai/gpt-4o");
        let (s2, _, v2) = body_json(
            app,
            Request::get("/v1/models/info?id=nope/nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::NOT_FOUND);
        assert_eq!(v2["error"]["code"], "model_not_found");
    }

    #[tokio::test]
    async fn auth_missing_key_401_code() {
        let app =
            router_with_state(AppState::new(Vec::new(), 1000).with_api_keys(vec!["good".into()]));
        let (s, _, v) = body_json(
            app,
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"m"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"]["code"], "invalid_api_key");
        assert_eq!(v["error"]["type"], "authentication_error");
    }

    #[tokio::test]
    async fn auth_invalid_key_401() {
        let app =
            router_with_state(AppState::new(Vec::new(), 1000).with_api_keys(vec!["good".into()]));
        let (s, _, v) = body_json(
            app,
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer bad")
                .body(Body::from(r#"{"model":"m"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"]["message"], "Invalid API key");
    }

    #[tokio::test]
    async fn auth_valid_key_reaches_upstream() {
        let app = router_with_state(
            AppState::new(
                vec![Upstream {
                    provider: "openai",
                    base_url: "http://127.0.0.1:1".into(),
                    api_key: "k".into(),
                }],
                500,
            )
            .with_api_keys(vec!["good".into()]),
        );
        let (s, _, _) = body_json(
            app,
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("x-api-key", "good")
                .body(Body::from(r#"{"model":"m"}"#))
                .unwrap(),
        )
        .await;
        // connection refused → 502, proving auth passed and upstream was attempted
        assert_eq!(s, StatusCode::BAD_GATEWAY);
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

    #[tokio::test]
    async fn responses_not_implemented_yet() {
        let (s, _, v) = body_json(
            router(),
            Request::post("/v1/responses")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(v["error"]["code"], "not_implemented");
    }
}
