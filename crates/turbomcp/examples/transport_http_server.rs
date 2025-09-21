//! HTTP/SSE Transport Server - Web-Compatible MCP
//!
//! This example demonstrates the HTTP transport with Server-Sent Events,
//! making MCP accessible from web browsers and HTTP clients.
//!
//! Run with: `cargo run --example transport_http_server`
//! Test with: curl -X POST http://localhost:3000/mcp -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'

use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

/// Weather service using HTTP/SSE transport (macro approach)
#[derive(Clone)]
struct WeatherService {
    locations: Arc<RwLock<Vec<String>>>,
}

#[server(
    name = "Weather Service",
    version = "1.0.0",
    description = "HTTP/SSE transport weather tracking service"
)]
impl WeatherService {
    fn new() -> Self {
        Self {
            locations: Arc::new(RwLock::new(vec![
                "New York".to_string(),
                "London".to_string(),
                "Tokyo".to_string(),
                "Sydney".to_string(),
            ])),
        }
    }

    #[tool("Get weather for a location")]
    async fn get_weather(&self, location: String) -> McpResult<String> {
        let locations = self.locations.read().await;

        if locations.contains(&location) {
            Ok(format!(
                "🌤️ Weather for {}: Partly cloudy, 22°C\n📍 Retrieved via HTTP/SSE transport\n🔗 WebSocket alternative available",
                location
            ))
        } else {
            Err(McpError::tool(format!(
                "Location '{}' not found. Available: {}",
                location,
                locations.join(", ")
            )))
        }
    }

    #[tool("Add a new location to track")]
    async fn add_location(&self, location: String) -> McpResult<String> {
        let mut locations = self.locations.write().await;

        if !locations.contains(&location) {
            locations.push(location.clone());
            Ok(format!("✅ Added {} to tracked locations", location))
        } else {
            Ok(format!("ℹ️  {} is already being tracked", location))
        }
    }

    #[tool("List all tracked locations")]
    async fn list_locations(&self) -> McpResult<String> {
        let locations = self.locations.read().await;
        let list = locations.join(", ");
        Ok(format!("📍 Tracked locations: {}", list))
    }

    #[resource("weather://locations")]
    async fn weather_locations_resource(&self) -> McpResult<String> {
        let locations = self.locations.read().await;
        let list = locations.join("\n• ");
        Ok(format!("📍 Weather Locations:\n• {}", list))
    }

    #[resource("weather://status")]
    async fn weather_status_resource(&self) -> McpResult<String> {
        let locations = self.locations.read().await;
        Ok(format!(
            "🌐 HTTP/SSE Weather Service Status:\n• Transport: HTTP with Server-Sent Events\n• Tracked locations: {}\n• Status: Active",
            locations.len()
        ))
    }

    #[prompt("Create weather report")]
    async fn weather_report_prompt(&self, location: Option<String>) -> McpResult<String> {
        match location {
            Some(loc) => Ok(format!(
                "Generate a detailed weather report for {}. Include current conditions, forecast, and travel recommendations.",
                loc
            )),
            None => Ok("Generate a general weather overview for multiple cities with travel recommendations.".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("🌐 Starting HTTP/SSE Weather Service");
    tracing::info!("🔗 This demonstrates HTTP transport with Server-Sent Events");
    tracing::info!("📡 Alternative to STDIO for web-compatible MCP");

    let service = WeatherService::new();

    tracing::info!("✅ HTTP/SSE server ready to start on http://localhost:3000/mcp");
    tracing::info!(
        "🧪 Test with: curl -X POST http://localhost:3000/mcp -H \"Content-Type: application/json\" -d '{{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}}'"
    );

    // Run on REAL HTTP/SSE transport for web compatibility
    service.run_http_with_path("127.0.0.1:3000", "/mcp").await?;

    Ok(())
}
