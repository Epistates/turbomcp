//! CORS configuration management
//!
//! This module provides CORS (Cross-Origin Resource Sharing) configuration
//! with secure defaults for different environments.

use std::time::Duration;

/// CORS configuration with secure defaults
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Enable CORS
    pub enabled: bool,
    /// Allowed origins (None = no CORS, Some(vec![]) = no origins allowed, Some(vec!["*"]) = all origins)
    pub allowed_origins: Option<Vec<String>>,
    /// Allowed methods
    pub allowed_methods: Vec<String>,
    /// Allowed headers
    pub allowed_headers: Vec<String>,
    /// Exposed headers
    pub expose_headers: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Max age for preflight requests
    pub max_age: Option<Duration>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::restrictive()
    }
}

impl CorsConfig {
    /// Permissive CORS for development (allows all origins)
    pub fn permissive() -> Self {
        Self {
            enabled: true,
            allowed_origins: Some(vec!["*".to_string()]),
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "OPTIONS".to_string()],
            allowed_headers: vec!["*".to_string()],
            expose_headers: vec![],
            allow_credentials: false, // Cannot be true with wildcard origins
            max_age: Some(Duration::from_secs(3600)),
        }
    }

    /// Restrictive CORS for staging (specific origins only)
    pub fn restrictive() -> Self {
        let allowed_origins = Self::load_cors_origins_from_env().unwrap_or_default(); // Must be configured explicitly

        Self {
            enabled: true,
            allowed_origins: Some(allowed_origins),
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "OPTIONS".to_string()],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Requested-With".to_string(),
            ],
            expose_headers: vec![],
            allow_credentials: true,
            max_age: Some(Duration::from_secs(1800)),
        }
    }

    /// Strict CORS for production (no origins allowed by default)
    pub fn strict() -> Self {
        let allowed_origins = Self::load_cors_origins_from_env().unwrap_or_default(); // Must be explicitly configured

        Self {
            enabled: true,
            allowed_origins: Some(allowed_origins),
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            expose_headers: vec![],
            allow_credentials: true,
            max_age: Some(Duration::from_secs(600)),
        }
    }

    /// Load CORS origins from environment variables
    ///
    /// Reads `CORS_ALLOWED_ORIGINS` as a comma-separated list of origins
    /// Example: `CORS_ALLOWED_ORIGINS="https://app.example.com,https://admin.example.com"`
    fn load_cors_origins_from_env() -> Option<Vec<String>> {
        std::env::var("CORS_ALLOWED_ORIGINS").ok().map(|origins| {
            origins
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
    }

    /// Disabled CORS
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            allowed_origins: None,
            allowed_methods: vec![],
            allowed_headers: vec![],
            expose_headers: vec![],
            allow_credentials: false,
            max_age: None,
        }
    }
}
