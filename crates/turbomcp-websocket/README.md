# turbomcp-websocket

WebSocket bidirectional transport implementation for the TurboMCP SDK.

## Overview

This crate provides full MCP 2025-06-18 protocol support for WebSocket transport with:

- **Bidirectional Communication**: Full request-response patterns with message correlation
- **Server-Initiated Requests**: Support for ping, sampling, roots, and elicitation
- **Elicitation Support**: Complete elicitation lifecycle management with timeouts
- **Automatic Reconnection**: Configurable exponential backoff retry logic
- **Keep-Alive**: Periodic WebSocket ping/pong to maintain connections
- **Compression**: Optional message compression support
- **TLS Support**: Secure WebSocket connections (WSS)
- **Background Tasks**: Efficient management of concurrent operations
- **Metrics Collection**: Comprehensive transport metrics and monitoring

## Installation

```toml
[dependencies]
turbomcp-websocket = "3.0"
```

Or use through the main transport crate:

```toml
[dependencies]
turbomcp-transport = { version = "3.0.0-beta.4", features = ["websocket"] }
```

## Quick Start

```rust
use turbomcp_websocket::{WebSocketBidirectionalTransport, WebSocketBidirectionalConfig};
use turbomcp_transport_traits::Transport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client configuration
    let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string())
        .with_max_concurrent_elicitations(5)
        .with_compression(true);

    // Create and connect transport
    let transport = WebSocketBidirectionalTransport::new(config).await?;
    transport.connect().await?;

    // Use the transport...
    Ok(())
}
```

## Architecture

```text
turbomcp-websocket/
├── config.rs        # Configuration types and builders
├── types.rs         # Core types and type aliases
├── connection.rs    # Connection management and lifecycle
├── tasks.rs         # Background task management
├── elicitation.rs   # Elicitation handling and timeout management
├── mcp_methods.rs   # MCP protocol method implementations
├── transport.rs     # Main Transport trait implementation
└── bidirectional.rs # BidirectionalTransport trait implementation
```

## v3.0 Modular Architecture

This crate is part of TurboMCP v3.0's modular transport architecture:

- **Foundation**: `turbomcp-transport-traits` provides core abstractions
- **Individual Transports**: Each transport (stdio, http, websocket) is a separate crate
- **Backward Compatibility**: `turbomcp-transport` re-exports all transports

This enables:
- Smaller binary sizes (only include what you need)
- Faster compilation (parallel crate building)
- Cleaner dependency graphs
- Independent versioning and updates

## License

MIT
