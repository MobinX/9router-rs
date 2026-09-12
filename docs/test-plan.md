# Test Plan (pyramid)

Unit → component → integration (axum oneshot + mock upstream) → differential (vs original, normalized) → e2e (serve + CLI). Commands: `cargo test --workspace`, `cargo test --all-features`, clippy `-D warnings`, `cargo fmt --check`; scripts `test.sh test-integration.sh test-differential.sh test-providers.sh test-oauth.sh`. Property tests: routing/config/model-id parsing (proptest). Fuzz targets documented for JSON/SSE/OAuth-callback/model-id parsing (cargo-fuzz, manual).
