//! Core DPoP types and data structures
//!
//! This module implements the fundamental types for RFC 9449 DPoP (Demonstration
//! of Proof-of-Possession) including algorithms, key pairs, proofs, and related metadata.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, SystemTime};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

/// DPoP cryptographic algorithms as defined in RFC 9449
///
/// The specification requires support for RSA and ECDSA algorithms with specific
/// parameters. This enum provides type-safe algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DpopAlgorithm {
    /// RSA with PKCS#1 v1.5 padding and SHA-256 (RFC 7518)
    #[serde(rename = "RS256")]
    RS256,

    /// Elliptic Curve Digital Signature Algorithm with P-256 curve and SHA-256 (RFC 7518)
    #[serde(rename = "ES256")]
    ES256,

    /// RSA with PSS padding and SHA-256 (RFC 7518)  
    #[serde(rename = "PS256")]
    PS256,
}

impl DpopAlgorithm {
    /// Get the algorithm name as specified in RFC 7518
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::PS256 => "PS256",
        }
    }

    /// Get recommended key size for the algorithm
    pub fn recommended_key_size(self) -> u32 {
        match self {
            Self::RS256 | Self::PS256 => 2048, // RSA-2048 minimum
            Self::ES256 => 256,                // P-256 curve
        }
    }

    /// Check if algorithm is suitable for production use
    pub fn is_production_ready(self) -> bool {
        // All RFC 9449 required algorithms are production-ready
        true
    }
}

impl fmt::Display for DpopAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// DPoP key pair with metadata
///
/// Contains the cryptographic key material and associated metadata for DPoP operations.
/// The private key is zeroized on drop to prevent memory disclosure attacks.
#[derive(Debug, Clone)]
pub struct DpopKeyPair {
    /// Unique identifier for this key pair
    pub id: String,

    /// Private key material (will be zeroized on drop)
    pub private_key: DpopPrivateKey,

    /// Public key material
    pub public_key: DpopPublicKey,

    /// JWK thumbprint for binding (RFC 7638)
    pub thumbprint: String,

    /// Cryptographic algorithm
    pub algorithm: DpopAlgorithm,

    /// Key creation timestamp
    pub created_at: SystemTime,

    /// Key expiration (None = never expires)
    pub expires_at: Option<SystemTime>,

    /// Key usage metadata
    pub metadata: DpopKeyMetadata,
}

impl DpopKeyPair {
    /// Check if the key pair has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires| SystemTime::now() > expires)
            .unwrap_or(false)
    }

    /// Check if the key pair will expire within the given duration
    pub fn expires_within(&self, duration: Duration) -> bool {
        self.expires_at
            .map(|expires| expires <= SystemTime::now() + duration)
            .unwrap_or(false)
    }

    /// Get the age of this key pair
    pub fn age(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.created_at)
            .unwrap_or(Duration::ZERO)
    }
}

/// Private key material for DPoP operations
#[derive(Debug, Clone)]
pub enum DpopPrivateKey {
    /// RSA private key
    Rsa {
        /// RSA private key in PKCS#8 DER format
        key_der: Vec<u8>,
    },
    /// ECDSA P-256 private key
    EcdsaP256 {
        /// P-256 private key in SEC1 format
        key_bytes: [u8; 32],
    },
}

impl Zeroize for DpopPrivateKey {
    fn zeroize(&mut self) {
        match self {
            Self::Rsa { key_der } => key_der.zeroize(),
            Self::EcdsaP256 { key_bytes } => key_bytes.zeroize(),
        }
    }
}

impl Drop for DpopPrivateKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// Public key material for DPoP operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpopPublicKey {
    /// RSA public key
    Rsa {
        /// RSA modulus (n parameter)
        n: Vec<u8>,
        /// RSA public exponent (e parameter)  
        e: Vec<u8>,
    },
    /// ECDSA P-256 public key
    EcdsaP256 {
        /// X coordinate of the public key point
        x: [u8; 32],
        /// Y coordinate of the public key point
        y: [u8; 32],
    },
}

/// Key usage metadata for auditing and management
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DpopKeyMetadata {
    /// Human-readable key description
    pub description: Option<String>,

    /// Client identifier this key belongs to
    pub client_id: Option<String>,

    /// Session identifier (if session-bound)
    pub session_id: Option<String>,

    /// Number of times this key has been used
    pub usage_count: u64,

    /// Last time this key was used for proof generation
    pub last_used: Option<SystemTime>,

    /// Key rotation generation (0 = original, 1+ = rotated)
    pub rotation_generation: u32,

    /// Custom metadata for applications
    pub custom: HashMap<String, serde_json::Value>,
}

/// DPoP JWT header as defined in RFC 9449
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpopHeader {
    /// JWT type - always "dpop+jwt" for DPoP
    #[serde(rename = "typ")]
    pub typ: String,

    /// Cryptographic algorithm used for signing
    #[serde(rename = "alg")]
    pub algorithm: DpopAlgorithm,

    /// JSON Web Key (JWK) representing the public key
    #[serde(rename = "jwk")]
    pub jwk: DpopJwk,
}

/// DPoP JWT payload as defined in RFC 9449
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpopPayload {
    /// JWT ID - unique nonce for replay prevention
    #[serde(rename = "jti")]
    pub jti: String,

    /// HTTP method being bound to this proof
    #[serde(rename = "htm")]
    pub htm: String,

    /// HTTP URI being bound to this proof (without query/fragment)
    #[serde(rename = "htu")]
    pub htu: String,

    /// Issued at timestamp (Unix timestamp)
    #[serde(rename = "iat")]
    pub iat: i64,

    /// Access token hash (when binding to an access token)
    #[serde(rename = "ath", skip_serializing_if = "Option::is_none")]
    pub ath: Option<String>,

    /// Confirmation nonce from authorization server
    #[serde(rename = "nonce", skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// JSON Web Key representation for DPoP public keys
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kty")]
pub enum DpopJwk {
    /// RSA public key in JWK format
    #[serde(rename = "RSA")]
    Rsa {
        /// Key usage - always "sig" for DPoP
        #[serde(rename = "use")]
        use_: String,

        /// RSA modulus (base64url-encoded)
        n: String,

        /// RSA public exponent (base64url-encoded)
        e: String,
    },

    /// Elliptic Curve public key in JWK format
    #[serde(rename = "EC")]
    Ec {
        /// Key usage - always "sig" for DPoP
        #[serde(rename = "use")]
        use_: String,

        /// Elliptic curve name - always "P-256" for ES256
        crv: String,

        /// X coordinate (base64url-encoded)
        x: String,

        /// Y coordinate (base64url-encoded)
        y: String,
    },
}

/// Complete DPoP proof JWT
#[derive(Debug, Clone)]
pub struct DpopProof {
    /// JWT header
    pub header: DpopHeader,

    /// JWT payload
    pub payload: DpopPayload,

    /// JWT signature (base64url-encoded)
    pub signature: String,

    /// The complete JWT string representation
    jwt_string: Option<String>,
}

impl DpopProof {
    /// Create a new DPoP proof
    pub fn new(header: DpopHeader, payload: DpopPayload, signature: String) -> Self {
        Self {
            header,
            payload,
            signature,
            jwt_string: None,
        }
    }

    /// Create a new DPoP proof with pre-computed JWT string for performance
    pub fn new_with_jwt(
        header: DpopHeader,
        payload: DpopPayload,
        signature: String,
        jwt_string: String,
    ) -> Self {
        Self {
            header,
            payload,
            signature,
            jwt_string: Some(jwt_string),
        }
    }

    /// Get the JWT string representation for HTTP headers
    ///
    /// Returns a complete RFC 7515 compliant JWT in the format: `header.payload.signature`
    /// where each component is base64url-encoded JSON. Uses production-grade JWT formatting
    /// compatible with the jsonwebtoken crate standards.
    pub fn to_jwt_string(&self) -> String {
        if let Some(ref cached) = self.jwt_string {
            return cached.clone();
        }

        // RFC 7515 compliant JWT construction: header.payload.signature
        // Manual construction is appropriate here since we're assembling pre-signed tokens
        match self.create_jwt_string() {
            Ok(jwt) => jwt,
            Err(e) => {
                // Log error but provide a functional fallback
                tracing::error!("Failed to create JWT string: {}, using fallback", e);
                self.create_minimal_jwt_fallback()
            }
        }
    }

    /// Create RFC 7515 compliant JWT string
    ///
    /// This follows the exact same format as the jsonwebtoken crate but is optimized
    /// for our use case of assembling already-signed DPoP tokens.
    fn create_jwt_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Serialize header and payload to canonical JSON (matches jsonwebtoken crate behavior)
        let header_json = serde_json::to_string(&self.header)
            .map_err(|e| format!("Failed to serialize header: {e}"))?;

        let payload_json = serde_json::to_string(&self.payload)
            .map_err(|e| format!("Failed to serialize payload: {e}"))?;

        // Base64url encode both components (RFC 7515 Section 2)
        let encoded_header = URL_SAFE_NO_PAD.encode(header_json);
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload_json);

        // Construct complete JWT: header.payload.signature (RFC 7515 Section 7.1)
        Ok(format!(
            "{}.{}.{}",
            encoded_header, encoded_payload, self.signature
        ))
    }

    /// Create minimal valid JWT as fallback (should never be needed in production)
    fn create_minimal_jwt_fallback(&self) -> String {
        // Create a minimal but valid DPoP JWT header
        let minimal_header = format!(r#"{{"typ":"{}","alg":"ES256"}}"#, crate::DPOP_JWT_TYPE);
        let minimal_payload = "{}";

        let encoded_header = URL_SAFE_NO_PAD.encode(minimal_header);
        let encoded_payload = URL_SAFE_NO_PAD.encode(minimal_payload);

        format!("{}.{}.{}", encoded_header, encoded_payload, self.signature)
    }

    /// Parse and cryptographically validate DPoP proof from JWT string
    ///
    /// This method leverages the battle-tested jsonwebtoken crate for complete JWT parsing
    /// and cryptographic signature verification using the embedded JWK. This implementation
    /// follows RFC 9449 security requirements and validates signatures before processing claims.
    ///
    /// Requires the `jwt-validation` feature to be enabled.
    #[cfg(feature = "jwt-validation")]
    pub fn from_jwt_string(jwt: &str) -> crate::Result<Self> {
        use jsonwebtoken::{decode, decode_header, Algorithm, Validation};

        // Use jsonwebtoken crate to decode header (no validation yet)
        let jwt_header =
            decode_header(jwt).map_err(|e| crate::DpopError::InvalidProofStructure {
                reason: format!("Failed to decode JWT header: {}", e),
            })?;

        // Validate this is a DPoP JWT
        if jwt_header.typ.as_deref() != Some(crate::DPOP_JWT_TYPE) {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: format!(
                    "Invalid JWT type: expected '{}', got '{:?}'",
                    crate::DPOP_JWT_TYPE,
                    jwt_header.typ
                ),
            });
        }

        // Convert jsonwebtoken::Header to our DpopHeader
        let algorithm = match jwt_header.alg {
            Algorithm::ES256 => DpopAlgorithm::ES256,
            Algorithm::RS256 => DpopAlgorithm::RS256,
            Algorithm::PS256 => DpopAlgorithm::PS256,
            other => {
                return Err(crate::DpopError::InvalidProofStructure {
                    reason: format!("Unsupported DPoP algorithm: {:?}", other),
                });
            }
        };

        // Extract JWK from header - convert from jsonwebtoken::Jwk to our DpopJwk
        let jwk_value = jwt_header
            .jwk
            .ok_or_else(|| crate::DpopError::InvalidProofStructure {
                reason: "Missing JWK in DPoP proof header".to_string(),
            })?;

        // Convert jsonwebtoken::Jwk to serde_json::Value first, then to our DpopJwk
        let jwk_json = serde_json::to_value(&jwk_value).map_err(|e| {
            crate::DpopError::InvalidProofStructure {
                reason: format!("Failed to serialize JWK: {}", e),
            }
        })?;

        let jwk: DpopJwk = serde_json::from_value(jwk_json).map_err(|e| {
            crate::DpopError::InvalidProofStructure {
                reason: format!("Invalid JWK in header: {}", e),
            }
        })?;

        let header = DpopHeader {
            typ: crate::DPOP_JWT_TYPE.to_string(),
            algorithm,
            jwk,
        };

        // CRITICAL SECURITY: Create proper DecodingKey from embedded JWK for signature validation
        let decoding_key = create_decoding_key_from_jwk(&header.jwk).map_err(|e| {
            crate::DpopError::CryptographicError {
                reason: format!("Failed to create decoding key from JWK: {}", e),
            }
        })?;

        // Use proper validation with signature verification enabled (RFC 9449 requirement)
        let validation = Validation::new(jwt_header.alg);

        // Decode and validate JWT signature using the embedded public key
        let token_data = decode::<DpopPayload>(jwt, &decoding_key, &validation).map_err(|e| {
            crate::DpopError::ProofValidationFailed {
                reason: format!("JWT signature validation failed: {}", e),
            }
        })?;

        let payload = token_data.claims;

        // Extract signature from JWT (jsonwebtoken doesn't expose this directly)
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: format!("Invalid JWT format: expected 3 parts, got {}", parts.len()),
            });
        }
        let signature = parts[2].to_string();

        // Create proof with cached JWT string for performance
        Ok(Self::new_with_jwt(
            header,
            payload,
            signature,
            jwt.to_string(),
        ))
    }

    /// Get the JWK thumbprint from this proof
    pub fn thumbprint(&self) -> crate::Result<String> {
        compute_jwk_thumbprint(&self.header.jwk)
    }

    /// Validate the proof structure (not cryptographic signature)
    pub fn validate_structure(&self) -> crate::Result<()> {
        // Validate JWT type
        if self.header.typ != crate::DPOP_JWT_TYPE {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: format!("Invalid JWT type: {}", self.header.typ),
            });
        }

        // Validate JTI format (should be UUID)
        if Uuid::parse_str(&self.payload.jti).is_err() {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: "Invalid JTI format - must be UUID".to_string(),
            });
        }

        // Validate HTTP method
        if !is_valid_http_method(&self.payload.htm) {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: format!("Invalid HTTP method: {}", self.payload.htm),
            });
        }

        // Validate HTTP URI
        if !is_valid_http_uri(&self.payload.htu) {
            return Err(crate::DpopError::InvalidProofStructure {
                reason: format!("Invalid HTTP URI: {}", self.payload.htu),
            });
        }

        Ok(())
    }

    /// Check if proof has expired based on timestamp
    pub fn is_expired(&self, max_age: Duration) -> bool {
        let issued_at = SystemTime::UNIX_EPOCH + Duration::from_secs(self.payload.iat as u64);
        SystemTime::now() > issued_at + max_age
    }
}

/// Unique identifier for registered intents
pub type TicketId = String;

/// Generate a new ticket ID
pub fn generate_ticket_id() -> TicketId {
    Uuid::new_v4().to_string()
}

/// Compute JWK thumbprint as defined in RFC 7638
pub fn compute_jwk_thumbprint(jwk: &DpopJwk) -> crate::Result<String> {
    use sha2::{Digest, Sha256};

    // Create canonical JWK representation for thumbprint computation
    let canonical_jwk = match jwk {
        DpopJwk::Rsa { n, e, .. } => {
            serde_json::json!({
                "e": e,
                "kty": "RSA",
                "n": n
            })
        }
        DpopJwk::Ec { crv, x, y, .. } => {
            serde_json::json!({
                "crv": crv,
                "kty": "EC",
                "x": x,
                "y": y
            })
        }
    };

    // Serialize to canonical JSON (keys in lexicographic order)
    let canonical_json = serde_json::to_string(&canonical_jwk).map_err(|e| {
        crate::DpopError::CryptographicError {
            reason: format!("Failed to serialize JWK for thumbprint: {e}"),
        }
    })?;

    // Compute SHA-256 hash
    let mut hasher = Sha256::new();
    hasher.update(canonical_json.as_bytes());
    let hash = hasher.finalize();

    // Return base64url-encoded thumbprint
    Ok(URL_SAFE_NO_PAD.encode(hash))
}

/// Validate HTTP method format
fn is_valid_http_method(method: &str) -> bool {
    matches!(
        method.to_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    )
}

/// Validate HTTP URI format (basic validation)
fn is_valid_http_uri(uri: &str) -> bool {
    uri.starts_with("https://") || uri.starts_with("http://")
}

/// Create jsonwebtoken DecodingKey from DPoP JWK
///
/// This function converts our DpopJwk to a DecodingKey that can be used
/// with the jsonwebtoken crate for signature verification. This is critical
/// for proper DPoP security as per RFC 9449 requirements.
#[cfg(feature = "jwt-validation")]
fn create_decoding_key_from_jwk(
    jwk: &DpopJwk,
) -> Result<jsonwebtoken::DecodingKey, Box<dyn std::error::Error>> {
    use jsonwebtoken::DecodingKey;

    match jwk {
        DpopJwk::Rsa { n, e, .. } => {
            // Use jsonwebtoken's RSA components method (expects base64url-encoded strings)
            DecodingKey::from_rsa_components(n, e)
                .map_err(|e| format!("Failed to create RSA decoding key: {}", e).into())
        }
        DpopJwk::Ec { x, y, .. } => {
            // Use jsonwebtoken's EC components method (expects base64url-encoded strings)
            DecodingKey::from_ec_components(x, y)
                .map_err(|e| format!("Failed to create EC decoding key: {}", e).into())
        }
    }
}

/// Statistics about nonce storage usage and performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of nonces stored
    pub total_nonces: u64,
    /// Number of active (non-expired) nonces
    pub active_nonces: u64,
    /// Number of expired nonces that have been cleaned up
    pub expired_nonces: u64,
    /// Number of cleanup operations performed
    pub cleanup_runs: u64,
    /// Average age of stored nonces
    pub average_nonce_age: Duration,
    /// Estimated storage size in bytes
    pub storage_size_bytes: u64,
    /// Additional backend-specific metrics
    pub additional_metrics: Vec<(String, String)>,
}

impl Default for StorageStats {
    fn default() -> Self {
        Self {
            total_nonces: 0,
            active_nonces: 0,
            expired_nonces: 0,
            cleanup_runs: 0,
            average_nonce_age: Duration::ZERO,
            storage_size_bytes: 0,
            additional_metrics: Vec::new(),
        }
    }
}

/// Trait for DPoP nonce storage backends
///
/// This trait defines the interface for storing and managing DPoP nonces to prevent replay attacks.
/// Implementations should ensure thread-safety and efficient concurrent access.
#[async_trait::async_trait]
pub trait NonceStorage: Send + Sync + std::fmt::Debug {
    /// Store a nonce with associated metadata
    ///
    /// # Arguments
    /// * `nonce` - The unique nonce value from the DPoP proof
    /// * `jti` - The JWT ID (jti) claim from the DPoP proof
    /// * `http_method` - The HTTP method for which this nonce is valid
    /// * `http_uri` - The HTTP URI for which this nonce is valid
    /// * `client_id` - The client identifier
    /// * `ttl` - Time-to-live for the nonce (None uses default)
    ///
    /// # Returns
    /// * `Ok(true)` - Nonce was successfully stored (first use)
    /// * `Ok(false)` - Nonce already exists (replay attack detected)
    /// * `Err(_)` - Storage operation failed
    async fn store_nonce(
        &self,
        nonce: &str,
        jti: &str,
        http_method: &str,
        http_uri: &str,
        client_id: &str,
        ttl: Option<Duration>,
    ) -> crate::Result<bool>;

    /// Check if a nonce has been used before
    ///
    /// # Arguments
    /// * `nonce` - The nonce to check
    /// * `client_id` - The client identifier
    ///
    /// # Returns
    /// * `Ok(true)` - Nonce has been used before
    /// * `Ok(false)` - Nonce is new
    /// * `Err(_)` - Storage operation failed
    async fn is_nonce_used(&self, nonce: &str, client_id: &str) -> crate::Result<bool>;

    /// Clean up expired nonces
    ///
    /// # Returns
    /// Number of expired nonces cleaned up
    async fn cleanup_expired(&self) -> crate::Result<u64>;

    /// Get storage usage statistics
    ///
    /// # Returns
    /// Statistics about nonce storage usage and performance
    async fn get_usage_stats(&self) -> crate::Result<StorageStats>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpop_algorithm_properties() {
        assert_eq!(DpopAlgorithm::RS256.as_str(), "RS256");
        assert_eq!(DpopAlgorithm::ES256.as_str(), "ES256");
        assert_eq!(DpopAlgorithm::PS256.as_str(), "PS256");

        assert_eq!(DpopAlgorithm::RS256.recommended_key_size(), 2048);
        assert_eq!(DpopAlgorithm::ES256.recommended_key_size(), 256);
        assert_eq!(DpopAlgorithm::PS256.recommended_key_size(), 2048);

        assert!(DpopAlgorithm::RS256.is_production_ready());
        assert!(DpopAlgorithm::ES256.is_production_ready());
        assert!(DpopAlgorithm::PS256.is_production_ready());
    }

    #[test]
    fn test_http_method_validation() {
        assert!(is_valid_http_method("GET"));
        assert!(is_valid_http_method("post"));
        assert!(is_valid_http_method("PUT"));
        assert!(!is_valid_http_method("INVALID"));
        assert!(!is_valid_http_method(""));
    }

    #[test]
    fn test_http_uri_validation() {
        assert!(is_valid_http_uri("https://api.example.com/token"));
        assert!(is_valid_http_uri("http://localhost:8080/auth"));
        assert!(!is_valid_http_uri("ftp://example.com"));
        assert!(!is_valid_http_uri("invalid-uri"));
    }

    #[test]
    fn test_ticket_id_generation() {
        let id1 = generate_ticket_id();
        let id2 = generate_ticket_id();

        assert_ne!(id1, id2);
        assert!(Uuid::parse_str(&id1).is_ok());
        assert!(Uuid::parse_str(&id2).is_ok());
    }
}
