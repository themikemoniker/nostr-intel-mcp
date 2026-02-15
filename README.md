# nostr-intel-mcp

[![CI](https://github.com/themikemoniker/nostr-intel-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/themikemoniker/nostr-intel-mcp/actions/workflows/ci.yml)
[![Fly Deploy](https://github.com/themikemoniker/nostr-intel-mcp/actions/workflows/fly-deploy.yml/badge.svg)](https://github.com/themikemoniker/nostr-intel-mcp/actions/workflows/fly-deploy.yml)

A Rust MCP (Model Context Protocol) server that provides AI agents with structured intelligence about the Nostr social network. Free tier for basic lookups, paid tier via Bitcoin Lightning payments.

- First Rust MCP server that accepts Bitcoin payments
- First MCP server focused on Nostr network data
- Reference implementation for multi-payment-standard paid MCPs
- Supports stdio and HTTP (Streamable HTTP) transports

## Tools

### Free Tools (10 calls/day)

| Tool | Description |
|------|-------------|
| `decode_nostr_uri` | Decode any Nostr bech32 entity (npub, note, nprofile, nevent, naddr) into its components |
| `resolve_nip05` | Resolve a NIP-05 identifier (user@domain.com) to a Nostr pubkey and relay list |
| `get_profile` | Fetch profile metadata (kind:0) for a pubkey. Accepts hex, npub, or NIP-05 |
| `check_relay` | Check a relay's online status, latency, and NIP-11 info document |

### Paid Tools (Lightning, after free tier)

| Tool | Cost | Description |
|------|------|-------------|
| `search_events` | 10-50 sats | Search events across relays with NIP-01 filters |
| `relay_discovery` | 20 sats | Discover relays used by a pubkey via NIP-65 relay list metadata |
| `trending_notes` | 20 sats | Find trending notes by reactions, reposts, and zaps |
| `get_follower_graph` | 50-100 sats | Get follower/following graph with mutual follows |
| `zap_analytics` | 50 sats | Analyze zap activity for a pubkey |

## Quick Start

### Prerequisites

- Rust 1.83+
- A Nostr Wallet Connect (NWC) URL for paid tools (optional)

### Build

```bash
git clone https://github.com/themikemoniker/nostr-intel-mcp.git
cd nostr-intel-mcp
cp .env.example .env
# Edit .env with your NWC_URL if you want paid tools
cargo build --release
```

### Run (stdio)

```bash
./target/release/nostr-intel-mcp
```

### Run (HTTP)

```bash
MCP_TRANSPORT=http ./target/release/nostr-intel-mcp
# Server listens on http://0.0.0.0:3000
# MCP endpoint: /mcp
# Health check: /health
```

### Connect to Claude Code

**stdio:**
```bash
claude mcp add nostr-intel -- ./target/release/nostr-intel-mcp
```

**HTTP (remote):**
```bash
claude mcp add --transport http nostr-intel https://nostr-intel-mcp.fly.dev/mcp
```

## Payment Flow

1. Agent calls a paid tool (e.g., `search_events`)
2. If free tier (10 calls/day) is not exhausted, results are returned immediately
3. If free tier is exhausted and no `payment_hash` provided, server returns a Lightning invoice
4. Agent pays the invoice (e.g., via Alby MCP)
5. Agent retries the tool call with `payment_hash` parameter
6. Server verifies payment via NWC and returns results

### L402 (HTTP transport)

When L402 is enabled, the `/l402/challenge/{tool_name}` endpoint returns:
- HTTP 402 with `WWW-Authenticate: L402` header
- Invoice and payment hash in the response body

## Configuration

### config.toml

```toml
[server]
name = "nostr-intel-mcp"
version = "0.1.0"
transport = "stdio"    # "stdio" or "http"
http_port = 3000

[relays]
default = [
  "wss://relay.damus.io",
  "wss://relay.nostr.band",
  "wss://nos.lol",
  "wss://relay.snort.social",
  "wss://purplepag.es",
  "wss://relay.primal.net",
]

[cache]
database_path = "nostr_cache.db"
profile_ttl_seconds = 3600
relay_info_ttl_seconds = 3600

[free_tier]
calls_per_day = 10

[pricing]
search_events_base = 10
relay_discovery = 20
trending_notes = 20
get_follower_graph = 50
zap_analytics = 50

[payment]
nwc_url = ""
invoice_expiry_seconds = 600
l402_secret = ""
enable_l402 = false
enable_x402 = false
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level (default: `info,nostr_intel_mcp=debug`) |
| `NWC_URL` | Nostr Wallet Connect URI for invoice generation |
| `L402_SECRET` | Hex-encoded secret for L402 token signing (min 32 bytes) |
| `MCP_TRANSPORT` | Override transport: `stdio` or `http` |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     AI Agent (Claude)                    │
└──────────────┬──────────────────────────┬───────────────┘
               │ stdio (JSON-RPC)         │ HTTP (/mcp)
               ▼                          ▼
┌─────────────────────────────────────────────────────────┐
│                   nostr-intel-mcp                        │
│                                                         │
│  ┌───────────┐  ┌──────────┐  ┌──────────────────────┐ │
│  │ Free Tools│  │Paid Tools│  │   Payment Gate       │ │
│  │           │  │          │  │                      │ │
│  │ decode    │  │ search   │  │ Free tier limiter    │ │
│  │ nip05     │  │ relay    │  │ NWC invoice gen      │ │
│  │ profile   │  │ trending │  │ L402 tokens (HMAC)   │ │
│  │ relay     │  │ follower │  │ x402 stub            │ │
│  │           │  │ zaps     │  │                      │ │
│  └───────────┘  └──────────┘  └──────────────────────┘ │
│                                                         │
│  ┌─────────────────┐  ┌─────────────────────────────┐  │
│  │  Nostr Client   │  │     SQLite Cache            │  │
│  │  (nostr-sdk)    │  │  profiles, relay info (TTL) │  │
│  └────────┬────────┘  └─────────────────────────────┘  │
└───────────┼─────────────────────────────────────────────┘
            │
            ▼
   ┌─────────────────┐
   │  Nostr Relays    │
   │  (relay pool)    │
   └─────────────────┘
```

## Deployment

### Docker

```bash
docker build -t nostr-intel-mcp .
docker run -p 3000:3000 \
  -e MCP_TRANSPORT=http \
  -e NWC_URL="nostr+walletconnect://..." \
  nostr-intel-mcp
```

### Fly.io

```bash
fly launch
fly secrets set NWC_URL="nostr+walletconnect://..." L402_SECRET="your-hex-secret"
fly deploy
```

## Roadmap

### Done

- [x] **Free tools** — `decode_nostr_uri`, `resolve_nip05`, `get_profile`, `check_relay`, `search_profiles`
- [x] **Paid tools** — `search_events`, `relay_discovery`, `trending_notes`, `get_follower_graph`, `zap_analytics`
- [x] **NWC payment gate** — Lightning invoice generation + verification via Nostr Wallet Connect
- [x] **Free tier rate limiter** — 10 calls/day per session, SQLite-backed
- [x] **L402 authentication** — HMAC-based token minting and verification for HTTP transport
- [x] **Dual transport** — stdio (JSON-RPC) and HTTP (Streamable HTTP via axum)
- [x] **SQLite cache** — profiles and relay info with configurable TTL
- [x] **Primal search integration** — fuzzy profile search by name via Primal's cache API
- [x] **Fly.io deployment** — Docker multi-stage build, live at `nostr-intel-mcp.fly.dev`
- [x] **CI pipeline** — GitHub Actions: fmt, check, clippy, test, Docker build
- [x] **Unit tests** — decode_nostr_uri parsing, rate limiter logic, L402 token lifecycle

### Up Next

- [ ] **x402 payments** — real USDC-on-Base payment verification (currently a stub)
- [ ] **Profile enrichment** — resolve names for pubkeys in `trending_notes`, `search_events` responses
- [ ] **WebSocket relay probing** — actual WS connect in `check_relay` alongside NIP-11 HTTP check
- [ ] **Background cache cleanup** — periodic task to evict expired profiles, relay info, and stale rate limit rows
- [ ] **Relay health monitoring** — scheduled checks across default relay pool with uptime tracking
- [ ] **HTTP auth / API keys** — optional bearer token auth for HTTP transport sessions
- [ ] **NIP-50 search improvements** — broader relay support and fallback strategies for full-text search
- [ ] **Streaming responses** — chunked delivery for large result sets (follower graphs, event searches)
- [ ] **Per-tool analytics** — track usage stats per tool for observability and pricing tuning

## Development

```bash
cargo check          # Fast compile check
cargo test           # Run tests
cargo clippy         # Lint
cargo build --release
```

## License

MIT
