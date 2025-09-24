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
│ ├── Structured McpError types             │
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
use turbomcp_core::{RequestContext, Message, Context, McpResult};

// Create a request context for correlation and observability
let mut context = RequestContext::new();

// Message parsing with optional SIMD acceleration
let json_data = br#"{"jsonrpc": "2.0", "method": "tools/list"}"#;
let message = Message::parse(json_data)?;

// Context provides rich observability and user information
context.info("Processing request").await?;

// Context features for authentication and user information
if context.is_authenticated() {
    let user = context.user().unwrap_or("unknown");
    let roles = context.roles();
    context.info(&format!("Authenticated user: {}, roles: {:?}", user, roles)).await?;
}
```

### Advanced Session Management

```rust
use turbomcp_core::{SessionManager, SessionConfig};

// Configure session management with LRU eviction
let config = SessionConfig::new()
    .with_max_sessions(1000)
    .with_ttl_seconds(3600);

let session_manager = SessionManager::with_config(config);

// Sessions are automatically managed with efficient cleanup
let session = session_manager.create_session().await?;
```

### Error Handling

```rust
use turbomcp_core::{McpError, McpResult};

fn process_request() -> McpResult<String> {
    // Rich error types with automatic context
    if invalid_input {
        return Err(McpError::InvalidInput(
            "Request missing required field".to_string()
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
turbomcp-core = { version = "1.1.0", features = ["simd"] }
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
    async fn my_tool(&self, ctx: Context) -> McpResult<String> {
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
    RequestContext, SessionManager, Message, McpError, McpResult
};

struct CustomHandler {
    sessions: SessionManager,
}

impl CustomHandler {
    async fn handle_request(&self, data: &[u8]) -> McpResult<String> {
        let context = RequestContext::new();
        let message = Message::parse(data)?;

        // Use core functionality directly
        context.info("Custom processing").await?;
        Ok("processed".to_string())
    }
}
```

## Error Types

Core error types for comprehensive error handling:

```rust
use turbomcp_core::McpError;

match result {
    Err(McpError::InvalidInput(msg)) => {
        // Handle validation errors
    },
    Err(McpError::SessionExpired(id)) => {
        // Handle session lifecycle
    },
    Err(McpError::Performance(details)) => {
        // Handle performance issues
    },
    Ok(value) => {
        // Process success case
    }
}
```

## Shareable Patterns for Async Concurrency

TurboMCP Core provides abstractions for thread-safe sharing that form the foundation for SharedClient, SharedTransport, and SharedServer:

### Generic Shareable Trait

The `Shareable<T>` trait provides a consistent interface for creating thread-safe wrappers:

```rust
use turbomcp_core::shared::{Shareable, Shared};

// Any type can implement Shareable
pub trait Shareable<T>: Clone + Send + Sync + 'static {
    fn new(inner: T) -> Self;
}

// Use with any type
struct MyService {
    counter: u64,
}

let service = MyService { counter: 0 };
let shared = Shared::new(service); // Implements Shareable<MyService>
```

### Shared<T> - General Purpose Wrapper

The `Shared<T>` wrapper provides closure-based access patterns for any type:

```rust
use turbomcp_core::shared::Shared;

struct Database {
    connections: Vec<Connection>,
}

impl Database {
    fn query(&self, sql: &str) -> Result<Vec<Row>, DbError> {
        // Query implementation
    }

    fn execute(&mut self, sql: &str) -> Result<u64, DbError> {
        // Execute implementation
    }
}

// Create shared wrapper
let db = Database::new();
let shared_db = Shared::new(db);

// Read access with closures
let results = shared_db.with(|db| {
    db.query("SELECT * FROM users")
}).await?;

// Mutable access with closures
let affected_rows = shared_db.with_mut(|db| {
    db.execute("UPDATE users SET active = true")
}).await?;

// Async closures also supported
let async_result = shared_db.with_async(|db| async {
    let rows = db.query("SELECT COUNT(*) FROM users")?;
    process_async(rows).await
}).await?;
```

### ConsumableShared<T> - One-Time Consumption Pattern

For types that need to be consumed (like servers), `ConsumableShared<T>` provides safe extraction:

```rust
use turbomcp_core::shared::{ConsumableShared, SharedError};

struct Server {
    config: ServerConfig,
}

impl Server {
    fn run(self) -> Result<(), ServerError> {
        // Consume self to run server
        println!("Running server with config: {:?}", self.config);
        Ok(())
    }

    fn status(&self) -> ServerStatus {
        // Non-consuming method
        ServerStatus::Ready
    }
}

// Create consumable shared wrapper
let server = Server::new(config);
let shared = ConsumableShared::new(server);

// Access before consumption
let status = shared.with(|s| s.status()).await?;
println!("Server status: {:?}", status);

// Clone for monitoring while consuming
let monitor = shared.clone();
tokio::spawn(async move {
    loop {
        match monitor.with(|s| s.status()).await {
            Ok(status) => println!("Status: {:?}", status),
            Err(SharedError::Consumed) => {
                println!("Server has been consumed");
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
});

// Consume the server (only possible once)
let server = shared.consume().await?;
server.run()?; // Server is now running
```

### Advanced Patterns

#### Custom Shared Implementations

Create domain-specific shared wrappers:

```rust
use turbomcp_core::shared::Shareable;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SharedHttpClient {
    inner: Arc<Mutex<HttpClient>>,
}

impl Shareable<HttpClient> for SharedHttpClient {
    fn new(client: HttpClient) -> Self {
        Self {
            inner: Arc::new(Mutex::new(client)),
        }
    }
}

impl Clone for SharedHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl SharedHttpClient {
    pub async fn get(&self, url: &str) -> Result<Response, HttpError> {
        self.inner.lock().await.get(url).await
    }

    pub async fn post(&self, url: &str, body: Vec<u8>) -> Result<Response, HttpError> {
        self.inner.lock().await.post(url, body).await
    }
}
```

#### Error Handling Patterns

```rust
use turbomcp_core::shared::{Shared, SharedError};

let shared_service = Shared::new(my_service);

// Handle potential errors in closures
let result = shared_service.with_mut(|service| {
    service.risky_operation()
        .map_err(|e| format!("Service error: {}", e))
}).await;

match result {
    Ok(success) => println!("Operation successful: {}", success),
    Err(e) => eprintln!("Operation failed: {}", e),
}

// Try operations without blocking
if let Some(result) = shared_service.try_with(|service| {
    service.quick_operation()
}) {
    println!("Quick operation result: {}", result);
} else {
    println!("Service is busy, will try later");
}
```

### Benefits

- **Type Safety**: Generic abstractions work with any type
- **Flexible Access**: Closure-based patterns for fine-grained control
- **Zero Overhead**: Same performance as manual Arc/Mutex usage
- **Async Native**: Built for async/await patterns
- **Error Handling**: Proper error propagation and handling
- **Consumption Safety**: Safe one-time consumption patterns

### Design Principles

The Shareable patterns follow key design principles:

1. **Hide Complexity**: Arc/Mutex details are encapsulated
2. **Preserve Semantics**: Original type behavior is maintained
3. **Enable Sharing**: Easy cloning for concurrent access
4. **Async First**: Designed for async/await workflows
5. **Type Generic**: Works with any Send + 'static type

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