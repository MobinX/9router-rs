use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "9router-rs",
    version,
    about = "Rust 9Router-compatible gateway"
)]
struct Cli {
    #[arg(short, long, default_value_t = 20128)]
    port: u16,
    #[arg(short = 'H', long, default_value = "0.0.0.0")]
    host: String,
    #[arg(short, long)]
    no_browser: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Serve {
        #[arg(short, long)]
        port: Option<u16>,
    },
    Version,
    /// Interactive OAuth login like 9Router: browser authorize or device-code poll.
    Login {
        provider: String,
        #[arg(long)]
        device: bool,
        /// Loopback callback server (auto-capture) instead of pasting the callback URL.
        #[arg(long)]
        auto: bool,
        #[arg(long, default_value = "http://127.0.0.1:20128")]
        gateway_url: String,
        #[arg(long)]
        no_browser: bool,
    },
}

/// Decide whether device polling should continue. Pure for tests.
pub fn poll_next(step: &serde_json::Value) -> PollNext {
    if step.get("success").and_then(|v| v.as_bool()) == Some(true)
        || step.get("ok").and_then(|v| v.as_bool()) == Some(true)
            && step.get("connection").is_some()
    {
        PollNext::Done
    } else if step.get("pending").and_then(|v| v.as_bool()) == Some(true)
        || step.get("error").and_then(|v| v.as_str()) == Some("authorization_pending")
        || step.get("error").and_then(|v| v.as_str()) == Some("slow_down")
    {
        PollNext::Wait
    } else {
        PollNext::Fail(
            step.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PollNext {
    Wait,
    Done,
    Fail(String),
}

fn open_browser(url: &str) {
    // ponytail: opener selection is best-effort; add when desktop UX is covered.
    for (bin, args) in [
        ("xdg-open", vec![url]),
        ("open", vec![url]),
        ("cmd", vec!["/c", "start", url]),
    ] {
        if std::process::Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

fn prompt(line: &str) -> String {
    print!("{line}");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

async fn login(provider: &str, device: bool, gateway_url: &str, no_browser: bool) {
    let gw = gateway_url.trim_end_matches('/');
    let client = reqwest::Client::new();
    if device {
        let url = format!("{gw}/api/oauth/{provider}/device-code");
        let dc: serde_json::Value = match client.get(&url).send().await {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(e) => {
                eprintln!("device-code request failed: {e}");
                std::process::exit(1);
            }
        };
        if dc.get("device_code").is_none() {
            eprintln!(
                "device flow failed: {}",
                dc.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
            );
            std::process::exit(1);
        }
        let durl = dc
            .get("verification_uri_complete")
            .or_else(|| dc.get("verification_uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("Open: {durl}");
        if !dc
            .get("user_code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            println!("Code: {}", dc["user_code"].as_str().unwrap_or(""));
        }
        if !no_browser {
            open_browser(durl);
        }
        // Poll every 5s x60 like upstream menus/providers.js.
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let step: serde_json::Value = match client
                .post(format!("{gw}/api/oauth/{provider}/poll"))
                .json(&serde_json::json!({
                    "deviceCode": dc.get("device_code"),
                    "codeVerifier": dc.get("codeVerifier"),
                    "extraData": dc.get("extraData"),
                }))
                .send()
                .await
            {
                Ok(r) => r.json().await.unwrap_or_default(),
                Err(_) => serde_json::json!({"pending": true}),
            };
            match poll_next(&step) {
                PollNext::Wait => continue,
                PollNext::Done => {
                    println!(
                        "login ok: {}",
                        step.get("connection")
                            .map(|c| c.to_string())
                            .unwrap_or_default()
                    );
                    return;
                }
                PollNext::Fail(e) => {
                    eprintln!("login failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        eprintln!("login timed out waiting for authorization");
        std::process::exit(1);
    }
    let url = format!("{gw}/api/oauth/{provider}/authorize?redirect_uri=");
    let auth: serde_json::Value = match client.get(&url).send().await {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(e) => {
            eprintln!("authorize request failed: {e}");
            std::process::exit(1);
        }
    };
    let auth_url = auth
        .get("authorizeUrl")
        .or_else(|| auth.get("authUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if auth_url.is_empty() {
        eprintln!(
            "authorize failed: {}",
            auth.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        );
        std::process::exit(1);
    }
    println!("Open: {auth_url}");
    if !no_browser {
        open_browser(&auth_url);
    }
    let pasted = prompt("Paste the callback URL: ");
    let (code, state) = nine_oauth::device::parse_callback_url(&pasted).unwrap_or_default();
    if code.is_empty() {
        eprintln!("no code found in pasted URL");
        std::process::exit(1);
    }
    let done: serde_json::Value = match client
        .post(format!("{gw}/api/oauth/{provider}/exchange"))
        .json(&serde_json::json!({
            "code": code,
            "state": if state.is_empty() { auth.get("state").cloned().unwrap_or_default() } else { serde_json::json!(state) },
            "codeVerifier": auth.get("codeVerifier"),
        }))
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(e) => {
            eprintln!("exchange failed: {e}");
            std::process::exit(1);
        }
    };
    println!("login ok: {done}");
}

/// Fixed loopback port + callback path per upstream (codex 1455 `/auth/callback`,
/// xai 56121 `/callback`); everything else takes an ephemeral port.
pub fn loopback_target(provider: &str) -> (u16, &'static str) {
    match provider {
        "codex" => (1455, "/auth/callback"),
        "xai" => (56121, "/callback"),
        _ => (0, "/callback"),
    }
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Authentication Successful</title></head><body style="font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh"><div><h1>Authentication Successful</h1><p>You can close this tab.</p><script>setTimeout(()=>window.close(),3000)</script></div></body></html>"#;

/// Wait for one loopback callback request and return `(code, state)`.
async fn wait_for_callback(listener: tokio::net::TcpListener) -> Option<(String, String)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let deadline = std::time::Duration::from_secs(300);
    let (mut sock, _) = tokio::time::timeout(deadline, listener.accept())
        .await
        .ok()?
        .ok()?;
    let mut reader = tokio::io::BufReader::new(&mut sock);
    let mut target = String::new();
    for i in 0..64 {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end();
        if i == 0 {
            target = line.split_whitespace().nth(1).unwrap_or("").to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    let body = SUCCESS_HTML.as_bytes();
    let _ = reader
        .get_mut()
        .write_all(
            format!(
                "HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: {}
Connection: close

",
                body.len()
            )
            .as_bytes(),
        )
        .await;
    let _ = reader.get_mut().write_all(body).await;
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    nine_oauth::device::parse_callback_url(&format!("http://127.0.0.1/?{query}"))
}

/// Loopback (auto-capture) login: bind, authorize with our redirect_uri, open the
/// browser, capture the callback, then exchange. Mirrors upstream startLocalServer.
async fn login_auto(provider: &str, gateway_url: &str, no_browser: bool) {
    let gw = gateway_url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let (fixed, path) = loopback_target(provider);
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", fixed)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind 127.0.0.1:{fixed}: {e}");
            std::process::exit(1);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(fixed);
    let redirect_uri = format!("http://127.0.0.1:{port}{path}");
    let url = format!(
        "{gw}/api/oauth/{provider}/authorize?redirect_uri={}",
        nine_oauth::device::urlencode(&redirect_uri)
    );
    let auth: serde_json::Value = match client.get(&url).send().await {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(e) => {
            eprintln!("authorize request failed: {e}");
            std::process::exit(1);
        }
    };
    let auth_url = auth
        .get("authorizeUrl")
        .or_else(|| auth.get("authUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if auth_url.is_empty() {
        eprintln!(
            "authorize failed: {}",
            auth.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        );
        std::process::exit(1);
    }
    println!("Listening on {redirect_uri}");
    println!("Open: {auth_url}");
    if !no_browser {
        open_browser(&auth_url);
    }
    let Some((code, state)) = wait_for_callback(listener).await else {
        eprintln!("login timed out waiting for callback");
        std::process::exit(1);
    };
    if code.is_empty() {
        eprintln!("no code in callback");
        std::process::exit(1);
    }
    let state = if state.is_empty() {
        auth.get("state").cloned().unwrap_or_default()
    } else {
        serde_json::json!(state)
    };
    let done: serde_json::Value = match client
        .post(format!("{gw}/api/oauth/{provider}/exchange"))
        .json(&serde_json::json!({
            "code": code,
            "state": state,
            "codeVerifier": auth.get("codeVerifier"),
            "redirectUri": redirect_uri,
        }))
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(e) => {
            eprintln!("exchange failed: {e}");
            std::process::exit(1);
        }
    };
    println!("login ok: {done}");
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    if let Some(Cmd::Login {
        provider,
        device,
        auto,
        gateway_url,
        no_browser,
    }) = &cli.cmd
    {
        let nb = *no_browser || cli.no_browser;
        if *auto {
            login_auto(provider, gateway_url, nb).await;
        } else {
            login(provider, *device, gateway_url, nb).await;
        }
        return;
    }
    if matches!(cli.cmd, Some(Cmd::Version)) {
        println!("0.1.0");
        return;
    }
    let port = match cli.cmd {
        Some(Cmd::Serve { port }) => port.unwrap_or(cli.port),
        _ => cli.port,
    };
    let settings = nine_config::Settings::default();
    let catalog = nine_providers::load_catalog(&settings.data_dir);
    let oauth_specs = nine_oauth::load_specs(&settings.data_dir);
    let store = nine_storage::Store::open(&settings.db_path())
        .map(std::sync::Arc::new)
        .ok();
    let api_keys = store
        .as_ref()
        .and_then(|s| s.list_api_keys().ok())
        .unwrap_or_default();
    let mut upstreams = Vec::new();
    if let Ok(url) = std::env::var("NINE_UPSTREAM_URL") {
        upstreams.push(nine_gateway::Upstream {
            provider: "openai",
            base_url: url,
            api_key: std::env::var("NINE_UPSTREAM_KEY").unwrap_or_default(),
        });
    }
    let mut state = nine_gateway::AppState::new(upstreams, settings.timeout_ms)
        .with_catalog(catalog)
        .with_api_keys(api_keys)
        .with_oauth_specs(oauth_specs);
    if let Some(st) = store {
        state = state.with_store(st);
    }
    let app = nine_gateway::router_with_state(state);
    let addr = format!("{}:{port}", cli.host);
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("9router-rs listening on {addr}");
    axum::serve(listener, app).await.expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_targets_match_upstream() {
        assert_eq!(loopback_target("codex"), (1455, "/auth/callback"));
        assert_eq!(loopback_target("xai"), (56121, "/callback"));
        assert_eq!(loopback_target("gemini-cli"), (0, "/callback"));
        assert_eq!(loopback_target("antigravity"), (0, "/callback"));
    }

    #[test]
    fn poll_outcome_routing() {
        assert_eq!(
            poll_next(&serde_json::json!({"pending": true})),
            PollNext::Wait
        );
        assert_eq!(
            poll_next(&serde_json::json!({"error": "authorization_pending"})),
            PollNext::Wait
        );
        assert_eq!(
            poll_next(&serde_json::json!({"ok": true, "connection": {"id": "1"}})),
            PollNext::Done
        );
        assert_eq!(
            poll_next(&serde_json::json!({"success": true})),
            PollNext::Done
        );
        assert!(matches!(
            poll_next(&serde_json::json!({"error": "access_denied"})),
            PollNext::Fail(_)
        ));
    }
}
