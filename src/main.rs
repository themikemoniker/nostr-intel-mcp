mod config;
#[allow(dead_code)]
mod error;
mod nostr;
mod payment;
mod server;
mod tools;

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
    tracing::info!("Configuration loaded");

    let server = server::NostrIntelServer::new(config).await?;
    tracing::info!("Server initialized, serving MCP over stdio");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
