# TurboMCP Examples

**17 focused examples demonstrating TurboMCP 2.0 - from Hello World to production apps.**

## 🚀 Quick Start

```bash
# Simplest server (24 lines)
cargo run --example hello_world

# Clean macro-based server (58 lines)
cargo run --example macro_server

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
| **tools.rs** | 77 | Parameter types, validation, error handling |
| **resources.rs** | 59 | Resource handlers with URIs |
| **stateful.rs** | 59 | Arc<RwLock<T>> shared state pattern |
| **http_server.rs** | 38 | HTTP/SSE transport (web-compatible) |

**Total:** 6 examples averaging 53 lines

---

### 2️⃣ Client Examples

Learn client usage patterns:

| Example | Lines | What It Teaches |
|---------|-------|----------------|
| **basic_client.rs** | 45 | Connect, list tools, call tools |
| **comprehensive.rs** | 76 | All MCP features (tools, resources, prompts) |
| **elicitation_interactive_client.rs** | 237 | Interactive user input handling |
| **sampling_client.rs** | 277 | LLM sampling protocol |

**Total:** 4 examples averaging 159 lines

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

## 📖 By Use Case

**Want to build a CLI tool?**
→ Start with `hello_world.rs`, then `macro_server.rs`

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

---

## ✨ Example Standards

All examples follow TurboMCP 2.0 principles:

✅ **Minimal & Focused** - One concept per example (avg 95 lines)
✅ **Production-Ready** - Real code, no placeholders
✅ **MCP 2025-06-18 Compliant** - Latest specification
✅ **Type-Safe** - Full compile-time validation
✅ **Well-Documented** - Clear purpose and usage

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

- **Total Examples:** 16 (was 48)
- **Average Length:** 95 lines (was 250)
- **All Runnable:** 100% configured
- **Zero Bloat:** Every example teaches one thing
- **New in 2.0:** Transport demos (TCP, Unix)

---

## 🔗 Related Documentation

- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [MCP Specification](https://modelcontextprotocol.io)
- [Migration Guide](../../../MIGRATION.md)
- [Main README](../../../README.md)

---

**New to MCP?** Start with `hello_world.rs` and work through the server examples!
