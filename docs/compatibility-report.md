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
