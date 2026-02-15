use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

// ==================== decode_nostr_uri ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DecodeNostrUriParams {
    /// Nostr bech32 entity to decode (npub, note, nprofile, nevent, naddr)
    pub uri: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DecodeNostrUriResponse {
    /// Entity type: pubkey, event_id, profile, event, or coordinate
    pub entity_type: String,
    /// Hex-encoded ID
    pub hex_id: String,
    /// Associated relay hints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relays: Option<Vec<String>>,
    /// Author pubkey in hex (for events/coordinates)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_hex: Option<String>,
    /// Event kind (for events/coordinates)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<u32>,
}

// ==================== resolve_nip05 ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveNip05Params {
    /// NIP-05 identifier, e.g. "jack@cash.app"
    pub nip05: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveNip05Response {
    /// Hex-encoded public key
    pub pubkey: String,
    /// Bech32-encoded public key (npub)
    pub pubkey_npub: String,
    /// Relay list from NIP-05 response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relays: Option<Vec<String>>,
}

// ==================== get_profile ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProfileParams {
    /// Public key in hex, npub (bech32), NIP-05 (user@domain), or display name (fuzzy search via Primal)
    pub pubkey: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetProfileResponse {
    /// Hex-encoded public key
    pub pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nip05: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lud16: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// How the profile was matched (e.g. "name_search" for fuzzy name lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_by: Option<String>,
}

// ==================== check_relay ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckRelayParams {
    /// Relay WebSocket URL, e.g. "wss://relay.damus.io"
    pub relay_url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckRelayResponse {
    /// Whether the relay is online and responding
    pub online: bool,
    /// Round-trip latency in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Relay name from NIP-11 info document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Relay description from NIP-11 info document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// List of supported NIP numbers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_nips: Option<Vec<u32>>,
    /// Relay software name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software: Option<String>,
    /// Relay software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

// ==================== search_profiles ====================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchProfilesParams {
    /// Name or keyword to search for
    pub query: String,
    /// Maximum number of profiles to return (default: 5, max: 20)
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchProfilesResponse {
    /// The search query used
    pub query: String,
    /// Matching profiles
    pub profiles: Vec<ProfileSearchResult>,
    /// Number of results returned
    pub count: u32,
    /// Data source used for the search
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileSearchResult {
    /// Hex-encoded public key
    pub pubkey: String,
    /// Bech32-encoded public key (npub)
    pub pubkey_npub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nip05: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lud16: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followers_count: Option<u64>,
}
