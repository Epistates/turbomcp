//! Redis-based storage implementation for DPoP nonce tracking
//!
//! This module provides Redis-backed persistent storage for DPoP nonce
//! tracking and replay protection when the `redis-storage` feature is enabled.

use super::{DpopError, NonceStorage, Result};
use async_trait::async_trait;
use redis::{AsyncCommands, Client, RedisResult};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, trace, warn};

/// Redis-based nonce storage implementation with comprehensive DPoP tracking
#[derive(Debug, Clone)]
pub struct RedisNonceStorage {
    /// Redis client for async operations
    client: Client,

    /// Key prefix for nonce storage
    nonce_prefix: String,

    /// Key prefix for JTI (JWT ID) tracking
    jti_prefix: String,

    /// Key prefix for rate limiting
    rate_limit_prefix: String,

    /// Default expiration time for nonces (5 minutes as per RFC 9449)
    default_ttl: Duration,

    /// Maximum number of retries for Redis operations
    max_retries: u32,
}

/// Stored nonce information with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredNonce {
    /// The nonce value
    nonce: String,

    /// JTI (JWT ID) for additional verification
    jti: String,

    /// HTTP method (GET, POST, etc.)
    http_method: String,

    /// HTTP URI being accessed
    http_uri: String,

    /// Timestamp when nonce was first used
    first_used: u64,

    /// Client identifier (derived from DPoP key thumbprint)
    client_id: String,

    /// Number of times this nonce was seen (for replay detection)
    usage_count: u32,
}

impl RedisNonceStorage {
    /// Create a new Redis nonce storage instance with production configuration
    pub async fn new(connection_string: &str) -> Result<Self> {
        let client = Client::open(connection_string).map_err(|e| DpopError::StorageError {
            reason: format!("Failed to create Redis client: {}", e),
        })?;

        // Test connection
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| DpopError::StorageError {
                reason: format!("Failed to connect to Redis: {}", e),
            })?;

        // Verify Redis is responsive
        let _: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| DpopError::StorageError {
                reason: format!("Redis ping failed: {}", e),
            })?;

        debug!("Redis connection established successfully");

        Ok(Self {
            client,
            nonce_prefix: "turbomcp:dpop:nonce:".to_string(),
            jti_prefix: "turbomcp:dpop:jti:".to_string(),
            rate_limit_prefix: "turbomcp:dpop:rate:".to_string(),
            default_ttl: Duration::from_secs(300), // 5 minutes per RFC 9449
            max_retries: 3,
        })
    }

    /// Create Redis storage with custom configuration
    pub async fn with_config(
        connection_string: &str,
        nonce_ttl: Duration,
        key_prefix: String,
    ) -> Result<Self> {
        let mut storage = Self::new(connection_string).await?;
        storage.default_ttl = nonce_ttl;
        storage.nonce_prefix = format!("{}:dpop:nonce:", key_prefix);
        storage.jti_prefix = format!("{}:dpop:jti:", key_prefix);
        storage.rate_limit_prefix = format!("{}:dpop:rate:", key_prefix);
        Ok(storage)
    }

    /// Execute Redis operation with retry logic
    async fn with_retries<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> futures::future::BoxFuture<'static, RedisResult<T>>,
        T: Send + 'static,
    {
        let mut attempts = 0;

        loop {
            attempts += 1;

            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if attempts >= self.max_retries => {
                    error!("Redis operation failed after {} attempts: {}", attempts, e);
                    return Err(DpopError::StorageError {
                        reason: format!("Redis operation failed: {}", e),
                    });
                }
                Err(e) => {
                    warn!("Redis operation failed (attempt {}): {}", attempts, e);
                    tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
                }
            }
        }
    }

    /// Generate unique key for nonce storage
    fn nonce_key(&self, nonce: &str, client_id: &str) -> String {
        format!("{}{}__{}", self.nonce_prefix, client_id, nonce)
    }

    /// Generate unique key for JTI tracking
    fn jti_key(&self, jti: &str, client_id: &str) -> String {
        format!("{}{}__{}", self.jti_prefix, client_id, jti)
    }

    /// Generate key for rate limiting
    #[allow(dead_code)] // Reserved for future rate limiting feature
    fn rate_limit_key(&self, client_id: &str) -> String {
        format!("{}{}", self.rate_limit_prefix, client_id)
    }

    /// Current timestamp as Unix seconds
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[async_trait]
impl NonceStorage for RedisNonceStorage {
    async fn store_nonce(
        &self,
        nonce: &str,
        jti: &str,
        http_method: &str,
        http_uri: &str,
        client_id: &str,
        ttl: Option<Duration>,
    ) -> Result<bool> {
        let ttl = ttl.unwrap_or(self.default_ttl);
        let nonce_key = self.nonce_key(nonce, client_id);
        let jti_key = self.jti_key(jti, client_id);

        let stored_nonce = StoredNonce {
            nonce: nonce.to_string(),
            jti: jti.to_string(),
            http_method: http_method.to_string(),
            http_uri: http_uri.to_string(),
            first_used: Self::current_timestamp(),
            client_id: client_id.to_string(),
            usage_count: 1,
        };

        let serialized =
            serde_json::to_string(&stored_nonce).map_err(|e| DpopError::StorageError {
                reason: format!("Failed to serialize nonce data: {}", e),
            })?;

        let client = self.client.clone();
        let ttl_secs = ttl.as_secs();

        self.with_retries(|| {
            let client = client.clone();
            let nonce_key = nonce_key.clone();
            let jti_key = jti_key.clone();
            let serialized = serialized.clone();

            Box::pin(async move {
                let mut conn = client.get_multiplexed_async_connection().await?;

                // Use Redis transaction for atomic operations
                let (nonce_exists, jti_exists): (bool, bool) = redis::pipe()
                    .atomic()
                    .exists(&nonce_key)
                    .exists(&jti_key)
                    .query_async(&mut conn)
                    .await?;

                if nonce_exists || jti_exists {
                    // Nonce or JTI already exists - replay attack detected
                    return Ok(false);
                }

                // Store nonce and JTI with expiration
                let _: () = redis::pipe()
                    .atomic()
                    .set_ex(&nonce_key, &serialized, ttl_secs)
                    .set_ex(&jti_key, &serialized, ttl_secs)
                    .query_async(&mut conn)
                    .await?;

                Ok(true)
            })
        })
        .await
        .inspect(|&success| {
            if success {
                trace!("Stored DPoP nonce: {} for client: {}", nonce, client_id);
            } else {
                warn!(
                    "DPoP replay attack detected: nonce {} for client {}",
                    nonce, client_id
                );
            }
        })
    }

    async fn is_nonce_used(&self, nonce: &str, client_id: &str) -> Result<bool> {
        let nonce_key = self.nonce_key(nonce, client_id);
        let client = self.client.clone();

        self.with_retries(|| {
            let client = client.clone();
            let nonce_key = nonce_key.clone();

            Box::pin(async move {
                let mut conn = client.get_multiplexed_async_connection().await?;
                conn.exists(&nonce_key).await
            })
        })
        .await
    }

    async fn cleanup_expired(&self) -> Result<u64> {
        // Redis automatically handles expiration via TTL
        // This method can be used for additional cleanup logic
        debug!("Redis TTL handles automatic cleanup of expired nonces");
        Ok(0)
    }

    async fn get_usage_stats(&self) -> Result<super::StorageStats> {
        let client = self.client.clone();
        let nonce_prefix = self.nonce_prefix.clone();
        let jti_prefix = self.jti_prefix.clone();

        self.with_retries(|| {
            let client = client.clone();
            let nonce_prefix = nonce_prefix.clone();
            let jti_prefix = jti_prefix.clone();

            Box::pin(async move {
                let mut conn = client.get_multiplexed_async_connection().await?;

                // Count keys using SCAN to avoid blocking Redis
                let nonce_pattern = format!("{}*", nonce_prefix);
                let jti_pattern = format!("{}*", jti_prefix);

                let mut nonce_count = 0u64;
                let mut jti_count = 0u64;

                // Use SCAN for non-blocking key counting
                let mut nonce_iter: redis::AsyncIter<'_, String> =
                    conn.scan_match(&nonce_pattern).await?;
                #[allow(clippy::redundant_pattern_matching)] // Preserve drop semantics
                while let Some(_) = nonce_iter.next_item().await {
                    nonce_count += 1;
                }

                // Create new connection for second scan to avoid borrowing conflicts
                let mut conn2 = client.get_multiplexed_async_connection().await?;
                let mut jti_iter: redis::AsyncIter<'_, String> =
                    conn2.scan_match(&jti_pattern).await?;
                #[allow(clippy::redundant_pattern_matching)] // Preserve drop semantics
                while let Some(_) = jti_iter.next_item().await {
                    jti_count += 1;
                }

                Ok((nonce_count, jti_count))
            })
        })
        .await
        .map(|(nonce_count, jti_count)| {
            super::StorageStats {
                total_nonces: nonce_count,
                active_nonces: nonce_count, // In Redis, all stored nonces are active
                expired_nonces: 0,          // Redis handles expiration automatically
                cleanup_runs: 0,
                average_nonce_age: Duration::ZERO, // Would require additional tracking
                storage_size_bytes: nonce_count * 200, // Rough estimate
                additional_metrics: vec![
                    ("jti_count".to_string(), jti_count.to_string()),
                    ("storage_backend".to_string(), "Redis".to_string()),
                ],
            }
        })
    }
}

/// Redis storage implementation when feature is disabled
/// This provides clear error messages for misconfiguration
#[cfg(not(feature = "redis-storage"))]
#[derive(Debug)]
pub struct RedisNonceStorage;

#[cfg(not(feature = "redis-storage"))]
impl RedisNonceStorage {
    /// Create a new Redis nonce storage instance (feature disabled)
    ///
    /// Returns a configuration error directing users to enable the 'redis-storage' feature
    /// to use Redis-backed DPoP nonce storage.
    pub async fn new(_connection_string: &str) -> super::Result<Self> {
        Err(super::DpopError::ConfigurationError {
            reason: "Redis storage feature not enabled. Enable 'redis-storage' feature in Cargo.toml to use Redis backend.".to_string(),
        })
    }

    /// Create Redis storage with custom configuration (feature disabled)
    pub async fn with_config(
        _connection_string: &str,
        _nonce_ttl: std::time::Duration,
        _key_prefix: String,
    ) -> super::Result<Self> {
        Self::new(_connection_string).await
    }
}
