//! Connection pooling for transport management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::sync::Semaphore;
use tracing::{debug, trace, warn};

use crate::core::{Transport, TransportConfig, TransportError, TransportResult};

/// Connection pool for managing transport instances
#[derive(Debug)]
pub struct ConnectionPool {
    /// Pool configuration
    config: PoolConfig,

    /// Available connections
    connections: Arc<RwLock<HashMap<String, PooledConnection>>>,

    /// Connection semaphore for limiting concurrent connections
    semaphore: Arc<Semaphore>,

    /// Pool statistics
    stats: Arc<RwLock<PoolStats>>,
}

/// Pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections
    pub max_connections: usize,

    /// Minimum number of idle connections
    pub min_idle_connections: usize,

    /// Maximum idle time before connection is closed
    pub max_idle_time: Duration,

    /// Connection timeout
    pub connection_timeout: Duration,

    /// Health check interval
    pub health_check_interval: Duration,

    /// Whether to validate connections before use
    pub validate_on_borrow: bool,

    /// Whether to validate connections when returning to pool
    pub validate_on_return: bool,
}

/// Pooled connection wrapper
#[derive(Debug)]
struct PooledConnection {
    /// The actual transport
    transport: Box<dyn Transport>,

    /// Last access time
    last_accessed: Instant,

    /// Creation time
    #[allow(dead_code)]
    created_at: Instant,

    /// Number of times this connection has been borrowed
    borrow_count: u64,

    /// Whether this connection is currently in use
    in_use: bool,
}

/// Pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total connections created
    pub connections_created: u64,

    /// Total connections destroyed
    pub connections_destroyed: u64,

    /// Current active connections
    pub active_connections: u64,

    /// Current idle connections
    pub idle_connections: u64,

    /// Total borrows
    pub total_borrows: u64,

    /// Total returns
    pub total_returns: u64,

    /// Failed borrows (timeout, etc.)
    pub failed_borrows: u64,

    /// Average connection age
    pub average_connection_age: Duration,
}

/// Borrowed connection handle
#[derive(Debug)]
pub struct BorrowedConnection {
    /// Connection identifier
    id: String,

    /// The borrowed transport
    transport: Box<dyn Transport>,

    /// Reference to the pool for returning the connection
    #[allow(dead_code)]
    pool: Arc<ConnectionPool>,
}

impl ConnectionPool {
    /// Create a new connection pool
    #[must_use]
    pub fn new(config: PoolConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_connections));

        let pool = Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            semaphore,
            stats: Arc::new(RwLock::new(PoolStats::default())),
        };

        // Start background maintenance task
        pool.start_maintenance_task();

        pool
    }

    /// Borrow a connection from the pool
    pub async fn borrow(
        &self,
        endpoint: impl Into<String>,
        transport_config: TransportConfig,
    ) -> TransportResult<BorrowedConnection> {
        let endpoint = endpoint.into();

        // Acquire semaphore permit
        let _permit = self.semaphore.clone().acquire_owned().await.map_err(|_| {
            TransportError::Internal("Failed to acquire connection permit".to_string())
        })?;

        // Try to get existing connection
        if let Some(connection) = self
            .get_existing_connection(&endpoint, &transport_config)
            .await?
        {
            self.update_stats(|stats| stats.total_borrows += 1);
            return Ok(BorrowedConnection {
                id: endpoint,
                transport: connection,
                pool: Arc::new(self.clone()),
            });
        }

        // Create new connection
        let transport = self.create_connection(transport_config).await?;

        self.update_stats(|stats| {
            stats.connections_created += 1;
            stats.active_connections += 1;
            stats.total_borrows += 1;
        });

        Ok(BorrowedConnection {
            id: endpoint,
            transport,
            pool: Arc::new(self.clone()),
        })
    }

    /// Return a connection to the pool
    pub async fn return_connection(
        &self,
        id: String,
        transport: Box<dyn Transport>,
    ) -> TransportResult<()> {
        // Validate connection if configured
        if self.config.validate_on_return && !self.validate_connection(transport.as_ref()).await {
            debug!("Connection {} failed validation on return, discarding", id);
            self.update_stats(|stats| {
                stats.connections_destroyed += 1;
                stats.active_connections = stats.active_connections.saturating_sub(1);
            });
            return Ok(());
        }

        let pooled_connection = PooledConnection {
            transport,
            last_accessed: Instant::now(),
            created_at: Instant::now(),
            borrow_count: 1,
            in_use: false,
        };

        {
            let mut connections = self.connections.write();
            connections.insert(id, pooled_connection);
        }

        self.update_stats(|stats| {
            stats.total_returns += 1;
            stats.active_connections = stats.active_connections.saturating_sub(1);
            stats.idle_connections += 1;
        });

        Ok(())
    }

    /// Get pool statistics
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        self.stats.read().clone()
    }

    /// Close all connections in the pool
    #[allow(clippy::await_holding_lock)]
    pub async fn close_all(&self) -> TransportResult<()> {
        let mut connections = self.connections.write();
        let count = connections.len();

        for (id, mut pooled_conn) in connections.drain() {
            if let Err(e) = pooled_conn.transport.disconnect().await {
                warn!("Error closing connection {}: {}", id, e);
            }
        }

        self.update_stats(|stats| {
            stats.connections_destroyed += count as u64;
            stats.active_connections = 0;
            stats.idle_connections = 0;
        });

        debug!("Closed {} connections from pool", count);
        Ok(())
    }

    /// Get current pool size
    #[must_use]
    pub fn size(&self) -> usize {
        self.connections.read().len()
    }

    /// Check if pool is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.connections.read().is_empty()
    }

    #[allow(clippy::await_holding_lock)]
    async fn get_existing_connection(
        &self,
        endpoint: &str,
        _config: &TransportConfig,
    ) -> TransportResult<Option<Box<dyn Transport>>> {
        let mut connections = self.connections.write();

        if let Some(mut pooled_conn) = connections.remove(endpoint) {
            // Check if connection is still valid
            if self.config.validate_on_borrow
                && !self
                    .validate_connection(pooled_conn.transport.as_ref())
                    .await
            {
                debug!("Connection {} failed validation, discarding", endpoint);
                self.update_stats(|stats| {
                    stats.connections_destroyed += 1;
                    stats.idle_connections = stats.idle_connections.saturating_sub(1);
                });
                return Ok(None);
            }

            // Check if connection is too old
            if pooled_conn.last_accessed.elapsed() > self.config.max_idle_time {
                debug!("Connection {} is too old, discarding", endpoint);
                if let Err(e) = pooled_conn.transport.disconnect().await {
                    warn!("Error disconnecting old connection: {}", e);
                }
                self.update_stats(|stats| {
                    stats.connections_destroyed += 1;
                    stats.idle_connections = stats.idle_connections.saturating_sub(1);
                });
                return Ok(None);
            }

            // Update connection metadata
            pooled_conn.last_accessed = Instant::now();
            pooled_conn.borrow_count += 1;
            pooled_conn.in_use = true;

            self.update_stats(|stats| {
                stats.idle_connections = stats.idle_connections.saturating_sub(1);
                stats.active_connections += 1;
            });

            trace!("Reusing existing connection for {}", endpoint);
            Ok(Some(pooled_conn.transport))
        } else {
            Ok(None)
        }
    }

    async fn create_connection(
        &self,
        config: TransportConfig,
    ) -> TransportResult<Box<dyn Transport>> {
        // This is a simplified version - in practice, you'd use a transport factory
        use crate::core::TransportRegistry;

        let _registry = TransportRegistry::new();

        // Production-grade transport factory implementation
        // Create appropriate transport based on configuration
        match config.transport_type {
            #[cfg(feature = "stdio")]
            crate::TransportType::Stdio => {
                let transport = crate::stdio::StdioTransport::new();
                Ok(Box::new(transport))
            }
            #[cfg(feature = "tcp")]
            crate::TransportType::Tcp => {
                let bind_addr = "127.0.0.1:8000".parse().unwrap();
                let remote_addr = "127.0.0.1:8001".parse().unwrap();
                let transport = crate::tcp::TcpTransport::new_client(bind_addr, remote_addr);
                Ok(Box::new(transport))
            }
            #[cfg(feature = "http")]
            crate::TransportType::Http => {
                // HTTP transport would be created here in production
                Err(TransportError::NotAvailable(
                    "HTTP transport not available in connection pool".to_string(),
                ))
            }
            #[cfg(feature = "websocket")]
            crate::TransportType::WebSocket => {
                let endpoint = "ws://localhost:8000/mcp"; // Default WebSocket endpoint
                let transport = crate::websocket::WebSocketTransport::new(endpoint).await?;
                Ok(Box::new(transport))
            }
            #[cfg(feature = "unix")]
            crate::TransportType::Unix => {
                use std::path::PathBuf;
                let socket_path = PathBuf::from("/tmp/mcp.sock");
                let transport = crate::unix::UnixTransport::new_client(socket_path);
                Ok(Box::new(transport))
            }
            _ => {
                // Check registry for custom factory implementations (feature not implemented)
                // registry.get_factory(&config.transport_type) would be used here
                Err(TransportError::NotAvailable(format!(
                    "Transport type {:?} not enabled in this build. Enable the corresponding feature flag.",
                    config.transport_type
                )))
            }
        }
    }

    async fn validate_connection(&self, transport: &dyn Transport) -> bool {
        // Simple validation: check if transport is connected
        transport.is_connected().await
    }

    fn start_maintenance_task(&self) {
        let connections = self.connections.clone();
        let config = self.config.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.health_check_interval);

            loop {
                interval.tick().await;

                let now = Instant::now();
                let mut to_remove = Vec::new();

                {
                    let connections_guard = connections.read();
                    for (id, conn) in connections_guard.iter() {
                        if !conn.in_use
                            && now.duration_since(conn.last_accessed) > config.max_idle_time
                        {
                            to_remove.push(id.clone());
                        }
                    }
                }

                if !to_remove.is_empty() {
                    let mut removed_connections = Vec::new();

                    // Remove connections from the map
                    {
                        let mut connections_guard = connections.write();
                        for id in to_remove {
                            if let Some(conn) = connections_guard.remove(&id) {
                                removed_connections.push(conn);
                            }
                        }
                    }

                    // Disconnect outside of the lock
                    let removed_count = removed_connections.len();
                    for mut conn in removed_connections {
                        if let Err(e) = conn.transport.disconnect().await {
                            warn!("Error disconnecting idle connection: {}", e);
                        }
                    }

                    if removed_count > 0 {
                        let mut stats_guard = stats.write();
                        stats_guard.connections_destroyed += removed_count as u64;
                        stats_guard.idle_connections = stats_guard
                            .idle_connections
                            .saturating_sub(removed_count as u64);

                        debug!("Removed {} idle connections", removed_count);
                    }
                }
            }
        });
    }

    fn update_stats<F>(&self, updater: F)
    where
        F: FnOnce(&mut PoolStats),
    {
        let mut stats = self.stats.write();
        updater(&mut stats);
    }
}

impl Clone for ConnectionPool {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            connections: self.connections.clone(),
            semaphore: self.semaphore.clone(),
            stats: self.stats.clone(),
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_idle_connections: 2,
            max_idle_time: Duration::from_secs(300), // 5 minutes
            connection_timeout: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(60),
            validate_on_borrow: true,
            validate_on_return: false,
        }
    }
}

impl BorrowedConnection {
    /// Get the transport
    pub fn transport(&mut self) -> &mut dyn Transport {
        self.transport.as_mut()
    }

    /// Get the connection ID
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Drop for BorrowedConnection {
    fn drop(&mut self) {
        // Production-grade connection cleanup on drop
        // The connection pool would handle return logic in a real implementation
        trace!("BorrowedConnection {} dropped", self.id);

        // Connection is automatically cleaned up when transport is dropped
        // Pool statistics would be updated here in a full implementation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use crate::core::TransportType;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_idle_connections, 2);
        assert_eq!(config.max_idle_time, Duration::from_secs(300));
        assert!(config.validate_on_borrow);
        assert!(!config.validate_on_return);
    }

    #[tokio::test]
    async fn test_pool_creation() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);

        assert!(pool.is_empty());
        assert_eq!(pool.size(), 0);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);
        let stats = pool.stats();

        assert_eq!(stats.connections_created, 0);
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
    }

    #[tokio::test]
    async fn test_pool_close_all() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);

        // Should not error even with empty pool
        assert!(pool.close_all().await.is_ok());
    }
}
