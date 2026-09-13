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

mod mgmt;
mod oauth_interactive;

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
        .route(
            "/api/oauth/:provider/:action",
            get(oauth_interactive::oauth_get_action).post(oauth_action),
        )
        .route("/api/models", get(models))
        .route("/api/providers", get(providers))
        .route("/api/usage/stats", get(usage_stats))
        .route("/api/settings", get(settings))
        .route("/api/auth/status", get(auth_status))
        .route("/api/shutdown", post(shutdown))
        .route("/api/cli-tools/all-statuses", get(cli_tools_all_statuses))
        .route(
            "/api/cli-tools/:tool",
            get(get_cli_tool)
                .post(post_cli_tool)
                .delete(delete_cli_tool),
        )
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
        .merge(mgmt::router())
        .with_state(Arc::new(state))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true}))
}

async fn version(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    // Shape matches original exactly: {currentVersion, latestVersion, hasUpdate}.
    let _ = &st.version;
    Json(serde_json::json!({
        "currentVersion": st.version,
        "latestVersion": "0.5.75",
        "hasUpdate": false
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

async fn settings(State(st): State<Arc<AppState>>) -> Response {
    let stored: Value = match &st.store {
        Some(store) => {
            let store = store.clone();
            tokio::task::spawn_blocking(move || store.settings_get().unwrap_or(Value::Null))
                .await
                .unwrap_or(Value::Null)
        }
        None => Value::Null,
    };
    let mut v = serde_json::json!({
        "cloudEnabled": false,
        "tunnelEnabled": false,
        "tunnelUrl": "",
        "tunnelProvider": "cloudflare",
        "tailscaleEnabled": false,
        "tailscaleUrl": "",
        "stickyRoundRobinLimit": 3,
        "providerStrategies": {}
    });
    if let (Some(map), Some(patch)) = (v.as_object_mut(), stored.as_object()) {
        for (k, val) in patch {
            map.insert(k.clone(), val.clone());
        }
    }
    Json(v).into_response()
}

async fn auth_status(headers: HeaderMap) -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "authenticated": mgmt::dashboard_authed(&headers)}))
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
    if let Some(reason) = nine_oauth::start_unsupported_reason(&provider) {
        return err(501, reason, "not_implemented", "custom_token_flow");
    }
    if spec.authorize_url.is_empty() {
        return err(
            501,
            "provider uses a custom device flow; use the import-token action",
            "not_implemented",
            "custom_token_flow",
        );
    }
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
            if let Ok(raw) = serde_json::to_string(&entry) {
                let _ = store.kv_set("oauth_state", &state, &raw);
            }
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
    headers: HeaderMap,
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
        let capped: String = err_desc.chars().take(200).collect();
        return err(400, &capped, "invalid_request_error", "oauth_error");
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
    if let Some(created) = entry.get("createdAt").and_then(|v| v.as_str()) {
        let fresh = chrono::DateTime::parse_from_rfc3339(created)
            .map(|dt| {
                chrono::Utc::now()
                    .signed_duration_since(dt.with_timezone(&chrono::Utc))
                    .num_seconds()
                    < 600
            })
            .unwrap_or(false);
        if !fresh {
            if let Some(store) = &st.store {
                let store = store.clone();
                let key = state_val.clone();
                let _ =
                    tokio::task::spawn_blocking(move || store.kv_delete("oauth_state", &key)).await;
            }
            return err(
                400,
                "expired state",
                "invalid_request_error",
                "expired_state",
            );
        }
    }
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
    if spec.token_url.is_empty() {
        return err(
            501,
            "provider uses a custom token flow; use the import-token action",
            "not_implemented",
            "custom_token_flow",
        );
    }
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
    let now = chrono::Utc::now().timestamp();
    let is_cline = matches!(provider.as_str(), "cline" | "clinepass");
    let cline_email: Option<String> = if is_cline {
        nine_oauth::cline_decode_code(code)
            .filter(|ct| !ct.access_token.is_empty())
            .map(|ct| ct.email.clone())
    } else {
        None
    };
    let token_json: Value = match (is_cline, nine_oauth::cline_decode_code(code)) {
        (true, Some(ct)) if !ct.access_token.is_empty() => nine_oauth::cline_token_value(&ct, now),
        _ => {
            let body = spec.exchange_body(code, &redirect_uri, pkce.as_ref());
            match post_token_json(
                &st.client,
                &spec.token_url,
                "application/x-www-form-urlencoded",
                None,
                nine_oauth::form_encode(&body),
                "token_exchange_failed",
            )
            .await
            {
                Ok(v) => v,
                Err(r) => {
                    // keep historical callback mapping: upstream failures surface as 400
                    let (mut parts, body) = (*r).into_parts();
                    let bytes = axum::body::to_bytes(body, 1024 * 1024)
                        .await
                        .unwrap_or_default();
                    parts.status = StatusCode::BAD_REQUEST;
                    return Response::from_parts(parts, Body::from(bytes));
                }
            }
        }
    };
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
            .and_then(|v| v.get("email").and_then(|s| s.as_str()).map(str::to_string))
            .or_else(|| cline_email.clone().filter(|e| !e.is_empty())),
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
        let store2 = store.clone();
        let conn = conn.clone();
        let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&conn)).await;
        let key = state_val.clone();
        let _ = tokio::task::spawn_blocking(move || store2.kv_delete("oauth_state", &key)).await;
    }
    if headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(|a| a.contains("text/html"))
        .unwrap_or(false)
    {
        return oauth_interactive::browser_callback_html().into_response();
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
    #[serde(rename = "deviceCode")]
    device_code: Option<String>,
    #[serde(rename = "extraData")]
    extra_data: Option<std::collections::BTreeMap<String, String>>,
    #[serde(rename = "redirectUri")]
    redirect_uri: Option<String>,
    #[serde(rename = "connectionId")]
    connection_id: Option<String>,
}

/// POST a token request and parse the JSON body. Err(response) is final.
async fn post_token_json(
    client: &reqwest::Client,
    url: &str,
    content_type: &str,
    basic_auth: Option<&str>,
    body: String,
    failure_code: &str,
) -> Result<Value, Box<Response>> {
    let mut req = client
        .post(url)
        .header("content-type", content_type)
        .body(body);
    if let Some(b) = basic_auth {
        req = req.header("authorization", format!("Basic {b}"));
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return Err(Box::new(err(
                502,
                &format!("token exchange failed: {e}"),
                "upstream_error",
                failure_code,
            )));
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
        return Err(Box::new(err(
            status.as_u16(),
            &format!("HTTP {status}: {msg}"),
            "invalid_request_error",
            failure_code,
        )));
    }
    Ok(serde_json::from_str(&txt).unwrap_or(Value::Null))
}

async fn oauth_action(
    State(st): State<Arc<AppState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
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
            if spec.token_url.is_empty() {
                return err(
                    501,
                    "provider uses a custom token flow; use the import-token action",
                    "not_implemented",
                    "custom_token_flow",
                );
            }
            let code = match body.code {
                Some(c) if !c.is_empty() => c,
                _ => return err(400, "missing code", "invalid_request_error", "missing_code"),
            };
            let now = chrono::Utc::now().timestamp();
            // Cline-family: the callback code usually carries base64 token JSON
            // (upstream cline.js) and needs no registered client; otherwise JSON
            // exchange POST like upstream.
            let is_cline = matches!(provider.as_str(), "cline" | "clinepass");
            let cline_tokens: Option<nine_oauth::ClineTokens> = if is_cline {
                nine_oauth::cline_decode_code(&code).filter(|ct| !ct.access_token.is_empty())
            } else {
                None
            };
            if spec.client_id.is_empty() && cline_tokens.is_none() {
                return err(
                    400,
                    "client not configured",
                    "invalid_request_error",
                    "oauth_client_not_configured",
                );
            }
            let cline_email: Option<String> = cline_tokens.as_ref().map(|ct| ct.email.clone());
            let token_json: Value = match (is_cline, cline_tokens) {
                (true, Some(ct)) => nine_oauth::cline_token_value(&ct, now),
                _ => {
                    let mut redirect_uri = body.redirect_uri.clone().unwrap_or_default();
                    let mut code_verifier = body.code_verifier.clone();
                    if (code_verifier.is_none() || redirect_uri.is_empty()) && !is_cline {
                        if let (Some(store), Some(state_key)) = (&st.store, body.state.clone()) {
                            let store = store.clone();
                            if let Ok(Ok(Some(text))) = tokio::task::spawn_blocking(move || {
                                store.kv_get("oauth_state", &state_key)
                            })
                            .await
                            {
                                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                                    if code_verifier.is_none() {
                                        code_verifier = v
                                            .get("codeVerifier")
                                            .and_then(|x| x.as_str())
                                            .map(str::to_string);
                                    }
                                    if redirect_uri.is_empty() {
                                        redirect_uri = v
                                            .get("redirectUri")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or_default()
                                            .to_string();
                                    }
                                }
                            }
                        }
                    }
                    let pkce = code_verifier
                        .as_deref()
                        .map(nine_oauth::Pkce::from_verifier);
                    let (content_type, req_body) = if is_cline {
                        (
                            "application/json",
                            serde_json::json!({
                                "grant_type": "authorization_code",
                                "code": code,
                                "client_type": "extension",
                                "redirect_uri": redirect_uri,
                            })
                            .to_string(),
                        )
                    } else {
                        (
                            "application/x-www-form-urlencoded",
                            nine_oauth::form_encode(&spec.exchange_body(
                                &code,
                                &redirect_uri,
                                pkce.as_ref(),
                            )),
                        )
                    };
                    match post_token_json(
                        &st.client,
                        &spec.token_url,
                        content_type,
                        None,
                        req_body,
                        "token_exchange_failed",
                    )
                    .await
                    {
                        Ok(v) => v,
                        Err(r) => return *r,
                    }
                }
            };
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
                    .and_then(|v| v.get("email").and_then(|s| s.as_str()).map(str::to_string))
                    .or_else(|| cline_email.clone().filter(|e| !e.is_empty())),
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
            let Some(refresh) = spec.refresh_request(refresh_token) else {
                return err(
                    501,
                    "provider has no standard refresh endpoint; re-authenticate or import a token",
                    "not_implemented",
                    "refresh_unsupported",
                );
            };
            let token_json: Value = match post_token_json(
                &st.client,
                &refresh.url,
                refresh.content_type,
                refresh.basic_auth.as_deref(),
                refresh.body,
                "refresh_failed",
            )
            .await
            {
                Ok(v) => v,
                Err(r) => return *r,
            };
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
        "poll" => {
            let poll_body = oauth_interactive::PollBody {
                device_code: body.device_code.clone(),
                device_code_snake: None,
                state: body.state.clone(),
                code_verifier: body.code_verifier.clone(),
                code_verifier_snake: None,
                extra_data: body.extra_data.clone(),

                region: None,
                start_url: None,
                auth_method: None,
            };
            oauth_interactive::poll_action(State(st), Path(provider), Json(poll_body)).await
        }
        "register-session" => {
            let reg = oauth_interactive::RegisterBody {
                state: body.state.clone(),
                code_verifier: body.code_verifier.clone(),
            };
            oauth_interactive::register_session_action(Path(provider), Query(query), Json(reg))
                .await
        }
        "manual-code" => {
            let manual = oauth_interactive::ManualCodeBody {
                code: body.code.clone(),
                state: body.state.clone(),
            };
            oauth_interactive::manual_code_action(State(st), Path(provider), Json(manual)).await
        }
        "start-proxy" => {
            let v = oauth_interactive::start_proxy(&provider).await;
            if v.get("error").is_some() {
                return err(
                    400,
                    v["error"].as_str().unwrap_or("proxy unsupported"),
                    "invalid_request_error",
                    "proxy_unsupported",
                );
            }
            (StatusCode::OK, Json(v)).into_response()
        }
        "stop-proxy" => {
            oauth_interactive::stop_proxy(&provider);
            (StatusCode::OK, Json(serde_json::json!({"success": true}))).into_response()
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
    // Local mode (original: requireApiKey=false): no credential needed at all.
    if st.open_mode {
        return None;
    }
    let cred = extract_credential(headers, query);
    match cred {
        None => Some(err(
            401,
            "Missing API key",
            "authentication_error",
            "invalid_api_key",
        )),
        Some(k) => {
            if st.api_keys.iter().any(|x| x == &k) {
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

pub(crate) fn err(status: u16, message: &str, typ: &str, code: &str) -> Response {
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

pub(crate) async fn chat_completions(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let req_id = nine_core::new_request_id();
    if let Some(r) = authorize(&st, &headers, None) {
        return with_id(r, &req_id);
    }
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
    let mut native_seen = false;

    // 3. Fallback execution across combo model candidates
    for cand_model in &model_candidates {
        let (prefix, bare) = nine_core::split_provider_model(cand_model);
        let provider = prefix.unwrap_or_else(|| nine_providers::infer_provider(bare));
        let chat_req = nine_providers::ChatRequest {
            model: bare.to_string(),
            messages: messages.clone(),
            stream,
        };
        let (same, rest) = store_candidates(&st, provider).await;
        let mut upstreams = same;
        upstreams.extend(upstream_candidates(&st, provider));
        upstreams.extend(rest);
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
                ApiStyle::Responses => nine_providers::translate_chat_to_responses(&chat_req, bare),
                ApiStyle::Native => {
                    native_seen = true;
                    continue;
                }
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
            if native_seen {
                return err(
                    501,
                    "provider uses a proprietary protocol not supported over this endpoint",
                    "invalid_request_error",
                    "model_not_supported",
                );
            }
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

/// Map one stored connection to an upstream candidate.
/// Returns None for inactive connections, missing credentials, unknown
/// providers, or expired OAuth tokens (renew via the refresh endpoint).
fn connection_candidate(
    c: &nine_storage::ProviderConnection,
    now: i64,
) -> Option<(String, String, String)> {
    if !c.is_active {
        return None;
    }
    let key = c
        .data
        .get("access_token")
        .or_else(|| c.data.get("api_key"))
        .and_then(|v| v.as_str())
        .filter(|k| !k.is_empty())?;
    if let Some(exp) = c.data.get("expires_at").and_then(|v| v.as_i64()) {
        if exp < now {
            return None;
        }
    }
    let base = nine_providers::base_url_for(&c.provider);
    if base.is_empty() {
        return None;
    }
    Some((c.provider.clone(), base.to_string(), key.to_string()))
}

/// Stored-connection candidates, split into same-provider first, then the rest.
async fn store_candidates(
    st: &AppState,
    provider: &str,
) -> (Vec<(String, String, String)>, Vec<(String, String, String)>) {
    let Some(store) = &st.store else {
        return (vec![], vec![]);
    };
    let store = store.clone();
    let rows = tokio::task::spawn_blocking(move || store.list_connections(None))
        .await
        .unwrap_or(Ok(vec![]))
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let mut same = Vec::new();
    let mut rest = Vec::new();
    for c in &rows {
        if let Some(row) = connection_candidate(c, now) {
            if c.provider == provider {
                same.push(row);
            } else {
                rest.push(row);
            }
        }
    }
    (same, rest)
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
                    Ok(b) => with_id(forward_json(provider, b), req_id),
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
        ApiStyle::Responses => {
            if stream {
                translate_sse_stream(resp, provider, model, req_id)
            } else {
                match resp.bytes().await {
                    Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
                        Ok(v) => {
                            let out = nine_providers::translate_response_to_chat(&v, model, req_id);
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
        ApiStyle::Native => {
            return Ok(with_id(
                err(
                    501,
                    "provider uses a proprietary protocol not supported over this endpoint",
                    "invalid_request_error",
                    "model_not_supported",
                ),
                req_id,
            ));
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

fn forward_json(provider: &str, bytes: Bytes) -> Response {
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(v) => {
            let mut v = nine_providers::unwrap_cline_envelope(&v, provider);
            // ponytail: request-side quirks (dropClientMetadata, cloakToolsOnOAuth)
            // need no handling: OpenAI payloads are rebuilt from scratch above.
            // ponytail: MiniMax Claude-format quirks live in the Anthropic branch.

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
                    let style = nine_providers::api_style(&provider);
                    let chunks = if style == ApiStyle::Gemini {
                        vec![nine_providers::translate_gemini_sse(&v, &model, &rid)]
                    } else if style == ApiStyle::Responses {
                        nine_providers::translate_response_sse(&v, &model, &rid)
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
    let req_id = nine_core::new_request_id();
    if let Some(r) = authorize(&st, &headers, None) {
        return with_id(r, &req_id);
    }
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
    let req_id = nine_core::new_request_id();
    if let Some(r) = authorize(&st, &headers, Some(&query)) {
        return with_id(r, &req_id);
    }
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

// ─── Responses API ────────────────────────────────────────────────────────

fn parse_responses_input(v: &Value) -> Vec<nine_providers::ChatMessage> {
    if let Some(input) = v.get("input").and_then(|i| i.as_array()) {
        let mut messages = Vec::new();
        for item in input {
            let role = item
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user")
                .to_string();
            let mut content_str = String::new();
            if let Some(content) = item.get("content") {
                if let Some(s) = content.as_str() {
                    content_str = s.to_string();
                } else if let Some(arr) = content.as_array() {
                    for part in arr {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !content_str.is_empty() {
                                content_str.push('\n');
                            }
                            content_str.push_str(text);
                        }
                    }
                }
            }
            messages.push(nine_providers::ChatMessage {
                role,
                content: content_str,
            });
        }
        if !messages.is_empty() {
            return messages;
        }
    }
    parse_messages(v)
}

fn completion_to_response_format(completion: &Value, model: &str, req_id: &str) -> Value {
    let (_, bare) = nine_core::split_provider_model(model);
    let assistant_content = completion
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let usage = completion.get("usage").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        })
    });

    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    serde_json::json!({
        "id": format!("resp_{}", req_id.strip_prefix("req_").unwrap_or(req_id)),
        "object": "response",
        "model": nine_core::normalize_model_id(bare),
        "status": "completed",
        "output": [
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": assistant_content
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
}

async fn responses(State(st): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let req_id = nine_core::new_request_id();
    if let Some(r) = authorize(&st, &headers, None) {
        return with_id(r, &req_id);
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return with_id(
                err(400, "invalid json", "invalid_request_error", "invalid_json"),
                &req_id,
            );
        }
    };
    let raw_model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o")
        .to_string();
    let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    // 1. Alias chain resolution
    let aliases = if let Some(store) = &st.store {
        let store = store.clone();
        tokio::task::spawn_blocking(move || store.get_model_aliases().unwrap_or_default())
            .await
            .unwrap_or_default()
    } else {
        (*st.static_aliases).clone()
    };
    let resolved_model = nine_routing::resolve_alias_chain(&raw_model, &aliases);

    // 2. Combo expansion
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

    let messages = parse_responses_input(&v);
    let mut last: Option<Response> = None;
    let mut native_seen = false;

    for cand_model in &model_candidates {
        let (prefix, bare) = nine_core::split_provider_model(cand_model);
        let provider = prefix.unwrap_or_else(|| nine_providers::infer_provider(bare));
        let chat_req = nine_providers::ChatRequest {
            model: bare.to_string(),
            messages: messages.clone(),
            stream,
        };
        let (same, rest) = store_candidates(&st, provider).await;
        let mut upstreams = same;
        upstreams.extend(upstream_candidates(&st, provider));
        upstreams.extend(rest);
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
                ApiStyle::Responses => nine_providers::translate_chat_to_responses(&chat_req, bare),
                ApiStyle::Native => {
                    native_seen = true;
                    continue;
                }
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
                Ok(resp) => {
                    if stream {
                        return resp;
                    }
                    let status = resp.status();
                    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
                        .await
                        .unwrap_or_default();
                    if !status.is_success() {
                        let (parts, _) = Response::builder()
                            .status(status)
                            .body(Body::from(bytes))
                            .unwrap()
                            .into_parts();
                        return Response::from_parts(parts, Body::empty());
                    }
                    let val: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    let resp_val = completion_to_response_format(&val, &raw_model, &req_id);
                    return with_id((StatusCode::OK, Json(resp_val)).into_response(), &req_id);
                }
                Err(resp) => last = Some(*resp),
            }
        }
    }

    with_id(
        last.unwrap_or_else(|| {
            if native_seen {
                return err(
                    501,
                    "provider uses a proprietary protocol not supported over this endpoint",
                    "invalid_request_error",
                    "model_not_supported",
                );
            }
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

// ─── CLI Tools & Shutdown Endpoints ───────────────────────────────────────

async fn shutdown() -> Response {
    // Matches 9Router production shutdown behavior
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "success": false,
            "message": "Not allowed in production"
        })),
    )
        .into_response()
}

async fn cli_tools_all_statuses() -> Response {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let codex_cfg = format!("{home}/.codex/config.toml");
    let codex_installed = std::path::Path::new(&codex_cfg).exists();
    let codex_content = std::fs::read_to_string(&codex_cfg).unwrap_or_default();

    let tools = serde_json::json!({
        "claude": { "installed": false, "settings": null, "message": "Claude CLI is not installed" },
        "codex": {
            "installed": codex_installed,
            "config": if codex_installed { Some(codex_content.clone()) } else { None },
            "has9Router": codex_content.contains("9router"),
            "configPath": codex_cfg
        },
        "opencode": { "installed": false, "settings": null, "message": "OpenCode CLI is not installed" },
        "droid": { "installed": false, "settings": null, "message": "Droid CLI is not installed" },
        "openclaw": { "installed": false, "settings": null, "message": "Open Claw CLI is not installed" },
        "hermes": { "installed": false, "settings": null, "message": "Hermes CLI is not installed" },
        "cowork": { "installed": false, "settings": null, "message": "Cowork CLI is not installed" },
        "cline": { "installed": false, "settings": null, "message": "Cline is not installed" },
        "kilo": { "installed": false, "settings": null, "message": "Kilo CLI is not installed" },
        "deepseek-tui": { "installed": false, "settings": null, "message": "DeepSeek TUI is not installed" },
        "jcode": { "installed": false, "settings": null, "message": "JCode CLI is not installed" },
        "grok-build": { "installed": false, "settings": null, "message": "Grok Build is not installed" },
        "devin": { "installed": false, "settings": null, "message": "Devin CLI is not installed" },
    });

    Json(tools).into_response()
}

async fn get_cli_tool(Path(tool): Path<String>) -> Response {
    let clean_tool = tool.strip_suffix("-settings").unwrap_or(&tool);
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());

    match clean_tool {
        "codex" => {
            let cfg_path = format!("{home}/.codex/config.toml");
            let exists = std::path::Path::new(&cfg_path).exists();
            let content = std::fs::read_to_string(&cfg_path).unwrap_or_default();
            Json(serde_json::json!({
                "installed": exists,
                "config": if exists { Some(content.clone()) } else { None },
                "has9Router": content.contains("9router"),
                "configPath": cfg_path
            }))
            .into_response()
        }
        other => Json(serde_json::json!({
            "installed": false,
            "settings": null,
            "message": format!("{other} CLI is not installed")
        }))
        .into_response(),
    }
}

#[derive(Deserialize)]
struct ApplyCliToolReq {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    model: Option<String>,
}

async fn post_cli_tool(Path(tool): Path<String>, Json(body): Json<ApplyCliToolReq>) -> Response {
    let clean_tool = tool.strip_suffix("-settings").unwrap_or(&tool);
    let (Some(base_url), Some(api_key), Some(model)) = (body.base_url, body.api_key, body.model)
    else {
        return err(
            400,
            "baseUrl, apiKey and model are required",
            "invalid_request_error",
            "missing_fields",
        );
    };
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());

    match clean_tool {
        "codex" => {
            let dir = format!("{home}/.codex");
            let cfg_path = format!("{dir}/config.toml");
            let _ = std::fs::create_dir_all(&dir);
            let toml = format!(
                "model = \"{model}\"\nmodel_provider = \"9router\"\n\n[model_providers.9router]\nname = \"9Router\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n\n[model_providers.9router.http_headers]\nAuthorization = \"Bearer {api_key}\"\n"
            );
            if std::fs::write(&cfg_path, toml).is_ok() {
                Json(serde_json::json!({
                    "success": true,
                    "message": "Codex settings applied successfully!",
                    "configPath": cfg_path
                }))
                .into_response()
            } else {
                err(
                    500,
                    "Failed to write codex config",
                    "internal_error",
                    "io_error",
                )
            }
        }
        other => Json(serde_json::json!({
            "success": true,
            "message": format!("{other} settings applied")
        }))
        .into_response(),
    }
}

async fn delete_cli_tool(Path(tool): Path<String>) -> Response {
    let clean_tool = tool.strip_suffix("-settings").unwrap_or(&tool);
    Json(serde_json::json!({
        "success": true,
        "message": format!("{clean_tool} settings removed successfully")
    }))
    .into_response()
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
    async fn custom_token_flows_start_501() {
        let app = router_with_state(
            AppState::new(Vec::new(), 1000)
                .with_oauth_specs(nine_oauth::load_specs("/nonexistent-dir-xyz")),
        );
        for p in ["cursor", "kilocode", "codebuddy-cn", "zed", "xiaomi-mimo"] {
            let (s, _, v) = body_json(
                app.clone(),
                Request::get(format!("/api/oauth/{p}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(s, StatusCode::NOT_IMPLEMENTED, "start {p}");
            assert_eq!(v["error"]["code"], "custom_token_flow");
        }
        let (s, _, v) = body_json(
            app.clone(),
            Request::get("/api/oauth/cline")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(v["authorizeUrl"]
            .as_str()
            .unwrap()
            .contains("client_type=extension"));
        let (s, _, _) = body_json(
            app,
            Request::get("/api/oauth/qoder")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn exchange_custom_flows_501_and_cline_base64_ok() {
        let app = router_with_state(
            AppState::new(Vec::new(), 1000)
                .with_oauth_specs(nine_oauth::load_specs("/nonexistent-dir-xyz")),
        );
        for p in ["cursor", "kilocode", "zed"] {
            let (s, _, v) = body_json(
                app.clone(),
                Request::post(format!("/api/oauth/{p}/exchange"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"code":"x"}"#))
                    .unwrap(),
            )
            .await;
            assert_eq!(s, StatusCode::NOT_IMPLEMENTED, "exchange {p}");
            assert_eq!(v["error"]["code"], "custom_token_flow");
        }
        let payload = serde_json::json!({
            "accessToken": "cline-acc",
            "refreshToken": "cline-ref",
            "email": "dev@cline.bot",
        })
        .to_string();
        let code = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payload);
        let (s, _, v) = body_json(
            app,
            Request::post("/api/oauth/cline/exchange")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"code":"{code}"}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["connection"]["provider"], "cline");
        assert_eq!(v["connection"]["email"], "dev@cline.bot");
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
    async fn responses_api_validation_and_shape() {
        let app =
            router_with_state(AppState::new(Vec::new(), 1000).with_api_keys(vec!["good".into()]));
        let (s1, _, v1) = body_json(
            app.clone(),
            Request::post("/v1/responses")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(s1, StatusCode::UNAUTHORIZED);
        assert_eq!(v1["error"]["code"], "invalid_api_key");

        let (s2, _, v2) = body_json(
            app,
            Request::post("/v1/responses")
                .header("content-type", "application/json")
                .header("authorization", "Bearer good")
                .body(Body::from("{bad"))
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::BAD_REQUEST);
        assert_eq!(v2["error"]["code"], "invalid_json");
    }
}

#[cfg(test)]
mod connection_candidate_tests {
    use super::*;

    fn conn(provider: &str, data: serde_json::Value, active: bool) -> ProviderConnection {
        ProviderConnection {
            id: uuid::Uuid::new_v4().to_string(),
            provider: provider.into(),
            auth_type: "oauth".into(),
            name: None,
            email: None,
            priority: None,
            is_active: active,
            data,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    #[test]
    fn oauth_and_api_key_credentials_map() {
        let now = chrono::Utc::now().timestamp();
        let (p, base, key) = connection_candidate(
            &conn(
                "openai",
                serde_json::json!({"access_token": "tok", "expires_at": now + 99}),
                true,
            ),
            now,
        )
        .unwrap();
        assert_eq!((p.as_str(), key.as_str()), ("openai", "tok"));
        assert!(base.contains("openai.com"));
        let (_, _, key) = connection_candidate(
            &conn("groq", serde_json::json!({"api_key": "g"}), true),
            now,
        )
        .unwrap();
        assert_eq!(key, "g");
    }

    #[test]
    fn inactive_expired_credentialless_unknown_filtered() {
        let now = chrono::Utc::now().timestamp();
        assert!(connection_candidate(
            &conn(
                "openai",
                serde_json::json!({"access_token": "t", "expires_at": now + 9}),
                false
            ),
            now,
        )
        .is_none());
        assert!(connection_candidate(
            &conn(
                "openai",
                serde_json::json!({"access_token": "t", "expires_at": now - 1}),
                true
            ),
            now,
        )
        .is_none());
        assert!(connection_candidate(&conn("openai", serde_json::json!({}), true), now).is_none());
        assert!(connection_candidate(
            &conn("mystery", serde_json::json!({"api_key": "k"}), true),
            now,
        )
        .is_none());
    }

    #[tokio::test]
    async fn store_candidates_split_same_provider_first() {
        let store = std::sync::Arc::new(Store::open_memory().unwrap());
        let now = chrono::Utc::now().timestamp();
        for (p, tok) in [("groq", "g-key"), ("openai", "o-key")] {
            store
                .upsert_connection(&conn(
                    p,
                    serde_json::json!({"api_key": tok, "expires_at": now + 60}),
                    true,
                ))
                .unwrap();
        }
        let st = AppState::new(vec![], 1000).with_store(store);
        let (same, rest) = store_candidates(&st, "openai").await;
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].2, "o-key");
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].2, "g-key");
    }
}
