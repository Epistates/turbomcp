//! RFC 8707 Resource Indicators for OAuth 2.0
//!
//! This module implements resource indicator support as required by the MCP specification.
//! Resource indicators bind access tokens to specific resource servers, preventing token
//! misuse across service boundaries.
//!
//! # MCP Requirements
//!
//! Per MCP specification:
//! - Clients MUST include the `resource` parameter in authorization and token requests
//! - The resource parameter MUST identify the MCP server canonical URI
//! - Servers MUST validate tokens were issued specifically for them
//!
//! # RFC 8707 Compliance
//!
//! This implementation follows RFC 8707 Section 2 for resource identifiers:
//! - Uses absolute URIs (https://api.example.com/path)
//! - Normalizes scheme and host to lowercase
//! - Removes fragments (forbidden by spec)
//! - Removes query parameters (normalized form)
//! - Preserves port numbers when non-default

use turbomcp_protocol::{Error as McpError, Result as McpResult};
use url::Url;

/// Validate and normalize a resource URI per RFC 8707
///
/// This function ensures the resource identifier meets RFC 8707 requirements
/// and returns it in canonical form for consistency.
///
/// # Requirements
///
/// - MUST be an absolute URI (scheme + host + path)
/// - MUST use http or https scheme
/// - MUST NOT contain fragments (#)
/// - SHOULD use lowercase scheme and host
/// - SHOULD omit trailing slash unless semantically significant
///
/// # Arguments
///
/// * `uri` - The resource URI to validate (e.g., "https://api.example.com/mcp")
///
/// # Returns
///
/// Canonical form of the URI suitable for use as a resource parameter
///
/// # Errors
///
/// Returns error if:
/// - URI is not a valid absolute URI
/// - Scheme is not http or https
/// - URI contains a fragment
/// - Host is missing
///
/// # Examples
///
/// ```rust
/// use turbomcp_auth::oauth2::validate_resource_uri;
///
/// // Valid URIs
/// assert_eq!(
///     validate_resource_uri("https://api.example.com/mcp").unwrap(),
///     "https://api.example.com/mcp"
/// );
///
/// // Normalizes scheme and host to lowercase
/// assert_eq!(
///     validate_resource_uri("HTTPS://API.EXAMPLE.COM/mcp").unwrap(),
///     "https://api.example.com/mcp"
/// );
///
/// // Preserves non-default ports
/// assert_eq!(
///     validate_resource_uri("https://api.example.com:8443/mcp").unwrap(),
///     "https://api.example.com:8443/mcp"
/// );
///
/// // Removes fragments (forbidden)
/// let result = validate_resource_uri("https://api.example.com#fragment");
/// assert!(result.is_err());
/// ```
pub fn validate_resource_uri(uri: &str) -> McpResult<String> {
    // Parse URL
    let url = Url::parse(uri)
        .map_err(|e| McpError::validation(format!("Invalid resource URI format: {e}")))?;

    // Validate scheme (MCP requires https, but allow http for localhost development)
    match url.scheme() {
        "https" => {}
        "http" => {
            // Only allow http for localhost/127.0.0.1 (development only)
            if let Some(host) = url.host_str() {
                let is_localhost = host == "localhost"
                    || host == "127.0.0.1"
                    || host == "0.0.0.0"
                    || host == "[::1]"; // IPv6 localhost

                if !is_localhost {
                    return Err(McpError::validation(
                        "Resource URI must use https scheme (http only allowed for localhost)"
                            .to_string(),
                    ));
                }
            }
        }
        scheme => {
            return Err(McpError::validation(format!(
                "Resource URI must use http or https scheme, got: {scheme}"
            )));
        }
    }

    // Validate host is present
    let host = url
        .host_str()
        .ok_or_else(|| McpError::validation("Resource URI must have a valid host".to_string()))?;

    // Reject fragments (RFC 8707 requirement)
    if url.fragment().is_some() {
        return Err(McpError::validation(
            "Resource URI must not contain fragment (#)".to_string(),
        ));
    }

    // Build canonical URI
    // RFC 8707: normalize scheme and host to lowercase, preserve path
    let canonical = build_canonical_uri(&url, host)?;

    Ok(canonical)
}

/// Build canonical URI form per RFC 8707
fn build_canonical_uri(url: &Url, host: &str) -> McpResult<String> {
    let scheme = url.scheme().to_lowercase();
    let host_lower = host.to_lowercase();

    // Handle port (only include if non-default)
    let port_str = match url.port() {
        Some(port) => {
            // Check if it's the default port for the scheme
            let is_default = (scheme == "https" && port == 443) || (scheme == "http" && port == 80);

            if is_default {
                String::new()
            } else {
                format!(":{port}")
            }
        }
        None => String::new(),
    };

    // Get path, removing trailing slash unless it's just "/"
    let path = url.path();
    let normalized_path = if path == "/" {
        path.to_string()
    } else {
        path.trim_end_matches('/').to_string()
    };

    // Assemble canonical URI (scheme + host + port + path, no query or fragment)
    Ok(format!(
        "{scheme}://{host_lower}{port_str}{normalized_path}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_https_uri() {
        let uri = "https://api.example.com/mcp";
        let result = validate_resource_uri(uri).unwrap();
        assert_eq!(result, "https://api.example.com/mcp");
    }

    #[test]
    fn test_uri_normalization() {
        // Uppercase scheme and host should be normalized
        let uri = "HTTPS://API.EXAMPLE.COM/MCP";
        let result = validate_resource_uri(uri).unwrap();
        assert_eq!(result, "https://api.example.com/MCP"); // Path preserves case
    }

    #[test]
    fn test_trailing_slash_removal() {
        let uri = "https://api.example.com/mcp/";
        let result = validate_resource_uri(uri).unwrap();
        assert_eq!(result, "https://api.example.com/mcp");

        // Root path preserves slash
        let uri2 = "https://api.example.com/";
        let result2 = validate_resource_uri(uri2).unwrap();
        assert_eq!(result2, "https://api.example.com/");
    }

    #[test]
    fn test_port_handling() {
        // Non-default port preserved
        let uri = "https://api.example.com:8443/mcp";
        let result = validate_resource_uri(uri).unwrap();
        assert_eq!(result, "https://api.example.com:8443/mcp");

        // Default HTTPS port (443) omitted
        let uri2 = "https://api.example.com:443/mcp";
        let result2 = validate_resource_uri(uri2).unwrap();
        assert_eq!(result2, "https://api.example.com/mcp");

        // Default HTTP port (80) omitted
        let uri3 = "http://localhost:80/mcp";
        let result3 = validate_resource_uri(uri3).unwrap();
        assert_eq!(result3, "http://localhost/mcp");
    }

    #[test]
    fn test_localhost_http_allowed() {
        let uris = vec![
            "http://localhost/mcp",
            "http://127.0.0.1/mcp",
            "http://0.0.0.0/mcp",
            "http://[::1]/mcp",
        ];

        for uri in uris {
            let result = validate_resource_uri(uri);
            assert!(result.is_ok(), "Should allow http for {uri}");
        }
    }

    #[test]
    fn test_http_non_localhost_rejected() {
        let uri = "http://api.example.com/mcp";
        let result = validate_resource_uri(uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("https scheme"));
    }

    #[test]
    fn test_fragment_rejected() {
        let uri = "https://api.example.com/mcp#fragment";
        let result = validate_resource_uri(uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fragment"));
    }

    #[test]
    fn test_invalid_scheme_rejected() {
        let uri = "ftp://api.example.com/mcp";
        let result = validate_resource_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_host_rejected() {
        // Invalid URI with missing host
        let uri = "https://";
        let result = validate_resource_uri(uri);
        assert!(result.is_err());

        // Relative URI (not absolute)
        let uri2 = "/path/to/resource";
        let result2 = validate_resource_uri(uri2);
        assert!(result2.is_err());
    }

    #[test]
    fn test_query_parameters_removed() {
        // Query parameters are stripped in canonical form
        let uri = "https://api.example.com/mcp?param=value";
        let result = validate_resource_uri(uri).unwrap();
        assert_eq!(result, "https://api.example.com/mcp");
    }

    #[test]
    fn test_mcp_examples() {
        // Examples from MCP specification
        let examples = vec![
            ("https://mcp.example.com/mcp", "https://mcp.example.com/mcp"),
            ("https://mcp.example.com", "https://mcp.example.com/"),
            (
                "https://mcp.example.com:8443",
                "https://mcp.example.com:8443/",
            ),
            (
                "https://mcp.example.com/server/mcp",
                "https://mcp.example.com/server/mcp",
            ),
        ];

        for (input, expected) in examples {
            let result = validate_resource_uri(input).unwrap();
            assert_eq!(result, expected, "Failed for input: {input}");
        }
    }
}
