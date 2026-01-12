//! Logging initialization for MCP servers
//!
//! This module provides configurable logging with file rotation support.
//! The API is designed to make guard requirements clear at compile time.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use turbomcp_server::LoggingConfig;
//!
//! // Stderr-only (no guard needed)
//! LoggingConfig::stderr_minimal().init()?;
//!
//! // File logging (guard must be held)
//! let _guard = LoggingConfig::stdio_file("/var/log/myserver").init()?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # When is a Guard Needed?
//!
//! | Output Target | Guard Required | Why |
//! |---------------|----------------|-----|
//! | `Stderr` | No | Direct writes, no buffering |
//! | `FileOnly` | **Yes** | Non-blocking I/O buffers logs |
//! | `Both` | **Yes** | File component needs flushing |
//! | `None` | No | No logging |
//!
//! The guard ensures buffered logs are flushed when your program exits.
//! If you drop the guard early, pending logs may be lost.
//!
//! # STDIO Transport
//!
//! For STDIO MCP servers, logs must NOT go to stdout (that's the protocol channel).
//! Use `LoggingConfig::stdio_file()` for pristine operation, or
//! `LoggingConfig::stderr_minimal()` for MCP-compliant stderr logging.

use crate::config::{LogOutput, LogRotation, LoggingConfig};
use std::io;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

/// Guard that ensures file logs are flushed on drop
///
/// This guard **must be held** for the duration of your program when using
/// file-based logging. When dropped, it flushes all pending log messages.
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_server::LoggingConfig;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     // Guard lives until main() returns
///     let _guard = LoggingConfig::stdio_file("/var/log/app").init()?;
///
///     // Your server code...
///
///     Ok(())
///     // Guard dropped here, logs flushed
/// }
/// ```
#[derive(Debug)]
pub struct LoggingGuard {
    _file_guard: WorkerGuard,
    _stderr_guard: Option<WorkerGuard>,
}

impl LoggingConfig {
    /// Initialize logging based on this configuration
    ///
    /// Returns `Some(LoggingGuard)` for file-based logging (must be held),
    /// or `None` for stderr-only logging (no guard needed).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File logging is configured but directory creation fails
    /// - The tracing subscriber cannot be initialized (already set)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_server::LoggingConfig;
    ///
    /// // Stderr-only - no guard needed
    /// LoggingConfig::stderr_minimal().init()?;
    ///
    /// // File logging - hold the guard!
    /// let _guard = LoggingConfig::stdio_file("/var/log/app").init()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn init(&self) -> io::Result<Option<LoggingGuard>> {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&self.level));

        match self.output {
            LogOutput::None => Ok(None),
            LogOutput::Stderr => {
                init_stderr(self, filter)?;
                Ok(None)
            }
            LogOutput::FileOnly => {
                let dir = self.directory.as_ref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "File logging requires a directory (use .directory field or stdio_file())",
                    )
                })?;
                let guard = init_file_only(self, dir, filter)?;
                Ok(Some(guard))
            }
            LogOutput::Both => {
                let dir = self.directory.as_ref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "File logging requires a directory (use .directory field or production())",
                    )
                })?;
                let guard = init_stderr_and_file(self, dir, filter)?;
                Ok(Some(guard))
            }
        }
    }
}

/// Initialize stderr-only logging (no guard needed)
fn init_stderr(config: &LoggingConfig, filter: EnvFilter) -> io::Result<()> {
    let subscriber = tracing_subscriber::registry().with(filter);

    if config.structured {
        subscriber
            .with(fmt::layer().json().with_writer(io::stderr))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))
    } else {
        subscriber
            .with(fmt::layer().with_writer(io::stderr))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

/// Initialize file-only logging (returns guard)
fn init_file_only(
    config: &LoggingConfig,
    dir: &std::path::Path,
    filter: EnvFilter,
) -> io::Result<LoggingGuard> {
    std::fs::create_dir_all(dir)?;

    let file_appender = match config.rotation {
        LogRotation::Minute => tracing_appender::rolling::minutely(dir, &config.file_prefix),
        LogRotation::Hourly => tracing_appender::rolling::hourly(dir, &config.file_prefix),
        LogRotation::Daily => tracing_appender::rolling::daily(dir, &config.file_prefix),
        LogRotation::Never => tracing_appender::rolling::never(dir, &config.file_prefix),
    };

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::registry().with(filter);

    if config.structured {
        subscriber
            .with(fmt::layer().json().with_writer(non_blocking))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))?;
    } else {
        subscriber
            .with(fmt::layer().with_writer(non_blocking))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))?;
    }

    Ok(LoggingGuard {
        _file_guard: guard,
        _stderr_guard: None,
    })
}

/// Initialize stderr + file logging (returns guard)
fn init_stderr_and_file(
    config: &LoggingConfig,
    dir: &std::path::Path,
    filter: EnvFilter,
) -> io::Result<LoggingGuard> {
    std::fs::create_dir_all(dir)?;

    let file_appender = match config.rotation {
        LogRotation::Minute => tracing_appender::rolling::minutely(dir, &config.file_prefix),
        LogRotation::Hourly => tracing_appender::rolling::hourly(dir, &config.file_prefix),
        LogRotation::Daily => tracing_appender::rolling::daily(dir, &config.file_prefix),
        LogRotation::Never => tracing_appender::rolling::never(dir, &config.file_prefix),
    };

    let (file_non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);
    let (stderr_non_blocking, stderr_guard) = tracing_appender::non_blocking(io::stderr());
    let combined = file_non_blocking.and(stderr_non_blocking);

    let subscriber = tracing_subscriber::registry().with(filter);

    if config.structured {
        subscriber
            .with(fmt::layer().json().with_writer(combined))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))?;
    } else {
        subscriber
            .with(fmt::layer().with_writer(combined))
            .try_init()
            .map_err(|e| io::Error::other(e.to_string()))?;
    }

    Ok(LoggingGuard {
        _file_guard: file_guard,
        _stderr_guard: Some(stderr_guard),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_config_presets() {
        // stderr_minimal - no directory, stderr output
        let config = LoggingConfig::stderr_minimal();
        assert_eq!(config.level, "error");
        assert_eq!(config.output, LogOutput::Stderr);
        assert!(config.directory.is_none());

        // stdio_file - has directory, file output
        let config = LoggingConfig::stdio_file("/var/log/test");
        assert_eq!(config.level, "info");
        assert_eq!(config.output, LogOutput::FileOnly);
        assert!(config.directory.is_some());

        // stderr_debug - no directory, stderr output
        let config = LoggingConfig::stderr_debug();
        assert_eq!(config.level, "debug");
        assert_eq!(config.output, LogOutput::Stderr);

        // production - has directory, both outputs
        let config = LoggingConfig::production("/var/log/prod");
        assert_eq!(config.output, LogOutput::Both);
        assert_eq!(config.rotation, LogRotation::Hourly);
    }

    #[test]
    fn test_file_only_requires_directory() {
        let config = LoggingConfig {
            level: "info".to_string(),
            structured: false,
            output: LogOutput::FileOnly,
            directory: None, // Missing!
            file_prefix: "test".to_string(),
            rotation: LogRotation::Never,
        };

        let result = config.init();
        assert!(result.is_err());
    }

    #[test]
    fn test_log_rotation_variants() {
        assert_eq!(LogRotation::default(), LogRotation::Never);

        // All variants are distinct
        let variants = [
            LogRotation::Minute,
            LogRotation::Hourly,
            LogRotation::Daily,
            LogRotation::Never,
        ];

        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
