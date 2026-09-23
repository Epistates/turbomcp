//! MCP handler implementation for OpenAPI operations.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolInputSchema,
    ToolOutputSchema, ToolResult,
};

use crate::error::{OpenApiError, Result};
use crate::mapping::McpType;
use crate::provider::{ExtractedOperation, OpenApiProvider, param_value};
use crate::schema::hoist_defs;

/// Longest tool name the MCP specification recommends.
const MAX_TOOL_NAME_LEN: usize = 128;

/// MCP handler that exposes OpenAPI operations as tools and resources.
#[derive(Clone)]
pub struct OpenApiHandler {
    provider: Arc<OpenApiProvider>,
    /// Each tool's name, paired with its index in `provider.operations()`.
    tools: Arc<[(String, usize)]>,
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
        let tools = Self::assign_tool_names(&provider).into();
        Self { provider, tools }
    }

    /// Get the underlying provider.
    pub fn provider(&self) -> &OpenApiProvider {
        &self.provider
    }

    /// Name every tool, uniquely.
    ///
    /// Tool names SHOULD be unique within a server, and two operations can
    /// want the same one: a spec may repeat an `operationId`, and two
    /// different ones can sanitize alike. A name wanted once is used as is.
    /// Of several operations wanting one name, the first in spec order keeps
    /// it and the rest take the first free `_2`, `_3`, … suffix, skipping any
    /// name another operation wants for itself.
    fn assign_tool_names(provider: &OpenApiProvider) -> Vec<(String, usize)> {
        let wanted: Vec<(String, usize)> = provider
            .operations()
            .iter()
            .enumerate()
            .filter(|(_, op)| op.mcp_type == McpType::Tool)
            .map(|(index, op)| (Self::tool_name(op), index))
            .collect();

        let mut taken: HashSet<String> = wanted.iter().map(|(name, _)| name.clone()).collect();
        let mut seen = HashSet::new();
        wanted
            .into_iter()
            .map(|(name, index)| {
                if seen.insert(name.clone()) {
                    return (name, index);
                }
                let unique = (2..)
                    .map(|n| {
                        let suffix = format!("_{n}");
                        let stem = &name[..name.len().min(MAX_TOOL_NAME_LEN - suffix.len())];
                        format!("{stem}{suffix}")
                    })
                    .find(|candidate| !taken.contains(candidate))
                    .expect("an unbounded range always has a free suffix");
                taken.insert(unique.clone());
                (unique, index)
            })
            .collect()
    }

    /// The name an operation wants as a tool.
    ///
    /// The `operationId` when there is one, otherwise `{method}_{path}`, kept
    /// to the characters the MCP specification allows in a tool name
    /// (`A-Z a-z 0-9 _ - .`, at most 128): anything else becomes `_`.
    /// `operationId`s such as `pets:list` or `get /pets` are common.
    fn tool_name(op: &ExtractedOperation) -> String {
        let name = op
            .operation_id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                // Generate name from method and path
                let path_part = op
                    .path
                    .trim_start_matches('/')
                    .replace('/', "_")
                    .replace(['{', '}'], "");
                format!("{}_{}", op.method.to_lowercase(), path_part)
            });
        name.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .take(MAX_TOOL_NAME_LEN)
            .collect()
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
            if op.request_body_required {
                required.push("body".to_string());
            }
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
        self.tools
            .iter()
            .find(|(tool, _)| tool == name)
            .map(|(_, index)| &self.provider.operations()[*index])
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

    /// Execute an operation via HTTP, returning the body of a 2xx response.
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
    ) -> Result<String> {
        let url = self.provider.build_url(op, &args)?;

        // Validate every argument before anything goes on the wire.
        let body = args.get("body");
        if op.request_body_required && body.is_none() {
            return Err(OpenApiError::MissingParameter("body".to_string()));
        }
        let mut headers = HeaderMap::new();
        let mut cookies = Vec::new();
        for param in &op.parameters {
            let Some(value) = args.get(&param.name) else {
                if param.required && matches!(param.location.as_str(), "header" | "cookie") {
                    return Err(OpenApiError::MissingParameter(param.name.clone()));
                }
                continue;
            };
            match param.location.as_str() {
                "header" => {
                    let invalid = || {
                        OpenApiError::InvalidParameter(
                            param.name.clone(),
                            "not a valid HTTP header".to_string(),
                        )
                    };
                    let name =
                        HeaderName::from_bytes(param.name.as_bytes()).map_err(|_| invalid())?;
                    let value =
                        HeaderValue::from_str(&param_value(value)).map_err(|_| invalid())?;
                    headers.append(name, value);
                }
                "cookie" => cookies.push(format!(
                    "{}={}",
                    param.name,
                    utf8_percent_encode(&param_value(value), COOKIE_VALUE)
                )),
                _ => {}
            }
        }
        if !cookies.is_empty() {
            // RFC 6265 §5.4: all cookies travel in a single `Cookie` header.
            let cookie = HeaderValue::from_str(&cookies.join("; ")).map_err(|_| {
                OpenApiError::InvalidParameter("cookie".to_string(), "not a valid cookie".into())
            })?;
            headers.insert(COOKIE, cookie);
        }

        // SSRF protection: validate URL before making request
        self.provider.ssrf().check_request_target(&url).await?;

        let client = self.provider.client();

        let mut request = match op.method.as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => {
                return Err(OpenApiError::OperationNotFound(format!(
                    "unsupported method {}",
                    op.method
                )));
            }
        };

        if let Some(body) = body {
            request = request.json(body);
        }
        request = request.headers(headers);

        // Inject auth credentials before sending. If the operation has security
        // requirements but no provider is installed, the request still goes
        // out — the upstream will return 401 and surface the misconfiguration.
        if !op.security.is_empty()
            && let Some(auth) = self.provider.auth_provider()
        {
            request = auth.apply(request, &op.security, self.provider.security_schemes());
        }

        let response = request.send().await.map_err(|e| self.request_error(&e))?;

        let status = response.status();
        let body = response.text().await.map_err(|e| self.request_error(&e))?;

        if !status.is_success() {
            return Err(OpenApiError::ApiError(format!(
                "upstream returned {status}: {body}"
            )));
        }
        Ok(body)
    }

    /// Classify a failed exchange with the upstream.
    fn request_error(&self, error: &reqwest::Error) -> OpenApiError {
        if error.is_timeout() {
            OpenApiError::Timeout(self.provider.timeout().as_secs())
        } else {
            OpenApiError::ApiError(format!("HTTP request failed: {}", error_chain(error)))
        }
    }
}

/// Characters percent-encoded in a cookie value: everything RFC 6265's
/// `cookie-octet` excludes, and `%` so the encoding is reversible.
const COOKIE_VALUE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b',')
    .add(b';')
    .add(b'\\')
    .add(b'%');

/// The MCP error an operation failure maps to.
///
/// Input the spec's schema should have caught is invalid params; the upstream
/// failing, timing out, or being refused by the SSRF guard keeps its own kind,
/// which a tool result carries in `_meta`.
fn to_mcp_error(error: OpenApiError) -> McpError {
    match error {
        OpenApiError::MissingParameter(_) | OpenApiError::InvalidParameter(..) => {
            McpError::invalid_params(error.to_string())
        }
        OpenApiError::SsrfBlocked(_) => McpError::security(error.to_string()),
        OpenApiError::Timeout(_) => McpError::timeout(error.to_string()),
        OpenApiError::ApiError(_) => McpError::external_service(error.to_string()),
        _ => McpError::internal(error.to_string()),
    }
}

/// A 2xx body as text for a content block, and as JSON if it is JSON.
///
/// JSON is re-indented for reading; anything else is passed through as sent,
/// not quoted as a JSON string.
fn render_body(body: String) -> (String, Option<Value>) {
    match serde_json::from_str::<Value>(&body) {
        Ok(json) => (
            serde_json::to_string_pretty(&json).unwrap_or(body),
            Some(json),
        ),
        Err(_) => (body, None),
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
        self.tools
            .iter()
            .map(|(name, index)| (name, &self.provider.operations()[*index]))
            .map(|(name, op)| Tool {
                name: name.clone(),
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

            match self.execute_operation(op, args_map).await {
                Ok(body) => Ok(ToolResult::text(render_body(body).0)),
                // Without a base URL no call can work, whatever the model
                // sends: that is the server's fault, not a tool failure.
                Err(e @ OpenApiError::NoBaseUrl) => Err(to_mcp_error(e)),
                // Everything else, bad arguments included (SEP-1303), is a
                // tool execution error the model can read and act on.
                Err(e) => Ok(to_mcp_error(e).to_tool_result()),
            }
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
            let body = self
                .execute_operation(op, HashMap::new())
                .await
                .map_err(to_mcp_error)?;

            Ok(ResourceResult::text(uri, render_body(body).0))
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
            request_body_required: false,
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
            request_body_required: false,
            mcp_type: McpType::Tool,
            security: Vec::new(),
            response_schema: None,
        };

        assert_eq!(OpenApiHandler::tool_name(&op_with_id), "createUser");
        assert_eq!(OpenApiHandler::tool_name(&op_without_id), "delete_users_id");
    }

    /// Whether `name` follows the MCP 2025-11-25 tool-name rules.
    fn is_valid_tool_name(name: &str) -> bool {
        (1..=128).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    }

    #[test]
    fn test_tool_names_are_valid_and_unique() {
        let long_id = "x".repeat(200);
        let spec = json!({
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0" },
            "paths": {
                "/v1/{name}:cancel": {
                    "post": { "responses": { "200": { "description": "ok" } } }
                },
                "/pets": {
                    "post": { "operationId": "pets:create", "responses": { "200": { "description": "ok" } } },
                    "put": { "operationId": "pets create", "responses": { "200": { "description": "ok" } } },
                    "patch": { "operationId": "pets_create", "responses": { "200": { "description": "ok" } } },
                    "delete": { "operationId": "pets_create_2", "responses": { "200": { "description": "ok" } } }
                },
                "/long": {
                    "post": { "operationId": long_id, "responses": { "200": { "description": "ok" } } },
                    "put": { "operationId": long_id, "responses": { "200": { "description": "ok" } } }
                }
            }
        });
        let handler = OpenApiProvider::from_string(&spec.to_string())
            .unwrap()
            .into_handler();
        let names: Vec<String> = handler.list_tools().into_iter().map(|t| t.name).collect();

        for name in &names {
            assert!(is_valid_tool_name(name), "invalid tool name {name:?}");
        }
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate tool names in {names:?}"
        );

        assert!(
            names.contains(&"post_v1_name_cancel".to_string()),
            "{names:?}"
        );
        // Operations come in POST, PUT, DELETE, PATCH order. Three want
        // `pets_create`: the first keeps it, the other two skip
        // `pets_create_2`, which the DELETE wants for itself.
        assert_eq!(
            &names[1..5],
            [
                "pets_create",
                "pets_create_3",
                "pets_create_2",
                "pets_create_4"
            ]
        );
        assert_eq!(names[5], "x".repeat(128));
        assert_eq!(names[6], format!("{}_2", "x".repeat(126)));

        // And each name routes to its own operation.
        for name in &names {
            assert!(handler.find_tool_operation(name).is_some(), "{name}");
        }
        assert_eq!(
            handler.find_tool_operation("pets_create_2").unwrap().method,
            "DELETE"
        );
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

        use wiremock::matchers::{header, method, path, query_param};
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

            let result = handler_for(FETCH_SPEC, &server)
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap();
            assert!(result.is_error());
            let text = result.first_text().unwrap();
            assert!(text.contains("blocked range 169.254.0.0/16"), "{text}");
        }

        fn error_code(result: &ToolResult) -> Option<&Value> {
            result.meta.as_ref()?.get("io.turbomcp/errorCode")
        }

        #[tokio::test]
        async fn test_upstream_error_status_is_a_tool_error() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/fetch"))
                .respond_with(ResponseTemplate::new(503).set_body_string("try later"))
                .mount(&server)
                .await;

            // Was a JSON-RPC -32603, which a client does not show the model.
            let result = handler_for(FETCH_SPEC, &server)
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap();
            assert!(result.is_error());
            let text = result.first_text().unwrap();
            assert!(text.contains("503") && text.contains("try later"), "{text}");
        }

        #[tokio::test]
        async fn test_unreachable_upstream_is_a_tool_error() {
            let server = MockServer::start().await;
            let handler = handler_for(FETCH_SPEC, &server);
            drop(server);

            let result = handler
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap();
            assert!(result.is_error(), "{result:?}");
        }

        #[tokio::test]
        async fn test_missing_base_url_is_a_protocol_error() {
            let handler = OpenApiProvider::from_string(FETCH_SPEC)
                .unwrap()
                .into_handler();
            let err = handler
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap_err();
            assert_eq!(err.jsonrpc_error_code(), -32603);
        }

        /// `/items/{id}` taking a required query, header and cookie parameter,
        /// an optional cookie, and an optional body.
        const PARAMS_SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0" },
            "paths": {
                "/items/{id}": {
                    "put": {
                        "operationId": "putItem",
                        "parameters": [
                            { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "mode", "in": "query", "required": true, "schema": { "type": "string" } },
                            { "name": "X-Trace", "in": "header", "required": true, "schema": { "type": "string" } },
                            { "name": "session", "in": "cookie", "required": true, "schema": { "type": "string" } },
                            { "name": "theme", "in": "cookie", "schema": { "type": "string" } }
                        ],
                        "requestBody": {
                            "content": { "application/json": { "schema": { "type": "object" } } }
                        },
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }
        }"#;

        fn all_params() -> Value {
            json!({ "id": "7", "mode": "fast", "X-Trace": "t-1", "session": "a b;c", "theme": "dark" })
        }

        #[tokio::test]
        async fn test_missing_required_params_are_invalid_params() {
            let server = MockServer::start().await;
            let handler = handler_for(PARAMS_SPEC, &server);

            for missing in ["id", "mode", "X-Trace", "session"] {
                let mut args = all_params();
                args.as_object_mut().unwrap().remove(missing);
                let result = handler
                    .call_tool("putItem", args, &RequestContext::new())
                    .await
                    .unwrap();
                // A tool execution error per SEP-1303, classified as invalid
                // params; was -32603, or for headers and cookies no error at
                // all.
                assert!(result.is_error(), "{missing}");
                assert_eq!(error_code(&result), Some(&json!(-32602)), "{missing}");
                assert!(result.first_text().unwrap().contains(missing));
            }
            assert!(server.received_requests().await.unwrap().is_empty());
        }

        #[tokio::test]
        async fn test_cookie_and_header_params_are_sent_and_body_is_optional() {
            let server = MockServer::start().await;
            Mock::given(method("PUT"))
                .and(path("/items/7"))
                .and(query_param("mode", "fast"))
                .and(header("x-trace", "t-1"))
                // One header, values encoded so `;` cannot start a new cookie.
                .and(header("cookie", "session=a%20b%3Bc; theme=dark"))
                .respond_with(ResponseTemplate::new(200).set_body_string("done"))
                .expect(1)
                .mount(&server)
                .await;
            let handler = handler_for(PARAMS_SPEC, &server);

            let tool = &handler.list_tools()[0];
            let required = tool.input_schema.required.as_ref().unwrap();
            assert!(!required.contains(&"body".to_string()), "{required:?}");

            let result = handler
                .call_tool("putItem", all_params(), &RequestContext::new())
                .await
                .unwrap();
            assert!(!result.is_error(), "{result:?}");
            // A non-JSON body comes back as sent, not as a quoted JSON string.
            assert_eq!(result.first_text(), Some("done"));
        }

        #[tokio::test]
        async fn test_required_body_is_enforced() {
            let spec = FETCH_SPEC.replace(
                r#""operationId": "fetch","#,
                r#""operationId": "fetch", "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object" } } } },"#,
            );
            let server = MockServer::start().await;
            let handler = handler_for(&spec, &server);

            let tool = &handler.list_tools()[0];
            assert_eq!(tool.input_schema.required, Some(vec!["body".to_string()]));

            let result = handler
                .call_tool("fetch", json!({}), &RequestContext::new())
                .await
                .unwrap();
            assert!(result.is_error());
            assert_eq!(error_code(&result), Some(&json!(-32602)));
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
