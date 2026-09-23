//! REST API adapter for MCP servers
//!
//! Exposes MCP server capabilities as a `RESTful` HTTP API with `OpenAPI` documentation.
//! Automatically generates REST endpoints from introspected tool and resource definitions.

// Always-available imports (stdlib + core dependencies)
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, info};

// Core proxy types
use crate::error::{ProxyError, ProxyResult};

// Feature-gated imports (only if rest feature is enabled)
#[cfg(feature = "rest")]
use crate::introspection::ServerSpec;
#[cfg(feature = "rest")]
use crate::proxy::BackendConnector;
#[cfg(feature = "rest")]
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};

/// REST adapter configuration
#[derive(Debug, Clone)]
pub struct RestAdapterConfig {
    /// Bind address (e.g., "127.0.0.1:3001")
    pub bind: String,
    /// Enable `OpenAPI` Swagger UI
    pub openapi_ui: bool,
}

impl RestAdapterConfig {
    /// Create a new REST adapter configuration
    pub fn new(bind: impl Into<String>, openapi_ui: bool) -> Self {
        Self {
            bind: bind.into(),
            openapi_ui,
        }
    }
}

/// REST adapter state
#[cfg(feature = "rest")]
#[derive(Clone)]
struct RestAdapterState {
    backend: BackendConnector, // Used for routing tool calls, resource reads, prompt gets
    spec: Arc<ServerSpec>,
}

/// REST API adapter for MCP servers
#[cfg(feature = "rest")]
pub struct RestAdapter {
    config: RestAdapterConfig,
    backend: BackendConnector,
    spec: ServerSpec,
}

#[cfg(feature = "rest")]
impl RestAdapter {
    /// Create a new REST adapter
    #[must_use]
    pub fn new(config: RestAdapterConfig, backend: BackendConnector, spec: ServerSpec) -> Self {
        Self {
            config,
            backend,
            spec,
        }
    }

    /// Run the REST adapter server
    ///
    /// # Errors
    ///
    /// Returns error if binding fails or server encounters fatal error
    pub async fn run(self) -> ProxyResult<()> {
        info!("Starting REST adapter on {}", self.config.bind);

        let router = router(RestAdapterState {
            backend: self.backend,
            spec: Arc::new(self.spec),
        });

        // Note: Full Swagger UI integration requires utoipa-swagger-ui feature
        if self.config.openapi_ui {
            info!("OpenAPI specification available at /openapi.json");
            info!("Full Swagger UI integration requires utoipa-swagger-ui crate");
        }

        // Parse bind address
        let listener = tokio::net::TcpListener::bind(&self.config.bind)
            .await
            .map_err(|e| {
                ProxyError::backend_connection(format!(
                    "Failed to bind REST adapter to {}: {}",
                    self.config.bind, e
                ))
            })?;

        info!("REST adapter listening on {}", self.config.bind);

        // Start server
        axum::serve(listener, router)
            .await
            .map_err(|e| ProxyError::backend(format!("REST adapter server error: {e}")))?;

        Ok(())
    }
}

/// The adapter's routes.
///
/// Path parameters use axum 0.8's `{name}` syntax. The `:name` form this used
/// to have is axum 0.7's, and 0.8 panics on it when the route is registered,
/// so the adapter crashed on startup. A resource URI takes the rest of the
/// path (`{*uri}`), since URIs contain slashes.
#[cfg(feature = "rest")]
fn router(state: RestAdapterState) -> Router {
    Router::new()
        .route("/api/tools", get(list_tools).post(call_tool))
        .route("/api/tools/{name}", post(call_tool_by_name))
        .route("/api/resources", get(list_resources))
        .route("/api/resources/{*uri}", get(read_resource))
        .route("/api/prompts", get(list_prompts))
        .route("/api/prompts/{name}", post(get_prompt))
        .route("/openapi.json", get(openapi_spec))
        .route("/health", get(health_check))
        .with_state(state)
}

// ============ REST Endpoint Handlers ============

/// Health check endpoint
#[cfg(feature = "rest")]
async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "turbomcp-rest-adapter"
    }))
}

/// List all tools
#[cfg(feature = "rest")]
async fn list_tools(State(state): State<RestAdapterState>) -> impl IntoResponse {
    debug!("GET /api/tools");

    let tools: Vec<Value> = state
        .spec
        .tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect();

    Json(json!({
        "tools": tools,
        "count": tools.len()
    }))
}

/// Call a tool (generic endpoint with tool name in body)
#[cfg(feature = "rest")]
async fn call_tool(
    State(state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/tools with payload: {}", payload);

    // Extract tool name and arguments from payload
    let Some(tool_name) = payload.get("name").and_then(|v| v.as_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Missing required field 'name' in request body",
                "code": -32602
            })),
        );
    };

    let arguments = payload
        .get("arguments")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<std::collections::HashMap<String, Value>>()
        });

    // Call the backend
    match state.backend.call_tool(tool_name, arguments).await {
        Ok(result) => (StatusCode::OK, Json(json!({ "result": result }))),
        Err(e) => {
            // Mirror the upstream JSON-RPC code when known instead of always
            // collapsing to -32603. A `Method not found` from upstream now
            // surfaces as `-32601` here (not `-32603`), letting frontend
            // retry/decision logic key off codes correctly.
            let code = e.upstream_jsonrpc_code().unwrap_or(-32603);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Tool call failed: {e}"),
                    "code": code,
                })),
            )
        }
    }
}

/// Call a specific tool by name
#[cfg(feature = "rest")]
async fn call_tool_by_name(
    Path(name): Path<String>,
    State(state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/tools/{} with payload: {}", name, payload);

    let arguments = payload.as_object().map(|obj| {
        obj.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::HashMap<String, Value>>()
    });

    // Call the backend
    match state.backend.call_tool(&name, arguments).await {
        Ok(result) => (StatusCode::OK, Json(json!({ "result": result }))),
        Err(e) => {
            let code = e.upstream_jsonrpc_code().unwrap_or(-32603);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Tool call failed: {e}"),
                    "tool": name,
                    "code": code,
                })),
            )
        }
    }
}

/// List all resources
#[cfg(feature = "rest")]
async fn list_resources(State(state): State<RestAdapterState>) -> impl IntoResponse {
    debug!("GET /api/resources");

    let resources: Vec<Value> = state
        .spec
        .resources
        .iter()
        .map(|res| {
            json!({
                "uri": res.uri,
                "name": res.name,
                "description": res.description,
                "mime_type": res.mime_type,
            })
        })
        .collect();

    Json(json!({
        "resources": resources,
        "count": resources.len()
    }))
}

/// Read a specific resource
#[cfg(feature = "rest")]
async fn read_resource(
    Path(uri): Path<String>,
    State(state): State<RestAdapterState>,
) -> impl IntoResponse {
    debug!("GET /api/resources/{}", uri);

    // Call the backend
    match state.backend.read_resource(&uri).await {
        Ok(result) => (StatusCode::OK, Json(json!({ "contents": result.contents }))),
        Err(e) => {
            let code = e.upstream_jsonrpc_code().unwrap_or(-32603);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Resource read failed: {e}"),
                    "uri": uri,
                    "code": code,
                })),
            )
        }
    }
}

/// List all prompts
#[cfg(feature = "rest")]
async fn list_prompts(State(state): State<RestAdapterState>) -> impl IntoResponse {
    debug!("GET /api/prompts");

    let prompts: Vec<Value> = state
        .spec
        .prompts
        .iter()
        .map(|prompt| {
            json!({
                "name": prompt.name,
                "description": prompt.description,
                "arguments": prompt.arguments,
            })
        })
        .collect();

    Json(json!({
        "prompts": prompts,
        "count": prompts.len()
    }))
}

/// Get a specific prompt
#[cfg(feature = "rest")]
async fn get_prompt(
    Path(name): Path<String>,
    State(state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/prompts/{} with payload: {}", name, payload);

    let arguments = payload
        .get("arguments")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<std::collections::HashMap<String, Value>>()
        });

    // Call the backend
    match state.backend.get_prompt(&name, arguments).await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({
                "description": result.description,
                "messages": result.messages
            })),
        ),
        Err(e) => {
            let code = e.upstream_jsonrpc_code().unwrap_or(-32603);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Prompt get failed: {e}"),
                    "prompt": name,
                    "code": code,
                })),
            )
        }
    }
}

/// `OpenAPI` specification endpoint
#[cfg(feature = "rest")]
async fn openapi_spec(State(_state): State<RestAdapterState>) -> impl IntoResponse {
    debug!("GET /openapi.json");

    let openapi = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "MCP REST API",
            "version": "1.0.0",
            "description": "REST API adapter for MCP servers"
        },
        "servers": [
            {
                "url": "http://localhost",
                "description": "Development server"
            }
        ],
        "paths": {
            "/api/tools": {
                "get": {
                    "summary": "List all tools",
                    "responses": {
                        "200": {
                            "description": "List of available tools"
                        }
                    }
                }
            },
            "/api/resources": {
                "get": {
                    "summary": "List all resources",
                    "responses": {
                        "200": {
                            "description": "List of available resources"
                        }
                    }
                }
            },
            "/api/prompts": {
                "get": {
                    "summary": "List all prompts",
                    "responses": {
                        "200": {
                            "description": "List of available prompts"
                        }
                    }
                }
            },
            "/health": {
                "get": {
                    "summary": "Health check",
                    "responses": {
                        "200": {
                            "description": "Service is healthy"
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Tool": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "input_schema": { "type": "object" }
                    }
                },
                "Resource": {
                    "type": "object",
                    "properties": {
                        "uri": { "type": "string" },
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "mime_type": { "type": "string" }
                    }
                }
            }
        }
    });

    Json(openapi)
}

#[cfg(not(feature = "rest"))]
/// Placeholder when REST feature is disabled
pub struct RestAdapter;

#[cfg(not(feature = "rest"))]
impl RestAdapter {
    /// Create a new REST adapter (stub)
    pub fn new(
        _config: RestAdapterConfig,
        _backend: crate::proxy::BackendConnector,
        _spec: crate::introspection::ServerSpec,
    ) -> Self {
        Self
    }

    /// Run the REST adapter server (stub)
    pub async fn run(self) -> ProxyResult<()> {
        Err(ProxyError::configuration(
            "REST adapter requires 'rest' feature to be enabled",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "rest")]
    use super::*;

    #[test]
    #[cfg(feature = "rest")]
    fn test_rest_adapter_config() {
        let config = RestAdapterConfig::new("127.0.0.1:3001", true);
        assert_eq!(config.bind, "127.0.0.1:3001");
        assert!(config.openapi_ui);
    }

    /// axum 0.8 panics while registering a route that uses the 0.7 `:name`
    /// syntax, so the adapter used to die before it served anything. Build
    /// the real router and send it a request on each parameterised route.
    #[tokio::test]
    #[cfg(feature = "rest")]
    async fn the_router_builds_and_routes_path_parameters() {
        use tower::ServiceExt;

        let backend = BackendConnector::from_static_data_for_test(
            vec![turbomcp_protocol::types::Tool {
                name: "echo".to_string(),
                ..Default::default()
            }],
            vec![],
            vec![],
            vec![],
        );
        let spec = backend.introspect().await.expect("introspection");
        let app = router(RestAdapterState {
            backend,
            spec: Arc::new(spec),
        });

        let call = axum::http::Request::post("/api/tools/echo")
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let response = app.clone().oneshot(call).await.unwrap();
        // The static test backend rejects every call; what matters is that
        // the request reached the handler with the tool name bound.
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["tool"], "echo");

        let read = axum::http::Request::get("/api/resources/file:///etc/motd")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(read).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["uri"], "file:///etc/motd");
    }
}
