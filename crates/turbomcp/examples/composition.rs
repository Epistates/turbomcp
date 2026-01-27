//! # Server Composition Example
//!
//! Demonstrates using CompositeHandler to mount multiple MCP servers
//! into a single unified server with namespaced tools and resources.
//!
//! This enables:
//! - Modular server design
//! - Team-based development (each team owns a handler)
//! - Avoiding naming conflicts through prefixes
//!
//! Run with: `cargo run --example composition`

use turbomcp::__macro_support::turbomcp_core::handler::McpHandler;
use turbomcp::prelude::*;
use turbomcp_server::CompositeHandler;

// ============================================================================
// Weather Service
// ============================================================================

#[derive(Clone)]
struct WeatherService;

#[turbomcp::server(name = "weather", version = "1.0.0")]
impl WeatherService {
    /// Get current weather for a city
    #[tool(description = "Get current weather")]
    async fn get_current(&self, city: String) -> McpResult<String> {
        Ok(format!("Weather in {}: Sunny, 72°F", city))
    }

    /// Get weather forecast
    #[tool(description = "Get 5-day forecast")]
    async fn get_forecast(&self, city: String, days: Option<u32>) -> McpResult<String> {
        let days = days.unwrap_or(5);
        Ok(format!(
            "{}-day forecast for {}: Sunny -> Cloudy -> Rain",
            days, city
        ))
    }

    /// Weather alerts resource
    #[resource("alerts://active", description = "Active weather alerts")]
    async fn get_alerts(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"alerts": ["Heat advisory until 8PM"]}"#.into())
    }
}

// ============================================================================
// News Service
// ============================================================================

#[derive(Clone)]
struct NewsService;

#[turbomcp::server(name = "news", version = "1.0.0")]
impl NewsService {
    /// Get top headlines
    #[tool(description = "Get top news headlines")]
    async fn get_headlines(&self, category: Option<String>) -> McpResult<String> {
        let cat = category.unwrap_or_else(|| "general".into());
        Ok(format!(
            "Top {} headlines: AI advances, Tech stocks rise",
            cat
        ))
    }

    /// Search news articles
    #[tool(description = "Search news articles")]
    async fn search(&self, query: String) -> McpResult<String> {
        Ok(format!("Found 42 articles matching '{}'", query))
    }

    /// News feed resource
    #[resource("feed://latest", description = "Latest news feed")]
    async fn get_feed(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"articles": [{"title": "Breaking: AI advances"}]}"#.into())
    }
}

// ============================================================================
// Calculator Service
// ============================================================================

#[derive(Clone)]
struct CalculatorService;

#[turbomcp::server(name = "calc", version = "1.0.0")]
impl CalculatorService {
    /// Add two numbers
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    /// Multiply two numbers
    #[tool(description = "Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Server Composition Demo ===\n");

    // Create individual services
    let weather = WeatherService;
    let news = NewsService;
    let calc = CalculatorService;

    // Show tools from each service
    println!("Individual services:\n");

    println!("Weather Service tools:");
    for tool in weather.list_tools() {
        println!("  - {}", tool.name);
    }

    println!("\nNews Service tools:");
    for tool in news.list_tools() {
        println!("  - {}", tool.name);
    }

    println!("\nCalculator Service tools:");
    for tool in calc.list_tools() {
        println!("  - {}", tool.name);
    }

    // Compose into a single server
    println!("\n=== Composed Server ===\n");

    let composite = CompositeHandler::new("unified-api", "1.0.0")
        .with_description("Unified API combining weather, news, and calculator")
        .mount(weather, "weather")
        .mount(news, "news")
        .mount(calc, "calc");

    // Show server info
    let info = composite.server_info();
    println!("Server: {} v{}", info.name, info.version);
    println!("Mounted handlers: {}", composite.handler_count());
    println!("Prefixes: {:?}", composite.prefixes());

    // Show all namespaced tools
    println!("\nAll tools (namespaced):");
    println!("-----------------------");
    for tool in composite.list_tools() {
        println!("  {} - {:?}", tool.name, tool.description);
    }

    // Show all namespaced resources
    println!("\nAll resources (namespaced):");
    println!("---------------------------");
    for resource in composite.list_resources() {
        println!("  {} ({})", resource.name, resource.uri);
    }

    // Demonstrate calling tools
    println!("\n=== Tool Calls ===\n");

    tokio::runtime::Runtime::new()?.block_on(async {
        let ctx = RequestContext::default();

        // Call weather tool
        let result = composite
            .call_tool(
                "weather_get_current",
                serde_json::json!({"city": "Seattle"}),
                &ctx,
            )
            .await?;
        println!("weather_get_current: {:?}", result.first_text());

        // Call news tool
        let result = composite
            .call_tool("news_get_headlines", serde_json::json!({}), &ctx)
            .await?;
        println!("news_get_headlines: {:?}", result.first_text());

        // Call calc tool
        let result = composite
            .call_tool("calc_add", serde_json::json!({"a": 5, "b": 3}), &ctx)
            .await?;
        println!("calc_add: {:?}", result.first_text());

        Ok::<_, McpError>(())
    })?;

    // Demonstrate error handling for duplicate prefixes
    println!("\n=== Error Handling ===\n");

    let result = CompositeHandler::new("test", "1.0.0")
        .mount(WeatherService, "api")
        .try_mount(NewsService, "api"); // Duplicate prefix!

    match result {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Correctly caught duplicate prefix: {}", e),
    }

    println!("\nNote: Use try_mount() for fallible mounting or mount() to panic on duplicates.");

    Ok(())
}
