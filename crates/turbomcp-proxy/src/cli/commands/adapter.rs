//! Protocol adapter command
//!
//! Runs protocol adapters (REST API, GraphQL) that bridge MCP to standard web protocols.

use clap::{Args, Subcommand};

use crate::cli::args::BackendArgs;
use crate::cli::output::OutputFormat;
use crate::error::ProxyResult;
use crate::proxy::BackendConnector;
use crate::proxy::BackendTransport;
use crate::proxy::backend::BackendConfig;

/// Protocol adapter command
///
/// Exposes MCP servers through standard web protocols (REST, GraphQL).
#[derive(Debug, Args)]
pub struct AdapterCommand {
    /// Backend configuration
    #[command(flatten)]
    pub backend: BackendArgs,

    /// Adapter protocol
    #[command(subcommand)]
    pub protocol: AdapterProtocol,

    /// Bind address for the adapter server
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:3001")]
    pub bind: String,

    /// Client name for initialization
    #[arg(long, default_value = "turbomcp-proxy")]
    pub client_name: String,

    /// Client version for initialization
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub client_version: String,
}

/// Adapter protocol options
#[derive(Debug, Subcommand)]
pub enum AdapterProtocol {
    /// REST API with `OpenAPI` documentation
    Rest {
        /// Enable Swagger UI at /docs
        #[arg(long)]
        openapi_ui: bool,
    },
    /// GraphQL API with playground
    #[command(name = "graphql", visible_alias = "gql")]
    GraphQL {
        /// Enable GraphQL Playground at /playground
        #[arg(long)]
        playground: bool,
    },
}

impl AdapterCommand {
    /// Execute the adapter command
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if command execution fails.
    pub async fn execute(self, _format: OutputFormat) -> ProxyResult<()> {
        // Validate backend arguments
        self.backend
            .validate()
            .map_err(crate::error::ProxyError::configuration)?;

        // Create backend config from args
        let backend_config = self.create_backend_config()?;

        // Connect to backend
        tracing::info!("Connecting to backend...");
        let backend = BackendConnector::new(backend_config).await?;
        tracing::info!("Backend connected successfully");

        // Introspect backend
        tracing::info!("Introspecting backend capabilities...");
        let spec = backend.introspect().await?;
        tracing::info!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        // Start adapter based on protocol
        match &self.protocol {
            AdapterProtocol::Rest { openapi_ui } => self.start_rest_adapter(*openapi_ui)?,
            AdapterProtocol::GraphQL { playground } => self.start_graphql_adapter(*playground)?,
        }

        Ok(())
    }

    /// Start REST API adapter
    fn start_rest_adapter(&self, enable_openapi_ui: bool) -> ProxyResult<()> {
        tracing::info!("Starting REST API adapter on {}", self.bind);

        if enable_openapi_ui {
            tracing::info!("  OpenAPI UI: http://{}/docs", self.bind);
        }
        tracing::info!("  API base: http://{}/api", self.bind);

        // NOTE: Phase 2 - REST adapter using Axum
        // - Create REST endpoint handlers for tools and resources
        // - Integrate OpenAPI schema generation
        // - Add Swagger UI if enabled
        Err(crate::error::ProxyError::configuration(
            "REST adapter not yet fully implemented",
        ))
    }

    /// Start GraphQL adapter
    fn start_graphql_adapter(&self, enable_playground: bool) -> ProxyResult<()> {
        tracing::info!("Starting GraphQL adapter on {}", self.bind);

        if enable_playground {
            tracing::info!("  GraphQL Playground: http://{}/playground", self.bind);
        }
        tracing::info!("  GraphQL endpoint: http://{}/graphql", self.bind);

        // NOTE: Phase 2 - GraphQL adapter using async-graphql
        // - Create GraphQL Query root from MCP tools and resources
        // - Set up subscription support for server-driven updates
        // - Add GraphQL Playground if enabled
        Err(crate::error::ProxyError::configuration(
            "GraphQL adapter not yet fully implemented",
        ))
    }

    /// Create backend configuration from args
    fn create_backend_config(&self) -> ProxyResult<BackendConfig> {
        use crate::cli::args::BackendType;

        let transport = match self.backend.backend_type() {
            Some(BackendType::Stdio) => {
                let cmd = self.backend.cmd.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("Command not specified".to_string())
                })?;

                BackendTransport::Stdio {
                    command: cmd.clone(),
                    args: self.backend.args.clone(),
                    working_dir: self
                        .backend
                        .working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                }
            }
            Some(BackendType::Http) => {
                let url = self.backend.http.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("HTTP URL not specified".to_string())
                })?;

                BackendTransport::Http {
                    url: url.clone(),
                    auth_token: None,
                }
            }
            Some(BackendType::Tcp) => {
                let addr = self.backend.tcp.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration("TCP address not specified".to_string())
                })?;

                let parts: Vec<&str> = addr.split(':').collect();
                if parts.len() != 2 {
                    return Err(crate::error::ProxyError::configuration(
                        "Invalid TCP address format. Use host:port".to_string(),
                    ));
                }

                let host = parts[0].to_string();
                let port = parts[1].parse::<u16>().map_err(|_| {
                    crate::error::ProxyError::configuration("Invalid port number".to_string())
                })?;

                BackendTransport::Tcp { host, port }
            }
            Some(BackendType::Unix) => {
                let path = self.backend.unix.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration(
                        "Unix socket path not specified".to_string(),
                    )
                })?;

                BackendTransport::Unix { path: path.clone() }
            }
            Some(BackendType::Websocket) => {
                let url = self.backend.websocket.as_ref().ok_or_else(|| {
                    crate::error::ProxyError::configuration(
                        "WebSocket URL not specified".to_string(),
                    )
                })?;

                BackendTransport::WebSocket { url: url.clone() }
            }
            None => {
                return Err(crate::error::ProxyError::configuration(
                    "No backend specified".to_string(),
                ));
            }
        };

        Ok(BackendConfig {
            transport,
            client_name: self.client_name.clone(),
            client_version: self.client_version.clone(),
        })
    }
}
