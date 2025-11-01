//! GraphQL adapter for MCP servers
//!
//! Exposes MCP server capabilities as a GraphQL API.
//! Automatically generates GraphQL schema from introspected tool and resource definitions.

// Always-available imports (stdlib + core dependencies)
use serde_json::json;
use tracing::{debug, info};

// Core proxy types
use crate::error::{ProxyError, ProxyResult};

// Feature-gated imports (only if graphql feature is enabled)
#[cfg(feature = "graphql")]
use crate::introspection::ServerSpec;
#[cfg(feature = "graphql")]
use crate::proxy::BackendConnector;
#[cfg(feature = "graphql")]
use axum::{Json, Router, response::IntoResponse, routing::get};

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

/// GraphQL adapter for MCP servers
#[cfg(feature = "graphql")]
pub struct GraphQLAdapter {
    config: GraphQLAdapterConfig,
    backend: BackendConnector,
    spec: ServerSpec,
}

#[cfg(feature = "graphql")]
impl GraphQLAdapter {
    /// Create a new GraphQL adapter
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

        // Build router with placeholder health check
        async fn graphql_endpoint(_body: String) -> Json<serde_json::Value> {
            debug!("GraphQL request received");
            // This is a simplified placeholder
            // Full implementation would use async_graphql_axum middleware
            Json(json!({
                "data": null,
                "errors": [{
                    "message": "GraphQL routing not yet fully implemented (awaiting async-graphql integration)"
                }]
            }))
        }

        let router = Router::new()
            .route("/graphql", axum::routing::post(graphql_endpoint))
            .route("/health", get(health_check));

        // Add playground if enabled
        if self.config.playground {
            info!("GraphQL Playground enabled at /playground (awaiting async-graphql integration)");
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
            .map_err(|e| ProxyError::backend(format!("GraphQL adapter server error: {}", e)))?;

        Ok(())
    }
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
