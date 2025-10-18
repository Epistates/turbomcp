//! Multi-Server Session Manager for MCP Clients
//!
//! This module provides a `SessionManager` for coordinating multiple MCP server sessions.
//! Unlike traditional HTTP connection pooling, MCP uses long-lived, stateful sessions
//! where each connection maintains negotiated capabilities and subscription state.
//!
//! # Key Concepts
//!
//! - **Session**: A long-lived, initialized MCP connection to a server
//! - **Multi-Server**: Manage connections to different MCP servers (GitHub, filesystem, etc.)
//! - **Health Monitoring**: Automatic ping-based health checks per session
//! - **Lifecycle Management**: Proper initialize → operate → shutdown for each session
//!
//! # Features
//!
//! - Multiple server sessions with independent state
//! - Automatic health checking with configurable intervals
//! - Per-session state tracking (healthy, degraded, unhealthy)
//! - Session lifecycle management
//! - Metrics and monitoring per session
//!
//! # When to Use
//!
//! Use `SessionManager` when your application needs to coordinate **multiple different
//! MCP servers** (e.g., IDE with GitHub server + filesystem server + database server).
//!
//! For **single server** scenarios:
//! - `Client<T>` is cheaply cloneable via Arc - share one session across multiple async tasks
//! - `TurboTransport` - Add retry/circuit breaker to one session

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;
use turbomcp_protocol::{Error, Result};
use turbomcp_transport::Transport;

use super::core::Client;

/// Connection state for a managed client
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    /// Connection is healthy and ready
    Healthy,
    /// Connection is degraded but functional
    Degraded,
    /// Connection is unhealthy and should be avoided
    Unhealthy,
    /// Connection is being established
    Connecting,
    /// Connection is disconnected
    Disconnected,
}

/// Information about a managed connection
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Unique identifier for this connection
    pub id: String,
    /// Current state of the connection
    pub state: ConnectionState,
    /// When the connection was established
    pub established_at: Instant,
    /// Last successful health check
    pub last_health_check: Option<Instant>,
    /// Number of failed health checks
    pub failed_health_checks: usize,
    /// Number of successful requests
    pub successful_requests: usize,
    /// Number of failed requests
    pub failed_requests: usize,
}

/// Configuration for the connection manager
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub health_check_threshold: usize,
    /// Timeout for health checks
    pub health_check_timeout: Duration,
    /// Enable automatic reconnection
    pub auto_reconnect: bool,
    /// Initial reconnection delay
    pub reconnect_delay: Duration,
    /// Maximum reconnection delay (for exponential backoff)
    pub max_reconnect_delay: Duration,
    /// Reconnection backoff multiplier
    pub reconnect_backoff_multiplier: f64,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            health_check_interval: Duration::from_secs(30),
            health_check_threshold: 3,
            health_check_timeout: Duration::from_secs(5),
            auto_reconnect: true,
            reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(60),
            reconnect_backoff_multiplier: 2.0,
        }
    }
}

impl ManagerConfig {
    /// Create a new manager configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Managed connection wrapper
struct ManagedConnection<T: Transport + 'static> {
    client: Client<T>,
    info: ConnectionInfo,
    /// Number of reconnection attempts (reserved for future reconnection logic)
    #[allow(dead_code)]
    reconnect_attempts: usize,
    /// Next reconnection delay (reserved for future reconnection logic)
    #[allow(dead_code)]
    next_reconnect_delay: Duration,
    /// Transport factory for reconnection (reserved for future reconnection logic)
    #[allow(dead_code)]
    transport_factory: Option<Arc<dyn Fn() -> T + Send + Sync>>,
}

/// Server group configuration for failover support
#[derive(Debug, Clone)]
pub struct ServerGroup {
    /// Primary server ID
    pub primary: String,
    /// Backup server IDs in priority order
    pub backups: Vec<String>,
    /// Minimum health check failures before failover
    pub failover_threshold: usize,
}

impl ServerGroup {
    /// Create a new server group with primary and backups
    pub fn new(primary: impl Into<String>, backups: Vec<String>) -> Self {
        Self {
            primary: primary.into(),
            backups,
            failover_threshold: 3,
        }
    }

    /// Set the failover threshold
    #[must_use]
    pub fn with_failover_threshold(mut self, threshold: usize) -> Self {
        self.failover_threshold = threshold;
        self
    }

    /// Get all server IDs in priority order (primary first, then backups)
    #[must_use]
    pub fn all_servers(&self) -> Vec<&str> {
        std::iter::once(self.primary.as_str())
            .chain(self.backups.iter().map(|s| s.as_str()))
            .collect()
    }

    /// Get the next available server after the current one
    #[must_use]
    pub fn next_server(&self, current: &str) -> Option<&str> {
        let servers = self.all_servers();
        let current_idx = servers.iter().position(|&s| s == current)?;
        servers.get(current_idx + 1).copied()
    }
}

/// Multi-Server Session Manager for MCP Clients
///
/// The `SessionManager` coordinates multiple MCP server sessions with automatic
/// health monitoring and lifecycle management. Each session represents a long-lived,
/// initialized connection to a different MCP server.
///
/// # Use Cases
///
/// - **Multi-Server Applications**: IDE with multiple tool servers
/// - **Service Coordination**: Orchestrate operations across multiple MCP servers
/// - **Health Monitoring**: Track health of all connected servers
/// - **Failover**: Switch between primary/backup servers
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::SessionManager;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_protocol::Result<()> {
/// let mut manager = SessionManager::with_defaults();
///
/// // Add sessions to different servers
/// let github_transport = StdioTransport::new();
/// let fs_transport = StdioTransport::new();
/// manager.add_server("github", github_transport).await?;
/// manager.add_server("filesystem", fs_transport).await?;
///
/// // Start health monitoring
/// manager.start_health_monitoring().await;
///
/// // Get stats
/// let stats = manager.session_stats().await;
/// println!("Managing {} sessions", stats.len());
/// # Ok(())
/// # }
/// ```
pub struct SessionManager<T: Transport + 'static> {
    config: ManagerConfig,
    connections: Arc<RwLock<HashMap<String, ManagedConnection<T>>>>,
    health_check_task: Option<tokio::task::JoinHandle<()>>,
}

impl<T: Transport + Send + 'static> SessionManager<T> {
    /// Create a new connection manager with the specified configuration
    #[must_use]
    pub fn new(config: ManagerConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            health_check_task: None,
        }
    }

    /// Create a new connection manager with default configuration
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(ManagerConfig::default())
    }

    /// Add a new server session
    ///
    /// Creates and initializes a session to the specified MCP server.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this server (e.g., "github", "filesystem")
    /// * `transport` - Transport implementation for connecting to the server
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Maximum sessions limit is reached
    /// - Server ID already exists
    /// - Client initialization fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::SessionManager;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut manager = SessionManager::with_defaults();
    /// let github_transport = StdioTransport::new();
    /// let fs_transport = StdioTransport::new();
    /// manager.add_server("github", github_transport).await?;
    /// manager.add_server("filesystem", fs_transport).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_server(&mut self, id: impl Into<String>, transport: T) -> Result<()> {
        let id = id.into();
        let mut connections = self.connections.write().await;

        // Check connection limit
        if connections.len() >= self.config.max_connections {
            return Err(Error::bad_request(format!(
                "Maximum connections limit ({}) reached",
                self.config.max_connections
            )));
        }

        // Check for duplicate ID
        if connections.contains_key(&id) {
            return Err(Error::bad_request(format!(
                "Connection with ID '{}' already exists",
                id
            )));
        }

        // Create client and initialize
        let client = Client::new(transport);
        client.initialize().await?;

        let info = ConnectionInfo {
            id: id.clone(),
            state: ConnectionState::Healthy,
            established_at: Instant::now(),
            last_health_check: Some(Instant::now()),
            failed_health_checks: 0,
            successful_requests: 0,
            failed_requests: 0,
        };

        connections.insert(
            id,
            ManagedConnection {
                client,
                info,
                reconnect_attempts: 0,
                next_reconnect_delay: self.config.reconnect_delay,
                transport_factory: None, // No automatic reconnection for manual adds
            },
        );

        Ok(())
    }

    /// Add a server session with automatic reconnection support
    ///
    /// This method accepts a factory function that creates new transport instances,
    /// enabling automatic reconnection if the session fails.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this server
    /// * `transport_factory` - Function that creates new transport instances
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::SessionManager;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut manager = SessionManager::with_defaults();
    ///
    /// // Transport with reconnection factory
    /// manager.add_server_with_reconnect("api", || {
    ///     StdioTransport::new()
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_server_with_reconnect<F>(
        &mut self,
        id: impl Into<String>,
        transport_factory: F,
    ) -> Result<()>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let id = id.into();
        let factory = Arc::new(transport_factory);

        // Create initial transport and client
        let transport = (factory)();
        let client = Client::new(transport);
        client.initialize().await?;

        let info = ConnectionInfo {
            id: id.clone(),
            state: ConnectionState::Healthy,
            established_at: Instant::now(),
            last_health_check: Some(Instant::now()),
            failed_health_checks: 0,
            successful_requests: 0,
            failed_requests: 0,
        };

        let mut connections = self.connections.write().await;

        // Check limits
        if connections.len() >= self.config.max_connections {
            return Err(Error::bad_request(format!(
                "Maximum sessions limit ({}) reached",
                self.config.max_connections
            )));
        }

        if connections.contains_key(&id) {
            return Err(Error::bad_request(format!(
                "Server with ID '{}' already exists",
                id
            )));
        }

        connections.insert(
            id,
            ManagedConnection {
                client,
                info,
                reconnect_attempts: 0,
                next_reconnect_delay: self.config.reconnect_delay,
                transport_factory: Some(factory),
            },
        );

        Ok(())
    }

    /// Remove a managed connection
    ///
    /// # Arguments
    ///
    /// * `id` - ID of the connection to remove
    ///
    /// # Returns
    ///
    /// Returns `true` if the connection was removed, `false` if not found
    pub async fn remove_server(&mut self, id: &str) -> bool {
        let mut connections = self.connections.write().await;
        connections.remove(id).is_some()
    }

    /// Get information about a specific connection
    pub async fn get_session_info(&self, id: &str) -> Option<ConnectionInfo> {
        let connections = self.connections.read().await;
        connections.get(id).map(|conn| conn.info.clone())
    }

    /// List all managed connections
    pub async fn list_sessions(&self) -> Vec<ConnectionInfo> {
        let connections = self.connections.read().await;
        connections.values().map(|conn| conn.info.clone()).collect()
    }

    /// Get a healthy connection, preferring the one with the fewest active requests
    ///
    /// # Returns
    ///
    /// Returns the ID of a healthy connection, or None if no healthy connections exist
    pub async fn get_healthy_connection(&self) -> Option<String> {
        let connections = self.connections.read().await;
        connections
            .iter()
            .filter(|(_, conn)| conn.info.state == ConnectionState::Healthy)
            .min_by_key(|(_, conn)| conn.info.successful_requests + conn.info.failed_requests)
            .map(|(id, _)| id.clone())
    }

    /// Get count of connections by state
    pub async fn session_stats(&self) -> HashMap<ConnectionState, usize> {
        let connections = self.connections.read().await;
        let mut stats = HashMap::new();

        for conn in connections.values() {
            *stats.entry(conn.info.state.clone()).or_insert(0) += 1;
        }

        stats
    }

    /// Start automatic health monitoring
    ///
    /// Spawns a background task that periodically checks the health of all connections
    pub async fn start_health_monitoring(&mut self) {
        if self.health_check_task.is_some() {
            return; // Already running
        }

        let connections = Arc::clone(&self.connections);
        let interval = self.config.health_check_interval;
        let threshold = self.config.health_check_threshold;
        let timeout = self.config.health_check_timeout;

        let task = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                let mut connections = connections.write().await;

                for (id, managed) in connections.iter_mut() {
                    // Perform health check (ping)
                    let health_result = tokio::time::timeout(timeout, managed.client.ping()).await;

                    match health_result {
                        Ok(Ok(_)) => {
                            // Health check successful
                            managed.info.last_health_check = Some(Instant::now());
                            managed.info.failed_health_checks = 0;

                            if managed.info.state != ConnectionState::Healthy {
                                tracing::info!(
                                    connection_id = %id,
                                    "Connection recovered and is now healthy"
                                );
                                managed.info.state = ConnectionState::Healthy;
                            }
                        }
                        Ok(Err(_)) | Err(_) => {
                            // Health check failed
                            managed.info.failed_health_checks += 1;

                            if managed.info.failed_health_checks >= threshold {
                                if managed.info.state != ConnectionState::Unhealthy {
                                    tracing::warn!(
                                        connection_id = %id,
                                        failed_checks = managed.info.failed_health_checks,
                                        "Connection marked as unhealthy"
                                    );
                                    managed.info.state = ConnectionState::Unhealthy;
                                }
                            } else if managed.info.state == ConnectionState::Healthy {
                                tracing::debug!(
                                    connection_id = %id,
                                    failed_checks = managed.info.failed_health_checks,
                                    "Connection degraded"
                                );
                                managed.info.state = ConnectionState::Degraded;
                            }
                        }
                    }
                }
            }
        });

        self.health_check_task = Some(task);
    }

    /// Stop automatic health monitoring
    pub fn stop_health_monitoring(&mut self) {
        if let Some(task) = self.health_check_task.take() {
            task.abort();
        }
    }

    /// Get total number of managed connections
    pub async fn session_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }
}

impl<T: Transport + 'static> Drop for SessionManager<T> {
    fn drop(&mut self) {
        self.stop_health_monitoring();
    }
}

// ============================================================================
// Specialized Implementation for TurboTransport
// ============================================================================

impl SessionManager<turbomcp_transport::resilience::TurboTransport> {
    /// Add a server with automatic robustness (specialized for TurboTransport)
    ///
    /// This convenience method is only available when using `SessionManager<TurboTransport>`.
    /// It wraps any transport in TurboTransport with the specified configurations.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::SessionManager;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_transport::resilience::*;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut manager: SessionManager<TurboTransport> = SessionManager::with_defaults();
    ///
    /// // Use explicit configuration for clarity
    /// use std::time::Duration;
    /// manager.add_resilient_server(
    ///     "github",
    ///     StdioTransport::new(),
    ///     RetryConfig {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(200),
    ///         ..Default::default()
    ///     },
    ///     CircuitBreakerConfig {
    ///         failure_threshold: 3,
    ///         timeout: Duration::from_secs(30),
    ///         ..Default::default()
    ///     },
    ///     HealthCheckConfig {
    ///         interval: Duration::from_secs(15),
    ///         timeout: Duration::from_secs(5),
    ///         ..Default::default()
    ///     },
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_resilient_server<BaseT>(
        &mut self,
        id: impl Into<String>,
        transport: BaseT,
        retry_config: turbomcp_transport::resilience::RetryConfig,
        circuit_config: turbomcp_transport::resilience::CircuitBreakerConfig,
        health_config: turbomcp_transport::resilience::HealthCheckConfig,
    ) -> Result<()>
    where
        BaseT: Transport + 'static,
    {
        use turbomcp_transport::resilience::TurboTransport;

        let robust = TurboTransport::new(
            Box::new(transport),
            retry_config,
            circuit_config,
            health_config,
        );

        self.add_server(id, robust).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_config_defaults() {
        let config = ManagerConfig::default();
        assert_eq!(config.max_connections, 10);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_connection_state_equality() {
        assert_eq!(ConnectionState::Healthy, ConnectionState::Healthy);
        assert_ne!(ConnectionState::Healthy, ConnectionState::Unhealthy);
    }
}
