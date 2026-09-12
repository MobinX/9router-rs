# Differential Report — Rust vs original 9Router (0.5.75)

Harness: `crates/testing` (`normalize`, `shape`, `parse_sse`, `sse_event_types`)
+ `crates/gateway/tests/differential.rs` (offline, mock upstreams, recorded
original fixtures) + `scripts/test-differential.sh` (live dual-server).

Volatile fields normalized before compare: `id created timestamp request_id
requestId etag syncedAt session_id nonce state code_verifier traceId`,
plus `req_* resp_* chatcmpl-* msg_* call_*` substrings.

## Findings fixed by differential testing

| # | Delta | Evidence | Fix |
|---|-------|----------|-----|
| 1 | Translated Anthropic/Gemini chat.completion missing `created` | shape diff vs recorded original (`{id object created model choices usage}` in `318.js`/`8895.js`) | `translate_anthropic_response`, `translate_gemini_response` now set `created: Utc::now()` |
| 2 | Translated SSE chunks missing `created` | original chunk builder `{id object:"chat.completion.chunk" created model choices}` (`32271`) | `translate_anthropic_sse`, `translate_gemini_sse` now set `created` |
| 3 | 401 auth rejects lacked `x-request-id` | `diff_auth_missing_key_shape` failed `contains_key("x-request-id")` | all 4 `authorize` early-returns wrapped in `with_id` |
| 4 | Fixture corrections (test-side, impl already correct) | original returns `{ok:true}` for `/api/health` (route bundle); `"Missing API key"` vs `"Invalid API key"` distinction (`requireApiKey` branch) | fixtures updated |

## Offline differential cases (all in `differential.rs`)

- auth missing/bad key: status 401 + exact normalized envelope + `x-request-id`
- OpenAI passthrough: response shape + normalized choices equal recorded original
- Anthropic translation: shape equal incl. `created`; content passthrough; `finish_reason stop`
- OpenAI SSE: event sequence `["message","[DONE]"]`, chunk `choices[0].delta.content`
- Anthropic SSE: all translated chunks parse, carry `choices`, joined deltas preserve text, `[DONE]` terminator
- Upstream 4xx envelope preserved verbatim (message passthrough)
- `/v1/responses`: `object:"response"` + `output[]` contains translated text
- `/api/health` `{ok:true}`, `/api/version` key set

## Live dual-server (`scripts/test-differential.sh`)

Compares status + normalized shape for health/version/models, chat (JSON +
SSE event types), responses against `ORIG` and `RUST` servers. Requires both
servers up; skips gracefully when original is down.

## Live results (2026-09-13, ORIG :20128 vs RUST :22128)

6 pass, 0 fail: health, version (exact 3-key shape), models (element shape),
chat 401 gating (both), chat-SSE 401 gating, responses 401 gating.
Unauthenticated chat/responses return 401 on both servers — auth gating parity
confirmed live; authenticated live completion calls need real provider keys
and are covered offline via mock-upstream differential tests above.
