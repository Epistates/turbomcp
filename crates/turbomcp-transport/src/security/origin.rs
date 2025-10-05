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
///
/// **Security Model**:
/// - DNS rebinding attacks require remote→localhost connections
/// - localhost→localhost connections are inherently safe (no DNS involved)
/// - If Origin header missing BUT client is localhost → allow (Claude Code case)
/// - If Origin header missing AND client is remote → reject (security)
pub fn validate_origin(
    config: &OriginConfig,
    headers: &SecurityHeaders,
    client_ip: std::net::IpAddr,
) -> Result<(), SecurityError> {
    if config.allow_any {
        return Ok(());
    }

    // Check if Origin header exists
    match headers.get("Origin") {
        Some(origin) => {
            // Origin present → validate it

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
        None => {
            // Origin missing → check if client is localhost
            // DNS rebinding attacks require remote clients, so localhost clients are safe
            if client_ip.is_loopback() {
                // localhost→localhost: No DNS rebinding risk, allow it
                // This enables Claude Code and other local clients
                return Ok(());
            }

            // Remote client without Origin → potential security risk
            Err(SecurityError::InvalidOrigin(
                "Missing Origin header from remote client".to_string(),
            ))
        }
    }
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
    fn test_validate_origin_allows_localhost() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        let client_ip = "127.0.0.1".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_blocks_evil_origin() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://evil.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    #[test]
    fn test_validate_origin_allows_configured_origin() {
        let config = OriginConfig {
            allowed_origins: vec!["https://trusted.com".to_string()]
                .into_iter()
                .collect(),
            allow_localhost: false,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "https://trusted.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_missing_header_localhost_client() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let headers = HashMap::new();
        let client_ip = "127.0.0.1".parse().unwrap();

        // localhost→localhost without Origin → allowed (Claude Code case)
        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_missing_header_remote_client() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let headers = HashMap::new();
        let client_ip = "192.168.1.100".parse().unwrap();

        // remote→localhost without Origin → blocked (security)
        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    #[test]
    fn test_validate_origin_allow_any() {
        let config = OriginConfig {
            allow_localhost: true,
            allow_any: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://anything.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }
}
