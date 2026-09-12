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
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Some(Cmd::Version) | None if false => {}
        _ => {}
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
    let addr = format!("{}:{}", cli.host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("9router-rs listening on {addr}");
    axum::serve(listener, app).await.expect("serve");
}
