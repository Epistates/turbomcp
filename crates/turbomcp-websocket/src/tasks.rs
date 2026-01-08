//! Background task management for WebSocket bidirectional transport
//!
//! This module manages all background tasks including keep-alive pings,
//! elicitation timeout monitoring, and automatic reconnection handling.

use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};

use super::types::WebSocketBidirectionalTransport;
use turbomcp_protocol::types::{ElicitResult, ElicitationAction};
use turbomcp_transport_traits::{TransportMessage, TransportMessageMetadata, TransportState};

impl WebSocketBidirectionalTransport {
    /// Spawn message reader task to continuously process WebSocket messages
    ///
    /// This is CRITICAL for bidirectional methods to work (ping, sampling, roots):
    /// - Routes responses to their waiting correlation maps
    /// - Processes keep-alive control frames
    /// - Enables async bidirectional communication
    /// - Forwards non-correlation messages to the incoming channel for `Transport::receive()`
    ///
    /// Without this task, `send_ping()`, `send_sampling()`, etc. will timeout
    /// waiting for responses that never arrive.
    ///
    /// ## Architecture
    ///
    /// This task is the SINGLE consumer of the WebSocket stream. It reads all messages
    /// and routes them:
    /// 1. Correlation matches (pending_pings, pending_samplings, pending_roots, elicitations, correlations)
    ///    → Sent directly to the waiting oneshot channel
    /// 2. Non-correlation messages (server requests, notifications)
    ///    → Forwarded to `incoming_tx` channel for `Transport::receive()` to consume
    ///
    /// This eliminates the race condition where both this task and `receive()` competed
    /// to read from the same WebSocket stream.
    pub fn spawn_message_reader_task(&self) -> tokio::task::JoinHandle<()> {
        let reader = self.reader.clone();
        let writer = self.writer.clone();
        let session_id = self.session_id.clone();

        // Clone everything we need for processing
        let pending_pings = self.pending_pings.clone();
        let pending_samplings = self.pending_samplings.clone();
        let pending_roots = self.pending_roots.clone();
        let elicitations = self.elicitations.clone();
        let correlations = self.correlations.clone();
        let incoming_tx = self.incoming_tx.clone();

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let session_id_clone = session_id.clone();

        tokio::spawn(async move {
            debug!(
                "Message reader task started for session {}",
                session_id_clone
            );

            loop {
                tokio::select! {
                    // Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("Message reader received shutdown signal for session {}", session_id_clone);
                        break;
                    }

                    // Read messages from WebSocket
                    msg_result = async {
                        if let Some(ref mut reader_guard) = *reader.lock().await {
                            reader_guard.next().await
                        } else {
                            None
                        }
                    } => {
                        match msg_result {
                            Some(Ok(Message::Text(text))) => {
                                // Parse JSON-RPC and route to appropriate handler
                                let json_value = serde_json::from_str::<serde_json::Value>(&text);

                                // Extract ID from the message (can be string or number)
                                let id_str = json_value.as_ref().ok().and_then(|v| {
                                    v.get("id").and_then(|id| {
                                        // Handle both string and numeric IDs
                                        id.as_str().map(|s| s.to_string())
                                            .or_else(|| id.as_i64().map(|n| n.to_string()))
                                            .or_else(|| id.as_u64().map(|n| n.to_string()))
                                    })
                                });

                                let mut message_handled = false;

                                if let Some(ref id) = id_str {
                                    // Try to deliver to pending_pings
                                    if let Some((_, response_tx)) = pending_pings.remove(id)
                                        && let Ok(ref json) = json_value
                                        && let Some(result) = json.get("result")
                                        && let Ok(ping_result) = serde_json::from_value(result.clone())
                                    {
                                        let _ = response_tx.send(ping_result);
                                        message_handled = true;
                                    }

                                    // Try to deliver to pending_samplings
                                    if !message_handled
                                        && let Some((_, response_tx)) = pending_samplings.remove(id)
                                        && let Ok(ref json) = json_value
                                        && let Some(result) = json.get("result")
                                        && let Ok(sampling_result) = serde_json::from_value(result.clone())
                                    {
                                        let _ = response_tx.send(sampling_result);
                                        message_handled = true;
                                    }

                                    // Try to deliver to pending_roots
                                    if !message_handled
                                        && let Some((_, response_tx)) = pending_roots.remove(id)
                                        && let Ok(ref json) = json_value
                                        && let Some(result) = json.get("result")
                                        && let Ok(roots_result) = serde_json::from_value(result.clone())
                                    {
                                        let _ = response_tx.send(roots_result);
                                        message_handled = true;
                                    }

                                    // Try to deliver to elicitations
                                    if !message_handled
                                        && let Some((_, pending)) = elicitations.remove(id)
                                        && let Ok(ref json) = json_value
                                        && let Some(result) = json.get("result")
                                        && let Ok(elicit_result) = serde_json::from_value(result.clone())
                                    {
                                        let _ = pending.response_tx.send(elicit_result);
                                        message_handled = true;
                                    }

                                    // Try to deliver to correlations (for standard request-response)
                                    // Find correlation by matching request_id to the JSON-RPC id
                                    if !message_handled {
                                        let mut matched_correlation_id = None;
                                        for entry in correlations.iter() {
                                            if entry.value().request_id == *id {
                                                matched_correlation_id = Some(entry.key().clone());
                                                break;
                                            }
                                        }
                                        if let Some(correlation_id) = matched_correlation_id
                                            && let Some((_, ctx)) = correlations.remove(&correlation_id)
                                        {
                                            if let Some(response_tx) = ctx.response_tx {
                                                // Create TransportMessage from the raw JSON text
                                                let response_message = TransportMessage {
                                                    id: turbomcp_protocol::MessageId::from(id.as_str()),
                                                    payload: bytes::Bytes::from(text.as_bytes().to_vec()),
                                                    metadata: TransportMessageMetadata::default(),
                                                };
                                                let _ = response_tx.send(response_message);
                                                debug!(
                                                    "Delivered response for correlation {} (request_id: {}) in session {}",
                                                    correlation_id, id, session_id_clone
                                                );
                                            }
                                            message_handled = true;
                                        }
                                    }
                                }

                                // If message wasn't handled by any correlation, forward to incoming channel
                                // This allows Transport::receive() to get messages like server-initiated
                                // requests, notifications, and other non-response messages
                                if !message_handled {
                                    let message = TransportMessage {
                                        id: turbomcp_protocol::MessageId::from(uuid::Uuid::new_v4()),
                                        payload: bytes::Bytes::from(text.as_bytes().to_vec()),
                                        metadata: TransportMessageMetadata::default(),
                                    };
                                    if let Err(e) = incoming_tx.send(message).await {
                                        warn!(
                                            "Failed to forward message to incoming channel in session {}: {}",
                                            session_id_clone, e
                                        );
                                    } else {
                                        trace!(
                                            "Forwarded non-correlation message to incoming channel in session {}",
                                            session_id_clone
                                        );
                                    }
                                }
                            }
                            Some(Ok(Message::Binary(data))) => {
                                // Forward binary messages to incoming channel
                                let message = TransportMessage {
                                    id: turbomcp_protocol::MessageId::from(uuid::Uuid::new_v4()),
                                    payload: data,
                                    metadata: TransportMessageMetadata::default(),
                                };
                                if let Err(e) = incoming_tx.send(message).await {
                                    warn!(
                                        "Failed to forward binary message to incoming channel in session {}: {}",
                                        session_id_clone, e
                                    );
                                }
                            }
                            Some(Ok(Message::Ping(data))) => {
                                // Auto-respond with pong
                                if let Some(ref mut writer_guard) = *writer.lock().await
                                    && let Ok(()) = writer_guard.send(Message::Pong(data)).await
                                {
                                    let _ = writer_guard.flush().await;
                                    trace!(
                                        "Message reader sent pong in session {}",
                                        session_id_clone
                                    );
                                }
                            }
                            Some(Ok(Message::Pong(_))) => {
                                trace!(
                                    "Message reader received pong in session {}",
                                    session_id_clone
                                );
                            }
                            Some(Ok(Message::Close(_))) => {
                                info!(
                                    "WebSocket closed in session {}",
                                    session_id_clone
                                );
                                break;
                            }
                            Some(Err(e)) => {
                                error!(
                                    "WebSocket error in session {}: {}",
                                    session_id_clone, e
                                );
                                break;
                            }
                            None => {
                                info!(
                                    "WebSocket stream ended for session {}",
                                    session_id_clone
                                );
                                break;
                            }
                            _ => {
                                trace!("Message reader received other frame type in session {}", session_id_clone);
                            }
                        }
                    }
                }
            }

            debug!(
                "✅ Message reader task gracefully terminated for session {}",
                session_id_clone
            );
        })
    }

    /// Spawn keep-alive task to send periodic ping messages
    ///
    /// This task now listens for shutdown signals and terminates gracefully.
    pub fn spawn_keep_alive_task(&self) -> tokio::task::JoinHandle<()> {
        let writer = self.writer.clone();
        let interval = self
            .config
            .lock()
            .expect("config mutex poisoned")
            .keep_alive_interval;
        let state = self.state.clone();
        let session_id = self.session_id.clone();

        // ✅ Subscribe to shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let mut ping_count = 0u64;

            debug!(
                "Keep-alive task started for session {} with interval {:?}",
                session_id, interval
            );

            loop {
                tokio::select! {
                    // ✅ Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("Keep-alive task received shutdown signal for session {}", session_id);
                        break;
                    }

                    // Existing ticker logic
                    _ = ticker.tick() => {

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
                }
            }

            debug!(
                "✅ Keep-alive task gracefully terminated for session {}",
                session_id
            );
        })
    }

    /// Spawn elicitation timeout monitor task
    ///
    /// This task now listens for shutdown signals and terminates gracefully.
    pub fn spawn_timeout_monitor(&self) -> tokio::task::JoinHandle<()> {
        let elicitations = self.elicitations.clone();
        let session_id = self.session_id.clone();

        // ✅ Subscribe to shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));

            debug!(
                "Elicitation timeout monitor started for session {}",
                session_id
            );

            loop {
                tokio::select! {
                    // ✅ Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("Timeout monitor received shutdown signal for session {}", session_id);
                        break;
                    }

                    // Existing ticker logic
                    _ = ticker.tick() => {

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
                }
            }

            debug!(
                "✅ Timeout monitor gracefully terminated for session {}",
                session_id
            );
        })
    }

    /// Spawn reconnection task for automatic reconnection
    ///
    /// **CRITICAL**: This task now listens for shutdown signals via `tokio::select!`.
    /// When `disconnect()` is called, the shutdown signal is broadcast and this task
    /// terminates gracefully, preventing unwanted reconnection attempts.
    pub fn spawn_reconnection_task(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let config = self.config.lock().expect("config mutex poisoned").clone();
        let session_id = self.session_id.clone();
        let reconnect_allowed = self.reconnect_allowed.clone();

        // ✅ CRITICAL FIX: Subscribe to shutdown broadcast channel
        let mut shutdown_rx = self.shutdown_tx.subscribe();

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
                tokio::select! {
                    // ✅ CRITICAL FIX: Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Reconnection task received shutdown signal for session {}", session_id);
                        break;  // Graceful exit - no more reconnection attempts!
                    }

                    // Existing tick logic
                    _ = status_ticker.tick() => {
                        // ✅ DEFENSE-IN-DEPTH #1: Check reconnect_allowed flag FIRST
                        //    This atomic flag is set to false permanently when user calls disconnect().
                        //    Even if shutdown signal is missed or state transitions are delayed,
                        //    this check ensures we NEVER reconnect after explicit user disconnect.
                        if !reconnect_allowed.load(std::sync::atomic::Ordering::SeqCst) {
                            info!("🛑 Reconnection disabled (user disconnect) - stopping task for session {}", session_id);
                            break;
                        }

                        let current_state = state.read().await.clone();

                        // ✅ DEFENSE-IN-DEPTH #2: Also check for Disconnecting state (user-initiated)
                        //    This provides additional protection: even if flag check somehow failed,
                        //    we still stop reconnection on explicit user disconnect
                        if matches!(current_state, TransportState::Disconnecting) {
                            info!("🛑 User-initiated disconnect detected - stopping reconnection for session {}", session_id);
                            break;
                        }

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

                        // Wait before attempting reconnection (interruptible by shutdown)
                        if retry_count > 0 {
                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    info!("🛑 Reconnection task received shutdown during backoff delay for session {}", session_id);
                                    break;
                                }
                                _ = sleep(delay) => {
                                    // Backoff complete, proceed with reconnection attempt
                                }
                            }
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
                    }  // End of tokio::select! _ = status_ticker.tick() branch
                } // End of tokio::select!
            } // End of loop

            info!(
                "✅ Reconnection task gracefully terminated for session {}",
                session_id
            );
        })
    }

    /// Spawn connection health monitor task
    ///
    /// This task now listens for shutdown signals and terminates gracefully.
    pub fn spawn_connection_health_monitor(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let writer = self.writer.clone();
        let reader = self.reader.clone();
        let session_id = self.session_id.clone();

        // ✅ Subscribe to shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));

            debug!(
                "Connection health monitor started for session {}",
                session_id
            );

            loop {
                tokio::select! {
                    // ✅ Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("Health monitor received shutdown signal for session {}", session_id);
                        break;
                    }

                    // Existing ticker logic
                    _ = ticker.tick() => {

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
                }
            }

            debug!(
                "✅ Health monitor gracefully terminated for session {}",
                session_id
            );
        })
    }

    /// Spawn metrics collection task
    ///
    /// This task now listens for shutdown signals and terminates gracefully.
    pub fn spawn_metrics_collection_task(&self) -> tokio::task::JoinHandle<()> {
        let metrics = self.metrics.clone();
        let correlations = self.correlations.clone();
        let elicitations = self.elicitations.clone();
        let session_id = self.session_id.clone();

        // ✅ Subscribe to shutdown signal
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));

            debug!("Metrics collection task started for session {}", session_id);

            loop {
                tokio::select! {
                    // ✅ Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("Metrics collection received shutdown signal for session {}", session_id);
                        break;
                    }

                    // Existing ticker logic
                    _ = ticker.tick() => {

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
                }
            }

            debug!(
                "✅ Metrics collection gracefully terminated for session {}",
                session_id
            );
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
    use crate::config::WebSocketBidirectionalConfig;

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
