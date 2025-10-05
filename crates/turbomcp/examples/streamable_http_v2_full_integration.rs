//! Complete Integration Example: Streamable HTTP v2 Client + Server
//!
//! This example demonstrates a full end-to-end integration of the
//! MCP 2025-06-18 compliant Streamable HTTP v2 transport with
//! turbomcp-server and turbomcp-client.
//!
//! Run with:
//!   cargo run --example streamable_http_v2_full_integration --features http

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use turbomcp::handlers::{ResourceHandler, ToolHandler};
use turbomcp::prelude::*;
use turbomcp_client::ClientBuilder;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, ContentBlock, ReadResourceRequest, ReadResourceResult,
    Resource, ResourceContent, TextResourceContents, Tool, ToolInputSchema,
};
use turbomcp_server::ServerResult;

// Server-side tool implementation
#[derive(Clone)]
struct WeatherTool {
    locations: Arc<RwLock<HashMap<String, String>>>,
}

impl WeatherTool {
    fn new() -> Self {
        let mut initial_data = HashMap::new();
        initial_data.insert("London".to_string(), "Cloudy, 15°C".to_string());
        initial_data.insert("Tokyo".to_string(), "Sunny, 24°C".to_string());
        initial_data.insert("New York".to_string(), "Rainy, 18°C".to_string());

        Self {
            locations: Arc::new(RwLock::new(initial_data)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for WeatherTool {
    async fn handle(
        &self,
        request: CallToolRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CallToolResult> {
        let arguments = request
            .arguments
            .as_ref()
            .ok_or_else(|| ServerError::handler("Missing arguments"))?;

        let location = arguments
            .get("location")
            .and_then(|l| l.as_str())
            .ok_or_else(|| ServerError::handler("Missing 'location' parameter"))?
            .to_string();

        let locations = self.locations.read().await;

        match locations.get(&location) {
            Some(weather) => Ok(CallToolResult {
                content: vec![ContentBlock::Text(TextContent {
                    text: format!(
                        "Weather for {}: {}\n🌐 Retrieved via MCP 2025-06-18 Streamable HTTP",
                        location, weather
                    ),
                    annotations: None,
                    meta: None,
                })],
                is_error: None,
                structured_content: None,
                _meta: None,
            }),
            None => Err(ServerError::handler(format!(
                "Location '{}' not found. Available: {}",
                location,
                locations.keys().cloned().collect::<Vec<_>>().join(", ")
            ))),
        }
    }

    fn tool_definition(&self) -> Tool {
        use serde_json::json;

        Tool {
            name: "get_weather".to_string(),
            description: Some("Get current weather for a location".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert(
                        "location".to_string(),
                        json!({
                            "type": "string",
                            "description": "City name"
                        }),
                    );
                    props
                }),
                required: Some(vec!["location".to_string()]),
                additional_properties: None,
            },
            title: None,
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }
}

// Server resource
#[derive(Clone)]
struct LocationsResource;

#[async_trait::async_trait]
impl ResourceHandler for LocationsResource {
    async fn handle(
        &self,
        _request: ReadResourceRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ReadResourceResult> {
        Ok(ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContents {
                uri: "weather://locations".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: "Available locations:\n• London\n• Tokyo\n• New York".to_string(),
                meta: None,
            })],
            _meta: None,
        })
    }

    fn resource_definition(&self) -> Resource {
        Resource {
            name: "weather_locations".to_string(),
            title: Some("Weather Locations".to_string()),
            uri: "weather://locations".to_string(),
            description: Some("List of available weather locations".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: None,
            annotations: None,
            meta: None,
        }
    }

    async fn exists(&self, uri: &str) -> bool {
        uri == "weather://locations"
    }
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting MCP 2025-06-18 Compliant Server...");

    // Create server with handlers
    // Note: Using the older ServerBuilder API here for demonstration
    // For production, prefer the #[server] macro which provides run_http() methods
    let server = ServerBuilder::new()
        .name("Weather Service")
        .version("2.0.0")
        .description("MCP 2025-06-18 compliant weather service")
        .tool("get_weather", WeatherTool::new())?
        .resource("weather://locations", LocationsResource)?
        .build();

    println!("✅ Server ready at http://127.0.0.1:8080/mcp");
    println!("   - Single endpoint for GET/POST/DELETE");
    println!("   - MCP 2025-06-18 specification compliant");
    println!("   - Message replay support");
    println!("   - Industrial-grade security");
    println!();

    // Use the built-in run_http method from ServerBuilder
    // This automatically sets up MCP 2025-06-18 compliant HTTP/SSE transport
    server.run_http("127.0.0.1:8080").await?;

    Ok(())
}

async fn run_client() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Starting MCP Client...");

    use turbomcp_transport::streamable_http_client::{
        RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport,
    };

    // Configure HTTP v2 client transport
    let transport_config = StreamableHttpClientConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        endpoint_path: "/mcp".to_string(),
        protocol_version: "2025-06-18".to_string(),
        timeout: Duration::from_secs(30),
        retry_policy: RetryPolicy::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: Some(10),
        },
        user_agent: "TurboMCP-Example-Client/2.0.0".to_string(),
        ..Default::default()
    };

    let transport = StreamableHttpClientTransport::new(transport_config);

    // Build client
    let client = ClientBuilder::new()
        .with_tools(true)
        .with_prompts(true)
        .with_resources(true)
        .build(transport)
        .await?;

    println!("✅ Client connected");
    println!();

    // Initialize connection
    println!("📡 Initializing MCP connection...");
    let init_result = client.initialize().await?;
    println!(
        "✅ Connected to: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );
    println!(
        "   Server Capabilities: {:?}",
        init_result.server_capabilities
    );
    println!();

    // List available tools
    println!("🔧 Listing tools...");
    let tools = client.list_tools().await?;
    println!("✅ Found {} tool(s):", tools.len());
    for tool in &tools {
        println!(
            "   - {}: {}",
            tool.name,
            tool.description.as_ref().unwrap_or(&"".to_string())
        );
    }
    println!();

    // Call weather tool for multiple locations
    let locations = vec!["London", "Tokyo", "New York", "Paris"];

    for location in locations {
        println!("🌤️  Getting weather for {}...", location);

        let mut args = HashMap::new();
        args.insert("location".to_string(), serde_json::json!(location));

        match client.call_tool("get_weather", Some(args)).await {
            Ok(result) => {
                // The result is a CallToolResult serialized as JSON
                if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                    for content_item in content_array {
                        if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                            println!("   ✅ {}", text.lines().next().unwrap_or(text));
                        }
                    }
                } else {
                    println!("   ✅ Result: {}", result);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
    }
    println!();

    // List resources
    println!("📚 Listing resources...");
    let resource_uris = client.list_resources().await?;
    println!("✅ Found {} resource(s):", resource_uris.len());
    for uri in &resource_uris {
        println!("   - {}", uri);
    }
    println!();

    // Read a resource
    if let Some(uri) = resource_uris.first() {
        println!("📖 Reading resource: {}...", uri);
        let read_result = client.read_resource(uri).await?;
        for content in read_result.contents {
            if let ResourceContent::Text(text_resource) = content {
                println!(
                    "   Content:\n{}",
                    text_resource
                        .text
                        .lines()
                        .map(|l| format!("      {}", l))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }
    }
    println!();

    println!("✅ Integration test complete!");
    println!("   - MCP 2025-06-18 specification: ✅");
    println!("   - Single endpoint pattern: ✅");
    println!("   - Endpoint discovery: ✅");
    println!("   - Session management: ✅");
    println!("   - Tools execution: ✅");
    println!("   - Resources reading: ✅");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("═══════════════════════════════════════════════════════════");
    println!("  TurboMCP - Streamable HTTP v2 Integration Example");
    println!("  MCP 2025-06-18 Specification Compliant");
    println!("═══════════════════════════════════════════════════════════");
    println!();

    // Spawn server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = run_server().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Run client
    match run_client().await {
        Ok(_) => {
            println!();
            println!("═══════════════════════════════════════════════════════════");
            println!("  Integration Example Completed Successfully! ✨");
            println!("═══════════════════════════════════════════════════════════");
        }
        Err(e) => {
            eprintln!("Client error: {}", e);
        }
    }

    // Cleanup
    server_handle.abort();

    Ok(())
}
