# Security Audit (Phase 9) — verified against source

Scope: `crates/{gateway,providers,oauth,storage,core,routing,config,cli}`.
Method: code inspection + regression tests. Original 9Router bundle cross-checked.

## Verified controls

| Vector | Status | Evidence |
|---|---|---|
| Secret storage | PASS | Tokens in sqlite `providerConnections.data`; `Store::open` chmods db `0600` on unix (`db_file_is_owner_only` test) |
| Secret logging | PASS | Gateway emits no request/response/header/body logs at all; `core::redact_headers` available + unit-tested for any future logging |
| Upstream error leakage | PASS | `map_upstream_err` extracts only `/error/message`, truncated to 1000 chars; reqwest error strings contain URL only, keys travel as headers |
| Auth model | PASS | Missing key -> `Missing API key`, bad key -> `Invalid API key`, both 401 `authentication_error/invalid_api_key` (differential-tested vs original) |
| Request IDs | PASS | `x-request-id` on all chat/messages/responses paths incl. 401 rejects |
| SSRF / arbitrary upstream | PASS | Upstream base URLs come only from server-side `Upstream` config; model->provider mapping fixed in `providers::base_url_for`; public egress gated by `NINE_ALLOW_DIRECT=1`; no endpoint accepts upstream URL from client |
| OAuth CSRF | PASS | `state` required, single-use (deleted after exchange), 10-min TTL (`expired_state_rejected_and_purged` test) |
| OAuth PKCE | PASS | S256 challenge/verifier generated per flow, verifier stored server-side, never sent to client |
| OAuth error reflection | PASS | Provider `error` query param capped at 200 chars (`provider_error_text_capped` test) |
| OAuth secrets | PASS | Client IDs from env/`oauth-specs.json` only; no hardcoded secrets; `id_token` claims used for display (account/email) only, never auth decisions |
| Callback server | PASS | `callback_base` defaults to `http://127.0.0.1:20128` (loopback); no open redirect (fixed `callback_path` per spec) |
| Path traversal | PASS | `Path(tool)` only selects fixed match arms; file paths built from `HOME` + fixed suffixes; combo IDs are sqlite keys, never paths |
| Body limits / DoS | PASS | axum 0.7 `Bytes`/`Json` extractors enforce default 2 MB cap; per-request upstream timeout (`timeout_ms`); SSE streams bounded passthrough; upstream errors truncated |
| Shutdown | PASS | `/api/shutdown` returns 403 in production, matching original |
| Unsafe code | PASS | No `unsafe` blocks; serde-only deserialization of untrusted input; `unwrap`/`expect` absent from non-test request paths (response-builder calls use generated header-safe IDs; `normalize_model_id` uses `unwrap_or`) |

## Accepted gaps (match original behavior)

- No at-rest encryption for `providerConnections.data` (sqlite file perms only); original same. Upgrade path: SQLCipher.
- No gateway rate limiting; original (local-first) same. Upgrade path: `tower_governor`.
- No JWT signature verification on provider `id_token`s; claims used for display only.

## Out of scope / not applicable

- Command injection: no shell execution paths; CLI writes only TOML/JSON via `std::fs`.
- Request smuggling: single axum/hyper HTTP/1.1 stack, no manual framing.
- CSRF on management APIs: local-first tool, same trust model as original.
