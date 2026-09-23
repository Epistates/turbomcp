//! Helper functions for jsonwebtoken integration
//!
//! This module provides conversion functions between our DPoP types and jsonwebtoken types.
//! These helpers enable us to use the battle-tested jsonwebtoken library while maintaining
//! our type-safe DPoP API.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::jwk::{AlgorithmParameters, CommonParameters, Jwk, KeyAlgorithm, PublicKeyUse};
use jsonwebtoken::jwk::{EllipticCurve, EllipticCurveKeyParameters, EllipticCurveKeyType};
// RSA support removed in v3.0 due to RUSTSEC-2023-0071 timing vulnerability
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use p256::SecretKey;
use p256::pkcs8::EncodePrivateKey;

use crate::Result;
use crate::errors::DpopError;
use crate::types::{DpopAlgorithm, DpopPrivateKey, DpopPublicKey};

/// Convert DpopAlgorithm to jsonwebtoken Algorithm
///
/// Only ES256 is supported as of TurboMCP v3.0+
pub fn algorithm_to_jwt(algorithm: DpopAlgorithm) -> Algorithm {
    match algorithm {
        DpopAlgorithm::ES256 => Algorithm::ES256,
    }
}

/// Convert jsonwebtoken Algorithm to DpopAlgorithm
///
/// Returns error for unsupported algorithms (only ES256 allowed as of TurboMCP v3.0+)
pub fn jwt_to_algorithm(algorithm: Algorithm) -> Result<DpopAlgorithm> {
    match algorithm {
        Algorithm::ES256 => Ok(DpopAlgorithm::ES256),
        other => Err(DpopError::InvalidProofStructure {
            reason: format!(
                "Unsupported DPoP algorithm: {:?}. Only ES256 is supported (RSA removed due to RUSTSEC-2023-0071)",
                other
            ),
        }),
    }
}

/// Convert private key to jsonwebtoken EncodingKey
///
/// This handles the conversion from our DpopPrivateKey enum to jsonwebtoken's EncodingKey,
/// including necessary format conversions (SEC1 → PKCS#8 for EC keys).
///
/// Only supports ES256 (ECDSA P-256) as of TurboMCP v3.0+
///
/// # Security Note
///
/// For EC keys, we convert from SEC1 format (raw 32 bytes) to PKCS#8 DER format as required
/// by jsonwebtoken.
pub fn private_key_to_encoding_key(key: &DpopPrivateKey) -> Result<EncodingKey> {
    match key {
        DpopPrivateKey::EcdsaP256 { key_bytes } => {
            // Convert SEC1 private key bytes to p256 SecretKey
            let secret_key = SecretKey::from_bytes(key_bytes.into()).map_err(|e| {
                DpopError::CryptographicError {
                    reason: format!("Invalid EC private key: {}", e),
                }
            })?;

            // Convert to PKCS#8 DER format (required by jsonwebtoken)
            let pkcs8_der =
                secret_key
                    .to_pkcs8_der()
                    .map_err(|e| DpopError::CryptographicError {
                        reason: format!("Failed to convert EC key to PKCS#8: {}", e),
                    })?;

            // Create EncodingKey from DER bytes
            Ok(EncodingKey::from_ec_der(pkcs8_der.as_bytes()))
        }
    }
}

/// Convert public key to jsonwebtoken JWK
///
/// This creates a RFC 7517 compliant JWK from our DpopPublicKey enum.
/// The JWK will be embedded in the DPoP proof header per RFC 9449.
///
/// Only supports ES256 (ECDSA P-256) as of TurboMCP v3.0+
///
/// # Security Note
///
/// JWK coordinates are base64url-encoded per RFC 7517 Section 6.
pub fn public_key_to_jwk(key: &DpopPublicKey) -> Result<Jwk> {
    match key {
        DpopPublicKey::EcdsaP256 { x, y } => {
            // Validate coordinate lengths (P-256 uses 32 bytes)
            if x.len() != 32 || y.len() != 32 {
                return Err(DpopError::CryptographicError {
                    reason: format!("Invalid EC key coordinates: x={}, y={}", x.len(), y.len()),
                });
            }

            // Base64url encode coordinates per RFC 7517
            let x_b64 = URL_SAFE_NO_PAD.encode(x);
            let y_b64 = URL_SAFE_NO_PAD.encode(y);

            Ok(Jwk {
                common: CommonParameters {
                    public_key_use: Some(PublicKeyUse::Signature),
                    key_operations: None,
                    key_algorithm: Some(KeyAlgorithm::ES256),
                    key_id: None,
                    x509_url: None,
                    x509_chain: None,
                    x509_sha1_fingerprint: None,
                    x509_sha256_fingerprint: None,
                },
                algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                    key_type: EllipticCurveKeyType::EC,
                    curve: EllipticCurve::P256,
                    x: x_b64,
                    y: y_b64,
                }),
            })
        }
    }
}

/// Convert JWK to jsonwebtoken DecodingKey
///
/// This extracts the public key from a JWK and creates a DecodingKey for signature verification.
/// Used during DPoP proof validation to verify the signature using the embedded public key.
///
/// Only supports ES256 (ECDSA P-256) as of TurboMCP v3.0+
///
/// # Security Note
///
/// This function validates key parameters and only supports P-256 for EC keys.
pub fn jwk_to_decoding_key(jwk: &Jwk) -> Result<DecodingKey> {
    match &jwk.algorithm {
        AlgorithmParameters::EllipticCurve(ec_params) => {
            // Validate curve (only P-256 supported for DPoP)
            if ec_params.curve != EllipticCurve::P256 {
                return Err(DpopError::InvalidProofStructure {
                    reason: format!(
                        "Unsupported elliptic curve: {:?} (only P-256 supported)",
                        ec_params.curve
                    ),
                });
            }

            // Use jsonwebtoken's from_ec_components which accepts base64url-encoded strings directly
            // This is the same approach used in the working from_jwt_string() method
            DecodingKey::from_ec_components(&ec_params.x, &ec_params.y).map_err(|e| {
                DpopError::InvalidProofStructure {
                    reason: format!("Failed to create EC decoding key: {}", e),
                }
            })
        }
        other => Err(DpopError::InvalidProofStructure {
            reason: format!(
                "Unsupported JWK algorithm parameters: {:?}. Only ES256 (ECDSA P-256) is supported (RSA removed due to RUSTSEC-2023-0071)",
                other
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_conversion() {
        // Only ES256 is supported
        assert_eq!(algorithm_to_jwt(DpopAlgorithm::ES256), Algorithm::ES256);

        assert_eq!(
            jwt_to_algorithm(Algorithm::ES256).unwrap(),
            DpopAlgorithm::ES256
        );

        // All other algorithms should error (including RSA variants)
        assert!(jwt_to_algorithm(Algorithm::RS256).is_err());
        assert!(jwt_to_algorithm(Algorithm::PS256).is_err());
        assert!(jwt_to_algorithm(Algorithm::HS256).is_err());
        assert!(jwt_to_algorithm(Algorithm::HS384).is_err());
    }

    #[test]
    fn test_ec_key_coordinate_validation() {
        // Valid EC key should work
        let valid_key = DpopPublicKey::EcdsaP256 {
            x: [0u8; 32],
            y: [1u8; 32],
        };

        assert!(public_key_to_jwk(&valid_key).is_ok());
    }
}

// Builder pattern for DPoP proof generation
//
// Provides an ergonomic, compile-time checked builder API using the bon crate.

use bon::Builder;

/// Builder parameters for DPoP proof generation
///
/// This struct uses the bon builder pattern to provide compile-time checks
/// and an ergonomic API for creating DPoP proofs.
#[derive(Builder)]
#[builder(on(String, into))]
pub struct DpopProofParams {
    /// HTTP method (e.g., "GET", "POST")
    http_method: String,

    /// HTTP URI (without query/fragment)
    http_uri: String,

    /// Optional access token for binding (ath claim)
    access_token: Option<String>,

    /// Optional server-provided nonce for replay prevention
    nonce: Option<String>,
}

impl DpopProofParams {
    /// Build the DPoP proof with the given key pair
    ///
    /// This method takes ownership of the builder and generates a proof
    /// using the provided key pair and parameters.
    ///
    /// # Errors
    /// Returns error if proof generation fails
    pub async fn build_with_key(
        self,
        key_pair: &crate::types::DpopKeyPair,
    ) -> crate::Result<crate::types::DpopProof> {
        // Use the existing proof generator
        let generator = crate::proof::DpopProofGenerator::new_simple().await?;
        generator
            .generate_proof_with_params(
                &self.http_method,
                &self.http_uri,
                self.access_token.as_deref(),
                self.nonce.as_deref(),
                Some(key_pair),
            )
            .await
    }
}

// `DpopValidator` / `ValidatedDpopClaims` used to live here as a second,
// lighter-weight validator. It never verified the JWT signature or bound
// `htm`/`htu` to the actual request — it only checked header shape, that
// `iat` was recent, and (optionally) the `ath` claim. That made it an unsafe
// public API: calling code that reached for "the DPoP validator" by name
// would get something that accepts a forged proof with an unrelated method
// and URI, signed by nobody. `DpopProofGenerator::validate_proof` (in
// `proof.rs`) is the real, spec-complete validator — it checks structure,
// HTTP binding, timestamps, replay (nonce tracking), `ath` binding per
// `ProofContext`, and the cryptographic signature. Nothing in this workspace
// called `DpopValidator` outside its own tests, so it was removed rather than
// turned into a wrapper: a thin shim over `validate_proof` would still need a
// key manager and nonce tracker to construct, making it no lighter-weight
// than calling `DpopProofGenerator` directly, just a second name for the same
// thing. Use `DpopProofGenerator::validate_proof` (or `parse_and_validate_jwt`
// for a raw JWT string) instead.
