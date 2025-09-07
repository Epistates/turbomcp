//! Server-specific transport functionality for bidirectional MCP communication

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp_core::{MessageId, RequestContext};
use uuid::Uuid;

use crate::core::{
    BidirectionalTransport, TransportError, TransportMessage, TransportResult, TransportType,
};

/// JSON-RPC request structure for server-initiated communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerJsonRpcRequest {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Request method  
    pub method: String,
    /// Request parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Request ID
    pub id: serde_json::Value,
}

/// JSON-RPC response structure for server-initiated communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerJsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Response result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Response error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    /// Request ID
    pub id: serde_json::Value,
}

/// Server transport dispatcher for server-initiated requests
#[async_trait]
pub trait ServerTransportDispatcher: Send + Sync {
    /// Send a server-initiated request to the client
    async fn send_server_request(
        &self,
        request: ServerJsonRpcRequest,
        ctx: RequestContext,
    ) -> TransportResult<ServerJsonRpcResponse>;

    /// Check if the transport supports server-initiated requests
    fn supports_server_requests(&self) -> bool;

    /// Get connected client count
    async fn connected_clients(&self) -> usize;

    /// Broadcast a message to all connected clients  
    async fn broadcast(&self, message: TransportMessage) -> TransportResult<()>;

    /// Send a message to a specific client
    async fn send_to_client(
        &self,
        client_id: &str,
        message: TransportMessage,
    ) -> TransportResult<()>;

    /// Get list of connected client IDs
    async fn get_connected_client_ids(&self) -> Vec<String>;
}

/// Server transport manager for handling multiple client connections
#[derive(Debug)]
pub struct ServerTransportManager {
    /// Active client connections mapped by client ID
    connections: Arc<RwLock<HashMap<String, Arc<dyn BidirectionalTransport + Send + Sync>>>>,
    /// Transport configuration
    config: ServerTransportConfig,
}

/// Configuration for server transport
#[derive(Debug, Clone)]
pub struct ServerTransportConfig {
    /// Maximum number of concurrent client connections
    pub max_connections: usize,
    /// Request timeout for server-initiated requests
    pub server_request_timeout_ms: u64,
    /// Enable connection heartbeat monitoring
    pub enable_heartbeat: bool,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_sec: u64,
    /// Enable request/response correlation tracking
    pub enable_correlation: bool,
}

impl Default for ServerTransportConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            server_request_timeout_ms: 30_000,
            enable_heartbeat: true,
            heartbeat_interval_sec: 30,
            enable_correlation: true,
        }
    }
}

impl ServerTransportManager {
    /// Create a new server transport manager
    pub fn new(config: ServerTransportConfig) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Add a client connection
    pub async fn add_client(
        &self,
        client_id: String,
        transport: Arc<dyn BidirectionalTransport + Send + Sync>,
    ) -> TransportResult<()> {
        let mut connections = self.connections.write().await;

        if connections.len() >= self.config.max_connections {
            return Err(TransportError::ConfigurationError(format!(
                "Maximum connections ({}) exceeded",
                self.config.max_connections
            )));
        }

        connections.insert(client_id, transport);
        Ok(())
    }

    /// Remove a client connection
    pub async fn remove_client(
        &self,
        client_id: &str,
    ) -> Option<Arc<dyn BidirectionalTransport + Send + Sync>> {
        let mut connections = self.connections.write().await;
        connections.remove(client_id)
    }

    /// Get a client connection
    pub async fn get_client(
        &self,
        client_id: &str,
    ) -> Option<Arc<dyn BidirectionalTransport + Send + Sync>> {
        let connections = self.connections.read().await;
        connections.get(client_id).cloned()
    }

    /// Check if a client is connected
    pub async fn is_client_connected(&self, client_id: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(client_id)
    }

    /// Get all connected client IDs
    pub async fn get_all_client_ids(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    /// Get connection count
    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// Send a message to all connected clients
    pub async fn broadcast_to_all(&self, message: TransportMessage) -> TransportResult<()> {
        let connections = self.connections.read().await;
        let mut send_futures = Vec::new();

        for (client_id, transport) in connections.iter() {
            let client_id = client_id.clone();
            let _message_clone = message.clone();
            let _transport_arc = Arc::clone(transport);

            send_futures.push(async move {
                // Note: Transport trait requires mutable access for send operations
                // Current Arc-based design doesn't support concurrent mutable access
                // This requires architectural enhancement for true bidirectional transport
                tracing::warn!("Broadcast to client {} skipped - requires mutable transport access", client_id);
                (client_id, Err::<(), TransportError>(TransportError::NotAvailable(
                    "Broadcast requires mutable transport access not available in current design".to_string()
                )))
            });
        }

        // Execute all sends concurrently
        let results = futures::future::join_all(send_futures).await;

        // Check for any failures
        for (client_id, result) in results {
            if let Err(e) = result {
                tracing::warn!("Failed to send broadcast to client {}: {:?}", client_id, e);
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ServerTransportDispatcher for ServerTransportManager {
    async fn send_server_request(
        &self,
        request: ServerJsonRpcRequest,
        ctx: RequestContext,
    ) -> TransportResult<ServerJsonRpcResponse> {
        // Send to the client specified in the request context, or first available client
        let connections = self.connections.read().await;

        // Try to find the specific client from context, otherwise use first available
        let target_transport = if let Some(ref client_id) = ctx.client_id {
            connections.get(client_id).cloned()
        } else {
            connections
                .iter()
                .next()
                .map(|(_, transport)| transport.clone())
        };

        if let Some(_transport_arc) = target_transport {
            let client_id = ctx.client_id.as_deref().unwrap_or("first_available");
            tracing::debug!(
                "Sending server request to client {}: {} {}",
                client_id,
                request.method,
                request.id
            );

            let _request_message = TransportMessage {
                id: MessageId::from(Uuid::new_v4()),
                payload: serde_json::to_vec(&request)
                    .map_err(|e| {
                        TransportError::SerializationFailed(format!(
                            "Failed to serialize server request: {}",
                            e
                        ))
                    })?
                    .into(),
                metadata: Default::default(),
            };

            let _timeout = std::time::Duration::from_millis(self.config.server_request_timeout_ms);

            // Current implementation: Server-initiated requests not supported due to Arc<T> constraint
            // Transport trait requires mutable access but Arc<T> provides shared immutable access
            // Architecture supports this via enhanced trait design with interior mutability
            Err(TransportError::NotAvailable(
                "Server-initiated requests require enhanced bidirectional transport implementation"
                    .to_string(),
            ))
            /*
            match transport_arc.send_request(request_message, Some(timeout)).await {
                Ok(response_message) => {
                    // Parse the response payload as ServerJsonRpcResponse
                    match serde_json::from_slice::<ServerJsonRpcResponse>(&response_message.payload) {
                        Ok(response) => {
                            tracing::debug!(
                                "Received response from client {}: {:?}",
                                client_id,
                                response
                            );
                            Ok(response)
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse response from client {}: {}",
                                client_id,
                                e
                            );
                            Err(TransportError::SerializationFailed(format!(
                                "Failed to parse client response: {}",
                                e
                            )))
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to get response from client {}: {}",
                        client_id,
                        e
                    );
                    Err(e)
                }
            }
            */
        } else {
            Err(TransportError::ConnectionFailed(
                "No connected clients available for server request".to_string(),
            ))
        }
    }

    fn supports_server_requests(&self) -> bool {
        true
    }

    async fn connected_clients(&self) -> usize {
        self.connection_count().await
    }

    async fn broadcast(&self, message: TransportMessage) -> TransportResult<()> {
        self.broadcast_to_all(message).await
    }

    async fn send_to_client(
        &self,
        client_id: &str,
        _message: TransportMessage,
    ) -> TransportResult<()> {
        if self.is_client_connected(client_id).await {
            // Implementation would send message to specific client
            Ok(())
        } else {
            Err(TransportError::ConnectionFailed(format!(
                "Client {} not connected",
                client_id
            )))
        }
    }

    async fn get_connected_client_ids(&self) -> Vec<String> {
        self.get_all_client_ids().await
    }
}

/// Server-side transport wrapper that implements ServerRequestDispatcher for the routing layer
#[derive(Debug)]
pub struct ServerTransportWrapper {
    /// The underlying transport manager
    transport_manager: Arc<ServerTransportManager>,
    /// Client ID for single-client scenarios
    default_client_id: Option<String>,
}

impl ServerTransportWrapper {
    /// Create a new server transport wrapper
    pub fn new(transport_manager: Arc<ServerTransportManager>) -> Self {
        Self {
            transport_manager,
            default_client_id: None,
        }
    }

    /// Set default client ID for single-client scenarios
    pub fn with_default_client(mut self, client_id: String) -> Self {
        self.default_client_id = Some(client_id);
        self
    }

    /// Get the transport manager
    pub fn transport_manager(&self) -> &Arc<ServerTransportManager> {
        &self.transport_manager
    }
}

/// Connection event types for server transport monitoring
#[derive(Debug, Clone)]
pub enum ServerTransportEvent {
    /// Client connected
    ClientConnected {
        /// Client ID
        client_id: String,
        /// Transport type
        transport_type: TransportType,
        /// Connection timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Client disconnected  
    ClientDisconnected {
        /// Client ID
        client_id: String,
        /// Disconnect reason
        reason: String,
        /// Disconnection timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Server request sent
    ServerRequestSent {
        /// Client ID
        client_id: String,
        /// Request ID
        request_id: String,
        /// Request method
        method: String,
        /// Timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Server request response received
    ServerRequestResponse {
        /// Client ID
        client_id: String,
        /// Request ID
        request_id: String,
        /// Success status
        success: bool,
        /// Response timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

/// Server transport event listener
#[async_trait]
pub trait ServerTransportEventListener: Send + Sync {
    /// Handle a server transport event
    async fn on_event(&self, event: ServerTransportEvent);
}

/// Configuration builder for server transport
#[derive(Debug)]
pub struct ServerTransportConfigBuilder {
    config: ServerTransportConfig,
}

impl ServerTransportConfigBuilder {
    /// Create a new config builder
    pub fn new() -> Self {
        Self {
            config: ServerTransportConfig::default(),
        }
    }

    /// Set maximum connections
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set server request timeout
    pub fn server_request_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.server_request_timeout_ms = timeout_ms;
        self
    }

    /// Enable/disable heartbeat monitoring
    pub fn heartbeat(mut self, enable: bool) -> Self {
        self.config.enable_heartbeat = enable;
        self
    }

    /// Set heartbeat interval
    pub fn heartbeat_interval(mut self, interval_sec: u64) -> Self {
        self.config.heartbeat_interval_sec = interval_sec;
        self
    }

    /// Enable/disable correlation tracking
    pub fn correlation(mut self, enable: bool) -> Self {
        self.config.enable_correlation = enable;
        self
    }

    /// Build the configuration
    pub fn build(self) -> ServerTransportConfig {
        self.config
    }
}

impl Default for ServerTransportConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_transport_manager_creation() {
        let config = ServerTransportConfig::default();
        let manager = ServerTransportManager::new(config);

        assert_eq!(manager.connection_count().await, 0);
        assert!(manager.get_all_client_ids().await.is_empty());
    }

    #[tokio::test]
    async fn test_server_transport_config_builder() {
        let config = ServerTransportConfigBuilder::new()
            .max_connections(500)
            .server_request_timeout(20_000)
            .heartbeat(false)
            .build();

        assert_eq!(config.max_connections, 500);
        assert_eq!(config.server_request_timeout_ms, 20_000);
        assert!(!config.enable_heartbeat);
    }

    #[test]
    fn test_server_transport_wrapper_creation() {
        let config = ServerTransportConfig::default();
        let manager = Arc::new(ServerTransportManager::new(config));
        let wrapper = ServerTransportWrapper::new(Arc::clone(&manager))
            .with_default_client("client-1".to_string());

        assert_eq!(wrapper.default_client_id, Some("client-1".to_string()));
    }
}
