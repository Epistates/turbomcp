//! Shared CLI argument types
//!
//! This module defines reusable argument types that are shared across
//! multiple commands, following the DRY principle.

use clap::{Args, ValueEnum};
use std::path::PathBuf;

/// Backend configuration for connecting to MCP servers
#[derive(Debug, Clone, Args)]
pub struct BackendArgs {
    /// STDIO backend - spawn a subprocess
    #[arg(long, value_name = "BACKEND", group = "backend-type")]
    pub backend: Option<BackendType>,

    /// Command to execute (for STDIO backend)
    #[arg(long, value_name = "COMMAND", requires = "backend")]
    pub cmd: Option<String>,

    /// Command arguments (for STDIO backend)
    #[arg(long, value_name = "ARGS", requires = "cmd")]
    pub args: Vec<String>,

    /// Working directory for subprocess (for STDIO backend)
    #[arg(long, value_name = "DIR", requires = "cmd")]
    pub working_dir: Option<PathBuf>,

    /// HTTP/SSE backend URL
    #[arg(long, value_name = "URL", group = "backend-type")]
    pub http: Option<String>,

    /// WebSocket backend URL
    #[arg(long, value_name = "URL", group = "backend-type")]
    pub websocket: Option<String>,
}

/// Backend type for MCP server connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BackendType {
    /// Standard input/output (subprocess)
    Stdio,
    /// HTTP with Server-Sent Events
    Http,
    /// WebSocket bidirectional
    Websocket,
}

impl BackendArgs {
    /// Get the backend type
    #[must_use]
    pub fn backend_type(&self) -> Option<BackendType> {
        self.backend.or_else(|| {
            if self.http.is_some() {
                Some(BackendType::Http)
            } else if self.websocket.is_some() {
                Some(BackendType::Websocket)
            } else {
                None
            }
        })
    }

    /// Validate that required arguments for the backend type are present
    ///
    /// # Errors
    ///
    /// Returns a string error message if required arguments for the specified backend type are missing.
    pub fn validate(&self) -> Result<(), String> {
        match self.backend_type() {
            Some(BackendType::Stdio) => {
                if self.cmd.is_none() {
                    return Err("--cmd is required for stdio backend".to_string());
                }
            }
            Some(BackendType::Http) => {
                if self.http.is_none() && self.backend == Some(BackendType::Http) {
                    return Err("--http URL is required for http backend".to_string());
                }
            }
            Some(BackendType::Websocket) => {
                if self.websocket.is_none() && self.backend == Some(BackendType::Websocket) {
                    return Err("--websocket URL is required for websocket backend".to_string());
                }
            }
            None => return Err("No backend specified".to_string()),
        }
        Ok(())
    }
}

/// Output destination for results
#[derive(Debug, Clone, Args)]
pub struct OutputArgs {
    /// Output file (default: stdout)
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Append to output file instead of overwriting
    #[arg(long, requires = "output")]
    pub append: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_detection() {
        let args = BackendArgs {
            backend: Some(BackendType::Stdio),
            cmd: Some("python".to_string()),
            args: vec![],
            working_dir: None,
            http: None,
            websocket: None,
        };
        assert_eq!(args.backend_type(), Some(BackendType::Stdio));
    }

    #[test]
    fn test_backend_validation_stdio() {
        let args = BackendArgs {
            backend: Some(BackendType::Stdio),
            cmd: None,
            args: vec![],
            working_dir: None,
            http: None,
            websocket: None,
        };
        assert!(args.validate().is_err());

        let args = BackendArgs {
            backend: Some(BackendType::Stdio),
            cmd: Some("python".to_string()),
            args: vec![],
            working_dir: None,
            http: None,
            websocket: None,
        };
        assert!(args.validate().is_ok());
    }
}
