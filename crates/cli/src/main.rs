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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    if let Some(Cmd::Login {
        provider,
        device,
        gateway_url,
        no_browser,
    }) = &cli.cmd
    {
        login(
            provider,
            *device,
            gateway_url,
            *no_browser || cli.no_browser,
        )
        .await;
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
