//! Bidirectional transport implementation for WebSocket
//!
//! This module implements the BidirectionalTransport trait, providing
//! request-response patterns with correlation handling and timeout management.

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::types::WebSocketBidirectionalTransport;
use turbomcp_transport_traits::{
    BidirectionalTransport, CorrelationContext, Transport, TransportError, TransportMessage,
    TransportResult,
};

#[async_trait]
impl BidirectionalTransport for WebSocketBidirectionalTransport {
    async fn send_request(
        &self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage> {
        let correlation_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Store correlation
        let ctx = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: message.id.to_string(),
            response_tx: Some(tx),
            timeout: timeout.unwrap_or(Duration::from_secs(30)),
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id.clone(), ctx);

        // Add correlation ID to message metadata
        let mut message = message;
        message.metadata.correlation_id = Some(correlation_id.clone());

        // Send the message
        self.send(message).await?;

        // Wait for response
        match timeout {
            Some(duration) => match tokio::time::timeout(duration, rx).await {
                Ok(Ok(response)) => {
                    tracing::debug!(
                        "Received response for correlation {} in session {}",
                        correlation_id,
                        self.session_id
                    );
                    Ok(response)
                }
                Ok(Err(_)) => {
                    self.correlations.remove(&correlation_id);
                    Err(TransportError::ReceiveFailed("Channel closed".to_string()))
                }
                Err(_) => {
                    self.correlations.remove(&correlation_id);
                    tracing::warn!(
                        "Request timed out for correlation {} in session {}",
                        correlation_id,
                        self.session_id
                    );
                    Err(TransportError::Timeout)
                }
            },
            None => {
                let response = rx.await.map_err(|_| {
                    self.correlations.remove(&correlation_id);
                    TransportError::ReceiveFailed("Channel closed".to_string())
                })?;
                tracing::debug!(
                    "Received response for correlation {} in session {} (no timeout)",
                    correlation_id,
                    self.session_id
                );
                Ok(response)
            }
        }
    }

    async fn start_correlation(&self, correlation_id: String) -> TransportResult<()> {
        // Create a correlation context to track request-response pairs
        let ctx = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: String::new(),
            response_tx: None,
            timeout: Duration::from_secs(30),
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id.clone(), ctx);
        tracing::debug!(
            "Started correlation {} in session {}",
            correlation_id,
            self.session_id
        );
        Ok(())
    }

    async fn stop_correlation(&self, correlation_id: &str) -> TransportResult<()> {
        let removed = self.correlations.remove(correlation_id).is_some();
        if removed {
            tracing::debug!(
                "Stopped correlation {} in session {}",
                correlation_id,
                self.session_id
            );
        } else {
            tracing::warn!(
                "Attempted to stop non-existent correlation {} in session {}",
                correlation_id,
                self.session_id
            );
        }
        Ok(())
    }
}

impl WebSocketBidirectionalTransport {
    /// Send a request with retry capability
    pub async fn send_request_with_retry(
        &self,
        message: TransportMessage,
        max_retries: u32,
        retry_delay: Duration,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= max_retries {
            match self.send_request(message.clone(), timeout).await {
                Ok(response) => {
                    if attempts > 0 {
                        tracing::debug!(
                            "Request succeeded after {} retries in session {}",
                            attempts,
                            self.session_id
                        );
                    }
                    return Ok(response);
                }
                Err(e) => {
                    last_error = Some(e);
                    attempts += 1;

                    if attempts <= max_retries {
                        tracing::debug!(
                            "Request attempt {} failed in session {}, retrying after {:?}",
                            attempts,
                            self.session_id,
                            retry_delay
                        );
                        tokio::time::sleep(retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TransportError::SendFailed("All request retry attempts failed".to_string())
        }))
    }

    /// Get all active correlation IDs
    pub fn get_active_correlation_ids(&self) -> Vec<String> {
        self.correlations
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get correlation information
    pub fn get_correlation_info(&self, correlation_id: &str) -> Option<CorrelationInfo> {
        self.correlations.get(correlation_id).map(|entry| {
            let ctx = entry.value();
            CorrelationInfo {
                correlation_id: ctx.correlation_id.clone(),
                request_id: ctx.request_id.clone(),
                timeout: ctx.timeout,
                created_at: ctx.created_at,
                elapsed: ctx.created_at.elapsed(),
                has_response_channel: ctx.response_tx.is_some(),
            }
        })
    }

    /// Get information about all active correlations
    pub fn get_all_correlation_info(&self) -> Vec<CorrelationInfo> {
        self.correlations
            .iter()
            .map(|entry| {
                let ctx = entry.value();
                CorrelationInfo {
                    correlation_id: ctx.correlation_id.clone(),
                    request_id: ctx.request_id.clone(),
                    timeout: ctx.timeout,
                    created_at: ctx.created_at,
                    elapsed: ctx.created_at.elapsed(),
                    has_response_channel: ctx.response_tx.is_some(),
                }
            })
            .collect()
    }

    /// Cancel a specific correlation
    pub fn cancel_correlation(&self, correlation_id: &str) -> bool {
        let removed = self.correlations.remove(correlation_id).is_some();
        if removed {
            tracing::debug!(
                "Cancelled correlation {} in session {}",
                correlation_id,
                self.session_id
            );
        }
        removed
    }

    /// Cancel all active correlations
    pub fn cancel_all_correlations(&self) -> usize {
        let count = self.correlations.len();
        self.correlations.clear();
        if count > 0 {
            tracing::debug!(
                "Cancelled {} correlations in session {}",
                count,
                self.session_id
            );
        }
        count
    }

    /// Clean up expired correlations
    pub fn cleanup_expired_correlations(&self) -> usize {
        let now = std::time::Instant::now();
        let mut expired_ids = Vec::new();

        // Find expired correlations
        for entry in self.correlations.iter() {
            let ctx = entry.value();
            if now.duration_since(ctx.created_at) >= ctx.timeout {
                expired_ids.push(entry.key().clone());
            }
        }

        // Remove expired correlations
        let mut cleaned_count = 0;
        for correlation_id in expired_ids {
            if self.correlations.remove(&correlation_id).is_some() {
                cleaned_count += 1;
            }
        }

        if cleaned_count > 0 {
            tracing::debug!(
                "Cleaned up {} expired correlations in session {}",
                cleaned_count,
                self.session_id
            );
        }

        cleaned_count
    }

    /// Check if a correlation exists
    pub fn has_correlation(&self, correlation_id: &str) -> bool {
        self.correlations.contains_key(correlation_id)
    }

    /// Get the number of correlations that have response channels
    pub fn pending_response_count(&self) -> usize {
        self.correlations
            .iter()
            .filter(|entry| entry.response_tx.is_some())
            .count()
    }

    /// Get the number of correlations without response channels (tracking only)
    pub fn tracking_only_correlation_count(&self) -> usize {
        self.correlations
            .iter()
            .filter(|entry| entry.response_tx.is_none())
            .count()
    }

    /// Process a response message with correlation
    pub async fn process_correlation_response(
        &self,
        correlation_id: &str,
        response: TransportMessage,
    ) -> bool {
        if let Some((_, ctx)) = self.correlations.remove(correlation_id) {
            if let Some(tx) = ctx.response_tx {
                let sent = tx.send(response).is_ok();
                tracing::debug!(
                    "Processed correlation response for {} in session {} (sent: {})",
                    correlation_id,
                    self.session_id,
                    sent
                );
                sent
            } else {
                tracing::debug!(
                    "Correlation {} in session {} was tracking-only, no response channel",
                    correlation_id,
                    self.session_id
                );
                false
            }
        } else {
            tracing::warn!(
                "Received response for unknown correlation {} in session {}",
                correlation_id,
                self.session_id
            );
            false
        }
    }
}

/// Information about a correlation
#[derive(Debug, Clone)]
pub struct CorrelationInfo {
    /// Correlation ID
    pub correlation_id: String,
    /// Associated request ID
    pub request_id: String,
    /// Timeout duration
    pub timeout: Duration,
    /// When the correlation was created
    pub created_at: std::time::Instant,
    /// Time elapsed since creation
    pub elapsed: Duration,
    /// Whether there's a response channel waiting
    pub has_response_channel: bool,
}

impl CorrelationInfo {
    /// Check if the correlation has expired
    pub fn is_expired(&self) -> bool {
        self.elapsed >= self.timeout
    }

    /// Get time remaining until timeout
    pub fn time_remaining(&self) -> Duration {
        if self.is_expired() {
            Duration::ZERO
        } else {
            self.timeout - self.elapsed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WebSocketBidirectionalConfig;
    use bytes::Bytes;
    use turbomcp_protocol::MessageId;
    use turbomcp_transport_traits::TransportMessageMetadata;

    #[tokio::test]
    async fn test_start_stop_correlation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let correlation_id = "test-correlation";

        // Start correlation
        let result = transport
            .start_correlation(correlation_id.to_string())
            .await;
        assert!(result.is_ok());
        assert!(transport.has_correlation(correlation_id));

        // Stop correlation
        let result = transport.stop_correlation(correlation_id).await;
        assert!(result.is_ok());
        assert!(!transport.has_correlation(correlation_id));
    }

    #[tokio::test]
    async fn test_get_correlation_info() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let correlation_id = "test-correlation";
        transport
            .start_correlation(correlation_id.to_string())
            .await
            .unwrap();

        let info = transport.get_correlation_info(correlation_id);
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.correlation_id, correlation_id);
        assert!(!info.has_response_channel);
    }

    #[tokio::test]
    async fn test_get_all_correlation_info() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        transport
            .start_correlation("corr1".to_string())
            .await
            .unwrap();
        transport
            .start_correlation("corr2".to_string())
            .await
            .unwrap();

        let all_info = transport.get_all_correlation_info();
        assert_eq!(all_info.len(), 2);
    }

    #[tokio::test]
    async fn test_cancel_correlation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let correlation_id = "test-correlation";
        transport
            .start_correlation(correlation_id.to_string())
            .await
            .unwrap();

        let cancelled = transport.cancel_correlation(correlation_id);
        assert!(cancelled);
        assert!(!transport.has_correlation(correlation_id));

        // Cancelling again should return false
        let cancelled_again = transport.cancel_correlation(correlation_id);
        assert!(!cancelled_again);
    }

    #[tokio::test]
    async fn test_cancel_all_correlations() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        transport
            .start_correlation("corr1".to_string())
            .await
            .unwrap();
        transport
            .start_correlation("corr2".to_string())
            .await
            .unwrap();

        let cancelled_count = transport.cancel_all_correlations();
        assert_eq!(cancelled_count, 2);
        assert_eq!(transport.active_correlations_count(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired_correlations() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // No correlations to clean up
        let cleaned = transport.cleanup_expired_correlations();
        assert_eq!(cleaned, 0);
    }

    #[tokio::test]
    async fn test_active_correlation_ids() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(transport.get_active_correlation_ids().len(), 0);

        transport
            .start_correlation("corr1".to_string())
            .await
            .unwrap();
        transport
            .start_correlation("corr2".to_string())
            .await
            .unwrap();

        let ids = transport.get_active_correlation_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"corr1".to_string()));
        assert!(ids.contains(&"corr2".to_string()));
    }

    #[tokio::test]
    async fn test_correlation_counts() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // All tracking-only correlations initially
        transport
            .start_correlation("corr1".to_string())
            .await
            .unwrap();
        transport
            .start_correlation("corr2".to_string())
            .await
            .unwrap();

        assert_eq!(transport.pending_response_count(), 0);
        assert_eq!(transport.tracking_only_correlation_count(), 2);
    }

    #[tokio::test]
    async fn test_correlation_info_methods() {
        let info = CorrelationInfo {
            correlation_id: "test".to_string(),
            request_id: "req-1".to_string(),
            timeout: Duration::from_secs(30),
            created_at: std::time::Instant::now() - Duration::from_secs(10),
            elapsed: Duration::from_secs(10),
            has_response_channel: true,
        };

        assert!(!info.is_expired());
        assert_eq!(info.time_remaining(), Duration::from_secs(20));

        let expired_info = CorrelationInfo {
            correlation_id: "test".to_string(),
            request_id: "req-1".to_string(),
            timeout: Duration::from_secs(5),
            created_at: std::time::Instant::now() - Duration::from_secs(10),
            elapsed: Duration::from_secs(10),
            has_response_channel: true,
        };

        assert!(expired_info.is_expired());
        assert_eq!(expired_info.time_remaining(), Duration::ZERO);
    }

    #[tokio::test]
    async fn test_process_correlation_response() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let correlation_id = "test-correlation";
        transport
            .start_correlation(correlation_id.to_string())
            .await
            .unwrap();

        let response = TransportMessage {
            id: MessageId::from(Uuid::new_v4()),
            payload: Bytes::from("response"),
            metadata: TransportMessageMetadata::default(),
        };

        // Should process successfully (even for tracking-only)
        let processed = transport
            .process_correlation_response(correlation_id, response)
            .await;
        assert!(!processed); // No response channel for tracking-only correlation
        assert!(!transport.has_correlation(correlation_id)); // Should be removed
    }
}
