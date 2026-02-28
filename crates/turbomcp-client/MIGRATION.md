> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Client 2.0.0 Migration Guide

This guide helps you migrate from turbomcp-client 1.x to 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)

## 🚀 Overview

**TurboMCP Client 2.0.0** has minimal breaking changes and focuses on enhancements.

### Key Changes
- **Zero Breaking API Changes** - Public API unchanged
- **Enhanced Connection Management** - Better retry and recovery
- **Improved Error Handling** - More detailed error context
- **Better Plugin System** - Enhanced plugin capabilities

### Migration Timeline
- **Zero Impact** - Most users won't need any changes
- **Backward Compatible** - v1.x code works without modification
- **Optional Enhancements** - New features available when needed

## 💥 Breaking Changes

### None! 🎉

**TurboMCP Client 2.0.0 has ZERO breaking changes.**

All existing client usage from v1.x continues to work without modification:
- Connection management - Works identically
- Tool calling - Works identically
- Resource reading - Works identically
- Prompt handling - Works identically
- Plugin system - Works identically (enhanced!)

## ✨ New Features

### 1. Enhanced Connection Recovery

**IMPROVED:** Better automatic retry and recovery:

```rust
use turbomcp_client::{ClientBuilder, RetryConfig};
use std::time::Duration;

let client = ClientBuilder::new()
    .with_retry_config(RetryConfig {
        max_retries: 5,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(30),
        backoff_multiplier: 2.0,
    })
    .build(transport)?;
```

**Benefits:**
- Automatic reconnection on transient failures
- Configurable backoff strategies
- Better resilience in production

### 2. Improved Error Context

**ENHANCED:** Error messages now include more context:

```rust
// v1.x error
// "Tool call failed"

// v2.0.0 error
// "Tool call 'process_data' failed after 3 retries: Connection timeout (last error: IO error: Connection refused)"
```

**Benefits:**
- Faster debugging
- Better error tracking
- Clearer production logs

### 3. Enhanced Plugin System

**IMPROVED:** Better plugin composition and lifecycle:

```rust
use turbomcp_client::{ClientBuilder, plugins::*};

let client = ClientBuilder::new()
    .with_plugin(RetryPlugin::new(config))
    .with_plugin(CachePlugin::new(Duration::from_secs(300)))
    .with_plugin(MetricsPlugin::new())
    .build(transport)?;
```

**Benefits:**
- Better plugin ordering
- Enhanced plugin capabilities
- More plugin hooks

### 4. Bidirectional Handler Support

**ENHANCED:** Better support for server-initiated requests:

```rust
use turbomcp_client::{ClientBuilder, handlers::*};

let client = ClientBuilder::new()
    .with_progress_handler(|progress| {
        println!("Progress: {}%", progress.percentage);
    })
    .with_log_handler(|log| {
        eprintln!("[{}] {}", log.level, log.message);
    })
    .with_resource_update_handler(|update| {
        println!("Resource updated: {}", update.uri);
    })
    .build(transport)?;
```

**Benefits:**
- Real-time progress tracking
- Better logging integration
- Resource change notifications

### 5. Connection State Management

**NEW:** Better connection state tracking:

```rust
use turbomcp_client::Client;

let client = Client::new(transport)?;

// Check connection state
if client.is_connected() {
    println!("Connected");
}

// Get detailed state
let state = client.connection_state();
println!("State: {:?}, Uptime: {:?}", state.status, state.uptime);
```

## 🔧 Migration Steps

### Step 1: Update Dependencies

```toml
# Before (v1.x)
[dependencies]
turbomcp-client = "1.1.2"

# After (v2.0.0)
[dependencies]
turbomcp-client = "2.0.0"
```

### Step 2: Build and Test

```bash
# Clean build
cargo clean
cargo build

# Run tests
cargo test
```

### Step 3: Optional - Adopt New Features

#### Use Enhanced Retry Configuration

```rust
use turbomcp_client::{ClientBuilder, RetryConfig};

let client = ClientBuilder::new()
    .with_retry_config(RetryConfig::default()
        .with_max_retries(5)
        .with_exponential_backoff()
    )
    .build(transport)?;
```

#### Add Bidirectional Handlers

```rust
let client = ClientBuilder::new()
    .with_progress_handler(|p| {
        log::info!("Progress: {}/{}", p.completed, p.total);
    })
    .build(transport)?;
```

#### Use Connection State Tracking

```rust
// Monitor connection health
if !client.is_connected() {
    log::warn!("Client disconnected, reconnecting...");
    client.reconnect().await?;
}
```

## 🎯 Common Migration Patterns

### Pattern 1: Basic Client (No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

let transport = StdioTransport::new("./server")?;
let client = Client::new(transport)?;

// Call tools
let result = client.call_tool("my_tool", args).await?;
```

### Pattern 2: Client with Plugins (No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_client::{ClientBuilder, plugins::*};

let client = ClientBuilder::new()
    .with_plugin(RetryPlugin::new(config))
    .with_plugin(CachePlugin::new(cache_config))
    .build(transport)?;
```

### Pattern 3: Enhanced Retry (NEW)

```rust
// v2.0.0 - Enhanced configuration
use turbomcp_client::{ClientBuilder, RetryConfig};
use std::time::Duration;

let client = ClientBuilder::new()
    .with_retry_config(RetryConfig {
        max_retries: 3,
        initial_backoff: Duration::from_millis(500),
        max_backoff: Duration::from_secs(60),
        backoff_multiplier: 2.0,
        retry_on_network_errors: true,
        retry_on_timeout: true,
    })
    .build(transport)?;
```

### Pattern 4: Bidirectional Communication (ENHANCED)

```rust
// v2.0.0 - Better handler support
use turbomcp_client::ClientBuilder;

let client = ClientBuilder::new()
    .with_progress_handler(|progress| {
        println!("{}%", progress.percentage);
    })
    .with_log_handler(|log| {
        match log.level {
            "error" => eprintln!("{}", log.message),
            _ => println!("{}", log.message),
        }
    })
    .build(transport)?;
```

## 🐛 Troubleshooting

### Issue: Build errors after upgrade

**Solution:** Clean and rebuild:

```bash
cargo clean
cargo build
```

### Issue: Connection issues after upgrade

**Solution:** Check retry configuration:

```rust
// Ensure retry is enabled
let client = ClientBuilder::new()
    .with_retry_config(RetryConfig::default())
    .build(transport)?;
```

### Issue: Plugin not working

**Solution:** Check plugin order:

```rust
// Plugins execute in order registered
let client = ClientBuilder::new()
    .with_plugin(AuthPlugin::new())      // First
    .with_plugin(RetryPlugin::new())     // Second
    .with_plugin(MetricsPlugin::new())   // Third
    .build(transport)?;
```

## 📊 Feature Comparison

| Feature | v1.x | v2.0.0 |
|---------|------|--------|
| Tool calling | ✅ | ✅ |
| Resource reading | ✅ | ✅ |
| Prompt handling | ✅ | ✅ |
| Plugin system | ✅ | ✅ Enhanced |
| Connection retry | Basic | ✅ Enhanced |
| Error messages | Basic | ✅ Enhanced |
| Bidirectional handlers | ✅ | ✅ Enhanced |
| Connection state | Basic | ✅ Enhanced |

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-client
- **Examples**: See `../../examples/` for client usage patterns
- **Plugin Guide**: See README.md for plugin documentation

## 🎉 Benefits of 2.0.0

- ✅ **Backward Compatible** - All v1.x code works unchanged
- ✅ **Better Resilience** - Enhanced retry and recovery
- ✅ **Improved Errors** - More detailed error context
- ✅ **Enhanced Plugins** - Better plugin capabilities
- ✅ **Production Ready** - Better connection management

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-client

## 📝 Version Compatibility

| turbomcp-client | Status | Migration |
|-----------------|--------|-----------|
| 2.0.0             | ✅ Current | Zero changes needed |
| 1.1.x           | 🟡 Maintenance | Upgrade for new features |
| 1.0.x           | ⚠️ EOL | Upgrade recommended |
