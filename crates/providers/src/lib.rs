use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

/// Upstream wire protocol used by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Providers speaking the Anthropic Messages API.
pub const ANTHROPIC_STYLE: &[&str] = &[
    "anthropic",
    "claude",
    "claude-code",
    "codex-anthropic",
    "cca",
    "oai-cc",
];

/// Providers speaking the Gemini generateContent API.
pub const GEMINI_STYLE: &[&str] = &[
    "gemini",
    "gemini-cli",
    "vertex",
    "vertex-partner",
    "antigravity",
    "ag",
    "nanobanana",
];

pub fn api_style(provider: &str) -> ApiStyle {
    let p = provider.to_lowercase();
    if ANTHROPIC_STYLE.contains(&p.as_str()) {
        ApiStyle::Anthropic
    } else if GEMINI_STYLE.contains(&p.as_str()) {
        ApiStyle::Gemini
    } else {
        ApiStyle::OpenAi
    }
}

/// Default upstream base URL for well-known providers. Empty means "not configured".
pub fn base_url_for(provider: &str) -> &'static str {
    match provider.to_lowercase().as_str() {
        "openai" => "https://api.openai.com/v1",
        "anthropic" | "claude" => "https://api.anthropic.com/v1",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        "deepseek" => "https://api.deepseek.com/v1",
        "groq" => "https://api.groq.com/openai/v1",
        "mistral" => "https://api.mistral.ai/v1",
        "xai" | "grok-web" => "https://api.x.ai/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        "together" => "https://api.together.xyz/v1",
        "fireworks" => "https://api.fireworks.ai/inference/v1",
        "cerebras" => "https://api.cerebras.ai/v1",
        "cohere" => "https://api.cohere.ai/compatibility/v1",
        "nvidia" => "https://integrate.api.nvidia.com/v1",
        "hyperbolic" => "https://api.hyperbolic.xyz/v1",
        "novita" => "https://api.novita.ai/v3/openai",
        "sambanova" => "https://api.sambanova.ai/v1",
        "nebius" => "https://api.studio.nebius.ai/v1",
        "siliconflow" => "https://api.siliconflow.cn/v1",
        "perplexity" => "https://api.perplexity.ai",
        "ollama" | "ollama-local" => "http://127.0.0.1:11434/v1",
        _ => "",
    }
}

/// Auth header placement for a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSpec {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-api-key: <key>` + `anthropic-version: 2023-06-01`
    AnthropicKey,
    /// `x-goog-api-key: <key>`
    GoogleKey,
}

pub fn auth_spec(provider: &str) -> AuthSpec {
    match api_style(provider) {
        ApiStyle::Anthropic => AuthSpec::AnthropicKey,
        ApiStyle::Gemini => AuthSpec::GoogleKey,
        ApiStyle::OpenAi => AuthSpec::Bearer,
    }
}

/// Build the upstream request headers for a provider credential.
pub fn auth_headers(provider: &str, key: &str) -> Vec<(&'static str, String)> {
    match auth_spec(provider) {
        AuthSpec::Bearer => vec![("authorization", format!("Bearer {key}"))],
        AuthSpec::AnthropicKey => vec![
            ("x-api-key", key.to_string()),
            ("anthropic-version", "2023-06-01".to_string()),
        ],
        AuthSpec::GoogleKey => vec![("x-goog-api-key", key.to_string())],
    }
}

/// Heuristic provider inference from a bare model id (no provider prefix),
/// matching 9Router's model→provider resolution when no connection is pinned.
pub fn infer_provider(model: &str) -> &'static str {
    let m = model.to_lowercase();
    if m.contains("claude") {
        "anthropic"
    } else if m.contains("gemini") || m.starts_with("models/") {
        "gemini"
    } else if m.contains("grok") {
        "xai"
    } else if m.contains("deepseek") {
        "deepseek"
    } else if m.contains("qwen") || m.contains("kimi") || m.contains("glm") {
        "openrouter"
    } else {
        "openai"
    }
}

/// Endpoint suffix (appended to base URL) for a provider style.
pub fn chat_path(provider: &str) -> &'static str {
    match api_style(provider) {
        ApiStyle::Anthropic => "/messages",
        ApiStyle::Gemini => "/models",
        ApiStyle::OpenAi => "/chat/completions",
    }
}

/// Full upstream URL for a chat request. Gemini embeds the model in the path.
pub fn chat_url(provider: &str, base: &str, model: &str) -> String {
    let base = base.trim_end_matches('/');
    match api_style(provider) {
        ApiStyle::Anthropic => format!("{base}/messages"),
        ApiStyle::Gemini => format!(
            "{base}/models/{}:generateContent",
            nine_core::normalize_model_id(model)
        ),
        ApiStyle::OpenAi => format!("{base}/chat/completions"),
    }
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn api_type(&self) -> &'static str;
    fn base_url(&self) -> &'static str;
    fn translate_request(&self, req: &ChatRequest) -> Value;
}

/// OpenAI-compatible passthrough adapter (default for api-key providers).
pub struct OpenAiPassthrough {
    pub provider_id: &'static str,
    pub base_url: &'static str,
}

impl Provider for OpenAiPassthrough {
    fn id(&self) -> &'static str {
        self.provider_id
    }
    fn api_type(&self) -> &'static str {
        "openai"
    }
    fn base_url(&self) -> &'static str {
        self.base_url
    }
    fn translate_request(&self, req: &ChatRequest) -> Value {
        json!({
            "model": nine_core::normalize_model_id(&req.model),
            "messages": req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "stream": req.stream,
        })
    }
}

/// Anthropic messages adapter: remaps system + roles to anthropic shape.
pub struct AnthropicAdapter;
impl Provider for AnthropicAdapter {
    fn id(&self) -> &'static str {
        "anthropic"
    }
    fn api_type(&self) -> &'static str {
        "anthropic"
    }
    fn base_url(&self) -> &'static str {
        "https://api.anthropic.com/v1"
    }
    fn translate_request(&self, req: &ChatRequest) -> Value {
        let (system, messages): (Vec<_>, Vec<_>) =
            req.messages.iter().partition(|m| m.role == "system");
        json!({
            "model": nine_core::normalize_model_id(&req.model),
            "system": system.into_iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n"),
            "messages": messages.into_iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "stream": req.stream,
            "max_tokens": 1024,
        })
    }
}

/// Gemini passthrough adapter.
pub struct GeminiAdapter;
impl Provider for GeminiAdapter {
    fn id(&self) -> &'static str {
        "gemini"
    }
    fn api_type(&self) -> &'static str {
        "gemini"
    }
    fn base_url(&self) -> &'static str {
        "https://generativelanguage.googleapis.com/v1beta"
    }
    fn translate_request(&self, req: &ChatRequest) -> Value {
        json!({
            "model": nine_core::normalize_model_id(&req.model),
            "contents": req.messages.iter().map(|m| json!({"role": if m.role=="assistant"{"model"}else{"user"}, "parts": [{"text": m.content}]})).collect::<Vec<_>>(),
        })
    }
}

// ─── Response translation ─────────────────────────────────────────────────

/// Anthropic stop_reason → OpenAI finish_reason.
pub fn map_anthropic_stop(stop: &str) -> &'static str {
    match stop {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

/// Gemini finishReason → OpenAI finish_reason.
pub fn map_gemini_finish(reason: &str) -> &'static str {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" => "content_filter",
        "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => "tool_calls",
        _ => "stop",
    }
}

fn text_from_anthropic_content(content: &Value) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = content.as_array() {
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
                Some("tool_use") => tool_calls.push(json!({
                    "id": b.get("id").cloned().unwrap_or(Value::Null),
                    "type": "function",
                    "function": {
                        "name": b.get("name").cloned().unwrap_or(Value::Null),
                        "arguments": b.get("input").map(|i| i.to_string()).unwrap_or_else(|| "{}".into()),
                    }
                })),
                _ => {}
            }
        }
    }
    (text, tool_calls)
}

/// Convert an Anthropic Messages response into an OpenAI chat.completion.
pub fn translate_anthropic_response(v: &Value, model: &str, req_id: &str) -> Value {
    let (text, tool_calls) = text_from_anthropic_content(v.get("content").unwrap_or(&Value::Null));
    let stop = v
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .unwrap_or("end_turn");
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let usage = v.get("usage").cloned().unwrap_or_else(|| json!({}));
    let input = usage
        .get("input_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    json!({
        "id": v.get("id").and_then(|i| i.as_str()).unwrap_or(req_id),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": nine_core::normalize_model_id(model),
        "choices": [{"index": 0, "message": message, "finish_reason": map_anthropic_stop(stop)}],
        "usage": {"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output},
    })
}

/// Convert a Gemini generateContent response into an OpenAI chat.completion.
pub fn translate_gemini_response(v: &Value, model: &str, req_id: &str) -> Value {
    let cand = v
        .pointer("/candidates/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
        for p in parts {
            if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
            if let Some(fc) = p.get("functionCall") {
                tool_calls.push(json!({
                    "id": format!("call_{}", tool_calls.len()),
                    "type": "function",
                    "function": {
                        "name": fc.get("name").cloned().unwrap_or(Value::Null),
                        "arguments": fc.get("args").map(|a| a.to_string()).unwrap_or_else(|| "{}".into()),
                    }
                }));
            }
        }
    }
    let reason = cand
        .get("finishReason")
        .and_then(|r| r.as_str())
        .unwrap_or("STOP");
    let usage = v.get("usageMetadata").cloned().unwrap_or_else(|| json!({}));
    let input = usage
        .get("promptTokenCount")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("candidatesTokenCount")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    json!({
        "id": req_id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": nine_core::normalize_model_id(model),
        "choices": [{"index": 0, "message": message, "finish_reason": map_gemini_finish(reason)}],
        "usage": {"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output},
    })
}

// ─── Streaming translation ────────────────────────────────────────────────

/// Translate one Anthropic SSE event (already parsed as JSON) into zero or more
/// OpenAI chat.completion.chunk values. Returns empty for control events that
/// have no OpenAI equivalent.
pub fn translate_anthropic_sse(event: &Value, model: &str, req_id: &str) -> Vec<Value> {
    let typ = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let base = |delta: Value, finish: Value| {
        json!({
            "id": req_id,
            "object": "chat.completion.chunk",
            "created": chrono::Utc::now().timestamp(),
            "model": nine_core::normalize_model_id(model),
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        })
    };
    match typ {
        "message_start" => vec![base(
            json!({"role": "assistant", "content": ""}),
            Value::Null,
        )],
        "content_block_delta" => {
            let d = event.get("delta").cloned().unwrap_or_else(|| json!({}));
            let text = d
                .get("text")
                .and_then(|t| t.as_str())
                .or_else(|| d.get("partial_json").and_then(|t| t.as_str()))
                .unwrap_or("");
            vec![base(json!({"content": text}), Value::Null)]
        }
        "message_delta" => {
            let stop = event
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .unwrap_or("end_turn");
            vec![base(json!({}), json!(map_anthropic_stop(stop)))]
        }
        "message_stop" => vec![base(json!({}), json!("stop"))],
        _ => Vec::new(),
    }
}

/// Translate a Gemini SSE JSON chunk into an OpenAI chat.completion.chunk.
pub fn translate_gemini_sse(chunk: &Value, model: &str, req_id: &str) -> Value {
    let cand = chunk
        .pointer("/candidates/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut text = String::new();
    if let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
        for p in parts {
            if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }
    let finish = cand.get("finishReason").and_then(|r| r.as_str());
    json!({
        "id": req_id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": nine_core::normalize_model_id(model),
        "choices": [{
            "index": 0,
            "delta": if text.is_empty() { json!({}) } else { json!({"content": text}) },
            "finish_reason": finish.map(map_gemini_finish),
        }],
    })
}

// ─── Catalog ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub owned_by: String,
    pub endpoint: String,
}

/// Load the 9Router model catalog (`~/.9router/model-catalog.json`) and expand
/// it into provider/model ids. Falls back to an empty list when absent.
pub fn load_catalog(data_dir: &str) -> Vec<ModelEntry> {
    let path = format!("{data_dir}/model-catalog.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(providers) = v.get("providers").and_then(|p| p.as_object()) {
        for (provider, models) in providers {
            if let Some(models) = models.as_object() {
                for model in models.keys() {
                    out.push(ModelEntry {
                        id: format!("{provider}/{model}"),
                        name: model.clone(),
                        kind: "llm".into(),
                        owned_by: provider.clone(),
                        endpoint: "/v1/chat/completions".into(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// OpenAI-shaped model list payload.
pub fn models_payload(entries: &[ModelEntry]) -> Value {
    json!({
        "object": "list",
        "data": entries.iter().map(|e| json!({
            "id": e.id,
            "object": "model",
            "owned_by": e.owned_by,
        })).collect::<Vec<_>>(),
    })
}

/// Gemini-shaped model list payload (`/v1beta/models`).
pub fn gemini_models_payload(entries: &[ModelEntry]) -> Value {
    json!({
        "models": entries.iter().map(|e| json!({
            "name": format!("models/{}", e.id),
            "displayName": e.name,
            "description": format!("{} model: {}", e.owned_by, e.name),
            "supportedGenerationMethods": ["generateContent"],
            "inputTokenLimit": 128000,
            "outputTokenLimit": 8192,
        })).collect::<Vec<_>>(),
    })
}

// Registry of all discovered provider ids (see docs/provider-matrix.md).
pub const PROVIDER_IDS: &[&str] = &[
    "agentrouter",
    "alicode-intl",
    "alicode",
    "alims-intl",
    "alitp-intl",
    "amp",
    "anthropic-m",
    "anthropic",
    "antigravity",
    "api-airforce",
    "assemblyai",
    "aws-polly",
    "azure",
    "baidu",
    "bazaarlink",
    "black-forest-labs",
    "blackbox",
    "bluesminds",
    "brave-search",
    "byteplus",
    "cartesia",
    "cerebras",
    "chutes",
    "claude",
    "cline",
    "clinepass",
    "cloudflare-ai",
    "codebuddy-cn",
    "codebuddy-intl",
    "codex",
    "cohere",
    "comfyui",
    "commandcode",
    "continue",
    "copilot",
    "coqui",
    "cursor",
    "deepgram",
    "deepseek-tui",
    "deepseek",
    "devin-cli",
    "droid",
    "edge-tts",
    "elevenlabs",
    "exa",
    "fal-ai",
    "featherless",
    "firecrawl",
    "fireworks",
    "fish-audio",
    "gemini-cli",
    "gemini",
    "github",
    "gitlab",
    "glm-cn",
    "glm",
    "google-pse",
    "google-tts",
    "grok-cli",
    "grok-web",
    "groq",
    "hermes",
    "huggingface",
    "hyperbolic",
    "iflow",
    "inworld",
    "jcode",
    "jina-ai",
    "jina-reader",
    "kilo-gateway",
    "kilocode",
    "kimchi",
    "kimi-coding",
    "kimi",
    "kiro",
    "linkup",
    "llm7",
    "local-device",
    "longcat",
    "mimo-free",
    "minimax-cn",
    "minimax",
    "mistral",
    "mmf",
    "morph",
    "nanobanana",
    "nebius",
    "novita",
    "nvidia",
    "oai-cc",
    "oai-r",
    "ollama-local",
    "ollama",
    "openai",
    "openclaw",
    "opencode-go",
    "opencode",
    "opendesign",
    "openrouter",
    "perplexity-agent",
    "perplexity-web",
    "perplexity",
    "playht",
    "poolside",
    "qoder",
    "qwen",
    "recraft",
    "reka",
    "roo",
    "runwayml",
    "sambanova",
    "sdwebui",
    "searchapi",
    "searxng",
    "selfhosted-embedding",
    "selfhosted-stt",
    "selfhosted-tts",
    "serper",
    "siliconflow",
    "stability-ai",
    "tavily",
    "tencent",
    "together",
    "tokenrouter",
    "topaz",
    "tortoise",
    "trae",
    "venice",
    "vercel-ai-gateway",
    "vercel",
    "vertex-partner",
    "vertex",
    "volcengine-ark",
    "voyage-ai",
    "windsurf",
    "workbuddy",
    "xai",
    "xiaomi-mimo",
    "xiaomi-tokenplan",
    "xquik",
    "youcom",
    "zed",
];

pub fn is_known_provider(id: &str) -> bool {
    PROVIDER_IDS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_complete() {
        assert!(
            PROVIDER_IDS.len() >= 140,
            "expected 143, got {}",
            PROVIDER_IDS.len()
        );
        for must in ["openai", "anthropic", "codex", "cursor", "gemini", "xai"] {
            assert!(is_known_provider(must), "missing {must}");
        }
    }

    #[test]
    fn openai_translate() {
        let a = OpenAiPassthrough {
            provider_id: "openai",
            base_url: "https://api.openai.com/v1",
        };
        let v = a.translate_request(&ChatRequest {
            model: "GPT_4o".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            stream: false,
        });
        assert_eq!(v["model"], "gpt-4o");
    }

    #[test]
    fn anthropic_system_split() {
        let v = AnthropicAdapter.translate_request(&ChatRequest {
            model: "claude".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: "sys".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "hi".into(),
                },
            ],
            stream: false,
        });
        assert_eq!(v["system"], "sys");
        assert_eq!(v["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn style_and_auth_selection() {
        assert_eq!(api_style("anthropic"), ApiStyle::Anthropic);
        assert_eq!(api_style("gemini-cli"), ApiStyle::Gemini);
        assert_eq!(api_style("openai"), ApiStyle::OpenAi);
        assert_eq!(auth_spec("anthropic"), AuthSpec::AnthropicKey);
        assert!(auth_headers("anthropic", "k")
            .iter()
            .any(|(n, _)| *n == "anthropic-version"));
        assert_eq!(auth_headers("openai", "k")[0].1, "Bearer k");
        assert_eq!(base_url_for("openai"), "https://api.openai.com/v1");
        assert_eq!(base_url_for("nope"), "");
    }

    #[test]
    fn anthropic_response_translation() {
        let v = json!({
            "id": "msg_1",
            "content": [{"type": "text", "text": "hello"}, {"type": "tool_use", "id": "tu_1", "name": "f", "input": {"x": 1}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        });
        let out = translate_anthropic_response(&v, "claude-sonnet", "req_1");
        assert_eq!(out["choices"][0]["message"]["content"], "hello");
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["usage"]["total_tokens"], 15);
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "f"
        );
    }

    #[test]
    fn gemini_response_translation() {
        let v = json!({
            "candidates": [{
                "content": {"parts": [{"text": "hi there"}]},
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": {"promptTokenCount": 7, "candidatesTokenCount": 3}
        });
        let out = translate_gemini_response(&v, "gemini-2.5-pro", "req_2");
        assert_eq!(out["choices"][0]["message"]["content"], "hi there");
        assert_eq!(out["choices"][0]["finish_reason"], "length");
        assert_eq!(out["usage"]["total_tokens"], 10);
    }

    #[test]
    fn chat_url_shapes() {
        assert_eq!(
            chat_url("openai", "https://x/v1/", "gpt-4o"),
            "https://x/v1/chat/completions"
        );
        assert_eq!(
            chat_url("anthropic", "https://x/v1", "claude"),
            "https://x/v1/messages"
        );
        assert_eq!(
            chat_url("gemini", "https://x/v1beta", "gemini-2.5-pro"),
            "https://x/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }
    #[test]
    fn provider_inference() {
        assert_eq!(infer_provider("claude-sonnet-4-6"), "anthropic");
        assert_eq!(infer_provider("models/gemini-2.5-pro"), "gemini");
        assert_eq!(infer_provider("grok-4"), "xai");
        assert_eq!(infer_provider("gpt-4o"), "openai");
    }
    #[test]
    fn stop_reason_maps() {
        assert_eq!(map_anthropic_stop("end_turn"), "stop");
        assert_eq!(map_anthropic_stop("max_tokens"), "length");
        assert_eq!(map_anthropic_stop("tool_use"), "tool_calls");
        assert_eq!(map_gemini_finish("STOP"), "stop");
        assert_eq!(map_gemini_finish("MAX_TOKENS"), "length");
        assert_eq!(map_gemini_finish("SAFETY"), "content_filter");
    }

    #[test]
    fn anthropic_sse_translation() {
        let start = json!({"type": "message_start"});
        assert_eq!(translate_anthropic_sse(&start, "m", "r").len(), 1);
        let delta =
            json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "abc"}});
        let out = translate_anthropic_sse(&delta, "m", "r");
        assert_eq!(out[0]["choices"][0]["delta"]["content"], "abc");
        let stop = json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}});
        assert_eq!(
            translate_anthropic_sse(&stop, "m", "r")[0]["choices"][0]["finish_reason"],
            "length"
        );
        let ping = json!({"type": "ping"});
        assert!(translate_anthropic_sse(&ping, "m", "r").is_empty());
    }

    #[test]
    fn gemini_sse_translation() {
        let chunk = json!({"candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": "STOP"}]});
        let out = translate_gemini_sse(&chunk, "m", "r");
        assert_eq!(out["choices"][0]["delta"]["content"], "x");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn catalog_payload_shapes() {
        let entries = vec![ModelEntry {
            id: "openai/gpt-4o".into(),
            name: "gpt-4o".into(),
            kind: "llm".into(),
            owned_by: "openai".into(),
            endpoint: "/v1/chat/completions".into(),
        }];
        let o = models_payload(&entries);
        assert_eq!(o["object"], "list");
        assert_eq!(o["data"][0]["id"], "openai/gpt-4o");
        assert_eq!(o["data"][0]["owned_by"], "openai");
        let g = gemini_models_payload(&entries);
        assert_eq!(g["models"][0]["name"], "models/openai/gpt-4o");
    }
}
