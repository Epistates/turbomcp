//! # OpenAPI Provider Example
//!
//! Demonstrates converting an OpenAPI specification into MCP tools and resources.
//!
//! This example uses a subset of the Petstore API to show:
//! - Loading OpenAPI specs from strings (can also load from files or URLs)
//! - Automatic conversion of GET endpoints to MCP resources
//! - Automatic conversion of POST/PUT/DELETE to MCP tools
//! - Built-in SSRF protection and request timeouts
//!
//! Run with: `cargo run --example petstore`

use std::time::Duration;

use turbomcp_core::handler::McpHandler;
use turbomcp_openapi::{McpType, OpenApiProvider, RouteMapping};

/// A simplified Petstore OpenAPI spec for demonstration.
const PETSTORE_SPEC: &str = r#"{
    "openapi": "3.0.0",
    "info": {
        "title": "Petstore API",
        "version": "1.0.0",
        "description": "A sample Petstore API for demonstrating OpenAPI to MCP conversion"
    },
    "paths": {
        "/pets": {
            "get": {
                "operationId": "listPets",
                "summary": "List all pets",
                "description": "Returns all pets from the system",
                "parameters": [
                    {
                        "name": "limit",
                        "in": "query",
                        "description": "Maximum number of pets to return",
                        "required": false,
                        "schema": { "type": "integer", "maximum": 100 }
                    }
                ],
                "responses": { "200": { "description": "A list of pets" } }
            },
            "post": {
                "operationId": "createPet",
                "summary": "Create a new pet",
                "description": "Adds a new pet to the store",
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["name"],
                                "properties": {
                                    "name": { "type": "string", "description": "Pet name" },
                                    "tag": { "type": "string", "description": "Optional tag" }
                                }
                            }
                        }
                    }
                },
                "responses": { "201": { "description": "Pet created" } }
            }
        },
        "/pets/{petId}": {
            "get": {
                "operationId": "getPet",
                "summary": "Get a specific pet",
                "parameters": [
                    {
                        "name": "petId",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    }
                ],
                "responses": { "200": { "description": "The pet" } }
            },
            "delete": {
                "operationId": "deletePet",
                "summary": "Delete a pet",
                "description": "Removes a pet from the store",
                "parameters": [
                    {
                        "name": "petId",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    }
                ],
                "responses": { "204": { "description": "Pet deleted" } }
            }
        }
    }
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== OpenAPI to MCP Provider Demo ===\n");

    // Create provider from the OpenAPI spec
    let provider = OpenApiProvider::from_string(PETSTORE_SPEC)?
        // Configure the base URL for API calls (required for actual requests)
        // For demo, we're using a placeholder - use your actual API URL
        .with_base_url("https://petstore.example.com/api/v1")?
        // Configure request timeout (default is 30 seconds)
        .with_timeout(Duration::from_secs(10));

    // Print basic info
    println!("API: {} v{}", provider.title(), provider.version());
    println!("Request timeout: {:?}", provider.timeout());
    println!();

    // Show all operations
    println!("Operations extracted from spec:");
    println!("---------------------------------");
    for op in provider.operations() {
        println!("  {} {} -> {:?}", op.method, op.path, op.mcp_type);
        if let Some(ref summary) = op.summary {
            println!("    Summary: {}", summary);
        }
    }
    println!();

    // Convert to MCP handler
    let handler = provider.into_handler();

    // List tools (POST/PUT/DELETE operations)
    println!("MCP Tools (mutating operations):");
    println!("---------------------------------");
    for tool in handler.list_tools() {
        println!("  Tool: {}", tool.name);
        if let Some(ref desc) = tool.description {
            println!("    Description: {}", desc);
        }
        if let Some(ref meta) = tool.meta {
            if let Some(method) = meta.get("method") {
                println!("    HTTP Method: {}", method);
            }
            if let Some(path) = meta.get("path") {
                println!("    Path: {}", path);
            }
        }
        println!();
    }

    // List resources (GET operations)
    println!("MCP Resources (read operations):");
    println!("---------------------------------");
    for resource in handler.list_resources() {
        println!("  Resource: {} ({})", resource.name, resource.uri);
        if let Some(ref desc) = resource.description {
            println!("    Description: {}", desc);
        }
        println!();
    }

    // Demonstrate custom route mapping
    println!("=== Custom Route Mapping Demo ===\n");

    let custom_mapping = RouteMapping::default_rules()
        // Make all /pets/* endpoints tools (even GET)
        .map_pattern(r"^/pets.*", McpType::Tool)?
        // But skip any /internal/* endpoints
        .map_pattern(r"^/internal.*", McpType::Skip)?;

    let custom_provider =
        OpenApiProvider::from_string(PETSTORE_SPEC)?.with_route_mapping(custom_mapping);

    println!("With custom mapping (all /pets/* as Tools):");
    for op in custom_provider.operations() {
        println!("  {} {} -> {:?}", op.method, op.path, op.mcp_type);
    }
    println!();

    println!("Security Features:");
    println!("------------------");
    println!("  - SSRF Protection: Blocks localhost, private IPs, cloud metadata");
    println!("  - Request Timeout: Configurable (default 30s)");
    println!("  - Input Validation: JSON Schema from OpenAPI spec");
    println!();

    println!("To use as an MCP server, call handler.run_stdio() or mount on HTTP.");

    Ok(())
}
