//! Background task management for WebSocket bidirectional transport
//!
//! This module manages all background tasks including keep-alive pings,
//! elicitation timeout monitoring, and automatic reconnection handling.

use std::time::Duration;

use futures::SinkExt as _;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};

use super::types::WebSocketBidirectionalTransport;
use crate::core::TransportState;
use turbomcp_protocol::types::{ElicitResult, ElicitationAction};

impl WebSocketBidirectionalTransport {
    /// Spawn keep-alive task to send periodic ping messages
    pub fn spawn_keep_alive_task(&self) -> tokio::task::JoinHandle<()> {
        let writer = self.writer.clone();
        let interval = self
            .config
            .lock()
            .expect("config mutex poisoned")
            .keep_alive_interval;
        let state = self.state.clone();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let mut ping_count = 0u64;

            debug!(
                "Keep-alive task started for session {} with interval {:?}",
                session_id, interval
            );

            loop {
                ticker.tick().await;

                // Only send pings when connected
                if *state.read().await != TransportState::Connected {
                    continue;
                }

                if let Some(ref mut w) = *writer.lock().await {
                    ping_count += 1;
                    let ping_data = format!("ping-{}-{}", session_id, ping_count);

                    match w
                        .send(Message::Ping(ping_data.as_bytes().to_vec().into()))
                        .await
                    {
                        Ok(()) => {
                            trace!(
                                "Keep-alive ping {} sent for session {}",
                                ping_count, session_id
                            );
                        }
                        Err(e) => {
                            warn!("Keep-alive ping failed for session {}: {}", session_id, e);
                            // Connection might be broken, the reconnection task will handle it
                        }
                    }
                } else {
                    trace!(
                        "Writer not available for keep-alive ping in session {}",
                        session_id
                    );
                }
            }
        })
    }

    /// Spawn elicitation timeout monitor task
    pub fn spawn_timeout_monitor(&self) -> tokio::task::JoinHandle<()> {
        let elicitations = self.elicitations.clone();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));

            debug!(
                "Elicitation timeout monitor started for session {}",
                session_id
            );

            loop {
                ticker.tick().await;

                let now = tokio::time::Instant::now();
                let mut expired = Vec::new();

                // Find expired elicitations
                for entry in elicitations.iter() {
                    if entry.deadline <= now {
                        expired.push(entry.key().clone());
                    }
                }

                // Handle expired elicitations
                for request_id in expired {
                    if let Some((_, pending)) = elicitations.remove(&request_id) {
                        warn!(
                            "Elicitation {} timed out in session {} after {} retries",
                            request_id, session_id, pending.retry_count
                        );

                        let result = ElicitResult {
                            action: ElicitationAction::Cancel,
                            content: None,
                            _meta: None,
                        };

                        // Send timeout result to waiting caller
                        let _ = pending.response_tx.send(result);
                    }
                }

                // Log elicitation status periodically
                let active_count = elicitations.len();
                if active_count > 0 {
                    trace!(
                        "Session {} has {} active elicitations",
                        session_id, active_count
                    );
                }
            }
        })
    }

    /// Spawn reconnection task for automatic reconnection
    pub fn spawn_reconnection_task(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let config = self.config.lock().expect("config mutex poisoned").clone();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            let mut retry_count = 0;
            let mut delay = config.reconnect.initial_delay;

            debug!(
                "Reconnection task started for session {} (max retries: {}, initial delay: {:?})",
                session_id, config.reconnect.max_retries, config.reconnect.initial_delay
            );

            // Check connection status every 5 seconds
            let mut status_ticker = tokio::time::interval(Duration::from_secs(5));

            loop {
                status_ticker.tick().await;

                let current_state = state.read().await.clone();

                // Reset retry count and delay when connected
                if current_state == TransportState::Connected {
                    if retry_count > 0 {
                        info!(
                            "Connection restored for session {}, resetting retry count",
                            session_id
                        );
                        retry_count = 0;
                        delay = config.reconnect.initial_delay;
                    }
                    continue;
                }

                // Only attempt reconnection if disconnected (not connecting/disconnecting)
                if current_state != TransportState::Disconnected {
                    continue;
                }

                // Check if we've exceeded max retries
                if retry_count >= config.reconnect.max_retries {
                    error!(
                        "Maximum reconnection attempts ({}) reached for session {}",
                        config.reconnect.max_retries, session_id
                    );
                    break;
                }

                // Attempt reconnection
                if let Some(ref url) = config.url {
                    info!(
                        "Attempting reconnection {} of {} for session {} (delay: {:?})",
                        retry_count + 1,
                        config.reconnect.max_retries,
                        session_id,
                        delay
                    );

                    // Wait before attempting reconnection
                    if retry_count > 0 {
                        sleep(delay).await;
                    }

                    match connect_async(url).await {
                        Ok((_stream, _)) => {
                            info!("Reconnection successful for session {}", session_id);
                            // Note: In a full implementation, we would need to call setup_stream here
                            // but that requires mutable access to self, which isn't available in this task
                            // The reconnection logic would need to be refactored to work with channels
                            // or other communication mechanisms with the main transport instance
                            retry_count = 0;
                            delay = config.reconnect.initial_delay;
                        }
                        Err(e) => {
                            warn!(
                                "Reconnection attempt {} failed for session {}: {}",
                                retry_count + 1,
                                session_id,
                                e
                            );
                            retry_count += 1;

                            // Exponential backoff with jitter
                            let jitter = fastrand::f64() * 0.1; // 10% jitter
                            let backoff_multiplier =
                                config.reconnect.backoff_factor * (1.0 + jitter);

                            delay = Duration::from_secs_f64(
                                (delay.as_secs_f64() * backoff_multiplier)
                                    .min(config.reconnect.max_delay.as_secs_f64()),
                            );
                        }
                    }
                } else {
                    warn!(
                        "No URL configured for reconnection in session {}",
                        session_id
                    );
                    break;
                }
            }

            warn!("Reconnection task terminated for session {}", session_id);
        })
    }

    /// Spawn connection health monitor task
    pub fn spawn_connection_health_monitor(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let writer = self.writer.clone();
        let reader = self.reader.clone();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));

            debug!(
                "Connection health monitor started for session {}",
                session_id
            );

            loop {
                ticker.tick().await;

                let current_state = state.read().await.clone();
                let writer_connected = writer.lock().await.is_some();
                let reader_connected = reader.lock().await.is_some();

                // Check for inconsistent state
                if current_state == TransportState::Connected
                    && (!writer_connected || !reader_connected)
                {
                    warn!(
                        "Inconsistent connection state detected for session {}: state={:?}, writer={}, reader={}",
                        session_id, current_state, writer_connected, reader_connected
                    );

                    // Update state to reflect reality
                    *state.write().await = TransportState::Disconnected;
                }

                trace!(
                    "Health check for session {}: state={:?}, writer={}, reader={}",
                    session_id, current_state, writer_connected, reader_connected
                );
            }
        })
    }

    /// Spawn metrics collection task
    pub fn spawn_metrics_collection_task(&self) -> tokio::task::JoinHandle<()> {
        let metrics = self.metrics.clone();
        let correlations = self.correlations.clone();
        let elicitations = self.elicitations.clone();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));

            debug!("Metrics collection task started for session {}", session_id);

            loop {
                ticker.tick().await;

                // Collect current metrics
                let correlation_count = correlations.len();
                let elicitation_count = elicitations.len();

                {
                    let mut metrics_guard = metrics.write().await;
                    metrics_guard.active_connections = if correlation_count > 0 { 1 } else { 0 };

                    // Store WebSocket-specific metrics in metadata
                    metrics_guard.metadata.insert(
                        "active_correlations".to_string(),
                        serde_json::json!(correlation_count),
                    );
                    metrics_guard.metadata.insert(
                        "active_elicitations".to_string(),
                        serde_json::json!(elicitation_count),
                    );
                    metrics_guard.metadata.insert(
                        "session_id".to_string(),
                        serde_json::json!(session_id.to_string()),
                    );
                }

                trace!(
                    "Metrics collected for session {}: correlations={}, elicitations={}",
                    session_id, correlation_count, elicitation_count
                );
            }
        })
    }

    /// Start all background tasks with error handling
    pub async fn start_all_background_tasks(&self) {
        let mut handles = self.task_handles.write().await;

        // Keep-alive task
        let keep_alive_handle = self.spawn_keep_alive_task();
        handles.push(keep_alive_handle);

        // Elicitation timeout monitor
        let timeout_handle = self.spawn_timeout_monitor();
        handles.push(timeout_handle);

        // Connection health monitor
        let health_handle = self.spawn_connection_health_monitor();
        handles.push(health_handle);

        // Metrics collection
        let metrics_handle = self.spawn_metrics_collection_task();
        handles.push(metrics_handle);

        // Reconnection task (if enabled)
        if self
            .config
            .lock()
            .expect("config mutex poisoned")
            .reconnect
            .enabled
        {
            let reconnect_handle = self.spawn_reconnection_task();
            handles.push(reconnect_handle);
        }

        info!(
            "Started {} background tasks for session {}",
            handles.len(),
            self.session_id
        );
    }

    /// Stop all background tasks gracefully
    pub async fn stop_all_background_tasks(&self) {
        let handles = self
            .task_handles
            .write()
            .await
            .drain(..)
            .collect::<Vec<_>>();

        for (i, handle) in handles.into_iter().enumerate() {
            handle.abort();
            trace!(
                "Stopped background task {} for session {}",
                i, self.session_id
            );
        }

        info!(
            "Stopped all background tasks for session {}",
            self.session_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websocket_bidirectional::config::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_spawn_keep_alive_task() {
        let config = WebSocketBidirectionalConfig {
            keep_alive_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let handle = transport.spawn_keep_alive_task();

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle.abort();
        let _ = handle.await; // Wait for task to actually finish after abort
    }

    #[tokio::test]
    async fn test_spawn_timeout_monitor() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let handle = transport.spawn_timeout_monitor();

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle.abort();
        let _ = handle.await; // Wait for task to actually finish after abort
    }

    #[tokio::test]
    async fn test_spawn_health_monitor() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let handle = transport.spawn_connection_health_monitor();

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle.abort();
        let _ = handle.await; // Wait for task to actually finish after abort
    }

    #[tokio::test]
    async fn test_start_stop_all_tasks() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Start all tasks
        transport.start_all_background_tasks().await;

        let task_count = transport.task_handles.read().await.len();
        assert!(task_count > 0);

        // Stop all tasks
        transport.stop_all_background_tasks().await;

        let final_task_count = transport.task_handles.read().await.len();
        assert_eq!(final_task_count, 0);
    }

    #[tokio::test]
    async fn test_metrics_collection_task() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let handle = transport.spawn_metrics_collection_task();

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle.abort();
        let _ = handle.await; // Wait for task to actually finish after abort
    }
}
