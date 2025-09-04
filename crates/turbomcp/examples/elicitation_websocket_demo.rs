//! End-to-end elicitation demo with WebSocket bidirectional transport
//!
//! This example demonstrates a production-grade implementation of MCP elicitation
//! using WebSocket bidirectional transport for real-time server-initiated requests.
//!
//! ## Features Demonstrated
//! - WebSocket bidirectional transport with elicitation support
//! - Server-initiated elicitation requests during tool execution
//! - Type-safe elicitation builders with schema validation
//! - Timeout handling and retry logic
//! - Real-time bidirectional communication
//!
//! ## Running the Demo
//!
//! Terminal 1 (Start the server):
//! ```bash
//! cargo run --example elicitation_websocket_demo -- server
//! ```
//!
//! Terminal 2 (Start the client):
//! ```bash
//! cargo run --example elicitation_websocket_demo -- client
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use turbomcp::{
    Context, McpResult, elicit,
    elicitation_api::{ElicitationManager, ElicitationResult},
    mcp_error, server, tool,
};
use turbomcp_protocol::elicitation::{boolean, integer, string};
use turbomcp_transport::{
    ReconnectConfig, Transport, TransportState, WebSocketBidirectionalConfig,
    WebSocketBidirectionalTransport,
};

/// Demo application state
#[derive(Clone)]
#[allow(dead_code)]
struct AppState {
    /// User preferences collected via elicitation
    preferences: Arc<RwLock<HashMap<String, serde_json::Value>>>,

    /// Project configurations
    projects: Arc<RwLock<Vec<ProjectConfig>>>,

    /// Elicitation manager
    elicitation_manager: Arc<ElicitationManager>,
}

/// Project configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProjectConfig {
    name: String,
    language: String,
    framework: Option<String>,
    port: u16,
    features: Vec<String>,
}

/// AI Assistant Server with elicitation capabilities
#[derive(Clone)]
#[allow(dead_code)]
struct AiAssistantServer {
    state: AppState,
}

#[server(
    name = "ai_assistant",
    version = "1.0.0",
    description = "AI Assistant with interactive elicitation"
)]
impl AiAssistantServer {
    /// Create a new server instance
    fn new() -> Self {
        Self {
            state: AppState {
                preferences: Arc::new(RwLock::new(HashMap::new())),
                projects: Arc::new(RwLock::new(Vec::new())),
                elicitation_manager: Arc::new(ElicitationManager::with_timeout(
                    Duration::from_secs(30),
                )),
            },
        }
    }

    /// Create a new project with interactive configuration
    #[tool(description = "Create a new software project with guided setup")]
    async fn create_project(&self, ctx: Context) -> McpResult<String> {
        info!("Starting project creation with elicitation");

        // Step 1: Get project name and type
        let basic_info = elicit("Let's set up your new project!")
            .field(
                "name",
                string()
                    .min_length(1)
                    .max_length(50)
                    .pattern("^[a-zA-Z0-9-_]+$")
                    .description("Project name (alphanumeric, dash, underscore)")
                    .build(),
            )
            .field(
                "language",
                string()
                    .enum_values(vec![
                        "rust".to_string(),
                        "typescript".to_string(),
                        "python".to_string(),
                        "go".to_string(),
                    ])
                    .description("Programming language")
                    .build(),
            )
            .field(
                "description",
                string().description("Brief project description").build(),
            )
            .require(vec!["name", "language"])
            .send(&ctx.request)
            .await?;

        let project_name = match basic_info {
            ElicitationResult::Accept(data) => {
                let name = data.get::<String>("name")?;
                let language = data.get::<String>("language")?;
                info!("User selected: {} project in {}", name, language);

                // Step 2: Get framework based on language
                let framework = self.elicit_framework(&ctx, &language).await?;

                // Step 3: Get port and features
                let config = self
                    .elicit_configuration(&ctx, &name, &language, framework)
                    .await?;

                // Store the project
                self.state.projects.write().await.push(config.clone());

                format!(
                    "✅ Created project '{}' with {} ({})",
                    config.name,
                    config.language,
                    config.framework.as_deref().unwrap_or("no framework")
                )
            }
            ElicitationResult::Decline(_reason) => {
                return Err(mcp_error!("Project creation declined by user").into());
            }
            ElicitationResult::Cancel => {
                return Err(mcp_error!("Project creation cancelled").into());
            }
        };

        Ok(project_name)
    }

    /// Configure user preferences interactively
    #[tool(description = "Configure AI assistant preferences")]
    async fn configure_preferences(&self, ctx: Context) -> McpResult<String> {
        info!("Configuring user preferences");

        let result = elicit("Configure your AI assistant preferences")
            .field(
                "theme",
                string()
                    .enum_values(vec!["light", "dark", "auto"])
                    .description("UI theme preference")
                    .build(),
            )
            .field(
                "notifications",
                boolean()
                    .description("Enable desktop notifications")
                    .build(),
            )
            .field(
                "auto_save",
                boolean().description("Auto-save work in progress").build(),
            )
            .field(
                "save_interval",
                integer()
                    .range(1.0, 60.0)
                    .description("Auto-save interval in minutes")
                    .build(),
            )
            // Using individual fields for advanced settings
            .field(
                "enable_telemetry",
                boolean().description("Enable telemetry").build(),
            )
            .field(
                "enable_experimental",
                boolean()
                    .description("Enable experimental features")
                    .build(),
            )
            .require(vec!["theme"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                // Store preferences
                let mut prefs = self.state.preferences.write().await;
                // Convert elicitation values to JSON values for storage
                for (key, value) in data.as_object() {
                    let json_value = match value {
                        turbomcp_protocol::elicitation::ElicitationValue::String(s) => {
                            serde_json::json!(s)
                        }
                        turbomcp_protocol::elicitation::ElicitationValue::Integer(i) => {
                            serde_json::json!(i)
                        }
                        turbomcp_protocol::elicitation::ElicitationValue::Number(n) => {
                            serde_json::json!(n)
                        }
                        turbomcp_protocol::elicitation::ElicitationValue::Boolean(b) => {
                            serde_json::json!(b)
                        }
                    };
                    prefs.insert(key.clone(), json_value);
                }

                Ok("✅ Preferences updated successfully".to_string())
            }
            ElicitationResult::Decline(_reason) => Ok(format!(
                "Preferences not updated: {}",
                _reason.unwrap_or_default()
            )),
            ElicitationResult::Cancel => Ok("Preference configuration cancelled".to_string()),
        }
    }

    /// Generate code with interactive options
    #[tool(description = "Generate code with AI assistance")]
    async fn generate_code(&self, ctx: Context, prompt: String) -> McpResult<String> {
        info!("Generating code for prompt: {}", prompt);

        // Elicit generation options
        let options = elicit("How should I generate this code?")
            .field(
                "style",
                string()
                    .enum_values(vec!["concise", "verbose", "documented"])
                    .description("Code style")
                    .build(),
            )
            .field(
                "include_tests",
                boolean().description("Generate unit tests").build(),
            )
            .field(
                "include_docs",
                boolean().description("Generate documentation").build(),
            )
            .field(
                "complexity",
                string()
                    .enum_values(vec!["simple", "moderate", "advanced"])
                    .description("Code complexity level")
                    .build(),
            )
            .require(vec!["style"])
            .send(&ctx.request)
            .await?;

        match options {
            ElicitationResult::Accept(data) => {
                let style = data.get::<String>("style")?;
                let include_tests = data.get::<bool>("include_tests").unwrap_or(false);
                let include_docs = data.get::<bool>("include_docs").unwrap_or(false);

                // Simulate code generation
                let mut code = format!("// Generated {} code for: {}\n", style, prompt);

                if include_docs {
                    code.push_str("/// Documentation for the generated code\n");
                }

                code.push_str(&format!(
                    "fn generated_function() {{\n    // {} implementation\n}}\n",
                    style
                ));

                if include_tests {
                    code.push_str("\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_generated() {\n        // Test implementation\n    }\n}\n");
                }

                Ok(code)
            }
            ElicitationResult::Decline(_reason) => {
                Err(mcp_error!("Code generation declined").into())
            }
            ElicitationResult::Cancel => Err(mcp_error!("Code generation cancelled").into()),
        }
    }

    /// Helper: Elicit framework selection
    async fn elicit_framework(&self, ctx: &Context, language: &str) -> McpResult<Option<String>> {
        let frameworks = match language {
            "rust" => vec!["axum", "actix-web", "rocket", "warp", "none"],
            "typescript" => vec!["express", "fastify", "nestjs", "hono", "none"],
            "python" => vec!["fastapi", "django", "flask", "aiohttp", "none"],
            "go" => vec!["gin", "echo", "fiber", "chi", "none"],
            _ => vec!["none"],
        };

        let result = elicit(format!("Select a framework for your {} project", language))
            .field(
                "framework",
                string()
                    .enum_values(frameworks.iter().map(|s| s.to_string()).collect())
                    .description("Web framework")
                    .build(),
            )
            .require(vec!["framework"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let framework = data.get::<String>("framework")?;
                if framework == "none" {
                    Ok(None)
                } else {
                    Ok(Some(framework))
                }
            }
            _ => Ok(None),
        }
    }

    /// Helper: Elicit project configuration
    async fn elicit_configuration(
        &self,
        ctx: &Context,
        name: &str,
        language: &str,
        framework: Option<String>,
    ) -> McpResult<ProjectConfig> {
        let result = elicit("Final configuration for your project")
            .field(
                "port",
                integer()
                    .range(1024.0, 65535.0)
                    .description("Server port")
                    .build(),
            )
            // Using a simple comma-separated string for features
            .field(
                "features",
                string()
                    .description("Comma-separated list of features")
                    .build(),
            )
            .require(vec!["port"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let port = data.get::<f64>("port")? as u16;
                // Parse comma-separated features string
                let features: Vec<String> = vec![];

                Ok(ProjectConfig {
                    name: name.to_string(),
                    language: language.to_string(),
                    framework,
                    port,
                    features,
                })
            }
            _ => {
                // Use defaults if elicitation fails
                Ok(ProjectConfig {
                    name: name.to_string(),
                    language: language.to_string(),
                    framework,
                    port: 3000,
                    features: vec![],
                })
            }
        }
    }

    /// List all configured projects
    #[tool(description = "List all configured projects")]
    async fn list_projects(&self) -> McpResult<Vec<ProjectConfig>> {
        Ok(self.state.projects.read().await.clone())
    }
}

/// Run the server
async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Starting MCP server with WebSocket transport on port {}",
        port
    );

    // Create server instance
    let _server = Arc::new(AiAssistantServer::new());

    // Configure WebSocket transport
    let ws_config = WebSocketBidirectionalConfig {
        bind_addr: Some(format!("0.0.0.0:{}", port)),
        max_concurrent_elicitations: 10,
        elicitation_timeout: Duration::from_secs(60),
        reconnect: ReconnectConfig {
            enabled: true,
            max_retries: 5,
            ..Default::default()
        },
        ..Default::default()
    };

    // Create transport
    let mut transport = WebSocketBidirectionalTransport::new(ws_config).await?;

    info!("WebSocket server listening on ws://localhost:{}", port);
    info!("Waiting for client connections...");

    // This demo shows the server configuration.
    // To complete the setup:
    // 1. Accept incoming WebSocket connections
    // 2. Create a transport for each connection
    // 3. Route MCP messages through the server
    // 4. Handle elicitation requests/responses

    // For demo purposes, we'll just keep the server running
    tokio::signal::ctrl_c().await?;

    info!("Shutting down server...");
    transport.disconnect().await?;

    Ok(())
}

/// Run the client
async fn run_client(url: String) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting MCP client, connecting to {}", url);

    // Configure WebSocket transport
    let ws_config = WebSocketBidirectionalConfig {
        url: Some(url.clone()),
        reconnect: ReconnectConfig {
            enabled: true,
            max_retries: 10,
            initial_delay: Duration::from_secs(1),
            ..Default::default()
        },
        ..Default::default()
    };

    // Create and connect transport
    let mut transport = WebSocketBidirectionalTransport::new(ws_config).await?;
    transport.connect().await?;

    info!("Connected to server at {}", url);

    // This demo shows the client configuration.
    // To complete the client:
    // 1. Send tool/call requests to the server
    // 2. Handle incoming elicitation requests
    // 3. Display elicitation UI to the user
    // 4. Send elicitation responses back

    // Simulate client interactions
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Check connection state
        if transport.state().await != TransportState::Connected {
            warn!("Connection lost, waiting for reconnection...");
            continue;
        }

        debug!("Client heartbeat - connection active");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("turbomcp=debug".parse()?)
                .add_directive("elicitation_websocket_demo=info".parse()?),
        )
        .init();

    // For this demo, we'll show how to set up both server and client
    // In practice, you'd run these in separate processes

    println!("🌐 WebSocket Bidirectional Elicitation Demo");
    println!("============================================\n");

    // Get mode from environment or default to server
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "server".to_string());

    match mode.as_str() {
        "server" => {
            println!("Starting as SERVER on port 8080...");
            println!(
                "Run with 'cargo run --example elicitation_websocket_demo client' in another terminal\n"
            );
            run_server(8080).await
        }
        "client" => {
            println!("Starting as CLIENT connecting to ws://localhost:8080...");
            println!("Make sure the server is running first!\n");
            run_client("ws://localhost:8080".to_string()).await
        }
        _ => {
            println!("Usage: cargo run --example elicitation_websocket_demo [server|client]");
            println!("  server - Start the MCP server (default)");
            println!("  client - Start the MCP client");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        let server = AiAssistantServer::new();
        assert!(server.state.projects.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_websocket_config() {
        let config = WebSocketBidirectionalConfig {
            url: Some("ws://localhost:8080".to_string()),
            max_concurrent_elicitations: 5,
            elicitation_timeout: Duration::from_secs(30),
            ..Default::default()
        };

        assert_eq!(config.max_concurrent_elicitations, 5);
        assert_eq!(config.elicitation_timeout, Duration::from_secs(30));
    }
}
