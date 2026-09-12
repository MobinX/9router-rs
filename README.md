# 9router-rs

Rust reimplementation of 9Router (behavioral reference: npm 9router@0.5.75).

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo run -p nine-cli -- serve --port 20128
```

Docs: docs/architecture.md, api-matrix.md, provider-matrix.md, oauth-matrix.md, feature-matrix.md, configuration-matrix.md, compatibility-plan.md, test-plan.md, compatibility-report.md, security-audit.md, performance-report.md.
