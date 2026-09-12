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

## Phase 5 verified status

All 12 providers reverse-engineered from 9Router bundle:
- `codex` — OpenAI OAuth, PKCE S256, fixedPort 1455, `/auth/callback`
- `claude` / `anthropic` — Claude OAuth, PKCE S256, `/api/oauth/callback`
- `grok-cli` / `xai` — xAI device+auth, PKCE S256, deviceFlow=true
- `kimi` / `kimi-coding` — Moonshot device flow, `/code/authorize_device`
- `iflow` — iFlow auth, client_id+secret exchange
- `github` / `copilot` — GitHub device+web flow
- `gemini` / `gemini-cli` — Google OAuth2, cloud-platform scope, PKCE S256
- `kiro` — Kiro auth desktop login, PKCE S256, device flow
- `gitlab` — GitLab OAuth, PKCE S256
- `cursor` — Cursor deep control auth
- `qoder` — Qoder OAuth
- `xiaomi-mimo` — Xiaomi token-plan auth

Endpoints implemented & tested:
- `GET /api/oauth/:provider` — authorize URL generation with PKCE S256 + CSRF state
- `GET /api/oauth/callback?code=&state=` — token exchange + ID-token sub/email decode + `providerConnections` row upsert + KV cleanup
- `POST /api/oauth/:provider/exchange` — headless exchange
- `POST /api/oauth/:provider/refresh` — token rotation via refresh_token
- `POST /api/oauth/:provider/import-token` — token import
- `POST /api/oauth/:provider/api-key` — manual key connection
- `POST /api/oauth/:provider/logout` — delete connection row
- Interactive flows (`auto-import`, `social-exchange`, `import-cli-proxy`) return explicit `501 not_implemented` per no-fake rule.
- Tokens stored in `providerConnections.data`; `GET /api/providers` masks sensitive fields, exposes `{id,provider,authType,name,email,expiresAt,scope}`.
