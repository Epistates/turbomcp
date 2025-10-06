//! OAuth 2.1 Validation Functions
//!
//! This module provides validation functions for OAuth 2.1 flows:
//! - RFC 8707 Resource Indicator validation
//! - URI format and security validation
//! - Canonical form validation

use turbomcp_core::{Error as McpError, Result as McpResult};

/// RFC 8707 canonical URI validation for Resource Indicators
///
/// Validates that a resource URI:
/// - Uses http or https scheme
/// - Does not contain fragments
/// - Has a valid host component
/// - Uses canonical form (lowercase scheme and host)
///
/// # Arguments
/// * `uri` - The resource URI to validate
///
/// # Returns
/// * `Ok(())` if the URI is valid
/// * `Err(McpError)` if validation fails
///
/// # RFC 8707 Compliance
/// This function ensures resource URIs are in canonical form as required by RFC 8707.
/// MCP servers must use canonical URIs to prevent token binding issues.
pub fn validate_canonical_resource_uri(uri: &str) -> McpResult<()> {
    use url::Url;

    // Check canonical form BEFORE parsing (URL parser normalizes automatically)
    // RFC 8707 requires canonical URIs: lowercase scheme and host
    let scheme_end = uri
        .find("://")
        .ok_or_else(|| McpError::validation("Resource URI must have a valid scheme".to_string()))?;

    let scheme = &uri[..scheme_end];
    if scheme != scheme.to_lowercase() {
        return Err(McpError::validation(
            "Resource URI must use canonical form (lowercase scheme and host)".to_string(),
        ));
    }

    let parsed =
        Url::parse(uri).map_err(|e| McpError::validation(format!("Invalid resource URI: {e}")))?;

    // RFC 8707 requirements
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(McpError::validation(
            "Resource URI must use http or https scheme".to_string(),
        ));
    }

    if parsed.fragment().is_some() {
        return Err(McpError::validation(
            "Resource URI must not contain fragment".to_string(),
        ));
    }

    // MCP-specific validation for canonical URIs
    if parsed.host_str().is_none() {
        return Err(McpError::validation(
            "Resource URI must include host".to_string(),
        ));
    }

    // Extract host once to verify it exists (safe because of validation above)
    let _host = parsed.host_str().expect("host validated above");

    // Check host is lowercase (canonical form)
    // We check the original URI since URL parser might normalize
    let host_start = uri.find("://").expect("scheme checked above") + 3;
    let host_in_uri = &uri[host_start..];
    let host_end = host_in_uri
        .find(['/', ':', '?', '#'])
        .unwrap_or(host_in_uri.len());
    let original_host = &host_in_uri[..host_end];

    if original_host != original_host.to_lowercase() {
        return Err(McpError::validation(
            "Resource URI must use canonical form (lowercase scheme and host)".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_https_uri() {
        assert!(validate_canonical_resource_uri("https://example.com/resource").is_ok());
    }

    #[test]
    fn test_valid_http_uri() {
        assert!(validate_canonical_resource_uri("http://example.com/resource").is_ok());
    }

    #[test]
    fn test_non_canonical_uppercase_host() {
        let result = validate_canonical_resource_uri("https://Example.COM/resource");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("canonical form"));
    }

    #[test]
    fn test_non_canonical_uppercase_scheme() {
        let result = validate_canonical_resource_uri("HTTPS://example.com/resource");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("canonical form"));
    }

    #[test]
    fn test_missing_host() {
        // file:// scheme is rejected before host check
        let result = validate_canonical_resource_uri("file:///etc/passwd");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("http or https scheme")
        );

        // For testing missing host with valid scheme, URL parser doesn't allow empty host with http/https
        // so we test the host check implicitly through the canonical form tests
    }

    #[test]
    fn test_fragment_not_allowed() {
        let result = validate_canonical_resource_uri("https://example.com/resource#fragment");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fragment"));
    }

    #[test]
    fn test_invalid_scheme() {
        let result = validate_canonical_resource_uri("ftp://example.com/resource");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("http or https scheme")
        );
    }
}
