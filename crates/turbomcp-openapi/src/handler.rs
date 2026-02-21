//! MCP handler implementation for OpenAPI operations.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolInputSchema, ToolResult,
};

use crate::provider::{ExtractedOperation, OpenApiProvider};
use crate::security::validate_url_for_ssrf;

/// MCP handler that exposes OpenAPI operations as tools and resources.
#[derive(Clone)]
pub struct OpenApiHandler {
    provider: Arc<OpenApiProvider>,
}

impl std::fmt::Debug for OpenApiHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenApiHandler")
            .field("title", &self.provider.title())
            .field("version", &self.provider.version())
            .field("operations", &self.provider.operations().len())
            .finish()
    }
}

impl OpenApiHandler {
    /// Create a new handler from a provider.
    pub fn new(provider: Arc<OpenApiProvider>) -> Self {
        Self { provider }
    }

    /// Get the underlying provider.
    pub fn provider(&self) -> &OpenApiProvider {
        &self.provider
    }

    /// Generate tool name from operation.
    fn tool_name(op: &ExtractedOperation) -> String {
        op.operation_id.clone().unwrap_or_else(|| {
            // Generate name from method and path
            let path_part = op
                .path
                .trim_start_matches('/')
                .replace('/', "_")
                .replace(['{', '}'], "");
            format!("{}_{}", op.method.to_lowercase(), path_part)
        })
    }

    /// Generate resource URI from operation.
    fn resource_uri(op: &ExtractedOperation) -> String {
        format!("openapi://{}{}", op.method.to_lowercase(), op.path)
    }

    /// Build JSON Schema for tool input.
    fn build_input_schema(op: &ExtractedOperation) -> ToolInputSchema {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        // Add parameters
        for param in &op.parameters {
            let mut param_schema = param.schema.clone().unwrap_or(json!({"type": "string"}));

            // Add description if available
            if let Some(desc) = &param.description
                && let Value::Object(ref mut map) = param_schema
            {
                map.insert("description".to_string(), json!(desc));
            }

            properties.insert(param.name.clone(), param_schema);

            if param.required {
                required.push(param.name.clone());
            }
        }

        // Add request body if present
        if let Some(body_schema) = &op.request_body_schema {
            properties.insert("body".to_string(), body_schema.clone());
            required.push("body".to_string());
        }

        ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(Value::Object(properties)),
            required: if required.is_empty() {
                None
            } else {
                Some(required)
            },
            additional_properties: None,
        }
    }

    /// Find operation by tool name.
    fn find_tool_operation(&self, name: &str) -> Option<&ExtractedOperation> {
        self.provider.tools().find(|op| Self::tool_name(op) == name)
    }

    /// Find operation by resource URI.
    fn find_resource_operation(&self, uri: &str) -> Option<&ExtractedOperation> {
        self.provider
            .resources()
            .find(|op| Self::resource_uri(op) == uri)
    }

    /// Execute an operation via HTTP.
    ///
    /// # Security
    ///
    /// This method validates URLs against SSRF attacks before making requests.
    /// Requests to private IP ranges, localhost, and cloud metadata endpoints
    /// are blocked.
    async fn execute_operation(
        &self,
        op: &ExtractedOperation,
        args: HashMap<String, Value>,
    ) -> McpResult<Value> {
        let url = self
            .provider
            .build_url(op, &args)
            .map_err(|e| McpError::internal(e.to_string()))?;

        // SSRF protection: validate URL before making request
        validate_url_for_ssrf(&url).map_err(|e| McpError::internal(e.to_string()))?;

        let client = self.provider.client();

        let mut request = match op.method.as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => {
                return Err(McpError::internal(format!(
                    "Unsupported method: {}",
                    op.method
                )));
            }
        };

        // Add request body if present
        if let Some(body) = args.get("body") {
            request = request.json(body);
        }

        // Add header parameters
        for param in &op.parameters {
            if param.location == "header"
                && let Some(value) = args.get(&param.name)
            {
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    _ => value.to_string(),
                };
                request = request.header(&param.name, value_str);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| McpError::internal(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| McpError::internal(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(McpError::internal(format!(
                "API returned {}: {}",
                status, body
            )));
        }

        // Try to parse as JSON, fallback to string
        match serde_json::from_str(&body) {
            Ok(json) => Ok(json),
            Err(_) => Ok(json!(body)),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl McpHandler for OpenApiHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new(self.provider.title(), self.provider.version())
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.provider
            .tools()
            .map(|op| Tool {
                name: Self::tool_name(op),
                description: op.summary.clone().or_else(|| op.description.clone()),
                input_schema: Self::build_input_schema(op),
                title: op.summary.clone(),
                icons: None,
                annotations: None,
                execution: None,
                output_schema: None,
                meta: Some({
                    let mut meta = HashMap::new();
                    meta.insert("method".to_string(), json!(op.method));
                    meta.insert("path".to_string(), json!(op.path));
                    if let Some(ref id) = op.operation_id {
                        meta.insert("operationId".to_string(), json!(id));
                    }
                    meta
                }),
            })
            .collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.provider
            .resources()
            .map(|op| Resource {
                uri: Self::resource_uri(op),
                name: op.operation_id.clone().unwrap_or_else(|| op.path.clone()),
                description: op.summary.clone().or_else(|| op.description.clone()),
                title: op.summary.clone(),
                icons: None,
                mime_type: Some("application/json".to_string()),
                annotations: None,
                size: None,
                meta: Some({
                    let mut meta = HashMap::new();
                    meta.insert("method".to_string(), json!(op.method));
                    meta.insert("path".to_string(), json!(op.path));
                    meta
                }),
            })
            .collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        // OpenAPI doesn't map to prompts
        Vec::new()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        _ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ToolResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            let op = self
                .find_tool_operation(name)
                .ok_or_else(|| McpError::tool_not_found(name))?;

            let args_map: HashMap<String, Value> = match args {
                Value::Object(map) => map.into_iter().collect(),
                Value::Null => HashMap::new(),
                _ => {
                    return Err(McpError::invalid_params(
                        "Arguments must be an object or null",
                    ));
                }
            };

            let result = self.execute_operation(op, args_map).await?;

            Ok(ToolResult::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            ))
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
    + turbomcp_core::marker::MaybeSend
    + 'a {
        async move {
            let op = self
                .find_resource_operation(uri)
                .ok_or_else(|| McpError::resource_not_found(uri))?;

            // Resources are GET operations with no body
            let result = self.execute_operation(op, HashMap::new()).await?;

            let content =
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

            Ok(ResourceResult::text(uri, content))
        }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        _args: Option<Value>,
        _ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<PromptResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move { Err(McpError::prompt_not_found(name)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::McpType;

    const TEST_SPEC: &str = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0" },
        "paths": {
            "/users": {
                "get": { "operationId": "listUsers", "summary": "List users", "responses": { "200": { "description": "Success" } } },
                "post": { "operationId": "createUser", "summary": "Create user", "responses": { "201": { "description": "Created" } } }
            }
        }
    }"#;

    #[test]
    fn test_list_tools() {
        let provider = OpenApiProvider::from_string(TEST_SPEC).unwrap();
        let handler = provider.into_handler();

        let tools = handler.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "createUser");
    }

    #[test]
    fn test_list_resources() {
        let provider = OpenApiProvider::from_string(TEST_SPEC).unwrap();
        let handler = provider.into_handler();

        let resources = handler.list_resources();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].name, "listUsers");
    }

    #[test]
    fn test_tool_name_generation() {
        let op_with_id = ExtractedOperation {
            method: "POST".to_string(),
            path: "/users".to_string(),
            operation_id: Some("createUser".to_string()),
            summary: None,
            description: None,
            parameters: vec![],
            request_body_schema: None,
            mcp_type: McpType::Tool,
        };

        let op_without_id = ExtractedOperation {
            method: "DELETE".to_string(),
            path: "/users/{id}".to_string(),
            operation_id: None,
            summary: None,
            description: None,
            parameters: vec![],
            request_body_schema: None,
            mcp_type: McpType::Tool,
        };

        assert_eq!(OpenApiHandler::tool_name(&op_with_id), "createUser");
        assert_eq!(OpenApiHandler::tool_name(&op_without_id), "delete_users_id");
    }
}
