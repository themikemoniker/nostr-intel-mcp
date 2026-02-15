use std::sync::Arc;

use nostr_sdk::prelude::*;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};

use crate::config::Config;
use crate::nostr::cache::{Cache, CachedProfile, CachedRelayInfo};
use crate::nostr::client::NostrClient;
use crate::payment::free_tier::FreeTierLimiter;
use crate::payment::nwc_gateway::NwcGateway;
use crate::tools::free::*;
use crate::tools::paid::*;

pub struct NostrIntelServer {
    config: Arc<Config>,
    nostr_client: Arc<NostrClient>,
    cache: Arc<Cache>,
    nwc_gateway: Option<Arc<NwcGateway>>,
    rate_limiter: Arc<FreeTierLimiter>,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NostrIntelServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Nostr intelligence server. Provides tools to decode Nostr entities, \
                 resolve NIP-05 identifiers, fetch profiles, check relay status, and \
                 search events. Paid tools require Lightning payment after free tier \
                 (10 calls/day) is exhausted."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tool_router(router = tool_router)]
impl NostrIntelServer {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let config = Arc::new(config);

        let cache = Cache::new(
            &config.cache.database_path,
            config.cache.profile_ttl_seconds,
            config.cache.relay_info_ttl_seconds,
        )
        .await?;
        let cache = Arc::new(cache);

        let nostr_client = NostrClient::new(config.relays.default.clone()).await?;
        let nostr_client = Arc::new(nostr_client);

        let rate_limiter = Arc::new(FreeTierLimiter::new());

        let nwc_gateway = if !config.payment.nwc_url.is_empty() {
            match NwcGateway::new(&config.payment.nwc_url) {
                Ok(gw) => {
                    tracing::info!("NWC gateway initialized");
                    Some(Arc::new(gw))
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize NWC gateway: {e}");
                    None
                }
            }
        } else {
            tracing::info!("NWC_URL not configured — paid tools will be free-tier only");
            None
        };

        Ok(Self {
            config,
            nostr_client,
            cache,
            nwc_gateway,
            rate_limiter,
            tool_router: Self::tool_router(),
        })
    }

    // ==================== Free tools ====================

    #[tool(
        name = "decode_nostr_uri",
        description = "Decode any Nostr bech32 entity (npub, note, nprofile, nevent, naddr) into its components"
    )]
    async fn decode_nostr_uri(
        &self,
        Parameters(params): Parameters<DecodeNostrUriParams>,
    ) -> Result<String, String> {
        let uri = params.uri.trim();
        let bech32 = uri.strip_prefix("nostr:").unwrap_or(uri);

        let nip19 = Nip19::from_bech32(bech32).map_err(|e| format!("Invalid Nostr URI: {e}"))?;

        let response = match nip19 {
            Nip19::Pubkey(pk) => DecodeNostrUriResponse {
                entity_type: "pubkey".into(),
                hex_id: pk.to_hex(),
                relays: None,
                author_hex: None,
                kind: None,
            },
            Nip19::EventId(id) => DecodeNostrUriResponse {
                entity_type: "event_id".into(),
                hex_id: id.to_hex(),
                relays: None,
                author_hex: None,
                kind: None,
            },
            Nip19::Profile(profile) => {
                let relays: Vec<String> =
                    profile.relays.into_iter().map(|r| r.to_string()).collect();
                DecodeNostrUriResponse {
                    entity_type: "profile".into(),
                    hex_id: profile.public_key.to_hex(),
                    relays: if relays.is_empty() {
                        None
                    } else {
                        Some(relays)
                    },
                    author_hex: None,
                    kind: None,
                }
            }
            Nip19::Event(event) => {
                let relays: Vec<String> =
                    event.relays.into_iter().map(|r| r.to_string()).collect();
                DecodeNostrUriResponse {
                    entity_type: "event".into(),
                    hex_id: event.event_id.to_hex(),
                    relays: if relays.is_empty() {
                        None
                    } else {
                        Some(relays)
                    },
                    author_hex: event.author.map(|a| a.to_hex()),
                    kind: event.kind.map(|k| k.as_u16() as u32),
                }
            }
            Nip19::Coordinate(coord) => {
                let relays: Vec<String> =
                    coord.relays.into_iter().map(|r| r.to_string()).collect();
                DecodeNostrUriResponse {
                    entity_type: "coordinate".into(),
                    hex_id: coord.coordinate.identifier.clone(),
                    relays: if relays.is_empty() {
                        None
                    } else {
                        Some(relays)
                    },
                    author_hex: Some(coord.coordinate.public_key.to_hex()),
                    kind: Some(coord.coordinate.kind.as_u16() as u32),
                }
            }
            _ => return Err("Unsupported NIP-19 entity type".into()),
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    #[tool(
        name = "resolve_nip05",
        description = "Resolve a NIP-05 identifier (user@domain.com) to a Nostr pubkey and relay list"
    )]
    async fn resolve_nip05(
        &self,
        Parameters(params): Parameters<ResolveNip05Params>,
    ) -> Result<String, String> {
        let nip05 = params.nip05.trim();

        let parts: Vec<&str> = nip05.split('@').collect();
        if parts.len() != 2 {
            return Err("Invalid NIP-05 format, expected user@domain".into());
        }
        let (name, domain) = (parts[0], parts[1]);

        let url = format!("https://{domain}/.well-known/nostr.json?name={name}");

        let http = reqwest::Client::new();
        let resp = http
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP error: {}", resp.status()));
        }

        let json: serde_json::Value =
            resp.json().await.map_err(|e| format!("JSON parse error: {e}"))?;

        let pubkey_hex = json["names"][name]
            .as_str()
            .ok_or_else(|| format!("NIP-05 name '{name}' not found at {domain}"))?
            .to_string();

        let pubkey = PublicKey::from_hex(&pubkey_hex)
            .map_err(|e| format!("Invalid pubkey in response: {e}"))?;

        let relays = json["relays"][&pubkey_hex].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

        let response = ResolveNip05Response {
            pubkey: pubkey_hex,
            pubkey_npub: pubkey.to_bech32().map_err(|e| e.to_string())?,
            relays,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    #[tool(
        name = "get_profile",
        description = "Fetch Nostr profile metadata (kind:0) for a given pubkey. Accepts hex, npub, or NIP-05 identifier."
    )]
    async fn get_profile(
        &self,
        Parameters(params): Parameters<GetProfileParams>,
    ) -> Result<String, String> {
        let input = params.pubkey.trim();

        let pubkey = if input.contains('@') {
            let nip05_params = ResolveNip05Params {
                nip05: input.to_string(),
            };
            let result_json = self.resolve_nip05(Parameters(nip05_params)).await?;
            let result: ResolveNip05Response =
                serde_json::from_str(&result_json).map_err(|e| e.to_string())?;
            PublicKey::from_hex(&result.pubkey).map_err(|e| e.to_string())?
        } else {
            NostrClient::parse_pubkey(input).map_err(|e| e.to_string())?
        };

        let pubkey_hex = pubkey.to_hex();

        // Check cache
        if let Ok(Some(cached)) = self.cache.get_profile(&pubkey_hex).await {
            tracing::debug!("Cache hit for profile: {pubkey_hex}");
            let response = GetProfileResponse {
                pubkey: pubkey_hex,
                name: cached.name,
                display_name: cached.display_name,
                about: cached.about,
                picture: cached.picture,
                banner: cached.banner,
                nip05: cached.nip05,
                lud16: cached.lud16,
                website: cached.website,
            };
            return serde_json::to_string_pretty(&response).map_err(|e| e.to_string());
        }

        // Fetch from relays
        tracing::debug!("Fetching profile from relays: {pubkey_hex}");
        let metadata = self
            .nostr_client
            .get_metadata(&pubkey)
            .await
            .map_err(|e| format!("Failed to fetch metadata: {e}"))?;

        match metadata {
            Some(meta) => {
                let cached = CachedProfile {
                    pubkey: pubkey_hex.clone(),
                    name: meta.name.clone(),
                    display_name: meta.display_name.clone(),
                    about: meta.about.clone(),
                    picture: meta.picture.clone(),
                    banner: meta.banner.clone(),
                    nip05: meta.nip05.clone(),
                    lud16: meta.lud16.clone(),
                    website: meta.website.clone(),
                };
                if let Err(e) = self.cache.set_profile(&cached).await {
                    tracing::warn!("Failed to cache profile: {e}");
                }

                let response = GetProfileResponse {
                    pubkey: pubkey_hex,
                    name: meta.name,
                    display_name: meta.display_name,
                    about: meta.about,
                    picture: meta.picture,
                    banner: meta.banner,
                    nip05: meta.nip05,
                    lud16: meta.lud16,
                    website: meta.website,
                };
                serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
            }
            None => Err(format!("Profile not found for pubkey: {pubkey_hex}")),
        }
    }

    #[tool(
        name = "check_relay",
        description = "Check a Nostr relay's online status, latency, and NIP-11 info document"
    )]
    async fn check_relay(
        &self,
        Parameters(params): Parameters<CheckRelayParams>,
    ) -> Result<String, String> {
        let relay_url = params.relay_url.trim();

        // Check cache
        if let Ok(Some(cached)) = self.cache.get_relay_info(relay_url).await {
            tracing::debug!("Cache hit for relay: {relay_url}");
            let response = CheckRelayResponse {
                online: cached.online,
                latency_ms: cached.latency_ms.map(|ms| ms as u64),
                name: cached.name,
                description: cached.description,
                supported_nips: Some(cached.supported_nips),
                software: cached.software,
                version: cached.version,
            };
            return serde_json::to_string_pretty(&response).map_err(|e| e.to_string());
        }

        // Convert wss:// to https:// for NIP-11 fetch
        let http_url = relay_url
            .replace("wss://", "https://")
            .replace("ws://", "http://");

        let http = reqwest::Client::new();
        let start = std::time::Instant::now();

        let result = http
            .get(&http_url)
            .header("Accept", "application/nostr+json")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                let latency_ms = start.elapsed().as_millis() as u64;

                let json: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse NIP-11: {e}"))?;

                let supported_nips = json["supported_nips"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let name = json["name"].as_str().map(String::from);
                let description = json["description"].as_str().map(String::from);
                let software = json["software"].as_str().map(String::from);
                let version = json["version"].as_str().map(String::from);

                // Cache
                let cached = CachedRelayInfo {
                    relay_url: relay_url.to_string(),
                    name: name.clone(),
                    description: description.clone(),
                    supported_nips: supported_nips.clone(),
                    software: software.clone(),
                    version: version.clone(),
                    online: true,
                    latency_ms: Some(latency_ms as i64),
                };
                if let Err(e) = self.cache.set_relay_info(&cached).await {
                    tracing::warn!("Failed to cache relay info: {e}");
                }

                let response = CheckRelayResponse {
                    online: true,
                    latency_ms: Some(latency_ms),
                    name,
                    description,
                    supported_nips: Some(supported_nips),
                    software,
                    version,
                };
                serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
            }
            Ok(resp) => {
                let response = CheckRelayResponse {
                    online: false,
                    latency_ms: None,
                    name: None,
                    description: Some(format!("HTTP error: {}", resp.status())),
                    supported_nips: None,
                    software: None,
                    version: None,
                };
                serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
            }
            Err(e) => {
                let response = CheckRelayResponse {
                    online: false,
                    latency_ms: None,
                    name: None,
                    description: Some(format!("Connection failed: {e}")),
                    supported_nips: None,
                    software: None,
                    version: None,
                };
                serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
            }
        }
    }

    // ==================== Paid tools ====================

    #[tool(
        name = "search_events",
        description = "Search Nostr events across multiple relays with filters. Costs 10-50 sats after free tier (10 calls/day)."
    )]
    async fn search_events(
        &self,
        Parameters(params): Parameters<SearchEventsParams>,
    ) -> Result<String, String> {
        // 1. If payment_hash provided, verify payment first
        if let Some(ref hash) = params.payment_hash {
            let gw = self
                .nwc_gateway
                .as_ref()
                .ok_or("Payment system not configured")?;
            let paid = gw.verify_payment(hash).await.map_err(|e| e.to_string())?;
            if !paid {
                return Err(
                    "Payment not confirmed. Invoice may be unpaid or expired.".into(),
                );
            }
            // Payment verified — fall through to execute search
        } else {
            // 2. Check free tier
            let under_limit = self
                .rate_limiter
                .check_and_increment("stdio", self.config.free_tier.calls_per_day)
                .await;
            if !under_limit {
                // 3. Generate invoice if NWC is configured
                let gw = self
                    .nwc_gateway
                    .as_ref()
                    .ok_or("Free tier exhausted and payment system not configured")?;
                let amount = self.calculate_price(&params);
                let inv = gw
                    .create_invoice(
                        "search_events",
                        amount,
                        "nostr-intel: search_events",
                        self.config.payment.invoice_expiry_seconds,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let resp = PaymentRequiredResponse {
                    payment_required: true,
                    tool_name: "search_events".into(),
                    amount_sats: amount,
                    invoice: inv.invoice,
                    payment_hash: inv.payment_hash,
                    message: format!(
                        "Free tier exhausted. Payment required: {amount} sats. \
                         Pay the invoice, then retry with the payment_hash parameter."
                    ),
                };
                return serde_json::to_string_pretty(&resp).map_err(|e| e.to_string());
            }
            // Under free tier — fall through to execute search
        }

        // 4. Execute search
        let authors = if let Some(ref author_strs) = params.authors {
            let mut pks = Vec::new();
            for a in author_strs {
                let pk = NostrClient::parse_pubkey(a)
                    .map_err(|e| format!("Invalid author pubkey '{a}': {e}"))?;
                pks.push(pk);
            }
            Some(pks)
        } else {
            None
        };

        let kinds = params.kinds.as_ref().map(|ks| {
            ks.iter().map(|k| Kind::from(*k as u16)).collect()
        });

        let since = params.since_hours.map(|hours| {
            let secs_ago = hours * 3600;
            let now = chrono::Utc::now().timestamp() as u64;
            Timestamp::from(now.saturating_sub(secs_ago))
        });

        let limit = params.limit;

        let events = self
            .nostr_client
            .search_events(authors, kinds, params.search.clone(), since, limit)
            .await
            .map_err(|e| format!("Search failed: {e}"))?;

        let relays_queried: Vec<String> = self.config.relays.default.clone();

        let event_summaries: Vec<EventSummary> = events
            .iter()
            .map(|event| {
                let content = if event.content.len() > 280 {
                    format!("{}...", &event.content[..280])
                } else {
                    event.content.clone()
                };

                let tags_summary = if event.tags.is_empty() {
                    "none".to_string()
                } else {
                    let tag_kinds: Vec<String> = event
                        .tags
                        .iter()
                        .take(5)
                        .map(|t| t.kind().to_string())
                        .collect();
                    if event.tags.len() > 5 {
                        format!("{} (+{} more)", tag_kinds.join(", "), event.tags.len() - 5)
                    } else {
                        tag_kinds.join(", ")
                    }
                };

                EventSummary {
                    id: event.id.to_hex(),
                    pubkey: event.pubkey.to_hex(),
                    kind: event.kind.as_u16() as u32,
                    content,
                    created_at: event.created_at.as_secs(),
                    tags_summary,
                }
            })
            .collect();

        let count = event_summaries.len() as u32;
        let response = SearchEventsResponse {
            events: event_summaries,
            count,
            relays_queried,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    fn calculate_price(&self, params: &SearchEventsParams) -> u64 {
        let mut price = self.config.pricing.search_events_base;
        if let Some(limit) = params.limit {
            if limit > 20 {
                price += 15;
            }
            if limit > 50 {
                price += 25;
            }
        }
        price
    }
}
