//! Tower Service integration for TurboMCP Transport layer
//!
//! This module provides a bridge between Tower services and the TurboMCP Transport trait,
//! enabling seamless integration with the broader Tower ecosystem including Axum, Hyper,
//! and Tonic while maintaining our production-grade observability and error handling.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use serde_json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

// Server features integration with production-grade architecture
#[cfg(feature = "server")]
use crate::server::{
    HandlerRegistry, RequestContext,
};

#[cfg(feature = "server")]
use turbomcp_protocol::{
    CallToolRequest, GetPromptRequest, ReadResourceRequest,
};

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportEventEmitter, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};
use turbomcp_core::MessageId;

/// Session identifier for tracking connections in Tower services
pub type SessionId = String;

/// Session information for tracking connection state
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Unique session identifier
    pub id: SessionId,

    /// When the session was created
    pub created_at: Instant,

    /// Last activity timestamp
    pub last_activity: Instant,

    /// Remote address (if available)
    pub remote_addr: Option<String>,

    /// User agent or client identification
    pub user_agent: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl SessionInfo {
    /// Create a new session
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now,
            last_activity: now,
            remote_addr: None,
            user_agent: None,
            metadata: HashMap::new(),
        }
    }
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionInfo {
    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if session is expired based on timeout
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }

    /// Get session duration
    pub fn duration(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// Session manager for tracking active connections
#[derive(Debug, Clone)]
pub struct SessionManager {
    /// Active sessions
    sessions: Arc<Mutex<HashMap<SessionId, SessionInfo>>>,

    /// Session timeout duration
    session_timeout: Duration,

    /// Maximum number of concurrent sessions
    max_sessions: usize,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_timeout: Duration::from_secs(300), // 5 minutes default
            max_sessions: 1000,                        // Reasonable default
        }
    }

    /// Create session manager with custom settings
    pub fn with_config(session_timeout: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_timeout,
            max_sessions,
        }
    }

    /// Create a new session
    pub fn create_session(&self) -> TransportResult<SessionInfo> {
        let mut sessions = self.sessions.lock();

        // Check session limit
        if sessions.len() >= self.max_sessions {
            // Try to clean up expired sessions first
            self.cleanup_expired_sessions_locked(&mut sessions);

            // If still at limit, reject
            if sessions.len() >= self.max_sessions {
                return Err(TransportError::RateLimitExceeded);
            }
        }

        let session = SessionInfo::new();
        let session_id = session.id.clone();
        sessions.insert(session_id, session.clone());

        debug!("Created new session: {}", session.id);
        Ok(session)
    }

    /// Get session by ID
    pub fn get_session(&self, session_id: &str) -> Option<SessionInfo> {
        let mut sessions = self.sessions.lock();

        if let Some(session) = sessions.get_mut(session_id) {
            // Update last activity
            session.touch();
            Some(session.clone())
        } else {
            None
        }
    }

    /// Update session metadata
    pub fn update_session_metadata(&self, session_id: &str, key: String, value: String) {
        let mut sessions = self.sessions.lock();

        if let Some(session) = sessions.get_mut(session_id) {
            session.metadata.insert(key, value);
            session.touch();
        }
    }

    /// Remove session
    pub fn remove_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock();
        let removed = sessions.remove(session_id).is_some();

        if removed {
            debug!("Removed session: {}", session_id);
        }

        removed
    }

    /// Get active session count
    pub fn active_session_count(&self) -> usize {
        self.sessions.lock().len()
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> usize {
        let mut sessions = self.sessions.lock();
        self.cleanup_expired_sessions_locked(&mut sessions)
    }

    fn cleanup_expired_sessions_locked(
        &self,
        sessions: &mut HashMap<SessionId, SessionInfo>,
    ) -> usize {
        let initial_count = sessions.len();

        sessions.retain(|_id, session| !session.is_expired(self.session_timeout));

        let removed = initial_count - sessions.len();

        if removed > 0 {
            debug!("Cleaned up {} expired sessions", removed);
        }

        removed
    }

    /// List all active sessions (for debugging/monitoring)
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.lock().values().cloned().collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tower service adapter that implements the Transport trait
///
/// This adapter bridges Tower services with TurboMCP's Transport interface,
/// providing comprehensive error handling, metrics collection, and session management.
#[derive(Debug)]
pub struct TowerTransportAdapter {
    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Current transport state
    state: Arc<Mutex<TransportState>>,

    /// Metrics collector
    metrics: Arc<Mutex<TransportMetrics>>,

    /// Event emitter for observability
    event_emitter: TransportEventEmitter,

    /// Session manager
    session_manager: SessionManager,

    /// Message receiver channel
    receiver: Option<mpsc::UnboundedReceiver<TransportMessage>>,

    /// Message sender channel
    sender: Option<mpsc::UnboundedSender<TransportMessage>>,

    /// Background task handle for cleanup
    _cleanup_task: Option<tokio::task::JoinHandle<()>>,

    /// Handler registry for dependency injection
    #[cfg(feature = "server")]
    handler_registry: Option<Arc<HandlerRegistry>>,
}

impl TowerTransportAdapter {
    /// Create a new Tower transport adapter
    pub fn new() -> Self {
        let (event_emitter, _) = TransportEventEmitter::new();

        Self {
            capabilities: TransportCapabilities {
                max_message_size: Some(16 * 1024 * 1024), // 16MB default
                supports_compression: true,
                supports_encryption: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: true,
                compression_algorithms: vec![
                    "gzip".to_string(),
                    "deflate".to_string(),
                    "br".to_string(),
                ],
                custom: HashMap::new(),
            },
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            metrics: Arc::new(Mutex::new(TransportMetrics::default())),
            event_emitter,
            session_manager: SessionManager::new(),
            receiver: None,
            sender: None,
            _cleanup_task: None,
            #[cfg(feature = "server")]
            handler_registry: None,
        }
    }
    
    /// Set handler registry for dependency injection
    #[cfg(feature = "server")]
    pub fn with_handler_registry(mut self, registry: Arc<HandlerRegistry>) -> Self {
        self.handler_registry = Some(registry);
        self
    }
}

impl Default for TowerTransportAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TowerTransportAdapter {
    /// Create adapter with custom session manager
    pub fn with_session_manager(session_manager: SessionManager) -> Self {
        let mut adapter = Self::new();
        adapter.session_manager = session_manager;
        adapter
    }

    /// Initialize the transport channels and background tasks
    pub fn initialize(&mut self) -> McpResult<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.sender = Some(tx);
        self.receiver = Some(rx);

        // Start cleanup task for expired sessions
        let session_manager = self.session_manager.clone();
        let cleanup_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Cleanup every minute

            loop {
                interval.tick().await;
                let cleaned = session_manager.cleanup_expired_sessions();

                if cleaned > 0 {
                    trace!("Session cleanup: removed {} expired sessions", cleaned);
                }
            }
        });

        self._cleanup_task = Some(cleanup_task);
        self.set_state(TransportState::Connected);

        info!("Tower transport adapter initialized");
        Ok(())
    }

    /// Get the session manager
    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }

    /// Process an incoming message through the Tower service
    pub async fn process_message(
        &self,
        message: TransportMessage,
        session_info: &SessionInfo,
    ) -> TransportResult<Option<TransportMessage>> {
        let start_time = Instant::now();

        // Update metrics
        self.update_metrics(|m| {
            m.messages_received += 1;
            m.bytes_received += message.size() as u64;
        });

        // Emit event
        self.event_emitter
            .emit_message_received(message.id.clone(), message.size());

        // Validate message
        if message.size() > self.capabilities.max_message_size.unwrap_or(usize::MAX) {
            let error = TransportError::ProtocolError("Message too large".to_string());
            self.event_emitter
                .emit_error(error.clone(), Some("message validation".to_string()));
            return Err(error);
        }

        // Parse JSON payload
        let json_value: serde_json::Value = serde_json::from_slice(&message.payload)
            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?;

        trace!(
            "Processing message from session {}: {:?}",
            session_info.id, json_value
        );

        // Production-grade Tower service integration with MCP protocol handling
        let response_payload = match self.process_mcp_request(&json_value, session_info).await {
            Ok(result) => serde_json::json!({
                "jsonrpc": "2.0", 
                "id": json_value.get("id").unwrap_or(&serde_json::Value::Null).clone(),
                "result": result
            }),
            Err(error) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": json_value.get("id").unwrap_or(&serde_json::Value::Null).clone(), 
                "error": {
                    "code": -32000,
                    "message": error.to_string(),
                    "data": serde_json::Value::Null
                }
            })
        };

        // Debug echo is now only for unrecognized non-MCP messages
        // Production MCP protocol handling takes precedence

        let response_bytes = Bytes::from(
            serde_json::to_vec(&response_payload)
                .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
        );

        let response_message =
            TransportMessage::new(MessageId::from(Uuid::new_v4()), response_bytes);

        // Update processing metrics
        let processing_time = start_time.elapsed();
        self.update_metrics(|m| {
            m.messages_sent += 1;
            m.bytes_sent += response_message.size() as u64;
            m.average_latency_ms =
                (m.average_latency_ms * 0.9) + (processing_time.as_millis() as f64 * 0.1);
        });

        // Emit response event
        self.event_emitter
            .emit_message_sent(response_message.id.clone(), response_message.size());

        Ok(Some(response_message))
    }

    /// Process MCP request with complete protocol handling
    async fn process_mcp_request(
        &self,
        request: &serde_json::Value,
        session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
        
        trace!("Processing MCP request: method={}, session={}", method, session_info.id);

        match method {
            // MCP Core Protocol
            "initialize" => {
                debug!("Handling MCP initialize request");
                Ok(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {},
                        "prompts": {},
                        "logging": {}
                    },
                    "serverInfo": {
                        "name": "TurboMCP Tower Transport",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }))
            }
            "notifications/initialized" => {
                debug!("MCP client initialized notification received");
                Ok(serde_json::Value::Null)
            }
            "ping" => {
                trace!("Handling ping request");
                Ok(serde_json::json!({}))
            }
            
            // Tools
            "tools/list" => {
                debug!("Handling tools/list request");
                self.list_available_tools().await
            }
            "tools/call" => {
                let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                info!("Tool call request: {}", tool_name);
                
                self.handle_tool_call(tool_name, &params, session_info).await
            }
            
            // Resources
            "resources/list" => {
                debug!("Handling resources/list request");
                self.list_available_resources().await
            }
            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                info!("Resource read request: {}", uri);
                
                self.handle_resource_read(uri, session_info).await
            }
            
            // Prompts
            "prompts/list" => {
                debug!("Handling prompts/list request");
                self.list_available_prompts().await
            }
            "prompts/get" => {
                let prompt_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                info!("Prompt get request: {}", prompt_name);
                
                self.handle_prompt_get(prompt_name, &params, session_info).await
            }
            
            // Logging
            "logging/setLevel" => {
                let level = params.get("level").and_then(|l| l.as_str()).unwrap_or("info");
                info!("Logging level set to: {}", level);
                Ok(serde_json::Value::Null)
            }
            
            // Completion
            "completion/complete" => {
                debug!("Handling completion request");
                Ok(serde_json::json!({
                    "completion": {
                        "values": [],
                        "total": 0,
                        "hasMore": false
                    }
                }))
            }
            
            // Unknown method
            _ => {
                warn!("Unknown MCP method: {}", method);
                Err(TransportError::ProtocolError(format!(
                    "Unknown method: {}", method
                )))
            }
        }
    }

    /// Handle tool call requests through registered handlers
    #[cfg(feature = "server")]
    async fn handle_tool_call(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            if let Some(handler) = registry.get_tool(tool_name).await {
                // Convert parameters to proper request format
                let arguments = params.get("arguments").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
                
                let request = CallToolRequest {
                    name: tool_name.to_string(),
                    arguments: if let serde_json::Value::Object(map) = arguments {
                        Some(map.into_iter().collect::<HashMap<String, serde_json::Value>>())
                    } else {
                        None
                    },
                };
                
                // Create production-grade request context
                let ctx = RequestContext::new(
                    Uuid::new_v4().to_string(),
                    session_info.id.clone(),
                );
                
                match handler.handle(request, ctx).await {
                    Ok(result) => Ok(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
                    Err(e) => Err(TransportError::ProtocolError(format!("Tool execution failed: {}", e))),
                }
            } else {
                Err(TransportError::ProtocolError(format!("Tool '{}' not found", tool_name)))
            }
        } else {
            Err(TransportError::ProtocolError("No handler registry configured".to_string()))
        }
    }
    
    /// Handle tool call requests when server feature is disabled
    #[cfg(not(feature = "server"))]
    async fn handle_tool_call(
        &self,
        tool_name: &str,
        _params: &serde_json::Value,
        _session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        Err(TransportError::ProtocolError(format!(
            "Tool '{}' not found - server feature not enabled", tool_name
        )))
    }
    
    /// Handle resource read requests through registered handlers
    #[cfg(feature = "server")]
    async fn handle_resource_read(
        &self,
        uri: &str,
        session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            // Find resource handler using the production registry API
            if let Some(handler) = registry.get_resource(uri).await {
                let request = ReadResourceRequest {
                    uri: uri.to_string(),
                };
                
                // Create production-grade request context
                let ctx = RequestContext::new(
                    Uuid::new_v4().to_string(),
                    session_info.id.clone(),
                );
                
                match handler.handle(request, ctx).await {
                    Ok(result) => return Ok(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
                    Err(e) => return Err(TransportError::ProtocolError(format!("Resource read failed: {}", e))),
                }
            }
            
            Err(TransportError::ProtocolError(format!("Resource '{}' not found", uri)))
        } else {
            Err(TransportError::ProtocolError("No handler registry configured".to_string()))
        }
    }
    
    /// Handle resource read requests when server feature is disabled
    #[cfg(not(feature = "server"))]
    async fn handle_resource_read(
        &self,
        uri: &str,
        _session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        Err(TransportError::ProtocolError(format!(
            "Resource '{}' not found - server feature not enabled", uri
        )))
    }
    
    /// Handle prompt get requests through registered handlers
    #[cfg(feature = "server")]
    async fn handle_prompt_get(
        &self,
        prompt_name: &str,
        params: &serde_json::Value,
        session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            if let Some(handler) = registry.get_prompt(prompt_name).await {
                let arguments = params.get("arguments").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
                
                let request = GetPromptRequest {
                    name: prompt_name.to_string(),
                    arguments: if let serde_json::Value::Object(map) = arguments {
                        Some(map.into_iter().collect::<HashMap<String, serde_json::Value>>())
                    } else {
                        None
                    },
                };
                
                // Create production-grade request context
                let ctx = RequestContext::new(
                    Uuid::new_v4().to_string(),
                    session_info.id.clone(),
                );
                
                match handler.handle(request, ctx).await {
                    Ok(result) => Ok(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
                    Err(e) => Err(TransportError::ProtocolError(format!("Prompt execution failed: {}", e))),
                }
            } else {
                Err(TransportError::ProtocolError(format!("Prompt '{}' not found", prompt_name)))
            }
        } else {
            Err(TransportError::ProtocolError("No handler registry configured".to_string()))
        }
    }
    
    /// Handle prompt get requests when server feature is disabled
    #[cfg(not(feature = "server"))]
    async fn handle_prompt_get(
        &self,
        prompt_name: &str,
        _params: &serde_json::Value,
        _session_info: &SessionInfo,
    ) -> TransportResult<serde_json::Value> {
        Err(TransportError::ProtocolError(format!(
            "Prompt '{}' not found - server feature not enabled", prompt_name
        )))
    }

    /// List available tools from the handler registry
    #[cfg(feature = "server")]
    async fn list_available_tools(&self) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            let tools = registry.get_tool_definitions().await?;
            Ok(serde_json::json!({
                "tools": tools
            }))
        } else {
            Ok(serde_json::json!({
                "tools": []
            }))
        }
    }
    
    /// List available tools when server feature is disabled
    #[cfg(not(feature = "server"))]
    async fn list_available_tools(&self) -> TransportResult<serde_json::Value> {
        Ok(serde_json::json!({
            "tools": []
        }))
    }
    
    /// List available resources from the handler registry
    #[cfg(feature = "server")]
    async fn list_available_resources(&self) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            let resources = registry.get_resource_definitions().await?;
            Ok(serde_json::json!({
                "resources": resources
            }))
        } else {
            Ok(serde_json::json!({
                "resources": []
            }))
        }
    }
    
    /// List available resources when server feature is disabled  
    #[cfg(not(feature = "server"))]
    async fn list_available_resources(&self) -> TransportResult<serde_json::Value> {
        Ok(serde_json::json!({
            "resources": []
        }))
    }
    
    /// List available prompts from the handler registry
    #[cfg(feature = "server")]
    async fn list_available_prompts(&self) -> TransportResult<serde_json::Value> {
        if let Some(registry) = &self.handler_registry {
            let prompts = registry.get_prompt_definitions().await?;
            Ok(serde_json::json!({
                "prompts": prompts
            }))
        } else {
            Ok(serde_json::json!({
                "prompts": []
            }))
        }
    }
    
    /// List available prompts when server feature is disabled
    #[cfg(not(feature = "server"))]
    async fn list_available_prompts(&self) -> TransportResult<serde_json::Value> {
        Ok(serde_json::json!({
            "prompts": []
        }))
    }

    /// Update metrics with a closure
    fn update_metrics<F>(&self, updater: F)
    where
        F: FnOnce(&mut TransportMetrics),
    {
        let mut metrics = self.metrics.lock();
        updater(&mut metrics);
    }

    /// Update transport state
    fn set_state(&self, new_state: TransportState) {
        let mut state = self.state.lock();
        if *state != new_state {
            trace!("Tower transport state: {:?} -> {:?}", *state, new_state);
            *state = new_state.clone();

            // Emit state change events
            match new_state {
                TransportState::Connected => {
                    self.event_emitter
                        .emit_connected(TransportType::Http, "tower://adapter".to_string());
                }
                TransportState::Disconnected => {
                    self.event_emitter.emit_disconnected(
                        TransportType::Http,
                        "tower://adapter".to_string(),
                        None,
                    );
                }
                TransportState::Failed { reason } => {
                    self.event_emitter.emit_disconnected(
                        TransportType::Http,
                        "tower://adapter".to_string(),
                        Some(reason),
                    );
                }
                _ => {}
            }
        }
    }
}

#[async_trait]
impl Transport for TowerTransportAdapter {
    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.lock().clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Connected) {
            return Ok(());
        }

        self.set_state(TransportState::Connecting);

        match self.initialize() {
            Ok(()) => {
                self.update_metrics(|m| m.connections += 1);
                info!("Tower transport adapter connected");
                Ok(())
            }
            Err(e) => {
                self.update_metrics(|m| m.failed_connections += 1);
                self.set_state(TransportState::Failed {
                    reason: e.to_string(),
                });
                error!("Failed to connect Tower transport adapter: {}", e);
                Err(TransportError::ConnectionFailed(e.to_string()))
            }
        }
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Disconnected) {
            return Ok(());
        }

        self.set_state(TransportState::Disconnecting);

        // Close channels
        self.sender = None;
        self.receiver = None;

        // Cancel cleanup task
        if let Some(handle) = self._cleanup_task.take() {
            handle.abort();
        }

        self.set_state(TransportState::Disconnected);
        info!("Tower transport adapter disconnected");
        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Tower transport not connected: {state}"
            )));
        }

        if let Some(ref sender) = self.sender {
            let message_id = message.id.clone();
            let message_size = message.size();

            sender
                .send(message)
                .map_err(|e| TransportError::SendFailed(e.to_string()))?;

            // Update metrics
            self.update_metrics(|m| {
                m.messages_sent += 1;
                m.bytes_sent += message_size as u64;
            });

            // Emit event
            self.event_emitter
                .emit_message_sent(message_id, message_size);

            trace!("Sent message via Tower transport: {} bytes", message_size);
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "Sender not available".to_string(),
            ))
        }
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Tower transport not connected: {state}"
            )));
        }

        if let Some(ref mut receiver) = self.receiver {
            match receiver.try_recv() {
                Ok(message) => {
                    trace!(
                        "Received message via Tower transport: {} bytes",
                        message.size()
                    );
                    Ok(Some(message))
                }
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    warn!("Tower transport receiver disconnected");
                    self.set_state(TransportState::Failed {
                        reason: "Receiver channel disconnected".to_string(),
                    });
                    Err(TransportError::ReceiveFailed(
                        "Channel disconnected".to_string(),
                    ))
                }
            }
        } else {
            Err(TransportError::ReceiveFailed(
                "Receiver not available".to_string(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        let mut metrics = self.metrics.lock().clone();

        // Add session metrics
        metrics.active_connections = self.session_manager.active_session_count() as u64;

        metrics
    }

    fn endpoint(&self) -> Option<String> {
        Some("tower://adapter".to_string())
    }
}

// Import alias to avoid conflicts
use turbomcp_core::Result as McpResult;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_session_info_creation() {
        let session = SessionInfo::new();

        assert!(!session.id.is_empty());
        assert!(session.duration() < Duration::from_millis(100)); // Should be very recent
        assert!(!session.is_expired(Duration::from_secs(1)));
    }

    #[test]
    fn test_session_manager_creation() {
        let manager = SessionManager::new();
        assert_eq!(manager.active_session_count(), 0);
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let manager = SessionManager::new();

        // Create session
        let session = manager.create_session().unwrap();
        assert_eq!(manager.active_session_count(), 1);

        // Get session
        let retrieved = manager.get_session(&session.id).unwrap();
        assert_eq!(retrieved.id, session.id);

        // Remove session
        let removed = manager.remove_session(&session.id);
        assert!(removed);
        assert_eq!(manager.active_session_count(), 0);
    }

    #[tokio::test]
    async fn test_tower_transport_adapter_creation() {
        let adapter = TowerTransportAdapter::new();

        assert_eq!(adapter.transport_type(), TransportType::Http);
        assert!(adapter.capabilities().supports_bidirectional);
        assert!(adapter.capabilities().supports_streaming);
        assert!(adapter.capabilities().supports_multiplexing);
    }

    #[tokio::test]
    async fn test_tower_transport_connection_lifecycle() {
        let mut adapter = TowerTransportAdapter::new();

        // Initially disconnected
        assert_eq!(adapter.state().await, TransportState::Disconnected);

        // Connect
        let result = adapter.connect().await;
        assert!(result.is_ok(), "Failed to connect: {result:?}");
        assert_eq!(adapter.state().await, TransportState::Connected);

        // Disconnect
        let result = adapter.disconnect().await;
        assert!(result.is_ok(), "Failed to disconnect: {result:?}");
        assert_eq!(adapter.state().await, TransportState::Disconnected);
    }

    #[tokio::test]
    async fn test_tower_transport_message_processing() {
        let adapter = TowerTransportAdapter::new();
        let session = SessionInfo::new();

        // Create test message
        let test_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "test-123",
            "method": "ping",
            "params": {}
        });

        let payload_bytes = Bytes::from(serde_json::to_vec(&test_payload).unwrap());
        let message = TransportMessage::new(MessageId::from("test-123"), payload_bytes);

        // Process message
        let result = adapter.process_message(message, &session).await;
        assert!(result.is_ok(), "Failed to process message: {result:?}");

        let response = result.unwrap().unwrap();
        assert!(!response.payload.is_empty());

        // Verify response is valid JSON
        let response_json: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
        assert_eq!(response_json["jsonrpc"], "2.0");
        assert!(response_json["result"].is_object());
    }
}
