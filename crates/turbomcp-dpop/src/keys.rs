//! DPoP key management and cryptographic operations
//!
//! This module provides production-grade key management for DPoP operations including
//! key generation, storage, rotation, and secure cryptographic primitives.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::OsRng;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    errors::DpopError,
    types::{DpopAlgorithm, DpopKeyMetadata, DpopKeyPair, DpopPrivateKey, DpopPublicKey},
    Result,
};

/// DPoP key manager for centralized key operations
#[derive(Debug)]
pub struct DpopKeyManager {
    /// Key storage backend
    storage: Arc<dyn DpopKeyStorage>,
    /// Key rotation policy
    rotation_policy: KeyRotationPolicy,
    /// In-memory key cache for performance
    cache: Arc<RwLock<HashMap<String, CachedKeyPair>>>,
}

/// Cached key pair with metadata
#[derive(Debug, Clone)]
struct CachedKeyPair {
    key_pair: DpopKeyPair,
    cached_at: SystemTime,
}

impl DpopKeyManager {
    /// Create a new key manager with memory storage (development only)
    pub async fn new_memory() -> Result<Self> {
        Self::new(
            Arc::new(MemoryKeyStorage::new()),
            KeyRotationPolicy::default(),
        )
        .await
    }

    /// Create a new key manager with custom storage and rotation policy
    pub async fn new(
        storage: Arc<dyn DpopKeyStorage>,
        rotation_policy: KeyRotationPolicy,
    ) -> Result<Self> {
        Ok(Self {
            storage,
            rotation_policy,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Generate a new DPoP key pair
    pub async fn generate_key_pair(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair> {
        let key_id = Uuid::new_v4().to_string();
        let now = SystemTime::now();

        let (private_key, public_key) = match algorithm {
            DpopAlgorithm::ES256 => generate_es256_key_pair()?,
            DpopAlgorithm::RS256 | DpopAlgorithm::PS256 => generate_rsa_key_pair(2048)?,
        };

        let key_pair = DpopKeyPair {
            id: key_id.clone(),
            private_key,
            public_key: public_key.clone(),
            thumbprint: compute_thumbprint(&public_key, algorithm)?,
            algorithm,
            created_at: now,
            expires_at: self.rotation_policy.calculate_expiration(now),
            metadata: DpopKeyMetadata::default(),
        };

        // Store the key pair
        self.storage.store_key_pair(&key_id, &key_pair).await?;

        // Cache the key pair
        self.cache_key_pair(&key_pair).await;

        tracing::info!(
            key_id = %key_id,
            algorithm = %algorithm,
            thumbprint = %key_pair.thumbprint,
            "Generated new DPoP key pair"
        );

        Ok(key_pair)
    }

    /// Get a key pair by ID, checking cache first
    pub async fn get_key_pair(&self, key_id: &str) -> Result<Option<DpopKeyPair>> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(key_id) {
                // Check if cached entry is still valid
                if cached.cached_at.elapsed().unwrap_or(Duration::MAX) < Duration::from_secs(300)
                // 5 minute cache
                {
                    return Ok(Some(cached.key_pair.clone()));
                }
            }
        }

        // Cache miss or expired, load from storage
        if let Some(key_pair) = self.storage.get_key_pair(key_id).await? {
            self.cache_key_pair(&key_pair).await;
            Ok(Some(key_pair))
        } else {
            Ok(None)
        }
    }

    /// Get a key pair by thumbprint
    pub async fn get_key_pair_by_thumbprint(
        &self,
        thumbprint: &str,
    ) -> Result<Option<DpopKeyPair>> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            for cached in cache.values() {
                if constant_time_compare(&cached.key_pair.thumbprint, thumbprint) {
                    return Ok(Some(cached.key_pair.clone()));
                }
            }
        }

        // Not in cache, search storage
        let all_keys = self.storage.list_key_pairs().await?;
        for key_pair in all_keys {
            if constant_time_compare(&key_pair.thumbprint, thumbprint) {
                self.cache_key_pair(&key_pair).await;
                return Ok(Some(key_pair));
            }
        }

        Ok(None)
    }

    /// Rotate a key pair (generate new key, mark old as expired)
    pub async fn rotate_key_pair(&self, key_id: &str) -> Result<DpopKeyPair> {
        // Get current key
        let current_key =
            self.get_key_pair(key_id)
                .await?
                .ok_or_else(|| DpopError::KeyManagementError {
                    reason: format!("Key {key_id} not found for rotation"),
                })?;

        // Extract algorithm and metadata before moving current_key
        let algorithm = current_key.algorithm;
        let client_id = current_key.metadata.client_id.clone();
        let session_id = current_key.metadata.session_id.clone();
        let rotation_generation = current_key.metadata.rotation_generation;

        // Generate new key with same algorithm
        let mut new_key = self.generate_key_pair(algorithm).await?;

        // Copy relevant metadata
        new_key.metadata.client_id = client_id;
        new_key.metadata.session_id = session_id;
        new_key.metadata.rotation_generation = rotation_generation + 1;

        // Mark old key as expired (set slightly in the past to ensure immediate expiration)
        let mut expired_key = current_key;
        expired_key.expires_at = Some(SystemTime::now() - Duration::from_millis(1));
        self.storage.store_key_pair(key_id, &expired_key).await?;

        // Update cache with expired key
        self.cache_key_pair(&expired_key).await;

        tracing::info!(
            old_key_id = %key_id,
            new_key_id = %new_key.id,
            generation = new_key.metadata.rotation_generation,
            "Rotated DPoP key pair"
        );

        Ok(new_key)
    }

    /// Clean up expired keys
    pub async fn cleanup_expired_keys(&self) -> Result<usize> {
        let all_keys = self.storage.list_key_pairs().await?;
        let mut cleaned = 0;

        for key in all_keys {
            if key.is_expired() {
                self.storage.delete_key_pair(&key.id).await?;

                // Remove from cache
                self.cache.write().await.remove(&key.id);

                cleaned += 1;
                tracing::debug!(
                    key_id = %key.id,
                    "Cleaned up expired DPoP key"
                );
            }
        }

        if cleaned > 0 {
            tracing::info!(cleaned, "Cleaned up expired DPoP keys");
        }

        Ok(cleaned)
    }

    /// Cache a key pair for performance
    async fn cache_key_pair(&self, key_pair: &DpopKeyPair) {
        let cached = CachedKeyPair {
            key_pair: key_pair.clone(),
            cached_at: SystemTime::now(),
        };

        self.cache.write().await.insert(key_pair.id.clone(), cached);
    }
}

/// Key rotation policy for automatic key management
#[derive(Debug, Clone)]
pub struct KeyRotationPolicy {
    /// How long keys should remain valid
    pub key_lifetime: Duration,
    /// Whether automatic rotation is enabled
    pub auto_rotate: bool,
    /// How often to check for keys that need rotation
    pub rotation_check_interval: Duration,
}

impl KeyRotationPolicy {
    /// Create a policy suitable for development environments
    pub fn development() -> Self {
        Self {
            key_lifetime: Duration::from_secs(24 * 3600), // 24 hours
            auto_rotate: false,
            rotation_check_interval: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Create a policy suitable for production environments
    pub fn production() -> Self {
        Self {
            key_lifetime: Duration::from_secs(7 * 24 * 3600), // 7 days
            auto_rotate: true,
            rotation_check_interval: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Calculate expiration time for a key created at the given time
    pub fn calculate_expiration(&self, created_at: SystemTime) -> Option<SystemTime> {
        if self.auto_rotate {
            Some(created_at + self.key_lifetime)
        } else {
            None // Keys don't expire if auto-rotation is disabled
        }
    }
}

impl Default for KeyRotationPolicy {
    fn default() -> Self {
        Self::development()
    }
}

/// Trait for DPoP key storage backends
#[async_trait]
pub trait DpopKeyStorage: Send + Sync + std::fmt::Debug {
    /// Store a DPoP key pair
    async fn store_key_pair(&self, key_id: &str, key_pair: &DpopKeyPair) -> Result<()>;

    /// Retrieve a DPoP key pair by ID
    async fn get_key_pair(&self, key_id: &str) -> Result<Option<DpopKeyPair>>;

    /// Delete a DPoP key pair
    async fn delete_key_pair(&self, key_id: &str) -> Result<()>;

    /// List all stored key pairs (for cleanup and management)
    async fn list_key_pairs(&self) -> Result<Vec<DpopKeyPair>>;

    /// Get storage health information
    async fn health_check(&self) -> Result<StorageHealth>;
}

/// Storage health information
#[derive(Debug, Clone)]
pub struct StorageHealth {
    /// Whether storage is accessible
    pub accessible: bool,
    /// Number of stored keys
    pub key_count: usize,
    /// Storage-specific health information
    pub details: HashMap<String, serde_json::Value>,
}

/// In-memory key storage for development and testing
#[derive(Debug)]
pub struct MemoryKeyStorage {
    keys: Arc<RwLock<HashMap<String, DpopKeyPair>>>,
}

impl MemoryKeyStorage {
    /// Create a new in-memory key storage
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl DpopKeyStorage for MemoryKeyStorage {
    async fn store_key_pair(&self, key_id: &str, key_pair: &DpopKeyPair) -> Result<()> {
        self.keys
            .write()
            .await
            .insert(key_id.to_string(), key_pair.clone());
        Ok(())
    }

    async fn get_key_pair(&self, key_id: &str) -> Result<Option<DpopKeyPair>> {
        Ok(self.keys.read().await.get(key_id).cloned())
    }

    async fn delete_key_pair(&self, key_id: &str) -> Result<()> {
        self.keys.write().await.remove(key_id);
        Ok(())
    }

    async fn list_key_pairs(&self) -> Result<Vec<DpopKeyPair>> {
        Ok(self.keys.read().await.values().cloned().collect())
    }

    async fn health_check(&self) -> Result<StorageHealth> {
        let keys = self.keys.read().await;
        let mut details = HashMap::new();
        details.insert("storage_type".to_string(), serde_json::json!("memory"));

        Ok(StorageHealth {
            accessible: true,
            key_count: keys.len(),
            details,
        })
    }
}

impl Default for MemoryKeyStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate ES256 (ECDSA P-256) key pair
fn generate_es256_key_pair() -> Result<(DpopPrivateKey, DpopPublicKey)> {
    use p256::ecdsa::{SigningKey, VerifyingKey};

    // Generate random signing key
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = VerifyingKey::from(&signing_key);

    // Extract private key bytes
    let private_bytes = signing_key.to_bytes();
    let private_key = DpopPrivateKey::EcdsaP256 {
        key_bytes: private_bytes.into(),
    };

    // Extract public key coordinates
    let public_point = verifying_key.to_encoded_point(false); // Uncompressed format
    let x_bytes: [u8; 32] = public_point
        .x()
        .ok_or_else(|| DpopError::CryptographicError {
            reason: "Failed to extract X coordinate from P-256 key".to_string(),
        })?
        .as_slice()
        .try_into()
        .map_err(|_| DpopError::CryptographicError {
            reason: "Invalid X coordinate length".to_string(),
        })?;

    let y_bytes: [u8; 32] = public_point
        .y()
        .ok_or_else(|| DpopError::CryptographicError {
            reason: "Failed to extract Y coordinate from P-256 key".to_string(),
        })?
        .as_slice()
        .try_into()
        .map_err(|_| DpopError::CryptographicError {
            reason: "Invalid Y coordinate length".to_string(),
        })?;

    let public_key = DpopPublicKey::EcdsaP256 {
        x: x_bytes,
        y: y_bytes,
    };

    Ok((private_key, public_key))
}

/// Generate RSA key pair (for RS256/PS256)
fn generate_rsa_key_pair(key_size: u32) -> Result<(DpopPrivateKey, DpopPublicKey)> {
    use rsa::{pkcs8::EncodePrivateKey, traits::PublicKeyParts, RsaPrivateKey, RsaPublicKey};

    // Generate RSA private key
    let private_key = RsaPrivateKey::new(&mut OsRng, key_size as usize).map_err(|e| {
        DpopError::CryptographicError {
            reason: format!("Failed to generate RSA key: {e}"),
        }
    })?;

    let public_key: RsaPublicKey = private_key.to_public_key();

    // Encode private key in PKCS#8 DER format
    let private_key_der = private_key
        .to_pkcs8_der()
        .map_err(|e| DpopError::CryptographicError {
            reason: format!("Failed to encode RSA private key: {e}"),
        })?
        .as_bytes()
        .to_vec();

    let dpop_private_key = DpopPrivateKey::Rsa {
        key_der: private_key_der,
    };

    // Extract RSA public key parameters
    let dpop_public_key = DpopPublicKey::Rsa {
        n: public_key.n().to_bytes_be(),
        e: public_key.e().to_bytes_be(),
    };

    Ok((dpop_private_key, dpop_public_key))
}

/// Compute JWK thumbprint for a public key
fn compute_thumbprint(public_key: &DpopPublicKey, algorithm: DpopAlgorithm) -> Result<String> {
    use sha2::{Digest, Sha256};

    // Create JWK representation
    let jwk = match (public_key, algorithm) {
        (DpopPublicKey::Rsa { n, e }, DpopAlgorithm::RS256 | DpopAlgorithm::PS256) => {
            serde_json::json!({
                "kty": "RSA",
                "n": URL_SAFE_NO_PAD.encode(n),
                "e": URL_SAFE_NO_PAD.encode(e),
            })
        }
        (DpopPublicKey::EcdsaP256 { x, y }, DpopAlgorithm::ES256) => {
            serde_json::json!({
                "kty": "EC",
                "crv": "P-256",
                "x": URL_SAFE_NO_PAD.encode(x),
                "y": URL_SAFE_NO_PAD.encode(y),
            })
        }
        _ => {
            return Err(DpopError::CryptographicError {
                reason: "Mismatched key type and algorithm".to_string(),
            });
        }
    };

    // Serialize to canonical JSON
    let canonical_json =
        serde_json::to_string(&jwk).map_err(|e| DpopError::SerializationError {
            reason: format!("Failed to serialize JWK: {e}"),
        })?;

    // Compute SHA-256 hash
    let mut hasher = Sha256::new();
    hasher.update(canonical_json.as_bytes());
    let hash = hasher.finalize();

    // Return base64url-encoded thumbprint
    Ok(URL_SAFE_NO_PAD.encode(hash))
}

/// Constant-time string comparison to prevent timing attacks
///
/// This function compares two strings in constant time to prevent timing attacks
/// on cryptographic values like thumbprints and other sensitive data.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_key_storage() {
        let storage = MemoryKeyStorage::new();

        // Health check on empty storage
        let health = storage.health_check().await.unwrap();
        assert!(health.accessible);
        assert_eq!(health.key_count, 0);

        // Generate and store a key
        let key_manager = DpopKeyManager::new_memory().await.unwrap();
        let key_pair = key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
            .unwrap();

        // Verify key was stored
        let retrieved = key_manager.get_key_pair(&key_pair.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().thumbprint, key_pair.thumbprint);
    }

    #[tokio::test]
    async fn test_key_generation_algorithms() {
        let key_manager = DpopKeyManager::new_memory().await.unwrap();

        // Test ES256
        let es256_key = key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
            .unwrap();
        assert_eq!(es256_key.algorithm, DpopAlgorithm::ES256);
        assert!(matches!(
            es256_key.private_key,
            DpopPrivateKey::EcdsaP256 { .. }
        ));

        // Test RS256
        let rs256_key = key_manager
            .generate_key_pair(DpopAlgorithm::RS256)
            .await
            .unwrap();
        assert_eq!(rs256_key.algorithm, DpopAlgorithm::RS256);
        assert!(matches!(rs256_key.private_key, DpopPrivateKey::Rsa { .. }));
    }

    #[tokio::test]
    async fn test_key_rotation() {
        let key_manager = DpopKeyManager::new_memory().await.unwrap();

        // Generate initial key
        let original_key = key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
            .unwrap();

        // Rotate the key
        let rotated_key = key_manager.rotate_key_pair(&original_key.id).await.unwrap();

        // Verify rotation
        assert_ne!(rotated_key.id, original_key.id);
        assert_ne!(rotated_key.thumbprint, original_key.thumbprint);
        assert_eq!(rotated_key.algorithm, original_key.algorithm);
        assert_eq!(rotated_key.metadata.rotation_generation, 1);
    }

    #[tokio::test]
    async fn test_thumbprint_lookup() {
        let key_manager = DpopKeyManager::new_memory().await.unwrap();
        let key_pair = key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
            .unwrap();

        // Test thumbprint lookup
        let found_key = key_manager
            .get_key_pair_by_thumbprint(&key_pair.thumbprint)
            .await
            .unwrap();

        assert!(found_key.is_some());
        assert_eq!(found_key.unwrap().id, key_pair.id);

        // Test non-existent thumbprint
        let not_found = key_manager
            .get_key_pair_by_thumbprint("nonexistent-thumbprint")
            .await
            .unwrap();

        assert!(not_found.is_none());
    }
}
