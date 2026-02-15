mod config;
#[allow(dead_code)]
mod error;
mod nostr;
mod payment;
mod server;
mod tools;

use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Log to stderr — stdout is reserved for MCP JSON-RPC protocol
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,nostr_intel_mcp=debug".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false),
        )
        .init();

    tracing::info!("Starting nostr-intel-mcp server");

    let config = config::Config::load()?;
    tracing::info!("Configuration loaded (transport={})", config.server.transport);

    match config.server.transport.as_str() {
        "http" => run_http(config).await,
        _ => run_stdio(config).await,
    }
}

async fn run_stdio(config: config::Config) -> anyhow::Result<()> {
    let server = server::NostrIntelServer::new(config).await?;
    tracing::info!("Server initialized, serving MCP over stdio");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

async fn run_http(config: config::Config) -> anyhow::Result<()> {
    use axum::routing::get;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let http_port = config.server.http_port;
    let l402_enabled = config.payment.enable_l402;
    let l402_secret = config.payment.l402_secret.clone();

    // Initialize server to extract shared state, then drop the original
    let init_server = server::NostrIntelServer::new(config).await?;
    let shared = Arc::new(init_server.shared_state());
    drop(init_server);

    // Build the MCP StreamableHttp service
    let shared_for_factory = Arc::clone(&shared);
    let mcp_service = StreamableHttpService::new(
        move || Ok(server::NostrIntelServer::from_shared(&shared_for_factory)),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    // Build axum router
    let mut app = axum::Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest_service("/mcp", mcp_service);

    // Add L402 challenge endpoint if enabled
    if l402_enabled && !l402_secret.is_empty() {
        let l402_mgr = Arc::new(
            payment::l402::L402Manager::new(&l402_secret)
                .map_err(|e| anyhow::anyhow!("Failed to init L402Manager: {e}"))?,
        );
        let shared_for_l402 = Arc::clone(&shared);

        app = app.route(
            "/l402/challenge/{tool_name}",
            get(move |axum::extract::Path(tool_name): axum::extract::Path<String>| {
                let l402_mgr = Arc::clone(&l402_mgr);
                let shared = Arc::clone(&shared_for_l402);
                async move {
                    l402_challenge_handler(tool_name, l402_mgr, shared).await
                }
            }),
        );
        tracing::info!("L402 challenge endpoint enabled at /l402/challenge/{{tool_name}}");
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{http_port}")).await?;
    tracing::info!("Serving MCP over HTTP on 0.0.0.0:{http_port}");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn l402_challenge_handler(
    tool_name: String,
    l402_mgr: Arc<payment::l402::L402Manager>,
    shared: Arc<server::SharedState>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let gw = match shared.nwc_gateway.as_ref() {
        Some(gw) => gw,
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "NWC gateway not configured").into_response();
        }
    };

    // Look up price for the tool
    let amount = match tool_name.as_str() {
        "search_events" => shared.config.pricing.search_events_base,
        "relay_discovery" => shared.config.pricing.relay_discovery,
        "trending_notes" => shared.config.pricing.trending_notes,
        "get_follower_graph" => shared.config.pricing.get_follower_graph,
        "zap_analytics" => shared.config.pricing.zap_analytics,
        _ => {
            return (StatusCode::NOT_FOUND, "Unknown tool").into_response();
        }
    };

    let description = format!("nostr-intel: {tool_name}");
    let inv = match gw
        .create_invoice(
            &tool_name,
            amount,
            &description,
            shared.config.payment.invoice_expiry_seconds,
        )
        .await
    {
        Ok(inv) => inv,
        Err(e) => {
            tracing::error!("Failed to create invoice for L402 challenge: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invoice creation failed")
                .into_response();
        }
    };

    let expires = chrono::Utc::now().timestamp() as u64
        + shared.config.payment.invoice_expiry_seconds;

    let challenge = l402_mgr.create_challenge(
        &inv.invoice,
        &inv.payment_hash,
        &tool_name,
        expires,
    );

    let body = serde_json::json!({
        "tool": tool_name,
        "amount_sats": amount,
        "invoice": inv.invoice,
        "payment_hash": inv.payment_hash,
    });

    (
        StatusCode::PAYMENT_REQUIRED,
        [("WWW-Authenticate", challenge)],
        axum::Json(body),
    )
        .into_response()
}
