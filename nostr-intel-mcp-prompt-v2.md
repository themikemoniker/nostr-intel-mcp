# nostr-intel-mcp — Claude Code Project Prompt

## What We're Building

A Rust MCP (Model Context Protocol) server called `nostr-intel-mcp` that provides AI agents with structured intelligence about the Nostr social network. It has a free tier for basic lookups and a paid tier that accepts Bitcoin Lightning payments via three different standards (NWC/PaidMCP, L402, and x402).

This will be:
- The first Rust MCP server that accepts Bitcoin payments
- The first MCP server focused on Nostr network data
- A reference implementation for multi-payment-standard paid MCPs
- Open source (MIT license), deployed on Fly.io

## Tech Stack

- **Language:** Rust (2021 edition)
- **MCP SDK:** `rmcp` (official Rust MCP SDK by Anthropic) — use latest version from crates.io, with features: `server`, `transport-io`, `transport-sse-server`, `macros`
- **Async runtime:** `tokio` with full features
- **Nostr:** `nostr-sdk` crate (by rust-nostr) — use features: `nip05`, `nip11`, `nip47`, `nip57`
- **HTTP framework:** `axum` 0.7+ (for HTTP streamable MCP transport + L402/x402 middleware)
- **Database:** `sqlx` with SQLite (for caching profiles, events, relay stats)
- **Lightning payments:**
  - `l402_middleware` crate for L402 macaroon generation/verification
  - NWC (Nostr Wallet Connect / NIP-47) via `nostr-sdk`'s nip47 feature for invoice generation
  - x402 can be a stub/placeholder initially
- **Serialization:** `serde` + `serde_json`
- **Error handling:** `anyhow` + `thiserror`
- **Logging:** `tracing` + `tracing-subscriber`
- **Schema generation:** `schemars` (for MCP tool JSON schemas)
- **HTTP client:** `reqwest` (for NIP-05 DNS lookups, external API calls)
- **Deployment:** Fly.io via multi-stage Dockerfile

## Project Structure

```
nostr-intel-mcp/
├── Cargo.toml
├── Dockerfile                  # Multi-stage: builder + minimal runtime
├── fly.toml                    # Fly.io deployment config
├── config.toml                 # Default relay list, pricing, settings
├── .env.example                # NWC_URL, L402 secrets, etc.
├── LICENSE                     # MIT
├── README.md
├── src/
│   ├── main.rs                 # Entry point: parse args, select transport (stdio vs HTTP), start server
│   ├── config.rs               # Load config from file + env vars
│   ├── server.rs               # MCP ServerHandler implementation — routes tool calls
│   ├── tools/
│   │   ├── mod.rs              # Tool registry, tool definitions with pricing metadata
│   │   ├── free.rs             # Free tools: resolve_nip05, get_profile, check_relay, decode_nostr_uri
│   │   └── paid.rs             # Paid tools: search_events, get_follower_graph, relay_discovery, trending_notes, zap_analytics
│   ├── payment/
│   │   ├── mod.rs              # PaymentGate trait + PaymentStatus/PaymentChallenge enums
│   │   ├── free_tier.rs        # Rate limiter: track calls per client, 10 free/day
│   │   ├── nwc.rs              # NWC invoice generation via NIP-47, payment verification
│   │   ├── l402.rs             # L402 macaroon minting, caveat creation, verification
│   │   └── x402.rs             # x402 stub — USDC payment details generation, placeholder verification
│   ├── nostr/
│   │   ├── mod.rs
│   │   ├── client.rs           # nostr-sdk Client wrapper, relay pool init, connection management
│   │   ├── cache.rs            # SQLite cache: store/retrieve profiles, events, relay info with TTL
│   │   └── analytics.rs        # Derived data: follower graphs, zap aggregation, trending calculation
│   └── error.rs                # Custom error types
└── tests/
    ├── free_tools_test.rs
    ├── payment_test.rs
    └── integration_test.rs
```

## MCP Tools Specification

### Free Tools (10 calls/day per client, no payment required)

#### `resolve_nip05`
- **Description:** Resolve a NIP-05 identifier (user@domain.com) to a Nostr pubkey and relay list
- **Input:** `{ "nip05": "string" }` — e.g. "jack@cash.app"
- **Output:** `{ "pubkey": "string (hex)", "pubkey_npub": "string (bech32)", "relays": ["string"] }`
- **Implementation:** HTTP GET to `https://{domain}/.well-known/nostr.json?name={user}`, parse response, cache result

#### `get_profile`
- **Description:** Fetch Nostr profile metadata (kind:0) for a given pubkey
- **Input:** `{ "pubkey": "string" }` — accepts hex, npub, or NIP-05
- **Output:** `{ "pubkey": "string", "name": "string", "display_name": "string", "about": "string", "picture": "string", "banner": "string", "nip05": "string", "lud16": "string", "website": "string" }`
- **Implementation:** If input is NIP-05, resolve first. Query relay pool for kind:0 event. Cache in SQLite with 1hr TTL.

#### `check_relay`
- **Description:** Check a relay's status, NIP-11 info document, and latency
- **Input:** `{ "relay_url": "string" }` — e.g. "wss://relay.damus.io"
- **Output:** `{ "online": "bool", "latency_ms": "u64", "name": "string", "description": "string", "supported_nips": "[u32]", "software": "string", "version": "string", "limitation": { "max_message_length": "u64", "max_event_tags": "u64" } }`
- **Implementation:** HTTP GET to relay URL with Accept: application/nostr+json header, measure latency, also try WebSocket connect

#### `decode_nostr_uri`
- **Description:** Decode any Nostr bech32 entity (npub, note, nprofile, nevent, naddr)
- **Input:** `{ "uri": "string" }` — e.g. "nevent1..."
- **Output:** `{ "type": "string", "hex_id": "string", "relays": ["string"], "author_hex": "string?", "kind": "u32?" }`
- **Implementation:** Pure decode using nostr-sdk's bech32 parsing, no network calls needed

### Paid Tools

#### `search_events` — 10-50 sats
- **Description:** Search Nostr events across multiple relays with NIP-01 filters
- **Input:** `{ "authors": ["string"]?, "kinds": [u32]?, "search": "string"?, "since_hours": "u64"?, "limit": "u32"?, "payment_hash": "string"? }`
- **Output:** `{ "events": [{ "id": "string", "pubkey": "string", "kind": "u32", "content": "string (truncated)", "created_at": "u64", "tags_summary": "string" }], "count": "u32", "relays_queried": ["string"] }`
- **Pricing:** 10 sats for limit <= 20, 25 sats for limit <= 50, 50 sats for limit > 50

#### `get_follower_graph` — 50-100 sats
- **Description:** Build follower/following graph for a pubkey using kind:3 contact lists
- **Input:** `{ "pubkey": "string", "depth": "u8"?, "payment_hash": "string"? }` — depth defaults to 1, max 2
- **Output:** `{ "pubkey": "string", "following_count": "u32", "following": [{ "pubkey": "string", "name": "string?" }], "followers_count": "u32", "followers_sample": [{ "pubkey": "string", "name": "string?" }], "mutual_follows": [{ "pubkey": "string", "name": "string?" }] }`
- **Pricing:** 50 sats for depth 1, 100 sats for depth 2

#### `relay_discovery` — 20 sats
- **Description:** Find relays where a pubkey is active (NIP-65 relay lists + event sightings)
- **Input:** `{ "pubkey": "string", "payment_hash": "string"? }`
- **Output:** `{ "write_relays": ["string"], "read_relays": ["string"], "last_event_seen": { "relay": "string", "timestamp": "u64" }, "recommended_relays": ["string"] }`

#### `trending_notes` — 20 sats
- **Description:** Aggregated trending content across relays (by reaction count, reposts, zaps)
- **Input:** `{ "timeframe": "string"?, "limit": "u32"?, "payment_hash": "string"? }` — timeframe: "1h", "24h", "7d" (default "24h"), limit default 20
- **Output:** `{ "notes": [{ "id": "string", "author_name": "string", "content_preview": "string", "reactions": "u32", "reposts": "u32", "zap_total_sats": "u64", "created_at": "u64" }] }`

#### `zap_analytics` — 50 sats
- **Description:** Analyze zap receipts (kind:9735) for a pubkey
- **Input:** `{ "pubkey": "string", "timeframe": "string"?, "payment_hash": "string"? }` — timeframe default "30d"
- **Output:** `{ "total_received_sats": "u64", "total_zaps_count": "u32", "avg_zap_sats": "u64", "top_zappers": [{ "pubkey": "string", "name": "string?", "total_sats": "u64" }], "top_zapped_notes": [{ "note_id": "string", "content_preview": "string", "total_sats": "u64" }], "zaps_over_time": [{ "date": "string", "count": "u32", "sats": "u64" }] }`

## Payment Flow

When an agent calls a paid tool:

1. Server checks if client has free calls remaining (tracked by transport session or IP)
2. If free calls exhausted and no `payment_hash` in params, server returns a tool result (NOT an error) containing payment instructions:

```json
{
  "content": [{
    "type": "text",
    "text": "Payment required: 20 sats for trending_notes.\n\nLightning invoice: lnbc200n1p...\nPayment hash: abc123...\n\nPay this invoice using your Lightning wallet, then call this tool again with the same parameters plus add \"payment_hash\": \"abc123...\" to get your results."
  }]
}
```

3. The agent (which should have Alby MCP or similar Lightning wallet connected) pays the invoice
4. Agent retries the tool call with the `payment_hash` included in params
5. Server verifies payment was received (checks via NWC that the invoice was paid), then returns the actual data

For L402 (HTTP transport only):
- Return HTTP 402 with `WWW-Authenticate: L402 macaroon="base64...", invoice="lnbc..."` header
- Client pays and retries with `Authorization: L402 macaroon:preimage` header

For x402 (HTTP transport, stub for now):
- Return HTTP 402 with payment details for USDC on Base
- Include payment address, amount, chain_id in response headers

## Configuration

```toml
# config.toml
[server]
name = "nostr-intel-mcp"
version = "0.1.0"
transport = "stdio"           # "stdio" or "http"
http_port = 3000

[free_tier]
calls_per_day = 10

[relays]
default = [
  "wss://relay.damus.io",
  "wss://relay.nostr.band",
  "wss://nos.lol",
  "wss://relay.snort.social",
  "wss://purplepag.es",
  "wss://relay.primal.net",
]

[pricing]
search_events_base = 10       # sats
get_follower_graph = 50
relay_discovery = 20
trending_notes = 20
zap_analytics = 50

[payment]
nwc_url = ""                  # from env: NWC_URL
l402_secret = ""              # from env: L402_SECRET (root key for macaroon signing)
enable_l402 = true
enable_x402 = false           # stub for now
```

Environment variables (.env):
```
NWC_URL=nostr+walletconnect://...
L402_SECRET=your-random-32-byte-hex-secret
RUST_LOG=info,nostr_intel_mcp=debug
```

## Deployment (Fly.io)

Dockerfile should be a multi-stage build:
1. Builder stage: `rust:1.83-slim-bookworm`, install build deps (pkg-config, libssl-dev, build-essential), `cargo build --release`
2. Runtime stage: `debian:bookworm-slim`, copy binary + config, expose port 3000
3. Runtime needs: `libssl3`, `ca-certificates`

fly.toml:
```toml
app = "nostr-intel-mcp"
primary_region = "iad"

[build]

[http_service]
  internal_port = 3000
  force_https = true
  auto_stop_machines = "stop"
  auto_start_machines = true
  min_machines_running = 0

[env]
  RUST_LOG = "info,nostr_intel_mcp=debug"
```

Secrets set via: `fly secrets set NWC_URL="nostr+walletconnect://..." L402_SECRET="..."`

## Build Order / Phases

### Phase 1: Skeleton + Free Tools (start here)
1. Initialize Cargo project with all dependencies
2. Implement `main.rs` with stdio transport using rmcp
3. Implement `decode_nostr_uri` (pure computation, no network needed — good first tool)
4. Implement `resolve_nip05` (simple HTTP call)
5. Implement `get_profile` (connects nostr-sdk to relays, fetches kind:0)
6. Implement `check_relay` (HTTP + WebSocket connection test)
7. Test all 4 tools with `claude mcp add` locally
8. Set up SQLite cache for profiles and relay info

### Phase 2: Paid Tools + NWC Payment
9. Implement free tier rate limiter (in-memory HashMap with daily reset)
10. Implement `search_events` (relay pool query with NIP-01 filters)
11. Implement NWC payment gate: generate invoice via NIP-47, verify payment
12. Wire payment gate into tool handler: check free tier -> require payment -> verify -> serve
13. Test payment flow with Alby Hub + Alby MCP connected to Claude Code

### Phase 3: More Paid Tools
14. Implement `relay_discovery` (NIP-65 kind:10002 lookups)
15. Implement `trending_notes` (aggregate reactions/zaps across cached events)
16. Implement `get_follower_graph` (kind:3 contact list crawling)
17. Implement `zap_analytics` (kind:9735 receipt aggregation)

### Phase 4: L402 + HTTP Transport + Deployment
18. Add axum-based HTTP transport for MCP (streamable HTTP)
19. Implement L402 middleware: macaroon creation with caveats, verification
20. Add x402 stub (return correct headers, don't verify yet)
21. Write Dockerfile + fly.toml
22. Deploy to Fly.io
23. Test remote MCP: `claude mcp add --transport http nostr-intel https://nostr-intel-mcp.fly.dev/mcp`

### Phase 5: Polish
24. README with usage examples, payment instructions, architecture diagram
25. Background tasks: periodic relay health checks, cache cleanup
26. Error handling improvements, graceful relay disconnection handling
27. GitHub Actions CI: cargo test, cargo clippy, build Docker image

## Important Implementation Notes

- **Use rmcp's `#[tool]` macro** for defining tools when possible — it auto-generates JSON schemas from struct definitions
- **nostr-sdk Client is async** — initialize it in main, pass via Arc to the server handler
- **SQLite via sqlx**: use `sqlx::sqlite::SqlitePoolOptions` with WAL mode for concurrent reads
- **Payment hash as verification**: when NWC invoice is generated, store `payment_hash -> (tool_name, params, expiry)` in memory (DashMap or tokio RwLock<HashMap>). When client retries with payment_hash, check NWC for payment status, then serve result.
- **Keep tool responses concise** — agents have context limits. Truncate content fields to ~280 chars, limit arrays to reasonable sizes.
- **Relay connections**: connect to relay pool on startup, handle disconnections gracefully, don't panic on individual relay failures
- **L402 macaroons**: use the `macaroon` crate or implement manually — a macaroon is essentially HMAC-chained caveats. Root key signs the base, each caveat chains another HMAC. Caveats: `tool = <tool_name>`, `expires = <unix_timestamp>`, `uses = 1`.
- **NWC flow**: the server acts as a service provider. It generates a Lightning invoice using its own NWC connection (to its Alby Hub), and verifies payment receipt via the same NWC connection.
- **Start with Phase 1 only.** Get the free tools compiling and working before touching payments. Rust compilation errors compound fast if you try to build everything at once.

## Reference Links

- rmcp (official Rust MCP SDK): https://github.com/modelcontextprotocol/rust-sdk
- nostr-sdk (Rust): https://github.com/rust-nostr/nostr / https://crates.io/crates/nostr-sdk
- rust-nostr book: https://rust-nostr.org/
- l402_middleware: https://github.com/DhananjayPurohit/l402_middleware
- rust_l402: https://crates.io/crates/rust_l402
- L402 spec: https://github.com/lightninglabs/L402
- PaidMCP (TypeScript reference): https://github.com/getAlby/paidmcp
- Alby MCP: https://github.com/getAlby/mcp
- NIP-47 (NWC): https://github.com/nostr-protocol/nips/blob/master/47.md
- NIP-05: https://github.com/nostr-protocol/nips/blob/master/05.md
- NIP-11: https://github.com/nostr-protocol/nips/blob/master/11.md
- NIP-65: https://github.com/nostr-protocol/nips/blob/master/65.md
- x402 protocol: https://www.x402.org/
- Fly.io Rust deployment: https://fly.io/docs/languages-and-frameworks/rust/
