# Configuration Examples

This directory contains example configuration files for TurboMCP.

## File-Based Configuration

TurboMCP supports loading configuration from files in multiple formats:

- **TOML** (`.toml`) - Recommended for production
- **YAML** (`.yaml`, `.yml`) - Alternative format
- **JSON** (`.json`) - Useful for programmatic generation

### Usage

```rust
use turbomcp_server::ServerConfig;

// Load from TOML file
let config = ServerConfig::from_file("config.toml")?;

// Load from YAML file
let config = ServerConfig::from_file("config.yaml")?;

// Load from JSON file
let config = ServerConfig::from_file("config.json")?;
```

### Environment Variables

Environment variables with the `TURBOMCP_` prefix will override file settings following the 12-factor app methodology:

```bash
# Override port from environment
TURBOMCP_PORT=9000 ./my-server

# Override nested config (use __ for nesting)
TURBOMCP_TIMEOUTS__REQUEST_TIMEOUT=60 ./my-server

# Custom environment prefix
# Use from_file_with_prefix() for custom prefix
MYAPP_PORT=9000 ./my-server
```

### Examples

- **`server.toml`** - Full configuration example with all options (TOML format)
- **`server.yaml`** - Full configuration example with all options (YAML format)
- **`minimal.toml`** - Minimal configuration showing bare essentials

### Programmatic Configuration

If you prefer programmatic configuration without files:

```rust
use turbomcp_server::ServerConfig;
use std::time::Duration;

let config = ServerConfig::builder()
    .name("my-server")
    .port(9000)
    .bind_address("0.0.0.0")
    .request_timeout(Duration::from_secs(30))
    .rate_limiting(100, 200)
    .log_level("debug")
    .build();
```

## Configuration Options

### Basic Settings

- `name` - Server name (string)
- `version` - Server version (string)
- `description` - Server description (optional string)
- `bind_address` - IP address to bind to (string, default: "127.0.0.1")
- `port` - Port to listen on (number, default: 8080)

### TLS Settings

- `enable_tls` - Enable TLS/HTTPS (boolean, default: false)
- `tls.cert_file` - Path to TLS certificate file (path)
- `tls.key_file` - Path to TLS private key file (path)

### Timeouts

- `timeouts.request_timeout` - Maximum request duration (duration, default: "30s")
- `timeouts.connection_timeout` - Connection timeout (duration, default: "10s")
- `timeouts.keep_alive_timeout` - Keep-alive timeout (duration, default: "60s")
- `timeouts.tool_execution_timeout` - Default tool timeout (duration, default: "60s")
- `timeouts.tool_timeouts.<tool_name>` - Per-tool timeout overrides (seconds)

### Rate Limiting

- `rate_limiting.enabled` - Enable rate limiting (boolean, default: false)
- `rate_limiting.requests_per_second` - Max requests/sec (number, default: 100)
- `rate_limiting.burst_capacity` - Burst capacity (number, default: 200)

### Logging

- `logging.level` - Log level: "trace", "debug", "info", "warn", "error" (string, default: "info")
- `logging.structured` - Enable structured logging (boolean, default: false)
- `logging.file` - Optional log file path (path, optional)

### Additional Settings

Use the `additional` section for custom application-specific settings:

```toml
[additional]
max_connections = 1000
custom_setting = "value"
```

Access via:
```rust
let max_conn: u32 = config.additional.get("max_connections")
    .and_then(|v| v.as_u64())
    .unwrap_or(100) as u32;
```
