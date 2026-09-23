# Your First Server

Build a complete, production-ready MCP server with state, multiple handlers, and error handling.

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
edition = "2024"

[dependencies]
turbomcp = { version = "3.5.0", features = ["full"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
schemars = "1"
```

## Build a Weather Server

Create `src/main.rs`. We'll use a shared state to simulate a database or cache.

```rust
use std::sync::{Arc, Mutex};
use turbomcp::prelude::*;
use serde::{Deserialize, Serialize};

// 1. Define your server state
#[derive(Clone)]
struct WeatherServer {
    // Shared state must be thread-safe (Arc<Mutex> or similar)
    cache: Arc<Mutex<std::collections::HashMap<String, WeatherData>>>
}

// 2. Define data types
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct WeatherData {
    city: String,
    temperature: f64,
    condition: String,
}

// 3. Implement the server
#[server(name = "weather-server", version = "1.0.0")]
impl WeatherServer {
    /// Get current weather for a city
    ///
    /// `Json<T>` sends the value as structured content and advertises
    /// `WeatherData`'s schema as the tool's output schema.
    #[tool]
    async fn get_weather(
        &self,
        #[description("City name (e.g. 'New York')")]
        city: String,
    ) -> McpResult<Json<WeatherData>> {
        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(data) = cache.get(&city) {
                return Ok(Json(data.clone()));
            }
        }

        // Simulate fetching data
        let weather = WeatherData {
            city: city.clone(),
            temperature: 72.0,
            condition: "Sunny".to_string(),
        };

        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(city, weather.clone());
        }

        Ok(Json(weather))
    }

    /// List all cities we have data for
    #[resource("weather://cities")]
    async fn list_cities(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        let cache = self.cache.lock().unwrap();
        let cities: Vec<String> = cache.keys().cloned().collect();
        Ok(cities.join(", "))
    }

    /// Get a weather analysis prompt
    #[prompt]
    async fn analyze_weather(&self, city: String, ctx: &RequestContext) -> String {
        format!("Analyze the weather patterns for {}, focusing on temperature trends.", city)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize state
    let server = WeatherServer {
        cache: Arc::new(Mutex::new(std::collections::HashMap::new()))
    };

    // Run via STDIO
    server.run_stdio().await?;
    Ok(())
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

## Add HTTP Transport

Want to expose your server over HTTP instead of STDIO? Replace `main` in the
file above with this (the `full` feature already includes `http`):

```rust,ignore
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = WeatherServer {
        cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    // Streamable HTTP at http://localhost:8080/mcp
    server.run_http("0.0.0.0:8080").await?;
    Ok(())
}
```

By default the HTTP transport refuses requests that carry no `Origin` header
unless they come from the local machine, which suits local testing. To serve
non-browser clients over the network, configure the server through
`ServerConfig::builder().allow_missing_origin(true)` and pair that with
authorization; see the [Authentication guide](../guide/authentication.md).

## Key Concepts Applied

✅ **State Management** - Using `Arc<Mutex<...>>` for shared state.
✅ **Zero Boilerplate** - No manual JSON schema definitions.
✅ **Type Safety** - Arguments and return types are strongly typed.
✅ **Documentation** - `#[description]` attributes and doc comments are used by the LLM.
✅ **Multiple Transports** - Switch between STDIO and HTTP easily.

## Next Steps

- **[Handlers Guide](../guide/handlers.md)** - Learn about all handler types.
- **[Deployment](../deployment/docker.md)** - Deploy your server with Docker.
