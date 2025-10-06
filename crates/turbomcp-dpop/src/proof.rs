//! DPoP proof generation and validation
//!
//! This module implements RFC 9449 compliant DPoP proof generation and validation
//! with comprehensive security features including replay attack prevention,
//! timing attack protection, and cryptographic validation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use signature::{SignatureEncoding, Signer, Verifier};
use tokio::sync::RwLock;
use tracing::debug;
use uuid::Uuid;

use super::{
    DEFAULT_PROOF_LIFETIME_SECONDS, DPOP_JWT_TYPE, MAX_CLOCK_SKEW_SECONDS, Result,
    errors::DpopError,
    keys::DpopKeyManager,
    types::{
        DpopAlgorithm, DpopHeader, DpopJwk, DpopKeyPair, DpopPayload, DpopPrivateKey, DpopProof,
        DpopPublicKey,
    },
};

#[cfg(feature = "redis-storage")]
use super::{redis_storage::RedisNonceStorage, types::NonceStorage};

/// DPoP proof generator with comprehensive security features
#[derive(Debug)]
pub struct DpopProofGenerator {
    /// Key manager for cryptographic operations
    key_manager: Arc<DpopKeyManager>,
    /// Nonce tracker for replay attack prevention
    nonce_tracker: Arc<dyn NonceTracker>,
    /// Clock skew tolerance in seconds
    clock_skew_tolerance: Duration,
    /// Default proof lifetime
    proof_lifetime: Duration,
}

impl DpopProofGenerator {
    /// Create a new DPoP proof generator
    pub fn new(key_manager: Arc<DpopKeyManager>) -> Self {
        Self::with_nonce_tracker(key_manager, Arc::new(MemoryNonceTracker::new()))
    }

    /// Create a new DPoP proof generator with custom nonce tracker
    pub fn with_nonce_tracker(
        key_manager: Arc<DpopKeyManager>,
        nonce_tracker: Arc<dyn NonceTracker>,
    ) -> Self {
        Self {
            key_manager,
            nonce_tracker,
            clock_skew_tolerance: Duration::from_secs(MAX_CLOCK_SKEW_SECONDS as u64),
            proof_lifetime: Duration::from_secs(DEFAULT_PROOF_LIFETIME_SECONDS),
        }
    }

    /// Generate a DPoP proof for an HTTP request
    pub async fn generate_proof(
        &self,
        method: &str,
        uri: &str,
        access_token: Option<&str>,
    ) -> Result<DpopProof> {
        self.generate_proof_with_key(method, uri, access_token, None)
            .await
    }

    /// Generate a DPoP proof using a specific key pair
    pub async fn generate_proof_with_key(
        &self,
        method: &str,
        uri: &str,
        access_token: Option<&str>,
        key_pair: Option<&DpopKeyPair>,
    ) -> Result<DpopProof> {
        // Get or generate key pair
        let key_pair = match key_pair {
            Some(kp) => kp.clone(),
            None => self.get_or_generate_default_key().await?,
        };

        // Validate inputs
        self.validate_inputs(method, uri)?;

        // Generate unique nonce (JTI)
        let jti = Uuid::new_v4().to_string();

        // Current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DpopError::InternalError {
                reason: "System clock before Unix epoch".to_string(),
            })?
            .as_secs() as i64;

        // Clean URI (remove query parameters and fragment)
        let clean_uri = clean_http_uri(uri)?;

        // Create JWT payload
        let mut payload = DpopPayload {
            jti: jti.clone(),
            htm: method.to_uppercase(),
            htu: clean_uri,
            iat: now,
            ath: None,
            nonce: None,
        };

        // Add access token hash if provided
        if let Some(token) = access_token {
            payload.ath = Some(compute_access_token_hash(token)?);
        }

        // Create JWK from public key
        let jwk = create_jwk_from_public_key(&key_pair.public_key, key_pair.algorithm)?;

        // Create JWT header
        let header = DpopHeader {
            typ: DPOP_JWT_TYPE.to_string(),
            algorithm: key_pair.algorithm,
            jwk,
        };

        // Sign the JWT
        let signature = self
            .sign_jwt(&header, &payload, &key_pair.private_key)
            .await?;

        // Note: Nonce tracking moved to validation step to prevent false replay detection in tests
        // In production, server-side validation tracks nonces, not client-side generation

        let proof = DpopProof::new(header, payload, signature);

        tracing::debug!(
            key_id = %key_pair.id,
            method = %method,
            uri = %uri,
            jti = %jti,
            "Generated DPoP proof"
        );

        Ok(proof)
    }

    /// Parse and validate a DPoP JWT string (high-level API)
    ///
    /// This is the main high-level API that auth integrations should use.
    /// It combines JWT parsing and comprehensive DPoP validation in one call.
    ///
    /// Requires the `jwt-validation` feature to be enabled.
    pub async fn parse_and_validate_jwt(
        &self,
        jwt_string: &str,
        method: &str,
        uri: &str,
        access_token: Option<&str>,
    ) -> Result<DpopValidationResult> {
        // Parse the JWT string into a DPoP proof
        let proof = DpopProof::from_jwt_string(jwt_string)?;

        // Validate the parsed proof
        self.validate_proof(&proof, method, uri, access_token).await
    }

    /// Validate a DPoP proof
    pub async fn validate_proof(
        &self,
        proof: &DpopProof,
        method: &str,
        uri: &str,
        access_token: Option<&str>,
    ) -> Result<DpopValidationResult> {
        // Basic structure validation
        proof.validate_structure()?;

        // Validate HTTP method and URI binding
        self.validate_http_binding(proof, method, uri)?;

        // Validate timestamp and expiration
        self.validate_timestamp(proof)?;

        // Check for replay attacks
        self.validate_nonce(proof).await?;

        // Validate access token hash logic
        match (access_token, &proof.payload.ath) {
            (Some(token), _) => {
                // Access token provided, validate if there's a hash
                self.validate_access_token_hash(proof, token)?;
            }
            (None, Some(_)) => {
                // Proof has token hash but no access token provided
                return Err(DpopError::AccessTokenHashFailed {
                    reason: "Proof contains access token hash but no access token provided for validation".to_string(),
                });
            }
            (None, None) => {
                // No access token and no hash - OK
            }
        }

        // Cryptographic signature validation
        self.validate_signature(proof).await?;

        // Track nonce after successful validation to prevent future replay attacks
        self.nonce_tracker
            .track_nonce(&proof.payload.jti, proof.payload.iat)
            .await?;

        let thumbprint = proof.thumbprint()?;

        Ok(DpopValidationResult {
            valid: true,
            thumbprint,
            key_algorithm: proof.header.algorithm,
            issued_at: UNIX_EPOCH + Duration::from_secs(proof.payload.iat as u64),
            expires_at: UNIX_EPOCH
                + Duration::from_secs(proof.payload.iat as u64)
                + self.proof_lifetime,
        })
    }

    /// Get or generate a default key pair with proven key management
    async fn get_or_generate_default_key(&self) -> Result<DpopKeyPair> {
        // Production implementation: Generate key with proper algorithm selection
        // Key rotation would be handled by the key manager's internal policies
        debug!("Generating DPoP key pair for proof generation");

        self.key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
    }

    /// Validate input parameters
    fn validate_inputs(&self, method: &str, uri: &str) -> Result<()> {
        // Validate HTTP method
        if !is_valid_http_method(method) {
            return Err(DpopError::InvalidProofStructure {
                reason: format!("Invalid HTTP method: {method}"),
            });
        }

        // Validate URI format
        if !is_valid_http_uri(uri) {
            return Err(DpopError::InvalidProofStructure {
                reason: format!("Invalid HTTP URI: {uri}"),
            });
        }

        Ok(())
    }

    /// Validate HTTP method and URI binding
    fn validate_http_binding(&self, proof: &DpopProof, method: &str, uri: &str) -> Result<()> {
        // Check HTTP method
        if proof.payload.htm.to_uppercase() != method.to_uppercase() {
            return Err(DpopError::HttpBindingFailed {
                reason: format!(
                    "HTTP method mismatch: proof has '{}', request uses '{}'",
                    proof.payload.htm, method
                ),
            });
        }

        // Clean and compare URI
        let clean_uri = clean_http_uri(uri)?;
        if proof.payload.htu != clean_uri {
            return Err(DpopError::HttpBindingFailed {
                reason: format!(
                    "HTTP URI mismatch: proof has '{}', request uses '{}'",
                    proof.payload.htu, clean_uri
                ),
            });
        }

        Ok(())
    }

    /// Validate proof timestamp and expiration
    fn validate_timestamp(&self, proof: &DpopProof) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DpopError::InternalError {
                reason: "System clock before Unix epoch".to_string(),
            })?
            .as_secs() as i64;

        let issued_at = proof.payload.iat;
        let time_diff = (now - issued_at).abs();

        // Check clock skew
        if time_diff > self.clock_skew_tolerance.as_secs() as i64 {
            return Err(DpopError::ClockSkewTooLarge {
                skew_seconds: time_diff,
                max_skew_seconds: self.clock_skew_tolerance.as_secs() as i64,
            });
        }

        // Check if proof has expired
        let proof_age = now - issued_at;
        if proof_age > self.proof_lifetime.as_secs() as i64 {
            return Err(DpopError::ProofExpired {
                issued_at,
                max_age_seconds: self.proof_lifetime.as_secs(),
            });
        }

        Ok(())
    }

    /// Validate nonce to prevent replay attacks
    async fn validate_nonce(&self, proof: &DpopProof) -> Result<()> {
        let is_used = self.nonce_tracker.is_nonce_used(&proof.payload.jti).await?;
        if is_used {
            return Err(DpopError::ReplayAttackDetected {
                nonce: proof.payload.jti.clone(),
            });
        }

        Ok(())
    }

    /// Validate access token hash
    fn validate_access_token_hash(&self, proof: &DpopProof, access_token: &str) -> Result<()> {
        match &proof.payload.ath {
            Some(provided_hash) => {
                // Proof has token hash, validate it matches the provided token
                let computed_hash = compute_access_token_hash(access_token)?;
                if !constant_time_compare(provided_hash, &computed_hash) {
                    return Err(DpopError::AccessTokenHashFailed {
                        reason: "Access token hash mismatch".to_string(),
                    });
                }
            }
            None => {
                // Proof has no token hash but access token provided - this is OK
                // The access token just isn't cryptographically bound to this proof
            }
        }

        Ok(())
    }

    /// Validate cryptographic signature
    async fn validate_signature(&self, proof: &DpopProof) -> Result<()> {
        // Get the public key from the JWK in the proof
        let public_key = extract_public_key_from_jwk(&proof.header.jwk)?;

        // Verify the signature
        verify_jwt_signature(proof, &public_key).await?;

        Ok(())
    }

    /// Sign a JWT with the given private key
    async fn sign_jwt(
        &self,
        header: &DpopHeader,
        payload: &DpopPayload,
        private_key: &DpopPrivateKey,
    ) -> Result<String> {
        // Serialize header and payload
        let header_json =
            serde_json::to_string(header).map_err(|e| DpopError::SerializationError {
                reason: format!("Failed to serialize header: {e}"),
            })?;

        let payload_json =
            serde_json::to_string(payload).map_err(|e| DpopError::SerializationError {
                reason: format!("Failed to serialize payload: {e}"),
            })?;

        // Base64url encode header and payload
        let encoded_header = URL_SAFE_NO_PAD.encode(header_json);
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload_json);

        // Create signing input
        let signing_input = format!("{}.{}", encoded_header, encoded_payload);

        // Sign based on key type
        let signature_bytes = match private_key {
            DpopPrivateKey::EcdsaP256 { key_bytes } => sign_with_es256(&signing_input, key_bytes)?,
            DpopPrivateKey::Rsa { key_der } => {
                sign_with_rsa(&signing_input, key_der, header.algorithm)?
            }
        };

        // Base64url encode signature
        Ok(URL_SAFE_NO_PAD.encode(signature_bytes))
    }
}

/// DPoP proof validation result
#[derive(Debug, Clone)]
pub struct DpopValidationResult {
    /// Whether the proof is valid
    pub valid: bool,
    /// JWK thumbprint of the key used to sign the proof
    pub thumbprint: String,
    /// Algorithm used for signing
    pub key_algorithm: DpopAlgorithm,
    /// When the proof was issued
    pub issued_at: SystemTime,
    /// When the proof expires
    pub expires_at: SystemTime,
}

/// Trait for nonce tracking to prevent replay attacks
#[async_trait]
pub trait NonceTracker: Send + Sync + std::fmt::Debug {
    /// Track a nonce as used
    async fn track_nonce(&self, nonce: &str, issued_at: i64) -> Result<()>;

    /// Check if a nonce has been used
    async fn is_nonce_used(&self, nonce: &str) -> Result<bool>;

    /// Clean up expired nonces
    async fn cleanup_expired_nonces(&self) -> Result<usize>;
}

/// In-memory nonce tracker for development and testing
#[derive(Debug)]
pub struct MemoryNonceTracker {
    /// Set of used nonces with their timestamps
    used_nonces: Arc<RwLock<HashMap<String, i64>>>,
    /// Maximum age for nonces (after which they can be cleaned up)
    max_nonce_age: Duration,
}

impl MemoryNonceTracker {
    /// Create a new memory nonce tracker
    pub fn new() -> Self {
        Self {
            used_nonces: Arc::new(RwLock::new(HashMap::new())),
            max_nonce_age: Duration::from_secs(600), // 10 minutes
        }
    }
}

#[async_trait]
impl NonceTracker for MemoryNonceTracker {
    async fn track_nonce(&self, nonce: &str, issued_at: i64) -> Result<()> {
        self.used_nonces
            .write()
            .await
            .insert(nonce.to_string(), issued_at);
        Ok(())
    }

    async fn is_nonce_used(&self, nonce: &str) -> Result<bool> {
        Ok(self.used_nonces.read().await.contains_key(nonce))
    }

    async fn cleanup_expired_nonces(&self) -> Result<usize> {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DpopError::InternalError {
                reason: "System clock before Unix epoch".to_string(),
            })?
            .as_secs() as i64
            - self.max_nonce_age.as_secs() as i64;

        let mut nonces = self.used_nonces.write().await;
        let initial_count = nonces.len();

        nonces.retain(|_, &mut timestamp| timestamp > cutoff);

        Ok(initial_count - nonces.len())
    }
}

impl Default for MemoryNonceTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Redis-based nonce tracker for distributed production deployments
///
/// This implementation provides Redis-backed nonce tracking with full
/// DPoP replay protection across multiple server instances. Only available
/// when the `redis-storage` feature is enabled.
#[cfg(feature = "redis-storage")]
#[derive(Debug)]
pub struct RedisNonceTracker {
    /// Underlying Redis storage implementation
    storage: RedisNonceStorage,
    /// Default client ID for single-tenant deployments
    default_client_id: String,
}

#[cfg(feature = "redis-storage")]
impl RedisNonceTracker {
    /// Create a new Redis nonce tracker with default configuration
    ///
    /// # Arguments
    /// * `connection_string` - Redis connection string (e.g., "redis://localhost:6379")
    ///
    /// # Returns
    /// A new Redis nonce tracker instance
    ///
    /// # Errors
    /// Returns error if Redis connection fails or feature is not enabled
    ///
    /// # Example
    /// ```no_run
    /// # tokio_test::block_on(async {
    /// use turbomcp_dpop::RedisNonceTracker;
    ///
    /// let tracker = RedisNonceTracker::new("redis://localhost:6379").await?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub async fn new(connection_string: &str) -> Result<Self> {
        let storage = RedisNonceStorage::new(connection_string).await?;
        Ok(Self {
            storage,
            default_client_id: "turbomcp-default".to_string(),
        })
    }

    /// Create Redis nonce tracker with custom configuration
    ///
    /// # Arguments
    /// * `connection_string` - Redis connection string
    /// * `nonce_ttl` - Time-to-live for nonces in Redis
    /// * `key_prefix` - Custom prefix for Redis keys
    ///
    /// # Example
    /// ```no_run
    /// # tokio_test::block_on(async {
    /// use std::time::Duration;
    /// use turbomcp_dpop::RedisNonceTracker;
    ///
    /// let tracker = RedisNonceTracker::with_config(
    ///     "redis://localhost:6379",
    ///     Duration::from_secs(600), // 10 minutes
    ///     "myapp".to_string()
    /// ).await?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub async fn with_config(
        connection_string: &str,
        nonce_ttl: Duration,
        key_prefix: String,
    ) -> Result<Self> {
        let storage =
            RedisNonceStorage::with_config(connection_string, nonce_ttl, key_prefix).await?;
        Ok(Self {
            storage,
            default_client_id: "turbomcp-default".to_string(),
        })
    }

    /// Set custom default client ID for single-tenant scenarios
    pub fn with_client_id(mut self, client_id: String) -> Self {
        self.default_client_id = client_id;
        self
    }
}

#[cfg(feature = "redis-storage")]
#[async_trait]
impl NonceTracker for RedisNonceTracker {
    async fn track_nonce(&self, nonce: &str, issued_at: i64) -> Result<()> {
        // Convert timestamp to system time for TTL calculation
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DpopError::InternalError {
                reason: "System clock before Unix epoch".to_string(),
            })?
            .as_secs() as i64;

        // Calculate appropriate TTL based on issued_at vs current time
        let age = current_time.saturating_sub(issued_at);
        let remaining_ttl = Duration::from_secs(300_u64.saturating_sub(age as u64)); // 5 minutes max

        // Store nonce with comprehensive metadata
        let stored = self
            .storage
            .store_nonce(
                nonce,
                &format!("jti-{}", nonce), // JTI based on nonce for simplicity
                "POST", // Default method - would need to be passed through in real usage
                "https://api.turbomcp.org/default", // Default URI - would need actual URI
                &self.default_client_id,
                Some(remaining_ttl),
            )
            .await?;

        if !stored {
            return Err(DpopError::ProofValidationFailed {
                reason: format!("Nonce replay detected: {}", nonce),
            });
        }

        Ok(())
    }

    async fn is_nonce_used(&self, nonce: &str) -> Result<bool> {
        self.storage
            .is_nonce_used(nonce, &self.default_client_id)
            .await
    }

    async fn cleanup_expired_nonces(&self) -> Result<usize> {
        // Redis handles expiration automatically via TTL
        // Return 0 as Redis cleanup is transparent
        self.storage
            .cleanup_expired()
            .await
            .map(|count| count as usize)
    }
}

/// Redis-based nonce tracker (feature disabled)
///
/// When the `redis-storage` feature is not enabled, this provides clear
/// error messages directing users to enable the feature.
#[cfg(not(feature = "redis-storage"))]
#[derive(Debug)]
pub struct RedisNonceTracker;

#[cfg(not(feature = "redis-storage"))]
impl RedisNonceTracker {
    /// Create a new Redis nonce tracker (feature disabled)
    ///
    /// Returns a configuration error directing users to enable the 'redis-storage' feature
    pub async fn new(_connection_string: &str) -> Result<Self> {
        Err(DpopError::ConfigurationError {
            reason: "Redis nonce tracking requires 'redis-storage' feature. Add 'redis-storage' to your Cargo.toml features.".to_string(),
        })
    }

    /// Create Redis nonce tracker with custom configuration (feature disabled)
    pub async fn with_config(
        _connection_string: &str,
        _nonce_ttl: Duration,
        _key_prefix: String,
    ) -> Result<Self> {
        Self::new(_connection_string).await
    }

    /// Set custom default client ID (feature disabled)
    pub fn with_client_id(self, _client_id: String) -> Self {
        self
    }
}

// Helper functions

/// Validate HTTP method format
fn is_valid_http_method(method: &str) -> bool {
    matches!(
        method.to_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "TRACE"
    )
}

/// Validate HTTP URI format
fn is_valid_http_uri(uri: &str) -> bool {
    uri.starts_with("https://") || uri.starts_with("http://")
}

/// Clean HTTP URI by removing query parameters and fragment
fn clean_http_uri(uri: &str) -> Result<String> {
    let url = url::Url::parse(uri).map_err(|e| DpopError::InvalidProofStructure {
        reason: format!("Invalid URI format: {e}"),
    })?;

    // Return scheme + authority (host:port) + path only
    let authority = match url.port() {
        Some(port) => format!(
            "{}:{}",
            url.host_str()
                .ok_or_else(|| DpopError::InvalidProofStructure {
                    reason: "URI missing host".to_string(),
                })?,
            port
        ),
        None => url
            .host_str()
            .ok_or_else(|| DpopError::InvalidProofStructure {
                reason: "URI missing host".to_string(),
            })?
            .to_string(),
    };

    Ok(format!("{}://{}{}", url.scheme(), authority, url.path()))
}

/// Compute SHA-256 hash of access token for binding
fn compute_access_token_hash(access_token: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(access_token.as_bytes());
    let hash = hasher.finalize();
    Ok(URL_SAFE_NO_PAD.encode(hash))
}

/// Constant-time string comparison to prevent timing attacks
///
/// This function compares two strings in constant time to prevent timing attacks
/// on cryptographic values like hashes, tokens, and thumbprints. This is critical
/// for DPoP security as per RFC 9449 security requirements.
fn constant_time_compare(a: &str, b: &str) -> bool {
    use std::cmp;

    // If lengths differ, still do a constant-time comparison to avoid timing leaks
    let len_a = a.len();
    let len_b = b.len();
    let max_len = cmp::max(len_a, len_b);

    let bytes_a = a.as_bytes();
    let bytes_b = b.as_bytes();

    let mut result = (len_a != len_b) as u8;

    for i in 0..max_len {
        let byte_a = bytes_a.get(i).copied().unwrap_or(0);
        let byte_b = bytes_b.get(i).copied().unwrap_or(0);
        result |= byte_a ^ byte_b;
    }

    result == 0
}

/// Create JWK from public key
fn create_jwk_from_public_key(
    public_key: &DpopPublicKey,
    algorithm: DpopAlgorithm,
) -> Result<DpopJwk> {
    match (public_key, algorithm) {
        (DpopPublicKey::Rsa { n, e }, DpopAlgorithm::RS256 | DpopAlgorithm::PS256) => {
            Ok(DpopJwk::Rsa {
                use_: "sig".to_string(),
                n: URL_SAFE_NO_PAD.encode(n),
                e: URL_SAFE_NO_PAD.encode(e),
            })
        }
        (DpopPublicKey::EcdsaP256 { x, y }, DpopAlgorithm::ES256) => Ok(DpopJwk::Ec {
            use_: "sig".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode(x),
            y: URL_SAFE_NO_PAD.encode(y),
        }),
        _ => Err(DpopError::CryptographicError {
            reason: "Mismatched key type and algorithm".to_string(),
        }),
    }
}

/// Extract public key from JWK
fn extract_public_key_from_jwk(jwk: &DpopJwk) -> Result<DpopPublicKey> {
    match jwk {
        DpopJwk::Rsa { n, e, .. } => {
            let n_bytes =
                URL_SAFE_NO_PAD
                    .decode(n)
                    .map_err(|e| DpopError::InvalidProofStructure {
                        reason: format!("Invalid RSA modulus encoding: {e}"),
                    })?;
            let e_bytes =
                URL_SAFE_NO_PAD
                    .decode(e)
                    .map_err(|e| DpopError::InvalidProofStructure {
                        reason: format!("Invalid RSA exponent encoding: {e}"),
                    })?;

            Ok(DpopPublicKey::Rsa {
                n: n_bytes,
                e: e_bytes,
            })
        }
        DpopJwk::Ec { x, y, crv, .. } => {
            if crv != "P-256" {
                return Err(DpopError::InvalidProofStructure {
                    reason: format!("Unsupported curve: {crv}"),
                });
            }

            let x_bytes =
                URL_SAFE_NO_PAD
                    .decode(x)
                    .map_err(|e| DpopError::InvalidProofStructure {
                        reason: format!("Invalid EC X coordinate encoding: {e}"),
                    })?;
            let y_bytes =
                URL_SAFE_NO_PAD
                    .decode(y)
                    .map_err(|e| DpopError::InvalidProofStructure {
                        reason: format!("Invalid EC Y coordinate encoding: {e}"),
                    })?;

            let x_array: [u8; 32] =
                x_bytes
                    .try_into()
                    .map_err(|_| DpopError::InvalidProofStructure {
                        reason: "EC X coordinate must be 32 bytes".to_string(),
                    })?;
            let y_array: [u8; 32] =
                y_bytes
                    .try_into()
                    .map_err(|_| DpopError::InvalidProofStructure {
                        reason: "EC Y coordinate must be 32 bytes".to_string(),
                    })?;

            Ok(DpopPublicKey::EcdsaP256 {
                x: x_array,
                y: y_array,
            })
        }
    }
}

/// Sign with ECDSA P-256 (ES256)
fn sign_with_es256(data: &str, private_key: &[u8; 32]) -> Result<Vec<u8>> {
    use p256::ecdsa::{Signature, SigningKey};

    let signing_key =
        SigningKey::from_bytes(private_key.into()).map_err(|e| DpopError::CryptographicError {
            reason: format!("Invalid ECDSA private key: {e}"),
        })?;

    let signature: Signature = signing_key.sign(data.as_bytes());
    Ok(signature.to_bytes().to_vec())
}

/// Sign with RSA (RS256 or PS256)
fn sign_with_rsa(data: &str, private_key_der: &[u8], algorithm: DpopAlgorithm) -> Result<Vec<u8>> {
    use rsa::{RsaPrivateKey, pkcs1v15::SigningKey, pkcs8::DecodePrivateKey};

    let private_key = RsaPrivateKey::from_pkcs8_der(private_key_der).map_err(|e| {
        DpopError::CryptographicError {
            reason: format!("Invalid RSA private key: {e}"),
        }
    })?;

    match algorithm {
        DpopAlgorithm::RS256 => {
            let signing_key = SigningKey::<Sha256>::new(private_key);
            let signature: rsa::pkcs1v15::Signature = signing_key
                .try_sign(data.as_bytes())
                .map_err(|e| DpopError::CryptographicError {
                    reason: format!("RSA signing failed: {e}"),
                })?;
            Ok(signature.to_bytes().to_vec())
        }
        DpopAlgorithm::PS256 => {
            use rsa::pss::BlindedSigningKey;
            use signature::RandomizedSigner;

            let mut rng = rand::thread_rng();
            let signing_key = BlindedSigningKey::<Sha256>::new(private_key);
            let signature = signing_key.sign_with_rng(&mut rng, data.as_bytes());
            Ok(signature.to_bytes().to_vec())
        }
        _ => Err(DpopError::CryptographicError {
            reason: format!("Unsupported RSA algorithm: {algorithm}"),
        }),
    }
}

/// Verify JWT signature
async fn verify_jwt_signature(proof: &DpopProof, public_key: &DpopPublicKey) -> Result<()> {
    // Reconstruct the signing input
    let header_json =
        serde_json::to_string(&proof.header).map_err(|e| DpopError::SerializationError {
            reason: format!("Failed to serialize header: {e}"),
        })?;
    let payload_json =
        serde_json::to_string(&proof.payload).map_err(|e| DpopError::SerializationError {
            reason: format!("Failed to serialize payload: {e}"),
        })?;

    let encoded_header = URL_SAFE_NO_PAD.encode(header_json);
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload_json);
    let signing_input = format!("{}.{}", encoded_header, encoded_payload);

    // Decode signature
    let signature =
        URL_SAFE_NO_PAD
            .decode(&proof.signature)
            .map_err(|e| DpopError::InvalidProofStructure {
                reason: format!("Invalid signature encoding: {e}"),
            })?;

    // Verify based on algorithm
    match (public_key, proof.header.algorithm) {
        (DpopPublicKey::EcdsaP256 { x, y }, DpopAlgorithm::ES256) => {
            verify_es256_signature(&signing_input, &signature, x, y)?;
        }
        (DpopPublicKey::Rsa { n, e }, DpopAlgorithm::RS256 | DpopAlgorithm::PS256) => {
            verify_rsa_signature(&signing_input, &signature, n, e, proof.header.algorithm)?;
        }
        _ => {
            return Err(DpopError::CryptographicError {
                reason: "Mismatched key type and algorithm for verification".to_string(),
            });
        }
    }

    Ok(())
}

/// Verify ECDSA P-256 signature
fn verify_es256_signature(data: &str, signature: &[u8], x: &[u8; 32], y: &[u8; 32]) -> Result<()> {
    use p256::{
        EncodedPoint,
        ecdsa::{Signature, VerifyingKey},
    };

    // Reconstruct public key from coordinates
    let mut uncompressed = [0u8; 65];
    uncompressed[0] = 0x04; // Uncompressed point indicator
    uncompressed[1..33].copy_from_slice(x);
    uncompressed[33..65].copy_from_slice(y);

    let point =
        EncodedPoint::from_bytes(uncompressed).map_err(|e| DpopError::CryptographicError {
            reason: format!("Invalid public key point: {e}"),
        })?;

    let verifying_key =
        VerifyingKey::from_encoded_point(&point).map_err(|e| DpopError::CryptographicError {
            reason: format!("Invalid ECDSA public key: {e}"),
        })?;

    let signature = Signature::try_from(signature).map_err(|e| DpopError::CryptographicError {
        reason: format!("Invalid ECDSA signature format: {e}"),
    })?;

    verifying_key
        .verify(data.as_bytes(), &signature)
        .map_err(|e| DpopError::ProofValidationFailed {
            reason: format!("ECDSA signature verification failed: {e}"),
        })
}

/// Verify RSA signature
fn verify_rsa_signature(
    data: &str,
    signature: &[u8],
    n: &[u8],
    e: &[u8],
    algorithm: DpopAlgorithm,
) -> Result<()> {
    use rsa::{BigUint, RsaPublicKey, pkcs1v15::VerifyingKey};

    // Reconstruct RSA public key
    let n_bigint = BigUint::from_bytes_be(n);
    let e_bigint = BigUint::from_bytes_be(e);

    let public_key =
        RsaPublicKey::new(n_bigint, e_bigint).map_err(|e| DpopError::CryptographicError {
            reason: format!("Invalid RSA public key: {e}"),
        })?;

    match algorithm {
        DpopAlgorithm::RS256 => {
            let verifying_key = VerifyingKey::<Sha256>::new(public_key);
            let signature_obj = rsa::pkcs1v15::Signature::try_from(signature).map_err(|e| {
                DpopError::CryptographicError {
                    reason: format!("Invalid RSA signature format: {e}"),
                }
            })?;
            verifying_key
                .verify(data.as_bytes(), &signature_obj)
                .map_err(|e| DpopError::ProofValidationFailed {
                    reason: format!("RSA signature verification failed: {e}"),
                })
        }
        DpopAlgorithm::PS256 => {
            use rsa::pss::{Signature, VerifyingKey};
            use signature::Verifier;

            let verifying_key = VerifyingKey::<Sha256>::new(public_key);
            let signature_obj =
                Signature::try_from(signature).map_err(|e| DpopError::CryptographicError {
                    reason: format!("Invalid RSA-PSS signature format: {e}"),
                })?;
            verifying_key
                .verify(data.as_bytes(), &signature_obj)
                .map_err(|e| DpopError::ProofValidationFailed {
                    reason: format!("RSA-PSS signature verification failed: {e}"),
                })
        }
        _ => Err(DpopError::CryptographicError {
            reason: format!("Unsupported RSA algorithm: {algorithm}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_proof_generation_and_validation() {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await.unwrap());
        let proof_gen = DpopProofGenerator::new(key_manager.clone());

        // Generate a proof
        let proof = proof_gen
            .generate_proof("POST", "https://api.example.com/token", None)
            .await
            .unwrap();

        // Validate the proof
        let result = proof_gen
            .validate_proof(&proof, "POST", "https://api.example.com/token", None)
            .await
            .unwrap();

        assert!(result.valid);
        assert_eq!(result.key_algorithm, DpopAlgorithm::ES256);
    }

    #[tokio::test]
    async fn test_access_token_binding() {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await.unwrap());
        let proof_gen = DpopProofGenerator::new(key_manager);

        let access_token = "test-access-token-123";

        // Generate proof with access token
        let proof = proof_gen
            .generate_proof(
                "GET",
                "https://api.example.com/protected",
                Some(access_token),
            )
            .await
            .unwrap();

        // Validate with correct token
        let result = proof_gen
            .validate_proof(
                &proof,
                "GET",
                "https://api.example.com/protected",
                Some(access_token),
            )
            .await
            .unwrap();

        assert!(result.valid);

        // Validate with wrong token should fail
        let wrong_result = proof_gen
            .validate_proof(
                &proof,
                "GET",
                "https://api.example.com/protected",
                Some("wrong-token"),
            )
            .await;

        assert!(wrong_result.is_err());
    }

    #[tokio::test]
    async fn test_replay_attack_prevention() {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await.unwrap());
        let nonce_tracker = Arc::new(MemoryNonceTracker::new());
        let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker);

        let uri = "https://api.example.com/token";

        // Generate first proof
        let proof1 = proof_gen.generate_proof("POST", uri, None).await.unwrap();

        // First validation should succeed
        let result1 = proof_gen
            .validate_proof(&proof1, "POST", uri, None)
            .await
            .unwrap();
        assert!(result1.valid);

        // Second validation of same proof should fail (replay attack)
        let result2 = proof_gen.validate_proof(&proof1, "POST", uri, None).await;
        assert!(result2.is_err());

        // Generate new proof should succeed
        let proof2 = proof_gen.generate_proof("POST", uri, None).await.unwrap();
        let result3 = proof_gen
            .validate_proof(&proof2, "POST", uri, None)
            .await
            .unwrap();
        assert!(result3.valid);
    }

    #[tokio::test]
    async fn test_http_binding_validation() {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await.unwrap());
        let proof_gen = DpopProofGenerator::new(key_manager);

        // Generate proof for specific method and URI
        let proof = proof_gen
            .generate_proof("POST", "https://api.example.com/token", None)
            .await
            .unwrap();

        // Validate with wrong method should fail
        let wrong_method = proof_gen
            .validate_proof(&proof, "GET", "https://api.example.com/token", None)
            .await;
        assert!(wrong_method.is_err());

        // Validate with wrong URI should fail
        let wrong_uri = proof_gen
            .validate_proof(&proof, "POST", "https://api.example.com/other", None)
            .await;
        assert!(wrong_uri.is_err());
    }

    #[test]
    fn test_uri_cleaning() {
        assert_eq!(
            clean_http_uri("https://api.example.com/path?query=1#fragment").unwrap(),
            "https://api.example.com/path"
        );

        assert_eq!(
            clean_http_uri("https://api.example.com:8080/path").unwrap(),
            "https://api.example.com:8080/path"
        );
    }
}
