//! Elicitation handling for WebSocket bidirectional transport
//!
//! This module manages server-initiated elicitation requests, including
//! sending elicitations, processing responses, and handling timeouts.

use std::time::Duration;

use bytes::Bytes;
use futures::SinkExt as _;
use serde_json::json;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};
use uuid::Uuid;

use super::types::{PendingElicitation, WebSocketBidirectionalTransport};
use crate::core::{TransportError, TransportMessage, TransportMessageMetadata, TransportResult};
use turbomcp_protocol::MessageId;
use turbomcp_protocol::types::{ElicitRequest, ElicitResult, ElicitationAction};

impl WebSocketBidirectionalTransport {
    /// Send an elicitation request
    pub async fn send_elicitation(
        &self,
        request: ElicitRequest,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<ElicitResult> {
        // Check if we're at capacity
        if self.is_at_elicitation_capacity() {
            return Err(TransportError::SendFailed(format!(
                "Maximum concurrent elicitations reached ({})",
                self.config
                    .lock()
                    .expect("config mutex poisoned")
                    .max_concurrent_elicitations
            )));
        }

        let request_id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel();

        let timeout_duration = timeout_duration.unwrap_or(
            self.config
                .lock()
                .expect("config mutex poisoned")
                .elicitation_timeout,
        );

        // Store pending elicitation
        let pending = PendingElicitation::new(request.clone(), response_tx, timeout_duration);

        self.elicitations.insert(request_id.clone(), pending);

        // Create JSON-RPC request
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": "elicitation/create",
            "params": request,
            "id": request_id
        });

        // Send via WebSocket
        let message_text = serde_json::to_string(&json_request)
            .map_err(|e| TransportError::SendFailed(format!("Failed to serialize: {}", e)))?;

        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(Message::Text(message_text.into()))
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            debug!(
                "Sent elicitation request {} for session {}",
                request_id, self.session_id
            );
        } else {
            self.elicitations.remove(&request_id);
            return Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ));
        }

        // Update metrics
        self.metrics.write().await.messages_sent += 1;

        // Wait for response with timeout
        let deadline = tokio::time::Instant::now() + timeout_duration;
        match timeout(
            deadline.duration_since(tokio::time::Instant::now()),
            response_rx,
        )
        .await
        {
            Ok(Ok(result)) => {
                debug!(
                    "Received elicitation response for {} in session {}",
                    request_id, self.session_id
                );
                Ok(result)
            }
            Ok(Err(_)) => {
                warn!(
                    "Elicitation response channel closed for {} in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::ReceiveFailed(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                warn!(
                    "Elicitation {} timed out in session {}",
                    request_id, self.session_id
                );
                self.elicitations.remove(&request_id);
                Err(TransportError::Timeout)
            }
        }
    }

    /// Send an elicitation with retry capability
    pub async fn send_elicitation_with_retry(
        &self,
        request: ElicitRequest,
        max_retries: u32,
        retry_delay: Duration,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<ElicitResult> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= max_retries {
            match self
                .send_elicitation(request.clone(), timeout_duration)
                .await
            {
                Ok(result) => {
                    if attempts > 0 {
                        debug!(
                            "Elicitation succeeded after {} retries in session {}",
                            attempts, self.session_id
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);
                    attempts += 1;

                    if attempts <= max_retries {
                        debug!(
                            "Elicitation attempt {} failed in session {}, retrying after {:?}",
                            attempts, self.session_id, retry_delay
                        );
                        tokio::time::sleep(retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TransportError::SendFailed("All elicitation retry attempts failed".to_string())
        }))
    }

    /// Process incoming message for elicitation responses
    pub async fn process_incoming_message(&self, text: String) -> TransportResult<()> {
        // Parse as JSON
        let json_value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| TransportError::ReceiveFailed(format!("Invalid JSON: {}", e)))?;

        // Extract the request ID if present
        let request_id = json_value.get("id").and_then(|v| v.as_str());

        // Check if it's an elicitation response
        if let Some(id) = request_id
            && let Some((_, pending)) = self.elicitations.remove(id)
        {
            debug!(
                "Processing elicitation response for {} in session {}",
                id, self.session_id
            );

            // Parse elicitation result from the result field
            if let Some(result) = json_value.get("result") {
                match serde_json::from_value::<ElicitResult>(result.clone()) {
                    Ok(elicitation_result) => {
                        let _ = pending.response_tx.send(elicitation_result);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse elicitation result for {} in session {}: {}",
                            id, self.session_id, e
                        );
                    }
                }
            }

            // Handle error response or malformed result
            if let Some(error) = json_value.get("error") {
                warn!(
                    "Elicitation error response for {} in session {}: {}",
                    id, self.session_id, error
                );
            } else {
                warn!(
                    "Malformed elicitation response for {} in session {}",
                    id, self.session_id
                );
            }

            // Send cancel result for any error or malformed response
            let cancel_result = ElicitResult {
                action: ElicitationAction::Cancel,
                content: None,
                _meta: None,
            };
            let _ = pending.response_tx.send(cancel_result);
            return Ok(());
        }

        // Check if it's a sampling response
        if let Some(id) = request_id
            && let Some((_, response_tx)) = self.pending_samplings.remove(id)
        {
            debug!(
                "Processing sampling response for {} in session {}",
                id, self.session_id
            );

            if let Some(result) = json_value.get("result") {
                match serde_json::from_value::<turbomcp_protocol::types::CreateMessageResult>(
                    result.clone(),
                ) {
                    Ok(sampling_result) => {
                        let _ = response_tx.send(sampling_result);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse sampling result for {} in session {}: {}",
                            id, self.session_id, e
                        );
                    }
                }
            }

            // If we reach here, there was an error or malformed response
            // The channel will be dropped, causing a receive error on the waiting side
            return Ok(());
        }

        // Check if it's a ping response
        if let Some(id) = request_id
            && let Some((_, response_tx)) = self.pending_pings.remove(id)
        {
            debug!(
                "Processing ping response for {} in session {}",
                id, self.session_id
            );

            if let Some(result) = json_value.get("result") {
                match serde_json::from_value::<turbomcp_protocol::types::PingResult>(result.clone())
                {
                    Ok(ping_result) => {
                        let _ = response_tx.send(ping_result);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse ping result for {} in session {}: {}",
                            id, self.session_id, e
                        );
                    }
                }
            }

            // If we reach here, there was an error or malformed response
            // The channel will be dropped, causing a receive error on the waiting side
            return Ok(());
        }

        // Check if it's a roots list response
        if let Some(id) = request_id
            && let Some((_, response_tx)) = self.pending_roots.remove(id)
        {
            debug!(
                "Processing roots/list response for {} in session {}",
                id, self.session_id
            );

            if let Some(result) = json_value.get("result") {
                match serde_json::from_value::<turbomcp_protocol::types::ListRootsResult>(
                    result.clone(),
                ) {
                    Ok(roots_result) => {
                        let _ = response_tx.send(roots_result);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse roots/list result for {} in session {}: {}",
                            id, self.session_id, e
                        );
                    }
                }
            }

            // If we reach here, there was an error or malformed response
            // The channel will be dropped, causing a receive error on the waiting side
            return Ok(());
        }

        // Process as regular message or correlation response
        if let Some(correlation_id) = json_value.get("correlation_id").and_then(|v| v.as_str())
            && let Some((_, ctx)) = self.correlations.remove(correlation_id)
            && let Some(tx) = ctx.response_tx
        {
            let message = TransportMessage {
                id: MessageId::from(Uuid::new_v4()),
                payload: Bytes::from(serde_json::to_vec(&json_value).unwrap_or_default()),
                metadata: TransportMessageMetadata::default(),
            };
            let _ = tx.send(message);
            debug!(
                "Processed correlation response for {} in session {}",
                correlation_id, self.session_id
            );
        }

        Ok(())
    }

    /// Get all pending elicitation request IDs
    pub fn get_pending_elicitation_ids(&self) -> Vec<String> {
        self.elicitations
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Cancel a pending elicitation
    pub fn cancel_elicitation(&self, request_id: &str) -> bool {
        if let Some((_, pending)) = self.elicitations.remove(request_id) {
            let cancel_result = ElicitResult {
                action: ElicitationAction::Cancel,
                content: None,
                _meta: None,
            };
            let _ = pending.response_tx.send(cancel_result);
            debug!(
                "Cancelled elicitation {} in session {}",
                request_id, self.session_id
            );
            true
        } else {
            false
        }
    }

    /// Cancel all pending elicitations
    pub fn cancel_all_elicitations(&self) -> usize {
        let mut cancelled_count = 0;
        let request_ids: Vec<String> = self
            .elicitations
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        for request_id in request_ids {
            if self.cancel_elicitation(&request_id) {
                cancelled_count += 1;
            }
        }

        if cancelled_count > 0 {
            debug!(
                "Cancelled {} elicitations in session {}",
                cancelled_count, self.session_id
            );
        }

        cancelled_count
    }

    /// Get information about a pending elicitation
    pub fn get_elicitation_info(&self, request_id: &str) -> Option<ElicitationInfo> {
        self.elicitations.get(request_id).map(|entry| {
            let pending = entry.value();
            ElicitationInfo {
                request_id: pending.request_id.clone(),
                request: pending.request.clone(),
                deadline: pending.deadline,
                retry_count: pending.retry_count,
                time_remaining: pending.time_remaining(),
                is_expired: pending.is_expired(),
            }
        })
    }

    /// Get information about all pending elicitations
    pub fn get_all_elicitation_info(&self) -> Vec<ElicitationInfo> {
        self.elicitations
            .iter()
            .map(|entry| {
                let pending = entry.value();
                ElicitationInfo {
                    request_id: pending.request_id.clone(),
                    request: pending.request.clone(),
                    deadline: pending.deadline,
                    retry_count: pending.retry_count,
                    time_remaining: pending.time_remaining(),
                    is_expired: pending.is_expired(),
                }
            })
            .collect()
    }

    /// Clean up expired elicitations (called by timeout monitor)
    pub fn cleanup_expired_elicitations(&self) -> usize {
        let now = tokio::time::Instant::now();
        let mut expired_ids = Vec::new();

        // Find expired elicitations
        for entry in self.elicitations.iter() {
            if entry.deadline <= now {
                expired_ids.push(entry.key().clone());
            }
        }

        // Remove and cancel expired elicitations
        let mut cleaned_count = 0;
        for request_id in expired_ids {
            if self.cancel_elicitation(&request_id) {
                cleaned_count += 1;
            }
        }

        if cleaned_count > 0 {
            debug!(
                "Cleaned up {} expired elicitations in session {}",
                cleaned_count, self.session_id
            );
        }

        cleaned_count
    }
}

/// Information about a pending elicitation
#[derive(Debug, Clone)]
pub struct ElicitationInfo {
    /// Request ID
    pub request_id: String,
    /// The elicitation request
    pub request: ElicitRequest,
    /// Timeout deadline
    pub deadline: tokio::time::Instant,
    /// Number of retries attempted
    pub retry_count: u32,
    /// Time remaining until timeout
    pub time_remaining: Duration,
    /// Whether the elicitation has expired
    pub is_expired: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websocket_bidirectional::config::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_elicitation_capacity_check() {
        let config = WebSocketBidirectionalConfig {
            max_concurrent_elicitations: 2,
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert!(!transport.is_at_elicitation_capacity());
        assert_eq!(transport.pending_elicitations_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_elicitation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return false for non-existent elicitation
        assert!(!transport.cancel_elicitation("non-existent"));
    }

    #[tokio::test]
    async fn test_cancel_all_elicitations() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return 0 when no elicitations exist
        assert_eq!(transport.cancel_all_elicitations(), 0);
    }

    #[tokio::test]
    async fn test_get_elicitation_info() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return None for non-existent elicitation
        assert!(transport.get_elicitation_info("non-existent").is_none());
    }

    #[tokio::test]
    async fn test_get_all_elicitation_info() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return empty vector when no elicitations exist
        assert!(transport.get_all_elicitation_info().is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_expired_elicitations() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return 0 when no expired elicitations exist
        assert_eq!(transport.cleanup_expired_elicitations(), 0);
    }

    #[tokio::test]
    async fn test_get_pending_elicitation_ids() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should return empty vector when no elicitations exist
        assert!(transport.get_pending_elicitation_ids().is_empty());
    }

    #[tokio::test]
    async fn test_process_incoming_message_invalid_json() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let result = transport
            .process_incoming_message("invalid json".to_string())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));
    }

    #[tokio::test]
    async fn test_process_incoming_message_valid_json() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let json_message = json!({
            "jsonrpc": "2.0",
            "result": "success"
        });

        let result = transport
            .process_incoming_message(json_message.to_string())
            .await;
        assert!(result.is_ok());
    }
}
