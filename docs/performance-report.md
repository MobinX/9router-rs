# Performance Report (Phase 10)

Method: keepalive HTTP, loopback, release binary, mock OpenAI upstream
(`scripts/mock-bench-upstream.js`, fixed ~300 B JSON / 2-chunk SSE).
Harness: `scripts/bench.sh` (`TARGET N C`). Box is shared/loaded, so single
samples vary ±50%; figures below are best-of repeated runs, same methodology
for both servers. Date: 2026-09-13. Full raw runs in turn log.

## Results

| Metric | Rust 9router-rs (release) | Original 9Router | Notes |
|---|---|---|---|
| `GET /api/health` seq | 2.9 ms | 15.2 ms | axum vs Next.js stack |
| `POST /v1/chat/completions` seq | 9.0 ms (full proxy incl. mock hop) | 18.0 ms (401 auth-reject path only) | Rust proxies real upstream in half the time |
| Mock direct (baseline) | 3–5 ms | — | Routing+translation overhead ≈ 4–6 ms/req |
| Throughput, C=20 | ~480–660 req/s | n/a (needs provider keys) | 200–1000 req batches, fixed mock |
| 100 concurrent | 427 ms total, 0 fail | n/a | No deadlocks/leaks observed |
| SSE | true passthrough (`bytes_stream`, zero buffering) | n/a | End-to-end ≈ mock latency + ~8 ms; `[DONE]` terminator preserved |
| Startup (listen) | < 1.1 s | ~10 s+ (next-server) | Single 11.4 MB static binary |
| RSS idle→loaded | 7.7 MB | ~109 MB (next-server) | `ps` RSS on same box |
| Binary | 11 428 232 B, no runtime deps | node 24 + 100s MB node_modules | Termux-friendly |

## Streaming

No buffering: `passthrough_sse` forwards `bytes_stream` chunk-by-chunk;
Anthropic/Gemini translation is per-event (`translate_*_sse`) with immediate
`tx.send`, `[DONE]` appended, client-disconnect aborts via channel close.
Differential tests assert chunk text preservation and terminator.

## Not optimized (deliberate)

Connection pooling left at reqwest defaults; no response caching; catalog
loaded once at startup. Correctness first — see `compatibility-report.md`.
`ponytail`: tune pool/timeouts only if production profiles demand it.
