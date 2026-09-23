//! # TurboMCP DPoP - RFC 9449 Implementation
//!
//! DPoP (Demonstrating Proof-of-Possession) implementation for OAuth 2.0 as specified in RFC 9449.

// Allow missing error/panic docs - this crate has extensive error types that are self-documenting
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
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
//! **TurboMCP v3.0+** removes RSA algorithm support (RS256, PS256) to eliminate
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

// Re-export builder from helpers. (`DpopValidator`/`ValidatedDpopClaims` were
// removed — they verified neither the signature nor the HTTP binding; use
// `proof::DpopProofGenerator::validate_proof` instead.)
pub use helpers::{DpopProofParams, DpopProofParamsBuilder};

/// DPoP result type
pub type Result<T> = std::result::Result<T, DpopError>;

/// DPoP JWT header type as defined in RFC 9449
pub const DPOP_JWT_TYPE: &str = "dpop+jwt";

/// Hard cap on clock skew accepted for DPoP proofs.
///
/// 5 minutes matches the upper bound RFC 9449 §11.1 considers acceptable; this
/// is the *maximum* a DPoP validator may use, not the recommended default.
/// Most deployments should use the smaller [`DEFAULT_CLOCK_SKEW_SECONDS`].
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

/// Default clock skew tolerance for DPoP proof validation (60 seconds).
///
/// RFC 9449 §11.1 recommends a short tolerance window so an exfiltrated proof
/// is replayable for as little time as possible. 60 seconds matches
/// [`DEFAULT_PROOF_LIFETIME_SECONDS`], so the effective acceptance window is
/// `lifetime + skew = 120s`. The previous default was [`MAX_CLOCK_SKEW_SECONDS`]
/// (300s) which made the effective window 6 minutes — needlessly permissive.
pub const DEFAULT_CLOCK_SKEW_SECONDS: i64 = 60;

/// Default proof lifetime (60 seconds)
pub const DEFAULT_PROOF_LIFETIME_SECONDS: u64 = 60;

/// Maximum proof lifetime (5 minutes)
pub const MAX_PROOF_LIFETIME_SECONDS: u64 = 300;
