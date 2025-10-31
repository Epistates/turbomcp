//! STDIO Backend for MCP Servers
//!
//! This backend uses turbomcp-transport's `StdioTransport` and `ChildProcessTransport`
//! to communicate with MCP servers over stdin/stdout.

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use tracing::{debug, trace};
use turbomcp_protocol::{
    InitializeRequest, InitializeResult, MessageId,
    jsonrpc::{
        JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload,
        JsonRpcVersion,
    },
};
use turbomcp_transport::{
    ChildProcessConfig, ChildProcessTransport, Transport, TransportMessage,
    core::TransportMessageMetadata,
};
use uuid::Uuid;

use crate::error::{ProxyError, ProxyResult};

use super::McpBackend;

/// STDIO backend for connecting to MCP servers running as subprocesses
///
/// This uses turbomcp-transport's `ChildProcessTransport` for maximum
/// correctness and `DRYness`.
pub struct StdioBackend {
    /// The underlying transport
    transport: ChildProcessTransport,
    /// Message ID counter
    next_id: std::sync::atomic::AtomicU64,
}

impl StdioBackend {
    /// Create a new STDIO backend
    ///
    /// # Arguments
    /// * `command` - The command to execute (e.g., "python", "node")
    /// * `args` - Command arguments (e.g., `["server.py"]`)
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the subprocess fails to start or connect.
    pub async fn new(command: impl Into<String>, args: Vec<String>) -> ProxyResult<Self> {
        let config = ChildProcessConfig {
            command: command.into(),
            args,
            working_directory: None,
            environment: None,
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);

        // Connect the transport (starts the subprocess)
        transport
            .connect()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to connect to subprocess: {e}")))?;

        Ok(Self {
            transport,
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Create with working directory
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the subprocess fails to start, connect, or if the working directory is invalid.
    pub async fn with_working_dir(
        command: impl Into<String>,
        args: Vec<String>,
        working_dir: String,
    ) -> ProxyResult<Self> {
        let config = ChildProcessConfig {
            command: command.into(),
            args,
            working_directory: Some(working_dir),
            environment: None,
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);

        // Connect the transport (starts the subprocess)
        transport
            .connect()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to connect to subprocess: {e}")))?;

        Ok(Self {
            transport,
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Get next message ID
    fn next_message_id(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Send a JSON-RPC request and wait for response
    async fn send_request(&self, method: &str, params: Value) -> ProxyResult<Value> {
        let id = self.next_message_id();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            // Cast u64 to i64 for JSON-RPC MessageId - IDs are sequential and won't overflow in practice
            #[allow(clippy::cast_possible_wrap)]
            id: MessageId::Number(id as i64),
            method: method.to_string(),
            params: Some(params),
        };

        trace!(method = %method, id = %id, "Sending introspection request");

        // Serialize request
        let request_json = serde_json::to_string(&request)
            .map_err(|e| ProxyError::backend(format!("Failed to serialize request: {e}")))?;

        // Send via transport
        let message = TransportMessage {
            id: turbomcp_protocol::MessageId::String(Uuid::new_v4().to_string()),
            payload: Bytes::from(request_json.into_bytes()),
            metadata: TransportMessageMetadata::default(),
        };

        self.transport
            .send(message)
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to send message: {e}")))?;

        // Receive response
        let response_message = self
            .transport
            .receive()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to receive response: {e}")))?
            .ok_or_else(|| {
                ProxyError::backend("No response received (transport closed)".to_string())
            })?;

        let response_str = String::from_utf8(response_message.payload.to_vec())
            .map_err(|e| ProxyError::backend(format!("Invalid UTF-8 in response: {e}")))?;

        trace!(response = %response_str, "Received introspection response");

        // Parse response
        let response: JsonRpcResponse = serde_json::from_str(&response_str)
            .map_err(|e| ProxyError::backend(format!("Failed to parse response: {e}")))?;

        // Extract result from response payload
        match response.payload {
            JsonRpcResponsePayload::Success { result } => Ok(result),
            JsonRpcResponsePayload::Error { error } => Err(ProxyError::backend(format!(
                "Server returned error: {error:?}"
            ))),
        }
    }
}

#[async_trait]
impl McpBackend for StdioBackend {
    async fn initialize(&mut self, request: InitializeRequest) -> ProxyResult<InitializeResult> {
        debug!("Initializing STDIO backend via turbomcp-transport");

        let params = serde_json::to_value(&request).map_err(|e| {
            ProxyError::backend(format!("Failed to serialize initialize request: {e}"))
        })?;

        let result = self.send_request("initialize", params).await?;

        let init_result: InitializeResult = serde_json::from_value(result).map_err(|e| {
            ProxyError::backend(format!("Failed to deserialize initialize result: {e}"))
        })?;

        debug!(
            server_name = %init_result.server_info.name,
            server_version = %init_result.server_info.version,
            protocol_version = %init_result.protocol_version,
            "Server initialized successfully"
        );

        // Send initialized notification
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(init_result)
    }

    async fn call_method(&mut self, method: &str, params: Value) -> ProxyResult<Value> {
        self.send_request(method, params).await
    }

    async fn send_notification(&mut self, method: &str, params: Value) -> ProxyResult<()> {
        let notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion,
            method: method.to_string(),
            params: Some(params),
        };

        let notification_json = serde_json::to_string(&notification)
            .map_err(|e| ProxyError::backend(format!("Failed to serialize notification: {e}")))?;

        trace!(method = %method, "Sending notification");

        let message = TransportMessage {
            id: turbomcp_protocol::MessageId::String(Uuid::new_v4().to_string()),
            payload: Bytes::from(notification_json.into_bytes()),
            metadata: TransportMessageMetadata::default(),
        };

        self.transport
            .send(message)
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to send notification: {e}")))?;

        Ok(())
    }

    async fn shutdown(&mut self) -> ProxyResult<()> {
        debug!("Shutting down STDIO backend");

        // ChildProcessTransport handles cleanup on drop
        // No explicit shutdown needed - process will be killed on drop if kill_on_drop is true

        Ok(())
    }

    fn description(&self) -> String {
        "STDIO backend via turbomcp-transport".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stdio_backend_creation() {
        let backend = StdioBackend::new("python", vec!["server.py".to_string()]).await;
        assert!(backend.is_ok());
    }

    #[tokio::test]
    async fn test_stdio_backend_with_working_dir() {
        let backend = StdioBackend::with_working_dir(
            "python",
            vec!["server.py".to_string()],
            "/tmp".to_string(),
        )
        .await;
        assert!(backend.is_ok());
    }
}
