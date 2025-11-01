# turbomcp-proxy Examples

This directory contains practical examples demonstrating turbomcp-proxy features.

## Running Examples

All examples can be run using:
```bash
cargo run --example <example_name>
```

## Available Examples

### 1. `runtime_proxy.rs` - Runtime Proxy Integration

Demonstrates building and running a runtime proxy programmatically.

```bash
cargo run --example runtime_proxy
```

Shows:
- Creating a `RuntimeProxyBuilder`
- Configuring backends and frontends
- Building and running the proxy server
- HTTP endpoint exposure

### 2. `tcp_backend.rs` - TCP Backend Connection

Demonstrates connecting to an MCP server via TCP and introspecting its capabilities.

```bash
# First, start an MCP server on TCP port 5000
your-mcp-server --listen-tcp localhost:5000

# Then run the example
cargo run --example tcp_backend
```

Shows:
- Configuring TCP backend (`host:port`)
- Backend connection and initialization
- Server introspection
- Accessing tools and resources

**Use Cases:**
- Connecting to remote MCP servers
- High-performance network communication
- Multi-host deployments

### 3. `unix_socket_backend.rs` - Unix Domain Socket Backend

Demonstrates connecting to an MCP server via Unix domain socket for efficient IPC.

```bash
# First, start an MCP server on Unix socket
your-mcp-server --listen-unix /tmp/mcp.sock

# Then run the example
cargo run --example unix_socket_backend
```

Shows:
- Configuring Unix socket backend
- Socket path validation
- Server introspection
- IPC benefits explanation

**Use Cases:**
- Same-host communication
- Container networking
- Security isolation with filesystem permissions
- Zero network overhead

### 4. `schema_export.rs` - Schema Generation

Demonstrates exporting MCP server capabilities as standard schemas.

```bash
cargo run --example schema_export
```

Shows:
- Introspecting server capabilities
- Generating OpenAPI 3.1 schema
- Generating GraphQL Schema Definition Language
- Generating Protobuf 3 definition

**Output Includes:**
- REST API documentation (OpenAPI)
- GraphQL type definitions
- Protocol buffer messages

## CLI Equivalents

All examples demonstrate functionality available via CLI:

### TCP Backend
```bash
turbomcp-proxy serve \
  --backend tcp --tcp localhost:5000 \
  --frontend http --bind 127.0.0.1:3001
```

### Unix Socket Backend
```bash
turbomcp-proxy serve \
  --backend unix --unix /tmp/mcp.sock \
  --frontend http --bind 127.0.0.1:3002
```

### Schema Export
```bash
# OpenAPI
turbomcp-proxy schema openapi \
  --backend tcp --tcp localhost:5000 \
  --output api-spec.json

# GraphQL
turbomcp-proxy schema graphql \
  --backend unix --unix /tmp/mcp.sock \
  --output schema.graphql

# Protobuf
turbomcp-proxy schema protobuf \
  --backend stdio --cmd "your-mcp-server" \
  --output server.proto
```

## Common Patterns

### Testing a Backend Connection

```bash
cargo run --example tcp_backend
# or
cargo run --example unix_socket_backend
```

### Generating API Documentation

```bash
cargo run --example schema_export
```

### Quick HTTP Proxy Setup

```bash
turbomcp-proxy serve \
  --backend tcp --tcp your-server:5000 \
  --frontend http --bind 0.0.0.0:3000 \
  --jwt-secret "your-secret" \
  --require-auth
```

## Requirements

- Rust 1.90.0+
- An MCP server accessible via:
  - STDIO (subprocess)
  - TCP (network)
  - Unix socket (same host)
  - HTTP/WebSocket (web service)

## Testing with Mock Servers

For testing without a real MCP server:

```bash
# Use the built-in example servers from turbomcp
cargo run --example <example_name> -- \
  --backend stdio \
  --cmd "your-test-server"
```

## Performance Notes

- **STDIO**: Best for subprocess communication, moderate latency
- **TCP**: Best for remote servers, network-dependent latency
- **Unix Sockets**: Best for same-host IPC, minimal latency and overhead
- **HTTP/WebSocket**: Best for web clients, network-dependent latency

## Next Steps

1. Choose the transport that fits your use case
2. Start your MCP server with the appropriate listener
3. Run the corresponding example
4. Use the CLI tool for production deployments
5. Refer to the main [README.md](../README.md) for detailed documentation

## Troubleshooting

**"Connection refused"**
- Ensure the MCP server is running and listening on the configured address
- Check port/socket permissions

**"Socket not found"**
- Verify the Unix socket path is correct
- Check that the server has permission to create the socket file

**"Backend connection error"**
- Check network connectivity (for TCP)
- Verify credentials/authentication tokens (if required)
- Review server logs for detailed error information
