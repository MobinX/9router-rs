use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenRecord {
    pub provider: String,
    pub account_id: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
}

impl TokenRecord {
    pub fn expired(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at - 60
    }
}

pub trait OAuthProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn authorize_url(&self, state: &str, code_challenge: &str) -> String;
}

/// Generic OAuth/subscription provider record (PKCE S256 assumed).
pub struct GenericOAuth(pub &'static str, pub &'static str);
impl OAuthProvider for GenericOAuth {
    fn id(&self) -> &'static str {
        self.0
    }
    fn authorize_url(&self, state: &str, code_challenge: &str) -> String {
        format!("{}/oauth/authorize?response_type=code&state={state}&code_challenge={code_challenge}&code_challenge_method=S256", self.1)
    }
}

pub const OAUTH_PROVIDERS: &[&str] = &[
    "codex",
    "cursor",
    "gitlab",
    "grok-cli",
    "iflow",
    "kiro",
    "xiaomi-mimo",
    "qoder",
    "xai",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_with_skew() {
        let t = TokenRecord {
            provider: "codex".into(),
            account_id: "a".into(),
            access_token: "x".into(),
            refresh_token: None,
            expires_at: 1000,
        };
        assert!(!t.expired(900));
        assert!(t.expired(941));
    }
    #[test]
    fn authorize_url_pkce() {
        let g = GenericOAuth("codex", "https://example.com");
        let u = g.authorize_url("st", "ch");
        assert!(u.contains("code_challenge=ch") && u.contains("state=st"));
    }
}
