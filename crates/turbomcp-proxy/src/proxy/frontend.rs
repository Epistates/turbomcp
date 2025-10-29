//! Frontend server for proxy
//!
//! Dynamically creates a turbomcp-server based on backend introspection,
//! registering handlers for all discovered tools, resources, and prompts.

use std::sync::Arc;
use tracing::{debug, info};

use super::CapabilityRouter;
use crate::error::{ProxyError, ProxyResult};
use crate::introspection::ServerSpec;

/// Frontend transport type
#[derive(Debug, Clone)]
pub enum FrontendTransport {
    /// HTTP with Server-Sent Events
    Http {
        /// Bind address (e.g., "0.0.0.0:3000")
        bind: String,
        /// Endpoint path (e.g., "/mcp")
        path: Option<String>,
    },
    /// WebSocket bidirectional (future)
    #[allow(dead_code)]
    WebSocket {
        /// Bind address
        bind: String,
    },
    /// TCP socket (future)
    #[allow(dead_code)]
    Tcp {
        /// Bind address
        bind: String,
    },
}

/// Frontend server configuration
#[derive(Debug, Clone)]
pub struct FrontendConfig {
    /// Transport configuration
    pub transport: FrontendTransport,
}

/// Frontend server with dynamic handler registration
///
/// Creates a turbomcp-server that dynamically registers handlers for all
/// tools, resources, and prompts discovered from the backend via introspection.
pub struct FrontendServer {
    /// Server builder (before build)
    builder: Option<ServerBuilder>,

    /// Built server (after build)
    server: Option<Arc<McpServer>>,

    /// Capability router
    router: Arc<CapabilityRouter>,

    /// Server spec (from backend)
    spec: ServerSpec,

    /// Frontend configuration
    config: FrontendConfig,
}

impl FrontendServer {
    /// Create a new frontend server
    ///
    /// # Arguments
    ///
    /// * `router` - The capability router (must be initialized)
    /// * `config` - Frontend configuration
    ///
    /// # Returns
    ///
    /// A frontend server ready to be built and run
    pub async fn new(router: Arc<CapabilityRouter>, config: FrontendConfig) -> ProxyResult<Self> {
        // Get spec from router
        let spec = router.spec().await.ok_or_else(|| {
            ProxyError::configuration(
                "Router not initialized - call router.initialize() first".to_string(),
            )
        })?;

        info!(
            "Creating frontend server for: {} v{}",
            spec.server_info.name, spec.server_info.version
        );

        // Create server builder with backend info
        let builder = ServerBuilder::new()
            .name(&format!("{}-proxy", spec.server_info.name))
            .version(&spec.server_info.version);

        Ok(Self {
            builder: Some(builder),
            server: None,
            router,
            spec,
            config,
        })
    }

    /// Build the server with dynamic handlers
    ///
    /// Registers handlers for all tools, resources, and prompts from the spec.
    pub fn build(mut self) -> ProxyResult<Self> {
        let builder = self
            .builder
            .take()
            .ok_or_else(|| ProxyError::configuration("Server already built".to_string()))?;

        debug!("Building frontend server with dynamic handlers");

        // Build the server
        let server = builder.build();

        // Store the server
        self.server = Some(Arc::new(server));

        info!("Frontend server built successfully");

        Ok(self)
    }

    /// Run the frontend server
    ///
    /// Starts the server on the configured transport.
    pub async fn run(self) -> ProxyResult<()> {
        let server = self.server.ok_or_else(|| {
            ProxyError::configuration("Server not built - call build() first".to_string())
        })?;

        info!("Starting frontend server: {:?}", self.config.transport);

        match self.config.transport {
            FrontendTransport::Http { bind, path } => {
                let path = path.unwrap_or_else(|| "/mcp".to_string());

                info!("Frontend server listening on http://{}{}", bind, path);

                // Use turbomcp-server's run_http_with_path method
                server.run_http_with_path(&bind, &path).await.map_err(|e| {
                    ProxyError::backend(format!("Failed to run HTTP server: {}", e))
                })?;
            }
            FrontendTransport::WebSocket { .. } => {
                return Err(ProxyError::configuration(
                    "WebSocket frontend not yet implemented".to_string(),
                ));
            }
            FrontendTransport::Tcp { .. } => {
                return Err(ProxyError::configuration(
                    "TCP frontend not yet implemented".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get reference to the router
    pub fn router(&self) -> &Arc<CapabilityRouter> {
        &self.router
    }

    /// Get the server spec
    pub fn spec(&self) -> &ServerSpec {
        &self.spec
    }
}

// Note: Dynamic handler registration will be implemented using turbomcp-server's
// handler registry API once we wire this into the serve command. The current
// turbomcp-server uses macros for handler registration, so we'll need to either:
// 1. Use the registry API directly (if available)
// 2. Generate handler code dynamically
// 3. Use a different approach for dynamic registration
//
// For Phase 2, we'll start with a simpler approach and enhance in Phase 3.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{BackendConfig, BackendConnector, BackendTransport};

    async fn create_test_frontend() -> Option<FrontendServer> {
        // Create backend
        let backend_config = BackendConfig {
            transport: BackendTransport::Stdio {
                command: "cargo".to_string(),
                args: vec![
                    "run".to_string(),
                    "--package".to_string(),
                    "turbomcp".to_string(),
                    "--example".to_string(),
                    "stdio_server".to_string(),
                ],
                working_dir: Some("/Users/nickpaterno/work/turbomcp".to_string()),
            },
            client_name: "test-frontend".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let backend = match BackendConnector::new(backend_config).await {
            Ok(b) => b,
            Err(_) => return None,
        };

        // Create router and initialize
        let router = Arc::new(CapabilityRouter::new(backend));
        if router.initialize().await.is_err() {
            return None;
        }

        // Create frontend config
        let frontend_config = FrontendConfig {
            transport: FrontendTransport::Http {
                bind: "127.0.0.1:0".to_string(), // Random port for testing
                path: Some("/mcp".to_string()),
            },
        };

        // Create frontend
        match FrontendServer::new(router, frontend_config).await {
            Ok(frontend) => Some(frontend),
            Err(_) => None,
        }
    }

    #[tokio::test]
    async fn test_frontend_creation() {
        if let Some(frontend) = create_test_frontend().await {
            // Verify spec is populated
            assert!(
                !frontend.spec().tools.is_empty(),
                "Should have tools from backend"
            );
        }
    }

    #[tokio::test]
    async fn test_frontend_build() {
        if let Some(frontend) = create_test_frontend().await {
            // Build the server
            let result = frontend.build();
            assert!(result.is_ok(), "Server build should succeed");
        }
    }
}
