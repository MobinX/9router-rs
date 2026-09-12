# OAuth Matrix

Generic flow: `GET /api/oauth/[provider]` (start: auth URL + PKCE + state) → callback `GET /api/oauth/[provider]/[action]` (code exchange) → `providerConnections` row (authType=oauth, data JSON: access/refresh/expiry) → background refresh (`backgroundTokenRefresh`) → rotation/multi-account via multiple rows + priority.

| Provider | Start/Exchange endpoints | Token storage | Refresh | Rust module | Status |
|---|---|---|---|---|---|
| codex | `/api/oauth/codex`, `/bulk-import`, `/import-token` | providerConnections.data | auto | oauth::codex | [ ] Discovered |
| cursor | `/api/oauth/cursor`, `/import`, `/auto-import` | providerConnections.data | auto | oauth::cursor | [ ] Discovered |
| gitlab | `/api/oauth/gitlab`, `/pat` | providerConnections.data | pat/manual | oauth::gitlab | [ ] Discovered |
| grok-cli | `/api/oauth/grok-cli`, `/bulk-import` | providerConnections.data | auto | oauth::grok_cli | [ ] Discovered |
| iflow | `/api/oauth/iflow`, `/cookie` | providerConnections.data | cookie | oauth::iflow | [ ] Discovered |
| kiro | `/api/oauth/kiro`, `/api-key`, `/auto-import`, `/import`, `/import-cli-proxy`, `/social-authorize`, `/social-exchange` | providerConnections.data | auto | oauth::kiro | [ ] Discovered |
| xiaomi-mimo | `/api/oauth/xiaomi-mimo`, `/api-key`, `/auto-import` + `lib/oauth/providers/xiaomi-mimo.js` | providerConnections.data | auto | oauth::xiaomi_mimo | [ ] Discovered |
| qoder | service `lib/oauth/services/qoder.js` | providerConnections.data | auto | oauth::qoder | [ ] Discovered |
| xai | service `lib/oauth/services/xai.js` (+ `xai video` CLI via gateway) | providerConnections.data | auto | oauth::xai | [ ] Discovered |
| generic [provider] | `/api/oauth/[provider]/[action]` | providerConnections.data | auto | oauth::generic | [ ] Discovered |

Security: no hardcoded secrets; PKCE S256, state CSRF, localhost callback validated, tokens never logged, sqlite file 0600, logout/revoke deletes row.
