//! Install MCP server to Claude Desktop or Cursor.
//!
//! This module implements the `turbomcp install` command which:
//! - Locates the application's MCP configuration file
//! - Adds or updates the server entry
//! - Validates the server binary exists and is executable
//!
//! # Usage
//!
//! ```bash
//! # Install to Claude Desktop
//! turbomcp install claude-desktop ./target/release/my-server
//!
//! # Install to Cursor
//! turbomcp install cursor ./my-server --name "My MCP Server"
//!
//! # With environment variables
//! turbomcp install claude-desktop ./my-server -e API_KEY=xxx -e DEBUG=true
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cli::{InstallArgs, InstallTarget};

/// MCP server configuration in claude_desktop_config.json format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpServerConfig {
    command: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    args: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    env: HashMap<String, String>,
}

/// Root configuration structure for Claude Desktop.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: HashMap<String, McpServerConfig>,
    #[serde(flatten)]
    other: HashMap<String, serde_json::Value>,
}

/// Execute the install command.
pub fn execute(args: &InstallArgs) -> Result<()> {
    // Validate server path exists
    let server_path = args.server_path.canonicalize().with_context(|| {
        format!(
            "Server binary not found at '{}'",
            args.server_path.display()
        )
    })?;

    // Check if it's executable
    if !is_executable(&server_path) {
        bail!(
            "Server binary '{}' is not executable. \
             Make sure it's compiled and has execute permissions.",
            server_path.display()
        );
    }

    // Get config file path for target
    let config_path = get_config_path(&args.target)?;

    // Derive server name
    let server_name = args.name.clone().unwrap_or_else(|| {
        server_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mcp-server")
            .to_string()
    });

    // Load or create config
    let mut config = load_config(&config_path)?;

    // Check if server already exists
    if config.mcp_servers.contains_key(&server_name) && !args.force {
        bail!(
            "Server '{}' already exists in config. Use --force to overwrite.",
            server_name
        );
    }

    // Parse environment variables
    let env: HashMap<String, String> = args
        .env
        .iter()
        .filter_map(|e| {
            let parts: Vec<&str> = e.splitn(2, '=').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                eprintln!(
                    "Warning: Ignoring invalid env var '{}' (expected KEY=VALUE)",
                    e
                );
                None
            }
        })
        .collect();

    // Create server config
    let server_config = McpServerConfig {
        command: server_path.to_string_lossy().to_string(),
        args: args.args.clone(),
        env,
    };

    // Add to config
    config
        .mcp_servers
        .insert(server_name.clone(), server_config);

    // Save config
    save_config(&config_path, &config)?;

    println!(
        "Successfully installed MCP server '{}' to {:?}",
        server_name, args.target
    );
    println!();
    println!("Configuration file: {}", config_path.display());
    println!();
    println!(
        "Restart {} to load the new server.",
        target_display_name(&args.target)
    );

    Ok(())
}

/// Get the configuration file path for a target application.
fn get_config_path(target: &InstallTarget) -> Result<PathBuf> {
    match target {
        InstallTarget::ClaudeDesktop => get_claude_desktop_config_path(),
        InstallTarget::Cursor => get_cursor_config_path(),
    }
}

/// Get Claude Desktop config path.
fn get_claude_desktop_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = dirs::home_dir()
            .context("Could not find home directory")?
            .join("Library/Application Support/Claude/claude_desktop_config.json");
        Ok(path)
    }

    #[cfg(target_os = "windows")]
    {
        let path = dirs::data_dir()
            .context("Could not find AppData directory")?
            .join("Claude/claude_desktop_config.json");
        Ok(path)
    }

    #[cfg(target_os = "linux")]
    {
        let path = dirs::config_dir()
            .context("Could not find config directory")?
            .join("Claude/claude_desktop_config.json");
        Ok(path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        bail!("Claude Desktop config path not known for this platform")
    }
}

/// Get Cursor config path.
fn get_cursor_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = dirs::home_dir()
            .context("Could not find home directory")?
            .join("Library/Application Support/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json");
        Ok(path)
    }

    #[cfg(target_os = "windows")]
    {
        let path = dirs::data_dir()
            .context("Could not find AppData directory")?
            .join(
                "Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
            );
        Ok(path)
    }

    #[cfg(target_os = "linux")]
    {
        let path = dirs::config_dir()
            .context("Could not find config directory")?
            .join(
                "Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
            );
        Ok(path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        bail!("Cursor config path not known for this platform")
    }
}

/// Load configuration from file, or return default if file doesn't exist.
fn load_config(path: &Path) -> Result<ClaudeDesktopConfig> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    } else {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        Ok(ClaudeDesktopConfig::default())
    }
}

/// Save configuration to file.
fn save_config(path: &Path, config: &ClaudeDesktopConfig) -> Result<()> {
    let contents = serde_json::to_string_pretty(config).context("Failed to serialize config")?;

    fs::write(path, contents)
        .with_context(|| format!("Failed to write config file: {}", path.display()))
}

/// Get display name for a target.
fn target_display_name(target: &InstallTarget) -> &'static str {
    match target {
        InstallTarget::ClaudeDesktop => "Claude Desktop",
        InstallTarget::Cursor => "Cursor",
    }
}

/// Check if a path is an executable file.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// List installed MCP servers (for info display).
pub fn list_installed(target: &InstallTarget) -> Result<Vec<String>> {
    let config_path = get_config_path(target)?;
    let config = load_config(&config_path)?;
    Ok(config.mcp_servers.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_missing_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let config = load_config(&path).unwrap();
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_load_save_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut config = ClaudeDesktopConfig::default();
        config.mcp_servers.insert(
            "test-server".to_string(),
            McpServerConfig {
                command: "/usr/bin/test".to_string(),
                args: vec!["--flag".to_string()],
                env: HashMap::new(),
            },
        );

        save_config(&path, &config).unwrap();

        let loaded = load_config(&path).unwrap();
        assert!(loaded.mcp_servers.contains_key("test-server"));
        assert_eq!(loaded.mcp_servers["test-server"].command, "/usr/bin/test");
    }

    #[test]
    fn test_parse_env_vars() {
        let env_strings = [
            "KEY1=value1".to_string(),
            "KEY2=value with spaces".to_string(),
            "KEY3=value=with=equals".to_string(),
        ];

        let env: HashMap<String, String> = env_strings
            .iter()
            .filter_map(|e| {
                let parts: Vec<&str> = e.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(env.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(env.get("KEY2"), Some(&"value with spaces".to_string()));
        assert_eq!(env.get("KEY3"), Some(&"value=with=equals".to_string()));
    }
}
