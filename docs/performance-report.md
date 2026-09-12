# Performance Report

Status: correctness phase; no benchmarks yet. Plan: k6-style concurrency (100/1000 reqs), SSE overhead, routing overhead vs original. Stack: tokio + axum + hyper, rusqlite bundled. ponytail: connection pooling/tuning deferred until differential suite passes.
