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
                let relays: Vec<String> = event.relays.into_iter().map(|r| r.to_string()).collect();
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
                let relays: Vec<String> = coord.relays.into_iter().map(|r| r.to_string()).collect();
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

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("JSON parse error: {e}"))?;

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
                return Err("Payment not confirmed. Invoice may be unpaid or expired.".into());
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

        let kinds = params
            .kinds
            .as_ref()
            .map(|ks| ks.iter().map(|k| Kind::from(*k as u16)).collect());

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

    // ==================== relay_discovery ====================

    #[tool(
        name = "relay_discovery",
        description = "Discover relays used by a Nostr pubkey via NIP-65 relay list metadata. Costs 20 sats after free tier."
    )]
    async fn relay_discovery(
        &self,
        Parameters(params): Parameters<RelayDiscoveryParams>,
    ) -> Result<String, String> {
        // Payment gate
        if let Some(ref hash) = params.payment_hash {
            let gw = self
                .nwc_gateway
                .as_ref()
                .ok_or("Payment system not configured")?;
            let paid = gw.verify_payment(hash).await.map_err(|e| e.to_string())?;
            if !paid {
                return Err("Payment not confirmed. Invoice may be unpaid or expired.".into());
            }
        } else {
            let under_limit = self
                .rate_limiter
                .check_and_increment("stdio", self.config.free_tier.calls_per_day)
                .await;
            if !under_limit {
                let gw = self
                    .nwc_gateway
                    .as_ref()
                    .ok_or("Free tier exhausted and payment system not configured")?;
                let amount = self.config.pricing.relay_discovery;
                let inv = gw
                    .create_invoice(
                        "relay_discovery",
                        amount,
                        "nostr-intel: relay_discovery",
                        self.config.payment.invoice_expiry_seconds,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let resp = PaymentRequiredResponse {
                    payment_required: true,
                    tool_name: "relay_discovery".into(),
                    amount_sats: amount,
                    invoice: inv.invoice,
                    payment_hash: inv.payment_hash,
                    message: format!("Free tier exhausted. Payment required: {amount} sats. Pay the invoice, then retry with the payment_hash parameter."),
                };
                return serde_json::to_string_pretty(&resp).map_err(|e| e.to_string());
            }
        }

        // Execute
        let pubkey = NostrClient::parse_pubkey(params.pubkey.trim())
            .map_err(|e| format!("Invalid pubkey: {e}"))?;

        let relay_events = self
            .nostr_client
            .fetch_relay_list(&pubkey)
            .await
            .map_err(|e| format!("Failed to fetch relay list: {e}"))?;

        let mut write_relays = Vec::new();
        let mut read_relays = Vec::new();

        if let Some(event) = relay_events.first() {
            for tag in event.tags.iter() {
                let tag_vec: Vec<String> = tag.as_slice().iter().map(|s| s.to_string()).collect();
                if tag_vec.first().map(|s| s.as_str()) == Some("r") {
                    if let Some(url) = tag_vec.get(1) {
                        match tag_vec.get(2).map(|s| s.as_str()) {
                            Some("read") => read_relays.push(url.clone()),
                            Some("write") => write_relays.push(url.clone()),
                            _ => {
                                // No marker means both read and write
                                read_relays.push(url.clone());
                                write_relays.push(url.clone());
                            }
                        }
                    }
                }
            }
        }

        // Build recommended relays from the union
        let mut recommended: Vec<String> = write_relays.clone();
        for r in &read_relays {
            if !recommended.contains(r) {
                recommended.push(r.clone());
            }
        }

        let response = RelayDiscoveryResponse {
            write_relays,
            read_relays,
            last_event_seen: relay_events.first().map(|e| LastEventSeen {
                relay: "relay_list_event".into(),
                timestamp: e.created_at.as_secs(),
            }),
            recommended_relays: recommended,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    // ==================== trending_notes ====================

    #[tool(
        name = "trending_notes",
        description = "Find trending Nostr notes by reactions, reposts, and zaps. Costs 20 sats after free tier."
    )]
    async fn trending_notes(
        &self,
        Parameters(params): Parameters<TrendingNotesParams>,
    ) -> Result<String, String> {
        // Payment gate
        if let Some(ref hash) = params.payment_hash {
            let gw = self
                .nwc_gateway
                .as_ref()
                .ok_or("Payment system not configured")?;
            let paid = gw.verify_payment(hash).await.map_err(|e| e.to_string())?;
            if !paid {
                return Err("Payment not confirmed. Invoice may be unpaid or expired.".into());
            }
        } else {
            let under_limit = self
                .rate_limiter
                .check_and_increment("stdio", self.config.free_tier.calls_per_day)
                .await;
            if !under_limit {
                let gw = self
                    .nwc_gateway
                    .as_ref()
                    .ok_or("Free tier exhausted and payment system not configured")?;
                let amount = self.config.pricing.trending_notes;
                let inv = gw
                    .create_invoice(
                        "trending_notes",
                        amount,
                        "nostr-intel: trending_notes",
                        self.config.payment.invoice_expiry_seconds,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let resp = PaymentRequiredResponse {
                    payment_required: true,
                    tool_name: "trending_notes".into(),
                    amount_sats: amount,
                    invoice: inv.invoice,
                    payment_hash: inv.payment_hash,
                    message: format!("Free tier exhausted. Payment required: {amount} sats. Pay the invoice, then retry with the payment_hash parameter."),
                };
                return serde_json::to_string_pretty(&resp).map_err(|e| e.to_string());
            }
        }

        // Execute
        let timeframe_str = params.timeframe.as_deref().unwrap_or("24h");
        let since_secs =
            parse_timeframe(timeframe_str).map_err(|e| format!("Invalid timeframe: {e}"))?;
        let now = chrono::Utc::now().timestamp() as u64;
        let since = Timestamp::from(now.saturating_sub(since_secs));

        let limit = params.limit.unwrap_or(20).min(50) as usize;

        // Fetch recent notes
        let notes = self
            .nostr_client
            .fetch_recent_notes(since, 200)
            .await
            .map_err(|e| format!("Failed to fetch notes: {e}"))?;

        if notes.is_empty() {
            let response = TrendingNotesResponse {
                notes: vec![],
                timeframe: timeframe_str.to_string(),
                count: 0,
            };
            return serde_json::to_string_pretty(&response).map_err(|e| e.to_string());
        }

        let note_ids: Vec<EventId> = notes.iter().map(|e| e.id).collect();

        // Fetch reactions, reposts, and zap receipts in parallel
        let (reactions, reposts) = tokio::join!(
            self.nostr_client.fetch_reactions(&note_ids, Some(since)),
            self.nostr_client.fetch_reposts(&note_ids, Some(since)),
        );
        let reactions = reactions.map_err(|e| format!("Failed to fetch reactions: {e}"))?;
        let reposts = reposts.map_err(|e| format!("Failed to fetch reposts: {e}"))?;

        // Count reactions per note
        let mut reaction_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for r in &reactions {
            for tag in r.tags.iter() {
                let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
                if tag_vec.first() == Some(&"e") {
                    if let Some(id) = tag_vec.get(1) {
                        *reaction_counts.entry(id.to_string()).or_default() += 1;
                    }
                }
            }
        }

        // Count reposts per note
        let mut repost_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for r in &reposts {
            for tag in r.tags.iter() {
                let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
                if tag_vec.first() == Some(&"e") {
                    if let Some(id) = tag_vec.get(1) {
                        *repost_counts.entry(id.to_string()).or_default() += 1;
                    }
                }
            }
        }

        // Score and sort notes
        let mut scored_notes: Vec<(u64, &Event)> = notes
            .iter()
            .map(|note| {
                let id_hex = note.id.to_hex();
                let r_count = reaction_counts.get(&id_hex).copied().unwrap_or(0);
                let rp_count = repost_counts.get(&id_hex).copied().unwrap_or(0);
                // Score: reactions * 1 + reposts * 3
                let score = r_count as u64 + rp_count as u64 * 3;
                (score, note)
            })
            .collect();

        scored_notes.sort_by(|a, b| b.0.cmp(&a.0));
        scored_notes.truncate(limit);

        let trending: Vec<TrendingNote> = scored_notes
            .into_iter()
            .map(|(score, note)| {
                let id_hex = note.id.to_hex();
                let content_preview = truncate_content(&note.content, 280);
                TrendingNote {
                    id: id_hex.clone(),
                    author_pubkey: note.pubkey.to_hex(),
                    author_name: None,
                    content_preview,
                    reactions: reaction_counts.get(&id_hex).copied().unwrap_or(0),
                    reposts: repost_counts.get(&id_hex).copied().unwrap_or(0),
                    zap_total_sats: 0,
                    score,
                    created_at: note.created_at.as_secs(),
                }
            })
            .collect();

        let count = trending.len() as u32;
        let response = TrendingNotesResponse {
            notes: trending,
            timeframe: timeframe_str.to_string(),
            count,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    // ==================== get_follower_graph ====================

    #[tool(
        name = "get_follower_graph",
        description = "Get the follower graph for a Nostr pubkey: following, followers, and mutual follows. Costs 50 sats (depth 1) or 100 sats (depth 2) after free tier."
    )]
    async fn get_follower_graph(
        &self,
        Parameters(params): Parameters<GetFollowerGraphParams>,
    ) -> Result<String, String> {
        let depth = params.depth.unwrap_or(1).clamp(1, 2);

        // Payment gate
        if let Some(ref hash) = params.payment_hash {
            let gw = self
                .nwc_gateway
                .as_ref()
                .ok_or("Payment system not configured")?;
            let paid = gw.verify_payment(hash).await.map_err(|e| e.to_string())?;
            if !paid {
                return Err("Payment not confirmed. Invoice may be unpaid or expired.".into());
            }
        } else {
            let under_limit = self
                .rate_limiter
                .check_and_increment("stdio", self.config.free_tier.calls_per_day)
                .await;
            if !under_limit {
                let gw = self
                    .nwc_gateway
                    .as_ref()
                    .ok_or("Free tier exhausted and payment system not configured")?;
                let amount = self.calculate_follower_graph_price(depth);
                let inv = gw
                    .create_invoice(
                        "get_follower_graph",
                        amount,
                        "nostr-intel: get_follower_graph",
                        self.config.payment.invoice_expiry_seconds,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let resp = PaymentRequiredResponse {
                    payment_required: true,
                    tool_name: "get_follower_graph".into(),
                    amount_sats: amount,
                    invoice: inv.invoice,
                    payment_hash: inv.payment_hash,
                    message: format!("Free tier exhausted. Payment required: {amount} sats. Pay the invoice, then retry with the payment_hash parameter."),
                };
                return serde_json::to_string_pretty(&resp).map_err(|e| e.to_string());
            }
        }

        // Execute
        let pubkey = NostrClient::parse_pubkey(params.pubkey.trim())
            .map_err(|e| format!("Invalid pubkey: {e}"))?;
        let pubkey_hex = pubkey.to_hex();

        // Fetch the target's contact list (who they follow)
        let contact_list = self
            .nostr_client
            .fetch_contact_list(&pubkey)
            .await
            .map_err(|e| format!("Failed to fetch contact list: {e}"))?;

        let mut following: Vec<PubkeySummary> = Vec::new();
        let mut following_set: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Some(ref cl) = contact_list {
            for tag in cl.tags.iter() {
                let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
                if tag_vec.first() == Some(&"p") {
                    if let Some(pk) = tag_vec.get(1) {
                        following_set.insert(pk.to_string());
                        following.push(PubkeySummary {
                            pubkey: pk.to_string(),
                            name: None,
                        });
                    }
                }
            }
        }

        // Try to resolve names from cache for following
        for f in &mut following {
            if let Ok(Some(cached)) = self.cache.get_profile(&f.pubkey).await {
                f.name = cached.name.or(cached.display_name);
            }
        }

        let following_count = following.len() as u32;

        // Fetch followers: kind:3 events that have our target in their p tags
        // This is expensive — we search for contact lists referencing this pubkey
        let follower_filter = Filter::new()
            .kind(Kind::ContactList)
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), pubkey_hex.clone())
            .limit(100);

        let follower_events = self
            .nostr_client
            .client()
            .fetch_events(follower_filter, std::time::Duration::from_secs(15))
            .await
            .map_err(|e| format!("Failed to fetch followers: {e}"))?;

        let mut followers: Vec<PubkeySummary> = Vec::new();
        let mut follower_set: std::collections::HashSet<String> = std::collections::HashSet::new();

        for event in follower_events.iter() {
            let pk_hex = event.pubkey.to_hex();
            if follower_set.insert(pk_hex.clone()) {
                let mut summary = PubkeySummary {
                    pubkey: pk_hex.clone(),
                    name: None,
                };
                if let Ok(Some(cached)) = self.cache.get_profile(&pk_hex).await {
                    summary.name = cached.name.or(cached.display_name);
                }
                followers.push(summary);
            }
        }

        let followers_count = followers.len() as u32;

        // Compute mutual follows
        let mutual_follows: Vec<PubkeySummary> = followers
            .iter()
            .filter(|f| following_set.contains(&f.pubkey))
            .cloned()
            .collect();

        let response = GetFollowerGraphResponse {
            pubkey: pubkey_hex,
            following_count,
            following,
            followers_count,
            followers_sample: followers,
            mutual_follows,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    // ==================== zap_analytics ====================

    #[tool(
        name = "zap_analytics",
        description = "Analyze zap (Lightning tip) activity for a Nostr pubkey. Costs 50 sats after free tier."
    )]
    async fn zap_analytics(
        &self,
        Parameters(params): Parameters<ZapAnalyticsParams>,
    ) -> Result<String, String> {
        // Payment gate
        if let Some(ref hash) = params.payment_hash {
            let gw = self
                .nwc_gateway
                .as_ref()
                .ok_or("Payment system not configured")?;
            let paid = gw.verify_payment(hash).await.map_err(|e| e.to_string())?;
            if !paid {
                return Err("Payment not confirmed. Invoice may be unpaid or expired.".into());
            }
        } else {
            let under_limit = self
                .rate_limiter
                .check_and_increment("stdio", self.config.free_tier.calls_per_day)
                .await;
            if !under_limit {
                let gw = self
                    .nwc_gateway
                    .as_ref()
                    .ok_or("Free tier exhausted and payment system not configured")?;
                let amount = self.config.pricing.zap_analytics;
                let inv = gw
                    .create_invoice(
                        "zap_analytics",
                        amount,
                        "nostr-intel: zap_analytics",
                        self.config.payment.invoice_expiry_seconds,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let resp = PaymentRequiredResponse {
                    payment_required: true,
                    tool_name: "zap_analytics".into(),
                    amount_sats: amount,
                    invoice: inv.invoice,
                    payment_hash: inv.payment_hash,
                    message: format!("Free tier exhausted. Payment required: {amount} sats. Pay the invoice, then retry with the payment_hash parameter."),
                };
                return serde_json::to_string_pretty(&resp).map_err(|e| e.to_string());
            }
        }

        // Execute
        let pubkey = NostrClient::parse_pubkey(params.pubkey.trim())
            .map_err(|e| format!("Invalid pubkey: {e}"))?;

        let timeframe_str = params.timeframe.as_deref().unwrap_or("30d");
        let since_secs =
            parse_timeframe(timeframe_str).map_err(|e| format!("Invalid timeframe: {e}"))?;
        let now = chrono::Utc::now().timestamp() as u64;
        let since = Timestamp::from(now.saturating_sub(since_secs));

        let zap_receipts = self
            .nostr_client
            .fetch_zap_receipts(&pubkey, Some(since))
            .await
            .map_err(|e| format!("Failed to fetch zap receipts: {e}"))?;

        let mut total_sats: u64 = 0;
        let mut zapper_totals: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut note_totals: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut daily_totals: std::collections::BTreeMap<String, (u32, u64)> =
            std::collections::BTreeMap::new();

        for event in &zap_receipts {
            // Parse amount from the zap request description tag or bolt11
            let amount_sats = extract_zap_amount(event);
            total_sats += amount_sats;

            // Extract zapper pubkey from uppercase P tag (sender's pubkey in zap request)
            // or from the embedded zap request in the description tag
            let zapper_pk = extract_zapper_pubkey(event);
            if let Some(ref pk) = zapper_pk {
                *zapper_totals.entry(pk.clone()).or_default() += amount_sats;
            }

            // Extract zapped note from e tag
            for tag in event.tags.iter() {
                let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
                if tag_vec.first() == Some(&"e") {
                    if let Some(note_id) = tag_vec.get(1) {
                        *note_totals.entry(note_id.to_string()).or_default() += amount_sats;
                    }
                }
            }

            // Group by date
            let date = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let entry = daily_totals.entry(date).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += amount_sats;
        }

        let total_zaps_count = zap_receipts.len() as u32;
        let avg_zap_sats = if total_zaps_count > 0 {
            total_sats / total_zaps_count as u64
        } else {
            0
        };

        // Top zappers
        let mut zapper_vec: Vec<(String, u64)> = zapper_totals.into_iter().collect();
        zapper_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_zappers: Vec<ZapperSummary> = Vec::new();
        for (pk, sats) in zapper_vec.into_iter().take(10) {
            let name = if let Ok(Some(cached)) = self.cache.get_profile(&pk).await {
                cached.name.or(cached.display_name)
            } else {
                None
            };
            top_zappers.push(ZapperSummary {
                pubkey: pk,
                name,
                total_sats: sats,
            });
        }

        // Top zapped notes
        let mut note_vec: Vec<(String, u64)> = note_totals.into_iter().collect();
        note_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_zapped_notes: Vec<ZappedNote> = note_vec
            .into_iter()
            .take(10)
            .map(|(note_id, sats)| ZappedNote {
                note_id,
                content_preview: String::new(),
                total_sats: sats,
            })
            .collect();

        // Zaps over time
        let zaps_over_time: Vec<ZapPeriod> = daily_totals
            .into_iter()
            .map(|(date, (count, sats))| ZapPeriod { date, count, sats })
            .collect();

        let response = ZapAnalyticsResponse {
            total_received_sats: total_sats,
            total_zaps_count,
            avg_zap_sats,
            top_zappers,
            top_zapped_notes,
            zaps_over_time,
        };

        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }

    // ==================== pricing helpers ====================

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

    fn calculate_follower_graph_price(&self, depth: u8) -> u64 {
        if depth >= 2 {
            self.config.pricing.get_follower_graph * 2
        } else {
            self.config.pricing.get_follower_graph
        }
    }
}

// ==================== SharedState for HTTP transport ====================

/// Shared state that can be cloned across sessions (all fields are Arc-wrapped).
pub struct SharedState {
    pub config: Arc<Config>,
    pub nostr_client: Arc<NostrClient>,
    pub cache: Arc<Cache>,
    pub nwc_gateway: Option<Arc<NwcGateway>>,
    pub rate_limiter: Arc<FreeTierLimiter>,
}

impl NostrIntelServer {
    /// Extract the shared state from an existing server instance.
    pub fn shared_state(&self) -> SharedState {
        SharedState {
            config: Arc::clone(&self.config),
            nostr_client: Arc::clone(&self.nostr_client),
            cache: Arc::clone(&self.cache),
            nwc_gateway: self.nwc_gateway.clone(),
            rate_limiter: Arc::clone(&self.rate_limiter),
        }
    }

    /// Create a new server instance from shared state (for per-session HTTP factory).
    pub fn from_shared(state: &SharedState) -> Self {
        Self {
            config: Arc::clone(&state.config),
            nostr_client: Arc::clone(&state.nostr_client),
            cache: Arc::clone(&state.cache),
            nwc_gateway: state.nwc_gateway.clone(),
            rate_limiter: Arc::clone(&state.rate_limiter),
            tool_router: Self::tool_router(),
        }
    }
}

// ==================== helper functions ====================

/// Parse timeframe strings like "1h", "24h", "7d", "30d", "90d", "1y" into seconds
fn parse_timeframe(tf: &str) -> Result<u64, String> {
    let tf = tf.trim().to_lowercase();
    if let Some(hours) = tf.strip_suffix('h') {
        let h: u64 = hours
            .parse()
            .map_err(|_| format!("Invalid hours: {hours}"))?;
        Ok(h * 3600)
    } else if let Some(days) = tf.strip_suffix('d') {
        let d: u64 = days.parse().map_err(|_| format!("Invalid days: {days}"))?;
        Ok(d * 86400)
    } else if let Some(years) = tf.strip_suffix('y') {
        let y: u64 = years
            .parse()
            .map_err(|_| format!("Invalid years: {years}"))?;
        Ok(y * 365 * 86400)
    } else {
        Err(format!(
            "Unknown timeframe format: {tf}. Use '1h', '24h', '7d', '30d', etc."
        ))
    }
}

/// Truncate content to a max length, appending "..." if truncated
fn truncate_content(content: &str, max_len: usize) -> String {
    if content.len() > max_len {
        format!("{}...", &content[..max_len])
    } else {
        content.to_string()
    }
}

/// Extract zap amount in sats from a kind:9735 zap receipt event.
/// Tries the `bolt11` tag first, then the embedded zap request `description` tag.
fn extract_zap_amount(event: &Event) -> u64 {
    // Try bolt11 tag
    for tag in event.tags.iter() {
        let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
        if tag_vec.first() == Some(&"bolt11") {
            if let Some(bolt11) = tag_vec.get(1) {
                if let Some(amount) = parse_bolt11_amount(bolt11) {
                    return amount;
                }
            }
        }
    }

    // Try description tag (embedded zap request with amount)
    for tag in event.tags.iter() {
        let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
        if tag_vec.first() == Some(&"description") {
            if let Some(desc) = tag_vec.get(1) {
                if let Ok(zap_request) = serde_json::from_str::<serde_json::Value>(desc) {
                    // Look for amount tag in the zap request
                    if let Some(tags) = zap_request["tags"].as_array() {
                        for t in tags {
                            if let Some(arr) = t.as_array() {
                                if arr.first().and_then(|v| v.as_str()) == Some("amount") {
                                    if let Some(msats_str) = arr.get(1).and_then(|v| v.as_str()) {
                                        if let Ok(msats) = msats_str.parse::<u64>() {
                                            return msats / 1000;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    0
}

/// Parse amount from a bolt11 invoice string.
/// Bolt11 amounts: number followed by multiplier (m=milli, u=micro, n=nano, p=pico)
fn parse_bolt11_amount(bolt11: &str) -> Option<u64> {
    let lower = bolt11.to_lowercase();
    // Find "lnbc"/"lntb"/"lnbcrt" prefix and extract the amount portion
    let after_prefix = if let Some(rest) = lower.strip_prefix("lnbcrt") {
        rest
    } else if let Some(rest) = lower.strip_prefix("lnbc") {
        rest
    } else if let Some(rest) = lower.strip_prefix("lntb") {
        rest
    } else {
        return None;
    };

    // Amount is digits + optional multiplier before the first '1' separator
    let sep_pos = after_prefix.find('1')?;
    let amount_str = &after_prefix[..sep_pos];

    if amount_str.is_empty() {
        return None; // No amount specified
    }

    // Check for multiplier suffix
    if let Some(n) = amount_str.strip_suffix('m') {
        let num: u64 = n.parse().ok()?;
        Some(num * 100_000) // milli-BTC to sats
    } else if let Some(n) = amount_str.strip_suffix('u') {
        let num: u64 = n.parse().ok()?;
        Some(num * 100) // micro-BTC to sats
    } else if let Some(n) = amount_str.strip_suffix('n') {
        let num: u64 = n.parse().ok()?;
        Some(num / 10) // nano-BTC to sats (0.1 sat each)
    } else if let Some(n) = amount_str.strip_suffix('p') {
        let num: u64 = n.parse().ok()?;
        Some(num / 100) // pico-BTC to sats (0.01 sat each)
    } else {
        let num: u64 = amount_str.parse().ok()?;
        Some(num * 100_000_000) // plain BTC to sats
    }
}

/// Extract the zapper's pubkey from a zap receipt event.
/// Looks for uppercase 'P' tag or parses from embedded zap request description.
fn extract_zapper_pubkey(event: &Event) -> Option<String> {
    // Check for uppercase P tag (zapper's pubkey)
    for tag in event.tags.iter() {
        let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
        if tag_vec.first() == Some(&"P") {
            return tag_vec.get(1).map(|s| s.to_string());
        }
    }

    // Try description tag (embedded zap request)
    for tag in event.tags.iter() {
        let tag_vec: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
        if tag_vec.first() == Some(&"description") {
            if let Some(desc) = tag_vec.get(1) {
                if let Ok(zap_request) = serde_json::from_str::<serde_json::Value>(desc) {
                    return zap_request["pubkey"].as_str().map(String::from);
                }
            }
        }
    }

    None
}
