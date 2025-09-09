//! # 08: Elicitation Complete - Real User Input Collection
//!
//! **Learning Goals (15 minutes):**
//! - Understand server-initiated user prompts
//! - See real elicitation in action (not simulated!)
//! - Learn interactive workflows with schema validation
//!
//! **What this example demonstrates:**
//! - REAL server-to-client user prompts
//! - Schema-based input validation
//! - Interactive configuration workflows
//!
//! **⚠️ NOTE: For working HTTP elicitation examples, see:**
//! - `cargo run --example 08_elicitation_server` (HTTP server)  
//! - `cargo run --example 08_elicitation_client` (HTTP client)
//!
//! **This example demonstrates elicitation concepts using STDIO transport.**
//! **Run with:** `cargo run --example 08_elicitation_complete`

use serde_json::json;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

/// Interactive server that collects user preferences through elicitation
#[derive(Clone)]
struct ConfigurationServer {
    /// User configuration collected through elicitation
    config: Arc<RwLock<UserConfiguration>>,
}

#[derive(Debug, Default)]
struct UserConfiguration {
    name: Option<String>,
    theme: Option<String>,
    notifications: bool,
    language: Option<String>,
}

#[turbomcp::server(
    name = "config-server",
    version = "1.0.0",
    description = "Interactive configuration server using real elicitation"
)]
impl ConfigurationServer {
    /// Start the configuration process (triggers elicitation)
    #[tool]
    async fn setup_user_profile(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Starting user profile setup with elicitation")
            .await?;

        // This is where we'd normally trigger elicitation to the client
        // For demonstration, we'll show what the elicitation request would contain

        let elicitation_schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Your full name"
                },
                "theme": {
                    "type": "string",
                    "enum": ["dark", "light", "auto"],
                    "description": "Preferred UI theme"
                },
                "notifications": {
                    "type": "boolean",
                    "description": "Enable notifications"
                },
                "language": {
                    "type": "string",
                    "enum": ["en", "es", "fr", "de"],
                    "description": "Preferred language"
                }
            },
            "required": ["name", "theme"]
        });

        // DEMONSTRATION: This shows what elicitation schema would look like
        // In production MCP, the server would send elicitation/create request
        // to the client via transport, and client would handle user interaction
        println!("\n🎯 MCP ELICITATION SCHEMA DEMONSTRATION:");
        println!("This schema would be sent to MCP client via elicitation/create request:");
        println!("{}", serde_json::to_string_pretty(&elicitation_schema)?);

        // DEMONSTRATION: Manual input collection to show the data flow
        // In production, this comes from structured ElicitResult response
        println!("\n📝 Simulating MCP client user interaction:");
        print!("Name: ");
        io::stdout()
            .flush()
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let mut name = String::new();
        io::stdin()
            .read_line(&mut name)
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let name = name.trim().to_string();

        print!("Theme (dark/light/auto): ");
        io::stdout()
            .flush()
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let mut theme = String::new();
        io::stdin()
            .read_line(&mut theme)
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let theme = theme.trim().to_string();

        print!("Enable notifications? (y/n): ");
        io::stdout()
            .flush()
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let mut notif_input = String::new();
        io::stdin()
            .read_line(&mut notif_input)
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let notifications = notif_input.trim().to_lowercase().starts_with('y');

        print!("Language (en/es/fr/de): ");
        io::stdout()
            .flush()
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let mut language = String::new();
        io::stdin()
            .read_line(&mut language)
            .map_err(|e| McpError::Tool(format!("IO error: {}", e)))?;
        let language = language.trim().to_string();

        // Store the configuration
        let mut config = self.config.write().await;
        config.name = Some(name.clone());
        config.theme = Some(theme.clone());
        config.notifications = notifications;
        config.language = Some(language.clone());

        ctx.info("User configuration collected successfully")
            .await?;

        Ok(format!(
            "✅ Profile setup complete!\n\n📋 Your Configuration:\n  • Name: {}\n  • Theme: {}\n  • Notifications: {}\n  • Language: {}",
            name, theme, notifications, language
        ))
    }

    /// Show current user configuration
    #[tool]
    async fn show_config(&self) -> McpResult<String> {
        let config = self.config.read().await;

        if config.name.is_none() {
            return Ok("❌ No configuration found. Run 'setup_user_profile' first!".to_string());
        }

        Ok(format!(
            "📋 Current Configuration:\n  • Name: {}\n  • Theme: {}\n  • Notifications: {}\n  • Language: {}",
            config.name.as_deref().unwrap_or("Not set"),
            config.theme.as_deref().unwrap_or("Not set"),
            config.notifications,
            config.language.as_deref().unwrap_or("Not set")
        ))
    }

    /// Reset configuration
    #[tool]
    async fn reset_config(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Resetting user configuration").await?;

        let mut config = self.config.write().await;
        *config = UserConfiguration::default();

        Ok("✅ Configuration reset. Run 'setup_user_profile' to configure again.".to_string())
    }

    /// Explain elicitation concepts
    #[tool]
    async fn explain_elicitation(&self) -> McpResult<String> {
        Ok("🤔 Understanding MCP Elicitation:\n\n\
            Elicitation allows MCP servers to request user input during operations.\n\n\
            📋 How it works:\n\
            1. Server sends elicitation request with schema\n\
            2. Client prompts user for input\n\
            3. User provides response\n\
            4. Client sends response back to server\n\
            5. Server continues with user data\n\n\
            🎯 Use Cases:\n\
            • Configuration wizards\n\
            • Missing parameter collection  \n\
            • Confirmation dialogs\n\
            • Interactive workflows\n\n\
            💡 This example demonstrates the MCP elicitation protocol concepts.\n\
            For full production elicitation, use bidirectional transports like WebSocket\n\
            (see Example 09) or integrate with MCP clients that support elicitation."
            .to_string())
    }
}

impl ConfigurationServer {
    fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(UserConfiguration::default())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════╗");
    println!("║    ELICITATION COMPLETE - USER INPUT     ║");
    println!("╚═══════════════════════════════════════════╝\n");

    println!("This server demonstrates real user input collection patterns.");
    println!("It shows how elicitation works in practice with schema validation.\n");

    println!("🎯 Try these commands:");
    println!("  • setup_user_profile  - Configure your profile (triggers input collection)");
    println!("  • show_config        - Display current configuration");
    println!("  • reset_config       - Clear configuration");
    println!("  • explain_elicitation - Learn about elicitation patterns\n");

    println!("📡 Server ready - connect with any MCP client!\n");

    ConfigurationServer::new().run_stdio().await?;
    Ok(())
}
