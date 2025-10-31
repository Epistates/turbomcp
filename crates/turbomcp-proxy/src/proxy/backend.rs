//! Backend connector for proxy
//!
//! Manages connection to the backend MCP server using turbomcp-client.
//! Supports multiple backend transport types (STDIO, HTTP, WebSocket).

use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};
use turbomcp_client::Client;
use turbomcp_protocol::types::{Prompt, ReadResourceResult, Resource, Tool};
use turbomcp_transport::{
    ChildProcessConfig, ChildProcessTransport, TcpTransport, Transport, UnixTransport,
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
    streamable_http_client::{StreamableHttpClientConfig, StreamableHttpClientTransport},
};

use crate::error::{ProxyError, ProxyResult};
use crate::introspection::{
    PromptSpec, PromptsCapability, ResourceSpec, ResourcesCapability, ServerCapabilities,
    ServerInfo, ServerSpec, ToolInputSchema, ToolSpec, ToolsCapability,
};

/// Type-erased client wrapper supporting multiple transports
///
/// This enum allows `BackendConnector` to work with different transport types
/// without requiring generic parameters that would complicate the API.
#[derive(Clone)]
enum AnyClient {
    /// STDIO transport (subprocess)
    Stdio(Arc<Client<ChildProcessTransport>>),

    /// HTTP with Server-Sent Events transport
    Http(Arc<Client<StreamableHttpClientTransport>>),

    /// TCP bidirectional transport
    Tcp(Arc<Client<TcpTransport>>),

    /// Unix socket bidirectional transport
    Unix(Arc<Client<UnixTransport>>),

    /// WebSocket bidirectional transport
    WebSocket(Arc<Client<WebSocketBidirectionalTransport>>),
}

/// Macro to dispatch method calls on `AnyClient` enum
macro_rules! dispatch_client {
    ($client:expr, $method:ident($($args:expr),*)) => {
        match $client {
            AnyClient::Stdio(c) => c.$method($($args),*).await,
            AnyClient::Http(c) => c.$method($($args),*).await,
            AnyClient::Tcp(c) => c.$method($($args),*).await,
            AnyClient::Unix(c) => c.$method($($args),*).await,
            AnyClient::WebSocket(c) => c.$method($($args),*).await,
        }
    };
}

/// Backend transport type
#[derive(Debug, Clone)]
pub enum BackendTransport {
    /// Standard I/O (subprocess)
    Stdio {
        /// Command to execute
        command: String,
        /// Command arguments
        args: Vec<String>,
        /// Working directory
        working_dir: Option<String>,
    },
    /// HTTP with Server-Sent Events
    Http {
        /// Base URL
        url: String,
        /// Optional authentication token
        auth_token: Option<String>,
    },
    /// TCP bidirectional communication
    Tcp {
        /// Host or IP address
        host: String,
        /// Port number
        port: u16,
    },
    /// Unix domain socket
    Unix {
        /// Socket file path
        path: String,
    },
    /// WebSocket bidirectional
    WebSocket {
        /// WebSocket URL
        url: String,
    },
}

/// Backend configuration
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Transport configuration
    pub transport: BackendTransport,

    /// Client name for initialization
    pub client_name: String,

    /// Client version for initialization
    pub client_version: String,
}

/// Backend connector wrapping turbomcp-client
///
/// Manages the connection to the backend MCP server and provides
/// type-safe methods for all MCP protocol operations.
#[derive(Clone)]
pub struct BackendConnector {
    /// The underlying turbomcp client (transport-agnostic)
    client: AnyClient,

    /// Backend configuration
    #[allow(dead_code)] // Kept for future use and debugging
    config: BackendConfig,

    /// Cached server spec (from introspection)
    spec: Option<ServerSpec>,
}

impl BackendConnector {
    /// Create a new backend connector
    ///
    /// # Arguments
    ///
    /// * `config` - Backend configuration
    ///
    /// # Returns
    ///
    /// A connected backend connector ready for requests
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the backend fails to initialize, connect, or if the transport type is not supported.
    ///
    /// # Panics
    ///
    /// Panics if "127.0.0.1:0" cannot be parsed as a `SocketAddr` (should never happen as it's a valid address).
    #[allow(clippy::too_many_lines)]
    pub async fn new(config: BackendConfig) -> ProxyResult<Self> {
        info!("Creating backend connector: {:?}", config.transport);

        // Create client based on transport type
        let client = match &config.transport {
            BackendTransport::Stdio {
                command,
                args,
                working_dir,
            } => {
                let process_config = ChildProcessConfig {
                    command: command.clone(),
                    args: args.clone(),
                    working_directory: working_dir.clone(),
                    environment: None,
                    ..Default::default()
                };

                let transport = ChildProcessTransport::new(process_config);

                // Connect the transport
                transport.connect().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to connect to subprocess: {e}"))
                })?;

                debug!("STDIO backend connected: {} {:?}", command, args);

                // Create and initialize client
                let client = Client::new(transport);
                let _init_result = client.initialize().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to initialize backend: {e}"))
                })?;

                AnyClient::Stdio(Arc::new(client))
            }

            BackendTransport::Http { url, auth_token } => {
                let http_config = StreamableHttpClientConfig {
                    base_url: url.clone(),
                    endpoint_path: "/mcp".to_string(),
                    timeout: std::time::Duration::from_secs(30),
                    auth_token: auth_token.clone(),
                    ..Default::default()
                };

                let transport = StreamableHttpClientTransport::new(http_config);

                // Connect the transport
                transport.connect().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to connect to HTTP backend: {e}"))
                })?;

                debug!("HTTP backend connected: {}", url);

                // Create and initialize client
                let client = Client::new(transport);
                let _init_result = client.initialize().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to initialize backend: {e}"))
                })?;

                AnyClient::Http(Arc::new(client))
            }

            BackendTransport::Tcp { host, port } => {
                let addr = format!("{host}:{port}")
                    .parse::<SocketAddr>()
                    .map_err(|e| ProxyError::backend(format!("Invalid TCP address: {e}")))?;

                let transport = TcpTransport::new_client(
                    "127.0.0.1:0"
                        .parse()
                        .unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap()),
                    addr,
                );

                // Connect the transport
                transport.connect().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to connect to TCP backend: {e}"))
                })?;

                debug!("TCP backend connected: {}:{}", host, port);

                // Create and initialize client
                let client = Client::new(transport);
                let _init_result = client.initialize().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to initialize backend: {e}"))
                })?;

                AnyClient::Tcp(Arc::new(client))
            }

            BackendTransport::Unix { path } => {
                let socket_path = PathBuf::from(path);

                let transport = UnixTransport::new_client(socket_path.clone());

                // Connect the transport
                transport.connect().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to connect to Unix socket: {e}"))
                })?;

                debug!("Unix socket backend connected: {}", path);

                // Create and initialize client
                let client = Client::new(transport);
                let _init_result = client.initialize().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to initialize backend: {e}"))
                })?;

                AnyClient::Unix(Arc::new(client))
            }

            BackendTransport::WebSocket { url } => {
                let ws_config = WebSocketBidirectionalConfig {
                    url: Some(url.clone()),
                    ..Default::default()
                };

                let transport = WebSocketBidirectionalTransport::new(ws_config)
                    .await
                    .map_err(|e| {
                        ProxyError::backend(format!("Failed to connect to WebSocket: {e}"))
                    })?;

                debug!("WebSocket backend connected: {}", url);

                // Create and initialize client
                let client = Client::new(transport);
                let _init_result = client.initialize().await.map_err(|e| {
                    ProxyError::backend(format!("Failed to initialize backend: {e}"))
                })?;

                AnyClient::WebSocket(Arc::new(client))
            }
        };

        info!("Backend initialized successfully");

        Ok(Self {
            client,
            config,
            spec: None,
        })
    }

    /// Introspect the backend server
    ///
    /// Discovers all capabilities (tools, resources, prompts) and caches
    /// the result for use by the frontend server.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the introspection fails or the server capabilities cannot be determined.
    pub async fn introspect(&mut self) -> ProxyResult<ServerSpec> {
        debug!("Introspecting backend server");

        // Perform introspection via the client
        let spec = self.introspect_via_client().await?;

        // Cache the spec
        self.spec = Some(spec.clone());

        info!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        Ok(spec)
    }

    /// Introspect via client methods
    async fn introspect_via_client(&self) -> ProxyResult<ServerSpec> {
        // List tools
        let tools = dispatch_client!(&self.client, list_tools())
            .map_err(|e| ProxyError::backend(format!("Failed to list tools: {e}")))?;

        // List resources
        let resources = dispatch_client!(&self.client, list_resources())
            .map_err(|e| ProxyError::backend(format!("Failed to list resources: {e}")))?;

        // List prompts
        let prompts = dispatch_client!(&self.client, list_prompts())
            .map_err(|e| ProxyError::backend(format!("Failed to list prompts: {e}")))?;

        // Build ServerSpec using our introspection types
        let server_info = ServerInfo {
            name: "backend-server".to_string(),
            version: "unknown".to_string(),
            title: None,
        };

        let protocol_version = "2025-06-18".to_string();

        let capabilities = ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: None }),
            resources: Some(ResourcesCapability {
                subscribe: None,
                list_changed: None,
            }),
            prompts: Some(PromptsCapability { list_changed: None }),
            logging: None,
            completions: None,
            experimental: None,
        };

        // Convert tools/resources/prompts to our spec types
        let tool_specs: Vec<ToolSpec> = tools
            .into_iter()
            .map(|t| {
                let mut additional = HashMap::new();
                if let Some(additional_props) = t.input_schema.additional_properties {
                    additional.insert(
                        "additionalProperties".to_string(),
                        Value::Bool(additional_props),
                    );
                }
                ToolSpec {
                    name: t.name,
                    title: None,
                    description: t.description,
                    input_schema: ToolInputSchema {
                        schema_type: t.input_schema.schema_type,
                        properties: t.input_schema.properties,
                        required: t.input_schema.required,
                        additional,
                    },
                    output_schema: None,
                    annotations: None,
                }
            })
            .collect();

        let resource_specs: Vec<ResourceSpec> = resources
            .into_iter()
            .map(|r| ResourceSpec {
                uri: r.uri,
                name: r.name,
                title: None,
                description: r.description,
                mime_type: r.mime_type,
                size: None,
                annotations: None,
            })
            .collect();

        let prompt_specs: Vec<PromptSpec> = prompts
            .into_iter()
            .map(|p| {
                let arguments = p
                    .arguments
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| crate::introspection::PromptArgument {
                        name: a.name,
                        title: None,
                        description: a.description,
                        required: a.required,
                    })
                    .collect();
                PromptSpec {
                    name: p.name,
                    title: None,
                    description: p.description,
                    arguments,
                }
            })
            .collect();

        Ok(ServerSpec {
            server_info,
            protocol_version,
            capabilities,
            tools: tool_specs,
            resources: resource_specs,
            prompts: prompt_specs,
            resource_templates: Vec::new(),
            instructions: None,
        })
    }

    /// Get cached server spec
    #[must_use]
    pub fn spec(&self) -> Option<&ServerSpec> {
        self.spec.as_ref()
    }

    /// Call a tool on the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the tool call fails or the tool is not found.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, Value>>,
    ) -> ProxyResult<Value> {
        debug!("Calling backend tool: {}", name);

        dispatch_client!(&self.client, call_tool(name, arguments))
            .map_err(|e| ProxyError::backend(format!("Tool call failed: {e}")))
    }

    /// List tools from the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if listing tools fails.
    pub async fn list_tools(&self) -> ProxyResult<Vec<Tool>> {
        dispatch_client!(&self.client, list_tools())
            .map_err(|e| ProxyError::backend(format!("Failed to list tools: {e}")))
    }

    /// List resources from the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if listing resources fails.
    pub async fn list_resources(&self) -> ProxyResult<Vec<Resource>> {
        dispatch_client!(&self.client, list_resources())
            .map_err(|e| ProxyError::backend(format!("Failed to list resources: {e}")))
    }

    /// Read a resource from the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if reading the resource fails or the resource is not found.
    pub async fn read_resource(&self, uri: &str) -> ProxyResult<ReadResourceResult> {
        dispatch_client!(&self.client, read_resource(uri))
            .map_err(|e| ProxyError::backend(format!("Failed to read resource: {e}")))
    }

    /// List prompts from the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if listing prompts fails.
    pub async fn list_prompts(&self) -> ProxyResult<Vec<Prompt>> {
        dispatch_client!(&self.client, list_prompts())
            .map_err(|e| ProxyError::backend(format!("Failed to list prompts: {e}")))
    }

    /// Get a prompt from the backend
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if getting the prompt fails or the prompt is not found.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, Value>>,
    ) -> ProxyResult<turbomcp_protocol::types::GetPromptResult> {
        dispatch_client!(&self.client, get_prompt(name, arguments))
            .map_err(|e| ProxyError::backend(format!("Failed to get prompt: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_creation() {
        let config = BackendConfig {
            transport: BackendTransport::Stdio {
                command: "python".to_string(),
                args: vec!["server.py".to_string()],
                working_dir: None,
            },
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
        };

        assert_eq!(config.client_name, "test-proxy");
        assert_eq!(config.client_version, "1.0.0");
    }

    #[tokio::test]
    async fn test_backend_connector_with_echo() {
        // This test requires the stdio_server example to be built
        let config = BackendConfig {
            transport: BackendTransport::Stdio {
                command: "cargo".to_string(),
                args: vec![
                    "run".to_string(),
                    "--package".to_string(),
                    "turbomcp".to_string(),
                    "--example".to_string(),
                    "stdio_server".to_string(),
                ],
                working_dir: Some("/Users/nickpaterno/work/turbomcp".to_string()),
            },
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let result = BackendConnector::new(config).await;
        if let Ok(mut backend) = result {
            // Try introspection
            let spec = backend.introspect().await;
            if let Ok(spec) = spec {
                assert!(!spec.tools.is_empty(), "Should have at least one tool");
            }
        }
    }
}
