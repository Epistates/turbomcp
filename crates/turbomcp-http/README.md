# turbomcp-http

MCP 2025-11-25 compliant HTTP/SSE client transport for TurboMCP.

## Overview

This crate provides the HTTP client transport implementation for the Model Context Protocol (MCP), implementing the Streamable HTTP transport specification with full SSE (Server-Sent Events) support.

## Features

- **MCP 2025-11-25 Specification Compliance**: Full implementation of the streamable HTTP spec
- **Single Endpoint Design**: All communication through one MCP endpoint
- **SSE Support**: Server-Sent Events for server-to-client streaming
- **Endpoint Discovery**: Automatic endpoint URI discovery via SSE events
- **Session Management**: Mcp-Session-Id header support for session tracking
- **Auto-Reconnect**: Configurable retry policies with exponential backoff
- **Last-Event-ID Resumability**: Resume SSE streams from last received event
- **TLS 1.3**: Minimum TLS version enforcement (v3.0 security requirement)
- **Size Limits**: Configurable request/response size validation

## Usage

```rust
use turbomcp_http::{StreamableHttpClientTransport, StreamableHttpClientConfig};
use turbomcp_transport_traits::Transport;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = StreamableHttpClientConfig {
        base_url: "http://localhost:8080".to_string(),
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        ..Default::default()
    };

    let transport = StreamableHttpClientTransport::new(config);
    transport.connect().await?;

    // Transport is ready for MCP communication
    Ok(())
}
```

## Configuration Options

```rust
use turbomcp_http::{StreamableHttpClientConfig, RetryPolicy};
use turbomcp_transport_traits::{LimitsConfig, TlsConfig};
use std::time::Duration;

let config = StreamableHttpClientConfig {
    base_url: "https://api.example.com".to_string(),
    endpoint_path: "/mcp".to_string(),
    timeout: Duration::from_secs(30),
    retry_policy: RetryPolicy::Exponential {
        base: Duration::from_secs(1),
        max_delay: Duration::from_secs(60),
        max_attempts: Some(10),
    },
    auth_token: Some("your-token".to_string()),
    limits: LimitsConfig::default(),
    tls: TlsConfig::modern(),
    ..Default::default()
};
```

## Security

- TLS 1.3 is required by default (v3.0 security requirement)
- Certificate validation is enabled by default
- Disabling certificate validation requires `TURBOMCP_ALLOW_INSECURE_TLS=1` environment variable

## Part of TurboMCP v3.0 Modular Architecture

This crate is part of the TurboMCP v3.0 transport extraction effort, enabling:
- Minimal builds with only needed transports
- Independent versioning of transport implementations
- Reduced binary sizes for embedded/WASM targets

## License

MIT
