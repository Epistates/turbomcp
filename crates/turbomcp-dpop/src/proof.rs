//! DPoP proof generation and validation
//!
//! This module implements RFC 9449 compliant DPoP proof generation and validation
//! with security features including replay attack prevention, timing attack protection,
//! and cryptographic validation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
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

/// DPoP proof generator with security features
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
    #[must_use]
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

    /// Create a simple proof generator for basic use cases
    ///
    /// Uses in-memory storage for key management and nonce tracking.
    /// For production use with persistence, use `new()` with a proper key manager.
    ///
    /// # Errors
    /// Returns error if key manager initialization fails
    pub async fn new_simple() -> Result<Self> {
        let key_manager = DpopKeyManager::new_memory().await?;
        Ok(Self::new(Arc::new(key_manager)))
    }

    /// Generate a DPoP proof with all parameters
    ///
    /// Extended version that accepts nonce parameter for server-provided nonces.
    pub async fn generate_proof_with_params(
        &self,
        method: &str,
        uri: &str,
        access_token: Option<&str>,
        _nonce: Option<&str>,
        key_pair: Option<&DpopKeyPair>,
    ) -> Result<DpopProof> {
        // For now, delegate to existing method (nonce will be auto-generated)
        // Full nonce support can be added later if needed
        self.generate_proof_with_key(method, uri, access_token, key_pair).await
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

        // Create JWK from public key for the DpopHeader
        // Note: This creates our custom DpopJwk for the proof structure
        // The actual JWT signing uses jsonwebtoken::Jwk (created in sign_jwt)
        let jwk = match (&key_pair.public_key, key_pair.algorithm) {
            (DpopPublicKey::Rsa { n, e }, DpopAlgorithm::RS256 | DpopAlgorithm::PS256) => {
                DpopJwk::Rsa {
                    use_: "sig".to_string(),
                    n: URL_SAFE_NO_PAD.encode(n),
                    e: URL_SAFE_NO_PAD.encode(e),
                }
            }
            (DpopPublicKey::EcdsaP256 { x, y }, DpopAlgorithm::ES256) => DpopJwk::Ec {
                use_: "sig".to_string(),
                crv: "P-256".to_string(),
                x: URL_SAFE_NO_PAD.encode(x),
                y: URL_SAFE_NO_PAD.encode(y),
            },
            _ => {
                return Err(DpopError::CryptographicError {
                    reason: "Mismatched key type and algorithm".to_string(),
                });
            }
        };

        // Create JWT header
        let header = DpopHeader {
            typ: DPOP_JWT_TYPE.to_string(),
            algorithm: key_pair.algorithm,
            jwk,
        };

        // Sign the JWT - returns complete JWT string
        let jwt_string = self
            .sign_jwt(
                &header,
                &payload,
                &key_pair.private_key,
                &key_pair.public_key,
            )
            .await?;

        // Note: Nonce tracking moved to validation step to prevent false replay detection in tests
        // In production, server-side validation tracks nonces, not client-side generation

        // Parse JWT string to extract signature for DpopProof struct
        // Format: header.payload.signature
        let parts: Vec<&str> = jwt_string.split('.').collect();
        if parts.len() != 3 {
            return Err(DpopError::InternalError {
                reason: format!("Invalid JWT format: expected 3 parts, got {}", parts.len()),
            });
        }
        let signature = parts[2].to_string();

        // Create proof with cached JWT string for performance and validation
        let proof = DpopProof::new_with_jwt(
            header.clone(),
            payload.clone(),
            signature,
            jwt_string.clone(),
        );

        // Verify the cached JWT is actually stored
        let retrieved_jwt = proof.to_jwt_string();
        if retrieved_jwt != jwt_string {
            eprintln!("ERROR: JWT string mismatch!");
            eprintln!("Original  len: {}", jwt_string.len());
            eprintln!("Retrieved len: {}", retrieved_jwt.len());
            eprintln!("Original : {}", &jwt_string[..50.min(jwt_string.len())]);
            eprintln!(
                "Retrieved: {}",
                &retrieved_jwt[..50.min(retrieved_jwt.len())]
            );
        }

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

    /// Get or generate a default key pair
    async fn get_or_generate_default_key(&self) -> Result<DpopKeyPair> {
        // Generate key with proper algorithm selection
        // Key rotation is handled by the key manager's internal policies
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

    /// Validate cryptographic signature using industry-standard jsonwebtoken
    ///
    /// This replaces custom signature verification with jsonwebtoken::decode().
    /// Security improvements:
    /// - Eliminates ~200 lines of custom crypto verification code
    /// - Uses battle-tested library (9.3M+ downloads)
    /// - Proper algorithm validation (prevents "none" algorithm attack)
    /// - Industry-standard verification (RFC 7515)
    async fn validate_signature(&self, proof: &DpopProof) -> Result<()> {
        use crate::helpers::jwk_to_decoding_key;
        use jsonwebtoken::{Validation, decode, decode_header};

        // Get the JWT string from the proof
        // CRITICAL: We must use the exact JWT string that was signed, not reconstruct it
        // Reconstructing would result in potentially different JSON serialization order
        let jwt = proof.to_jwt_string();

        tracing::debug!(jwt_len = jwt.len(), "Validating JWT signature");

        // 1. Decode header (peek, no signature verification yet)
        let header = decode_header(&jwt).map_err(|e| DpopError::InvalidProofStructure {
            reason: format!("Failed to decode JWT header: {}", e),
        })?;

        // 2. Validate algorithm is allowed (whitelist - prevents "none" algorithm attack)
        const ALLOWED_ALGS: &[jsonwebtoken::Algorithm] = &[
            jsonwebtoken::Algorithm::ES256,
            jsonwebtoken::Algorithm::RS256,
            jsonwebtoken::Algorithm::PS256,
        ];
        if !ALLOWED_ALGS.contains(&header.alg) {
            return Err(DpopError::InvalidProofStructure {
                reason: format!("Algorithm {:?} not allowed for DPoP", header.alg),
            });
        }

        // 3. Validate typ field
        if header.typ.as_deref() != Some(DPOP_JWT_TYPE) {
            return Err(DpopError::InvalidProofStructure {
                reason: format!(
                    "Invalid JWT typ: expected '{}', got '{:?}'",
                    DPOP_JWT_TYPE, header.typ
                ),
            });
        }

        // 4. Extract JWK from header (BEFORE signature verification)
        let jwk = header.jwk.ok_or_else(|| DpopError::InvalidProofStructure {
            reason: "DPoP proof missing JWK in header".to_string(),
        })?;

        // 5. Create decoding key from JWK
        let decoding_key = jwk_to_decoding_key(&jwk)?;

        // 6. Configure validation
        let mut validation = Validation::new(header.alg);
        validation.validate_exp = false; // DPoP uses iat, not exp
        validation.set_required_spec_claims(&["iat"]); // Require iat claim
        validation.leeway = 60; // 60 seconds clock skew tolerance (MCP spec)

        // 7. Decode and VERIFY SIGNATURE
        // This is the critical security step - jsonwebtoken verifies the signature
        let _token_data = decode::<DpopPayload>(&jwt, &decoding_key, &validation).map_err(|e| {
            DpopError::ProofValidationFailed {
                reason: format!("JWT signature verification failed: {}", e),
            }
        })?;

        tracing::debug!(
            algorithm = ?header.alg,
            "Successfully verified DPoP JWT signature using jsonwebtoken"
        );

        Ok(())
    }

    /// Sign a JWT with the given private key using industry-standard jsonwebtoken
    ///
    /// This replaces custom JWT construction with the battle-tested jsonwebtoken crate.
    /// Security improvements:
    /// - Eliminates ~400 lines of custom crypto code
    /// - Uses proven library (9.3M+ downloads)
    /// - Automatic security updates via dependency
    /// - Industry-standard JWT construction (RFC 7515)
    async fn sign_jwt(
        &self,
        header: &DpopHeader,
        payload: &DpopPayload,
        private_key: &DpopPrivateKey,
        public_key: &DpopPublicKey, // Added public_key parameter
    ) -> Result<String> {
        use crate::helpers::{algorithm_to_jwt, private_key_to_encoding_key, public_key_to_jwk};
        use jsonwebtoken::{Header, encode};

        // Create jsonwebtoken Header with DPoP-specific fields
        let mut jwt_header = Header::new(algorithm_to_jwt(header.algorithm));
        jwt_header.typ = Some(DPOP_JWT_TYPE.to_string());

        // Embed JWK in header (RFC 9449 requirement)
        // Create jsonwebtoken::Jwk directly from public key (not from custom DpopJwk)
        let jwk = public_key_to_jwk(public_key)?;
        jwt_header.jwk = Some(jwk);

        // Create EncodingKey from private key
        let encoding_key = private_key_to_encoding_key(private_key)?;

        // Sign JWT using jsonwebtoken (handles all RFC 7515 mechanics)
        let jwt = encode(&jwt_header, payload, &encoding_key).map_err(|e| {
            DpopError::CryptographicError {
                reason: format!("JWT signing failed: {}", e),
            }
        })?;

        tracing::debug!(
            algorithm = ?header.algorithm,
            "Signed DPoP JWT using jsonwebtoken"
        );

        Ok(jwt)
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
    #[must_use]
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

/// Redis-based nonce tracker for distributed deployments
///
/// This implementation provides Redis-backed nonce tracking with DPoP replay
/// protection across multiple server instances. Only available when the
/// `redis-storage` feature is enabled.
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
    #[must_use]
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
///
/// Uses the industry-standard `subtle` crate which provides cryptographically
/// secure constant-time comparisons with compiler optimization barriers.
fn constant_time_compare(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Create JWK from public key
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
