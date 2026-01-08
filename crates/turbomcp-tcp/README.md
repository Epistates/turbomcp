# turbomcp-tcp

TCP socket transport implementation for the TurboMCP SDK.

## Overview

This crate provides TCP transport with:

- **Server Mode**: Accept multiple client connections with automatic handling
- **Client Mode**: Connect to a remote TCP server
- **Bidirectional Communication**: Full-duplex message exchange
- **Backpressure Handling**: Bounded channels prevent memory exhaustion
- **Graceful Shutdown**: Clean task termination on disconnect
- **Message Framing**: Uses LinesCodec for reliable newline-delimited JSON

## Installation

```toml
[dependencies]
turbomcp-tcp = "3.0"
```

Or use through the main transport crate:

```toml
[dependencies]
turbomcp-transport = { version = "2.3", features = ["tcp"] }
```

## Quick Start

### Server Mode

```rust
use turbomcp_tcp::{TcpTransport, TcpTransportBuilder};
use turbomcp_transport_traits::Transport;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let transport = TcpTransportBuilder::new()
        .bind_addr(addr)
        .build();

    transport.connect().await?; // Starts listening
    Ok(())
}
```

### Client Mode

```rust
use turbomcp_tcp::{TcpTransport, TcpTransportBuilder};
use turbomcp_transport_traits::Transport;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr: SocketAddr = "127.0.0.1:0".parse()?;
    let remote_addr: SocketAddr = "127.0.0.1:8080".parse()?;

    let transport = TcpTransportBuilder::new()
        .bind_addr(bind_addr)
        .remote_addr(remote_addr)
        .build();

    transport.connect().await?;
    Ok(())
}
```

## v3.0 Modular Architecture

This crate is part of TurboMCP v3.0's modular transport architecture:

- **Foundation**: `turbomcp-transport-traits` provides core abstractions
- **Individual Transports**: Each transport (stdio, http, websocket, tcp) is a separate crate
- **Backward Compatibility**: `turbomcp-transport` re-exports all transports

## License

MIT
