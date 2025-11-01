//! Schema export command
//!
//! Exports MCP server capabilities as `OpenAPI`, GraphQL, or Protobuf schemas.

use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::cli::args::BackendArgs;
use crate::cli::output::OutputFormat;
use crate::error::ProxyResult;
use crate::proxy::BackendConnector;
use crate::proxy::BackendTransport;
use crate::proxy::backend::BackendConfig;

/// Schema export command
///
/// Generates schema files (`OpenAPI`, GraphQL, Protobuf) from MCP server capabilities.
#[derive(Debug, Args)]
pub struct SchemaCommand {
    /// Backend configuration
    #[command(flatten)]
    pub backend: BackendArgs,

    /// Schema format
    #[command(subcommand)]
    pub format: SchemaFormat,

    /// Output file path
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Client name for initialization
    #[arg(long, default_value = "turbomcp-proxy")]
    pub client_name: String,

    /// Client version for initialization
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub client_version: String,
}

/// Schema format options
#[derive(Debug, Subcommand)]
pub enum SchemaFormat {
    /// Export as `OpenAPI` 3.1 specification
    #[command(name = "openapi", visible_alias = "oas")]
    OpenApi {
        /// Include request/response examples
        #[arg(long)]
        with_examples: bool,
    },
    /// Export as GraphQL schema definition
    #[command(name = "graphql", visible_alias = "gql")]
    GraphQL,
    /// Export as Protobuf 3 specification
    #[command(name = "protobuf")]
    Protobuf,
}

impl SchemaCommand {
    /// Execute the schema command
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if command execution fails.
    pub async fn execute(self, _format: OutputFormat) -> ProxyResult<()> {
        // Validate backend arguments
        self.backend
            .validate()
            .map_err(crate::error::ProxyError::configuration)?;

        // Create backend config from args
        let backend_config = self.create_backend_config()?;

        // Connect to backend
        tracing::info!("Connecting to backend...");
        let backend = BackendConnector::new(backend_config).await?;
        tracing::info!("Backend connected successfully");

        // Introspect backend
        tracing::info!("Introspecting backend capabilities...");
        let spec = backend.introspect().await?;
        tracing::info!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        // Generate schema based on format
        let schema_content = match &self.format {
            SchemaFormat::OpenApi { with_examples } => {
                Self::generate_openapi(&spec, *with_examples)?
            }
            SchemaFormat::GraphQL => Self::generate_graphql(&spec),
            SchemaFormat::Protobuf => Self::generate_protobuf(&spec),
        };

        // Write output
        self.write_output(&schema_content)?;

        Ok(())
    }

    /// Generate `OpenAPI` 3.1 schema
    fn generate_openapi(
        spec: &crate::introspection::ServerSpec,
        _with_examples: bool,
    ) -> ProxyResult<String> {
        use serde_json::json;

        let mut paths = serde_json::Map::new();

        // Add tool paths
        for tool in &spec.tools {
            let tool_path = format!("/tools/{}", tool.name);
            let request_body = json!({
                "description": tool.description.as_deref().unwrap_or(""),
                "content": {
                    "application/json": {
                        "schema": serde_json::to_value(&tool.input_schema).unwrap_or_else(|_| json!({"type": "object"}))
                    }
                }
            });

            let operation = json!({
                "summary": format!("Call tool: {}", tool.name),
                "description": tool.description.as_deref().unwrap_or(""),
                "requestBody": request_body,
                "responses": {
                    "200": {
                        "description": "Successful response",
                        "content": {
                            "application/json": {
                                "schema": {"type": "object"}
                            }
                        }
                    },
                    "400": {
                        "description": "Invalid request"
                    },
                    "500": {
                        "description": "Server error"
                    }
                }
            });

            paths.insert(tool_path, json!({"post": operation}));
        }

        // Add resource paths
        for resource in &spec.resources {
            let resource_path = format!("/resources{}", resource.uri);
            let operation = json!({
                "summary": format!("Read resource: {}", resource.uri),
                "description": resource.description.as_deref().unwrap_or(""),
                "responses": {
                    "200": {
                        "description": "Resource contents",
                        "content": {
                            "application/json": {
                                "schema": {"type": "object"}
                            }
                        }
                    },
                    "404": {
                        "description": "Resource not found"
                    }
                }
            });

            paths.insert(resource_path, json!({"get": operation}));
        }

        let openapi = json!({
            "openapi": "3.1.0",
            "info": {
                "title": format!("{} API", spec.server_info.name),
                "description": format!("MCP Server - {}", spec.server_info.title.as_deref().unwrap_or("Model Context Protocol Server")),
                "version": spec.server_info.version
            },
            "paths": paths,
            "servers": [{
                "url": "/",
                "description": "MCP Server"
            }]
        });

        Ok(serde_json::to_string_pretty(&openapi)?)
    }

    /// Generate GraphQL schema
    fn generate_graphql(spec: &crate::introspection::ServerSpec) -> String {
        use std::fmt::Write;

        let mut schema = String::from("# GraphQL Schema generated from MCP server\n\n");

        // Write types
        schema.push_str("type Query {\n");
        for tool in &spec.tools {
            let tool_name = tool.name.replace(['-', ' '], "_");
            let desc = tool.description.as_deref().unwrap_or("");
            writeln!(schema, "  \"\"\"{desc}\"\"\"").ok();
            writeln!(schema, "  {tool_name}(input: JSON!): JSON!").ok();
        }

        for resource in &spec.resources {
            let resource_name = resource.uri.replace(['/', '-'], "_");
            let desc = resource.description.as_deref().unwrap_or("");
            writeln!(schema, "  \"\"\"{desc}\"\"\"").ok();
            writeln!(schema, "  resource_{resource_name}: JSON!").ok();
        }

        schema.push_str("}\n\n");

        // Scalar types
        schema.push_str("scalar JSON\n");

        schema
    }

    /// Generate Protobuf schema
    fn generate_protobuf(spec: &crate::introspection::ServerSpec) -> String {
        use std::fmt::Write;

        let mut schema = String::from("syntax = \"proto3\";\n\n");
        schema.push_str("package mcp_server;\n\n");
        schema.push_str("// Generated from MCP server introspection\n\n");

        // Generate message types for tools
        let mut message_counter = 1;

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
                .collect::<String>();

            let desc = tool.description.as_deref().unwrap_or("");
            writeln!(schema, "message {tool_name} {{").ok();
            writeln!(schema, "  // {desc}").ok();
            schema.push_str("  string input = 1;\n");
            schema.push_str("  string output = 2;\n");
            schema.push_str("}\n\n");

            message_counter += 1;
        }

        // Generate resource messages
        for resource in &spec.resources {
            let resource_name = format!("Resource{message_counter}");
            let desc = resource.description.as_deref().unwrap_or("");
            let uri = &resource.uri;
            writeln!(schema, "message {resource_name} {{").ok();
            writeln!(schema, "  // {desc}").ok();
            writeln!(schema, "  string uri = 1; // {uri}").ok();
            schema.push_str("  string contents = 2;\n");
            schema.push_str("}\n\n");

            message_counter += 1;
        }

        schema
    }

    /// Create backend configuration from args
    fn create_backend_config(&self) -> ProxyResult<BackendConfig> {
        use crate::cli::args::BackendType;

        let transport = match self.backend.backend_type() {
            Some(BackendType::Stdio) => {
                let cmd = self.backend.cmd.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("Command not specified".to_string())
                })?;

                BackendTransport::Stdio {
                    command: cmd.clone(),
                    args: self.backend.args.clone(),
                    working_dir: self
                        .backend
                        .working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                }
            }
            Some(BackendType::Http) => {
                let url = self.backend.http.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("HTTP URL not specified".to_string())
                })?;

                BackendTransport::Http {
                    url: url.clone(),
                    auth_token: None,
                }
            }
            Some(BackendType::Tcp) => {
                let addr = self.backend.tcp.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("TCP address not specified".to_string())
                })?;

                let parts: Vec<&str> = addr.split(':').collect();
                if parts.len() != 2 {
                    return Err(crate::error::ProxyError::configuration(
                        "Invalid TCP address format. Use host:port".to_string(),
                    ));
                }

                let host = parts[0].to_string();
                let port = parts[1].parse::<u16>().map_err(|_| {
                    crate::error::ProxyError::configuration("Invalid port number".to_string())
                })?;

                BackendTransport::Tcp { host, port }
            }
            Some(BackendType::Unix) => {
                let path = self.backend.unix.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration(
                        "Unix socket path not specified".to_string(),
                    )
                })?;

                BackendTransport::Unix { path: path.clone() }
            }
            Some(BackendType::Websocket) => {
                let url = self.backend.websocket.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration(
                        "WebSocket URL not specified".to_string(),
                    )
                })?;

                BackendTransport::WebSocket { url: url.clone() }
            }
            None => {
                return Err(crate::error::ProxyError::configuration(
                    "No backend specified".to_string(),
                ));
            }
        };

        Ok(BackendConfig {
            transport,
            client_name: self.client_name.clone(),
            client_version: self.client_version.clone(),
        })
    }

    /// Write schema to output file or stdout
    fn write_output(&self, content: &str) -> ProxyResult<()> {
        use std::fs;
        use std::io::Write;

        if let Some(path) = &self.output {
            let mut file = fs::File::create(path)?;
            file.write_all(content.as_bytes())?;
            tracing::info!("Schema written to {}", path.display());
        } else {
            println!("{content}");
        }

        Ok(())
    }
}
