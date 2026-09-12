use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

// ─── PKCE ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    /// RFC 7636 verifier: 64 chars from the unreserved set, S256 challenge.
    pub fn generate() -> Self {
        let mut verifier = String::new();
        while verifier.len() < 64 {
            verifier.push_str(&uuid::Uuid::new_v4().simple().to_string());
        }
        verifier.truncate(64);
        Self::from_verifier(&verifier)
    }

    pub fn from_verifier(verifier: &str) -> Self {
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        Self {
            verifier: verifier.to_string(),
            challenge,
        }
    }
}

/// A fresh anti-CSRF state value.
pub fn new_state() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// ─── Token records ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenRecord {
    pub provider: String,
    pub account_id: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
}

impl TokenRecord {
    /// Treat a token as expired 60s early to avoid racing the upstream.
    pub fn expired(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at - 60
    }
}

/// Parse a provider token endpoint response. Handles expiring tokens
/// (\`expires_in\`) and non-expiring ones (defaults to +1 year).
pub fn parse_token_response(
    provider: &str,
    account_id: &str,
    v: &Value,
    now_unix: i64,
) -> TokenRecord {
    let access = v
        .get("access_token")
        .or_else(|| v.get("accessToken"))
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    let refresh = v
        .get("refresh_token")
        .or_else(|| v.get("refreshToken"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let expires_in = v
        .get("expires_in")
        .or_else(|| v.get("expiresIn"))
        .and_then(|e| e.as_i64());
    TokenRecord {
        provider: provider.to_string(),
        account_id: account_id.to_string(),
        access_token: access,
        refresh_token: refresh,
        expires_at: now_unix + expires_in.unwrap_or(31_536_000),
        id_token: v
            .get("id_token")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        scope: v.get("scope").and_then(|t| t.as_str()).map(str::to_string),
        token_type: v
            .get("token_type")
            .and_then(|t| t.as_str())
            .map(str::to_string),
    }
}

/// Pick an account to use: prefer non-expired, else the one expiring latest
/// that still has a refresh token.
pub fn pick_account(records: &[TokenRecord], now_unix: i64) -> Option<&TokenRecord> {
    records.iter().find(|r| !r.expired(now_unix)).or_else(|| {
        let mut with_refresh: Vec<&TokenRecord> = records
            .iter()
            .filter(|r| r.refresh_token.is_some())
            .collect();
        with_refresh.sort_by_key(|r| std::cmp::Reverse(r.expires_at));
        with_refresh.into_iter().next()
    })
}

// ─── Provider specs ───────────────────────────────────────────────────────

/// OAuth client metadata. Client ids/secrets are never hardcoded: they are
/// read from the environment or from \`~/.9router/oauth-specs.json\`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthSpec {
    pub provider: String,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub pkce: bool,
    #[serde(default)]
    pub fixed_port: Option<u16>,
    #[serde(default = "default_callback_path")]
    pub callback_path: String,
    #[serde(default)]
    pub device_flow: bool,
    #[serde(default)]
    pub extra_params: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub refresh_lead_ms: i64,
}

fn default_callback_path() -> String {
    "/api/oauth/callback".into()
}

/// Public, non-secret metadata discovered from the upstream bundle.
fn base_spec(provider: &str) -> Option<OAuthSpec> {
    let (authorize_url, token_url, scopes, pkce, fixed_port, callback_path, device_flow) =
        match provider {
            "codex" => (
                "https://auth.openai.com/oauth/authorize",
                "https://auth.openai.com/oauth/token",
                vec!["openid", "profile", "email", "offline_access"],
                true,
                Some(1455),
                "/auth/callback",
                false,
            ),
            "claude" | "anthropic" => (
                "https://claude.ai/oauth/authorize",
                "https://api.anthropic.com/v1/oauth/token",
                vec!["org:create_api_key", "user:profile", "user:inference"],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            "grok-cli" | "xai" => (
                "https://auth.x.ai/oauth2/authorize",
                "https://auth.x.ai/oauth2/token",
                vec![
                    "openid",
                    "profile",
                    "email",
                    "offline_access",
                    "grok-cli:access",
                    "api:access",
                    "conversations:read",
                    "conversations:write",
                ],
                true,
                None,
                "/api/oauth/callback",
                true,
            ),
            "kimi" | "kimi-coding" => (
                "https://www.kimi.com/code/authorize_device",
                "https://auth.kimi.com/api/oauth/token",
                vec!["openid", "profile"],
                true,
                None,
                "/api/oauth/callback",
                true,
            ),
            "iflow" => (
                "https://iflow.cn/oauth",
                "https://iflow.cn/oauth/token",
                vec!["api", "read_user"],
                false,
                None,
                "/api/oauth/callback",
                false,
            ),
            "github" | "copilot" => (
                "https://github.com/login/oauth/authorize",
                "https://github.com/login/oauth/access_token",
                vec!["read:user"],
                false,
                None,
                "/api/oauth/callback",
                true,
            ),
            "gemini" | "gemini-cli" => (
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                vec![
                    "https://www.googleapis.com/auth/cloud-platform",
                    "https://www.googleapis.com/auth/userinfo.profile",
                    "https://www.googleapis.com/auth/userinfo.email",
                ],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            "kiro" => (
                "https://prod.us-east-1.auth.desktop.kiro.dev/login",
                "https://prod.us-east-1.auth.desktop.kiro.dev/oauth/token",
                vec!["openid", "profile", "email", "offline_access"],
                true,
                None,
                "/api/oauth/callback",
                true,
            ),
            "gitlab" => (
                "https://gitlab.com/oauth/authorize",
                "https://gitlab.com/oauth/token",
                vec!["read_user", "api"],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            "cursor" => (
                "https://cursor.com/loginDeepControl",
                "https://cursor.com/api/auth/oauth/token",
                vec!["openid", "profile"],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            "qoder" => (
                "https://qoder.com/oauth/authorize",
                "https://qoder.com/oauth/token",
                vec!["openid", "profile"],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            "xiaomi-mimo" => (
                "https://platform.xiaomimimo.com/authorize",
                "https://platform.xiaomimimo.com/api/oauth/token",
                vec!["openid", "profile"],
                true,
                None,
                "/api/oauth/callback",
                false,
            ),
            _ => return None,
        };
    Some(OAuthSpec {
        provider: provider.to_string(),
        authorize_url: authorize_url.into(),
        token_url: token_url.into(),
        client_id: String::new(),
        client_secret: None,
        scopes: scopes.into_iter().map(str::to_string).collect(),
        pkce,
        fixed_port,
        callback_path: callback_path.into(),
        device_flow,
        extra_params: Default::default(),
        refresh_lead_ms: 0,
    })
}

/// Env var name holding a provider client id/secret.
fn env_suffix(provider: &str) -> String {
    provider.to_uppercase().replace('-', "_")
}

/// Load specs from \`<data_dir>/oauth-specs.json\` (array or map), falling back
/// to the built-in public metadata. Client credentials come from that file or
/// from \`NINE_<PROVIDER>_CLIENT_ID\` / \`NINE_<PROVIDER>_CLIENT_SECRET\`.
pub fn load_specs(data_dir: &str) -> Vec<OAuthSpec> {
    let mut specs: Vec<OAuthSpec> = OAUTH_PROVIDERS
        .iter()
        .filter_map(|p| base_spec(p))
        .collect();
    let path = format!("{data_dir}/oauth-specs.json");
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let entries: Vec<OAuthSpec> = match v {
                Value::Array(a) => a
                    .into_iter()
                    .filter_map(|e| serde_json::from_value(e).ok())
                    .collect(),
                Value::Object(m) => m
                    .into_values()
                    .filter_map(|e| serde_json::from_value(e).ok())
                    .collect(),
                _ => Vec::new(),
            };
            for e in entries {
                if let Some(existing) = specs.iter_mut().find(|s| s.provider == e.provider) {
                    *existing = e;
                } else {
                    specs.push(e);
                }
            }
        }
    }
    for s in specs.iter_mut() {
        let suffix = env_suffix(&s.provider);
        if let Ok(id) = std::env::var(format!("NINE_{suffix}_CLIENT_ID")) {
            if !id.is_empty() {
                s.client_id = id;
            }
        }
        if let Ok(secret) = std::env::var(format!("NINE_{suffix}_CLIENT_SECRET")) {
            if !secret.is_empty() {
                s.client_secret = Some(secret);
            }
        }
    }
    specs
}

pub fn spec_for<'a>(specs: &'a [OAuthSpec], provider: &str) -> Option<&'a OAuthSpec> {
    let p = match provider {
        "anthropic" => "claude",
        "xai" => "grok-cli",
        other => other,
    };
    specs.iter().find(|s| s.provider == p)
}

impl OAuthSpec {
    /// Build the authorization (or device-authorization) URL.
    pub fn authorize_url(&self, state: &str, pkce: Option<&Pkce>, redirect_uri: &str) -> String {
        let mut params: Vec<(String, String)> = vec![
            ("client_id".into(), self.client_id.clone()),
            ("redirect_uri".into(), redirect_uri.to_string()),
            ("response_type".into(), "code".into()),
            ("state".into(), state.to_string()),
        ];
        if !self.scopes.is_empty() {
            params.push(("scope".into(), self.scopes.join(" ")));
        }
        if let Some(p) = pkce {
            params.push(("code_challenge".into(), p.challenge.clone()));
            params.push(("code_challenge_method".into(), "S256".into()));
        }
        for (k, v) in &self.extra_params {
            params.push((k.clone(), v.clone()));
        }
        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let sep = if self.authorize_url.contains('?') {
            '&'
        } else {
            '?'
        };
        format!("{}{sep}{query}", self.authorize_url)
    }

    fn form_params(
        &self,
        grant_type: &str,
        code: Option<&str>,
        redirect_uri: &str,
        verifier: Option<&str>,
        refresh_token: Option<&str>,
        device_code: Option<&str>,
    ) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = vec![
            ("grant_type".into(), grant_type.to_string()),
            ("client_id".into(), self.client_id.clone()),
        ];
        if let Some(s) = &self.client_secret {
            params.push(("client_secret".into(), s.clone()));
        }
        if let Some(c) = code {
            params.push(("code".into(), c.to_string()));
            params.push(("redirect_uri".into(), redirect_uri.to_string()));
        }
        if let Some(v) = verifier {
            params.push(("code_verifier".into(), v.to_string()));
        }
        if let Some(r) = refresh_token {
            params.push(("refresh_token".into(), r.to_string()));
        }
        if let Some(d) = device_code {
            params.push(("device_code".into(), d.to_string()));
        }
        params
    }

    pub fn device_authorize_body(&self) -> Vec<(String, String)> {
        let mut params = vec![("client_id".into(), self.client_id.clone())];
        if !self.scopes.is_empty() {
            params.push(("scope".into(), self.scopes.join(" ")));
        }
        params
    }

    pub fn exchange_body(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce: Option<&Pkce>,
    ) -> Vec<(String, String)> {
        self.form_params(
            "authorization_code",
            Some(code),
            redirect_uri,
            pkce.map(|p| p.verifier.as_str()),
            None,
            None,
        )
    }

    pub fn refresh_body(&self, refresh_token: &str) -> Vec<(String, String)> {
        self.form_params("refresh_token", None, "", None, Some(refresh_token), None)
    }

    pub fn device_token_body(&self, device_code: &str) -> Vec<(String, String)> {
        self.form_params(
            "urn:ietf:params:oauth:grant-type:device_code",
            None,
            "",
            None,
            None,
            Some(device_code),
        )
    }
}

/// Encode params as `application/x-www-form-urlencoded`.
pub fn form_encode(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ─── Device flow helpers ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

pub fn parse_device_code(v: &Value) -> Option<DeviceCode> {
    Some(DeviceCode {
        device_code: v.get("device_code")?.as_str()?.to_string(),
        user_code: v.get("user_code")?.as_str()?.to_string(),
        verification_uri: v
            .get("verification_uri")
            .or_else(|| v.get("verification_url"))
            .and_then(|u| u.as_str())
            .unwrap_or_default()
            .to_string(),
        expires_in: v.get("expires_in").and_then(|e| e.as_i64()).unwrap_or(900),
        interval: v.get("interval").and_then(|e| e.as_i64()).unwrap_or(5),
    })
}

/// Device-flow polling state: pending until the provider stops saying
/// \`authorization_pending\`/\`slow_down\`.
pub fn device_poll_status(v: &Value) -> DevicePoll {
    match v.get("error").and_then(|e| e.as_str()).unwrap_or_default() {
        "authorization_pending" => DevicePoll::Pending,
        "slow_down" => DevicePoll::SlowDown,
        "expired_token" => DevicePoll::Expired,
        "access_denied" => DevicePoll::Denied,
        _ => {
            if v.get("access_token").is_some() {
                DevicePoll::Complete
            } else {
                DevicePoll::Pending
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePoll {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Complete,
}

pub const OAUTH_PROVIDERS: &[&str] = &[
    "codex",
    "claude",
    "grok-cli",
    "kimi",
    "iflow",
    "github",
    "gemini",
    "kiro",
    "gitlab",
    "cursor",
    "qoder",
    "xiaomi-mimo",
];

pub fn has_provider(provider: &str) -> bool {
    spec_for(
        &OAUTH_PROVIDERS
            .iter()
            .filter_map(|p| base_spec(p))
            .collect::<Vec<_>>(),
        provider,
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pkce_s256_rfc7636_vector() {
        // RFC 7636 Appendix B
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_pkce_is_well_formed() {
        let p = Pkce::generate();
        assert_eq!(p.verifier.len(), 64);
        assert!(p
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(
            !p.challenge.contains('='),
            "S256 challenge must be unpadded"
        );
        assert_ne!(
            Pkce::generate().verifier,
            p.verifier,
            "verifiers must be unique"
        );
    }

    #[test]
    fn state_is_unique() {
        assert_ne!(new_state(), new_state());
    }

    #[test]
    fn expiry_with_skew() {
        let t = TokenRecord {
            provider: "codex".into(),
            account_id: "a".into(),
            access_token: "x".into(),
            refresh_token: None,
            expires_at: 1000,
            id_token: None,
            scope: None,
            token_type: None,
        };
        assert!(!t.expired(900));
        assert!(t.expired(941));
    }

    #[test]
    fn parses_expiring_and_static_tokens() {
        let now = 1_000_000;
        let t = parse_token_response(
            "codex",
            "acct",
            &json!({"access_token": "at", "refresh_token": "rt", "expires_in": 3600}),
            now,
        );
        assert_eq!(t.expires_at, now + 3600);
        assert_eq!(t.refresh_token.as_deref(), Some("rt"));
        let static_t = parse_token_response("iflow", "a2", &json!({"access_token": "at2"}), now);
        assert!(static_t.expires_at > now);
        assert!(static_t.refresh_token.is_none());
    }

    #[test]
    fn picks_fresh_then_refreshable_account() {
        let now = 1000;
        let fresh = TokenRecord {
            provider: "codex".into(),
            account_id: "fresh".into(),
            access_token: "a".into(),
            refresh_token: None,
            expires_at: 5000,
            id_token: None,
            scope: None,
            token_type: None,
        };
        let stale = TokenRecord {
            provider: "codex".into(),
            account_id: "stale".into(),
            access_token: "b".into(),
            refresh_token: Some("rt".into()),
            expires_at: 100,
            id_token: None,
            scope: None,
            token_type: None,
        };
        assert_eq!(
            pick_account(&[stale.clone(), fresh.clone()], now)
                .unwrap()
                .account_id,
            "fresh"
        );
        assert_eq!(pick_account(&[stale], now).unwrap().account_id, "stale");
        assert!(pick_account(&[], now).is_none());
    }

    #[test]
    fn build_authorize_url_with_pkce() {
        let mut spec = base_spec("codex").unwrap();
        spec.client_id = "cid".into();
        let pkce = Pkce::from_verifier(
            "verifierverifierverifierverifierverifierverifierverifierverifierverifier",
        );
        let url = spec.authorize_url("st8", Some(&pkce), "http://127.0.0.1:1455/auth/callback");
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("state=st8"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("response_type=code"));
    }

    #[test]
    fn build_exchange_and_refresh_bodies() {
        let mut spec = base_spec("codex").unwrap();
        spec.client_id = "cid".into();
        let pkce = Pkce::from_verifier("v");
        let body = spec.exchange_body("code123", "http://cb", Some(&pkce));
        let map: std::collections::HashMap<_, _> = body.into_iter().collect();
        assert_eq!(map["grant_type"], "authorization_code");
        assert_eq!(map["code"], "code123");
        assert_eq!(map["code_verifier"], "v");
        let r = spec
            .refresh_body("rt")
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(r["grant_type"], "refresh_token");
        assert_eq!(r["refresh_token"], "rt");
    }

    #[test]
    fn form_encoding_roundtrip_shape() {
        let body = form_encode(&[
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), "a b&c".into()),
            (
                "redirect_uri".into(),
                "http://127.0.0.1:1455/auth/callback".into(),
            ),
        ]);
        assert_eq!(
            body,
            "grant_type=authorization_code&code=a%20b%26c&redirect_uri=http%3A%2F%2F127.0.0.1%3A1455%2Fauth%2Fcallback"
        );
    }
    #[test]
    fn provider_aliases_resolve() {
        let specs = load_specs("/nonexistent-dir-xyz");
        assert!(spec_for(&specs, "anthropic").is_some());
        assert!(spec_for(&specs, "xai").is_some());
        assert!(spec_for(&specs, "codex").is_some());
        assert!(spec_for(&specs, "nope").is_none());
        // no hardcoded secrets ever
        for s in &specs {
            assert!(
                s.client_secret.is_none(),
                "spec {} must not embed a secret",
                s.provider
            );
        }
    }

    #[test]
    fn device_code_parsing_and_polling() {
        let dc = parse_device_code(&json!({
            "device_code": "dc", "user_code": "ABCD-EFGH",
            "verification_uri": "https://x/device", "expires_in": 600, "interval": 5
        }))
        .unwrap();
        assert_eq!(dc.user_code, "ABCD-EFGH");
        assert_eq!(
            device_poll_status(&json!({"error": "authorization_pending"})),
            DevicePoll::Pending
        );
        assert_eq!(
            device_poll_status(&json!({"error": "slow_down"})),
            DevicePoll::SlowDown
        );
        assert_eq!(
            device_poll_status(&json!({"access_token": "t"})),
            DevicePoll::Complete
        );
        assert_eq!(
            device_poll_status(&json!({"error": "expired_token"})),
            DevicePoll::Expired
        );
    }

    #[test]
    fn specs_load_from_file_and_env() {
        let dir = std::env::temp_dir().join(format!("nine-oauth-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("oauth-specs.json"),
            r#"[{"provider":"codex","authorize_url":"https://a","token_url":"https://t","client_id":"file-cid","pkce":true}]"#,
        )
        .unwrap();
        std::env::set_var("NINE_IFLOW_CLIENT_ID", "env-cid");
        let specs = load_specs(dir.to_str().unwrap());
        assert_eq!(spec_for(&specs, "codex").unwrap().client_id, "file-cid");
        assert_eq!(spec_for(&specs, "iflow").unwrap().client_id, "env-cid");
        std::env::remove_var("NINE_IFLOW_CLIENT_ID");
        std::fs::remove_dir_all(&dir).ok();
    }
}
