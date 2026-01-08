//! # TurboMCP Compatibility Layer
//!
//! This crate provides backward compatibility types for migrating from
//! TurboMCP v2.x to v3.x.
//!
//! All types in this crate are **deprecated** and will guide you to the
//! correct v3.x alternatives via compiler warnings.
//!
//! ## Usage
//!
//! ```rust
//! use turbomcp_compat::v2::*;
//!
//! // Deprecation warnings will guide migration
//! ```
//!
//! ## Migration Timeline
//!
//! - **v3.0.0**: All types deprecated with migration guidance
//! - **v3.1.0**: Warnings become errors
//! - **v4.0.0**: This crate removed

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Version 2.x compatibility types
///
/// This module provides type aliases and shims for v2.x code.
/// All types are deprecated with migration guidance.
pub mod v2 {
    // Re-export the actual types first
    pub use turbomcp_core::error::{ErrorKind, McpError};

    // =========================================================================
    // Error Type Aliases
    // =========================================================================

    /// Deprecated: Use `McpError` instead.
    ///
    /// # Migration
    ///
    /// ```rust,ignore
    /// // Before (v2.x)
    /// use turbomcp_server::ServerError;
    ///
    /// // After (v3.x)
    /// use turbomcp::McpError;
    /// ```
    #[deprecated(
        since = "3.0.0",
        note = "Use `turbomcp::McpError` instead. ServerError has been unified into McpError."
    )]
    pub type ServerError = McpError;

    /// Deprecated: Use `McpResult<T>` instead.
    ///
    /// # Migration
    ///
    /// ```rust,ignore
    /// // Before (v2.x)
    /// use turbomcp_server::ServerResult;
    /// fn my_handler() -> ServerResult<Value> { ... }
    ///
    /// // After (v3.x)
    /// use turbomcp::McpResult;
    /// fn my_handler() -> McpResult<Value> { ... }
    /// ```
    #[deprecated(
        since = "3.0.0",
        note = "Use `turbomcp::McpResult<T>` instead. ServerResult has been unified into McpResult."
    )]
    pub type ServerResult<T> = Result<T, McpError>;

    /// Deprecated: Use `McpError` instead.
    ///
    /// The protocol-level `Error` type has been consolidated into `McpError`.
    #[deprecated(
        since = "3.0.0",
        note = "Use `turbomcp::McpError` instead. Protocol Error has been unified into McpError."
    )]
    pub type Error = McpError;

    // =========================================================================
    // Error Kind Mapping Helpers
    // =========================================================================

    /// Helper to create errors with v2.x-style constructors.
    ///
    /// This provides a migration path for code using v2.x error patterns.
    #[deprecated(
        since = "3.0.0",
        note = "Use McpError constructors directly: McpError::internal(), McpError::invalid_params(), etc."
    )]
    pub mod error_compat {
        use turbomcp_core::error::{ErrorKind, McpError};

        /// Create a handler error (was ServerError::Handler in v2.x)
        ///
        /// Maps to `McpError::new(ErrorKind::Internal, msg)` in v3.x
        #[deprecated(since = "3.0.0", note = "Use McpError::internal() instead")]
        pub fn handler_error(message: impl Into<String>) -> McpError {
            McpError::new(ErrorKind::Internal, message)
        }

        /// Create an internal error
        #[deprecated(since = "3.0.0", note = "Use McpError::internal() instead")]
        pub fn internal_error(message: impl Into<String>) -> McpError {
            McpError::new(ErrorKind::Internal, message)
        }

        /// Create a transport error
        #[deprecated(
            since = "3.0.0",
            note = "Use McpError::new(ErrorKind::Transport, msg) instead"
        )]
        pub fn transport_error(message: impl Into<String>) -> McpError {
            McpError::new(ErrorKind::Transport, message)
        }

        /// Create a timeout error
        #[deprecated(
            since = "3.0.0",
            note = "Use McpError::new(ErrorKind::Timeout, msg) instead"
        )]
        pub fn timeout_error(message: impl Into<String>) -> McpError {
            McpError::new(ErrorKind::Timeout, message)
        }
    }

    // =========================================================================
    // Server Compatibility (feature-gated)
    // =========================================================================

    #[cfg(feature = "server")]
    #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
    pub mod server {
        //! Server compatibility types.
        //!
        //! These types require the `server` feature.

        use serde::{Deserialize, Serialize};

        /// Deprecated: Use `turbomcp_auth::AuthContext` instead.
        ///
        /// This type provides backward compatibility for JWT claims parsing.
        ///
        /// # Migration
        ///
        /// ```rust,ignore
        /// // Before (v2.x)
        /// use turbomcp_server::middleware::Claims;
        ///
        /// // After (v3.x)
        /// use turbomcp_auth::AuthContext;
        /// ```
        #[deprecated(
            since = "3.0.0",
            note = "Use `turbomcp_auth::AuthContext` instead. Claims will be removed in v4.0.0."
        )]
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct Claims {
            /// Subject (user ID)
            pub sub: String,
            /// Expiration time (Unix timestamp)
            pub exp: i64,
            /// Issued at (Unix timestamp)
            #[serde(default)]
            pub iat: i64,
            /// Issuer
            #[serde(default)]
            pub iss: Option<String>,
            /// Audience
            #[serde(default)]
            pub aud: Option<String>,
        }

        #[allow(deprecated)]
        impl Claims {
            /// Check if the token is expired
            pub fn is_expired(&self) -> bool {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                self.exp < now
            }

            /// Get the subject (user ID)
            pub fn subject(&self) -> &str {
                &self.sub
            }
        }
    }
}

/// Prelude module for common imports
pub mod prelude {
    #[allow(deprecated)]
    pub use crate::v2::*;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(deprecated)]
    fn test_server_error_alias() {
        let err: v2::ServerError = v2::McpError::internal("test error");
        assert_eq!(err.kind, v2::ErrorKind::Internal);
    }

    #[test]
    #[allow(deprecated)]
    fn test_server_result_alias() {
        fn returns_result() -> v2::ServerResult<i32> {
            Ok(42)
        }
        let result = returns_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    #[allow(deprecated)]
    fn test_error_compat_helpers() {
        let err = v2::error_compat::handler_error("test");
        assert_eq!(err.kind, v2::ErrorKind::Internal);
    }

    #[cfg(feature = "server")]
    #[test]
    #[allow(deprecated)]
    fn test_claims_compat() {
        let claims = v2::server::Claims {
            sub: "user123".to_string(),
            exp: i64::MAX,
            iat: 0,
            iss: Some("test".to_string()),
            aud: None,
        };
        assert!(!claims.is_expired());
        assert_eq!(claims.subject(), "user123");
    }
}
