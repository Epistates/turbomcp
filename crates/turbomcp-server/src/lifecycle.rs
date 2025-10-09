//! Server lifecycle management and graceful shutdown

use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::time::Instant;

/// Server lifecycle manager
#[derive(Debug)]
pub struct ServerLifecycle {
    /// Current server state
    state: Arc<RwLock<ServerState>>,
    /// Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,
    /// Health status
    health: Arc<RwLock<HealthStatus>>,
}

/// Server states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// Server is starting up
    Starting,
    /// Server is running normally
    Running,
    /// Server is shutting down
    ShuttingDown,
    /// Server has stopped
    Stopped,
}

/// Health status information
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Overall health
    pub healthy: bool,
    /// Health check timestamp
    pub timestamp: Instant,
    /// Health details
    pub details: Vec<HealthCheck>,
}

/// Individual health check
#[derive(Debug, Clone)]
pub struct HealthCheck {
    /// Check name
    pub name: String,
    /// Check status
    pub healthy: bool,
    /// Check message
    pub message: Option<String>,
    /// Check timestamp
    pub timestamp: Instant,
}

/// Shutdown signal
pub type ShutdownSignal = broadcast::Receiver<()>;

impl ServerLifecycle {
    /// Create a new lifecycle manager
    #[must_use]
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            state: Arc::new(RwLock::new(ServerState::Starting)),
            shutdown_tx,
            health: Arc::new(RwLock::new(HealthStatus {
                healthy: true,
                timestamp: Instant::now(),
                details: Vec::new(),
            })),
        }
    }

    /// Get current server state
    pub async fn state(&self) -> ServerState {
        *self.state.read().await
    }

    /// Set server state
    pub async fn set_state(&self, state: ServerState) {
        *self.state.write().await = state;
    }

    /// Start the server
    pub async fn start(&self) {
        self.set_state(ServerState::Running).await;
        tracing::info!("Server started");
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self) {
        self.set_state(ServerState::ShuttingDown).await;
        let _ = self.shutdown_tx.send(());
        tracing::info!("Server shutdown initiated");
    }

    /// Subscribe to shutdown signals
    #[must_use]
    pub fn shutdown_signal(&self) -> ShutdownSignal {
        self.shutdown_tx.subscribe()
    }

    /// Get health status
    pub async fn health(&self) -> HealthStatus {
        self.health.read().await.clone()
    }

    /// Update health status
    pub async fn update_health(&self, healthy: bool, details: Vec<HealthCheck>) {
        let mut health = self.health.write().await;
        health.healthy = healthy;
        health.timestamp = Instant::now();
        health.details = details;
    }

    /// Add health check
    pub async fn add_health_check(&self, check: HealthCheck) {
        let mut health = self.health.write().await;
        health.details.push(check);
        health.healthy = health.details.iter().all(|c| c.healthy);
        health.timestamp = Instant::now();
    }
}

impl Default for ServerLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthStatus {
    /// Create a healthy status
    #[must_use]
    pub fn healthy() -> Self {
        Self {
            healthy: true,
            timestamp: Instant::now(),
            details: Vec::new(),
        }
    }

    /// Create an unhealthy status
    #[must_use]
    pub fn unhealthy() -> Self {
        Self {
            healthy: false,
            timestamp: Instant::now(),
            details: Vec::new(),
        }
    }
}

impl HealthCheck {
    /// Create a healthy check
    pub fn healthy(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            healthy: true,
            message: None,
            timestamp: Instant::now(),
        }
    }

    /// Create an unhealthy check
    pub fn unhealthy(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            healthy: false,
            message: Some(message.into()),
            timestamp: Instant::now(),
        }
    }
}

// Comprehensive tests in separate file (tokio/axum pattern)
#[cfg(test)]
mod tests;
