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
