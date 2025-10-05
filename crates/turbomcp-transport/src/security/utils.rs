//! Security utility functions for transport layer
//!
//! This module provides utility functions for common security operations
//! including message size validation, header manipulation, and security
//! checks that don't fit into the main security modules.

use super::errors::SecurityError;
use std::collections::HashMap;

/// Generic header representation for cross-framework compatibility
pub type HeaderValue = String;
/// Security headers type for HTTP requests
pub type SecurityHeaders = HashMap<String, HeaderValue>;

/// Message size validation
pub fn validate_message_size(data: &[u8], max_size: usize) -> Result<(), SecurityError> {
    if data.len() > max_size {
        return Err(SecurityError::MessageTooLarge {
            size: data.len(),
            limit: max_size,
        });
    }
    Ok(())
}

/// Validate message size with string input
pub fn validate_string_size(data: &str, max_size: usize) -> Result<(), SecurityError> {
    validate_message_size(data.as_bytes(), max_size)
}

/// Validate JSON message size
pub fn validate_json_size(json: &serde_json::Value, max_size: usize) -> Result<(), SecurityError> {
    let json_string = serde_json::to_string(json).map_err(|_| SecurityError::MessageTooLarge {
        size: 0,
        limit: max_size,
    })?;
    validate_string_size(&json_string, max_size)
}

/// Extract client IP from various header sources
pub fn extract_client_ip(headers: &SecurityHeaders) -> Option<std::net::IpAddr> {
    // Check X-Forwarded-For header first (most common)
    if let Some(forwarded) = headers.get("X-Forwarded-For")
        && let Some(first_ip) = forwarded.split(',').next()
        && let Ok(ip) = first_ip.trim().parse()
    {
        return Some(ip);
    }

    // Check X-Real-IP header
    if let Some(real_ip) = headers.get("X-Real-IP")
        && let Ok(ip) = real_ip.parse()
    {
        return Some(ip);
    }

    // Check CF-Connecting-IP (Cloudflare)
    if let Some(cf_ip) = headers.get("CF-Connecting-IP")
        && let Ok(ip) = cf_ip.parse()
    {
        return Some(ip);
    }

    // Check X-Client-IP
    if let Some(client_ip) = headers.get("X-Client-IP")
        && let Ok(ip) = client_ip.parse()
    {
        return Some(ip);
    }

    None
}

/// Sanitize header value to prevent header injection attacks
pub fn sanitize_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect::<String>()
        .replace(['\n', '\r'], "")
}

/// Create security headers for response
pub fn create_security_headers() -> SecurityHeaders {
    let mut headers = SecurityHeaders::new();

    // Prevent clickjacking
    headers.insert("X-Frame-Options".to_string(), "DENY".to_string());

    // Prevent MIME type sniffing
    headers.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());

    // Enable XSS protection
    headers.insert("X-XSS-Protection".to_string(), "1; mode=block".to_string());

    // Enforce HTTPS
    headers.insert(
        "Strict-Transport-Security".to_string(),
        "max-age=31536000; includeSubDomains; preload".to_string(),
    );

    // Content Security Policy (basic)
    headers.insert(
        "Content-Security-Policy".to_string(),
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'".to_string(),
    );

    // Referrer Policy
    headers.insert(
        "Referrer-Policy".to_string(),
        "strict-origin-when-cross-origin".to_string(),
    );

    headers
}

/// Create CORS headers for preflight responses
pub fn create_cors_headers(allowed_origin: &str) -> SecurityHeaders {
    let mut headers = SecurityHeaders::new();

    headers.insert(
        "Access-Control-Allow-Origin".to_string(),
        allowed_origin.to_string(),
    );

    headers.insert(
        "Access-Control-Allow-Methods".to_string(),
        "GET, POST, OPTIONS".to_string(),
    );

    headers.insert(
        "Access-Control-Allow-Headers".to_string(),
        "Content-Type, Authorization, Origin".to_string(),
    );

    headers.insert(
        "Access-Control-Max-Age".to_string(),
        "86400".to_string(), // 24 hours
    );

    headers
}

/// Validate that a header value is safe
pub fn is_safe_header_value(value: &str) -> bool {
    // Check for control characters that could enable header injection
    !value.chars().any(|c| {
        c.is_control() && c != '\t' // Allow tab but no other control chars
    })
}

/// Extract Bearer token from Authorization header
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    if auth_header.starts_with("Bearer ") && auth_header.len() > 7 {
        Some(&auth_header[7..])
    } else {
        None
    }
}

/// Extract API key from Authorization header
pub fn extract_api_key(auth_header: &str) -> Option<&str> {
    if auth_header.starts_with("ApiKey ") && auth_header.len() > 7 {
        Some(&auth_header[7..])
    } else {
        None
    }
}

/// Check if origin is localhost variant
pub fn is_localhost_origin(origin: &str) -> bool {
    let localhost_patterns = [
        "http://localhost",
        "https://localhost",
        "http://127.0.0.1",
        "https://127.0.0.1",
    ];

    localhost_patterns
        .iter()
        .any(|&pattern| origin.starts_with(pattern))
}

/// Generate a secure random string for tokens/keys
pub fn generate_secure_token(length: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut result = String::with_capacity(length);

    for _ in 0..length {
        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        uuid::Uuid::new_v4().hash(&mut hasher);

        let hash = hasher.finish();
        let char_index = (hash % 62) as usize;

        let chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        result.push(chars[char_index] as char);
    }

    result
}

/// Common message size limits
pub mod size_limits {
    /// Small message limit (1KB)
    pub const SMALL_MESSAGE: usize = 1024;

    /// Medium message limit (1MB)
    pub const MEDIUM_MESSAGE: usize = 1024 * 1024;

    /// Large message limit (10MB)
    pub const LARGE_MESSAGE: usize = 10 * 1024 * 1024;

    /// Maximum reasonable message size (100MB)
    pub const MAX_MESSAGE: usize = 100 * 1024 * 1024;

    /// Default JSON RPC message limit (1MB)
    pub const JSON_RPC_DEFAULT: usize = MEDIUM_MESSAGE;

    /// WebSocket message limit (16MB)
    pub const WEBSOCKET_DEFAULT: usize = 16 * 1024 * 1024;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_message_size_success() {
        let data = b"small message";
        assert!(validate_message_size(data, 1024).is_ok());
    }

    #[test]
    fn test_validate_message_size_failure() {
        let data = vec![0u8; 2000]; // 2KB message
        assert!(validate_message_size(&data, 1024).is_err());
    }

    #[test]
    fn test_validate_string_size() {
        assert!(validate_string_size("hello", 10).is_ok());
        assert!(validate_string_size("hello world", 5).is_err());
    }

    #[test]
    fn test_validate_json_size() {
        let small_json = json!({"key": "value"});
        let large_json = json!({"key": "value".repeat(1000)});

        assert!(validate_json_size(&small_json, 1024).is_ok());
        assert!(validate_json_size(&large_json, 100).is_err());
    }

    #[test]
    fn test_extract_client_ip() {
        let mut headers = SecurityHeaders::new();

        // Test X-Forwarded-For
        headers.insert(
            "X-Forwarded-For".to_string(),
            "192.168.1.1, 10.0.0.1".to_string(),
        );
        let ip = extract_client_ip(&headers).unwrap();
        assert_eq!(ip.to_string(), "192.168.1.1");

        // Test X-Real-IP
        headers.clear();
        headers.insert("X-Real-IP".to_string(), "203.0.113.1".to_string());
        let ip = extract_client_ip(&headers).unwrap();
        assert_eq!(ip.to_string(), "203.0.113.1");

        // Test no headers
        headers.clear();
        assert!(extract_client_ip(&headers).is_none());
    }

    #[test]
    fn test_sanitize_header_value() {
        assert_eq!(sanitize_header_value("normal value"), "normal value");
        assert_eq!(
            sanitize_header_value("value\nwith\rnewlines"),
            "valuewithnewlines"
        );
        assert_eq!(
            sanitize_header_value("value\x00with\x01control"),
            "valuewithcontrol"
        );
    }

    #[test]
    fn test_create_security_headers() {
        let headers = create_security_headers();

        assert!(headers.contains_key("X-Frame-Options"));
        assert!(headers.contains_key("X-Content-Type-Options"));
        assert!(headers.contains_key("X-XSS-Protection"));
        assert!(headers.contains_key("Strict-Transport-Security"));
        assert!(headers.contains_key("Content-Security-Policy"));
        assert!(headers.contains_key("Referrer-Policy"));
    }

    #[test]
    fn test_create_cors_headers() {
        let headers = create_cors_headers("https://example.com");

        assert_eq!(
            headers.get("Access-Control-Allow-Origin"),
            Some(&"https://example.com".to_string())
        );
        assert!(headers.contains_key("Access-Control-Allow-Methods"));
        assert!(headers.contains_key("Access-Control-Allow-Headers"));
        assert!(headers.contains_key("Access-Control-Max-Age"));
    }

    #[test]
    fn test_is_safe_header_value() {
        assert!(is_safe_header_value("safe value"));
        assert!(is_safe_header_value("value\twith\ttab"));
        assert!(!is_safe_header_value("value\nwith\nnewline"));
        assert!(!is_safe_header_value("value\rwith\rcarriage"));
        assert!(!is_safe_header_value("value\x00with\x01control"));
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(extract_bearer_token("Bearer token123"), Some("token123"));
        assert_eq!(extract_bearer_token("Bearer "), None);
        assert_eq!(extract_bearer_token("Basic token123"), None);
        assert_eq!(extract_bearer_token(""), None);
    }

    #[test]
    fn test_extract_api_key() {
        assert_eq!(extract_api_key("ApiKey key123"), Some("key123"));
        assert_eq!(extract_api_key("ApiKey "), None);
        assert_eq!(extract_api_key("Bearer key123"), None);
        assert_eq!(extract_api_key(""), None);
    }

    #[test]
    fn test_is_localhost_origin() {
        assert!(is_localhost_origin("http://localhost:3000"));
        assert!(is_localhost_origin("https://localhost"));
        assert!(is_localhost_origin("http://127.0.0.1:8080"));
        assert!(is_localhost_origin("https://127.0.0.1"));
        assert!(!is_localhost_origin("https://example.com"));
        assert!(!is_localhost_origin("http://evil.com"));
    }

    #[test]
    fn test_generate_secure_token() {
        let token1 = generate_secure_token(32);
        let token2 = generate_secure_token(32);

        assert_eq!(token1.len(), 32);
        assert_eq!(token2.len(), 32);
        assert_ne!(token1, token2); // Should be different

        // Should only contain alphanumeric characters
        assert!(token1.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_size_limits_constants() {
        assert_eq!(size_limits::SMALL_MESSAGE, 1024);
        assert_eq!(size_limits::MEDIUM_MESSAGE, 1024 * 1024);
        assert_eq!(size_limits::LARGE_MESSAGE, 10 * 1024 * 1024);
        assert_eq!(size_limits::JSON_RPC_DEFAULT, size_limits::MEDIUM_MESSAGE);
        assert_eq!(size_limits::MAX_MESSAGE, 100 * 1024 * 1024); // Updated to match actual constant
    }
}
