use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use super::{err, AppState};
use nine_oauth::device::{self, DeviceOptions};

// ─── Proxy sessions (loopback callback state) ────────────────────────────

#[derive(Debug, Clone)]
pub struct ProxySession {
    pub status: String,
    pub code: Option<String>,
    pub verifier: Option<String>,
    pub redirect_uri: Option<String>,
    pub error: Option<String>,
}

fn sessions() -> &'static Mutex<HashMap<String, ProxySession>> {
    static S: OnceLock<Mutex<HashMap<String, ProxySession>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn servers() -> &'static Mutex<HashMap<String, tokio::task::AbortHandle>> {
    static S: OnceLock<Mutex<HashMap<String, tokio::task::AbortHandle>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_session(provider: &str, state: &str, verifier: Option<String>) -> bool {
    if !matches!(
        provider,
        "trae" | "windsurf" | "zed" | "xai" | "codex" | "xiaomi-mimo"
    ) {
        return false;
    }
    sessions().lock().unwrap().insert(
        session_key(provider, state),
        ProxySession {
            status: "pending".into(),
            code: None,
            verifier,
            redirect_uri: None,
            error: None,
        },
    );
    true
}

fn session_key(provider: &str, state: &str) -> String {
    format!("{provider}:{state}")
}

pub fn session_status(provider: &str, state: &str) -> Option<ProxySession> {
    sessions()
        .lock()
        .unwrap()
        .get(&session_key(provider, state))
        .cloned()
}

fn complete_session(provider: &str, state: &str, code: Option<String>, error: Option<String>) {
    let mut m = sessions().lock().unwrap();
    if let Some(s) = m.get_mut(&session_key(provider, state)) {
        if let Some(c) = code {
            s.code = Some(c);
            s.status = "done".into();
        } else if let Some(e) = error {
            s.error = Some(e);
            s.status = "error".into();
        }
    } else {
        m.insert(
            session_key(provider, state),
            ProxySession {
                status: if code.is_some() {
                    "done".into()
                } else {
                    "error".into()
                },
                code,
                verifier: None,
                redirect_uri: None,
                error,
            },
        );
    }
}

fn clear_session(provider: &str, state: &str) {
    sessions()
        .lock()
        .unwrap()
        .remove(&session_key(provider, state));
}

fn is_loopback_origin(origin: Option<&str>) -> bool {
    // ponytail: full CSRF model (host allowlist, Referer check) can extend
    // this when callback proxies gain non-loopback deployments.
    match origin {
        None => true,
        Some(o) => {
            o.starts_with("http://127.0.0.1")
                || o.starts_with("http://localhost")
                || o.starts_with("http://[::1]")
        }
    }
}

pub const SUCCESS_HTML: &str = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Authentication Successful</title></head><body style=\"font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh\"><div><h1>Authentication Successful</h1><p>You can close this tab.</p><script>setTimeout(()=>window.close(),3000)</script></div></body></html>";

fn fixed_port(provider: &str) -> Option<u16> {
    match provider {
        "codex" => Some(1455),
        "xai" => Some(56121),
        _ => None,
    }
}

fn callback_paths() -> Vec<&'static str> {
    vec!["/callback", "/auth/callback"]
}

/// Start a loopback callback proxy. Binds the fixed port for codex/xai
/// (upstream server.js) or an ephemeral port otherwise; captures the next
/// callback per state and serves the upstream success page.
pub async fn start_proxy(provider: &str) -> Value {
    if !matches!(
        provider,
        "codex" | "xai" | "trae" | "windsurf" | "zed" | "xiaomi-mimo"
    ) {
        return serde_json::json!({"error": "Proxy only supported for codex/xai/trae/windsurf/zed/xiaomi-mimo"});
    }
    if servers().lock().unwrap().contains_key(provider) {
        let port = proxy_port(provider).unwrap_or(0);
        return serde_json::json!({"success": true, "port": port, "callbackUrl": format!("http://127.0.0.1:{port}/auth/callback")});
    }
    let prov = provider.to_string();
    let listener = match fixed_port(provider) {
        Some(p) => tokio::net::TcpListener::bind(format!("127.0.0.1:{p}")).await,
        None => tokio::net::TcpListener::bind("127.0.0.1:0").await,
    };
    let listener = match listener {
        Ok(l) => l,
        Err(e) => return serde_json::json!({"success": false, "reason": e.to_string()}),
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    {
        let mut m = sessions().lock().unwrap();
        m.insert(
            format!("{prov}:__port"),
            ProxySession {
                status: "running".into(),
                code: None,
                verifier: None,
                redirect_uri: Some(port.to_string()),
                error: None,
            },
        );
    }
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let prov = prov.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                let mut reader = tokio::io::BufReader::new(&mut sock);
                // Request line + headers (cap 64 lines; fragments rejected).
                let mut origin: Option<String> = None;
                let mut target = String::new();
                for i in 0..64 {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end().to_string();
                    if i == 0 {
                        target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    } else if line.to_lowercase().starts_with("origin:") {
                        origin = line.split_once(':').map(|(_, v)| v.trim().to_string());
                    }
                    if line.is_empty() {
                        break;
                    }
                }
                let w = reader.get_mut();
                if !is_loopback_origin(origin.as_deref()) {
                    let _ = w
                        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 8\r\n\r\nrejected")
                        .await;
                    return;
                }
                let (path, query) = match target.split_once('?') {
                    Some((p, q)) => (p.to_string(), q.to_string()),
                    None => (target.clone(), String::new()),
                };
                if !callback_paths().contains(&path.as_str()) {
                    let _ = w
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot found")
                        .await;
                    return;
                }
                let mut code = None;
                let mut state = None;
                let mut error = None;
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        match k {
                            "code" => code = Some(v.to_string()),
                            "state" => state = Some(v.to_string()),
                            "error" => error = Some(v.to_string()),
                            _ => {}
                        }
                    }
                }
                if let Some(s) = state.clone() {
                    complete_session(&prov, &s, code.clone(), error.clone());
                }
                let body = SUCCESS_HTML.as_bytes();
                let _ = w
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
                let _ = w.write_all(body).await;
            });
        }
    });
    servers()
        .lock()
        .unwrap()
        .insert(provider.into(), handle.abort_handle());
    serde_json::json!({"success": true, "port": port, "callbackUrl": format!("http://127.0.0.1:{port}/auth/callback")})
}

fn proxy_port(provider: &str) -> Option<u16> {
    sessions()
        .lock()
        .unwrap()
        .get(&format!("{provider}:__port"))
        .and_then(|s| s.redirect_uri.as_deref()?.parse().ok())
}

pub fn stop_proxy(provider: &str) -> bool {
    if let Some(h) = servers().lock().unwrap().remove(provider) {
        h.abort();
    }
    sessions()
        .lock()
        .unwrap()
        .retain(|k, _| !k.starts_with(&format!("{provider}:")));
    true
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────

async fn send_device(client: &reqwest::Client, req: &device::DeviceHttpRequest) -> (u16, Value) {
    let mut builder = match req.method {
        "GET" => client.get(&req.url),
        _ => client.post(&req.url).body(req.body.clone()),
    };
    builder = builder.header("content-type", req.content_type);
    for (k, v) in &req.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => return (502, serde_json::json!({"error": e.to_string()})),
    };
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

// ─── GET /api/oauth/:provider/:action ─────────────────────────────────────

pub async fn oauth_get_action(
    State(st): State<Arc<AppState>>,
    Path((provider, action)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let _ = &headers;
    match action.as_str() {
        "authorize" => authorize_action(st, provider, query).await,
        "device-code" => device_code_action(st, provider, query).await,
        "poll-status" => {
            let state = query.get("state").cloned().unwrap_or_default();
            if state.is_empty() {
                return err(
                    400,
                    "Missing state",
                    "invalid_request_error",
                    "missing_state",
                );
            }
            match session_status(&provider, &state) {
                None => Json(serde_json::json!({"status": "unknown"})).into_response(),
                Some(s) => {
                    if s.status == "done" || s.status == "error" {
                        let payload = serde_json::json!({"status": s.status, "code": s.code, "error": s.error});
                        if provider != "xiaomi-mimo" {
                            clear_session(&provider, &state);
                        } else if s.status == "error" {
                            clear_session(&provider, &state);
                            stop_proxy(&provider);
                        }
                        Json(payload).into_response()
                    } else {
                        Json(serde_json::json!({"status": s.status})).into_response()
                    }
                }
            }
        }
        "start-proxy" => {
            let v = start_proxy(&provider).await;
            if v.get("error").is_some() {
                return err(
                    400,
                    v["error"].as_str().unwrap_or("proxy unsupported"),
                    "invalid_request_error",
                    "proxy_unsupported",
                );
            }
            Json(v).into_response()
        }
        "stop-proxy" => {
            if !matches!(
                provider.as_str(),
                "codex" | "xai" | "trae" | "windsurf" | "zed" | "xiaomi-mimo"
            ) {
                return err(
                    400,
                    "Proxy only supported for codex/xai/trae/windsurf/zed/xiaomi-mimo",
                    "invalid_request_error",
                    "proxy_unsupported",
                );
            }
            stop_proxy(&provider);
            Json(serde_json::json!({"success": true})).into_response()
        }
        "ide-status" => {
            if provider != "trae" && provider != "windsurf" {
                return err(
                    400,
                    "ide-status only supported for trae/windsurf",
                    "invalid_request_error",
                    "ide_unsupported",
                );
            }
            let bin = if provider == "trae" {
                "trae"
            } else {
                "windsurf"
            };
            let installed = std::env::var("PATH")
                .map(|p| {
                    p.split(':')
                        .any(|d| std::path::Path::new(d).join(bin).exists())
                })
                .unwrap_or(false);
            Json(serde_json::json!({"provider": provider, "installed": installed})).into_response()
        }
        _ => err(
            400,
            &format!("Unknown action: {action}"),
            "invalid_request_error",
            "unknown_action",
        ),
    }
}

async fn authorize_action(
    st: Arc<AppState>,
    provider: String,
    query: HashMap<String, String>,
) -> Response {
    if provider == "xiaomi-mimo" {
        return err(
            501,
            "xiaomi-mimo uses X25519+AES-GCM callback encryption; import a token instead",
            "not_implemented",
            "custom_token_flow",
        );
    }
    let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &provider) else {
        return err(
            404,
            "unknown provider",
            "not_found_error",
            "unknown_provider",
        );
    };
    if spec.authorize_url.is_empty() {
        return err(
            501,
            "provider uses a custom device flow; use device-code",
            "not_implemented",
            "custom_token_flow",
        );
    }
    if let Some(reason) = nine_oauth::start_unsupported_reason(&provider) {
        return err(501, reason, "not_implemented", "custom_token_flow");
    }
    let redirect_uri = query
        .get("redirect_uri")
        .cloned()
        .unwrap_or_else(|| format!("{}{}", st.callback_base, spec.callback_path));
    let pkce = if spec.pkce {
        Some(nine_oauth::Pkce::generate())
    } else {
        None
    };
    let state = query
        .get("state")
        .cloned()
        .unwrap_or_else(nine_oauth::new_state);
    let url = spec.authorize_url(&state, pkce.as_ref(), &redirect_uri);
    let entry = serde_json::json!({
        "state": state,
        "codeChallenge": pkce.as_ref().map(|p| p.challenge.clone()),
        "codeVerifier": pkce.as_ref().map(|p| p.verifier.clone()),
        "provider": provider,
        "redirectUri": redirect_uri,
    });
    if let Some(store) = &st.store {
        let store = store.clone();
        let key = state.clone();
        let raw = entry.to_string();
        let _ = tokio::task::spawn_blocking(move || store.kv_set("oauth_state", &key, &raw)).await;
    }
    // ponytail: provider meta params (gitlab baseUrl/clientId) pass through
    // but are not yet applied to the stored exchange; add when gitlab
    // self-hosted login is covered.
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "state": state,
            "authUrl": url,
            "authorizeUrl": url,
            "redirectUri": redirect_uri,
            "codeVerifier": pkce.as_ref().map(|p| p.verifier.clone()),
            "codeChallenge": pkce.as_ref().map(|p| p.challenge.clone()),
            "flowType": "authorization_code",
            "fixedPort": spec.fixed_port,
            "callbackPath": spec.callback_path,
        })),
    )
        .into_response()
}

async fn device_code_action(
    st: Arc<AppState>,
    provider: String,
    query: HashMap<String, String>,
) -> Response {
    let provider = if provider == "kimi-coding" {
        "kimi".into()
    } else {
        provider
    };
    if !device::supports_device(&provider) {
        return err(
            400,
            "Provider does not support device code flow",
            "invalid_request_error",
            "device_unsupported",
        );
    }
    if provider == "qoder" {
        let nonce = uuid::Uuid::new_v4().to_string();
        let machine_id = uuid::Uuid::new_v4().to_string();
        let verifier = nine_oauth::Pkce::generate().verifier;
        let challenge = device::qoder_challenge(&verifier);
        let url = device::qoder_verification_url(&challenge, &nonce, &machine_id);
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "device_code": nonce,
                "user_code": "",
                "verification_uri": url,
                "verification_uri_complete": url,
                "expires_in": 600,
                "interval": 2,
                "codeVerifier": verifier,
                "extraData": {"_qoderNonce": nonce, "_qoderVerifier": verifier, "_qoderMachineId": machine_id},
            })),
        )
            .into_response();
    }
    if provider == "kiro" {
        let mut optmap = BTreeMap::new();
        for (k, v) in &query {
            optmap.insert(k.clone(), v.clone());
        }
        let opts = DeviceOptions::from_map(&optmap);
        // Step 1: client registration (SSO OIDC), step 2: device authorization.
        let (s1, reg) = send_device(&st.client, &device::kiro_register_request(&opts)).await;
        if !(200..300).contains(&s1) {
            return err(
                502,
                "Device authorization failed",
                "upstream_error",
                "device_code_failed",
            );
        }
        let cid = reg.get("clientId").and_then(|v| v.as_str()).unwrap_or("");
        let csec = reg
            .get("clientSecret")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if cid.is_empty() {
            return err(
                502,
                "Device authorization failed",
                "upstream_error",
                "device_code_failed",
            );
        }
        let (s2, auth) = send_device(
            &st.client,
            &device::kiro_device_auth_request(cid, csec, &opts),
        )
        .await;
        if !(200..300).contains(&s2) {
            return err(
                502,
                "Device authorization failed",
                "upstream_error",
                "device_code_failed",
            );
        }
        let mut v = device::normalize_device_response("kiro", &auth).unwrap_or(auth);
        v["_clientId"] = Value::String(cid.into());
        v["_clientSecret"] = Value::String(csec.into());
        v["_region"] = Value::String(device::kiro_region_of(&opts));
        let region = v["_region"].as_str().unwrap_or("").to_string();
        let extra = serde_json::json!({"_clientId": cid, "_clientSecret": csec, "_region": region});
        v["extraData"] = extra;
        return (StatusCode::OK, Json(v)).into_response();
    }
    let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, &provider) else {
        return err(
            404,
            "unknown provider",
            "not_found_error",
            "unknown_provider",
        );
    };
    let opts = DeviceOptions {
        kimi_device_id: Some(uuid::Uuid::new_v4().to_string()),
        ..Default::default()
    };
    let Some(req) =
        device::device_code_request(&provider, &spec.client_id, &spec.scopes.join(" "), &opts)
    else {
        return err(
            500,
            "device flow not configured",
            "upstream_error",
            "device_code_failed",
        );
    };
    let (status, raw) = send_device(&st.client, &req).await;
    if !(200..300).contains(&status) {
        return err(
            502,
            "Device code request failed",
            "upstream_error",
            "device_code_failed",
        );
    }
    let Some(mut v) = device::normalize_device_response(&provider, &raw) else {
        return err(
            502,
            "Device code request failed",
            "upstream_error",
            "device_code_failed",
        );
    };
    // Preserve kimi device id + any provider extras for the poll step.
    let mut extra = opts.extra_map();
    for (k, val) in raw.as_object().cloned().unwrap_or_default() {
        if k.starts_with('_') {
            extra.insert(k, val.as_str().unwrap_or("").into());
        }
    }
    if provider == "kimi" {
        if let Some(h) = req.headers.iter().find(|(k, _)| k == "X-Msh-Device-Id") {
            extra.insert("_kimiDeviceId".into(), h.1.clone());
        }
    }
    v["extraData"] = serde_json::json!(extra);
    (StatusCode::OK, Json(v)).into_response()
}

// ─── POST device poll / register-session / manual-code ───────────────────

#[derive(serde::Deserialize, Default)]
pub struct PollBody {
    #[serde(rename = "deviceCode")]
    pub device_code: Option<String>,
    #[serde(rename = "device_code")]
    pub device_code_snake: Option<String>,
    #[serde(rename = "codeVerifier")]
    pub code_verifier: Option<String>,
    #[serde(rename = "code_verifier")]
    pub code_verifier_snake: Option<String>,
    #[serde(rename = "extraData")]
    pub extra_data: Option<BTreeMap<String, String>>,
    pub state: Option<String>,
    pub region: Option<String>,
    #[serde(rename = "startUrl")]
    pub start_url: Option<String>,
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,
}

impl PollBody {
    pub fn code(&self) -> String {
        self.device_code
            .clone()
            .or(self.device_code_snake.clone())
            .unwrap_or_default()
    }
    pub fn verifier(&self) -> Option<String> {
        self.code_verifier
            .clone()
            .or(self.code_verifier_snake.clone())
    }
    pub fn opts(&self) -> DeviceOptions {
        let mut m = self.extra_data.clone().unwrap_or_default();
        if let Some(s) = &self.state {
            m.insert("proxyState".into(), s.clone());
        }
        if let Some(r) = &self.region {
            m.insert("region".into(), r.clone());
        }
        if let Some(s) = &self.start_url {
            m.insert("startUrl".into(), s.clone());
        }
        if let Some(a) = &self.auth_method {
            m.insert("authMethod".into(), a.clone());
        }
        if let Some(v) = self.verifier() {
            m.entry("codeVerifier".into()).or_insert(v);
        }
        let mut o = DeviceOptions::from_map(&m);
        if o.qoder_verifier.is_none() {
            o.qoder_verifier = self.verifier();
        }
        o
    }
}

pub async fn poll_action(
    State(st): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Json(body): Json<PollBody>,
) -> Response {
    let provider = if provider == "kimi-coding" {
        "kimi".into()
    } else {
        provider
    };
    if !device::supports_device(&provider) {
        return err(
            400,
            "Provider does not support device code flow",
            "invalid_request_error",
            "device_unsupported",
        );
    }
    let code = body.code();
    if code.is_empty() {
        return err(
            400,
            "Missing deviceCode",
            "invalid_request_error",
            "missing_device_code",
        );
    }
    let opts = body.opts();
    let spec = nine_oauth::spec_for(&st.oauth_specs, &provider).cloned();
    let (client_id, scope) = spec
        .as_ref()
        .map(|s| (s.client_id.clone(), s.scopes.join(" ")))
        .unwrap_or_default();
    let Some(req) = device::device_poll_request(&provider, &client_id, &code, &opts) else {
        return err(
            500,
            "device poll not configured",
            "upstream_error",
            "poll_failed",
        );
    };
    let (status, raw) = send_device(&st.client, &req).await;
    // Kilocode approved responses carry `token` (not access_token); map first.
    let raw = if provider == "kilocode" && raw.get("access_token").is_none() {
        if let Some(t) = raw
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        {
            serde_json::json!({"access_token": t, "_userEmail": raw.get("userEmail")})
        } else {
            raw
        }
    } else {
        raw
    };
    match device::normalize_poll(&provider, status, &raw) {
        device::PollOutcome::Pending => Json(serde_json::json!({"ok": false, "success": false, "pending": true, "error": "authorization_pending"})).into_response(),
        device::PollOutcome::SlowDown => Json(serde_json::json!({"ok": false, "success": false, "pending": true, "error": "slow_down"})).into_response(),
        device::PollOutcome::Expired => Json(serde_json::json!({"ok": false, "success": false, "error": "expired_token"})).into_response(),
        device::PollOutcome::Denied => Json(serde_json::json!({"ok": false, "success": false, "error": "access_denied"})).into_response(),
        device::PollOutcome::Failed(e) => Json(serde_json::json!({"ok": false, "success": false, "error": e})).into_response(),
        device::PollOutcome::Complete(tokens) => {
            let now = chrono::Utc::now().timestamp();
            let parsed = nine_oauth::parse_token_response(&provider, &provider, &tokens, now);
            let conn = nine_storage::ProviderConnection {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider.clone(),
                auth_type: "oauth".into(),
                name: Some("account".into()),
                email: None,
                priority: None,
                is_active: true,
                data: serde_json::json!({
                    "access_token": parsed.access_token,
                    "refresh_token": parsed.refresh_token,
                    "expires_at": parsed.expires_at,
                }),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Some(store) = &st.store {
                let store = store.clone();
                let c = conn.clone();
                let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&c)).await;
            }
            let _ = &scope;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true, "success": true,
                    "connection": {"id": conn.id, "provider": conn.provider, "authType": "oauth"},
                })),
            )
                .into_response()
        }
    }
}

#[derive(serde::Deserialize, Default)]
pub struct RegisterBody {
    pub state: Option<String>,
    #[serde(rename = "codeVerifier")]
    pub code_verifier: Option<String>,
}

pub async fn register_session_action(
    Path(provider): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<RegisterBody>,
) -> Response {
    let state = query
        .get("state")
        .cloned()
        .or(body.state)
        .unwrap_or_default();
    if state.is_empty() {
        return err(
            400,
            "Missing state",
            "invalid_request_error",
            "missing_state",
        );
    }
    if !register_session(&provider, &state, body.code_verifier) {
        return err(
            400,
            "register-session only supported for trae/windsurf/zed",
            "invalid_request_error",
            "register_unsupported",
        );
    }
    // xai sessions also accept registration (upstream xai proxy flow).
    Json(serde_json::json!({"success": true})).into_response()
}

#[derive(serde::Deserialize, Default)]
pub struct ManualCodeBody {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn manual_code_action(
    State(st): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Json(body): Json<ManualCodeBody>,
) -> Response {
    if provider != "xai" {
        return err(
            400,
            "manual-code only supported for xai",
            "invalid_request_error",
            "manual_unsupported",
        );
    }
    let code = body.code.unwrap_or_default();
    let state = body.state.unwrap_or_default();
    if code.is_empty() || state.is_empty() {
        return err(
            400,
            "Missing code or state",
            "invalid_request_error",
            "missing_code",
        );
    }
    let session = session_status("xai", &state);
    let verifier = session
        .as_ref()
        .and_then(|s| s.verifier.clone())
        .unwrap_or_default();
    let redirect = session
        .as_ref()
        .and_then(|s| s.redirect_uri.clone())
        .unwrap_or_else(|| format!("{}{}", st.callback_base, "/auth/callback"));
    let Some(spec) = nine_oauth::spec_for(&st.oauth_specs, "xai").cloned() else {
        return err(
            404,
            "unknown provider",
            "not_found_error",
            "unknown_provider",
        );
    };
    let pkce = if verifier.is_empty() {
        None
    } else {
        Some(nine_oauth::Pkce::from_verifier(&verifier))
    };
    let req_body = nine_oauth::form_encode(&spec.exchange_body(&code, &redirect, pkce.as_ref()));
    let resp = st
        .client
        .post(&spec.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(req_body)
        .send()
        .await;
    let resp = match resp {
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
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        clear_session("xai", &state);
        stop_proxy("xai");
        return err(
            status.as_u16(),
            "token exchange failed",
            "invalid_request_error",
            "token_exchange_failed",
        );
    }
    let token_json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let now = chrono::Utc::now().timestamp();
    let parsed = nine_oauth::parse_token_response("xai", "xai", &token_json, now);
    let conn = nine_storage::ProviderConnection {
        id: uuid::Uuid::new_v4().to_string(),
        provider: "xai".into(),
        auth_type: "oauth".into(),
        name: Some("account".into()),
        email: None,
        priority: None,
        is_active: true,
        data: serde_json::json!({
            "access_token": parsed.access_token,
            "refresh_token": parsed.refresh_token,
            "expires_at": parsed.expires_at,
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Some(store) = &st.store {
        let store = store.clone();
        let c = conn.clone();
        let _ = tokio::task::spawn_blocking(move || store.upsert_connection(&c)).await;
    }
    clear_session("xai", &state);
    stop_proxy("xai");
    (
        StatusCode::OK,
        Json(
            serde_json::json!({"success": true, "connection": {"id": conn.id, "provider": "xai"}}),
        ),
    )
        .into_response()
}

pub fn browser_callback_html() -> Html<&'static str> {
    Html(SUCCESS_HTML)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{router_with_state, AppState};
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;

    async fn body_json(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
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

    fn app() -> Router {
        router_with_state(
            AppState::new(Vec::new(), 5000)
                .with_oauth_specs(nine_oauth::load_specs("/nonexistent-dir-xyz")),
        )
    }

    #[tokio::test]
    async fn authorize_shape_matches_upstream() {
        let (s, v) = body_json(
            app(),
            Request::get("/api/oauth/claude/authorize?redirect_uri=http://127.0.0.1:8080/callback")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(v["authorizeUrl"]
            .as_str()
            .unwrap()
            .contains("response_type=code"));
        assert_eq!(v["authUrl"], v["authorizeUrl"]);
        assert!(!v["state"].as_str().unwrap_or("").is_empty());
        assert!(v["codeVerifier"].as_str().unwrap_or("").len() >= 43);
    }

    #[tokio::test]
    async fn device_code_rejects_non_device_provider() {
        let (s, v) = body_json(
            app(),
            Request::get("/api/oauth/claude/device-code")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("device code"));
    }

    #[tokio::test]
    async fn qoder_device_flow_is_local() {
        let (s, v) = body_json(
            app(),
            Request::get("/api/oauth/qoder/device-code")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(v["verification_uri_complete"]
            .as_str()
            .unwrap()
            .contains("challenge="));
        assert!(!v["codeVerifier"].as_str().unwrap_or("").is_empty());
        assert!(!v["device_code"].as_str().unwrap_or("").is_empty());
        // Qoder poll against a mock 202 stays pending.
        let qmock = mock_server(202, r#"{}"#).await;
        std::env::set_var("NINE_DEVICE_MOCK_QODER_TOKEN", format!("{qmock}/poll"));
        let (s2, v2) = body_json(
            app(),
            Request::post("/api/oauth/qoder/poll")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"deviceCode":"{}","codeVerifier":"{}"}}"#,
                    v["device_code"].as_str().unwrap(),
                    v["codeVerifier"].as_str().unwrap()
                )))
                .unwrap(),
        )
        .await;
        // Upstream 202/404 while waiting maps to pending (upstream qoder.js).
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(v2["pending"], true);
        std::env::remove_var("NINE_DEVICE_MOCK_QODER_TOKEN");
    }

    /// Minimal canned HTTP mock: responds with fixed status+body to every request.
    async fn mock_server(status: u16, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                use tokio::io::AsyncWriteExt;
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn github_device_pending_then_success() {
        let base = mock_server(200, r#"{"device_code":"dc","user_code":"UC","verification_uri":"https://x/d","expires_in":600,"interval":5}"#).await;
        std::env::set_var("NINE_DEVICE_MOCK_GITHUB_CODE", format!("{base}/code"));
        let (s, v) = body_json(
            app(),
            Request::get("/api/oauth/github/device-code")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["device_code"], "dc");
        assert_eq!(v["user_code"], "UC");

        let pend = mock_server(200, r#"{"error":"authorization_pending"}"#).await;
        std::env::set_var("NINE_DEVICE_MOCK_GITHUB_TOKEN", format!("{pend}/token"));
        let (s2, v2) = body_json(
            app(),
            Request::post("/api/oauth/github/poll")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"deviceCode":"dc"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(v2["pending"], true);

        let done = mock_server(200, r#"{"access_token":"tok","expires_in":3600}"#).await;
        std::env::set_var("NINE_DEVICE_MOCK_GITHUB_TOKEN", format!("{done}/token"));
        let (s3, v3) = body_json(
            app(),
            Request::post("/api/oauth/github/poll")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"deviceCode":"dc"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s3, StatusCode::OK);
        assert_eq!(v3["success"], true);
        assert_eq!(v3["connection"]["provider"], "github");
        std::env::remove_var("NINE_DEVICE_MOCK_GITHUB_CODE");
        std::env::remove_var("NINE_DEVICE_MOCK_GITHUB_TOKEN");
    }

    #[tokio::test]
    async fn proxy_session_lifecycle() {
        let (s, v) = body_json(
            app(),
            Request::get("/api/oauth/trae/start-proxy")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["success"], true);
        let port = v["port"].as_u64().unwrap();

        let (s2, v2) = body_json(
            app(),
            Request::post("/api/oauth/trae/register-session?state=st1")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(v2["success"], true);

        let (s3, v3) = body_json(
            app(),
            Request::get("/api/oauth/trae/poll-status?state=st1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s3, StatusCode::OK);
        assert_eq!(v3["status"], "pending");

        // Simulate the browser callback hitting the loopback proxy.
        let url = format!("http://127.0.0.1:{port}/auth/callback?code=c1&state=st1");
        let _ = reqwest::get(&url).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let (s4, v4) = body_json(
            app(),
            Request::get("/api/oauth/trae/poll-status?state=st1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s4, StatusCode::OK);
        assert_eq!(v4["status"], "done");
        assert_eq!(v4["code"], "c1");

        let (s5, _) = body_json(
            app(),
            Request::get("/api/oauth/trae/stop-proxy")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s5, StatusCode::OK);
    }

    /// Captured-request mock: returns 200 + canned JSON, records the request body.
    async fn mock_capture(body: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let seen2 = seen2.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = vec![0u8; 65536];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    *seen2.lock().unwrap() = req;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    #[tokio::test]
    async fn exchange_uses_stored_verifier_and_redirect_uri() {
        let (base, seen) = mock_capture(r#"{"access_token":"tok","expires_in":3600}"#).await;
        std::env::set_var("NINE_CODEX_TOKEN_URL", format!("{base}/token"));
        let store = Arc::new(nine_storage::Store::open_memory().unwrap());
        let app = router_with_state(
            AppState::new(Vec::new(), 5000)
                .with_store(store)
                .with_oauth_specs(nine_oauth::load_specs("/nonexistent-dir-xyz")),
        );
        let redirect = "http://127.0.0.1:1455/auth/callback";
        let (s1, v1) = body_json(
            app.clone(),
            Request::get(format!(
                "/api/oauth/codex/authorize?redirect_uri={redirect}"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(s1, StatusCode::OK);
        let state = v1["state"].as_str().unwrap().to_string();
        // Exchange sends only code+state; verifier/redirect_uri come from /authorize.
        let (s2, v2) = body_json(
            app,
            Request::post("/api/oauth/codex/exchange")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"code":"abc","state":"{state}"}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::OK, "{v2}");
        assert_eq!(v2["connection"]["provider"], "codex");
        let req = seen.lock().unwrap().clone();
        assert!(req.contains("redirect_uri="), "redirect_uri missing: {req}");
        assert!(
            req.contains("code_verifier="),
            "code_verifier missing: {req}"
        );
        assert!(
            req.contains("127.0.0.1%3A1455"),
            "redirect_uri not the loopback one: {req}"
        );
        std::env::remove_var("NINE_CODEX_TOKEN_URL");
    }

    #[tokio::test]
    async fn manual_code_and_ide_status_guards() {
        let (s, _) = body_json(
            app(),
            Request::post("/api/oauth/claude/manual-code")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"code":"c","state":"s"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s2, _) = body_json(
            app(),
            Request::get("/api/oauth/claude/ide-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s2, StatusCode::BAD_REQUEST);
        let (s3, _) = body_json(
            app(),
            Request::post("/api/oauth/zed/register-session")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"state":"s"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(s3, StatusCode::OK);
    }
}
