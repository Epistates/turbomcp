//! `TurboMCP` Session Management - Ergonomic wrapper over mcp-core state management
//!
//! Provides enhanced session management API while leveraging the comprehensive
//! `mcp-core::state` infrastructure for actual state management.

use crate::McpErrorConstructors;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export core state management functionality
pub use turbomcp_protocol::RequestContext;
pub use turbomcp_protocol::state::StateManager;

use crate::{McpError, McpResult};

/// Enhanced session information with TurboMCP-specific features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID
    pub session_id: String,
    /// Client ID
    pub client_id: String,
    /// Session creation time
    pub created_at: SystemTime,
    /// Last activity time
    pub last_activity: SystemTime,
    /// Session metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Client session information (alias for SSE server compatibility)
pub type ClientSession = SessionInfo;

/// Request to create a new session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    /// Client ID
    pub client_id: String,
    /// Client name (optional)
    pub client_name: Option<String>,
    /// Client version (optional)
    pub client_version: Option<String>,
    /// Transport type
    pub transport_type: String,
    /// Client IP address (optional)
    pub client_ip: Option<String>,
    /// User agent (optional)
    pub user_agent: Option<String>,
    /// Session metadata
    pub metadata: HashMap<String, String>,
    /// Authentication token (optional)
    pub auth_token: Option<String>,
}

/// Session configuration for `TurboMCP` enhancements
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Session timeout duration
    pub timeout: Duration,
    /// Enable request analytics and performance monitoring
    pub enable_analytics: bool,
    /// Maximum sessions per client (enforced via LRU eviction)
    pub max_sessions_per_client: Option<u32>,
    /// Maximum total sessions (prevents memory exhaustion)
    pub max_total_sessions: Option<u32>,
    /// Cleanup interval for expired sessions
    pub cleanup_interval: Duration,
    /// Enable session activity tracking for analytics
    pub track_activity: bool,
    /// Session data size limit per session (in bytes)
    pub max_session_data_size: Option<usize>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(3600), // 1 hour
            enable_analytics: true,
            max_sessions_per_client: Some(10),
            max_total_sessions: Some(1000), // Prevent memory exhaustion
            cleanup_interval: Duration::from_secs(300), // 5 minutes
            track_activity: true,
            max_session_data_size: Some(1024 * 1024), // 1MB per session
        }
    }
}

impl SessionConfig {
    /// Create a fast configuration
    #[must_use]
    pub const fn high_performance() -> Self {
        Self {
            timeout: Duration::from_secs(1800), // 30 minutes
            enable_analytics: true,
            max_sessions_per_client: Some(5),
            max_total_sessions: Some(5000),
            cleanup_interval: Duration::from_secs(60), // 1 minute
            track_activity: true,
            max_session_data_size: Some(512 * 1024), // 512KB per session
        }
    }

    /// Create a memory-optimized configuration for resource-constrained environments
    #[must_use]
    pub const fn memory_optimized() -> Self {
        Self {
            timeout: Duration::from_secs(900), // 15 minutes
            enable_analytics: false,
            max_sessions_per_client: Some(3),
            max_total_sessions: Some(100),
            cleanup_interval: Duration::from_secs(30),
            track_activity: false,
            max_session_data_size: Some(64 * 1024), // 64KB per session
        }
    }

    /// Create a development configuration with relaxed limits
    #[must_use]
    pub const fn development() -> Self {
        Self {
            timeout: Duration::from_secs(7200), // 2 hours
            enable_analytics: true,
            max_sessions_per_client: Some(50),
            max_total_sessions: Some(500),
            cleanup_interval: Duration::from_secs(600), // 10 minutes
            track_activity: true,
            max_session_data_size: Some(5 * 1024 * 1024), // 5MB per session
        }
    }
}

/// `TurboMCP` session manager that wraps `mcp-core::state::StateManager`
#[derive(Debug)]
pub struct SessionManager {
    /// Underlying state manager from mcp-core
    state_manager: Arc<StateManager>,
    /// TurboMCP-specific configuration
    config: SessionConfig,
    /// Session metadata storage with LRU ordering
    session_metadata: Arc<tokio::sync::RwLock<HashMap<String, SessionInfo>>>,
    /// Client session tracking for per-client limits
    client_sessions: Arc<tokio::sync::RwLock<HashMap<String, Vec<String>>>>,
    /// Session access order for LRU eviction
    access_order: Arc<tokio::sync::RwLock<Vec<String>>>,
    /// Cleanup task handle
    cleanup_handle: Arc<tokio::sync::RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Session data size tracking
    session_data_sizes: Arc<tokio::sync::RwLock<HashMap<String, usize>>>,
}

impl SessionManager {
    /// Create new session manager with optimized configuration
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        let manager = Self {
            state_manager: Arc::new(StateManager::new()),
            config: config.clone(),
            session_metadata: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            client_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            access_order: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            cleanup_handle: Arc::new(tokio::sync::RwLock::new(None)),
            session_data_sizes: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        };

        // Start cleanup task if enabled
        if config.cleanup_interval.as_secs() > 0 {
            manager.start_cleanup_task();
        }

        manager
    }

    /// Start background cleanup task for expired sessions
    fn start_cleanup_task(&self) {
        let state_manager = self.state_manager.clone();
        let session_metadata = self.session_metadata.clone();
        let client_sessions = self.client_sessions.clone();
        let access_order = self.access_order.clone();
        let session_data_sizes = self.session_data_sizes.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let session_timeout = self.config.timeout;
        let cleanup_handle = self.cleanup_handle.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;

                // Find expired sessions
                let now = SystemTime::now();
                let mut expired_sessions = Vec::new();

                {
                    let metadata = session_metadata.read().await;
                    for (session_id, session_info) in metadata.iter() {
                        if let Ok(elapsed) = now.duration_since(session_info.last_activity)
                            && elapsed > session_timeout
                        {
                            expired_sessions.push(session_id.clone());
                        }
                    }
                }

                // Remove expired sessions
                for session_id in expired_sessions {
                    Self::cleanup_session(
                        &session_id,
                        &state_manager,
                        &session_metadata,
                        &client_sessions,
                        &access_order,
                        &session_data_sizes,
                    )
                    .await;
                }
            }
        });

        // Store the handle
        if let Ok(mut handle_lock) = cleanup_handle.try_write() {
            *handle_lock = Some(handle);
        };
    }

    /// Clean up a specific session from all tracking structures
    async fn cleanup_session(
        session_id: &str,
        state_manager: &StateManager,
        session_metadata: &tokio::sync::RwLock<HashMap<String, SessionInfo>>,
        client_sessions: &tokio::sync::RwLock<HashMap<String, Vec<String>>>,
        access_order: &tokio::sync::RwLock<Vec<String>>,
        session_data_sizes: &tokio::sync::RwLock<HashMap<String, usize>>,
    ) {
        // Remove from state manager
        let session_key = format!("session:{session_id}");
        let _ = state_manager.remove(&session_key);

        // Remove session data keys
        let data_prefix = format!("session:{session_id}:data:");
        for key in state_manager.list_keys() {
            if key.starts_with(&data_prefix) {
                let _ = state_manager.remove(&key);
            }
        }

        // Remove from local tracking
        let client_id = {
            let mut metadata = session_metadata.write().await;
            metadata.remove(session_id).map(|info| info.client_id)
        };

        if let Some(client_id) = client_id {
            let mut clients = client_sessions.write().await;
            if let Some(sessions) = clients.get_mut(&client_id) {
                sessions.retain(|id| id != session_id);
                if sessions.is_empty() {
                    clients.remove(&client_id);
                }
            }
        }

        access_order.write().await.retain(|id| id != session_id);
        session_data_sizes.write().await.remove(session_id);
    }

    /// Enforce capacity limits by evicting least recently used sessions
    async fn enforce_capacity_limits(&self, new_client_id: &str) -> McpResult<()> {
        // Check total session limit
        if let Some(max_total) = self.config.max_total_sessions {
            let current_total = self.session_metadata.read().await.len();
            if current_total >= max_total as usize {
                // Evict oldest session
                if let Some(oldest_session_id) = self.access_order.read().await.first().cloned() {
                    self.terminate_session(&oldest_session_id).await?;
                }
            }
        }

        // Check per-client session limit
        if let Some(max_per_client) = self.config.max_sessions_per_client {
            let oldest_session_id = {
                let client_sessions = self.client_sessions.read().await;
                if let Some(sessions) = client_sessions.get(new_client_id) {
                    if sessions.len() >= max_per_client as usize {
                        // Get oldest session ID for this client
                        sessions.first().cloned()
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            // Evict oldest session if needed (outside of the lock)
            if let Some(session_id) = oldest_session_id {
                self.terminate_session(&session_id).await?;
            }
        }

        Ok(())
    }

    /// Update session access order for LRU tracking
    async fn update_access_order(&self, session_id: &str) {
        let mut access_order = self.access_order.write().await;
        // Remove if exists, then add to end (most recent)
        access_order.retain(|id| id != session_id);
        access_order.push(session_id.to_string());
    }

    /// Create a new session from a creation request
    pub async fn create_session(&self, request: CreateSessionRequest) -> McpResult<SessionInfo> {
        // Enforce capacity limits before creating new session
        self.enforce_capacity_limits(&request.client_id).await?;

        let session_id = Uuid::new_v4().to_string();
        let now = SystemTime::now();

        // Create TurboMCP session info
        let mut metadata = HashMap::new();
        for (k, v) in request.metadata {
            metadata.insert(k, serde_json::Value::String(v));
        }

        let session_info = SessionInfo {
            session_id: session_id.clone(),
            client_id: request.client_id.clone(),
            created_at: now,
            last_activity: now,
            metadata,
        };

        // Store session data in mcp-core state manager
        let session_key = format!("session:{session_id}");
        self.state_manager.set(
            session_key,
            serde_json::to_value(&session_info)
                .map_err(|e| McpError::tool(format!("Failed to serialize session: {e}")))?,
        );

        // Store in local metadata for quick access
        self.session_metadata
            .write()
            .await
            .insert(session_id.clone(), session_info.clone());

        // Update client session tracking
        self.client_sessions
            .write()
            .await
            .entry(request.client_id.clone())
            .or_insert_with(Vec::new)
            .push(session_id.clone());

        // Update access order
        self.update_access_order(&session_id).await;

        // Initialize session data size tracking
        self.session_data_sizes
            .write()
            .await
            .insert(session_id.clone(), 0);

        Ok(session_info)
    }

    /// Get session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<SessionInfo> {
        // Check if session exists in local metadata
        if let Some(session) = self.session_metadata.read().await.get(session_id) {
            // Update access order for LRU tracking
            self.update_access_order(session_id).await;
            return Some(session.clone());
        }

        // Try to load from mcp-core state manager
        let session_key = format!("session:{session_id}");
        if let Some(session_data) = self.state_manager.get(&session_key)
            && let Ok(session_info) = serde_json::from_value::<SessionInfo>(session_data)
        {
            // Cache in local metadata
            self.session_metadata
                .write()
                .await
                .insert(session_id.to_string(), session_info.clone());

            // Update access order
            self.update_access_order(session_id).await;

            return Some(session_info);
        }

        None
    }

    /// Update session activity
    pub async fn update_activity(&self, session_id: &str) -> McpResult<()> {
        let now = SystemTime::now();

        // Update TurboMCP metadata
        if let Some(session) = self.session_metadata.write().await.get_mut(session_id) {
            session.last_activity = now;

            // Update in mcp-core state manager
            let session_key = format!("session:{session_id}");
            self.state_manager.set(
                session_key,
                serde_json::to_value(session)
                    .map_err(|e| McpError::tool(format!("Failed to serialize session: {e}")))?,
            );

            // Update access order for LRU tracking
            self.update_access_order(session_id).await;
        }

        Ok(())
    }

    /// Set session data (uses mcp-core `StateManager`) with size tracking
    pub async fn set_session_data(
        &self,
        session_id: &str,
        key: String,
        value: serde_json::Value,
    ) -> McpResult<()> {
        // Check session data size limits
        if let Some(max_size) = self.config.max_session_data_size {
            let value_size = serde_json::to_string(&value)
                .map_err(|e| McpError::tool(format!("Failed to serialize value: {e}")))?
                .len();

            let current_size = self
                .session_data_sizes
                .read()
                .await
                .get(session_id)
                .copied()
                .unwrap_or(0);

            if current_size + value_size > max_size {
                return Err(McpError::tool(format!(
                    "Session data size limit exceeded: {current_size} + {value_size} > {max_size}"
                )));
            }

            // Update size tracking
            self.session_data_sizes
                .write()
                .await
                .entry(session_id.to_string())
                .and_modify(|size| *size += value_size)
                .or_insert(value_size);
        }

        let data_key = format!("session:{session_id}:data:{key}");
        self.state_manager.set(data_key, value);

        // Update access tracking
        self.update_access_order(session_id).await;

        Ok(())
    }

    /// Get session data (uses mcp-core `StateManager`)  
    pub async fn get_session_data(&self, session_id: &str, key: &str) -> Option<serde_json::Value> {
        let data_key = format!("session:{session_id}:data:{key}");
        let result = self.state_manager.get(&data_key);

        // Update access tracking when data is accessed
        if result.is_some() {
            self.update_access_order(session_id).await;
        }

        result
    }

    /// Terminate session (removes from mcp-core `StateManager`)
    pub async fn terminate_session(&self, session_id: &str) -> McpResult<()> {
        Self::cleanup_session(
            session_id,
            &self.state_manager,
            &self.session_metadata,
            &self.client_sessions,
            &self.access_order,
            &self.session_data_sizes,
        )
        .await;

        Ok(())
    }

    /// Get all active sessions
    pub async fn get_active_sessions(&self) -> Vec<SessionInfo> {
        let metadata = self.session_metadata.read().await;
        metadata.values().cloned().collect()
    }

    /// Get comprehensive session statistics
    pub async fn get_statistics(&self) -> SessionStatistics {
        let sessions = self.get_active_sessions().await;
        let total_sessions = sessions.len();
        let now = SystemTime::now();

        // Calculate sessions per client
        let client_sessions = self.client_sessions.read().await;
        let sessions_per_client: HashMap<String, usize> = client_sessions
            .iter()
            .map(|(client_id, session_list)| (client_id.clone(), session_list.len()))
            .collect();

        // Calculate session ages
        let mut session_ages: Vec<f64> = Vec::new();
        for session in &sessions {
            if let Ok(age) = now.duration_since(session.created_at) {
                session_ages.push(age.as_secs_f64());
            }
        }

        let average_session_age = if session_ages.is_empty() {
            0.0
        } else {
            session_ages.iter().sum::<f64>() / session_ages.len() as f64
        };

        let oldest_session_age = session_ages
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        // Calculate total session data size
        let session_data_sizes = self.session_data_sizes.read().await;
        let total_session_data_size: usize = session_data_sizes.values().sum();

        // Estimate memory usage (rough calculation)
        let estimated_memory_usage = total_sessions * 1024 + total_session_data_size; // Base overhead + data

        SessionStatistics {
            total_sessions,
            active_sessions: total_sessions, // All sessions we track are active
            sessions_per_client,
            estimated_memory_usage,
            average_session_age,
            oldest_session_age,
            total_session_data_size,
            limits: SessionLimits {
                max_sessions_per_client: self.config.max_sessions_per_client,
                max_total_sessions: self.config.max_total_sessions,
                session_timeout_secs: self.config.timeout.as_secs(),
                max_session_data_size: self.config.max_session_data_size,
            },
        }
    }

    /// Get underlying mcp-core state manager
    #[must_use]
    pub fn state_manager(&self) -> Arc<StateManager> {
        self.state_manager.clone()
    }

    /// Get session health status
    pub async fn get_session_health(&self) -> SessionHealth {
        let stats = self.get_statistics().await;
        let mut health = SessionHealth {
            status: SessionHealthStatus::Healthy,
            warnings: Vec::new(),
            metrics: stats.clone(),
        };

        // Check for potential issues
        if let Some(max_total) = stats.limits.max_total_sessions {
            let usage_percent = (stats.total_sessions as f64 / f64::from(max_total)) * 100.0;
            if usage_percent > 90.0 {
                health.status = SessionHealthStatus::Critical;
                health.warnings.push(format!(
                    "Total session usage at {:.1}% ({}/{})",
                    usage_percent, stats.total_sessions, max_total
                ));
            } else if usage_percent > 75.0 {
                health.status = SessionHealthStatus::Warning;
                health.warnings.push(format!(
                    "Total session usage at {:.1}% ({}/{})",
                    usage_percent, stats.total_sessions, max_total
                ));
            }
        }

        // Check memory usage (rough heuristic)
        if stats.estimated_memory_usage > 100 * 1024 * 1024 {
            // 100MB
            health.status = SessionHealthStatus::Warning;
            health.warnings.push(format!(
                "High estimated memory usage: {} MB",
                stats.estimated_memory_usage / (1024 * 1024)
            ));
        }

        // Check for very old sessions
        if stats.oldest_session_age > 86400.0 {
            // 24 hours
            health.warnings.push(format!(
                "Very old session detected: {:.1} hours old",
                stats.oldest_session_age / 3600.0
            ));
        }

        health
    }

    /// Force cleanup of expired sessions (manual trigger)
    pub async fn force_cleanup(&self) -> usize {
        let now = SystemTime::now();
        let mut expired_sessions = Vec::new();

        {
            let metadata = self.session_metadata.read().await;
            for (session_id, session_info) in metadata.iter() {
                if let Ok(elapsed) = now.duration_since(session_info.last_activity)
                    && elapsed > self.config.timeout
                {
                    expired_sessions.push(session_id.clone());
                }
            }
        }

        let cleanup_count = expired_sessions.len();

        for session_id in expired_sessions {
            Self::cleanup_session(
                &session_id,
                &self.state_manager,
                &self.session_metadata,
                &self.client_sessions,
                &self.access_order,
                &self.session_data_sizes,
            )
            .await;
        }

        cleanup_count
    }

    /// Get sessions for a specific client
    pub async fn get_client_sessions(&self, client_id: &str) -> Vec<SessionInfo> {
        let client_sessions = self.client_sessions.read().await;
        if let Some(session_ids) = client_sessions.get(client_id) {
            let metadata = self.session_metadata.read().await;
            session_ids
                .iter()
                .filter_map(|id| metadata.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get configuration being used
    #[must_use]
    pub const fn get_config(&self) -> &SessionConfig {
        &self.config
    }
}

/// Enhanced session statistics with performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatistics {
    /// Total number of sessions
    pub total_sessions: usize,
    /// Number of active sessions
    pub active_sessions: usize,
    /// Sessions per client breakdown
    pub sessions_per_client: HashMap<String, usize>,
    /// Total memory usage estimate (bytes)
    pub estimated_memory_usage: usize,
    /// Average session age (seconds)
    pub average_session_age: f64,
    /// Oldest session age (seconds)
    pub oldest_session_age: f64,
    /// Total session data size (bytes)
    pub total_session_data_size: usize,
    /// Configuration limits
    pub limits: SessionLimits,
}

/// Session configuration limits for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLimits {
    /// Maximum sessions per client
    pub max_sessions_per_client: Option<u32>,
    /// Maximum total sessions
    pub max_total_sessions: Option<u32>,
    /// Session timeout (seconds)
    pub session_timeout_secs: u64,
    /// Maximum session data size (bytes)
    pub max_session_data_size: Option<usize>,
}

/// Session health monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHealth {
    /// Overall health status
    pub status: SessionHealthStatus,
    /// Health warnings and alerts
    pub warnings: Vec<String>,
    /// Current session metrics
    pub metrics: SessionStatistics,
}

/// Session health status levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionHealthStatus {
    /// All systems operating normally
    Healthy,
    /// Some concerns but not critical
    Warning,
    /// Critical issues requiring attention
    Critical,
}

/// Convenience functions for session management
impl SessionManager {
    /// Create session with client ID only (backward compatibility)
    pub async fn create_session_simple(&self, client_id: String) -> McpResult<SessionInfo> {
        let request = CreateSessionRequest {
            client_id,
            client_name: None,
            client_version: None,
            transport_type: "unknown".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: HashMap::new(),
            auth_token: None,
        };
        self.create_session(request).await
    }

    /// Create session with metadata
    pub async fn create_session_with_metadata(
        &self,
        client_id: String,
        metadata: HashMap<String, serde_json::Value>,
    ) -> McpResult<SessionInfo> {
        let request = CreateSessionRequest {
            client_id,
            client_name: None,
            client_version: None,
            transport_type: "unknown".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: metadata
                .into_iter()
                .map(|(k, v)| {
                    let string_value = match v {
                        serde_json::Value::String(s) => s,
                        _ => v.to_string(),
                    };
                    (k, string_value)
                })
                .collect(),
            auth_token: None,
        };
        self.create_session(request).await
    }

    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> McpResult<()> {
        if let Some(session) = self.session_metadata.write().await.get_mut(session_id) {
            session.metadata = metadata;
            Ok(())
        } else {
            Err(McpError::tool("Session not found".to_string()))
        }
    }

    /// Check if session exists
    pub async fn session_exists(&self, session_id: &str) -> bool {
        let session_key = format!("session:{session_id}");
        self.state_manager.get(&session_key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let request = CreateSessionRequest {
            client_id: "test-client".to_string(),
            client_name: None,
            client_version: None,
            transport_type: "test".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: HashMap::new(),
            auth_token: None,
        };
        let session = manager.create_session(request).await.unwrap();
        assert_eq!(session.client_id, "test-client");
        assert!(!session.session_id.is_empty());
    }

    #[tokio::test]
    async fn test_session_data_storage() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let request = CreateSessionRequest {
            client_id: "test-client".to_string(),
            client_name: None,
            client_version: None,
            transport_type: "test".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: HashMap::new(),
            auth_token: None,
        };
        let session = manager.create_session(request).await.unwrap();

        // Set data
        manager
            .set_session_data(
                &session.session_id,
                "test_key".to_string(),
                serde_json::json!("test_value"),
            )
            .await
            .unwrap();

        // Get data
        let value = manager
            .get_session_data(&session.session_id, "test_key")
            .await
            .unwrap();

        assert_eq!(value, serde_json::json!("test_value"));
    }

    #[tokio::test]
    async fn test_session_with_metadata() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        let mut metadata = HashMap::new();
        metadata.insert("app_version".to_string(), serde_json::json!("1.0.0"));

        let session = manager
            .create_session_with_metadata("test-client".to_string(), metadata)
            .await
            .unwrap();

        assert_eq!(
            session.metadata["app_version"],
            serde_json::Value::String("1.0.0".to_string())
        );
    }

    #[tokio::test]
    async fn test_session_statistics() {
        let config = SessionConfig::default();
        let manager = SessionManager::new(config);

        // Create multiple sessions
        let request1 = CreateSessionRequest {
            client_id: "client1".to_string(),
            client_name: None,
            client_version: None,
            transport_type: "test".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: HashMap::new(),
            auth_token: None,
        };
        manager.create_session(request1).await.unwrap();

        let request2 = CreateSessionRequest {
            client_id: "client2".to_string(),
            client_name: None,
            client_version: None,
            transport_type: "test".to_string(),
            client_ip: None,
            user_agent: None,
            metadata: HashMap::new(),
            auth_token: None,
        };
        manager.create_session(request2).await.unwrap();

        let stats = manager.get_statistics().await;
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.active_sessions, 2);
    }
}
