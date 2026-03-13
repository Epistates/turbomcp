> For workspace-level migration (protocol, server, macros, etc.), see the [top-level MIGRATION.md](../../MIGRATION.md).

# turbomcp-transport Migration Guide

This guide covers breaking changes and migration steps for the `turbomcp-transport` crate.

---

## v2.x to v3.0

v3.0 is the only version of this crate with significant breaking changes. The primary change is architectural: transport implementations were extracted from the monolithic `turbomcp-transport` crate into individual crates (`turbomcp-stdio`, `turbomcp-http`, `turbomcp-websocket`, `turbomcp-tcp`, `turbomcp-unix`). The `turbomcp-transport` crate is now an aggregator that re-exports from these modular crates.

### Modular transport crates

Each transport is now a standalone crate:

| Transport | Crate |
|-----------|-------|
| STDIO | `turbomcp-stdio` |
| HTTP/SSE | `turbomcp-http` |
| WebSocket | `turbomcp-websocket` |
| TCP | `turbomcp-tcp` |
| Unix sockets | `turbomcp-unix` |

If you depend only on `turbomcp-transport`, the re-exports preserve your existing import paths and no changes are required for most users. If you depended directly on transport implementation types from `turbomcp-transport::stdio`, `turbomcp-transport::http`, etc., those modules now re-export from the underlying crates. Existing paths remain valid.

If you want a minimal build without the aggregator, you can depend directly on the individual crates:

```toml
# Minimal: only STDIO
[dependencies]
turbomcp-stdio = "3.0"

# Only HTTP
[dependencies]
turbomcp-http = "3.0"
```

### Feature flag changes

Feature names are unchanged. The default feature remains `stdio`.

```toml
# v2.x and v3.0 - feature names identical
turbomcp-transport = { version = "3.0", features = ["http", "websocket", "tcp"] }
```

Each feature now activates the corresponding modular crate as an optional dependency in addition to pulling in required framework crates (axum, tower, tokio-tungstenite, etc.):

| Feature | Activates |
|---------|-----------|
| `stdio` | `dep:turbomcp-stdio` |
| `http` | `dep:turbomcp-http`, `axum`, `tower`, `tower-http`, `async-stream` |
| `websocket` | `dep:turbomcp-websocket`, `tokio-tungstenite`, `http`, `futures-util` |
| `tcp` | `dep:turbomcp-tcp`, `tokio/net` |
| `unix` | `dep:turbomcp-unix`, `tokio/net` |

### Timeout configuration

In v3.0, timeouts are configured via `TransportConfigBuilder` with a `TimeoutConfig` struct. Setting timeouts directly on individual transport types (e.g., `.with_timeout()` on `StdioTransport`) is not supported.

```rust
use turbomcp_transport::config::{TransportConfigBuilder, TimeoutConfig, TransportType};

// Use a preset
let config = TransportConfigBuilder::new(TransportType::Http)
    .timeouts(TimeoutConfig::fast())
    .build()?;

// Or configure manually
let timeouts = TimeoutConfig {
    connect: Duration::from_secs(10),
    request: Some(Duration::from_secs(30)),
    read: Some(Duration::from_secs(15)),
    total: Some(Duration::from_secs(60)),
};
let config = TransportConfigBuilder::new(TransportType::Http)
    .timeouts(timeouts)
    .build()?;
```

`TimeoutConfig` presets:

| Preset | connect | request | read | total |
|--------|---------|---------|------|-------|
| `default()` | 30s | 60s | 30s | 120s |
| `fast()` | 5s | 10s | 5s | 15s |
| `patient()` | 60s | 300s | 120s | 600s |
| `unlimited()` | 30s | None | None | None |

### Circuit breaker API

The circuit breaker statistics method is `statistics()`, returning `CircuitBreakerStats`. There is no `metrics()` method.

```rust
use turbomcp_transport::resilience::{CircuitBreaker, CircuitBreakerConfig};

let mut breaker = CircuitBreaker::new(CircuitBreakerConfig::default());

// Record results
breaker.record_result(true, Duration::from_millis(50));

// Read statistics
let stats = breaker.statistics();
println!("State: {:?}", stats.state);
println!("Failure count: {}", stats.failure_count);
println!("Failure rate: {:.2}", stats.failure_rate);
println!("Avg duration: {:?}", stats.avg_operation_duration);
```

The returned `CircuitBreakerStats` struct contains:

| Field | Type | Description |
|-------|------|-------------|
| `state` | `CircuitState` | Current state (Closed, Open, HalfOpen) |
| `failure_count` | `u32` | Failures in current window |
| `success_count` | `u32` | Successes in half-open state |
| `failure_rate` | `f64` | Rate from 0.0 to 1.0 |
| `avg_operation_duration` | `Duration` | Rolling window average |
| `time_in_current_state` | `Duration` | Time since last state change |

### Runtime transport feature detection

v3.0 adds `Features` for runtime detection of compiled-in transports:

```rust
use turbomcp_transport::Features;

if Features::has_websocket() {
    // websocket feature was compiled in
}

let available = Features::available_transports();
```

---

## v1.x to v2.0

v2.0 had no breaking changes to the public API. All v1.x transport code compiles and behaves identically under v2.0. The only required change is the version number in `Cargo.toml`:

```toml
# Before
turbomcp-transport = "1"

# After
turbomcp-transport = "2"
```

v2.0 additions (all opt-in, no migration required):

- Circuit breaker statistics via `CircuitBreaker::statistics()`
- `TransportConfigBuilder` with `TimeoutConfig` and `LimitsConfig`
- Enhanced error context in `TransportError` variants

---

## Additional resources

- [Top-level MIGRATION.md](../../MIGRATION.md) - Workspace-wide migration guide
- [API documentation](https://docs.rs/turbomcp-transport)
- [Transport README](README.md)
- [GitHub issues](https://github.com/Epistates/turbomcp/issues)
