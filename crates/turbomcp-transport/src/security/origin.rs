//! Origin header validation for DNS rebinding protection
//!
//! This module implements critical origin validation required by MCP specification
//! to prevent DNS rebinding attacks. It provides flexible configuration for
//! development, staging, and production environments.

use super::errors::SecurityError;
use crate::security::SecurityHeaders;
use std::collections::HashSet;

/// Origin validation configuration
#[derive(Clone, Debug)]
pub struct OriginConfig {
    /// Allowed origins for CORS
    pub allowed_origins: HashSet<String>,
    /// Whether to allow localhost origins (for development)
    pub allow_localhost: bool,
    /// Whether to allow any origin (DANGEROUS - only for testing)
    pub allow_any: bool,
}

impl Default for OriginConfig {
    fn default() -> Self {
        Self {
            allowed_origins: HashSet::new(),
            allow_localhost: true,
            allow_any: false,
        }
    }
}

impl OriginConfig {
    /// Create a new origin configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create development configuration allowing localhost
    pub fn for_development() -> Self {
        Self {
            allowed_origins: HashSet::new(),
            allow_localhost: true,
            allow_any: false,
        }
    }

    /// Create production configuration with specific allowed origins
    pub fn for_production(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins: allowed_origins.into_iter().collect(),
            allow_localhost: false,
            allow_any: false,
        }
    }

    /// Create testing configuration allowing any origin (DANGEROUS)
    pub fn for_testing() -> Self {
        Self {
            allowed_origins: HashSet::new(),
            allow_localhost: true,
            allow_any: true,
        }
    }

    /// Add an allowed origin
    pub fn add_origin(&mut self, origin: String) {
        self.allowed_origins.insert(origin);
    }

    /// Add multiple allowed origins
    pub fn add_origins(&mut self, origins: Vec<String>) {
        self.allowed_origins.extend(origins);
    }

    /// Set whether to allow localhost origins
    pub fn set_allow_localhost(&mut self, allow: bool) {
        self.allow_localhost = allow;
    }

    /// Set whether to allow any origin (use with extreme caution)
    pub fn set_allow_any(&mut self, allow: bool) {
        self.allow_any = allow;
    }
}

/// Validate Origin header to prevent DNS rebinding attacks
///
/// Per MCP 2025-06-18 specification:
/// "Servers MUST validate the Origin header on all incoming connections
/// to prevent DNS rebinding attacks"
pub fn validate_origin(
    config: &OriginConfig,
    headers: &SecurityHeaders,
) -> Result<(), SecurityError> {
    if config.allow_any {
        return Ok(());
    }

    let origin = headers
        .get("Origin")
        .ok_or_else(|| SecurityError::InvalidOrigin("Missing Origin header".to_string()))?;

    // Allow explicitly configured origins
    if config.allowed_origins.contains(origin) {
        return Ok(());
    }

    // Allow localhost origins for development
    if config.allow_localhost {
        let localhost_patterns = [
            "http://localhost",
            "https://localhost",
            "http://127.0.0.1",
            "https://127.0.0.1",
        ];

        if localhost_patterns
            .iter()
            .any(|&pattern| origin.starts_with(pattern))
        {
            return Ok(());
        }
    }

    Err(SecurityError::InvalidOrigin(format!(
        "Origin '{}' not allowed",
        origin
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_origin_config_default() {
        let config = OriginConfig::default();
        assert!(config.allow_localhost);
        assert!(!config.allow_any);
        assert!(config.allowed_origins.is_empty());
    }

    #[test]
    fn test_origin_config_for_development() {
        let config = OriginConfig::for_development();
        assert!(config.allow_localhost);
        assert!(!config.allow_any);
    }

    #[test]
    fn test_origin_config_for_production() {
        let origins = vec!["https://app.example.com".to_string()];
        let config = OriginConfig::for_production(origins.clone());
        assert!(!config.allow_localhost);
        assert!(!config.allow_any);
        assert!(config.allowed_origins.contains(&origins[0]));
    }

    #[test]
    fn test_validate_origin_allows_localhost() {
        let config = OriginConfig::for_development();
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());

        assert!(validate_origin(&config, &headers).is_ok());
    }

    #[test]
    fn test_validate_origin_blocks_evil_origin() {
        let config = OriginConfig::for_development();
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://evil.com".to_string());

        assert!(validate_origin(&config, &headers).is_err());
    }

    #[test]
    fn test_validate_origin_allows_configured_origin() {
        let config = OriginConfig::for_production(vec!["https://trusted.com".to_string()]);
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "https://trusted.com".to_string());

        assert!(validate_origin(&config, &headers).is_ok());
    }

    #[test]
    fn test_validate_origin_missing_header() {
        let config = OriginConfig::for_development();
        let headers = HashMap::new();

        assert!(validate_origin(&config, &headers).is_err());
    }

    #[test]
    fn test_validate_origin_allow_any() {
        let config = OriginConfig::for_testing();
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://anything.com".to_string());

        assert!(validate_origin(&config, &headers).is_ok());
    }
}
