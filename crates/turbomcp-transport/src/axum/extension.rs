//! AxumMcpExt extension trait for adding MCP routes to Axum routers

use std::sync::Arc;
use std::time::Duration;

use axum::{
    http::Method,
    middleware,
    routing::{get, post},
    Router,
};
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::axum::{
    config::{CorsConfig, McpServerConfig},
    handlers::*,
    middleware::*,
    types::{McpAppState, McpService},
};
use turbomcp_core::SessionManager;

/// Axum integration extension trait
///
/// This trait provides convenient methods for adding MCP routes to existing
/// Axum routers with various configuration options. It supports both simple
/// integration and advanced state-preserving patterns for existing applications.
pub trait AxumMcpExt {
    /// Add MCP routes to an existing router with custom configuration
    fn turbo_mcp_routes_with_config<T: McpService + 'static>(
        self,
        service: T,
        config: McpServerConfig,
    ) -> Self
    where
        Self: Sized;

    /// Add MCP routes to an existing router with default configuration
    fn turbo_mcp_routes<T: McpService + 'static>(self, service: T) -> Self
    where
        Self: Sized,
    {
        self.turbo_mcp_routes_with_config(service, McpServerConfig::default())
    }

    /// Create a complete MCP server with opinionated defaults
    fn turbo_mcp_server<T: McpService + 'static>(service: T) -> Router {
        Router::<()>::new().turbo_mcp_routes(service)
    }

    /// Create a complete MCP server with custom configuration
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
    /// ```rust,ignore
    /// # use axum::{Router, routing::get};
    /// # use turbomcp_transport::{AxumMcpExt, McpService, McpServerConfig, SessionInfo};
    /// # async fn list_users() -> &'static str { "users" }
    /// # #[derive(Clone)]
    /// # struct AppState;
    /// # struct MyMcpService;
    /// # impl McpService for MyMcpService {
    /// #     async fn process_request(
    /// #         &self,
    /// #         _request: serde_json::Value,
    /// #         _session: &SessionInfo,
    /// #     ) -> turbomcp_core::Result<serde_json::Value> {
    /// #         Ok(serde_json::json!({}))
    /// #     }
    /// # }
    /// # let app_state = AppState;
    /// # let my_mcp_service = MyMcpService;
    /// let rest_router = Router::new()
    ///     .route("/api/users", get(list_users))
    ///     .with_state(app_state);
    ///
    /// let mcp_router = Router::turbo_mcp_routes_for_merge(my_mcp_service, McpServerConfig::default());
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
    fn turbo_mcp_routes_for_merge_default<T: McpService + 'static>(service: T) -> Router {
        Self::turbo_mcp_routes_for_merge(service, McpServerConfig::default())
    }
}

impl<S> AxumMcpExt for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn turbo_mcp_routes_with_config<T: McpService + 'static>(
        self,
        service: T,
        config: McpServerConfig,
    ) -> Router<S> {
        let session_manager = Arc::new(SessionManager::with_config(
            Duration::from_secs(300), // 5 minute session timeout
            config.max_connections,
        ));

        let (sse_sender, _) = broadcast::channel(1000);

        let app_state = McpAppState {
            service: Arc::new(service) as Arc<dyn McpService>,
            session_manager,
            sse_sender,
            config: config.clone(),
        };

        // Create new router with MCP routes and state
        let mcp_router = Router::new()
            .route("/mcp", post(json_rpc_handler))
            .route("/mcp/capabilities", get(capabilities_handler))
            .route("/mcp/sse", get(sse_handler))
            .route("/mcp/ws", get(websocket_handler))
            .route("/mcp/health", get(health_handler))
            .route("/mcp/metrics", get(metrics_handler))
            .with_state(app_state);

        // Merge with existing router
        let router = self.merge(mcp_router);

        // Apply proven middleware stack
        apply_middleware(router, &config)
    }
}

/// Apply comprehensive middleware stack based on configuration
fn apply_middleware<S>(router: Router<S>, config: &McpServerConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut router = router;

    // 1. Basic MCP middleware (always applied)
    router = router.layer(middleware::from_fn(mcp_middleware));

    // 2. Security headers (applied based on config and environment)
    if config.security.enabled {
        router = router.layer(middleware::from_fn_with_state(
            config.security.clone(),
            security_headers_middleware,
        ));
    }

    // 3. Rate limiting (applied if enabled)
    if config.rate_limiting.enabled {
        router = router.layer(middleware::from_fn_with_state(
            config.rate_limiting.clone(),
            rate_limiting_middleware,
        ));
    }

    // 4. Authentication (applied if configured)
    if let Some(auth_config) = &config.auth
        && auth_config.enabled
    {
        router = router.layer(middleware::from_fn_with_state(
            auth_config.clone(),
            authentication_middleware,
        ));
    }

    // 5. CORS (applied based on configuration)
    if config.cors.enabled {
        router = router.layer(build_cors_layer(&config.cors));
    }

    // 6. Compression (applied if enabled)
    if config.enable_compression {
        router = router.layer(CompressionLayer::new());
    }

    // 7. Request tracing (applied if enabled)
    if config.enable_tracing {
        router = router.layer(TraceLayer::new_for_http());
    }

    // 8. Timeout (always applied for reliability)
    router = router.layer(TimeoutLayer::new(config.request_timeout));

    router
}

/// Build CORS layer from configuration
fn build_cors_layer(cors_config: &CorsConfig) -> CorsLayer {
    let mut cors = CorsLayer::new();

    // Configure allowed methods
    if !cors_config.allowed_methods.is_empty() {
        let methods: Vec<Method> = cors_config
            .allowed_methods
            .iter()
            .filter_map(|m| m.parse().ok())
            .collect();
        cors = cors.allow_methods(methods);
    }

    // Configure allowed origins
    match &cors_config.allowed_origins {
        Some(origins) if origins.contains(&"*".to_string()) => {
            cors = cors.allow_origin(Any);
        }
        Some(origins) => {
            let origin_list: Vec<_> = origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            cors = cors.allow_origin(origin_list);
        }
        None => {
            // Default to any origin if not specified
            cors = cors.allow_origin(Any);
        }
    }

    // Configure allowed headers
    if !cors_config.allowed_headers.is_empty() {
        let headers: Vec<_> = cors_config
            .allowed_headers
            .iter()
            .filter_map(|h| h.parse().ok())
            .collect();
        cors = cors.allow_headers(headers);
    }

    // Configure credentials
    if cors_config.allow_credentials {
        cors = cors.allow_credentials(true);
    }

    // Configure max age
    if let Some(max_age) = cors_config.max_age {
        cors = cors.max_age(max_age);
    }

    cors
}