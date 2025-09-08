//! Example 16: Completion Demo - Server
//!
//! Demonstrates the #[completion] attribute macro for intelligent autocompletion.
//!
//! Run the server:
//! ```bash
//! cargo run --example 16_completion_demo_server
//! ```
//!
//! In another terminal, run the client:
//! ```bash
//! cargo run --example 16_completion_demo_client
//! ```

use std::collections::HashMap;
use turbomcp::{Context, McpResult, completion, server, tool};

#[derive(Clone)]
struct CompletionServer {
    // Simulated file system for path completion
    files: HashMap<String, Vec<String>>,
    // Available commands
    commands: Vec<String>,
    // User database
    users: Vec<String>,
}

impl CompletionServer {
    fn new() -> Self {
        let mut files = HashMap::new();
        files.insert(
            "/".to_string(),
            vec![
                "home".to_string(),
                "usr".to_string(),
                "var".to_string(),
                "etc".to_string(),
                "tmp".to_string(),
            ],
        );
        files.insert(
            "/home".to_string(),
            vec![
                "alice".to_string(),
                "bob".to_string(),
                "charlie".to_string(),
            ],
        );
        files.insert(
            "/home/alice".to_string(),
            vec![
                "documents".to_string(),
                "downloads".to_string(),
                "projects".to_string(),
                "pictures".to_string(),
            ],
        );
        files.insert(
            "/home/alice/projects".to_string(),
            vec![
                "turbomcp".to_string(),
                "rust-app".to_string(),
                "website".to_string(),
            ],
        );
        files.insert(
            "/usr".to_string(),
            vec![
                "bin".to_string(),
                "lib".to_string(),
                "local".to_string(),
                "share".to_string(),
            ],
        );
        files.insert(
            "/usr/bin".to_string(),
            vec![
                "cargo".to_string(),
                "rustc".to_string(),
                "git".to_string(),
                "vim".to_string(),
                "code".to_string(),
            ],
        );

        let commands = vec![
            "deploy".to_string(),
            "delete".to_string(),
            "describe".to_string(),
            "download".to_string(),
            "debug".to_string(),
            "list".to_string(),
            "login".to_string(),
            "logout".to_string(),
            "create".to_string(),
            "connect".to_string(),
            "configure".to_string(),
            "copy".to_string(),
            "cancel".to_string(),
        ];

        let users = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
            "david".to_string(),
            "eve".to_string(),
            "frank".to_string(),
        ];

        Self {
            files,
            commands,
            users,
        }
    }
}

#[server(
    name = "completion-demo",
    version = "1.0.5",
    description = "Demonstrates completion attribute macro"
)]
impl CompletionServer {
    /// Complete file paths
    #[completion("Complete file paths")]
    async fn complete_path(&self, partial: String) -> McpResult<Vec<String>> {
        let mut completions = Vec::new();

        // Handle root path
        if partial.is_empty() || partial == "/" {
            if let Some(entries) = self.files.get("/") {
                for entry in entries {
                    completions.push(format!("/{}", entry));
                }
            }
            return Ok(completions);
        }

        // Extract directory and partial filename
        let (dir, partial_name) = if partial.contains('/') {
            let last_slash = partial.rfind('/').unwrap();
            let dir = &partial[..=last_slash];
            let name = &partial[last_slash + 1..];
            (dir.trim_end_matches('/'), name)
        } else {
            ("/", partial.as_str())
        };

        // Find completions
        if let Some(entries) = self.files.get(dir) {
            for entry in entries {
                if entry.starts_with(partial_name) {
                    if dir == "/" {
                        completions.push(format!("/{}", entry));
                    } else {
                        completions.push(format!("{}/{}", dir, entry));
                    }
                }
            }
        }

        // Also check subdirectories
        for path in self.files.keys() {
            if path.starts_with(&partial) && path != &partial {
                completions.push(path.clone());
            }
        }

        completions.sort();
        completions.dedup();
        Ok(completions)
    }

    /// Complete command names
    #[completion("Complete command names")]
    async fn complete_command(&self, partial: String) -> McpResult<Vec<String>> {
        let partial_lower = partial.to_lowercase();
        let mut completions: Vec<String> = self
            .commands
            .iter()
            .filter(|cmd| cmd.to_lowercase().starts_with(&partial_lower))
            .cloned()
            .collect();

        completions.sort();
        Ok(completions)
    }

    /// Complete usernames
    #[completion("Complete usernames")]
    async fn complete_user(&self, partial: String) -> McpResult<Vec<String>> {
        let partial_lower = partial.to_lowercase();
        let mut completions: Vec<String> = self
            .users
            .iter()
            .filter(|user| user.to_lowercase().starts_with(&partial_lower))
            .map(|user| format!("@{}", user))
            .collect();

        completions.sort();
        Ok(completions)
    }

    /// Smart completion that detects context
    #[completion("Smart context-aware completion")]
    async fn smart_complete(&self, input: String) -> McpResult<Vec<String>> {
        // Parse the input to determine context
        let parts: Vec<&str> = input.split_whitespace().collect();

        if parts.is_empty() {
            // Start with commands
            return self.complete_command("".to_string()).await;
        }

        // Check if we're completing a path (contains /)
        if let Some(last) = parts.last() {
            if last.contains('/') {
                return self.complete_path(last.to_string()).await;
            }

            // Check if we're completing a user (starts with @)
            if last.starts_with('@') {
                return self
                    .complete_user(last.trim_start_matches('@').to_string())
                    .await;
            }
        }

        // If first word is incomplete, complete commands
        if parts.len() == 1 {
            return self.complete_command(parts[0].to_string()).await;
        }

        // Otherwise, provide contextual suggestions
        let mut suggestions = Vec::new();

        // Suggest paths
        if let Ok(paths) = self.complete_path("".to_string()).await {
            suggestions.extend(paths.into_iter().take(3));
        }

        // Suggest users
        suggestions.push("@alice".to_string());
        suggestions.push("@bob".to_string());

        Ok(suggestions)
    }

    /// Demo tool that uses completion
    #[tool("Execute a command with path and user")]
    async fn execute(
        &self,
        ctx: Context,
        command: String,
        path: String,
        user: String,
    ) -> McpResult<String> {
        ctx.info(&format!("Executing: {} {} {}", command, path, user))
            .await?;

        // Validate command
        if !self.commands.contains(&command) {
            return Err(turbomcp::McpError::Tool(format!(
                "Unknown command: {}",
                command
            )));
        }

        // Validate path (simplified)
        let valid_path = self.files.keys().any(|p| path.starts_with(p));
        if !valid_path {
            return Err(turbomcp::McpError::Tool(format!("Invalid path: {}", path)));
        }

        // Validate user
        let user_clean = user.trim_start_matches('@');
        if !self.users.contains(&user_clean.to_string()) {
            return Err(turbomcp::McpError::Tool(format!("Unknown user: {}", user)));
        }

        Ok(format!(
            "✅ Executed '{}' on path '{}' as user '{}'",
            command, path, user
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging if needed
    // env_logger::init();

    println!("🚀 Completion Demo Server");
    println!("=========================");
    println!("This server provides intelligent autocompletion for:");
    println!("- File paths (e.g., /home/alice/projects)");
    println!("- Commands (e.g., deploy, create, configure)");
    println!("- Usernames (e.g., @alice, @bob)");
    println!();
    println!("Completion handlers:");
    println!("- complete_path: Complete file system paths");
    println!("- complete_command: Complete command names");
    println!("- complete_user: Complete usernames");
    println!("- smart_complete: Context-aware completion");
    println!();
    println!("Run the client to see autocompletion in action:");
    println!("cargo run --example 16_completion_demo_client");
    println!();
    println!("Server running on stdio...");

    let server = CompletionServer::new();
    server.run_stdio().await?;

    Ok(())
}
