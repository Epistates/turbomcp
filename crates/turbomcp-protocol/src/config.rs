//! Configuration types and utilities for MCP core
//!
//! This module provides configuration management for `TurboMCP` applications
//! with builder pattern support and validation.
//!
//! # Examples
//!
//! ## Creating a default configuration
//!
//! ```
//! # #[cfg(feature = "fancy-errors")]
//! # {
//! use turbomcp_protocol::config::CoreConfig;
//!
//! let config = CoreConfig::default();
//! assert_eq!(config.max_message_size, 64 * 1024 * 1024);
//! assert_eq!(config.timeout_ms, 30_000);
//! assert!(config.tracing_enabled);
//! # }
//! ```
//!
//! ## Using the configuration builder
//!
//! ```
//! # #[cfg(feature = "fancy-errors")]
//! # {
//! use turbomcp_protocol::config::ConfigBuilder;
//!
//! let config = ConfigBuilder::new()
//!     .max_message_size(1024 * 1024).unwrap() // 1MB
//!     .timeout_ms(10_000).unwrap() // 10 seconds
//!     .tracing_enabled(false)
//!     .option("env", "production").unwrap()
//!     .build();
//!
//! assert_eq!(config.max_message_size, 1024 * 1024);
//! assert_eq!(config.timeout_ms, 10_000);
//! assert!(!config.tracing_enabled);
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core configuration for MCP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Timeout for operations in milliseconds
    pub timeout_ms: u64,
    /// Enable tracing
    pub tracing_enabled: bool,
    /// Additional configuration options
    pub options: HashMap<String, serde_json::Value>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            max_message_size: 64 * 1024 * 1024, // 64MB
            timeout_ms: 30_000,                 // 30 seconds
            tracing_enabled: true,
            options: HashMap::new(),
        }
    }
}

/// Configuration builder for core settings
#[derive(Debug)]
pub struct ConfigBuilder {
    config: CoreConfig,
}

impl ConfigBuilder {
    /// Create a new configuration builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: CoreConfig::default(),
        }
    }

    /// Set maximum message size
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "fancy-errors")]
    /// # {
    /// use turbomcp_protocol::config::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .max_message_size(1024 * 1024).unwrap()
    ///     .build();
    /// assert_eq!(config.max_message_size, 1024 * 1024);
    ///
    /// // Validation errors
    /// assert!(ConfigBuilder::new().max_message_size(0).is_err());
    /// assert!(ConfigBuilder::new().max_message_size(2 * 1024 * 1024 * 1024).is_err());
    /// # }
    /// ```
    pub fn max_message_size(mut self, size: usize) -> Result<Self, String> {
        if size == 0 {
            return Err("Maximum message size cannot be zero".to_string());
        }
        if size > 1024 * 1024 * 1024 {
            // 1GB limit
            return Err("Maximum message size cannot exceed 1GB".to_string());
        }
        self.config.max_message_size = size;
        Ok(self)
    }

    /// Set operation timeout
    pub fn timeout_ms(mut self, timeout: u64) -> Result<Self, String> {
        if timeout == 0 {
            return Err("Timeout cannot be zero".to_string());
        }
        if timeout > 10 * 60 * 1000 {
            // 10 minutes max
            return Err("Timeout cannot exceed 10 minutes".to_string());
        }
        self.config.timeout_ms = timeout;
        Ok(self)
    }

    /// Enable or disable tracing
    #[must_use]
    pub const fn tracing_enabled(mut self, enabled: bool) -> Self {
        self.config.tracing_enabled = enabled;
        self
    }

    /// Add a configuration option
    pub fn option<V: serde::Serialize>(
        mut self,
        key: &str,
        value: V,
    ) -> Result<Self, serde_json::Error> {
        let json_value = serde_json::to_value(value)?;
        self.config.options.insert(key.to_string(), json_value);
        Ok(self)
    }

    /// Build the configuration
    #[must_use]
    pub fn build(self) -> CoreConfig {
        self.config
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
