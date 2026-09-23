//! `ProxyService` - MCP handler that forwards requests to backend servers.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use turbomcp_protocol::types::{
    PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
};
use turbomcp_protocol::{Error as McpError, Result as McpResult};
use turbomcp_server::{JsonRpcIncoming, McpHandler, RequestContext};

use super::BackendConnector;
use crate::error::{ProxyError, ProxyResult};
use crate::introspection::ServerSpec;

/// Convert a `ProxyError` into an `McpError`. An error the upstream returned
/// comes back unchanged, `data` included; see [`BackendConnector::call_tool`].
fn proxy_error_to_mcp(err: ProxyError) -> McpError {
    err.into()
}

/// Proxy service that forwards MCP requests to a backend server
///
/// This is an ordinary [`McpHandler`], so every frontend (stdio, Streamable
/// HTTP, WebSocket) serves it through `turbomcp-server`'s transports. The
/// handshake, version negotiation, `ping`, notifications, pagination, and
/// JSON-RPC framing are the server stack's, not the proxy's; the proxy only
/// supplies the upstream's catalogue and forwards calls.
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

    /// Upper bound on each forwarded call, if any
    request_timeout: Option<Duration>,
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
            request_timeout: None,
        }
    }

    /// Bound every forwarded call (`tools/call`, `resources/read`,
    /// `prompts/get`) by `timeout`; one that overruns fails with a timeout
    /// error instead of holding the client's request open indefinitely.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Await a forwarded call, applying the configured timeout.
    async fn forward<T>(
        &self,
        operation: &str,
        call: impl Future<Output = ProxyResult<T>>,
    ) -> McpResult<T> {
        let result = match self.request_timeout {
            Some(timeout) => tokio::time::timeout(timeout, call).await.map_err(|_| {
                McpError::timeout(format!("{operation} exceeded {}ms", timeout.as_millis()))
            })?,
            None => call.await,
        };
        result.map_err(proxy_error_to_mcp)
    }

    /// Dispatch one JSON-RPC request through the server router.
    ///
    /// Used by the Tower integration, which works in terms of request and
    /// result values rather than a transport. Routing it through the same
    /// dispatcher the transports use keeps one answer per method: a separate
    /// hand-written match here disagreed with the transports on error codes
    /// (an unknown method came back `-32603` instead of `-32601`) and on
    /// which methods existed at all.
    pub(crate) async fn process_value(&self, request: Value) -> McpResult<Value> {
        let request: JsonRpcIncoming =
            serde_json::from_value(request).map_err(|e| McpError::parse_error(e.to_string()))?;
        let response = turbomcp_server::route_request(self, request, &RequestContext::new()).await;

        match (response.result, response.error) {
            (_, Some(error)) => {
                let mut err = McpError::from_rpc_code(error.code, error.message);
                if let Some(data) = error.data {
                    err = err.with_data(data);
                }
                Err(err)
            }
            (Some(result), None) => Ok(result),
            (None, None) => Ok(Value::Null),
        }
    }
}

/// The capabilities the proxy can honour, given what the upstream declared.
///
/// A capability advertised here is one a client will exercise against the
/// *proxy*, so mirroring the upstream only works for what the proxy relays.
/// It forwards tools, resources, and prompts, and nothing else: no
/// `list_changed` or `resources/updated` notifications, no subscriptions, no
/// `logging/setLevel`, no `completion/complete`, no experimental methods.
/// Advertising any of those promised notifications that never arrive or
/// methods that answer "not found".
fn relayed_capabilities(upstream: &ServerCapabilities) -> ServerCapabilities {
    ServerCapabilities {
        tools: upstream
            .tools
            .as_ref()
            .map(|_| ToolsCapabilities { list_changed: None }),
        resources: upstream.resources.as_ref().map(|_| ResourcesCapabilities {
            subscribe: None,
            list_changed: None,
        }),
        prompts: upstream
            .prompts
            .as_ref()
            .map(|_| PromptsCapabilities { list_changed: None }),
        ..Default::default()
    }
}

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

impl McpHandler for ProxyService {
    fn server_info(&self) -> turbomcp_server::prelude::ServerInfo {
        let upstream = &self.spec.server_info;
        turbomcp_server::prelude::ServerInfo {
            name: format!("{}-proxy", upstream.name),
            version: upstream.version.clone(),
            title: upstream
                .title
                .as_ref()
                .map(|title| format!("{title} Proxy")),
            description: upstream.description.clone(),
            icons: upstream.icons.clone(),
            website_url: upstream.website_url.clone(),
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

    fn server_capabilities(&self) -> ServerCapabilities {
        relayed_capabilities(&self.spec.capabilities)
    }

    fn list_tools(&self) -> Vec<turbomcp_protocol::types::Tool> {
        self.spec.tools.clone()
    }

    fn list_resources(&self) -> Vec<turbomcp_protocol::types::Resource> {
        self.spec.resources.clone()
    }

    fn list_resource_templates(&self) -> Vec<turbomcp_protocol::types::ResourceTemplate> {
        self.spec.resource_templates.clone()
    }

    fn list_prompts(&self) -> Vec<turbomcp_protocol::types::Prompt> {
        self.spec.prompts.clone()
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Value,
        _ctx: &RequestContext,
    ) -> McpResult<turbomcp_server::prelude::ToolResult> {
        let arguments = tool_arguments_from_value(args)?;
        let result = self
            .forward("tools/call", self.backend.call_tool(name, arguments))
            .await?;

        serde_json::from_value(result).map_err(|e| McpError::internal(e.to_string()))
    }

    async fn read_resource(
        &self,
        uri: &str,
        _ctx: &RequestContext,
    ) -> McpResult<turbomcp_server::prelude::ResourceResult> {
        let result = self
            .forward("resources/read", self.backend.read_resource(uri))
            .await?;

        serde_json::to_value(result)
            .and_then(serde_json::from_value)
            .map_err(|e| McpError::internal(e.to_string()))
    }

    async fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
        _ctx: &RequestContext,
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
            .forward("prompts/get", self.backend.get_prompt(name, arguments))
            .await?;

        serde_json::to_value(result)
            .and_then(serde_json::from_value)
            .map_err(|e| McpError::internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use turbomcp_protocol::types::{Icon, Implementation, ResourceTemplate, Tool, ToolExecution};

    async fn service_over(backend: BackendConnector) -> ProxyService {
        let spec = backend.introspect().await.expect("introspection");
        ProxyService::new(backend, spec)
    }

    #[tokio::test]
    async fn test_resource_templates_list_is_forwarded() {
        let template = ResourceTemplate {
            uri_template: "repo://{owner}/{name}".to_string(),
            name: "repo".to_string(),
            title: Some("Repository".to_string()),
            description: Some("Repository metadata".to_string()),
            mime_type: Some("application/json".to_string()),
            ..Default::default()
        };
        let service = service_over(BackendConnector::from_static_data_for_test(
            Vec::new(),
            Vec::new(),
            vec![template],
            Vec::new(),
        ))
        .await;

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

    /// Icons, `execution`, and `_meta` on a tool, and the server's description
    /// and icons, all have to survive the hop. The proxy used to copy entries
    /// into its own snake_case mirrors, which carried only the fields someone
    /// had remembered, so each of these was silently dropped.
    #[tokio::test]
    async fn the_upstream_catalogue_reaches_the_client_losslessly() {
        let icon = Icon {
            src: "https://example.com/icon.png".to_string(),
            mime_type: Some("image/png".to_string()),
            ..Default::default()
        };
        let tool = Tool {
            name: "search".to_string(),
            title: Some("Search".to_string()),
            icons: Some(vec![icon.clone()]),
            execution: Some(ToolExecution::default()),
            meta: Some(HashMap::from([(
                "io.example/owner".to_string(),
                serde_json::json!("search-team"),
            )])),
            ..Default::default()
        };
        let mut backend =
            BackendConnector::from_static_data_for_test(vec![tool], vec![], vec![], vec![]);
        backend.set_server_info_for_test(Implementation {
            name: "upstream".to_string(),
            version: "2.0.0".to_string(),
            description: Some("Searches things".to_string()),
            icons: Some(vec![icon]),
            website_url: Some("https://example.com".to_string()),
            ..Default::default()
        });
        let service = service_over(backend).await;

        let listed =
            serde_json::to_value(service.list_tools()).expect("tools serialize to the wire shape");
        assert_eq!(listed[0]["icons"][0]["src"], "https://example.com/icon.png");
        assert!(listed[0].get("execution").is_some(), "execution: {listed}");
        assert_eq!(listed[0]["_meta"]["io.example/owner"], "search-team");

        let info = service.server_info();
        assert_eq!(info.name, "upstream-proxy");
        assert_eq!(info.description.as_deref(), Some("Searches things"));
        assert_eq!(info.icons.as_ref().map(Vec::len), Some(1));
        assert_eq!(info.website_url.as_deref(), Some("https://example.com"));
    }

    /// The proxy relays none of the list-changed or resource-updated
    /// notifications, subscriptions, logging, completions, or experimental
    /// methods, so it must not advertise them even when the upstream does.
    #[tokio::test]
    async fn only_relayed_capabilities_are_advertised() {
        let upstream = ServerCapabilities {
            tools: Some(ToolsCapabilities {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapabilities {
                subscribe: Some(true),
                list_changed: Some(true),
            }),
            prompts: Some(PromptsCapabilities {
                list_changed: Some(true),
            }),
            logging: Some(Default::default()),
            completions: Some(Default::default()),
            experimental: Some(HashMap::from([(
                "io.example/feature".to_string(),
                serde_json::json!({}),
            )])),
            ..Default::default()
        };

        let advertised = serde_json::to_value(relayed_capabilities(&upstream)).expect("caps");
        assert_eq!(
            advertised,
            serde_json::json!({ "tools": {}, "resources": {}, "prompts": {} })
        );
    }
}
