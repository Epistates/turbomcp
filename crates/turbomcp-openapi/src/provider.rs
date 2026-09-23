//! OpenAPI provider for generating MCP components from OpenAPI specs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openapiv3::{
    OpenAPI, Operation, Parameter, ParameterSchemaOrContent, ReferenceOr, Schema, SecurityScheme,
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
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
    /// Effective security requirements: a list of alternative
    /// [`SecurityRequirement`](openapiv3::SecurityRequirement) objects. Each
    /// entry maps a scheme name from `components.securitySchemes` to the
    /// scopes that must be present. Satisfy any one alternative. Operation-level
    /// `security` overrides the spec-level `security`; an explicit empty list
    /// (`security: []`) on an operation disables auth.
    pub security: Vec<HashMap<String, Vec<String>>>,
    /// JSON Schema of the operation's primary success response (first 2xx
    /// `application/json` response, with `$ref`s inlined). Surfaces in the
    /// generated MCP `Tool::output_schema` for clients that consume MCP
    /// 2025-11-25's `outputSchema`. `None` if the operation has no JSON
    /// response or only `default` / non-2xx responses.
    pub response_schema: Option<Value>,
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

/// Hook for satisfying an operation's [`SecurityRequirement`](openapiv3::SecurityRequirement)s
/// before the request is sent.
///
/// OpenAPI specifications declare auth via `securitySchemes` (`apiKey`,
/// `http bearer`/`basic`, `oauth2`, `openIdConnect`) and per-operation/spec-level
/// `security`. This crate parses both — `OpenApiProvider::security_schemes`
/// returns the scheme definitions, and each [`ExtractedOperation::security`]
/// holds the operation's effective requirements. Implement this trait to
/// inject credentials matching one of the requirement alternatives.
///
/// # Example
///
/// ```rust,ignore
/// use std::collections::HashMap;
/// use std::sync::Arc;
/// use turbomcp_openapi::{AuthProvider, OpenApiProvider};
///
/// #[derive(Debug)]
/// struct StaticBearer(String);
///
/// impl AuthProvider for StaticBearer {
///     fn apply(
///         &self,
///         request: reqwest::RequestBuilder,
///         _requirements: &[HashMap<String, Vec<String>>],
///         _schemes: &HashMap<String, openapiv3::SecurityScheme>,
///     ) -> reqwest::RequestBuilder {
///         request.bearer_auth(&self.0)
///     }
/// }
///
/// let provider = OpenApiProvider::from_string(spec)?
///     .with_auth_provider(Arc::new(StaticBearer("token".into())));
/// ```
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    /// Apply auth to an outgoing request.
    ///
    /// `requirements` is the list of alternative [`SecurityRequirement`](openapiv3::SecurityRequirement)
    /// objects from the operation; satisfying any one alternative is sufficient.
    /// Each entry maps a scheme name (from `components.securitySchemes`) to the
    /// required scopes. `schemes` is the spec's `components.securitySchemes`
    /// map, with references already resolved.
    ///
    /// Returning the unmodified `request` is acceptable for operations whose
    /// requirements your implementation cannot satisfy — the request will then
    /// fail with whatever auth error the upstream returns.
    fn apply(
        &self,
        request: reqwest::RequestBuilder,
        requirements: &[HashMap<String, Vec<String>>],
        schemes: &HashMap<String, SecurityScheme>,
    ) -> reqwest::RequestBuilder;
}

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
    /// Resolved security scheme definitions, keyed by scheme name.
    security_schemes: HashMap<String, SecurityScheme>,
    /// Request timeout
    timeout: std::time::Duration,
    /// Optional auth provider that satisfies each operation's `security` requirements.
    auth_provider: Option<Arc<dyn AuthProvider>>,
}

impl OpenApiProvider {
    /// Create a provider from a parsed OpenAPI specification.
    ///
    /// If `spec.servers` is non-empty, `base_url` is initialized from
    /// `spec.servers[0].url` (with any default `variables` substituted in). Use
    /// [`Self::with_base_url`] to override. Server URLs that fail to parse as
    /// absolute leave `base_url` unset; `with_base_url` must then be called
    /// before any tool/resource invocation.
    pub fn from_spec(spec: OpenAPI) -> Self {
        let mapping = RouteMapping::default_rules();
        let timeout = std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS);
        // `Client::builder().build()` only fails on egregious config (e.g.
        // a missing TLS backend) — not silently downgrading to
        // `Client::new()` (which would lose the configured timeout) is the
        // correct stance: the user asked for a timeout, surface the error.
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest::Client::builder() failed; check TLS backend / build features");

        let base_url = spec
            .servers
            .first()
            .and_then(|server| Self::resolve_server_url(server).ok());
        let security_schemes = Self::collect_security_schemes(&spec);

        let mut provider = Self {
            spec,
            base_url,
            mapping,
            client,
            operations: Vec::new(),
            security_schemes,
            timeout,
            auth_provider: None,
        };
        provider.extract_operations();
        provider
    }

    /// Substitute the default values for any `{var}` placeholders in a server URL,
    /// then parse the result into a [`Url`].
    fn resolve_server_url(server: &openapiv3::Server) -> Result<Url> {
        let mut url = server.url.clone();
        if let Some(vars) = &server.variables {
            for (name, var) in vars {
                let placeholder = format!("{{{name}}}");
                url = url.replace(&placeholder, &var.default);
            }
        }
        Ok(Url::parse(&url)?)
    }

    /// Resolve all `securitySchemes` to inline `SecurityScheme` definitions.
    /// `Reference` entries (`{"$ref": "..."}`) at the top level are skipped:
    /// the OpenAPI spec allows them but they're rare in practice and would
    /// require a separate dereference pass against `components`.
    fn collect_security_schemes(spec: &OpenAPI) -> HashMap<String, SecurityScheme> {
        spec.components
            .as_ref()
            .map(|c| {
                c.security_schemes
                    .iter()
                    .filter_map(|(name, entry)| match entry {
                        ReferenceOr::Item(scheme) => Some((name.clone(), scheme.clone())),
                        ReferenceOr::Reference { .. } => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
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

    /// Install an [`AuthProvider`] that injects credentials matching each
    /// operation's [`SecurityRequirement`](openapiv3::SecurityRequirement)s.
    /// Without this, operations on authenticated upstream APIs will fail with
    /// 401 unless the caller has installed equivalent auth via
    /// [`Self::with_client`].
    #[must_use]
    pub fn with_auth_provider(mut self, provider: Arc<dyn AuthProvider>) -> Self {
        self.auth_provider = Some(provider);
        self
    }

    /// Get the spec's security scheme definitions, keyed by scheme name.
    /// Reference entries are silently dropped (rare in practice).
    pub fn security_schemes(&self) -> &HashMap<String, SecurityScheme> {
        &self.security_schemes
    }

    /// Get the installed auth provider, if any.
    pub(crate) fn auth_provider(&self) -> Option<&Arc<dyn AuthProvider>> {
        self.auth_provider.as_ref()
    }

    /// Set a custom request timeout.
    ///
    /// This rebuilds the HTTP client with the new timeout. The default timeout
    /// is 30 seconds.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        // See `from_spec`: rebuilding without the configured timeout would
        // silently regress to reqwest's default; expect on builder failure
        // instead so the caller's intent isn't lost.
        self.client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest::Client::builder() failed in with_timeout");
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

        // Operation-level `security` overrides spec-level. An explicit empty
        // list (`security: []`) on the operation disables auth and must NOT
        // fall back to spec-level — we model that as the empty Vec.
        let security = operation
            .security
            .as_ref()
            .or(self.spec.security.as_ref())
            .map(|reqs| {
                reqs.iter()
                    .map(|req| {
                        req.iter()
                            .map(|(name, scopes)| (name.clone(), scopes.clone()))
                            .collect::<HashMap<_, _>>()
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Pick the first 2xx response with an `application/json` body and
        // inline its schema. Falls back to whichever 2xx the iterator yields
        // first if none expose a JSON body.
        let response_schema = operation
            .responses
            .responses
            .iter()
            .filter_map(|(code, resp)| {
                let code_str = code.to_string();
                let is_2xx = code_str
                    .strip_prefix('2')
                    .map(|rest| {
                        rest.len() == 2
                            && rest
                                .chars()
                                .all(|c| c.is_ascii_digit() || c == 'X' || c == 'x')
                    })
                    .unwrap_or(false);
                if !is_2xx {
                    return None;
                }
                match resp {
                    ReferenceOr::Item(r) => r
                        .content
                        .get("application/json")
                        .and_then(|mt| mt.schema.as_ref())
                        .and_then(|s| self.schema_to_json(s)),
                    ReferenceOr::Reference { .. } => None,
                }
            })
            .next();

        ExtractedOperation {
            method: method.to_string(),
            path: path.to_string(),
            operation_id: operation.operation_id.clone(),
            summary: operation.summary.clone(),
            description: operation.description.clone(),
            parameters,
            request_body_schema,
            mcp_type,
            security,
            response_schema,
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

    /// Convert an OpenAPI schema to a JSON Schema value, inlining `$ref`s
    /// against `components.schemas`.
    ///
    /// OpenAPI lets schemas reference each other through
    /// `{"$ref": "#/components/schemas/Foo"}`. MCP tool-input schemas have
    /// no cross-operation component dictionary to share, so we resolve those
    /// refs inline. Cycles are broken by leaving the first re-visited
    /// reference as a `$ref` literal rather than expanding it forever.
    fn schema_to_json(&self, schema: &ReferenceOr<Schema>) -> Option<Value> {
        let initial = match schema {
            ReferenceOr::Item(s) => serde_json::to_value(s).ok()?,
            ReferenceOr::Reference { reference } => {
                json!({ "$ref": reference })
            }
        };
        let mut visited = std::collections::HashSet::new();
        Some(self.resolve_refs(initial, &mut visited))
    }

    /// Recursively inline `$ref` pointers that target `components.schemas`.
    ///
    /// `visited` tracks the ref path currently being expanded; re-encountering
    /// the same pointer during expansion leaves the `$ref` in place so the
    /// output stays finite on self-referential schemas (the default interpretation
    /// consumers do — most JSON Schema validators understand internal `$ref`).
    fn resolve_refs(&self, value: Value, visited: &mut std::collections::HashSet<String>) -> Value {
        match value {
            Value::Object(mut map) => {
                if let Some(Value::String(reference)) = map.get("$ref").cloned()
                    && map.len() == 1
                {
                    if !visited.insert(reference.clone()) {
                        map.insert("$ref".to_string(), Value::String(reference));
                        return Value::Object(map);
                    }
                    let expanded = self.lookup_ref(&reference).map(|target| {
                        let target_json = serde_json::to_value(target).unwrap_or(Value::Null);
                        self.resolve_refs(target_json, visited)
                    });
                    visited.remove(&reference);
                    return expanded.unwrap_or(Value::Object({
                        let mut fallback = serde_json::Map::new();
                        fallback.insert("$ref".to_string(), Value::String(reference));
                        fallback
                    }));
                }
                let resolved = map
                    .into_iter()
                    .map(|(k, v)| (k, self.resolve_refs(v, visited)))
                    .collect();
                Value::Object(resolved)
            }
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|v| self.resolve_refs(v, visited))
                    .collect(),
            ),
            other => other,
        }
    }

    /// Look up a `#/components/schemas/Name` reference in the parsed spec.
    /// Follows reference chains up to `MAX_DEPTH` levels deep with cycle detection,
    /// so chains like `Foo -> Bar -> Baz` resolve correctly without unbounded recursion.
    fn lookup_ref(&self, reference: &str) -> Option<&Schema> {
        const PREFIX: &str = "#/components/schemas/";
        const MAX_DEPTH: usize = 10;
        let mut name = reference.strip_prefix(PREFIX)?;
        let components = self.spec.components.as_ref()?;
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for _ in 0..MAX_DEPTH {
            if !seen.insert(name) {
                // Cycle.
                return None;
            }
            match components.schemas.get(name)? {
                ReferenceOr::Item(schema) => return Some(schema),
                ReferenceOr::Reference { reference } => {
                    name = reference.strip_prefix(PREFIX)?;
                }
            }
        }
        None
    }

    /// Build the full URL for an operation.
    pub(crate) fn build_url(
        &self,
        operation: &ExtractedOperation,
        args: &HashMap<String, Value>,
    ) -> Result<Url> {
        let base = self.base_url.as_ref().ok_or(OpenApiError::NoBaseUrl)?;

        // Append to the base path rather than `Url::join`ing onto it: join
        // resolves the operation path as an absolute reference, which drops
        // the base's own path (`https://api.x/v1` + `/pets` gave
        // `https://api.x/pets`), and a value beginning with `/` turned the
        // path into a network-path reference that swapped the host.
        let mut path = base.path().trim_end_matches('/').to_string();
        for segment in operation.path.trim_start_matches('/').split('/') {
            path.push('/');
            path.push_str(&expand_path_segment(segment, args)?);
        }

        let mut url = base.clone();
        url.set_path(&path);
        url.set_fragment(None);

        // Collect query parameters first
        let mut query_params: Vec<(String, String)> = Vec::new();
        for param in &operation.parameters {
            if param.location == "query" {
                if let Some(value) = args.get(&param.name) {
                    query_params.push((param.name.clone(), param_value(value).into_owned()));
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

/// Characters a path parameter value keeps verbatim: RFC 3986 `unreserved`.
///
/// Everything else is percent-encoded, so a value can never contribute a `/`,
/// `?` or `#` and with them leave its own segment, the query, or the host.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Render an argument the way it goes on the wire: strings bare, everything
/// else as its JSON text.
pub(crate) fn param_value(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(s) => Cow::Borrowed(s),
        other => Cow::Owned(other.to_string()),
    }
}

/// Substitute the `{name}` placeholders in one segment of a path template.
///
/// Each value is encoded as data within the segment. Two values would still
/// escape it after encoding, because `.` is unreserved: a segment that comes
/// out as exactly `.` or `..` is a dot-segment, which URL normalisation
/// resolves against the path before it and so walks out of the operation's
/// route. Those, and empty values (which silently address the parent
/// collection), are refused.
fn expand_path_segment(segment: &str, args: &HashMap<String, Value>) -> Result<String> {
    let mut expanded = String::with_capacity(segment.len());
    let mut last_param = None;
    let mut rest = segment;

    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else {
            break;
        };
        let name = &rest[open + 1..open + len];
        let value = args
            .get(name)
            .ok_or_else(|| OpenApiError::MissingParameter(name.to_string()))?;
        let value = param_value(value);
        if value.is_empty() {
            return Err(OpenApiError::InvalidParameter(
                name.to_string(),
                "path parameters must not be empty".to_string(),
            ));
        }

        expanded.push_str(&rest[..open]);
        expanded.extend(utf8_percent_encode(&value, PATH_SEGMENT));
        last_param = Some(name);
        rest = &rest[open + len + 1..];
    }
    expanded.push_str(rest);

    if let Some(name) = last_param
        && (expanded == "." || expanded == "..")
    {
        return Err(OpenApiError::InvalidParameter(
            name.to_string(),
            format!("`{expanded}` is not a valid path segment"),
        ));
    }

    Ok(expanded)
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

    /// A single-operation spec at `path`, for exercising `build_url`.
    fn provider_for_path(path: &str, base_url: &str) -> OpenApiProvider {
        let spec = json!({
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "paths": { path: { "post": { "responses": { "200": { "description": "ok" } } } } }
        });
        OpenApiProvider::from_string(&spec.to_string())
            .unwrap()
            .with_base_url(base_url)
            .unwrap()
    }

    fn url_for(path: &str, base_url: &str, args: Value) -> Result<Url> {
        let provider = provider_for_path(path, base_url);
        let args: HashMap<String, Value> = serde_json::from_value(args).unwrap();
        provider.build_url(&provider.operations()[0], &args)
    }

    #[test]
    fn test_build_url_keeps_base_path_prefix() {
        for base in ["https://api.example.com/v1", "https://api.example.com/v1/"] {
            let url = url_for("/pets/{id}", base, json!({ "id": 7 })).unwrap();
            assert_eq!(
                url.as_str(),
                "https://api.example.com/v1/pets/7",
                "base {base}"
            );
        }
    }

    #[test]
    fn test_build_url_keeps_base_query() {
        let url = url_for("/pets", "https://api.example.com/v1?key=abc", json!({})).unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1/pets?key=abc");
    }

    #[test]
    fn test_build_url_encodes_path_params_as_one_segment() {
        let url = url_for(
            "/users/{id}/posts",
            "https://api.example.com/v1",
            json!({ "id": "../../admin?x=1#frag" }),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/users/..%2F..%2Fadmin%3Fx%3D1%23frag/posts"
        );
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_build_url_path_param_cannot_swap_host() {
        let url = url_for(
            "/{tenant}/items",
            "https://api.example.com",
            json!({ "tenant": "/evil.example" }),
        )
        .unwrap();
        assert_eq!(url.host_str(), Some("api.example.com"));
        assert_eq!(url.path(), "/%2Fevil.example/items");
    }

    #[test]
    fn test_build_url_rejects_dot_segments_and_empty_values() {
        for id in [".", "..", ""] {
            let result = url_for(
                "/users/{id}",
                "https://api.example.com/v1",
                json!({ "id": id }),
            );
            assert!(
                matches!(result, Err(OpenApiError::InvalidParameter(ref name, _)) if name == "id"),
                "{id:?} gave {result:?}"
            );
        }
        // A dot within a larger segment is just data.
        let url = url_for(
            "/files/{name}.json",
            "https://api.example.com",
            json!({ "name": ".." }),
        )
        .unwrap();
        assert_eq!(url.path(), "/files/...json");
    }

    #[test]
    fn test_ref_resolution_inlines_components() {
        const REF_SPEC: &str = r##"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "paths": {
                "/pets": {
                    "post": {
                        "operationId": "createPet",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Pet" }
                                }
                            }
                        },
                        "responses": { "201": { "description": "ok" } }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "owner": { "$ref": "#/components/schemas/Owner" }
                        }
                    },
                    "Owner": {
                        "type": "object",
                        "properties": {
                            "email": { "type": "string" }
                        }
                    }
                }
            }
        }"##;

        let provider = OpenApiProvider::from_string(REF_SPEC).unwrap();
        let op = provider
            .operations()
            .iter()
            .find(|o| o.operation_id.as_deref() == Some("createPet"))
            .expect("createPet operation");
        let body = op.request_body_schema.as_ref().expect("body schema");
        let props = body.get("properties").expect("properties");
        let owner = props.get("owner").expect("owner property");
        // The owner $ref must have been replaced by the inlined Owner schema.
        assert!(
            owner.get("$ref").is_none(),
            "owner $ref was not inlined: {owner}"
        );
        let owner_props = owner.get("properties").expect("owner inlined properties");
        assert!(owner_props.get("email").is_some());
    }

    #[test]
    fn test_ref_resolution_handles_cycles() {
        const CYCLE_SPEC: &str = r##"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "paths": {
                "/n": {
                    "post": {
                        "operationId": "makeNode",
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
                            "next": { "$ref": "#/components/schemas/Node" }
                        }
                    }
                }
            }
        }"##;

        let provider = OpenApiProvider::from_string(CYCLE_SPEC).unwrap();
        let op = provider
            .operations()
            .iter()
            .find(|o| o.operation_id.as_deref() == Some("makeNode"))
            .unwrap();
        // Must not infinite-loop or panic — resolver should have returned a finite value
        // with the inner cycle preserved as a $ref.
        let body = op.request_body_schema.as_ref().unwrap();
        let next = body.pointer("/properties/next").expect("next property");
        assert_eq!(
            next.get("$ref").and_then(|v| v.as_str()),
            Some("#/components/schemas/Node")
        );
    }

    #[test]
    fn test_base_url_defaults_from_servers() {
        const SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "servers": [
                { "url": "https://api.example.com/v1" }
            ],
            "paths": {}
        }"#;
        let provider = OpenApiProvider::from_string(SPEC).unwrap();
        assert_eq!(
            provider.base_url.as_ref().map(Url::as_str),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn test_base_url_substitutes_server_variables() {
        const SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "servers": [
                {
                    "url": "https://{host}/api",
                    "variables": {
                        "host": { "default": "api.example.com" }
                    }
                }
            ],
            "paths": {}
        }"#;
        let provider = OpenApiProvider::from_string(SPEC).unwrap();
        assert_eq!(
            provider.base_url.as_ref().map(Url::as_str),
            Some("https://api.example.com/api")
        );
    }

    #[test]
    fn test_with_base_url_overrides_servers_default() {
        const SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "servers": [{ "url": "https://default.example.com" }],
            "paths": {}
        }"#;
        let provider = OpenApiProvider::from_string(SPEC)
            .unwrap()
            .with_base_url("https://override.example.com")
            .unwrap();
        assert_eq!(
            provider.base_url.as_ref().map(Url::as_str),
            Some("https://override.example.com/")
        );
    }

    #[test]
    fn test_security_propagated_to_extracted_operation() {
        const SPEC: &str = r#"{
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1.0.0" },
            "security": [{ "globalKey": [] }],
            "components": {
                "securitySchemes": {
                    "globalKey": {
                        "type": "apiKey",
                        "name": "X-API-Key",
                        "in": "header"
                    },
                    "perOpBearer": {
                        "type": "http",
                        "scheme": "bearer"
                    }
                }
            },
            "paths": {
                "/admin": {
                    "post": {
                        "operationId": "adminOp",
                        "security": [{ "perOpBearer": [] }],
                        "responses": { "200": { "description": "ok" } }
                    }
                },
                "/public": {
                    "get": {
                        "operationId": "publicOp",
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }
        }"#;
        let provider = OpenApiProvider::from_string(SPEC).unwrap();

        let admin = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("adminOp"))
            .unwrap();
        assert_eq!(admin.security.len(), 1);
        assert!(admin.security[0].contains_key("perOpBearer"));

        let public = provider
            .operations()
            .iter()
            .find(|op| op.operation_id.as_deref() == Some("publicOp"))
            .unwrap();
        // No operation-level security → falls back to spec-level
        assert_eq!(public.security.len(), 1);
        assert!(public.security[0].contains_key("globalKey"));

        assert_eq!(provider.security_schemes().len(), 2);
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
