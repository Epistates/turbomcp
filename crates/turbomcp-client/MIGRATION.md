# turbomcp-client Migration Guide

For workspace-level changes (protocol types, error unification, transport renames), see the
[top-level MIGRATION.md](../../MIGRATION.md).

---

## v1.x to v2.0

v2.0 had no breaking changes to the public client API. Existing code compiles and runs
without modification. The dependency version bump is the only required change:

```toml
# Cargo.toml
[dependencies]
turbomcp-client = "2.0"
```

---

## v2.x to v3.0

v3.0 introduced several breaking changes. The sections below cover each area.

### Plugin system removed

The v2.x plugin system (`ClientPlugin` trait, `with_plugin()` builder method,
`turbomcp_client::plugins` module) has been removed entirely. The `RetryPlugin`,
`CachePlugin`, and `MetricsPlugin` types no longer exist.

Equivalent functionality is now provided through Tower middleware, which is composable
via `tower::ServiceBuilder`:

| v2.x plugin | v3.0 Tower layer |
|---|---|
| `MetricsPlugin` | `turbomcp_client::middleware::MetricsLayer` |
| `CachePlugin` | `turbomcp_client::middleware::CacheLayer` |
| `RetryPlugin` | `tower::retry::RetryLayer` (from the `tower` crate) |

Before (v2.x):
```rust
use turbomcp_client::{ClientBuilder, plugins::*};

let client = ClientBuilder::new()
    .with_plugin(MetricsPlugin::new())
    .with_plugin(CachePlugin::new(Duration::from_secs(300)))
    .build(transport)?;
```

After (v3.0):
```rust
use tower::ServiceBuilder;
use turbomcp_client::middleware::{MetricsLayer, CacheLayer, TracingLayer};
use std::time::Duration;

let service = ServiceBuilder::new()
    .layer(TracingLayer::new())
    .layer(MetricsLayer::new())
    .layer(CacheLayer::default())
    .timeout(Duration::from_secs(30))
    .service(transport);
```

### RetryConfig field names changed

`RetryConfig` (from `turbomcp_transport::resilience`) has renamed fields. Update any
struct literals or field access accordingly:

| v2.x field | v3.0 field |
|---|---|
| `max_retries` | `max_attempts` |
| `initial_backoff` | `base_delay` |
| `max_backoff` | `max_delay` |
| `backoff_multiplier` | `backoff_multiplier` (unchanged) |

Before (v2.x):
```rust
use turbomcp_client::RetryConfig;
use std::time::Duration;

RetryConfig {
    max_retries: 5,
    initial_backoff: Duration::from_millis(100),
    max_backoff: Duration::from_secs(30),
    backoff_multiplier: 2.0,
}
```

After (v3.0):
```rust
use turbomcp_transport::resilience::RetryConfig;
use std::time::Duration;

RetryConfig {
    max_attempts: 5,
    base_delay: Duration::from_millis(100),
    max_delay: Duration::from_secs(30),
    backoff_multiplier: 2.0,
    ..Default::default()
}
```

To use retry with the builder, call `with_retry_config()` and optionally
`build_resilient()` to wrap the transport in `TurboTransport`:

```rust
use turbomcp_client::ClientBuilder;
use turbomcp_transport::resilience::RetryConfig;
use turbomcp_transport::stdio::StdioTransport;
use std::time::Duration;

let client = ClientBuilder::new()
    .with_retry_config(RetryConfig {
        max_attempts: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(30),
        backoff_multiplier: 2.0,
        ..Default::default()
    })
    .build_resilient(StdioTransport::new())
    .await?;
```

### Connection state methods removed

The following methods do not exist in v3.0 and must be removed:

- `client.is_connected()` - no replacement; check the result of `initialize()` or handle
  errors returned by individual requests
- `client.connection_state()` - removed; observe errors from call sites instead
- `client.reconnect()` - removed; create a new `Client` instance with a fresh transport

### Handler trait changes

`ProgressHandler` does not exist in v3.0. The supported handler traits are:

- `ElicitationHandler` - server-initiated user input requests
- `LogHandler` - server log message notifications
- `ResourceUpdateHandler` - resource change notifications
- `RootsHandler` - roots list management
- `CancellationHandler` - request cancellation notifications
- `ResourceListChangedHandler` - resource list change notifications
- `ToolListChangedHandler` - tool list change notifications
- `PromptListChangedHandler` - prompt list change notifications

Remove any use of `with_progress_handler()` or implementations of `ProgressHandler`.
Register remaining handlers via `ClientBuilder`:

```rust
use turbomcp_client::{ClientBuilder, handlers::{ElicitationHandler, LogHandler}};
use std::sync::Arc;

let client = ClientBuilder::new()
    .with_elicitation_handler(Arc::new(MyElicitationHandler))
    .with_log_handler(Arc::new(MyLogHandler))
    .build(transport)
    .await?;
```

### Error types unified

`turbomcp_client::Error` is now re-exported from `turbomcp_protocol::Error`. If you
matched against client-specific error variants, update your match arms to use the unified
error type. See the top-level MIGRATION.md for the full error variant mapping.

### Dependency version

```toml
# Cargo.toml
[dependencies]
turbomcp-client = "3.0"
```

---

## Additional resources

- [Top-level MIGRATION.md](../../MIGRATION.md) - workspace-wide breaking changes
- [API documentation](https://docs.rs/turbomcp-client)
- [Examples](../../crates/turbomcp/examples/)
