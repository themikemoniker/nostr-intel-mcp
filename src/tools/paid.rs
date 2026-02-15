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
