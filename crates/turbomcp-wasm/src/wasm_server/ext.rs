//! Extension trait for running McpHandler in WASM environments.
//!
//! This module provides the `WasmHandlerExt` trait that extends `McpHandler`
//! with methods for running in WASM environments like Cloudflare Workers.
//!
//! # Architecture
//!
//! Uses the shared router from `turbomcp_core::router` for consistent behavior
//! between native and WASM platforms. The only WASM-specific code is the
//! Worker SDK integration.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::WasmHandlerExt;
//! use turbomcp_core::handler::McpHandler;
//!
//! #[derive(Clone)]
//! struct MyServer;
//!
//! // Implement McpHandler for MyServer...
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!     MyServer.handle_worker_request(req).await
//! }
//! ```

use serde_json::Value;
use turbomcp_core::context::{RequestContext, TransportType};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::jsonrpc::{JsonRpcIncoming, JsonRpcOutgoing};
use turbomcp_core::router::{RouteConfig, route_request};
use worker::{Request, Response};

/// Extension trait for running `McpHandler` in WASM environments.
///
/// This trait is automatically implemented for all types that implement `McpHandler`.
/// It provides methods for handling requests in Cloudflare Workers and other WASM
/// runtime environments.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::wasm_server::WasmHandlerExt;
///
/// #[event(fetch)]
/// async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
///     MyServer.handle_worker_request(req).await
/// }
/// ```
pub trait WasmHandlerExt: McpHandler {
    /// Handle an incoming Cloudflare Worker request.
    ///
    /// This is the main entry point for MCP servers running in Cloudflare Workers.
    /// It parses the JSON-RPC request, routes it to the appropriate handler, and
    /// returns a JSON-RPC response.
    fn handle_worker_request(
        &self,
        req: Request,
    ) -> impl std::future::Future<Output = worker::Result<Response>>;

    /// Handle a raw JSON-RPC request value.
    ///
    /// This method is useful for environments that don't use the Worker SDK
    /// directly, such as custom HTTP handlers or testing.
    fn handle_json_rpc_request(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = McpResult<Value>>;
}

impl<T: McpHandler> WasmHandlerExt for T {
    fn handle_worker_request(
        &self,
        mut req: Request,
    ) -> impl std::future::Future<Output = worker::Result<Response>> {
        let handler = self.clone();
        async move {
            // Parse request body as JSON
            let body = req.text().await?;
            let request: JsonRpcIncoming = match serde_json::from_str(&body) {
                Ok(r) => r,
                Err(e) => {
                    let response = JsonRpcOutgoing::error(
                        None,
                        McpError::parse_error(format!("Invalid JSON: {}", e)),
                    );
                    return Response::from_json(&response);
                }
            };

            // Generate a unique request ID for context
            let request_id = request
                .id
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "wasm-request".to_string());
            let ctx = RequestContext::new(request_id, TransportType::Wasm);

            // Route using the shared core router
            let config = RouteConfig::default();
            let response = route_request(&handler, request, &ctx, &config).await;

            // Only send response if it should be sent (per JSON-RPC 2.0)
            if response.should_send() {
                Response::from_json(&response)
            } else {
                // For notifications, return empty 204
                Response::empty()
            }
        }
    }

    fn handle_json_rpc_request(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = McpResult<Value>> {
        let handler = self.clone();
        async move {
            let request: JsonRpcIncoming = serde_json::from_value(request)
                .map_err(|e| McpError::parse_error(format!("Invalid request: {}", e)))?;

            // Generate a unique request ID for context
            let request_id = request
                .id
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "wasm-request".to_string());
            let ctx = RequestContext::new(request_id, TransportType::Wasm);

            // Route using the shared core router
            let config = RouteConfig::default();
            let response = route_request(&handler, request, &ctx, &config).await;

            serde_json::to_value(&response)
                .map_err(|e| McpError::internal(format!("Serialization error: {}", e)))
        }
    }
}
