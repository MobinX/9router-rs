use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct DeviceOptions {
    pub kiro_start_url: Option<String>,
    pub kiro_region: Option<String>,
    pub kiro_auth_method: Option<String>,
    pub kimi_device_id: Option<String>,
    pub qoder_nonce: Option<String>,
    pub qoder_verifier: Option<String>,
    pub qoder_machine_id: Option<String>,
    pub kiro_client_id: Option<String>,
    pub kiro_client_secret: Option<String>,
}

impl DeviceOptions {
    pub fn from_map(m: &BTreeMap<String, String>) -> Self {
        let g = |k: &str| m.get(k).cloned();
        Self {
            kiro_start_url: g("startUrl").or_else(|| g("start_url")),
            kiro_region: g("region"),
            kiro_auth_method: g("authMethod").or_else(|| g("auth_method")),
            kimi_device_id: g("deviceId").or_else(|| g("_kimiDeviceId")),
            qoder_nonce: g("nonce").or_else(|| g("deviceCode")),
            qoder_verifier: g("codeVerifier").or_else(|| g("code_verifier")),
            qoder_machine_id: g("machineId").or_else(|| g("machine_id")),
            kiro_client_id: g("_clientId").or_else(|| g("clientId")),
            kiro_client_secret: g("_clientSecret").or_else(|| g("clientSecret")),
        }
    }
    pub fn extra_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        if let Some(v) = &self.kimi_device_id {
            m.insert("_kimiDeviceId".into(), v.clone());
        }
        if let Some(v) = &self.qoder_nonce {
            m.insert("_qoderNonce".into(), v.clone());
        }
        if let Some(v) = &self.qoder_verifier {
            m.insert("_qoderVerifier".into(), v.clone());
        }
        if let Some(v) = &self.qoder_machine_id {
            m.insert("_qoderMachineId".into(), v.clone());
        }
        if let Some(v) = &self.kiro_client_id {
            m.insert("_clientId".into(), v.clone());
        }
        if let Some(v) = &self.kiro_client_secret {
            m.insert("_clientSecret".into(), v.clone());
        }
        if let Some(v) = &self.kiro_region {
            m.insert("_region".into(), v.clone());
        }
        m
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceHttpRequest {
    pub url: String,
    pub method: &'static str,
    pub content_type: &'static str,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

pub fn supports_device(provider: &str) -> bool {
    matches!(
        provider,
        "github"
            | "grok-cli"
            | "kimi"
            | "kimi-coding"
            | "kilocode"
            | "codebuddy-cn"
            | "codebuddy-intl"
            | "qoder"
            | "kiro"
    )
}

pub fn kiro_region_of(opts: &DeviceOptions) -> String {
    opts.kiro_region
        .clone()
        .filter(|r| valid_aws_region(r))
        .unwrap_or_else(|| "us-east-1".into())
}

/// Upstream kiro.js region guard: `/^[a-z]{2}-[a-z]+-\d{1,2}$/`.
pub fn valid_aws_region(r: &str) -> bool {
    let parts: Vec<&str> = r.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    if parts[0].len() != 2 || !parts[0].chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    if parts[1].is_empty() || !parts[1].chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    !parts[2].is_empty() && parts[2].len() <= 2 && parts[2].chars().all(|c| c.is_ascii_digit())
}

fn mock_env(provider: &str, kind: &str) -> Option<String> {
    let key = format!(
        "NINE_DEVICE_MOCK_{}_{}",
        provider.replace('-', "_").to_uppercase(),
        kind.to_uppercase()
    );
    std::env::var(&key).ok().filter(|v| !v.is_empty())
}

pub fn device_code_request(
    provider: &str,
    client_id: &str,
    scope: &str,
    opts: &DeviceOptions,
) -> Option<DeviceHttpRequest> {
    match provider {
        "github" => Some(DeviceHttpRequest {
            url: mock_env(provider, "code")
                .unwrap_or_else(|| "https://github.com/login/device/code".into()),
            method: "POST",
            content_type: "application/x-www-form-urlencoded",
            body: format!(
                "client_id={}&scope={}",
                urlencode(client_id),
                urlencode(scope)
            ),
            headers: vec![("Accept".into(), "application/json".into())],
        }),
        "grok-cli" => Some(DeviceHttpRequest {
            url: mock_env(provider, "code")
                .unwrap_or_else(|| "https://auth.x.ai/oauth2/device/code".into()),
            method: "POST",
            content_type: "application/x-www-form-urlencoded",
            body: format!(
                "client_id={}&scope={}",
                urlencode(client_id),
                urlencode(scope)
            ),
            headers: vec![
                ("Accept".into(), "application/json".into()),
                ("Referer".into(), "grok-build".into()),
                ("User-Agent".into(), "grok-pager/0.2.93".into()),
            ],
        }),
        "kimi" | "kimi-coding" => {
            let did = opts
                .kimi_device_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            Some(DeviceHttpRequest {
                url: mock_env(provider, "code").unwrap_or_else(|| {
                    "https://auth.kimi.com/api/oauth/device_authorization".into()
                }),
                method: "POST",
                content_type: "application/x-www-form-urlencoded",
                body: format!("client_id={}", urlencode(client_id)),
                headers: vec![
                    ("Accept".into(), "application/json".into()),
                    ("X-Msh-Device-Id".into(), did),
                ],
            })
        }
        "kilocode" => Some(DeviceHttpRequest {
            url: mock_env(provider, "code")
                .unwrap_or_else(|| "https://api.kilo.ai/api/device-auth/codes".into()),
            method: "POST",
            content_type: "application/json",
            body: "{}".into(),
            headers: vec![],
        }),
        "codebuddy-cn" => Some(DeviceHttpRequest {
            url: format!(
                "{}?platform=CLI",
                mock_env(provider, "code").unwrap_or_else(|| {
                    "https://copilot.tencent.com/v2/plugin/auth/state".into()
                })
            ),
            method: "POST",
            content_type: "application/json",
            body: "{}".into(),
            headers: vec![
                ("Accept".into(), "application/json".into()),
                ("User-Agent".into(), "CLI/2.63.2 CodeBuddy/2.63.2".into()),
                ("X-Product".into(), "SaaS".into()),
            ],
        }),
        "codebuddy-intl" => Some(DeviceHttpRequest {
            url: format!(
                "{}?platform=CLI",
                mock_env(provider, "code")
                    .unwrap_or_else(|| "https://www.codebuddy.ai/v2/plugin/auth/state".into())
            ),
            method: "POST",
            content_type: "application/json",
            body: "{}".into(),
            headers: vec![
                ("Accept".into(), "application/json".into()),
                ("User-Agent".into(), "CLI/2.63.2 CodeBuddy/2.63.2".into()),
                ("X-Product".into(), "SaaS".into()),
            ],
        }),
        // ponytail: qoder/kiro need multi-step local crypto or client
        // registration; device_code_request returns None and the gateway
        // builds those flows inline. Add when covering custom flows fully.
        _ => None,
    }
}

pub fn device_poll_request(
    provider: &str,
    client_id: &str,
    device_code: &str,
    opts: &DeviceOptions,
) -> Option<DeviceHttpRequest> {
    match provider {
        "github" | "grok-cli" | "kimi" | "kimi-coding" => Some(DeviceHttpRequest {
            url: mock_env(provider, "token").unwrap_or_else(|| match provider {
                "github" => "https://github.com/login/oauth/access_token".into(),
                "grok-cli" => "https://auth.x.ai/oauth2/token".into(),
                _ => "https://auth.kimi.com/api/oauth/token".into(),
            }),
            method: "POST",
            content_type: "application/x-www-form-urlencoded",
            body: format!(
                "grant_type={}&client_id={}&device_code={}",
                urlencode("urn:ietf:params:oauth:grant-type:device_code"),
                urlencode(client_id),
                urlencode(device_code)
            ),
            headers: vec![("Accept".into(), "application/json".into())],
        }),
        "kilocode" => Some(DeviceHttpRequest {
            url: format!(
                "{}/{}",
                mock_env(provider, "token")
                    .unwrap_or_else(|| "https://api.kilo.ai/api/device-auth/codes".into()),
                urlencode(device_code)
            ),
            method: "GET",
            content_type: "application/json",
            body: String::new(),
            headers: vec![],
        }),
        "codebuddy-cn" => Some(DeviceHttpRequest {
            url: format!(
                "{}?state={}",
                mock_env(provider, "token").unwrap_or_else(|| {
                    "https://copilot.tencent.com/v2/plugin/auth/token".into()
                }),
                urlencode(device_code)
            ),
            method: "GET",
            content_type: "application/json",
            body: String::new(),
            headers: vec![("Accept".into(), "application/json".into())],
        }),
        "codebuddy-intl" => Some(DeviceHttpRequest {
            url: format!(
                "{}?state={}",
                mock_env(provider, "token")
                    .unwrap_or_else(|| "https://www.codebuddy.ai/v2/plugin/auth/token".into()),
                urlencode(device_code)
            ),
            method: "GET",
            content_type: "application/json",
            body: String::new(),
            headers: vec![("Accept".into(), "application/json".into())],
        }),
        "qoder" => {
            let nonce = opts
                .qoder_nonce
                .clone()
                .unwrap_or_else(|| device_code.into());
            let verifier = opts.qoder_verifier.clone().unwrap_or_default();
            Some(DeviceHttpRequest {
                url: format!(
                    "{}?nonce={}&verifier={}&challenge_method=S256",
                    mock_env(provider, "token").unwrap_or_else(|| {
                        "https://openapi.qoder.sh/api/v1/deviceToken/poll".into()
                    }),
                    urlencode(&nonce),
                    urlencode(&verifier)
                ),
                method: "GET",
                content_type: "application/json",
                body: String::new(),
                headers: vec![("User-Agent".into(), "Go-http-client/2.0".into())],
            })
        }
        "kiro" => {
            let region = kiro_region_of(opts);
            Some(DeviceHttpRequest {
                url: mock_env(provider, "token")
                    .unwrap_or_else(|| format!("https://oidc.{region}.amazonaws.com/token")),
                method: "POST",
                content_type: "application/json",
                body: serde_json::json!({
                    "clientId": opts.kiro_client_id.clone().unwrap_or_default(),
                    "clientSecret": opts.kiro_client_secret.clone().unwrap_or_default(),
                    "deviceCode": device_code,
                    "grantType": "urn:ietf:params:oauth:grant-type:device_code",
                })
                .to_string(),
                headers: vec![("Accept".into(), "application/json".into())],
            })
        }
        _ => None,
    }
}

pub fn kiro_register_request(opts: &DeviceOptions) -> DeviceHttpRequest {
    let region = kiro_region_of(opts);
    DeviceHttpRequest {
        url: mock_env("kiro", "register")
            .unwrap_or_else(|| format!("https://oidc.{region}.amazonaws.com/client/register")),
        method: "POST",
        content_type: "application/json",
        body: serde_json::json!({
            "clientName": "kiro-oauth-client",
            "clientType": "public",
            "scopes": ["codewhisperer:completions", "codewhisperer:analysis"],
            "grantTypes": ["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"],
        })
        .to_string(),
        headers: vec![("Accept".into(), "application/json".into())],
    }
}

pub fn kiro_device_auth_request(
    client_id: &str,
    client_secret: &str,
    opts: &DeviceOptions,
) -> DeviceHttpRequest {
    let region = kiro_region_of(opts);
    let start = opts
        .kiro_start_url
        .clone()
        .unwrap_or_else(|| "https://view.awsapps.com/start".into());
    DeviceHttpRequest {
        url: mock_env("kiro", "code")
            .unwrap_or_else(|| format!("https://oidc.{region}.amazonaws.com/device_authorization")),
        method: "POST",
        content_type: "application/json",
        body: serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "startUrl": start,
        })
        .to_string(),
        headers: vec![("Accept".into(), "application/json".into())],
    }
}

/// Normalize a device-code response into the gateway device shape.
pub fn normalize_device_response(provider: &str, raw: &Value) -> Option<Value> {
    match provider {
        "kilocode" => Some(serde_json::json!({
            "device_code": raw.get("code").and_then(|v| v.as_str()).unwrap_or(""),
            "user_code": raw.get("code").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri": raw.get("verificationUrl").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri_complete": raw.get("verificationUrl").and_then(|v| v.as_str()).unwrap_or(""),
            "expires_in": raw.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(300),
            "interval": 3,
        })),
        "codebuddy-cn" | "codebuddy-intl" => {
            let d = raw.get("data")?;
            if raw.get("code").and_then(|v| v.as_i64()) != Some(0) {
                return None;
            }
            Some(serde_json::json!({
                "device_code": d.get("state").and_then(|v| v.as_str()).unwrap_or(""),
                "user_code": "",
                "verification_uri": d.get("authUrl").and_then(|v| v.as_str()).unwrap_or(""),
                "verification_uri_complete": d.get("authUrl").and_then(|v| v.as_str()).unwrap_or(""),
                "expires_in": 600,
                "interval": 5,
            }))
        }
        "kiro" => Some(serde_json::json!({
            "device_code": raw.get("deviceCode").and_then(|v| v.as_str()).unwrap_or(""),
            "user_code": raw.get("userCode").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri": raw.get("verificationUri").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri_complete": raw.get("verificationUriComplete").and_then(|v| v.as_str()).unwrap_or(""),
            "expires_in": raw.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(600),
            "interval": raw.get("interval").and_then(|v| v.as_i64()).unwrap_or(5),
            "_clientId": raw.get("clientId").and_then(|v| v.as_str()).unwrap_or(""),
            "_clientSecret": raw.get("clientSecret").and_then(|v| v.as_str()).unwrap_or(""),
        })),
        _ => super::parse_device_code(raw).map(|d| {
            serde_json::json!({
                "device_code": d.device_code,
                "user_code": d.user_code,
                "verification_uri": d.verification_uri,
                "verification_uri_complete": raw.get("verification_uri_complete").and_then(|v| v.as_str()).unwrap_or(d.verification_uri.as_str()),
                "expires_in": d.expires_in,
                "interval": d.interval,
            })
        }),
    }
    .and_then(|mut v: Value| {
        if v.get("device_code").and_then(|x| x.as_str()).unwrap_or("").is_empty() {
            None
        } else {
            // Qoder local flow passes its URL through raw.
            if provider == "qoder" {
                if let Some(u) = raw.get("verification_uri_complete").and_then(|x| x.as_str()) {
                    v["verification_uri_complete"] = Value::String(u.into());
                    v["verification_uri"] = Value::String(u.into());
                }
            }
            Some(v)
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Failed(String),
    Complete(Value),
}

/// Normalize a poll response body + HTTP status into an outcome.
pub fn normalize_poll(provider: &str, status: u16, raw: &Value) -> PollOutcome {
    match provider {
        "kilocode" => match status {
            202 => PollOutcome::Pending,
            403 => PollOutcome::Denied,
            410 => PollOutcome::Expired,
            _ => {
                if raw.get("access_token").and_then(|v| v.as_str()).is_some() {
                    PollOutcome::Complete(raw.clone())
                } else if raw.get("token").and_then(|v| v.as_str()).is_some() {
                    PollOutcome::Complete(serde_json::json!({
                        "access_token": raw.get("token"),
                        "_userEmail": raw.get("userEmail"),
                    }))
                } else {
                    match raw.get("error").and_then(|v| v.as_str()).unwrap_or("") {
                        "authorization_pending" => PollOutcome::Pending,
                        "slow_down" => PollOutcome::SlowDown,
                        "expired_token" => PollOutcome::Expired,
                        "access_denied" => PollOutcome::Denied,
                        e => PollOutcome::Failed(e.into()),
                    }
                }
            }
        },
        "codebuddy-cn" | "codebuddy-intl" => {
            let code = raw.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            if code == 0 {
                if let Some(d) = raw.get("data") {
                    if d.get("accessToken").and_then(|v| v.as_str()).is_some() {
                        return PollOutcome::Complete(serde_json::json!({
                            "access_token": d.get("accessToken"),
                            "refresh_token": d.get("refreshToken"),
                            "expires_in": d.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(86400),
                        }));
                    }
                }
                PollOutcome::Failed("missing token".into())
            } else if code == 11217 {
                PollOutcome::Pending
            } else {
                PollOutcome::Failed(format!("code {code}"))
            }
        }
        "qoder" => {
            if status == 202 || status == 404 {
                PollOutcome::Pending
            } else if raw.get("access_token").and_then(|v| v.as_str()).is_some()
                || raw.get("accessToken").and_then(|v| v.as_str()).is_some()
            {
                let tok = raw
                    .get("access_token")
                    .or_else(|| raw.get("accessToken"))
                    .cloned()
                    .unwrap_or_default();
                PollOutcome::Complete(serde_json::json!({"access_token": tok}))
            } else {
                PollOutcome::Pending
            }
        }
        "kiro" => {
            if let Some(t) = raw.get("accessToken").and_then(|v| v.as_str()) {
                PollOutcome::Complete(serde_json::json!({
                    "access_token": t,
                    "refresh_token": raw.get("refreshToken"),
                    "expires_in": raw.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(3600),
                }))
            } else {
                match raw
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("authorization_pending")
                {
                    "authorization_pending" => PollOutcome::Pending,
                    "slow_down" => PollOutcome::SlowDown,
                    "expired_token" => PollOutcome::Expired,
                    "access_denied" => PollOutcome::Denied,
                    e => PollOutcome::Failed(e.into()),
                }
            }
        }
        "kimi" | "kimi-coding" => {
            // Upstream: 200 + authorization_pending JSON means keep polling.
            if raw.get("access_token").and_then(|v| v.as_str()).is_some() {
                PollOutcome::Complete(raw.clone())
            } else {
                match super::device_poll_status(raw) {
                    super::DevicePoll::Pending => PollOutcome::Pending,
                    super::DevicePoll::SlowDown => PollOutcome::SlowDown,
                    super::DevicePoll::Expired => PollOutcome::Expired,
                    super::DevicePoll::Denied => PollOutcome::Denied,
                    super::DevicePoll::Complete => PollOutcome::Complete(raw.clone()),
                }
            }
        }
        _ => {
            if raw.get("access_token").and_then(|v| v.as_str()).is_some() {
                PollOutcome::Complete(raw.clone())
            } else {
                match super::device_poll_status(raw) {
                    super::DevicePoll::Pending => PollOutcome::Pending,
                    super::DevicePoll::SlowDown => PollOutcome::SlowDown,
                    super::DevicePoll::Expired => PollOutcome::Expired,
                    super::DevicePoll::Denied => PollOutcome::Denied,
                    super::DevicePoll::Complete => PollOutcome::Complete(raw.clone()),
                }
            }
        }
    }
}

/// Qoder local initiation: PKCE challenge + browser URL (upstream qoder.js).
pub fn qoder_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(verifier.as_bytes());
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, hash)
}

pub fn qoder_verification_url(challenge: &str, nonce: &str, machine_id: &str) -> String {
    let base = mock_env("qoder", "code")
        .unwrap_or_else(|| "https://qoder.com/device/selectAccounts".into());
    format!(
        "{base}?challenge={}&challenge_method=S256&machine_id={}&nonce={}",
        urlencode(challenge),
        urlencode(machine_id),
        urlencode(nonce)
    )
}

/// Extract `code`/`state` from a pasted callback URL (CLI paste step).
pub fn parse_callback_url(url: &str) -> Option<(String, String)> {
    let q = url.split('?').nth(1)?;
    let mut code = None;
    let mut state = None;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=')?;
        match k {
            "code" => code = Some(urldecode(v)),
            "state" => state = Some(urldecode(v)),
            _ => {}
        }
    }
    Some((code?, state?))
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

fn urldecode(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn region_guard_rejects_ssrf_hosts() {
        assert!(valid_aws_region("us-east-1"));
        assert!(valid_aws_region("eu-central-1"));
        assert!(!valid_aws_region("evil.com"));
        assert!(!valid_aws_region("us-east-1.evil.com"));
        assert!(!valid_aws_region(""));
    }

    #[test]
    fn github_device_request_shape() {
        let r =
            device_code_request("github", "cid", "read:user", &DeviceOptions::default()).unwrap();
        assert!(r.url.contains("github.com/login/device/code"));
        assert!(r.body.contains("client_id=cid"));
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "Accept" && v == "application/json"));
    }

    #[test]
    fn kilocode_poll_status_mapping() {
        assert_eq!(
            normalize_poll("kilocode", 202, &json!({})),
            PollOutcome::Pending
        );
        assert_eq!(
            normalize_poll("kilocode", 403, &json!({})),
            PollOutcome::Denied
        );
        assert_eq!(
            normalize_poll("kilocode", 410, &json!({})),
            PollOutcome::Expired
        );
        assert!(matches!(
            normalize_poll("kilocode", 200, &json!({"token": "t"})),
            PollOutcome::Complete(_)
        ));
    }

    #[test]
    fn codebuddy_pending_and_success() {
        assert_eq!(
            normalize_poll("codebuddy-cn", 200, &json!({"code": 11217})),
            PollOutcome::Pending
        );
        assert!(matches!(
            normalize_poll(
                "codebuddy-cn",
                200,
                &json!({"code": 0, "data": {"accessToken": "t"}})
            ),
            PollOutcome::Complete(_)
        ));
    }

    #[test]
    fn qoder_url_and_poll_pending() {
        let url = qoder_verification_url("ch", "n", "m");
        assert!(url.contains("challenge=ch"));
        assert!(url.contains("nonce=n"));
        assert_eq!(
            normalize_poll("qoder", 202, &json!({})),
            PollOutcome::Pending
        );
        assert_eq!(
            normalize_poll("qoder", 404, &json!({})),
            PollOutcome::Pending
        );
    }

    #[test]
    fn callback_url_parsing() {
        let (c, s) =
            parse_callback_url("http://127.0.0.1:1455/auth/callback?code=abc&state=xyz").unwrap();
        assert_eq!((c, s), ("abc".into(), "xyz".into()));
        assert!(parse_callback_url("http://x/").is_none());
    }
}
