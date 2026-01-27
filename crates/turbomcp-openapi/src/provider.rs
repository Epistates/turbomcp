//! OpenAPI provider for generating MCP components from OpenAPI specs.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openapiv3::{OpenAPI, Operation, Parameter, ParameterSchemaOrContent, ReferenceOr, Schema};
use serde_json::{Value, json};
use url::Url;

use crate::error::{OpenApiError, Result};
use crate::handler::OpenApiHandler;
use crate::mapping::{McpType, RouteMapping};
use crate::parser::{fetch_from_url, load_from_file, parse_spec};

/// An operation extracted from an OpenAPI spec.
#[derive(Debug, Clone)]
pub struct ExtractedOperation {
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Path template (e.g., "/users/{id}")
    pub path: String,
    /// Operation ID (if specified)
    pub operation_id: Option<String>,
    /// Summary/description
    pub summary: Option<String>,
    /// Operation description
    pub description: Option<String>,
    /// Parameters
    pub parameters: Vec<ExtractedParameter>,
    /// Request body schema (if any)
    pub request_body_schema: Option<Value>,
    /// What MCP type this maps to
    pub mcp_type: McpType,
}

/// A parameter extracted from an OpenAPI operation.
#[derive(Debug, Clone)]
pub struct ExtractedParameter {
    /// Parameter name
    pub name: String,
    /// Where the parameter goes (path, query, header, cookie)
    pub location: String,
    /// Whether the parameter is required
    pub required: bool,
    /// Description
    pub description: Option<String>,
    /// JSON Schema for the parameter
    pub schema: Option<Value>,
}

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// OpenAPI to MCP provider.
///
/// This provider parses OpenAPI specifications and converts them to MCP
/// tools and resources that can be used with a TurboMCP server.
///
/// # Security
///
/// The provider includes built-in SSRF protection that blocks requests to:
/// - Localhost and loopback addresses (127.0.0.0/8, ::1)
/// - Private IP ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
/// - Link-local addresses (169.254.0.0/16) including cloud metadata endpoints
/// - Other reserved ranges
///
/// Requests have a default timeout of 30 seconds to prevent slowloris attacks.
#[derive(Debug)]
pub struct OpenApiProvider {
    /// The parsed OpenAPI specification
    spec: OpenAPI,
    /// Base URL for API calls
    base_url: Option<Url>,
    /// Route mapping configuration
    mapping: RouteMapping,
    /// HTTP client for making API calls
    client: reqwest::Client,
    /// Extracted operations
    operations: Vec<ExtractedOperation>,
    /// Request timeout
    timeout: std::time::Duration,
}

impl OpenApiProvider {
    /// Create a provider from a parsed OpenAPI specification.
    pub fn from_spec(spec: OpenAPI) -> Self {
        let mapping = RouteMapping::default_rules();
        let timeout = std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut provider = Self {
            spec,
            base_url: None,
            mapping,
            client,
            operations: Vec::new(),
            timeout,
        };
        provider.extract_operations();
        provider
    }

    /// Create a provider from an OpenAPI specification string.
    pub fn from_string(content: &str) -> Result<Self> {
        let spec = parse_spec(content)?;
        Ok(Self::from_spec(spec))
    }

    /// Create a provider by loading from a file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let spec = load_from_file(path)?;
        Ok(Self::from_spec(spec))
    }

    /// Create a provider by fetching from a URL.
    pub async fn from_url(url: &str) -> Result<Self> {
        let spec = fetch_from_url(url).await?;
        Ok(Self::from_spec(spec))
    }

    /// Set the base URL for API calls.
    pub fn with_base_url(mut self, base_url: &str) -> Result<Self> {
        self.base_url = Some(Url::parse(base_url)?);
        Ok(self)
    }

    /// Set a custom route mapping configuration.
    #[must_use]
    pub fn with_route_mapping(mut self, mapping: RouteMapping) -> Self {
        self.mapping = mapping;
        self.extract_operations(); // Re-extract with new mapping
        self
    }

    /// Set a custom HTTP client.
    ///
    /// # Warning
    ///
    /// When using a custom client, ensure it has appropriate timeout settings.
    /// The default client uses a 30-second timeout.
    #[must_use]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    /// Set a custom request timeout.
    ///
    /// This rebuilds the HTTP client with the new timeout. The default timeout
    /// is 30 seconds.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self.client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        self
    }

    /// Get the current request timeout.
    pub fn timeout(&self) -> std::time::Duration {
        self.timeout
    }

    /// Get the API title from the spec.
    pub fn title(&self) -> &str {
        &self.spec.info.title
    }

    /// Get the API version from the spec.
    pub fn version(&self) -> &str {
        &self.spec.info.version
    }

    /// Get all extracted operations.
    pub fn operations(&self) -> &[ExtractedOperation] {
        &self.operations
    }

    /// Get operations that map to MCP tools.
    pub fn tools(&self) -> impl Iterator<Item = &ExtractedOperation> {
        self.operations
            .iter()
            .filter(|op| op.mcp_type == McpType::Tool)
    }

    /// Get operations that map to MCP resources.
    pub fn resources(&self) -> impl Iterator<Item = &ExtractedOperation> {
        self.operations
            .iter()
            .filter(|op| op.mcp_type == McpType::Resource)
    }

    /// Convert this provider into an McpHandler.
    pub fn into_handler(self) -> OpenApiHandler {
        OpenApiHandler::new(Arc::new(self))
    }

    /// Extract operations from the OpenAPI spec.
    fn extract_operations(&mut self) {
        self.operations.clear();

        for (path, path_item) in &self.spec.paths.paths {
            let path_item = match path_item {
                ReferenceOr::Item(item) => item,
                ReferenceOr::Reference { .. } => continue, // Skip references for now
            };

            // Extract operations for each HTTP method
            let methods = [
                ("GET", &path_item.get),
                ("POST", &path_item.post),
                ("PUT", &path_item.put),
                ("DELETE", &path_item.delete),
                ("PATCH", &path_item.patch),
            ];

            for (method, operation) in methods {
                if let Some(op) = operation {
                    let mcp_type = self.mapping.get_mcp_type(method, path);
                    if mcp_type == McpType::Skip {
                        continue;
                    }

                    self.operations
                        .push(self.extract_operation(method, path, op, mcp_type));
                }
            }
        }
    }

    /// Extract a single operation.
    fn extract_operation(
        &self,
        method: &str,
        path: &str,
        operation: &Operation,
        mcp_type: McpType,
    ) -> ExtractedOperation {
        let parameters = operation
            .parameters
            .iter()
            .filter_map(|p| match p {
                ReferenceOr::Item(param) => Some(self.extract_parameter(param)),
                ReferenceOr::Reference { .. } => None,
            })
            .collect();

        let request_body_schema = operation.request_body.as_ref().and_then(|rb| match rb {
            ReferenceOr::Item(body) => body
                .content
                .get("application/json")
                .and_then(|mt| mt.schema.as_ref())
                .and_then(|s| self.schema_to_json(s)),
            ReferenceOr::Reference { .. } => None,
        });

        ExtractedOperation {
            method: method.to_string(),
            path: path.to_string(),
            operation_id: operation.operation_id.clone(),
            summary: operation.summary.clone(),
            description: operation.description.clone(),
            parameters,
            request_body_schema,
            mcp_type,
        }
    }

    /// Extract a parameter definition.
    fn extract_parameter(&self, param: &Parameter) -> ExtractedParameter {
        let (name, location, required, description, schema) = match param {
            Parameter::Query { parameter_data, .. } => (
                parameter_data.name.clone(),
                "query".to_string(),
                parameter_data.required,
                parameter_data.description.clone(),
                self.extract_param_schema(&parameter_data.format),
            ),
            Parameter::Header { parameter_data, .. } => (
                parameter_data.name.clone(),
                "header".to_string(),
                parameter_data.required,
                parameter_data.description.clone(),
                self.extract_param_schema(&parameter_data.format),
            ),
            Parameter::Path { parameter_data, .. } => (
                parameter_data.name.clone(),
                "path".to_string(),
                true, // Path params are always required
                parameter_data.description.clone(),
                self.extract_param_schema(&parameter_data.format),
            ),
            Parameter::Cookie { parameter_data, .. } => (
                parameter_data.name.clone(),
                "cookie".to_string(),
                parameter_data.required,
                parameter_data.description.clone(),
                self.extract_param_schema(&parameter_data.format),
            ),
        };

        ExtractedParameter {
            name,
            location,
            required,
            description,
            schema,
        }
    }

    /// Extract schema from parameter format.
    fn extract_param_schema(&self, format: &ParameterSchemaOrContent) -> Option<Value> {
        match format {
            ParameterSchemaOrContent::Schema(schema) => self.schema_to_json(schema),
            ParameterSchemaOrContent::Content(_) => None,
        }
    }

    /// Convert an OpenAPI schema to a JSON Schema value.
    fn schema_to_json(&self, schema: &ReferenceOr<Schema>) -> Option<Value> {
        match schema {
            ReferenceOr::Item(s) => Some(serde_json::to_value(s).ok()?),
            ReferenceOr::Reference { reference } => Some(json!({ "$ref": reference })),
        }
    }

    /// Build the full URL for an operation.
    pub(crate) fn build_url(
        &self,
        operation: &ExtractedOperation,
        args: &HashMap<String, Value>,
    ) -> Result<Url> {
        let base = self.base_url.as_ref().ok_or(OpenApiError::NoBaseUrl)?;

        // Replace path parameters
        let mut path = operation.path.clone();
        for param in &operation.parameters {
            if param.location == "path" {
                if let Some(value) = args.get(&param.name) {
                    let value_str = match value {
                        Value::String(s) => s.clone(),
                        _ => value.to_string(),
                    };
                    path = path.replace(&format!("{{{}}}", param.name), &value_str);
                } else if param.required {
                    return Err(OpenApiError::MissingParameter(param.name.clone()));
                }
            }
        }

        let mut url = base.join(&path)?;

        // Collect query parameters first
        let mut query_params: Vec<(String, String)> = Vec::new();
        for param in &operation.parameters {
            if param.location == "query" {
                if let Some(value) = args.get(&param.name) {
                    let value_str = match value {
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => value.to_string(),
                    };
                    query_params.push((param.name.clone(), value_str));
                } else if param.required {
                    return Err(OpenApiError::MissingParameter(param.name.clone()));
                }
            }
        }

        // Only add query string if there are parameters
        if !query_params.is_empty() {
            let mut query_pairs = url.query_pairs_mut();
            for (key, value) in query_params {
                query_pairs.append_pair(&key, &value);
            }
        }

        Ok(url)
    }

    /// Get the HTTP client.
    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SPEC: &str = r#"{
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "summary": "List all users",
                    "responses": { "200": { "description": "Success" } }
                },
                "post": {
                    "operationId": "createUser",
                    "summary": "Create a user",
                    "responses": { "201": { "description": "Created" } }
                }
            },
            "/users/{id}": {
                "get": {
                    "operationId": "getUser",
                    "summary": "Get a user by ID",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": { "200": { "description": "Success" } }
                },
                "delete": {
                    "operationId": "deleteUser",
                    "summary": "Delete a user",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": { "204": { "description": "Deleted" } }
                }
            }
        }
    }"#;

    #[test]
    fn test_provider_from_string() {
        let provider = OpenApiProvider::from_string(TEST_SPEC).unwrap();

        assert_eq!(provider.title(), "Test API");
        assert_eq!(provider.version(), "1.0.0");
    }

    #[test]
    fn test_operation_extraction() {
        let provider = OpenApiProvider::from_string(TEST_SPEC).unwrap();

        assert_eq!(provider.operations().len(), 4);

        // Check GET /users is a resource
        let list_users = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("listUsers"))
            .unwrap();
        assert_eq!(list_users.mcp_type, McpType::Resource);
        assert_eq!(list_users.method, "GET");

        // Check POST /users is a tool
        let create_user = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("createUser"))
            .unwrap();
        assert_eq!(create_user.mcp_type, McpType::Tool);
        assert_eq!(create_user.method, "POST");
    }

    #[test]
    fn test_tools_and_resources() {
        let provider = OpenApiProvider::from_string(TEST_SPEC).unwrap();

        let tools: Vec<_> = provider.tools().collect();
        let resources: Vec<_> = provider.resources().collect();

        // GET operations -> resources
        assert_eq!(resources.len(), 2);
        // POST, DELETE operations -> tools
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_build_url_with_path_params() {
        let provider = OpenApiProvider::from_string(TEST_SPEC)
            .unwrap()
            .with_base_url("https://api.example.com")
            .unwrap();

        let get_user = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("getUser"))
            .unwrap();

        let mut args = HashMap::new();
        args.insert("id".to_string(), json!("123"));

        let url = provider.build_url(get_user, &args).unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/users/123");
    }

    #[test]
    fn test_missing_required_param() {
        let provider = OpenApiProvider::from_string(TEST_SPEC)
            .unwrap()
            .with_base_url("https://api.example.com")
            .unwrap();

        let get_user = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("getUser"))
            .unwrap();

        let args = HashMap::new(); // Missing 'id'

        let result = provider.build_url(get_user, &args);
        assert!(matches!(result, Err(OpenApiError::MissingParameter(_))));
    }
}
