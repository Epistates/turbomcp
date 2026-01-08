//! Telemetry error types

use thiserror::Error;

/// Errors that can occur during telemetry operations
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// Failed to initialize telemetry
    #[error("Failed to initialize telemetry: {0}")]
    InitializationFailed(String),

    /// Invalid configuration
    #[error("Invalid telemetry configuration: {0}")]
    InvalidConfiguration(String),

    /// Export failed
    #[error("Failed to export telemetry data: {0}")]
    ExportFailed(String),

    /// Tracing subscriber error
    #[error("Tracing subscriber error: {0}")]
    TracingError(String),

    /// OpenTelemetry error
    #[cfg(feature = "opentelemetry")]
    #[error("OpenTelemetry error: {0}")]
    OpenTelemetryError(String),

    /// Metrics error
    #[cfg(feature = "prometheus")]
    #[error("Metrics error: {0}")]
    MetricsError(String),
}

/// Result type for telemetry operations
pub type TelemetryResult<T> = Result<T, TelemetryError>;
