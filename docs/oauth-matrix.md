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

## Phase 6 verified status (all 20 upstream PROVIDER_OAUTH keys)

Reverse-engineered from `open-sse/providers/index.js` PROVIDER_OAUTH + `src/lib/oauth/*`.
Public installed-app client ids ship as defaults (as upstream); secrets are env-only
(`NINE_<PROVIDER>_CLIENT_ID` / `NINE_<PROVIDER>_CLIENT_SECRET` / `oauth-specs.json` win).

Full generic flow (Verified): start + PKCE + state + exchange + refresh + import + logout:
- `codex` — authcode PKCE, fixedPort 1455, extra codex_cli params; form refresh with scope
- `claude` — authcode PKCE; JSON refresh without secret (upstream REFRESH_PROFILES)
- `grok-cli` / `xai` — device flow; public client id; standard refresh
- `kimi` / `kimi-coding` — device flow (`/code/authorize_device`); public client id
- `iflow` — authcode; public client id; secret env-only; Basic-auth refresh; phone extra params
- `github` / `copilot` — device+web flow; public client id; secret iff configured
- `gemini-cli` / `gemini` — Google authcode (no PKCE, `access_type=offline`+`prompt=consent`)
- `antigravity` — Google authcode with cclog/experimentsandconfigs scopes
- `gitlab` — authcode PKCE, `api read_user`
- `cline` / `clinepass` — base64 token-code exchange (no client needed); JSON fallback POST;
  JSON refresh against `.../auth/refresh`; non-stream envelope unwrap in gateway

Partial (documented limits, 501s where interactive polling is required):
- `qoder` — real device page URL; custom PKCE+nonce polling deferred
- `kiro` — social-login spec present; AWS OIDC device flow deferred
- `cursor` — import-token only by design (no browser endpoints upstream either)
- `kimchi` — browser-token flow (`/cli-auth?callback=&state=`); stored via import-token

Blocked (spec present for discovery; flows 501 with reason per no-fake rule):
- `codebuddy-cn` / `codebuddy-intl` — custom state-POST + GET-poll device flow
- `kilocode` — custom device flow, no refresh upstream either
- `zed` — RSA native-app flow (needs `rsa` crate + local keypair loopback)
- `xiaomi-mimo` — X25519+AES-GCM callback encryption (needs crypto deps)

Endpoints implemented & tested:
- `GET /api/oauth/:provider` — authorize URL (+ custom Cline/Kimchi shapes); 501 for
  custom-crypto (zed, xiaomi-mimo) and import-only/device flows without a browser URL
- `GET /api/oauth/callback?code=&state=` — exchange incl. Cline base64 + email capture
- `POST /api/oauth/:provider/exchange` — headless exchange incl. Cline base64/JSON
- `POST /api/oauth/:provider/refresh` — per-provider shape (Claude JSON, Cline JSON+URL,
  iFlow Basic, Codex scope); 501 when no standard endpoint exists
- `POST /api/oauth/:provider/import-token` — token import (covers cursor/kimchi/trae/windsurf)
- `POST /api/oauth/:provider/api-key` — manual key connection
- `POST /api/oauth/:provider/logout` — delete connection row
- Interactive device-polling stays deferred (matrix `deferredOrUnsupported`), 501 by design.

## Interactive browser/device OAuth (implemented)

Mirrors `src/app/api/oauth/[provider]/[action]/route.js` + `cli/src/cli/menus/providers.js`:

- `GET /api/oauth/:provider/authorize?redirect_uri=` (+ provider meta passthrough) —
  returns `{state, authUrl, authorizeUrl, redirectUri, codeVerifier, codeChallenge,
  flowType, fixedPort, callbackPath}`; 501 for `xiaomi-mimo` (X25519 crypto) and
  `zed` (RSA flow) with import guidance, matching `start_unsupported_reason`.
- `GET /api/oauth/:provider/device-code` (`start_url`/`region`/`auth_method` for kiro) —
  per-provider request shapes: github form+Accept, grok-cli form+referrer/UA,
  kimi form+`X-Msh-Device-Id`, kilocode JSON initiate, codebuddy state-POST,
  qoder fully local (PKCE+nonce+machineId, no network), kiro OIDC register+authorize
  with `valid_aws_region` SSRF guard. Mock overrides `NINE_DEVICE_MOCK_<PROV>_{CODE,TOKEN,REGISTER}` for offline tests.
- `POST /api/oauth/:provider/poll` (`deviceCode`, `codeVerifier`, `extraData`) —
  normalizes pending/slow_down/expired/denied/complete per provider
  (kilocode 202/403/410, codebuddy `code` 11217/0, qoder 202/404, kiro camelCase,
  kimi 200+`authorization_pending`); on success stores a `providerConnections` row
  and returns `{ok, success, connection}` (both flags for CLI compat).
- Loopback proxies: `GET|POST start-proxy` / `stop-proxy` for
  codex (fixed 1455) / xai (fixed 56121) / trae / windsurf / zed / xiaomi-mimo —
  real 127.0.0.1 listener, `Origin` non-loopback guard, upstream success page.
- `POST register-session` (trae/windsurf/zed/xai), `GET poll-status?state=`,
  `POST manual-code` (xai-only, exchanges with session verifier), `GET ide-status`
  (trae/windsurf only, 400 otherwise).
- Browser callback `GET /api/oauth/callback` serves the success HTML when
  `Accept: text/html`, JSON otherwise.
- CLI `9router-rs login <provider> [--device] [--gateway-url URL] [--no-browser]`:
  authorize→print/open URL→paste callback→exchange, or device-code→print URL/code→
  poll 5s×60. Pure helpers: `nine_oauth::device::{parse_callback_url,
  qoder_challenge, normalize_poll, valid_aws_region}`, cli `poll_next`.

Public installed-app client ids ship as defaults exactly as upstream publishes them;
secrets stay env-only. No credentials in code or fixtures.
