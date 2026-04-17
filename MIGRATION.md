# TurboMCP Migration Guide

This guide helps you migrate between major TurboMCP versions.

Current v3 policy:
- TurboMCP targets MCP `2025-11-25`.
- As of v3.1.0, the default `ProtocolConfig` accepts every stable version (with
  per-version adapters); use `ProtocolConfig::strict(...)` for exact-match.
- Older version notes below are historical migration reference, not active compatibility guidance.

## Table of Contents

- [v3.1.0 Migration (v3.0.x → v3.1.0)](#v310-migration-v30x--v310)
- [v3.0.0 Migration (v2.x → v3.x)](#v300-migration-v2x--v3x)
- [v2.0.0 Migration (v1.x → v2.x)](#v200-migration-v1x--v2x)

---

# v3.1.0 Migration (v3.0.x → v3.1.0)

v3.1.0 lands the audit-remediation pass tracked in
`.strategy/AUDIT_v3.0.13_ACTION_PLAN.md`. Several breaking changes; most are
small ergonomic updates at compile sites. The full per-item context is in the
v3.1.0 entry of `CHANGELOG.md`.

## TokenInfo gains `issued_at`

`TokenInfo` now has an additional public field. The new field is
`#[serde(default)]`, so on-disk caches written by v3.0.x deserialize cleanly as
`issued_at: None`. Construction sites that build `TokenInfo` literally must add
the new field:

```rust
// v3.0.x
let token = TokenInfo {
    access_token, token_type, refresh_token, expires_in, scope,
};

// v3.1.0
let token = TokenInfo {
    access_token, token_type, refresh_token, expires_in,
    issued_at: Some(SystemTime::now()),
    scope,
};
```

`TokenInfo::is_expired()` and `TokenInfo::expires_at()` are new convenience
helpers. `OAuth2Client::is_token_expired(...)` now actually returns `true` for
expired tokens (it didn't before — see CHANGELOG).

## DPoP `validate_proof` / `parse_and_validate_jwt` take `ProofContext`

```rust
// v3.0.x
gen.validate_proof(&proof, "POST", uri, Some(token)).await?;

// v3.1.0
use turbomcp_dpop::ProofContext;
gen.validate_proof(&proof, "POST", uri, Some(token), ProofContext::ResourceServer).await?;
```

For proofs validated at the OAuth token endpoint, pass
`ProofContext::TokenEndpoint`. At a resource server with an access token, the
proof must now carry an `ath` claim — see RFC 9449 §4.3.

## `StreamableHttpClientTransport::new` returns `Result`

```rust
// v3.0.x
let transport = StreamableHttpClientTransport::new(config);

// v3.1.0
let transport = StreamableHttpClientTransport::new(config)?;
```

The error path triggers on bad TLS configuration; pre-3.1 this was an
`expect()` that panicked the calling process.

## `OAuth2Client::authorization_code_flow` returns `SecretString`

```rust
// v3.0.x
let (auth_url, code_verifier): (String, String) =
    client.authorization_code_flow(scopes, state);

// v3.1.0
use secrecy::ExposeSecret;
let (auth_url, code_verifier) = client.authorization_code_flow(scopes, state);
// code_verifier: secrecy::SecretString
let verifier_str = code_verifier.expose_secret();
```

## `ApiKeyProvider` API changes

- `add_api_key` now returns `McpResult<()>` and rejects keys shorter than
  `MIN_API_KEY_LENGTH` (32 chars).
- `list_api_keys` is removed — keys are stored as digests at rest, so plaintext
  listing is not possible. Use `api_key_count()` for count.

```rust
// v3.0.x
provider.add_api_key(key, user_info).await;
let all = provider.list_api_keys().await;

// v3.1.0
provider.add_api_key(key, user_info).await?;
let n = provider.api_key_count().await;
```

## `JwtValidator::new` applies SSRF protection by default

`JwtValidator::new(issuer, audience)` now wraps `SsrfValidator::default()`,
which blocks loopback / RFC 1918 / link-local / cloud metadata addresses on
the OIDC discovery fetch. For tests against private OIDC providers, switch to
`JwtValidator::new_unchecked(...)` (or supply a custom `SsrfValidator` via
`new_with_ssrf`). Same change for `MultiIssuerValidator::add_issuer`
(opt-out: `add_issuer_unchecked`).

## `ProtocolConfig::default()` is multi-version

The default now accepts all `ProtocolVersion::STABLE` versions instead of
`[LATEST]` only. Older clients are routed through the existing version
adapters. To restore exact-match behavior:

```rust
let cfg = ServerConfig::builder()
    .protocol(ProtocolConfig::strict(ProtocolVersion::LATEST.clone()))
    .build();
```

## OAuth loopback redirect URIs

`http://0.0.0.0:PORT/callback` is no longer accepted as a loopback redirect.
Use `http://127.0.0.1:PORT/callback`, `http://[::1]:PORT/callback`, or
`http://localhost:PORT/callback`.

---

# v3.0.0 Migration (v2.x → v3.x)

TurboMCP 3.0.0 is a major modular architecture redesign with **individual transport crates**, a **`no_std` core layer**, **unified error types**, and full **MCP 2025-11-25 specification compliance**.

## Quick Start for v3.0

```toml
# Before (v2.x)
[dependencies]
turbomcp = "2.x"

# After (v3.x)
[dependencies]
turbomcp = "3.0.2"
```

Feature flags for transports work the same way:

```toml
turbomcp = { version = "3.0.2", features = ["stdio", "http", "websocket"] }
```

## Breaking Changes in v3.0.0

### 1. Unified Error Types

**What Changed:**

In v2.x, each layer had its own error type:
- `turbomcp_server::ServerError` / `turbomcp_server::ServerResult`
- `turbomcp_protocol::Error` / `turbomcp_protocol::Result`
- `turbomcp::McpError` (wrapper enum over the above)

In v3.x, there is a single canonical error type across all crates:
- `McpError` (defined in `turbomcp-core`, re-exported everywhere)
- `McpResult<T>` (alias for `Result<T, McpError>`)

**Migration:**

```rust
// Before (v2.x)
use turbomcp_server::{ServerError, ServerResult};
use turbomcp_protocol::{Error, Result};

fn my_handler() -> ServerResult<Value> {
    Err(ServerError::Internal("failed".to_string()))
}

// After (v3.x)
use turbomcp::{McpError, McpResult};
// Or use the prelude (recommended):
use turbomcp::prelude::*;

fn my_handler() -> McpResult<Value> {
    Err(McpError::internal("failed"))
}
```

### 2. Modular Transport Architecture

**What Changed:**

Transports have been extracted from the monolithic `turbomcp-transport` crate into individual crates. The `turbomcp-transport` crate still exists as a re-export hub, so **existing feature flags continue to work**.

**New Transport Crates:**

| Crate | Feature | Use Case |
|-------|---------|----------|
| `turbomcp-stdio` | `stdio` | Standard MCP transport (default) |
| `turbomcp-http` | `http` | HTTP/SSE Streamable HTTP client |
| `turbomcp-websocket` | `websocket` | Bidirectional WebSocket |
| `turbomcp-tcp` | `tcp` | Raw TCP sockets |
| `turbomcp-unix` | `unix` | Unix domain sockets |
| `turbomcp-grpc` | — | gRPC transport via tonic (standalone) |

**Migration:**

```toml
# Before (v2.x) - monolithic transport
turbomcp = { version = "2.x", features = ["http", "websocket"] }

# After (v3.x) - same feature flags, modular internals
turbomcp = { version = "3.0.2", features = ["http", "websocket"] }

# Or use individual transport crates directly (advanced)
turbomcp-http = "3.0.2"
turbomcp-websocket = "3.0.2"
```

### 3. `no_std` Core Layer

**What Changed:**

In v2.x, `turbomcp-core` was merged into `turbomcp-protocol`. In v3.x, `turbomcp-core` is re-extracted as a `no_std + alloc` foundation layer for WASM and embedded environments.

**New foundation crates:**
- `turbomcp-types` — canonical MCP type definitions (`no_std` ready via `alloc` feature)
- `turbomcp-core` — error types, handler trait, JSON-RPC, auth primitives (`no_std` compatible)
- `turbomcp-wire` — codec abstraction (JSON, SIMD JSON, MessagePack) (`no_std` compatible)
- `turbomcp-transport-traits` — transport trait definitions (requires `std`/tokio)

**Migration:**

```toml
# For no_std/WASM environments
turbomcp-core = { version = "3.0.2", default-features = false }
turbomcp-wire = { version = "3.0.2", default-features = false }

# For standard environments (default, no change needed)
turbomcp = "3.0.2"
```

Most users will not interact with these crates directly — they are re-exported through the main `turbomcp` crate.

### 4. Protocol Version Support

**What Changed:**
- Default protocol version updated to `2025-11-25` (latest MCP spec)
- Configurable protocol version negotiation with fallback support
- New `ProtocolConfig` type replaces ad-hoc version handling

**Migration:**

```rust
use turbomcp_server::{ServerConfig, ProtocolConfig};

// Default: Latest spec (2025-11-25) with fallback enabled
// Supports all versions: 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05
let config = ServerConfig::builder().build();

// Strict mode: Only accept specific version, no fallback
let config = ServerConfig::builder()
    .protocol(ProtocolConfig::strict("2025-11-25"))
    .build();

// Custom: Specific preferred version with fallback
let config = ServerConfig::builder()
    .protocol(ProtocolConfig {
        preferred_version: "2025-06-18".to_string(),
        supported_versions: vec![
            "2025-11-25".to_string(),
            "2025-06-18".to_string(),
        ],
        allow_fallback: true,
    })
    .build();
```

### 5. Feature Flag Changes

**MCP spec features removed (now always enabled):**

In v2.x, several MCP 2025-11-25 draft features required explicit feature flags on `turbomcp-protocol`. In v3.x, these are always available:

| Old Feature Flag (v2.x) | Types Now Always Available (v3.x) |
|--------------------------|-----------------------------------|
| `mcp-icons` | `Icons`, `IconTheme` |
| `mcp-url-elicitation` | URL mode in elicitation |
| `mcp-sampling-tools` | `tools`/`tool_choice` in `CreateMessageRequest` |
| `mcp-enum-improvements` | `EnumSchema`, `EnumOption` |
| `mcp-draft` | Bundle of all above |

```toml
# Before (v2.x) - with feature flags
turbomcp-protocol = { version = "2.x", features = ["mcp-icons", "mcp-url-elicitation"] }

# After (v3.x) - no feature flags needed
turbomcp-protocol = "3.0.2"
```

**Only `experimental-tasks` remains as a feature flag** for the experimental Tasks API (SEP-1686):

```toml
turbomcp = { version = "3.0.2", features = ["experimental-tasks"] }
```

**New feature bundles in v3.x:**

| Bundle | Contents |
|--------|----------|
| `minimal` | STDIO only |
| `full` | All transports + telemetry |
| `full-stack` | `full` + full client library |
| `all-transports` | stdio, http, websocket, tcp, unix, channel |

**New features in v3.x:**

| Feature | Description |
|---------|-------------|
| `channel` | In-process channel transport (zero-overhead for testing) |
| `telemetry` | OpenTelemetry, metrics, structured logging |
| `full-client` | Client library with all transports |

**Removed v2.x features** (no longer on the main `turbomcp` or `turbomcp-server` crate in v3.x):

| Feature | Notes |
|---------|-------|
| `context-injection` | Always enabled in v3 |
| `uri-templates` | Always enabled in v3 |
| `middleware` | Always available when server feature is enabled |
| `tls` | Handled per-transport in v3 |
| `network` | Use `["stdio", "tcp"]` instead |
| `server-only` | Use `["tcp", "unix"]` instead |
| `multi-tenancy` | Removed |
| `sessions` | Removed |
| `security` | Removed |
| `input-validation` | Removed |
| `rate-limiting` | Removed |
| `dpop-redis` | Use `turbomcp-dpop` crate directly with `redis-storage` feature |
| `dpop-hsm-pkcs11` | Use `turbomcp-dpop` crate directly with `hsm-pkcs11` feature |
| `dpop-hsm-yubico` | Use `turbomcp-dpop` crate directly with `hsm-yubico` feature |
| `dpop-test-utils` | Use `turbomcp-dpop` crate directly with `test-utils` feature |

Note: `schema-generation` was already always-on since v2 (schemars required by macros).

### 6. Authentication Migration

**What Changed:**

The `Claims` type (v2 `turbomcp_server::middleware::Claims`) is replaced by more specific types:
- `StandardClaims` in `turbomcp_core::auth` — JWT standard claims
- `AuthContext` in `turbomcp_auth` — rich authentication context (OAuth 2.1, API key, JWT)

```rust
// Before (v2.x)
use turbomcp_server::middleware::Claims;
// or
use turbomcp_server::Claims;

fn check_auth(claims: &Claims) { ... }

// After (v3.x)
use turbomcp_core::auth::StandardClaims;   // JWT claims
use turbomcp_auth::AuthContext;             // Rich auth context (requires "auth" feature)
```

### 7. New Crates in v3.0

| Crate | Description |
|-------|-------------|
| `turbomcp-types` | Canonical MCP type definitions (`no_std` ready via `alloc` feature) |
| `turbomcp-core` | `no_std` core: errors, handler trait, JSON-RPC |
| `turbomcp-wire` | Wire format codec abstraction (JSON, SIMD, MsgPack) |
| `turbomcp-transport-traits` | Transport trait definitions |
| `turbomcp-transport-streamable` | Portable Streamable HTTP types (`no_std`/WASM) |
| `turbomcp-telemetry` | OpenTelemetry, Prometheus, structured logging |
| `turbomcp-grpc` | gRPC transport via tonic |
| `turbomcp-wasm` | WebAssembly bindings (browser + WASI) |
| `turbomcp-wasm-macros` | WASM server proc macros |
| `turbomcp-openapi` | OpenAPI 3.0 to MCP tool/resource conversion |
| `turbomcp-stdio` | Extracted STDIO transport |
| `turbomcp-http` | Extracted HTTP/SSE client transport |
| `turbomcp-websocket` | Extracted WebSocket transport |
| `turbomcp-tcp` | Extracted TCP transport |
| `turbomcp-unix` | Extracted Unix socket transport |

## Type Mapping Reference (v2 → v3)

| v2.x Type | v3.x Type | Location |
|-----------|-----------|----------|
| `turbomcp_server::ServerError` | `McpError` | `turbomcp_core::error` |
| `turbomcp_server::ServerResult<T>` | `McpResult<T>` | `turbomcp_core::error` |
| `turbomcp_protocol::Error` | `McpError` | `turbomcp_core::error` |
| `turbomcp_protocol::Result<T>` | `McpResult<T>` | `turbomcp_core::error` |
| `turbomcp::McpError` (wrapper enum) | `McpError` (unified) | `turbomcp_core::error` |
| `turbomcp::McpResult<T>` | `McpResult<T>` | `turbomcp_core::error` |
| `turbomcp_server::Claims` | `StandardClaims` | `turbomcp_core::auth` |
| N/A | `AuthContext` | `turbomcp_auth` (new) |

All v3 types are re-exported from the `turbomcp` crate and available via `use turbomcp::prelude::*`.

## Version Compatibility

| TurboMCP Version | Rust Version | MCP Spec | Status |
|-----------------|--------------|----------|--------|
| 3.0.x           | 1.89.0+      | 2025-11-25 | Current |
| 2.3.x           | 1.89.0+      | 2025-06-18 | Maintenance |
| 1.1.x           | 1.89.0+      | 2025-06-18 | EOL |

---

# v2.0.0 Migration (v1.x → v2.x)

The 2.0.0 release focused on **clean minimal core + progressive enhancement**, with separate auth/DPoP crates and `turbomcp-core` merged into `turbomcp-protocol`.

## Quick Start

```toml
# Before (v1.x) - full by default
[dependencies]
turbomcp = "1.x"

# After (v2.0) - minimal by default, add features as needed
[dependencies]
turbomcp = { version = "2.0", features = ["full"] }  # Restores v1.x behavior
# Or start minimal:
turbomcp = "2.0"  # STDIO only (default)
```

## Breaking Changes Summary

### 1. Default Features Changed

```toml
# 1.x default
default = ["full", "simd"]  # All features enabled

# 2.0 default
default = ["stdio"]  # Minimal by default
```

If you relied on all transports being available by default, add the `full` feature:

```toml
turbomcp = { version = "2.0", features = ["full"] }
```

Or selectively enable what you need:

```toml
turbomcp = { version = "2.0", features = ["stdio", "http", "tcp"] }
```

### 2. `turbomcp-core` Merged into `turbomcp-protocol`

**What Changed:**
- The `turbomcp-core` crate was merged into `turbomcp-protocol`
- Direct imports from `turbomcp_core::` need to be updated to `turbomcp_protocol::`
- The `turbomcp-core` crate no longer exists as a workspace member in v2
- Context types (`RequestContext`, etc.) moved from `turbomcp_core::context` to `turbomcp_protocol::context`

```rust
// Before (v1.x) - importing from turbomcp-core
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::Error;

// After (v2.0) - import from turbomcp-protocol
use turbomcp_protocol::context::RequestContext;
use turbomcp_protocol::error::Error;

// Or use the prelude (recommended - works in both versions)
use turbomcp::prelude::*;
```

### 3. New Crate Architecture

**What Changed:**
- Authentication extracted from `turbomcp-server` to new `turbomcp-auth` crate
- DPoP remains in `turbomcp-dpop` (existed since v1.x)
- Both are optional dependencies

```toml
# Before (v1.x) - auth feature on server, DPoP as separate crate
[dependencies]
turbomcp = { version = "1.x", features = ["dpop"] }

# After (v2.0) - auth extracted to separate crate, accessible via features
[dependencies]
turbomcp = { version = "2.0", features = ["auth", "dpop"] }

# Or use crates directly
turbomcp-auth = "2.0"
turbomcp-dpop = "2.0"
```

v2 added the `Claims` type in `turbomcp_server::middleware` for JWT-based auth:

```rust
// New in v2.0 - Claims type for auth middleware
use turbomcp_server::Claims;
use turbomcp_auth::*;
```

### 4. Feature Flag Changes

**v2.0 added:**
- `auth` — OAuth 2.1, API key authentication (via new `turbomcp-auth` crate)
- `context-injection` — Enhanced Context API
- `uri-templates` — URI template matching for resources
- `middleware` — Tower middleware support
- `multi-tenancy` — Multi-tenant SaaS support
- `security` — Handler name validation
- `input-validation` — Input validation with `garde`
- `rate-limiting` — Rate limiting via `governor`
- `sessions` — Session management via `tower-sessions`

**v2.0 changed:**
- `schema-generation` — Still exists in v2.0.0 but removed by v2.3.x (schemars always required by macros)
- `full` (main crate) — Includes context-injection, uri-templates, all transports, tls
- `full` (server crate) — Includes all-transports, auth, dpop, sessions, security, input-validation, multi-tenancy, health-checks, metrics, middleware, rate-limiting, mcp-tasks, tls

**v2.0 added DPoP sub-features on the main crate:**
```toml
# New in v2.x (on main turbomcp crate, not present in v1.x):
dpop-redis = ["dpop", "turbomcp-dpop/redis-storage"]
dpop-hsm-pkcs11 = ["dpop", "turbomcp-dpop/hsm-pkcs11"]
dpop-hsm-yubico = ["dpop", "turbomcp-dpop/hsm-yubico"]
dpop-test-utils = ["dpop", "turbomcp-dpop/test-utils"]
```

### 5. Protocol Feature Flags Added (MCP 2025-11-25 Draft)

v2.x added feature-gated support for MCP 2025-11-25 draft specification features on `turbomcp-protocol`:

```toml
# Enable draft spec features in v2.x
turbomcp-protocol = { version = "2.x", features = ["mcp-draft"] }
# Or individually:
turbomcp-protocol = { version = "2.x", features = ["mcp-icons", "mcp-url-elicitation"] }
```

These became always-enabled in v3.0.

## Crate-by-Crate Migration

### turbomcp-core (Removed in v2.0)

Merged into `turbomcp-protocol`. Update imports:

```rust
// Before (v1.x)
use turbomcp_core::context::RequestContext;

// After (v2.0)
use turbomcp_protocol::context::RequestContext;
```

### turbomcp-protocol

- Now contains everything from the former `turbomcp-core`
- Error types: `Error`, `ErrorKind`, `Result` (v2 names)
- Internal module reorganization is transparent via re-exports

### turbomcp-transport

- No breaking public API changes
- Enhanced resilience with circuit breaker metrics

### turbomcp-server

- Default features changed from `["auth", "health-checks", "metrics", "stdio"]` to `["stdio"]`
- New features: `middleware`, `multi-tenancy`, `security`, `input-validation`, `rate-limiting`, `sessions`

```toml
# Restore v1.x server defaults
turbomcp-server = { version = "2.0", features = ["auth", "health-checks", "metrics", "stdio"] }
```

### turbomcp-client

- No breaking changes
- Re-exports `Error` and `Result` from `turbomcp-protocol`

### turbomcp-macros

- No breaking changes

### turbomcp-auth (NEW in v2.0)

New crate containing OAuth 2.1 and authentication functionality, extracted from `turbomcp-server`.

```toml
turbomcp = { version = "2.0", features = ["auth"] }
# Or directly:
turbomcp-auth = "2.0"
```

### turbomcp-dpop (Existing, Updated)

DPoP crate updated for v2.0. Feature names changed:

```toml
# Before (v1.x)
turbomcp = { version = "1.x", features = ["dpop"] }

# After (v2.0)
turbomcp = { version = "2.0", features = ["dpop"] }
# Redis storage:
turbomcp-dpop = { version = "2.0", features = ["redis-storage"] }
```

### turbomcp (Main SDK)

- Default features: `["full", "simd"]` → `["stdio"]`
- `McpError` enum wraps `ServerError` and protocol errors
- Auth/DPoP now in separate crates, accessible via feature flags

## Common Migration Patterns

### Pattern 1: Basic STDIO Server (No Changes Needed)

```rust
// Works in both v1.x and v2.0
use turbomcp::prelude::*;

#[server]
struct MyServer;

#[tool("my_tool", "Description")]
async fn my_tool(&self, arg: String) -> McpResult<String> {
    Ok(format!("Hello {}", arg))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_stdio().await
}
```

### Pattern 2: HTTP Server (Enable Feature Explicitly)

```toml
# Before (v1.x) - HTTP enabled by default via "full"
turbomcp = "1.x"

# After (v2.0) - Enable HTTP explicitly
turbomcp = { version = "2.0", features = ["http"] }
```

### Pattern 3: Full Feature Parity with v1.x

```toml
turbomcp = { version = "2.0", features = ["full"] }
```

## Troubleshooting

### "HTTP server not working"

Enable the HTTP feature — it's no longer included by default:

```toml
turbomcp = { version = "2.0", features = ["http"] }
```

### "module 'dpop' not found"

Enable the dpop feature:

```toml
turbomcp = { version = "2.0", features = ["dpop"] }
```

### Import errors after upgrade

Use the prelude or update imports to new crate structure:

```rust
// Option 1: Use prelude (recommended)
use turbomcp::prelude::*;

// Option 2: Update imports
use turbomcp_protocol::Error;       // was: turbomcp_core::Error
use turbomcp_protocol::ErrorKind;   // was: turbomcp_core::ErrorKind
use turbomcp_protocol::context::RequestContext;  // was: turbomcp_core::context::RequestContext
```

### Larger binary size than expected

Use minimal features instead of "full":

```toml
# Instead of:
turbomcp = { version = "2.0", features = ["full"] }

# Use only what you need:
turbomcp = { version = "2.0", features = ["stdio", "tcp"] }
```

## Version Compatibility

| TurboMCP Version | Rust Version | MCP Spec | Status |
|-----------------|--------------|----------|--------|
| 2.3.x           | 1.89.0+      | 2025-06-18 | Maintenance |
| 1.1.x           | 1.89.0+      | 2025-06-18 | EOL |
| 1.0.x           | 1.89.0+      | 2024-11-05 | EOL |

## Getting Help

- **Issues:** https://github.com/Epistates/turbomcp/issues
- **Discussions:** https://github.com/Epistates/turbomcp/discussions
- **Examples:** See `crates/turbomcp/examples/` directory
- **Documentation:** https://docs.rs/turbomcp
