//! # 08: Elicitation Server - MCP Server with User Input
//!
//! **Learning Goals (10 minutes):**
//! - Real MCP server that demonstrates elicitation patterns
//! - Understand elicitation schema and user input handling
//! - See complete client/server interaction
//!
//! **What this example demonstrates:**
//! - MCP server with elicitation tools via STDIO transport
//! - Elicitation schema according to MCP 2025-06-18 spec
//! - User configuration management with validation
//!
//! **Usage (Terminal 1 - Start Server):**
//! ```bash
//! cargo run --example 08_elicitation_server
//! ```
//!
//! **Usage (Terminal 2 - Test Client):**
//! ```bash
//! cargo run --example 08_elicitation_client
//! ```
//!
//! **Or test manually with turbomcp-cli:**
//! ```bash
//! echo '{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0.0"}}}' | cargo run --example 08_elicitation_server
//! ```

use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

/// Elicitation server that demonstrates real MCP elicitation patterns
#[derive(Clone)]
struct ElicitationServer {
    /// User configuration state
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
    name = "elicitation-server",
    version = "1.0.0",
    description = "MCP server demonstrating real elicitation over HTTP transport"
)]
impl ElicitationServer {
    /// Setup user profile - demonstrates MCP elicitation schema
    #[tool("Configure user profile using MCP elicitation")]
    async fn setup_user_profile(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Preparing user profile elicitation").await?;

        // Create MCP 2025-06-18 compliant elicitation schema
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

        // In production MCP, this would trigger elicitation/create request
        // The server would send this schema to the client via elicitation protocol
        ctx.info("Elicitation schema ready for client").await?;

        // Store demo configuration (simulate successful elicitation response)
        let mut config = self.config.write().await;
        config.name = Some("Demo User".to_string());
        config.theme = Some("dark".to_string());
        config.notifications = true;
        config.language = Some("en".to_string());

        Ok(format!(
            "✅ User profile elicitation ready!\n\n📋 Schema:\n{}\n\n💡 In production MCP:\n1. Server sends elicitation/create request with this schema\n2. Client prompts user with form based on schema\n3. User fills out form (name, theme, notifications, language)\n4. Client sends elicitation response back to server\n5. Server processes user input and continues",
            serde_json::to_string_pretty(&elicitation_schema).unwrap()
        ))
    }

    /// Show current configuration
    #[tool("Display current user configuration")]
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
    #[tool("Reset user configuration")]
    async fn reset_config(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Resetting user configuration").await?;

        let mut config = self.config.write().await;
        *config = UserConfiguration::default();

        Ok("✅ Configuration reset. Run 'setup_user_profile' to configure again.".to_string())
    }

    /// Explain MCP elicitation protocol
    #[tool("Learn about MCP elicitation protocol")]
    async fn explain_elicitation(&self) -> McpResult<String> {
        Ok("🚀 MCP 2025-06-18 Elicitation Protocol\n\n\
            Real MCP elicitation flow:\n\n\
            📡 1. Server→Client: elicitation/create request\n\
            📋    • Message: \"Please configure your profile\"\n\
            📋    • Schema: Flat object with primitive properties\n\
            📋    • Required fields: name, theme\n\n\
            👤 2. Client prompts user with form UI\n\
            📝    • Text input for name\n\
            📝    • Dropdown for theme (dark/light/auto)\n\
            📝    • Checkbox for notifications\n\
            📝    • Dropdown for language\n\n\
            📤 3. Client→Server: elicitation response\n\
            📋    • Action: \"accept\" | \"decline\" | \"cancel\"\n\
            📋    • Content: User-provided data (if accepted)\n\n\
            🔄 4. Server continues processing with user input\n\n\
            🎯 This HTTP server demonstrates the schema structure.\n\
            For full bidirectional elicitation, use WebSocket transport\n\
            or integrate with MCP clients that support elicitation."
            .to_string())
    }
}

impl ElicitationServer {
    fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(UserConfiguration::default())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // STDIO transport must be completely silent per MCP specification
    // stdout is reserved exclusively for JSON-RPC messages

    let server = ElicitationServer::new();

    // MCP STDIO server - ready for Claude Code and other MCP clients
    server.run_stdio().await?;

    Ok(())
}
