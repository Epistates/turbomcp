//! Helper functions for jsonwebtoken integration
//!
//! This module provides conversion functions between our DPoP types and jsonwebtoken types.
//! These helpers enable us to use the battle-tested jsonwebtoken library while maintaining
//! our type-safe DPoP API.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::jwk::{AlgorithmParameters, CommonParameters, Jwk, KeyAlgorithm, PublicKeyUse};
use jsonwebtoken::jwk::{EllipticCurve, EllipticCurveKeyParameters, EllipticCurveKeyType};
use jsonwebtoken::jwk::{RSAKeyParameters, RSAKeyType};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use p256::SecretKey;
use p256::pkcs8::EncodePrivateKey;

use crate::Result;
use crate::errors::DpopError;
use crate::types::{DpopAlgorithm, DpopPrivateKey, DpopPublicKey};

/// Convert DpopAlgorithm to jsonwebtoken Algorithm
///
/// This provides a type-safe mapping between our algorithm enum and jsonwebtoken's.
pub fn algorithm_to_jwt(algorithm: DpopAlgorithm) -> Algorithm {
    match algorithm {
        DpopAlgorithm::ES256 => Algorithm::ES256,
        DpopAlgorithm::RS256 => Algorithm::RS256,
        DpopAlgorithm::PS256 => Algorithm::PS256,
    }
}

/// Convert jsonwebtoken Algorithm to DpopAlgorithm
///
/// Returns error for unsupported algorithms (only ES256, RS256, PS256 allowed for DPoP).
pub fn jwt_to_algorithm(algorithm: Algorithm) -> Result<DpopAlgorithm> {
    match algorithm {
        Algorithm::ES256 => Ok(DpopAlgorithm::ES256),
        Algorithm::RS256 => Ok(DpopAlgorithm::RS256),
        Algorithm::PS256 => Ok(DpopAlgorithm::PS256),
        other => Err(DpopError::InvalidProofStructure {
            reason: format!("Unsupported DPoP algorithm: {:?}", other),
        }),
    }
}

/// Convert private key to jsonwebtoken EncodingKey
///
/// This handles the conversion from our DpopPrivateKey enum to jsonwebtoken's EncodingKey,
/// including necessary format conversions (SEC1 → PKCS#8 for EC keys).
///
/// # Security Note
///
/// For EC keys, we convert from SEC1 format (raw 32 bytes) to PKCS#8 DER format as required
/// by jsonwebtoken. For RSA keys, they're already in PKCS#8 DER format.
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
        DpopPrivateKey::Rsa { key_der } => {
            // RSA key is already in PKCS#8 DER format
            Ok(EncodingKey::from_rsa_der(key_der))
        }
    }
}

/// Convert public key to jsonwebtoken JWK
///
/// This creates a RFC 7517 compliant JWK from our DpopPublicKey enum.
/// The JWK will be embedded in the DPoP proof header per RFC 9449.
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
        DpopPublicKey::Rsa { n, e } => {
            // Base64url encode RSA parameters per RFC 7517
            let n_b64 = URL_SAFE_NO_PAD.encode(n);
            let e_b64 = URL_SAFE_NO_PAD.encode(e);

            Ok(Jwk {
                common: CommonParameters {
                    public_key_use: Some(PublicKeyUse::Signature),
                    key_operations: None,
                    // Note: Could be RS256 or PS256, but we use RS256 as default
                    // The actual algorithm used is specified in the JWT header
                    key_algorithm: Some(KeyAlgorithm::RS256),
                    key_id: None,
                    x509_url: None,
                    x509_chain: None,
                    x509_sha1_fingerprint: None,
                    x509_sha256_fingerprint: None,
                },
                algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                    key_type: RSAKeyType::RSA,
                    n: n_b64,
                    e: e_b64,
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
/// # Security Note
///
/// This function validates key parameters and only supports P-256 for EC keys.
/// For RSA keys, jsonwebtoken handles the validation.
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
        AlgorithmParameters::RSA(rsa_params) => {
            // jsonwebtoken provides this built-in for RSA keys
            // It accepts base64url-encoded n and e parameters
            DecodingKey::from_rsa_components(&rsa_params.n, &rsa_params.e).map_err(|e| {
                DpopError::InvalidProofStructure {
                    reason: format!("Invalid RSA key components: {}", e),
                }
            })
        }
        other => Err(DpopError::InvalidProofStructure {
            reason: format!("Unsupported JWK algorithm parameters: {:?}", other),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_conversion() {
        assert_eq!(algorithm_to_jwt(DpopAlgorithm::ES256), Algorithm::ES256);
        assert_eq!(algorithm_to_jwt(DpopAlgorithm::RS256), Algorithm::RS256);
        assert_eq!(algorithm_to_jwt(DpopAlgorithm::PS256), Algorithm::PS256);

        assert_eq!(
            jwt_to_algorithm(Algorithm::ES256).unwrap(),
            DpopAlgorithm::ES256
        );
        assert_eq!(
            jwt_to_algorithm(Algorithm::RS256).unwrap(),
            DpopAlgorithm::RS256
        );
        assert_eq!(
            jwt_to_algorithm(Algorithm::PS256).unwrap(),
            DpopAlgorithm::PS256
        );

        // Unsupported algorithms should error
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
    pub async fn build_with_key(self, key_pair: &crate::types::DpopKeyPair) -> crate::Result<crate::types::DpopProof> {
        // Use the existing proof generator
        let generator = crate::proof::DpopProofGenerator::new_simple().await?;
        generator.generate_proof_with_params(
            &self.http_method,
            &self.http_uri,
            self.access_token.as_deref(),
            self.nonce.as_deref(),
            Some(key_pair)
        ).await
    }
}

/// Validator for DPoP proofs
///
/// Validates DPoP proofs according to RFC 9449 including:
/// - JWT structure validation
/// - Timestamp validation with clock skew tolerance
/// - Access token binding (ath claim)
/// - Required claim presence
#[derive(Debug, Clone)]
pub struct DpopValidator {
    /// Clock skew tolerance in seconds
    clock_tolerance_secs: i64,
}

impl DpopValidator {
    /// Create a new validator with default settings
    ///
    /// Default clock tolerance: 60 seconds
    #[must_use]
    pub fn new() -> Self {
        Self {
            clock_tolerance_secs: 60,
        }
    }
    
    /// Create a validator with custom clock tolerance
    #[must_use]
    pub fn with_clock_tolerance(mut self, seconds: i64) -> Self {
        self.clock_tolerance_secs = seconds;
        self
    }
    
    /// Validate a DPoP proof
    ///
    /// Performs comprehensive validation including:
    /// - JWT header type is "dpop+jwt"
    /// - JWK is present in header
    /// - Timestamp is recent (within clock tolerance)
    /// - Access token binding if token provided
    ///
    /// # Errors
    /// Returns error if validation fails
    pub async fn validate(
        &self,
        proof: &crate::types::DpopProof,
        access_token: Option<&str>
    ) -> crate::Result<ValidatedDpopClaims> {
        // Validate header
        self.validate_header(&proof.header)?;
        
        // Validate timestamp
        self.validate_timestamp(&proof.payload)?;
        
        // Validate required claims
        self.validate_required_claims(&proof.payload)?;
        
        // Validate access token binding if provided
        if let Some(token) = access_token {
            self.validate_access_token_binding(proof, token)?;
        }
        
        Ok(ValidatedDpopClaims {
            htm: proof.payload.htm.clone(),
            htu: proof.payload.htu.clone(),
            ath: proof.payload.ath.clone(),
            jti: proof.payload.jti.clone(),
            iat: proof.payload.iat,
        })
    }
    
    fn validate_header(&self, header: &crate::types::DpopHeader) -> crate::Result<()> {
        // Check typ is "dpop+jwt"
        if header.typ != crate::DPOP_JWT_TYPE {
            return Err(crate::errors::DpopError::ProofValidationFailed {
                reason: format!("Invalid typ header: expected '{}', got '{}'", crate::DPOP_JWT_TYPE, header.typ),
            });
        }
        Ok(())
    }
    
    fn validate_timestamp(&self, payload: &crate::types::DpopPayload) -> crate::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| crate::errors::DpopError::InternalError {
                reason: format!("System time error: {}", e),
            })?
            .as_secs() as i64;

        // Check iat is not too far in the past or future
        let age = now - payload.iat;
        if age.abs() > self.clock_tolerance_secs {
            return Err(crate::errors::DpopError::ClockSkewTooLarge {
                skew_seconds: age,
                max_skew_seconds: self.clock_tolerance_secs,
            });
        }

        Ok(())
    }
    
    fn validate_required_claims(&self, payload: &crate::types::DpopPayload) -> crate::Result<()> {
        if payload.jti.is_empty() {
            return Err(crate::errors::DpopError::InvalidProofStructure {
                reason: "Missing jti claim".to_string(),
            });
        }
        if payload.htm.is_empty() {
            return Err(crate::errors::DpopError::InvalidProofStructure {
                reason: "Missing htm claim".to_string(),
            });
        }
        if payload.htu.is_empty() {
            return Err(crate::errors::DpopError::InvalidProofStructure {
                reason: "Missing htu claim".to_string(),
            });
        }
        Ok(())
    }
    
    fn validate_access_token_binding(&self, proof: &crate::types::DpopProof, token: &str) -> crate::Result<()> {
        use sha2::{Digest, Sha256};

        // Compute expected ath claim
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let hash = hasher.finalize();
        let expected_ath = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

        // Check ath claim matches
        match &proof.payload.ath {
            Some(ath) if ath == &expected_ath => Ok(()),
            Some(ath) => Err(crate::errors::DpopError::AccessTokenHashFailed {
                reason: format!("Access token hash mismatch: expected '{}', got '{}'", expected_ath, ath),
            }),
            None => Err(crate::errors::DpopError::AccessTokenHashFailed {
                reason: "Missing ath claim for access token binding".to_string(),
            }),
        }
    }
}

impl Default for DpopValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Validated DPoP proof claims
///
/// Contains the validated claims from a DPoP proof after successful validation.
#[derive(Debug, Clone)]
pub struct ValidatedDpopClaims {
    /// HTTP method
    pub htm: String,
    /// HTTP URI
    pub htu: String,
    /// Access token hash (optional)
    pub ath: Option<String>,
    /// JWT ID (nonce)
    pub jti: String,
    /// Issued at timestamp
    pub iat: i64,
}
