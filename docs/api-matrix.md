# API Matrix (9Router 0.5.75)

Discovered Next.js route dirs under `.next-cli-build/server/app/api`. Rewrites: `/v1/*→/api/v1/*`, `/codex/*→/api/v1/responses`, `/responses→/api/v1/responses`, `/v1beta/*→/api/v1beta/*`.

| Route | Rust handler | Status |
|---|---|---|
| `/api` | gateway::routes | [ ] Discovered |
| `/api/auth` | gateway::routes | [ ] Discovered |
| `/api/auth/login` | gateway::mgmt::local | [x] Implemented
| `/api/auth/logout` | gateway::mgmt::logout | [x] Implemented
| `/api/auth/oidc` | gateway::routes | [ ] Discovered |
| `/api/auth/oidc/callback` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/oidc/start` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/oidc/test` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/reset-password` | gateway::mgmt::local | [x] Implemented
| `/api/auth/saml` | gateway::routes | [ ] Discovered |
| `/api/auth/saml/acs` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/saml/metadata` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/saml/start` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/saml/test` | gateway::mgmt::enterprise | [x] Blocked
| `/api/auth/status` | gateway::routes | [ ] Discovered |
| `/api/cli-tools` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/all-statuses` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/antigravity-mitm` | gateway::mgmt::status/config; | [x] Partial
| `/api/cli-tools/antigravity-mitm/alias` | gateway::mgmt::mitm | [x] Implemented
| `/api/cli-tools/claude-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/cline-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/codex-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/copilot-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/cowork-mcp-registry` | gateway::mgmt::registry | [x] Implemented
| `/api/cli-tools/cowork-mcp-tools` | gateway::mgmt::MCP | [x] Implemented
| `/api/cli-tools/cowork-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/deepseek-tui-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/devin-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/droid-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/grok-build-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/hermes-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/jcode-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/kilo-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/openclaw-settings` | gateway::routes | [ ] Discovered |
| `/api/cli-tools/opencode-settings` | gateway::routes | [ ] Discovered |
| `/api/combos` | gateway::routes | [ ] Discovered |
| `/api/combos/[id]` | gateway::routes | [ ] Discovered |
| `/api/headroom` | gateway::routes | [ ] Discovered |
| `/api/headroom/extras` | gateway::mgmt::extras | [x] Implemented
| `/api/headroom/proxy` | gateway::routes | [ ] Discovered |
| `/api/headroom/proxy/[...path]` | gateway::routes | [ ] Discovered |
| `/api/headroom/restart` | gateway::mgmt::501 | [x] Blocked
| `/api/headroom/start` | gateway::mgmt::501 | [x] Blocked
| `/api/headroom/status` | gateway::mgmt::static | [x] Implemented
| `/api/headroom/stop` | gateway::mgmt::501 | [x] Blocked
| `/api/health` | gateway::routes | [ ] Discovered |
| `/api/init` | gateway::routes | [ ] Discovered |
| `/api/keys` | gateway::mgmt::keys | [x] Implemented
| `/api/keys/[id]` | gateway::mgmt::keys | [x] Implemented
| `/api/locale` | gateway::mgmt::locale | [x] Implemented
| `/api/mcp` | gateway::routes | [ ] Discovered |
| `/api/mcp/[plugin]` | gateway::routes | [ ] Discovered |
| `/api/mcp/[plugin]/message` | gateway::mgmt::501 | [x] Blocked
| `/api/mcp/[plugin]/sse` | gateway::mgmt::501 | [x] Blocked
| `/api/media-providers` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts/deepgram` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts/deepgram/voices` | gateway::mgmt::501 | [x] Blocked
| `/api/media-providers/tts/elevenlabs` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts/elevenlabs/voices` | gateway::mgmt::501 | [x] Blocked
| `/api/media-providers/tts/inworld` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts/inworld/voices` | gateway::mgmt::501 | [x] Blocked
| `/api/media-providers/tts/minimax` | gateway::routes | [ ] Discovered |
| `/api/media-providers/tts/minimax/voices` | gateway::mgmt::501 | [x] Blocked
| `/api/media-providers/tts/voices` | gateway::mgmt::empty | [x] Implemented
| `/api/models` | gateway::mgmt::alias | [x] Implemented
| `/api/models/alias` | gateway::routes | [ ] Discovered |
| `/api/models/availability` | gateway::mgmt::model | [x] Implemented
| `/api/models/catalog-sync` | gateway::mgmt::catalog | [x] Implemented
| `/api/models/custom` | gateway::mgmt::custom | [x] Implemented
| `/api/models/disabled` | gateway::mgmt::disabled | [x] Implemented
| `/api/models/test` | gateway::mgmt::catalog | [x] Implemented
| `/api/oauth` | gateway::routes | [ ] Discovered |
| `/api/oauth/[provider]` | gateway::routes | [ ] Discovered |
| `/api/oauth/[provider]/[action]` | gateway::routes | [ ] Discovered |
| `/api/oauth/codex` | gateway::routes | [ ] Discovered |
| `/api/oauth/codex/bulk-import` | gateway::mgmt::bulk | [x] Implemented
| `/api/oauth/codex/import-token` | gateway::mgmt::codex | [x] Implemented
| `/api/oauth/cursor` | gateway::routes | [ ] Discovered |
| `/api/oauth/cursor/auto-import` | gateway::mgmt::local | [x] Implemented
| `/api/oauth/cursor/import` | gateway::mgmt::token | [x] Implemented
| `/api/oauth/gitlab` | gateway::routes | [ ] Discovered |
| `/api/oauth/gitlab/pat` | gateway::mgmt::PAT | [x] Implemented
| `/api/oauth/grok-cli` | gateway::routes | [ ] Discovered |
| `/api/oauth/grok-cli/bulk-import` | gateway::mgmt::bulk | [x] Implemented
| `/api/oauth/iflow` | gateway::routes | [ ] Discovered |
| `/api/oauth/iflow/cookie` | gateway::mgmt::cookie | [x] Implemented
| `/api/oauth/kiro` | gateway::routes | [ ] Discovered |
| `/api/oauth/kiro/api-key` | gateway::mgmt::api-key | [x] Implemented
| `/api/oauth/kiro/auto-import` | gateway::mgmt::local | [x] Implemented
| `/api/oauth/kiro/import` | gateway::mgmt::token | [x] Implemented
| `/api/oauth/kiro/import-cli-proxy` | gateway::mgmt::token | [x] Implemented
| `/api/oauth/kiro/social-authorize` | gateway::mgmt::Cognito | [x] Implemented
| `/api/oauth/kiro/social-exchange` | gateway::mgmt::Cognito | [x] Implemented
| `/api/oauth/xiaomi-mimo` | gateway::routes | [ ] Discovered |
| `/api/oauth/xiaomi-mimo/api-key` | gateway::mgmt::api-key | [x] Implemented
| `/api/oauth/xiaomi-mimo/auto-import` | gateway::mgmt::local | [x] Implemented
| `/api/pricing` | gateway::mgmt::pricing | [x] Implemented
| `/api/provider-nodes` | gateway::mgmt::nodes | [x] Implemented
| `/api/provider-nodes/[id]` | gateway::mgmt::nodes | [x] Implemented
| `/api/provider-nodes/validate` | gateway::mgmt::node | [x] Implemented
| `/api/providers` | gateway::mgmt::provider | [x] Implemented
| `/api/providers/[id]` | gateway::mgmt::provider | [x] Implemented
| `/api/providers/[id]/models` | gateway::mgmt::provider | [x] Implemented
| `/api/providers/[id]/test` | gateway::mgmt::provider | [x] Implemented
| `/api/providers/[id]/test-models` | gateway::mgmt::provider | [x] Implemented
| `/api/providers/client` | gateway::mgmt::sanitized | [x] Implemented
| `/api/providers/kilo` | gateway::routes | [ ] Discovered |
| `/api/providers/kilo/free-models` | gateway::mgmt::kilo | [x] Implemented
| `/api/providers/suggested-models` | gateway::mgmt::catalog | [x] Implemented
| `/api/providers/test-batch` | gateway::mgmt::batch | [x] Implemented
| `/api/providers/validate` | gateway::mgmt::provider | [x] Implemented
| `/api/proxy-pools` | gateway::mgmt::pools | [x] Implemented
| `/api/proxy-pools/[id]` | gateway::mgmt::pools | [x] Implemented
| `/api/proxy-pools/[id]/test` | gateway::mgmt::pool | [x] Implemented
| `/api/proxy-pools/cloudflare-deploy` | gateway::mgmt::501 | [x] Blocked
| `/api/proxy-pools/deno-deploy` | gateway::mgmt::501 | [x] Blocked
| `/api/proxy-pools/vercel-deploy` | gateway::mgmt::501 | [x] Blocked
| `/api/pxpipe` | gateway::routes | [ ] Discovered |
| `/api/pxpipe/health` | gateway::mgmt::501 | [x] Blocked
| `/api/pxpipe/install` | gateway::mgmt::501 | [x] Blocked
| `/api/pxpipe/logs` | gateway::mgmt::empty | [x] Implemented
| `/api/pxpipe/restart` | gateway::mgmt::501 | [x] Blocked
| `/api/pxpipe/start` | gateway::mgmt::501 | [x] Blocked
| `/api/pxpipe/stats` | gateway::mgmt::settings-backed | [x] Implemented
| `/api/pxpipe/status` | gateway::mgmt::settings-backed | [x] Implemented
| `/api/pxpipe/stop` | gateway::mgmt::501 | [x] Blocked
| `/api/settings` | gateway::mgmt::settings | [x] Implemented
| `/api/settings/database` | gateway::mgmt::db | [x] Implemented
| `/api/settings/proxy-test` | gateway::mgmt::proxy | [x] Implemented
| `/api/settings/require-login` | gateway::mgmt::require-login | [x] Implemented
| `/api/shutdown` | gateway::routes | [ ] Discovered |
| `/api/tags` | gateway::mgmt::ollama | [x] Implemented
| `/api/translator` | gateway::routes | [ ] Discovered |
| `/api/translator/console-logs` | gateway::mgmt::in-process | [x] Implemented
| `/api/translator/console-logs/stream` | gateway::mgmt::SSE | [x] Implemented
| `/api/translator/load` | gateway::mgmt::allowlisted | [x] Implemented
| `/api/translator/save` | gateway::mgmt::allowlisted | [x] Implemented
| `/api/translator/send` | gateway::mgmt::501 | [x] Blocked
| `/api/translator/translate` | gateway::mgmt::step-1 | [x] Partial
| `/api/tunnel` | gateway::routes | [ ] Discovered |
| `/api/tunnel/disable` | gateway::mgmt::501 | [x] Blocked
| `/api/tunnel/enable` | gateway::mgmt::501 | [x] Blocked
| `/api/tunnel/status` | gateway::mgmt::static | [x] Implemented
| `/api/tunnel/tailscale-check` | gateway::mgmt::static | [x] Implemented
| `/api/tunnel/tailscale-disable` | gateway::mgmt::501 | [x] Blocked
| `/api/tunnel/tailscale-enable` | gateway::mgmt::501 | [x] Blocked
| `/api/tunnel/tailscale-install` | gateway::mgmt::501 | [x] Blocked
| `/api/usage` | gateway::routes | [ ] Discovered |
| `/api/usage/[connectionId]` | gateway::mgmt::connection | [x] Implemented
| `/api/usage/[connectionId]/codex-reset-credits` | gateway::mgmt::BLOCKED | [x] Blocked
| `/api/usage/chart` | gateway::mgmt::usage | [x] Implemented
| `/api/usage/history` | gateway::mgmt::usage | [x] Implemented
| `/api/usage/logs` | gateway::mgmt::recent | [x] Implemented
| `/api/usage/providers` | gateway::mgmt::usage | [x] Implemented
| `/api/usage/request-details` | gateway::mgmt::request | [x] Implemented
| `/api/usage/request-logs` | gateway::mgmt::recent | [x] Implemented
| `/api/usage/stats` | gateway::routes | [ ] Discovered |
| `/api/usage/stream` | gateway::mgmt::usage | [x] Implemented
| `/api/v1` | gateway::routes | [ ] Discovered |
| `/api/v1/api` | gateway::routes | [ ] Discovered |
| `/api/v1/api/chat` | gateway::mgmt::delegates | [x] Implemented
| `/api/v1/audio` | gateway::routes | [ ] Discovered |
| `/api/v1/audio/speech` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/audio/transcriptions` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/audio/voices` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/chat` | gateway::routes | [ ] Discovered |
| `/api/v1/chat/completions` | gateway::routes | [ ] Discovered |
| `/api/v1/embeddings` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/images` | gateway::routes | [ ] Discovered |
| `/api/v1/images/generations` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/messages` | gateway::routes | [ ] Discovered |
| `/api/v1/messages/count_tokens` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/models` | gateway::routes | [ ] Discovered |
| `/api/v1/models/[...model]` | gateway::mgmt::catalog | [x] Implemented
| `/api/v1/models/info` | gateway::routes | [ ] Discovered |
| `/api/v1/responses` | gateway::routes | [ ] Discovered |
| `/api/v1/responses/compact` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/search` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/videos` | gateway::routes | [ ] Discovered |
| `/api/v1/videos/[id]` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/videos/edits` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/videos/extensions` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/videos/generations` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1/web` | gateway::routes | [ ] Discovered |
| `/api/v1/web/fetch` | gateway::mgmt::upstream | [x] Implemented
| `/api/v1beta` | gateway::routes | [ ] Discovered |
| `/api/v1beta/models` | gateway::routes | [ ] Discovered |
| `/api/v1beta/models/[...path]` | gateway::routes | [ ] Discovered |
| `/api/version` | gateway::routes | [ ] Discovered |
| `/api/version/shutdown` | gateway::mgmt::403 | [x] Implemented
| `/api/version/update` | gateway::mgmt::no-update | [x] Implemented

## Key LLM endpoints

- `POST /api/v1/chat/completions` (+ rewrite `/v1/chat/completions`) OpenAI chat, SSE when `stream:true`
- `POST /api/v1/responses` (+ `/codex/*`, `/responses`) Responses API
- `GET /api/v1/models` (+ `/v1/models`) list; `/api/v1/models/info`, `/api/v1/models/[...model]`
- `POST /api/v1/messages` Anthropic; `POST /api/v1/messages/count_tokens`
- `POST /api/v1/embeddings`, `/api/v1/audio/*`, `/api/v1/images/generations`, `/api/v1/videos/*`, `/api/v1/search`, `/api/v1/web/fetch`, `/api/v1/api/chat`
- `/api/v1beta/models*` Gemini passthrough
- `GET /api/health`, `/api/version`, `/api/init`, `/api/auth/*`, `/api/keys*`, `/api/providers*`, `/api/oauth/*`, `/api/usage/*`, `/api/settings`, `/api/mcp/*`

## Phase 4 verified shapes (live original, 2026-09-12)

- `GET /v1/models` (public): `{object:"list", data:[{id:"<provider>/<model>", object:"model", owned_by, capabilities?, context_length?, max_completion_tokens?}]}`
- `GET /v1/models/info?id=<provider>/<model>`: 400 without id (`invalid_request_error`), else `{id,name,kind,owned_by,endpoint}`
- `GET /v1beta/models?key=` (public): `{models:[{name:"models/<provider>/<model>", displayName, description, supportedGenerationMethods, inputTokenLimit, outputTokenLimit}]}`
- `POST /v1beta/models/{provider}/{model}:generateContent`: auth via `?key=`/`x-goog-api-key`/Bearer; 401 `{error:{message:"Missing API key",type:"authentication_error",code:"invalid_api_key"}}`
- `POST /v1/messages`: Anthropic native in/out; 401 body has `code:invalid_api_key`
- `POST /v1/chat/completions`: OpenAI shape out for all styles (anthropic/gemini translated)
- Auth error envelope everywhere: `{error:{message,type,code}}`
