# CLAUDE.md — nostr-intel-mcp

## Project Overview

Rust MCP server providing AI agents with Nostr network intelligence. Free tier for basic lookups, paid tier (future) via Lightning payments (NWC, L402, x402).

## Build & Test

```bash
cargo check              # Fast compile check
cargo build --release    # Release binary → target/release/nostr-intel-mcp
cargo test               # Run tests
cargo clippy             # Lint
```

The server requires `config.toml` in the working directory. The binary communicates via stdio (stdin/stdout = MCP JSON-RPC protocol).

## Architecture

```
src/
├── main.rs              # Entry point: tracing init, config load, stdio transport
├── config.rs            # Config from config.toml + .env
├── error.rs             # NostrIntelError enum (thiserror)
├── server.rs            # MCP ServerHandler + all tool implementations
├── tools/
│   ├── mod.rs
│   └── free.rs          # Parameter/response structs for free tools
└── nostr/
    ├── mod.rs
    ├── client.rs         # nostr-sdk Client wrapper
    └── cache.rs          # SQLite cache with TTL
```

`server.rs` is the central file — it contains the `NostrIntelServer` struct with rmcp tool macros and all tool logic. Tools return JSON strings via `serde_json::to_string_pretty()`.

## Critical: rmcp Macro Patterns

The rmcp crate (v0.15) uses proc macros that are **very specific about structure**:

```rust
// tool_router goes on the impl block containing new() AND tool functions
#[tool_router(router = tool_router)]
impl MyServer {
    pub fn new() -> Self { ... tool_router: Self::tool_router() ... }

    #[tool(name = "my_tool", description = "...")]
    async fn my_tool(&self, Parameters(p): Parameters<MyParams>) -> Result<String, String> { ... }
}

// tool_handler goes on the ServerHandler impl, pointing to the field
#[tool_handler(router = self.tool_router)]
impl ServerHandler for MyServer {
    fn get_info(&self) -> ServerInfo { ... }
}
```

- `new()` MUST be inside the `#[tool_router]` block (not a separate impl)
- The struct MUST have a `tool_router: ToolRouter<Self>` field
- Tool parameter types need `Deserialize + rmcp::schemars::JsonSchema`
- Tool functions return `String`, `Result<String, String>`, or `Result<Json<T>, String>`

## Critical: schemars Version

rmcp v0.15 depends on **schemars 1.x**. Do NOT add schemars as a direct dependency (crates.io resolves to 0.8 by default, causing trait mismatch). Instead:

```rust
use rmcp::schemars::{self, JsonSchema};
```

## Critical: stdout is MCP Protocol

All logging MUST go to stderr. stdout is exclusively for MCP JSON-RPC messages.

```rust
tracing_subscriber::fmt::layer()
    .with_writer(std::io::stderr)
    .with_ansi(false)
```

## nostr-sdk (v0.44) Gotchas

- `Client::default()` creates a read-only client (no keys needed for Phase 1)
- `client.fetch_events(filter, timeout)` takes a **single Filter**, not Vec
- `Kind::as_u16()` — there is no `as_u32()`
- `Metadata` fields (picture, banner, website) are `Option<String>`, not URL types
- `Nip19Coordinate` has `.coordinate: Coordinate` with inner fields `.kind`, `.public_key`, `.identifier`, plus `.relays` directly
- NIP-05 and NIP-11 support is built into the core crate — they are NOT feature flags

## Phases

- **Phase 1** (done): Free tools — decode_nostr_uri, resolve_nip05, get_profile, check_relay + SQLite cache
- **Phase 2**: Paid tools + NWC payment gate (search_events, rate limiter, invoice generation)
- **Phase 3**: More paid tools (relay_discovery, trending_notes, get_follower_graph, zap_analytics)
- **Phase 4**: L402 + HTTP transport (axum) + Fly.io deployment
- **Phase 5**: Polish, CI, README

## Spec

Full specification is in `nostr-intel-mcp-prompt-v2.md` at the project root.
