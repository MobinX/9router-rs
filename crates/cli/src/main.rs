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
    let mut api_keys: Vec<String> = Vec::new();
    if let Ok(conn) = rusqlite::Connection::open(settings.db_path()) {
        let _ = nine_storage::migrate(&conn);
        if let Ok(mut stmt) = conn.prepare("SELECT key FROM apiKeys WHERE isActive = 1") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                api_keys = rows.filter_map(Result::ok).collect();
            }
        }
    }
    let mut upstreams = Vec::new();
    if let Ok(url) = std::env::var("NINE_UPSTREAM_URL") {
        upstreams.push(nine_gateway::Upstream {
            provider: "openai",
            base_url: url,
            api_key: std::env::var("NINE_UPSTREAM_KEY").unwrap_or_default(),
        });
    }
    let app = nine_gateway::router_with_state(
        nine_gateway::AppState::new(upstreams, settings.timeout_ms)
            .with_catalog(catalog)
            .with_api_keys(api_keys),
    );
    let addr = format!("{}:{}", cli.host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("9router-rs listening on {addr}");
    axum::serve(listener, app).await.expect("serve");
}
