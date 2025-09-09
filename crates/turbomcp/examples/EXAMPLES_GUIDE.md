# TurboMCP Examples Guide

## 🎯 Learning Path: From Zero to Production

This guide provides a structured learning path through TurboMCP's capabilities. Each example builds on previous concepts, creating a smooth learning curve rather than a cliff.

### 📚 Example Organization

| Level | Examples | Focus | Prerequisites |
|-------|----------|-------|---------------|
| **Foundation** | 01-03 | Core concepts, basic patterns | None |
| **Core MCP** | 04-07 | Protocol features, API patterns | Foundation |
| **Advanced** | 08-10 | Power features, bidirectional | Core MCP |
| **Production** | 11-12 | Real deployment, integration | All previous |

---

## 🚀 Foundation (Start Here)

### 01_hello_world.rs
**Difficulty**: ⭐ Beginner  
**Time**: 5 minutes  
**Concepts**: Basic server, tool registration, stdio transport  

The perfect starting point. Creates a minimal MCP server with a single tool.

```bash
cargo run --example 01_hello_world
```

### 02_clean_server.rs  
**Difficulty**: ⭐ Beginner  
**Time**: 5 minutes  
**Concepts**: Modern minimal server, macro API introduction  

Shows the cleanest possible server implementation using macros.

```bash
cargo run --example 02_clean_server
```

### 03_basic_tools.rs
**Difficulty**: ⭐⭐ Beginner  
**Time**: 10 minutes  
**Concepts**: Parameters, validation, error handling, context  

Demonstrates tool parameter patterns and proper error handling.

```bash
cargo run --example 03_basic_tools
```

---

## 🔧 Core MCP Features

### 04_resources_and_prompts.rs
**Difficulty**: ⭐⭐ Intermediate  
**Time**: 15 minutes  
**Concepts**: Resources, prompts, templates, subscriptions  

Complete tutorial on MCP resources and prompt systems.

```bash
cargo run --example 04_resources_and_prompts
```

### 05_stateful_patterns.rs
**Difficulty**: ⭐⭐ Intermediate  
**Time**: 10 minutes  
**Concepts**: State management, context patterns, Arc/RwLock  

Shows how to maintain state in MCP servers safely.

```bash
cargo run --example 05_stateful_patterns
```

### 06_architecture_patterns.rs ⭐ NEW
**Difficulty**: ⭐⭐ Intermediate  
**Time**: 15 minutes  
**Concepts**: Builder vs Macro APIs, functional equivalence  

**Interactive demo showing both API styles with identical functionality.**

```bash
# Three modes:
cargo run --example 06_architecture_patterns         # Help menu
cargo run --example 06_architecture_patterns builder # Builder server
cargo run --example 06_architecture_patterns macro   # Macro server
cargo run --example 06_architecture_patterns client  # Test client
```

### 07_transport_showcase.rs
**Difficulty**: ⭐⭐⭐ Intermediate  
**Time**: 20 minutes  
**Concepts**: STDIO, HTTP/SSE, WebSocket, TCP transports  

All transport methods in one comprehensive example.

```bash
cargo run --example 07_transport_showcase [stdio|http|ws|tcp]
```

---

## 🚀 Advanced Capabilities

### 08_elicitation_complete.rs
**Difficulty**: ⭐⭐⭐ Advanced  
**Time**: 20 minutes  
**Concepts**: Server-initiated prompts, user interaction  

Complete elicitation system demonstration with all patterns.

```bash
cargo run --example 08_elicitation_complete
```

### 09_bidirectional_communication.rs
**Difficulty**: ⭐⭐⭐ Advanced  
**Time**: 25 minutes  
**Concepts**: All 4 handler types, progress tracking, logging  

Production-grade bidirectional communication with file processing workflow.

```bash
cargo run --example 09_bidirectional_communication
```

### 10_protocol_mastery.rs
**Difficulty**: ⭐⭐⭐ Advanced  
**Time**: 30 minutes  
**Concepts**: Complete protocol implementation, all methods  

Comprehensive demonstration of every MCP protocol method.

```bash
cargo run --example 10_protocol_mastery
```

---

## 🏭 Production & Integration

### 11_production_deployment.rs
**Difficulty**: ⭐⭐⭐⭐ Expert  
**Time**: 30 minutes  
**Concepts**: Security, monitoring, graceful shutdown, Docker  

Production-ready server with all enterprise features.

```bash
# Local development
cargo run --example 11_production_deployment

# Docker deployment
docker build -f examples/production.Dockerfile -t mcp-server .
docker run -p 8080:8080 mcp-server
```

### 12_client_integration.rs
**Difficulty**: ⭐⭐⭐⭐ Expert  
**Time**: 30 minutes  
**Concepts**: Client builder, LLM integration, end-to-end  

Complete client implementation with all features.

```bash
cargo run --example 12_client_integration
```

---

## 🎓 Learning Tips

### For Beginners
1. Start with examples 01-03 in order
2. Run each example and read the code comments
3. Modify the examples to experiment
4. Use `RUST_LOG=debug` for detailed logging

### Choosing Builder vs Macro Pattern
After example 06, you'll understand:
- **Use Builder**: When you need explicit control or dynamic configuration
- **Use Macros**: For clean, declarative servers with static configuration

### Testing Your Examples
```bash
# Quick test any stdio server
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run --example 01_hello_world

# Use the CLI tool for comprehensive testing
cargo install turbomcp-cli
turbomcp-cli test --command "cargo run --example 01_hello_world"
```

### Common Patterns

#### Context Usage
The `Context` parameter provides request correlation and logging:
```rust
ctx.info("Processing request").await?;  // Structured logging
ctx.warn("Deprecated feature used").await?;
```

#### Error Handling
Always use `McpResult` for proper error propagation:
```rust
#[tool]
async fn my_tool(&self) -> McpResult<String> {
    // Automatic error conversion with ?
    let data = fetch_data().await?;
    Ok(process(data))
}
```

#### State Management
For stateful servers, use Arc<RwLock<T>>:
```rust
#[derive(Clone)]
struct StatefulServer {
    state: Arc<RwLock<State>>,
}
```

---

## 📊 Feature Matrix

| Example | Tools | Resources | Prompts | Elicitation | Transport | State | Production |
|---------|-------|-----------|---------|-------------|-----------|-------|------------|
| 01 | ✅ | - | - | - | STDIO | - | - |
| 02 | ✅ | - | - | - | STDIO | - | - |
| 03 | ✅ | - | - | - | STDIO | - | ✅ |
| 04 | ✅ | ✅ | ✅ | - | STDIO | - | - |
| 05 | ✅ | - | - | - | STDIO | ✅ | - |
| 06 | ✅ | - | - | - | STDIO | - | - |
| 07 | ✅ | ✅ | - | - | ALL | - | - |
| 08 | ✅ | - | - | ✅ | STDIO | - | - |
| 09 | ✅ | ✅ | - | ✅ | STDIO | ✅ | - |
| 10 | ✅ | ✅ | ✅ | ✅ | STDIO | - | - |
| 11 | ✅ | ✅ | ✅ | - | HTTP | ✅ | ✅ |
| 12 | Client | Client | Client | Client | ALL | - | ✅ |

---

## 🔗 Additional Resources

- [MCP Specification](https://modelcontextprotocol.io)
- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [GitHub Repository](https://github.com/yourusername/turbomcp)

## 💡 Need Help?

- Each example includes detailed inline documentation
- Run with `RUST_LOG=debug` for verbose output
- Check the feature matrix to find examples for specific features
- Examples are designed to be modified - experiment freely!