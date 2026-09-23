//! Extension trait for running McpHandler in WASM environments.
//!
//! This module provides the `WasmHandlerExt` trait that extends `McpHandler`
//! with methods for running in WASM environments like Cloudflare Workers.
//!
//! # Architecture
//!
//! Uses the shared router from `turbomcp_core::router` for consistent behavior
//! between native and WASM platforms. The only WASM-specific code is the
//! Worker SDK integration, which lives in the crate's shared HTTP edge so that
//! `McpServer::handle`, the wrappers and this trait all answer identically.
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
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use worker::{Request, Response};

use super::endpoint::{self, EndpointConfig, Inbound};

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
    /// It serves the stateless JSON-RPC endpoint with the default
    /// [`EndpointConfig`]: POST only, JSON bodies up to 1 MiB, and browser
    /// origins limited to loopback ones. Notifications are answered `202` with
    /// no body.
    fn handle_worker_request(
        &self,
        req: Request,
    ) -> impl std::future::Future<Output = worker::Result<Response>>;

    /// Handle an incoming Cloudflare Worker request with an explicit
    /// [`EndpointConfig`] — typically to allow the browser origin a web
    /// application calls the Worker from.
    fn handle_worker_request_with_config(
        &self,
        req: Request,
        config: &EndpointConfig,
    ) -> impl std::future::Future<Output = worker::Result<Response>>;

    /// Handle a raw JSON-RPC request value.
    ///
    /// This method is useful for environments that don't use the Worker SDK
    /// directly, such as custom HTTP handlers or testing. A notification
    /// yields `Value::Null`, since it has no response.
    fn handle_json_rpc_request(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = McpResult<Value>>;
}

#[allow(clippy::manual_async_fn)]
impl<T: McpHandler> WasmHandlerExt for T {
    fn handle_worker_request(
        &self,
        req: Request,
    ) -> impl std::future::Future<Output = worker::Result<Response>> {
        async move {
            endpoint::serve(
                self,
                req,
                &EndpointConfig::default(),
                |ctx| ctx,
                endpoint::admit_all,
            )
            .await
        }
    }

    fn handle_worker_request_with_config(
        &self,
        req: Request,
        config: &EndpointConfig,
    ) -> impl std::future::Future<Output = worker::Result<Response>> {
        async move { endpoint::serve(self, req, config, |ctx| ctx, endpoint::admit_all).await }
    }

    fn handle_json_rpc_request(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = McpResult<Value>> {
        async move {
            let response = match endpoint::parse_message(&request.to_string()) {
                Inbound::Message(request) => {
                    let ctx = super::context::new_wasm_context();
                    endpoint::route(self, request, &ctx, None).await
                }
                Inbound::ClientResponse => return Ok(Value::Null),
                Inbound::Invalid(error) => error,
            };
            if !response.should_send() {
                return Ok(Value::Null);
            }
            serde_json::to_value(&response)
                .map_err(|e| McpError::internal(format!("Serialization error: {}", e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm_server::McpServer;

    #[tokio::test]
    async fn json_rpc_entry_point_routes_through_core() {
        let server = McpServer::builder("ext", "1.0.0").build();

        let ping = server
            .handle_json_rpc_request(
                serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
            )
            .await
            .unwrap();
        assert_eq!(ping["result"], serde_json::json!({}));

        let notification = server
            .handle_json_rpc_request(
                serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            )
            .await
            .unwrap();
        assert!(notification.is_null());

        let bad = server
            .handle_json_rpc_request(
                serde_json::json!({"jsonrpc": "2.0", "id": null, "method": "ping"}),
            )
            .await
            .unwrap();
        assert_eq!(bad["error"]["code"], -32600);
        assert!(bad["id"].is_null());
    }
}
