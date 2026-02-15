use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub relays: RelayConfig,
    pub cache: CacheConfig,
    pub free_tier: FreeTierConfig,
    pub pricing: PricingConfig,
    pub payment: PaymentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub version: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
}

fn default_transport() -> String {
    "stdio".into()
}

fn default_http_port() -> u16 {
    3000
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelayConfig {
    pub default: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    pub database_path: String,
    pub profile_ttl_seconds: u64,
    pub relay_info_ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FreeTierConfig {
    pub calls_per_day: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PricingConfig {
    pub search_events_base: u64,
    pub relay_discovery: u64,
    pub trending_notes: u64,
    pub get_follower_graph: u64,
    pub zap_analytics: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentConfig {
    pub nwc_url: String,
    pub invoice_expiry_seconds: u64,
    #[serde(default)]
    pub l402_secret: String,
    #[serde(default)]
    pub enable_l402: bool,
    #[serde(default)]
    pub enable_x402: bool,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let content = std::fs::read_to_string("config.toml")
            .context("Failed to read config.toml")?;

        let mut config: Config =
            toml::from_str(&content).context("Failed to parse config.toml")?;

        // Override nwc_url from env var if set
        if let Ok(nwc_url) = std::env::var("NWC_URL") {
            if !nwc_url.is_empty() {
                config.payment.nwc_url = nwc_url;
            }
        }

        // Override L402 secret from env var
        if let Ok(secret) = std::env::var("L402_SECRET") {
            if !secret.is_empty() {
                config.payment.l402_secret = secret;
            }
        }

        // Override transport from env var
        if let Ok(transport) = std::env::var("MCP_TRANSPORT") {
            if !transport.is_empty() {
                config.server.transport = transport;
            }
        }

        Ok(config)
    }
}
