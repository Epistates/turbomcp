//! Router builder implementation
//!
//! This module contains the actual implementation of the AxumMcpExt trait for Router,
//! providing the functionality to add MCP capabilities to Axum applications.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer, trace::TraceLayer};

use crate::axum::config::McpServerConfig;
use crate::axum::handlers::{
    capabilities_handler, health_handler, json_rpc_handler, metrics_handler, sse_handler,
    websocket_handler,
};
use crate::axum::router::AxumMcpExt;
use crate::axum::service::{McpAppState, McpService};
use crate::tower::{SessionInfo, SessionManager};

#[cfg(any(feature = "auth", feature = "jwt-validation"))]
use crate::axum::middleware::authentication_middleware;

/// Session middleware - adds session tracking to all requests
async fn session_middleware(
    mut request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    // Create new session for this request
    let mut session = SessionInfo::new();

    // Extract headers and store in session metadata
    for (name, value) in request.headers().iter() {
        if let Ok(value_str) = value.to_str() {
            session
                .metadata
                .insert(name.to_string(), value_str.to_string());
        }
    }

    // Extract specific useful headers
    if let Some(user_agent) = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
    {
        session.user_agent = Some(user_agent.to_string());
    }

    if let Some(remote_addr) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        session.remote_addr = Some(remote_addr.to_string());
    }

    request.extensions_mut().insert(session);
    next.run(request).await
}

/// Apply proven middleware stack to router
fn apply_middleware<S>(router: Router<S>, config: &McpServerConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // Build comprehensive middleware stack using ServiceBuilder for optimal performance
    let middleware_stack = ServiceBuilder::new()
        // 1. Distributed tracing (first for observability)
        .layer(TraceLayer::new_for_http())
        // 2. Request timeout (protect against slow clients)
        .layer(TimeoutLayer::new(config.request_timeout))
        // 3. Response compression (reduce bandwidth)
        .layer(CompressionLayer::new());

    let mut router = router
        // Apply tower middleware stack
        .layer(middleware_stack)
        // Apply session tracking middleware (adds SessionInfo to extensions)
        .layer(middleware::from_fn(session_middleware));

    // Apply authentication middleware if configured (CRITICAL SECURITY FIX)
    #[cfg(any(feature = "auth", feature = "jwt-validation"))]
    if let Some(auth_config) = &config.auth {
        router = router.layer(middleware::from_fn_with_state(
            auth_config.clone(),
            authentication_middleware,
        ));
    }

    router
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
