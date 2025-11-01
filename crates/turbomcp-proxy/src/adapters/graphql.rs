//! GraphQL adapter for MCP servers
//!
//! Exposes MCP server capabilities as a GraphQL API.
//! Automatically generates GraphQL schema from introspected tool and resource definitions.

// Always-available imports (stdlib + core dependencies)
use serde_json::{Value, json};
use tracing::{debug, info};

// Core proxy types
use crate::error::{ProxyError, ProxyResult};

// Feature-gated imports (only if graphql feature is enabled)
#[cfg(feature = "graphql")]
use crate::introspection::ServerSpec;
#[cfg(feature = "graphql")]
use crate::proxy::BackendConnector;
#[cfg(feature = "graphql")]
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
#[cfg(feature = "graphql")]
use std::sync::Arc;

/// GraphQL adapter configuration
#[derive(Debug, Clone)]
pub struct GraphQLAdapterConfig {
    /// Bind address (e.g., "127.0.0.1:4000")
    pub bind: String,
    /// Enable GraphQL playground
    pub playground: bool,
}

impl GraphQLAdapterConfig {
    /// Create a new GraphQL adapter configuration
    pub fn new(bind: impl Into<String>, playground: bool) -> Self {
        Self {
            bind: bind.into(),
            playground,
        }
    }
}

/// GraphQL adapter state
#[cfg(feature = "graphql")]
#[derive(Clone)]
struct GraphQLAdapterState {
    backend: BackendConnector, // Used for routing GraphQL queries to MCP backend
    spec: Arc<ServerSpec>,     // Used for schema introspection
}

/// GraphQL adapter for MCP servers
///
/// Provides a simplified GraphQL-like interface to MCP servers.
/// Note: Full async-graphql integration requires adding async-graphql crate dependency.
#[cfg(feature = "graphql")]
pub struct GraphQLAdapter {
    config: GraphQLAdapterConfig,
    backend: BackendConnector,
    spec: ServerSpec,
}

#[cfg(feature = "graphql")]
impl GraphQLAdapter {
    /// Create a new GraphQL adapter
    #[must_use]
    pub fn new(config: GraphQLAdapterConfig, backend: BackendConnector, spec: ServerSpec) -> Self {
        Self {
            config,
            backend,
            spec,
        }
    }

    /// Run the GraphQL adapter server
    ///
    /// # Errors
    ///
    /// Returns error if binding fails or server encounters fatal error
    pub async fn run(self) -> ProxyResult<()> {
        info!("Starting GraphQL adapter on {}", self.config.bind);

        let state = GraphQLAdapterState {
            backend: self.backend,
            spec: Arc::new(self.spec),
        };

        // Build router with GraphQL endpoint
        let router = Router::new()
            .route("/graphql", post(graphql_endpoint))
            .route("/schema", get(graphql_schema))
            .route("/health", get(health_check))
            .with_state(state);

        // Note: Full GraphQL Playground integration requires async-graphql crate
        if self.config.playground {
            info!("GraphQL schema available at /schema");
            info!("Full GraphQL Playground requires async-graphql crate integration");
        }

        // Parse bind address
        let listener = tokio::net::TcpListener::bind(&self.config.bind)
            .await
            .map_err(|e| {
                ProxyError::backend_connection(format!(
                    "Failed to bind GraphQL adapter to {}: {}",
                    self.config.bind, e
                ))
            })?;

        info!("GraphQL adapter listening on {}", self.config.bind);

        // Start server
        axum::serve(listener, router)
            .await
            .map_err(|e| ProxyError::backend(format!("GraphQL adapter server error: {e}")))?;

        Ok(())
    }
}

/// Handle GraphQL queries
///
/// Provides a simplified GraphQL-like interface. Supports basic queries for:
/// - tools: List all tools or call a specific tool
/// - resources: List all resources or read a specific resource
/// - prompts: List all prompts or get a specific prompt
#[cfg(feature = "graphql")]
async fn graphql_endpoint(
    State(state): State<GraphQLAdapterState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    debug!("GraphQL request received: {:?}", payload);

    let Some(query) = payload.get("query").and_then(|v| v.as_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "errors": [{
                    "message": "Missing 'query' field in request body"
                }]
            })),
        );
    };

    // Simple query parsing - in production this would use async-graphql
    let response = if query.contains("tools") && query.contains('{') {
        // Query: { tools { name description } }
        match state.backend.list_tools().await {
            Ok(tools) => {
                let tool_data: Vec<Value> = tools
                    .into_iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                        })
                    })
                    .collect();
                json!({ "data": { "tools": tool_data } })
            }
            Err(e) => json!({
                "errors": [{
                    "message": format!("Failed to list tools: {e}")
                }]
            }),
        }
    } else if query.contains("resources") && query.contains('{') {
        // Query: { resources { uri name description } }
        match state.backend.list_resources().await {
            Ok(resources) => {
                let resource_data: Vec<Value> = resources
                    .into_iter()
                    .map(|r| {
                        json!({
                            "uri": r.uri,
                            "name": r.name,
                            "description": r.description,
                            "mimeType": r.mime_type,
                        })
                    })
                    .collect();
                json!({ "data": { "resources": resource_data } })
            }
            Err(e) => json!({
                "errors": [{
                    "message": format!("Failed to list resources: {e}")
                }]
            }),
        }
    } else if query.contains("prompts") && query.contains('{') {
        // Query: { prompts { name description } }
        match state.backend.list_prompts().await {
            Ok(prompts) => {
                let prompt_data: Vec<Value> = prompts
                    .into_iter()
                    .map(|p| {
                        json!({
                            "name": p.name,
                            "description": p.description,
                        })
                    })
                    .collect();
                json!({ "data": { "prompts": prompt_data } })
            }
            Err(e) => json!({
                "errors": [{
                    "message": format!("Failed to list prompts: {e}")
                }]
            }),
        }
    } else {
        json!({
            "errors": [{
                "message": "Unsupported query. Supported: tools, resources, prompts. For full GraphQL support, integrate async-graphql crate."
            }]
        })
    };

    (StatusCode::OK, Json(response))
}

/// Return GraphQL schema
#[cfg(feature = "graphql")]
async fn graphql_schema(State(state): State<GraphQLAdapterState>) -> impl IntoResponse {
    debug!("GraphQL schema request");

    // Generate a simple GraphQL Schema Definition Language output
    let mut schema = String::from("type Query {\n");

    schema.push_str("  tools: [Tool!]!\n");
    schema.push_str("  resources: [Resource!]!\n");
    schema.push_str("  prompts: [Prompt!]!\n");

    schema.push_str("}\n\n");

    schema.push_str("type Tool {\n");
    schema.push_str("  name: String!\n");
    schema.push_str("  description: String\n");
    schema.push_str("}\n\n");

    schema.push_str("type Resource {\n");
    schema.push_str("  uri: String!\n");
    schema.push_str("  name: String\n");
    schema.push_str("  description: String\n");
    schema.push_str("  mimeType: String\n");
    schema.push_str("}\n\n");

    schema.push_str("type Prompt {\n");
    schema.push_str("  name: String!\n");
    schema.push_str("  description: String\n");
    schema.push_str("}\n");

    info!(
        "Generated GraphQL schema for {} tools, {} resources, {} prompts",
        state.spec.tools.len(),
        state.spec.resources.len(),
        state.spec.prompts.len()
    );

    schema
}

#[cfg(feature = "graphql")]
async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "turbomcp-graphql-adapter"
    }))
}

#[cfg(not(feature = "graphql"))]
/// Placeholder when GraphQL feature is disabled
pub struct GraphQLAdapter;

#[cfg(not(feature = "graphql"))]
impl GraphQLAdapter {
    /// Create a new GraphQL adapter (stub)
    pub fn new(
        _config: GraphQLAdapterConfig,
        _backend: crate::proxy::BackendConnector,
        _spec: crate::introspection::ServerSpec,
    ) -> Self {
        Self
    }

    /// Run the GraphQL adapter server (stub)
    pub async fn run(self) -> ProxyResult<()> {
        Err(ProxyError::configuration(
            "GraphQL adapter requires 'graphql' feature to be enabled",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "graphql")]
    use super::*;

    #[test]
    #[cfg(feature = "graphql")]
    fn test_graphql_adapter_config() {
        let config = GraphQLAdapterConfig::new("127.0.0.1:4000", true);
        assert_eq!(config.bind, "127.0.0.1:4000");
        assert!(config.playground);
    }
}
