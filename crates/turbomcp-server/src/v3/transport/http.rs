//! HTTP transport implementation for v3.
//!
//! Provides JSON-RPC over HTTP POST using Axum.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::post;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;

use crate::v3::config::{RateLimiter, ServerConfig};
use crate::v3::context::RequestContext;
use crate::v3::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

/// Maximum request body size (10MB).
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Run a handler on HTTP transport.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to (e.g., "0.0.0.0:8080")
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::v3::transport::http;
///
/// http::run(&handler, "0.0.0.0:8080").await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H, addr: &str) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let app = Router::new()
        .route("/", post(handle_json_rpc::<H>))
        .route("/mcp", post(handle_json_rpc::<H>))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(handler.clone());

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    tracing::info!("v3 MCP server listening on http://{}", socket_addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// Run a handler on HTTP transport with custom configuration.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (rate limits, etc.)
pub async fn run_with_config<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let rate_limiter = config
        .rate_limit
        .as_ref()
        .map(|cfg| Arc::new(RateLimiter::new(cfg.clone())));

    let state = HttpState {
        handler: handler.clone(),
        rate_limiter,
    };

    let app = Router::new()
        .route("/", post(handle_json_rpc_with_rate_limit::<H>))
        .route("/mcp", post(handle_json_rpc_with_rate_limit::<H>))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state);

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    let rate_limit_info = config
        .rate_limit
        .as_ref()
        .map(|cfg| {
            format!(
                " (rate limit: {}/{}s)",
                cfg.max_requests,
                cfg.window.as_secs()
            )
        })
        .unwrap_or_default();

    tracing::info!(
        "v3 MCP server listening on http://{}{}",
        socket_addr,
        rate_limit_info
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// HTTP state with optional rate limiting.
#[derive(Clone)]
struct HttpState<H: McpHandler> {
    handler: H,
    rate_limiter: Option<Arc<RateLimiter>>,
}

/// Axum handler for JSON-RPC requests.
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(handler): axum::extract::State<H>,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> axum::Json<JsonRpcOutgoing> {
    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();
    let response = router::route_request(&handler, request, &core_ctx).await;
    axum::Json(response)
}

/// Axum handler for JSON-RPC requests with rate limiting.
async fn handle_json_rpc_with_rate_limit<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<HttpState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> Result<axum::Json<JsonRpcOutgoing>, axum::http::StatusCode> {
    // Check rate limit if configured
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();
    let response = router::route_request(&state.handler, request, &core_ctx).await;
    Ok(axum::Json(response))
}

#[cfg(test)]
mod tests {
    // HTTP tests are in /tests/ as they require network access
}
