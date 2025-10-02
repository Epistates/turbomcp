//! Core MCP service implementation using Tower pattern
//!
//! This module provides the core MCP service that can be wrapped with middleware
//! layers to create a complete, production-ready MCP server.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use tower::Service;
use tracing::{error, info, warn};

use turbomcp_core::RequestContext;
use turbomcp_protocol::jsonrpc::{
    JsonRpcError, JsonRpcMessage, JsonRpcResponse, JsonRpcResponsePayload, JsonRpcVersion,
    ResponseId,
};

use crate::{
    ServerError, metrics::ServerMetrics, registry::HandlerRegistry, routing::RequestRouter,
};

/// Core MCP service that handles JSON-RPC requests
#[derive(Debug, Clone)]
pub struct McpService {
    registry: Arc<HandlerRegistry>,
    router: Arc<RequestRouter>,
    metrics: Arc<ServerMetrics>,
}

impl McpService {
    /// Create a new MCP service
    pub fn new(
        registry: Arc<HandlerRegistry>,
        router: Arc<RequestRouter>,
        metrics: Arc<ServerMetrics>,
    ) -> Self {
        Self {
            registry,
            router,
            metrics,
        }
    }

    /// Process a JSON-RPC message and return a response
    async fn process_jsonrpc(
        &self,
        message: JsonRpcMessage,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        match message {
            JsonRpcMessage::Request(req) => {
                info!(
                    request_id = ?req.id,
                    method = %req.method,
                    "Processing JSON-RPC request"
                );

                // Record request start
                self.metrics.record_request_start();

                let start_time = std::time::Instant::now();

                // Route the request through our business logic
                let response = self.router.route(req, ctx).await;

                let duration = start_time.elapsed();

                // Update metrics based on response
                match &response.payload {
                    JsonRpcResponsePayload::Success { .. } => {
                        self.metrics.record_request_success(duration);
                    }
                    JsonRpcResponsePayload::Error { error } => {
                        // Categorize error type for metrics
                        let error_type = match error.code {
                            -32700 => "validation", // Parse error
                            -32600 => "validation", // Invalid Request
                            -32601 => "validation", // Method not found
                            -32602 => "validation", // Invalid params
                            -32603 => "internal",   // Internal error
                            _ => "unknown",
                        };
                        self.metrics.record_request_failure(error_type, duration);
                    }
                }

                response
            }
            JsonRpcMessage::Notification(notif) => {
                warn!(method = %notif.method, "Received notification (not supported)");
                JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    payload: JsonRpcResponsePayload::Error {
                        error: JsonRpcError {
                            code: -32601,
                            message: "Notifications are not supported".to_string(),
                            data: None,
                        },
                    },
                    id: ResponseId::null(),
                }
            }
            JsonRpcMessage::Response(_) => {
                warn!("Received JSON-RPC response (unexpected)");
                JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    payload: JsonRpcResponsePayload::Error {
                        error: JsonRpcError {
                            code: -32600,
                            message: "Invalid request: unexpected response".to_string(),
                            data: None,
                        },
                    },
                    id: ResponseId::null(),
                }
            }
            JsonRpcMessage::RequestBatch(_) => {
                warn!("Received JSON-RPC request batch (not yet supported)");
                JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    payload: JsonRpcResponsePayload::Error {
                        error: JsonRpcError {
                            code: -32601,
                            message: "Batch requests are not yet supported".to_string(),
                            data: None,
                        },
                    },
                    id: ResponseId::null(),
                }
            }
            JsonRpcMessage::ResponseBatch(_) => {
                warn!("Received JSON-RPC response batch (unexpected)");
                JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    payload: JsonRpcResponsePayload::Error {
                        error: JsonRpcError {
                            code: -32600,
                            message: "Invalid request: unexpected response batch".to_string(),
                            data: None,
                        },
                    },
                    id: ResponseId::null(),
                }
            }
            JsonRpcMessage::MessageBatch(_) => {
                warn!("Received JSON-RPC message batch (not yet supported)");
                JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    payload: JsonRpcResponsePayload::Error {
                        error: JsonRpcError {
                            code: -32601,
                            message: "Message batches are not yet supported".to_string(),
                            data: None,
                        },
                    },
                    id: ResponseId::null(),
                }
            }
        }
    }
}

impl Service<Request<Bytes>> for McpService {
    type Response = Response<Bytes>;
    type Error = ServerError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Bytes>) -> Self::Future {
        let registry = Arc::clone(&self.registry);
        let router = Arc::clone(&self.router);
        let metrics = Arc::clone(&self.metrics);

        Box::pin(async move {
            // Extract the body as a string
            let body = req.into_body();
            let json_str = match std::str::from_utf8(&body) {
                Ok(s) => s,
                Err(e) => {
                    error!("Invalid UTF-8 in request body: {}", e);
                    let error_response = JsonRpcResponse {
                        jsonrpc: JsonRpcVersion,
                        payload: JsonRpcResponsePayload::Error {
                            error: JsonRpcError {
                                code: -32700,
                                message: "Parse error: invalid UTF-8".to_string(),
                                data: None,
                            },
                        },
                        id: ResponseId::null(),
                    };
                    let response_json = serde_json::to_string(&error_response)
                        .unwrap_or_else(|_| r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#.to_string());

                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .body(Bytes::from(response_json))
                        .unwrap());
                }
            };

            // Parse JSON-RPC message
            let parsed = serde_json::from_str::<JsonRpcMessage>(json_str);
            let response = match parsed {
                Ok(message) => {
                    let ctx = RequestContext::new().with_metadata("transport", "http");

                    let service = McpService::new(registry, router, metrics);
                    service.process_jsonrpc(message, ctx).await
                }
                Err(e) => {
                    error!("Failed to parse JSON-RPC: {}", e);
                    JsonRpcResponse {
                        jsonrpc: JsonRpcVersion,
                        payload: JsonRpcResponsePayload::Error {
                            error: JsonRpcError {
                                code: -32700,
                                message: format!("Parse error: {}", e),
                                data: None,
                            },
                        },
                        id: ResponseId::null(),
                    }
                }
            };

            // Serialize response
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize JSON-RPC response: {}", e);
                    r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error: failed to serialize response"}}"#.to_string()
                }
            };

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Bytes::from(response_json))
                .unwrap())
        })
    }
}

/// Type alias for the complete middleware-wrapped MCP service
pub type WrappedMcpService = Box<
    dyn Service<
            Request<Bytes>,
            Response = Response<Bytes>,
            Error = ServerError,
            Future = Pin<Box<dyn Future<Output = Result<Response<Bytes>, ServerError>> + Send>>,
        > + Send
        + Sync,
>;
