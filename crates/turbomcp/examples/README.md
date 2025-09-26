# TurboMCP Examples

This directory contains comprehensive examples demonstrating TurboMCP's capabilities, from basic "Hello World" servers to advanced transport implementations and deployment examples.

## 📚 Learning Path

### 🚀 Getting Started (Numbered Tutorial Series)

Follow these examples in order to learn TurboMCP step-by-step:

1. **`01_hello_world.rs`** - Simplest possible MCP server with basic tool
2. **`02_clean_server.rs`** - Well-structured server with proper organization
3. **`03_basic_tools.rs`** - Multiple tools with various parameter types
4. **`04_resources_and_prompts.rs`** - Resources and prompt handlers
5. **`05_stateful_patterns.rs`** - Managing server state and persistence
6. **`06_architecture_patterns.rs`** - Advanced architectural patterns
7. **`07_transport_showcase.rs`** - Transport layer demonstration
8. **`08_elicitation_*.rs`** - Interactive user input (server, client, complete)
9. **`09_bidirectional_communication.rs`** - Two-way protocol communication
10. **`10_protocol_mastery.rs`** - Advanced MCP protocol features
11. **`11_production_deployment.rs`** - Production-ready deployment patterns

### 🏗️ Transport Examples

**Discrete Transport Implementations:**
- **`transport_stdio_server.rs` / `transport_stdio_client.rs`** - Standard I/O transport
- **`transport_tcp_server.rs` / `transport_tcp_client.rs`** - TCP socket transport
- **`transport_http_server.rs` / `transport_http_client.rs`** - HTTP/SSE transport
- **`transport_websocket_server.rs` / `transport_websocket_client.rs`** - WebSocket transport
- **`transport_unix_server.rs` / `transport_unix_client.rs`** - Unix domain sockets

**Legacy Transport Examples (being deprecated):**
- `stdio_server.rs` / `stdio_client.rs` - Basic STDIO examples
- `tcp_server.rs` / `tcp_client.rs` - Basic TCP examples
- `http_server.rs` / `http_client.rs` - Basic HTTP examples
- `websocket_server.rs` / `websocket_client.rs` - Basic WebSocket examples
- `unix_socket_server.rs` / `unix_socket_client.rs` - Basic Unix socket examples

**Multi-Transport Demos:**
- **`all_transports_demo.rs`** - Single server supporting all transport types
- **`tcp_client_server_demo.rs`** - Complete TCP client-server demonstration

### 🎯 Architecture Patterns

- **`06_architecture_patterns.rs`** - Various server architecture approaches
- **`06b_architecture_client.rs`** - Client architecture patterns

### 🔄 Advanced Features

- **`08_elicitation_server.rs`** - Server-initiated user input requests
- **`08_elicitation_client.rs`** - Client handling elicitation flows
- **`08_elicitation_complete.rs`** - Complete elicitation demonstration
- **`09_bidirectional_communication.rs`** - Two-way protocol communication
- **`10_protocol_mastery.rs`** - Advanced MCP protocol features

## 🎯 Quick Start

```bash
# Start with the basics
cargo run --example 01_hello_world

# Try different transports
cargo run --example transport_stdio_server
cargo run --example transport_tcp_server

# See advanced features
cargo run --example 11_production_deployment
```

## 🛠️ Testing Examples

Test any example with turbomcp-cli:

```bash
# Test a STDIO server
turbomcp-cli tools-list --command "cargo run --example transport_stdio_server"

# Test initialization
turbomcp-cli initialize --command "cargo run --example 01_hello_world"
```

## 📖 Example Categories

### By Transport Type
- **STDIO**: `transport_stdio_*`, `stdio_*`
- **TCP**: `transport_tcp_*`, `tcp_*`
- **HTTP/SSE**: `transport_http_*`, `http_*`
- **WebSocket**: `transport_websocket_*`, `websocket_*`
- **Unix Sockets**: `transport_unix_*`, `unix_socket_*`

### By Complexity Level
- **Beginner**: `01_hello_world.rs` → `04_resources_and_prompts.rs`
- **Intermediate**: `05_stateful_patterns.rs` → `07_transport_showcase.rs`
- **Advanced**: `08_elicitation_*.rs` → `11_production_deployment.rs`

### By Use Case
- **Simple CLI Tool**: Start with `01_hello_world.rs`
- **Web Service**: Use `transport_http_*` or `transport_websocket_*`
- **Local IPC**: Use `transport_unix_*`
- **Network Service**: Use `transport_tcp_*`
- **Interactive Tools**: See `08_elicitation_*`

## 🔍 Example Standards

All examples follow these standards:
- ✅ **Production-ready code** - No shortcuts or placeholders
- ✅ **Complete functionality** - Working end-to-end examples
- ✅ **Comprehensive documentation** - Clear learning goals and usage
- ✅ **MCP 2025-06-18 compliance** - Latest specification adherence
- ✅ **Error handling** - Proper error management patterns
- ✅ **Type safety** - Full compile-time validation

## 📊 Summary

- **Total Examples**: 35
- **Transport Types**: 5 (STDIO, TCP, HTTP/SSE, WebSocket, Unix)
- **Tutorial Progression**: 11 numbered examples
- **Architecture Coverage**: Complete MCP specification
- **Quality**: Production-ready, zero-tolerance for shortcuts

## 🔗 Related Documentation

- [TurboMCP Main Documentation](https://docs.rs/turbomcp)
- [MCP 2025-06-18 Specification](https://modelcontextprotocol.io)
- [Transport Guide](../../../docs/transports.md)
- [Getting Started Guide](../../../README.md)

---

**Need help?** Start with `01_hello_world.rs` and work through the numbered examples in order!