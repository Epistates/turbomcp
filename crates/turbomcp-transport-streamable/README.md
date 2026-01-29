# turbomcp-transport-streamable

Streamable HTTP transport types for TurboMCP - SSE encoding, session management, MCP 2025-11-25 spec.

## Overview

This crate provides core types for the MCP 2025-11-25 Streamable HTTP transport specification.
It is designed to be portable across native and WASM environments.

## Features

- **Session Management**: `SessionId`, `Session`, `SessionStore` trait for stateful connections
- **SSE Encoding/Decoding**: Pure, no-I/O Server-Sent Events implementation
- **Protocol Types**: Request/response types for streamable HTTP endpoints
- **Cross-Platform**: Works on native and WASM (with `MaybeSend` marker trait)

## Usage

```rust
use turbomcp_transport_streamable::{
    SessionId, SessionStore, SseEvent, SseEncoder, StreamableConfig
};

// Create a new session ID
let session_id = SessionId::new();

// Encode an SSE event
let event = SseEvent::message("Hello, world!");
let encoded = SseEncoder::encode(&event);

// Parse SSE events
let mut parser = SseParser::new();
let events = parser.feed(&encoded);
```

## no_std Support

This crate supports `no_std` environments with the `alloc` feature:

```toml
[dependencies]
turbomcp-transport-streamable = { version = "3.0", default-features = false, features = ["alloc"] }
```

## Feature Flags

- `std` (default): Enable std library support
- `alloc`: Enable alloc-only support for no_std environments
- `wasm`: Enable WASM-specific optimizations

## License

MIT
