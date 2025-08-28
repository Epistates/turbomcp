//! Test utilities for DPoP implementation
//!
//! This module provides comprehensive testing utilities and mock implementations
//! for DPoP components when the `test-utils` feature is enabled.
//!
//! These utilities are designed for testing scenarios only and should never
//! be used in production code.

#[cfg(feature = "test-utils")]
use crate::{DpopAlgorithm, DpopKeyPair, DpopError, NonceStorage, Result, StorageStats};
#[cfg(feature = "test-utils")]
use std::collections::HashMap;
#[cfg(feature = "test-utils")]
use std::sync::{Arc, RwLock};
#[cfg(feature = "test-utils")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(feature = "test-utils")]
use ring::{rand, signature};
#[cfg(feature = "test-utils")]
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(feature = "test-utils")]
use serde_json::json;

/// Comprehensive mock key manager for testing DPoP key operations
#[cfg(feature = "test-utils")]
#[derive(Debug)]
pub struct MockKeyManager {
    /// Storage for generated test keys  
    keys: Arc<RwLock<HashMap<String, DpopKeyPair>>>,
    
    /// Test key generation statistics
    stats: Arc<RwLock<TestKeyStats>>,
}

/// Statistics for test key operations
#[cfg(feature = "test-utils")]
#[derive(Debug, Default, Clone)]
pub struct TestKeyStats {
    /// Number of keys generated
    pub keys_generated: u64,
    
    /// Number of keys rotated
    pub keys_rotated: u64,
    
    /// Number of signature operations
    pub signatures_created: u64,
    
    /// Number of verification operations
    pub verifications_performed: u64,
    
    /// Test execution time tracking
    pub total_test_time: Duration,
}

/// Mock in-memory nonce storage for testing
#[cfg(feature = "test-utils")]
#[derive(Debug, Default)]
pub struct MockNonceStorage {
    /// In-memory nonce storage
    nonces: Arc<RwLock<HashMap<String, StoredTestNonce>>>,
    
    /// JTI storage for replay protection testing
    jtis: Arc<RwLock<HashMap<String, StoredTestNonce>>>,
    
    /// Storage statistics
    stats: Arc<RwLock<MockStorageStats>>,
}

/// Test nonce information
#[cfg(feature = "test-utils")]
#[derive(Debug, Clone)]
struct StoredTestNonce {
    nonce: String,
    jti: String,
    client_id: String,
    stored_at: SystemTime,
    ttl: Duration,
    usage_count: u32,
}

/// Mock storage statistics  
#[cfg(feature = "test-utils")]
#[derive(Debug, Default)]
struct MockStorageStats {
    store_operations: u64,
    lookup_operations: u64,
    cleanup_operations: u64,
}

#[cfg(feature = "test-utils")]
impl MockKeyManager {
    /// Create a new mock key manager for testing
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(TestKeyStats::default())),
        }
    }

    /// Generate a production-grade test key pair using ring cryptography
    pub async fn generate_test_key(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair> {
        let start_time = SystemTime::now();
        
        match algorithm {
            DpopAlgorithm::Es256 => {
                // Generate ECDSA P-256 key pair using ring
                let rng = rand::SystemRandom::new();
                let key_pair = signature::EcdsaKeyPair::generate_pkcs8(
                    &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
                    &rng
                ).map_err(|e| DpopError::KeyManagementError {
                    reason: format!("Failed to generate ECDSA P-256 key: {}", e)
                })?;
                
                let private_key = key_pair.as_ref().to_vec();
                let public_key = key_pair.public_key().as_ref().to_vec();
                
                // Generate JWK thumbprint for key identification
                let thumbprint = self.generate_jwk_thumbprint(&public_key, &algorithm)?;
                
                let dpop_key = DpopKeyPair {
                    algorithm,
                    private_key,
                    public_key,
                    key_id: Some(thumbprint.clone()),
                    created_at: SystemTime::now(),
                };
                
                // Store key for reuse in tests
                {
                    let mut keys = self.keys.write().unwrap();
                    keys.insert(thumbprint, dpop_key.clone());
                }
                
                // Update statistics
                {
                    let mut stats = self.stats.write().unwrap();
                    stats.keys_generated += 1;
                    if let Ok(elapsed) = start_time.elapsed() {
                        stats.total_test_time += elapsed;
                    }
                }
                
                Ok(dpop_key)
            }
            DpopAlgorithm::Rs256 => {
                // For testing, generate a minimal RSA key representation
                // Note: ring doesn't support RSA key generation, so we simulate it
                let rng = rand::SystemRandom::new();
                let mut key_bytes = vec![0u8; 256]; // 2048-bit key simulation
                rand::fill(&rng, &mut key_bytes).map_err(|e| DpopError::KeyManagementError {
                    reason: format!("Failed to generate random bytes: {}", e)
                })?;
                
                let thumbprint = format!("test_rsa_{}", hex::encode(&key_bytes[..8]));
                
                let dpop_key = DpopKeyPair {
                    algorithm,
                    private_key: key_bytes.clone(),
                    public_key: key_bytes[128..].to_vec(), // Simulate public key portion
                    key_id: Some(thumbprint.clone()),
                    created_at: SystemTime::now(),
                };
                
                {
                    let mut keys = self.keys.write().unwrap();
                    keys.insert(thumbprint, dpop_key.clone());
                }
                
                {
                    let mut stats = self.stats.write().unwrap();
                    stats.keys_generated += 1;
                    if let Ok(elapsed) = start_time.elapsed() {
                        stats.total_test_time += elapsed;
                    }
                }
                
                Ok(dpop_key)
            }
        }
    }
    
    /// Create a test JWT for DPoP testing
    pub fn create_test_dpop_jwt(
        &self,
        key_pair: &DpopKeyPair,
        http_method: &str,
        http_uri: &str,
        nonce: Option<&str>,
    ) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        // Create test JWT header
        let header = json!({
            "alg": match key_pair.algorithm {
                DpopAlgorithm::Es256 => "ES256",
                DpopAlgorithm::Rs256 => "RS256",
            },
            "typ": "dpop+jwt",
            "jwk": self.create_test_jwk(&key_pair.public_key, &key_pair.algorithm)?
        });
        
        // Create test JWT payload
        let mut payload = json!({
            "jti": format!("test_jti_{}", rand::rand()),
            "htm": http_method,
            "htu": http_uri,
            "iat": now,
            "exp": now + 300, // 5 minutes
        });
        
        if let Some(n) = nonce {
            payload["nonce"] = json!(n);
        }
        
        // Create unsigned JWT for testing
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let signature_b64 = URL_SAFE_NO_PAD.encode(b"test_signature");
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.signatures_created += 1;
        }
        
        Ok(format!("{}.{}.{}", header_b64, payload_b64, signature_b64))
    }
    
    /// Get test key by thumbprint
    pub fn get_test_key(&self, thumbprint: &str) -> Option<DpopKeyPair> {
        let keys = self.keys.read().unwrap();
        keys.get(thumbprint).cloned()
    }
    
    /// Get all generated test keys
    pub fn get_all_test_keys(&self) -> Vec<DpopKeyPair> {
        let keys = self.keys.read().unwrap();
        keys.values().cloned().collect()
    }
    
    /// Clear all test keys (useful for test cleanup)
    pub fn clear_test_keys(&self) {
        let mut keys = self.keys.write().unwrap();
        keys.clear();
    }
    
    /// Get test key statistics
    pub fn get_test_stats(&self) -> TestKeyStats {
        let stats = self.stats.read().unwrap();
        stats.clone()
    }
    
    /// Generate JWK thumbprint for key identification
    fn generate_jwk_thumbprint(&self, public_key: &[u8], algorithm: &DpopAlgorithm) -> Result<String> {
        // Simplified thumbprint generation for testing
        let key_info = format!("{:?}_{}", algorithm, hex::encode(&public_key[..8.min(public_key.len())]));
        Ok(format!("test_thumbprint_{}", URL_SAFE_NO_PAD.encode(key_info.as_bytes())))
    }
    
    /// Create test JWK representation
    fn create_test_jwk(&self, public_key: &[u8], algorithm: &DpopAlgorithm) -> Result<serde_json::Value> {
        match algorithm {
            DpopAlgorithm::Es256 => Ok(json!({
                "kty": "EC",
                "crv": "P-256", 
                "x": URL_SAFE_NO_PAD.encode(&public_key[..32.min(public_key.len())]),
                "y": URL_SAFE_NO_PAD.encode(&public_key[32..64.min(public_key.len())]),
                "use": "sig"
            })),
            DpopAlgorithm::Rs256 => Ok(json!({
                "kty": "RSA",
                "n": URL_SAFE_NO_PAD.encode(public_key),
                "e": URL_SAFE_NO_PAD.encode(b"AQAB"), // 65537
                "use": "sig"
            }))
        }
    }
}

#[cfg(feature = "test-utils")]
impl MockNonceStorage {
    /// Create new mock nonce storage for testing
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create test nonce key
    fn nonce_key(&self, nonce: &str, client_id: &str) -> String {
        format!("test_nonce_{}_{}", client_id, nonce)
    }
    
    /// Create test JTI key  
    fn jti_key(&self, jti: &str, client_id: &str) -> String {
        format!("test_jti_{}_{}", client_id, jti)
    }
    
    /// Get current storage statistics
    pub fn get_mock_stats(&self) -> MockStorageStats {
        let stats = self.stats.read().unwrap();
        stats.clone()
    }
    
    /// Clear all test data
    pub fn clear_test_data(&self) {
        let mut nonces = self.nonces.write().unwrap();
        let mut jtis = self.jtis.write().unwrap();
        nonces.clear();
        jtis.clear();
    }
}

#[cfg(feature = "test-utils")]
impl NonceStorage for MockNonceStorage {
    async fn store_nonce(
        &self,
        nonce: &str,
        jti: &str,
        http_method: &str,
        http_uri: &str,
        client_id: &str,
        ttl: Option<Duration>,
    ) -> Result<bool> {
        let nonce_key = self.nonce_key(nonce, client_id);
        let jti_key = self.jti_key(jti, client_id);
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.store_operations += 1;
        }
        
        // Check for existing nonce or JTI (replay detection)
        {
            let nonces = self.nonces.read().unwrap();
            let jtis = self.jtis.read().unwrap();
            
            if nonces.contains_key(&nonce_key) || jtis.contains_key(&jti_key) {
                return Ok(false); // Replay detected
            }
        }
        
        let stored_nonce = StoredTestNonce {
            nonce: nonce.to_string(),
            jti: jti.to_string(),
            client_id: client_id.to_string(),
            stored_at: SystemTime::now(),
            ttl: ttl.unwrap_or(Duration::from_secs(300)),
            usage_count: 1,
        };
        
        // Store both nonce and JTI
        {
            let mut nonces = self.nonces.write().unwrap();
            let mut jtis = self.jtis.write().unwrap();
            
            nonces.insert(nonce_key, stored_nonce.clone());
            jtis.insert(jti_key, stored_nonce);
        }
        
        Ok(true)
    }
    
    async fn is_nonce_used(&self, nonce: &str, client_id: &str) -> Result<bool> {
        let nonce_key = self.nonce_key(nonce, client_id);
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.lookup_operations += 1;
        }
        
        let nonces = self.nonces.read().unwrap();
        Ok(nonces.contains_key(&nonce_key))
    }
    
    async fn cleanup_expired(&self) -> Result<u64> {
        let mut removed = 0u64;
        let now = SystemTime::now();
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.cleanup_operations += 1;
        }
        
        // Clean up expired nonces
        {
            let mut nonces = self.nonces.write().unwrap();
            let mut jtis = self.jtis.write().unwrap();
            
            nonces.retain(|_, stored| {
                let expired = now.duration_since(stored.stored_at)
                    .unwrap_or_default() > stored.ttl;
                if expired {
                    removed += 1;
                }
                !expired
            });
            
            jtis.retain(|_, stored| {
                now.duration_since(stored.stored_at)
                    .unwrap_or_default() <= stored.ttl
            });
        }
        
        Ok(removed)
    }
    
    async fn get_usage_stats(&self) -> Result<StorageStats> {
        let nonces = self.nonces.read().unwrap();
        let mock_stats = self.stats.read().unwrap();
        
        Ok(StorageStats {
            total_nonces: nonces.len() as u64,
            active_nonces: nonces.len() as u64,
            expired_nonces: 0,
            cleanup_runs: mock_stats.cleanup_operations,
            average_nonce_age: Duration::from_secs(150), // Mock average
            storage_size_bytes: nonces.len() as u64 * 100, // Rough estimate
            additional_metrics: vec![
                ("storage_type".to_string(), "mock_memory".to_string()),
                ("store_ops".to_string(), mock_stats.store_operations.to_string()),
                ("lookup_ops".to_string(), mock_stats.lookup_operations.to_string()),
            ]
        })
    }
}

#[cfg(feature = "test-utils")]
impl Default for MockKeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Test utilities when feature is disabled
/// This provides clear error messages for misconfiguration
#[cfg(not(feature = "test-utils"))]
#[derive(Debug)]
pub struct MockKeyManager;

#[cfg(not(feature = "test-utils"))]  
#[derive(Debug)]
pub struct MockNonceStorage;

#[cfg(not(feature = "test-utils"))]
impl MockKeyManager {
    /// Create a new mock key manager (feature disabled)
    /// 
    /// Test utilities are not available when the `test-utils` feature is disabled.
    /// Enable the feature in Cargo.toml to use mock implementations for testing.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "test-utils"))]
impl MockNonceStorage {
    /// Create a new mock nonce storage (feature disabled)
    ///
    /// Test utilities are not available when the `test-utils` feature is disabled.
    /// Enable the feature in Cargo.toml to use mock implementations for testing.
    pub fn new() -> Self {
        Self  
    }
}
