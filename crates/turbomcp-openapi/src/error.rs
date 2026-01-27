//! Error types for OpenAPI operations.

use thiserror::Error;

/// Result type for OpenAPI operations.
pub type Result<T> = std::result::Result<T, OpenApiError>;

/// Errors that can occur during OpenAPI operations.
#[derive(Debug, Error)]
pub enum OpenApiError {
    /// Failed to fetch OpenAPI spec from URL.
    #[error("failed to fetch OpenAPI spec: {0}")]
    FetchError(#[from] reqwest::Error),

    /// Failed to parse OpenAPI spec.
    #[error("failed to parse OpenAPI spec: {0}")]
    ParseError(String),

    /// Failed to read OpenAPI spec from file.
    #[error("failed to read OpenAPI spec file: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid URL.
    #[error("invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    /// Invalid regex pattern.
    #[error("invalid regex pattern: {0}")]
    InvalidPattern(#[from] regex::Error),

    /// API call failed.
    #[error("API call failed: {0}")]
    ApiError(String),

    /// Missing required parameter.
    #[error("missing required parameter: {0}")]
    MissingParameter(String),

    /// Invalid parameter value.
    #[error("invalid parameter value for '{0}': {1}")]
    InvalidParameter(String, String),

    /// Operation not found.
    #[error("operation not found: {0}")]
    OperationNotFound(String),

    /// Base URL not configured.
    #[error("base URL not configured - call with_base_url() before making API calls")]
    NoBaseUrl,

    /// SSRF protection blocked the request.
    #[error("SSRF protection: {0}")]
    SsrfBlocked(String),

    /// Request timed out.
    #[error("request timed out after {0} seconds")]
    Timeout(u64),
}

impl From<serde_json::Error> for OpenApiError {
    fn from(err: serde_json::Error) -> Self {
        Self::ParseError(err.to_string())
    }
}

impl From<serde_yaml::Error> for OpenApiError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::ParseError(err.to_string())
    }
}
