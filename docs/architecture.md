# 9Router Architecture (reverse-engineered v0.5.75)

Source: npm `9router@0.5.75` (global install) + `~/.9router` live state. No public git repo found; npm tarball is behavioral reference.

## Components

- `cli.js` (launcher): parses `--port/-p --host/-H --no-browser/-n --log/-l --tray/-t --skip-update --help --version`, subcommand `xai video`, spawns Next.js standalone server (`app/server.js` → `custom-server.js`), self-heals sqlite/tray runtimes into `~/.9router/runtime`, manages pid/tunnel pid files.
- Gateway: Next.js 16 standalone (`app/.next-cli-build`) serving UI + `/api/*` routes. Rewrites: `/v1/*→/api/v1/*`, `/codex/*→/api/v1/responses`, `/responses→/api/v1/responses`, `/v1beta/*→/api/v1beta/*`.
- MITM proxy (`app/src/mitm/server.js`, bundled): HTTPS intercept for `antigravity` (+ cursor passthrough allowlist), ALPN h2/http1.1, log/tee mode. Separate local port ( pid file `runtime/mitm` ).
- DB: sqlite (`~/.9router/db/data.sqlite`), adapters better-sqlite3/node:sqlite/bun:sqlite/sql.js fallback. Tables: `_meta apiKeys combos kv providerConnections providerNodes proxyPools requestDetails settings usageDaily usageHistory`.
- OAuth services: generic `/api/oauth/[provider]/[action]` + dedicated `codex cursor gitlab grok-cli iflow kiro xiaomi-mimo`, services `qoder xai`, provider `xiaomi-mimo`.
- CLI-tools config endpoints: writes editor/agent configs (claude/codex/copilot/cursor/opencode etc).
- Background jobs: `quotaAutoPing`, `backgroundTokenRefresh`, updater, pxpipe/headroom/tunnel managers.

## Request path (LLM)

Client (OpenAI/Anthropic/Gemini SDK) → `Authorization: Bearer <9router key>` → `/api/v1/*` → auth (apiKeys table / cli-secret / JWT+machine-id) → model alias resolve → combos/routing (priority/fallback) → providerConnection select → provider adapter (transform req/headers) → upstream (reqwest) → transform resp/SSE → usageHistory/requestDetails log → client.

## Rust mapping

- `crates/core`: error hierarchy, ids, sse types.
- `crates/config`: env+file+cli settings.
- `crates/storage`: rusqlite schema mirroring 11 tables.
- `crates/providers`: `Provider` trait + openai/anthropic/gemini passthrough adapters + registry (143 ids from `app/public/providers`).
- `crates/oauth`: `OAuthProvider` trait + codex/cursor/gitlab/grok-cli/iflow/kiro/xiaomi-mimo/qoder/xai stubs with full token-refresh state machine.
- `crates/routing`: alias/priority/fallback/round-robin.
- `crates/gateway`: axum router reproducing `/api/*`, `/v1/*`, `/v1beta/*` surface (full list in api-matrix).
- `crates/cli`: `9router-rs` binary (serve/test physicians).
