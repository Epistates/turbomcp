//! # Structured Output Example
//!
//! Demonstrates how to use structured_content with output_schema for type-safe,
//! schema-validated tool responses. This example shows best practices for:
//!
//! - Defining output schemas for tools
//! - Returning structured_content alongside text for backward compatibility
//! - Using structured_content in clients for programmatic access
//! - Handling annotations and metadata
//!
//! Run with: `cargo run --example structured_output`

use serde::{Deserialize, Serialize};
use serde_json::json;
use turbomcp::prelude::*;

#[derive(Clone)]
struct WeatherServer;

/// Weather data structure matching our output schema
#[derive(Debug, Serialize, Deserialize)]
struct WeatherData {
    temperature: f64,
    conditions: String,
    humidity: u32,
    wind_speed: f64,
    forecast: Vec<String>,
}

/// Search result structure
#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    total_results: u32,
    results: Vec<SearchItem>,
    query_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchItem {
    title: String,
    url: String,
    snippet: String,
    relevance_score: f64,
}

#[turbomcp::server(
    name = "structured-output-demo",
    version = "1.0.0",
    description = "Demonstrates structured output with output_schema",
    transports = ["stdio"]
)]
impl WeatherServer {
    /// Get current weather with structured output
    ///
    /// This tool demonstrates the MCP best practice for structured output:
    /// 1. Define an output_schema for validation
    /// 2. Return structured_content matching the schema
    /// 3. Include text representation for backward compatibility
    #[tool("Get current weather conditions with detailed data")]
    async fn get_weather(&self, _location: String) -> McpResult<serde_json::Value> {
        // Simulate weather data
        let weather = WeatherData {
            temperature: 22.5,
            conditions: "Partly cloudy".to_string(),
            humidity: 65,
            wind_speed: 12.3,
            forecast: vec![
                "Clear skies tomorrow".to_string(),
                "Rain expected Friday".to_string(),
            ],
        };

        // MCP Best Practice: Return BOTH structured and text content
        //
        // The structured_content field is automatically populated from our return value.
        // The text representation is generated from the JSON for backward compatibility.
        //
        // When using the #[tool] macro, it handles this automatically!

        Ok(serde_json::to_value(&weather)?)
    }

    /// Search with rich structured results
    #[tool("Search for information and return structured results")]
    async fn search(
        &self,
        query: String,
        max_results: Option<u32>,
    ) -> McpResult<serde_json::Value> {
        let max = max_results.unwrap_or(5);

        // Simulate search results
        let results = SearchResult {
            total_results: 42,
            results: (0..max.min(5))
                .map(|i| SearchItem {
                    title: format!("Result {} for '{}'", i + 1, query),
                    url: format!("https://example.com/result-{}", i + 1),
                    snippet: format!(
                        "This is a snippet containing information about {}...",
                        query
                    ),
                    relevance_score: 0.95 - (i as f64 * 0.1),
                })
                .collect(),
            query_time_ms: 42,
        };

        Ok(serde_json::to_value(&results)?)
    }

    /// Example showing manual CallToolResult creation
    ///
    /// For advanced cases where you need fine control over both text and
    /// structured output, you can construct CallToolResult directly.
    #[tool("Get system status with custom formatting")]
    async fn system_status(&self) -> McpResult<CallToolResult> {
        use turbomcp_protocol::types::{CallToolResult, ContentBlock, TextContent};

        // Create structured output
        let structured = json!({
            "status": "healthy",
            "uptime_seconds": 86400,
            "memory_mb": 512,
            "cpu_percent": 23.5,
            "active_connections": 42
        });

        // Create human-friendly text version
        let text = "System Status:\n\
                   - Status: Healthy ✓\n\
                   - Uptime: 1 day\n\
                   - Memory: 512 MB\n\
                   - CPU: 23.5%\n\
                   - Connections: 42";

        Ok(CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: text.to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: Some(false),
            structured_content: Some(structured),
            _meta: Some(json!({
                "timestamp": "2025-11-06T10:30:00Z",
                "collection_time_ms": 15
            })),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Structured Output Example Server");
    println!("=================================\n");
    println!("This server demonstrates MCP structured output best practices:\n");
    println!("1. Tools return structured_content for programmatic access");
    println!("2. Text content is included for backward compatibility");
    println!("3. Metadata (_meta) carries tracking information\n");
    println!("Available tools:");
    println!("  - get_weather: Returns weather data with schema validation");
    println!("  - search: Returns search results with relevance scores");
    println!("  - system_status: Returns system metrics with custom formatting\n");
    println!("Client Usage Example:");
    println!("---------------------");
    println!("```rust");
    println!("let result = client.call_tool(\"get_weather\", args).await?;");
    println!();
    println!("// Access structured output (preferred)");
    println!("if let Some(structured) = result.structured_content {{");
    println!("    let weather: WeatherData = serde_json::from_value(structured)?;");
    println!("    println!(\"Temp: {{}}°C\", weather.temperature);");
    println!("}}");
    println!();
    println!("// Or use text for display (backward compatible)");
    println!("println!(\"Text: {{}}\", result.first_text().unwrap_or(\"\"));");
    println!("```\n");

    WeatherServer.run_stdio().await?;
    Ok(())
}
