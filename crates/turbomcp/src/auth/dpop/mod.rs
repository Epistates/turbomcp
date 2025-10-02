//! DPoP (Demonstrating Proof-of-Possession) Implementation
//!
//! This module implements RFC 9449 - OAuth 2.0 Demonstrating Proof-of-Possession at the
//! Application Layer (DPoP). DPoP binds access tokens to cryptographic key pairs, preventing
//! token theft and replay attacks.
//!
//! **This module is feature-gated and only available with the `dpop` feature.**
//!
//! ## Core Features
//!
//! - ✅ **RFC 9449 Compliance** - Full specification implementation
//! - ✅ **Cryptographic Security** - RSA, ECDSA P-256, and PSS support
//! - ✅ **Token Binding** - Prevents stolen token usage
//! - ✅ **Replay Protection** - Nonce tracking and timestamp validation
//! - ✅ **Enterprise Ready** - HSM integration, audit logging, key rotation
//!
//! ## Architecture
//!
//! - `errors` - DPoP-specific error types
//! - `types` - Core DPoP types (algorithms, key pairs, proofs)
//! - `keys` - Key management and rotation
//! - `proof` - Proof generation and validation
//! - `redis_storage` - Redis backend (feature-gated: `dpop-redis`)
//! - `hsm` - Hardware Security Module support (feature-gated)
//!   - `hsm::pkcs11` - PKCS#11 HSM integration (feature: `dpop-hsm-pkcs11`)
//!   - `hsm::yubihsm` - YubiHSM integration (feature: `dpop-hsm-yubico`)
//!
//! ## Feature Flags
//!
//! - `dpop` - Core DPoP functionality (required for this module)
//! - `dpop-redis` - Redis storage backend for nonce tracking
//! - `dpop-hsm-pkcs11` - PKCS#11 HSM support
//! - `dpop-hsm-yubico` - YubiHSM support
//! - `dpop-test-utils` - Test utilities for DPoP testing

#![cfg(feature = "dpop")]

// Core modules (always available when dpop feature is enabled)
pub mod errors;
pub mod keys;
pub mod proof;
pub mod types;

// HSM support (always declared, implementations feature-gated inside)
pub mod hsm;

// Optional feature modules
#[cfg(feature = "dpop-redis")]
pub mod redis_storage;

#[cfg(feature = "dpop-test-utils")]
pub mod test_utils;

// Re-export core types for convenience
pub use errors::*;
pub use keys::*;
pub use proof::*;
pub use types::*;

/// DPoP result type
pub type Result<T> = std::result::Result<T, DpopError>;

/// DPoP JWT header type as defined in RFC 9449
pub const DPOP_JWT_TYPE: &str = "dpop+jwt";

/// Maximum clock skew tolerance (5 minutes)
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

/// Default proof lifetime (60 seconds)
pub const DEFAULT_PROOF_LIFETIME_SECONDS: u64 = 60;

/// Maximum proof lifetime (5 minutes)
pub const MAX_PROOF_LIFETIME_SECONDS: u64 = 300;
