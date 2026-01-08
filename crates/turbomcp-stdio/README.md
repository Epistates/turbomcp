# turbomcp-stdio

Standard I/O transport implementation for the TurboMCP Model Context Protocol SDK.

## Overview

This crate provides the `StdioTransport` implementation for MCP communication over stdin/stdout, which is the standard way MCP servers communicate with clients. It supports JSON-RPC over newline-delimited JSON.

## MCP Specification Compliance

This implementation is **fully compliant** with the MCP stdio transport specification (2025-06-18):

- **Newline-delimited JSON**: Uses `LinesCodec` for proper message framing
- **No embedded newlines**: Validates messages don't contain `\n` or `\r` characters
- **UTF-8 encoding**: All messages are UTF-8 encoded
- **stderr for logging**: Uses `tracing` crate which outputs to stderr by default
- **Bidirectional communication**: Supports both client→server and server→client messages
- **Valid JSON only**: Validates all messages are well-formed JSON before sending

## Usage

```rust
use turbomcp_stdio::{StdioTransport, Transport};

#[tokio::main]
async fn main() {
    let transport = StdioTransport::new();
    transport.connect().await.unwrap();

    // Send and receive messages...
}
```

## Features

- Zero-copy message handling with `Bytes`
- Lock-free atomic metrics for high performance
- Background reader task with bounded channel for backpressure
- Configurable message size limits

## Architecture

The transport follows the hybrid mutex pattern for optimal async performance:

- `std::sync::Mutex` for state/config (short-lived locks, never cross `.await`)
- `AtomicMetrics` for lock-free counter updates
- `tokio::sync::Mutex` for I/O streams (necessary for async I/O)

## License

MIT
