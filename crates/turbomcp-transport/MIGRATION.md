> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Transport 2.0.0 Migration Guide

This guide helps you migrate from turbomcp-transport 1.x to 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)

## 🚀 Overview

**TurboMCP Transport 2.0.0** has minimal breaking changes and focuses on enhancements.

### Key Changes
- **Zero Breaking API Changes** - Public API unchanged
- **Enhanced Resilience** - Circuit breaker metrics and better timeout handling
- **Performance Improvements** - Optimized message processing
- **Better Error Messages** - More detailed error context

### Migration Timeline
- **Minimal Impact** - Most users won't need any changes
- **Backward Compatible** - v1.x code works without modification
- **Optional Enhancements** - New features available when needed

## 💥 Breaking Changes

### None! 🎉

**TurboMCP Transport 2.0.0 has ZERO breaking changes.**

All existing transport usage from v1.x continues to work without modification:
- STDIO transport - Works identically
- HTTP/SSE transport - Works identically
- WebSocket transport - Works identically
- TCP transport - Works identically
- Unix domain socket transport - Works identically

## ✨ New Features

### 1. Enhanced Circuit Breaker

**NEW:** Circuit breaker now exposes metrics:

```rust
use turbomcp_transport::resilience::CircuitBreaker;

let breaker = CircuitBreaker::new(config);

// NEW: Access metrics
let metrics = breaker.metrics();
println!("Failures: {}", metrics.failure_count);
println!("State: {:?}", metrics.state);
println!("Last failure: {:?}", metrics.last_failure_time);
```

**Benefits:**
- Better observability
- Easier debugging
- Production monitoring

### 2. Improved Timeout Handling

**ENHANCED:** Better timeout configuration and error messages:

```rust
use turbomcp_transport::stdio::StdioTransport;
use std::time::Duration;

let transport = StdioTransport::new(command)
    .with_timeout(Duration::from_secs(30))  // More granular control
    .with_read_timeout(Duration::from_secs(5))  // Separate read timeout
    .with_write_timeout(Duration::from_secs(5)); // Separate write timeout
```

**Benefits:**
- Fine-grained timeout control
- Better error messages
- Reduced timeout-related issues

### 3. Enhanced Error Context

**IMPROVED:** Error messages now include more context:

```rust
// v1.x error
// "Connection failed"

// v2.0.0 error
// "Connection failed to tcp://localhost:8080: Connection refused (errno 61)"
```

**Benefits:**
- Faster debugging
- Better error tracking
- Clearer production logs

### 4. Performance Optimizations

- **Zero-copy message processing** for supported transports
- **Reduced allocations** in hot paths
- **Better buffering** for network transports
- **Optimized serialization** with SIMD support

## 🔧 Migration Steps

### Step 1: Update Dependencies

```toml
# Before (v1.x)
[dependencies]
turbomcp-transport = "1.1.2"

# After (v2.0.0)
[dependencies]
turbomcp-transport = "2.0.0"
```

### Step 2: Build and Test

```bash
# Clean build
cargo clean
cargo build --all-features

# Run tests
cargo test
```

### Step 3: Optional - Adopt New Features

#### Use Circuit Breaker Metrics

```rust
use turbomcp_transport::resilience::CircuitBreaker;

let breaker = CircuitBreaker::new(config);

// Monitor circuit breaker state
if let Some(metrics) = breaker.metrics() {
    if metrics.failure_count > 10 {
        log::warn!("High failure rate detected");
    }
}
```

#### Configure Granular Timeouts

```rust
use turbomcp_transport::http::HttpTransport;
use std::time::Duration;

let transport = HttpTransport::new(url)
    .with_connect_timeout(Duration::from_secs(5))
    .with_request_timeout(Duration::from_secs(30))
    .with_read_timeout(Duration::from_secs(10));
```

## 🎯 Common Migration Patterns

### Pattern 1: STDIO Transport (No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_transport::stdio::StdioTransport;

let transport = StdioTransport::new("./my-server")?;
```

### Pattern 2: HTTP Transport (No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_transport::http::HttpTransport;

let transport = HttpTransport::new("http://localhost:3000/mcp")?;
```

### Pattern 3: WebSocket Transport (No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_transport::websocket::WebSocketTransport;

let transport = WebSocketTransport::new("ws://localhost:8080")?;
```

### Pattern 4: TCP Transport with New Timeouts

```rust
// v1.x - Basic config
let transport = TcpTransport::new("localhost:8080")?;

// v2.0.0 - Enhanced config (optional)
let transport = TcpTransport::new("localhost:8080")?
    .with_connect_timeout(Duration::from_secs(5))
    .with_read_timeout(Duration::from_secs(10));
```

## 🐛 Troubleshooting

### Issue: Build errors after upgrade

**Solution:** Clean and rebuild:

```bash
cargo clean
cargo build
```

### Issue: Feature flags not recognized

**Solution:** Check feature names are correct:

```toml
# Correct
turbomcp-transport = { version = "2.0.0", features = ["http", "websocket"] }

# Feature names unchanged from v1.x
```

### Issue: Transport not working as expected

**Solution:** Check logs for enhanced error messages:

```rust
// Enable detailed logging
env_logger::init();

// Errors now include more context
```

## 📊 Feature Comparison

| Feature | v1.x | v2.0.0 |
|---------|------|--------|
| STDIO transport | ✅ | ✅ |
| HTTP/SSE transport | ✅ | ✅ Enhanced |
| WebSocket transport | ✅ | ✅ Enhanced |
| TCP transport | ✅ | ✅ Enhanced |
| Unix socket transport | ✅ | ✅ Enhanced |
| Circuit breaker | ✅ | ✅ + Metrics |
| Timeouts | Basic | ✅ Granular |
| Error messages | Basic | ✅ Enhanced |
| Performance | Fast | ✅ Faster |

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-transport
- **Examples**: See `../../examples/` for transport usage
- **Transport Guide**: See README.md for comprehensive documentation

## 🎉 Benefits of 2.0.0

- ✅ **Backward Compatible** - All v1.x code works unchanged
- ✅ **Better Observability** - Circuit breaker metrics and enhanced errors
- ✅ **Performance** - Optimized message processing
- ✅ **Flexibility** - Granular timeout configuration
- ✅ **Production Ready** - Enhanced resilience features

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-transport

## 📝 Version Compatibility

| turbomcp-transport | Status | Migration |
|--------------------|--------|-----------|
| 2.0.0                | ✅ Current | Zero changes needed |
| 1.1.x              | 🟡 Maintenance | Upgrade for new features |
| 1.0.x              | ⚠️ EOL | Upgrade recommended |
