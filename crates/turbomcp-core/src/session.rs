//! Session management for `TurboMCP` applications
//!
//! Provides comprehensive session tracking, client management, and request analytics
//! for MCP servers that need to manage multiple clients and track usage patterns.
//!
//! ## Features
//!
//! - **LRU Eviction**: Automatically evicts least-recently-used sessions when capacity is reached
//! - **Request Analytics**: Track request patterns, success rates, and client behavior
//! - **Sensitive Data Protection**: Automatic sanitization of passwords, tokens, and secrets
//! - **Elicitation Management**: Track pending elicitations and their states
//! - **Completion Tracking**: Monitor active completions across sessions
//!
//! ## Example
//!
//! ```rust
//! use turbomcp_core::{SessionManager, SessionConfig};
//! use chrono::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure session management with custom settings
//! let config = SessionConfig {
//!     max_sessions: 1000,                    // Maximum concurrent sessions
//!     session_timeout: Duration::hours(24), // Session lifetime
//!     max_request_history: 10000,           // Request analytics depth
//!     max_requests_per_session: Some(10000), // Rate limiting per session
//!     cleanup_interval: std::time::Duration::from_secs(300),
//!     enable_analytics: true,
//! };
//!
//! let manager = SessionManager::new(config);
//! manager.start(); // Begin background cleanup task
//!
//! // Create and authenticate sessions
//! let session = manager.get_or_create_session(
//!     "client-123".to_string(),
//!     "websocket".to_string()
//! );
//!
//! manager.authenticate_client(
//!     "client-123",
//!     Some("MyApp/1.0".to_string()),
//!     Some("auth-token".to_string())
//! );
//!
//! // Track request analytics
//! use turbomcp_core::RequestInfo;
//! let request_info = RequestInfo::new(
//!     "client-123".to_string(),
//!     "tools/list".to_string(),
//!     serde_json::json!({}),
//! ).complete_success(150); // 150ms execution time
//!
//! manager.record_request(request_info);
//!
//! // Get analytics
//! let analytics = manager.get_analytics();
//! println!("Active sessions: {}", analytics.active_sessions);
//! println!("Success rate: {:.1}%",
//!     (analytics.successful_requests as f64 /
//!      analytics.total_requests as f64) * 100.0
//! );
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::time::{Interval, interval};

use crate::context::{
    ClientIdExtractor, ClientSession, CompletionContext, ElicitationContext, RequestInfo,
};

/// Configuration for session management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum number of sessions to track
    pub max_sessions: usize,
    /// Session timeout (inactive sessions will be removed)
    pub session_timeout: Duration,
    /// Maximum request history to keep per session
    pub max_request_history: usize,
    /// Optional hard cap on requests per individual session
    pub max_requests_per_session: Option<usize>,
    /// Cleanup interval for expired sessions
    pub cleanup_interval: StdDuration,
    /// Whether to track request analytics
    pub enable_analytics: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1000,
            session_timeout: Duration::hours(24),
            max_request_history: 1000,
            max_requests_per_session: None,
            cleanup_interval: StdDuration::from_secs(300), // 5 minutes
            enable_analytics: true,
        }
    }
}

/// Session analytics and usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalytics {
    /// Total number of sessions created
    pub total_sessions: usize,
    /// Currently active sessions
    pub active_sessions: usize,
    /// Total requests processed
    pub total_requests: usize,
    /// Total successful requests
    pub successful_requests: usize,
    /// Total failed requests
    pub failed_requests: usize,
    /// Average session duration
    pub avg_session_duration: Duration,
    /// Most active clients (top 10)
    pub top_clients: Vec<(String, usize)>,
    /// Most used tools/methods (top 10)
    pub top_methods: Vec<(String, usize)>,
    /// Request rate (requests per minute)
    pub requests_per_minute: f64,
}

/// Comprehensive session manager for MCP applications
#[derive(Debug)]
pub struct SessionManager {
    /// Configuration
    config: SessionConfig,
    /// Active client sessions
    sessions: Arc<DashMap<String, ClientSession>>,
    /// Client ID extractor for authentication
    client_extractor: Arc<ClientIdExtractor>,
    /// Request history for analytics
    request_history: Arc<RwLock<VecDeque<RequestInfo>>>,
    /// Session creation history for analytics
    session_history: Arc<RwLock<VecDeque<SessionEvent>>>,
    /// Cleanup timer
    cleanup_timer: Arc<RwLock<Option<Interval>>>,
    /// Global statistics
    stats: Arc<RwLock<SessionStats>>,
    /// Pending elicitations by client ID
    pending_elicitations: Arc<DashMap<String, Vec<ElicitationContext>>>,
    /// Active completions by client ID
    active_completions: Arc<DashMap<String, Vec<CompletionContext>>>,
}

/// Internal statistics tracking
#[derive(Debug, Default)]
struct SessionStats {
    total_sessions: usize,
    total_requests: usize,
    successful_requests: usize,
    failed_requests: usize,
    total_session_duration: Duration,
}

/// Session lifecycle events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Client ID
    pub client_id: String,
    /// Event type
    pub event_type: SessionEventType,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Types of session events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEventType {
    /// Session created
    Created,
    /// Session authenticated
    Authenticated,
    /// Session updated (activity)
    Updated,
    /// Session expired
    Expired,
    /// Session terminated
    Terminated,
}

impl SessionManager {
    /// Create a new session manager
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(DashMap::new()),
            client_extractor: Arc::new(ClientIdExtractor::new()),
            request_history: Arc::new(RwLock::new(VecDeque::new())),
            session_history: Arc::new(RwLock::new(VecDeque::new())),
            cleanup_timer: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(SessionStats::default())),
            pending_elicitations: Arc::new(DashMap::new()),
            active_completions: Arc::new(DashMap::new()),
        }
    }

    /// Start the session manager (begin cleanup task)
    pub fn start(&self) {
        let mut timer_guard = self.cleanup_timer.write();
        if timer_guard.is_none() {
            *timer_guard = Some(interval(self.config.cleanup_interval));
        }
        drop(timer_guard);

        // Start cleanup task
        let sessions = self.sessions.clone();
        let config = self.config.clone();
        let session_history = self.session_history.clone();
        let stats = self.stats.clone();
        let pending_elicitations = self.pending_elicitations.clone();
        let active_completions = self.active_completions.clone();

        tokio::spawn(async move {
            let mut timer = interval(config.cleanup_interval);
            loop {
                timer.tick().await;
                Self::cleanup_expired_sessions(
                    &sessions,
                    &config,
                    &session_history,
                    &stats,
                    &pending_elicitations,
                    &active_completions,
                );
            }
        });
    }

    /// Create or get existing session for a client
    #[must_use]
    pub fn get_or_create_session(
        &self,
        client_id: String,
        transport_type: String,
    ) -> ClientSession {
        self.sessions.get(&client_id).map_or_else(
            || {
                // Enforce capacity before inserting a new session
                self.enforce_capacity();

                let session = ClientSession::new(client_id.clone(), transport_type);
                self.sessions.insert(client_id.clone(), session.clone());

                // Record session creation
                let mut stats = self.stats.write();
                stats.total_sessions += 1;
                drop(stats);

                self.record_session_event(client_id, SessionEventType::Created, HashMap::new());

                session
            },
            |session| session.clone(),
        )
    }

    /// Update client activity
    pub fn update_client_activity(&self, client_id: &str) {
        if let Some(mut session) = self.sessions.get_mut(client_id) {
            session.update_activity();

            // Optional: enforce per-session request cap by early termination
            if let Some(cap) = self.config.max_requests_per_session
                && session.request_count > cap
            {
                // Terminate the session when the cap is exceeded
                // This is a conservative protection to prevent abusive sessions
                drop(session);
                let _ = self.terminate_session(client_id);
            }
        }
    }

    /// Authenticate a client session
    #[must_use]
    pub fn authenticate_client(
        &self,
        client_id: &str,
        client_name: Option<String>,
        token: Option<String>,
    ) -> bool {
        if let Some(mut session) = self.sessions.get_mut(client_id) {
            session.authenticate(client_name.clone());

            if let Some(token) = token {
                self.client_extractor
                    .register_token(token, client_id.to_string());
            }

            let mut metadata = HashMap::new();
            if let Some(name) = client_name {
                metadata.insert("client_name".to_string(), serde_json::json!(name));
            }

            self.record_session_event(
                client_id.to_string(),
                SessionEventType::Authenticated,
                metadata,
            );

            return true;
        }
        false
    }

    /// Record a request for analytics
    pub fn record_request(&self, mut request_info: RequestInfo) {
        if !self.config.enable_analytics {
            return;
        }

        // Update session activity
        self.update_client_activity(&request_info.client_id);

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_requests += 1;
        if request_info.success {
            stats.successful_requests += 1;
        } else {
            stats.failed_requests += 1;
        }
        drop(stats);

        // Add to request history
        let mut history = self.request_history.write();
        if history.len() >= self.config.max_request_history {
            history.pop_front();
        }

        // Sanitize sensitive data before storing
        request_info.parameters = self.sanitize_parameters(request_info.parameters);
        history.push_back(request_info);
    }

    /// Get session analytics
    #[must_use]
    pub fn get_analytics(&self) -> SessionAnalytics {
        let sessions = self.sessions.clone();

        // Calculate active sessions
        let active_sessions = sessions.len();

        // Calculate average session duration
        let total_duration = sessions
            .iter()
            .map(|entry| entry.session_duration())
            .reduce(|acc, dur| acc + dur)
            .unwrap_or_else(Duration::zero);

        let avg_session_duration = if active_sessions > 0 {
            total_duration / active_sessions as i32
        } else {
            Duration::zero()
        };

        // Calculate top clients by request count
        let mut client_requests: HashMap<String, usize> = HashMap::new();
        let mut method_requests: HashMap<String, usize> = HashMap::new();

        let (recent_requests, top_clients, top_methods) = {
            let history = self.request_history.read();
            for request in history.iter() {
                *client_requests
                    .entry(request.client_id.clone())
                    .or_insert(0) += 1;
                *method_requests
                    .entry(request.method_name.clone())
                    .or_insert(0) += 1;
            }

            let mut top_clients: Vec<(String, usize)> = client_requests.into_iter().collect();
            top_clients.sort_by(|a, b| b.1.cmp(&a.1));
            top_clients.truncate(10);

            let mut top_methods: Vec<(String, usize)> = method_requests.into_iter().collect();
            top_methods.sort_by(|a, b| b.1.cmp(&a.1));
            top_methods.truncate(10);

            // Calculate request rate (requests per minute over last hour)
            let one_hour_ago = Utc::now() - Duration::hours(1);
            let recent_requests = history
                .iter()
                .filter(|req| req.timestamp > one_hour_ago)
                .count();
            drop(history);

            (recent_requests, top_clients, top_methods)
        };
        let requests_per_minute = recent_requests as f64 / 60.0;

        let stats = self.stats.read();
        SessionAnalytics {
            total_sessions: stats.total_sessions,
            active_sessions,
            total_requests: stats.total_requests,
            successful_requests: stats.successful_requests,
            failed_requests: stats.failed_requests,
            avg_session_duration,
            top_clients,
            top_methods,
            requests_per_minute,
        }
    }

    /// Get all active sessions
    #[must_use]
    pub fn get_active_sessions(&self) -> Vec<ClientSession> {
        self.sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get session by client ID
    #[must_use]
    pub fn get_session(&self, client_id: &str) -> Option<ClientSession> {
        self.sessions.get(client_id).map(|session| session.clone())
    }

    /// Get client ID extractor
    #[must_use]
    pub fn client_extractor(&self) -> Arc<ClientIdExtractor> {
        self.client_extractor.clone()
    }

    /// Terminate a session
    #[must_use]
    pub fn terminate_session(&self, client_id: &str) -> bool {
        if let Some((_, session)) = self.sessions.remove(client_id) {
            let mut stats = self.stats.write();
            stats.total_session_duration += session.session_duration();
            drop(stats);

            // Clean up associated elicitations and completions
            self.pending_elicitations.remove(client_id);
            self.active_completions.remove(client_id);

            self.record_session_event(
                client_id.to_string(),
                SessionEventType::Terminated,
                HashMap::new(),
            );

            true
        } else {
            false
        }
    }

    /// Get request history
    #[must_use]
    pub fn get_request_history(&self, limit: Option<usize>) -> Vec<RequestInfo> {
        let history = self.request_history.read();
        let limit = limit.unwrap_or(100);

        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get session events
    #[must_use]
    pub fn get_session_events(&self, limit: Option<usize>) -> Vec<SessionEvent> {
        let events = self.session_history.read();
        let limit = limit.unwrap_or(100);

        events.iter().rev().take(limit).cloned().collect()
    }

    // Elicitation Management

    /// Add a pending elicitation for a client
    pub fn add_pending_elicitation(&self, client_id: String, elicitation: ElicitationContext) {
        self.pending_elicitations
            .entry(client_id)
            .or_default()
            .push(elicitation);
    }

    /// Get all pending elicitations for a client
    #[must_use]
    pub fn get_pending_elicitations(&self, client_id: &str) -> Vec<ElicitationContext> {
        self.pending_elicitations
            .get(client_id)
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    /// Update an elicitation state
    pub fn update_elicitation_state(
        &self,
        client_id: &str,
        elicitation_id: &str,
        state: crate::context::ElicitationState,
    ) -> bool {
        if let Some(mut elicitations) = self.pending_elicitations.get_mut(client_id) {
            for elicitation in elicitations.iter_mut() {
                if elicitation.elicitation_id == elicitation_id {
                    elicitation.set_state(state);
                    return true;
                }
            }
        }
        false
    }

    /// Remove completed elicitations for a client
    pub fn remove_completed_elicitations(&self, client_id: &str) {
        if let Some(mut elicitations) = self.pending_elicitations.get_mut(client_id) {
            elicitations.retain(|e| !e.is_complete());
        }
    }

    /// Clear all elicitations for a client
    pub fn clear_elicitations(&self, client_id: &str) {
        self.pending_elicitations.remove(client_id);
    }

    // Completion Management

    /// Add an active completion for a client
    pub fn add_active_completion(&self, client_id: String, completion: CompletionContext) {
        self.active_completions
            .entry(client_id)
            .or_default()
            .push(completion);
    }

    /// Get all active completions for a client
    #[must_use]
    pub fn get_active_completions(&self, client_id: &str) -> Vec<CompletionContext> {
        self.active_completions
            .get(client_id)
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    /// Remove a specific completion
    pub fn remove_completion(&self, client_id: &str, completion_id: &str) -> bool {
        if let Some(mut completions) = self.active_completions.get_mut(client_id) {
            let original_len = completions.len();
            completions.retain(|c| c.completion_id != completion_id);
            return completions.len() < original_len;
        }
        false
    }

    /// Clear all completions for a client
    pub fn clear_completions(&self, client_id: &str) {
        self.active_completions.remove(client_id);
    }

    /// Get session statistics including elicitations and completions
    #[must_use]
    pub fn get_enhanced_analytics(&self) -> SessionAnalytics {
        let analytics = self.get_analytics();

        // Add elicitation and completion counts
        let mut _total_elicitations = 0;
        let mut _pending_elicitations = 0;
        let mut _total_completions = 0;

        for entry in self.pending_elicitations.iter() {
            let elicitations = entry.value();
            _total_elicitations += elicitations.len();
            _pending_elicitations += elicitations.iter().filter(|e| !e.is_complete()).count();
        }

        for entry in self.active_completions.iter() {
            _total_completions += entry.value().len();
        }

        // Additional analytics can be added to SessionAnalytics struct when extended
        // Currently tracking: _total_elicitations, _pending_elicitations, _total_completions

        analytics
    }

    // Private helper methods

    fn cleanup_expired_sessions(
        sessions: &Arc<DashMap<String, ClientSession>>,
        config: &SessionConfig,
        session_history: &Arc<RwLock<VecDeque<SessionEvent>>>,
        stats: &Arc<RwLock<SessionStats>>,
        pending_elicitations: &Arc<DashMap<String, Vec<ElicitationContext>>>,
        active_completions: &Arc<DashMap<String, Vec<CompletionContext>>>,
    ) {
        let cutoff_time = Utc::now() - config.session_timeout;
        let mut expired_sessions = Vec::new();

        for entry in sessions.iter() {
            if entry.last_activity < cutoff_time {
                expired_sessions.push(entry.client_id.clone());
            }
        }

        for client_id in expired_sessions {
            if let Some((_, session)) = sessions.remove(&client_id) {
                // Update stats
                let mut stats_guard = stats.write();
                stats_guard.total_session_duration += session.session_duration();
                drop(stats_guard);

                // Clean up associated elicitations and completions
                pending_elicitations.remove(&client_id);
                active_completions.remove(&client_id);

                // Record event
                let event = SessionEvent {
                    timestamp: Utc::now(),
                    client_id,
                    event_type: SessionEventType::Expired,
                    metadata: HashMap::new(),
                };

                let mut history = session_history.write();
                if history.len() >= 1000 {
                    history.pop_front();
                }
                history.push_back(event);
            }
        }
    }

    fn record_session_event(
        &self,
        client_id: String,
        event_type: SessionEventType,
        metadata: HashMap<String, serde_json::Value>,
    ) {
        let event = SessionEvent {
            timestamp: Utc::now(),
            client_id,
            event_type,
            metadata,
        };

        let mut history = self.session_history.write();
        if history.len() >= 1000 {
            history.pop_front();
        }
        history.push_back(event);
    }

    /// Ensure the number of active sessions does not exceed `max_sessions`.
    /// This uses an LRU-like policy (evict least recently active sessions first).
    fn enforce_capacity(&self) {
        let target = self.config.max_sessions;
        // Fast path
        if self.sessions.len() < target {
            return;
        }

        // Collect sessions sorted by last_activity ascending (least recent first)
        let mut entries: Vec<_> = self
            .sessions
            .iter()
            .map(|entry| (entry.key().clone(), entry.last_activity))
            .collect();
        entries.sort_by_key(|(_, ts)| *ts);

        // Evict until under capacity
        let mut to_evict = self.sessions.len().saturating_sub(target) + 1; // make room for 1 new
        for (client_id, _) in entries {
            if to_evict == 0 {
                break;
            }
            if let Some((_, session)) = self.sessions.remove(&client_id) {
                let mut stats = self.stats.write();
                stats.total_session_duration += session.session_duration();
                drop(stats);

                // Record eviction as termination event
                let event = SessionEvent {
                    timestamp: Utc::now(),
                    client_id: client_id.clone(),
                    event_type: SessionEventType::Terminated,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("reason".to_string(), serde_json::json!("capacity_eviction"));
                        m
                    },
                };
                {
                    let mut history = self.session_history.write();
                    if history.len() >= 1000 {
                        history.pop_front();
                    }
                    history.push_back(event);
                } // Drop history lock early
                to_evict = to_evict.saturating_sub(1);
            }
        }
    }

    fn sanitize_parameters(&self, mut params: serde_json::Value) -> serde_json::Value {
        let _ = self; // Currently unused, may use config in future
        // Remove or mask sensitive fields
        if let Some(obj) = params.as_object_mut() {
            let sensitive_keys = &["password", "token", "api_key", "secret", "auth"];
            for key in sensitive_keys {
                if obj.contains_key(*key) {
                    obj.insert(
                        (*key).to_string(),
                        serde_json::Value::String("[REDACTED]".to_string()),
                    );
                }
            }
        }
        params
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(SessionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let manager = SessionManager::new(SessionConfig::default());

        let session = manager.get_or_create_session("client-1".to_string(), "http".to_string());

        assert_eq!(session.client_id, "client-1");
        assert_eq!(session.transport_type, "http");
        assert!(!session.authenticated);

        let analytics = manager.get_analytics();
        assert_eq!(analytics.total_sessions, 1);
        assert_eq!(analytics.active_sessions, 1);
    }

    #[tokio::test]
    async fn test_session_authentication() {
        let manager = SessionManager::new(SessionConfig::default());

        let session = manager.get_or_create_session("client-1".to_string(), "http".to_string());
        assert!(!session.authenticated);

        let success = manager.authenticate_client(
            "client-1",
            Some("Test Client".to_string()),
            Some("token123".to_string()),
        );

        assert!(success);

        let updated_session = manager.get_session("client-1").unwrap();
        assert!(updated_session.authenticated);
        assert_eq!(updated_session.client_name, Some("Test Client".to_string()));
    }

    #[tokio::test]
    async fn test_request_recording() {
        let mut manager = SessionManager::new(SessionConfig::default());
        manager.config.enable_analytics = true;

        let request = RequestInfo::new(
            "client-1".to_string(),
            "test_method".to_string(),
            serde_json::json!({"param": "value"}),
        )
        .complete_success(100);

        manager.record_request(request);

        let analytics = manager.get_analytics();
        assert_eq!(analytics.total_requests, 1);
        assert_eq!(analytics.successful_requests, 1);
        assert_eq!(analytics.failed_requests, 0);

        let history = manager.get_request_history(Some(10));
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].method_name, "test_method");
    }

    #[tokio::test]
    async fn test_session_termination() {
        let manager = SessionManager::new(SessionConfig::default());

        let _ = manager.get_or_create_session("client-1".to_string(), "http".to_string());
        assert!(manager.get_session("client-1").is_some());

        let terminated = manager.terminate_session("client-1");
        assert!(terminated);
        assert!(manager.get_session("client-1").is_none());

        let analytics = manager.get_analytics();
        assert_eq!(analytics.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_parameter_sanitization() {
        let manager = SessionManager::new(SessionConfig::default());

        let sensitive_params = serde_json::json!({
            "username": "testuser",
            "password": "secret123",
            "api_key": "key456",
            "data": "normal_data"
        });

        let sanitized = manager.sanitize_parameters(sensitive_params);
        let obj = sanitized.as_object().unwrap();

        assert_eq!(
            obj["username"],
            serde_json::Value::String("testuser".to_string())
        );
        assert_eq!(
            obj["password"],
            serde_json::Value::String("[REDACTED]".to_string())
        );
        assert_eq!(
            obj["api_key"],
            serde_json::Value::String("[REDACTED]".to_string())
        );
        assert_eq!(
            obj["data"],
            serde_json::Value::String("normal_data".to_string())
        );
    }
}
