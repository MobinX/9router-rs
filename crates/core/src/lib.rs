use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("upstream {0}: {1}")]
    Upstream(u16, String),
    #[error("auth expired: {0}")]
    AuthExpired(String),
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn new_request_id() -> String {
    format!("req_{}", uuid::Uuid::new_v4().simple())
}

pub fn normalize_model_id(raw: &str) -> String {
    let s = raw.trim().to_lowercase().replace('_', "-");
    let s = s.strip_prefix("models/").unwrap_or(&s);
    s.to_string()
}

pub fn redact_headers(headers: &serde_json::Value) -> serde_json::Value {
    match headers {
        serde_json::Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, v) in m {
                let kl = k.to_lowercase();
                if kl.contains("auth")
                    || kl.contains("key")
                    || kl.contains("token")
                    || kl == "cookie"
                    || kl == "set-cookie"
                {
                    out.insert(k.clone(), serde_json::Value::String("[REDACTED]".into()));
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            serde_json::Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Split "provider/model" syntax used by 9Router routing.
pub fn split_provider_model(s: &str) -> (Option<&str>, &str) {
    match s.split_once('/') {
        Some((p, m)) if !p.is_empty() && !m.is_empty() && !m.contains('/') => (Some(p), m),
        _ => (None, s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_ids_normalize() {
        assert_eq!(normalize_model_id("Models/GPT_4o"), "gpt-4o");
        assert_eq!(normalize_model_id(" claude-sonnet "), "claude-sonnet");
    }
    #[test]
    fn provider_model_split() {
        assert_eq!(
            split_provider_model("openai/gpt-4o"),
            (Some("openai"), "gpt-4o")
        );
        assert_eq!(split_provider_model("gpt-4o"), (None, "gpt-4o"));
        assert_eq!(split_provider_model("a/b/c"), (None, "a/b/c"));
    }
    #[test]
    fn headers_redacted() {
        let h =
            serde_json::json!({"authorization": "Bearer x", "content-type": "application/json"});
        let r = redact_headers(&h);
        assert_eq!(r["authorization"], "[REDACTED]");
        assert_eq!(r["content-type"], "application/json");
    }
}
