# ADR-001: Transport Architecture

> **Status**: Accepted
> **Date**: 2026-01-20
> **Context**: v3.0.0-beta.1

## Decision

TurboMCP v3 uses a **dual transport architecture**:

1. **`turbomcp-server`**: Server-side transports with `LineTransportRunner`
2. **`turbomcp-transport` + individual crates**: Client-side transports

## Context

The v3 audit report identified that `turbomcp-server` manages its own transport execution loop via `LineTransportRunner` rather than using `turbomcp-transport`.

This was raised as potential redundancy, but upon analysis, this is **intentional and correct**.

## Rationale

### Server-Side (`turbomcp-server`)

The `LineTransportRunner` is optimized for MCP server patterns:

```rust
pub struct LineTransportRunner<H: McpHandler> {
    handler: H,
}

impl<H: McpHandler> LineTransportRunner<H> {
    /// Run the transport loop:
    /// 1. Read line from input
    /// 2. Parse as JSON-RPC
    /// 3. Route to handler via McpHandler trait
    /// 4. Write response as line
    pub async fn run<R, W, F>(
        &self,
        reader: R,
        writer: W,
        ctx_factory: F,
    ) -> Result<(), McpError>
}
```

**Why server-specific:**
- Tightly integrated with `McpHandler` trait
- Direct access to request routing
- Single-connection, request-response pattern
- No client reconnection logic needed
- Graceful shutdown coordinated with server lifecycle

### Client-Side (`turbomcp-transport` + individual crates)

The transport crates (`turbomcp-stdio`, `turbomcp-http`, `turbomcp-tcp`, etc.) provide:

```rust
pub trait Transport: Send + Sync + 'static {
    async fn send(&self, message: &[u8]) -> Result<(), TransportError>;
    async fn receive(&self) -> Result<Vec<u8>, TransportError>;
    async fn close(&self) -> Result<(), TransportError>;
}
```

**Why client-specific:**
- Connection management (connect, reconnect, close)
- Circuit breaker patterns
- Connection pooling (HTTP)
- Retry logic with exponential backoff
- Multiple transport backends (reqwest, tokio, etc.)

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Server Architecture                           │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    turbomcp-server                            │  │
│  │                                                               │  │
│  │  ┌─────────────────────────────────────────────────────────┐ │  │
│  │  │              LineTransportRunner<H>                      │ │  │
│  │  │  • Read line → Parse JSON-RPC → Route → Respond         │ │  │
│  │  └─────────────────────────────────────────────────────────┘ │  │
│  │                           │                                   │  │
│  │       ┌───────────────────┼───────────────────┐              │  │
│  │       ▼                   ▼                   ▼              │  │
│  │   stdio.rs            tcp.rs             unix.rs             │  │
│  │                                                               │  │
│  │  ┌─────────────────────────────────────────────────────────┐ │  │
│  │  │              http.rs + websocket.rs                      │ │  │
│  │  │  • Frame-based (not line-based)                         │ │  │
│  │  │  • axum/tower integration                               │ │  │
│  │  └─────────────────────────────────────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                        Client Architecture                           │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    turbomcp-client                            │  │
│  │                                                               │  │
│  │  ┌─────────────────────────────────────────────────────────┐ │  │
│  │  │              Transport Trait                             │ │  │
│  │  │  • send() / receive() / close()                         │ │  │
│  │  │  • Reconnection, pooling, circuit breaker               │ │  │
│  │  └─────────────────────────────────────────────────────────┘ │  │
│  │                           │                                   │  │
│  │       ┌───────────────────┼───────────────────┐              │  │
│  │       ▼                   ▼                   ▼              │  │
│  │  turbomcp-stdio     turbomcp-http      turbomcp-tcp          │  │
│  │  turbomcp-websocket turbomcp-unix                            │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Consequences

### Positive

1. **Separation of Concerns**: Server and client have different requirements
2. **Minimal Dependencies**: Servers don't need client reconnection logic
3. **Performance**: Server loop is optimized for single-handler dispatch
4. **Simplicity**: Each crate has a focused responsibility

### Negative

1. **Some Code Duplication**: Line reading/writing logic exists in both
2. **Documentation Clarity**: Users may wonder why two approaches exist

### Mitigations

1. Document the architecture clearly (this ADR)
2. Keep low-level I/O utilities in shared crates where possible
3. Consider extracting common line I/O to a shared module in future

## Alternatives Considered

### Option A: Unified Transport (Rejected)

Force server to use client transport abstractions.

**Rejected because:**
- Server doesn't need reconnection logic
- Would add unnecessary complexity to handler dispatch
- Different lifecycle management needs

### Option B: Shared Core + Extensions (Considered)

Extract common I/O to `turbomcp-transport-traits`, extend for server/client.

**Status:** Partially implemented with `turbomcp-transport-traits` for client.
Server continues to use integrated `LineTransportRunner` for simplicity.

## References

- `crates/turbomcp-server/src/transport/line.rs` - LineTransportRunner implementation
- `crates/turbomcp-transport-traits/` - Client transport trait definitions
- VESTIGIAL_REPORT.md - Initial audit finding
