use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

// ==================== search_events ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchEventsParams {
    /// Filter by author public keys (hex or npub)
    pub authors: Option<Vec<String>>,
    /// Filter by event kinds (e.g., 1 for text notes)
    pub kinds: Option<Vec<u32>>,
    /// Full-text search (NIP-50)
    pub search: Option<String>,
    /// Only events from the last N hours
    pub since_hours: Option<u64>,
    /// Maximum number of events to return (default: 20, max: 100)
    pub limit: Option<u32>,
    /// Payment hash from a paid Lightning invoice (required after free tier exhausted)
    pub payment_hash: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchEventsResponse {
    pub events: Vec<EventSummary>,
    pub count: u32,
    pub relays_queried: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EventSummary {
    pub id: String,
    pub pubkey: String,
    pub kind: u32,
    pub content: String,
    pub created_at: u64,
    pub tags_summary: String,
}

// ==================== relay_discovery ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelayDiscoveryParams {
    /// Public key (hex or npub) to discover relays for
    pub pubkey: String,
    /// Payment hash from a paid Lightning invoice (required after free tier exhausted)
    pub payment_hash: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RelayDiscoveryResponse {
    pub write_relays: Vec<String>,
    pub read_relays: Vec<String>,
    pub last_event_seen: Option<LastEventSeen>,
    pub recommended_relays: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LastEventSeen {
    pub relay: String,
    pub timestamp: u64,
}

// ==================== trending_notes ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrendingNotesParams {
    /// Timeframe: "1h", "24h", "7d" (default "24h")
    pub timeframe: Option<String>,
    /// Maximum number of trending notes to return (default: 20, max: 50)
    pub limit: Option<u32>,
    /// Payment hash from a paid Lightning invoice (required after free tier exhausted)
    pub payment_hash: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TrendingNotesResponse {
    pub notes: Vec<TrendingNote>,
    pub timeframe: String,
    pub count: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TrendingNote {
    pub id: String,
    pub author_pubkey: String,
    pub author_name: Option<String>,
    pub content_preview: String,
    pub reactions: u32,
    pub reposts: u32,
    pub zap_total_sats: u64,
    pub score: u64,
    pub created_at: u64,
}

// ==================== get_follower_graph ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFollowerGraphParams {
    /// Public key (hex or npub) to get the follower graph for
    pub pubkey: String,
    /// Graph depth: 1 (default) or 2 (more expensive)
    pub depth: Option<u8>,
    /// Payment hash from a paid Lightning invoice (required after free tier exhausted)
    pub payment_hash: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetFollowerGraphResponse {
    pub pubkey: String,
    pub following_count: u32,
    pub following: Vec<PubkeySummary>,
    pub followers_count: u32,
    pub followers_sample: Vec<PubkeySummary>,
    pub mutual_follows: Vec<PubkeySummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PubkeySummary {
    pub pubkey: String,
    pub name: Option<String>,
}

// ==================== zap_analytics ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ZapAnalyticsParams {
    /// Public key (hex or npub) to analyze zaps for
    pub pubkey: String,
    /// Timeframe: "7d", "30d" (default), "90d", "1y"
    pub timeframe: Option<String>,
    /// Payment hash from a paid Lightning invoice (required after free tier exhausted)
    pub payment_hash: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ZapAnalyticsResponse {
    pub total_received_sats: u64,
    pub total_zaps_count: u32,
    pub avg_zap_sats: u64,
    pub top_zappers: Vec<ZapperSummary>,
    pub top_zapped_notes: Vec<ZappedNote>,
    pub zaps_over_time: Vec<ZapPeriod>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ZapperSummary {
    pub pubkey: String,
    pub name: Option<String>,
    pub total_sats: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ZappedNote {
    pub note_id: String,
    pub content_preview: String,
    pub total_sats: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ZapPeriod {
    pub date: String,
    pub count: u32,
    pub sats: u64,
}

// ==================== payment required ====================

#[derive(Debug, Serialize, JsonSchema)]
pub struct PaymentRequiredResponse {
    pub payment_required: bool,
    pub tool_name: String,
    pub amount_sats: u64,
    pub invoice: String,
    pub payment_hash: String,
    pub message: String,
}
