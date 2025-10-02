//! Axum router extension trait for MCP integration
//!
//! This module provides the AxumMcpExt trait which extends Axum routers
//! with MCP capabilities while preserving existing application state.

#[cfg(feature = "http")]
use axum::Router;

#[cfg(feature = "http")]
use crate::axum::{McpServerConfig, McpService};

#[cfg(feature = "http")]
/// Extension trait for adding MCP capabilities to Axum routers
///
/// This trait provides several methods for integrating MCP services:
/// - Direct integration with existing routers
/// - State-preserving merge capabilities for complex applications
/// - Opinionated defaults for rapid development
pub trait AxumMcpExt {
    /// Add MCP routes to an existing router with custom configuration
    ///
    /// This method integrates MCP routes directly into an existing router,
    /// applying the specified configuration for security, rate limiting, etc.
    ///
    /// # Arguments
    ///
    /// * `service` - The MCP service implementation
    /// * `config` - Server configuration for middleware and security
    fn turbo_mcp_routes_with_config<T: McpService + 'static>(
        self,
        service: T,
        config: McpServerConfig,
    ) -> Self
    where
        Self: Sized;

    /// Add MCP routes to an existing router with default configuration
    ///
    /// Convenience method that uses development-friendly defaults.
    /// For production use, prefer `turbo_mcp_routes_with_config`.
    fn turbo_mcp_routes<T: McpService + 'static>(self, service: T) -> Self
    where
        Self: Sized,
    {
        self.turbo_mcp_routes_with_config(service, McpServerConfig::default())
    }

    /// Create a complete MCP server with opinionated defaults
    ///
    /// Creates a new router with only MCP routes and development defaults.
    /// This is the quickest way to get a minimal MCP server running.
    fn turbo_mcp_server<T: McpService + 'static>(service: T) -> Router {
        Router::<()>::new().turbo_mcp_routes(service)
    }

    /// Create a complete MCP server with custom configuration
    ///
    /// Creates a new router with only MCP routes and the specified configuration.
    /// Use this for production deployments with specific security requirements.
    fn turbo_mcp_server_with_config<T: McpService + 'static>(
        service: T,
        config: McpServerConfig,
    ) -> Router {
        Router::<()>::new().turbo_mcp_routes_with_config(service, config)
    }

    /// Create an MCP router that preserves your state when merged (PRODUCTION-GRADE ENHANCEMENT)
    ///
    /// This method creates a stateless MCP router that can be merged with any stateful router
    /// without losing the original state. This is the cleanest way to add MCP capabilities
    /// to existing applications.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use axum::{Router, routing::get};
    /// use turbomcp_transport::{AxumMcpExt, McpService, McpServerConfig};
    ///
    /// async fn list_users() -> &'static str { "users" }
    ///
    /// #[derive(Clone)]
    /// struct AppState;
    ///
    /// let rest_router = Router::new()
    ///     .route("/api/users", get(list_users))
    ///     .with_state(app_state);
    ///
    /// let mcp_router = Router::turbo_mcp_routes_for_merge(
    ///     my_mcp_service,
    ///     McpServerConfig::default()
    /// );
    ///
    /// let combined = rest_router.merge(mcp_router);  // State is preserved!
    /// ```
    fn turbo_mcp_routes_for_merge<T: McpService + 'static>(
        service: T,
        config: McpServerConfig,
    ) -> Router {
        Self::turbo_mcp_server_with_config(service, config)
    }

    /// Create an MCP router for merging with default configuration
    ///
    /// Convenience method for state-preserving merge with development defaults.
    fn turbo_mcp_routes_for_merge_default<T: McpService + 'static>(service: T) -> Router {
        Self::turbo_mcp_routes_for_merge(service, McpServerConfig::default())
    }
}
