# Security Audit

- Secrets: no hardcoded keys/tokens; tokens live in sqlite providerConnections.data (0600 perms expected); logs redact auth/key/token/cookie headers (core::redact_headers).
- No unwrap/expect/panic in request paths (constructors only in tests).
- Callback server: state CSRF + PKCE S256 required; localhost callback only.
- SSRF: upstream base URLs fixed per provider id; no arbitrary upstream URLs accepted.
- Open improvements: at-rest encryption for providerConnections.data, JWT auth parity with original, rate limiting, request size caps.
