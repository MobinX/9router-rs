//! Dashboard-management routes mirroring 9Router's `/api/*` surface.
//!
//! Inference routes (chat/responses/messages) live in `lib.rs`; everything
//! here is account/config/telemetry management backed by [`Store`] via
//! `spawn_blocking`. Secrets in connection `data` are never serialized.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use nine_storage::{ProviderConnection, Store};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{err, AppState};

type Shared = Arc<AppState>;

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/keys", get(list_keys).post(create_key))
        .route(
            "/api/keys/:id",
            get(get_key).put(update_key).delete(delete_key),
        )
        .route("/api/locale", post(set_locale))
        .route("/api/tags", get(tags).options(cors_preflight))
        .route(
            "/api/pricing",
            get(get_pricing).patch(update_pricing).delete(reset_pricing),
        )
        .route("/api/settings", patch(update_settings))
        .route("/api/settings/database", get(export_db).post(import_db))
        .route("/api/settings/proxy-test", post(proxy_test))
        .route("/api/settings/require-login", get(require_login))
        .route("/api/providers", post(create_provider))
        .route(
            "/api/providers/:id",
            get(get_provider)
                .put(update_provider)
                .delete(delete_provider),
        )
        .route("/api/providers/:id/models", get(provider_models))
        .route("/api/providers/:id/test", post(test_provider))
        .route("/api/providers/:id/test-models", post(test_provider_models))
        .route("/api/providers/client", get(providers_client))
        .route("/api/providers/kilo/free-models", get(kilo_free_models))
        .route("/api/providers/suggested-models", get(suggested_models))
        .route("/api/providers/test-batch", post(test_provider_batch))
        .route("/api/providers/validate", post(validate_provider))
        .route("/api/models", put(put_models))
        .route(
            "/api/models/availability",
            get(model_availability).post(set_model_availability),
        )
        .route(
            "/api/models/catalog-sync",
            get(catalog_sync).post(run_catalog_sync),
        )
        .route(
            "/api/models/custom",
            get(list_custom_models)
                .post(add_custom_model)
                .delete(delete_custom_model),
        )
        .route(
            "/api/models/disabled",
            get(get_disabled).post(set_disabled).delete(clear_disabled),
        )
        .route("/api/models/test", post(test_model))
        .route("/api/usage/history", get(usage_history))
        .route("/api/usage/chart", get(usage_chart))
        .route("/api/usage/logs", get(usage_logs))
        .route("/api/usage/request-logs", get(usage_logs))
        .route("/api/usage/request-details", get(request_details))
        .route("/api/usage/providers", get(usage_providers))
        .route("/api/usage/stream", get(usage_stream))
        .route("/api/usage/:connectionId", get(usage_for_connection))
        .route(
            "/api/usage/:connectionId/codex-reset-credits",
            get(codex_credits).post(codex_consume_credit),
        )
        .route("/api/oauth/codex/bulk-import", post(bulk_import_tokens))
        .route("/api/oauth/codex/import-token", post(import_codex_token))
        .route("/api/oauth/cursor/auto-import", get(auto_import_probe))
        .route(
            "/api/oauth/cursor/import",
            get(auto_import_probe).post(import_token_generic),
        )
        .route("/api/oauth/gitlab/pat", post(import_gitlab_pat))
        .route("/api/oauth/grok-cli/bulk-import", post(bulk_import_tokens))
        .route("/api/oauth/iflow/cookie", post(import_token_generic))
        .route("/api/oauth/kiro/api-key", post(import_token_generic))
        .route("/api/oauth/kiro/auto-import", get(auto_import_probe))
        .route("/api/oauth/kiro/import", post(import_token_generic))
        .route(
            "/api/oauth/kiro/import-cli-proxy",
            post(import_token_generic),
        )
        .route(
            "/api/oauth/kiro/social-authorize",
            get(kiro_social_authorize),
        )
        .route(
            "/api/oauth/kiro/social-exchange",
            post(kiro_social_exchange),
        )
        .route("/api/oauth/xiaomi-mimo/api-key", post(import_token_generic))
        .route("/api/oauth/xiaomi-mimo/auto-import", get(auto_import_probe))
        .route(
            "/api/cli-tools/antigravity-mitm",
            get(mitm_status)
                .post(mitm_control)
                .delete(mitm_control)
                .patch(mitm_config),
        )
        .route(
            "/api/cli-tools/antigravity-mitm/alias",
            get(mitm_alias).put(mitm_alias_put),
        )
        .route("/api/cli-tools/cowork-mcp-registry", get(mcp_registry))
        .route("/api/cli-tools/cowork-mcp-tools", post(mcp_probe))
        .route("/api/media-providers/tts/voices", get(tts_voices))
        .route(
            "/api/media-providers/tts/deepgram/voices",
            get(tts_voices_unconfigured),
        )
        .route(
            "/api/media-providers/tts/elevenlabs/voices",
            get(tts_voices_unconfigured),
        )
        .route(
            "/api/media-providers/tts/inworld/voices",
            get(tts_voices_unconfigured),
        )
        .route(
            "/api/media-providers/tts/minimax/voices",
            get(tts_voices_unconfigured),
        )
        .route(
            "/api/v1/audio/speech",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/audio/transcriptions",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/audio/voices",
            get(media_get_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/embeddings",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/images/generations",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/messages/count_tokens",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/models/*model",
            get(v1_model_detail).options(cors_preflight),
        )
        .route(
            "/api/v1/responses/compact",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/search",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/videos/generations",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/videos/edits",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/videos/extensions",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/videos/:id",
            get(media_get_passthrough).options(cors_preflight),
        )
        .route(
            "/api/v1/web/fetch",
            post(media_passthrough).options(cors_preflight),
        )
        .route("/api/v1/api/chat", post(api_chat).options(cors_preflight))
        .route(
            "/v1/audio/speech",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/audio/transcriptions",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/audio/voices",
            get(media_get_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/embeddings",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/images/generations",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/messages/count_tokens",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/responses/compact",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/search",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/videos/generations",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/videos/edits",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/videos/extensions",
            post(media_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/videos/:id",
            get(media_get_passthrough).options(cors_preflight),
        )
        .route(
            "/v1/web/fetch",
            post(media_passthrough).options(cors_preflight),
        )
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/reset-password", post(reset_password))
        .route("/api/auth/oidc/start", get(enterprise))
        .route("/api/auth/oidc/callback", get(enterprise))
        .route("/api/auth/oidc/test", post(enterprise))
        .route("/api/auth/saml/start", get(enterprise))
        .route("/api/auth/saml/metadata", get(enterprise))
        .route("/api/auth/saml/acs", post(enterprise))
        .route("/api/auth/saml/test", post(enterprise))
        .route("/api/provider-nodes", get(list_nodes).post(create_node))
        .route(
            "/api/provider-nodes/:id",
            put(update_node).delete(delete_node),
        )
        .route("/api/provider-nodes/validate", post(validate_node))
        .route("/api/proxy-pools", get(list_pools).post(create_pool))
        .route(
            "/api/proxy-pools/:id",
            get(get_pool).put(update_pool).delete(delete_pool),
        )
        .route("/api/proxy-pools/:id/test", post(test_pool))
        .route(
            "/api/proxy-pools/cloudflare-deploy",
            post(deploy_unavailable),
        )
        .route("/api/proxy-pools/deno-deploy", post(deploy_unavailable))
        .route("/api/proxy-pools/vercel-deploy", post(deploy_unavailable))
        .route("/api/translator/send", post(translator_send))
        .route("/api/translator/translate", post(translator_translate))
        .route("/api/translator/load", get(translator_load))
        .route("/api/translator/save", post(translator_save))
        .route(
            "/api/translator/console-logs",
            get(console_logs).delete(clear_console_logs),
        )
        .route(
            "/api/translator/console-logs/stream",
            get(console_log_stream),
        )
        .route("/api/mcp/:plugin/message", post(mcp_message))
        .route("/api/mcp/:plugin/sse", get(mcp_sse))
        .route("/api/headroom/status", get(headroom_status))
        .route(
            "/api/headroom/extras",
            get(headroom_extras)
                .post(headroom_extras_set)
                .delete(headroom_extras_del),
        )
        .route("/api/headroom/start", post(daemon_unmanaged))
        .route("/api/headroom/stop", post(daemon_unmanaged))
        .route("/api/headroom/restart", post(daemon_unmanaged))
        .route("/api/pxpipe/status", get(pxpipe_status))
        .route("/api/pxpipe/stats", get(pxpipe_status))
        .route("/api/pxpipe/logs", get(pxpipe_logs))
        .route("/api/pxpipe/health", post(daemon_unmanaged))
        .route("/api/pxpipe/install", post(daemon_unmanaged))
        .route("/api/pxpipe/start", post(daemon_unmanaged))
        .route("/api/pxpipe/stop", post(daemon_unmanaged))
        .route("/api/pxpipe/restart", post(daemon_unmanaged))
        .route("/api/tunnel/status", get(tunnel_status))
        .route("/api/tunnel/tailscale-check", get(tailscale_check))
        .route("/api/tunnel/enable", post(daemon_unmanaged))
        .route("/api/tunnel/disable", post(daemon_unmanaged))
        .route("/api/tunnel/tailscale-enable", post(daemon_unmanaged))
        .route("/api/tunnel/tailscale-disable", post(daemon_unmanaged))
        .route("/api/tunnel/tailscale-install", post(daemon_unmanaged))
        .route("/api/version/shutdown", post(version_shutdown))
        .route("/api/version/update", post(version_update))
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

fn ok(v: Value) -> Response {
    Json(v).into_response()
}

fn bad(msg: &str) -> Response {
    err(400, msg, "invalid_request_error", "bad_request")
}

fn missing_store() -> Response {
    err(500, "storage unavailable", "internal_error", "no_store")
}

fn unavailable(feature: &str) -> Response {
    err(
        501,
        &format!("{feature} is not available in this build"),
        "not_implemented",
        "unavailable",
    )
}

#[allow(clippy::result_large_err)]
async fn db<T, F>(st: &AppState, f: F) -> Result<T, Response>
where
    T: Send + 'static,
    F: FnOnce(Arc<Store>) -> Result<T, nine_storage::DbError> + Send + 'static,
{
    let store = st.store.clone().ok_or_else(missing_store)?;
    tokio::task::spawn_blocking(move || f(store))
        .await
        .map_err(|_| err(500, "storage task failed", "internal_error", "db_join"))?
        .map_err(|_| err(500, "storage error", "internal_error", "db_error"))
}

/// Public-safe connection shape. `data` holds tokens/cookies and is never emitted.
pub(crate) fn pub_conn(c: &ProviderConnection) -> Value {
    serde_json::json!({
        "id": c.id,
        "provider": c.provider,
        "authType": c.auth_type,
        "name": c.name,
        "email": c.email,
        "priority": c.priority,
        "isActive": c.is_active,
        "expiresAt": c.data.get("expires_at")
            .or_else(|| c.data.get("expiresAt"))
            .and_then(|v| v.as_i64())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| dt.to_rfc3339()),
        "scope": c.data.get("scope").and_then(|v| v.as_str()).unwrap_or_default(),
        "createdAt": c.created_at,
        "updatedAt": c.updated_at,
    })
}

async fn cors_preflight() -> Response {
    (
        StatusCode::NO_CONTENT,
        [
            ("access-control-allow-origin", "*"),
            (
                "access-control-allow-methods",
                "GET, POST, PUT, PATCH, DELETE, OPTIONS",
            ),
            ("access-control-allow-headers", "*"),
        ],
    )
        .into_response()
}

// ─── API keys ───────────────────────────────────────────────────────────────

async fn list_keys(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.list_api_key_rows()).await {
        Ok(rows) => ok(serde_json::json!({"keys": rows})),
        Err(r) => r,
    }
}

async fn create_key(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return bad("Name is required");
    }
    // ponytail: machine id is a random install id; upstream derives a stable
    // machine id. Persist ours so restarts keep it stable.
    let machine_id = match db(&st, |s| s.kv_list("install")).await {
        Ok(m) => m.get("machineId").cloned().unwrap_or_default(),
        Err(_) => String::new(),
    };
    let machine_id = if machine_id.is_empty() {
        let id = uuid::Uuid::new_v4().to_string();
        let v = id.clone();
        let _ = db(&st, move |s| {
            s.kv_set("install", "machineId", &v)?;
            Ok(())
        })
        .await;
        id
    } else {
        machine_id
    };
    match db(&st, move |s| s.create_api_key(&name, &machine_id)).await {
        Ok(k) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "key": k["key"], "name": k["name"], "id": k["id"], "machineId": k["machineId"],
            })),
        )
            .into_response(),
        Err(r) => r,
    }
}

async fn get_key(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.get_api_key_row(&id)).await {
        Ok(Some(k)) => ok(serde_json::json!({"key": k})),
        Ok(None) => err(404, "Key not found", "not_found_error", "key_not_found"),
        Err(r) => r,
    }
}

async fn update_key(
    State(st): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let active = body.get("isActive").and_then(|v| v.as_bool());
    match db(&st, move |s| {
        Ok(match active {
            Some(a) => s.set_api_key_active(&id, a)?,
            None => s.get_api_key_row(&id)?,
        })
    })
    .await
    {
        Ok(Some(k)) => ok(serde_json::json!({"key": k})),
        Ok(None) => err(404, "Key not found", "not_found_error", "key_not_found"),
        Err(r) => r,
    }
}

async fn delete_key(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.delete_api_key(&id)).await {
        Ok(n) if n > 0 => ok(serde_json::json!({"message": "Key deleted successfully"})),
        Ok(_) => err(404, "Key not found", "not_found_error", "key_not_found"),
        Err(r) => r,
    }
}

// ─── Locale / tags ──────────────────────────────────────────────────────────

const LOCALES: &[&str] = &[
    "en", "vi", "zh-CN", "zh-TW", "ja", "pt-BR", "pt-PT", "ko", "es", "de", "fr", "he", "ar", "ru",
    "pl", "cs", "nl", "tr", "uk", "tl", "id", "km", "th", "hi", "bn", "ur", "ro", "sv", "it", "el",
    "hu", "fi", "da", "no", "fa",
];

async fn set_locale(Json(body): Json<Value>) -> Response {
    let raw = body.get("locale").and_then(|v| v.as_str()).unwrap_or("");
    let hit = LOCALES
        .iter()
        .find(|l| **l == raw)
        .or_else(|| LOCALES.iter().find(|l| l.eq_ignore_ascii_case(raw)));
    let Some(locale) = hit else {
        return bad("Invalid locale");
    };
    (
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            format!("locale={locale}; Path=/; Max-Age=31536000"),
        )],
        Json(serde_json::json!({"success": true, "locale": locale})),
    )
        .into_response()
}

/// Mirrors upstream `open-sse/config/ollamaModels.js` served at `/api/tags`.
async fn tags() -> Response {
    (
        StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("access-control-allow-origin", "*"),
            ("access-control-allow-methods", "GET, OPTIONS"),
            ("access-control-allow-headers", "*"),
        ],
        Json(serde_json::json!({
            "models": [
                {"name": "llama3.2", "modified_at": "2025-12-26T00:00:00Z", "size": 2000000000i64,
                 "digest": "abc123def456",
                 "details": {"format": "gguf", "family": "llama", "parameter_size": "3B", "quantization_level": "Q4_K_M"}},
                {"name": "qwen2.5", "modified_at": "2025-12-26T00:00:00Z", "size": 4000000000i64,
                 "digest": "def456abc123",
                 "details": {"format": "gguf", "family": "qwen", "parameter_size": "7B", "quantization_level": "Q4_K_M"}}
            ]
        })),
    )
        .into_response()
}

// ─── Pricing ────────────────────────────────────────────────────────────────

async fn get_pricing(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.kv_get("pricing", "config")).await {
        Ok(Some(raw)) => ok(serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)),
        Ok(None) => ok(serde_json::json!({})),
        Err(r) => r,
    }
}

async fn update_pricing(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if !body.is_object() {
        return bad("Invalid pricing data format");
    }
    for (provider, models) in body.as_object().unwrap() {
        let Some(models) = models.as_object() else {
            return err(
                400,
                &format!("Invalid pricing for provider: {provider}"),
                "invalid_request_error",
                "bad_request",
            );
        };
        for (model, pricing) in models {
            if !pricing.is_object() {
                return err(
                    400,
                    &format!("Invalid pricing for model: {model}"),
                    "invalid_request_error",
                    "bad_request",
                );
            }
        }
    }
    match db(&st, move |s| {
        let cur: Value = s
            .kv_get("pricing", "config")?
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or(serde_json::json!({}));
        let mut merged = cur.as_object().cloned().unwrap_or_default();
        for (k, v) in body.as_object().unwrap() {
            merged.insert(k.clone(), v.clone());
        }
        let raw = serde_json::to_string(&Value::Object(merged))?;
        s.kv_set("pricing", "config", &raw)?;
        Ok(raw)
    })
    .await
    {
        Ok(raw) => ok(serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)),
        Err(r) => r,
    }
}

async fn reset_pricing(State(st): State<Shared>) -> Response {
    match db(&st, |s| {
        s.kv_delete("pricing", "config")?;
        Ok(())
    })
    .await
    {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

// ─── Settings ───────────────────────────────────────────────────────────────

async fn update_settings(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if !body.is_object() {
        return bad("Invalid settings body");
    }
    match db(&st, move |s| s.settings_merge(&body)).await {
        Ok(v) => ok(v),
        Err(r) => r,
    }
}

async fn require_login(State(st): State<Shared>) -> Response {
    let v = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v,
        Err(_) => Value::Null,
    };
    ok(serde_json::json!({
        "requireLogin": v.get("requireLogin").and_then(|b| b.as_bool()).unwrap_or(true),
        "tunnelDashboardAccess": v.get("tunnelDashboardAccess").and_then(|b| b.as_bool()).unwrap_or(false),
        "tunnelUrl": v.get("tunnelUrl").and_then(|u| u.as_str()).unwrap_or(""),
        "tailscaleUrl": v.get("tailscaleUrl").and_then(|u| u.as_str()).unwrap_or(""),
    }))
}

async fn export_db(State(st): State<Shared>) -> Response {
    let settings = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let connections = match db(&st, |s| s.list_connections(None)).await {
        Ok(v) => v.into_iter().map(|c| pub_conn(&c)).collect::<Vec<_>>(),
        Err(r) => return r,
    };
    ok(serde_json::json!({"settings": settings, "connections": connections}))
}

async fn import_db(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if !body.is_object() {
        return bad("Invalid import payload");
    }
    if let Some(settings) = body.get("settings") {
        let patch = settings.clone();
        if let Err(r) = db(&st, move |s| s.settings_merge(&patch)).await.map(|_| ()) {
            return r;
        }
    }
    ok(serde_json::json!({"success": true}))
}

async fn proxy_test(Json(body): Json<Value>) -> Response {
    let url = body
        .get("proxyUrl")
        .or_else(|| body.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match proxy_reachable(url, Duration::from_secs(5)).await {
        Ok(ms) => ok(serde_json::json!({"ok": true, "latencyMs": ms})),
        Err(e) => ok(serde_json::json!({"ok": false, "error": e})),
    }
}

async fn proxy_reachable(url: &str, timeout: Duration) -> Result<u128, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid proxy URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err("Unsupported proxy scheme".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "Proxy URL has no host".to_string())?
        .to_string();
    let port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
    let start = Instant::now();
    tokio::time::timeout(
        timeout,
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| "Connection timed out".to_string())?
    .map_err(|e| format!("Connection failed: {e}"))?;
    Ok(start.elapsed().as_millis())
}

// ─── Providers ──────────────────────────────────────────────────────────────

async fn create_provider(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let provider = body
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if provider.is_empty() {
        return bad("provider is required");
    }
    if nine_providers::base_url_for(&provider).is_empty()
        && body
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
    {
        return bad("unknown provider");
    }
    let auth_type = body
        .get("authType")
        .and_then(|v| v.as_str())
        .unwrap_or("apiKey")
        .to_string();
    let name = body.get("name").and_then(|v| v.as_str()).map(String::from);
    let email = body.get("email").and_then(|v| v.as_str()).map(String::from);
    let priority = body.get("priority").and_then(|v| v.as_i64());
    let mut data = body.get("data").cloned().unwrap_or(serde_json::json!({}));
    if !data.is_object() {
        data = serde_json::json!({});
    }
    for k in ["apiKey", "baseUrl", "accessToken", "refreshToken"] {
        if let Some(v) = body.get(k) {
            data[k] = v.clone();
        }
    }
    match db(&st, move |s| {
        s.create_connection(
            &provider,
            &auth_type,
            name.as_deref(),
            email.as_deref(),
            priority,
            &data,
        )
    })
    .await
    {
        Ok(c) => (StatusCode::CREATED, Json(pub_conn(&c))).into_response(),
        Err(r) => r,
    }
}

async fn get_provider(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.get_connection(&id)).await {
        Ok(Some(c)) => ok(pub_conn(&c)),
        Ok(None) => err(
            404,
            "Connection not found",
            "not_found_error",
            "connection_not_found",
        ),
        Err(r) => r,
    }
}

async fn update_provider(
    State(st): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match db(&st, move |s| s.update_connection_fields(&id, &body)).await {
        Ok(Some(c)) => ok(pub_conn(&c)),
        Ok(None) => err(
            404,
            "Connection not found",
            "not_found_error",
            "connection_not_found",
        ),
        Err(r) => r,
    }
}

async fn delete_provider(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.delete_connection(&id)).await {
        Ok(n) if n > 0 => ok(serde_json::json!({"success": true})),
        Ok(_) => err(
            404,
            "Connection not found",
            "not_found_error",
            "connection_not_found",
        ),
        Err(r) => r,
    }
}

fn conn_base_and_key(c: &ProviderConnection) -> (String, String) {
    let base = c
        .data
        .get("baseUrl")
        .or_else(|| c.data.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let base = if base.is_empty() {
        nine_providers::base_url_for(&c.provider).to_string()
    } else {
        base
    };
    let key = c
        .data
        .get("apiKey")
        .or_else(|| c.data.get("accessToken"))
        .or_else(|| c.data.get("access_token"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    (base, key)
}

async fn provider_models(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    let conn = match db(&st, move |s| s.get_connection(&id)).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return err(
                404,
                "Connection not found",
                "not_found_error",
                "connection_not_found",
            )
        }
        Err(r) => return r,
    };
    let (base, key) = conn_base_and_key(&conn);
    if base.is_empty() {
        return bad("provider has no base URL");
    }
    let url = format!("{}/models", base.trim_end_matches('/'));
    let mut req = st.client.get(&url).timeout(Duration::from_secs(15));
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let v: Value = resp.json().await.unwrap_or(Value::Null);
            let models = v
                .get("data")
                .or_else(|| v.get("models"))
                .cloned()
                .unwrap_or(Value::Null);
            ok(serde_json::json!({"models": models, "status": status}))
        }
        Err(e) => err(
            502,
            &format!("provider request failed: {e}"),
            "upstream_error",
            "provider_unreachable",
        ),
    }
}

async fn test_provider(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    let conn = match db(&st, move |s| s.get_connection(&id)).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return err(
                404,
                "Connection not found",
                "not_found_error",
                "connection_not_found",
            )
        }
        Err(r) => return r,
    };
    let (base, key) = conn_base_and_key(&conn);
    if base.is_empty() || key.is_empty() {
        return ok(
            serde_json::json!({"ok": false, "error": "connection has no base URL or credential"}),
        );
    }
    let url = format!("{}/models", base.trim_end_matches('/'));
    let start = Instant::now();
    match st
        .client
        .get(&url)
        .bearer_auth(&key)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 401 || status == 403 {
                ok(
                    serde_json::json!({"ok": false, "status": status, "error": "invalid credential"}),
                )
            } else if resp.status().is_success() {
                ok(
                    serde_json::json!({"ok": true, "status": status, "latencyMs": start.elapsed().as_millis() as u64}),
                )
            } else {
                ok(serde_json::json!({"ok": false, "status": status}))
            }
        }
        Err(e) => ok(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

async fn test_provider_models(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    let conn = match db(&st, move |s| s.get_connection(&id)).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return err(
                404,
                "Connection not found",
                "not_found_error",
                "connection_not_found",
            )
        }
        Err(r) => return r,
    };
    let (base, key) = conn_base_and_key(&conn);
    let catalog_models: Vec<(String, String)> = st
        .catalog
        .iter()
        .filter(|m| {
            m.owned_by == conn.provider || nine_providers::infer_provider(&m.id) == conn.provider
        })
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();
    if catalog_models.is_empty() {
        return bad("No models configured for this provider");
    }
    if base.is_empty() {
        return bad("provider has no base URL");
    }
    let url = format!("{}/models", base.trim_end_matches('/'));
    let mut req = st.client.get(&url).timeout(Duration::from_secs(15));
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    let live: std::collections::HashSet<String> = match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let v: Value = resp.json().await.unwrap_or(Value::Null);
            v.get("data")
                .and_then(|d| d.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => std::collections::HashSet::new(),
    };
    let results: Vec<Value> = catalog_models
        .into_iter()
        .map(|(mid, name)| {
            let bare = mid.split('/').next_back().unwrap_or(&mid);
            let ok = live.is_empty() || live.contains(bare) || live.contains(&mid);
            serde_json::json!({"modelId": bare, "name": name, "ok": ok})
        })
        .collect();
    ok(serde_json::json!({"results": results}))
}

async fn providers_client(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.list_connections(None)).await {
        Ok(rows) => ok(serde_json::json!({
            "connections": rows.into_iter().filter(|c| c.is_active).map(|c| pub_conn(&c)).collect::<Vec<_>>(),
        })),
        Err(r) => r,
    }
}

async fn suggested_models(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let models: Vec<Value> = st.catalog.iter()
        .filter(|m| q.get("provider").map(|p| &m.owned_by == p).unwrap_or(true))
        .take(50)
        .map(|m| serde_json::json!({"id": m.id, "name": m.name, "kind": m.kind, "owned_by": m.owned_by}))
        .collect();
    ok(serde_json::json!({"models": models}))
}

async fn validate_provider(Json(body): Json<Value>) -> Response {
    let provider = body.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    if provider.is_empty() {
        return bad("provider is required");
    }
    let known = !nine_providers::base_url_for(provider).is_empty();
    let base = body
        .get("baseUrl")
        .or_else(|| body.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !known && base.is_empty() {
        return bad("unknown provider");
    }
    if !base.is_empty() {
        let parsed = reqwest::Url::parse(base)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        if parsed.is_none() {
            return bad("invalid baseUrl");
        }
    }
    ok(serde_json::json!({"ok": true, "provider": provider,
        "baseUrl": if base.is_empty() { nine_providers::base_url_for(provider).to_string() } else { base.to_string() }}))
}

async fn test_provider_batch(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let ids: Vec<String> = body
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return bad("ids[] required");
    }
    let mut results = Vec::new();
    for id in ids {
        let probe_id = id.clone();
        let conn = match db(&st, move |s| s.get_connection(&probe_id)).await {
            Ok(Some(c)) => c,
            _ => {
                results.push(
                    serde_json::json!({"id": id, "ok": false, "error": "Connection not found"}),
                );
                continue;
            }
        };
        let (base, key) = conn_base_and_key(&conn);
        if base.is_empty() || key.is_empty() {
            results.push(serde_json::json!({"id": conn.id, "ok": false, "error": "no base URL or credential"}));
            continue;
        }
        let url = format!("{}/models", base.trim_end_matches('/'));
        let r = st
            .client
            .get(&url)
            .bearer_auth(&key)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        match r {
            Ok(resp) => results.push(serde_json::json!({"id": conn.id, "ok": resp.status().is_success(), "status": resp.status().as_u16()})),
            Err(e) => results.push(serde_json::json!({"id": conn.id, "ok": false, "error": e.to_string()})),
        }
    }
    ok(serde_json::json!({"results": results}))
}

static KILO_CACHE: OnceLock<Mutex<(Instant, Value)>> = OnceLock::new();

async fn kilo_free_models(State(st): State<Shared>) -> Response {
    if let Some(lock) = KILO_CACHE.get() {
        if let Ok(g) = lock.lock() {
            if g.0.elapsed() < Duration::from_secs(3600) {
                return ok(serde_json::json!({"models": g.1, "cached": true}));
            }
        }
    }
    let resp = st
        .client
        .get("https://api.kilo.ai/api/gateway/models")
        .header("accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    let free: Option<Value> = match resp {
        Ok(r) if r.status().is_success() => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            Some(Value::Array(v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default()
                .into_iter().filter(|m| m.get("isFree").and_then(|b| b.as_bool()).unwrap_or(false))
                .map(|m| serde_json::json!({
                    "id": m.get("id"), "name": m.get("name"), "isFree": true,
                    "context_length": m.get("context_length").and_then(|c| c.as_u64()).unwrap_or(0),
                })).collect()))
        }
        _ => None,
    };
    match free {
        Some(models) => {
            let lock = KILO_CACHE.get_or_init(|| Mutex::new((Instant::now(), Value::Null)));
            if let Ok(mut g) = lock.lock() {
                *g = (Instant::now(), models.clone());
            }
            ok(serde_json::json!({"models": models, "cached": false}))
        }
        None => {
            if let Some(lock) = KILO_CACHE.get() {
                if let Ok(g) = lock.lock() {
                    if !g.1.is_null() {
                        return ok(
                            serde_json::json!({"models": g.1, "cached": true, "warning": "upstream unreachable"}),
                        );
                    }
                }
            }
            err(
                502,
                "Kilo API unreachable",
                "upstream_error",
                "provider_unreachable",
            )
        }
    }
}

// ─── Models extras ──────────────────────────────────────────────────────────

async fn put_models(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let (Some(model), Some(alias)) = (
        body.get("model").and_then(|v| v.as_str()),
        body.get("alias").and_then(|v| v.as_str()),
    ) else {
        if let Some(aliases) = body.get("aliases").and_then(|v| v.as_object()) {
            let pairs: Vec<(String, String)> = aliases
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_string())))
                .collect();
            if pairs.is_empty() {
                return bad("Model and alias required");
            }
            let res = db(&st, move |s| {
                for (a, m) in &pairs {
                    s.set_model_alias(a, m)?;
                }
                Ok(())
            })
            .await;
            return match res {
                Ok(()) => ok(serde_json::json!({"success": true})),
                Err(r) => r,
            };
        }
        return bad("Model and alias required");
    };
    let (m, a) = (model.to_string(), alias.to_string());
    match db(&st, move |s| s.set_model_alias(&m, &a)).await {
        Ok(()) => ok(serde_json::json!({"success": true, "model": model, "alias": alias})),
        Err(r) => r,
    }
}

async fn model_availability(State(st): State<Shared>) -> Response {
    let locks: HashMap<String, String> = match db(&st, |s| s.kv_list("modelLocks")).await {
        Ok(m) => m,
        Err(r) => return r,
    };
    let now = chrono::Utc::now().to_rfc3339();
    let models: Vec<Value> = locks
        .into_iter()
        .filter(|(_, until)| until > &now)
        .map(|(k, until)| {
            let mut parts = k.splitn(2, ':');
            serde_json::json!({
                "connectionId": parts.next().unwrap_or(""),
                "model": parts.next().unwrap_or("__all"),
                "until": until, "active": true,
            })
        })
        .collect();
    ok(serde_json::json!({"models": models}))
}

async fn set_model_availability(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let conn = body
        .get("connectionId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("__all")
        .to_string();
    let until = body
        .get("until")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| {
            let mins = body.get("minutes").and_then(|v| v.as_i64()).unwrap_or(10);
            (chrono::Utc::now() + chrono::Duration::minutes(mins)).to_rfc3339()
        });
    if conn.is_empty() {
        return bad("connectionId is required");
    }
    let key = format!("{conn}:{model}");
    match db(&st, move |s| {
        s.kv_set("modelLocks", &key, &until)?;
        Ok(())
    })
    .await
    {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

fn catalog_summary(st: &AppState) -> Value {
    let mut providers = std::collections::HashSet::new();
    for m in st.catalog.iter() {
        providers.insert(m.owned_by.clone());
    }
    serde_json::json!({
        "syncedAt": Value::Null,
        "models": st.catalog.len(),
        "providers": providers.len(),
        "bytes": 0,
        "source": "builtin",
    })
}

async fn catalog_sync(State(st): State<Shared>) -> Response {
    ok(catalog_summary(&st))
}

async fn run_catalog_sync(State(st): State<Shared>) -> Response {
    let mut v = catalog_summary(&st);
    v["success"] = Value::Bool(true);
    ok(v)
}

async fn list_custom_models(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.kv_list("customModels")).await {
        Ok(m) => {
            let models: Vec<Value> = m
                .values()
                .filter_map(|r| serde_json::from_str(r).ok())
                .collect();
            ok(serde_json::json!({"models": models}))
        }
        Err(r) => r,
    }
}

async fn add_custom_model(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return bad("id is required");
    }
    let mut m = body.clone();
    if m.get("providerAlias").is_none() {
        let p = body
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("custom")
            .to_string();
        m["providerAlias"] = Value::String(p);
    }
    match db(&st, move |s| {
        let raw = serde_json::to_string(&m)?;
        s.kv_set("customModels", &id, &raw)?;
        Ok(raw)
    })
    .await
    {
        Ok(raw) => ok(
            serde_json::json!({"success": true, "model": serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)}),
        ),
        Err(r) => r,
    }
}

async fn delete_custom_model(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(id) = q.get("id").cloned().filter(|v| !v.is_empty()) else {
        return bad("id is required");
    };
    match db(&st, move |s| s.kv_delete("customModels", &id)).await {
        Ok(_) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

async fn get_disabled(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    match db(&st, |s| s.kv_list("disabledModels")).await {
        Ok(m) => {
            let mut all = serde_json::Map::new();
            for (k, v) in &m {
                all.insert(k.clone(), serde_json::from_str(v).unwrap_or(Value::Null));
            }
            if let Some(alias) = q.get("providerAlias") {
                ok(
                    serde_json::json!({"ids": all.get(alias).cloned().unwrap_or(serde_json::json!([]))}),
                )
            } else {
                ok(serde_json::json!({"disabled": all}))
            }
        }
        Err(r) => r,
    }
}

async fn set_disabled(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let (Some(alias), Some(ids)) = (
        body.get("providerAlias").and_then(|v| v.as_str()),
        body.get("ids").and_then(|v| v.as_array()),
    ) else {
        return bad("providerAlias and ids[] required");
    };
    let (alias, mut ids): (String, Vec<String>) = (
        alias.to_string(),
        ids.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
    );
    match db(&st, move |s| {
        let mut cur: Vec<String> = s
            .kv_get("disabledModels", &alias)?
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or_default();
        for id in std::mem::take(&mut ids) {
            if !cur.contains(&id) {
                cur.push(id);
            }
        }
        s.kv_set("disabledModels", &alias, &serde_json::to_string(&cur)?)?;
        Ok(())
    })
    .await
    {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

async fn clear_disabled(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    match q.get("providerAlias").cloned() {
        Some(alias) if !alias.is_empty() => {
            match db(&st, move |s| s.kv_delete("disabledModels", &alias)).await {
                Ok(_) => ok(serde_json::json!({"success": true})),
                Err(r) => r,
            }
        }
        _ => match db(&st, |s| {
            for k in s
                .kv_list("disabledModels")?
                .keys()
                .cloned()
                .collect::<Vec<_>>()
            {
                s.kv_delete("disabledModels", &k)?;
            }
            Ok(())
        })
        .await
        {
            Ok(()) => ok(serde_json::json!({"success": true})),
            Err(r) => r,
        },
    }
}

async fn test_model(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let Some(model) = body.get("model").and_then(|v| v.as_str()).map(String::from) else {
        return bad("Model required");
    };
    let kind = body
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("llm")
        .to_string();
    let resolved = match db(&st, |s| s.get_model_aliases()).await {
        Ok(a) => nine_routing::resolve_alias_chain(&model, &a),
        Err(_) => model.clone(),
    };
    let known = st.catalog.iter().any(|m| m.id == resolved || m.id == model);
    if known {
        ok(
            serde_json::json!({"ok": true, "model": model, "resolved": resolved, "kind": kind, "source": "catalog"}),
        )
    } else {
        ok(
            serde_json::json!({"ok": false, "model": model, "kind": kind, "error": "unknown model", "source": "catalog"}),
        )
    }
}

// ─── Usage detail ───────────────────────────────────────────────────────────

const PERIODS: &[&str] = &["today", "24h", "7d", "30d", "60d", "all"];

#[allow(clippy::result_large_err)]
fn check_period(q: &HashMap<String, String>) -> Result<(), Response> {
    let period = q.get("period").map(|s| s.as_str()).unwrap_or("7d");
    if PERIODS.contains(&period) {
        Ok(())
    } else {
        Err(bad("Invalid period"))
    }
}

async fn usage_history(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.usage_totals()).await {
        Ok(v) => ok(v),
        Err(r) => r,
    }
}

async fn usage_chart(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = check_period(&q) {
        return r;
    }
    match db(&st, |s| s.usage_chart(60)).await {
        Ok(v) => ok(v),
        Err(r) => r,
    }
}

async fn usage_logs(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.usage_recent(200)).await {
        Ok(v) => ok(Value::Array(v)),
        Err(r) => r,
    }
}

async fn request_details(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.request_details_list(200)).await {
        Ok(v) => ok(Value::Array(v)),
        Err(r) => r,
    }
}

async fn usage_providers(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.usage_by_provider()).await {
        Ok(v) => ok(v),
        Err(r) => r,
    }
}

async fn usage_stream(State(st): State<Shared>) -> Response {
    let totals = match db(&st, |s| s.usage_totals()).await {
        Ok(v) => v,
        Err(_) => serde_json::json!({}),
    };
    (
        StatusCode::OK,
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        format!("data: {totals}\n\n"),
    )
        .into_response()
}

async fn usage_for_connection(
    State(st): State<Shared>,
    Path(connection_id): Path<String>,
) -> Response {
    let conn = match db(&st, move |s| s.get_connection(&connection_id)).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return err(
                404,
                "Connection not found",
                "not_found_error",
                "connection_not_found",
            )
        }
        Err(r) => return r,
    };
    ok(serde_json::json!({"connection": pub_conn(&conn), "usage": Value::Null}))
}

async fn codex_credits(State(st): State<Shared>, Path(connection_id): Path<String>) -> Response {
    match db(&st, move |s| s.get_connection(&connection_id)).await {
        Ok(Some(_)) => unavailable("live Codex credit lookup (requires provider session)"),
        Ok(None) => err(
            404,
            "Connection not found",
            "not_found_error",
            "connection_not_found",
        ),
        Err(r) => r,
    }
}

async fn codex_consume_credit(
    State(st): State<Shared>,
    Path(connection_id): Path<String>,
) -> Response {
    match db(&st, move |s| s.get_connection(&connection_id)).await {
        Ok(Some(_)) => unavailable("live Codex credit redeem (requires provider session)"),
        Ok(None) => err(
            404,
            "Connection not found",
            "not_found_error",
            "connection_not_found",
        ),
        Err(r) => r,
    }
}

// ─── OAuth token imports ────────────────────────────────────────────────────

fn provider_from_path(provider: &str) -> &str {
    match provider {
        p if p.starts_with("xiaomi") => "xiaomi-mimo",
        p if p.starts_with("grok") => "grok-cli",
        p if p.starts_with("cursor") => "cursor",
        p if p.starts_with("kiro") => "kiro",
        p if p.starts_with("iflow") => "iflow",
        p if p.starts_with("codex") => "codex",
        p => p,
    }
}

async fn store_imported(
    st: &AppState,
    provider: &str,
    auth_type: &str,
    name: Option<String>,
    email: Option<String>,
    data: Value,
) -> Response {
    let (p, a) = (provider.to_string(), auth_type.to_string());
    match db(st, move |s| {
        s.create_connection(&p, &a, name.as_deref(), email.as_deref(), None, &data)
    })
    .await
    {
        // Shape mirrors the generic import-token action: {ok, connection{id,..}}.
        Ok(c) => ok(
            serde_json::json!({"ok": true, "success": true, "id": c.id, "provider": c.provider,
            "connection": {"id": c.id, "provider": c.provider, "authType": c.auth_type, "name": c.name}}),
        ),
        Err(r) => r,
    }
}

fn provider_from_uri(uri: &axum::http::Uri) -> String {
    let seg = uri.path().split("/").nth(3).unwrap_or("");
    provider_from_path(seg).to_string()
}

async fn import_token_generic(
    State(st): State<Shared>,
    uri: axum::http::Uri,
    Json(body): Json<Value>,
) -> Response {
    let provider = provider_from_uri(&uri);
    let token = body
        .get("accessToken")
        .or_else(|| body.get("token"))
        .or_else(|| body.get("apiKey"))
        .or_else(|| body.get("cookie"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return bad("token is required");
    }
    let auth_type = if body.get("cookie").is_some() {
        "cookie"
    } else if body.get("apiKey").is_some() {
        "apiKey"
    } else {
        "oauth"
    };
    let name = body.get("name").and_then(|v| v.as_str()).map(String::from);
    let data = serde_json::json!({"access_token": token, "authMethod": auth_type});
    store_imported(&st, &provider, auth_type, name, None, data).await
}

async fn import_codex_token(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let token = body
        .get("accessToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if token.is_empty() {
        return bad("Access token is required");
    }
    // Display-only JWT decode mirrors upstream (email for the connections list).
    let mut email: Option<String> = None;
    if token.split('.').count() == 3 {
        if let Some(payload) = token.split('.').nth(1) {
            let mut b64 = payload.replace('-', "+").replace('_', "/");
            while b64.len() % 4 != 0 {
                b64.push('=');
            }
            if let Ok(raw) = base64_decode(&b64) {
                if let Ok(v) = serde_json::from_slice::<Value>(&raw) {
                    email = v
                        .pointer("/https://api.openai.com/profile/email")
                        .or_else(|| v.get("email"))
                        .and_then(|e| e.as_str())
                        .map(String::from);
                }
            }
        }
    }
    let name = body.get("name").and_then(|v| v.as_str()).map(String::from);
    let data = serde_json::json!({"access_token": token, "authMethod": "access_token"});
    store_imported(&st, "codex", "access_token", name, email, data).await
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|&x| x == c).ok_or("bad base64")? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

async fn bulk_import_tokens(
    State(st): State<Shared>,
    uri: axum::http::Uri,
    Json(body): Json<Value>,
) -> Response {
    let tokens: Vec<Value> = body
        .get("tokens")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| body.get("token").map(|t| vec![t.clone()]))
        .unwrap_or_default();
    if tokens.is_empty() {
        return bad("tokens[] required");
    }
    let path = body
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let provider = if path.is_empty() {
        provider_from_uri(&uri)
    } else {
        provider_from_path(&path).to_string()
    };
    let mut imported = 0;
    for t in tokens {
        let token = t
            .as_str()
            .or_else(|| t.get("accessToken").and_then(|v| v.as_str()))
            .unwrap_or("");
        if token.is_empty() {
            continue;
        }
        let data = serde_json::json!({"access_token": token, "authMethod": "bulk_import"});
        let p = provider.clone();
        let r: Result<(), Response> = db(&st, move |s| {
            s.create_connection(&p, "oauth", None, None, None, &data)?;
            Ok(())
        })
        .await;
        if r.is_ok() {
            imported += 1;
        }
    }
    ok(serde_json::json!({"success": true, "imported": imported}))
}

async fn import_gitlab_pat(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let token = body
        .get("token")
        .or_else(|| body.get("privateToken"))
        .or_else(|| body.get("accessToken"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return bad("token is required");
    }
    let base = body
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("https://gitlab.com")
        .to_string();
    // Live validation mirrors upstream: reject bad tokens instead of storing them.
    let url = format!("{}/api/v4/user", base.trim_end_matches('/'));
    let check = st
        .client
        .get(&url)
        .header("PRIVATE-TOKEN", &token)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    let username: Option<String> = match check {
        Ok(r) if r.status().is_success() => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("username").and_then(|u| u.as_str()).map(String::from)),
        Ok(r) if r.status().as_u16() == 401 => {
            return err(
                401,
                "invalid token",
                "authentication_error",
                "invalid_token",
            )
        }
        _ => None,
    };
    let data = serde_json::json!({"access_token": token, "baseUrl": base, "authMethod": "pat"});
    store_imported(&st, "gitlab", "pat", username.clone(), None, data).await
}

static AUTO_IMPORT_PATHS: &[(&str, &str)] = &[
    ("cursor", ".cursor/auth.json"),
    ("kiro", ".kiro/auth.json"),
    ("xiaomi-mimo", ".xiaomi/auth.json"),
];

async fn auto_import_probe(uri: axum::http::Uri) -> Response {
    let provider = provider_from_uri(&uri);
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let hits: Vec<String> = AUTO_IMPORT_PATHS
        .iter()
        .filter(|(p, _)| *p == provider)
        .map(|(_, rel)| format!("{home}/{rel}"))
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    ok(
        serde_json::json!({"success": true, "imported": 0, "checked": hits,
        "message": "no local credentials found"}),
    )
}

const KIRO_AUTH_SERVICE: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
const KIRO_REDIRECT: &str = "kiro://kiro.kiroAgent/authenticate-success";

async fn kiro_social_authorize(Query(q): Query<HashMap<String, String>>) -> Response {
    let idp = match q.get("provider").map(|s| s.as_str()) {
        Some("google") => "Google",
        Some("github") => "Github",
        _ => return bad("Invalid provider. Use 'google' or 'github'"),
    };
    let pkce = nine_oauth::Pkce::generate();
    let state = nine_oauth::new_state();
    let auth_url = format!("{KIRO_AUTH_SERVICE}/login?idp={idp}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={state}&prompt=select_account",
        percent_encode(KIRO_REDIRECT), pkce.challenge);
    ok(serde_json::json!({
        "authUrl": auth_url, "state": state,
        "codeVerifier": pkce.verifier, "codeChallenge": pkce.challenge,
        "provider": q.get("provider"),
    }))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn kiro_social_exchange(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if let Some(token) = body.get("accessToken").and_then(|v| v.as_str()) {
        let t = token.to_string();
        let data = serde_json::json!({"access_token": t, "authMethod": "social"});
        return store_imported(&st, "kiro", "oauth", None, None, data).await;
    }
    let (Some(code), Some(verifier)) = (
        body.get("code").and_then(|v| v.as_str()),
        body.get("codeVerifier").and_then(|v| v.as_str()),
    ) else {
        return bad("code and codeVerifier are required");
    };
    let resp = st.client.post(format!("{KIRO_AUTH_SERVICE}/oauth/token"))
        .json(&serde_json::json!({"code": code, "code_verifier": verifier, "redirect_uri": KIRO_REDIRECT}))
        .timeout(Duration::from_secs(15)).send().await;
    let tokens: Value = match resp {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        Ok(r) => {
            return err(
                502,
                &format!("token exchange failed: {}", r.status()),
                "upstream_error",
                "exchange_failed",
            )
        }
        Err(e) => {
            return err(
                502,
                &format!("token exchange failed: {e}"),
                "upstream_error",
                "exchange_failed",
            )
        }
    };
    let access = tokens
        .get("accessToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if access.is_empty() {
        return err(
            502,
            "token exchange returned no access token",
            "upstream_error",
            "exchange_failed",
        );
    }
    let data = serde_json::json!({
        "access_token": access,
        "refresh_token": tokens.get("refreshToken"),
        "profile_arn": tokens.get("profileArn"),
        "expires_in": tokens.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(3600),
        "authMethod": "social",
    });
    store_imported(&st, "kiro", "oauth", None, None, data).await
}

// ─── CLI-tool extras ────────────────────────────────────────────────────────

async fn mitm_status(State(st): State<Shared>) -> Response {
    let cfg = match db(&st, |s| s.kv_get("mitm", "config")).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    ok(serde_json::json!({"running": false, "managed": false, "config": cfg}))
}

async fn mitm_control() -> Response {
    unavailable("antigravity MITM proxy (not managed by this build)")
}

async fn mitm_config(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if !body.is_object() {
        return bad("Invalid config body");
    }
    match db(&st, move |s| {
        let raw = serde_json::to_string(&body)?;
        s.kv_set("mitm", "config", &raw)?;
        Ok(raw)
    })
    .await
    {
        Ok(raw) => ok(
            serde_json::json!({"success": true, "config": serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)}),
        ),
        Err(r) => r,
    }
}

async fn mitm_alias(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    match db(&st, |s| s.kv_list("mitmAliases")).await {
        Ok(m) => {
            let mut all = serde_json::Map::new();
            for (k, v) in &m {
                all.insert(k.clone(), serde_json::from_str(v).unwrap_or(Value::Null));
            }
            if let Some(tool) = q.get("tool") {
                ok(serde_json::json!({"aliases": all.get(tool).cloned().unwrap_or(Value::Null)}))
            } else {
                ok(serde_json::json!({"aliases": all}))
            }
        }
        Err(r) => r,
    }
}

async fn mitm_alias_put(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let tool = body
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let aliases = body.get("aliases").cloned().unwrap_or(Value::Null);
    match db(&st, move |s| {
        let raw = serde_json::to_string(&aliases)?;
        s.kv_set("mitmAliases", &tool, &raw)?;
        Ok(())
    })
    .await
    {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

static MCP_REGISTRY_CACHE: OnceLock<Mutex<(Instant, Value)>> = OnceLock::new();

async fn mcp_registry(State(st): State<Shared>) -> Response {
    if let Some(lock) = MCP_REGISTRY_CACHE.get() {
        if let Ok(g) = lock.lock() {
            if g.0.elapsed() < Duration::from_secs(3600) {
                return ok(serde_json::json!({"servers": g.1, "cached": true}));
            }
        }
    }
    let mut servers = Vec::new();
    let mut cursor = String::new();
    for _ in 0..20 {
        let url = format!("https://api.anthropic.com/mcp-registry/v0/servers?limit=500&visibility=commercial,gsuite,gsuite-google{cursor}");
        let resp = st
            .client
            .get(&url)
            .header("accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        let page: Value = match resp {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
            _ => break,
        };
        for s in page
            .get("servers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let url = s
                .get("url")
                .or_else(|| s.pointer("/transport/url"))
                .and_then(|u| u.as_str())
                .unwrap_or("");
            if url.starts_with("https://")
                && !url.contains("mcp.claude.com")
                && !url.contains("api.anthropic.com/mcp")
            {
                servers.push(s);
            }
        }
        cursor = page
            .get("nextCursor")
            .or_else(|| page.get("cursor"))
            .and_then(|c| c.as_str())
            .map(|c| format!("&cursor={c}"))
            .unwrap_or_default();
        if cursor.is_empty() {
            break;
        }
    }
    if servers.is_empty() {
        if let Some(lock) = MCP_REGISTRY_CACHE.get() {
            if let Ok(g) = lock.lock() {
                if !g.1.is_null() {
                    return ok(
                        serde_json::json!({"servers": g.1, "cached": true, "warning": "registry unreachable"}),
                    );
                }
            }
        }
        return ok(serde_json::json!({"servers": [], "warning": "registry unreachable"}));
    }
    let v = Value::Array(servers);
    let lock = MCP_REGISTRY_CACHE.get_or_init(|| Mutex::new((Instant::now(), Value::Null)));
    if let Ok(mut g) = lock.lock() {
        *g = (Instant::now(), v.clone());
    }
    ok(serde_json::json!({"servers": v, "cached": false}))
}

async fn mcp_probe(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let parsed = match reqwest::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
    {
        Some(u) => u,
        None => return bad("valid http(s) url is required"),
    };
    if let Some(host) = parsed.host_str() {
        if host == "localhost" || host.starts_with("127.") || host == "::1" {
            return bad("url must be public");
        }
    }
    let init = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "9router", "version": "0.1.0"}}
    });
    let r1 = st
        .client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-06-18")
        .json(&init)
        .timeout(Duration::from_secs(8))
        .send()
        .await;
    let ok_init = match r1 {
        Ok(r) if r.status().as_u16() == 401 => {
            return ok(serde_json::json!({"url": url, "authRequired": true, "tools": []}));
        }
        Ok(r) => r.status().is_success(),
        Err(e) => return ok(serde_json::json!({"url": url, "ok": false, "error": e.to_string()})),
    };
    if !ok_init {
        return ok(serde_json::json!({"url": url, "ok": false}));
    }
    let tools_req =
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
    let tools: Value = match st
        .client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-06-18")
        .json(&tools_req)
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        Ok(r) => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("result").and_then(|r| r.get("tools")).cloned())
            .unwrap_or(Value::Array(vec![])),
        Err(_) => Value::Array(vec![]),
    };
    ok(serde_json::json!({"url": url, "ok": true, "tools": tools}))
}

// ─── Media-provider TTS directory ───────────────────────────────────────────

async fn tts_voices() -> Response {
    ok(serde_json::json!({"voices": []}))
}

async fn tts_voices_unconfigured() -> Response {
    unavailable("TTS voices (no TTS provider configured)")
}

// ─── v1 media passthrough ───────────────────────────────────────────────────

fn first_upstream(st: &AppState) -> Option<(String, String)> {
    st.upstreams
        .first()
        .map(|u| (u.base_url.clone(), u.api_key.clone()))
}

async fn passthrough_post(st: &AppState, headers: &HeaderMap, path: &str, body: Bytes) -> Response {
    let Some((base, key)) = first_upstream(st) else {
        return unavailable(&format!("{path} (no upstream configured)"));
    };
    if serde_json::from_slice::<Value>(&body).is_err() {
        return err(400, "invalid json", "invalid_request_error", "invalid_json");
    }
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let mut req = st
        .client
        .post(&url)
        .header("content-type", ct)
        .body(body.to_vec())
        .timeout(Duration::from_millis(st.timeout_ms));
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    forward_upstream(req).await
}

async fn passthrough_get(st: &AppState, path: &str) -> Response {
    let Some((base, key)) = first_upstream(st) else {
        return unavailable(&format!("{path} (no upstream configured)"));
    };
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let mut req = st
        .client
        .get(&url)
        .timeout(Duration::from_millis(st.timeout_ms));
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    forward_upstream(req).await
}

async fn forward_upstream(req: reqwest::RequestBuilder) -> Response {
    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let ct = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            let bytes = resp.bytes().await.unwrap_or_default();
            (status, [("content-type", ct)], bytes).into_response()
        }
        Err(e) => err(
            502,
            &format!("upstream request failed: {e}"),
            "upstream_error",
            "upstream_unreachable",
        ),
    }
}

async fn media_passthrough(
    State(st): State<Shared>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Bytes,
) -> Response {
    passthrough_post(&st, &headers, &upstream_path(uri.path()), body).await
}

async fn media_get_passthrough(State(st): State<Shared>, uri: axum::http::Uri) -> Response {
    passthrough_get(&st, &upstream_path(uri.path())).await
}

/// Strip the local `/api` prefix so `/api/v1/embeddings` and
/// `/v1/embeddings` both forward to `/v1/embeddings` upstream.
fn upstream_path(path: &str) -> String {
    path.strip_prefix("/api").unwrap_or(path).to_string()
}
async fn api_chat(State(st): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    super::chat_completions(State(st), headers, body).await
}

async fn v1_model_detail(State(st): State<Shared>, Path(model): Path<String>) -> Response {
    let id = model.trim_start_matches('/');
    match st.catalog.iter().find(|m| m.id == id) {
        Some(m) => ok(serde_json::json!({
            "id": m.id, "object": "model", "owned_by": m.owned_by,
            "name": m.name, "kind": m.kind, "endpoint": m.endpoint,
        })),
        None => err(
            404,
            &format!("model not found: {id}"),
            "not_found_error",
            "model_not_found",
        ),
    }
}

// ─── Local dashboard auth ───────────────────────────────────────────────────

static AUTH_TOKEN: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static LOGIN_FAILS: OnceLock<Mutex<HashMap<String, (u32, Instant)>>> = OnceLock::new();

fn auth_token() -> &'static Mutex<Option<String>> {
    AUTH_TOKEN.get_or_init(|| Mutex::new(None))
}

fn login_fails() -> &'static Mutex<HashMap<String, (u32, Instant)>> {
    LOGIN_FAILS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn dashboard_authed(headers: &HeaderMap) -> bool {
    let token = auth_token()
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    if token.is_empty() {
        return false;
    }
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains(&format!("dashboard_auth={token}")))
        .unwrap_or(false)
}

async fn login(State(st): State<Shared>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("local")
        .to_string();
    {
        let g = login_fails().lock().unwrap();
        if let Some((_, until)) = g.get(&ip) {
            if Instant::now() < *until {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({"error": "Too many failed attempts"})),
                )
                    .into_response();
            }
        }
    }
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let stored = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v.get("password").and_then(|p| p.as_str()).map(String::from),
        Err(r) => return r,
    };
    let expected = stored.unwrap_or_else(|| "123456".to_string());
    if password != expected {
        let mut g = login_fails().lock().unwrap();
        let e = g.entry(ip).or_insert((0, Instant::now()));
        e.0 += 1;
        if e.0 >= 5 {
            e.1 = Instant::now() + Duration::from_secs(300);
        }
        return err(
            401,
            "Invalid password",
            "authentication_error",
            "invalid_password",
        );
    }
    login_fails().lock().unwrap().remove("local");
    let token = uuid::Uuid::new_v4().to_string();
    *auth_token().lock().unwrap() = Some(token.clone());
    (
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            format!("dashboard_auth={token}; Path=/; HttpOnly"),
        )],
        Json(serde_json::json!({"success": true})),
    )
        .into_response()
}

async fn logout() -> Response {
    *auth_token().lock().unwrap() = None;
    (
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            "dashboard_auth=; Path=/; Max-Age=0".to_string(),
        )],
        Json(serde_json::json!({"success": true})),
    )
        .into_response()
}

async fn reset_password(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let current = body
        .get("currentPassword")
        .or_else(|| body.get("password"))
        .and_then(|v| v.as_str());
    let next = body
        .get("newPassword")
        .and_then(|v| v.as_str())
        .unwrap_or("123456")
        .to_string();
    if next.len() < 6 {
        return bad("new password must be at least 6 characters");
    }
    let stored: Option<String> = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v.get("password").and_then(|p| p.as_str()).map(String::from),
        Err(r) => return r,
    };
    if let Some(prev) = stored {
        if current != Some(prev.as_str()) {
            return err(
                401,
                "Invalid current password",
                "authentication_error",
                "invalid_password",
            );
        }
    }
    match db(&st, move |s| {
        s.settings_merge(&serde_json::json!({"password": next}))
    })
    .await
    {
        Ok(_) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

async fn enterprise() -> Response {
    err(
        403,
        "Enterprise SSO is not configured",
        "authentication_error",
        "sso_not_configured",
    )
}

// ─── Provider nodes / proxy pools ───────────────────────────────────────────

async fn list_nodes(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.list_nodes()).await {
        Ok(v) => ok(serde_json::json!({"nodes": v})),
        Err(r) => r,
    }
}

async fn create_node(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    if !body.is_object() {
        return bad("Invalid node body");
    }
    match db(&st, move |s| s.create_node(body)).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(r) => r,
    }
}

async fn update_node(
    State(st): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match db(&st, move |s| s.update_node(&id, &body)).await {
        Ok(Some(v)) => ok(v),
        Ok(None) => err(404, "Node not found", "not_found_error", "node_not_found"),
        Err(r) => r,
    }
}

async fn delete_node(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.delete_node(&id)).await {
        Ok(n) if n > 0 => ok(serde_json::json!({"success": true})),
        Ok(_) => err(404, "Node not found", "not_found_error", "node_not_found"),
        Err(r) => r,
    }
}

async fn validate_node(Json(body): Json<Value>) -> Response {
    if body
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.is_empty())
        .unwrap_or(true)
        && body
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
    {
        return bad("name or type is required");
    }
    ok(serde_json::json!({"ok": true}))
}

async fn list_pools(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.list_pools()).await {
        Ok(v) => ok(serde_json::json!({"pools": v})),
        Err(r) => r,
    }
}

async fn create_pool(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return bad("Name is required");
    }
    let url = body
        .get("proxyUrl")
        .or_else(|| body.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return bad("Proxy URL is required");
    }
    if reqwest::Url::parse(&url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https" | "socks5" | "socks5h"))
        .is_none()
    {
        return bad("Invalid proxy URL");
    }
    match db(&st, move |s| s.create_pool(body)).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(r) => r,
    }
}

async fn get_pool(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.get_pool(&id)).await {
        Ok(Some(v)) => ok(v),
        Ok(None) => err(404, "Pool not found", "not_found_error", "pool_not_found"),
        Err(r) => r,
    }
}

async fn update_pool(
    State(st): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match db(&st, move |s| s.update_pool(&id, &body)).await {
        Ok(Some(v)) => ok(v),
        Ok(None) => err(404, "Pool not found", "not_found_error", "pool_not_found"),
        Err(r) => r,
    }
}

async fn delete_pool(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    match db(&st, move |s| s.delete_pool(&id)).await {
        Ok(n) if n > 0 => ok(serde_json::json!({"success": true})),
        Ok(_) => err(404, "Pool not found", "not_found_error", "pool_not_found"),
        Err(r) => r,
    }
}

async fn test_pool(State(st): State<Shared>, Path(id): Path<String>) -> Response {
    let pool = match db(&st, move |s| s.get_pool(&id)).await {
        Ok(Some(v)) => v,
        Ok(None) => return err(404, "Pool not found", "not_found_error", "pool_not_found"),
        Err(r) => return r,
    };
    let url = pool
        .get("proxyUrl")
        .or_else(|| pool.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match proxy_reachable(&url, Duration::from_secs(10)).await {
        Ok(ms) => ok(serde_json::json!({"ok": true, "latencyMs": ms})),
        Err(e) => ok(serde_json::json!({"ok": false, "error": e})),
    }
}

async fn deploy_unavailable() -> Response {
    unavailable("proxy-pool cloud deploy (requires external account)")
}

// ─── Translator ─────────────────────────────────────────────────────────────

static CONSOLE_LOGS: OnceLock<Mutex<Vec<Value>>> = OnceLock::new();

fn console_buf() -> &'static Mutex<Vec<Value>> {
    CONSOLE_LOGS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn console_push(entry: Value) {
    if let Ok(mut g) = console_buf().lock() {
        g.push(entry);
        if g.len() > 200 {
            let excess = g.len() - 200;
            g.drain(..excess);
        }
    }
}

const TRANSLATOR_FILES: &[&str] = &[
    "1_req_client.json",
    "2_req_source.json",
    "3_req_openai.json",
    "4_req_target.json",
    "5_res_provider.txt",
    "6_res_openai.txt",
    "7_res_client.txt",
];

async fn translator_send() -> Response {
    unavailable("translator execution (requires provider executor)")
}

async fn translator_translate(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let (Some(step), Some(payload)) = (
        body.get("step").and_then(|v| v.as_u64()),
        body.get("body").cloned(),
    ) else {
        return ok(serde_json::json!({"success": false, "error": "Step and body required"}));
    };
    if step != 1 {
        return unavailable("translator steps 2+ (requires translator engine)");
    }
    let client_body = payload.get("body").unwrap_or(&payload);
    let raw_model = client_body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let aliases = db(&st, |s| s.get_model_aliases()).await.unwrap_or_default();
    let resolved = nine_routing::resolve_alias_chain(&raw_model, &aliases);
    let (prefix, bare) = nine_core::split_provider_model(&resolved);
    let provider = prefix
        .unwrap_or_else(|| nine_providers::infer_provider(bare))
        .to_string();
    let source = detect_format(client_body);
    let target = match provider.as_str() {
        p if p.contains("anthropic") => "anthropic",
        p if p.contains("gemini") => "gemini",
        _ => "openai",
    };
    console_push(serde_json::json!({"kind": "translate", "model": raw_model}));
    ok(serde_json::json!({"success": true, "result": {
        "provider": provider, "model": bare, "sourceFormat": source, "targetFormat": target,
    }}))
}

fn detect_format(v: &Value) -> &'static str {
    if v.get("anthropic_version").is_some() {
        return "anthropic";
    }
    if let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) {
        let blocks = msgs
            .iter()
            .any(|m| m.get("content").map(|c| c.is_array()).unwrap_or(false));
        if blocks {
            return "anthropic";
        }
    }
    if v.get("messages").is_some() || v.get("prompt").is_some() {
        return "openai";
    }
    "unknown"
}

fn translator_dir(st: &AppState) -> String {
    format!("{}/translator", st.data_dir)
}

async fn translator_load(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let file = q.get("file").map(|s| s.as_str()).unwrap_or("");
    if !TRANSLATOR_FILES.contains(&file) {
        return ok(serde_json::json!({"success": false, "error": "File parameter required"}));
    }
    let path = format!("{}/{}", translator_dir(&st), file);
    match std::fs::read_to_string(&path) {
        Ok(content) => ok(serde_json::json!({"success": true, "file": file, "content": content})),
        Err(_) => ok(serde_json::json!({"success": false, "error": "file not found"})),
    }
}

async fn translator_save(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let (Some(file), Some(content)) = (
        body.get("file").and_then(|v| v.as_str()),
        body.get("content").and_then(|v| v.as_str()),
    ) else {
        return ok(serde_json::json!({"success": false, "error": "File and content required"}));
    };
    if !TRANSLATOR_FILES.contains(&file) {
        return ok(serde_json::json!({"success": false, "error": "file not allowed"}));
    }
    let dir = translator_dir(&st);
    let _ = std::fs::create_dir_all(&dir);
    // Defense: fixed allowlist + no separators can escape `dir`.
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return ok(serde_json::json!({"success": false, "error": "file not allowed"}));
    }
    match std::fs::write(format!("{dir}/{file}"), content) {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(e) => ok(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

async fn console_logs() -> Response {
    let logs = console_buf().lock().map(|g| g.clone()).unwrap_or_default();
    ok(serde_json::json!({"success": true, "logs": logs}))
}

async fn clear_console_logs() -> Response {
    if let Ok(mut g) = console_buf().lock() {
        g.clear();
    }
    ok(serde_json::json!({"success": true}))
}

async fn console_log_stream() -> Response {
    let logs = console_buf().lock().map(|g| g.clone()).unwrap_or_default();
    let body = logs
        .into_iter()
        .map(|l| format!("data: {l}\n\n"))
        .collect::<String>();
    (
        StatusCode::OK,
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        body,
    )
        .into_response()
}

// ─── MCP ────────────────────────────────────────────────────────────────────

async fn mcp_message(Path(plugin): Path<String>) -> Response {
    let _ = plugin;
    unavailable("MCP servers (none configured)")
}

async fn mcp_sse(Path(plugin): Path<String>) -> Response {
    let _ = plugin;
    unavailable("MCP servers (none configured)")
}

// ─── External daemons ───────────────────────────────────────────────────────

async fn headroom_status(State(st): State<Shared>) -> Response {
    let v = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v,
        Err(_) => Value::Null,
    };
    ok(serde_json::json!({
        "running": false, "managed": false, "managedPid": Value::Null,
        "url": v.get("headroomUrl").and_then(|u| u.as_str()).unwrap_or("http://127.0.0.1:11918"),
    }))
}

async fn headroom_extras(State(st): State<Shared>) -> Response {
    match db(&st, |s| s.kv_list("headroomExtras")).await {
        Ok(m) => {
            let mut all = serde_json::Map::new();
            for (k, v) in &m {
                all.insert(k.clone(), serde_json::from_str(v).unwrap_or(Value::Null));
            }
            ok(serde_json::json!({"extras": all}))
        }
        Err(r) => r,
    }
}

async fn headroom_extras_set(State(st): State<Shared>, Json(body): Json<Value>) -> Response {
    let pairs: Vec<(String, Value)> = match body.as_object() {
        Some(o) => o.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        None => return bad("Invalid extras body"),
    };
    match db(&st, move |s| {
        for (k, v) in &pairs {
            s.kv_set("headroomExtras", k, &serde_json::to_string(v)?)?;
        }
        Ok(())
    })
    .await
    {
        Ok(()) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

async fn headroom_extras_del(
    State(st): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(key) = q.get("key").cloned().filter(|k| !k.is_empty()) else {
        return bad("key is required");
    };
    match db(&st, move |s| s.kv_delete("headroomExtras", &key)).await {
        Ok(_) => ok(serde_json::json!({"success": true})),
        Err(r) => r,
    }
}

async fn daemon_unmanaged() -> Response {
    unavailable("external daemon control (not managed by this build)")
}

async fn pxpipe_status(State(st): State<Shared>) -> Response {
    let v = match db(&st, |s| s.settings_get()).await {
        Ok(v) => v,
        Err(_) => Value::Null,
    };
    ok(serde_json::json!({
        "running": false, "installed": false,
        "enabled": v.get("pxpipeEnabled").and_then(|b| b.as_bool()).unwrap_or(false),
        "autoInstall": v.get("pxpipeAutoInstall").and_then(|b| b.as_bool()).unwrap_or(false),
        "minChars": v.get("pxpipeMinChars"),
        "timeoutMs": v.get("pxpipeTimeoutMs"),
    }))
}

async fn pxpipe_logs() -> Response {
    ok(serde_json::json!({"logs": []}))
}

async fn tunnel_status() -> Response {
    ok(serde_json::json!({
        "tunnel": {"enabled": false, "url": Value::Null},
        "tailscale": {"enabled": false, "url": Value::Null},
        "download": Value::Null,
    }))
}

async fn tailscale_check() -> Response {
    ok(serde_json::json!({"installed": false, "enabled": false}))
}

// ─── Version ────────────────────────────────────────────────────────────────

async fn version_shutdown() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"success": false, "message": "Not allowed in production"})),
    )
        .into_response()
}

async fn version_update(State(st): State<Shared>) -> Response {
    ok(serde_json::json!({
        "currentVersion": st.version,
        "latestVersion": st.version,
        "hasUpdate": false,
    }))
}
