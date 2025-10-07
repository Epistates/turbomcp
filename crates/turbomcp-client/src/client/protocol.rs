//! Protocol client for JSON-RPC communication
//!
//! This module provides the ProtocolClient which handles the low-level
//! JSON-RPC protocol communication with MCP servers.

use std::sync::atomic::{AtomicU64, Ordering};

use turbomcp_protocol::{Error, Result};
use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};
use turbomcp_transport::{Transport, TransportMessage};

/// JSON-RPC protocol handler for MCP communication
///
/// Handles request/response correlation, serialization, and protocol-level concerns.
/// This is the missing abstraction layer between raw Transport and high-level Client APIs.
#[derive(Debug)]
pub(super) struct ProtocolClient<T: Transport> {
    transport: T,
    next_id: AtomicU64,
}

impl<T: Transport> ProtocolClient<T> {
    pub(super) fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    /// Send JSON-RPC request and await typed response
    pub(super) async fn request<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: turbomcp_protocol::MessageId::from(id.to_string()),
            method: method.to_string(),
            params,
        };

        // Serialize and send
        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::protocol(format!("Failed to serialize request: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from(format!("req-{id}")),
            payload.into(),
        );
        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))?;

        // Receive and deserialize response
        let response_msg = self
            .transport
            .receive()
            .await
            .map_err(|e| Error::transport(format!("Transport receive failed: {e}")))?
            .ok_or_else(|| Error::transport("No response received".to_string()))?;

        let response: JsonRpcResponse = serde_json::from_slice(&response_msg.payload)
            .map_err(|e| Error::protocol(format!("Invalid JSON-RPC response: {e}")))?;

        if let Some(error) = response.error() {
            return Err(Error::rpc(error.code, &error.message));
        }

        serde_json::from_value(response.result().unwrap_or_default().clone())
            .map_err(|e| Error::protocol(format!("Failed to deserialize response: {e}")))
    }

    /// Send JSON-RPC notification (no response expected)
    pub(super) async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::protocol(format!("Failed to serialize notification: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from("notification"),
            payload.into(),
        );

        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))
    }

    /// Connect the transport
    #[allow(dead_code)] // Reserved for future use
    pub(super) async fn connect(&self) -> Result<()> {
        self.transport
            .connect()
            .await
            .map_err(|e| Error::transport(format!("Transport connect failed: {e}")))
    }

    /// Disconnect the transport
    #[allow(dead_code)] // Reserved for future use
    pub(super) async fn disconnect(&self) -> Result<()> {
        self.transport
            .disconnect()
            .await
            .map_err(|e| Error::transport(format!("Transport disconnect failed: {e}")))
    }

    /// Get transport reference
    pub(super) fn transport(&self) -> &T {
        &self.transport
    }
}
