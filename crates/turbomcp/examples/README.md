# TurboMCP Examples

**15 focused examples demonstrating TurboMCP 3.0 - from Hello World to production patterns.**

## Quick Start

```bash
# Simplest server (24 lines)
cargo run --example hello_world

# Clean macro-based server
cargo run --example macro_server

# NEW in v3: Progressive disclosure with visibility
cargo run --example visibility

# NEW in v3: Server composition with namespacing
cargo run --example composition

# NEW in v3: Typed middleware
cargo run --example middleware

# NEW in v3: In-memory test client
cargo run --example test_client
```

---

## Learning Path

### 1. Server Examples (Start Here!)

Learn server creation patterns:

| Example | What It Teaches |
|---------|----------------|
| **hello_world.rs** | Absolute simplest MCP server - one tool |
| **macro_server.rs** | Clean `#[server]` macro API, multiple tools |
| **calculator.rs** | Tool with structured input (calculator operations) |
| **stateful.rs** | `Arc<RwLock<T>>` shared state pattern |
| **tags_versioning.rs** | Tags and versioning for component organization |
| **visibility.rs** | Progressive disclosure with VisibilityLayer (v3) |
| **composition.rs** | Mounting multiple servers with CompositeHandler (v3) |
| **middleware.rs** | Typed middleware for logging, metrics, access control (v3) |

---

### 2. Client & Testing Examples

Learn client usage and testing patterns:

| Example | What It Teaches |
|---------|----------------|
| **test_client.rs** | In-memory testing with McpTestClient (v3) |
| **tcp_client.rs** | TCP client connecting to a server |
| **unix_client.rs** | Unix socket client connection |

**In-Memory Testing (v3):**
```bash
cargo run --example test_client
```

The `McpTestClient` enables fast unit testing without network transport overhead:
- Direct handler invocation (no TCP/HTTP setup)
- Fluent assertion API
- Session simulation for stateful tests

---

### 3. Transport Examples

Learn different transport mechanisms:

| Example | Transport | What It Teaches |
|---------|-----------|----------------|
| **tcp_server.rs** | TCP | Network server with `run_tcp()` |
| **tcp_client.rs** | TCP | Network client with auto-connect |
| **unix_client.rs** | Unix Socket | Local IPC client |
| **transports_demo.rs** | Multiple | Explicit transport selection with `#[server(transports = [...])]` |

**Running Transport Examples:**
```bash
# TCP (Terminal 1: Server, Terminal 2: Client)
cargo run --example tcp_server --features tcp
cargo run --example tcp_client --features tcp

# Unix socket client
cargo run --example unix_client --features unix

# Transport selection demo
cargo run --example transports_demo
```

---

### 4. Validation Examples

Learn parameter validation strategies:

| Example | What It Teaches |
|---------|----------------|
| **validation.rs** | Multiple validation approaches with CLI flags |

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

---

### 5. Advanced Patterns

| Example | What It Teaches |
|---------|----------------|
| **type_state_builders_demo.rs** | Type-state builder pattern for compile-time correctness |

```bash
cargo run --example type_state_builders_demo
```

---

## By Use Case

**Want to build a CLI tool?**
Start with `hello_world.rs`, then `macro_server.rs`

**Want TCP network communication?**
Use `tcp_server.rs` + `tcp_client.rs`

**Want local IPC?**
Use `unix_client.rs` for Unix socket IPC

**Want to validate parameters?**
Run `validation.rs --compare` to choose the right approach

**Need shared state?**
See `stateful.rs` for `Arc<RwLock<T>>` pattern

**Want progressive disclosure (hide admin tools)?**
See `visibility.rs` for VisibilityLayer with tag-based filtering (v3)

**Want to compose multiple servers?**
See `composition.rs` for CompositeHandler with prefix namespacing (v3)

**Want typed middleware (logging, metrics)?**
See `middleware.rs` for McpMiddleware trait (v3)

**Want in-memory testing?**
See `test_client.rs` for McpTestClient without network overhead (v3)

**Want to expose REST APIs as MCP?**
See `turbomcp-openapi` crate

---

## Example Standards

All examples follow TurboMCP 3.0 principles:

- **Minimal & Focused** - One concept per example
- **Production-Ready** - Real code, no placeholders
- **MCP 2025-11-25 Compliant** - Latest specification
- **Type-Safe** - Full compile-time validation
- **Well-Documented** - Clear purpose and usage

---

## Features Required

Most examples use `stdio` (default):
```bash
cargo run --example hello_world
```

Transport examples need their specific features:
```bash
# TCP transport
cargo run --example tcp_server --features tcp
cargo run --example tcp_client --features tcp

# Unix sockets (Unix/Linux/macOS only)
cargo run --example unix_client --features unix
```

Or use `--all-features` to enable everything:
```bash
cargo build --examples --all-features
```

---

## Summary

- **Total Examples:** 15
- **All Runnable:** 100% configured
- **Zero Bloat:** Every example teaches one thing
- **New in v3:** Progressive disclosure, composition, middleware, test client

---

## Related Documentation

- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [MCP Specification](https://modelcontextprotocol.io)
- [Migration Guide](../../../MIGRATION.md)
- [Main README](../../../README.md)
- [OpenAPI Integration](../../turbomcp-openapi/README.md) - REST-to-MCP conversion

---

**New to MCP?** Start with `hello_world.rs` and work through the server examples!

**Upgrading from v2?** Check the new v3 examples: `visibility.rs`, `composition.rs`, `middleware.rs`, `test_client.rs`
