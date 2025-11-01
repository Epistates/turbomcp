//! REST API adapter for MCP servers
//!
//! Exposes MCP server capabilities as a RESTful HTTP API with OpenAPI documentation.
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
    /// Enable OpenAPI Swagger UI
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
    backend: BackendConnector,
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

        let state = RestAdapterState {
            backend: self.backend,
            spec: Arc::new(self.spec),
        };

        // Build router with OpenAPI routes
        let router = Router::new()
            .route("/api/tools", get(list_tools).post(call_tool))
            .route("/api/tools/:name", post(call_tool_by_name))
            .route("/api/resources", get(list_resources))
            .route("/api/resources/:uri", get(read_resource))
            .route("/api/prompts", get(list_prompts))
            .route("/api/prompts/:name", post(get_prompt))
            .route("/openapi.json", get(openapi_spec))
            .route("/health", get(health_check))
            .with_state(state);

        // Add Swagger UI if enabled
        if self.config.openapi_ui {
            info!("OpenAPI Swagger UI enabled at /docs");
            // Note: Swagger UI integration requires utoipa-swagger-ui feature
            // This is a placeholder for full implementation
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
            .map_err(|e| ProxyError::backend(format!("REST adapter server error: {}", e)))?;

        Ok(())
    }
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
    State(_state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/tools with payload: {}", payload);

    // This is a placeholder implementation
    // Full implementation would:
    // 1. Extract tool name and arguments from payload
    // 2. Route to backend via BackendConnector
    // 3. Translate message IDs via IdTranslator
    // 4. Return response

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Tool call routing not yet fully implemented",
            "code": -32603
        })),
    )
}

/// Call a specific tool by name
#[cfg(feature = "rest")]
async fn call_tool_by_name(
    Path(name): Path<String>,
    State(_state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/tools/{} with payload: {}", name, payload);

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Tool call routing not yet fully implemented",
            "tool": name,
            "code": -32603
        })),
    )
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
    State(_state): State<RestAdapterState>,
) -> impl IntoResponse {
    debug!("GET /api/resources/{}", uri);

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Resource reading not yet fully implemented",
            "uri": uri,
            "code": -32603
        })),
    )
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
    State(_state): State<RestAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("POST /api/prompts/{} with payload: {}", name, payload);

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Prompt execution not yet fully implemented",
            "prompt": name,
            "code": -32603
        })),
    )
}

/// OpenAPI specification endpoint
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
}
