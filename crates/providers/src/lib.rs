use serde::{Deserialize, Serialize};

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

pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn api_type(&self) -> &'static str;
    fn base_url(&self) -> &'static str;
    fn translate_request(&self, req: &ChatRequest) -> serde_json::Value;
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
    fn translate_request(&self, req: &ChatRequest) -> serde_json::Value {
        serde_json::json!({
            "model": nine_core::normalize_model_id(&req.model),
            "messages": req.messages.iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
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
        "https://api.anthropic.com"
    }
    fn translate_request(&self, req: &ChatRequest) -> serde_json::Value {
        let (system, messages): (Vec<_>, Vec<_>) =
            req.messages.iter().partition(|m| m.role == "system");
        serde_json::json!({
            "model": nine_core::normalize_model_id(&req.model),
            "system": system.into_iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n"),
            "messages": messages.into_iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
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
        "https://generativelanguage.googleapis.com"
    }
    fn translate_request(&self, req: &ChatRequest) -> serde_json::Value {
        serde_json::json!({
            "model": nine_core::normalize_model_id(&req.model),
            "contents": req.messages.iter().map(|m| serde_json::json!({"role": if m.role=="assistant"{"model"}else{"user"}, "parts": [{"text": m.content}]})).collect::<Vec<_>>(),
        })
    }
}

// Registry of all discovered provider ids (see docs/provider-matrix.md).
pub const PROVIDER_IDS: &[&str] = &[
    "agentrouter",
    "alicode",
    "alicode-intl",
    "alims-intl",
    "alitp-intl",
    "amp",
    "anthropic",
    "anthropic-m",
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
    "deepseek",
    "deepseek-tui",
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
    "gemini",
    "gemini-cli",
    "github",
    "gitlab",
    "glm",
    "glm-cn",
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
    "kimchi",
    "kimi",
    "kimi-coding",
    "kiro",
    "linkup",
    "llm7",
    "local-device",
    "longcat",
    "mimo-free",
    "minimax",
    "minimax-cn",
    "mistral",
    "mmf",
    "morph",
    "nanobanana",
    "nebius",
    "novita",
    "nvidia",
    "oai-cc",
    "oai-r",
    "ollama",
    "ollama-local",
    "openai",
    "openclaw",
    "opencode",
    "opencode-go",
    "opendesign",
    "openrouter",
    "perplexity",
    "perplexity-agent",
    "perplexity-web",
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
    "vercel",
    "vercel-ai-gateway",
    "vertex",
    "vertex-partner",
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
}
