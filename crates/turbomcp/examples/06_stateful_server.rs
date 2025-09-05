//! # 06: Stateful Server - State Management and Context
//!
//! **Learning Goals (15 minutes):**
//! - Manage persistent state across requests  
//! - Use Context for request-scoped data
//! - Implement thread-safe state operations
//! - Demonstrate filesystem roots configuration
//!
//! **Prerequisites:** Previous tutorials (01-05)
//!
//! **Run with:** `cargo run --example 06_stateful_server`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::{Context, McpResult, mcp_error, server, tool};

/// Application state with user preferences and data
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppState {
    theme: String,
    language: String,
    notifications_enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// User session with preferences and history
#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserSession {
    user_id: String,
    preferences: AppState,
    command_history: Vec<String>,
    last_activity: chrono::DateTime<chrono::Utc>,
}

/// Stateful server managing user sessions and global state
#[derive(Clone)]
struct StatefulServer {
    /// Global application state
    global_state: Arc<RwLock<AppState>>,
    /// Per-user sessions
    user_sessions: Arc<RwLock<HashMap<String, UserSession>>>,
    /// Configuration values
    config: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

#[server(
    name = "stateful-server",
    version = "1.0.0",
    description = "Tutorial 06: Stateful server with context and roots",
    root = "file:///workspace:Workspace Files",
    root = "file:///tmp:Temporary Storage"
)]
impl StatefulServer {
    fn new() -> Self {
        let default_state = AppState {
            theme: "dark".to_string(),
            language: "en".to_string(),
            notifications_enabled: true,
            created_at: chrono::Utc::now(),
        };

        let mut initial_config = HashMap::new();
        initial_config.insert(
            "server_info".to_string(),
            serde_json::json!({
                "message": "Welcome to the stateful TurboMCP server",
                "features": ["state_management", "user_sessions", "context_data", "filesystem_roots"],
                "roots": ["/workspace", "/tmp"]
            })
        );

        Self {
            global_state: Arc::new(RwLock::new(default_state)),
            user_sessions: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(initial_config)),
        }
    }

    #[tool("Get global application state")]
    async fn get_global_state(&self, ctx: Context) -> McpResult<AppState> {
        ctx.info("Retrieving global application state").await?;
        let state = self.global_state.read().await;
        Ok(state.clone())
    }

    #[tool("Update global theme")]
    async fn update_theme(&self, ctx: Context, theme: String) -> McpResult<String> {
        ctx.info(&format!("Updating global theme to: {}", theme))
            .await?;

        // Validate theme
        if !["light", "dark", "auto"].contains(&theme.as_str()) {
            ctx.error("Invalid theme provided").await?;
            return Err(mcp_error!("Theme must be one of: light, dark, auto").into());
        }

        let mut state = self.global_state.write().await;
        let old_theme = state.theme.clone();
        state.theme = theme.clone();

        // Store action in context data
        ctx.set("previous_theme", &old_theme).await?;
        ctx.set("new_theme", &theme).await?;

        Ok(format!("Theme updated from {} to {}", old_theme, theme))
    }

    #[tool("Create or update user session")]
    async fn create_user_session(
        &self,
        ctx: Context,
        user_id: String,
        preferences: Option<AppState>,
    ) -> McpResult<String> {
        ctx.info(&format!("Creating session for user: {}", user_id))
            .await?;

        let session = UserSession {
            user_id: user_id.clone(),
            preferences: preferences.unwrap_or_else(|| {
                // Use global state as default
                futures::executor::block_on(async { self.global_state.read().await.clone() })
            }),
            command_history: Vec::new(),
            last_activity: chrono::Utc::now(),
        };

        let mut sessions = self.user_sessions.write().await;
        let is_new = !sessions.contains_key(&user_id);
        sessions.insert(user_id.clone(), session);

        // Store session info in context
        ctx.set("user_id", &user_id).await?;
        ctx.set("session_created", chrono::Utc::now()).await?;

        if is_new {
            Ok(format!("Created new session for user: {}", user_id))
        } else {
            Ok(format!("Updated existing session for user: {}", user_id))
        }
    }

    #[tool("Get user session info")]
    async fn get_user_session(&self, ctx: Context, user_id: String) -> McpResult<UserSession> {
        ctx.info(&format!("Retrieving session for user: {}", user_id))
            .await?;

        let sessions = self.user_sessions.read().await;
        match sessions.get(&user_id) {
            Some(session) => {
                ctx.set("session_found", true).await?;
                Ok(session.clone())
            }
            None => {
                ctx.warn(&format!("No session found for user: {}", user_id))
                    .await?;
                Err(mcp_error!("User session not found: {}", user_id).into())
            }
        }
    }

    #[tool("Add command to user history")]
    async fn add_to_history(
        &self,
        ctx: Context,
        user_id: String,
        command: String,
    ) -> McpResult<String> {
        ctx.info(&format!(
            "Adding command '{}' to user {} history",
            command, user_id
        ))
        .await?;

        let mut sessions = self.user_sessions.write().await;
        match sessions.get_mut(&user_id) {
            Some(session) => {
                session.command_history.push(command.clone());
                session.last_activity = chrono::Utc::now();

                // Limit history size
                if session.command_history.len() > 100 {
                    session.command_history.remove(0);
                }

                ctx.set("history_length", session.command_history.len())
                    .await?;
                Ok(format!(
                    "Added '{}' to user {} history ({} total)",
                    command,
                    user_id,
                    session.command_history.len()
                ))
            }
            None => {
                ctx.error(&format!("User session not found: {}", user_id))
                    .await?;
                Err(mcp_error!("User session not found: {}", user_id).into())
            }
        }
    }

    #[tool("Get server configuration")]
    async fn get_config(&self, ctx: Context, key: Option<String>) -> McpResult<serde_json::Value> {
        let config = self.config.read().await;

        match key {
            Some(k) => {
                ctx.info(&format!("Retrieving config key: {}", k)).await?;
                match config.get(&k) {
                    Some(value) => Ok(value.clone()),
                    None => Err(mcp_error!("Configuration key not found: {}", k).into()),
                }
            }
            None => {
                ctx.info("Retrieving all configuration").await?;
                Ok(serde_json::to_value(&*config).unwrap())
            }
        }
    }

    #[tool("List active user sessions")]
    async fn list_sessions(&self, ctx: Context) -> McpResult<Vec<String>> {
        let sessions = self.user_sessions.read().await;
        let user_ids: Vec<String> = sessions.keys().cloned().collect();

        ctx.info(&format!("Found {} active sessions", user_ids.len()))
            .await?;
        ctx.set("session_count", user_ids.len()).await?;

        Ok(user_ids)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("🏗️ Starting Tutorial 06: Stateful Server");
    tracing::info!("This server demonstrates:");
    tracing::info!("  - Global state management");
    tracing::info!("  - Per-user sessions");
    tracing::info!("  - Context data storage");
    tracing::info!("  - Filesystem roots configuration");
    tracing::info!("  - Thread-safe state operations");

    let server = StatefulServer::new();

    // The run_stdio method is generated by the #[server] macro
    server.run_stdio().await?;

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run the server:
//    cargo run --example 06_stateful_server
//
//    Test state operations:
//    - Tool: get_global_state {}
//    - Tool: update_theme { "theme": "light" }
//    - Tool: create_user_session { "user_id": "alice" }
//    - Tool: add_to_history { "user_id": "alice", "command": "test command" }
//    - Tool: get_user_session { "user_id": "alice" }
//    - Tool: list_sessions {}
//    - Tool: get_config { "key": "server_info" }

/* 📝 **Key Concepts:**

**State Management:**
- Arc<RwLock<T>> for thread-safe shared state
- Global state vs per-user sessions
- State validation and limits
- Persistence patterns

**Context Usage:**
- Store request-scoped data with ctx.set()
- Retrieve context data with ctx.get()
- Logging with different levels (info, warn, error)
- Request correlation and tracing

**Filesystem Roots:**
- Configure roots directly in #[server] macro
- Multiple roots with descriptive names
- OS-aware path handling
- Foundation for file-based tools

**Thread Safety:**
- RwLock for read-heavy workloads
- Async-aware synchronization
- Deadlock prevention patterns
- Resource cleanup strategies

**User Session Management:**
- Session creation and updates
- Command history tracking
- Activity timestamps
- Resource limits and cleanup

**Best Practices:**
- Validate state changes
- Provide helpful error messages
- Use context for request correlation
- Implement resource limits
- Clean up expired sessions

**Next Steps:**
- Continue to advanced examples
- Explore resources and prompts
- Learn transport configuration
- Implement production patterns
*/
