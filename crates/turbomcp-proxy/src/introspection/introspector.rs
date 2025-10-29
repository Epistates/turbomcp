//! MCP Server Introspector
//!
//! This module provides the core introspection logic for discovering MCP server
//! capabilities by communicating via the MCP protocol.

use tracing::{debug, info, trace};
use turbomcp_protocol::{
    types::{
        prompts::{ListPromptsRequest, ListPromptsResult},
        resources::{ListResourcesRequest, ListResourcesResult},
        tools::{ListToolsRequest, ListToolsResult},
        ClientCapabilities, Cursor, ElicitationCapabilities, Implementation, RootsCapabilities,
        SamplingCapabilities,
    },
    InitializeRequest, InitializeResult,
};

use super::backends::McpBackend;
use super::spec::*;
use crate::error::{ProxyError, ProxyResult};

/// MCP Server Introspector
///
/// Discovers server capabilities by performing MCP protocol handshake
/// and listing all available tools, resources, and prompts.
pub struct McpIntrospector {
    /// Client name to send during initialization
    client_name: String,
    /// Client version
    client_version: String,
}

impl McpIntrospector {
    /// Create a new introspector with default client info
    pub fn new() -> Self {
        Self {
            client_name: "turbomcp-proxy-introspector".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Create an introspector with custom client info
    pub fn with_client_info(
        client_name: impl Into<String>,
        client_version: impl Into<String>,
    ) -> Self {
        Self {
            client_name: client_name.into(),
            client_version: client_version.into(),
        }
    }

    /// Perform full introspection of an MCP server
    ///
    /// This will:
    /// 1. Connect to the server via the backend
    /// 2. Perform initialization handshake
    /// 3. List all tools, resources, and prompts
    /// 4. Build a complete ServerSpec
    pub async fn introspect(&self, backend: &mut dyn McpBackend) -> ProxyResult<ServerSpec> {
        info!(
            client = %self.client_name,
            version = %self.client_version,
            backend = %backend.description(),
            "Starting MCP server introspection"
        );

        // Step 1: Initialize connection
        let init_result = self.initialize(backend).await?;

        debug!(
            server_name = %init_result.server_info.name,
            server_version = %init_result.server_info.version,
            protocol_version = %init_result.protocol_version,
            "Server initialization successful"
        );

        // Step 2: Extract server info and capabilities
        let server_info = ServerInfo {
            name: init_result.server_info.name.clone(),
            version: init_result.server_info.version.clone(),
            title: None, // Not provided in InitializeResult
        };

        let capabilities = self.extract_capabilities(&init_result);

        // Step 3: List tools (if server supports them)
        let tools = if capabilities.tools.is_some() {
            self.list_tools(backend).await?
        } else {
            debug!("Server does not support tools");
            Vec::new()
        };

        // Step 4: List resources (if server supports them)
        let (resources, resource_templates) = if capabilities.resources.is_some() {
            self.list_resources(backend).await?
        } else {
            debug!("Server does not support resources");
            (Vec::new(), Vec::new())
        };

        // Step 5: List prompts (if server supports them)
        let prompts = if capabilities.prompts.is_some() {
            self.list_prompts(backend).await?
        } else {
            debug!("Server does not support prompts");
            Vec::new()
        };

        // Build final ServerSpec
        let spec = ServerSpec {
            server_info,
            protocol_version: init_result.protocol_version.clone(),
            capabilities,
            tools,
            resources,
            resource_templates,
            prompts,
            instructions: init_result.instructions.clone(),
        };

        info!(
            server = %spec.server_info.name,
            tools = spec.tools.len(),
            resources = spec.resources.len(),
            prompts = spec.prompts.len(),
            "Introspection complete"
        );

        Ok(spec)
    }

    /// Initialize connection with the server
    async fn initialize(&self, backend: &mut dyn McpBackend) -> ProxyResult<InitializeResult> {
        let request = InitializeRequest {
            protocol_version: "2025-06-18".to_string(),
            capabilities: ClientCapabilities {
                roots: Some(RootsCapabilities {
                    list_changed: Some(true),
                }),
                sampling: Some(SamplingCapabilities {}),
                elicitation: Some(ElicitationCapabilities {
                    schema_validation: None,
                }),
                experimental: None,
            },
            client_info: Implementation {
                name: self.client_name.clone(),
                title: None,
                version: self.client_version.clone(),
            },
            _meta: None,
        };

        backend.initialize(request).await
    }

    /// Extract capabilities from InitializeResult
    fn extract_capabilities(&self, init_result: &InitializeResult) -> ServerCapabilities {
        let caps = &init_result.capabilities;

        ServerCapabilities {
            logging: caps.logging.as_ref().map(|_| LoggingCapability {}),
            completions: caps.completions.as_ref().map(|_| EmptyCapability {}),
            prompts: caps.prompts.as_ref().map(|p| PromptsCapability {
                list_changed: p.list_changed,
            }),
            resources: caps.resources.as_ref().map(|r| ResourcesCapability {
                subscribe: r.subscribe,
                list_changed: r.list_changed,
            }),
            tools: caps.tools.as_ref().map(|t| ToolsCapability {
                list_changed: t.list_changed,
            }),
            experimental: caps.experimental.clone(),
        }
    }

    /// List all tools from the server (with pagination support)
    async fn list_tools(&self, backend: &mut dyn McpBackend) -> ProxyResult<Vec<ToolSpec>> {
        let mut all_tools = Vec::new();
        let mut cursor: Option<Cursor> = None;

        loop {
            trace!(cursor = ?cursor, "Fetching tools page");

            let request = ListToolsRequest {
                cursor: cursor.clone(),
                _meta: None,
            };

            let params = serde_json::to_value(&request).map_err(|e| {
                ProxyError::backend(format!("Failed to serialize tools/list request: {}", e))
            })?;

            let result_value = backend.call_method("tools/list", params).await?;

            let result: ListToolsResult = serde_json::from_value(result_value).map_err(|e| {
                ProxyError::backend(format!("Failed to parse tools/list response: {}", e))
            })?;

            // Convert protocol tools to spec tools
            for tool in result.tools {
                all_tools.push(ToolSpec {
                    name: tool.name,
                    title: None, // Not in Tool type
                    description: tool.description,
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: tool.input_schema.properties,
                        required: tool.input_schema.required,
                        additional: std::collections::HashMap::new(),
                    },
                    output_schema: tool.output_schema.map(|schema| ToolOutputSchema {
                        schema_type: "object".to_string(),
                        properties: schema.properties,
                        required: schema.required,
                        additional: std::collections::HashMap::new(),
                    }),
                    annotations: tool.annotations.map(|ann| ToolAnnotations {
                        title: ann.title,
                        read_only_hint: ann.read_only_hint,
                        destructive_hint: ann.destructive_hint,
                        idempotent_hint: ann.idempotent_hint,
                        open_world_hint: ann.open_world_hint,
                    }),
                });
            }

            // Check for next page
            if let Some(next_cursor) = result.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        debug!(count = all_tools.len(), "Listed all tools");
        Ok(all_tools)
    }

    /// List all resources from the server (with pagination support)
    async fn list_resources(
        &self,
        backend: &mut dyn McpBackend,
    ) -> ProxyResult<(Vec<ResourceSpec>, Vec<ResourceTemplateSpec>)> {
        let mut all_resources = Vec::new();
        let all_templates = Vec::new();
        let mut cursor: Option<Cursor> = None;

        loop {
            trace!(cursor = ?cursor, "Fetching resources page");

            let request = ListResourcesRequest {
                cursor: cursor.clone(),
                _meta: None,
            };

            let params = serde_json::to_value(&request).map_err(|e| {
                ProxyError::backend(format!("Failed to serialize resources/list request: {}", e))
            })?;

            let result_value = backend.call_method("resources/list", params).await?;

            let result: ListResourcesResult =
                serde_json::from_value(result_value).map_err(|e| {
                    ProxyError::backend(format!("Failed to parse resources/list response: {}", e))
                })?;

            // Convert protocol resources to spec resources
            for resource in result.resources {
                all_resources.push(ResourceSpec {
                    uri: resource.uri,
                    name: resource.name,
                    title: None,
                    description: resource.description,
                    mime_type: resource.mime_type,
                    size: resource.size,
                    annotations: resource.annotations.map(|ann| Annotations {
                        fields: serde_json::from_value(
                            serde_json::to_value(ann).unwrap_or_default(),
                        )
                        .unwrap_or_default(),
                    }),
                });
            }

            // Note: resource_templates are returned inline with resources in MCP spec
            // Not as a separate field in ListResourcesResult

            // Check for next page
            if let Some(next_cursor) = result.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        debug!(
            resources = all_resources.len(),
            templates = all_templates.len(),
            "Listed all resources"
        );

        Ok((all_resources, all_templates))
    }

    /// List all prompts from the server (with pagination support)
    async fn list_prompts(&self, backend: &mut dyn McpBackend) -> ProxyResult<Vec<PromptSpec>> {
        let mut all_prompts = Vec::new();
        let mut cursor: Option<Cursor> = None;

        loop {
            trace!(cursor = ?cursor, "Fetching prompts page");

            let request = ListPromptsRequest {
                cursor: cursor.clone(),
                _meta: None,
            };

            let params = serde_json::to_value(&request).map_err(|e| {
                ProxyError::backend(format!("Failed to serialize prompts/list request: {}", e))
            })?;

            let result_value = backend.call_method("prompts/list", params).await?;

            let result: ListPromptsResult = serde_json::from_value(result_value).map_err(|e| {
                ProxyError::backend(format!("Failed to parse prompts/list response: {}", e))
            })?;

            // Convert protocol prompts to spec prompts
            for prompt in result.prompts {
                all_prompts.push(PromptSpec {
                    name: prompt.name,
                    title: None,
                    description: prompt.description,
                    arguments: prompt
                        .arguments
                        .unwrap_or_default()
                        .into_iter()
                        .map(|arg| PromptArgument {
                            name: arg.name,
                            title: None,
                            description: arg.description,
                            required: arg.required,
                        })
                        .collect(),
                });
            }

            // Check for next page
            if let Some(next_cursor) = result.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        debug!(count = all_prompts.len(), "Listed all prompts");
        Ok(all_prompts)
    }
}

impl Default for McpIntrospector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspector_creation() {
        let introspector = McpIntrospector::new();
        assert_eq!(introspector.client_name, "turbomcp-proxy-introspector");

        let custom = McpIntrospector::with_client_info("my-client", "2.0.0");
        assert_eq!(custom.client_name, "my-client");
        assert_eq!(custom.client_version, "2.0.0");
    }
}
