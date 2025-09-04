//! Real Elicitation Example with TurboMCP Macros
//!
//! This example shows a working MCP server that uses elicitation to request
//! structured user input from the client, using TurboMCP's macro system.
//!
//! Run with:
//! ```bash
//! cargo run --example feature_elicitation_server
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::elicitation_api::{ElicitationResult, boolean, integer, string};
use turbomcp::{Context, McpError, McpResult, elicit, server, tool};

/// Project Setup Wizard - Uses elicitation for configuration
#[derive(Clone)]
struct SetupWizard {
    /// Store project configurations
    #[allow(dead_code)]
    projects: Arc<RwLock<HashMap<String, ProjectConfig>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProjectConfig {
    name: String,
    project_type: String,
    language: String,
    use_database: bool,
    database_type: Option<String>,
    port: u16,
    enable_auth: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[server(
    name = "setup-wizard",
    version = "1.0.0",
    description = "Interactive project setup wizard using elicitation"
)]
impl SetupWizard {
    fn new() -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool("Start a new project setup wizard")]
    async fn new_project(&self, ctx: Context) -> McpResult<String> {
        // Send real elicitation request to gather project information
        let result = elicit("Let's set up your new project!")
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
                "project_type",
                string()
                    .enum_values(vec![
                        "web_app".to_string(),
                        "cli_tool".to_string(),
                        "library".to_string(),
                        "api_service".to_string(),
                    ])
                    .description("Type of project")
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
                "use_database",
                boolean()
                    .description("Will this project use a database?")
                    .build(),
            )
            .field(
                "database_type",
                string()
                    .enum_values(vec![
                        "postgresql".to_string(),
                        "mysql".to_string(),
                        "sqlite".to_string(),
                        "mongodb".to_string(),
                    ])
                    .description("Database type (if using database)")
                    .build(),
            )
            .field(
                "port",
                integer()
                    .range(1024.0, 65535.0)
                    .description("Port number for the service")
                    .build(),
            )
            .field(
                "enable_auth",
                boolean().description("Enable authentication?").build(),
            )
            .require(vec!["name", "project_type", "language", "port"])
            .send(&ctx.request)
            .await?;

        let project_config = match result {
            ElicitationResult::Accept(data) => {
                let use_database = data.get::<bool>("use_database").unwrap_or(false);
                let database_type = if use_database {
                    data.get::<String>("database_type").ok()
                } else {
                    None
                };

                ProjectConfig {
                    name: data.get::<String>("name")?,
                    project_type: data.get::<String>("project_type")?,
                    language: data.get::<String>("language")?,
                    use_database,
                    database_type,
                    port: data.get::<f64>("port")? as u16,
                    enable_auth: data.get::<bool>("enable_auth").unwrap_or(false),
                    created_at: chrono::Utc::now(),
                }
            }
            ElicitationResult::Decline(reason) => {
                return Ok(format!(
                    "Project creation cancelled: {}",
                    reason.unwrap_or_else(|| "User declined".to_string())
                ));
            }
            ElicitationResult::Cancel => {
                return Ok("Project creation cancelled by user.".to_string());
            }
        };

        // Store the configuration
        let project_id = format!("proj_{}", uuid::Uuid::new_v4());
        self.projects
            .write()
            .await
            .insert(project_id.clone(), project_config.clone());

        let db_info = if project_config.use_database {
            project_config.database_type.as_deref().unwrap_or("None")
        } else {
            "Not configured"
        };

        Ok(format!(
            "## Project Created Successfully! 🎉\n\n\
            **ID**: {}\n\
            **Name**: {}\n\
            **Type**: {}\n\
            **Language**: {}\n\
            **Database**: {}\n\
            **Port**: {}\n\
            **Authentication**: {}\n\n\
            *Configuration gathered via elicitation/create*",
            project_id,
            project_config.name,
            project_config.project_type,
            project_config.language,
            db_info,
            project_config.port,
            if project_config.enable_auth {
                "Enabled"
            } else {
                "Disabled"
            }
        ))
    }

    #[tool("Quick setup with defaults")]
    async fn quick_setup(&self, ctx: Context, project_name: String) -> McpResult<String> {
        // Ask for user confirmation using real elicitation
        let result = elicit(format!(
            "Create project '{}' with default settings?",
            project_name
        ))
        .field(
            "confirm",
            boolean()
                .description("Confirm project creation with defaults")
                .build(),
        )
        .require(vec!["confirm"])
        .send(&ctx.request)
        .await?;

        let confirmed = match result {
            ElicitationResult::Accept(data) => data.get::<bool>("confirm").unwrap_or(false),
            _ => false,
        };

        if !confirmed {
            return Ok("Setup cancelled by user.".to_string());
        }

        // Create with defaults
        let config = ProjectConfig {
            name: project_name.clone(),
            project_type: "web_app".to_string(),
            language: "typescript".to_string(),
            use_database: true,
            database_type: Some("postgresql".to_string()),
            port: 3000,
            enable_auth: true,
            created_at: chrono::Utc::now(),
        };

        let project_id = format!("proj_{}", uuid::Uuid::new_v4());
        self.projects
            .write()
            .await
            .insert(project_id.clone(), config);

        Ok(format!(
            "## Quick Setup Complete! ⚡\n\n\
            Project '{}' created with recommended defaults.\n\
            ID: {}\n\n\
            *User confirmed via elicitation*",
            project_name, project_id
        ))
    }

    #[tool("Configure database for existing project")]
    async fn configure_database(&self, ctx: Context, project_id: String) -> McpResult<String> {
        // Use real elicitation to gather database configuration
        let result = elicit("Select database configuration")
            .field(
                "database_type",
                string()
                    .enum_values(vec![
                        "postgresql".to_string(),
                        "mysql".to_string(),
                        "sqlite".to_string(),
                        "mongodb".to_string(),
                        "redis".to_string(),
                    ])
                    .description("Database type")
                    .build(),
            )
            .field(
                "connection_pooling",
                boolean().description("Enable connection pooling?").build(),
            )
            .field(
                "max_connections",
                integer()
                    .range(1.0, 100.0)
                    .description("Maximum connections in pool")
                    .build(),
            )
            .require(vec!["database_type"])
            .send(&ctx.request)
            .await?;

        let db_config = match result {
            ElicitationResult::Accept(data) => data.get::<String>("database_type")?,
            ElicitationResult::Decline(_) => {
                return Ok("Database configuration cancelled.".to_string());
            }
            ElicitationResult::Cancel => return Ok("Database configuration cancelled.".to_string()),
        };

        // Then update the project
        let mut projects = self.projects.write().await;
        let project = projects
            .get_mut(&project_id)
            .ok_or_else(|| McpError::Tool(format!("Project {} not found", project_id)))?;

        let project_name = project.name.clone();
        project.use_database = true;
        project.database_type = Some(db_config.clone());

        Ok(format!(
            "## Database Configured! 🗄️\n\n\
            Project '{}' now uses: {}\n\n\
            *Configuration gathered via elicitation*",
            project_name, db_config
        ))
    }

    #[tool("List all configured projects")]
    async fn list_projects(&self) -> McpResult<String> {
        let projects = self.projects.read().await;

        if projects.is_empty() {
            return Ok("No projects configured yet. Use 'new_project' to start!".to_string());
        }

        let mut result = String::from("## Configured Projects\n\n");
        for (id, config) in projects.iter() {
            result.push_str(&format!(
                "**{}** ({})\n  - Type: {}\n  - Language: {}\n  - Created: {}\n\n",
                config.name,
                id,
                config.project_type,
                config.language,
                config.created_at.format("%Y-%m-%d %H:%M:%S")
            ));
        }

        Ok(result)
    }

    #[tool("Get project details")]
    async fn get_project(&self, project_id: String) -> McpResult<String> {
        let projects = self.projects.read().await;

        let project = projects
            .get(&project_id)
            .ok_or_else(|| McpError::Tool(format!("Project {} not found", project_id)))?;

        let config_json = serde_json::to_string_pretty(project)
            .map_err(|e| McpError::Tool(format!("Failed to serialize: {}", e)))?;

        Ok(format!(
            "## Project Configuration\n\n```json\n{}\n```",
            config_json
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧙 Setup Wizard Server (Real Elicitation with Macros)");
    println!("====================================================\n");

    // Create server
    let _wizard = SetupWizard::new();

    println!("This example demonstrates real elicitation APIs.");
    println!("In a connected MCP environment, these would trigger");
    println!("client UI for user input. Since we're running standalone,");
    println!("the elicitation requests would need a connected client.\n");

    // Show available tools
    println!("📋 Available Tools:");
    println!("  • new_project - Interactive project setup wizard");
    println!("  • quick_setup - Quick project creation with confirmation");
    println!("  • configure_database - Database configuration wizard");
    println!("  • list_projects - List all configured projects");
    println!("  • get_project - Get project details by ID");

    println!("\n✅ Real Elicitation Server Ready!");
    println!("\n📋 How This Real Implementation Works:");
    println!("1. Tools use the `elicit()` builder to create requests");
    println!("2. Schema builders (string(), boolean(), integer()) define fields");
    println!("3. Context parameter enables elicitation via ctx.request");
    println!("4. ElicitationResult enum handles Accept/Decline/Cancel");
    println!("5. Data extraction uses type-safe getters");

    println!("\n🔗 Integration:");
    println!("To use this server with a real MCP client:");
    println!("1. Connect via stdio, TCP, or WebSocket transport");
    println!("2. Client must implement ElicitationHandler");
    println!("3. Client presents UI based on schema");
    println!("4. User responses flow back through ElicitationResult");

    Ok(())
}
