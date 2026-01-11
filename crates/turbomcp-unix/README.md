# turbomcp-unix

Unix domain socket transport implementation for the TurboMCP SDK.

## Overview

This crate provides Unix domain socket transport with:

- **Server Mode**: Accept multiple client connections with automatic handling
- **Client Mode**: Connect to a Unix socket server
- **Bidirectional Communication**: Full-duplex message exchange
- **Backpressure Handling**: Bounded channels prevent memory exhaustion
- **Graceful Shutdown**: Clean task termination and socket cleanup
- **Message Framing**: Uses LinesCodec for reliable newline-delimited JSON
- **Security**: Configurable file permissions (default 0o600)

## Installation

```toml
[dependencies]
turbomcp-unix = "3.0"
```

Or use through the main transport crate:

```toml
[dependencies]
turbomcp-transport = { version = "3.0.0-exp", features = ["unix"] }
```

## Quick Start

### Server Mode

```rust
use turbomcp_unix::{UnixTransport, UnixTransportBuilder};
use turbomcp_transport_traits::Transport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = UnixTransportBuilder::new_server()
        .socket_path("/tmp/my-mcp.sock")
        .permissions(0o600)
        .build();

    transport.connect().await?; // Starts listening
    Ok(())
}
```

### Client Mode

```rust
use turbomcp_unix::{UnixTransport, UnixTransportBuilder};
use turbomcp_transport_traits::Transport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = UnixTransportBuilder::new_client()
        .socket_path("/tmp/my-mcp.sock")
        .build();

    transport.connect().await?;
    Ok(())
}
```

## v3.0 Modular Architecture

This crate is part of TurboMCP v3.0's modular transport architecture:

- **Foundation**: `turbomcp-transport-traits` provides core abstractions
- **Individual Transports**: Each transport (stdio, http, websocket, tcp, unix) is a separate crate
- **Backward Compatibility**: `turbomcp-transport` re-exports all transports

## License

MIT
