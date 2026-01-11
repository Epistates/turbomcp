# Your First Server

Build a complete, production-ready MCP server with multiple handlers, context injection, and error handling.

## Project Setup

Create a new project:

```bash
cargo new weather-mcp-server
cd weather-mcp-server
```

Update `Cargo.toml`:

```toml
[package]
name = "weather-mcp-server"
version = "0.1.0"
edition = "2021"

[dependencies]
turbomcp = { version = "3.0.0-exp", features = ["full"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## Build a Weather Server

Create `src/main.rs`:

```rust
use turbomcp::prelude::*;
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .with_name("weather-server")
        .stdio()
        .run()
        .await?;

    Ok(())
}

// Tool: Get weather for a city
#[tool(description = "Get current weather for a city")]
async fn get_weather(
    #[description = "City name"]
    city: String,
) -> McpResult<WeatherData> {
    // Simulate fetching weather data
    let weather = WeatherData {
        city,
        temperature: 72.0,
        condition: "Sunny".to_string(),
        humidity: 65,
    };

    Ok(weather)
}

// Tool: Get forecast
#[tool(description = "Get 7-day weather forecast")]
async fn get_forecast(
    #[description = "City name"]
    city: String,
    logger: Logger,
) -> McpResult<String> {
    logger.info(&format!("Fetching forecast for {}", city)).await?;

    // Simulate API call
    let forecast = vec![
        "Day 1: Sunny, 75°F",
        "Day 2: Cloudy, 70°F",
        "Day 3: Rainy, 65°F",
        "Day 4: Sunny, 75°F",
        "Day 5: Sunny, 78°F",
        "Day 6: Cloudy, 72°F",
        "Day 7: Rainy, 68°F",
    ];

    Ok(forecast.join("\n"))
}

// Resource: List supported cities
#[resource(uri = "cities://list", description = "List all supported cities")]
async fn list_cities(
    cache: Cache,
) -> McpResult<String> {
    // Check cache first
    if let Some(cached) = cache.get::<String>("cities")? {
        return Ok(cached);
    }

    let cities = vec!["New York", "Los Angeles", "Chicago", "Houston"];
    let result = cities.join(", ");

    // Cache the result
    cache.set("cities", &result).await?;

    Ok(result)
}

// Prompt: Weather analysis template
#[prompt(description = "Analyze weather patterns")]
async fn analyze_weather(
    logger: Logger,
) -> McpResult<String> {
    logger.info("Providing weather analysis template").await?;

    Ok(r#"
Analyze the weather data provided:
1. Identify patterns
2. Predict trends
3. Suggest recommendations

Format your response as:
- Pattern: [description]
- Prediction: [forecast]
- Recommendation: [suggestion]
"#.to_string())
}

// Response type (must be serializable)
#[derive(Serialize, Deserialize, Debug)]
struct WeatherData {
    city: String,
    temperature: f64,
    condition: String,
    humidity: u32,
}
```

## Test Your Server

### Using the CLI

First, install the TurboMCP CLI:

```bash
cargo install turbomcp-cli
```

Build your server:

```bash
cargo build --release
```

List available tools:

```bash
turbomcp-cli tools list --command "./target/release/weather-mcp-server"
```

Call a tool:

```bash
turbomcp-cli tools call get_weather \
  --arguments '{"city": "New York"}' \
  --command "./target/release/weather-mcp-server"
```

### Using Raw JSON-RPC

In one terminal, run the server:

```bash
cargo run
```

In another, send a request:

```bash
cat <<'EOF' | ./target/debug/weather-mcp-server
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_weather","arguments":{"city":"New York"}}}
EOF
```

## Add Configuration

Update `src/main.rs` to add configuration:

```rust
#[tokio::main]
async fn main() -> McpResult<()> {
    let mut config = Config::new();
    config.set("api_key", "your-api-key")?;
    config.set("cache_ttl", 3600)?;

    let server = McpServer::new()
        .with_name("weather-server")
        .with_config(config)
        .stdio()
        .run()
        .await?;

    Ok(())
}
```

Then use it in handlers:

```rust
#[tool]
async fn get_weather(
    city: String,
    config: Config,
) -> McpResult<WeatherData> {
    let api_key: Option<String> = config.get("api_key")?;
    let cache_ttl: Option<u32> = config.get("cache_ttl")?;

    // Use API key and cache TTL...
    Ok(weather)
}
```

## Add HTTP Transport

Add HTTP support:

```rust
#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .with_name("weather-server")
        .stdio()
        .http(8080)           // HTTP on port 8080
        .websocket(8081)      // WebSocket on port 8081
        .run()
        .await?;

    Ok(())
}
```

Now you can make HTTP requests:

```bash
curl -X POST http://localhost:8080/tools/call \
  -H "Content-Type: application/json" \
  -d '{"name": "get_weather", "arguments": {"city": "New York"}}'
```

## Add Graceful Shutdown

Handle signals properly:

```rust
#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .with_name("weather-server")
        .stdio()
        .with_graceful_shutdown(std::time::Duration::from_secs(30))
        .run()
        .await?;

    Ok(())
}
```

## Complete Example

See the full example in the [examples/weather.rs](https://github.com/turbomcp/turbomcp/blob/main/crates/turbomcp/examples/weather.rs) file in the repository.

## Next Steps

- **[Handlers Guide](../guide/handlers.md)** - All handler types
- **[Context & DI](../guide/context-injection.md)** - Dependency injection
- **[Authentication](../guide/authentication.md)** - Add OAuth
- **[Deployment](../deployment/docker.md)** - Deploy to production
- **[Examples](../examples/basic.md)** - More real-world patterns

## Key Concepts Applied

✅ **Multiple handler types** - Tools, resources, prompts
✅ **Dependency injection** - Logger, cache, config
✅ **Error handling** - Proper error types
✅ **Documentation** - Descriptions for schema
✅ **Caching** - In-memory caching pattern
✅ **Configuration** - Runtime configuration
✅ **Multiple transports** - STDIO, HTTP, WebSocket

---

Great job! You've built a complete MCP server. Ready to deploy? → [Deployment Guide](../deployment/docker.md)
