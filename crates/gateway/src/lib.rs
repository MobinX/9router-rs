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
        }
    }

    pub fn with_catalog(mut self, catalog: Vec<ModelEntry>) -> Self {
        self.catalog = Arc::new(catalog);
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
        .route("/api/oauth/:provider", get(oauth_start))
        .route("/api/models", get(models))
        .route("/api/providers", get(providers))
        .route("/api/usage/stats", get(usage_stats))
        .route("/api/settings", get(settings))
        .route("/api/auth/status", get(auth_status))
        .route("/api/keys", get(keys))
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

async fn providers() -> impl IntoResponse {
    Json(serde_json::json!({
        "providers": nine_providers::PROVIDER_IDS.iter().take(5).collect::<Vec<_>>(),
        "total": nine_providers::PROVIDER_IDS.len()
    }))
}

async fn usage_stats() -> impl IntoResponse {
    Json(serde_json::json!({
        "totalRequests": 0,
        "totalPromptTokens": 0,
        "totalCompletionTokens": 0,
        "totalCachedTokens": 0,
        "totalCost": 0,
        "byProvider": {}
    }))
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

async fn oauth_start(Path(provider): Path<String>) -> impl IntoResponse {
    if !nine_oauth::OAUTH_PROVIDERS.contains(&provider.as_str())
        && !nine_providers::is_known_provider(&provider)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": {"message": "unknown provider"}})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"provider": provider, "authorizeUrl": "https://accounts.example.com/oauth/authorize"})),
    )
        .into_response()
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
    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o")
        .to_string();
    let stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let (prefix, bare) = nine_core::split_provider_model(&model);
    let provider = prefix.unwrap_or_else(|| nine_providers::infer_provider(bare));
    let chat_req = nine_providers::ChatRequest {
        model: bare.to_string(),
        messages: parse_messages(&v),
        stream,
    };
    let candidates = upstream_candidates(&st, provider);
    if candidates.is_empty() {
        return with_id(
            err(
                502,
                "no upstream configured",
                "upstream_error",
                "no_upstream",
            ),
            &req_id,
        );
    }
    let mut last: Option<Response> = None;
    for (cand_provider, base, key) in &candidates {
        let payload = match nine_providers::api_style(cand_provider) {
            ApiStyle::Anthropic => nine_providers::AnthropicAdapter.translate_request(&chat_req),
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
            &model,
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
    with_id(
        last.unwrap_or_else(|| {
            err(
                502,
                "all upstreams failed",
                "upstream_error",
                "upstream_error",
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
