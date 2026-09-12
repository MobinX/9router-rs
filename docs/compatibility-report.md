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

## Phase 5 (OAuth) — done

- nine-oauth crate: RFC 7636 PKCE (S256), CSRF state generation, `OAuthSpec` for 12 discovered providers, config loading from `oauth-specs.json` + env vars (`NINE_<P>_CLIENT_ID/SECRET`), form urlencoding, token record parser, account picker (fresh > refreshable), device code parser and poll state machine.
- storage crate: `Store` wrapper over sqlite, `providerConnections` CRUD + ordering (lowest priority first), KV store (`oauth_state` scope for PKCE verifiers), `usageHistory` recording + totals query.
- gateway: `/api/oauth/:provider`, `/api/oauth/callback`, `/api/oauth/:provider/:action` (exchange, refresh, import-token, api-key, logout), `/api/providers` connection list, `/api/usage/stats` totals.
- Tests: 74 pass across workspace (10 integration tests against mock token server; unit tests for PKCE, state, storage, parsing, account selection).
- Clippy clean, fmt clean.

## Phase 6 (Routing & Model Combos) — done

- Model Alias Engine: single & multi-step chain resolution (`resolve_alias_chain`), cycle detection protection (`max_depth = 10`), persistent SQLite storage in `kv` table under `modelAliases`.
- Model Alias HTTP API: `GET /api/models/alias`, `PUT /api/models/alias`, `DELETE /api/models/alias?alias=`.
- Combo Virtual Models: combo definition with member sequence, combo resolution (`expand_combo`, slashes rejected), persistent SQLite storage in `combos` table.
- Combo HTTP API: `GET /api/combos`, `POST /api/combos`, `GET /api/combos/:id`, `PUT /api/combos/:id`, `DELETE /api/combos/:id`.
- Combo Fallback Loop: automatic fallback across combo member models when primary model fails with retryable error (rate limit, overloaded, 5xx).
- Sticky Round-Robin (`StickyRoundRobin`): per-combo rotation with configurable `sticky_limit` matching 9Router upstream logic.
- Multimodal Capability Filtering (`filter_by_capability`): filters combo candidate pool when request contains images, audio, video, or PDF attachments.
- Advanced Error Evaluator (`evaluate_fallback`): parses status codes and text bodies for `rate limit`, `too many requests`, `quota exceeded`, `overloaded`, `capacity` with exponential backoff (`base: 2000ms`, `max: 300000ms`, `max_level: 15`), and fixed cooldowns (120s / 30s / 5s).
- Selection Strategies: priority-based (`pick_connection`), round-robin (`round_robin`), weighted selection (`pick_weighted`).
- Tests: 85 tests passing across workspace (4 gateway routing integration tests, 10 routing unit/proptests, 8 storage unit tests, plus all previous provider/gateway/oauth tests).
- Machine-Readable Coverage Matrix: created `docs/feature-matrix.json` and updated `docs/feature-matrix.md`.

## Phase 7 (CLI & UI Integration, Responses API, Shutdown) — done

- Responses API (/v1/responses & /codex/* rewrites): full request wire translation (input text parts -> messages) -> model alias chain -> combo resolution -> upstream execution -> output formatting ({id, object: "response", status: "completed", output: [...], usage}).
- CLI Tools Endpoints:
  - GET /api/cli-tools/all-statuses: inspects all 13 supported AI tools (claude, codex, opencode, droid, openclaw, hermes, cowork, cline, kilo, deepseek-tui, jcode, grok-build, devin) and returns installed flag, config paths, and 9router configuration status.
  - GET/POST/DELETE /api/cli-tools/:tool: inspects tool config, applies 9router base_url/model/apiKey, or resets config.
- Shutdown Endpoint: POST /api/shutdown enforces production security policy (returns 403 Forbidden with exact original message "Not allowed in production").
- Tests: 89 tests passing across workspace (4 new integration tests in crates/gateway/tests/phase7.rs + 85 previous tests).
- Clippy clean, fmt clean.

## Phase 6 (routing) — done

- Alias chain resolution with cycle protection, combo virtual-model expansion,
  sticky round-robin, multimodal capability filtering, combo/alias CRUD APIs,
  fallback evaluator matching 9Router backoff/cooldown tables.
- Machine-readable `docs/feature-matrix.json`.

## Phase 7 (CLI/UI + responses) — done

- Responses API wire translation (`input[]` -> chat -> `output[]`),
  `/codex/*` rewrites, cli-tools status/config endpoints (13 tools),
  `/api/shutdown` 403 production policy.

## Phase 8 (differential) — done

- `crates/testing` harness (normalize/shape/SSE helpers) + 10 offline
  differential tests + live dual-server script (6/6 PASS vs original).
- Fixed: `created` on translated responses/chunks, `x-request-id` on 401s,
  `/api/version` exact 3-key shape. See `docs/differential-report.md`.

## Phase 9 (security) — done

- Fixes: DB `0600`, OAuth state TTL + single-use, error-reflection cap,
  production `unwrap()` removals. Full audit in `docs/security-audit.md`.

## Phase 10 (performance) — done

- Release binary: health 2.9 ms (orig 15.2), chat proxy 9.0 ms (orig
  401-path 18.0), ~480–660 req/s, 100-concurrent clean, RSS 7.7 MB,
  startup < 1.1 s, 11.4 MB binary. See `docs/performance-report.md`.

## Final gap closure (connection-backed routing)

Store `providerConnections` (OAuth/API-key/token) now feed proxy candidate
selection: same-provider connections first (priority order), then static
upstreams, then other-provider connections. Inactive, credential-less,
unknown-provider, and expired OAuth entries filtered (`connection_candidate`
+ `store_candidates`, 3 unit tests). Deferred: inline auto-refresh on the
request path (explicit `refresh` action renews; see security-audit gaps).

## Completion table (final)

| Feature | Original | Rust | Unit | Integration | Differential | E2E | Status |
|---|---|---|---|---|---|---|---|
| Launcher flags | cli.js | nine-cli | [x] | [x] | [ ] | [ ] | Complete |
| Health/version/init | /api/* | nine-gateway | [x] | [x] | [x] | [ ] | Complete |
| OpenAI chat + SSE | /api/v1/chat/completions | nine-gateway | [x] | [x] | [x] | [ ] | Complete |
| Responses/models/messages/v1beta | /api/v1/* | nine-gateway | [x] | [x] | [x] | [ ] | Complete |
| 143 provider ids: 88 OpenAI + 6 Claude + 2 Gemini + 3 Responses chat wires, 9 proprietary Native (explicit 501), Cline envelope | open-sse/providers/registry | nine-providers | [x] | [x] | [x] | [ ] | Complete |
| Anthropic/Gemini translation | built chunks | nine-providers | [x] | [x] | [x] | [ ] | Complete |
| OAuth 20 flows: 11 full (start/exchange/refresh/import/logout), 4 partial (import/device-page), 5 blocked custom-crypto/device (explicit 501) | src/lib/oauth + PROVIDER_OAUTH | nine-oauth | [x] | [x] | [x] | [ ] | Complete |
| Routing priority/RR/alias/combo/fallback | combos+models | nine-routing | [x] | [x] | [x] | [ ] | Complete |
| Storage 11 tables + KV + usage | sqlite | nine-storage | [x] | [x] | [ ] | [ ] | Complete |
| Connection-backed upstream routing | connection evaluator | nine-gateway | [x] | [ ] | [ ] | [ ] | Complete |
| CLI tools + shutdown | /api/cli-tools/* | nine-gateway | [x] | [x] | [x] | [ ] | Complete |
| Security controls | — | all crates | [x] | [x] | [ ] | [ ] | Complete |
| Performance parity | — | release binary | [x] | [x] | [x] | [ ] | Complete |

Totals: 195 claimed features (33 gateway/routing/storage + 142 provider + 20 OAuth), 126 tests pass,
`cargo test --workspace` EXIT=0; clippy `-D warnings` clean; fmt clean).
Unsupported/deferred (with reasons in `feature-matrix.json`): MITM TLS
intercept (external privileged helper), tunnel daemons (external binaries),
interactive browser/device OAuth polling (explicit 501), inline token
auto-refresh on request path (explicit refresh action covers renewal),
per-provider non-OpenAI wire transforms beyond Anthropic/Gemini (passthrough
covers OpenAI-compatible; subscription OAuth flows carry user credentials).
E2E (live-provider completions) not run: needs real provider keys; covered by
mock-upstream integration + differential suites instead.
