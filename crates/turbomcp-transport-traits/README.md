# turbomcp-transport-traits

Core transport traits and types for the TurboMCP Model Context Protocol SDK.

## Overview

This crate provides the foundational abstractions that all transport implementations depend on:

- **Traits**: `Transport`, `BidirectionalTransport`, `StreamingTransport`, `TransportFactory`
- **Types**: `TransportType`, `TransportState`, `TransportCapabilities`, `TransportMessage`
- **Errors**: `TransportError`, `TransportResult`
- **Config**: `LimitsConfig`, `TimeoutConfig`, `TlsConfig`
- **Metrics**: `TransportMetrics`, `AtomicMetrics`

## Usage

Transport implementations should depend on this crate and implement the `Transport` trait:

```rust
use turbomcp_transport_traits::{Transport, TransportResult, TransportMessage};
use async_trait::async_trait;

struct MyTransport { /* ... */ }

#[async_trait]
impl Transport for MyTransport {
    fn transport_type(&self) -> TransportType { /* ... */ }
    // ... other trait methods
}
```

## Part of TurboMCP v3

This crate is part of the TurboMCP v3.0 restructuring effort to provide:

- **Lean core**: Only trait definitions and types (~800 LOC)
- **No transport implementations**: Implementations live in separate crates
- **Foundation for all transports**: STDIO, HTTP, WebSocket, TCP, Unix, gRPC

## License

MIT
