# TurboMCP Examples

**20+ focused examples demonstrating TurboMCP 3.0 - from Hello World to production apps.**

## 🚀 Quick Start

```bash
# Simplest server (24 lines)
cargo run --example hello_world

# Clean macro-based server (58 lines)
cargo run --example macro_server

# NEW in v3: Progressive disclosure with visibility
cargo run --example visibility

# NEW in v3: Server composition with namespacing
cargo run --example composition

# NEW in v3: Typed middleware
cargo run --example middleware

# NEW in v3: In-memory test client
cargo run --example test_client

# Complete STDIO app
cargo run --example stdio_app

# HTTP server
cargo run --example http_server --features http

# TCP transport
cargo run --example tcp_transport_demo --features tcp

# Unix socket transport
cargo run --example unix_transport_demo --features unix
```

---

## 📚 Learning Path

### 1️⃣ Server Examples (Start Here!)

Learn server creation patterns:

| Example | Lines | What It Teaches |
|---------|-------|----------------|
| **hello_world.rs** | 24 | Absolute simplest MCP server - one tool |
| **macro_server.rs** | 58 | Clean `#[server]` macro API, multiple tools |
| **tags_versioning.rs** | 130 | Tags and versioning for component organization |
| **stateful.rs** | 59 | Arc<RwLock<T>> shared state pattern |
| **visibility.rs** | 140 | Progressive disclosure with VisibilityLayer |
| **composition.rs** | 195 | Mounting multiple servers with CompositeHandler |
| **middleware.rs** | 250 | Typed middleware for logging, metrics, access control |

**Total:** 7 examples

---

### 2️⃣ Client & Testing Examples

Learn client usage patterns:

| Example | Lines | What It Teaches |
|---------|-------|----------------|
| **basic_client.rs** | 45 | Connect, list tools, call tools |
| **comprehensive.rs** | 76 | All MCP features (tools, resources, prompts) |
| **elicitation_interactive_client.rs** | 237 | Interactive user input handling |
| **sampling_client.rs** | 277 | LLM sampling protocol |
| **test_client.rs** | 190 | In-memory testing with McpTestClient (NEW in v3) |

**Total:** 5 examples

**In-Memory Testing (NEW in v3):**
```bash
cargo run --example test_client
```

The `McpTestClient` enables fast unit testing without network transport overhead:
- Direct handler invocation (no TCP/HTTP setup)
- Fluent assertion API (`result.assert_text("expected")`)
- Session simulation for stateful tests

---

### 3️⃣ Transport Examples

Learn different transport mechanisms with complete server + client pairs:

#### Server Examples
| Example | Transport | What It Teaches |
|---------|-----------|----------------|
| **tcp_server.rs** | TCP | Network server |
| **websocket_server_simple.rs** | WebSocket | Real-time bidirectional |
| **http_server.rs** | HTTP/SSE | Web-compatible server |
| **unix_server_simple.rs** | Unix Socket | Local IPC server |

#### Client Examples
| Example | Transport | What It Teaches |
|---------|-----------|----------------|
| **tcp_client_simple.rs** | TCP | Network client with auto-connect |
| **websocket_client_simple.rs** | WebSocket | WebSocket client setup |
| **http_client_simple.rs** | HTTP/SSE | HTTP client with SSE support |
| **unix_client_simple.rs** | Unix Socket | Unix socket client |

**Running Transport Examples:**
```bash
# TCP (Terminal 1: Server, Terminal 2: Client)
cargo run --example tcp_server --features tcp
cargo run --example tcp_client_simple --features tcp

# WebSocket (requires both http and websocket features)
cargo run --example websocket_server_simple --features "http,websocket"
cargo run --example websocket_client_simple --features "http,websocket"

# HTTP/SSE
cargo run --example http_server --features http
cargo run --example http_client_simple --features http

# Unix Socket
cargo run --example unix_server_simple --features unix
cargo run --example unix_client_simple --features unix
```

**Legacy Transport Demos (single-file):**
| Example | Lines | What It Teaches |
|---------|-------|----------------|
| **tcp_transport_demo.rs** | 63 | TCP network communication (server only) |
| **unix_transport_demo.rs** | 78 | Unix socket IPC (server only) |

**Total:** 12 transport examples (8 new, 2 legacy)

---

### 4️⃣ Validation Examples

Learn parameter validation strategies:

| Example | What It Teaches |
|---------|----------------|
| **validation.rs** | All validation approaches with CLI flags |

```bash
# Run all demonstrations
cargo run --example validation

# Show specific approach
cargo run --example validation -- --approach newtype
cargo run --example validation -- --approach garde
cargo run --example validation -- --approach validator
cargo run --example validation -- --approach nutype

# Show comparison and decision tree
cargo run --example validation -- --compare
```

**Approaches covered:**
- Manual newtypes (zero dependencies)
- garde (modern runtime validation)
- validator (mature ecosystem)
- nutype (type-level guarantees)

**See also:** `VALIDATION_GUIDE.md` for comprehensive documentation

---

### 5️⃣ Complete Applications

Production-ready reference implementations:

| Example | Lines | What It Teaches |
|---------|-------|----------------|
| **stdio_app.rs** | 43 | Complete STDIO application |
| **http_app.rs** | 59 | Complete HTTP application with state |
| **anthropic_integration.rs** | 178 | Anthropic Claude integration |
| **openai_integration.rs** | 184 | OpenAI GPT integration |

**Total:** 4 examples averaging 116 lines

---

## 🎯 NEW in 2.0.4: Explicit Transport Selection

The `#[server]` macro now supports the `transports` attribute to specify which transports your server uses:

```rust
// Only stdio transport
#[server(name = "my-server", version = "0.1.0", transports = ["stdio"])]
impl MyServer { ... }

// Multiple transports
#[server(name = "my-server", version = "0.1.0", transports = ["stdio", "http", "tcp"])]
impl MyServer { ... }
```

**Benefits:**
- ✅ Explicit intent about which transports you support
- ✅ Smaller generated code (unused methods not generated)
- ✅ Zero cfg warnings on Nightly Rust
- ✅ Fully backward compatible (omitting attribute generates all transports)

**See also:** `transports_demo.rs` for comprehensive examples of all usage patterns

---

## 📖 By Use Case

**Want to build a CLI tool?**
→ Start with `hello_world.rs`, then `macro_server.rs`
→ Both now include `transports = ["stdio"]` for best practices

**Want to build a web service?**
→ Use `http_server.rs`, then `http_app.rs`

**Want to validate parameters?**
→ Run `validation.rs --compare` to choose the right approach

**Want TCP network communication?**
→ Use `tcp_transport_demo.rs` for TCP server

**Want local IPC (Inter-Process Communication)?**
→ Use `unix_transport_demo.rs` for fast Unix socket IPC

**Want to integrate with Claude/GPT?**
→ See `anthropic_integration.rs` or `openai_integration.rs`

**Want to build a client?**
→ Start with `basic_client.rs`, then `comprehensive.rs`

**Need shared state?**
→ See `stateful.rs` for Arc<RwLock<T>> pattern

**NEW in v3: Want progressive disclosure (hide admin tools)?**
→ See `visibility.rs` for VisibilityLayer with tag-based filtering

**NEW in v3: Want to compose multiple servers?**
→ See `composition.rs` for CompositeHandler with prefix namespacing

**NEW in v3: Want typed middleware (logging, metrics)?**
→ See `middleware.rs` for McpMiddleware trait

**NEW in v3: Want in-memory testing?**
→ See `test_client.rs` for McpTestClient without network overhead

**NEW in v3: Want to expose REST APIs as MCP?**
→ See `turbomcp-openapi` crate with `petstore` example

---

## ✨ Example Standards

All examples follow TurboMCP 3.0 principles:

✅ **Minimal & Focused** - One concept per example (avg 100 lines)
✅ **Production-Ready** - Real code, no placeholders
✅ **MCP 2025-11-25 Compliant** - Latest specification
✅ **Type-Safe** - Full compile-time validation
✅ **Well-Documented** - Clear purpose and usage
✅ **Security-Hardened** - SSRF protection, timeouts (OpenAPI)

---

## 🎯 Features Required

Most examples use `stdio` (default):
```bash
cargo run --example hello_world
```

HTTP examples need the `http` feature:
```bash
cargo run --example http_server --features http
```

Transport examples need their specific features:
```bash
# TCP transport
cargo run --example tcp_transport_demo --features tcp

# Unix sockets (Unix/Linux/macOS only)
cargo run --example unix_transport_demo --features unix
```

Or use `--all-features` to enable everything:
```bash
cargo build --examples --all-features
```

---

## 📊 Summary

- **Total Examples:** 20+ (was 48 in v1)
- **Average Length:** ~100 lines (was 250)
- **All Runnable:** 100% configured
- **Zero Bloat:** Every example teaches one thing
- **New in v3:** Progressive disclosure, composition, middleware, test client, OpenAPI

---

## 🔗 Related Documentation

- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [MCP Specification](https://modelcontextprotocol.io)
- [Migration Guide](../../../MIGRATION.md)
- [Main README](../../../README.md)
- [OpenAPI Integration](../../turbomcp-openapi/README.md) - REST-to-MCP conversion
- [Feature Gap Analysis](../../../docs/FEATURE_GAP_ANALYSIS.md) - v3 feature comparison

---

**New to MCP?** Start with `hello_world.rs` and work through the server examples!

**Upgrading from v2?** Check the new v3 examples: `visibility.rs`, `composition.rs`, `middleware.rs`, `test_client.rs`
