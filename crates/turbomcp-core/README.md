# TurboMCP Core

[![Crates.io](https://img.shields.io/crates/v/turbomcp-core.svg)](https://crates.io/crates/turbomcp-core)
[![Documentation](https://docs.rs/turbomcp-core/badge.svg)](https://docs.rs/turbomcp-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Core abstractions and utilities for the TurboMCP framework, providing foundational types, session management, and optimized message processing for Model Context Protocol implementations.

## Overview

`turbomcp-core` provides the essential building blocks for MCP implementations in Rust. It includes session management, request contexts, error handling, message types, and performance-optimized data structures.

## Key Features

### 🚀 **JSON Processing with SIMD Support**
- Optional SIMD acceleration with `simd-json` and `sonic-rs`
- Standard JSON processing with serde_json as fallback
- Bytes-based message handling for efficient memory usage

### 📦 **Optimized Data Structures**
- Memory-efficient processing with careful allocation patterns
- SmallVec and CompactStr for small data optimization
- Thread-safe concurrent data structures

### 🧵 **Session Management**
- Concurrent session tracking with thread-safe state management
- Configurable session limits and cleanup policies
- Request correlation and tracing support

### 🎯 **Error Handling**
- Structured error types with context using `thiserror`
- Error propagation with automatic conversion
- Debugging support with detailed error information

### 📊 **Observability Integration**
- Metrics hooks for performance monitoring
- Tracing integration for distributed observability
- Request correlation IDs for end-to-end tracking

### 🎯 **Context Types**
- ElicitationContext for server-initiated user input requests
- CompletionContext for autocompletion functionality
- PingContext for health monitoring and keepalive
- ResourceTemplateContext for dynamic resource generation

### 🔄 **Shareable Patterns for Async Concurrency**
- Generic Shareable trait for thread-safe wrappers
- Shared<T> wrapper with Arc/Mutex encapsulation
- ConsumableShared<T> for one-time consumption patterns
- Flexible access patterns with closure-based APIs

## Architecture

```
┌─────────────────────────────────────────────┐
│                TurboMCP Core                │
├─────────────────────────────────────────────┤
│ Message Processing                          │
│ ├── JSON parsing with optional SIMD        │
│ ├── Bytes-based message handling           │
│ └── Structured data types                  │
├─────────────────────────────────────────────┤
│ Session & Context Management               │
│ ├── RequestContext lifecycle              │
│ ├── Thread-safe session state             │
│ └── Correlation ID management             │
├─────────────────────────────────────────────┤
│ Error Handling & Observability            │
│ ├── Structured Error types                │
│ ├── Context preservation                  │
│ └── Metrics & tracing hooks               │
└─────────────────────────────────────────────┘
```

## Components

### Core Types
- Message parsing and serialization
- Request/response handling
- Context management for observability

### Optimizations
- Optional SIMD acceleration for JSON processing
- Efficient memory allocation patterns
- Concurrent data structures for multi-threaded usage
- Small data optimization with SmallVec and CompactStr

## Usage

### Basic Usage

```rust
use turbomcp_core::{RequestContext, Message, MessageId};
use bytes::Bytes;

// Create a request context for correlation and observability
let context = RequestContext::new();

// Message handling with optional SIMD acceleration
let json_data = Bytes::from(r#"{"jsonrpc": "2.0", "method": "tools/list"}"#);
let message = Message::deserialize(json_data)?;

// Parse message payload
let payload: serde_json::Value = message.parse_json()?;
println!("Method: {}", payload["method"]);

# Ok::<(), Box<dyn std::error::Error>>(())
```

### Advanced Session Management

```rust
use turbomcp_core::{SessionManager, SessionConfig};
use chrono::Duration;

// Configure session management with LRU eviction
let config = SessionConfig {
    max_sessions: 1000,
    session_timeout: Duration::hours(1),
    max_request_history: 1000,
    max_requests_per_session: None,
    cleanup_interval: std::time::Duration::from_secs(300),
    enable_analytics: true,
};

let session_manager = SessionManager::new(config);

// Sessions are automatically managed with efficient cleanup
let session = session_manager.get_or_create_session(
    "client-123".to_string(),
    "websocket".to_string()
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Error Handling

```rust
use turbomcp_core::{Error, Result};

fn process_request() -> Result<String> {
    // Rich error types with automatic context
    if invalid_input {
        return Err(Error::validation(
            "Request missing required field"
        ));
    }
    
    // Errors automatically include correlation context
    Ok("processed".to_string())
}
```

### SIMD Feature Flag

Enable SIMD acceleration for JSON processing:

```toml
[dependencies]
turbomcp-core = { version = "2.0.0", features = ["simd"] }
```

**Note**: SIMD features require compatible CPU architectures (x86_64 with SSE2+ or ARM with NEON).

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `simd` | Enable SIMD-accelerated JSON processing | ❌ |
| `metrics` | Enable built-in performance metrics | ✅ |
| `tracing` | Enable distributed tracing support | ✅ |
| `compression` | Enable message compression utilities | ❌ |

## Integration

### With TurboMCP Framework

`turbomcp-core` is automatically included when using the main TurboMCP framework:

```rust
use turbomcp::prelude::*;

// Core functionality is available through the prelude
#[server]
impl MyServer {
    #[tool("Example with context")]
    async fn my_tool(&self, ctx: Context) -> Result<String> {
        // Context is powered by turbomcp-core
        ctx.info("Processing request").await?;
        Ok("result".to_string())
    }
}
```

### Direct Usage

For custom implementations or integrations:

```rust
use turbomcp_core::{
    RequestContext, SessionManager, Message, Error, Result
};

struct CustomHandler {
    sessions: SessionManager,
}

impl CustomHandler {
    async fn handle_request(&self, data: &[u8]) -> Result<String> {
        let context = RequestContext::new();
        let bytes = bytes::Bytes::from(data.to_vec());
        let message = Message::deserialize(bytes)?;

        // Parse and process message
        let payload: serde_json::Value = message.parse_json()?;
        Ok("processed".to_string())
    }
}
```

## Error Types

Core error types for comprehensive error handling:

```rust
use turbomcp_core::{Error, ErrorKind};

match result {
    Err(e) if e.kind == ErrorKind::Validation => {
        // Handle validation errors
    },
    Err(e) if e.kind == ErrorKind::Timeout => {
        // Handle timeout errors
    },
    Err(e) if e.kind == ErrorKind::Internal => {
        // Handle internal errors
    },
    Ok(value) => {
        // Process success case
    }
}
```

## Internal Utilities

This crate provides internal shared wrappers (`Shared<T>`, `ConsumableShared<T>`) used by `SharedClient` and `SharedTransport` in higher-level crates. Most users should use the higher-level wrappers rather than these primitives directly

## Development

### Building

```bash
# Build with all features
cargo build --features simd,metrics,tracing

# Build optimized for production
cargo build --release --features simd
```

### Testing

```bash
# Run comprehensive tests
cargo test

# Run performance benchmarks
cargo bench

# Test SIMD features (requires compatible CPU)
cargo test --features simd
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses this crate)
- **[turbomcp-protocol](../turbomcp-protocol/)** - MCP protocol implementation
- **[turbomcp-transport](../turbomcp-transport/)** - Transport layer
- **[turbomcp-server](../turbomcp-server/)** - Server framework

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*