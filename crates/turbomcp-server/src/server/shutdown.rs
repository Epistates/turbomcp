//! Graceful server shutdown coordination
//!
//! Provides external control over server shutdown with support for signal handling,
//! container orchestration, health checks, and multi-service coordination.

use std::sync::Arc;

use crate::lifecycle::ServerLifecycle;

/// Handle for triggering graceful server shutdown
///
/// Provides external control over server shutdown with support for:
/// - **Signal handling**: SIGTERM, SIGINT, custom signals
/// - **Container orchestration**: Kubernetes graceful termination
/// - **Health checks**: Coordinated shutdown with load balancers
/// - **Multi-service coordination**: Synchronized shutdown sequences
/// - **Testing**: Controlled server lifecycle in tests
///
/// The handle is cloneable and thread-safe, allowing multiple components
/// to coordinate shutdown or check shutdown status.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    lifecycle: Arc<ServerLifecycle>,
}

impl ShutdownHandle {
    /// Create a new shutdown handle
    pub(crate) fn new(lifecycle: Arc<ServerLifecycle>) -> Self {
        Self { lifecycle }
    }

    /// Trigger graceful server shutdown
    pub async fn shutdown(&self) {
        self.lifecycle.shutdown().await;
    }

    /// Check if shutdown has been initiated
    pub async fn is_shutting_down(&self) -> bool {
        use crate::lifecycle::ServerState;
        matches!(
            self.lifecycle.state().await,
            ServerState::ShuttingDown | ServerState::Stopped
        )
    }
}
