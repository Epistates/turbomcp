//! MCP application state management
//!
//! This module defines the shared state structure used by Axum applications
//! to manage MCP services, sessions, and configuration.

#[cfg(feature = "http")]
use std::sync::Arc;

#[cfg(feature = "http")]
use tokio::sync::broadcast;

#[cfg(feature = "http")]
use super::McpService;
#[cfg(feature = "http")]
use crate::axum::config::McpServerConfig;
#[cfg(feature = "http")]
use crate::tower::SessionManager;

#[cfg(feature = "http")]
/// Shared state for Axum application using trait objects for flexibility
///
/// This state is cloned for each request handler and provides access
/// to the MCP service, session management, and configuration.
#[derive(Clone)]
pub struct McpAppState {
    /// MCP service instance (trait object for flexibility)
    pub service: Arc<dyn McpService>,

    /// Session manager for tracking client sessions
    pub session_manager: Arc<SessionManager>,

    /// SSE broadcast sender for real-time updates
    pub sse_sender: broadcast::Sender<String>,

    /// Configuration options
    pub config: McpServerConfig,
}

#[cfg(feature = "http")]
impl std::fmt::Debug for McpAppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAppState")
            .field("service", &"<dyn McpService>")
            .field("session_manager", &self.session_manager)
            .field("sse_sender", &"<broadcast::Sender>")
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(feature = "http")]
impl McpAppState {
    /// Create new application state
    pub fn new(
        service: Arc<dyn McpService>,
        session_manager: Arc<SessionManager>,
        config: McpServerConfig,
    ) -> Self {
        let (sse_sender, _) = broadcast::channel(1000);

        Self {
            service,
            session_manager,
            sse_sender,
            config,
        }
    }

    /// Get a receiver for SSE broadcasts
    pub fn subscribe_sse(&self) -> broadcast::Receiver<String> {
        self.sse_sender.subscribe()
    }

    /// Broadcast an SSE event to all connected clients
    pub fn broadcast_sse(
        &self,
        event: String,
    ) -> Result<usize, broadcast::error::SendError<String>> {
        self.sse_sender.send(event)
    }

    /// Get service capabilities
    pub fn get_capabilities(&self) -> serde_json::Value {
        self.service.get_capabilities()
    }

    /// Process an MCP request
    pub async fn process_request(
        &self,
        request: serde_json::Value,
        session: &crate::tower::SessionInfo,
    ) -> turbomcp_core::Result<serde_json::Value> {
        self.service.process_request(request, session).await
    }
}
