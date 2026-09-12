# Compatibility Report

Reference: 9router@0.5.75 (npm tarball + live ~/.9router state).

| Feature | Original | Rust | Unit | Integration | Differential | E2E | Status |
|---|---|---|---|---|---|---|---|
| Launcher flags | cli.js | nine-cli | [x] | [ ] | [ ] | [ ] | Partial |
| Health/version/init | /api/* | nine-gateway | [x] | [x] | [ ] | [ ] | Partial |
| OpenAI chat + SSE | /api/v1/chat/completions | nine-gateway | [x] | [x] | [ ] | [ ] | Partial |
| Responses/models/messages/v1beta | /api/v1/* | nine-gateway | [x] | [ ] | [ ] | [ ] | Partial |
| 143-provider registry | public/providers | nine-providers | [x] | [ ] | [ ] | [ ] | Partial |
| Anthropic/Gemini translation | built chunks | nine-providers | [x] | [ ] | [ ] | [ ] | Partial |
| OAuth 9 flows | /api/oauth/* | nine-oauth | [x] | [ ] | [ ] | [ ] | Partial |
| Routing priority/RR/alias | combos+models | nine-routing | [x] | [ ] | [ ] | [ ] | Partial |
| Storage 11 tables | sqlite | nine-storage | [x] | [ ] | [ ] | [ ] | Partial |

Totals: discovered ~192 routes, 143 providers, 9 oauth flows, 11 tables. Fully tested: none end-to-end yet. Unsupported: MITM cert intercept (privileged helper, documented), cloudflared/tailscale/systray OS integrations, 130+ provider upstream transforms (per-provider Phase 4 pending), TTS/MCP/media endpoints (route stubs pending).

## Phase 3 (core proxy) — done

- Upstream forwarding with provider-prefix routing, SSE byte passthrough, x-request-id, error schema {error:{message,type}}, 408/429/5xx fallback, timeouts (504), 4xx fail-fast.
- Tests: 27 pass (21 unit + 6 mock-upstream proxy); clippy -D warnings clean; fmt clean.
- Wiring: NINE_UPSTREAM_URL / NINE_UPSTREAM_KEY / TIMEOUT_MS. Storage-backed connections + usage logging deferred to Phase 4/6.

## Phase 4 (provider adapters) — done

- providers crate: ApiStyle/auth_spec/auth_headers/chat_url, model catalog loader, OpenAI+Anthropic+Gemini request adapters, response translation (stop reasons, tool_calls, usage), SSE translation for Anthropic and Gemini.
- gateway: real `/v1/models`, `/v1/models/info`, `/v1beta/models`, `/v1beta/models/*:generateContent`, `/v1/messages`; auth via apiKeys (open_mode for dev) with `{error:{message,type,code}}`; provider-prefix routing + fallback loop.
- Tests: 49 pass (8 adapter mock-integration incl. streaming + errors; 11 gateway; 11 providers; routing/config/storage/core).
- clippy `-D warnings` clean, fmt clean.
- Deferred: `/v1/responses` (501 stub), embeddings/audio/images/video/search media endpoints, connections from DB (Phase 6), differential parity for model count (needs connections).

### Phase 4 live smoke (vs original on :20128)

| Check | Original | Rust |
|---|---|---|
| GET /api/health | {ok:true} | {ok:true} |
| GET /v1/models/info (no id) | 400 invalid_request_error + same message | identical |
| GET /v1beta/models | {models:[{name,displayName,description,supportedGenerationMethods,inputTokenLimit,outputTokenLimit}]} | identical shape |
| POST /v1/messages bad key | 401 {error:{message,type,code}} | identical envelope |
| POST /v1/chat/completions bad key | 401 {error:{message,type,code}} | identical envelope |
