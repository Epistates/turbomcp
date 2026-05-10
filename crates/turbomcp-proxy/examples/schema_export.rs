//! Example: Schema Export from MCP Server
//!
//! Demonstrates exporting server capabilities as OpenAPI, GraphQL, and Protobuf schemas.
//!
//! Usage:
//!   - Run with no arguments for a self-contained mock server spec.
//!   - Or point it at a real MCP backend.
//!
//! Mock mode:
//!   cargo run --example schema_export
//!
//! STDIO backend:
//!   cargo run --example schema_export -- --backend stdio --cmd "your-mcp-server"
//!
//! For TCP backend:
//!   cargo run --example schema_export -- --backend tcp --tcp 127.0.0.1:8765
//!
//! For Unix socket:
//!   cargo run --example schema_export -- --backend unix --unix /tmp/turbomcp-demo.sock

use std::collections::HashMap;
use std::env;
use std::error::Error;

use serde_json::json;
use turbomcp_proxy::MCP_PROTOCOL_VERSION;
use turbomcp_proxy::introspection::{
    ResourceSpec, ResourcesCapability, ServerCapabilities, ServerInfo, ServerSpec, ToolInputSchema,
    ToolSpec, ToolsCapability,
};
use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

enum ExampleBackend {
    Mock,
    Real(BackendConfig),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 MCP Schema Export Example");
    println!("============================\n");

    let spec = match parse_backend()? {
        ExampleBackend::Mock => {
            println!("📦 Using built-in mock server specification");
            mock_spec()
        }
        ExampleBackend::Real(config) => {
            println!("📡 Connecting to MCP server...");
            let backend = BackendConnector::new(config).await?;
            println!("✅ Connected successfully\n");

            println!("🔍 Introspecting server capabilities...");
            let spec = backend.introspect().await?;
            println!("✅ Introspection complete\n");
            spec
        }
    };

    print_schemas(&spec)?;

    Ok(())
}

fn parse_backend() -> Result<ExampleBackend, Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|arg| arg == "--mock") {
        return Ok(ExampleBackend::Mock);
    }

    let backend = value_after(&args, "--backend").ok_or_else(|| {
        invalid_input(
            "missing --backend; use --mock, --backend stdio, --backend tcp, or --backend unix",
        )
    })?;

    let transport = match backend.as_str() {
        "stdio" => {
            let cmd = value_after(&args, "--cmd")
                .ok_or_else(|| invalid_input("stdio backend requires --cmd"))?;
            let mut parts = cmd.split_whitespace();
            let command = parts
                .next()
                .ok_or_else(|| invalid_input("--cmd cannot be empty"))?
                .to_string();
            let mut cmd_args: Vec<String> = parts.map(ToString::to_string).collect();
            cmd_args.extend(values_after(&args, "--arg"));

            BackendTransport::Stdio {
                command,
                args: cmd_args,
                working_dir: value_after(&args, "--working-dir"),
            }
        }
        "tcp" => {
            let endpoint = value_after(&args, "--tcp")
                .ok_or_else(|| invalid_input("tcp backend requires --tcp host:port"))?;
            let (host, port) = parse_host_port(&endpoint)?;
            BackendTransport::Tcp { host, port }
        }
        #[cfg(unix)]
        "unix" => {
            let path = value_after(&args, "--unix")
                .ok_or_else(|| invalid_input("unix backend requires --unix /path/to/socket"))?;
            BackendTransport::Unix { path }
        }
        other => {
            return Err(invalid_input(format!(
                "unsupported backend '{other}'; expected stdio, tcp, or unix"
            )));
        }
    };

    Ok(ExampleBackend::Real(BackendConfig {
        transport,
        client_name: "schema-export-example".to_string(),
        client_version: "1.0.0".to_string(),
    }))
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].clone()))
}

fn values_after(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
        .collect()
}

fn parse_host_port(endpoint: &str) -> Result<(String, u16), Box<dyn Error>> {
    let (host, port) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| invalid_input("TCP endpoint must be host:port"))?;
    if host.is_empty() {
        return Err(invalid_input("TCP host cannot be empty"));
    }
    let port = port.parse::<u16>()?;
    Ok((host.to_string(), port))
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

fn mock_spec() -> ServerSpec {
    let echo_properties = HashMap::from([(
        "message".to_string(),
        json!({
            "type": "string",
            "description": "Message to echo back"
        }),
    )]);
    let add_properties = HashMap::from([
        ("a".to_string(), json!({"type": "number"})),
        ("b".to_string(), json!({"type": "number"})),
    ]);

    ServerSpec {
        server_info: ServerInfo {
            name: "demo-mcp-server".to_string(),
            version: "1.0.0".to_string(),
            title: Some("Demo MCP Server".to_string()),
        },
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(false),
            }),
            resources: Some(ResourcesCapability {
                subscribe: Some(false),
                list_changed: Some(false),
            }),
            ..Default::default()
        },
        tools: vec![
            ToolSpec {
                name: "echo".to_string(),
                title: Some("Echo".to_string()),
                description: Some("Echo a message back to the caller".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(echo_properties),
                    required: Some(vec!["message".to_string()]),
                    additional: HashMap::new(),
                },
                output_schema: None,
                annotations: None,
            },
            ToolSpec {
                name: "add".to_string(),
                title: Some("Add".to_string()),
                description: Some("Add two numbers".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(add_properties),
                    required: Some(vec!["a".to_string(), "b".to_string()]),
                    additional: HashMap::new(),
                },
                output_schema: None,
                annotations: None,
            },
        ],
        resources: vec![ResourceSpec {
            uri: "demo://status".to_string(),
            name: "status".to_string(),
            title: Some("Status".to_string()),
            description: Some("Current demo server status".to_string()),
            mime_type: Some("application/json".to_string()),
            size: None,
            annotations: None,
        }],
        resource_templates: vec![],
        prompts: vec![],
        instructions: Some("Mock spec used when no backend is configured.".to_string()),
    }
}

fn print_schemas(spec: &ServerSpec) -> Result<(), Box<dyn Error>> {
    println!("📊 Server Information:");
    println!("   Name: {}", spec.server_info.name);
    println!("   Version: {}", spec.server_info.version);
    println!("   Tools: {}", spec.tools.len());
    println!("   Resources: {}", spec.resources.len());

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

    println!("🔧 Generated Protobuf Schema:");
    println!("──────────────────────────────");
    let mut protobuf = String::from("syntax = \"proto3\";\n\npackage mcp_server;\n\n");
    for tool in &spec.tools {
        let tool_name = tool
            .name
            .split('-')
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
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
    println!("\nCLI equivalents:");
    println!("  turbomcp-proxy schema openapi --backend stdio --cmd \"your-server\" -o api.json");
    println!(
        "  turbomcp-proxy schema graphql --backend tcp --tcp 127.0.0.1:8765 -o schema.graphql"
    );
    println!(
        "  turbomcp-proxy schema protobuf --backend unix --unix /tmp/turbomcp-demo.sock -o server.proto"
    );

    Ok(())
}
