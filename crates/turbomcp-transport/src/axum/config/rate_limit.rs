//! Rate limiting configuration management
//!
//! This module provides rate limiting configuration with different strategies
//! and environment-specific presets.

/// Rate limiting key strategies
#[derive(Debug, Clone)]
pub enum RateLimitKey {
    /// Rate limit by IP address
    IpAddress,
    /// Rate limit by authenticated user ID
    UserId,
    /// Custom key extraction
    Custom,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Enable rate limiting
    pub enabled: bool,
    /// Requests per minute per IP
    pub requests_per_minute: u32,
    /// Burst capacity
    pub burst_capacity: u32,
    /// Key function (IP, User, Custom)
    pub key_function: RateLimitKey,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::moderate()
    }
}

impl RateLimitConfig {
    /// Disabled rate limiting
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 0,
            burst_capacity: 0,
            key_function: RateLimitKey::IpAddress,
        }
    }

    /// Moderate rate limiting for staging
    pub fn moderate() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 300, // 5 requests per second
            burst_capacity: 50,
            key_function: RateLimitKey::IpAddress,
        }
    }

    /// Strict rate limiting for production
    pub fn strict() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 120, // 2 requests per second
            burst_capacity: 20,
            key_function: RateLimitKey::IpAddress,
        }
    }

    /// Create custom rate limiting configuration
    pub fn custom(
        requests_per_minute: u32,
        burst_capacity: u32,
        key_function: RateLimitKey,
    ) -> Self {
        Self {
            enabled: true,
            requests_per_minute,
            burst_capacity,
            key_function,
        }
    }
}
