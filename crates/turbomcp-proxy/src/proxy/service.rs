//! `ProxyService` - MCP handler that forwards requests to backend servers.

use std::sync::Arc;

use serde_json::Value;
use tracing::{debug, error, trace};
use turbomcp_protocol::{Error as McpError, Result as McpResult, jsonrpc::JsonRpcRequest};

use super::BackendConnector;
use crate::error::ProxyError;
use crate::introspection::ServerSpec;

/// Convert a `ProxyError` into an `McpError`, preserving the upstream JSON-RPC
/// error code (e.g. `-32601`, `-32602`, user-rejected `-1`) when the failure
/// originated as a wire-level error from the backend MCP server. Pre-3.2.0 the
/// proxy stringified everything to `-32603 Internal error`, breaking frontend
/// retry/decision logic that keys off codes.
fn proxy_error_to_mcp(err: ProxyError) -> McpError {
    err.into()
}

/// Proxy service that forwards MCP requests to a backend server
///
/// This service implements `turbomcp_server::McpHandler` for the supported
/// server transports. All requests are forwarded to the backend server via
/// `turbomcp-client`.
///
/// # Performance Note
///
/// The backend connector is wrapped in `Arc` without an additional lock
/// because `BackendConnector` internally uses Arc-wrapped fields and only
/// requires `&self` access. This eliminates read-lock contention on the hot path.
#[derive(Clone)]
pub struct ProxyService {
    /// Backend connector (Arc for cheap cloning, no lock needed - all access is &self)
    backend: Arc<BackendConnector>,

    /// Cached server spec from introspection
    spec: Arc<ServerSpec>,
}

impl ProxyService {
    /// Create a new proxy service
    ///
    /// # Arguments
    ///
    /// * `backend` - The backend connector (must be introspected)
    /// * `spec` - The server spec from introspection
    #[must_use]
    pub fn new(backend: BackendConnector, spec: ServerSpec) -> Self {
        Self {
            backend: Arc::new(backend),
            spec: Arc::new(spec),
        }
    }

    /// Process a JSON-RPC request by forwarding to backend
    async fn process_jsonrpc(&self, request: JsonRpcRequest) -> McpResult<Value> {
        trace!(
            "Processing JSON-RPC: method={}, id={:?}",
            request.method, request.id
        );

        // Route based on method
        match request.method.as_str() {
            // Tools
            "tools/list" => {
                debug!("Forwarding tools/list to backend");
                let tools = self
                    .backend
                    .list_tools()
                    .await
                    .map_err(proxy_error_to_mcp)?;

                Ok(serde_json::json!({
                    "tools": tools
                }))
            }

            "tools/call" => {
                debug!("Forwarding tools/call to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for tools/call".to_string())
                })?;

                let call_request: turbomcp_protocol::types::CallToolRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let result = self
                    .backend
                    .call_tool(&call_request.name, call_request.arguments)
                    .await
                    .map_err(proxy_error_to_mcp)?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Resources
            "resources/list" => {
                debug!("Forwarding resources/list to backend");
                let resources = self
                    .backend
                    .list_resources()
                    .await
                    .map_err(proxy_error_to_mcp)?;

                Ok(serde_json::json!({
                    "resources": resources
                }))
            }

            "resources/templates/list" => self.forward_resource_templates_list().await,

            "resources/read" => {
                debug!("Forwarding resources/read to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for resources/read".to_string())
                })?;

                let read_request: turbomcp_protocol::types::ReadResourceRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let contents = self
                    .backend
                    .read_resource(&read_request.uri)
                    .await
                    .map_err(proxy_error_to_mcp)?;

                // `read_resource` already returns a `ReadResourceResult`, i.e.
                // a `{ "contents": [...] }` object. Wrapping it again made
                // `contents` an OBJECT, but the spec types it as an ARRAY, so
                // every read through the proxy failed to deserialize in any
                // typed client.
                serde_json::to_value(contents).map_err(|e| {
                    McpError::internal(format!("Failed to serialize resource contents: {e}"))
                })
            }

            // Prompts
            "prompts/list" => {
                debug!("Forwarding prompts/list to backend");
                let prompts = self
                    .backend
                    .list_prompts()
                    .await
                    .map_err(proxy_error_to_mcp)?;

                Ok(serde_json::json!({
                    "prompts": prompts
                }))
            }

            "prompts/get" => {
                debug!("Forwarding prompts/get to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for prompts/get".to_string())
                })?;

                let get_request: turbomcp_protocol::types::GetPromptRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                // Arguments are already HashMap<String, Value>
                let arguments = get_request.arguments;

                let result = self
                    .backend
                    .get_prompt(&get_request.name, arguments)
                    .await
                    .map_err(proxy_error_to_mcp)?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Unknown method
            method => {
                error!("Unknown method: {}", method);
                Err(McpError::internal(format!("Method not found: {method}")))
            }
        }
    }

    async fn forward_resource_templates_list(&self) -> McpResult<Value> {
        debug!("Forwarding resources/templates/list to backend");
        let resource_templates = self
            .backend
            .list_resource_templates()
            .await
            .map_err(proxy_error_to_mcp)?;

        Ok(serde_json::json!({
            "resourceTemplates": resource_templates
        }))
    }

    pub(crate) async fn process_value(&self, request: Value) -> McpResult<Value> {
        let json_rpc_request: JsonRpcRequest =
            serde_json::from_value(request).map_err(|e| McpError::serialization(e.to_string()))?;

        self.process_jsonrpc(json_rpc_request).await
    }

    #[cfg(test)]
    pub(crate) fn capabilities_value(&self) -> Value {
        // Return backend capabilities from introspection
        serde_json::json!({
            "protocolVersion": self.spec.protocol_version,
            "serverInfo": {
                "name": format!("{}-proxy", self.spec.server_info.name),
                "version": self.spec.server_info.version,
            },
            "capabilities": self.spec.capabilities,
        })
    }
}

#[cfg(feature = "runtime")]
fn spec_tool_to_mcp_tool(spec: &crate::introspection::ToolSpec) -> turbomcp_protocol::types::Tool {
    use serde_json::{Map, Value};
    use turbomcp_protocol::types::{Tool, ToolAnnotations, ToolInputSchema, ToolOutputSchema};

    let mut additional = spec.input_schema.additional.clone();
    let additional_properties = additional.remove("additionalProperties");
    let properties = spec.input_schema.properties.as_ref().map(|properties| {
        Value::Object(
            properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>(),
        )
    });

    Tool {
        name: spec.name.clone(),
        description: spec.description.clone(),
        input_schema: ToolInputSchema {
            schema_type: Some(Value::String(spec.input_schema.schema_type.clone())),
            properties,
            required: spec.input_schema.required.clone(),
            additional_properties,
            extra_keywords: additional,
        },
        title: spec.title.clone(),
        annotations: spec
            .annotations
            .as_ref()
            .map(|annotations| ToolAnnotations {
                title: annotations.title.clone(),
                read_only_hint: annotations.read_only_hint,
                destructive_hint: annotations.destructive_hint,
                idempotent_hint: annotations.idempotent_hint,
                open_world_hint: annotations.open_world_hint,
            }),
        output_schema: spec.output_schema.as_ref().map(|schema| {
            let mut additional = schema.additional.clone();
            let additional_properties = additional.remove("additionalProperties");
            let properties = schema.properties.as_ref().map(|properties| {
                Value::Object(
                    properties
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect::<Map<_, _>>(),
                )
            });
            ToolOutputSchema {
                schema_type: Some(Value::String(schema.schema_type.clone())),
                properties,
                required: schema.required.clone(),
                additional_properties,
                extra_keywords: additional,
            }
        }),
        ..Default::default()
    }
}

#[cfg(feature = "runtime")]
fn spec_resource_to_mcp_resource(
    spec: &crate::introspection::ResourceSpec,
) -> turbomcp_protocol::types::Resource {
    turbomcp_protocol::types::Resource {
        uri: spec.uri.clone(),
        name: spec.name.clone(),
        description: spec.description.clone(),
        title: spec.title.clone(),
        mime_type: spec.mime_type.clone(),
        size: spec.size,
        annotations: spec
            .annotations
            .as_ref()
            .and_then(resource_annotations_from_spec),
        ..Default::default()
    }
}

#[cfg(feature = "runtime")]
fn spec_resource_template_to_mcp_resource_template(
    spec: &crate::introspection::ResourceTemplateSpec,
) -> turbomcp_protocol::types::ResourceTemplate {
    turbomcp_protocol::types::ResourceTemplate {
        uri_template: spec.uri_template.clone(),
        name: spec.name.clone(),
        description: spec.description.clone(),
        title: spec.title.clone(),
        mime_type: spec.mime_type.clone(),
        annotations: spec
            .annotations
            .as_ref()
            .and_then(resource_annotations_from_spec),
        ..Default::default()
    }
}

#[cfg(feature = "runtime")]
fn resource_annotations_from_spec(
    annotations: &crate::introspection::Annotations,
) -> Option<turbomcp_protocol::types::ResourceAnnotations> {
    serde_json::from_value(serde_json::to_value(annotations).ok()?).ok()
}

#[cfg(feature = "runtime")]
fn spec_prompt_to_mcp_prompt(
    spec: &crate::introspection::PromptSpec,
) -> turbomcp_protocol::types::Prompt {
    use turbomcp_protocol::types::{Prompt, PromptArgument};

    Prompt {
        name: spec.name.clone(),
        description: spec.description.clone(),
        title: spec.title.clone(),
        arguments: Some(
            spec.arguments
                .iter()
                .map(|arg| PromptArgument {
                    name: arg.name.clone(),
                    title: arg.title.clone(),
                    description: arg.description.clone(),
                    required: arg.required,
                })
                .collect(),
        ),
        ..Default::default()
    }
}

#[cfg(feature = "runtime")]
fn server_capabilities_from_spec(
    spec: &crate::introspection::ServerCapabilities,
) -> turbomcp_protocol::types::ServerCapabilities {
    use turbomcp_protocol::types::{
        PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
    };

    ServerCapabilities {
        tools: spec.tools.as_ref().map(|tools| ToolsCapabilities {
            list_changed: tools.list_changed,
        }),
        resources: spec
            .resources
            .as_ref()
            .map(|resources| ResourcesCapabilities {
                // `subscribe` is deliberately dropped rather than mirrored: the
                // proxy forwards neither `resources/subscribe` nor the
                // backend's `notifications/resources/updated`, so claiming it
                // would promise updates that never arrive.
                subscribe: None,
                list_changed: resources.list_changed,
            }),
        prompts: spec.prompts.as_ref().map(|prompts| PromptsCapabilities {
            list_changed: prompts.list_changed,
        }),
        // A capability the proxy advertises is one a client will use against
        // the *proxy*, not the backend. Mirroring the backend's declaration
        // only works for capabilities the proxy actually relays; it forwards
        // neither `logging/setLevel` nor `completion/complete`, so both would
        // answer "capability not supported" to a client that took the
        // advertisement at face value.
        logging: None,
        completions: None,
        experimental: spec.experimental.clone(),
        ..Default::default()
    }
}

#[cfg(feature = "runtime")]
fn tool_arguments_from_value(
    args: Value,
) -> McpResult<Option<std::collections::HashMap<String, Value>>> {
    match args {
        Value::Null => Ok(None),
        Value::Object(map) => Ok(Some(map.into_iter().collect())),
        _ => Err(McpError::invalid_params(
            "tools/call arguments must be an object".to_string(),
        )),
    }
}

#[cfg(feature = "runtime")]
impl turbomcp_server::McpHandler for ProxyService {
    fn server_info(&self) -> turbomcp_server::prelude::ServerInfo {
        turbomcp_server::prelude::ServerInfo {
            name: format!("{}-proxy", self.spec.server_info.name),
            version: self.spec.server_info.version.clone(),
            title: self
                .spec
                .server_info
                .title
                .as_ref()
                .map(|title| format!("{title} Proxy")),
            ..Default::default()
        }
    }

    /// Relay the upstream's usage guidance.
    ///
    /// `instructions` is the one handshake field written for the model rather
    /// than the client, so a proxy that swallows it makes the server it fronts
    /// measurably worse to work with than the same server reached directly.
    fn instructions(&self) -> Option<String> {
        self.spec.instructions.clone()
    }

    fn server_capabilities(&self) -> turbomcp_protocol::types::ServerCapabilities {
        server_capabilities_from_spec(&self.spec.capabilities)
    }

    fn list_tools(&self) -> Vec<turbomcp_protocol::types::Tool> {
        self.spec.tools.iter().map(spec_tool_to_mcp_tool).collect()
    }

    fn list_resources(&self) -> Vec<turbomcp_protocol::types::Resource> {
        self.spec
            .resources
            .iter()
            .map(spec_resource_to_mcp_resource)
            .collect()
    }

    fn list_resource_templates(&self) -> Vec<turbomcp_protocol::types::ResourceTemplate> {
        self.spec
            .resource_templates
            .iter()
            .map(spec_resource_template_to_mcp_resource_template)
            .collect()
    }

    fn list_prompts(&self) -> Vec<turbomcp_protocol::types::Prompt> {
        self.spec
            .prompts
            .iter()
            .map(spec_prompt_to_mcp_prompt)
            .collect()
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Value,
        _ctx: &turbomcp_server::RequestContext,
    ) -> McpResult<turbomcp_server::prelude::ToolResult> {
        let result = self
            .backend
            .call_tool(name, tool_arguments_from_value(args)?)
            .await
            .map_err(proxy_error_to_mcp)?;

        serde_json::from_value(result).map_err(|e| McpError::internal(e.to_string()))
    }

    async fn read_resource(
        &self,
        uri: &str,
        _ctx: &turbomcp_server::RequestContext,
    ) -> McpResult<turbomcp_server::prelude::ResourceResult> {
        let result = self
            .backend
            .read_resource(uri)
            .await
            .map_err(proxy_error_to_mcp)?;

        serde_json::to_value(result)
            .and_then(serde_json::from_value)
            .map_err(|e| McpError::internal(e.to_string()))
    }

    async fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
        _ctx: &turbomcp_server::RequestContext,
    ) -> McpResult<turbomcp_server::prelude::PromptResult> {
        let arguments = match args {
            Some(Value::Object(map)) => Some(map.into_iter().collect()),
            Some(Value::Null) | None => None,
            Some(_) => {
                return Err(McpError::invalid_params(
                    "prompts/get arguments must be an object".to_string(),
                ));
            }
        };
        let result = self
            .backend
            .get_prompt(name, arguments)
            .await
            .map_err(proxy_error_to_mcp)?;

        serde_json::to_value(result)
            .and_then(serde_json::from_value)
            .map_err(|e| McpError::internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{BackendConfig, BackendTransport};

    async fn create_test_service() -> Option<ProxyService> {
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

        let Ok(backend) = BackendConnector::new(config).await else {
            return None;
        };

        let Ok(spec) = backend.introspect().await else {
            return None;
        };

        Some(ProxyService::new(backend, spec))
    }

    #[tokio::test]
    async fn test_resource_templates_list_is_forwarded() {
        let template = turbomcp_protocol::types::ResourceTemplate {
            uri_template: "repo://{owner}/{name}".to_string(),
            name: "repo".to_string(),
            title: Some("Repository".to_string()),
            description: Some("Repository metadata".to_string()),
            mime_type: Some("application/json".to_string()),
            ..Default::default()
        };
        let backend = BackendConnector::from_static_data_for_test(
            Vec::new(),
            Vec::new(),
            vec![template],
            Vec::new(),
        );
        let spec = ServerSpec {
            server_info: crate::introspection::ServerInfo {
                name: "backend".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            protocol_version: turbomcp_protocol::PROTOCOL_VERSION.to_string(),
            capabilities: crate::introspection::ServerCapabilities {
                resources: Some(crate::introspection::ResourcesCapability::default()),
                ..Default::default()
            },
            tools: Vec::new(),
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
            instructions: None,
        };
        let service = ProxyService::new(backend, spec);

        let result = service
            .process_value(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "resources/templates/list"
            }))
            .await
            .expect("resources/templates/list result");

        let templates = result["resourceTemplates"].as_array().expect("templates");
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0]["uriTemplate"], "repo://{owner}/{name}");
        assert_eq!(templates[0]["title"], "Repository");
        assert_eq!(templates[0]["mimeType"], "application/json");
    }

    #[tokio::test]
    #[ignore = "Requires building stdio_server example via cargo run, which can take 60+ seconds"]
    async fn test_service_creation() {
        if let Some(service) = create_test_service().await {
            // Verify capabilities
            let caps = service.capabilities_value();
            assert!(caps.get("capabilities").is_some());
        }
    }

    #[tokio::test]
    #[ignore = "Requires building stdio_server example via cargo run, which can take 60+ seconds"]
    async fn test_tools_list() {
        if let Some(service) = create_test_service().await {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            });

            let result = service.process_value(request).await;
            assert!(result.is_ok());
        }
    }
}
