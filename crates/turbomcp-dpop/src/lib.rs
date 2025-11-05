//! # TurboMCP DPoP - RFC 9449 Implementation
//!
//! DPoP (Demonstrating Proof-of-Possession) implementation for OAuth 2.0 as specified in RFC 9449.
//! DPoP binds access tokens to cryptographic key pairs, preventing token theft and replay attacks.
//!
//! ## Core Features
//!
//! - ✅ **RFC 9449 Compliance** - Full specification implementation
//! - ✅ **Cryptographic Security** - ES256 (ECDSA P-256) only for maximum security
//! - ✅ **Token Binding** - Prevents stolen token usage
//! - ✅ **Replay Protection** - Nonce tracking and timestamp validation
//! - ✅ **Production Features** - HSM integration, audit logging, key rotation
//!
//! ## Security Notice
//!
//! **TurboMCP v2.2.0+** removes RSA algorithm support (RS256, PS256) to eliminate
//! timing attack vulnerabilities (RUSTSEC-2023-0071). Only ES256 (ECDSA P-256) is supported.
//! ES256 provides superior security, faster performance, and smaller key sizes.
//!
//! ## Architecture
//!
//! - `errors` - DPoP-specific error types
//! - `types` - Core DPoP types (algorithms, key pairs, proofs)
//! - `keys` - Key management and rotation
//! - `proof` - Proof generation and validation
//! - `redis_storage` - Redis backend (feature-gated: `redis-storage`)
//! - `hsm` - Hardware Security Module support (feature-gated)
//!   - `hsm::pkcs11` - PKCS#11 HSM integration (feature: `hsm-pkcs11`)
//!   - `hsm::yubihsm` - YubiHSM integration (feature: `hsm-yubico`)
//!
//! ## Feature Flags
//!
//! - `default` - Core DPoP functionality (no optional features)
//! - `redis-storage` - Redis storage backend for nonce tracking
//! - `hsm-pkcs11` - PKCS#11 HSM support
//! - `hsm-yubico` - YubiHSM support
//! - `hsm` - Enable all HSM backends
//! - `test-utils` - Test utilities for DPoP testing

// Core modules (always available when dpop feature is enabled)
pub mod errors;
pub mod helpers;
pub mod keys;
pub mod proof;
pub mod types;

// HSM support (always declared, implementations feature-gated inside)
pub mod hsm;

// Optional feature modules
#[cfg(feature = "redis-storage")]
pub mod redis_storage;

#[cfg(feature = "test-utils")]
pub mod test_utils;

// Re-export core types for convenience
pub use errors::*;
pub use keys::*;
pub use proof::*;
pub use types::*;

// Re-export builder and validator from helpers
pub use helpers::{DpopProofParams, DpopProofParamsBuilder, DpopValidator, ValidatedDpopClaims};

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
