//! Security headers configuration management
//!
//! This module provides security headers configuration with environment-specific
//! presets following security best practices.

use std::time::Duration;

/// X-Frame-Options configuration
#[derive(Debug, Clone, PartialEq)]
pub enum FrameOptions {
    /// Deny all framing
    Deny,
    /// Allow framing from same origin
    SameOrigin,
    /// Allow framing from specific origin
    AllowFrom(String),
    /// Disable frame options header
    Disabled,
}

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable security headers
    pub enabled: bool,
    /// Content Security Policy
    pub content_security_policy: Option<String>,
    /// HTTP Strict Transport Security
    pub hsts_max_age: Option<Duration>,
    /// X-Frame-Options
    pub frame_options: FrameOptions,
    /// X-Content-Type-Options
    pub content_type_options: bool,
    /// Referrer-Policy
    pub referrer_policy: Option<String>,
    /// Permissions-Policy
    pub permissions_policy: Option<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self::staging()
    }
}

impl SecurityConfig {
    /// Development security (minimal headers)
    pub fn development() -> Self {
        Self {
            enabled: false, // Disabled for easier development
            content_security_policy: None,
            hsts_max_age: None,
            frame_options: FrameOptions::Disabled,
            content_type_options: false,
            referrer_policy: None,
            permissions_policy: None,
        }
    }

    /// Staging security (moderate headers)
    pub fn staging() -> Self {
        Self {
            enabled: true,
            content_security_policy: Some(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".to_string()
            ),
            hsts_max_age: Some(Duration::from_secs(31536000)), // 1 year
            frame_options: FrameOptions::SameOrigin,
            content_type_options: true,
            referrer_policy: Some("strict-origin-when-cross-origin".to_string()),
            permissions_policy: None,
        }
    }

    /// Production security (maximum headers)
    pub fn production() -> Self {
        Self {
            enabled: true,
            content_security_policy: Some(
                "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; media-src 'self'; frame-src 'none'".to_string()
            ),
            hsts_max_age: Some(Duration::from_secs(63072000)), // 2 years
            frame_options: FrameOptions::Deny,
            content_type_options: true,
            referrer_policy: Some("no-referrer".to_string()),
            permissions_policy: Some(
                "geolocation=(), microphone=(), camera=(), payment=(), usb=()".to_string()
            ),
        }
    }

    /// Disable all security headers
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            content_security_policy: None,
            hsts_max_age: None,
            frame_options: FrameOptions::Disabled,
            content_type_options: false,
            referrer_policy: None,
            permissions_policy: None,
        }
    }
}
