//! Example demonstrating state management and custom configuration

use serde_json::json;
use turbomcp_core::StateManager;
use turbomcp_server::{McpServer, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with debug level
    tracing_subscriber::fmt().with_env_filter("debug").init();

    tracing::info!("Starting TurboMCP server with custom state");

    // Create custom configuration
    let config = ServerConfig {
        name: "stateful-server".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Example server with pre-populated state".to_string()),
        ..Default::default()
    };

    // Create server with custom config
    let _server = McpServer::new(config);

    // Create state manager and pre-populate some state
    let state = StateManager::new();
    state.set(
        "initial_key".to_string(),
        json!({
            "message": "Welcome to the stateful TurboMCP server",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "features": ["state_management", "custom_config", "json_storage"]
        }),
    );

    state.set(
        "app_config".to_string(),
        json!({
            "theme": "dark",
            "language": "en",
            "notifications_enabled": true
        }),
    );

    tracing::info!("Server initialized with pre-populated state");
    tracing::info!("State size: {}", state.size());

    // In a real application, you would run the server here
    // For this example, we'll just demonstrate the state functionality
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Show that state persists and can be accessed
    if let Some(initial_data) = state.get("initial_key") {
        tracing::info!("Retrieved initial data: {}", initial_data);
    }

    // Demonstrate export/import
    let exported = state.export();
    tracing::info!("Exported state: {}", exported);

    let new_state = StateManager::new();
    new_state.import(exported)?;
    tracing::info!(
        "Successfully imported state to new manager, size: {}",
        new_state.size()
    );

    Ok(())
}
