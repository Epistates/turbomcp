# turbomcp-protocol Migration Guide

This document covers breaking changes and migration steps specific to the `turbomcp-protocol` crate. For workspace-wide migration guidance covering transport, server, client, and macros, see the top-level [MIGRATION.md](../../MIGRATION.md).

Historical note:
- v3 targets MCP `2025-11-25` only.
- References to v1/v2 below are archival migration notes, not active compatibility guidance.

---

## v2.x to v3.0

### turbomcp-core re-extracted as a no_std foundation

In v3.0, `turbomcp-core` was re-introduced as a separate crate that `turbomcp-protocol` depends on. It provides a `no_std`-compatible foundation with:

- Unified error types: `McpError`, `McpResult`, `ErrorKind`, `ErrorContext`
- Protocol constants: `PROTOCOL_VERSION`, `SUPPORTED_VERSIONS`, `MAX_MESSAGE_SIZE`, `DEFAULT_TIMEOUT_MS`
- Constant namespaces: `methods`, `error_codes`, `features`
- Handler response types: `IntoToolResponse`, `IntoToolError`, `Text`, `Json`, `Image`, `ToolError`

All of these are re-exported at the `turbomcp_protocol` crate root. No changes are required unless you took a direct dependency on `turbomcp-core`, which was not a public crate in v2.x.

The foundation crate is also accessible as `turbomcp_protocol::mcp_core` for advanced use cases.

### McpError is now the canonical error type

In v2.x, `turbomcp_protocol::Error` was a `thiserror`-derived enum defined in this crate. In v3.0, `McpError` from `turbomcp-core` is the canonical type. `Error` and `Result<T>` remain ergonomic re-exports of the canonical core types.

```rust
// v2.x - continues to compile in v3.0 via alias
use turbomcp_protocol::Error;

// v3.0 preferred
use turbomcp_protocol::McpError;
use turbomcp_protocol::McpResult;
```

### Default protocol version updated to 2025-11-25

`PROTOCOL_VERSION` is `"2025-11-25"`. v3 runtime negotiation is exact-match only; the old fallback/multi-version policy no longer applies.

### MCP 2025-11-25 features are always enabled

In v2.x, the following features required explicit feature flags. In v3.0, they are unconditionally compiled:

- Icons on Tool, Resource, Prompt (SEP-973)
- URL mode for elicitation (SEP-1036)
- Tool calling in sampling requests (SEP-1577)
- Enum schema improvements for ElicitResult (SEP-1330)

Remove any feature flag entries for these. Runtime availability is determined by the current MCP `2025-11-25` surface, not compile-time flags.

The only experimental feature flag remaining is `experimental-tasks` for the Tasks API (SEP-1686).

### Feature flags in v3.0

| Feature | Default | Enables |
|---------|---------|---------|
| `std` | yes | Standard library support |
| `simd` | yes | `simd-json`, `sonic-rs`, `simdutf8` for SIMD-accelerated JSON |
| `zero-copy` | no | `bytes/serde` for zero-copy message handling |
| `rkyv` | no | rkyv zero-copy serialization bridge |
| `wire` | no | `turbomcp-wire` codec abstraction |
| `wire-simd` | no | Wire codec with SIMD acceleration |
| `wire-msgpack` | no | Wire codec with MessagePack support |
| `lock-free` | no | Lock-free data structures (requires `unsafe`) |
| `mmap` | no | Memory-mapped file support (requires `unsafe`) |
| `fancy-errors` | no | `miette`-based error diagnostics |
| `experimental-tasks` | no | Tasks API (SEP-1686) |

To opt out of the default SIMD acceleration:

```toml
turbomcp-protocol = { version = "3.0", default-features = false, features = ["std"] }
```

---

## v1.x to v2.0

### turbomcp-core merged into turbomcp-protocol

The main change in v2.0 was merging the `turbomcp-core` crate into `turbomcp-protocol`. The separate `turbomcp-core` dependency was removed from the workspace.

**Dependency update:**

```toml
# v1.x
[dependencies]
turbomcp-core = "1.x"
turbomcp-protocol = "1.x"

# v2.0
[dependencies]
turbomcp-protocol = "2.0"
```

**Import update:**

```rust
// v1.x
use turbomcp_core::RequestContext;
use turbomcp_core::Error;
use turbomcp_protocol::types::CreateMessageRequest;

// v2.0
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::Error;
use turbomcp_protocol::types::CreateMessageRequest;
```

Find and replace `turbomcp_core::` with `turbomcp_protocol::` across your codebase. The public API is otherwise unchanged.

### Context module split from monolith into submodules

The monolithic `context.rs` (2,046 lines) was split into eight focused submodules. All types remain accessible via `turbomcp_protocol::*` or `turbomcp_protocol::context::*` without further qualification — each submodule re-exports its contents flat.

**Submodules and their primary types:**

| Submodule | Key types |
|-----------|-----------|
| `context::request` | `RequestContext`, `ResponseContext`, `RequestContextExt`, `BidirectionalContext`, `CommunicationDirection`, `CommunicationInitiator` |
| `context::capabilities` | `ServerToClientRequests` (trait), `CompletionCapabilities`, `ConnectionMetrics` |
| `context::client` | `ClientCapabilities` (re-exported as `ContextClientCapabilities`), `ClientSession`, `ClientId`, `ClientIdExtractor` |
| `context::elicitation` | `ElicitationContext`, `ElicitationState` |
| `context::completion` | `CompletionContext`, `CompletionOption`, `CompletionReference` (re-exported as `ContextCompletionReference`) |
| `context::ping` | `PingContext`, `PingOrigin` |
| `context::server_initiated` | `ServerInitiatedContext`, `ServerInitiatedType` |
| `context::templates` | `ResourceTemplateContext`, `TemplateParameter` |
| `context::rich` | `SessionStateGuard`, `RichContextExt`, `StateError`, `active_sessions_count`, `cleanup_session_state` |

No import changes are required if you used `turbomcp_protocol::RequestContext` or `turbomcp_protocol::context::*`. Only direct imports of internal submodule paths are affected.

Note on naming: the capabilities submodule exposes the `ServerToClientRequests` trait. There is no type named `CapabilitiesContext`. The client submodule exposes `ClientCapabilities` and `ClientSession`, not a type named `ClientContext`. The templates submodule type is `ResourceTemplateContext`, not `ResourceTemplatesContext`.

### Types module split from monolith into submodules

The monolithic `types.rs` (2,888 lines) was split into 17-18 focused submodules. The `turbomcp_protocol::types` namespace is unchanged — all types remain accessible as before via `use turbomcp_protocol::types::*` or by full path.

### SessionManager now accepts SessionConfig

In v1.x, `SessionManager::new` accepted individual parameters. In v2.0, it accepts a `SessionConfig` struct.

```rust
// v1.x
let manager = SessionManager::new(1000, some_duration);

// v2.0
use turbomcp_protocol::{SessionManager, SessionConfig};
use chrono::Duration;

let config = SessionConfig {
    max_sessions: 1000,
    session_timeout: Duration::hours(24),  // field is session_timeout
    max_request_history: 10000,
    max_requests_per_session: Some(10000),
    cleanup_interval: std::time::Duration::from_secs(300),
    enable_analytics: true,
};
let manager = SessionManager::new(config);
```

The timeout field is named `session_timeout`. There is no `idle_timeout` field.

### Security validation functions added to crate root

`turbomcp_protocol::security` exposes three path security functions, all re-exported at the crate root:

```rust
use turbomcp_protocol::{validate_path, validate_path_within, validate_file_extension};
```

These did not exist in v1.x. No migration action needed unless you had your own implementations that should be replaced.

### ServerToClientRequests trait is now public

This trait lives in `context::capabilities` and is re-exported at the crate root. It enables fully-typed server-initiated requests (sampling, elicitation, roots). In v1.x, bidirectional communication was not available as a typed public trait.

```rust
use turbomcp_protocol::ServerToClientRequests;
```

### SIMD and zero-copy features available

The `simd` feature (on by default) enables `simd-json`, `sonic-rs`, and `simdutf8` for accelerated JSON processing. The `zero-copy` feature enables `bytes/serde` support for `ZeroCopyMessage` in the `zero_copy` module. Neither was available as a first-class feature in v1.x.

---

For transport-layer, server, or macro migration details, see the top-level [MIGRATION.md](../../MIGRATION.md).
