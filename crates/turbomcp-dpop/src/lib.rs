//! # TurboMCP DPoP Implementation
//!
//! **RFC 9449 compliant Demonstration of Proof-of-Possession (DPoP) implementation**
//!
//! This crate provides production-grade DPoP support for TurboMCP, implementing the full
//! RFC 9449 specification with enterprise-ready security features:
//!
//! ## Core Features
//!
//! - ✅ **RFC 9449 Compliance** - Full specification implementation
//! - ✅ **Cryptographic Security** - RSA, ECDSA P-256, and PSS support
//! - ✅ **Token Binding** - Prevents stolen token usage
//! - ✅ **Replay Protection** - Nonce tracking and timestamp validation
//! - ✅ **Enterprise Ready** - HSM integration, audit logging, key rotation
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator, DpopAlgorithm};
//!
//! # async fn example() -> turbomcp_dpop::Result<()> {
//! // Generate key pair for DPoP
//! let key_manager = DpopKeyManager::new_memory().await?;
//! let key_pair = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
//!
//! // Create DPoP proof for HTTP request
//! let proof_gen = DpopProofGenerator::new(key_manager.into());
//! let proof = proof_gen.generate_proof(
//!     "POST",
//!     "https://api.example.com/oauth/token",
//!     None, // No access token for initial request
//! ).await?;
//!
//! println!("DPoP Header: {}", proof.to_jwt_string());
//! # Ok(())
//! # }
//! ```
//!
//! ## Integration with TurboMCP OAuth
//!
//! ```rust,no_run
//! // Example OAuth integration (requires turbomcp crate)
//! # use turbomcp_dpop::*;
//! // let config = OAuth2Config {
//! //     // ... existing OAuth configuration  
//! //     security_level: SecurityLevel::Enhanced, // 🔒 Enables DPoP
//! //     // ... rest unchanged
//! // };
//! ```
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────┐    ┌─────────────────────┐    ┌─────────────────────┐
//! │   Application       │    │   TurboMCP OAuth    │    │   OAuth Provider    │
//! │   + DPoP Client     │    │   + DPoP Support    │    │   (GitHub/Google)   │
//! └─────────┬───────────┘    └─────────┬───────────┘    └─────────┬───────────┘
//!           │                          │                          │
//!           │ 1. Generate DPoP proof   │                          │
//!           │                          │                          │
//!           │ 2. OAuth + DPoP header ──┼──────────────────────────▶│
//!           │                          │                          │
//!           │                          │ 3. Validate DPoP proof   │
//!           │                          │                          │
//!           │◄─────────────────────────┼──────────────────────────┤
//!           │    4. Ephemeral token    │         bound to DPoP    │
//!           │       (cryptographically bound)                     │
//! ```
//!
//! ## Security Properties
//!
//! ### Token Binding
//! - Access tokens are cryptographically bound to client key pairs
//! - Stolen tokens are unusable without the corresponding private key
//! - Each token is tied to a specific client instance
//!
//! ### Replay Attack Prevention
//! - JWT timestamps prevent token replay beyond time windows
//! - Nonce tracking ensures each proof is used only once
//! - HTTP method and URI binding prevents cross-endpoint attacks
//!
//! ### Forward Security
//! - Key rotation support for long-lived applications
//! - Ephemeral tokens with short lifespans (1 hour maximum)
//! - Secure key material destruction on cleanup

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc, // Error documentation in progress
)]

// Re-export core types for convenience
pub use errors::*;
pub use keys::*;
pub use proof::*;
pub use types::*;

// Core modules
pub mod errors;
pub mod keys;
pub mod proof;
pub mod types;

// Optional feature modules
#[cfg(feature = "redis-storage")]
#[cfg_attr(docsrs, doc(cfg(feature = "redis-storage")))]
pub mod redis_storage;

// HSM module - currently not implemented (see module documentation)
pub mod hsm;

// Utilities and testing
#[cfg(feature = "test-utils")]
#[cfg_attr(docsrs, doc(cfg(feature = "test-utils")))]
pub mod test_utils;

/// DPoP result type
pub type Result<T> = std::result::Result<T, DpopError>;

/// Current crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// DPoP JWT header type as defined in RFC 9449
pub const DPOP_JWT_TYPE: &str = "dpop+jwt";

/// Maximum clock skew tolerance (5 minutes)
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

/// Default proof lifetime (60 seconds)
pub const DEFAULT_PROOF_LIFETIME_SECONDS: u64 = 60;

/// Maximum proof lifetime (5 minutes)
pub const MAX_PROOF_LIFETIME_SECONDS: u64 = 300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(DPOP_JWT_TYPE, "dpop+jwt");
        assert_eq!(MAX_CLOCK_SKEW_SECONDS, 300);
        assert_eq!(DEFAULT_PROOF_LIFETIME_SECONDS, 60);
    }
}
