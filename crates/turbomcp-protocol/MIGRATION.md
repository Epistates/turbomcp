> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Protocol 2.0.0 Migration Guide

This guide helps you migrate from turbomcp-protocol 1.x to 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)
- [Troubleshooting](#troubleshooting)

## 🚀 Overview

**TurboMCP Protocol 2.0.0** represents a major architectural consolidation:

### Key Changes
- **Merged `turbomcp-core`**: All core functionality integrated into `turbomcp-protocol`
- **Module Reorganization**: Context and types modules split into focused submodules
- **Zero Breaking API Changes**: Public API unchanged, only import paths affected
- **Enhanced Features**: New zero-copy message processing and SIMD acceleration

### Migration Timeline
- **Minimal Impact**: Most users won't need any changes
- **Import Updates**: Only if using internal modules directly
- **Full Compatibility**: v1.x code continues working with minor import adjustments

## 💥 Breaking Changes

### 1. turbomcp-core Merged into turbomcp-protocol

**What Changed:**
```toml
# v1.x - Two separate crates
[dependencies]
turbomcp-core = "1.x"
turbomcp-protocol = "1.x"

# v2.0.0 - Single crate
[dependencies]
turbomcp-protocol = "2.0.0"
```

**Why:**
- Eliminates circular dependency issues
- Better cohesion and maintainability
- Simpler dependency graph
- Enables fully-typed bidirectional communication

**Migration:**
```rust
// Before (v1.x)
use turbomcp_core::RequestContext;
use turbomcp_core::Error;
use turbomcp_protocol::types::CreateMessageRequest;

// After (v2.0.0)
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::Error;
use turbomcp_protocol::types::CreateMessageRequest;

// Or using a single import
use turbomcp_protocol::{RequestContext, Error, types::CreateMessageRequest};
```

### 2. Module Reorganization

**Context Module Split** (2,046 lines → 8 focused modules):
```rust
// Before (v1.x) - Monolithic module
use turbomcp_core::context::*;

// After (v2.0.0) - Specific submodules
use turbomcp_protocol::context::request::RequestContext;
use turbomcp_protocol::context::capabilities::CapabilitiesContext;
use turbomcp_protocol::context::client::ClientContext;
use turbomcp_protocol::context::elicitation::ElicitationContext;
use turbomcp_protocol::context::completion::CompletionContext;
use turbomcp_protocol::context::ping::PingContext;
use turbomcp_protocol::context::server_initiated::ServerInitiatedContext;
use turbomcp_protocol::context::templates::ResourceTemplatesContext;

// Re-exports still work (backward compatible)
use turbomcp_protocol::context::*;  // Gets all contexts
```

**Types Module Split** (2,888 lines → 12 focused modules):
```rust
// Before (v1.x) - Monolithic module
use turbomcp_protocol::types::*;

// After (v2.0.0) - Specific submodules available
use turbomcp_protocol::types::core::*;
use turbomcp_protocol::types::tools::*;
use turbomcp_protocol::types::resources::*;
use turbomcp_protocol::types::prompts::*;
use turbomcp_protocol::types::capabilities::*;
// ... etc

// Re-exports still work (backward compatible)
use turbomcp_protocol::types::*;  // Gets all types
```

**Why:**
- Improved maintainability (no 2,000+ line files)
- Better code organization and navigation
- Clearer semantic grouping
- Easier to contribute and review changes

**Impact:**
- **Zero breaking changes** for users of `use turbomcp_protocol::types::*;`
- Only affects users importing internal submodules directly

## ✨ New Features

### 1. Zero-Copy Message Processing

**NEW:** Advanced `ZeroCopyMessage` type for ultra-high throughput scenarios:

```rust
use turbomcp_protocol::message::ZeroCopyMessage;
use bytes::Bytes;

// Zero-allocation message processing
let raw_bytes = Bytes::from(json_data);
let message = ZeroCopyMessage::from_bytes(raw_bytes)?;

// Process without copying
match message {
    ZeroCopyMessage::Request(req) => {
        // No allocation - direct access to underlying bytes
        process_request(req).await?;
    }
    _ => {}
}
```

**Benefits:**
- Eliminates unnecessary allocations in hot paths
- Reduces memory pressure for high-throughput scenarios
- Enables efficient message forwarding/proxying

### 2. Enhanced SIMD Support

**NEW:** Improved SIMD-accelerated JSON processing:

```toml
[dependencies]
turbomcp-protocol = { version = "2.0.0", features = ["simd"] }
```

**Features:**
- `sonic-rs` integration for fast JSON serialization
- `simd-json` for accelerated parsing
- `simdutf8` for UTF-8 validation
- Automatic fallback to standard JSON when SIMD unavailable

### 3. Security Validation Module

**NEW:** Built-in security utilities from dissolved security crate:

```rust
use turbomcp_protocol::security::{
    validate_path,
    validate_path_within,
    validate_file_extension,
};

// Path traversal protection
let safe_path = validate_path(&user_input)?;

// Boundary enforcement
let safe_relative = validate_path_within(&base_dir, &user_path)?;

// Extension validation
validate_file_extension(&path, &["json", "txt"])?;
```

### 4. Enhanced Session Management

**NEW:** Memory-bounded session management with automatic cleanup:

```rust
use turbomcp_protocol::session::SessionManager;

let session_mgr = SessionManager::new(
    max_sessions: 1000,      // Max concurrent sessions
    idle_timeout: 300,       // 5 minutes
    cleanup_interval: 60,    // Check every minute
);

// Automatic LRU eviction when limit reached
// Background cleanup of expired sessions
```

## 🔄 Migration Steps

### Step 1: Update Dependencies

```toml
# Before (v1.x)
[dependencies]
turbomcp-core = "1.1.2"
turbomcp-protocol = "1.1.2"

# After (v2.0.0)
[dependencies]
turbomcp-protocol = "2.0.0"
```

### Step 2: Update Imports

**Option A: Search and Replace (Recommended)**

```bash
# Find all turbomcp_core imports
rg "use turbomcp_core::" -l | xargs sed -i '' 's/turbomcp_core::/turbomcp_protocol::/g'

# Or use ast-grep for structural replacement
ast-grep --pattern 'use turbomcp_core::$$$' \
         --rewrite 'use turbomcp_protocol::$$$' \
         --lang rust -i
```

**Option B: Manual Update**

Find and replace in your code:
- `turbomcp_core::` → `turbomcp_protocol::`
- `use turbomcp_core` → `use turbomcp_protocol`

### Step 3: Verify Build

```bash
# Clean build to ensure no stale artifacts
cargo clean

# Build with warnings as errors
cargo build --all-features

# Run tests
cargo test --all-features
```

### Step 4: Optional - Use New Features

If you have high-throughput requirements:

```toml
[dependencies]
turbomcp-protocol = { version = "2.0.0", features = ["simd", "zero-copy"] }
```

```rust
// Enable zero-copy processing in hot paths
use turbomcp_protocol::message::ZeroCopyMessage;

async fn process_messages(raw: bytes::Bytes) -> Result<()> {
    let msg = ZeroCopyMessage::from_bytes(raw)?;
    // Process without allocating
    Ok(())
}
```

## 🐛 Troubleshooting

### Issue: "crate `turbomcp_core` not found"

**Solution:**
```toml
# Remove turbomcp-core dependency
[dependencies]
# turbomcp-core = "1.x"  # Remove this line
turbomcp-protocol = "2.0.0"
```

Update imports:
```rust
// Change this:
use turbomcp_core::RequestContext;

// To this:
use turbomcp_protocol::RequestContext;
```

### Issue: "module `context` is private"

**Solution:** You're likely importing an internal module. Use the re-exports:

```rust
// Instead of:
use turbomcp_protocol::context::request::RequestContext;

// Use:
use turbomcp_protocol::RequestContext;
// Or:
use turbomcp_protocol::context::RequestContext;
```

### Issue: Compilation errors after updating imports

**Solution:** Make sure you've updated ALL imports, not just some:

```bash
# Check for remaining turbomcp_core references
rg "turbomcp_core" --type rust

# Should return no results (except in comments/documentation)
```

### Issue: "type `X` not found in module `types`"

**Solution:** Type might have moved to a specific submodule. Check the documentation:

```rust
// If you get an error like: "ToolInfo not found"
// The type exists but might be in a submodule

// Try:
use turbomcp_protocol::types::tools::ToolInfo;
// Or use the wildcard (all types re-exported):
use turbomcp_protocol::types::*;
```

## 📊 Before/After Comparison

### Dependency Graph

**Before (v1.x):**
```
turbomcp-server
├── turbomcp-protocol
│   └── turbomcp-core  ❌ Circular dependency risk
└── turbomcp-core
```

**After (v2.0.0):**
```
turbomcp-server
└── turbomcp-protocol  ✅ Clean linear dependency
```

### Import Simplification

**Before (v1.x):**
```rust
use turbomcp_core::RequestContext;
use turbomcp_core::Error;
use turbomcp_core::session::SessionManager;
use turbomcp_protocol::types::Tool;
use turbomcp_protocol::types::Resource;
use turbomcp_protocol::jsonrpc::JsonRpcRequest;
```

**After (v2.0.0):**
```rust
use turbomcp_protocol::{
    RequestContext,
    Error,
    session::SessionManager,
    types::{Tool, Resource},
    jsonrpc::JsonRpcRequest,
};
```

## 🎯 Common Migration Patterns

### Pattern 1: Basic Tool Implementation

```rust
// v1.x
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::ToolInfo;

async fn my_tool(ctx: RequestContext, input: String) -> Result<String> {
    Ok(format!("Processed: {}", input))
}

// v2.0.0 (only import changes)
use turbomcp_protocol::{RequestContext, types::ToolInfo};

async fn my_tool(ctx: RequestContext, input: String) -> Result<String> {
    Ok(format!("Processed: {}", input))
}
```

### Pattern 2: Server-to-Client Communication

```rust
// v1.x
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::CreateMessageRequest;
use turbomcp_core::ServerToClientRequests;

async fn sample_llm(ctx: RequestContext) -> Result<String> {
    let req = CreateMessageRequest { /* ... */ };
    let resp = ctx.server_to_client()?.create_message(req, ctx.clone()).await?;
    Ok(resp.content.text)
}

// v2.0.0 (only import changes)
use turbomcp_protocol::{
    RequestContext,
    types::CreateMessageRequest,
    ServerToClientRequests,
};

async fn sample_llm(ctx: RequestContext) -> Result<String> {
    let req = CreateMessageRequest { /* ... */ };
    let resp = ctx.server_to_client()?.create_message(req, ctx.clone()).await?;
    Ok(resp.content.text)
}
```

### Pattern 3: Custom Error Handling

```rust
// v1.x
use turbomcp_core::Error as McpError;
use turbomcp_protocol::jsonrpc::{JsonRpcError, ErrorCode};

fn custom_error(msg: &str) -> McpError {
    McpError::Protocol(JsonRpcError {
        code: ErrorCode::InvalidRequest,
        message: msg.to_string(),
        data: None,
    })
}

// v2.0.0 (only import changes)
use turbomcp_protocol::{
    Error as McpError,
    jsonrpc::{JsonRpcError, ErrorCode},
};

fn custom_error(msg: &str) -> McpError {
    McpError::Protocol(JsonRpcError {
        code: ErrorCode::InvalidRequest,
        message: msg.to_string(),
        data: None,
    })
}
```

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-protocol
- **Examples**: See `../../examples/` for updated 2.0 examples
- **Changelog**: See `../../CHANGELOG.md` for complete version history

## 🎉 Benefits of 2.0.0

After migration, you'll enjoy:

- ✅ **Simpler Dependencies** - One crate instead of two
- ✅ **Better Organization** - Focused modules instead of monoliths
- ✅ **Zero-Copy Performance** - Optional high-performance message processing
- ✅ **Enhanced Security** - Built-in security validation utilities
- ✅ **Better Documentation** - Clearer module structure and examples
- ✅ **Same API** - No breaking changes to public interfaces

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-protocol

## 📝 Version Compatibility

| turbomcp-protocol | Replaces | Status |
|-------------------|----------|--------|
| 2.0.0               | turbomcp-core 1.x + turbomcp-protocol 1.x | ✅ Current |
| 1.1.x             | - | 🟡 Maintenance |
| 1.0.x             | - | ⚠️ EOL |
