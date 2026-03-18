//! JWT Infrastructure - Shared JWT validation and signing for TurboMCP
//!
//! This module provides a unified JWT handling layer used by both:
//! - Bearer token validation (MCP servers)
//! - DPoP proof generation/validation (RFC 9449)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │   JWT Infrastructure (Foundation)   │
//! │  - JWKS fetching & caching          │
//! │  - Algorithm support                │
//! │  - Validation (aud/iss/exp/nbf)     │
//! │  - Signing support                  │
//! └─────────────────────────────────────┘
//!          ▲                  ▲
//!          │                  │
//!   ┌──────┴──────┐    ┌─────┴──────┐
//!   │  Bearer     │    │    DPoP    │
//!   │ Validation  │    │   Proofs   │
//!   └─────────────┘    └────────────┘
//! ```
//!
//! # Design Principles
//!
//! - **Industry Standard**: Uses `jsonwebtoken` crate (9.3M downloads)
//! - **Security First**: JWKS caching with TTL, clock skew tolerance
//! - **MCP Compliant**: Audience validation, issuer validation
//! - **Production Ready**: Comprehensive error handling, observability
//!
//! # Modules
//!
//! - `validator` - JWT validation with JWKS support
//! - `signer` - JWT signing (for DPoP, service tokens)
//! - `jwks` - JWKS fetching and caching
//! - `claims` - Common JWT claims handling

pub mod jwks;
pub mod validator;

// Re-export commonly used types
pub use jwks::{JwksCache, JwksClient};
pub use validator::{JwtValidationResult, JwtValidator};

use serde::{Deserialize, Serialize};
use serde_with::{OneOrMany, formats::PreferOne, serde_as};
use std::collections::HashMap;

/// Standard JWT claims per RFC 7519
///
/// This struct represents the registered claims defined in RFC 7519 Section 4.1.
/// Additional claims can be stored in the `additional` field.
///
/// The `aud` field accepts both a single string and an array of strings per
/// RFC 7519 §4.1.3, using `serde_with::OneOrMany` to handle both formats.
/// Enterprise IdPs (Google, Azure, Okta) commonly serialize `aud` as an array.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StandardClaims {
    /// Issuer (iss) - identifies who issued the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Subject (sub) - identifies the principal (user ID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Audience (aud) - identifies the recipients (RFC 7519 §4.1.3)
    ///
    /// Accepts both `"aud": "single"` and `"aud": ["one", "two"]` formats.
    /// Always deserialized as `Vec<String>` for uniform handling.
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub aud: Option<Vec<String>>,

    /// Expiration Time (exp) - Unix timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,

    /// Not Before (nbf) - Unix timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,

    /// Issued At (iat) - Unix timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,

    /// JWT ID (jti) - unique identifier for replay prevention
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Additional claims not in RFC 7519
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}
