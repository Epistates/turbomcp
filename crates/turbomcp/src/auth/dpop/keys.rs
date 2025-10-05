//! DPoP key management and cryptographic operations
//!
//! This module provides proven key management for DPoP operations including
//! key generation, storage, rotation, and secure cryptographic primitives.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::rngs::OsRng;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::{
    Result,
    errors::DpopError,
    types::{DpopAlgorithm, DpopKeyMetadata, DpopKeyPair, DpopPrivateKey, DpopPublicKey},
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
    use rsa::{RsaPrivateKey, RsaPublicKey, pkcs8::EncodePrivateKey, traits::PublicKeyParts};

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

/// Automated key rotation service for production deployments
///
/// This service runs as a background task, monitoring key expiration and automatically
/// rotating keys based on the configured rotation policy. Provides
/// key lifecycle management with monitoring and error recovery.
#[derive(Debug)]
pub struct AutoRotationService {
    /// Key manager for rotation operations
    key_manager: Arc<DpopKeyManager>,
    /// Cancellation token for graceful shutdown
    cancellation_token: CancellationToken,
    /// Notification for manual rotation triggers
    notify: Arc<Notify>,
    /// Background task handle
    task_handle: Option<JoinHandle<()>>,
    /// Rotation metrics and monitoring
    metrics: Arc<RotationMetrics>,
}

/// Rotation metrics for monitoring and alerting
#[derive(Debug)]
pub struct RotationMetrics {
    /// Total number of successful rotations
    pub successful_rotations: std::sync::atomic::AtomicU64,
    /// Total number of failed rotations
    pub failed_rotations: std::sync::atomic::AtomicU64,
    /// Last rotation timestamp
    pub last_rotation_time: RwLock<Option<SystemTime>>,
    /// Last error timestamp and message
    pub last_error: RwLock<Option<(SystemTime, String)>>,
    /// Keys currently tracked for rotation
    pub tracked_keys: std::sync::atomic::AtomicU64,
}

impl RotationMetrics {
    /// Create new rotation metrics instance
    pub fn new() -> Self {
        Self {
            successful_rotations: std::sync::atomic::AtomicU64::new(0),
            failed_rotations: std::sync::atomic::AtomicU64::new(0),
            last_rotation_time: RwLock::new(None),
            last_error: RwLock::new(None),
            tracked_keys: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Record successful rotation
    pub async fn record_success(&self) {
        self.successful_rotations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.last_rotation_time.write().await = Some(SystemTime::now());
    }

    /// Record failed rotation
    pub async fn record_failure(&self, error: &str) {
        self.failed_rotations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.last_error.write().await = Some((SystemTime::now(), error.to_string()));
    }

    /// Get current metrics snapshot
    pub async fn get_snapshot(&self) -> RotationMetricsSnapshot {
        RotationMetricsSnapshot {
            successful_rotations: self
                .successful_rotations
                .load(std::sync::atomic::Ordering::SeqCst),
            failed_rotations: self
                .failed_rotations
                .load(std::sync::atomic::Ordering::SeqCst),
            last_rotation_time: *self.last_rotation_time.read().await,
            last_error: self.last_error.read().await.clone(),
            tracked_keys: self.tracked_keys.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
}

impl Default for RotationMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of rotation metrics at a point in time
#[derive(Debug, Clone)]
pub struct RotationMetricsSnapshot {
    /// Total number of successful key rotations
    pub successful_rotations: u64,
    /// Total number of failed key rotations
    pub failed_rotations: u64,
    /// Timestamp of the last successful rotation
    pub last_rotation_time: Option<SystemTime>,
    /// Timestamp and message of the last error
    pub last_error: Option<(SystemTime, String)>,
    /// Number of keys currently being tracked for rotation
    pub tracked_keys: u64,
}

impl AutoRotationService {
    /// Create a new auto-rotation service
    ///
    /// # Arguments
    /// * `key_manager` - The key manager to use for rotation operations
    ///
    /// # Returns
    /// A new auto-rotation service instance (not yet started)
    ///
    /// # Example
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use turbomcp_dpop::{DpopKeyManager, AutoRotationService};
    /// # tokio_test::block_on(async {
    /// let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    /// let mut service = AutoRotationService::new(key_manager);
    /// service.start().await?;
    /// // Service runs in background...
    /// service.stop().await?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub fn new(key_manager: Arc<DpopKeyManager>) -> Self {
        Self {
            key_manager,
            cancellation_token: CancellationToken::new(),
            notify: Arc::new(Notify::new()),
            task_handle: None,
            metrics: Arc::new(RotationMetrics::new()),
        }
    }

    /// Start the auto-rotation service
    ///
    /// This spawns a background tokio task that monitors key expiration
    /// and performs automatic rotation according to the rotation policy.
    pub async fn start(&mut self) -> Result<()> {
        if self.task_handle.is_some() {
            return Err(DpopError::KeyManagementError {
                reason: "Auto-rotation service is already running".to_string(),
            });
        }

        info!("Starting DPoP auto-rotation service");

        let key_manager = self.key_manager.clone();
        let cancellation_token = self.cancellation_token.clone();
        let notify = self.notify.clone();
        let metrics = self.metrics.clone();

        let task_handle = tokio::spawn(async move {
            Self::rotation_loop(key_manager, cancellation_token, notify, metrics).await;
        });

        self.task_handle = Some(task_handle);

        info!("DPoP auto-rotation service started successfully");
        Ok(())
    }

    /// Stop the auto-rotation service
    ///
    /// This gracefully shuts down the background task and waits for it to complete.
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping DPoP auto-rotation service");

        // Signal cancellation
        self.cancellation_token.cancel();

        // Wait for task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => info!("DPoP auto-rotation service stopped successfully"),
                Err(e) => error!("Error stopping auto-rotation service: {}", e),
            }
        }

        Ok(())
    }

    /// Trigger manual rotation check
    ///
    /// This immediately wakes up the rotation service to check for keys
    /// that need rotation, bypassing the normal interval.
    pub fn trigger_rotation_check(&self) {
        debug!("Manual rotation check triggered");
        self.notify.notify_one();
    }

    /// Get current rotation metrics
    pub async fn get_metrics(&self) -> RotationMetricsSnapshot {
        self.metrics.get_snapshot().await
    }

    /// Main rotation loop (runs in background task)
    async fn rotation_loop(
        key_manager: Arc<DpopKeyManager>,
        cancellation_token: CancellationToken,
        notify: Arc<Notify>,
        metrics: Arc<RotationMetrics>,
    ) {
        let policy = &key_manager.rotation_policy;
        let check_interval = policy.rotation_check_interval;

        info!(
            auto_rotate = policy.auto_rotate,
            check_interval_secs = check_interval.as_secs(),
            "Auto-rotation loop started"
        );

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("Auto-rotation service cancelled, shutting down");
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    debug!("Rotation check interval elapsed");
                }
                _ = notify.notified() => {
                    debug!("Manual rotation check requested");
                }
            }

            if !policy.auto_rotate {
                debug!("Auto-rotation is disabled, skipping rotation check");
                continue;
            }

            if let Err(e) = Self::perform_rotation_check(&key_manager, &metrics).await {
                error!("Error during rotation check: {}", e);
                metrics.record_failure(&e.to_string()).await;
            }
        }

        info!("Auto-rotation loop terminated");
    }

    /// Perform rotation check for all keys
    async fn perform_rotation_check(
        key_manager: &DpopKeyManager,
        metrics: &RotationMetrics,
    ) -> Result<()> {
        debug!("Starting rotation check");

        // Get all keys that might need rotation
        let all_keys = key_manager.storage.list_key_pairs().await?;
        let now = SystemTime::now();

        metrics
            .tracked_keys
            .store(all_keys.len() as u64, std::sync::atomic::Ordering::SeqCst);

        let mut rotation_count = 0;

        for key_pair in all_keys {
            if let Some(expires_at) = key_pair.expires_at {
                if now >= expires_at {
                    info!(
                        key_id = %key_pair.id,
                        algorithm = %key_pair.algorithm,
                        rotation_generation = key_pair.metadata.rotation_generation,
                        "Rotating expired key"
                    );

                    match key_manager.rotate_key_pair(&key_pair.id).await {
                        Ok(new_key) => {
                            info!(
                                old_key_id = %key_pair.id,
                                new_key_id = %new_key.id,
                                algorithm = %new_key.algorithm,
                                new_generation = new_key.metadata.rotation_generation,
                                "Key rotation completed successfully"
                            );

                            metrics.record_success().await;
                            rotation_count += 1;
                        }
                        Err(e) => {
                            error!(
                                key_id = %key_pair.id,
                                error = %e,
                                "Failed to rotate key"
                            );
                            metrics
                                .record_failure(&format!("Key {}: {}", key_pair.id, e))
                                .await;
                        }
                    }
                } else {
                    // Calculate time until expiration for debugging
                    if let Ok(time_until_expiry) = expires_at.duration_since(now) {
                        debug!(
                            key_id = %key_pair.id,
                            expires_in_hours = time_until_expiry.as_secs() / 3600,
                            "Key rotation not needed yet"
                        );
                    }
                }
            } else {
                debug!(
                    key_id = %key_pair.id,
                    "Key has no expiration (auto-rotation disabled for this key)"
                );
            }
        }

        if rotation_count > 0 {
            info!(rotated_keys = rotation_count, "Rotation check completed");
        } else {
            debug!("Rotation check completed - no keys needed rotation");
        }

        Ok(())
    }
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
