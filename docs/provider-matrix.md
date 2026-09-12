# Provider Matrix (143 icon ids from app/public/providers)

Auth/API/BaseURL/transforms reverse-engineered per-adapter during Phase 4. Registry implemented in crates/providers registry.

| Provider ID | Auth | API type | Rust adapter | Status |
|---|---|---|---|---|
| `agentrouter` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `alicode` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `alicode-intl` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `alims-intl` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `alitp-intl` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `amp` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `anthropic` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `anthropic-m` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `antigravity` | oauth/subscription | openai-or-native | providers::registry | [ ] Unsupported (Native(antigravity)) |
| `api-airforce` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `assemblyai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `aws-polly` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `azure` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `baidu` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `bazaarlink` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `black-forest-labs` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `blackbox` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `bluesminds` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `brave-search` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `byteplus` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `cartesia` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `cerebras` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `chutes` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `claude` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `cline` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `clinepass` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `cloudflare-ai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `codebuddy-cn` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `codebuddy-intl` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `codex` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (Responses) |
| `cohere` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `comfyui` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `commandcode` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(commandcode)) |
| `continue` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `copilot` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `coqui` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `cursor` | oauth/subscription | openai-or-native | providers::registry | [ ] Unsupported (Native(cursor)) |
| `deepgram` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `deepseek` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `deepseek-tui` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `devin-cli` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `droid` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `edge-tts` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `elevenlabs` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `exa` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `fal-ai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `featherless` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `firecrawl` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `fireworks` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `fish-audio` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `gemini` | api-key | openai-or-native | providers::registry | [x] Implemented (Gemini) |
| `gemini-cli` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (Gemini) |
| `github` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `gitlab` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `glm` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `glm-cn` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `google-pse` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `google-tts` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `grok-cli` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (Responses) |
| `grok-web` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(grok-web)) |
| `groq` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `hermes` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `huggingface` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `hyperbolic` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `iflow` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `inworld` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `jcode` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `jina-ai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `jina-reader` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kilo-gateway` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kilocode` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kimchi` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kimchi` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kimi` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `kimi-coding` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `kiro` | oauth/subscription | openai-or-native | providers::registry | [ ] Unsupported (Native(kiro)) |
| `linkup` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `llm7` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `local-device` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `longcat` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `mimo-free` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `minimax` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `minimax-cn` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `mistral` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `mmf` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `morph` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `nanobanana` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `nebius` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `novita` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `nvidia` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `oai-cc` | api-key | openai-or-native | providers::registry | [x] Implemented (Anthropic) |
| `oai-r` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `ollama` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(ollama)) |
| `ollama-local` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `openai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `openclaw` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `opencode` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `opencode-go` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `opendesign` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `openrouter` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `perplexity` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `perplexity-agent` | api-key | openai-or-native | providers::registry | [x] Implemented (Responses) |
| `perplexity-web` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(perplexity-web)) |
| `playht` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `poolside` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `qoder` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `qwen` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `recraft` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `reka` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `roo` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `runwayml` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `sambanova` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `sdwebui` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `searchapi` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `searxng` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `selfhosted-embedding` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `selfhosted-stt` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `selfhosted-tts` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `serper` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `siliconflow` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `stability-ai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `tavily` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `tencent` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `together` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `tokenrouter` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `topaz` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `tortoise` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `trae` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `venice` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `vercel` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `vercel-ai-gateway` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `vertex` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(vertex)) |
| `vertex-partner` | api-key | openai-or-native | providers::registry | [ ] Unsupported (Native(vertex)) |
| `volcengine-ark` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `voyage-ai` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `windsurf` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `workbuddy` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `xai` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `xiaomi-mimo` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `xiaomi-tokenplan` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `xquik` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `youcom` | api-key | openai-or-native | providers::registry | [x] Implemented (OpenAi) |
| `zed` | oauth/subscription | openai-or-native | providers::registry | [x] Implemented (OpenAi) |

## Phase 4 adapter status

Implemented + tested (mock upstream + translation):
- `openai` — Bearer, `{base}/chat/completions`, pass-through JSON/SSE
- `anthropic` — x-api-key + anthropic-version, `{base}/messages`, system-split request, tool_use + stop_reason + usage translation, SSE→OpenAI chunk translation
- `gemini` — x-goog-api-key, `{base}/models/{model}:generateContent`, parts/finishReason/usageMetadata translation, SSE→OpenAI chunk translation
- Fallback order = provider-prefix match first, then configured connections; 408/429/5xx retry, 4xx fail-fast, timeout 504

Still open (documented): per-provider special headers/transforms for the remaining ~140 ids, embeddings/audio/images/video/search/web adapters, storage-backed connections.
