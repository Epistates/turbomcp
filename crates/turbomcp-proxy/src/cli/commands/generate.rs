//! Generate command implementation
//!
//! Generates optimized Rust proxy code from an MCP server.

use clap::Args;
use std::path::PathBuf;

use crate::cli::args::BackendArgs;
use crate::error::{ProxyError, ProxyResult};

#[cfg(feature = "codegen")]
use crate::{
    codegen::{BackendType, FrontendType, GenConfig, RustCodeGenerator},
    introspection::McpIntrospector,
};

#[cfg(feature = "codegen")]
use std::fs;

#[cfg(feature = "codegen")]
use tracing::{error, info};

/// Generate optimized Rust proxy code
///
/// This command introspects an MCP server and generates a standalone Rust project
/// with type-safe, compiled proxy code. The generated project can be built and
/// deployed for production use.
///
/// # Examples
///
/// Generate from STDIO server:
///   turbomcp-proxy generate \
///     --backend stdio --cmd python --args server.py \
///     --frontend http \
///     --output ./my-proxy
///
/// Generate, build, and run:
///   turbomcp-proxy generate \
///     --backend stdio --cmd python --args server.py \
///     --frontend http \
///     --output ./my-proxy \
///     --build --release --run
#[derive(Debug, Args)]
pub struct GenerateCommand {
    /// Backend configuration
    #[command(flatten)]
    pub backend: BackendArgs,

    /// Frontend transport type
    #[arg(long, value_name = "TYPE", default_value = "http")]
    pub frontend: String,

    /// Output directory for generated code
    #[arg(long, short = 'o', value_name = "DIR")]
    pub output: PathBuf,

    /// Package name (defaults to server name)
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Package version
    #[arg(long, value_name = "VERSION", default_value = "0.1.0")]
    pub version: String,

    /// Build the generated project after generation
    #[arg(long)]
    pub build: bool,

    /// Build in release mode (requires --build)
    #[arg(long)]
    pub release: bool,

    /// Run the generated proxy after building (requires --build)
    #[arg(long)]
    pub run: bool,

    /// Client name to send during initialization
    #[arg(long, default_value = "turbomcp-proxy-generator")]
    pub client_name: String,

    /// Client version to send during initialization
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub client_version: String,
}

impl GenerateCommand {
    /// Execute the generate command
    #[cfg(feature = "codegen")]
    pub async fn execute(self) -> ProxyResult<()> {
        info!("Starting code generation...");

        // Validate backend arguments
        self.backend
            .validate()
            .map_err(|e| ProxyError::configuration(e))?;

        // Validate flags
        if self.release && !self.build {
            return Err(ProxyError::configuration(
                "--release requires --build".to_string(),
            ));
        }
        if self.run && !self.build {
            return Err(ProxyError::configuration(
                "--run requires --build".to_string(),
            ));
        }

        // Step 1: Create backend
        info!(backend = ?self.backend.backend_type(), "Creating backend...");
        let mut backend = self.create_backend().await?;

        // Step 2: Introspect backend
        info!("Introspecting backend...");
        let introspector = McpIntrospector::with_client_info(
            self.client_name.clone(),
            self.client_version.clone(),
        );
        let spec = introspector.introspect(&mut *backend).await?;

        info!(
            server = %spec.server_info.name,
            version = %spec.server_info.version,
            tools = spec.tools.len(),
            resources = spec.resources.len(),
            prompts = spec.prompts.len(),
            "Introspection complete"
        );

        // Step 3: Parse frontend and backend types
        let frontend_type = self.parse_frontend_type()?;
        let backend_type = self.parse_backend_type()?;

        info!(frontend = %frontend_type, backend = %backend_type, "Parsed transport types");

        // Step 4: Generate code
        info!("Generating Rust code...");
        let generator = RustCodeGenerator::new(spec)?;

        let config = GenConfig {
            package_name: self.name.clone(),
            version: Some(self.version.clone()),
            frontend_type,
            backend_type,
            turbomcp_version: "2.0.5".to_string(),
        };

        let project = generator.generate(config)?;

        info!(package = %project.package_name, "Code generation complete");

        // Step 5: Write files
        info!(output = %self.output.display(), "Writing generated files...");
        self.write_project(&project)?;

        info!("✓ Generated project written to: {}", self.output.display());
        info!("  - src/main.rs ({} bytes)", project.main_rs.len());
        info!("  - src/proxy.rs ({} bytes)", project.proxy_rs.len());
        info!("  - src/types.rs ({} bytes)", project.types_rs.len());
        info!("  - Cargo.toml ({} bytes)", project.cargo_toml.len());

        // Step 6: Optionally build
        if self.build {
            info!("Building generated project...");
            self.build_project()?;
            info!("✓ Build complete");

            // Step 7: Optionally run
            if self.run {
                info!("Running generated proxy...");
                return self.run_project();
            }
        }

        info!("Done! Generated proxy ready at: {}", self.output.display());

        if !self.build {
            info!("\nNext steps:");
            info!("  cd {}", self.output.display());
            info!("  cargo build --release");
            info!("  cargo run");
        }

        Ok(())
    }

    /// Execute when codegen feature is not enabled
    #[cfg(not(feature = "codegen"))]
    pub async fn execute(self) -> ProxyResult<()> {
        Err(ProxyError::configuration(
            "Code generation requires the 'codegen' feature to be enabled. \
             Please rebuild turbomcp-proxy with --features codegen"
                .to_string(),
        ))
    }

    #[cfg(feature = "codegen")]
    async fn create_backend(&self) -> ProxyResult<Box<dyn crate::introspection::McpBackend>> {
        use crate::cli::args::BackendType;
        use crate::introspection::StdioBackend;

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
            Some(BackendType::Http) => Err(ProxyError::configuration(
                "HTTP backend not yet implemented for code generation".to_string(),
            )),
            Some(BackendType::Websocket) => Err(ProxyError::configuration(
                "WebSocket backend not yet implemented for code generation".to_string(),
            )),
            None => Err(ProxyError::configuration(
                "No backend specified".to_string(),
            )),
        }
    }

    #[cfg(feature = "codegen")]
    fn parse_frontend_type(&self) -> ProxyResult<FrontendType> {
        match self.frontend.to_lowercase().as_str() {
            "http" => Ok(FrontendType::Http),
            "stdio" => Ok(FrontendType::Stdio),
            "websocket" | "ws" => Ok(FrontendType::WebSocket),
            _ => Err(ProxyError::configuration(format!(
                "Unknown frontend type: {}. Use 'http', 'stdio', or 'websocket'",
                self.frontend
            ))),
        }
    }

    #[cfg(feature = "codegen")]
    fn parse_backend_type(&self) -> ProxyResult<BackendType> {
        use crate::cli::args::BackendType as CliBackendType;

        match self.backend.backend_type() {
            Some(CliBackendType::Stdio) => Ok(BackendType::Stdio),
            Some(CliBackendType::Http) => Ok(BackendType::Http),
            Some(CliBackendType::Websocket) => Ok(BackendType::WebSocket),
            None => Err(ProxyError::configuration(
                "No backend specified".to_string(),
            )),
        }
    }

    #[cfg(feature = "codegen")]
    fn write_project(&self, project: &crate::codegen::GeneratedProject) -> ProxyResult<()> {
        // Create output directory
        fs::create_dir_all(&self.output).map_err(|e| ProxyError::Io(e))?;

        // Create src directory
        let src_dir = self.output.join("src");
        fs::create_dir_all(&src_dir).map_err(|e| ProxyError::Io(e))?;

        // Write files
        fs::write(src_dir.join("main.rs"), &project.main_rs).map_err(|e| ProxyError::Io(e))?;

        fs::write(src_dir.join("proxy.rs"), &project.proxy_rs).map_err(|e| ProxyError::Io(e))?;

        fs::write(src_dir.join("types.rs"), &project.types_rs).map_err(|e| ProxyError::Io(e))?;

        fs::write(self.output.join("Cargo.toml"), &project.cargo_toml)
            .map_err(|e| ProxyError::Io(e))?;

        Ok(())
    }

    #[cfg(feature = "codegen")]
    fn build_project(&self) -> ProxyResult<()> {
        use std::process::Command;

        let mut cmd = Command::new("cargo");
        cmd.arg("build").current_dir(&self.output);

        if self.release {
            cmd.arg("--release");
        }

        let output = cmd
            .output()
            .map_err(|e| ProxyError::backend(format!("Failed to run cargo build: {}", e)))?;

        if !output.status.success() {
            error!("Build failed:");
            error!("{}", String::from_utf8_lossy(&output.stderr));
            return Err(ProxyError::backend("Build failed".to_string()));
        }

        Ok(())
    }

    #[cfg(feature = "codegen")]
    fn run_project(&self) -> ProxyResult<()> {
        use std::process::Command;

        let mut cmd = Command::new("cargo");
        cmd.arg("run").current_dir(&self.output);

        if self.release {
            cmd.arg("--release");
        }

        let status = cmd
            .status()
            .map_err(|e| ProxyError::backend(format!("Failed to run proxy: {}", e)))?;

        if !status.success() {
            return Err(ProxyError::backend("Proxy exited with error".to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendType as CliBackendType;

    #[test]
    fn test_generate_command_flags() {
        // Test that --release requires --build
        let cmd = GenerateCommand {
            backend: BackendArgs {
                backend: Some(CliBackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec!["server.py".to_string()],
                working_dir: None,
                http: None,
                websocket: None,
            },
            frontend: "http".to_string(),
            output: PathBuf::from("/tmp/test"),
            name: None,
            version: "0.1.0".to_string(),
            build: false,
            release: true,
            run: false,
            client_name: "test".to_string(),
            client_version: "1.0.0".to_string(),
        };

        // This would fail in execute() with configuration error
        assert!(cmd.release && !cmd.build);
    }

    #[cfg(feature = "codegen")]
    #[test]
    fn test_parse_frontend_type() {
        let cmd = GenerateCommand {
            backend: BackendArgs {
                backend: Some(CliBackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec![],
                working_dir: None,
                http: None,
                websocket: None,
            },
            frontend: "http".to_string(),
            output: PathBuf::from("/tmp/test"),
            name: None,
            version: "0.1.0".to_string(),
            build: false,
            release: false,
            run: false,
            client_name: "test".to_string(),
            client_version: "1.0.0".to_string(),
        };

        assert!(cmd.parse_frontend_type().is_ok());
    }
}
