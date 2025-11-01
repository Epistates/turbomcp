//! Example: Schema Export from MCP Server
//!
//! Demonstrates exporting server capabilities as OpenAPI, GraphQL, and Protobuf schemas.
//!
//! Usage:
//!   1. Start an MCP server (STDIO, TCP, or Unix socket)
//!   2. Run: cargo run --example schema_export -- --backend stdio --cmd "your-mcp-server"
//!   3. Schemas will be printed to stdout
//!
//! For TCP backend:
//!   cargo run --example schema_export -- --backend tcp --tcp localhost:5000
//!
//! For Unix socket:
//!   cargo run --example schema_export -- --backend unix --unix /tmp/mcp.sock

use serde_json::json;
use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 MCP Schema Export Example");
    println!("============================\n");

    // For demo purposes, create a simple STDIO backend config
    // In real usage, this would be configurable
    let backend_config = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "echo".to_string(), // Dummy command for demo
            args: vec!["Hello MCP".to_string()],
            working_dir: None,
        },
        client_name: "schema-export-example".to_string(),
        client_version: "1.0.0".to_string(),
    };

    println!("📡 Connecting to MCP server...");

    // Create backend connector
    match BackendConnector::new(backend_config).await {
        Ok(backend) => {
            println!("✅ Connected successfully\n");

            // Introspect server
            println!("🔍 Introspecting server capabilities...");
            match backend.introspect().await {
                Ok(spec) => {
                    println!("✅ Introspection complete\n");

                    println!("📊 Server Information:");
                    println!("   Name: {}", spec.server_info.name);
                    println!("   Version: {}", spec.server_info.version);
                    println!("   Tools: {}", spec.tools.len());
                    println!("   Resources: {}", spec.resources.len());

                    // Generate OpenAPI schema
                    println!("\n📝 Generated OpenAPI 3.1 Schema:");
                    println!("─────────────────────────────────");
                    let openapi = json!({
                        "openapi": "3.1.0",
                        "info": {
                            "title": format!("{} API", spec.server_info.name),
                            "version": spec.server_info.version
                        },
                        "paths": {
                            "/tools": {
                                "get": {
                                    "summary": "List available tools",
                                    "responses": {
                                        "200": {
                                            "description": "List of tools",
                                            "content": {
                                                "application/json": {
                                                    "schema": {"type": "array"}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    });
                    println!("{}\n", serde_json::to_string_pretty(&openapi)?);

                    // Generate GraphQL schema snippet
                    println!("🎯 Generated GraphQL Schema:");
                    println!("────────────────────────────");
                    let mut graphql = String::from("type Query {\n");
                    for tool in &spec.tools {
                        let tool_name = tool.name.replace('-', "_");
                        graphql.push_str(&format!(
                            "  \"\"\"{}\"\"\"\n",
                            tool.description.as_deref().unwrap_or("")
                        ));
                        graphql.push_str(&format!("  {}(input: JSON!): JSON!\n", tool_name));
                    }
                    graphql.push_str("}\n\nscalar JSON\n");
                    println!("{}\n", graphql);

                    // Generate Protobuf schema snippet
                    println!("🔧 Generated Protobuf Schema:");
                    println!("──────────────────────────────");
                    let mut protobuf =
                        String::from("syntax = \"proto3\";\n\npackage mcp_server;\n\n");
                    for tool in spec.tools.iter() {
                        let tool_name = tool
                            .name
                            .split('-')
                            .map(|s| {
                                let mut chars = s.chars();
                                match chars.next() {
                                    None => String::new(),
                                    Some(first) => {
                                        first.to_uppercase().collect::<String>() + chars.as_str()
                                    }
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");

                        protobuf.push_str(&format!("message {} {{\n", tool_name));
                        protobuf.push_str(&format!(
                            "  // {}\n",
                            tool.description.as_deref().unwrap_or("")
                        ));
                        protobuf.push_str("  string input = 1;\n");
                        protobuf.push_str("  string output = 2;\n");
                        protobuf.push_str("}\n\n");
                    }
                    println!("{}", protobuf);

                    println!("✨ Schema generation complete!");
                    println!("\nYou can export these schemas using turbomcp-proxy CLI:");
                    println!(
                        "  turbomcp-proxy schema openapi --backend stdio --cmd \"your-server\" -o api.json"
                    );
                    println!(
                        "  turbomcp-proxy schema graphql --backend tcp --tcp localhost:5000 -o schema.graphql"
                    );
                    println!(
                        "  turbomcp-proxy schema protobuf --backend unix --unix /tmp/mcp.sock -o server.proto"
                    );
                }
                Err(e) => {
                    eprintln!("❌ Introspection failed: {}", e);
                    return Err(e.into());
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Connection failed: {}", e);
            eprintln!("\nUsage examples:");
            eprintln!("  STDIO:  cargo run --example schema_export");
            eprintln!(
                "  TCP:    cargo run --example schema_export -- --backend tcp --tcp localhost:5000"
            );
            eprintln!(
                "  Unix:   cargo run --example schema_export -- --backend unix --unix /tmp/mcp.sock"
            );
            return Err(e.into());
        }
    }

    Ok(())
}
