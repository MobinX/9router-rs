# Feature Coverage Matrix (Machine-Readable Verified)

**Last Updated:** 2026-09-12T18:31:13.894Z  
**Claimed & Fully Tested:** 32 / 32 features  
**All claimed features have automated unit/integration tests attached.** Discovered-only items are explicitly documented as deferred below.

## Verified Features

| Feature ID | Name | Public Interface | Rust Implementation | Test Proof | Status |
|---|---|---|---|---|---|
| `gateway.health` | Health Endpoint | `GET /api/health` | `nine_gateway::health` | `crates/gateway/src/lib.rs::tests::health_ok` | **Verified** |
| `gateway.version` | Version Endpoint | `GET /api/version` | `nine_gateway::version` | `crates/gateway/src/lib.rs::tests::version` | **Verified** |
| `gateway.models.list` | OpenAI Models List | `GET /v1/models, GET /api/v1/models` | `nine_gateway::models` | `crates/gateway/src/lib.rs::tests::models_public_list` | **Verified** |
| `gateway.models.info` | Model Info by ID | `GET /v1/models/info, GET /api/v1/models/info` | `nine_gateway::model_info` | `crates/gateway/src/lib.rs::tests::model_info_requires_id` | **Verified** |
| `gateway.v1beta.models` | Gemini Models List | `GET /v1beta/models` | `nine_gateway::gemini_models` | `crates/gateway/src/lib.rs::tests::gemini_models_shape` | **Verified** |
| `gateway.chat.completions.json` | Chat Completions JSON | `POST /v1/chat/completions` | `nine_gateway::chat_completions` | `crates/gateway/tests/proxy.rs::non_stream_passthrough_shape` | **Verified** |
| `gateway.chat.completions.sse` | Chat Completions SSE Stream | `POST /v1/chat/completions (stream=true)` | `nine_gateway::passthrough_sse, translate_sse_stream` | `crates/gateway/tests/proxy.rs::sse_bytes_passthrough` | **Verified** |
| `gateway.messages` | Anthropic Messages Native & Stream | `POST /v1/messages` | `nine_gateway::messages` | `crates/gateway/tests/adapters.rs::messages_endpoint_passthrough` | **Verified** |
| `gateway.gemini_generate` | Gemini Native generateContent | `POST /v1beta/models/*:generateContent` | `nine_gateway::gemini_generate` | `crates/gateway/tests/adapters.rs::gemini_generate_content_passthrough` | **Verified** |
| `gateway.auth` | Multi-Credential Authentication | `Authorization: Bearer, x-api-key, x-goog-api-key, ?key=` | `nine_gateway::authorize` | `crates/gateway/src/lib.rs::tests::auth_missing_key_401_code` | **Verified** |
| `routing.alias.chain` | Model Alias Multi-Step Chain Resolution | `Internal & Request Routing` | `nine_routing::resolve_alias_chain` | `crates/routing/src/lib.rs::tests::alias_single_and_chain` | **Verified** |
| `routing.alias.cycle_protection` | Model Alias Cycle Protection | `Internal` | `nine_routing::resolve_alias_chain` | `crates/routing/src/lib.rs::tests::alias_cycle_protection` | **Verified** |
| `routing.alias.api` | Model Alias CRUD API | `GET/PUT/DELETE /api/models/alias` | `nine_gateway::get_model_aliases, put_model_alias, delete_model_alias` | `crates/storage/src/lib.rs::tests::model_alias_crud_lifecycle` | **Verified** |
| `routing.combos.expansion` | Combo Virtual Model Expansion | `Internal & POST /v1/chat/completions` | `nine_routing::expand_combo` | `crates/routing/src/lib.rs::tests::combo_expansion_rules` | **Verified** |
| `routing.combos.api` | Combo CRUD API | `GET/POST /api/combos, GET/PUT/DELETE /api/combos/:id` | `nine_gateway::list_combos, create_combo, get_combo, update_combo, delete_combo` | `crates/storage/src/lib.rs::tests::combo_crud_lifecycle` | **Verified** |
| `routing.combos.sticky_round_robin` | Combo Sticky Round-Robin | `Internal` | `nine_routing::StickyRoundRobin` | `crates/routing/src/lib.rs::tests::sticky_round_robin_sequence` | **Verified** |
| `routing.multimodal_filtering` | Combo Multimodal Capability Filter | `Internal` | `nine_routing::filter_by_capability` | `crates/routing/src/lib.rs::tests::multimodal_filtering` | **Verified** |
| `routing.priority` | Priority-Based Connection Selection | `Internal` | `nine_routing::pick_connection` | `crates/routing/src/lib.rs::tests::priority_skips_unhealthy` | **Verified** |
| `routing.weighted` | Weighted Connection Selection | `Internal` | `nine_routing::pick_weighted` | `crates/routing/src/lib.rs::tests::weighted_selection` | **Verified** |
| `routing.fallback_backoff` | Exponential Backoff & Fallback Evaluator | `Internal & Request Error Loop` | `nine_routing::evaluate_fallback, backoff_cooldown_ms` | `crates/routing/src/lib.rs::tests::fallback_evaluation_scenarios` | **Verified** |
| `oauth.pkce` | RFC 7636 PKCE S256 | `Internal` | `nine_oauth::Pkce` | `crates/oauth/src/lib.rs::tests::pkce_s256_rfc7636_vector` | **Verified** |
| `oauth.start` | OAuth Start (PKCE + CSRF State) | `GET /api/oauth/:provider` | `nine_gateway::oauth_start` | `crates/gateway/tests/oauth.rs::start_returns_authorize_url_with_pkce_and_state` | **Verified** |
| `oauth.callback` | OAuth Code Exchange & Connection Storage | `GET /api/oauth/callback` | `nine_gateway::oauth_callback` | `crates/gateway/tests/oauth.rs::callback_exchanges_code_and_stores_connection` | **Verified** |
| `oauth.refresh` | OAuth Token Refresh & Rotation | `POST /api/oauth/:provider/refresh` | `nine_gateway::oauth_action(refresh)` | `crates/gateway/tests/oauth.rs::refresh_rotates_tokens` | **Verified** |
| `oauth.import_and_logout` | OAuth Token Import & Logout | `POST /api/oauth/:provider/import-token, /logout` | `nine_gateway::oauth_action(import-token, logout)` | `crates/gateway/tests/oauth.rs::missing_code_400_and_import_token_and_logout` | **Verified** |
| `storage.schema` | 11-Table SQLite Schema Bootstrap | `Database Migration` | `nine_storage::migrate` | `crates/storage/src/lib.rs::tests::schema_bootstraps` | **Verified** |
| `storage.connections` | Provider Connections Storage & Ordering | `Internal & GET /api/providers` | `nine_storage::Store::upsert_connection, list_connections` | `crates/storage/src/lib.rs::tests::connection_roundtrip_and_ordering` | **Verified** |
| `storage.usage` | Usage Logging & Aggregation | `Internal & GET /api/usage/stats` | `nine_storage::Store::record_usage, usage_totals` | `crates/storage/src/lib.rs::tests::usage_totals_accumulate` | **Verified** |
| `gateway.responses` | Responses API Wire Translation | `POST /v1/responses, POST /codex/*` | `nine_gateway::responses` | `crates/gateway/src/lib.rs::tests::responses_api_validation_and_shape` | **Verified** |
| `cli_tools.all_statuses` | CLI Tools Status Inspection | `GET /api/cli-tools/all-statuses` | `nine_gateway::cli_tools_all_statuses` | `crates/gateway/tests/phase7.rs::cli_tools_all_statuses_returns_all_tools` | **Verified** |
| `cli_tools.settings_crud` | CLI Tools Settings Inspection & Application | `GET/POST/DELETE /api/cli-tools/:tool` | `nine_gateway::get_cli_tool, post_cli_tool, delete_cli_tool` | `crates/gateway/tests/phase7.rs::cli_tools_individual_get_and_post` | **Verified** |
| `gateway.shutdown` | Production Shutdown Policy | `POST /api/shutdown` | `nine_gateway::shutdown` | `crates/gateway/tests/phase7.rs::shutdown_endpoint_matches_production_policy` | **Verified** |

## Deferred / Unsupported Features (No Fake Implementation)

| Feature | Reason |
|---|---|
| **MITM TLS Interception** | Requires local CA certificate injection and OS root trust; implemented as separate external mitm helper in upstream 9Router. |
| **Tunnel Integration (Cloudflared / Tailscale)** | Requires external native daemons (`cloudflared`, `tailscale`) managed via platform subprocesses. |
| **Interactive Browser-Loopback / Device-Polling OAuth** | Requires interactive user browser UI interaction; endpoints explicitly return 501 per no-fake rule. |
