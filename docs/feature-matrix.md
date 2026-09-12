# Feature Matrix

| Feature | Original | Rust module | Tests | Status |
|---|---|---|---|---|
| Launcher CLI flags (--port/--host/--no-browser/--log/--tray/--skip-update/--help/--version) | cli.js | crates/cli | unit+integration | [ ] Discovered |
| `xai video` subcommand | src/cli/commands/xaiVideo.js | crates/cli | integration | [ ] Discovered |
| Health/init/version | /api/health /api/init /api/version | gateway | integration | [ ] Discovered |
| OpenAI chat completions + SSE | /api/v1/chat/completions | gateway+providers | unit+integration+diff | [ ] Discovered |
| Responses API | /api/v1/responses (+/codex,/responses rewrites) | gateway+providers | integration | [ ] Discovered |
| Models list/info | /api/v1/models* | gateway+routing | integration | [ ] Discovered |
| Anthropic messages | /api/v1/messages(+count_tokens) | providers::anthropic | integration+diff | [ ] Discovered |
| Gemini passthrough | /api/v1beta/models* | providers::gemini | integration | [ ] Discovered |
| Embeddings/audio/images/videos/search/web | /api/v1/* | gateway+providers | integration | [ ] Discovered |
| Model alias/combos/routing/fallback/retry/round-robin | /api/models/* /api/combos* | routing | unit+property | [ ] Discovered |
| Provider CRUD/test/validate | /api/providers* /api/provider-nodes /api/proxy-pools | gateway+providers+storage | integration | [ ] Discovered |
| OAuth all providers | /api/oauth/* | oauth+storage | unit+integration | [ ] Discovered |
| Keys/auth (apiKeys, JWT+machine-id, cli-secret) | /api/keys* /api/auth/* | gateway | unit+integration | [ ] Discovered |
| Usage/stats/logs/stream | /api/usage/* | gateway+storage | integration | [ ] Discovered |
| Settings/locale/require-login/db/proxy-test | /api/settings* | gateway+config | integration | [ ] Discovered |
| CLI-tools config writers | /api/cli-tools/* | gateway | integration | [ ] Discovered |
| MCP bridge | /api/mcp/* | gateway | integration | [ ] Discovered |
| TTS voices | /api/media-providers/* | gateway | integration | [ ] Discovered |
| Tunnel/tailscale, headroom, pxpipe, translator, shutdown | /api/tunnel* /api/headroom* /api/pxpipe* /api/translator* /api/shutdown | gateway+cli | integration | [ ] Discovered |
| MITM antigravity intercept | app/src/mitm/server.js | gateway::mitm (documented; separate privileged helper) | unit | [ ] Discovered |
| Persistence 11 tables + migrations + backup | lib/db | storage (rusqlite) | unit+integration | [ ] Discovered |
| Background refresh/quota ping/updater | sse/lib services | gateway tasks | integration | [ ] Discovered |
