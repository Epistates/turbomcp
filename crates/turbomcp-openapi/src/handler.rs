//! MCP handler implementation for OpenAPI operations.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolInputSchema,
    ToolOutputSchema, ToolResult,
};

use crate::provider::{ExtractedOperation, OpenApiProvider, param_value};
use crate::schema::hoist_defs;

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
        let mut defs = serde_json::Map::new();

        // Add parameters
        for param in &op.parameters {
            let mut param_schema = param.schema.clone().unwrap_or(json!({"type": "string"}));
            hoist_defs(&mut param_schema, &mut defs);

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
            let mut body_schema = body_schema.clone();
            hoist_defs(&mut body_schema, &mut defs);
            properties.insert("body".to_string(), body_schema);
            required.push("body".to_string());
        }

        // Carry the SEP-1613 default dialect by deferring to `Default::default`
        // for `extra_keywords`, which now contains `$schema = 2020-12`.
        let mut schema = ToolInputSchema {
            schema_type: Some("object".into()),
            properties: Some(Value::Object(properties)),
            required: if required.is_empty() {
                None
            } else {
                Some(required)
            },
            additional_properties: None,
            ..ToolInputSchema::default()
        };
        // Recursive definitions from any property live at the root, which is
        // what their `#/$defs/…` pointers resolve against.
        if !defs.is_empty() {
            schema
                .extra_keywords
                .insert("$defs".to_string(), Value::Object(defs));
        }
        schema
    }

    /// Find operation by tool name.
    fn find_tool_operation(&self, name: &str) -> Option<&ExtractedOperation> {
        self.provider.tools().find(|op| Self::tool_name(op) == name)
    }

    /// Build the `meta` map shared by both tool and resource exposure paths.
    /// Surfaces the operation's effective `security` requirements so MCP
    /// clients can detect that auth is needed even when no `auth_provider` is
    /// installed yet.
    fn build_operation_meta(&self, op: &ExtractedOperation) -> HashMap<String, Value> {
        let mut meta = HashMap::new();
        meta.insert("method".to_string(), json!(op.method));
        meta.insert("path".to_string(), json!(op.path));
        if let Some(ref id) = op.operation_id {
            meta.insert("operationId".to_string(), json!(id));
        }
        if !op.security.is_empty() {
            meta.insert("security".to_string(), json!(&op.security));
            // Surface the matching scheme definitions so a downstream client
            // can render auth requirements without re-fetching the spec.
            let referenced: HashMap<&String, &openapiv3::SecurityScheme> = op
                .security
                .iter()
                .flat_map(|req| req.keys())
                .filter_map(|name| {
                    self.provider
                        .security_schemes()
                        .get(name)
                        .map(|scheme| (name, scheme))
                })
                .collect();
            if !referenced.is_empty()
                && let Ok(value) = serde_json::to_value(&referenced)
            {
                meta.insert("securitySchemes".to_string(), value);
            }
        }
        meta
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
        self.provider
            .ssrf()
            .check_request_target(&url)
            .await
            .map_err(|e| McpError::internal(e.to_string()))?;

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
                request = request.header(&param.name, param_value(value).as_ref());
            }
        }

        // Inject auth credentials before sending. If the operation has security
        // requirements but no provider is installed, the request still goes
        // out — the upstream will return 401 and surface the misconfiguration.
        if !op.security.is_empty()
            && let Some(auth) = self.provider.auth_provider()
        {
            request = auth.apply(request, &op.security, self.provider.security_schemes());
        }

        let response = request
            .send()
            .await
            .map_err(|e| McpError::internal(format!("HTTP request failed: {}", error_chain(&e))))?;

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

/// Render an error with its sources.
///
/// reqwest's own `Display` stops at "error sending request for url (…)"; the
/// reason, such as the SSRF guard refusing a redirect or a resolved address,
/// is further down the chain.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        rendered.push_str(": ");
        rendered.push_str(&cause.to_string());
        source = cause.source();
    }
    rendered
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
                // MCP 2025-11-25 outputSchema: pulled from the operation's
                // first 2xx `application/json` response with `$ref`s
                // inlined. `None` for operations with no JSON response.
                output_schema: op
                    .response_schema
                    .as_ref()
                    .map(|v| ToolOutputSchema::from_value(v.clone())),
                meta: Some(self.build_operation_meta(op)),
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
                meta: Some(self.build_operation_meta(op)),
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
            security: Vec::new(),
            response_schema: None,
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
            security: Vec::new(),
            response_schema: None,
        };

        assert_eq!(OpenApiHandler::tool_name(&op_with_id), "createUser");
        assert_eq!(OpenApiHandler::tool_name(&op_without_id), "delete_users_id");
    }

    #[test]
    fn test_input_schema_is_2020_12_with_root_defs() {
        const SPEC: &str = r##"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0" },
            "paths": {
                "/trees": {
                    "post": {
                        "operationId": "plantTree",
                        "parameters": [{
                            "name": "note", "in": "query",
                            "schema": { "type": "string", "nullable": true }
                        }],
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Node" }
                                }
                            }
                        },
                        "responses": { "201": { "description": "ok" } }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Node": {
                        "type": "object",
                        "properties": {
                            "children": {
                                "type": "array",
                                "items": { "$ref": "#/components/schemas/Node" }
                            }
                        }
                    }
                }
            }
        }"##;

        let tools = OpenApiProvider::from_string(SPEC)
            .unwrap()
            .into_handler()
            .list_tools();
        let schema = serde_json::to_value(&tools[0].input_schema).unwrap();

        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(
            schema.pointer("/properties/note/type"),
            Some(&json!(["string", "null"]))
        );
        // The body's recursion points at the input schema's own root `$defs`,
        // which is where `#/$defs/Node` resolves from.
        assert_eq!(
            schema.pointer("/properties/body/properties/children/items/$ref"),
            Some(&json!("#/$defs/Node"))
        );
        assert!(schema.pointer("/$defs/Node").is_some(), "{schema:#}");
        assert!(schema.pointer("/properties/body/$defs").is_none());
    }

    mod upstream {
        //! Calls that reach an upstream API, served by a local mock.

        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use super::*;

        /// One tool, `fetch`, which POSTs to `/fetch`.
        const FETCH_SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0" },
            "paths": {
                "/fetch": {
                    "post": { "operationId": "fetch", "responses": { "200": { "description": "ok" } } }
                }
            }
        }"#;

        fn handler_for(spec: &str, server: &MockServer) -> OpenApiHandler {
            OpenApiProvider::from_string(spec)
                .unwrap()
                .allowing_loopback()
                .with_base_url(&server.uri())
                .unwrap()
                .into_handler()
        }

        #[tokio::test]
        async fn test_redirect_to_blocked_address_is_not_followed() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/fetch"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
                )
                .mount(&server)
                .await;

            let err = handler_for(FETCH_SPEC, &server)
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("blocked range 169.254.0.0/16"),
                "{err}"
            );
        }

        #[tokio::test]
        async fn test_redirect_to_allowed_address_is_followed() {
            let target = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/moved"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
                .mount(&target)
                .await;
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/fetch"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("location", format!("{}/moved", target.uri())),
                )
                .mount(&server)
                .await;

            let result = handler_for(FETCH_SPEC, &server)
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap();
            assert!(!result.is_error(), "{result:?}");
            assert!(result.first_text().unwrap().contains("\"ok\""));
        }
    }
}
