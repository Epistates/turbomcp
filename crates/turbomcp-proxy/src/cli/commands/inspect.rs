//! Inspect command implementation
//!
//! Discovers MCP server capabilities by performing protocol introspection.

use clap::Args;
use std::fs::File;
use std::io::{self, BufWriter};
use tracing::info;

use crate::cli::args::{BackendArgs, OutputArgs};
use crate::cli::output::{OutputFormat, get_formatter};
use crate::error::{ProxyError, ProxyResult};
use crate::introspection::{McpIntrospector, StdioBackend};

/// Inspect an MCP server to discover its capabilities
///
/// This command connects to an MCP server, performs the initialization handshake,
/// and lists all available tools, resources, and prompts.
///
/// # Examples
///
/// Inspect a Python MCP server:
///   turbomcp-proxy inspect --backend stdio --cmd python --args server.py
///
/// Inspect with JSON output:
///   turbomcp-proxy inspect --backend stdio --cmd python --args server.py -f json
///
/// Save to file:
///   turbomcp-proxy inspect --backend stdio --cmd node --args dist/server.js -o spec.json
#[derive(Debug, Args)]
pub struct InspectCommand {
    /// Backend configuration
    #[command(flatten)]
    pub backend: BackendArgs,

    /// Output configuration
    #[command(flatten)]
    pub output: OutputArgs,

    /// Client name to send during initialization
    #[arg(long, default_value = "turbomcp-proxy")]
    pub client_name: String,

    /// Client version to send during initialization
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub client_version: String,
}

impl InspectCommand {
    /// Execute the inspect command
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if backend validation fails or introspection fails.
    pub async fn execute(self, format: OutputFormat) -> ProxyResult<()> {
        // Validate backend arguments
        self.backend.validate().map_err(ProxyError::configuration)?;

        info!(
            backend = ?self.backend.backend_type(),
            "Starting MCP server introspection"
        );

        // Create backend based on type
        let mut backend = self.create_backend().await?;

        // Create introspector
        let introspector = McpIntrospector::with_client_info(
            self.client_name.clone(),
            self.client_version.clone(),
        );

        // Perform introspection
        let spec = introspector.introspect(&mut *backend).await?;

        // Write output
        self.write_output(&spec, format)?;

        Ok(())
    }

    /// Create the appropriate backend based on configuration
    async fn create_backend(&self) -> ProxyResult<Box<dyn crate::introspection::McpBackend>> {
        use crate::cli::args::BackendType;

        match self.backend.backend_type() {
            Some(BackendType::Stdio) => {
                let cmd = self.backend.cmd.as_ref().ok_or_else(|| {
                    ProxyError::configuration("Command not specified".to_string())
                })?;

                let backend: StdioBackend = if let Some(ref working_dir) = self.backend.working_dir
                {
                    StdioBackend::with_working_dir(
                        cmd.clone(),
                        self.backend.args.clone(),
                        working_dir.to_string_lossy().to_string(),
                    )
                    .await?
                } else {
                    StdioBackend::new(cmd.clone(), self.backend.args.clone()).await?
                };

                Ok(Box::new(backend))
            }
            Some(BackendType::Http) => {
                // TODO: Implement HTTP backend in future phase
                Err(ProxyError::configuration(
                    "HTTP backend not yet implemented".to_string(),
                ))
            }
            Some(BackendType::Websocket) => {
                // TODO: Implement WebSocket backend in future phase
                Err(ProxyError::configuration(
                    "WebSocket backend not yet implemented".to_string(),
                ))
            }
            None => Err(ProxyError::configuration(
                "No backend specified".to_string(),
            )),
        }
    }

    /// Write the output to the appropriate destination
    fn write_output(
        &self,
        spec: &crate::introspection::ServerSpec,
        format: OutputFormat,
    ) -> ProxyResult<()> {
        let formatter = get_formatter(format);

        if let Some(ref output_path) = self.output.output {
            // Write to file
            let file: File = if self.output.append {
                File::options()
                    .create(true)
                    .append(true)
                    .open(output_path)?
            } else {
                File::create(output_path)?
            };

            let mut writer = BufWriter::new(file);
            formatter.write_spec(spec, &mut writer)?;
            formatter.write_success(
                &format!(
                    "Introspection complete. Output written to: {}",
                    output_path.display()
                ),
                &mut io::stdout(),
            )?;
        } else {
            // Write to stdout
            let mut writer = BufWriter::new(io::stdout());
            formatter.write_spec(spec, &mut writer)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_validation() {
        let cmd = InspectCommand {
            backend: BackendArgs {
                backend: Some(crate::cli::args::BackendType::Stdio),
                cmd: None,
                args: vec![],
                working_dir: None,
                http: None,
                websocket: None,
            },
            output: OutputArgs {
                output: None,
                append: false,
            },
            client_name: "test".to_string(),
            client_version: "1.0.0".to_string(),
        };

        assert!(cmd.backend.validate().is_err());
    }

    #[tokio::test]
    async fn test_stdio_backend_creation() {
        let cmd = InspectCommand {
            backend: BackendArgs {
                backend: Some(crate::cli::args::BackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec!["-c".to_string(), "print('test')".to_string()],
                working_dir: None,
                http: None,
                websocket: None,
            },
            output: OutputArgs {
                output: None,
                append: false,
            },
            client_name: "test".to_string(),
            client_version: "1.0.0".to_string(),
        };

        // Should successfully create backend (python command is in allowlist)
        let backend = cmd.create_backend().await;
        assert!(backend.is_ok());
    }
}
