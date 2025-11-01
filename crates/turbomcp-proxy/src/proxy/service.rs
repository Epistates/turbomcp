//! `ProxyService` - MCP service that forwards requests to backend servers
//!
//! This service implements the `McpService` trait from turbomcp-transport,
//! enabling it to be used with the Axum integration for HTTP/SSE transport.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, trace};
use turbomcp_protocol::{Error as McpError, Result as McpResult, jsonrpc::JsonRpcRequest};
use turbomcp_transport::tower::SessionInfo;

use super::BackendConnector;
use crate::introspection::ServerSpec;

/// Proxy service that forwards MCP requests to a backend server
///
/// This service implements the `McpService` trait, allowing it to be used
/// with turbomcp-transport's Axum integration for HTTP/SSE transport.
/// All requests are forwarded to the backend server via turbomcp-client.
#[derive(Clone)]
pub struct ProxyService {
    /// Backend connector (Arc+RwLock for cheap cloning and async access)
    backend: Arc<RwLock<BackendConnector>>,

    /// Cached server spec from introspection
    spec: Arc<ServerSpec>,
}

impl ProxyService {
    /// Create a new proxy service
    ///
    /// # Arguments
    ///
    /// * `backend` - The backend connector (must be introspected)
    /// * `spec` - The server spec from introspection
    #[must_use]
    pub fn new(backend: BackendConnector, spec: ServerSpec) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            spec: Arc::new(spec),
        }
    }

    /// Process a JSON-RPC request by forwarding to backend
    async fn process_jsonrpc(&self, request: JsonRpcRequest) -> McpResult<Value> {
        trace!(
            "Processing JSON-RPC: method={}, id={:?}",
            request.method, request.id
        );

        // Route based on method
        match request.method.as_str() {
            // Tools
            "tools/list" => {
                debug!("Forwarding tools/list to backend");
                let backend = self.backend.read().await;
                let tools = backend
                    .list_tools()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "tools": tools
                }))
            }

            "tools/call" => {
                debug!("Forwarding tools/call to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for tools/call".to_string())
                })?;

                let call_request: turbomcp_protocol::types::CallToolRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let backend = self.backend.read().await;
                let result = backend
                    .call_tool(&call_request.name, call_request.arguments)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Resources
            "resources/list" => {
                debug!("Forwarding resources/list to backend");
                let backend = self.backend.read().await;
                let resources = backend
                    .list_resources()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "resources": resources
                }))
            }

            "resources/read" => {
                debug!("Forwarding resources/read to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for resources/read".to_string())
                })?;

                let read_request: turbomcp_protocol::types::ReadResourceRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let backend = self.backend.read().await;
                let contents = backend
                    .read_resource(&read_request.uri)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "contents": contents
                }))
            }

            // Prompts
            "prompts/list" => {
                debug!("Forwarding prompts/list to backend");
                let backend = self.backend.read().await;
                let prompts = backend
                    .list_prompts()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "prompts": prompts
                }))
            }

            "prompts/get" => {
                debug!("Forwarding prompts/get to backend");
                let params = request.params.ok_or_else(|| {
                    McpError::invalid_params("Missing params for prompts/get".to_string())
                })?;

                let get_request: turbomcp_protocol::types::GetPromptRequest =
                    serde_json::from_value(params)
                        .map_err(|e| McpError::invalid_params(e.to_string()))?;

                // Arguments are already HashMap<String, Value>
                let arguments = get_request.arguments;

                let backend = self.backend.read().await;
                let result = backend
                    .get_prompt(&get_request.name, arguments)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Unknown method
            method => {
                error!("Unknown method: {}", method);
                Err(McpError::protocol(format!("Method not found: {method}")))
            }
        }
    }
}

#[async_trait]
impl turbomcp_transport::axum::McpService for ProxyService {
    async fn process_request(&self, request: Value, _session: &SessionInfo) -> McpResult<Value> {
        // Parse JSON-RPC request
        let json_rpc_request: JsonRpcRequest =
            serde_json::from_value(request).map_err(|e| McpError::serialization(e.to_string()))?;

        // Process the request
        self.process_jsonrpc(json_rpc_request).await
    }

    fn get_capabilities(&self) -> Value {
        // Return backend capabilities from introspection
        serde_json::json!({
            "protocolVersion": self.spec.protocol_version,
            "serverInfo": {
                "name": format!("{}-proxy", self.spec.server_info.name),
                "version": self.spec.server_info.version,
            },
            "capabilities": self.spec.capabilities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{BackendConfig, BackendTransport};
    use turbomcp_transport::McpService;

    async fn create_test_service() -> Option<ProxyService> {
        let config = BackendConfig {
            transport: BackendTransport::Stdio {
                command: "cargo".to_string(),
                args: vec![
                    "run".to_string(),
                    "--package".to_string(),
                    "turbomcp".to_string(),
                    "--example".to_string(),
                    "stdio_server".to_string(),
                ],
                working_dir: Some("/Users/nickpaterno/work/turbomcp".to_string()),
            },
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let Ok(backend) = BackendConnector::new(config).await else {
            return None;
        };

        let Ok(spec) = backend.introspect().await else {
            return None;
        };

        Some(ProxyService::new(backend, spec))
    }

    #[tokio::test]
    async fn test_service_creation() {
        if let Some(service) = create_test_service().await {
            // Verify capabilities
            let caps = service.get_capabilities();
            assert!(caps.get("capabilities").is_some());
        }
    }

    #[tokio::test]
    async fn test_tools_list() {
        if let Some(service) = create_test_service().await {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            });

            let session = SessionInfo {
                id: "test".to_string(),
                created_at: std::time::Instant::now(),
                last_activity: std::time::Instant::now(),
                remote_addr: Some("test-client".to_string()),
                user_agent: None,
                metadata: std::collections::HashMap::new(),
            };

            let result = service.process_request(request, &session).await;
            assert!(result.is_ok());
        }
    }
}
